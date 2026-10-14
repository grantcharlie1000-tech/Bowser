//! Isolated WebView2 host for the HTML chrome toolbar (see docs/html-chrome-protocol.md).
//!
//! Each chrome view is one controller in the active browsing profile's tab environment (shared
//! browser, GPU and network processes), but in its own private, InPrivate WebView2 profile
//! (`CHROME_PROFILE`), so it shares no cookies, storage or permissions with browsing. The page can
//! reach nothing except the embedded assets served through the `bowser-ui` custom scheme, and its
//! only channel back to Rust is the string bridge in `protocol.rs`.

use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    time::Instant,
};

use url::Url;
use webview2_com::{
    AcceleratorKeyPressedEventHandler, CoTaskMemPWSTR, CoreWebView2CustomSchemeRegistration,
    CreateCoreWebView2ControllerCompletedHandler, DownloadStartingEventHandler,
    FrameCreatedEventHandler, FrameNavigationStartingEventHandler, IFixedEnvironmentOptions4,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler,
    PermissionRequestedEventHandler, ProcessFailedEventHandler, WebMessageReceivedEventHandler,
    WebResourceRequestedEventHandler, Microsoft::Web::WebView2::Win32::*,
};
use windows::{
    Win32::{
        Foundation::{E_FAIL, E_POINTER, HWND, RECT},
        System::Com::IStream,
        UI::{
            Input::KeyboardAndMouse::{GetKeyState, VIRTUAL_KEY, VK_CONTROL, VK_MENU, VK_SHIFT},
            Shell::SHCreateMemStream,
        },
    },
    core::{Error, Interface, PCWSTR, PWSTR, Result},
};

use super::{
    assets::{
        self, Asset, DIALOG_START_URL, HOVER_START_URL, NOTES_DIALOG_START_URL, PANEL_START_URL, POPUP_START_URL,
        PRIVACY_DIALOG_START_URL, RAIL_START_URL, RENAME_DIALOG_START_URL, RESPONSE_HEADERS, SCHEME,
        SETTINGS_START_URL, START_URL, TABS_START_URL, HISTORY_PAGE_START_URL, DOWNLOADS_PAGE_START_URL,
    },
    protocol::{
        Command, PanelType, MESSAGE_BURST, MESSAGE_RATE_PER_SEC, POPUP_COMMAND_INTERVAL, Throttle,
        TokenBucket, is_popup_command, parse_command,
    },
};
// --- split view ---
use super::{assets::SPLIT_START_URL, protocol::SplitCommand};
// --- end split view ---
use crate::shortcuts::{self, Command as Shortcut};

const VK_BACK: u32 = 0x08;
const VK_ESCAPE: u32 = 0x1B;
const VK_END: u32 = 0x23;
const VK_HOME: u32 = 0x24;
const VK_LEFT: u32 = 0x25;
const VK_RIGHT: u32 = 0x27;
const VK_DELETE: u32 = 0x2E;
const VK_A: u32 = 0x41;
const VK_C: u32 = 0x43;
const VK_V: u32 = 0x56;
const VK_X: u32 = 0x58;
const VK_Y: u32 = 0x59;
const VK_Z: u32 = 0x5A;

/// One key press the chrome page did not consume, read from an `AcceleratorKeyPressed` event.
/// Modifiers are the physical state at the time of the event (`GetKeyState`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accel {
    pub vk: u32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// KeyDown or SystemKeyDown. Key-up events are never claimed.
    pub key_down: bool,
    /// SystemKeyDown or SystemKeyUp (Alt is held, or F10).
    pub is_system: bool,
    /// `WasKeyDown`: this key press is an auto-repeat.
    pub repeat: bool,
}

/// Called for each key the chrome page did not consume. Returns true when the key was claimed; the
/// event is then marked handled and never reaches the page.
pub type AccelHandler = Rc<dyn Fn(Accel) -> bool>;

/// Maps a chrome key press to the browser command it runs, if any. Pure.
///
/// Uses the same chord table as the tab WebViews (`shortcuts::command_for`), so modifiers match
/// exactly. Escape never claims a key: it must reach the page (it cancels omnibox edits there).
/// While the HTML omnibox is being edited, editing chords are left to the field: Ctrl+A/C/V/X/Z,
/// Ctrl+Y, Ctrl+Shift+Z, and Ctrl or Shift with Left/Right/Home/End/Backspace/Delete. Alt+Left and
/// Alt+Right stay navigation. Repeats of toggle commands are not claimed.
pub fn accel_command(accel: Accel, omnibox_editing: bool) -> Option<Shortcut> {
    accel_command_gated(accel, omnibox_editing, shortcuts::GatedChords::default())
}

/// [`accel_command`] that also claims the setting-gated chords `gated` binds right now.
pub fn accel_command_gated(
    accel: Accel,
    omnibox_editing: bool,
    gated: shortcuts::GatedChords,
) -> Option<Shortcut> {
    if !accel.key_down || accel.vk == VK_ESCAPE {
        return None;
    }
    if omnibox_editing && is_editing_chord(accel) {
        return None;
    }
    let command =
        shortcuts::command_for_gated(accel.vk, accel.ctrl, accel.shift, accel.alt, gated)?;
    if accel.repeat && shortcuts::is_toggle(command) {
        return None;
    }
    Some(command)
}

/// Key chords the text field uses for editing or selection, so the omnibox keeps them.
fn is_editing_chord(accel: Accel) -> bool {
    match accel.vk {
        // Ctrl+Shift+Delete opens the Privacy dashboard even from the field.
        VK_DELETE if accel.ctrl && accel.shift => false,
        VK_LEFT | VK_RIGHT | VK_HOME | VK_END | VK_BACK | VK_DELETE => accel.ctrl || accel.shift,
        VK_V => accel.ctrl && !accel.shift,
        VK_A | VK_C | VK_X | VK_Z => accel.ctrl,
        VK_Y => accel.ctrl && !accel.shift,
        _ => false,
    }
}

/// Reads the key and modifier state of an accelerator event. `None` when the event cannot be read.
fn read_accel(args: &ICoreWebView2AcceleratorKeyPressedEventArgs) -> Option<Accel> {
    let mut vk = 0u32;
    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
    let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
    unsafe {
        args.VirtualKey(&mut vk).ok()?;
        args.KeyEventKind(&mut kind).ok()?;
        args.PhysicalKeyStatus(&mut status).ok()?;
    }
    let held = |key: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(key.0)) < 0 };
    Some(Accel {
        vk,
        ctrl: held(VK_CONTROL),
        shift: held(VK_SHIFT),
        alt: held(VK_MENU),
        key_down: matches!(
            kind,
            COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN | COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
        ),
        is_system: matches!(
            kind,
            COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN | COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_UP
        ),
        repeat: status.WasKeyDown.as_bool(),
    })
}

const TRANSPARENT: COREWEBVIEW2_COLOR = COREWEBVIEW2_COLOR {
    A: 0,
    R: 0,
    G: 0,
    B: 0,
};

type ReadySlot<T> = Rc<RefCell<Option<Box<dyn FnOnce(Result<T>)>>>>;

/// Which page a chrome host shows. Each host accepts only its own start URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Toolbar,
    Tabs,
    Rail,
    Panel,
    Popup,
    Dialog,
    DialogNotes,
    DialogPrivacy,
    DialogRename,
    /// The no-activate hover layer: tooltips, the tab hover card and the island menu.
    Hover,
    // --- split view ---
    /// The split view's empty side ("choose a tab") and the tab drop-zone preview.
    Split,
    // --- end split view ---
    /// The settings page shown over a `bowser://settings` tab (docs/design/settings-page.md). The
    /// only view whose commands change settings.
    Settings,
    /// The history page shown over a `bowser://history` tab (docs/design/history-page.md). Sends
    /// only the `hpage` family.
    HistoryPage,
    /// The downloads page shown over a `bowser://downloads` tab (docs/design/downloads.md). Sends
    /// only the `dpage` family.
    DownloadsPage,
}

impl View {
    pub fn start_url(self) -> &'static str {
        match self {
            Self::Toolbar => START_URL,
            Self::Tabs => TABS_START_URL,
            Self::Rail => RAIL_START_URL,
            Self::Panel => PANEL_START_URL,
            Self::Popup => POPUP_START_URL,
            Self::Dialog => DIALOG_START_URL,
            Self::DialogNotes => NOTES_DIALOG_START_URL,
            Self::DialogPrivacy => PRIVACY_DIALOG_START_URL,
            Self::DialogRename => RENAME_DIALOG_START_URL,
            Self::Hover => HOVER_START_URL,
            Self::Split => SPLIT_START_URL,
            Self::Settings => SETTINGS_START_URL,
            Self::HistoryPage => HISTORY_PAGE_START_URL,
            Self::DownloadsPage => DOWNLOADS_PAGE_START_URL,
        }
    }
}

/// WebView2 profile that every chrome host runs in. It lives in the browsing profile's environment
/// (sharing its browser, GPU, network and utility processes) but is a separate, InPrivate WebView2
/// profile, so the chrome's cookies, storage, cache and permissions never mix with the tabs' and
/// nothing is written to disk.
pub const CHROME_PROFILE: &str = "BowserChrome";

/// Whether `name` is a valid WebView2 profile name: 1 to 64 ASCII letters, digits, `_` or `-`.
/// (WebView2 also accepts a few more punctuation characters; Bowser keeps to this strict subset.)
pub fn valid_profile_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// Registers the `bowser-ui` scheme on environment options that are about to create the tab
/// environment. Only the chrome webviews have a `WebResourceRequested` handler that serves it, and
/// tab navigation blocks the scheme, so a tab can never load chrome assets.
pub fn register_ui_scheme(options: &impl Interface) -> Result<()> {
    let options4: IFixedEnvironmentOptions4 = options.cast()?;
    let scheme: ICoreWebView2CustomSchemeRegistration =
        CoreWebView2CustomSchemeRegistration::new(SCHEME.to_owned()).into();
    unsafe {
        scheme.SetTreatAsSecure(true)?;
        scheme.SetHasAuthorityComponent(true)?;
        // The generated wrapper method is private (the macro drops the trait's visibility), so the
        // COM slot is called through the vtable; the signature is the same one webview2-com uses
        // in its own tests. No allowed origins are set, so no other origin may request the scheme.
        let schemes = [scheme.as_raw()];
        (Interface::vtable(&options4).SetCustomSchemeRegistrations)(
            Interface::as_raw(&options4),
            1,
            schemes.as_ptr(),
        )
        .ok()
    }
}

/// The WebView2 environment the chrome hosts are created in: the active browsing profile's tab
/// environment, created with the `bowser-ui` scheme registered (see [`register_ui_scheme`]). The
/// chrome itself runs in the private [`CHROME_PROFILE`]. Clones share one environment. Each live
/// host keeps a clone; the engine owns the environment's lifetime, and the window closes its chrome
/// hosts before the engine closes. Closing a host never touches the environment.
#[derive(Clone)]
pub struct ChromeEnv {
    inner: Rc<ICoreWebView2Environment>,
}

impl ChromeEnv {
    /// Wraps the shared tab environment. It must have been created with [`register_ui_scheme`].
    pub fn new(environment: ICoreWebView2Environment) -> Self {
        Self {
            inner: Rc::new(environment),
        }
    }
}

/// Where a chrome controller sits while its page loads: one pixel at the parent's top-left corner.
///
/// The controller must be visible while it loads. Chromium treats a hidden WebView2 as a background
/// page and drops its renderer to idle priority, so under CPU contention (parallel builds, benchmark
/// workloads) the page can take many seconds, or never, to run its script and post `ready`, and the
/// window keeps the native chrome. One visible pixel keeps the renderer at normal priority without
/// covering native controls (whose clicks a transparent full-size view would swallow) while it loads.
const LOADING_BOUNDS: RECT = RECT { left: 0, top: 0, right: 1, bottom: 1 };

/// Everything the creation callbacks need. Cheap to clone; all fields are shared or Copy.
#[derive(Clone)]
struct Setup {
    env: ChromeEnv,
    parent: HWND,
    bounds: RECT,
    view: View,
    on_command: Rc<dyn Fn(Command)>,
    on_failed: Rc<dyn Fn()>,
    on_accelerator: Option<AccelHandler>,
    ready: ReadySlot<ChromeHost>,
}

struct State {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
    /// Keeps the shared environment alive for as long as this host exists.
    _env: ChromeEnv,
    /// The window the controller is a child of; `applied_bounds` are in its client coordinates.
    parent: HWND,
    applied_bounds: RECT,
    /// Where the caller wants the view. Applied once the page has said `ready`; until then the
    /// controller is parked at `LOADING_BOUNDS`.
    wanted_bounds: RECT,
    /// What the caller last asked for. Defaults to visible; only shown once the page has said `ready`.
    wanted_visible: bool,
    applied_visible: Option<bool>,
    /// Set when the page posts `ready`.
    revealed: bool,
    closed: bool,
    removers: Vec<Box<dyn FnOnce()>>,
    failures: FailureCounter,
    on_failed: Rc<dyn Fn()>,
}

#[derive(Default)]
struct FailureCounter {
    consecutive: u8,
    tripped: bool,
}

impl FailureCounter {
    /// Returns true exactly once, on the third consecutive failure.
    fn record(&mut self, success: bool) -> bool {
        if success {
            self.consecutive = 0;
            return false;
        }
        self.consecutive = self.consecutive.saturating_add(1);
        if self.consecutive >= 3 && !self.tripped {
            self.tripped = true;
            return true;
        }
        false
    }
}

/// A WebView2 controller showing one chrome view. Clones share one controller.
#[derive(Clone)]
pub struct ChromeHost {
    state: Rc<RefCell<State>>,
}

impl ChromeHost {
    /// Starts creating a controller for `view` in the shared `env`, as a child of `parent`. `on_ready`
    /// runs once, with the host or the reason creation failed. The controller stays hidden until the
    /// page posts `ready`. `on_accelerator`, when given, sees each key the page did not consume (see
    /// [`AccelHandler`]). Returns an error only when the request could not be started; then
    /// `on_ready` never runs.
    pub fn create(
        env: &ChromeEnv,
        parent: HWND,
        bounds: RECT,
        view: View,
        on_command: Rc<dyn Fn(Command)>,
        on_failed: Rc<dyn Fn()>,
        on_accelerator: Option<AccelHandler>,
        on_ready: impl FnOnce(Result<ChromeHost>) + 'static,
    ) -> Result<()> {
        let setup = Setup {
            env: env.clone(),
            parent,
            bounds,
            view,
            on_command,
            on_failed,
            on_accelerator,
            ready: Rc::new(RefCell::new(Some(Box::new(on_ready)))),
        };
        create_controller(&env.inner, setup)
    }

    pub fn set_bounds(&self, bounds: RECT) -> Result<()> {
        let controller = {
            let mut state = self.state.try_borrow_mut().map_err(|_| fail("Chrome host is busy"))?;
            if state.closed { return Err(fail("Chrome host is closed")); }
            state.wanted_bounds = bounds;
            if !state.revealed { return Ok(()); }
            if same_rect(state.applied_bounds, bounds) { return Ok(()); }
            state.controller.clone()
        };
        self.observe(unsafe { controller.SetBounds(bounds) })?;
        self.state.try_borrow_mut().map_err(|_| fail("Chrome host is busy"))?.applied_bounds = bounds;
        Ok(())
    }

    /// The controller's bounds in screen coordinates, or `None` when the host is closed or busy.
    pub fn screen_bounds(&self) -> Option<RECT> {
        let (parent, bounds) = {
            let state = self.state.try_borrow().ok()?;
            if state.closed { return None; }
            (state.parent, state.applied_bounds)
        };
        let mut origin = windows::Win32::Foundation::POINT::default();
        if !unsafe { windows::Win32::Graphics::Gdi::ClientToScreen(parent, &mut origin) }.as_bool() {
            return None;
        }
        Some(RECT {
            left: origin.x + bounds.left,
            top: origin.y + bounds.top,
            right: origin.x + bounds.right,
            bottom: origin.y + bounds.bottom,
        })
    }

    /// Sends one JSON message to the page and reports failure to the caller.
    pub fn post_json(&self, json: &str) -> Result<()> {
        let webview = {
            let state = self.state.try_borrow().map_err(|_| fail("Chrome host is busy"))?;
            if state.closed { return Err(fail("Chrome host is closed")); }
            state.webview.clone()
        };
        let message = wide(json);
        self.observe(unsafe { webview.PostWebMessageAsJson(PCWSTR(message.as_ptr())) })
    }

    /// Moves keyboard focus into the chrome page (programmatic move).
    pub fn focus(&self) -> Result<()> {
        let controller = {
            let state = self.state.try_borrow().map_err(|_| fail("Chrome host is busy"))?;
            if state.closed { return Err(fail("Chrome host is closed")); }
            state.controller.clone()
        };
        self.observe(unsafe { controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) })
    }

    /// Starts suspension. Initiation errors are returned; completion failures notify `on_failed`.
    pub fn try_suspend(&self) -> Result<()> {
        let webview = {
            let state = self.state.try_borrow().map_err(|_| fail("Chrome host is busy"))?;
            if state.closed { return Err(fail("Chrome host is closed")); }
            state.webview.clone()
        };
        let webview3: ICoreWebView2_3 = webview.cast()?;
        let weak = Rc::downgrade(&self.state);
        let handler = webview2_com::TrySuspendCompletedHandler::create(Box::new(
            move |result, suspended| {
                if result.is_err() || !suspended {
                    if let Some(state) = weak.upgrade() {
                        let callback = state
                            .try_borrow()
                            .ok()
                            .filter(|s| !s.closed)
                            .map(|s| s.on_failed.clone());
                        if let Some(callback) = callback { callback(); }
                    }
                }
                Ok(())
            },
        ));
        self.observe(unsafe { webview3.TrySuspend(&handler) })
    }

    /// Wakes a suspended page. COM and re-entry errors are returned to the caller.
    pub fn resume(&self) -> Result<()> {
        let webview = {
            let state = self.state.try_borrow().map_err(|_| fail("Chrome host is busy"))?;
            if state.closed { return Err(fail("Chrome host is closed")); }
            state.webview.clone()
        };
        let webview3: ICoreWebView2_3 = webview.cast()?;
        self.observe(unsafe { webview3.Resume() })
    }

    /// Records the caller's visibility. The controller is only shown once the page is ready.
    pub fn set_visible(&self, visible: bool) -> Result<()> {
        let controller = {
            let mut state = self.state.try_borrow_mut().map_err(|_| fail("Chrome host is busy"))?;
            if state.closed { return Err(fail("Chrome host is closed")); }
            state.wanted_visible = visible;
            if !state.revealed { return Ok(()); }
            if state.applied_visible == Some(visible) { return Ok(()); }
            state.controller.clone()
        };
        self.observe(unsafe { controller.SetIsVisible(visible) })?;
        self.state.try_borrow_mut().map_err(|_| fail("Chrome host is busy"))?.applied_visible = Some(visible);
        Ok(())
    }

    pub fn close(&self) {
        let (controller, removers) = {
            let Ok(mut state) = self.state.try_borrow_mut() else { return; };
            if state.closed { return; }
            state.closed = true;
            (state.controller.clone(), std::mem::take(&mut state.removers))
        };
        for remove in removers { remove(); }
        let _ = unsafe { controller.Close() };
    }

    fn observe(&self, result: Result<()>) -> Result<()> {
        let failed = {
            let mut state = self.state.try_borrow_mut().map_err(|_| fail("Chrome host is busy"))?;
            if state.closed { return Err(fail("Chrome host is closed")); }
            if state.failures.record(result.is_ok()) { Some(state.on_failed.clone()) } else { None }
        };
        if let Some(on_failed) = failed { on_failed(); }
        result
    }
}

impl Drop for ChromeHost {
    fn drop(&mut self) {
        if Rc::strong_count(&self.state) == 1 {
            self.close();
        }
    }
}

fn fail(message: &str) -> Error {
    Error::new(E_FAIL, message)
}

fn same_rect(a: RECT, b: RECT) -> bool {
    a.left == b.left && a.top == b.top && a.right == b.right && a.bottom == b.bottom
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Runs the one-shot ready callback, if it has not run yet.
fn finish<T>(ready: &ReadySlot<T>, result: Result<T>) {
    let callback = ready.borrow_mut().take();
    if let Some(callback) = callback {
        callback(result);
    }
}

/// Whether a chrome host should fall back to native chrome after a `ProcessFailed` of `kind`.
///
/// The chrome shares the tab environment, so it also hears about shared processes that WebView2
/// restarts on its own (GPU, utility, sandbox helper, plugin, unknown); the chrome page survives
/// those and stays. A browser-process exit, or the chrome's own renderer exiting or hanging, ends
/// the chrome page: fall back. A failure whose kind cannot be read also falls back (`None`).
fn process_failure_is_fatal(kind: Option<COREWEBVIEW2_PROCESS_FAILED_KIND>) -> bool {
    !matches!(
        kind,
        Some(
            COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED
                | COREWEBVIEW2_PROCESS_FAILED_KIND_UTILITY_PROCESS_EXITED
                | COREWEBVIEW2_PROCESS_FAILED_KIND_SANDBOX_HELPER_PROCESS_EXITED
                | COREWEBVIEW2_PROCESS_FAILED_KIND_PPAPI_PLUGIN_PROCESS_EXITED
                | COREWEBVIEW2_PROCESS_FAILED_KIND_PPAPI_BROKER_PROCESS_EXITED
                | COREWEBVIEW2_PROCESS_FAILED_KIND_UNKNOWN_PROCESS_EXITED
        )
    )
}

fn create_controller(environment: &ICoreWebView2Environment, setup: Setup) -> Result<()> {
    debug_assert!(valid_profile_name(CHROME_PROFILE));
    let environment10: ICoreWebView2Environment10 = environment.cast()?;
    let options = unsafe { environment10.CreateCoreWebView2ControllerOptions()? };
    // A separate, in-memory WebView2 profile: the chrome never shares cookies, storage, cache or
    // permissions with the browsing profile, and nothing persists on disk. Both are required; a
    // runtime that refuses either fails creation and the window keeps its native chrome.
    let profile_name = wide(CHROME_PROFILE);
    unsafe {
        options.SetProfileName(PCWSTR(profile_name.as_ptr()))?;
        options.SetIsInPrivateModeEnabled(true)?;
    }
    // Preferred path: transparent background set at creation. If the runtime lacks
    // ControllerOptions3 the value is applied right after creation instead (see `prepare`).
    let background_at_creation = unsafe {
        options
            .cast::<ICoreWebView2ControllerOptions3>()
            .and_then(|options3| options3.SetDefaultBackgroundColor(TRANSPARENT))
            .is_ok()
    };
    let environment = environment.clone();
    let handler_setup = setup.clone();
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |status, controller| {
            let result = match (status, controller) {
                (Ok(()), Some(controller)) => {
                    configure(&environment, controller, &handler_setup, background_at_creation)
                }
                (Err(error), _) => Err(error),
                _ => Err(Error::from(E_POINTER)),
            };
            finish(&handler_setup.ready, result);
            Ok(())
        },
    ));
    unsafe {
        environment10.CreateCoreWebView2ControllerWithOptions(setup.parent, &options, &handler)
    }
}

/// Builds the host around a fresh controller. Once the `ChromeHost` exists, dropping it on an
/// error closes the controller and removes any handlers already installed.
fn configure(
    environment: &ICoreWebView2Environment,
    controller: ICoreWebView2Controller,
    setup: &Setup,
    background_at_creation: bool,
) -> Result<ChromeHost> {
    let webview = match unsafe { controller.CoreWebView2() } {
        Ok(webview) => webview,
        Err(error) => {
            let _ = unsafe { controller.Close() };
            return Err(error);
        }
    };
    let host = ChromeHost {
        state: Rc::new(RefCell::new(State {
            controller,
            webview,
            _env: setup.env.clone(),
            parent: setup.parent,
            applied_bounds: LOADING_BOUNDS,
            wanted_bounds: setup.bounds,
            wanted_visible: true,
            applied_visible: Some(true),
            revealed: false,
            closed: false,
            removers: Vec::new(),
            failures: FailureCounter::default(),
            on_failed: setup.on_failed.clone(),
        })),
    };
    prepare(&host, environment, setup, background_at_creation)?;
    Ok(host)
}

/// Geometry, settings, handlers, then the first navigation. Handlers exist before navigation.
fn prepare(
    host: &ChromeHost,
    environment: &ICoreWebView2Environment,
    setup: &Setup,
    background_at_creation: bool,
) -> Result<()> {
    let (controller, webview) = {
        let state = host.state.borrow();
        (state.controller.clone(), state.webview.clone())
    };
    unsafe {
        // Loads while visible but parked at one pixel (see `LOADING_BOUNDS`); `reveal` moves it.
        controller.SetBounds(LOADING_BOUNDS)?;
        controller.SetIsVisible(true)?;
        if !background_at_creation {
            let controller2: ICoreWebView2Controller2 = controller.cast()?;
            controller2.SetDefaultBackgroundColor(TRANSPARENT)?;
        }
        if let Ok(controller4) = controller.cast::<ICoreWebView2Controller4>() {
            controller4.SetAllowExternalDrop(false)?;
        }
    }
    verify_chrome_profile(&webview)?;
    configure_settings(&webview)?;
    install_handlers(host, environment, setup, &webview)?;
    let start = wide(setup.view.start_url());
    unsafe { webview.Navigate(PCWSTR(start.as_ptr())) }
}

/// Confirms the controller really runs in the private chrome profile, so a runtime that ignored the
/// controller options can never put the chrome in the browsing profile.
fn verify_chrome_profile(webview: &ICoreWebView2) -> Result<()> {
    let webview13: ICoreWebView2_13 = webview.cast()?;
    let profile = unsafe { webview13.Profile()? };
    let mut name = PWSTR::null();
    let mut in_private = windows::core::BOOL::default();
    unsafe {
        profile.ProfileName(&mut name)?;
        profile.IsInPrivateModeEnabled(&mut in_private)?;
    }
    let name = CoTaskMemPWSTR::from(name).to_string();
    if name.eq_ignore_ascii_case(CHROME_PROFILE) && in_private.as_bool() {
        Ok(())
    } else {
        Err(fail("WebView2 did not use the private chrome profile"))
    }
}

fn configure_settings(webview: &ICoreWebView2) -> Result<()> {
    unsafe {
        let settings = webview.Settings()?;
        settings.SetIsWebMessageEnabled(true)?;
        settings.SetAreHostObjectsAllowed(false)?;
        settings.SetAreDefaultContextMenusEnabled(false)?;
        settings.SetIsStatusBarEnabled(false)?;
        settings.SetIsZoomControlEnabled(false)?;
        settings.SetAreDefaultScriptDialogsEnabled(false)?;
        settings.SetAreDevToolsEnabled(cfg!(debug_assertions))?;
        let settings3: ICoreWebView2Settings3 = settings.cast()?;
        settings3.SetAreBrowserAcceleratorKeysEnabled(false)?;
        let settings4: ICoreWebView2Settings4 = settings.cast()?;
        settings4.SetIsPasswordAutosaveEnabled(false)?;
        let settings5: ICoreWebView2Settings5 = settings.cast()?;
        settings5.SetIsPinchZoomEnabled(false)?;
        let settings6: ICoreWebView2Settings6 = settings.cast()?;
        settings6.SetIsSwipeNavigationEnabled(false)?;
        settings6.SetIsGeneralAutofillEnabled(false)?;
    }
    Ok(())
}

/// Installs every event handler. Each handler holds only a `Weak` to the host state, so the
/// controller does not keep the host alive.
fn install_handlers(
    host: &ChromeHost,
    environment: &ICoreWebView2Environment,
    setup: &Setup,
    webview: &ICoreWebView2,
) -> Result<()> {
    let weak = Rc::downgrade(&host.state);
    let mut removers: Vec<Box<dyn FnOnce()>> = Vec::new();
    let mut token: i64 = 0;
    let view = setup.view;

    unsafe {
        // Top-level navigation: only this host's start URL may load.
        webview.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    let allowed = args.Uri(&mut uri).is_ok()
                        && is_start_url(view, &CoTaskMemPWSTR::from(uri).to_string());
                    if !allowed {
                        args.SetCancel(true)?;
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        let w = webview.clone();
        removers.push(Box::new(move || {
            let _ = w.remove_NavigationStarting(token);
        }));

        // Subframes never navigate: each frame created gets a NavigationStarting that cancels.
        let webview4: ICoreWebView2_4 = webview.cast()?;
        webview4.add_FrameCreated(
            &FrameCreatedEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let frame: ICoreWebView2Frame2 = args.Frame()?.cast()?;
                    let mut frame_token: i64 = 0;
                    frame.add_NavigationStarting(
                        &FrameNavigationStartingEventHandler::create(Box::new(
                            move |_frame, args| {
                                if let Some(args) = args {
                                    args.SetCancel(true)?;
                                }
                                Ok(())
                            },
                        )),
                        &mut frame_token,
                    )?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        let w = webview4.clone();
        removers.push(Box::new(move || {
            let _ = w.remove_FrameCreated(token);
        }));

        // Popups are swallowed.
        webview.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    args.SetHandled(true)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        let w = webview.clone();
        removers.push(Box::new(move || {
            let _ = w.remove_NewWindowRequested(token);
        }));

        // Downloads are cancelled.
        webview4.add_DownloadStarting(
            &DownloadStartingEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    args.SetCancel(true)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        let w = webview4.clone();
        removers.push(Box::new(move || {
            let _ = w.remove_DownloadStarting(token);
        }));

        // Every permission request is denied.
        webview.add_PermissionRequested(
            &PermissionRequestedEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        let w = webview.clone();
        removers.push(Box::new(move || {
            let _ = w.remove_PermissionRequested(token);
        }));

        let on_failed = setup.on_failed.clone();
        webview.add_ProcessFailed(
            &ProcessFailedEventHandler::create(Box::new(move |_sender, args| {
                let kind = args.and_then(|args| {
                    let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                    args.ProcessFailedKind(&mut kind).ok().map(|()| kind)
                });
                if process_failure_is_fatal(kind) {
                    on_failed();
                }
                Ok(())
            })),
            &mut token,
        )?;
        let w = webview.clone();
        removers.push(Box::new(move || {
            let _ = w.remove_ProcessFailed(token);
        }));

        // The bridge: top-level page only, string payloads only, and a rate limit per host. Dropped
        // messages never reach the parser or the caller.
        let on_command = setup.on_command.clone();
        let weak_bridge = weak.clone();
        let mut bucket = TokenBucket::new(MESSAGE_BURST, MESSAGE_RATE_PER_SEC);
        let mut popup_gate = Throttle::new(POPUP_COMMAND_INTERVAL);
        webview.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else {
                    return Ok(());
                };
                // Both the sender's source and the message's own source must be this host's page.
                let mut sender_source = PWSTR::null();
                if sender.Source(&mut sender_source).is_err() {
                    return Ok(());
                }
                let sender_uri = CoTaskMemPWSTR::from(sender_source).to_string();
                let mut message_source = PWSTR::null();
                if args.Source(&mut message_source).is_err() {
                    return Ok(());
                }
                let message_uri = CoTaskMemPWSTR::from(message_source).to_string();
                if !is_trusted_message(view, &sender_uri, &message_uri) {
                    return Ok(());
                }
                if !bucket.take(Instant::now()) {
                    return Ok(());
                }
                let mut raw = PWSTR::null();
                if args.TryGetWebMessageAsString(&mut raw).is_err() {
                    return Ok(());
                }
                let raw = CoTaskMemPWSTR::from(raw).to_string();
                let Ok(command) = parse_command(&raw) else {
                    return Ok(());
                };
                if !view_accepts_command(view, &command) { return Ok(()); }
                if matches!(command, Command::Ready) {
                    reveal(&weak_bridge);
                } else if is_popup_command(&command) && !popup_gate.allow(Instant::now()) {
                    return Ok(());
                }
                on_command(command);
                Ok(())
            })),
            &mut token,
        )?;
        let w = webview.clone();
        removers.push(Box::new(move || {
            let _ = w.remove_WebMessageReceived(token);
        }));

        // Keys the page does not consume reach the caller, which may claim them as browser
        // shortcuts. Browser accelerator keys are disabled in settings, so nothing else runs here.
        if let Some(on_accelerator) = setup.on_accelerator.clone() {
            let controller = host.state.borrow().controller.clone();
            controller.add_AcceleratorKeyPressed(
                &AcceleratorKeyPressedEventHandler::create(Box::new(move |_sender, args| {
                    let Some(args) = args else {
                        return Ok(());
                    };
                    if let Some(accel) = read_accel(&args)
                        && on_accelerator(accel)
                    {
                        args.SetHandled(true)?;
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            let c = controller.clone();
            removers.push(Box::new(move || {
                let _ = c.remove_AcceleratorKeyPressed(token);
            }));
        }

        // Every `bowser-ui` request is answered here: embedded asset or 404. The filter below keeps
        // real network requests out of this handler, so a page load never wakes it.
        let env = environment.clone();
        webview.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_sender, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                // An unreadable URI yields an empty string, which matches no asset (fails closed).
                let uri = args
                    .Request()
                    .ok()
                    .and_then(|request| {
                        let mut uri = PWSTR::null();
                        request.Uri(&mut uri).ok()?;
                        Some(CoTaskMemPWSTR::from(uri).to_string())
                    })
                    .unwrap_or_default();
                let asset = request_path(&uri).and_then(|path| assets::lookup(&path));
                respond(&env, &args, asset)
            })),
            &mut token,
        )?;
        let w = webview.clone();
        removers.push(Box::new(move || {
            let _ = w.remove_WebResourceRequested(token);
        }));
        let filter = wide(UI_REQUEST_FILTER);
        webview.AddWebResourceRequestedFilter(
            PCWSTR(filter.as_ptr()),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
        )?;
    }

    host.state.try_borrow_mut().map_err(|_| fail("Chrome host is busy"))?.removers.append(&mut removers);
    Ok(())
}

/// Answers one request with the asset, or a 404 with an empty body when there is no asset or the
/// asset response cannot be built.
fn respond(
    environment: &ICoreWebView2Environment,
    args: &ICoreWebView2WebResourceRequestedEventArgs,
    asset: Option<Asset>,
) -> Result<()> {
    let found = asset.and_then(|asset| asset_response(environment, asset).ok());
    let response = match found {
        Some(response) => response,
        None => {
            let headers = wide(RESPONSE_HEADERS);
            let reason = wide("Not Found");
            unsafe {
                environment.CreateWebResourceResponse(
                    None::<&IStream>,
                    404,
                    PCWSTR(reason.as_ptr()),
                    PCWSTR(headers.as_ptr()),
                )?
            }
        }
    };
    unsafe { args.SetResponse(&response) }
}

fn asset_response(
    environment: &ICoreWebView2Environment,
    asset: Asset,
) -> Result<ICoreWebView2WebResourceResponse> {
    // SHCreateMemStream copies the bytes, so the embedded slice is not referenced afterwards.
    let stream = unsafe { SHCreateMemStream(Some(asset.bytes)) }
        .ok_or_else(|| fail("Could not create the asset stream"))?;
    let headers = wide(&format!("Content-Type: {}\r\n{RESPONSE_HEADERS}", asset.mime));
    let reason = wide("OK");
    unsafe {
        environment.CreateWebResourceResponse(
            Some(&stream),
            200,
            PCWSTR(reason.as_ptr()),
            PCWSTR(headers.as_ptr()),
        )
    }
}

/// The page said `ready`: move the controller from its loading spot to the caller's bounds, and
/// show it there if the caller wants it visible (otherwise hide it before it moves).
fn reveal(weak: &Weak<RefCell<State>>) {
    let Some(shared) = weak.upgrade() else {
        return;
    };
    let (wanted_bounds, wanted_visible) = {
        let Ok(mut state) = shared.try_borrow_mut() else { return; };
        if state.closed || state.revealed { return; }
        state.revealed = true;
        (state.wanted_bounds, state.wanted_visible)
    };
    let host = ChromeHost { state: shared };
    if wanted_visible {
        let _ = host.set_bounds(wanted_bounds);
        let _ = host.set_visible(true);
    } else {
        let _ = host.set_visible(false);
        let _ = host.set_bounds(wanted_bounds);
    }
}

/// Exact match against the start URL of `view`. Nothing else may load in that chrome webview.
fn is_start_url(view: View, uri: &str) -> bool {
    uri == view.start_url()
}

/// Whether a web message came from this host's own page: the sender's source and the message's
/// source must both be exactly its start URL.
fn is_trusted_message(view: View, sender_uri: &str, message_uri: &str) -> bool {
    is_start_url(view, sender_uri) && is_start_url(view, message_uri)
}

/// The URL check establishes origin; this check limits each page to its own command family.
fn view_accepts_command(view: View, command: &Command) -> bool {
    if matches!(command, Command::Ready) { return true; }
    match view {
        View::Toolbar => matches!(command, Command::DownloadsToggle { .. } | Command::Nav(_) | Command::Edit { .. } | Command::Submit(_) | Command::Escape | Command::Focus(_) | Command::Open(_) | Command::BeginDrag | Command::Caption(_) | Command::Tip { .. } | Command::TipOff | Command::SuggestPick { .. } | Command::SuggestPreview { .. } | Command::SuggestEnd),
        View::Tabs => matches!(command, Command::Tab { .. } | Command::TabMove { .. } | Command::Group { .. } | Command::Side(_) | Command::BeginDrag | Command::Caption(_) | Command::Tip { .. } | Command::TipOff | Command::Hover(_))
            // --- split view ---
            || matches!(command, Command::Split(SplitCommand::DragOver { .. } | SplitCommand::Drop { .. } | SplitCommand::DragEnd { .. })),
            // --- end split view ---
        View::Rail => matches!(command, Command::Rail { .. } | Command::RailMove { .. } | Command::Tip { .. } | Command::TipOff),
        View::Panel => matches!(command, Command::Panel { kind: PanelType::Panel | PanelType::History | PanelType::Bookmarks | PanelType::Customize, .. } | Command::ComposerOpen { .. }),
        View::Popup => matches!(command, Command::PopupSize { .. } | Command::PopupPick { .. } | Command::PopupDismiss { .. } | Command::PopupAct { .. } | Command::PopupCompose { .. } | Command::DownloadsAct { .. }),
        View::Dialog => matches!(command, Command::Panel { kind: PanelType::Panel, .. }),
        View::DialogNotes => matches!(command, Command::Panel { kind: PanelType::Panel | PanelType::Notes, .. }),
        View::DialogPrivacy => matches!(command, Command::Panel { kind: PanelType::Panel | PanelType::Privacy, .. }),
        View::DialogRename => matches!(command, Command::Panel { kind: PanelType::Panel | PanelType::Rename, .. }),
        View::Hover => matches!(command, Command::PopupSize { .. } | Command::HoverAct { .. } | Command::SuggestPick { .. }),
        // --- split view ---
        View::Split => matches!(command, Command::Split(SplitCommand::Pick { .. } | SplitCommand::Reopen { .. } | SplitCommand::Go(_) | SplitCommand::Close | SplitCommand::Focus | SplitCommand::ClosePrivate)),
        // --- end split view ---
        // No `panel` family (dismiss): the page is a tab, closed like one.
        // The Privacy dashboard is a section of the page, so its `priv` family reaches this host too.
        View::Settings => matches!(command, Command::Panel { kind: PanelType::Settings | PanelType::Privacy, .. }),
        // Likewise a tab: only its own family, never the rail panel's `hist` or `panel`.
        View::HistoryPage => matches!(command, Command::Panel { kind: PanelType::HistoryPage, .. }),
        View::DownloadsPage => matches!(command, Command::Panel { kind: PanelType::DownloadsPage, .. }),
    }
}

/// The only requests the chrome WebView2 raises `WebResourceRequested` for: its own scheme. A bare
/// `*` would hand every network request the chrome makes to the handler as well.
const UI_REQUEST_FILTER: &str = "bowser-ui://*";

/// The path of a `bowser-ui://app/...` URI, or `None` for any other scheme or host.
fn request_path(uri: &str) -> Option<String> {
    let url = Url::parse(uri).ok()?;
    if url.scheme() != SCHEME || url.host_str() != Some("app") {
        return None;
    }
    Some(url.path().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_consecutive_com_failures_trip_and_success_resets() {
        let mut failures = FailureCounter::default();
        assert!(!failures.record(false));
        assert!(!failures.record(false));
        assert!(!failures.record(true));
        assert!(!failures.record(false));
        assert!(!failures.record(false));
        assert!(failures.record(false));
        assert!(!failures.record(false));
    }

    #[test]
    fn loading_bounds_are_one_visible_pixel_and_never_match_real_bounds() {
        assert_eq!((LOADING_BOUNDS.right - LOADING_BOUNDS.left, LOADING_BOUNDS.bottom - LOADING_BOUNDS.top), (1, 1));
        assert!(!same_rect(LOADING_BOUNDS, RECT { left: 0, top: 0, right: 800, bottom: 40 }));
        assert!(same_rect(LOADING_BOUNDS, LOADING_BOUNDS));
    }

    #[test]
    fn the_request_filter_is_the_ui_scheme_only() {
        assert_eq!(UI_REQUEST_FILTER, format!("{SCHEME}://*"));
        assert_ne!(UI_REQUEST_FILTER, "*");
        // Everything the filter lets through is something `request_path` understands.
        assert!(request_path("bowser-ui://app/index.html").is_some());
        assert!(request_path("https://example.com/").is_none());
    }

    #[test]
    fn start_url_must_match_exactly() {
        assert!(is_start_url(View::Toolbar, START_URL));
        assert!(is_start_url(View::Tabs, TABS_START_URL));
        assert!(is_start_url(View::Toolbar, "bowser-ui://app/index.html?view=toolbar"));
        assert!(is_start_url(View::Tabs, "bowser-ui://app/index.html?view=tabs"));
        for uri in [
            "",
            " bowser-ui://app/index.html?view=toolbar",
            "bowser-ui://app/index.html",
            "bowser-ui://app/index.html?view=toolbar&x=1",
            "bowser-ui://app/index.html?view=toolbar#top",
            "bowser-ui://app/index.html?view=toolbar/",
            "bowser-ui://app/?view=toolbar",
            "bowser-ui://app/app.js?view=toolbar",
            "BOWSER-UI://app/index.html?view=toolbar",
            "bowser-ui://APP/index.html?view=toolbar",
            "bowser-ui://app/index.html?view=tabs",
            "https://example.com/",
        ] {
            assert!(
                !is_start_url(View::Toolbar, uri),
                "{uri:?} should not be the toolbar start URL"
            );
        }
        for uri in [
            "",
            "bowser-ui://app/index.html",
            "bowser-ui://app/index.html?view=tabs&x=1",
            "bowser-ui://app/index.html?view=Tabs",
            "bowser-ui://app/index.html?view=toolbar",
            "bowser-ui://app/?view=tabs",
            "https://example.com/",
        ] {
            assert!(
                !is_start_url(View::Tabs, uri),
                "{uri:?} should not be the tabs start URL"
            );
        }
    }

    #[test]
    fn web_messages_need_both_sources_to_be_the_host_page() {
        assert!(is_trusted_message(View::Toolbar, START_URL, START_URL));
        assert!(is_trusted_message(View::Tabs, TABS_START_URL, TABS_START_URL));
        for (view, sender, message) in [
            (View::Toolbar, START_URL, "https://example.com/"),
            (View::Toolbar, "https://example.com/", START_URL),
            (View::Toolbar, TABS_START_URL, START_URL),
            (View::Tabs, TABS_START_URL, START_URL),
            (View::Toolbar, "", ""),
        ] {
            assert!(!is_trusted_message(view, sender, message), "{view:?} {sender:?} {message:?}");
        }
    }

    #[test]
    fn chrome_profile_name_is_a_valid_webview2_profile_name() {
        assert!(valid_profile_name(CHROME_PROFILE));
        assert_eq!(CHROME_PROFILE, "BowserChrome");
        for name in ["a", "Default_2", "x-y", &"a".repeat(64)] {
            assert!(valid_profile_name(name), "{name:?}");
        }
        for name in [
            "",
            &"a".repeat(65),
            "Bowser Chrome",
            "Bowser.Chrome",
            "..",
            "a/b",
            "a\\b",
            "Bowsér",
            "name\0",
        ] {
            assert!(!valid_profile_name(name), "{name:?}");
        }
    }

    #[test]
    fn only_restartable_shared_process_failures_keep_the_chrome() {
        for kind in [
            COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_UTILITY_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_SANDBOX_HELPER_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_PPAPI_PLUGIN_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_PPAPI_BROKER_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_UNKNOWN_PROCESS_EXITED,
        ] {
            assert!(!process_failure_is_fatal(Some(kind)), "{kind:?}");
        }
        for kind in [
            Some(COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED),
            Some(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED),
            Some(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE),
            Some(COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED),
            Some(COREWEBVIEW2_PROCESS_FAILED_KIND(99)),
            None,
        ] {
            assert!(process_failure_is_fatal(kind), "{kind:?}");
        }
    }

    #[test]
    fn each_view_has_its_own_start_url() {
        assert_eq!(View::Toolbar.start_url(), "bowser-ui://app/index.html?view=toolbar");
        assert_eq!(View::Tabs.start_url(), "bowser-ui://app/index.html?view=tabs");
        assert_eq!(View::Rail.start_url(), "bowser-ui://app/index.html?view=rail");
        assert_eq!(View::Panel.start_url(), "bowser-ui://app/index.html?view=panel");
        assert_eq!(View::Popup.start_url(), "bowser-ui://app/index.html?view=popup");
        assert_eq!(View::Dialog.start_url(), "bowser-ui://app/index.html?view=dialog");
        assert_eq!(View::DialogNotes.start_url(), "bowser-ui://app/index.html?view=dialog&p=notes");
        assert_eq!(View::Settings.start_url(), "bowser-ui://app/index.html?view=settings");
        assert_eq!(View::DialogPrivacy.start_url(), "bowser-ui://app/index.html?view=dialog&p=privacy");
        assert_eq!(View::DialogRename.start_url(), "bowser-ui://app/index.html?view=dialog&p=rename");
        assert_eq!(View::Hover.start_url(), "bowser-ui://app/index.html?view=hover");
        let views = [View::Toolbar, View::Tabs, View::Rail, View::Panel, View::Popup, View::Dialog, View::DialogNotes, View::Settings, View::DialogPrivacy, View::DialogRename, View::Hover, View::HistoryPage, View::DownloadsPage];
        for view in views {
            assert!(is_start_url(view, view.start_url()), "{view:?}");
            for other in views.iter().filter(|other| **other != view) {
                assert!(!is_start_url(view, other.start_url()), "{view:?} accepted {other:?}");
            }
        }
    }

    #[test]
    fn the_settings_page_is_trusted_only_from_its_exact_url_and_only_for_settings_commands() {
        let url = View::Settings.start_url();
        assert!(is_trusted_message(View::Settings, url, url));
        for other in [
            "bowser-ui://app/index.html?view=settings#privacy",
            "bowser-ui://app/index.html?view=settings&p=x",
            "bowser-ui://app/index.html?view=dialog&p=settings",
            "bowser-ui://APP/index.html?view=settings",
            "bowser-ui://app/index.html?view=Settings",
            "https://evil.example/index.html?view=settings",
            "bowser://settings",
            START_URL,
            "",
        ] {
            assert!(!is_start_url(View::Settings, other), "{other}");
            assert!(!is_trusted_message(View::Settings, other, url), "sender {other}");
            assert!(!is_trusted_message(View::Settings, url, other), "message {other}");
        }
        let save = parse_command(r#"{"v":1,"t":"set","a":"save","text":"x"}"#).unwrap();
        let section = parse_command(r#"{"v":1,"t":"set","a":"section","text":"privacy"}"#).unwrap();
        for command in [&save, &section] {
            assert!(view_accepts_command(View::Settings, command));
            for view in [View::Toolbar, View::Tabs, View::Rail, View::Panel, View::Popup, View::Dialog, View::DialogNotes, View::DialogPrivacy, View::DialogRename, View::Hover, View::Split] {
                assert!(!view_accepts_command(view, command), "{view:?}");
            }
        }
        for foreign in [
            r#"{"v":1,"t":"panel","a":"dismiss"}"#,
            r#"{"v":1,"t":"hist","a":"open","id":1}"#,
            r#"{"v":1,"t":"notes","a":"add-note"}"#,
            r#"{"v":1,"t":"nav","a":"back"}"#,
            r#"{"v":1,"t":"open","a":"menu"}"#,
            r#"{"v":1,"t":"pick","tok":1,"id":2}"#,
        ] {
            let command = parse_command(foreign).unwrap();
            assert!(!view_accepts_command(View::Settings, &command), "{foreign}");
        }
        // The Privacy dashboard is a section of this page, so its own family is allowed here.
        let privacy = parse_command(r#"{"v":1,"t":"priv","a":"clear-stats"}"#).unwrap();
        assert!(view_accepts_command(View::Settings, &privacy));
        assert!(view_accepts_command(View::Settings, &Command::Ready));
    }

    // --- history page (docs/design/history-page.md) ---
    #[test]
    fn the_history_page_is_trusted_only_from_its_exact_url_and_only_for_its_own_commands() {
        let url = View::HistoryPage.start_url();
        assert_eq!(url, "bowser-ui://app/index.html?view=history");
        assert!(is_trusted_message(View::HistoryPage, url, url));
        for other in [
            "bowser-ui://app/index.html?view=history#today",
            "bowser-ui://app/index.html?view=history&q=x",
            "bowser-ui://app/index.html?view=History",
            "bowser-ui://APP/index.html?view=history",
            "bowser-ui://app/index.html?view=panel",
            "https://evil.example/index.html?view=history",
            "bowser://history",
            View::Settings.start_url(),
            View::Panel.start_url(),
            START_URL,
            "",
        ] {
            assert!(!is_start_url(View::HistoryPage, other), "{other}");
            assert!(!is_trusted_message(View::HistoryPage, other, url), "sender {other}");
            assert!(!is_trusted_message(View::HistoryPage, url, other), "message {other}");
        }
        let all = [View::Toolbar, View::Tabs, View::Rail, View::Panel, View::Popup, View::Dialog, View::DialogNotes, View::Settings, View::DialogPrivacy, View::DialogRename, View::Hover, View::Split];
        for view in all {
            assert!(!is_start_url(view, url), "{view:?} accepted the history page URL");
        }
        let open = parse_command(r#"{"v":1,"t":"hpage","a":"open","id":7}"#).unwrap();
        let search = parse_command(r#"{"v":1,"t":"hpage","a":"search","text":"rust"}"#).unwrap();
        let remove = parse_command(r#"{"v":1,"t":"hpage","a":"remove","id":7}"#).unwrap();
        for command in [&open, &search, &remove] {
            assert!(view_accepts_command(View::HistoryPage, command));
            for view in all {
                assert!(!view_accepts_command(view, command), "{view:?}");
            }
        }
        for foreign in [
            r#"{"v":1,"t":"hist","a":"open","id":1}"#,
            r#"{"v":1,"t":"hist","a":"clear","n":2}"#,
            r#"{"v":1,"t":"panel","a":"dismiss"}"#,
            r#"{"v":1,"t":"set","a":"history","on":true}"#,
            r#"{"v":1,"t":"set","a":"privacy"}"#,
            r#"{"v":1,"t":"priv","a":"clear-stats"}"#,
            r#"{"v":1,"t":"bm","a":"open","id":1}"#,
            r#"{"v":1,"t":"nav","a":"back"}"#,
            r#"{"v":1,"t":"submit","text":"https://example.com/"}"#,
            r#"{"v":1,"t":"tab","a":"close","id":1}"#,
            r#"{"v":1,"t":"pick","tok":1,"id":2}"#,
        ] {
            let command = parse_command(foreign).unwrap();
            assert!(!view_accepts_command(View::HistoryPage, &command), "{foreign}");
        }
        assert!(view_accepts_command(View::HistoryPage, &Command::Ready));
    }

    #[test]
    fn history_page_commands_parse_strictly_and_share_the_bridge_rate_limit() {
        for bad in [
            r#"{"v":1,"t":"hpage"}"#,
            r#"{"v":1,"t":"hpage","a":"Open","id":1}"#,
            r#"{"v":1,"t":"hpage","a":"open","id":-1}"#,
            r#"{"v":1,"t":"hpage","a":"open","id":"1"}"#,
            r#"{"v":1,"t":"hpage","a":"open","id":1,"extra":1}"#,
            r#"{"v":1,"t":"hpage","a":"search","text":5}"#,
            r#"{"v":2,"t":"hpage","a":"refresh"}"#,
            r#"{"v":1,"t":"HPAGE","a":"refresh"}"#,
        ] {
            assert!(parse_command(bad).is_err(), "{bad}");
        }
        // One bucket per host: a page flooding its bridge is cut off after the burst.
        let mut bucket = TokenBucket::new(MESSAGE_BURST, MESSAGE_RATE_PER_SEC);
        let now = Instant::now();
        let passed = (0..MESSAGE_BURST * 2).filter(|_| bucket.take(now)).count();
        assert_eq!(passed, MESSAGE_BURST as usize);
    }
    // --- end history page ---

    // --- downloads (docs/design/downloads.md) ---
    #[test]
    fn the_downloads_page_is_trusted_only_from_its_exact_url_and_only_for_its_own_commands() {
        let url = View::DownloadsPage.start_url();
        assert_eq!(url, "bowser-ui://app/index.html?view=downloads");
        assert!(is_trusted_message(View::DownloadsPage, url, url));
        for other in [
            "bowser-ui://app/index.html?view=downloads#today",
            "bowser-ui://app/index.html?view=downloads&q=x",
            "bowser-ui://app/index.html?view=Downloads",
            "bowser-ui://APP/index.html?view=downloads",
            "bowser-ui://app/index.html?view=history",
            "https://evil.example/index.html?view=downloads",
            "bowser://downloads",
            View::HistoryPage.start_url(),
            View::Popup.start_url(),
            START_URL,
            "",
        ] {
            assert!(!is_start_url(View::DownloadsPage, other), "{other}");
            assert!(!is_trusted_message(View::DownloadsPage, other, url), "sender {other}");
            assert!(!is_trusted_message(View::DownloadsPage, url, other), "message {other}");
        }
        let all = [View::Toolbar, View::Tabs, View::Rail, View::Panel, View::Popup, View::Dialog, View::DialogNotes, View::Settings, View::DialogPrivacy, View::DialogRename, View::Hover, View::Split, View::HistoryPage];
        for view in all {
            assert!(!is_start_url(view, url), "{view:?} accepted the downloads page URL");
        }
        // The page speaks `dpage` and nothing else; no other host accepts `dpage`.
        for raw in [
            r#"{"v":1,"t":"dpage","a":"open","id":7}"#,
            r#"{"v":1,"t":"dpage","a":"search","text":"zip"}"#,
            r#"{"v":1,"t":"dpage","a":"clear"}"#,
        ] {
            let command = parse_command(raw).unwrap();
            assert!(view_accepts_command(View::DownloadsPage, &command), "{raw}");
            for view in all {
                assert!(!view_accepts_command(view, &command), "{view:?} {raw}");
            }
        }
        for foreign in [
            r#"{"v":1,"t":"hpage","a":"open","id":1}"#,
            r#"{"v":1,"t":"hist","a":"open","id":1}"#,
            r#"{"v":1,"t":"panel","a":"dismiss"}"#,
            r#"{"v":1,"t":"set","a":"downloadask","on":true}"#,
            r#"{"v":1,"t":"dlact","tok":1,"id":1,"n":0}"#,
            r#"{"v":1,"t":"dl","a":"toggle","text":"1,2,3,4"}"#,
            r#"{"v":1,"t":"submit","text":"https://example.com/"}"#,
            r#"{"v":1,"t":"pick","tok":1,"id":2}"#,
        ] {
            let command = parse_command(foreign).unwrap();
            assert!(!view_accepts_command(View::DownloadsPage, &command), "{foreign}");
        }
        assert!(view_accepts_command(View::DownloadsPage, &Command::Ready));
    }

    #[test]
    fn the_download_button_and_bubble_commands_reach_only_their_own_hosts() {
        let toggle = parse_command(r#"{"v":1,"t":"dl","a":"toggle","text":"640,3,28,28"}"#).unwrap();
        let act = parse_command(r#"{"v":1,"t":"dlact","tok":3,"id":9,"n":2}"#).unwrap();
        let others = [View::Tabs, View::Rail, View::Panel, View::Popup, View::Dialog, View::DialogNotes, View::Settings, View::DialogPrivacy, View::DialogRename, View::Hover, View::Split, View::HistoryPage, View::DownloadsPage];
        assert!(view_accepts_command(View::Toolbar, &toggle));
        assert!(!view_accepts_command(View::Toolbar, &act));
        assert!(view_accepts_command(View::Popup, &act));
        assert!(!view_accepts_command(View::Popup, &toggle));
        for view in others {
            assert!(!view_accepts_command(view, &toggle), "{view:?}");
            assert!(view == View::Popup || !view_accepts_command(view, &act), "{view:?}");
        }
        // The toggle shares the popup-command gate (one per 250 ms); the bubble's actions do not.
        let mut gate = Throttle::new(POPUP_COMMAND_INTERVAL);
        let now = Instant::now();
        assert!(is_popup_command(&toggle) && gate.allow(now));
        assert!(!gate.allow(now));
        assert!(!is_popup_command(&act));
    }
    // --- end downloads ---

    // --- omnibox suggest ---
    #[test]
    fn suggest_commands_come_only_from_the_toolbar_and_the_hover_layer() {
        let pick = parse_command(r#"{"v":1,"t":"suggestPick","seq":3,"i":1}"#).unwrap();
        let preview = parse_command(r#"{"v":1,"t":"suggestPreview","seq":3,"i":1}"#).unwrap();
        let end = parse_command(r#"{"v":1,"t":"suggestEnd"}"#).unwrap();
        for command in [&pick, &preview, &end] {
            assert!(view_accepts_command(View::Toolbar, command));
            for view in [View::Tabs, View::Rail, View::Panel, View::Popup, View::Dialog] {
                assert!(!view_accepts_command(view, command), "{view:?}");
            }
        }
        assert!(view_accepts_command(View::Hover, &pick));
        assert!(!view_accepts_command(View::Hover, &preview));
        assert!(!view_accepts_command(View::Hover, &end));
    }
    // --- end omnibox suggest ---

    #[test]
    fn new_views_reject_nearby_urls_and_foreign_command_families() {
        for view in [View::Panel, View::Popup, View::Dialog, View::DialogNotes, View::Settings, View::DialogPrivacy, View::DialogRename, View::HistoryPage, View::DownloadsPage] {
            let url = view.start_url();
            assert!(is_trusted_message(view, url, url));
            assert!(!is_start_url(view, &format!("{url}&x=1")));
            assert!(!is_start_url(view, &format!("{url}#top")));
            assert!(!is_trusted_message(view, START_URL, url));
            assert!(!is_trusted_message(view, url, START_URL));
        }
        // The vertical sidebar (the Tabs view) sends workspace, new tab and collapse; no other view may.
        for action in ["workspace", "newtab", "collapse"] {
            let side = parse_command(&format!(r#"{{"v":1,"t":"side","a":"{action}"}}"#)).unwrap();
            assert!(view_accepts_command(View::Tabs, &side), "{action}");
            for view in [View::Toolbar, View::Rail, View::Panel, View::Popup, View::Dialog] {
                assert!(!view_accepts_command(view, &side), "{view:?} {action}");
            }
        }
        let panel = parse_command(r#"{"v":1,"t":"hist","a":"open","id":1}"#).unwrap();
        let popup = parse_command(r#"{"v":1,"t":"pick","tok":1,"id":2}"#).unwrap();
        for view in [View::Toolbar, View::Tabs, View::Rail] {
            assert!(!view_accepts_command(view, &panel));
            assert!(!view_accepts_command(view, &popup));
        }
        assert!(view_accepts_command(View::Panel, &panel));
        assert!(!view_accepts_command(View::Dialog, &panel));
        let dialog = parse_command(r#"{"v":1,"t":"set","a":"open"}"#).unwrap();
        assert!(view_accepts_command(View::Settings, &dialog));
        assert!(!view_accepts_command(View::DialogNotes, &dialog));
        assert!(!view_accepts_command(View::Panel, &dialog));
        let notes = parse_command(r#"{"v":1,"t":"notes","a":"add-note"}"#).unwrap();
        assert!(view_accepts_command(View::DialogNotes, &notes));
        assert!(!view_accepts_command(View::DialogPrivacy, &notes));
        let privacy = parse_command(r#"{"v":1,"t":"priv","a":"clear-stats"}"#).unwrap();
        assert!(view_accepts_command(View::DialogPrivacy, &privacy));
        assert!(view_accepts_command(View::Settings, &privacy));
        assert!(!view_accepts_command(View::DialogNotes, &privacy));
        assert!(!view_accepts_command(View::Popup, &panel));
        assert!(view_accepts_command(View::Popup, &popup));
        assert!(!view_accepts_command(View::Panel, &popup));
        // Popup page actions (site info toggles) reach only the popup host; the rename dialog's
        // family reaches only the rename dialog.
        let act = parse_command(r#"{"v":1,"t":"act","tok":1,"id":2,"n":0}"#).unwrap();
        assert!(view_accepts_command(View::Popup, &act));
        for view in [View::Toolbar, View::Tabs, View::Rail, View::Panel, View::Dialog, View::DialogRename] {
            assert!(!view_accepts_command(view, &act), "{view:?}");
        }
        // The composer's submit reaches only the popup host; the Customize flyout alone may open one.
        let compose = parse_command(r#"{"v":1,"t":"compose","tok":1,"name":"Work"}"#).unwrap();
        assert!(view_accepts_command(View::Popup, &compose));
        let open_composer = parse_command(r#"{"v":1,"t":"composeopen","a":"app-add","r":"1,2,3,4"}"#).unwrap();
        assert!(view_accepts_command(View::Panel, &open_composer));
        for view in [View::Toolbar, View::Tabs, View::Rail, View::Dialog, View::DialogRename, View::Hover] {
            assert!(!view_accepts_command(view, &compose), "{view:?}");
            assert!(!view_accepts_command(view, &open_composer), "{view:?}");
        }
        assert!(!view_accepts_command(View::Panel, &compose));
        assert!(!view_accepts_command(View::Popup, &open_composer));
        let rename = parse_command(r#"{"v":1,"t":"ren","a":"save","text":"Work"}"#).unwrap();
        assert!(view_accepts_command(View::DialogRename, &rename));
        for view in [View::Popup, View::Panel, View::Dialog, View::DialogNotes, View::Settings, View::DialogPrivacy] {
            assert!(!view_accepts_command(view, &rename), "{view:?}");
        }
        let dismiss = parse_command(r#"{"v":1,"t":"panel","a":"dismiss"}"#).unwrap();
        assert!(view_accepts_command(View::DialogRename, &dismiss));
    }

    #[test]
    fn hover_commands_reach_only_their_views() {
        let tip = parse_command(r#"{"v":1,"t":"tip","text":"Back","r":"0,0,28,28"}"#).unwrap();
        let off = parse_command(r#"{"v":1,"t":"tipoff"}"#).unwrap();
        let hover = parse_command(r#"{"v":1,"t":"hover","k":"tab","id":1,"r":"0,0,9,9"}"#).unwrap();
        let act = parse_command(r#"{"v":1,"t":"hact","tok":1,"a":"next"}"#).unwrap();
        let size = parse_command(r#"{"v":1,"t":"size","tok":1,"w":10,"h":10}"#).unwrap();
        for view in [View::Toolbar, View::Tabs, View::Rail] {
            assert!(view_accepts_command(view, &tip), "{view:?}");
            assert!(view_accepts_command(view, &off), "{view:?}");
            assert!(!view_accepts_command(view, &act), "{view:?}");
        }
        assert!(view_accepts_command(View::Tabs, &hover));
        assert!(!view_accepts_command(View::Toolbar, &hover));
        assert!(!view_accepts_command(View::Rail, &hover));
        assert!(view_accepts_command(View::Hover, &act));
        assert!(view_accepts_command(View::Hover, &size));
        for command in [&tip, &off, &hover] {
            assert!(!view_accepts_command(View::Hover, command));
        }
        for view in [View::Panel, View::Popup, View::Dialog, View::DialogNotes] {
            assert!(!view_accepts_command(view, &act), "{view:?}");
            assert!(!view_accepts_command(view, &tip), "{view:?}");
        }
        assert!(is_trusted_message(View::Hover, HOVER_START_URL, HOVER_START_URL));
        assert!(!is_trusted_message(View::Hover, POPUP_START_URL, HOVER_START_URL));
        assert!(!is_start_url(View::Hover, "bowser-ui://app/index.html?view=hover&x=1"));
    }

    #[test]
    fn rail_start_url_matches_exactly_and_nothing_else() {
        assert!(is_start_url(View::Rail, RAIL_START_URL));
        for uri in [
            "bowser-ui://app/index.html?view=rail&x=1",
            "bowser-ui://app/index.html?view=Rail",
            "bowser-ui://app/index.html?view=rail ",
            "bowser-ui://app/index.html?view=rails",
            "bowser-ui://app/index.html?view=",
            "bowser-ui://app/index.html",
            "bowser-ui://app/?view=rail",
            "https://app/index.html?view=rail",
        ] {
            assert!(!is_start_url(View::Rail, uri), "{uri:?}");
        }
    }

    #[test]
    fn rail_messages_need_the_rail_page_as_both_sources() {
        assert!(is_trusted_message(View::Rail, RAIL_START_URL, RAIL_START_URL));
        assert!(!is_trusted_message(View::Rail, RAIL_START_URL, TABS_START_URL));
        assert!(!is_trusted_message(View::Rail, TABS_START_URL, RAIL_START_URL));
        assert!(!is_trusted_message(View::Toolbar, RAIL_START_URL, RAIL_START_URL));
    }

    #[test]
    fn request_path_reads_app_host_only() {
        assert_eq!(
            request_path("bowser-ui://app/index.html").as_deref(),
            Some("/index.html")
        );
        assert_eq!(request_path("bowser-ui://app/").as_deref(), Some("/"));
        assert_eq!(
            request_path("bowser-ui://app/app.css?v=3#x").as_deref(),
            Some("/app.css")
        );
        for uri in [
            "",
            "not a url",
            "bowser-ui:index.html",
            "bowser-ui://other/index.html",
            "bowser-ui://app.example/index.html",
            "https://app/index.html",
            "http://example.com/index.html",
            "https://example.com/",
            "file:///C:/Windows/win.ini",
            "about:blank",
            "data:text/html,hi",
        ] {
            assert_eq!(request_path(uri), None, "{uri:?}");
        }
    }

    #[test]
    fn encoded_traversal_is_not_an_asset() {
        let path = request_path("bowser-ui://app/..%2Fapp.js").expect("app host");
        assert!(assets::lookup(&path).is_none(), "{path:?}");
    }

    fn key(vk: u32, ctrl: bool, shift: bool, alt: bool) -> Accel {
        Accel {
            vk,
            ctrl,
            shift,
            alt,
            key_down: true,
            is_system: alt,
            repeat: false,
        }
    }

    #[test]
    fn accelerator_maps_chords_through_the_shared_table() {
        assert_eq!(
            accel_command(key(0x54, true, false, false), false),
            Some(Shortcut::NewTab)
        );
        assert_eq!(
            accel_command(key(0x74, false, false, false), false),
            Some(Shortcut::Reload)
        );
        assert_eq!(
            accel_command(key(0x52, true, false, false), false),
            Some(Shortcut::Reload)
        );
        assert_eq!(
            accel_command(key(0x74, true, false, false), false),
            Some(Shortcut::HardReload)
        );
        assert_eq!(
            accel_command(key(0x25, false, false, true), false),
            Some(Shortcut::Back)
        );
        assert_eq!(
            accel_command(key(0x44, true, false, false), false),
            Some(Shortcut::AddBookmark)
        );
        assert_eq!(
            accel_command(key(0x4B, true, true, false), false),
            Some(Shortcut::ShowCommandPalette)
        );
        assert_eq!(
            accel_command(key(0x09, true, false, false), false),
            Some(Shortcut::NextTab)
        );
        assert_eq!(
            accel_command(key(0x39, true, false, false), false),
            Some(Shortcut::SelectLastTab)
        );
    }

    #[test]
    fn accelerator_modifiers_must_match_exactly() {
        // Ctrl+Alt (AltGr) and extra Shift never run a command.
        assert_eq!(accel_command(key(0x54, true, false, true), false), None);
        assert_eq!(accel_command(key(0x54, true, true, false), false), None);
        assert_eq!(accel_command(key(0x74, false, true, false), false), None);
        // A bare letter is never a shortcut.
        assert_eq!(accel_command(key(0x54, false, false, false), false), None);
    }

    #[test]
    fn ctrl_alt_r_toggles_reader_view_and_does_not_repeat() {
        let mut chord = key(0x52, true, false, true);
        assert_eq!(accel_command(chord, false), Some(Shortcut::ToggleReader));
        // Reader view stays available from the omnibox: it is not an editing chord.
        assert_eq!(accel_command(chord, true), Some(Shortcut::ToggleReader));
        chord.repeat = true;
        assert_eq!(accel_command(chord, false), None, "a held chord must not flip it again");
        // Neighbours stay as they were.
        assert_eq!(accel_command(key(0x52, true, false, false), false), Some(Shortcut::Reload));
        assert_eq!(accel_command(key(0x52, false, false, true), false), None);
    }

    #[test]
    fn accelerator_ignores_key_up_and_escape() {
        let mut up = key(0x54, true, false, false);
        up.key_down = false;
        assert_eq!(accel_command(up, false), None);
        assert_eq!(accel_command(key(0x1B, false, false, false), false), None);
        assert_eq!(accel_command(key(0x1B, false, false, false), true), None);
    }

    #[test]
    fn accelerator_repeat_does_not_flip_toggles() {
        let mut toggle = key(0x42, true, true, false); // Ctrl+Shift+B, blocker toggle.
        assert_eq!(accel_command(toggle, false), Some(Shortcut::ToggleBlocker));
        toggle.repeat = true;
        assert_eq!(accel_command(toggle, false), None);
        // Non-toggles still repeat, as in the tab WebViews.
        let mut next = key(0x09, true, false, false);
        next.repeat = true;
        assert_eq!(accel_command(next, false), Some(Shortcut::NextTab));
    }

    #[test]
    fn accelerator_claims_gated_chords_only_when_bound() {
        let copy = key(0x43, true, true, false); // Ctrl+Shift+C
        let save = key(0x53, true, false, false); // Ctrl+S
        let all = shortcuts::GatedChords::new(true, true, true);
        assert_eq!(accel_command(copy, false), None);
        assert_eq!(accel_command(save, false), None);
        assert_eq!(accel_command_gated(copy, false, all), Some(Shortcut::CopyLink));
        assert_eq!(accel_command_gated(save, false, all), Some(Shortcut::ToggleSidebar));
        // The omnibox field keeps Ctrl+C-style editing: Ctrl+Shift+C is claimed by neither while editing.
        assert_eq!(accel_command_gated(copy, true, all), None);
        // Holding Ctrl+S does not flip the sidebar again.
        let mut held = save;
        held.repeat = true;
        assert_eq!(accel_command_gated(held, false, all), None);
    }

    #[test]
    fn omnibox_editing_keeps_editing_chords() {
        for accel in [
            key(0x41, true, false, false), // Ctrl+A
            key(0x43, true, false, false), // Ctrl+C
            key(0x56, true, false, false), // Ctrl+V
            key(0x58, true, false, false), // Ctrl+X
            key(0x5A, true, false, false), // Ctrl+Z
            key(0x59, true, false, false), // Ctrl+Y
            key(0x5A, true, true, false),  // Ctrl+Shift+Z (the field's redo)
            key(0x25, true, false, false), // Ctrl+Left
            key(0x27, true, true, false),  // Ctrl+Shift+Right
            key(0x24, false, true, false), // Shift+Home
            key(0x23, true, true, false),  // Ctrl+Shift+End
            key(0x08, true, false, false), // Ctrl+Backspace
            key(0x2E, false, true, false), // Shift+Delete
            key(0x25, false, true, false), // Shift+Left
        ] {
            assert_eq!(accel_command(accel, true), None, "{accel:?}");
        }
    }

    #[test]
    fn omnibox_editing_does_not_swallow_ctrl_shift_v_or_ctrl_shift_delete() {
        assert_eq!(accel_command(key(0x56, true, true, false), true), None);
        assert_eq!(
            accel_command(key(0x2E, true, true, false), true),
            Some(Shortcut::OpenPrivacyDashboard)
        );
        assert_eq!(
            accel_command(key(0xDC, true, true, false), true),
            Some(Shortcut::ToggleSplit)
        );
    }

    #[test]
    fn omnibox_editing_still_runs_other_shortcuts() {
        // Alt+Left/Right navigate, and the field ignores them.
        assert_eq!(
            accel_command(key(0x25, false, false, true), true),
            Some(Shortcut::Back)
        );
        assert_eq!(
            accel_command(key(0x27, false, false, true), true),
            Some(Shortcut::Forward)
        );
        for (vk, ctrl, shift, alt, expected) in [
            (0x54, true, false, false, Shortcut::NewTab),
            (0x57, true, false, false, Shortcut::CloseTab),
            (0x09, true, false, false, Shortcut::NextTab),
            (0x09, true, true, false, Shortcut::PrevTab),
            (0x31, true, false, false, Shortcut::SelectTab(0)),
            (0x4C, true, false, false, Shortcut::FocusOmnibox),
            (0x44, false, false, true, Shortcut::FocusOmnibox),
            (0x75, false, false, false, Shortcut::FocusOmnibox),
            (0x74, false, false, false, Shortcut::Reload),
            (0x74, true, false, false, Shortcut::HardReload),
            (0x44, true, false, false, Shortcut::AddBookmark),
            (0x4C, true, true, false, Shortcut::FillPassword),
            (0x4B, true, true, false, Shortcut::ShowCommandPalette),
            (0x45, true, true, false, Shortcut::ShowExtensionsMenu),
        ] {
            assert_eq!(
                accel_command(key(vk, ctrl, shift, alt), true),
                Some(expected),
                "vk {vk:#04X} ctrl={ctrl} shift={shift} alt={alt}"
            );
        }
    }

    #[test]
    fn unknown_app_paths_are_not_assets() {
        let path = request_path("bowser-ui://app/missing.html").expect("app host");
        assert_eq!(path, "/missing.html");
        assert!(assets::lookup(&path).is_none());
        let page = request_path(START_URL).and_then(|path| assets::lookup(&path));
        assert!(page.is_some());
    }
}
