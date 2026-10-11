use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet, VecDeque},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use url::Url;
use zeroize::{Zeroize, Zeroizing};
use webview2_com::{
    AcceleratorKeyPressedEventHandler, AddScriptToExecuteOnDocumentCreatedCompletedHandler,
    BrowserExtensionEnableCompletedHandler,
    BrowserExtensionRemoveCompletedHandler, CoTaskMemPWSTR, ContentLoadingEventHandler,
    CoreWebView2EnvironmentOptions, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, DOMContentLoadedEventHandler,
    DocumentTitleChangedEventHandler, DownloadStartingEventHandler, ExecuteScriptCompletedHandler,
    FaviconChangedEventHandler, FocusChangedEventHandler, GetCookiesCompletedHandler, GetFaviconCompletedHandler,
    Microsoft::Web::WebView2::Win32::*,
    NavigationCompletedEventHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, PermissionRequestedEventHandler, ProcessFailedEventHandler,
    ProfileAddBrowserExtensionCompletedHandler, ProfileGetBrowserExtensionsCompletedHandler,
    SourceChangedEventHandler, TrySuspendCompletedHandler, WebMessageReceivedEventHandler,
    WebResourceRequestedEventHandler,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT, WPARAM},
        System::Com::IStream,
        UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_ESCAPE, VK_MENU, VK_SHIFT},
    },
    core::{BOOL, Error, HSTRING, Interface, PCWSTR, PWSTR, Ref, Result, implement, w},
};

use super::{
    EngineAdapter, EngineEvents, MemoryTargetApply, MemoryUsageTarget, PermissionsHandle,
    TrackingPreventionLevel,
};
use crate::permissions::Permission;
use crate::tabs::TabId;
use crate::{shortcuts, win32};

#[derive(Debug)]
pub struct RuntimeUnavailable(pub String);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PopupDecision {
    NewTab,
    Block,
}

pub fn popup_decision(uri: &Url, user_initiated: bool) -> PopupDecision {
    if user_initiated && matches!(uri.scheme(), "http" | "https") {
        PopupDecision::NewTab
    } else {
        PopupDecision::Block
    }
}

/// A browser shortcut pressed while a page has focus. The WebView2 accelerator is consumed and the chord is
/// re-posted to the main window as `WM_SHORTCUT_KEY`, which runs the same command as the omnibox path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ShortcutForward {
    key: u32,
    ctrl: bool,
    shift: bool,
    alt: bool,
}

impl ShortcutForward {
    /// Packs the modifiers for the `WM_SHORTCUT_KEY` LPARAM: ctrl=1, shift=2, alt=4.
    fn modifier_bits(self) -> isize {
        (self.ctrl as isize) | ((self.shift as isize) << 1) | ((self.alt as isize) << 2)
    }
}

/// Only mapped chords are returned, so typing, clipboard and other page keys always reach the page.
/// Escape is excluded: it stops a load only while the tab is loading, which this handler cannot see,
/// so it is left to the page.
fn shortcut_forward(
    key: u32,
    kind: COREWEBVIEW2_KEY_EVENT_KIND,
    ctrl: bool,
    shift: bool,
    alt: bool,
) -> Option<ShortcutForward> {
    if !is_key_down(kind) || key == u32::from(VK_ESCAPE.0) {
        return None;
    }
    shortcuts::command_for(key, ctrl, shift, alt)?;
    Some(ShortcutForward {
        key,
        ctrl,
        shift,
        alt,
    })
}

fn is_key_down(kind: COREWEBVIEW2_KEY_EVENT_KIND) -> bool {
    matches!(
        kind,
        COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN | COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
    )
}

type ExternalSchemeCallback = Box<
    dyn FnMut(
        Option<ICoreWebView2>,
        Option<ICoreWebView2LaunchingExternalUriSchemeEventArgs>,
    ) -> Result<()>,
>;

#[implement(ICoreWebView2LaunchingExternalUriSchemeEventHandler)]
struct ExternalSchemeHandler(RefCell<ExternalSchemeCallback>);

impl ICoreWebView2LaunchingExternalUriSchemeEventHandler_Impl for ExternalSchemeHandler_Impl {
    fn Invoke(
        &self,
        sender: Ref<'_, ICoreWebView2>,
        args: Ref<'_, ICoreWebView2LaunchingExternalUriSchemeEventArgs>,
    ) -> Result<()> {
        (self.0.borrow_mut())(sender.cloned(), args.cloned())
    }
}

fn navigation_scheme_allowed(uri: &Url) -> bool {
    matches!(uri.scheme(), "http" | "https")
}

fn frame_navigation_scheme_allowed(uri: &Url) -> bool {
    matches!(uri.scheme(), "http" | "https" | "blob" | "data")
        || (uri.scheme() == "about" && matches!(uri.path(), "blank" | "srcdoc"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NavigationStartPolicy {
    Allow,
    Block,
    ExternalProtocol,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControllerSecuritySetting {
    WebMessages,
    HostObjects,
}

fn apply_controller_security_policy<E>(
    mut disable: impl FnMut(ControllerSecuritySetting) -> std::result::Result<(), E>,
) -> std::result::Result<(), E> {
    for setting in [
        ControllerSecuritySetting::WebMessages,
        ControllerSecuritySetting::HostObjects,
    ] {
        disable(setting)?;
    }
    Ok(())
}

fn password_autosave_disabled(enabled: bool) -> bool {
    !enabled
}

fn navigation_start_policy(uri: &Url, allowed_file: Option<&Url>) -> NavigationStartPolicy {
    match uri.scheme() {
        "http" | "https" => NavigationStartPolicy::Allow,
        "file" if allowed_file == Some(uri) => NavigationStartPolicy::Allow,
        "file"
        | "about"
        | "blob"
        | "chrome"
        | "data"
        | "devtools"
        | "edge"
        | "javascript"
        | "ms-appx"
        | "ms-appx-web"
        | "ms-browser-extension"
        | "res"
        | "vbscript"
        | "view-source" => NavigationStartPolicy::Block,
        _ => NavigationStartPolicy::ExternalProtocol,
    }
}

fn consume_allowed_file_navigation(
    uri: &Url,
    allowed_file: Option<&Url>,
    pending: &Cell<bool>,
) -> bool {
    allowed_file == Some(uri) && pending.replace(false)
}

fn web_url_from_source(source: &str) -> Option<Url> {
    let url = Url::parse(source).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}

/// Bounded, page-side detection only: no credential data is exposed from native code to JS.
const LOGIN_DETECTION_SCRIPT: &str = r#"(()=>{if(window.top!==window||!window.chrome?.webview)return;let sent=false;const visible=e=>e&&e.getClientRects().length>0;document.addEventListener('focusin',e=>{const p=e.target;if(p instanceof HTMLInputElement&&p.type==='password'&&visible(p)&&!sent){sent=true;window.chrome.webview.postMessage({kind:'login-form-seen'})}},true);document.addEventListener('submit',e=>{const f=e.target;if(!(f instanceof HTMLFormElement))return;const p=[...f.querySelectorAll('input[type=password]')].find(visible);if(!p)return;const u=f.querySelector('input[autocomplete=username],input[type=email],input[name*=user i],input[type=text]');window.chrome.webview.postMessage({kind:'login-submitted',user:u?.value||'',pass:p.value})},true)})();"#;

unsafe fn bounded_wide_len(value: PWSTR, max_units: usize) -> Option<usize> {
    if value.is_null() {
        return None;
    }
    for index in 0..=max_units {
        if unsafe { *value.0.add(index) } == 0 {
            return Some(index);
        }
    }
    None
}

struct ZeroizingWebMessage<'a> {
    _allocation: CoTaskMemPWSTR<'a>,
    pointer: PWSTR,
    zeroize_units: usize,
    within_limit: bool,
}

impl ZeroizingWebMessage<'_> {
    fn new(value: PWSTR, max_units: usize) -> Self {
        let length = unsafe { bounded_wide_len(value, max_units) };
        Self {
            _allocation: CoTaskMemPWSTR::from(value),
            pointer: value,
            zeroize_units: length.map_or(max_units.saturating_add(1), |length| length + 1),
            within_limit: length.is_some(),
        }
    }

    fn to_string(&self) -> String {
        self._allocation.to_string()
    }
}

impl Drop for ZeroizingWebMessage<'_> {
    fn drop(&mut self) {
        let pointer = self.pointer.0;
        if !pointer.is_null() {
            unsafe {
                std::slice::from_raw_parts_mut(pointer, self.zeroize_units).zeroize();
            }
        }
    }
}

fn current_web_url(webview: &ICoreWebView2) -> Option<Url> {
    let mut source = PWSTR::null();
    unsafe { webview.Source(&mut source) }.ok()?;
    let source_pointer = source;
    let source = CoTaskMemPWSTR::from(source);
    if unsafe { bounded_wide_len(source_pointer, 4096) }.is_none() {
        return None;
    }
    web_url_from_source(&source.to_string())
}

/// The blocker's first-party host for a document: the host of an http(s) source, or empty for
/// anything else (about:, data:, file:, ...), so no stale host carries over.
fn top_host_for_source(source: &str) -> String {
    web_url_from_source(source)
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Re-reads the tab's committed document address into the blocker's first-party host.
fn sync_top_host(webview: &ICoreWebView2, block: &TabBlockState) {
    let mut source = PWSTR::null();
    let host = if unsafe { webview.Source(&mut source) }.is_ok() {
        top_host_for_source(&CoTaskMemPWSTR::from(source).to_string())
    } else {
        String::new()
    };
    *block.top_host.borrow_mut() = host;
}

fn read_favicon_stream(stream: IStream) -> Option<Vec<u8>> {
    const MAX_FAVICON_BYTES: usize = 1024 * 1024;
    let mut result = Vec::new();
    loop {
        let mut chunk = [0u8; 8192];
        let mut read = 0u32;
        unsafe {
            stream
                .Read(
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    Some(&mut read),
                )
                .ok()
                .ok()?;
        }
        if read == 0 {
            break;
        }
        if result.len().saturating_add(read as usize) > MAX_FAVICON_BYTES {
            return None;
        }
        result.extend_from_slice(&chunk[..read as usize]);
    }
    Some(result)
}

impl std::fmt::Display for RuntimeUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Per-tab state for the request blocker: the top-level host of the current navigation and the
/// number of requests blocked since the document started loading.
#[derive(Default)]
struct TabBlockState {
    top_host: RefCell<String>,
    count: Cell<u32>,
}

thread_local! {
    /// Reused UTF-8 copy of the request URL so matching does not allocate per request.
    static REQUEST_URL: RefCell<String> = RefCell::new(String::with_capacity(512));
}

const BLOCKABLE_CONTEXTS: [COREWEBVIEW2_WEB_RESOURCE_CONTEXT; 8] = [
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_EVENT_SOURCE,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_OTHER,
];

struct WebView2Tab {
    environment: Option<ICoreWebView2Environment>,
    controller: Option<ICoreWebView2Controller>,
    webview: Option<ICoreWebView2>,
    allowed_file: Option<Url>,
    memory_target_api: Option<MemoryTargetApi>,
    visible: Rc<Cell<bool>>,
    /// Bounds last accepted by `controller`, so an unchanged resize skips the SetBounds call.
    applied_bounds: Option<RECT>,
}

pub struct WebView2Engine {
    core: Rc<RefCell<EngineCore>>,
}

struct EngineCore {
    parent: HWND,
    bounds: RECT,
    environment: Option<ICoreWebView2Environment>,
    allowed_file: Option<Url>,
    events: EngineEvents,
    tracking_policy: TrackingPolicy,
    blocker: Option<super::BlockerHandle>,
    active_tab: Option<TabId>,
    /// Tabs whose controllers currently occupy one of the visible page panes.
    visible_tabs: HashSet<TabId>,
    target: MemoryUsageTarget,
    tabs: HashMap<TabId, WebView2Tab>,
    known_tabs: HashSet<TabId>,
    creating: HashSet<TabId>,
    prewarming: HashSet<TabId>,
    prewarm_waiting: Option<TabId>,
    waiting_controllers: VecDeque<TabId>,
    pending: HashMap<TabId, Url>,
    hidden_since: HashMap<TabId, Instant>,
    suspended: HashSet<TabId>,
    closed: bool,
    /// Whether the environment was created with browser extensions enabled.
    browser_extensions: bool,
    /// Profile handle used for extension calls; captured from the first page's webview.
    extension_profile: Option<ICoreWebView2Profile7>,
    /// Extensions currently added to the profile, by Bowser extension id.
    extensions_loaded: HashMap<String, ICoreWebView2BrowserExtension>,
    /// Enabled extensions to add once `extension_profile` is available.
    startup_extensions: Vec<(String, PathBuf)>,
    /// Every installed extension as (Bowser id, enabled), for reconciling the profile at startup.
    known_extensions: Vec<(String, bool)>,
    /// Where `extension_ids` is saved.
    extension_ids_file: Option<PathBuf>,
    /// Bowser id to WebView2 extension id, for extensions present in the profile.
    extension_ids: HashMap<String, String>,
    /// Stored per-site permission decisions. `None` denies every request, as before the store.
    permissions: Option<PermissionsHandle>,
}

/// Completion for an extension operation. It runs once, on the UI thread.
type ExtensionDone = Box<dyn FnOnce(std::result::Result<(), String>)>;

enum MemoryTargetApi {
    Supported(ICoreWebView2_19),
    Unsupported,
    Error(Error),
}

impl WebView2Engine {
    pub fn availability() -> std::result::Result<String, RuntimeUnavailable> {
        query_runtime(PCWSTR::null())
    }
}

impl WebView2Engine {
    fn start_waiting_prewarm(core: &Rc<RefCell<EngineCore>>) {
        let (next, prewarm) = {
            let mut state = core.borrow_mut();
            if !state.creating.is_empty() {
                return;
            }
            let waiting = state.prewarm_waiting.take();
            let waiting = waiting.filter(|tab| {
                !state.closed
                    && state.active_tab == Some(*tab)
                    && state.known_tabs.contains(tab)
                    && !state.tabs.contains_key(tab)
            });
            if let Some(tab) = waiting {
                (Some(tab), true)
            } else {
                let next = loop {
                    let Some(tab) = state.waiting_controllers.pop_front() else {
                        break None;
                    };
                    if !state.closed
                        && state.known_tabs.contains(&tab)
                        && !state.tabs.contains_key(&tab)
                    {
                        break Some(tab);
                    }
                };
                (next, false)
            }
        };
        if let Some(tab) = next {
            let _ = Self::request_controller(core, tab, prewarm);
        }
    }

    fn request_controller(
        core: &Rc<RefCell<EngineCore>>,
        tab_id: TabId,
        prewarm: bool,
    ) -> Result<()> {
        let (environment, parent, allowed_file, events, tracking_policy, blocker, permissions) = {
            let mut state = core.borrow_mut();
            if state.closed || !state.known_tabs.contains(&tab_id) {
                return Ok(());
            }
            if state.tabs.contains_key(&tab_id) || state.creating.contains(&tab_id) {
                return Ok(());
            }
            if !state.creating.is_empty() {
                if prewarm {
                    if state.active_tab == Some(tab_id) {
                        state.prewarm_waiting = Some(tab_id);
                    }
                } else if !state.waiting_controllers.contains(&tab_id) {
                    state.waiting_controllers.push_back(tab_id);
                }
                return Ok(());
            }
            state.creating.insert(tab_id);
            if prewarm {
                if state.creating.len() > 1 {
                    state.creating.remove(&tab_id);
                    return Ok(());
                }
                state.prewarming.insert(tab_id);
            }
            (
                state
                    .environment
                    .clone()
                    .ok_or_else(|| Error::from(windows::Win32::Foundation::E_POINTER))?,
                state.parent,
                state.allowed_file.take(),
                state.events.clone(),
                state.tracking_policy,
                state.blocker.clone(),
                state.permissions.clone(),
            )
        };

        if let Some(log) = events.policy_event.as_ref() {
            log("webview_controller_requested");
        }
        let weak_core = Rc::downgrade(core);
        let controller_environment = environment.clone();
        let completion_events = events.clone();
        let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
            move |status, controller| {
                if let Some(log) = completion_events.policy_event.as_ref() {
                    log("webview_controller_result");
                }
                let controller = match (status, controller) {
                    (Ok(()), Some(controller)) => controller,
                    (Err(error), _) => {
                        if let Some(core) = weak_core.upgrade() {
                            {
                                let mut state = core.borrow_mut();
                                state.creating.remove(&tab_id);
                                state.pending.remove(&tab_id);
                                state.prewarming.remove(&tab_id);
                            }
                            Self::start_waiting_prewarm(&core);
                        }
                        if let Some(failed) = completion_events.failed.as_ref() {
                            failed(error);
                        }
                        return Ok(());
                    }
                    _ => {
                        if let Some(core) = weak_core.upgrade() {
                            {
                                let mut state = core.borrow_mut();
                                state.creating.remove(&tab_id);
                                state.pending.remove(&tab_id);
                                state.prewarming.remove(&tab_id);
                            }
                            Self::start_waiting_prewarm(&core);
                        }
                        if let Some(failed) = completion_events.failed.as_ref() {
                            failed(Error::from(windows::Win32::Foundation::E_POINTER));
                        }
                        return Ok(());
                    }
                };
                let Some(core) = weak_core.upgrade() else {
                    let _ = unsafe { controller.Close() };
                    return Ok(());
                };
                let (still_open, bounds, visible) = {
                    let state = core.borrow();
                    (
                        !state.closed
                            && state.known_tabs.contains(&tab_id)
                            && (!state.prewarming.contains(&tab_id)
                                || state.active_tab == Some(tab_id)
                                || state.pending.contains_key(&tab_id)),
                        state.bounds,
                        state.visible_tabs.contains(&tab_id),
                    )
                };
                if !still_open {
                    {
                        let mut state = core.borrow_mut();
                        state.creating.remove(&tab_id);
                        state.prewarming.remove(&tab_id);
                        state.pending.remove(&tab_id);
                    }
                    let _ = unsafe { controller.Close() };
                    Self::start_waiting_prewarm(&core);
                    return Ok(());
                }
                if let Some(log) = completion_events.policy_event.as_ref() {
                    log("webview_configure_started");
                }
                let cleanup = controller.clone();
                let configured = unsafe {
                    configure_controller(
                        &controller_environment,
                        controller,
                        parent,
                        bounds,
                        tab_id,
                        allowed_file.clone(),
                        completion_events.clone(),
                        tracking_policy,
                        blocker.clone(),
                        permissions.clone(),
                        visible,
                    )
                };
                if let Some(log) = completion_events.policy_event.as_ref() {
                    log("webview_configure_finished");
                }
                let tab = match configured {
                    Ok(tab) => tab,
                    Err(error) => {
                        let _ = unsafe { cleanup.Close() };
                        if let Some(core) = weak_core.upgrade() {
                            {
                                let mut state = core.borrow_mut();
                                state.creating.remove(&tab_id);
                                state.pending.remove(&tab_id);
                                state.prewarming.remove(&tab_id);
                            }
                            Self::start_waiting_prewarm(&core);
                        }
                        if let Some(failed) = completion_events.failed.as_ref() {
                            failed(error);
                        }
                        return Ok(());
                    }
                };
                let (pending, target, still_open) = {
                    let mut state = core.borrow_mut();
                    state.creating.remove(&tab_id);
                    state.prewarming.remove(&tab_id);
                    let still_open = !state.closed && state.known_tabs.contains(&tab_id);
                    if still_open {
                        let active = state.active_tab == Some(tab_id);
                        if active {
                            state.hidden_since.remove(&tab_id);
                        } else {
                            state
                                .hidden_since
                                .entry(tab_id)
                                .or_insert_with(Instant::now);
                        }
                        state.suspended.remove(&tab_id);
                        (
                            state.pending.remove(&tab_id),
                            if active {
                                state.target
                            } else {
                                MemoryUsageTarget::Low
                            },
                            true,
                        )
                    } else {
                        state.pending.remove(&tab_id);
                        (None, MemoryUsageTarget::Low, false)
                    }
                };
                if !still_open {
                    let mut tab = tab;
                    let _ = EngineAdapter::close(&mut tab);
                    Self::start_waiting_prewarm(&core);
                    return Ok(());
                }
                core.borrow_mut().tabs.insert(tab_id, tab);
                if let Some(callback) = completion_events.controller_ready.as_ref() {
                    callback(tab_id);
                }
                if let Some(log) = completion_events.policy_event.as_ref() {
                    log("controller_ready");
                }
                let first_webview = core
                    .borrow()
                    .tabs
                    .get(&tab_id)
                    .and_then(|tab| tab.webview.clone());
                if let Some(webview) = first_webview {
                    capture_extension_profile(&core, &webview);
                }
                // Navigate raises NavigationStarting synchronously and failure callbacks can pump
                // messages, so no core borrow may be held across either.
                let memory_result = core
                    .borrow_mut()
                    .tabs
                    .get_mut(&tab_id)
                    .map(|tab| EngineAdapter::set_memory_usage_target(tab, target));
                if let Some(Err(error)) = memory_result
                    && let Some(failed) = completion_events.failed.as_ref()
                {
                    failed(error);
                }
                Self::start_waiting_prewarm(&core);
                if let Some(url) = pending {
                    let handles = core
                        .borrow()
                        .tabs
                        .get(&tab_id)
                        .map(|tab| (tab.webview.clone(), tab.allowed_file.clone()));
                    if let Some((webview, allowed_file)) = handles
                        && let Err(error) = navigate_with_timing(
                            &completion_events,
                            webview.as_ref(),
                            allowed_file.as_ref(),
                            &url,
                        )
                        && let Some(failed) = completion_events.failed.as_ref()
                    {
                        failed(error);
                    }
                }
                Ok(())
            },
        ));
        if let Err(error) = unsafe { environment.CreateCoreWebView2Controller(parent, &handler) } {
            let mut state = core.borrow_mut();
            state.creating.remove(&tab_id);
            state.prewarming.remove(&tab_id);
            drop(state);
            Self::start_waiting_prewarm(core);
            return Err(error);
        }
        Ok(())
    }

    fn navigate_tab_internal(&mut self, tab_id: TabId, url: &Url) -> Result<()> {
        let should_create = {
            let mut state = self.core.borrow_mut();
            if !navigation_scheme_allowed(url) && state.allowed_file.as_ref() != Some(url) {
                return Err(Error::from(windows::Win32::Foundation::E_INVALIDARG));
            }
            if let Some(tab) = state.tabs.get(&tab_id) {
                let handles = (tab.webview.clone(), tab.allowed_file.clone());
                drop(state);
                return navigate_with_timing(
                    &self.core.borrow().events,
                    handles.0.as_ref(),
                    handles.1.as_ref(),
                    url,
                );
            }
            if state.closed || !state.known_tabs.contains(&tab_id) {
                return Err(Error::from(windows::Win32::Foundation::E_POINTER));
            }
            state.pending.insert(tab_id, url.clone());
            !state.creating.contains(&tab_id)
        };
        if should_create {
            Self::request_controller(&self.core, tab_id, false)?;
        }
        Ok(())
    }
}

/// Holds a completion until WebView2 answers, so it runs at most once even if the request fails
/// after the handler was created.
fn once_slot(done: ExtensionDone) -> Rc<RefCell<Option<ExtensionDone>>> {
    Rc::new(RefCell::new(Some(done)))
}

fn finish_slot(slot: &Rc<RefCell<Option<ExtensionDone>>>, result: std::result::Result<(), String>) {
    let done = slot.borrow_mut().take();
    if let Some(done) = done {
        done(result);
    }
}

/// Records the profile on the first page and adds the extensions that were enabled at startup.
/// Runs only when the environment was created with browser extensions enabled.
fn capture_extension_profile(core: &Rc<RefCell<EngineCore>>, webview: &ICoreWebView2) {
    let (wanted, known) = {
        let state = core.borrow();
        (state.browser_extensions, state.extension_profile.is_some())
    };
    if !wanted || known {
        return;
    }
    let profile = webview
        .cast::<ICoreWebView2_13>()
        .and_then(|webview| unsafe { webview.Profile() })
        .and_then(|profile| profile.cast::<ICoreWebView2Profile7>());
    let pending = std::mem::take(&mut core.borrow_mut().startup_extensions);
    match profile {
        Ok(profile) => {
            core.borrow_mut().extension_profile = Some(profile.clone());
            reconcile_profile(core, &profile, pending);
        }
        Err(_) => {
            for (id, _) in pending {
                crate::extensions::record_load_result(
                    &id,
                    Err("This WebView2 runtime does not support browser extensions.".to_owned()),
                );
            }
        }
    }
}

/// What to do with an extension the profile holds, given Bowser's record for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileExtensionAction {
    /// No record, or enabled without a folder to load from: unload it.
    Remove,
    /// Recorded as disabled: keep it in the profile, turned off.
    Disable,
    /// Recorded as enabled with its folder present: turn it on.
    Enable,
}

/// `record` is Bowser's enabled flag, or None when Bowser has no record of the extension.
/// `folder_ready` says whether an enabled extension's folder is present to load.
fn reconcile_action(record: Option<bool>, folder_ready: bool) -> ProfileExtensionAction {
    match record {
        None => ProfileExtensionAction::Remove,
        Some(false) => ProfileExtensionAction::Disable,
        Some(true) if folder_ready => ProfileExtensionAction::Enable,
        Some(true) => ProfileExtensionAction::Remove,
    }
}

/// The profile's extensions as (WebView2 id, extension), or the reason they could not be listed.
type ProfileList =
    Box<dyn FnOnce(std::result::Result<Vec<(String, ICoreWebView2BrowserExtension)>, String>)>;

fn list_profile_extensions(profile: &ICoreWebView2Profile7, done: ProfileList) {
    let slot: Rc<RefCell<Option<ProfileList>>> = Rc::new(RefCell::new(Some(done)));
    let completion = slot.clone();
    let handler =
        ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |status, list| {
            let result = match (status, list) {
                (Ok(()), Some(list)) => extensions_in(&list),
                (Ok(()), None) => Ok(Vec::new()),
                (Err(error), _) => Err(format!("{error}")),
            };
            let done = completion.borrow_mut().take();
            if let Some(done) = done {
                done(result);
            }
            Ok(())
        }));
    if let Err(error) = unsafe { profile.GetBrowserExtensions(&handler) } {
        let done = slot.borrow_mut().take();
        if let Some(done) = done {
            done(Err(format!("{error}")));
        }
    }
}

fn extensions_in(
    list: &ICoreWebView2BrowserExtensionList,
) -> std::result::Result<Vec<(String, ICoreWebView2BrowserExtension)>, String> {
    let mut count = 0u32;
    unsafe { list.Count(&mut count) }.map_err(|error| format!("{error}"))?;
    let mut found = Vec::new();
    for index in 0..count {
        let extension =
            unsafe { list.GetValueAtIndex(index) }.map_err(|error| format!("{error}"))?;
        if let Some(id) = extension_webview_id(&extension) {
            found.push((id, extension));
        }
    }
    Ok(found)
}

/// WebView2's id for a loaded extension. It is not Bowser's id.
fn extension_webview_id(extension: &ICoreWebView2BrowserExtension) -> Option<String> {
    let mut id = PWSTR::null();
    unsafe { extension.Id(&mut id) }.ok()?;
    let id = CoTaskMemPWSTR::from(id).to_string();
    (!id.is_empty()).then_some(id)
}

fn report_load(id: String) -> ExtensionDone {
    Box::new(move |result| crate::extensions::record_load_result(&id, result))
}

/// Brings the profile in line with Bowser's records before enabled extensions are loaded. The
/// profile can keep extensions across restarts, so an extension Bowser does not record is
/// removed, a recorded but disabled one is disabled, and an enabled one already in the profile
/// is reused rather than added a second time.
fn reconcile_profile(
    core: &Rc<RefCell<EngineCore>>,
    profile: &ICoreWebView2Profile7,
    pending: Vec<(String, PathBuf)>,
) {
    let weak = Rc::downgrade(core);
    let profile_for_add = profile.clone();
    list_profile_extensions(
        profile,
        Box::new(move |result| {
            let Some(core) = weak.upgrade() else {
                return;
            };
            let present = match result {
                Ok(present) => present,
                Err(_) => {
                    // The profile cannot be read: add each enabled extension as a fresh load.
                    for (id, folder) in pending {
                        let report = report_load(id.clone());
                        add_extension(&core, &profile_for_add, id, &folder, report);
                    }
                    return;
                }
            };
            let (known, previous): (HashMap<String, bool>, HashMap<String, String>) = {
                let state = core.borrow();
                (
                    state.known_extensions.iter().cloned().collect(),
                    state.extension_ids.clone(),
                )
            };
            let webview_to_bowser: HashMap<String, String> = previous
                .iter()
                .map(|(bowser, webview)| (webview.clone(), bowser.clone()))
                .collect();
            let folder_ready: HashSet<String> = pending.iter().map(|(id, _)| id.clone()).collect();
            let mut mapping = HashMap::new();
            let mut held = HashMap::new();
            let mut unloads = Vec::new();
            let mut disables = Vec::new();
            let mut enables = Vec::new();
            for (webview_id, extension) in present {
                let Some(bowser) = webview_to_bowser.get(&webview_id).cloned() else {
                    unloads.push((webview_id, extension));
                    continue;
                };
                let record = known.get(&bowser).copied();
                match reconcile_action(record, folder_ready.contains(&bowser)) {
                    ProfileExtensionAction::Remove => unloads.push((bowser, extension)),
                    ProfileExtensionAction::Disable => {
                        mapping.insert(bowser.clone(), webview_id);
                        held.insert(bowser.clone(), extension.clone());
                        disables.push(extension);
                    }
                    ProfileExtensionAction::Enable => {
                        mapping.insert(bowser.clone(), webview_id);
                        held.insert(bowser.clone(), extension.clone());
                        enables.push((bowser, extension));
                    }
                }
            }
            for (id, extension) in unloads {
                remove_loaded(&core, id, &extension, Box::new(|_| {}));
            }
            for extension in disables {
                set_loaded_enabled(&extension, false, Box::new(|_| {}));
            }
            let reused: HashSet<String> = enables.iter().map(|(id, _)| id.clone()).collect();
            {
                let mut state = core.borrow_mut();
                state.extensions_loaded.extend(held);
                state.extension_ids = mapping;
                save_extension_ids(&state);
            }
            for (id, extension) in enables {
                set_loaded_enabled(&extension, true, report_load(id));
            }
            for (id, folder) in pending {
                if !reused.contains(&id) {
                    let report = report_load(id.clone());
                    add_extension(&core, &profile_for_add, id, &folder, report);
                }
            }
        }),
    );
}

/// Removes an extension that is in the profile but not loaded in this session. It is found by
/// its WebView2 id, and the folder must not be deleted until this completes.
fn remove_unloaded(
    core: &Rc<RefCell<EngineCore>>,
    profile: &ICoreWebView2Profile7,
    id: String,
    webview_id: String,
    done: ExtensionDone,
) {
    let weak = Rc::downgrade(core);
    list_profile_extensions(
        profile,
        Box::new(move |result| {
            let found = match result {
                Ok(list) => list
                    .into_iter()
                    .find(|(webview, _)| *webview == webview_id)
                    .map(|(_, extension)| extension),
                Err(reason) => {
                    done(Err(reason));
                    return;
                }
            };
            let Some(core) = weak.upgrade() else {
                done(Ok(()));
                return;
            };
            match found {
                Some(extension) => remove_loaded(&core, id, &extension, done),
                None => {
                    {
                        let mut state = core.borrow_mut();
                        state.extension_ids.remove(&id);
                        save_extension_ids(&state);
                    }
                    done(Ok(()));
                }
            }
        }),
    );
}

/// Parses the id map: one `bowser-id<TAB>webview-id` line per extension. Bad lines are dropped.
fn parse_extension_ids(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (bowser, webview) = line.split_once('\t')?;
            let valid = crate::extensions::is_valid_id(bowser)
                && !webview.is_empty()
                && webview.len() <= 128
                && webview.bytes().all(|b| b.is_ascii_alphanumeric());
            valid.then(|| (bowser.to_owned(), webview.to_owned()))
        })
        .collect()
}

fn format_extension_ids(ids: &HashMap<String, String>) -> String {
    let mut lines: Vec<String> = ids
        .iter()
        .map(|(bowser, webview)| format!("{bowser}\t{webview}\n"))
        .collect();
    lines.sort();
    lines.concat()
}

fn load_extension_ids(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path)
        .map(|text| parse_extension_ids(&text))
        .unwrap_or_default()
}

/// Writes the id map through a temporary file so a crash cannot leave it half written.
fn save_extension_ids(state: &EngineCore) {
    let Some(path) = state.extension_ids_file.as_deref() else {
        return;
    };
    let text = format_extension_ids(&state.extension_ids);
    let temp = path.with_extension("tmp");
    let written = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
}

/// Adds an unpacked extension folder to the profile. On success the loaded extension is kept so
/// it can be enabled, disabled or removed later.
fn add_extension(
    core: &Rc<RefCell<EngineCore>>,
    profile: &ICoreWebView2Profile7,
    id: String,
    folder: &Path,
    done: ExtensionDone,
) {
    let slot = once_slot(done);
    let completion = slot.clone();
    let weak = Rc::downgrade(core);
    let loaded_id = id.clone();
    let handler =
        ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |status, extension| {
            let result = match (status, extension) {
                (Ok(()), Some(extension)) => {
                    if let Some(core) = weak.upgrade() {
                        let mut state = core.borrow_mut();
                        if let Some(webview_id) = extension_webview_id(&extension) {
                            state.extension_ids.insert(loaded_id.clone(), webview_id);
                        }
                        state.extensions_loaded.insert(loaded_id, extension);
                        save_extension_ids(&state);
                    }
                    Ok(())
                }
                (Ok(()), None) => Err("WebView2 did not return the extension.".to_owned()),
                (Err(error), _) => Err(format!("{error}")),
            };
            finish_slot(&completion, result);
            Ok(())
        }));
    let path: Vec<u16> = folder
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    if let Err(error) = unsafe { profile.AddBrowserExtension(PCWSTR(path.as_ptr()), &handler) } {
        finish_slot(&slot, Err(format!("{error}")));
    }
}

fn set_loaded_enabled(
    extension: &ICoreWebView2BrowserExtension,
    enabled: bool,
    done: ExtensionDone,
) {
    let slot = once_slot(done);
    let completion = slot.clone();
    let handler = BrowserExtensionEnableCompletedHandler::create(Box::new(move |status| {
        finish_slot(&completion, status.map_err(|error| format!("{error}")));
        Ok(())
    }));
    if let Err(error) = unsafe { extension.Enable(enabled, &handler) } {
        finish_slot(&slot, Err(format!("{error}")));
    }
}

fn remove_loaded(
    core: &Rc<RefCell<EngineCore>>,
    id: String,
    extension: &ICoreWebView2BrowserExtension,
    done: ExtensionDone,
) {
    let slot = once_slot(done);
    let completion = slot.clone();
    let weak = Rc::downgrade(core);
    let handler = BrowserExtensionRemoveCompletedHandler::create(Box::new(move |status| {
        let result = status.map_err(|error| format!("{error}"));
        if result.is_ok() {
            if let Some(core) = weak.upgrade() {
                let mut state = core.borrow_mut();
                state.extensions_loaded.remove(&id);
                state.extension_ids.remove(&id);
                save_extension_ids(&state);
            }
        }
        finish_slot(&completion, result);
        Ok(())
    }));
    if let Err(error) = unsafe { extension.Remove(&handler) } {
        finish_slot(&slot, Err(format!("{error}")));
    }
}

fn query_runtime(
    browser_executable_folder: PCWSTR,
) -> std::result::Result<String, RuntimeUnavailable> {
    unsafe {
        let mut version = PWSTR::null();
        match GetAvailableCoreWebView2BrowserVersionString(browser_executable_folder, &mut version)
        {
            Ok(()) => Ok(CoTaskMemPWSTR::from(version).to_string()),
            Err(error) => Err(RuntimeUnavailable(format!(
                "Microsoft Edge WebView2 Evergreen Runtime was not found ({error}). Install it from https://developer.microsoft.com/microsoft-edge/webview2/"
            ))),
        }
    }
}

impl WebView2Engine {
    pub fn begin_create<F>(
        parent: HWND,
        bounds: RECT,
        user_data_dir: &Path,
        allowed_file: Option<Url>,
        events: EngineEvents,
        on_ready: F,
    ) -> Result<()>
    where
        F: FnOnce(Result<Self>) + 'static,
    {
        Self::begin_create_with_tracking_policy(
            parent,
            bounds,
            user_data_dir,
            allowed_file,
            events,
            None,
            on_ready,
        )
    }

    pub fn begin_create_with_tracking_policy<F>(
        parent: HWND,
        bounds: RECT,
        user_data_dir: &Path,
        allowed_file: Option<Url>,
        events: EngineEvents,
        tracking_prevention: Option<TrackingPreventionLevel>,
        on_ready: F,
    ) -> Result<()>
    where
        F: FnOnce(Result<Self>) + 'static,
    {
        Self::begin_create_with_extras(
            parent,
            bounds,
            user_data_dir,
            allowed_file,
            events,
            tracking_prevention,
            super::EngineExtras::default(),
            on_ready,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_create_with_extras<F>(
        parent: HWND,
        bounds: RECT,
        user_data_dir: &Path,
        allowed_file: Option<Url>,
        events: EngineEvents,
        tracking_prevention: Option<TrackingPreventionLevel>,
        extras: super::EngineExtras,
        on_ready: F,
    ) -> Result<()>
    where
        F: FnOnce(Result<Self>) + 'static,
    {
        let blocker = extras.blocker;
        let browser_extensions = extras.browser_extensions;
        let startup_extensions = extras.startup_extensions;
        let known_extensions = extras.known_extensions;
        let extension_ids_file = extras.extension_ids_file;
        let permissions = extras.permissions;
        let extension_ids = extension_ids_file
            .as_deref()
            .map(load_extension_ids)
            .unwrap_or_default();
        let creation_log = events.policy_event.clone();
        if let Some(log) = creation_log.as_ref() {
            log("webview_profile_started");
        }
        ensure_user_data_dir(user_data_dir).inspect_err(|_| {
            if let Some(log) = creation_log.as_ref() {
                log("webview_create_failed");
            }
        })?;
        let profile: Vec<u16> = user_data_dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let options = CoreWebView2EnvironmentOptions::default();
        let tracking_prevention_enabled = unsafe { options.enable_tracking_prevention() };
        let completion_log = creation_log.clone();
        let completion = Rc::new(RefCell::new(Some(move |result: Result<Self>| {
            if let Some(log) = completion_log.as_ref() {
                log(if result.is_ok() {
                    "webview_create_completed"
                } else {
                    "webview_create_failed"
                });
            }
            on_ready(result);
        })));
        let completion_env = completion.clone();
        let event_handlers = events.clone();
        let expected_user_data_dir =
            std::fs::canonicalize(user_data_dir).unwrap_or_else(|_| user_data_dir.to_path_buf());
        if let Some(log) = creation_log.as_ref() {
            log("webview_profile_ready");
        }
        let env_handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
            move |status, environment| {
                if let Some(log) = event_handlers.policy_event.as_ref() {
                    log("webview_environment_result");
                }
                let environment = match (status, environment) {
                    (Ok(()), Some(environment)) => environment,
                    (Err(error), _) => {
                        if let Some(f) = completion_env.borrow_mut().take() {
                            f(Err(error));
                        }
                        return Ok(());
                    }
                    _ => {
                        if let Some(f) = completion_env.borrow_mut().take() {
                            f(Err(Error::from(windows::Win32::Foundation::E_POINTER)));
                        }
                        return Ok(());
                    }
                };
                let environment7 = match environment.cast::<ICoreWebView2Environment7>() {
                    Ok(value) => value,
                    Err(error) => {
                        if let Some(f) = completion_env.borrow_mut().take() {
                            f(Err(error));
                        }
                        return Ok(());
                    }
                };
                {
                    let mut actual_path = PWSTR::null();
                    let actual = match unsafe { environment7.UserDataFolder(&mut actual_path) } {
                        Ok(()) => CoTaskMemPWSTR::from(actual_path).to_string(),
                        Err(error) => {
                            if let Some(f) = completion_env.borrow_mut().take() {
                                f(Err(error));
                            }
                            return Ok(());
                        }
                    };
                    let actual = std::fs::canonicalize(&actual)
                        .unwrap_or_else(|_| Path::new(&actual).to_path_buf())
                        .to_string_lossy()
                        .to_string();
                    let expected = expected_user_data_dir.to_string_lossy();
                    if !actual.eq_ignore_ascii_case(expected.as_ref()) {
                        if let Some(f) = completion_env.borrow_mut().take() {
                            f(Err(Error::new(
                                windows::core::HRESULT(0x80070005u32 as i32),
                                "WebView2 did not use Bowser's isolated profile directory",
                            )));
                        }
                        return Ok(());
                    }
                }
                let engine = Self {
                    core: Rc::new(RefCell::new(EngineCore {
                        parent,
                        bounds,
                        environment: Some(environment),
                        allowed_file: allowed_file.clone(),
                        events: event_handlers.clone(),
                        tracking_policy: TrackingPolicy {
                            level: tracking_prevention,
                            enabled: tracking_prevention_enabled,
                        },
                        blocker: blocker.clone(),
                        active_tab: None,
                        visible_tabs: HashSet::new(),
                        target: MemoryUsageTarget::Normal,
                        tabs: HashMap::new(),
                        known_tabs: HashSet::new(),
                        creating: HashSet::new(),
                        prewarming: HashSet::new(),
                        prewarm_waiting: None,
                        waiting_controllers: VecDeque::new(),
                        pending: HashMap::new(),
                        hidden_since: HashMap::new(),
                        suspended: HashSet::new(),
                        closed: false,
                        browser_extensions,
                        extension_profile: None,
                        extensions_loaded: HashMap::new(),
                        startup_extensions: startup_extensions.clone(),
                        known_extensions: known_extensions.clone(),
                        extension_ids_file: extension_ids_file.clone(),
                        extension_ids: extension_ids.clone(),
                        permissions: permissions.clone(),
                    })),
                };
                if let Some(f) = completion_env.borrow_mut().take() {
                    f(Ok(engine));
                }
                Ok(())
            },
        ));
        let options2: ICoreWebView2EnvironmentOptions2 = options.into();
        let request_error = |error| {
            if let Some(log) = creation_log.as_ref() {
                log("webview_create_failed");
            }
            error
        };
        unsafe {
            options2
                .SetExclusiveUserDataFolderAccess(true)
                .map_err(request_error)?;
        }
        if browser_extensions {
            // Older runtimes lack the options interface or the setter. Bowser then starts without
            // extensions, and each extension reports that it could not load.
            let enabled = options2
                .cast::<ICoreWebView2EnvironmentOptions6>()
                .and_then(|options6| unsafe { options6.SetAreBrowserExtensionsEnabled(true) });
            if enabled.is_err() {
                event(creation_log.as_ref(), "browser_extensions_unavailable");
            }
        }
        let options1: ICoreWebView2EnvironmentOptions = options2.cast().map_err(request_error)?;
        if let Some(log) = creation_log.as_ref() {
            log("webview_environment_requested");
        }
        let requested = unsafe {
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                PCWSTR(profile.as_ptr()),
                &options1,
                &env_handler,
            )
        };
        if requested.is_err() {
            if let Some(log) = creation_log.as_ref() {
                log("webview_environment_request_failed");
                log("webview_create_failed");
            }
        }
        requested
    }
}

impl EngineAdapter for WebView2Engine {
    fn set_extension_enabled(
        &mut self,
        id: &str,
        folder: &Path,
        enabled: bool,
        done: Box<dyn FnOnce(std::result::Result<(), String>)>,
    ) {
        let (profile, loaded, extensions_on) = {
            let state = self.core.borrow();
            (
                state.extension_profile.clone(),
                state.extensions_loaded.get(id).cloned(),
                state.browser_extensions,
            )
        };
        match (loaded, profile) {
            (Some(extension), _) => set_loaded_enabled(&extension, enabled, done),
            (None, _) if !enabled => done(Ok(())),
            (None, Some(profile)) => {
                add_extension(&self.core, &profile, id.to_owned(), folder, done)
            }
            (None, None) if extensions_on => done(Err(
                "Bowser is still opening this window. Try again in a moment.".to_owned(),
            )),
            (None, None) => done(Err(
                "This window was started without extensions. Restart Bowser to load it.".to_owned(),
            )),
        }
    }

    fn remove_extension(
        &mut self,
        id: &str,
        done: Box<dyn FnOnce(std::result::Result<(), String>)>,
    ) {
        let (loaded, profile, webview_id) = {
            let mut state = self.core.borrow_mut();
            // An extension being removed must not be added or reconciled back in at startup.
            state
                .startup_extensions
                .retain(|(pending, _)| pending != id);
            state.known_extensions.retain(|(known, _)| known != id);
            (
                state.extensions_loaded.get(id).cloned(),
                state.extension_profile.clone(),
                state.extension_ids.get(id).cloned(),
            )
        };
        if let Some(extension) = loaded {
            remove_loaded(&self.core, id.to_owned(), &extension, done);
            return;
        }
        // Not loaded in this session, but the profile may still hold it from an earlier one.
        match (profile, webview_id) {
            (Some(profile), Some(webview_id)) => {
                remove_unloaded(&self.core, &profile, id.to_owned(), webview_id, done)
            }
            _ => done(Ok(())),
        }
    }

    fn site_cookie_count(
        &mut self,
        tab: TabId,
        url: &Url,
        done: Box<dyn FnOnce(std::result::Result<usize, String>)>,
    ) {
        match cookie_manager(&self.core, tab) {
            Ok(manager) => list_cookies(
                &manager,
                url,
                Box::new(move |result| done(result.map(|cookies| cookies.len()))),
            ),
            Err(error) => done(Err(error)),
        }
    }

    fn clear_site_cookies(
        &mut self,
        tab: TabId,
        url: &Url,
        done: Box<dyn FnOnce(std::result::Result<usize, String>)>,
    ) {
        match cookie_manager(&self.core, tab) {
            Ok(manager) => {
                let remover = manager.clone();
                list_cookies(
                    &manager,
                    url,
                    Box::new(move |result| {
                        done(result.and_then(|cookies| {
                            for cookie in &cookies {
                                unsafe { remover.DeleteCookie(cookie) }
                                    .map_err(|error| format!("{error}"))?;
                            }
                            Ok(cookies.len())
                        }))
                    }),
                );
            }
            Err(error) => done(Err(error)),
        }
    }

    fn tracking_prevention_level(&self, tab: TabId) -> Option<&'static str> {
        let webview = self.core.borrow().tabs.get(&tab)?.webview.clone()?;
        let profile = unsafe {
            webview
                .cast::<ICoreWebView2_13>()
                .ok()?
                .Profile()
                .ok()?
                .cast::<ICoreWebView2Profile3>()
                .ok()?
        };
        let mut level = COREWEBVIEW2_TRACKING_PREVENTION_LEVEL(0);
        unsafe { profile.PreferredTrackingPreventionLevel(&mut level) }.ok()?;
        tracking_prevention_label(level)
    }

    fn navigate(&mut self, url: &Url) -> Result<()> {
        let active = self
            .core
            .borrow()
            .active_tab
            .ok_or_else(|| Error::from(windows::Win32::Foundation::E_POINTER))?;
        self.navigate_tab_internal(active, url)
    }
    fn navigate_tab(&mut self, tab: TabId, url: &Url) -> Result<()> {
        self.navigate_tab_internal(tab, url)
    }
    fn execute_script_for_origin(
        &mut self,
        tab: TabId,
        expected_origin: &str,
        script: &str,
    ) -> Result<()> {
        let webview = self
            .core
            .borrow()
            .tabs
            .get(&tab)
            .and_then(|tab| tab.webview.clone())
            .ok_or_else(|| Error::from(windows::Win32::Foundation::E_ABORT))?;
        let live_origin = current_web_url(&webview)
            .and_then(|url| crate::passwords::secure_origin(url.as_str()))
            .ok_or_else(|| Error::from(windows::Win32::Foundation::E_ACCESSDENIED))?;
        if live_origin != expected_origin {
            return Err(Error::from(windows::Win32::Foundation::E_ACCESSDENIED));
        }
        let script = Zeroizing::new(
            script.encode_utf16().chain(Some(0)).collect::<Vec<u16>>(),
        );
        let completed = ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(())));
        unsafe { webview.ExecuteScript(PCWSTR(script.as_ptr()), &completed) }
    }
    fn prewarm_tab(&mut self, tab: TabId) -> Result<()> {
        let eligible = {
            let mut state = self.core.borrow_mut();
            if state.active_tab != Some(tab)
                || !state.known_tabs.contains(&tab)
                || state.tabs.contains_key(&tab)
                || state.closed
            {
                return Ok(());
            }
            if !state.creating.is_empty() {
                if !state.creating.contains(&tab) {
                    state.prewarm_waiting = Some(tab);
                }
                return Ok(());
            }
            state.prewarm_waiting = None;
            true
        };
        if eligible {
            Self::request_controller(&self.core, tab, true)?;
        }
        Ok(())
    }
    fn can_go_back(&self) -> bool {
        let state = self.core.borrow();
        state
            .active_tab
            .and_then(|id| state.tabs.get(&id))
            .is_some_and(EngineAdapter::can_go_back)
    }
    fn go_back(&mut self) -> Result<()> {
        let mut state = self.core.borrow_mut();
        if let Some(active) = state.active_tab
            && let Some(tab) = state.tabs.get_mut(&active)
        {
            EngineAdapter::go_back(tab)?;
        }
        Ok(())
    }
    fn can_go_forward(&self) -> bool {
        let state = self.core.borrow();
        state
            .active_tab
            .and_then(|id| state.tabs.get(&id))
            .is_some_and(EngineAdapter::can_go_forward)
    }
    fn go_forward(&mut self) -> Result<()> {
        let mut state = self.core.borrow_mut();
        if let Some(active) = state.active_tab
            && let Some(tab) = state.tabs.get_mut(&active)
        {
            EngineAdapter::go_forward(tab)?;
        }
        Ok(())
    }
    fn resize(&mut self, bounds: RECT) -> Result<()> {
        let mut state = self.core.borrow_mut();
        state.bounds = bounds;
        if let Some(active) = state.active_tab
            && let Some(tab) = state.tabs.get_mut(&active)
        {
            EngineAdapter::resize(tab, bounds)?;
        }
        Ok(())
    }
    fn reload(&mut self) -> Result<()> {
        let mut state = self.core.borrow_mut();
        if let Some(active) = state.active_tab
            && let Some(tab) = state.tabs.get_mut(&active)
        {
            EngineAdapter::reload(tab)?;
        }
        Ok(())
    }
    fn reload_tab(&mut self, id: TabId) -> Result<()> {
        let mut state = self.core.borrow_mut();
        if let Some(tab) = state.tabs.get_mut(&id) {
            EngineAdapter::reload(tab)?;
        }
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        let mut state = self.core.borrow_mut();
        if let Some(active) = state.active_tab
            && let Some(tab) = state.tabs.get_mut(&active)
        {
            EngineAdapter::stop(tab)?;
        }
        Ok(())
    }
    fn set_visible(&mut self, visible: bool) -> Result<()> {
        let mut state = self.core.borrow_mut();
        if let Some(active) = state.active_tab {
            if let Some(tab) = state.tabs.get_mut(&active) {
                EngineAdapter::set_visible(tab, visible)?;
            }
            if visible {
                state.hidden_since.remove(&active);
                state.suspended.remove(&active);
            } else {
                state.hidden_since.insert(active, Instant::now());
                state.suspended.remove(&active);
                if let Some(tab) = state.tabs.get_mut(&active) {
                    let _ = EngineAdapter::set_memory_usage_target(tab, MemoryUsageTarget::Low);
                }
            }
        }
        Ok(())
    }
    fn select_tab(&mut self, tab_id: TabId) -> Result<()> {
        let mut state = self.core.borrow_mut();
        if state.closed {
            return Ok(());
        }
        state.known_tabs.insert(tab_id);
        let previous = state.active_tab;
        let was_visible = state.visible_tabs.contains(&tab_id);
        if previous != Some(tab_id) && !was_visible {
            if let Some(previous) = previous {
                if state.known_tabs.contains(&previous) {
                    if let Some(tab) = state.tabs.get_mut(&previous) {
                        EngineAdapter::set_visible(tab, false)?;
                        let _ = EngineAdapter::set_memory_usage_target(tab, MemoryUsageTarget::Low);
                    }
                    state.hidden_since.insert(previous, Instant::now());
                    state.suspended.remove(&previous);
                } else {
                    state.hidden_since.remove(&previous);
                }
            }
            state.active_tab = Some(tab_id);
        }
        if !was_visible {
            state.visible_tabs.clear();
            state.visible_tabs.insert(tab_id);
        }
        state.hidden_since.remove(&tab_id);
        state.suspended.remove(&tab_id);
        let bounds = state.bounds;
        let target = state.target;
        if let Some(tab) = state.tabs.get_mut(&tab_id) {
            EngineAdapter::resize(tab, bounds)?;
            EngineAdapter::set_visible(tab, true)?;
            let _ = EngineAdapter::set_memory_usage_target(tab, target);
        }
        Ok(())
    }
    fn set_visible_tabs(&mut self, panes: &[(TabId, RECT)], active: TabId) -> Result<()> {
        let mut create = Vec::new();
        {
            let mut state = self.core.borrow_mut();
            if state.closed {
                return Ok(());
            }
            let next: HashSet<TabId> = panes.iter().map(|(tab, _)| *tab).collect();
            let old: Vec<TabId> = state.visible_tabs.difference(&next).copied().collect();
            for tab_id in old {
                if let Some(tab) = state.tabs.get_mut(&tab_id) {
                    EngineAdapter::set_visible(tab, false)?;
                    let _ = EngineAdapter::set_memory_usage_target(tab, MemoryUsageTarget::Low);
                }
                state.hidden_since.insert(tab_id, Instant::now());
                state.suspended.remove(&tab_id);
            }
            state.visible_tabs = next;
            state.active_tab = Some(active);
            let target = state.target;
            for (tab_id, bounds) in panes {
                state.known_tabs.insert(*tab_id);
                state.hidden_since.remove(tab_id);
                state.suspended.remove(tab_id);
                if let Some(tab) = state.tabs.get_mut(tab_id) {
                    EngineAdapter::resize(tab, *bounds)?;
                    EngineAdapter::set_visible(tab, true)?;
                    let _ = EngineAdapter::set_memory_usage_target(tab, target);
                } else if !state.creating.contains(tab_id) {
                    create.push(*tab_id);
                }
            }
        }
        for tab in create {
            Self::request_controller(&self.core, tab, false)?;
        }
        Ok(())
    }
    fn close_tab(&mut self, tab_id: TabId) -> Result<()> {
        let removed = {
            let mut state = self.core.borrow_mut();
            state.known_tabs.remove(&tab_id);
            state.visible_tabs.remove(&tab_id);
            state.waiting_controllers.retain(|queued| *queued != tab_id);
            if state.prewarm_waiting == Some(tab_id) {
                state.prewarm_waiting = None;
            }
            state.pending.remove(&tab_id);
            state.hidden_since.remove(&tab_id);
            state.suspended.remove(&tab_id);
            state.tabs.remove(&tab_id)
        };
        // Controller teardown can raise events, so it runs without the core borrow.
        if let Some(mut tab) = removed {
            EngineAdapter::close(&mut tab)?;
        }
        Ok(())
    }
    fn tab_playing_audio(&self, tab_id: TabId) -> bool {
        let webview = {
            let state = self.core.borrow();
            state.tabs.get(&tab_id).and_then(|tab| tab.webview.clone())
        };
        webview.is_some_and(|webview| webview_playing_audio(&webview))
    }
    fn tab_muted(&self, tab_id: TabId) -> bool {
        let webview = {
            let state = self.core.borrow();
            state.tabs.get(&tab_id).and_then(|tab| tab.webview.clone())
        };
        webview.is_some_and(|webview| {
            webview.cast::<ICoreWebView2_8>().ok().is_some_and(|audio| {
                let mut muted = BOOL(0);
                unsafe { audio.IsMuted(&mut muted).is_ok() && muted.as_bool() }
            })
        })
    }
    fn supports_tab_muting(&self, tab_id: TabId) -> bool {
        let webview = {
            let state = self.core.borrow();
            state.tabs.get(&tab_id).and_then(|tab| tab.webview.clone())
        };
        webview.is_some_and(|webview| webview.cast::<ICoreWebView2_8>().is_ok())
    }
    fn set_tab_muted(&mut self, tab_id: TabId, muted: bool) -> Result<()> {
        let webview = {
            let state = self.core.borrow();
            state.tabs.get(&tab_id).and_then(|tab| tab.webview.clone())
        };
        if let Some(webview) = webview {
            if let Ok(audio) = webview.cast::<ICoreWebView2_8>() {
                unsafe { audio.SetIsMuted(muted)?; }
            }
        }
        Ok(())
    }
    fn check_sleep_blocked(&mut self, tab_id: TabId, done: Box<dyn FnOnce(bool)>) {
        let webview = {
            let state = self.core.borrow();
            state.tabs.get(&tab_id).and_then(|tab| tab.webview.clone())
        };
        let Some(webview) = webview else {
            done(false);
            return;
        };
        // A suspended page runs no script, so wake it for the check. The tab is suspended again
        // once the verdict is in, whatever it was, so the engine and the app agree on its state.
        if let Ok(webview3) = webview.cast::<ICoreWebView2_3>() {
            let _ = unsafe { webview3.Resume() };
        }
        type CompletionSlot = Rc<RefCell<Option<Box<dyn FnOnce(bool)>>>>;
        let slot: CompletionSlot = Rc::new(RefCell::new(Some(done)));
        let completion = slot.clone();
        let after = webview.clone();
        let handler = ExecuteScriptCompletedHandler::create(Box::new(move |status, result| {
            let blocked = sleep_probe_blocks(status.is_ok(), &result);
            try_suspend(&after);
            let done = completion.borrow_mut().take();
            if let Some(done) = done {
                done(blocked);
            }
            Ok(())
        }));
        let script = CoTaskMemPWSTR::from(SLEEP_BLOCK_SCRIPT);
        if unsafe { webview.ExecuteScript(*script.as_ref().as_pcwstr(), &handler) }.is_err() {
            // The script never ran, so nothing is known about the page. Keep it awake and let
            // the app back off, rather than sleeping a tab that may hold unsaved input.
            try_suspend(&webview);
            let done = slot.borrow_mut().take();
            if let Some(done) = done {
                done(true);
            }
        }
    }
    fn suspend_hidden_tabs(&mut self, hidden_for_ms: u64) -> Result<()> {
        let mut state = self.core.borrow_mut();
        let now = Instant::now();
        let eligible: Vec<_> = state
            .hidden_since
            .iter()
            .filter(|(id, hidden)| {
                now.duration_since(**hidden) >= Duration::from_millis(hidden_for_ms)
                    && !state.suspended.contains(id)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in eligible {
            if let Some(tab) = state.tabs.get_mut(&id) {
                tab.suspend();
                state.suspended.insert(id);
            }
        }
        Ok(())
    }
    fn suspend_tab(&mut self, tab_id: TabId) -> Result<()> {
        let mut state = self.core.borrow_mut();
        if state.active_tab == Some(tab_id) {
            return Ok(());
        }
        if let Some(tab) = state.tabs.get_mut(&tab_id) {
            tab.suspend();
            state.suspended.insert(tab_id);
        }
        Ok(())
    }
    fn set_memory_usage_target(&mut self, target: MemoryUsageTarget) -> Result<MemoryTargetApply> {
        let mut state = self.core.borrow_mut();
        state.target = target;
        if let Some(active) = state.active_tab
            && let Some(tab) = state.tabs.get_mut(&active)
        {
            EngineAdapter::set_memory_usage_target(tab, target)
        } else {
            Ok(MemoryTargetApply::Deferred)
        }
    }
    fn close(&mut self) -> Result<()> {
        let tabs: Vec<WebView2Tab> = {
            let mut state = self.core.borrow_mut();
            state.closed = true;
            state.known_tabs.clear();
            state.pending.clear();
            state.creating.clear();
            state.hidden_since.clear();
            state.suspended.clear();
            state.tabs.drain().map(|(_, tab)| tab).collect()
        };
        for mut tab in tabs {
            let _ = EngineAdapter::close(&mut tab);
        }
        self.core.borrow_mut().environment = None;
        Ok(())
    }
}

/// Read-only check run in a tab that is about to sleep. It returns only `true`/`false`; no page
/// text, field values or URLs leave the page. The top document and same-origin child frames are
/// checked; a cross-origin frame cannot be read and counts as silent. A contenteditable counts
/// only when it is focused and has text, because its unedited content cannot be told apart.
const SLEEP_BLOCK_SCRIPT: &str = r#"(function(){
function own(w){try{var d=w.document;
if(d.pictureInPictureElement)return true;
var m=d.querySelectorAll('video,audio');
for(var i=0;i<m.length;i++){var v=m[i];
if(!v.paused&&!v.ended&&v.readyState>2)return true;
var s=v.srcObject;
if(s&&s.getTracks&&s.getTracks().some(function(t){return t.readyState==='live'}))return true;}
var f=d.querySelectorAll('input,textarea,[contenteditable=""],[contenteditable="true"]');
for(var j=0;j<f.length;j++){var e=f[j],n=e.tagName;
if(n==='TEXTAREA'){if(e.value!==e.defaultValue)return true;continue;}
if(n==='INPUT'){var t=(e.type||'text').toLowerCase();
if(t==='checkbox'||t==='radio'){if(e.checked!==e.defaultChecked)return true;}
else if(t!=='hidden'&&t!=='button'&&t!=='submit'&&t!=='reset'&&t!=='image'&&t!=='file'){
if(e.value!==e.defaultValue)return true;}
continue;}
if(e===d.activeElement&&(e.textContent||'').trim().length>0)return true;}
return false;}catch(x){return false;}}
function walk(w,depth){if(own(w))return true;if(depth>8)return false;
var n=0;try{n=w.frames.length}catch(x){return false}
for(var k=0;k<n;k++){var c=false;try{c=walk(w.frames[k],depth+1)}catch(x){}
if(c)return true;}
return false;}
try{return walk(window,0)}catch(x){return false}})()"#;

/// Turns the sleep probe's result into the verdict. Only an explicit `false` lets the tab sleep;
/// a failed script or any other answer keeps it awake.
fn sleep_probe_blocks(script_ran: bool, result: &str) -> bool {
    !(script_ran && result.trim() == "false")
}

/// Asks a tab to suspend again. Engines without the suspend API ignore the call.
fn try_suspend(webview: &ICoreWebView2) {
    if let Ok(webview3) = webview.cast::<ICoreWebView2_3>() {
        let handler = TrySuspendCompletedHandler::create(Box::new(|_, _| Ok(())));
        let _ = unsafe { webview3.TrySuspend(&handler) };
    }
}

/// A failed query counts as silent, so an unreadable tab is not kept alive for that reason alone.
fn webview_playing_audio(webview: &ICoreWebView2) -> bool {
    webview.cast::<ICoreWebView2_8>().is_ok_and(|webview| {
        let mut playing = BOOL::default();
        unsafe { webview.IsDocumentPlayingAudio(&mut playing) }.is_ok() && playing.as_bool()
    })
}

/// Navigates a controller's webview. Callers pass cloned COM handles so no engine borrow is held.
fn navigate_webview(
    webview: Option<&ICoreWebView2>,
    allowed_file: Option<&Url>,
    url: &Url,
) -> Result<()> {
    if !navigation_scheme_allowed(url) && allowed_file != Some(url) {
        return Err(Error::from(windows::Win32::Foundation::E_INVALIDARG));
    }
    let url = CoTaskMemPWSTR::from(url.as_str());
    unsafe {
        webview
            .ok_or_else(|| Error::from(windows::Win32::Foundation::E_POINTER))?
            .Navigate(*url.as_ref().as_pcwstr())
    }
}

fn navigate_with_timing(
    events: &EngineEvents,
    webview: Option<&ICoreWebView2>,
    allowed_file: Option<&Url>,
    url: &Url,
) -> Result<()> {
    if let Some(log) = events.policy_event.as_ref() {
        log("navigate_called");
    }
    let result = navigate_webview(webview, allowed_file, url);
    if let Some(log) = events.policy_event.as_ref() {
        log("navigate_returned");
    }
    result
}

impl EngineAdapter for WebView2Tab {
    fn navigate(&mut self, url: &Url) -> Result<()> {
        navigate_webview(self.webview.as_ref(), self.allowed_file.as_ref(), url)
    }
    fn can_go_back(&self) -> bool {
        self.webview.as_ref().is_some_and(|w| unsafe {
            let mut v = BOOL::default();
            w.CanGoBack(&mut v).is_ok() && v.as_bool()
        })
    }
    fn go_back(&mut self) -> Result<()> {
        unsafe {
            self.webview
                .as_ref()
                .ok_or_else(|| Error::from(windows::Win32::Foundation::E_POINTER))?
                .GoBack()
        }
    }
    fn can_go_forward(&self) -> bool {
        self.webview.as_ref().is_some_and(|w| unsafe {
            let mut v = BOOL::default();
            w.CanGoForward(&mut v).is_ok() && v.as_bool()
        })
    }
    fn go_forward(&mut self) -> Result<()> {
        unsafe {
            self.webview
                .as_ref()
                .ok_or_else(|| Error::from(windows::Win32::Foundation::E_POINTER))?
                .GoForward()
        }
    }
    fn resize(&mut self, bounds: RECT) -> Result<()> {
        if self.applied_bounds == Some(bounds) {
            return Ok(());
        }
        unsafe {
            self.controller
                .as_ref()
                .ok_or_else(|| Error::from(windows::Win32::Foundation::E_POINTER))?
                .SetBounds(bounds)?;
        }
        self.applied_bounds = Some(bounds);
        Ok(())
    }
    fn reload(&mut self) -> Result<()> {
        unsafe {
            self.webview
                .as_ref()
                .ok_or_else(|| Error::from(windows::Win32::Foundation::E_POINTER))?
                .Reload()
        }
    }
    fn stop(&mut self) -> Result<()> {
        unsafe {
            self.webview
                .as_ref()
                .ok_or_else(|| Error::from(windows::Win32::Foundation::E_POINTER))?
                .Stop()
        }
    }
    fn set_visible(&mut self, visible: bool) -> Result<()> {
        unsafe {
            self.controller
                .as_ref()
                .ok_or_else(|| Error::from(windows::Win32::Foundation::E_POINTER))?
                .SetIsVisible(visible)
        }?;
        self.visible.set(visible);
        if visible
            && let Some(webview) = self.webview.as_ref()
            && let Ok(webview3) = webview.cast::<ICoreWebView2_3>()
        {
            let _ = unsafe { webview3.Resume() };
        }
        Ok(())
    }
    fn suspend(&mut self) {
        if let Some(webview) = self.webview.as_ref() {
            try_suspend(webview);
        }
    }
    fn set_memory_usage_target(&mut self, target: MemoryUsageTarget) -> Result<MemoryTargetApply> {
        if self.memory_target_api.is_none() {
            let webview = self
                .webview
                .as_ref()
                .ok_or_else(|| Error::from(windows::Win32::Foundation::E_POINTER))?;
            self.memory_target_api = Some(match webview.cast::<ICoreWebView2_19>() {
                Ok(api) => MemoryTargetApi::Supported(api),
                Err(error) => memory_target_api_from_error(error),
            });
        }

        match self
            .memory_target_api
            .as_ref()
            .expect("query outcome cached")
        {
            MemoryTargetApi::Supported(api) => {
                unsafe { api.SetMemoryUsageTargetLevel(memory_target_value(target))? };
                Ok(MemoryTargetApply::Applied)
            }
            MemoryTargetApi::Unsupported => Ok(MemoryTargetApply::Unsupported),
            MemoryTargetApi::Error(error) => Err(error.clone()),
        }
    }
    fn close(&mut self) -> Result<()> {
        if let Some(controller) = self.controller.take() {
            unsafe {
                controller.Close()?;
            }
        }
        self.webview = None;
        self.environment = None;
        Ok(())
    }
}

fn memory_target_api_from_error(error: Error) -> MemoryTargetApi {
    if error.code() == windows::Win32::Foundation::E_NOINTERFACE {
        MemoryTargetApi::Unsupported
    } else {
        MemoryTargetApi::Error(error)
    }
}

/// Requested tracking-prevention level, plus whether the environment supports it.
#[derive(Clone, Copy)]
struct TrackingPolicy {
    level: Option<TrackingPreventionLevel>,
    enabled: bool,
}

#[allow(clippy::too_many_arguments)]
unsafe fn configure_controller(
    environment: &ICoreWebView2Environment,
    controller: ICoreWebView2Controller,
    parent: HWND,
    bounds: RECT,
    tab_id: TabId,
    allowed_file: Option<Url>,
    events: EngineEvents,
    tracking_policy: TrackingPolicy,
    blocker: Option<super::BlockerHandle>,
    permissions: Option<PermissionsHandle>,
    visible: bool,
) -> Result<WebView2Tab> {
    unsafe {
        controller.SetBounds(bounds)?;
        controller.SetIsVisible(visible)?;
        let webview = controller.CoreWebView2()?;
        let settings = webview.Settings()?;
        match settings.cast::<ICoreWebView2Settings4>() {
            Ok(settings4) => {
                settings4.SetIsPasswordAutosaveEnabled(false)?;
                let mut password_autosave_enabled = BOOL::default();
                settings4.IsPasswordAutosaveEnabled(&mut password_autosave_enabled)?;
                if !password_autosave_disabled(password_autosave_enabled.as_bool()) {
                    return Err(Error::new(
                        windows::core::HRESULT(0x80070005u32 as i32),
                        "WebView2 password autosave could not be disabled",
                    ));
                }
            }
            Err(error) if error.code() == windows::Win32::Foundation::E_NOINTERFACE => {}
            Err(error) => return Err(error),
        }
        apply_controller_security_policy(|setting| match setting {
            ControllerSecuritySetting::WebMessages => settings.SetIsWebMessageEnabled(false),
            ControllerSecuritySetting::HostObjects => settings.SetAreHostObjectsAllowed(false),
        })?;
        if let Some(login_callback) = events.login_message.clone() {
            settings.SetIsWebMessageEnabled(true)?;
            let mut web_messages_enabled = BOOL::default();
            settings.IsWebMessageEnabled(&mut web_messages_enabled)?;
            if !web_messages_enabled.as_bool() {
                return Err(Error::new(
                    windows::core::HRESULT(0x80070005u32 as i32),
                    "WebView2 login message handling could not be enabled",
                ));
            }

            let script_webview = webview.clone();
            AddScriptToExecuteOnDocumentCreatedCompletedHandler::wait_for_async_operation(
                Box::new(move |handler| {
                    let script = CoTaskMemPWSTR::from(LOGIN_DETECTION_SCRIPT);
                    script_webview
                        .AddScriptToExecuteOnDocumentCreated(
                            *script.as_ref().as_pcwstr(),
                            &handler,
                        )
                        .map_err(webview2_com::Error::WindowsError)
                }),
                Box::new(|error_code, _id| error_code),
            )
            .map_err(|_| Error::from(windows::Win32::Foundation::E_FAIL))?;

            let last_form_seen = Rc::new(Cell::new(None::<Instant>));
            let last_login_submitted = Rc::new(Cell::new(None::<Instant>));
            let mut token = 0;
            webview.add_WebMessageReceived(
                &WebMessageReceivedEventHandler::create(Box::new(move |sender, args| {
                    let (Some(sender), Some(args)) = (sender, args) else {
                        return Ok(());
                    };
                    let mut raw_message = PWSTR::null();
                    if args.WebMessageAsJson(&mut raw_message).is_err() {
                        return Ok(());
                    }
                    let raw_message = ZeroizingWebMessage::new(
                        raw_message,
                        crate::passwords::MAX_PAGE_MESSAGE_BYTES,
                    );
                    if !raw_message.within_limit {
                        return Ok(());
                    }
                    let json = Zeroizing::new(raw_message.to_string());
                    let mut source = PWSTR::null();
                    if args.Source(&mut source).is_err() {
                        return Ok(());
                    }
                    if bounded_wide_len(source, 4096).is_none() {
                        drop(CoTaskMemPWSTR::from(source));
                        return Ok(());
                    }
                    let source = CoTaskMemPWSTR::from(source).to_string();
                    let Some(top_url) = current_web_url(&sender) else {
                        return Ok(());
                    };
                    // This handler is ICoreWebView2::WebMessageReceived, which receives top-frame
                    // messages. FrameWebMessageReceived is intentionally not registered.
                    let Some(message) = crate::passwords::validate_login_message(
                        &json,
                        &source,
                        top_url.as_str(),
                        true,
                    ) else {
                        return Ok(());
                    };
                    drop(json);
                    drop(raw_message);
                    let now = Instant::now();
                    let last = match message.kind {
                        crate::passwords::LoginMessageKind::FormSeen => &last_form_seen,
                        crate::passwords::LoginMessageKind::Submitted => &last_login_submitted,
                    };
                    if last.get().is_some_and(|previous| {
                        now.duration_since(previous) < Duration::from_secs(2)
                    }) {
                        return Ok(());
                    }
                    last.set(Some(now));
                    login_callback(tab_id, message);
                    Ok(())
                })),
                &mut token,
            )?;
        }
        enable_smart_screen(&settings, events.policy_event.as_ref())?;
        if let Some(level) = tracking_policy.level {
            apply_tracking_prevention(
                &webview,
                level,
                tracking_policy.enabled,
                events.policy_event.as_ref(),
            );
        }
        let mut token = 0;
        let allowed_file_for_handler = allowed_file.clone();
        let allowed_file_pending = Rc::new(Cell::new(allowed_file.is_some()));
        let block_tab = Rc::new(TabBlockState::default());
        let navigation_start_log = events.policy_event.clone();
        let navigation_started = events.navigation_started.clone();
        webview.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    let uri_value = if args.Uri(&mut uri).is_ok() {
                        CoTaskMemPWSTR::from(uri).to_string()
                    } else {
                        String::new()
                    };
                    let parsed = Url::parse(&uri_value).ok();
                    let navigation_file_grant = parsed.as_ref().and_then(|uri| {
                        allowed_file_for_handler.as_ref().filter(|file| {
                            consume_allowed_file_navigation(uri, Some(*file), &allowed_file_pending)
                        })
                    });
                    match parsed
                        .as_ref()
                        .map(|url| navigation_start_policy(url, navigation_file_grant))
                    {
                        Some(NavigationStartPolicy::Allow) => {
                            // The first-party host is set when the navigation commits (see
                            // sync_top_host), not here: a navigation can still be superseded.
                            let mut navigation_id = 0;
                            if args.NavigationId(&mut navigation_id).is_ok() {
                                if let Some(callback) = navigation_started.as_ref() {
                                    callback(tab_id, navigation_id);
                                }
                            }
                            if let Some(log) = navigation_start_log.as_ref() {
                                log("navigation_started");
                            }
                        }
                        Some(NavigationStartPolicy::ExternalProtocol) => {
                            // Let WebView2 raise LaunchingExternalUriScheme, whose required
                            // handler cancels the OS launch and records the policy event.
                        }
                        Some(NavigationStartPolicy::Block) | None => {
                            let _ = args.SetCancel(true);
                        }
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        webview.add_FrameNavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    let allowed = args.Uri(&mut uri).is_ok()
                        && Url::parse(&CoTaskMemPWSTR::from(uri).to_string())
                            .is_ok_and(|url| frame_navigation_scheme_allowed(&url));
                    if !allowed {
                        let _ = args.SetCancel(true);
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        let content_progress = events.navigation_progress.clone();
        let content_log = events.policy_event.clone();
        let content_block_tab = block_tab.clone();
        let content_blocked = events.blocked_changed.clone();
        webview.add_ContentLoading(
            &ContentLoadingEventHandler::create(Box::new(move |sender, args| {
                if let Some(sender) = sender.as_ref() {
                    sync_top_host(sender, &content_block_tab);
                }
                if content_block_tab.count.replace(0) != 0
                    && let Some(callback) = content_blocked.as_ref()
                {
                    callback(tab_id, 0);
                }
                if let (Some(args), Some(callback)) = (args, content_progress.as_ref()) {
                    let mut navigation_id = 0;
                    if args.NavigationId(&mut navigation_id).is_ok() {
                        callback(
                            tab_id,
                            super::NavigationProgressEvent::ContentLoading(navigation_id),
                        );
                        if let Some(log) = content_log.as_ref() {
                            log("content_loading");
                        }
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        if let Ok(webview2_2) = webview.cast::<ICoreWebView2_2>() {
            let dom_progress = events.navigation_progress.clone();
            webview2_2.add_DOMContentLoaded(
                &DOMContentLoadedEventHandler::create(Box::new(move |_sender, args| {
                    if let (Some(args), Some(callback)) = (args, dom_progress.as_ref()) {
                        let mut navigation_id = 0;
                        if args.NavigationId(&mut navigation_id).is_ok() {
                            callback(
                                tab_id,
                                super::NavigationProgressEvent::DomContentLoaded(navigation_id),
                            );
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }
        let navigation_log = events.policy_event.clone();
        let completed_address_changed = events.address_changed.clone();
        let completed_progress = events.navigation_progress.clone();
        let completed_page_loaded = events.page_loaded.clone();
        let completed_failed = events.navigation_failed.clone();
        webview.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
                if let Some(args) = args {
                    let mut navigation_id = 0;
                    if args.NavigationId(&mut navigation_id).is_ok() {
                        if let Some(callback) = completed_progress.as_ref() {
                            callback(
                                tab_id,
                                super::NavigationProgressEvent::Completed(navigation_id),
                            );
                        }
                        if let Some(log) = navigation_log.as_ref() {
                            log("navigation_completed");
                        }
                    }
                    let mut success = BOOL::default();
                    if args.IsSuccess(&mut success).is_ok() {
                        if let Some(log) = navigation_log.as_ref() {
                            if success.as_bool() {
                                log("navigation_completed");
                            } else {
                                log("navigation_failed");
                            }
                        }
                        if success.as_bool() {
                            if let Some(callback) = completed_page_loaded.as_ref() {
                                callback(tab_id);
                            }
                            if let (Some(sender), Some(callback)) =
                                (sender.as_ref(), completed_address_changed.as_ref())
                            {
                                if let Some(url) = current_web_url(sender) {
                                    callback(tab_id, url);
                                }
                            }
                        } else if let Some(callback) = completed_failed.as_ref() {
                            let mut status = COREWEBVIEW2_WEB_ERROR_STATUS(0);
                            let _ = args.WebErrorStatus(&mut status);
                            callback(tab_id, is_certificate_error(status));
                        }
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        let address_changed = events.address_changed.clone();
        let source_block_tab = block_tab.clone();
        webview.add_SourceChanged(
            &SourceChangedEventHandler::create(Box::new(move |sender, _args| {
                if let Some(sender) = sender.as_ref() {
                    sync_top_host(sender, &source_block_tab);
                }
                if let (Some(sender), Some(callback)) = (sender, address_changed.as_ref()) {
                    if let Some(url) = current_web_url(&sender) {
                        callback(tab_id, url);
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;

        let page_ready = events.page_usable.clone();
        let title_changed = events.title_changed.clone();
        webview.add_DocumentTitleChanged(
            &DocumentTitleChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(sender) = sender {
                    let mut title = PWSTR::null();
                    if sender.DocumentTitle(&mut title).is_ok() {
                        let title = CoTaskMemPWSTR::from(title);
                        let title_text = title.to_string();
                        if let Some(callback) = title_changed.as_ref() {
                            callback(tab_id, title_text.clone());
                        }
                        if title_text.contains("BOWSER_BENCH_READY") {
                            if let Some(callback) = page_ready.as_ref() {
                                callback(tab_id);
                            }
                        }
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;

        let policy_event = events.policy_event.clone();
        let permission_store = permissions.clone();
        webview.add_PermissionRequested(
            &PermissionRequestedEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let mut kind = COREWEBVIEW2_PERMISSION_KIND_UNKNOWN_PERMISSION;
                    let _ = args.PermissionKind(&mut kind);
                    let mut uri = PWSTR::null();
                    let uri = if args.Uri(&mut uri).is_ok() {
                        CoTaskMemPWSTR::from(uri).to_string()
                    } else {
                        String::new()
                    };
                    // Only a stored Allow for this exact origin grants. Everything else, including
                    // kinds Bowser has no store for, is denied, as before the store existed.
                    let allowed = match (permission_for_kind(kind), permission_store.as_ref()) {
                        (Some(permission), Some(store)) => store.borrow().allows(&uri, permission),
                        _ => false,
                    };
                    let state = if allowed {
                        COREWEBVIEW2_PERMISSION_STATE_ALLOW
                    } else {
                        COREWEBVIEW2_PERMISSION_STATE_DENY
                    };
                    let _ = args.SetState(state);
                    // The store is the only record of a decision: WebView2 must not keep its own,
                    // or a later Block would not reach the next request.
                    if let Ok(args3) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() {
                        let _ = args3.SetSavesInProfile(false);
                    }
                    if let Some(log) = policy_event.as_ref() {
                        log(if allowed {
                            "permission_allowed"
                        } else {
                            "permission_denied"
                        });
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;

        let policy_event = events.policy_event.clone();
        let webview4: ICoreWebView2_4 = webview.cast()?;
        webview4.add_DownloadStarting(
            &DownloadStartingEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let _ = args.SetCancel(true);
                }
                if let Some(log) = policy_event.as_ref() {
                    log("download_cancelled");
                }
                Ok(())
            })),
            &mut token,
        )?;

        let policy_event = events.policy_event.clone();
        let popup_callback = events.new_window_requested.clone();
        webview.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    let mut initiated = BOOL::default();
                    let uri_result = args.Uri(&mut uri);
                    let initiated_result = args.IsUserInitiated(&mut initiated);
                    let uri_text = CoTaskMemPWSTR::from(uri).to_string();
                    let safe =
                        uri_result.is_ok() && initiated_result.is_ok() && initiated.as_bool();
                    let target = if safe {
                        Url::parse(&uri_text).ok()
                    } else {
                        None
                    };
                    let decision = target
                        .as_ref()
                        .map(|url| popup_decision(url, initiated.as_bool()))
                        .unwrap_or(PopupDecision::Block);
                    if decision == PopupDecision::NewTab {
                        let url = target.expect("new-tab decision requires a parsed target");
                        if let Some(callback) = popup_callback.as_ref() {
                            callback(tab_id, url);
                        }
                        if let Some(log) = policy_event.as_ref() {
                            log("popup_new_tab");
                        }
                    } else if let Some(log) = policy_event.as_ref() {
                        log("popup_blocked");
                    }
                    let _ = args.SetHandled(true);
                }
                Ok(())
            })),
            &mut token,
        )?;

        let external_webview: ICoreWebView2_18 = webview.cast()?;
        let external_policy_event = events.policy_event.clone();
        let external_handler: ICoreWebView2LaunchingExternalUriSchemeEventHandler =
            ExternalSchemeHandler(RefCell::new(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let _ = args.SetCancel(true);
                    if let Some(log) = external_policy_event.as_ref() {
                        log("external_protocol_blocked");
                    }
                }
                Ok(())
            })))
            .into();
        external_webview.add_LaunchingExternalUriScheme(&external_handler, &mut token)?;

        if let Ok(webview15) = webview.cast::<ICoreWebView2_15>() {
            let favicon_callback = events.favicon_changed.clone();
            let favicon_handler = FaviconChangedEventHandler::create(Box::new(move |sender, _| {
                if let (Some(sender), Some(callback)) = (sender, favicon_callback.as_ref()) {
                    if let Ok(webview15) = sender.cast::<ICoreWebView2_15>() {
                        let callback = callback.clone();
                        let completed =
                            GetFaviconCompletedHandler::create(Box::new(move |status, stream| {
                                if status.is_ok() {
                                    if let Some(bytes) = stream.and_then(read_favicon_stream) {
                                        callback(tab_id, bytes);
                                    }
                                }
                                Ok(())
                            }));
                        let _ =
                            webview15.GetFavicon(COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, &completed);
                    }
                }
                Ok(())
            }));
            let _ = webview15.add_FaviconChanged(&favicon_handler, &mut token);
        }

        if let Some(blocker) = blocker {
            let response_environment = environment.clone();
            let blocked_callback = events.blocked_changed.clone();
            let policy_log = events.policy_event.clone();
            let request_tab = block_tab.clone();
            webview.add_WebResourceRequested(
                &WebResourceRequestedEventHandler::create(Box::new(move |_sender, args| {
                    let Some(args) = args else {
                        return Ok(());
                    };
                    let request = args.Request()?;
                    let mut uri = PWSTR::null();
                    request.Uri(&mut uri)?;
                    let guard = CoTaskMemPWSTR::from(uri);
                    let block = REQUEST_URL.with(|buffer| {
                        let mut buffer = buffer.borrow_mut();
                        buffer.clear();
                        buffer.extend(
                            char::decode_utf16(uri.as_wide().iter().copied())
                                .map(|unit| unit.unwrap_or('\u{FFFD}')),
                        );
                        blocker
                            .borrow()
                            .should_block(&buffer, &request_tab.top_host.borrow())
                    });
                    drop(guard);
                    if block {
                        let response = response_environment.CreateWebResourceResponse(
                            None::<&IStream>,
                            403,
                            w!("Blocked by Bowser"),
                            w!(""),
                        )?;
                        args.SetResponse(&response)?;
                        let count = request_tab.count.get().saturating_add(1);
                        request_tab.count.set(count);
                        if let Some(callback) = blocked_callback.as_ref() {
                            callback(tab_id, count);
                        }
                        if let Some(log) = policy_log.as_ref() {
                            log("request_blocked");
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            // Subresource contexts only: never Document (main frames and iframes), never Stylesheet
            // or Font, which could break page layout for little privacy gain. The context list
            // is what excludes documents; the source-kind filter adds requests made by workers,
            // which the older call does not see.
            match webview.cast::<ICoreWebView2_22>() {
                Ok(webview22) => {
                    for context in BLOCKABLE_CONTEXTS {
                        webview22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                            w!("*"),
                            context,
                            COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                        )?;
                    }
                }
                Err(_) => {
                    for context in BLOCKABLE_CONTEXTS {
                        webview.AddWebResourceRequestedFilter(w!("*"), context)?;
                    }
                }
            }
        }

        controller.add_AcceleratorKeyPressed(
            &AcceleratorKeyPressedEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let mut key = 0u32;
                    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                    let _ = args.VirtualKey(&mut key);
                    let _ = args.KeyEventKind(&mut kind);
                    let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
                    let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
                    let alt = GetKeyState(VK_MENU.0 as i32) < 0;
                    if let Some(forward) = shortcut_forward(key, kind, ctrl, shift, alt) {
                        win32::post(
                            parent,
                            win32::WM_SHORTCUT_KEY,
                            WPARAM(forward.key as usize),
                            LPARAM(forward.modifier_bits()),
                        );
                        let _ = args.SetHandled(true);
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        if let Some(callback) = events.focus_changed.clone() {
            controller.add_GotFocus(
                &FocusChangedEventHandler::create(Box::new(move |_sender, _args| {
                    callback(tab_id);
                    Ok(())
                })),
                &mut token,
            )?;
        }

        if let Some(callback) = events.failed.as_ref() {
            let failure_callback = callback.clone();
            let render_callback = events.render_process_failed.clone();
            webview.add_ProcessFailed(
                &ProcessFailedEventHandler::create(Box::new(move |_sender, args| {
                    let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                    let render_exited = args.is_some_and(|args| {
                        args.ProcessFailedKind(&mut kind).is_ok()
                            && kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
                    });
                    match render_callback.as_ref() {
                        // A crashed page process is recoverable by reloading that tab.
                        Some(render) if render_exited => render(tab_id),
                        _ => failure_callback(Error::from(windows::Win32::Foundation::E_FAIL)),
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }
        Ok(WebView2Tab {
            environment: Some(environment.clone()),
            controller: Some(controller),
            webview: Some(webview),
            allowed_file,
            memory_target_api: None,
            visible: Rc::new(Cell::new(visible)),
            applied_bounds: Some(bounds),
        })
    }
}

fn tracking_prevention_value(
    level: TrackingPreventionLevel,
) -> COREWEBVIEW2_TRACKING_PREVENTION_LEVEL {
    match level {
        TrackingPreventionLevel::Balanced => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED,
        TrackingPreventionLevel::Strict => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT,
    }
}

fn tracking_prevention_name(level: COREWEBVIEW2_TRACKING_PREVENTION_LEVEL) -> &'static str {
    if level == COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED {
        "balanced"
    } else if level == COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT {
        "strict"
    } else {
        "other"
    }
}

/// The level as shown to the user, or `None` for a value this build does not know.
fn tracking_prevention_label(level: COREWEBVIEW2_TRACKING_PREVENTION_LEVEL) -> Option<&'static str> {
    if level == COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_NONE {
        Some("Off")
    } else if level == COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED {
        Some("Balanced")
    } else if level == COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT {
        Some("Strict")
    } else {
        None
    }
}

/// The stored-permission kind for a WebView2 request. Kinds Bowser keeps no store for return `None`.
fn permission_for_kind(kind: COREWEBVIEW2_PERMISSION_KIND) -> Option<Permission> {
    if kind == COREWEBVIEW2_PERMISSION_KIND_MICROPHONE {
        Some(Permission::Microphone)
    } else if kind == COREWEBVIEW2_PERMISSION_KIND_CAMERA {
        Some(Permission::Camera)
    } else if kind == COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION {
        Some(Permission::Geolocation)
    } else if kind == COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS {
        Some(Permission::Notifications)
    } else {
        None
    }
}

/// True for the certificate failures WebView2 reports for a navigation.
fn is_certificate_error(status: COREWEBVIEW2_WEB_ERROR_STATUS) -> bool {
    [
        COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_COMMON_NAME_IS_INCORRECT,
        COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_EXPIRED,
        COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_IS_INVALID,
        COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_REVOKED,
    ]
    .contains(&status)
}

type CookieResult = std::result::Result<Vec<ICoreWebView2Cookie>, String>;

fn cookie_manager(
    core: &RefCell<EngineCore>,
    tab: TabId,
) -> std::result::Result<ICoreWebView2CookieManager, String> {
    let webview = core
        .borrow()
        .tabs
        .get(&tab)
        .and_then(|tab| tab.webview.clone())
        .ok_or_else(|| "The tab has no page yet.".to_owned())?;
    let webview2 = webview
        .cast::<ICoreWebView2_2>()
        .map_err(|_| "This WebView2 runtime cannot read cookies.".to_owned())?;
    unsafe { webview2.CookieManager() }.map_err(|error| format!("{error}"))
}

type CookieDone = Box<dyn FnOnce(CookieResult)>;

/// Lists the cookies the browser would send to `url`. The callback runs on the UI thread later.
fn list_cookies(manager: &ICoreWebView2CookieManager, url: &url::Url, done: CookieDone) {
    let slot: Rc<RefCell<Option<CookieDone>>> = Rc::new(RefCell::new(Some(done)));
    let completion = slot.clone();
    let handler = GetCookiesCompletedHandler::create(Box::new(move |status, list| {
        let result = match (status, list) {
            (Ok(()), Some(list)) => cookies_in(&list),
            (Ok(()), None) => Ok(Vec::new()),
            (Err(error), _) => Err(format!("{error}")),
        };
        let done = completion.borrow_mut().take();
        if let Some(done) = done {
            done(result);
        }
        Ok(())
    }));
    let uri = HSTRING::from(url.as_str());
    if let Err(error) = unsafe { manager.GetCookies(&uri, &handler) } {
        let done = slot.borrow_mut().take();
        if let Some(done) = done {
            done(Err(format!("{error}")));
        }
    }
}

fn cookies_in(list: &ICoreWebView2CookieList) -> CookieResult {
    let mut count = 0u32;
    unsafe { list.Count(&mut count) }.map_err(|error| format!("{error}"))?;
    let mut found = Vec::with_capacity(count as usize);
    for index in 0..count {
        found.push(unsafe { list.GetValueAtIndex(index) }.map_err(|error| format!("{error}"))?);
    }
    Ok(found)
}

fn event(log: Option<&Rc<dyn Fn(&'static str)>>, name: &'static str) {
    if let Some(log) = log {
        log(name);
    }
}

fn enable_smart_screen(
    settings: &ICoreWebView2Settings,
    log: Option<&Rc<dyn Fn(&'static str)>>,
) -> Result<()> {
    let settings8 = match settings.cast::<ICoreWebView2Settings8>() {
        Ok(settings8) => settings8,
        Err(error) if error.code() == windows::Win32::Foundation::E_NOINTERFACE => {
            event(log, "smartscreen_default_enabled_unverified");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    unsafe { settings8.SetIsReputationCheckingRequired(true)? };
    let mut enabled = BOOL::default();
    unsafe { settings8.IsReputationCheckingRequired(&mut enabled)? };
    if !enabled.as_bool() {
        return Err(Error::new(
            windows::core::HRESULT(0x80070005u32 as i32),
            "WebView2 SmartScreen setting could not be enabled",
        ));
    }
    event(log, "smartscreen_setting_enabled_verified");
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrackingPolicyApplyResult {
    Verified,
    InitialReadFailed,
    Reverted,
    ProfileUnknown,
}

fn apply_tracking_prevention_transaction<E>(
    requested: COREWEBVIEW2_TRACKING_PREVENTION_LEVEL,
    mut read_level: impl FnMut() -> std::result::Result<COREWEBVIEW2_TRACKING_PREVENTION_LEVEL, E>,
    mut set_level: impl FnMut(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL) -> std::result::Result<(), E>,
    mut emit: impl FnMut(&'static str),
) -> TrackingPolicyApplyResult {
    let previous = match read_level() {
        Ok(level) => level,
        Err(_) => {
            emit("tracking_prevention_error");
            return TrackingPolicyApplyResult::InitialReadFailed;
        }
    };

    if set_level(requested).is_err() {
        emit("tracking_prevention_error");
        return restore_tracking_prevention_level(
            previous,
            &mut read_level,
            &mut set_level,
            &mut emit,
        );
    }
    emit("tracking_prevention_applied");

    match read_level() {
        Ok(effective) => {
            emit(match tracking_prevention_name(effective) {
                "balanced" => "tracking_prevention_readback_balanced",
                "strict" => "tracking_prevention_readback_strict",
                _ => "tracking_prevention_readback_other",
            });
            if effective == requested {
                TrackingPolicyApplyResult::Verified
            } else {
                emit("tracking_prevention_mismatch");
                restore_tracking_prevention_level(
                    previous,
                    &mut read_level,
                    &mut set_level,
                    &mut emit,
                )
            }
        }
        Err(_) => {
            emit("tracking_prevention_error");
            restore_tracking_prevention_level(previous, &mut read_level, &mut set_level, &mut emit)
        }
    }
}

fn restore_tracking_prevention_level<E>(
    previous: COREWEBVIEW2_TRACKING_PREVENTION_LEVEL,
    read_level: &mut impl FnMut() -> std::result::Result<COREWEBVIEW2_TRACKING_PREVENTION_LEVEL, E>,
    set_level: &mut impl FnMut(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL) -> std::result::Result<(), E>,
    emit: &mut impl FnMut(&'static str),
) -> TrackingPolicyApplyResult {
    if set_level(previous).is_err() {
        emit("tracking_prevention_profile_unknown");
        return TrackingPolicyApplyResult::ProfileUnknown;
    }
    match read_level() {
        Ok(effective) if effective == previous => {
            emit("tracking_prevention_restored");
            TrackingPolicyApplyResult::Reverted
        }
        Ok(_) => {
            emit("tracking_prevention_mismatch");
            emit("tracking_prevention_profile_unknown");
            TrackingPolicyApplyResult::ProfileUnknown
        }
        Err(_) => {
            emit("tracking_prevention_error");
            emit("tracking_prevention_profile_unknown");
            TrackingPolicyApplyResult::ProfileUnknown
        }
    }
}

fn apply_tracking_prevention(
    webview: &ICoreWebView2,
    requested: TrackingPreventionLevel,
    enabled: bool,
    log: Option<&Rc<dyn Fn(&'static str)>>,
) {
    event(
        log,
        match requested {
            TrackingPreventionLevel::Balanced => "tracking_prevention_requested_balanced",
            TrackingPreventionLevel::Strict => "tracking_prevention_requested_strict",
        },
    );
    event(
        log,
        if enabled {
            "tracking_prevention_enabled_true"
        } else {
            "tracking_prevention_enabled_false"
        },
    );
    if !enabled {
        event(log, "tracking_prevention_unsupported");
        return;
    }

    let profile = match webview
        .cast::<ICoreWebView2_13>()
        .and_then(|webview| unsafe { webview.Profile() })
        .and_then(|profile| profile.cast::<ICoreWebView2Profile3>())
    {
        Ok(profile) => profile,
        Err(error) => {
            if error.code() == windows::Win32::Foundation::E_NOINTERFACE {
                event(log, "tracking_prevention_unsupported");
            } else {
                event(log, "tracking_prevention_error");
            }
            return;
        }
    };
    let _ = apply_tracking_prevention_transaction(
        tracking_prevention_value(requested),
        || {
            let mut level = COREWEBVIEW2_TRACKING_PREVENTION_LEVEL(0);
            unsafe { profile.PreferredTrackingPreventionLevel(&mut level) }?;
            Ok(level)
        },
        |level| unsafe { profile.SetPreferredTrackingPreventionLevel(level) },
        |name| event(log, name),
    );
}

fn memory_target_value(target: MemoryUsageTarget) -> COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL {
    match target {
        MemoryUsageTarget::Low => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
        MemoryUsageTarget::Normal => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    }
}

fn ensure_user_data_dir(user_data_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(user_data_dir).map_err(|error| {
        Error::new(
            windows::core::HRESULT(
                (0x80070000u32 | error.raw_os_error().unwrap_or(5) as u32) as i32,
            ),
            format!(
                "Unable to create Bowser's WebView2 profile directory {}: {error}",
                user_data_dir.display()
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_security_policy_disables_unused_webview_capabilities() {
        let mut applied = Vec::new();
        apply_controller_security_policy(|setting| {
            applied.push(setting);
            Ok::<(), ()>(())
        })
        .unwrap();

        assert_eq!(
            applied,
            [
                ControllerSecuritySetting::WebMessages,
                ControllerSecuritySetting::HostObjects,
            ]
        );
    }

    #[test]
    fn controller_security_policy_stops_after_a_setting_failure() {
        let mut applied = Vec::new();
        let result = apply_controller_security_policy(|setting| {
            applied.push(setting);
            if setting == ControllerSecuritySetting::HostObjects {
                Err("setting failed")
            } else {
                Ok(())
            }
        });

        assert_eq!(result, Err("setting failed"));
        assert_eq!(
            applied,
            [
                ControllerSecuritySetting::WebMessages,
                ControllerSecuritySetting::HostObjects,
            ]
        );
    }

    #[test]
    fn password_autosave_policy_disables_and_verifies_the_setting() {
        assert!(password_autosave_disabled(false));
    }

    #[test]
    fn password_autosave_policy_fails_if_setting_cannot_be_disabled() {
        assert!(!password_autosave_disabled(true));
    }

    #[test]
    fn tracking_policy_does_not_write_when_initial_read_fails() {
        let operations = Rc::new(RefCell::new(Vec::new()));
        let events = Rc::new(RefCell::new(Vec::new()));
        let read_operations = operations.clone();
        let write_operations = operations.clone();
        let event_rows = events.clone();

        let result = apply_tracking_prevention_transaction(
            COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT,
            move || {
                read_operations.borrow_mut().push("read");
                Err(())
            },
            move |_| {
                write_operations.borrow_mut().push("write");
                Ok(())
            },
            move |name| event_rows.borrow_mut().push(name),
        );

        assert_eq!(result, TrackingPolicyApplyResult::InitialReadFailed);
        assert_eq!(*operations.borrow(), ["read"]);
        assert_eq!(*events.borrow(), ["tracking_prevention_error"]);
    }

    #[test]
    fn tracking_policy_reports_success_only_after_matching_readback() {
        let level = std::cell::Cell::new(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED);
        let events = Rc::new(RefCell::new(Vec::new()));
        let event_rows = events.clone();

        let result = apply_tracking_prevention_transaction(
            COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT,
            || Ok::<_, ()>(level.get()),
            |requested| {
                level.set(requested);
                Ok(())
            },
            move |name| event_rows.borrow_mut().push(name),
        );

        assert_eq!(result, TrackingPolicyApplyResult::Verified);
        assert_eq!(level.get(), COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT);
        assert_eq!(
            *events.borrow(),
            [
                "tracking_prevention_applied",
                "tracking_prevention_readback_strict"
            ]
        );
    }

    #[test]
    fn tracking_policy_rolls_back_after_failed_readback() {
        let level = std::cell::Cell::new(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED);
        let read_count = std::cell::Cell::new(0);
        let events = Rc::new(RefCell::new(Vec::new()));
        let event_rows = events.clone();

        let result = apply_tracking_prevention_transaction(
            COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT,
            || {
                let count = read_count.get() + 1;
                read_count.set(count);
                match count {
                    1 => Ok(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED),
                    2 => Err(()),
                    _ => Ok(level.get()),
                }
            },
            |requested| {
                level.set(requested);
                Ok(())
            },
            move |name| event_rows.borrow_mut().push(name),
        );

        assert_eq!(result, TrackingPolicyApplyResult::Reverted);
        assert_eq!(level.get(), COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED);
        assert!(events.borrow().contains(&"tracking_prevention_error"));
        assert!(events.borrow().contains(&"tracking_prevention_restored"));
        assert!(
            !events
                .borrow()
                .contains(&"tracking_prevention_profile_unknown")
        );
    }

    #[test]
    fn tracking_policy_rolls_back_after_mismatched_readback() {
        let level = std::cell::Cell::new(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED);
        let events = Rc::new(RefCell::new(Vec::new()));
        let event_rows = events.clone();

        let result = apply_tracking_prevention_transaction(
            COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT,
            || Ok::<_, ()>(level.get()),
            |requested| {
                if requested == COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT {
                    // Simulate a successful setter whose readback stays at the prior value.
                } else {
                    level.set(requested);
                }
                Ok(())
            },
            move |name| event_rows.borrow_mut().push(name),
        );

        assert_eq!(result, TrackingPolicyApplyResult::Reverted);
        assert_eq!(level.get(), COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED);
        assert!(events.borrow().contains(&"tracking_prevention_mismatch"));
        assert!(events.borrow().contains(&"tracking_prevention_restored"));
    }

    #[test]
    fn tracking_policy_marks_the_profile_unknown_when_rollback_fails() {
        let level = std::cell::Cell::new(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED);
        let read_count = std::cell::Cell::new(0);
        let write_count = std::cell::Cell::new(0);
        let events = Rc::new(RefCell::new(Vec::new()));
        let event_rows = events.clone();

        let result = apply_tracking_prevention_transaction(
            COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT,
            || {
                let count = read_count.get() + 1;
                read_count.set(count);
                if count == 1 {
                    Ok(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED)
                } else {
                    Err(())
                }
            },
            |requested| {
                let count = write_count.get() + 1;
                write_count.set(count);
                if count == 1 {
                    level.set(requested);
                    Ok(())
                } else {
                    Err(())
                }
            },
            move |name| event_rows.borrow_mut().push(name),
        );

        assert_eq!(result, TrackingPolicyApplyResult::ProfileUnknown);
        assert_eq!(level.get(), COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT);
        assert!(
            events
                .borrow()
                .contains(&"tracking_prevention_profile_unknown")
        );
        assert!(!events.borrow().contains(&"tracking_prevention_restored"));
    }

    #[test]
    fn tracking_prevention_levels_map_to_the_pinned_webview2_values() {
        assert_eq!(
            tracking_prevention_value(TrackingPreventionLevel::Balanced),
            COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED
        );
        assert_eq!(
            tracking_prevention_value(TrackingPreventionLevel::Strict),
            COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT
        );
        assert_eq!(
            tracking_prevention_name(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED),
            "balanced"
        );
        assert_eq!(
            tracking_prevention_name(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT),
            "strict"
        );
        assert_eq!(
            tracking_prevention_name(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_NONE),
            "other"
        );
    }

    #[test]
    fn memory_targets_map_to_the_pinned_webview2_values() {
        assert_eq!(
            memory_target_value(MemoryUsageTarget::Low),
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
        );
        assert_eq!(
            memory_target_value(MemoryUsageTarget::Normal),
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
        );
    }

    #[test]
    fn missing_memory_interface_is_distinct_from_a_query_failure() {
        assert!(matches!(
            memory_target_api_from_error(Error::from(windows::Win32::Foundation::E_NOINTERFACE)),
            MemoryTargetApi::Unsupported
        ));
        assert!(matches!(
            memory_target_api_from_error(Error::from(windows::Win32::Foundation::E_FAIL)),
            MemoryTargetApi::Error(_)
        ));
    }

    #[test]
    fn address_source_accepts_only_http_and_https_urls() {
        assert_eq!(
            web_url_from_source("https://example.com/path?q=1")
                .unwrap()
                .as_str(),
            "https://example.com/path?q=1"
        );
        assert_eq!(
            web_url_from_source("http://localhost:3000/")
                .unwrap()
                .as_str(),
            "http://localhost:3000/"
        );
        for source in [
            "about:blank",
            "file:///C:/secret",
            "javascript:alert(1)",
            "not a URL",
        ] {
            assert!(web_url_from_source(source).is_none(), "{source}");
        }
    }

    #[test]
    fn missing_runtime_message_has_install_guidance() {
        let path = std::env::temp_dir().join(format!(
            "bowser-missing-webview-runtime-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        let folder: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let error = query_runtime(PCWSTR(folder.as_ptr()))
            .expect_err("empty folder cannot contain a fixed WebView2 runtime");
        assert!(error.to_string().contains("Evergreen Runtime"));
        assert!(
            error
                .to_string()
                .contains("https://developer.microsoft.com/microsoft-edge/webview2/")
        );
        let _ = std::fs::remove_dir_all(path);
    }
    #[test]
    fn profile_creation_errors_are_actionable() {
        let path = std::env::temp_dir().join(format!(
            "bowser-unwritable-profile-parent-file-{}",
            std::process::id()
        ));
        std::fs::write(&path, "file blocks directory creation").unwrap();
        let child = path.join("profile");
        assert!(ensure_user_data_dir(&child).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn popup_policy_contains_only_user_initiated_http_and_https() {
        assert_eq!(
            popup_decision(&Url::parse("https://example.com").unwrap(), true),
            PopupDecision::NewTab
        );
        assert_eq!(
            popup_decision(&Url::parse("http://example.com").unwrap(), true),
            PopupDecision::NewTab
        );
        assert_eq!(
            popup_decision(&Url::parse("https://example.com").unwrap(), false),
            PopupDecision::Block
        );
        assert_eq!(
            popup_decision(&Url::parse("mailto:person@example.com").unwrap(), true),
            PopupDecision::Block
        );
    }

    #[test]
    fn navigation_schemes_are_fail_closed() {
        for value in ["https://example.com", "http://example.com"] {
            assert!(navigation_scheme_allowed(&Url::parse(value).unwrap()));
        }
        for value in [
            "file:///C:/secret",
            "data:text/html,hi",
            "javascript:alert(1)",
            "mailto:a@b.test",
            "tel:+44123",
        ] {
            assert!(!navigation_scheme_allowed(&Url::parse(value).unwrap()));
        }
    }

    #[test]
    fn frame_navigation_blocks_local_files_and_internal_browser_schemes() {
        for value in [
            "file:///C:/secret",
            "javascript:alert(1)",
            "edge://settings",
            "chrome://settings",
            "devtools://devtools/",
            "vbscript:msgbox(1)",
        ] {
            assert!(
                !frame_navigation_scheme_allowed(&Url::parse(value).unwrap()),
                "{value} must be blocked in frames"
            );
        }
    }

    #[test]
    fn frame_navigation_keeps_normal_web_and_document_frames_working() {
        for value in [
            "https://example.com/frame",
            "http://localhost:3000/frame",
            "about:blank",
            "about:srcdoc",
            "blob:https://example.com/id",
            "data:text/html,frame",
        ] {
            assert!(
                frame_navigation_scheme_allowed(&Url::parse(value).unwrap()),
                "{value} must remain available to websites"
            );
        }
    }

    #[test]
    fn external_protocols_reach_the_explicit_cancel_handler_only() {
        let allowed_file = Url::parse("file:///C:/Bowser/fixture.html").unwrap();
        assert_eq!(
            navigation_start_policy(&Url::parse("https://example.com").unwrap(), None),
            NavigationStartPolicy::Allow
        );
        assert_eq!(
            navigation_start_policy(&allowed_file, Some(&allowed_file)),
            NavigationStartPolicy::Allow
        );
        for value in [
            "file:///C:/secret",
            "javascript:alert(1)",
            "data:text/html,hi",
            "about:blank",
            "blob:https://example.com/id",
            "edge://settings",
            "chrome://settings",
            "devtools://devtools/",
            "ms-appx-web://example/index.html",
            "vbscript:msgbox(1)",
        ] {
            assert_eq!(
                navigation_start_policy(&Url::parse(value).unwrap(), Some(&allowed_file)),
                NavigationStartPolicy::Block,
                "{value} must be blocked before WebView2 navigation"
            );
        }
        for value in [
            "mailto:person@example.com",
            "tel:+44123",
            "bowser-smoke://test",
        ] {
            assert_eq!(
                navigation_start_policy(&Url::parse(value).unwrap(), None),
                NavigationStartPolicy::ExternalProtocol,
                "{value} must reach the LaunchingExternalUriScheme cancellation handler"
            );
        }
    }

    #[test]
    fn benchmark_file_grant_can_authorize_only_its_exact_first_navigation() {
        let allowed = Url::parse("file:///C:/Bowser/benchmarks/fixtures/light.html").unwrap();
        let other_file = Url::parse("file:///C:/Users/charlie/secret.txt").unwrap();
        let pending = Cell::new(true);

        assert!(!consume_allowed_file_navigation(
            &other_file,
            Some(&allowed),
            &pending
        ));
        assert!(pending.get(), "a rejected URL must not consume the grant");
        assert!(consume_allowed_file_navigation(
            &allowed,
            Some(&allowed),
            &pending
        ));
        assert!(!consume_allowed_file_navigation(
            &allowed,
            Some(&allowed),
            &pending
        ));
        assert!(!pending.get());
        assert!(!consume_allowed_file_navigation(
            &other_file,
            Some(&allowed),
            &pending
        ));
    }

    fn forward(key: u32, ctrl: bool, shift: bool, alt: bool) -> Option<ShortcutForward> {
        Some(ShortcutForward {
            key,
            ctrl,
            shift,
            alt,
        })
    }

    #[test]
    fn history_shortcuts_accept_alt_keydown_and_system_keydown_only() {
        for kind in [
            COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
            COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
        ] {
            assert!(is_key_down(kind));
            assert_eq!(
                shortcut_forward(0x25, kind, false, false, true),
                forward(0x25, false, false, true)
            );
            assert_eq!(
                shortcut_forward(0x27, kind, false, false, true),
                forward(0x27, false, false, true)
            );
        }
        assert_eq!(
            shortcut_forward(
                0x25,
                COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
                false,
                false,
                false
            ),
            None
        );
        assert_eq!(
            shortcut_forward(
                0x41,
                COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
                false,
                false,
                true
            ),
            None
        );
        assert_eq!(
            shortcut_forward(0x25, COREWEBVIEW2_KEY_EVENT_KIND_KEY_UP, false, false, true),
            None
        );
        assert!(!is_key_down(COREWEBVIEW2_KEY_EVENT_KIND_KEY_UP));
        assert!(!is_key_down(COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_UP));
    }

    #[test]
    fn shortcut_forward_matches_command_table_on_key_down_only() {
        for kind in [
            COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
            COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
            COREWEBVIEW2_KEY_EVENT_KIND_KEY_UP,
            COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_UP,
        ] {
            for key in 0..=0xFF_u32 {
                for modifiers in 0..8_u8 {
                    let (ctrl, shift, alt) =
                        (modifiers & 1 != 0, modifiers & 2 != 0, modifiers & 4 != 0);
                    let expected = if is_key_down(kind)
                        && key != u32::from(VK_ESCAPE.0)
                        && shortcuts::command_for(key, ctrl, shift, alt).is_some()
                    {
                        forward(key, ctrl, shift, alt)
                    } else {
                        None
                    };
                    assert_eq!(
                        shortcut_forward(key, kind, ctrl, shift, alt),
                        expected,
                        "key {key:#04X} ctrl={ctrl} shift={shift} alt={alt}"
                    );
                }
            }
        }
    }

    #[test]
    fn shortcut_forward_forwards_common_browser_chords() {
        let down = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
        assert_eq!(
            shortcut_forward(0x54, down, true, false, false),
            forward(0x54, true, false, false)
        );
        assert_eq!(
            shortcut_forward(0x4C, down, true, false, false),
            forward(0x4C, true, false, false)
        );
        assert_eq!(
            shortcut_forward(0x74, down, false, false, false),
            forward(0x74, false, false, false)
        );
        assert_eq!(
            shortcut_forward(0x52, down, true, true, false),
            forward(0x52, true, true, false)
        );
    }

    #[test]
    fn shortcut_forward_never_swallows_page_editing_and_navigation_keys() {
        let down = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
        for (key, ctrl, shift, alt) in [
            (0x43, true, false, false),  // Ctrl+C
            (0x56, true, false, false),  // Ctrl+V
            (0x58, true, false, false),  // Ctrl+X
            (0x5A, true, false, false),  // Ctrl+Z
            (0x41, true, false, false),  // Ctrl+A
            (0x46, true, false, false),  // Ctrl+F
            (0x47, true, false, false),  // Ctrl+G
            (0x41, false, false, false), // A
            (0x20, false, false, false), // Space
            (0x0D, false, false, false), // Enter
            (0x08, false, false, false), // Backspace
            (0x2E, false, false, false), // Delete
            (0x09, false, false, false), // Tab
            (0x25, false, false, false), // Left
            (0x26, false, false, false), // Up
            (0x27, false, false, false), // Right
            (0x28, false, false, false), // Down
            (0x25, true, false, false),  // Ctrl+Left, word movement in text fields
            (0x25, false, true, false),  // Shift+Left, selection
            (0x21, false, false, false), // Page Up
            (0x22, false, false, false), // Page Down
            (0x7B, false, false, false), // F12
            (0x7B, true, false, false),  // Ctrl+F12
            (0x1B, false, false, false), // Escape, left to the page
        ] {
            assert_eq!(
                shortcut_forward(key, down, ctrl, shift, alt),
                None,
                "key {key:#04X} ctrl={ctrl} shift={shift} alt={alt}"
            );
        }
    }

    #[test]
    fn modifier_bits_follow_the_wm_shortcut_key_layout() {
        assert_eq!(
            forward(0x54, false, false, false).unwrap().modifier_bits(),
            0
        );
        assert_eq!(
            forward(0x54, true, false, false).unwrap().modifier_bits(),
            1
        );
        assert_eq!(forward(0x52, true, true, false).unwrap().modifier_bits(), 3);
        assert_eq!(
            forward(0x25, false, false, true).unwrap().modifier_bits(),
            4
        );
    }

    #[test]
    fn sleep_probe_only_sleeps_on_explicit_false() {
        assert!(!sleep_probe_blocks(true, "false"));
        assert!(!sleep_probe_blocks(true, " false\n"));
        assert!(sleep_probe_blocks(true, "true"));
        assert!(sleep_probe_blocks(true, ""));
        assert!(sleep_probe_blocks(true, "null"));
        // A script that failed to run says nothing about the page, so the tab stays awake.
        assert!(sleep_probe_blocks(false, "false"));
    }

    #[test]
    fn top_host_is_set_only_for_web_documents() {
        assert_eq!(
            top_host_for_source("https://example.com/a?b=1"),
            "example.com"
        );
        assert_eq!(
            top_host_for_source("http://news.example.org/"),
            "news.example.org"
        );
        assert_eq!(top_host_for_source("about:blank"), "");
        assert_eq!(top_host_for_source("data:text/html,hi"), "");
        assert_eq!(top_host_for_source("file:///C:/x.html"), "");
        assert_eq!(top_host_for_source("ms-browser-extension://x/"), "");
        assert_eq!(top_host_for_source("not a url"), "");
    }

    #[test]
    fn profile_extensions_follow_bowser_records() {
        assert_eq!(
            reconcile_action(None, false),
            ProfileExtensionAction::Remove
        );
        assert_eq!(reconcile_action(None, true), ProfileExtensionAction::Remove);
        assert_eq!(
            reconcile_action(Some(false), false),
            ProfileExtensionAction::Disable
        );
        assert_eq!(
            reconcile_action(Some(false), true),
            ProfileExtensionAction::Disable
        );
        assert_eq!(
            reconcile_action(Some(true), true),
            ProfileExtensionAction::Enable
        );
        // Enabled in the records but its folder is gone: it cannot run, so unload it.
        assert_eq!(
            reconcile_action(Some(true), false),
            ProfileExtensionAction::Remove
        );
    }

    #[test]
    fn extension_id_file_round_trips_and_drops_bad_lines() {
        let mut ids = HashMap::new();
        ids.insert(
            "ext-0123456789abcdef".to_owned(),
            "ahbcdefghijklmnopabcdefghijklmnop".to_owned(),
        );
        ids.insert("ext-00000000000000ff".to_owned(), "bnopqrs".to_owned());
        let text = format_extension_ids(&ids);
        assert_eq!(parse_extension_ids(&text), ids);

        let bad =
            "../escape\tabc\next-0123456789abcdef\t\next-0123456789abcdef\tab-cd\nno tab here\n";
        assert!(parse_extension_ids(bad).is_empty());
    }
}
