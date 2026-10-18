//! Native content for a pinned web app. Each controller lives in the profile's separate WebApps
//! environment and remains alive while another panel or app is selected. Escape closes the panel
//! when its native host has focus; while a page has focus, WebView2 receives the key first.

use super::{panel_widgets::nonempty_wide, side_panel::{self, PanelContent, PanelId}};
use crate::{
    app::{App, AppRef},
    engine::web_app::{AppNavigationState, WebAppEvents, WebAppHost},
    profile,
    sidebar::{SidebarConfig, WebApp},
    web_app_lifecycle::CreationGate,
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum ViewState {
    #[default]
    Loading,
    Ready,
    Error(String),
    Removed,
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
    // --- window and panels (thread T9) ---
    /// The count the page title announces ("(3) Inbox"); 0 when none or when notifications are off.
    unread: u32,
    /// When the page last reloaded (or was created): the periodic reload counts from it.
    last_reload: Instant,
    // --- end window and panels ---
}

struct Shared {
    creation: CreationGate,
    closed: bool,
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

// --- window and panels (thread T9) --- per-panel options from the rail menu (src/panel_prefs.rs)
/// Timer id for periodic reloads; the main window's WM_TIMER arm calls `reload_timer`.
pub(crate) const RELOAD_TIMER: usize = 0xB05E;
const RELOAD_TICK_MS: u32 = 30_000;

/// Applies a panel's saved zoom and mute to its page (a no-op until the page exists).
fn apply_prefs(shared: &SharedRef, id: u32) {
    let prefs = crate::panel_prefs::current(id);
    let host = shared.borrow().host.clone();
    if let Some(host) = host {
        let key = id.to_string();
        let _ = host.set_zoom_percent(&key, prefs.zoom);
        let _ = host.set_muted(&key, prefs.muted);
    }
}

/// Re-applies one panel's options in every window after the rail menu changed them.
pub fn prefs_changed(id: u32) {
    let all: Vec<SharedRef> = WINDOWS.with(|windows| windows.borrow().values().cloned().collect());
    for shared in all {
        apply_prefs(&shared, id);
        // A page whose notifications were switched off loses its mark at once.
        if !crate::panel_prefs::current(id).notify {
            if let Some(view) = shared.borrow().views.get(&id).cloned() { view.borrow_mut().unread = 0; }
        }
        let main = shared.borrow().main_hwnd;
        arm_reload_timer(main);
    }
    ICON_CHANGED.with(|slot| { if let Some(callback) = slot.borrow().as_ref() { callback(); } });
}

/// Starts the reload tick when any panel reloads on a timer (it stops itself when none does).
pub fn arm_reload_timer(main: HWND) {
    let any = crate::profile::active().ok().is_some_and(|profile| crate::panel_prefs::PanelPrefsFile::load(profile.root()).any_reload());
    if any { unsafe { SetTimer(Some(main), RELOAD_TIMER, RELOAD_TICK_MS, None); } }
}

/// One reload tick for the window `hwnd`: reloads each loaded panel whose interval has passed.
/// Returns whether any panel still reloads on a timer, i.e. whether the tick should keep running.
pub(crate) fn reload_timer(hwnd: HWND) -> bool {
    let Some(shared) = WINDOWS.with(|windows| windows.borrow().values().find(|shared| shared.borrow().main_hwnd == hwnd).cloned()) else { return false; };
    let Ok(profile) = crate::profile::active() else { return false; };
    let file = crate::panel_prefs::PanelPrefsFile::load(profile.root());
    let host = shared.borrow().host.clone();
    let views: Vec<(u32, Rc<RefCell<AppView>>)> = shared.borrow().views.iter().map(|(id, view)| (*id, view.clone())).collect();
    for (id, view) in views {
        let minutes = file.get(id).reload_minutes;
        if !crate::panel_prefs::reload_due(minutes, view.borrow().last_reload.elapsed()) { continue; }
        if let Some(host) = host.as_ref() {
            if host.reload_if_open(&id.to_string()) { view.borrow_mut().last_reload = Instant::now(); }
        }
    }
    file.any_reload()
}

/// The address the web app's page is showing now (its start address before the page exists).
pub fn page_url(id: u32) -> Option<String> {
    WINDOWS.with(|windows| {
        windows.borrow().values().find_map(|shared| {
            let shared = shared.borrow();
            let view = shared.views.get(&id)?;
            shared.host.as_ref().and_then(|host| host.current_url(&id.to_string())).or_else(|| Some(view.borrow().initial_url.clone()))
        })
    })
}

/// The count to show on the rail button of web app `id` (0 while its panel is open or the page announces none).
pub fn unread_count(id: u32) -> u32 {
    WINDOWS.with(|windows| {
        windows.borrow().values().map(|shared| {
            let shared = shared.borrow();
            if shared.active_id == Some(id) { return 0; }
            shared.views.get(&id).map_or(0, |view| view.borrow().unread)
        }).max().unwrap_or(0)
    })
}
// --- end window and panels ---

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
        creation: CreationGate::default(),
        closed: false,
        app: app.clone(),
        profile_dir: profile.as_ref().map_or_else(PathBuf::new, |p| p.web_apps_dir()),
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
    arm_reload_timer(app.borrow().hwnd); // --- window and panels (thread T9) --- only when a panel reloads on a timer
}

/// Registers an app added after startup. The Customize panel or rail refresh path can call this.
pub fn register_app(app: &AppRef, web_app: WebApp) {
    let owner = app.borrow().hwnd.0 as isize;
    let shared = WINDOWS.with(|windows| windows.borrow().get(&owner).cloned());
    if let Some(shared) = shared { register_app_with(shared, web_app); }
}

struct Registration {
    view: Rc<RefCell<AppView>>,
    retired: Option<Rc<RefCell<AppView>>>,
}

/// Rail activation re-registers the current saved app on EVERY click. That is not a remove/re-add:
/// preserve its exact page registration, navigation/session, unread state and reload timestamp.
fn retain_or_replace_view(views: &mut HashMap<u32, Rc<RefCell<AppView>>>, id: u32, url: &str) -> Registration {
    if let Some(view) = views.get(&id) {
        let unchanged = {
            let view = view.borrow();
            view.initial_url == url && !matches!(view.state, ViewState::Removed)
        };
        if unchanged { return Registration { view: view.clone(), retired: None }; }
    }
    let view = Rc::new(RefCell::new(AppView {
        initial_url: url.to_owned(), current_url: url.to_owned(),
        state: ViewState::Loading, unread: 0, last_reload: Instant::now(),
    }));
    let retired = views.insert(id, view.clone());
    Registration { view, retired }
}

fn register_app_with(shared: SharedRef, web_app: WebApp) {
    let id = web_app.id;
    let (registration, host) = {
        let mut state = shared.borrow_mut();
        if state.closed { return; }
        (retain_or_replace_view(&mut state.views, id, &web_app.url), state.host.clone())
    };
    if let Some(retired) = registration.retired {
        retired.borrow_mut().state.remove();
        if let Some(host) = host { host.remove_app(&id.to_string()); }
    }
    let view = registration.view;
    // Capture this exact registration, never resurrect an ID from a retired factory.
    side_panel::register_content(PanelId::WebApp(id), move || {
        Box::new(Content::new(shared.clone(), id, view.clone()))
    });
}

/// Called when the rail's app list changes. Removes its controller and any live panel content.
pub fn forget_app(id: u32) {
    let shared_windows = WINDOWS.with(|windows| windows.borrow().values().cloned().collect::<Vec<_>>());
    for shared in shared_windows {
        let (view, host, owner) = {
            let mut state = shared.borrow_mut();
            if state.active_id == Some(id) { state.active_id = None; }
            (state.views.remove(&id), state.host.clone(), state.main_hwnd)
        };
        if let Some(view) = view { view.borrow_mut().state.remove(); }
        if let Some(host) = host { host.remove_app(&id.to_string()); }
        side_panel::close_content(owner, PanelId::WebApp(id));
    }
    side_panel::unregister_content(PanelId::WebApp(id));
}

/// Invalidate before cleanup; retired factories can still retain Shared after WINDOWS is cleared.
pub fn shutdown() {
    let shared_windows = WINDOWS.with(|windows| windows.borrow_mut().drain().map(|(_, shared)| shared).collect::<Vec<_>>());
    for shared in shared_windows {
        let host = {
            let mut state = shared.borrow_mut();
            state.closed = true;
            state.creation.close();
            state.parent = None;
            state.active_id = None;
            state.host.take()
        };
        if let Some(host) = host { host.close(); }
    }
    crate::engine::web_app::shutdown_profiles();
}

/// Clears all pinned-app cookies, storage, cache, and service workers for the active profile.
/// Customize can call this after confirming the user wants to clear every web app's site data.
pub fn clear_all_data() -> std::io::Result<()> {
    let dir = profile::active().map_err(|_| std::io::Error::new(std::io::ErrorKind::NotFound, "active profile unavailable"))?.web_apps_dir();
    crate::engine::web_app::begin_clear_profile(&dir)?;
    let shared_windows = WINDOWS.with(|windows| windows.borrow().values().cloned().collect::<Vec<_>>());
    for shared in shared_windows {
        let host = {
            let mut state = shared.borrow_mut();
            state.creation.invalidate_all();
            state.active_id = None;
            state.host.take()
        };
        if let Some(host) = host { host.close(); }
    }
    crate::engine::web_app::finish_clear_profile(&dir)
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
    fn new(shared: SharedRef, id: u32, view: Rc<RefCell<AppView>>) -> Self {
        Self { shared, id, view, bounds: RECT::default(), dpi: 96 }
    }

    fn is_current(&self) -> bool {
        let state = self.shared.borrow();
        !state.closed && state.views.get(&self.id).is_some_and(|view| Rc::ptr_eq(view, &self.view))
            && !matches!(self.view.borrow().state, ViewState::Removed)
    }

    fn ensure_host(&mut self, parent: HWND) {
        if !self.is_current() { return; }
        let (reservation, profile_dir, app, weak_shared, initial, existing) = {
            let mut shared = self.shared.borrow_mut();
            shared.parent = Some(parent);
            shared.active_id = Some(self.id);
            let reservation = match shared.creation.reserve("environment") {
                Ok(reservation) => reservation,
                Err(()) => return,
            };
            (reservation, shared.profile_dir.clone(), shared.app.clone(),
                Rc::downgrade(&self.shared), shared.webview_bounds, shared.host.is_some())
        };
        let Some(lease) = reservation else {
            if existing { self.open_current(); }
            return;
        };
        let events = make_events(weak_shared.clone(), app);
        let completion_lease = lease.clone();
        let result = WebAppHost::begin_create(parent, initial, &profile_dir, events, move |result| {
            let Some(shared) = weak_shared.upgrade() else { return; };
            // Factories can retain Shared after window shutdown. Weak upgrade alone is not a
            // validity test. No stale result may install a host or report an error to a re-add.
            if !shared.borrow().creation.pending("environment", &completion_lease) { return; }
            match result {
                Ok(host) => {
                    let active = {
                        let mut state = shared.borrow_mut();
                        if !state.creation.commit("environment", &completion_lease) { return; }
                        state.host = Some(host.clone());
                        let bounds = state.webview_bounds;
                        state.active_id.and_then(|id| state.views.get(&id).cloned().map(|view| (id, view, bounds)))
                    };
                    if let Some((id, view, bounds)) = active {
                        let url = view.borrow().initial_url.clone();
                        let _ = host.set_bounds(bounds);
                        let _ = host.open_app(&id.to_string(), &url);
                    }
                }
                Err(error) => {
                    let active = {
                        let mut state = shared.borrow_mut();
                        if state.creation.fail("environment", &completion_lease) { state.active_id } else { None }
                    };
                    if let Some(id) = active { set_error(&shared, id, format!("The web app could not start: {error}")); }
                }
            }
        });
        if let Err(error) = result {
            let active = {
                let mut state = self.shared.borrow_mut();
                if state.creation.fail("environment", &lease) { state.active_id } else { None }
            };
            if let Some(id) = active { set_error(&self.shared, id, format!("The web app could not start: {error}")); }
        }
    }

    fn open_current(&self) {
        if !self.is_current() { return; }
        let (host, url) = {
            let mut state = self.shared.borrow_mut();
            state.active_id = Some(self.id);
            (state.host.clone(), self.view.borrow().initial_url.clone())
        };
        if let Some(host) = host {
            if let Err(error) = host.open_app(&self.id.to_string(), &url) {
                if self.is_current() { self.view.borrow_mut().state.fail(format!("The web app could not open: {error}")); }
            }
        }
    }

    /// Re-runs a failed load, or starts the host again if it never came up.
    fn retry(&mut self) {
        if !self.is_current() { return; }
        self.view.borrow_mut().state.retry();
        let host = self.shared.borrow().host.clone();
        if let Some(host) = host {
            let id = self.id.to_string(); let url = self.view.borrow().initial_url.clone();
            let result = crate::web_app_lifecycle::retry_route(host.has_app(&id),
                || host.retry_app(&id), || host.open_app(&id, &url));
            if let Err(error) = result {
                if self.is_current() { self.view.borrow_mut().state.fail(format!("The web app could not retry: {error}")); }
            }
        }
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
        if !self.is_current() { return; }
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
        if !self.is_current() { return; }
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
    let title_weak = weak.clone(); // --- window and panels (thread T9) ---
    let nav_weak = weak.clone();
    let failed_weak = weak.clone();
    let (download_event, download_plan) = App::download_hooks(&app);
    WebAppEvents {
        download_event: Some(download_event),
        download_plan: Some(download_plan),
        policy_event: None,
        diagnostic_lifecycle: App::diagnostic_lifecycle_log(&app),
        // --- window and panels (thread T9) --- a count in the title ("(3) Inbox") marks the rail button.
        title_changed: Some(Rc::new(move |id, title| {
            let Some(shared) = title_weak.upgrade() else { return; };
            let Some(view) = lookup_view(&shared, id) else { return; };
            let count = if id.parse::<u32>().is_ok_and(|id| crate::panel_prefs::current(id).notify) { crate::panel_prefs::title_notifications(&title) } else { 0 };
            let changed = std::mem::replace(&mut view.borrow_mut().unread, count) != count;
            if changed { ICON_CHANGED.with(|slot| { if let Some(callback) = slot.borrow().as_ref() { callback(); } }); }
        })),
        // --- end window and panels ---
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
                AppNavigationState::Ready => { view.state = ViewState::Ready; view.last_reload = Instant::now(); }
                AppNavigationState::Failed if view.state == ViewState::Loading => view.state = ViewState::Error("The page failed to load.".into()),
                AppNavigationState::Failed => {}
            }
            }
            // --- window and panels (thread T9) --- the page is up: its saved zoom and mute apply.
            if let Ok(app_id) = id.parse::<u32>() { apply_prefs(&shared, app_id); }
            // --- end window and panels ---
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
    #[test]
    fn ordinary_a_b_a_registration_retains_hidden_page_and_session_state() {
        let mut views = HashMap::new();
        let a = retain_or_replace_view(&mut views, 1, "https://a.example/");
        assert!(a.retired.is_none());
        {
            let mut page = a.view.borrow_mut(); page.state = ViewState::Ready;
            page.current_url = "https://a.example/form-with-unsaved-work".to_owned(); page.unread = 7;
        }
        let timestamp = a.view.borrow().last_reload;
        let b = retain_or_replace_view(&mut views, 2, "https://b.example/");
        assert!(b.retired.is_none());
        let again = retain_or_replace_view(&mut views, 1, "https://a.example/");
        assert!(again.retired.is_none()); assert!(Rc::ptr_eq(&a.view, &again.view));
        let page = again.view.borrow();
        assert_eq!(page.state, ViewState::Ready); assert_eq!(page.unread, 7);
        assert_eq!(page.current_url, "https://a.example/form-with-unsaved-work");
        assert_eq!(page.last_reload, timestamp);
    }

    #[test]
    fn real_remove_readd_and_url_edit_bind_new_registration_not_retired_factory_view() {
        let mut views = HashMap::new(); let old = retain_or_replace_view(&mut views, 1, "https://a.example/").view;
        views.remove(&1).unwrap().borrow_mut().state.remove();
        let added = retain_or_replace_view(&mut views, 1, "https://a.example/");
        assert!(added.retired.is_none()); assert!(!Rc::ptr_eq(&old, &added.view));
        assert_eq!(old.borrow().state, ViewState::Removed);
        let edited = retain_or_replace_view(&mut views, 1, "https://new.example/");
        assert!(Rc::ptr_eq(edited.retired.as_ref().unwrap(), &added.view));
        assert!(!Rc::ptr_eq(&edited.view, &added.view));
        assert_eq!(edited.view.borrow().initial_url, "https://new.example/");
    }

}
