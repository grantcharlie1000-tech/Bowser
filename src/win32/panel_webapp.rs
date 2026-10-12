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
};
use url::Url;
use windows::{
    Win32::{
        Foundation::{HWND, RECT, RECT as GdiRect},
        Graphics::Gdi::{
            BI_RGB, DIB_RGB_COLORS, DT_CENTER, DT_LEFT, DT_SINGLELINE, DT_VCENTER, DrawTextW,
            HDC, InvalidateRect, SetBkMode, SetTextColor, StretchDIBits,
            BITMAPINFO, BITMAPINFOHEADER, TRANSPARENT,
        },
        UI::HiDpi::GetDpiForWindow,
    },
};

const APP_HEADER_DIP: i32 = 78;
const HEADER_BUTTONS_DIP: i32 = 30;
const BUTTON_WIDTH_DIP: i32 = 54;
const BUTTON_GAP_DIP: i32 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HeaderAction {
    Back,
    ReloadStop,
    NewTab,
    Home,
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HeaderButton {
    action: HeaderAction,
    rect: Rect,
}

fn header_buttons(width: i32, dpi: u32) -> Vec<Rect> {
    let scale = |dip: i32| dip.saturating_mul(dpi.max(96) as i32) / 96;
    let button = scale(BUTTON_WIDTH_DIP);
    let gap = scale(BUTTON_GAP_DIP);
    let total = button * 5 + gap * 4;
    let left = ((width - total) / 2).max(scale(8));
    let top = scale(APP_HEADER_DIP - HEADER_BUTTONS_DIP);
    (0..5)
        .map(|index| {
            let x = left + index * (button + gap);
            Rect { left: x, top, right: x + button, bottom: top + scale(HEADER_BUTTONS_DIP) }
        })
        .collect()
}

fn hit_header_button(buttons: &[HeaderButton], point: (i32, i32)) -> Option<HeaderAction> {
    buttons.iter().find(|b| contains(b.rect, point)).map(|b| b.action)
}

fn contains(rect: Rect, (x, y): (i32, i32)) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

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
    name: String,
    initial_url: String,
    current_url: String,
    unnamed: bool,
    state: ViewState,
    favicon: Option<Vec<u8>>,
}

struct Shared {
    app: AppRef,
    profile_dir: PathBuf,
    parent: Option<HWND>,
    host: Option<WebAppHost>,
    active_id: Option<u32>,
    views: HashMap<u32, Rc<RefCell<AppView>>>,
}

type SharedRef = Rc<RefCell<Shared>>;

thread_local! {
    static WINDOWS: RefCell<HashMap<isize, SharedRef>> = RefCell::new(HashMap::new());
    static ICON_CHANGED: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
}

/// Installs factories for the apps already present in the current profile.
pub fn register(app: &AppRef) {
    let profile = profile::active().ok();
    let shared = Rc::new(RefCell::new(Shared {
        app: app.clone(),
        profile_dir: profile.as_ref().map_or_else(|| PathBuf::new(), |p| p.web_apps_dir()),
        parent: None,
        host: None,
        active_id: None,
        views: HashMap::new(),
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
        Box::new(Content::new(shared.clone(), id, web_app.name.clone(), web_app.url.clone()))
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
    fn new(shared: SharedRef, id: u32, name: String, url: String) -> Self {
        let unnamed = name.trim().is_empty();
        let view = {
            let mut state = shared.borrow_mut();
            state.views.entry(id).or_insert_with(|| Rc::new(RefCell::new(AppView {
                name: if unnamed { "Web app".to_owned() } else { name },
                initial_url: url.clone(),
                current_url: url,
                unnamed,
                state: ViewState::Loading,
                favicon: None,
            }))).clone()
        };
        Self { shared, id, view, bounds: RECT::default(), dpi: 96 }
    }

    fn ensure_host(&mut self, parent: HWND) {
        let (needs_create, profile_dir, app, weak_shared) = {
            let mut shared = self.shared.borrow_mut();
            shared.parent = Some(parent);
            shared.active_id = Some(self.id);
            (shared.host.is_none(), shared.profile_dir.clone(), shared.app.clone(), Rc::downgrade(&self.shared))
        };
        if !needs_create { self.open_current(); return; }

        let events = make_events(weak_shared.clone(), app.clone());
        let initial = self.bounds;
        let app_id = self.id;
        let result = WebAppHost::begin_create(parent, initial, &profile_dir, events, move |result| {
            let Some(shared) = weak_shared.upgrade() else { return; };
            match result {
                Ok(host) => {
                    let active = {
                        let mut state = shared.borrow_mut();
                        state.host = Some(host.clone());
                        state.active_id.map(|id| (id, state.views.get(&id).cloned()))
                    };
                    if let Some((id, Some(view))) = active {
                        let url = view.borrow().initial_url.clone();
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

    fn action(&mut self, action: HeaderAction) {
        match action {
            HeaderAction::Close => {
                let owner = self.shared.borrow().app.borrow().hwnd;
                side_panel::close(owner);
            }
            HeaderAction::NewTab => self.open_in_tab(),
            HeaderAction::Back => {
                if let Some(host) = self.shared.borrow().host.clone() { let _ = host.go_back(&self.id.to_string()); }
            }
            HeaderAction::ReloadStop => {
                let loading = self.view.borrow().state == ViewState::Loading;
                let host = self.shared.borrow().host.clone();
                if let Some(host) = host {
                    if loading { let _ = host.stop_app(&self.id.to_string()); }
                    else { let _ = host.retry_app(&self.id.to_string()); }
                    self.view.borrow_mut().state = if loading { ViewState::Ready } else { ViewState::Loading };
                } else {
                    let parent = self.shared.borrow().parent;
                    if let Some(parent) = parent { self.ensure_host(parent); }
                }
            }
            HeaderAction::Home => {
                let url = self.view.borrow().initial_url.clone();
                if let Some(host) = self.shared.borrow().host.clone() {
                    let _ = host.navigate_app(&self.id.to_string(), &url);
                }
                self.view.borrow_mut().state.retry();
            }
        }
        self.invalidate();
    }

    fn open_in_tab(&self) {
        let current = self.shared.borrow().host.as_ref()
            .and_then(|host| host.current_url(&self.id.to_string()))
            .unwrap_or_else(|| self.view.borrow().current_url.clone());
        let Ok(url) = Url::parse(&current) else { return; };
        let app = self.shared.borrow().app.clone();
        App::new_tab(&app);
        App::navigate_current(&app, &url);
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
        let top = bounds.top + scale(APP_HEADER_DIP, self.dpi);
        if bounds.right <= bounds.left || bounds.bottom <= top {
            if let Some(host) = self.shared.borrow().host.clone() { let _ = host.hide_all(); }
            return;
        }
        if let Some(host) = self.shared.borrow().host.clone() {
            let _ = host.set_bounds(RECT { left: bounds.left, top, right: bounds.right, bottom: bounds.bottom });
        }
    }

    fn paint(&mut self, hdc: HDC, rect: RECT, palette: &Palette) {
        let view = self.view.borrow();
        let header = scale(APP_HEADER_DIP, self.dpi);
        let title = nonempty_wide(&view.name);
        let mut title_rect = GdiRect { left: scale(56, self.dpi), top: rect.top + scale(6, self.dpi), right: rect.right - scale(8, self.dpi), bottom: rect.top + scale(34, self.dpi) };
        unsafe {
            let _ = SetBkMode(hdc, TRANSPARENT);
            let _ = SetTextColor(hdc, colorref(palette.omnibox_text));
            if let Some(mut title) = title { let _ = DrawTextW(hdc, &mut title, &mut title_rect, DT_LEFT | DT_VCENTER | DT_SINGLELINE); }
            draw_favicon(hdc, view.favicon.as_deref(), rect.left + scale(18, self.dpi), rect.top + scale(8, self.dpi), self.dpi);
        }
        let buttons = build_buttons(rect.right - rect.left, self.dpi);
        let labels = ["Back", if view.state == ViewState::Loading { "Stop" } else { "Reload" }, "New tab", "Home", "Close"];
        for (button, label) in buttons.iter().zip(labels) {
            let Some(mut text) = nonempty_wide(label) else { continue };
            let mut area = GdiRect { left: rect.left + button.rect.left, top: rect.top + button.rect.top, right: rect.left + button.rect.right, bottom: rect.top + button.rect.bottom };
            unsafe { let _ = DrawTextW(hdc, &mut text, &mut area, DT_CENTER | DT_VCENTER | DT_SINGLELINE); }
        }
        match &view.state {
            ViewState::Loading => draw_status(hdc, "Loading…", rect, header, self.dpi, palette),
            ViewState::Error(message) => {
                draw_status(hdc, message, rect, header, self.dpi, palette);
                let mut text: Vec<u16> = "Retry".encode_utf16().collect();
                let mut retry = GdiRect { left: rect.left + scale(16, self.dpi), top: rect.top + header + scale(42, self.dpi), right: rect.left + scale(104, self.dpi), bottom: rect.top + header + scale(78, self.dpi) };
                unsafe { let _ = DrawTextW(hdc, &mut text, &mut retry, DT_CENTER | DT_VCENTER | DT_SINGLELINE); }
            }
            ViewState::Ready => {}
            ViewState::Removed => draw_status(hdc, view.state.label(), rect, header, self.dpi, palette),
        }
    }

    fn on_mouse_down(&mut self, x: i32, y: i32) {
        let top = scale(APP_HEADER_DIP, self.dpi);
        let local = (x, y - self.bounds.top);
        let buttons = build_buttons(self.bounds.right - self.bounds.left, self.dpi);
        if let Some(action) = hit_header_button(&buttons, local) {
            self.action(action);
        } else if matches!(self.view.borrow().state, ViewState::Error(_))
            && local.0 >= scale(16, self.dpi)
            && local.0 < scale(104, self.dpi)
            && local.1 >= top + scale(42, self.dpi)
            && local.1 < top + scale(78, self.dpi)
        {
            self.view.borrow_mut().state.retry();
            let host = self.shared.borrow().host.clone();
            if let Some(host) = host { let _ = host.retry_app(&self.id.to_string()); }
            else {
                let parent = self.shared.borrow().parent;
                if let Some(parent) = parent { self.ensure_host(parent); }
            }
            self.invalidate();
        }
    }

    fn on_focus(&mut self, focused: bool) {
        if focused {
            self.open_current();
        } else {
            let host = self.shared.borrow().host.clone();
            if let Some(host) = host { let _ = host.hide_all(); }
            self.shared.borrow_mut().active_id = None;
        }
    }

    fn title(&self) -> &str { "Web app" }
    fn preferred_width(&self) -> i32 { 420 }
}

fn make_events(weak: Weak<RefCell<Shared>>, app: AppRef) -> WebAppEvents {
    let titles_weak = weak.clone();
    let favicons_weak = weak.clone();
    let nav_weak = weak.clone();
    let failed_weak = weak.clone();
    WebAppEvents {
        title_changed: Some(Rc::new(move |id, title| {
            if title.trim().is_empty() { return; }
            let Some(shared) = titles_weak.upgrade() else { return; };
            if let Some(view) = lookup_view(&shared, id) {
                let mut view = view.borrow_mut();
                if view.unnamed { view.name = title; }
            }
            invalidate_shared(&shared);
        })),
        favicon_changed: Some(Rc::new(move |id, url, bytes| {
            let (Some(shared), Some(bytes)) = (favicons_weak.upgrade(), bytes) else { return; };
            let host = url.as_ref().and_then(Url::host_str).map(str::to_owned)
                .or_else(|| lookup_view(&shared, id).and_then(|view| Url::parse(&view.borrow().current_url).ok()).and_then(|url| url.host_str().map(str::to_owned)));
            if let Some(host) = host {
                if crate::favicon::store_favicon(&host, &bytes).is_ok() {
                    persist_favicon_pointer(id, &bytes);
                }
            }
            if let Some(view) = lookup_view(&shared, id) { view.borrow_mut().favicon = Some(bytes); }
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

fn build_buttons(width: i32, dpi: u32) -> Vec<HeaderButton> {
    let actions = [HeaderAction::Back, HeaderAction::ReloadStop, HeaderAction::NewTab, HeaderAction::Home, HeaderAction::Close];
    header_buttons(width, dpi).into_iter().zip(actions).map(|(rect, action)| HeaderButton { action, rect }).collect()
}

fn draw_status(hdc: HDC, text: &str, rect: RECT, header: i32, dpi: u32, palette: &Palette) {
    let Some(mut text) = nonempty_wide(text) else { return };
    let mut area = GdiRect { left: rect.left + scale(16, dpi), top: rect.top + header + scale(16, dpi), right: rect.right - scale(12, dpi), bottom: rect.bottom };
    unsafe { let _ = SetTextColor(hdc, colorref(palette.omnibox_text)); let _ = DrawTextW(hdc, &mut text, &mut area, DT_LEFT | DT_VCENTER); }
}

unsafe fn draw_favicon(hdc: HDC, bytes: Option<&[u8]>, x: i32, y: i32, dpi: u32) {
    let Some(bitmap) = bytes.and_then(|bytes| crate::favicon::decode(bytes, 24)) else { return; };
    let mut info = BITMAPINFO::default();
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: bitmap.width as i32,
        biHeight: -(bitmap.height as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let side = scale(24, dpi);
    unsafe { let _ = StretchDIBits(hdc, x, y, side, side, 0, 0, bitmap.width as i32, bitmap.height as i32, Some(bitmap.bgra_premultiplied.as_ptr().cast()), &info, DIB_RGB_COLORS, windows::Win32::Graphics::Gdi::SRCCOPY); }
}

fn scale(value: i32, dpi: u32) -> i32 { value.saturating_mul(dpi.max(96) as i32) / 96 }
fn colorref(value: theme::Rgba) -> windows::Win32::Foundation::COLORREF { windows::Win32::Foundation::COLORREF(u32::from(value.r) | (u32::from(value.g) << 8) | (u32::from(value.b) << 16)) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_buttons_are_centered_and_hit_test_without_overlap() {
        let buttons = build_buttons(420, 96);
        assert_eq!(buttons.len(), 5);
        for pair in buttons.windows(2) { assert!(pair[0].rect.right <= pair[1].rect.left); }
        assert_eq!(hit_header_button(&buttons, center(buttons[0].rect)), Some(HeaderAction::Back));
        assert_eq!(hit_header_button(&buttons, center(buttons[4].rect)), Some(HeaderAction::Close));
        assert_eq!(hit_header_button(&buttons, (0, 0)), None);
    }

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
    fn header_layout_scales_at_higher_dpi() {
        let normal = header_buttons(600, 96);
        let high = header_buttons(900, 144);
        assert_eq!(normal[0].right - normal[0].left, 54);
        assert_eq!(high[0].right - high[0].left, 81);
    }

    fn center(rect: Rect) -> (i32, i32) { ((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2) }
}
