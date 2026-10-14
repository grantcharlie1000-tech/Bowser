//! Native content for a pinned web app. Each controller lives in the profile's separate WebApps
//! environment and remains alive while another panel or app is selected. Escape closes the panel
//! when its native host has focus; while a page has focus, WebView2 receives the key first.

use super::{panel_widgets::nonempty_wide, side_panel::{self, PanelContent, PanelId}};
use crate::{
    app::{App, AppRef},
    engine::web_app::{AppNavigationState, WebAppEvents, WebAppHost},
    profile,
    sidebar::{SidebarConfig, WebApp},
    theme::{self, Palette},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    rc::{Rc, Weak},
    time::Instant,
};
use url::Url;
use windows::{
    Win32::{
        Foundation::{HWND, RECT, RECT as GdiRect},
        Graphics::Gdi::{
            DT_CENTER, DT_LEFT, DT_SINGLELINE, DT_VCENTER, DrawTextW, HDC, InvalidateRect,
            SetBkMode, SetTextColor, TRANSPARENT,
        },
        UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::SetTimer},
    },
};

/// The Retry button shown over a failed page, in panel-local DIPs (left, top, right, bottom below
/// the content top). The only painted control left: the site otherwise fills the whole panel.
const RETRY_DIP: (i32, i32, i32, i32) = (16, 42, 104, 78);

#[derive(Clone, Debug, Eq, PartialEq)]
enum ViewState {
    Loading,
    Ready,
    Error(String),
    Removed,
}

impl Default for ViewState {
    fn default() -> Self { Self::Loading }
}

impl ViewState {
    fn label(&self) -> &str {
        match self {
            Self::Loading => "Loading…",
            Self::Ready => "",
            Self::Error(message) => message,
            Self::Removed => "This web app was removed.",
        }
    }

    fn fail(&mut self, message: String) { *self = Self::Error(message); }
    fn retry(&mut self) { if !matches!(self, Self::Removed) { *self = Self::Loading; } }
    fn remove(&mut self) { *self = Self::Removed; }
}

#[derive(Clone)]
struct AppView {
    initial_url: String,
    current_url: String,
    state: ViewState,
}

struct Shared {
    app: AppRef,
    profile_dir: PathBuf,
    /// The browser window that owns the idle timer.
    main_hwnd: HWND,
    parent: Option<HWND>,
    host: Option<WebAppHost>,
    active_id: Option<u32>,
    views: HashMap<u32, Rc<RefCell<AppView>>>,
    /// The most recent webview rectangle (the whole panel content area). The host is created
    /// asynchronously, so bounds that arrive before it exists are kept here and applied the moment
    /// the controller is ready — otherwise the first-opened app sits at a 0x0 rect and stays blank.
    webview_bounds: RECT,
}

type SharedRef = Rc<RefCell<Shared>>;

thread_local! {
    static WINDOWS: RefCell<HashMap<isize, SharedRef>> = RefCell::new(HashMap::new());
    static ICON_CHANGED: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
}

/// Timer id for suspending hidden apps; the main window's WM_TIMER arm calls `idle_timer`.
pub(crate) const IDLE_TIMER: usize = 0xB05D;
const IDLE_TIMER_MS: u32 = 30_000;

/// Suspends apps that have stayed hidden long enough for the window `hwnd`. Returns whether any
/// app is still hidden and awake, i.e. whether the timer should keep running.
pub(crate) fn idle_timer(hwnd: HWND) -> bool {
    let shared = WINDOWS.with(|windows| {
        windows.borrow().values().find(|shared| shared.borrow().main_hwnd == hwnd).cloned()
    });
    let Some(host) = shared.and_then(|shared| shared.borrow().host.clone()) else { return false; };
    host.suspend_hidden(Instant::now(), crate::efficiency::NORMAL_SUSPEND_AFTER);
    host.has_pending_suspend()
}

fn arm_idle_timer(shared: &SharedRef) {
    let main = shared.borrow().main_hwnd;
    unsafe { SetTimer(Some(main), IDLE_TIMER, IDLE_TIMER_MS, None); }
}

/// Installs factories for the apps already present in the current profile.
pub fn register(app: &AppRef) {
    let profile = profile::active().ok();
    let shared = Rc::new(RefCell::new(Shared {
        app: app.clone(),
        profile_dir: profile.as_ref().map_or_else(|| PathBuf::new(), |p| p.web_apps_dir()),
        main_hwnd: app.borrow().hwnd,
        parent: None,
        host: None,
        active_id: None,
        views: HashMap::new(),
        webview_bounds: RECT::default(),
    }));
    WINDOWS.with(|windows| { windows.borrow_mut().insert(app.borrow().hwnd.0 as isize, shared.clone()); });
    let config = profile.as_ref().and_then(|p| SidebarConfig::load(p.root()).ok()).unwrap_or_default();
    for web_app in config.apps { register_app_with(shared.clone(), web_app); }
}

/// Registers an app added after startup. The Customize panel or rail refresh path can call this.
pub fn register_app(app: &AppRef, web_app: WebApp) {
    let owner = app.borrow().hwnd.0 as isize;
    let shared = WINDOWS.with(|windows| windows.borrow().get(&owner).cloned());
    if let Some(shared) = shared { register_app_with(shared, web_app); }
}

fn register_app_with(shared: SharedRef, web_app: WebApp) {
    let id = web_app.id;
    side_panel::register_content(PanelId::WebApp(id), move || {
        Box::new(Content::new(shared.clone(), id, web_app.url.clone()))
    });
}

/// Called when the rail's app list changes. Removes its controller and any live panel content.
pub fn forget_app(id: u32) {
    WINDOWS.with(|windows| {
        for shared in windows.borrow().values() {
            let (view, host, owner) = {
                let mut state = shared.borrow_mut();
                (state.views.remove(&id), state.host.clone(), state.app.borrow().hwnd)
            };
            if let Some(view) = view { view.borrow_mut().state.remove(); }
            if let Some(host) = host { host.remove_app(&id.to_string()); }
            side_panel::close_content(owner, PanelId::WebApp(id));
        }
    });
    side_panel::unregister_content(PanelId::WebApp(id));
}

/// Closes every WebView2 controller before the browser window and its profile are destroyed.
pub fn shutdown() {
    WINDOWS.with(|windows| {
        let shared_windows: Vec<_> = windows.borrow().values().cloned().collect();
        for shared in shared_windows {
            let mut state = shared.borrow_mut();
            if let Some(host) = state.host.take() { host.close(); }
            state.parent = None;
            state.active_id = None;
        }
        windows.borrow_mut().clear();
    });
}

/// Clears all pinned-app cookies, storage, cache, and service workers for the active profile.
/// Customize can call this after confirming the user wants to clear every web app's site data.
pub fn clear_all_data() -> std::io::Result<()> {
    let hosts = WINDOWS.with(|windows| {
        windows.borrow().values().filter_map(|shared| {
            let mut state = shared.borrow_mut();
            state.active_id = None;
            state.host.take()
        }).collect::<Vec<_>>()
    });
    for host in hosts { host.close(); }
    let dir = profile::active().map_err(|_| std::io::Error::new(std::io::ErrorKind::NotFound, "active profile unavailable"))?.web_apps_dir();
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Lets startup wiring post its existing sidebar/rail rebuild notification after a favicon arrives.
pub fn set_on_icon_changed(callback: Box<dyn Fn()>) {
    ICON_CHANGED.with(|slot| *slot.borrow_mut() = Some(callback));
}

struct Content {
    shared: SharedRef,
    id: u32,
    view: Rc<RefCell<AppView>>,
    bounds: RECT,
    dpi: u32,
}

impl Content {
    fn new(shared: SharedRef, id: u32, url: String) -> Self {
        let view = {
            let mut state = shared.borrow_mut();
            state.views.entry(id).or_insert_with(|| Rc::new(RefCell::new(AppView {
                initial_url: url.clone(),
                current_url: url,
                state: ViewState::Loading,
            }))).clone()
        };
        Self { shared, id, view, bounds: RECT::default(), dpi: 96 }
    }

    fn ensure_host(&mut self, parent: HWND) {
        let (needs_create, profile_dir, app, weak_shared, initial) = {
            let mut shared = self.shared.borrow_mut();
            shared.parent = Some(parent);
            shared.active_id = Some(self.id);
            (shared.host.is_none(), shared.profile_dir.clone(), shared.app.clone(), Rc::downgrade(&self.shared), shared.webview_bounds)
        };
        if !needs_create { self.open_current(); return; }

        let events = make_events(weak_shared.clone(), app.clone());
        let app_id = self.id;
        let result = WebAppHost::begin_create(parent, initial, &profile_dir, events, move |result| {
            let Some(shared) = weak_shared.upgrade() else { return; };
            match result {
                Ok(host) => {
                    let active = {
                        let mut state = shared.borrow_mut();
                        state.host = Some(host.clone());
                        let bounds = state.webview_bounds;
                        state.active_id.map(|id| (id, state.views.get(&id).cloned(), bounds))
                    };
                    if let Some((id, Some(view), bounds)) = active {
                        let url = view.borrow().initial_url.clone();
                        // Apply the bounds that accumulated during async creation before the first
                        // controller is made, so it opens at the real panel rect, not 0x0.
                        let _ = host.set_bounds(bounds);
                        let _ = host.open_app(&id.to_string(), &url);
                    }
                }
                Err(error) => set_error(&shared, app_id, format!("The web app could not start: {error}")),
            }
        });
        if let Err(error) = result {
            self.view.borrow_mut().state.fail(format!("The web app could not start: {error}"));
        }
    }

    fn open_current(&self) {
        let (host, url) = {
            let mut state = self.shared.borrow_mut();
            state.active_id = Some(self.id);
            (state.host.clone(), self.view.borrow().initial_url.clone())
        };
        if let Some(host) = host {
            if let Err(error) = host.open_app(&self.id.to_string(), &url) {
                self.view.borrow_mut().state.fail(format!("The web app could not open: {error}"));
            }
        }
    }

    /// Re-runs a failed load, or starts the host again if it never came up.
    fn retry(&mut self) {
        self.view.borrow_mut().state.retry();
        let host = self.shared.borrow().host.clone();
        if let Some(host) = host { let _ = host.retry_app(&self.id.to_string()); }
        else {
            let parent = self.shared.borrow().parent;
            if let Some(parent) = parent { self.ensure_host(parent); }
        }
        self.invalidate();
    }

    fn invalidate(&self) {
        if let Some(parent) = self.shared.borrow().parent {
            unsafe { let _ = InvalidateRect(Some(parent), None, false); }
        }
    }
}

impl PanelContent for Content {
    fn on_host_created(&mut self, host: HWND) {
        self.dpi = unsafe { GetDpiForWindow(host).max(96) };
        self.ensure_host(host);
    }

    fn on_bounds_changed(&mut self, bounds: RECT) {
        self.bounds = bounds;
        let target = webview_rect(bounds);
        // Remember the latest rect even while the host is still being created, so the controller can
        // be placed correctly the instant it arrives rather than staying at its 0x0 initial bounds.
        self.shared.borrow_mut().webview_bounds = target.unwrap_or_default();
        if let Some(host) = self.shared.borrow().host.clone() {
            match target {
                Some(rect) => { let _ = host.show_app(&self.id.to_string(), rect); }
                None => { let _ = host.hide_all(); arm_idle_timer(&self.shared); }
            }
        }
    }

    fn paint(&mut self, hdc: HDC, rect: RECT, palette: &Palette) {
        // The site fills the panel, so only the transient states below are ever drawn natively.
        let view = self.view.borrow();
        unsafe {
            let _ = SetBkMode(hdc, TRANSPARENT);
            let _ = SetTextColor(hdc, colorref(palette.omnibox_text));
        }
        match &view.state {
            ViewState::Loading => draw_status(hdc, "Loading…", rect, self.dpi, palette),
            ViewState::Error(message) => {
                draw_status(hdc, message, rect, self.dpi, palette);
                let mut text: Vec<u16> = "Retry".encode_utf16().collect();
                let (left, top, right, bottom) = retry_rect(self.dpi);
                let mut retry = GdiRect { left: rect.left + left, top: rect.top + top, right: rect.left + right, bottom: rect.top + bottom };
                unsafe { let _ = DrawTextW(hdc, &mut text, &mut retry, DT_CENTER | DT_VCENTER | DT_SINGLELINE); }
            }
            ViewState::Ready => {}
            ViewState::Removed => draw_status(hdc, view.state.label(), rect, self.dpi, palette),
        }
    }

    fn on_mouse_down(&mut self, x: i32, y: i32) {
        // Only a failed page has anything native to click; a ready page is entirely the webview.
        if !matches!(self.view.borrow().state, ViewState::Error(_)) { return; }
        let (left, top, right, bottom) = retry_rect(self.dpi);
        let local = (x, y - self.bounds.top);
        if local.0 >= left && local.0 < right && local.1 >= top && local.1 < bottom { self.retry(); }
    }

    fn on_focus(&mut self, focused: bool) {
        if focused {
            self.open_current();
        } else {
            let host = self.shared.borrow().host.clone();
            if let Some(host) = host {
                let _ = host.hide_all();
                arm_idle_timer(&self.shared);
            }
            self.shared.borrow_mut().active_id = None;
        }
    }

    /// Empty so the host paints no caption: the site, not a label, is the panel's content.
    fn title(&self) -> &str { "" }
    fn preferred_width(&self) -> i32 { 420 }
}

fn make_events(weak: Weak<RefCell<Shared>>, app: AppRef) -> WebAppEvents {
    let favicons_weak = weak.clone();
    let nav_weak = weak.clone();
    let failed_weak = weak.clone();
    let (download_event, download_plan) = App::download_hooks(&app);
    WebAppEvents {
        download_event: Some(download_event),
        download_plan: Some(download_plan),
        policy_event: None,
        title_changed: None,
        favicon_changed: Some(Rc::new(move |id, url, bytes| {
            let (Some(shared), Some(bytes)) = (favicons_weak.upgrade(), bytes) else { return; };
            let host = url.as_ref().and_then(Url::host_str).map(str::to_owned)
                .or_else(|| lookup_view(&shared, id).and_then(|view| Url::parse(&view.borrow().current_url).ok()).and_then(|url| url.host_str().map(str::to_owned)));
            if let Some(host) = host {
                if crate::favicon::store_favicon(&host, &bytes).is_ok() {
                    persist_favicon_pointer(id, &bytes);
                }
            }
            ICON_CHANGED.with(|slot| { if let Some(callback) = slot.borrow().as_ref() { callback(); } });
            invalidate_shared(&shared);
        })),
        navigation_state: Some(Rc::new(move |id, state| {
            let Some(shared) = nav_weak.upgrade() else { return; };
            if let Some(view) = lookup_view(&shared, id) {
            let mut view = view.borrow_mut();
            match state {
                AppNavigationState::Loading => view.state = ViewState::Loading,
                AppNavigationState::Ready => view.state = ViewState::Ready,
                AppNavigationState::Failed if view.state == ViewState::Loading => view.state = ViewState::Error("The page failed to load.".into()),
                AppNavigationState::Failed => {}
            }
            }
            invalidate_shared(&shared);
        })),
        failed: Some(Rc::new(move |id, message| {
            if let Some(shared) = failed_weak.upgrade() { set_error(&shared, id.parse().unwrap_or(0), message); }
        })),
        on_open_in_tab: Some(Rc::new(move |url| { App::new_tab(&app); App::navigate_current(&app, &url); })),
    }
}

fn lookup_view(shared: &SharedRef, id: &str) -> Option<Rc<RefCell<AppView>>> {
    let id = id.parse::<u32>().ok()?;
    shared.borrow().views.get(&id).cloned()
}

fn set_error(shared: &SharedRef, id: u32, message: String) {
    if let Some(view) = shared.borrow().views.get(&id).cloned() { view.borrow_mut().state.fail(message); }
    invalidate_shared(shared);
}

fn invalidate_shared(shared: &SharedRef) {
    if let Some(parent) = shared.borrow().parent { unsafe { let _ = InvalidateRect(Some(parent), None, false); } }
}

fn persist_favicon_pointer(id: &str, bytes: &[u8]) {
    let Ok(profile) = profile::active() else { return; };
    if profile.is_guest() { return; }
    let Ok(entries) = std::fs::read_dir(profile.favicons_dir()) else { return; };
    let Some(file_name) = entries.flatten().find_map(|entry| {
        let path = entry.path();
        (path.extension().is_some_and(|ext| ext == "ico")
            && std::fs::read(&path).is_ok_and(|stored| stored == bytes))
        .then(|| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        .flatten()
    }) else { return; };
    let Ok(id) = id.parse::<u32>() else { return; };
    let root = profile.root();
    let Ok(mut config) = SidebarConfig::load(root) else { return; };
    let Some(app) = config.apps.iter_mut().find(|app| app.id == id) else { return; };
    app.favicon_file = Some(file_name);
    let _ = config.save(root);
}

fn draw_status(hdc: HDC, text: &str, rect: RECT, dpi: u32, palette: &Palette) {
    let Some(mut text) = nonempty_wide(text) else { return };
    let mut area = GdiRect { left: rect.left + scale(16, dpi), top: rect.top + scale(16, dpi), right: rect.right - scale(12, dpi), bottom: rect.bottom };
    unsafe { let _ = SetTextColor(hdc, colorref(palette.omnibox_text)); let _ = DrawTextW(hdc, &mut text, &mut area, DT_LEFT | DT_VCENTER); }
}

/// The Retry button's rectangle relative to the content origin, in pixels. Pure.
fn retry_rect(dpi: u32) -> (i32, i32, i32, i32) {
    let (left, top, right, bottom) = RETRY_DIP;
    (scale(left, dpi), scale(top, dpi), scale(right, dpi), scale(bottom, dpi))
}

/// The webview rectangle for a panel content area: the whole area, so the site fills the panel with
/// no native header above it. Returns `None` when the panel is too small (collapsed or
/// mid-animation) to show any page, so the webview is hidden instead of being sized to a
/// zero/negative rectangle. Pure.
fn webview_rect(bounds: RECT) -> Option<RECT> {
    (bounds.right > bounds.left && bounds.bottom > bounds.top).then_some(bounds)
}

fn scale(value: i32, dpi: u32) -> i32 { value.saturating_mul(dpi.max(96) as i32) / 96 }
fn colorref(value: theme::Rgba) -> windows::Win32::Foundation::COLORREF { windows::Win32::Foundation::COLORREF(u32::from(value.r) | (u32::from(value.g) << 8) | (u32::from(value.b) << 16)) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_error_retry_and_removal_transitions_are_stable() {
        let mut state = ViewState::default();
        assert_eq!(state.label(), "Loading…");
        state.fail("Network unavailable".into());
        assert_eq!(state, ViewState::Error("Network unavailable".into()));
        state.retry();
        assert_eq!(state, ViewState::Loading);
        state.remove();
        assert_eq!(state, ViewState::Removed);
        state.retry();
        assert_eq!(state, ViewState::Removed);
    }

    #[test]
    fn retry_button_scales_with_dpi() {
        assert_eq!(retry_rect(96), (16, 42, 104, 78));
        assert_eq!(retry_rect(192), (32, 84, 208, 156));
    }

    #[test]
    fn webview_rect_fills_the_panel_content_area_and_hides_when_too_small() {
        // The site fills the whole content area: no header offset at any DPI.
        let bounds = RECT { left: 0, top: 10, right: 420, bottom: 800 };
        assert_eq!(webview_rect(bounds), Some(bounds));
        // Collapsed/animating width yields no rect, so the controller is hidden rather than 0-sized.
        assert_eq!(webview_rect(RECT { left: 0, top: 0, right: 0, bottom: 800 }), None);
        // A panel with no height never produces a zero/negative-height webview.
        assert_eq!(webview_rect(RECT { left: 0, top: 50, right: 420, bottom: 50 }), None);
    }
}
