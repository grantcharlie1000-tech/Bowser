use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet, VecDeque},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use url::Url;
use webview2_com::{
    AcceleratorKeyPressedEventHandler, AddScriptToExecuteOnDocumentCreatedCompletedHandler,
    BrowserExtensionEnableCompletedHandler, BrowserExtensionRemoveCompletedHandler,
    CallDevToolsProtocolMethodCompletedHandler, ClearBrowsingDataCompletedHandler, CoTaskMemPWSTR,
    ContentLoadingEventHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    DOMContentLoadedEventHandler, DevToolsProtocolEventReceivedEventHandler,
    DocumentTitleChangedEventHandler, DownloadStartingEventHandler,
    ExecuteScriptCompletedHandler, FaviconChangedEventHandler, FocusChangedEventHandler,
    GetCookiesCompletedHandler, GetFaviconCompletedHandler, Microsoft::Web::WebView2::Win32::*,
    NavigationCompletedEventHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, PermissionRequestedEventHandler, ProcessFailedEventHandler,
    ProfileAddBrowserExtensionCompletedHandler, ProfileGetBrowserExtensionsCompletedHandler,
    SourceChangedEventHandler, TrySuspendCompletedHandler, WebMessageReceivedEventHandler,
    WebResourceRequestedEventHandler, ZoomFactorChangedEventHandler,
};
// --- split view ---
use webview2_com::{ContextMenuRequestedEventHandler, CustomItemSelectedEventHandler};
// --- end split view ---
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT, WPARAM},
        System::Com::IStream,
        UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_ESCAPE, VK_MENU, VK_SHIFT},
    },
    core::{BOOL, Error, HSTRING, Interface, PCWSTR, PWSTR, Ref, Result, implement, w},
};
use zeroize::{Zeroize, Zeroizing};

use super::{
    BlockedChangedCallback, BrowsingDataKind, BrowsingDataRange, EngineAdapter, EngineEvents,
    MemoryTargetApply, MemoryUsageTarget, PermissionsHandle, TrackingPreventionLevel,
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

/// What a window request becomes. Only http(s) targets ever open, as a new tab. A click or other
/// user gesture opens one. A script-opened window opens only when the opener's site has a stored
/// Allow for popups (`site_allows`); otherwise it is blocked, as before the store had that kind.
/// `target_blocked` (the target should_blocks against the opener's host) overrides both: an ad
/// target never opens a tab, whatever the gesture or stored allow.
pub fn popup_decision(
    uri: &Url,
    user_initiated: bool,
    site_allows: bool,
    target_blocked: bool,
) -> PopupDecision {
    if target_blocked {
        return PopupDecision::Block;
    }
    if (user_initiated || site_allows) && matches!(uri.scheme(), "http" | "https") {
        PopupDecision::NewTab
    } else {
        PopupDecision::Block
    }
}

const PIP_ELIGIBILITY_SCRIPT: &str = "(()=>{if(!document.pictureInPictureEnabled)return false;return Array.from(document.querySelectorAll('video')).some(v=>{const r=v.getBoundingClientRect(),s=getComputedStyle(v);return !v.paused&&!v.ended&&v.readyState>=2&&!v.disablePictureInPicture&&r.width>0&&r.height>0&&s.visibility!=='hidden'&&s.display!=='none'})})()";

const PIP_TOGGLE_PARAMETERS: &str = r#"{"expression":"(async()=>{if(!document.pictureInPictureEnabled)return false;if(document.pictureInPictureElement){await document.exitPictureInPicture();return true}const eligible=v=>{const r=v.getBoundingClientRect(),s=getComputedStyle(v);return !v.paused&&!v.ended&&v.readyState>=2&&!v.disablePictureInPicture&&r.width>0&&r.height>0&&s.visibility!=='hidden'&&s.display!=='none'},videos=Array.from(document.querySelectorAll('video')).filter(eligible).sort((a,b)=>b.getBoundingClientRect().width*b.getBoundingClientRect().height-a.getBoundingClientRect().width*a.getBoundingClientRect().height),video=videos[0];if(!video)return false;const aborter=new AbortController(),close=()=>{if(document.pictureInPictureElement===video)void document.exitPictureInPicture().catch(()=>{})};video.addEventListener('pause',close,{signal:aborter.signal});video.addEventListener('ended',close,{signal:aborter.signal});video.addEventListener('leavepictureinpicture',()=>aborter.abort(),{once:true});try{await video.requestPictureInPicture();return true}catch(error){aborter.abort();throw error}})()","awaitPromise":true,"returnByValue":true,"userGesture":true}"#;

const PIP_EXIT_PARAMETERS: &str = r#"{"expression":"document.pictureInPictureElement?document.exitPictureInPicture().then(()=>true):false","awaitPromise":true,"returnByValue":true}"#;

fn query_picture_in_picture_eligibility(webview: &ICoreWebView2, done: Box<dyn FnOnce(bool)>) {
    let result = Rc::new(RefCell::new(Some(done)));
    let completion = result.clone();
    let handler = ExecuteScriptCompletedHandler::create(Box::new(move |status, value| {
        let available = status.is_ok() && value.to_string().trim() == "true";
        if let Some(done) = completion.borrow_mut().take() {
            done(available);
        }
        Ok(())
    }));
    let script = CoTaskMemPWSTR::from(PIP_ELIGIBILITY_SCRIPT);
    if unsafe { webview.ExecuteScript(*script.as_ref().as_pcwstr(), &handler) }.is_err()
        && let Some(done) = result.borrow_mut().take()
    {
        done(false);
    }
}

/// Longest answer taken back from a reader script. A valid result is under 50 characters, so a
/// longer one is not from this script and is dropped without being read.
const READER_RESULT_LIMIT: usize = 64;

/// Runs a reader script in a page, only while the live address is http(s). `done` gets the raw
/// `ExecuteScript` result, or `None` when the page is not scriptable, the call failed, or the answer
/// is oversized. It never runs for `bowser-ui:` or any other scheme: the address is read from the
/// WebView at call time, so a navigation racing the request cannot redirect it.
fn run_reader_script(
    webview: &ICoreWebView2,
    script: &str,
    done: Box<dyn FnOnce(Option<String>)>,
) {
    run_capped_script(webview, script, READER_RESULT_LIMIT, done);
}

/// The same for a page-translation script, whose answer is a batch of text.
fn run_translate_script(
    webview: &ICoreWebView2,
    script: &str,
    done: Box<dyn FnOnce(Option<String>)>,
) {
    run_capped_script(webview, script, crate::translate::MAX_ANSWER_BYTES, done);
}

/// The same for the feed-detection script, whose answer is a short list of `<link>` tags.
fn run_feed_script(
    webview: &ICoreWebView2,
    script: &str,
    done: Box<dyn FnOnce(Option<String>)>,
) {
    run_capped_script(webview, script, crate::app::MAX_FEED_ANSWER_BYTES, done);
}

fn run_capped_script(
    webview: &ICoreWebView2,
    script: &str,
    limit: usize,
    done: Box<dyn FnOnce(Option<String>)>,
) {
    if current_web_url(webview).is_none() {
        done(None);
        return;
    }
    let slot = Rc::new(RefCell::new(Some(done)));
    let completion = slot.clone();
    let handler = ExecuteScriptCompletedHandler::create(Box::new(move |status, value| {
        let answer = status
            .is_ok()
            .then(|| value.to_string())
            .filter(|text| text.len() <= limit);
        if let Some(done) = completion.borrow_mut().take() {
            done(answer);
        }
        Ok(())
    }));
    let wide = CoTaskMemPWSTR::from(script);
    if unsafe { webview.ExecuteScript(*wide.as_ref().as_pcwstr(), &handler) }.is_err()
        && let Some(done) = slot.borrow_mut().take()
    {
        done(None);
    }
}

// --- page tools (T7) ---
/// Calls a capture DevTools method. Only the two methods page capture needs are allowed, only on a page
/// whose live address is http(s), and an answer past the capture limit is dropped unread.
fn devtools_capture_call(
    webview: &ICoreWebView2,
    method: &str,
    params: &str,
    done: Box<dyn FnOnce(Option<String>)>,
) {
    if !matches!(method, crate::capture::METHOD_METRICS | crate::capture::METHOD_SHOT)
        || current_web_url(webview).is_none()
    {
        done(None);
        return;
    }
    let slot = Rc::new(RefCell::new(Some(done)));
    let completion = slot.clone();
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |error, result| {
        let answer = (error.is_ok()
            && !devtools_protocol_error(&result)
            && result.len() <= crate::page_tools::MAX_SLICE_BASE64 + 4096)
            .then_some(result);
        if let Some(done) = completion.borrow_mut().take() {
            done(answer);
        }
        Ok(())
    }));
    let method = HSTRING::from(method);
    let params = HSTRING::from(params);
    let queued = unsafe {
        webview.CallDevToolsProtocolMethod(PCWSTR(method.as_ptr()), PCWSTR(params.as_ptr()), &handler)
    };
    if queued.is_err()
        && let Some(done) = slot.borrow_mut().take()
    {
        done(None);
    }
}

/// Runs a script that needs no answer in whatever the tab shows now.
fn run_quiet_script(webview: &ICoreWebView2, script: &str) {
    let wide = CoTaskMemPWSTR::from(script);
    let handler = ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(())));
    let _ = unsafe { webview.ExecuteScript(*wide.as_ref().as_pcwstr(), &handler) };
}

/// Makes one tab match Settings > Page tools: Ctrl+scroll zoom, and the spatial navigation script (added
/// for every document the tab loads from now on and run in the one it shows, or taken away again).
/// `spatial_id` holds the id WebView2 gave the added script while it is in place.
fn apply_page_tools_to(webview: &ICoreWebView2, spatial_id: &Rc<RefCell<Option<String>>>) {
    if let Ok(settings) = unsafe { webview.Settings() } {
        let _ = unsafe { settings.SetIsZoomControlEnabled(crate::page_tools::wheel_zoom_on()) };
    }
    let want = crate::page_tools::spatial_on();
    let have = spatial_id.borrow().is_some();
    if want && !have {
        // An empty id marks "being added", so a second apply before the answer does not add it twice.
        *spatial_id.borrow_mut() = Some(String::new());
        let slot = spatial_id.clone();
        let target = webview.clone();
        let script = CoTaskMemPWSTR::from(crate::page_tools::SPATIAL_SCRIPT);
        let handler = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(move |status, id| {
            if status.is_err() {
                *slot.borrow_mut() = None;
            } else if crate::page_tools::spatial_on() {
                *slot.borrow_mut() = Some(id);
            } else {
                // Switched off while it was being added.
                *slot.borrow_mut() = None;
                let wide = HSTRING::from(id);
                let _ = unsafe { target.RemoveScriptToExecuteOnDocumentCreated(PCWSTR(wide.as_ptr())) };
            }
            Ok(())
        }));
        if unsafe { webview.AddScriptToExecuteOnDocumentCreated(*script.as_ref().as_pcwstr(), &handler) }.is_err() {
            *spatial_id.borrow_mut() = None;
            return;
        }
        run_quiet_script(webview, crate::page_tools::SPATIAL_SCRIPT);
    } else if !want && have {
        if let Some(id) = spatial_id.borrow_mut().take().filter(|id| !id.is_empty()) {
            let wide = HSTRING::from(id);
            let _ = unsafe { webview.RemoveScriptToExecuteOnDocumentCreated(PCWSTR(wide.as_ptr())) };
        }
        run_quiet_script(webview, crate::page_tools::SPATIAL_OFF_SCRIPT);
    }
}
// --- end page tools ---

fn toggle_native_picture_in_picture(webview: &ICoreWebView2) {
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
    let params = HSTRING::from(PIP_TOGGLE_PARAMETERS);
    let _ = unsafe {
        webview.CallDevToolsProtocolMethod(
            w!("Runtime.evaluate"),
            PCWSTR(params.as_ptr()),
            &handler,
        )
    };
}

fn exit_native_picture_in_picture(webview: &ICoreWebView2) {
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
    let params = HSTRING::from(PIP_EXIT_PARAMETERS);
    let _ = unsafe {
        webview.CallDevToolsProtocolMethod(
            w!("Runtime.evaluate"),
            PCWSTR(params.as_ptr()),
            &handler,
        )
    };
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
/// so it is left to the page. A setting-gated chord (`gated`) is mapped only while its setting is on.
#[cfg_attr(not(test), allow(dead_code))]
fn shortcut_forward(
    key: u32,
    kind: COREWEBVIEW2_KEY_EVENT_KIND,
    ctrl: bool,
    shift: bool,
    alt: bool,
    gated: shortcuts::GatedChords,
) -> Option<ShortcutForward> {
    shortcut_forward_on_page(key, kind, ctrl, shift, alt, gated, || None)
}

/// [`shortcut_forward`] for a page whose host `page_host` reads only when a site list exists: a site
/// that prioritises its own shortcuts keeps every key but the browser priority list
/// (`crate::commands`), and a typing key is never a shortcut from a page.
fn shortcut_forward_on_page(
    key: u32,
    kind: COREWEBVIEW2_KEY_EVENT_KIND,
    ctrl: bool,
    shift: bool,
    alt: bool,
    gated: shortcuts::GatedChords,
    page_host: impl Fn() -> Option<String>,
) -> Option<ShortcutForward> {
    if !is_key_down(kind) || key == u32::from(VK_ESCAPE.0) {
        return None;
    }
    let live = crate::commands::live();
    let host = if live.store.sites.is_empty() { None } else { page_host() };
    let context = crate::commands::ResolveContext {
        gated,
        source: crate::commands::Source::Page,
        typing: false,
        host: host.as_deref(),
    };
    crate::commands::resolve_in(&live, key, ctrl, shift, alt, &context)?;
    Some(ShortcutForward {
        key,
        ctrl,
        shift,
        alt,
    })
}

/// An unmodified Escape going down. A held key repeats, which is harmless: leaving reader view
/// twice is the same as once.
fn is_escape_press(
    key: u32,
    kind: COREWEBVIEW2_KEY_EVENT_KIND,
    ctrl: bool,
    shift: bool,
    alt: bool,
) -> bool {
    is_key_down(kind) && key == u32::from(VK_ESCAPE.0) && !ctrl && !shift && !alt
}

fn is_key_down(kind: COREWEBVIEW2_KEY_EVENT_KIND) -> bool {
    matches!(
        kind,
        COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN | COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
    )
}

pub(super) type ExternalSchemeCallback = Box<
    dyn FnMut(
        Option<ICoreWebView2>,
        Option<ICoreWebView2LaunchingExternalUriSchemeEventArgs>,
    ) -> Result<()>,
>;

#[implement(ICoreWebView2LaunchingExternalUriSchemeEventHandler)]
pub(super) struct ExternalSchemeHandler(pub(super) RefCell<ExternalSchemeCallback>);

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
pub(super) enum ControllerSecuritySetting {
    WebMessages,
    HostObjects,
}

pub(super) fn apply_controller_security_policy<E>(
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

pub(crate) fn password_login_messages_enabled(vault_enabled: bool, guest_profile: bool) -> bool {
    vault_enabled && !guest_profile
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
        | "view-source"
        // The HTML chrome's scheme is registered in the shared environment in HTML chrome mode.
        // Tabs must never load it, whatever the mode.
        | "bowser-ui" => NavigationStartPolicy::Block,
        _ => NavigationStartPolicy::ExternalProtocol,
    }
}

/// Navigation policy of a Bowser-hosted extension page (popup or options). The grant is for the
/// one URL Bowser built from the extension's manifest and is used up by the first navigation to it.
/// It is not a scheme unblock: tabs keep `navigation_start_policy`, which refuses every extension
/// scheme.
pub(super) struct HostedPagePolicy {
    granted: Url,
    pending: Cell<bool>,
}

impl HostedPagePolicy {
    /// `None` unless `url` is exactly a URL [`crate::extensions::page_url`] builds.
    pub(super) fn new(url: &Url) -> Option<Self> {
        crate::extensions::is_page_url(url).then(|| Self {
            granted: url.clone(),
            pending: Cell::new(true),
        })
    }

    pub(super) fn allow_navigation(&self, uri: &Url) -> bool {
        consume_allowed_file_navigation(uri, Some(&self.granted), &self.pending)
    }

    /// Frames may be blank or the granted extension's own pages, nothing else.
    pub(super) fn allow_frame(&self, uri: &Url) -> bool {
        (uri.scheme() == "about" && matches!(uri.path(), "blank" | "srcdoc"))
            || (crate::extensions::is_page_url(uri) && uri.host_str() == self.granted.host_str())
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
    (0..=max_units).find(|&index| unsafe { *value.0.add(index) } == 0)
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

    fn owned_text(&self) -> String {
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
    unsafe { bounded_wide_len(source_pointer, 4096) }?;
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

// --- tab options ---
/// Reads a captured page image back out of the memory stream it was written to.
fn read_capture_stream(stream: &IStream) -> Option<Vec<u8>> {
    let mut result = Vec::new();
    unsafe { stream.Seek(0, windows::Win32::System::Com::STREAM_SEEK_SET, None) }.ok()?;
    loop {
        let mut chunk = [0u8; 16384];
        let mut read = 0u32;
        unsafe { stream.Read(chunk.as_mut_ptr().cast(), chunk.len() as u32, Some(&mut read)).ok().ok()? };
        if read == 0 {
            break;
        }
        if result.len().saturating_add(read as usize) > crate::thumbnail::MAX_CAPTURE_BYTES {
            return None;
        }
        result.extend_from_slice(&chunk[..read as usize]);
    }
    (!result.is_empty()).then_some(result)
}

/// The working set of the page process behind each tab: a renderer process whose frames include the
/// tab's own address belongs to that tab. Tabs of the same address share an answer, and a tab whose
/// process cannot be told apart is left out, never guessed.
fn renderer_memory(infos: &ICoreWebView2ProcessExtendedInfoCollection, sources: &[(TabId, String)]) -> Vec<(TabId, u64)> {
    let mut count = 0u32;
    if unsafe { infos.Count(&mut count) }.is_err() {
        return Vec::new();
    }
    let mut best: HashMap<TabId, u64> = HashMap::new();
    for index in 0..count.min(512) {
        let Ok(info) = (unsafe { infos.GetValueAtIndex(index) }) else { continue };
        let Ok(process) = (unsafe { info.ProcessInfo() }) else { continue };
        let mut kind = COREWEBVIEW2_PROCESS_KIND::default();
        let mut pid = 0i32;
        if unsafe { process.Kind(&mut kind) }.is_err() || kind != COREWEBVIEW2_PROCESS_KIND_RENDERER || unsafe { process.ProcessId(&mut pid) }.is_err() || pid <= 0 {
            continue;
        }
        let Ok(frames) = (unsafe { info.AssociatedFrameInfos() }) else { continue };
        let Ok(iterator) = (unsafe { frames.GetIterator() }) else { continue };
        let mut owned: Vec<TabId> = Vec::new();
        loop {
            let mut has = BOOL::default();
            if unsafe { iterator.HasCurrent(&mut has) }.is_err() || !has.as_bool() {
                break;
            }
            if let Ok(frame) = unsafe { iterator.GetCurrent() } {
                let mut source = PWSTR::null();
                if unsafe { frame.Source(&mut source) }.is_ok() {
                    let source = CoTaskMemPWSTR::from(source).to_string();
                    owned.extend(sources.iter().filter(|(_, tab)| *tab == source).map(|(id, _)| *id));
                }
            }
            let mut more = BOOL::default();
            if unsafe { iterator.MoveNext(&mut more) }.is_err() || !more.as_bool() {
                break;
            }
        }
        if owned.is_empty() {
            continue;
        }
        let Some(bytes) = process_working_set(pid as u32) else { continue };
        for id in owned {
            let entry = best.entry(id).or_insert(0);
            *entry = (*entry).max(bytes);
        }
    }
    best.into_iter().collect()
}

/// The working set of process `pid` in bytes, when the process can be queried.
fn process_working_set(pid: u32) -> Option<u64> {
    use windows::Win32::{
        Foundation::CloseHandle,
        System::{
            ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
            Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut counters = PROCESS_MEMORY_COUNTERS { cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
        let ok = K32GetProcessMemoryInfo(handle, &mut counters, counters.cb).as_bool();
        let _ = CloseHandle(handle);
        ok.then_some(counters.WorkingSetSize as u64)
    }
}
// --- end tab options ---

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

/// Per-tab state for the request blocker: the top-level host of the current navigation, the
/// number of requests blocked since the document started loading, and the (registrable domain,
/// blocking, player-response rewrite) signature last pushed to DevTools as
/// `Network.setBlockedURLs` and `Fetch.enable`, so a same-domain navigation does not rebuild and
/// resend the pattern lists.
#[derive(Default)]
struct TabBlockState {
    top_host: RefCell<String>,
    count: Cell<u32>,
    doc_filter: RefCell<Option<(String, crate::blocker::Level, bool, u32)>>,
    /// `Fetch.requestPaused` is subscribed, so `Fetch.enable` will not pause requests nobody answers.
    fetch_listener: Cell<bool>,
    /// `Network.enable` has been accepted, so later refreshes only update `setBlockedURLs`.
    network_enabled: Cell<bool>,
    /// The last cosmetic script sent to this tab installed a style, so a page with no cosmetic
    /// CSS still has one to remove.
    cosmetic_installed: Cell<bool>,
}

/// Escapes `value` for embedding as a double-quoted JavaScript or JSON string literal. Defence in
/// depth: callers only ever pass cosmetic CSS (whose selector grammar already forbids quotes and
/// backslashes, see `valid_cosmetic_selector`) or DevTools URL patterns built from rule hosts, so
/// none of this should ever fire in practice, but string-building code must not depend on that.
fn json_string_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '<' => out.push_str("\\u003c"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

const COSMETIC_STYLE_ID: &str = "bowser-cosmetic";

/// Removes the cosmetic `<style>` element, if present. Safe to run on a document that never had
/// one.
const COSMETIC_REMOVE_SCRIPT: &str =
    "(()=>{const s=document.getElementById('bowser-cosmetic');if(s)s.remove()})()";

/// Idempotent installer: creates `<style id="bowser-cosmetic">` on `documentElement` if it is not
/// already there, then sets its `textContent` to `css`. Safe to run more than once per document.
fn cosmetic_install_script(css: &str) -> String {
    format!(
        "(()=>{{let s=document.getElementById('{COSMETIC_STYLE_ID}');\
         if(!s){{s=document.createElement('style');s.id='{COSMETIC_STYLE_ID}';\
         document.documentElement.appendChild(s)}}s.textContent={}}})()",
        json_string_literal(css)
    )
}

/// `Network.setBlockedURLs` parameters. `urlPatterns` is what current Edge blocks with.
/// The legacy `urls` wildcard list is a fallback only: it matches the host text anywhere in
/// the URL, including a first-party path or query, so it is not sent beside `urlPatterns`.
fn blocked_urls_params(patterns: &[String]) -> String {
    let mut json = String::from("{\"urlPatterns\":[");
    for (index, pattern) in patterns.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str("{\"urlPattern\":");
        json.push_str(&json_string_literal(pattern));
        json.push_str(",\"block\":true}");
    }
    json.push_str("]}");
    json
}

/// Older `Network.setBlockedURLs` `urls` list, used only when `urlPatterns` is rejected.
fn blocked_urls_legacy_params(patterns: &[String]) -> String {
    let mut json = String::from("{\"urls\":[");
    for (index, pattern) in patterns.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str(&json_string_literal(pattern));
    }
    json.push_str("]}");
    json
}

/// `Fetch.enable` parameters. Only the YouTube player responses pause, at the Response stage.
/// Host rules are blocked by `Network.setBlockedURLs`, not by pausing every request here.
/// An empty list pauses nothing.
fn fetch_enable_params(responses: &[crate::blocker::youtube::ResponsePattern]) -> String {
    let mut json = String::from("{\"patterns\":[");
    for (index, response) in responses.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str("{\"urlPattern\":");
        json.push_str(&json_string_literal(response.url_pattern));
        if let Some(resource_type) = response.resource_type {
            json.push_str(",\"resourceType\":");
            json.push_str(&json_string_literal(resource_type));
        }
        json.push_str(",\"requestStage\":\"Response\"}");
    }
    json.push_str("]}");
    json
}

/// True when DevTools returned a protocol error as JSON (`{"code":...,"message":...}`)
/// rather than an empty success object. A failed HRESULT is reported separately by the caller.
fn devtools_protocol_error(result: &str) -> bool {
    let trimmed = result.trim();
    trimmed.starts_with('{') && trimmed.contains("\"code\"")
}

/// The first JSON string value of `key`, or `None` when it is absent.
fn json_string_field(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = json.find(&needle)? + needle.len();
    let mut out = String::new();
    let mut chars = json[start..].chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => out.push(chars.next().unwrap_or('\\')),
            '"' => return Some(out),
            _ => out.push(ch),
        }
    }
    None
}

fn call_devtools(webview: &ICoreWebView2, method: &str, params: &str) {
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
    let method = HSTRING::from(method);
    let params = HSTRING::from(params);
    let _ = unsafe {
        webview.CallDevToolsProtocolMethod(PCWSTR(method.as_ptr()), PCWSTR(params.as_ptr()), &handler)
    };
}

/// The WebView2 data kinds `kinds` stand for, all in one mask (Delete Browsing Data). Saved
/// passwords are not a kind here: they live in Bowser's own vault.
fn browsing_data_mask(kinds: &[BrowsingDataKind]) -> COREWEBVIEW2_BROWSING_DATA_KINDS {
    let mut mask = COREWEBVIEW2_BROWSING_DATA_KINDS(0);
    for kind in kinds {
        mask |= match kind {
            BrowsingDataKind::SiteData => {
                COREWEBVIEW2_BROWSING_DATA_KINDS_COOKIES | COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_DOM_STORAGE
            }
            BrowsingDataKind::Cache => {
                COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE | COREWEBVIEW2_BROWSING_DATA_KINDS_CACHE_STORAGE
            }
            BrowsingDataKind::History => COREWEBVIEW2_BROWSING_DATA_KINDS_BROWSING_HISTORY,
            BrowsingDataKind::Cookies => COREWEBVIEW2_BROWSING_DATA_KINDS_COOKIES,
            BrowsingDataKind::Storage => {
                COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_DOM_STORAGE | COREWEBVIEW2_BROWSING_DATA_KINDS_SERVICE_WORKERS
            }
            BrowsingDataKind::Autofill => COREWEBVIEW2_BROWSING_DATA_KINDS_GENERAL_AUTOFILL,
            BrowsingDataKind::SiteSettings => COREWEBVIEW2_BROWSING_DATA_KINDS_SETTINGS,
        };
    }
    mask
}

/// Tells the browsing profile which colour scheme sites should be shown (`prefers-color-scheme`).
fn apply_preferred_colour_scheme(webview: &ICoreWebView2, appearance: crate::settings::WebsiteAppearance) {
    use crate::settings::WebsiteAppearance;
    let scheme = match appearance {
        WebsiteAppearance::Auto => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO,
        WebsiteAppearance::Light => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT,
        WebsiteAppearance::Dark => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK,
    };
    let _ = unsafe {
        webview
            .cast::<ICoreWebView2_13>()
            .and_then(|webview| webview.Profile())
            .and_then(|profile| profile.SetPreferredColorScheme(scheme))
    };
}

/// Gives a new tab the website look in force: the profile's colour scheme, and Chromium's automatic
/// dark mode for the page when `force_dark_websites` is on. A look never changed from its defaults
/// sends nothing, so a profile that never changed it pays no cost; after a change (even back to Auto)
/// it is sent, so a private profile that kept an older Dark is put right (`website_look::new_tab_needs`).
fn start_website_look(webview: &ICoreWebView2) {
    let look = crate::website_look::current();
    if crate::website_look::new_tab_needs_now() {
        apply_preferred_colour_scheme(webview, look.appearance);
        call_devtools(webview, "Emulation.setAutoDarkModeOverride", look.auto_dark_params());
    }
}

/// Sent again for each document: forced dark mode is a per-page override, and a navigation to a new
/// process must not lose it. Nothing is sent while the option is off.
fn refresh_force_dark(webview: &ICoreWebView2) {
    let look = crate::website_look::current();
    if look.force_dark {
        call_devtools(webview, "Emulation.setAutoDarkModeOverride", look.auto_dark_params());
    }
}

/// `Network.enable` keeps almost no response bytes. The domain is on only so
/// `setBlockedURLs` is enforced; video bodies are not what this buffer is for.
const NETWORK_ENABLE_PARAMS: &str =
    r#"{"maxTotalBufferSize":1024,"maxResourceBufferSize":1024,"maxPostDataSize":0}"#;

/// Sends `urlPatterns`. If that call is rejected, sends the legacy `urls` list instead. Current
/// Edge accepts `urlPatterns`, so the legacy JSON is built only after a rejection.
fn push_blocked_urls(webview: &ICoreWebView2, patterns: Rc<[String]>) {
    let target = webview.clone();
    let rejected = Rc::clone(&patterns);
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |error, result| {
        if error.is_err() || devtools_protocol_error(&result) {
            let legacy = blocked_urls_legacy_params(&rejected);
            call_devtools(&target, "Network.setBlockedURLs", &legacy);
        }
        Ok(())
    }));
    let method = HSTRING::from("Network.setBlockedURLs");
    let params = HSTRING::from(blocked_urls_params(&patterns));
    let queued = unsafe {
        webview.CallDevToolsProtocolMethod(PCWSTR(method.as_ptr()), PCWSTR(params.as_ptr()), &handler)
    };
    if queued.is_err() {
        call_devtools(webview, "Network.setBlockedURLs", &blocked_urls_legacy_params(&patterns));
    }
}

/// Enables the Network domain once, then installs the blocked-URL list. A failed enable still
/// sends the list, in case the domain was already on.
fn enable_network_then_block(webview: &ICoreWebView2, block: &Rc<TabBlockState>, patterns: Vec<String>) {
    let patterns: Rc<[String]> = patterns.into();
    if block.network_enabled.get() {
        push_blocked_urls(webview, patterns);
        return;
    }
    let target = webview.clone();
    let armed = Rc::clone(block);
    let queued_patterns = Rc::clone(&patterns);
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |error, result| {
        if error.is_ok() && !devtools_protocol_error(&result) {
            armed.network_enabled.set(true);
        }
        push_blocked_urls(&target, Rc::clone(&queued_patterns));
        Ok(())
    }));
    let method = HSTRING::from("Network.enable");
    let params = HSTRING::from(NETWORK_ENABLE_PARAMS);
    let queued = unsafe {
        webview.CallDevToolsProtocolMethod(PCWSTR(method.as_ptr()), PCWSTR(params.as_ptr()), &handler)
    };
    if queued.is_err() {
        push_blocked_urls(webview, patterns);
    }
}

fn fetch_continue(webview: &ICoreWebView2, request_id: &str) {
    let params = format!("{{\"requestId\":{}}}", json_string_literal(request_id));
    call_devtools(webview, "Fetch.continueRequest", &params);
}

/// Largest response body the player-ads rewrite will hold; anything bigger passes through.
const MAX_REWRITE_BODY_BYTES: usize = 16 * 1024 * 1024;

/// A `Fetch.requestPaused` event at the Response stage: the request's URL, its HTTP status
/// (`None` when the response failed) and the response headers.
#[derive(Debug, Eq, PartialEq)]
struct PausedResponse {
    url: String,
    status: Option<u16>,
    headers: Vec<(String, String)>,
}

/// Which stage a `Fetch.requestPaused` event paused at. A Response-stage event that cannot be
/// read is still a response, so it is continued rather than failed as a blocked request.
#[derive(Debug, Eq, PartialEq)]
enum PausedStage {
    Request,
    Response(Option<PausedResponse>),
}

fn paused_stage(event: &str) -> PausedStage {
    use crate::blocker::json;
    let Some(members) = json::whole_object(event) else {
        return if event.contains("\"responseStatusCode\"")
            || event.contains("\"responseErrorReason\"")
        {
            PausedStage::Response(None)
        } else {
            PausedStage::Request
        };
    };
    let status = json::field(&members, "responseStatusCode");
    if status.is_none() && json::field(&members, "responseErrorReason").is_none() {
        return PausedStage::Request;
    }
    let url = json::field(&members, "request")
        .and_then(json::whole_object)
        .and_then(|request| json::field(&request, "url").and_then(json::string_value));
    let headers = json::field(&members, "responseHeaders")
        .and_then(json::array_elements)
        .map(|entries| {
            entries
                .into_iter()
                .filter_map(|entry| {
                    let entry = json::whole_object(entry)?;
                    Some((
                        json::string_value(json::field(&entry, "name")?)?,
                        json::string_value(json::field(&entry, "value")?)?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    PausedStage::Response(url.map(|url| PausedResponse {
        url,
        status: status.and_then(|status| status.parse().ok()),
        headers,
    }))
}

/// True for a JSON or HTML response that is UTF-8 (stated, or the default for JSON and for
/// YouTube's pages): the only bodies the rewrite will read and re-encode.
fn utf8_text_response(headers: &[(String, String)]) -> bool {
    let Some(content_type) = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.to_ascii_lowercase())
    else {
        return false;
    };
    let mut parts = content_type.split(';');
    let essence = parts.next().unwrap_or("").trim();
    matches!(essence, "application/json" | "text/html")
        && parts.all(|part| {
            part.trim()
                .strip_prefix("charset=")
                .is_none_or(|charset| charset.trim_matches('"') == "utf-8")
        })
}

/// The text of a `Fetch.getResponseBody` result, or `None` when it is not UTF-8 or is too big.
fn response_body_text(result: &str) -> Option<String> {
    use crate::blocker::json;
    let members = json::whole_object(result)?;
    let body = json::string_value(json::field(&members, "body")?)?;
    let text = if json::field(&members, "base64Encoded") == Some("true") {
        String::from_utf8(json::base64_decode(&body)?).ok()?
    } else {
        body
    };
    (text.len() <= MAX_REWRITE_BODY_BYTES).then_some(text)
}

/// `Fetch.fulfillRequest` parameters for a rewritten body. DevTools hands the body over already
/// decoded, so the headers that described the network bytes (length, content and transfer
/// encoding) are dropped; every other header, `Set-Cookie` included, is passed on as received.
fn fulfill_params(
    request_id: &str,
    status: u16,
    headers: &[(String, String)],
    body: &[u8],
) -> String {
    let mut json = format!(
        "{{\"requestId\":{},\"responseCode\":{status},\"responseHeaders\":[",
        json_string_literal(request_id)
    );
    let kept = headers.iter().filter(|(name, _)| {
        !["content-length", "content-encoding", "transfer-encoding"]
            .iter()
            .any(|dropped| name.eq_ignore_ascii_case(dropped))
    });
    for (index, (name, value)) in kept.enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str("{\"name\":");
        json.push_str(&json_string_literal(name));
        json.push_str(",\"value\":");
        json.push_str(&json_string_literal(value));
        json.push('}');
    }
    json.push_str("],\"body\":\"");
    json.push_str(&crate::blocker::json::base64_encode(body));
    json.push_str("\"}");
    json
}

/// Calls `method` for a paused request and continues the request unchanged if the call fails,
/// so a paused request is never left waiting.
fn call_devtools_or_continue(
    webview: &ICoreWebView2,
    method: &str,
    params: &str,
    request_id: &str,
) {
    let target = webview.clone();
    let id = request_id.to_owned();
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |error, _| {
        if error.is_err() {
            fetch_continue(&target, &id);
        }
        Ok(())
    }));
    let method = HSTRING::from(method);
    let params = HSTRING::from(params);
    let queued = unsafe {
        webview.CallDevToolsProtocolMethod(
            PCWSTR(method.as_ptr()),
            PCWSTR(params.as_ptr()),
            &handler,
        )
    };
    if queued.is_err() {
        fetch_continue(webview, request_id);
    }
}

/// Answers a request paused at the Response stage. Only the YouTube player-response patterns
/// pause there. A response the blocker strips is fulfilled with the same status and headers and
/// the stripped body. Every other one, and any failure along the way, continues unchanged.
fn answer_paused_response(
    webview: &ICoreWebView2,
    request_id: &str,
    paused: Option<PausedResponse>,
    block: &TabBlockState,
    blocker: &super::BlockerHandle,
    private: bool,
) {
    let rewrite = paused
        .filter(|paused| paused.status == Some(200) && utf8_text_response(&paused.headers))
        .and_then(|paused| {
            let top_host = block.top_host.borrow();
            let payload = blocker
                .borrow()
                .player_ads_rewrite(&paused.url, &top_host, private)?;
            Some((paused, payload))
        });
    let Some((paused, payload)) = rewrite else {
        fetch_continue(webview, request_id);
        return;
    };
    let target = webview.clone();
    let id = request_id.to_owned();
    let handler =
        CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |error, result| {
            let stripped = error
                .ok()
                .and_then(|()| response_body_text(&result))
                .and_then(|text| crate::blocker::youtube::strip_player_ads(&text, payload));
            match stripped {
                Some(body) => call_devtools_or_continue(
                    &target,
                    "Fetch.fulfillRequest",
                    &fulfill_params(&id, 200, &paused.headers, body.as_bytes()),
                    &id,
                ),
                None => fetch_continue(&target, &id),
            }
            Ok(())
        }));
    let params = format!("{{\"requestId\":{}}}", json_string_literal(request_id));
    let method = HSTRING::from("Fetch.getResponseBody");
    let params = HSTRING::from(params);
    let queued = unsafe {
        webview.CallDevToolsProtocolMethod(
            PCWSTR(method.as_ptr()),
            PCWSTR(params.as_ptr()),
            &handler,
        )
    };
    if queued.is_err() {
        fetch_continue(webview, request_id);
    }
}

/// Subscribes to `Fetch.requestPaused` once. Enabling Fetch before this would pause matching
/// requests with nobody to continue them, which hangs the load. Host rules are not paused here.
fn install_fetch_listener(
    webview: &ICoreWebView2,
    block: &Rc<TabBlockState>,
    blocker: super::BlockerHandle,
    private: bool,
) {
    let receiver = match unsafe { webview.GetDevToolsProtocolEventReceiver(w!("Fetch.requestPaused")) } {
        Ok(receiver) => receiver,
        Err(_) => return,
    };
    let listener = Rc::clone(block);
    let mut token = 0i64;
    let subscribed = unsafe {
        receiver.add_DevToolsProtocolEventReceived(
            &DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else {
                    return Ok(());
                };
                let mut raw = PWSTR::null();
                if args.ParameterObjectAsJson(&mut raw).is_err() {
                    return Ok(());
                }
                let json = CoTaskMemPWSTR::from(raw).to_string();
                let Some(request_id) = json_string_field(&json, "requestId") else {
                    return Ok(());
                };
                // The Response stage is only ever the YouTube player response: rewrite or
                // continue it. A Request-stage pause is not how hosts are blocked, so it is
                // continued rather than left hanging.
                if let PausedStage::Response(paused) = paused_stage(&json) {
                    answer_paused_response(
                        &sender,
                        &request_id,
                        paused,
                        &listener,
                        &blocker,
                        private,
                    );
                    return Ok(());
                }
                fetch_continue(&sender, &request_id);
                Ok(())
            })),
            &mut token,
        )
    };
    if subscribed.is_ok() {
        block.fetch_listener.set(true);
    }
}

fn note_blocked_request(
    block: &TabBlockState,
    tab_id: TabId,
    blocked_callback: &Option<BlockedChangedCallback>,
    policy_log: &Option<Rc<dyn Fn(&'static str)>>,
) {
    let count = block.count.get().saturating_add(1);
    block.count.set(count);
    if let Some(callback) = blocked_callback.as_ref() {
        callback(tab_id, count, block.top_host.borrow().clone());
    }
    if let Some(log) = policy_log.as_ref() {
        log("request_blocked");
    }
}

/// Counts renderer blocks from `Network.setBlockedURLs`. Those requests never reach
/// WebResourceRequested, so the 403 path does not see them. `blockedReason` `inspector` is
/// that block; other failures are left to the page.
fn install_loading_failed_listener(
    webview: &ICoreWebView2,
    block: &Rc<TabBlockState>,
    tab_id: TabId,
    blocked_callback: Option<BlockedChangedCallback>,
    policy_log: Option<Rc<dyn Fn(&'static str)>>,
) {
    let receiver = match unsafe { webview.GetDevToolsProtocolEventReceiver(w!("Network.loadingFailed")) }
    {
        Ok(receiver) => receiver,
        Err(_) => return,
    };
    let listener = Rc::clone(block);
    let mut token = 0i64;
    let _ = unsafe {
        receiver.add_DevToolsProtocolEventReceived(
            &DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_sender, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                let mut raw = PWSTR::null();
                if args.ParameterObjectAsJson(&mut raw).is_err() {
                    return Ok(());
                }
                let json = CoTaskMemPWSTR::from(raw).to_string();
                if json_string_field(&json, "blockedReason").as_deref() == Some("inspector") {
                    note_blocked_request(&listener, tab_id, &blocked_callback, &policy_log);
                }
                Ok(())
            })),
            &mut token,
        )
    };
}

/// Pushes this tab's DevTools blocked-URL list and the YouTube response-stage `Fetch` patterns.
/// `page_host`, when set, is the navigation that is starting; the committed top host is left
/// as it is until the document commits. A same signature is not sent again.
/// Called when the webview is configured (empty top host, so every listed host is blocked),
/// on main-frame NavigationStarting for the destination host, and after the document commits.
fn refresh_blocked_urls(
    webview: &ICoreWebView2,
    block: &Rc<TabBlockState>,
    blocker: &super::BlockerHandle,
    private: bool,
    page_host: Option<&str>,
) {
    let stored_host = block.top_host.borrow().clone();
    let top_host = page_host.unwrap_or(&stored_host);
    let state = blocker.borrow();
    // The level and the rule generation decide the patterns: a change of either (a level chosen
    // for the site, a list switched on or off) is sent even on the same site.
    let (level, generation) = state.filter_signature(top_host, private);
    // Only a YouTube page pauses player responses; anywhere else Fetch.enable pauses nothing.
    let responses = state.response_patterns(top_host, private);
    let signature = (
        crate::blocker::registrable_domain(top_host).to_owned(),
        level,
        !responses.is_empty(),
        generation,
    );
    if block.doc_filter.borrow().as_ref() == Some(&signature) {
        return;
    }
    let patterns = if private {
        state.blocked_url_patterns_private(top_host)
    } else {
        state.blocked_url_patterns(top_host)
    };
    drop(state);
    *block.doc_filter.borrow_mut() = Some(signature);
    // Host rules and plain path rules fail in the renderer once Network.enable has landed. The
    // 403 below answers what only the in-process matcher can decide (`request_filters`).
    enable_network_then_block(webview, block, patterns);
    if block.fetch_listener.get() {
        call_devtools(webview, "Fetch.enable", &fetch_enable_params(responses));
    }
}

/// The cosmetic script for a new document: install `css`, or remove the style when the page has
/// none and an earlier document on the tab `installed` one. `None` when there is nothing to do,
/// so a page with no cosmetic CSS after a page with none runs no script at all.
fn cosmetic_script(css: Option<&str>, installed: bool) -> Option<String> {
    match css {
        Some(css) => Some(cosmetic_install_script(css)),
        None if installed => Some(COSMETIC_REMOVE_SCRIPT.to_owned()),
        None => None,
    }
}

/// Installs or removes the cosmetic `<style>` element for this tab's current document, from the
/// blocker's cosmetic rules for its page host. Never runs for a non-http(s) document (an empty
/// top host).
fn apply_cosmetic_css(
    webview: &ICoreWebView2,
    block: &TabBlockState,
    blocker: &super::BlockerHandle,
    private: bool,
) {
    let top_host = block.top_host.borrow().clone();
    if top_host.is_empty() {
        return;
    }
    let state = blocker.borrow();
    let css = if private {
        state.cosmetic_css_for_host_private(&top_host)
    } else {
        state.cosmetic_css_for_host(&top_host)
    };
    let x_script = state.x_promoted_script(&top_host, private);
    drop(state);
    let installed = block.cosmetic_installed.replace(css.is_some());
    let Some(script) = cosmetic_script(css.as_deref(), installed) else {
        return;
    };
    // X promoted posts: the marker script runs after the style it depends on.
    let script = match x_script {
        Some(marker) => format!("{script};\n{marker}"),
        None => script,
    };
    // Fire-and-forget: a cosmetic touch-up must never gate on the completion handler (see
    // `engine_never_blocks_on_a_webview2_completion`).
    let handler = ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(())));
    let wide = CoTaskMemPWSTR::from(script.as_str());
    let _ = unsafe { webview.ExecuteScript(*wide.as_ref().as_pcwstr(), &handler) };
}

thread_local! {
    /// Reused UTF-8 copy of the request URL so matching does not allocate per request.
    static REQUEST_URL: RefCell<String> = RefCell::new(String::with_capacity(512));
}

/// The `WebResourceRequested` filters a tab installs on `ICoreWebView2_22`, as (URI, request
/// source kinds), from the blocker's `request_filters`. A page's own requests use those narrow
/// filters: host rules and plain path rules already fail in DevTools `setBlockedURLs`, so the
/// other requests never wait on the UI thread. Shared and service worker requests keep `*`: the
/// tab's DevTools session does not block for them, so the in-process matcher still sees them all.
/// A `*` from the blocker (a rule that can match any URL) keeps every request, as before.
fn request_filter_uris(
    filters: Vec<String>,
) -> Vec<(String, COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS)> {
    if filters.iter().any(|filter| filter == "*") {
        return vec![("*".to_owned(), COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL)];
    }
    let mut uris: Vec<_> = filters
        .into_iter()
        .map(|filter| (filter, COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_DOCUMENT))
        .collect();
    uris.push((
        "*".to_owned(),
        COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_SHARED_WORKER
            | COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_SERVICE_WORKER,
    ));
    uris
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
    /// The id of the spatial navigation script while it is added to this tab (page tools, T7).
    spatial_id: Rc<RefCell<Option<String>>>,
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
    /// Blocker for private tabs, which ignores the global switch and per-site exceptions.
    private_blocker: Option<super::BlockerHandle>,
    /// Tabs created InPrivate (see `crate::private`). Marked before the controller exists.
    private_tabs: HashSet<TabId>,
    active_tab: Option<TabId>,
    /// Tabs whose controllers currently occupy one of the visible page panes.
    visible_tabs: HashSet<TabId>,
    // --- split view ---
    /// Pane bounds of the split sides set by `set_visible_tabs`; a side whose controller is still
    /// being created is created there (not at the single-page `bounds`).
    pane_bounds: std::collections::HashMap<TabId, RECT>,
    // --- end split view ---
    target: MemoryUsageTarget,
    tabs: HashMap<TabId, WebView2Tab>,
    known_tabs: HashSet<TabId>,
    creating: HashSet<TabId>,
    prewarming: HashSet<TabId>,
    prewarm_waiting: Option<TabId>,
    waiting_controllers: VecDeque<TabId>,
    pending: HashMap<TabId, Url>,
    /// Form fields to POST to the `pending` address of a tab whose controller is still being made
    /// (a search with an engine that posts its query).
    /// Each body is kept with the address it was queued for, so it is sent only to that
    /// navigation (see `take_pending_post`).
    pending_post: HashMap<TabId, (Url, String)>,
    hidden_since: HashMap<TabId, Instant>,
    suspended: HashSet<TabId>,
    closed: bool,
    /// Whether the environment was created with browser extensions enabled.
    browser_extensions: bool,
    /// Why extensions were requested but not enabled (runtime below the floor), shown instead of
    /// the restart advice.
    extensions_refused: Option<String>,
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
    /// The environment was created with the `bowser-ui` scheme registered (HTML chrome mode), so
    /// the chrome views may be hosted in it.
    chrome_ui_scheme: bool,
    /// The one open Bowser-hosted extension page (popup or options), if any. Opening another
    /// closes it.
    extension_page: Option<super::extension_page::HostedPage>,
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

    /// The tab environment, for hosting the HTML chrome views in it (each in its own private
    /// WebView2 profile). `None` unless the environment was created with the `bowser-ui` scheme
    /// registered (`EngineExtras::chrome_ui_scheme`), or once the engine is closed.
    pub fn chrome_environment(&self) -> Option<ICoreWebView2Environment> {
        let state = self.core.borrow();
        if state.closed || !state.chrome_ui_scheme {
            return None;
        }
        state.environment.clone()
    }

    /// Opens an enabled, loaded extension's popup or options page in a Bowser-owned window. The URL
    /// is built here from the manifest in `folder` and the id WebView2 gave the extension; `show`
    /// is false only for the live probe.
    fn open_extension_page_with(
        &mut self,
        id: &str,
        folder: &Path,
        kind: crate::extensions::PageKind,
        title: &str,
        show: bool,
        done: Box<dyn FnOnce(std::result::Result<(), String>)>,
    ) {
        let (environment, webview_id, loaded) = {
            let state = self.core.borrow();
            (
                state.environment.clone().filter(|_| !state.closed),
                state.extension_ids.get(id).cloned(),
                state.extensions_loaded.contains_key(id),
            )
        };
        let Some(environment) = environment else {
            done(Err("Bowser is still opening this window. Try again in a moment.".to_owned()));
            return;
        };
        let (Some(webview_id), true) = (webview_id, loaded) else {
            done(Err("This extension is not loaded in this window.".to_owned()));
            return;
        };
        let url = match crate::extensions::extension_page_url(folder, kind, &webview_id) {
            Ok(url) => url,
            Err(error) => {
                done(Err(error.to_string()));
                return;
            }
        };
        let previous = self.core.borrow_mut().extension_page.take();
        if let Some(previous) = previous {
            previous.close();
        }
        let weak = Rc::downgrade(&self.core);
        super::extension_page::open(
            &environment,
            url,
            title,
            show,
            Box::new(move |result| match result {
                Ok(page) => match weak.upgrade() {
                    Some(core) if !core.borrow().closed => {
                        core.borrow_mut().extension_page = Some(page);
                        done(Ok(()));
                    }
                    _ => {
                        page.close();
                        done(Err("Bowser closed this window.".to_owned()));
                    }
                },
                Err(reason) => done(Err(reason)),
            }),
        );
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
        let (
            environment,
            parent,
            allowed_file,
            events,
            tracking_policy,
            blocker,
            permissions,
            private,
        ) = {
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
            let private = state.private_tabs.contains(&tab_id);
            if private && state.private_blocker.is_none() {
                // Fail closed: a private tab never runs without its always-on blocker.
                let failed = state.events.failed.clone();
                drop(state);
                let error = Error::from(windows::Win32::Foundation::E_ACCESSDENIED);
                if let Some(failed) = failed {
                    failed(error.clone());
                }
                return Err(error);
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
                TrackingPolicy {
                    level: crate::private::tracking_level(private, state.tracking_policy.level),
                    enabled: state.tracking_policy.enabled,
                },
                if private {
                    state.private_blocker.clone()
                } else {
                    state.blocker.clone()
                },
                state.permissions.clone(),
                private,
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
                    let mut state = core.borrow_mut();
                    // The active tab is what the window is showing. A blank tab opened from a
                    // split is active before it is listed in `visible_tabs`; creating that
                    // controller hidden leaves the page area empty after the search starts.
                    if !state.closed
                        && state.known_tabs.contains(&tab_id)
                        && state.active_tab == Some(tab_id)
                    {
                        state.visible_tabs.insert(tab_id);
                    }
                    (
                        !state.closed
                            && state.known_tabs.contains(&tab_id)
                            && (!state.prewarming.contains(&tab_id)
                                || state.active_tab == Some(tab_id)
                                || state.pending.contains_key(&tab_id)),
                        state.pane_bounds.get(&tab_id).copied().unwrap_or(state.bounds), // --- split view ---
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
                        private,
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
                let pending_post =
                    take_pending_post(&mut core.borrow_mut().pending_post, tab_id, pending.as_ref());
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
                // Extensions live in the browsing profile only: a private tab's webview is in the
                // private profile and must never be taken for it.
                let first_webview = core
                    .borrow()
                    .tabs
                    .get(&tab_id)
                    .filter(|_| !private)
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
                        && let Err(error) = match pending_post.as_deref() {
                            // A search that posts its query: the same request the page would send.
                            Some(body) => post_webview(webview.as_ref(), Some(&controller_environment), &url, body),
                            None => navigate_with_timing(
                                &completion_events,
                                webview.as_ref(),
                                allowed_file.as_ref(),
                                &url,
                            ),
                        }
                        && let Some(failed) = completion_events.failed.as_ref()
                    {
                        failed(error);
                    }
                }
                Ok(())
            },
        ));
        let created = if private {
            create_private_controller(&environment, parent, &handler)
        } else {
            unsafe { environment.CreateCoreWebView2Controller(parent, &handler) }
        };
        if let Err(error) = created {
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
            if state.closed {
                return Err(Error::from(windows::Win32::Foundation::E_POINTER));
            }
            // Register a tab the window is navigating even if `select_tab` was skipped.
            // `App::navigate_tab` refuses ids that are no longer open, so this does not
            // recreate a closed tab. Unknown-and-live used to return E_POINTER, which is
            // the "Navigation failed: Invalid pointer" dialog.
            state.known_tabs.insert(tab_id);
            state.pending.insert(tab_id, url.clone());
            // A new navigation supersedes a POST waiting for the controller; `navigate_tab_post`
            // stores its own body after this call.
            state.pending_post.remove(&tab_id);
            !state.creating.contains(&tab_id)
        };
        if should_create {
            Self::request_controller(&self.core, tab_id, false)?;
        }
        Ok(())
    }
}

/// Oldest runtime this build enables browser extensions on. Microsoft documents 120.0.2210.55 as the
/// first runtime with the API. 134.0.3124.44 is where MV3 service workers were fixed to register
/// (WebView2Feedback #5113); that floor is inferred from the issue, not documented by Microsoft.
const EXTENSION_RUNTIME_FLOOR: [u32; 4] = [134, 0, 3124, 44];

/// `a.b.c.d` from the start of a runtime version string. Preview channels add a word after the
/// number, which is ignored. Anything else is not a version.
fn parse_runtime_version(version: &str) -> Option<[u32; 4]> {
    let mut parts = version.split_whitespace().next()?.split('.');
    let mut numbers = [0u32; 4];
    for number in &mut numbers {
        let part = parts.next()?;
        if part.is_empty() || part.len() > 9 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *number = part.parse().ok()?;
    }
    parts.next().is_none().then_some(numbers)
}

/// Whether `version` is at or above [`EXTENSION_RUNTIME_FLOOR`]. An unreadable version is not.
fn runtime_supports_extensions(version: &str) -> bool {
    parse_runtime_version(version).is_some_and(|parsed| parsed >= EXTENSION_RUNTIME_FLOOR)
}

fn runtime_floor_log_line(version: &str, enabled: bool) -> String {
    let [a, b, c, d] = EXTENSION_RUNTIME_FLOOR;
    let version = parse_runtime_version(version)
        .map_or_else(|| "unreadable".to_owned(), |[a, b, c, d]| format!("{a}.{b}.{c}.{d}"));
    format!("extension call=runtime_floor version={version} floor={a}.{b}.{c}.{d} enabled={enabled}")
}

/// Text from the runtime or an extension, made safe for one log line: control and invisible format
/// characters, quotes and backslashes become `?`, and the length is capped.
fn log_text(text: &str, max_chars: usize) -> String {
    text.chars()
        .take(max_chars)
        .map(|c| {
            if c.is_control() || crate::extensions::is_invisible_format(c) || matches!(c, '"' | '\\')
            {
                '?'
            } else {
                c
            }
        })
        .collect()
}

/// One diagnostic line for an extension call. `status` is the HRESULT the call or its completion
/// gave, or `None` when it reported success without returning the extension. Only the WebView2 id
/// and the name are logged, never extension files or their contents.
fn extension_log_line(
    call: &str,
    bowser: Option<&str>,
    status: Option<windows::core::HRESULT>,
    webview_id: Option<&str>,
    name: Option<&str>,
) -> String {
    let (result, hresult) = match status {
        Some(code) if code.is_ok() => ("ok", format!("0x{:08X}", code.0 as u32)),
        Some(code) => ("failed", format!("0x{:08X}", code.0 as u32)),
        None => ("no_extension", "none".to_owned()),
    };
    let bowser = bowser.map_or_else(|| "none".to_owned(), |id| log_text(id, 32));
    let webview_id = webview_id.map_or_else(|| "none".to_owned(), |id| log_text(id, 128));
    let name = name.map_or_else(|| "none".to_owned(), |name| format!("\"{}\"", log_text(name, 64)));
    format!(
        "extension call={call} bowser={bowser} result={result} hresult={hresult} id={webview_id} name={name}"
    )
}

fn profile7_log_line(status: windows::core::HRESULT) -> String {
    format!(
        "extension call=profile7_cast supported={} hresult=0x{:08X}",
        status.is_ok(),
        status.0 as u32
    )
}

/// Extension diagnostics are off unless `BOWSER_EXTENSION_TRACE` is set, then go to stderr (the same
/// opt-in as `BOWSER_SUGGEST_TRACE`). The benchmark event log only takes fixed lowercase names, so
/// it cannot carry an HRESULT or a name, and benchmark launches never load extensions.
fn extension_trace_enabled() -> bool {
    std::env::var_os("BOWSER_EXTENSION_TRACE").is_some()
}

fn extension_trace(line: &str) {
    if extension_trace_enabled() {
        eprintln!("{line}");
    }
}

fn status_code(status: &Result<()>) -> windows::core::HRESULT {
    status.as_ref().err().map_or(windows::core::HRESULT(0), Error::code)
}

/// The WebView2 id and name of a live extension, for the trace. Reads nothing unless tracing is on.
fn extension_identity(
    extension: &ICoreWebView2BrowserExtension,
) -> (Option<String>, Option<String>) {
    if !extension_trace_enabled() {
        return (None, None);
    }
    (extension_webview_id(extension), extension_name(extension))
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
    extension_trace(&profile7_log_line(
        profile.as_ref().err().map_or(windows::core::HRESULT(0), Error::code),
    ));
    let pending = std::mem::take(&mut core.borrow_mut().startup_extensions);
    match profile {
        Ok(profile) => {
            core.borrow_mut().extension_profile = Some(profile.clone());
            reconcile_profile(core, &profile, pending);
        }
        Err(_) => {
            for (id, _) in pending {
                record_load(
                    &id,
                    Err("This WebView2 runtime does not support browser extensions.".to_owned()),
                    win32::extensions_changed,
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
            extension_trace(&extension_log_line(
                "list",
                None,
                Some(status_code(&status)),
                None,
                None,
            ));
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
        extension_trace(&extension_log_line("list", None, Some(error.code()), None, None));
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
        let extension = match unsafe { list.GetValueAtIndex(index) } {
            Ok(extension) => extension,
            Err(error) => {
                extension_trace(&extension_log_line(
                    "list_item",
                    None,
                    Some(error.code()),
                    None,
                    None,
                ));
                return Err(format!("{error}"));
            }
        };
        let (traced_id, traced_name) = extension_identity(&extension);
        if extension_trace_enabled() {
            extension_trace(&extension_log_line(
                "list_item",
                None,
                Some(windows::core::HRESULT(0)),
                traced_id.as_deref(),
                traced_name.as_deref(),
            ));
        }
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

/// The extension's name as WebView2 reports it (a localized `__MSG_` name is resolved).
fn extension_name(extension: &ICoreWebView2BrowserExtension) -> Option<String> {
    let mut name = PWSTR::null();
    unsafe { extension.Name(&mut name) }.ok()?;
    Some(CoTaskMemPWSTR::from(name).to_string())
}

/// Records how an extension's load went, then runs `announce` so the windows read it: the toolbar
/// button and the settings page both show the result and only catch up when told. The engine calls
/// this on the UI thread, where `win32::extensions_changed` posts to every window.
fn record_load(id: &str, result: std::result::Result<(), String>, announce: impl FnOnce()) {
    crate::extensions::record_load_result(id, result);
    announce();
}

fn report_load(id: String) -> ExtensionDone {
    Box::new(move |result| record_load(&id, result, win32::extensions_changed))
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
                    let (traced_id, traced_name) = extension_identity(&extension);
                    extension_trace(&extension_log_line(
                        "add",
                        Some(&loaded_id),
                        Some(windows::core::HRESULT(0)),
                        traced_id.as_deref(),
                        traced_name.as_deref(),
                    ));
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
                (Ok(()), None) => {
                    extension_trace(&extension_log_line("add", Some(&loaded_id), None, None, None));
                    Err("WebView2 did not return the extension.".to_owned())
                }
                (Err(error), _) => {
                    extension_trace(&extension_log_line(
                        "add",
                        Some(&loaded_id),
                        Some(error.code()),
                        None,
                        None,
                    ));
                    Err(format!("{error}"))
                }
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
        extension_trace(&extension_log_line("add", Some(&id), Some(error.code()), None, None));
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
    let call = if enabled { "enable" } else { "disable" };
    let (traced_id, traced_name) = extension_identity(extension);
    let completion_identity = (traced_id.clone(), traced_name.clone());
    let handler = BrowserExtensionEnableCompletedHandler::create(Box::new(move |status| {
        extension_trace(&extension_log_line(
            call,
            None,
            Some(status_code(&status)),
            completion_identity.0.as_deref(),
            completion_identity.1.as_deref(),
        ));
        finish_slot(&completion, status.map_err(|error| format!("{error}")));
        Ok(())
    }));
    if let Err(error) = unsafe { extension.Enable(enabled, &handler) } {
        extension_trace(&extension_log_line(
            call,
            None,
            Some(error.code()),
            traced_id.as_deref(),
            traced_name.as_deref(),
        ));
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
    // Read before the call: a removed extension may no longer answer for its id or name.
    let (traced_id, traced_name) = extension_identity(extension);
    let completion_identity = (traced_id.clone(), traced_name.clone());
    let requested_id = id.clone();
    let handler = BrowserExtensionRemoveCompletedHandler::create(Box::new(move |status| {
        extension_trace(&extension_log_line(
            "remove",
            Some(&id),
            Some(status_code(&status)),
            completion_identity.0.as_deref(),
            completion_identity.1.as_deref(),
        ));
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
        extension_trace(&extension_log_line(
            "remove",
            Some(&requested_id),
            Some(error.code()),
            traced_id.as_deref(),
            traced_name.as_deref(),
        ));
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
        let private_blocker = extras.private_blocker;
        let startup_extensions = extras.startup_extensions;
        // Extensions are only enabled on a runtime at or above the floor. The version comes from
        // the same probe the start-up runtime check uses, so no new COM surface is involved.
        let mut extensions_refused: Option<String> = None;
        if extras.browser_extensions {
            let version = query_runtime(PCWSTR::null()).ok();
            let supported = version.as_deref().is_some_and(runtime_supports_extensions);
            extension_trace(&runtime_floor_log_line(
                version.as_deref().unwrap_or(""),
                supported,
            ));
            if !supported {
                let [a, b, c, d] = EXTENSION_RUNTIME_FLOOR;
                let reason = match version.as_deref().and_then(parse_runtime_version) {
                    Some([ra, rb, rc, rd]) => format!(
                        "This WebView2 runtime ({ra}.{rb}.{rc}.{rd}) is older than {a}.{b}.{c}.{d}, which browser extensions need."
                    ),
                    None => "Bowser could not read the WebView2 runtime version, so extensions stay off."
                        .to_owned(),
                };
                for (id, _) in &startup_extensions {
                    record_load(id, Err(reason.clone()), win32::extensions_changed);
                }
                extensions_refused = Some(reason);
            }
        }
        let browser_extensions = extras.browser_extensions && extensions_refused.is_none();
        let known_extensions = extras.known_extensions;
        let extension_ids_file = extras.extension_ids_file;
        let permissions = extras.permissions;
        let chrome_ui_scheme_requested = extras.chrome_ui_scheme;
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
        // Set below, before the environment is requested, once the scheme is registered.
        let chrome_ui_scheme = Rc::new(Cell::new(false));
        let chrome_ui_scheme_env = chrome_ui_scheme.clone();
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
                        private_blocker: private_blocker.clone(),
                        private_tabs: HashSet::new(),
                        active_tab: None,
                        visible_tabs: HashSet::new(),
                        pane_bounds: std::collections::HashMap::new(),
                        target: MemoryUsageTarget::Normal,
                        tabs: HashMap::new(),
                        known_tabs: HashSet::new(),
                        creating: HashSet::new(),
                        prewarming: HashSet::new(),
                        prewarm_waiting: None,
                        waiting_controllers: VecDeque::new(),
                        pending: HashMap::new(),
                        pending_post: HashMap::new(),
                        hidden_since: HashMap::new(),
                        suspended: HashSet::new(),
                        closed: false,
                        browser_extensions,
                        extensions_refused: extensions_refused.clone(),
                        extension_profile: None,
                        extensions_loaded: HashMap::new(),
                        startup_extensions: startup_extensions.clone(),
                        known_extensions: known_extensions.clone(),
                        extension_ids_file: extension_ids_file.clone(),
                        extension_ids: extension_ids.clone(),
                        permissions: permissions.clone(),
                        chrome_ui_scheme: chrome_ui_scheme_env.get(),
                        extension_page: None,
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
            // A second window of this profile is another process on this same folder
            // ("Move tab to new window", "open profile window"). Exclusive access makes
            // that process fail to create its browser, after the original tab was closed.
            // The session lock still refuses a plain second launch. Web apps keep their
            // own folder and their own exclusive access.
            options2
                .SetExclusiveUserDataFolderAccess(false)
                .map_err(request_error)?;
        }
        if browser_extensions {
            // This options object is webview2-com's own, so the cast and the setter only record the
            // request and cannot report whether the runtime supports extensions. The runtime's
            // answer is the `ICoreWebView2Profile7` cast in `capture_extension_profile`, which logs it.
            let _ = options2
                .cast::<ICoreWebView2EnvironmentOptions6>()
                .and_then(|options6| unsafe { options6.SetAreBrowserExtensionsEnabled(true) });
        }
        if chrome_ui_scheme_requested {
            // HTML chrome mode only. A runtime without custom schemes still runs the tabs; the
            // window then keeps its native chrome (no chrome environment is offered).
            match crate::chrome_host::host::register_ui_scheme(&options2) {
                Ok(()) => chrome_ui_scheme.set(true),
                Err(_) => event(creation_log.as_ref(), "chrome_ui_scheme_unavailable"),
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
    fn focus_page(&mut self) -> Result<()> {
        let controller = {
            let state = self.core.borrow();
            state
                .active_tab
                .and_then(|active| state.tabs.get(&active))
                .and_then(|tab| tab.controller.clone())
        };
        match controller {
            Some(controller) => unsafe {
                controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)
            },
            None => Ok(()),
        }
    }

    fn open_dev_tools(&mut self) -> Result<()> {
        // No active page (blank or internal tab) means no DevTools to open: nothing happens and
        // nothing fails. Opening the DevTools UI leaves the ad blocker's protocol session intact.
        let active_webview = {
            let state = self.core.borrow();
            state
                .active_tab
                .and_then(|active| state.tabs.get(&active))
                .and_then(|tab| tab.webview.clone())
        };
        match active_webview {
            Some(webview) => unsafe { webview.OpenDevToolsWindow() },
            None => Ok(()),
        }
    }

    fn set_extension_enabled(
        &mut self,
        id: &str,
        folder: &Path,
        enabled: bool,
        done: Box<dyn FnOnce(std::result::Result<(), String>)>,
    ) {
        let (profile, loaded, extensions_on, refused) = {
            let state = self.core.borrow();
            (
                state.extension_profile.clone(),
                state.extensions_loaded.get(id).cloned(),
                state.browser_extensions,
                state.extensions_refused.clone(),
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
            (None, None) => done(Err(refused.unwrap_or_else(|| {
                "This window was started without extensions. Restart Bowser to load it.".to_owned()
            }))),
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

    fn open_extension_page(
        &mut self,
        id: &str,
        folder: &Path,
        kind: crate::extensions::PageKind,
        title: &str,
        done: Box<dyn FnOnce(std::result::Result<(), String>)>,
    ) {
        self.open_extension_page_with(id, folder, kind, title, true, done);
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

    fn clear_browsing_data(
        &mut self,
        tab: TabId,
        kind: BrowsingDataKind,
        range: BrowsingDataRange,
        done: Box<dyn FnOnce(std::result::Result<(), String>)>,
    ) {
        self.clear_browsing_data_set(tab, &[kind], range, done);
    }

    // --- page tools (T7) ---
    fn devtools_call(&mut self, tab: TabId, method: &str, params: &str, done: Box<dyn FnOnce(Option<String>)>) {
        let webview = self.core.borrow().tabs.get(&tab).and_then(|tab| tab.webview.clone());
        match webview {
            Some(webview) => devtools_capture_call(&webview, method, params, done),
            None => done(None),
        }
    }

    fn run_page_script(&mut self, tab: TabId, script: &str, limit: usize, done: Box<dyn FnOnce(Option<String>)>) {
        let webview = self.core.borrow().tabs.get(&tab).and_then(|tab| tab.webview.clone());
        match webview {
            Some(webview) => run_capped_script(&webview, script, limit, done),
            None => done(None),
        }
    }

    fn apply_page_tools(&mut self) {
        let tabs: Vec<(ICoreWebView2, Rc<RefCell<Option<String>>>)> = self
            .core
            .borrow()
            .tabs
            .values()
            .filter_map(|tab| Some((tab.webview.clone()?, tab.spatial_id.clone())))
            .collect();
        for (webview, spatial_id) in &tabs {
            apply_page_tools_to(webview, spatial_id);
        }
    }
    // --- end page tools ---

    fn apply_website_look(&mut self) {
        let look = crate::website_look::current();
        let webviews: Vec<ICoreWebView2> = self
            .core
            .borrow()
            .tabs
            .values()
            .filter_map(|tab| tab.webview.clone())
            .collect();
        // --- wave-1 fixes --- the preferred colour scheme belongs to the browsing profile, and normal and
        // private tabs have different profiles, so each tab sets it (a repeat on the same profile is a
        // no-op); the dark-mode override is per page.
        for webview in &webviews {
            apply_preferred_colour_scheme(webview, look.appearance);
            call_devtools(webview, "Emulation.setAutoDarkModeOverride", look.auto_dark_params());
        }
    }

    // --- layout (thread T6) ---
    fn apply_status_overlay(&mut self) {
        let shown = crate::layout_prefs::status_overlay_shown();
        let webviews: Vec<ICoreWebView2> = self
            .core
            .borrow()
            .tabs
            .values()
            .filter_map(|tab| tab.webview.clone())
            .collect();
        for webview in &webviews {
            let _ = unsafe { webview.Settings().and_then(|settings| settings.SetIsStatusBarEnabled(shown)) };
        }
    }
    // --- end layout ---

    fn clear_browsing_data_set(
        &mut self,
        tab: TabId,
        kinds: &[BrowsingDataKind],
        range: BrowsingDataRange,
        done: Box<dyn FnOnce(std::result::Result<(), String>)>,
    ) {
        let webview = self
            .core
            .borrow()
            .tabs
            .get(&tab)
            .and_then(|tab| tab.webview.clone());
        let Some(webview) = webview else {
            done(Err("The active profile is not ready.".to_owned()));
            return;
        };
        let profile = unsafe {
            webview
                .cast::<ICoreWebView2_13>()
                .and_then(|webview| webview.Profile())
                .and_then(|profile| profile.cast::<ICoreWebView2Profile2>())
        };
        let profile = match profile {
            Ok(profile) => profile,
            Err(error) => {
                done(Err(format!(
                    "This WebView2 runtime cannot clear that data: {error}"
                )));
                return;
            }
        };
        let kinds = browsing_data_mask(kinds);
        if kinds.0 == 0 {
            done(Ok(()));
            return;
        }
        let completion = Rc::new(RefCell::new(Some(done)));
        let callback = completion.clone();
        let handler = ClearBrowsingDataCompletedHandler::create(Box::new(move |status| {
            if let Some(done) = callback.borrow_mut().take() {
                done(status.map_err(|error| format!("{error}")));
            }
            Ok(())
        }));
        let result = unsafe {
            match range.seconds() {
                None => profile.ClearBrowsingData(kinds, &handler),
                Some(seconds) => {
                    let end = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs_f64();
                    profile.ClearBrowsingDataInTimeRange(kinds, (end - seconds as f64).max(0.0), end, &handler)
                }
            }
        };
        if let Err(error) = result {
            if let Some(done) = completion.borrow_mut().take() {
                done(Err(format!("{error}")));
            }
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

    fn zoom_percent(&self, tab: TabId) -> Option<u16> {
        let controller = self.core.borrow().tabs.get(&tab)?.controller.clone()?;
        let mut factor = 1.0_f64;
        unsafe { controller.ZoomFactor(&mut factor) }.ok()?;
        Some(crate::site_prefs::percent_of_factor(factor))
    }

    fn set_zoom_percent(&mut self, tab: TabId, percent: u16) -> Result<bool> {
        let controller = self
            .core
            .borrow()
            .tabs
            .get(&tab)
            .and_then(|tab| tab.controller.clone());
        let Some(controller) = controller else {
            return Ok(false);
        };
        unsafe { controller.SetZoomFactor(crate::site_prefs::factor_of_percent(percent)) }?;
        Ok(true)
    }

    fn smart_screen_enabled(&self, tab: TabId) -> Option<bool> {
        let webview = self.core.borrow().tabs.get(&tab)?.webview.clone()?;
        let settings = unsafe {
            webview
                .Settings()
                .ok()?
                .cast::<ICoreWebView2Settings8>()
                .ok()?
        };
        let mut enabled = BOOL::default();
        unsafe { settings.IsReputationCheckingRequired(&mut enabled) }.ok()?;
        Some(enabled.as_bool())
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
        if let Some(webview) = self
            .core
            .borrow()
            .tabs
            .get(&tab)
            .and_then(|tab| tab.webview.clone())
        {
            exit_native_picture_in_picture(&webview);
        }
        self.navigate_tab_internal(tab, url)
    }
    fn navigate_tab_post(&mut self, tab: TabId, url: &Url, body: &str) -> Result<()> {
        let live = {
            let state = self.core.borrow();
            state.tabs.get(&tab).map(|tab| (tab.webview.clone(), tab.environment.clone()))
        };
        if let Some((webview, environment)) = live {
            return post_webview(webview.as_ref(), environment.as_ref(), url, body);
        }
        // No controller yet: it is made as for any navigation, and the POST is sent when it is ready.
        self.navigate_tab_internal(tab, url)?;
        let mut state = self.core.borrow_mut();
        if state.pending.contains_key(&tab) {
            state.pending_post.insert(tab, (url.clone(), body.to_owned()));
        }
        Ok(())
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
        let mut wide_script = Zeroizing::new(Vec::with_capacity(script.encode_utf16().count() + 1));
        wide_script.extend(script.encode_utf16());
        wide_script.push(0);
        let completed = ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(())));
        unsafe { webview.ExecuteScript(PCWSTR(wide_script.as_ptr()), &completed) }
    }
    fn run_reader_script(
        &mut self,
        tab: TabId,
        script: &str,
        done: Box<dyn FnOnce(Option<String>)>,
    ) {
        let webview = self
            .core
            .borrow()
            .tabs
            .get(&tab)
            .and_then(|tab| tab.webview.clone());
        match webview {
            Some(webview) => run_reader_script(&webview, script, done),
            None => done(None),
        }
    }
    fn run_translate_script(
        &mut self,
        tab: TabId,
        script: &str,
        done: Box<dyn FnOnce(Option<String>)>,
    ) {
        let webview = self
            .core
            .borrow()
            .tabs
            .get(&tab)
            .and_then(|tab| tab.webview.clone());
        match webview {
            Some(webview) => run_translate_script(&webview, script, done),
            None => done(None),
        }
    }
    fn run_feed_script(
        &mut self,
        tab: TabId,
        script: &str,
        done: Box<dyn FnOnce(Option<String>)>,
    ) {
        let webview = self
            .core
            .borrow()
            .tabs
            .get(&tab)
            .and_then(|tab| tab.webview.clone());
        match webview {
            Some(webview) => run_feed_script(&webview, script, done),
            None => done(None),
        }
    }
    fn picture_in_picture_eligible(&mut self, tab: TabId, done: Box<dyn FnOnce(bool)>) {
        let webview = self
            .core
            .borrow()
            .tabs
            .get(&tab)
            .and_then(|tab| tab.webview.clone());
        if let Some(webview) = webview {
            query_picture_in_picture_eligibility(&webview, done);
        } else {
            done(false);
        }
    }
    fn toggle_picture_in_picture(&mut self, tab: TabId) {
        let webview = self
            .core
            .borrow()
            .tabs
            .get(&tab)
            .and_then(|tab| tab.webview.clone());
        if let Some(webview) = webview {
            toggle_native_picture_in_picture(&webview);
        }
    }
    fn set_tab_private(&mut self, tab: TabId) {
        let mut state = self.core.borrow_mut();
        if !state.closed && !state.tabs.contains_key(&tab) {
            state.private_tabs.insert(tab);
        }
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
    fn tab_history(&self, tab: crate::tabs::TabId) -> Option<(bool, bool)> {
        let state = self.core.borrow();
        let tab = state.tabs.get(&tab)?;
        tab.webview.as_ref()?;
        Some((EngineAdapter::can_go_back(tab), EngineAdapter::can_go_forward(tab)))
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
        state.pane_bounds.clear(); // --- split view ---
        if let Some(active) = state.active_tab
            && let Some(tab) = state.tabs.get_mut(&active)
        {
            EngineAdapter::resize(tab, bounds)?;
        }
        Ok(())
    }
    fn parent_window_moved(&mut self) {
        // Every live controller: split view shows more than the active tab, and a hidden one is
        // placed right when it is shown again. The calls run after the core borrow ends.
        let controllers: Vec<_> = self.core.try_borrow().map_or_else(
            |_| Vec::new(),
            |state| state.tabs.values().filter_map(|tab| tab.controller.clone()).collect(),
        );
        for controller in controllers {
            let _ = unsafe { controller.NotifyParentWindowPositionChanged() };
        }
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
        let active_webview = {
            let state = self.core.borrow();
            state
                .active_tab
                .and_then(|active| state.tabs.get(&active))
                .and_then(|tab| tab.webview.clone())
        };
        if let Some(webview) = active_webview {
            exit_native_picture_in_picture(&webview);
        }
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
        let onscreen: Vec<TabId> = if !state.visible_tabs.is_empty() {
            state.visible_tabs.iter().copied().collect()
        } else if let Some(active) = state.active_tab {
            vec![active]
        } else {
            Vec::new()
        };
        let target = state.target;
        for tab_id in onscreen {
            if let Some(tab) = state.tabs.get_mut(&tab_id) {
                EngineAdapter::set_visible(tab, visible)?;
                if !visible {
                    let _ = EngineAdapter::set_memory_usage_target(tab, MemoryUsageTarget::Low);
                } else {
                    let _ = EngineAdapter::set_memory_usage_target(tab, target);
                }
            }
            if visible {
                state.hidden_since.remove(&tab_id);
                state.suspended.remove(&tab_id);
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
        if !was_visible {
            state.pane_bounds.clear(); // --- split view ---
        }
        let steps = switch_steps(previous, tab_id, was_visible);
        if previous != Some(tab_id) && !was_visible {
            if let Some(previous) = previous {
                if state.known_tabs.contains(&previous) {
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
        // Show and size the new tab before hiding the previous one so no blank frame is presented.
        for step in steps {
            match step {
                SwitchStep::Show(shown) => {
                    if let Some(tab) = state.tabs.get_mut(&shown) {
                        EngineAdapter::resize(tab, bounds)?;
                        EngineAdapter::set_visible(tab, true)?;
                        let _ = EngineAdapter::set_memory_usage_target(tab, target);
                    }
                }
                SwitchStep::Hide(hidden) => {
                    if state.known_tabs.contains(&hidden) {
                        if let Some(tab) = state.tabs.get_mut(&hidden) {
                            EngineAdapter::set_visible(tab, false)?;
                            let _ =
                                EngineAdapter::set_memory_usage_target(tab, MemoryUsageTarget::Low);
                        }
                    }
                }
            }
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
            let shown: Vec<TabId> = panes.iter().map(|(tab, _)| *tab).collect();
            let steps = pane_switch_steps(&shown, &old);
            for tab_id in old {
                state.hidden_since.insert(tab_id, Instant::now());
                state.suspended.remove(&tab_id);
            }
            state.visible_tabs = next;
            state.active_tab = Some(active);
            // --- split view ---
            state.pane_bounds = panes.iter().copied().collect();
            // --- end split view ---
            let target = state.target;
            for (tab_id, _) in panes {
                state.known_tabs.insert(*tab_id);
                state.hidden_since.remove(tab_id);
                state.suspended.remove(tab_id);
                if !state.tabs.contains_key(tab_id) && !state.creating.contains(tab_id) {
                    create.push(*tab_id);
                }
            }
            // The next panes are shown and sized before the old ones are hidden, so the host
            // never presents an empty content area between the two.
            for step in steps {
                match step {
                    SwitchStep::Show(tab_id) => {
                        let Some(bounds) = state.pane_bounds.get(&tab_id).copied() else {
                            continue;
                        };
                        if let Some(tab) = state.tabs.get_mut(&tab_id) {
                            EngineAdapter::resize(tab, bounds)?;
                            EngineAdapter::set_visible(tab, true)?;
                            let _ = EngineAdapter::set_memory_usage_target(tab, target);
                        }
                    }
                    SwitchStep::Hide(tab_id) => {
                        if let Some(tab) = state.tabs.get_mut(&tab_id) {
                            EngineAdapter::set_visible(tab, false)?;
                            let _ =
                                EngineAdapter::set_memory_usage_target(tab, MemoryUsageTarget::Low);
                        }
                    }
                }
            }
        }
        for tab in create {
            Self::request_controller(&self.core, tab, false)?;
        }
        Ok(())
    }
    fn close_tab(&mut self, tab_id: TabId) -> Result<()> {
        if let Some(webview) = self
            .core
            .borrow()
            .tabs
            .get(&tab_id)
            .and_then(|tab| tab.webview.clone())
        {
            exit_native_picture_in_picture(&webview);
        }
        let removed = {
            let mut state = self.core.borrow_mut();
            state.known_tabs.remove(&tab_id);
            state.private_tabs.remove(&tab_id);
            state.visible_tabs.remove(&tab_id);
            state.waiting_controllers.retain(|queued| *queued != tab_id);
            if state.prewarm_waiting == Some(tab_id) {
                state.prewarm_waiting = None;
            }
            state.pending.remove(&tab_id);
            state.pending_post.remove(&tab_id);
            state.hidden_since.remove(&tab_id);
            state.suspended.remove(&tab_id);
            state.tabs.remove(&tab_id)
        };
        // Controller teardown can raise events, so it runs without the core borrow.
        let closed = match removed {
            Some(mut tab) => EngineAdapter::close(&mut tab),
            None => Ok(()),
        };
        // Downloads cannot outlive their webview: they are cancelled and listed as interrupted.
        super::download::interrupt_owner(super::download::tab_owner(tab_id.0));
        closed
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
                unsafe {
                    audio.SetIsMuted(muted)?;
                }
            }
        }
        Ok(())
    }
    // --- tab options --- the tab hover card's thumbnail and memory line.
    fn capture_tab_thumbnail(&mut self, tab_id: TabId, max_w: u32, max_h: u32, done: Box<dyn FnOnce(Option<Vec<u8>>)>) {
        let (webview, on_screen) = {
            let state = self.core.borrow();
            (
                state.tabs.get(&tab_id).and_then(|tab| tab.webview.clone()),
                state.active_tab == Some(tab_id) || state.visible_tabs.contains(&tab_id),
            )
        };
        // A hidden controller paints nothing, so only a tab on screen is captured.
        let (Some(webview), true) = (webview, on_screen) else {
            done(None);
            return;
        };
        let Some(stream) = (unsafe { windows::Win32::UI::Shell::SHCreateMemStream(None) }) else {
            done(None);
            return;
        };
        let keep = stream.clone();
        let done = std::cell::RefCell::new(Some(done));
        let handler = webview2_com::CapturePreviewCompletedHandler::create(Box::new(move |status| {
            let bytes = if status.is_ok() { read_capture_stream(&keep) } else { None };
            if let Some(done) = done.borrow_mut().take() {
                done(bytes.and_then(|bytes| crate::thumbnail::downscale_jpeg(&bytes, max_w, max_h)));
            }
            Ok(())
        }));
        let _ = unsafe { webview.CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG, &stream, &handler) };
    }
    fn tab_memory_usage(&mut self, done: Box<dyn FnOnce(Vec<(TabId, u64)>)>) {
        let (environment, webviews) = {
            let state = self.core.borrow();
            (
                state.environment.clone(),
                state.tabs.iter().filter_map(|(id, tab)| tab.webview.clone().map(|webview| (*id, webview))).collect::<Vec<_>>(),
            )
        };
        let sources: Vec<(TabId, String)> = webviews
            .into_iter()
            .filter_map(|(id, webview)| {
                let mut source = PWSTR::null();
                unsafe { webview.Source(&mut source) }.ok()?;
                Some((id, CoTaskMemPWSTR::from(source).to_string()))
            })
            .collect();
        let Some(environment) = environment.and_then(|environment| environment.cast::<ICoreWebView2Environment13>().ok()) else {
            done(Vec::new());
            return;
        };
        let done = std::cell::RefCell::new(Some(done));
        let handler = webview2_com::GetProcessExtendedInfosCompletedHandler::create(Box::new(move |status, infos| {
            let usage = match (status.is_ok(), infos) {
                (true, Some(infos)) => renderer_memory(&infos, &sources),
                _ => Vec::new(),
            };
            if let Some(done) = done.borrow_mut().take() {
                done(usage);
            }
            Ok(())
        }));
        let _ = unsafe { environment.GetProcessExtendedInfos(&handler) };
    }
    // --- end tab options ---
    fn check_sleep_blocked(&mut self, tab_id: TabId, done: Box<dyn FnOnce(bool)>) {
        // A download dies with its webview, so a tab that is downloading stays awake.
        if super::download::owner_downloading(super::download::tab_owner(tab_id.0)) {
            done(true);
            return;
        }
        let webview = self.core.borrow().tabs.get(&tab_id).and_then(|tab| tab.webview.clone());
        let Some(webview) = webview else {
            done(true);
            return;
        };
        // Suspension is only a scheduling hint: TrySuspend is asynchronous and may be refused,
        // and even a successfully suspended page can retain unsaved form input. Always wake and
        // inspect the actual page before any caller may close its controller.
        resume_then_sleep_probe(
            || webview.cast::<ICoreWebView2_3>().and_then(|api| unsafe { api.Resume() }),
            |done| {
                type CompletionSlot = Rc<RefCell<Option<Box<dyn FnOnce(bool)>>>>;
                let slot: CompletionSlot = Rc::new(RefCell::new(Some(done)));
                let completion = slot.clone();
                let handler = ExecuteScriptCompletedHandler::create(Box::new(move |status, result| {
                    let blocked = sleep_probe_blocks(status.is_ok(), &result);
                    let done = completion.borrow_mut().take();
                    if let Some(done) = done {
                        done(blocked);
                    }
                    Ok(())
                }));
                let script = CoTaskMemPWSTR::from(SLEEP_BLOCK_SCRIPT);
                if unsafe { webview.ExecuteScript(*script.as_ref().as_pcwstr(), &handler) }.is_err() {
                    let done = slot.borrow_mut().take();
                    if let Some(done) = done {
                        done(true);
                    }
                }
                // Do not re-suspend after a blocked/failed probe, or from a late callback after
                // tab activation. The caller rechecks eligibility before any explicit hibernate.
            },
            done,
        );
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
    fn force_background_memory_low(&mut self) -> usize {
        let mut state = self.core.borrow_mut();
        let visible = state.visible_tabs.clone();
        let active = state.active_tab;
        let mut set = 0;
        for (id, tab) in state.tabs.iter_mut() {
            if visible.contains(id) || active == Some(*id) {
                continue;
            }
            if matches!(
                EngineAdapter::set_memory_usage_target(tab, MemoryUsageTarget::Low),
                Ok(MemoryTargetApply::Applied)
            ) {
                set += 1;
            }
        }
        set
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
        let page = self.core.borrow_mut().extension_page.take();
        if let Some(page) = page {
            page.close();
        }
        let tabs: Vec<WebView2Tab> = {
            let mut state = self.core.borrow_mut();
            state.closed = true;
            state.known_tabs.clear();
            state.pending.clear();
            state.pending_post.clear();
            state.creating.clear();
            state.hidden_since.clear();
            state.suspended.clear();
            state.tabs.drain().map(|(_, tab)| tab).collect()
        };
        for mut tab in tabs {
            let _ = EngineAdapter::close(&mut tab);
        }
        super::download::interrupt_all();
        self.core.borrow_mut().environment = None;
        Ok(())
    }
}

/// Read-only check run in a tab that is about to sleep. It returns only `true`/`false`; no page
/// text, field values or URLs leave the page. Unreadable frames, recursion limits and exceptions
/// fail closed. Nonempty editable content blocks even after blur: its original text is unknown.
const SLEEP_BLOCK_SCRIPT: &str = r#"(function(){
function own(w){try{var d=w.document;
if(d.pictureInPictureElement)return true;
var m=d.querySelectorAll('video,audio');
for(var i=0;i<m.length;i++){var v=m[i];
if(!v.paused&&!v.ended&&v.readyState>2)return true;
var s=v.srcObject;
if(s&&s.getTracks&&s.getTracks().some(function(t){return t.readyState==='live'}))return true;}
var f=d.querySelectorAll('input,textarea,select,[contenteditable]');
for(var j=0;j<f.length;j++){var e=f[j],n=e.tagName;
if(n==='TEXTAREA'){if(e.value!==e.defaultValue)return true;continue;}
if(n==='SELECT'){var o=e.options;for(var q=0;q<o.length;q++){if(o[q].selected!==o[q].defaultSelected)return true;}continue;}
if(n==='INPUT'){var t=(e.type||'text').toLowerCase();
if(t==='file'){if(!e.files||e.files.length>0)return true;continue;}
if(t==='checkbox'||t==='radio'){if(e.checked!==e.defaultChecked)return true;}
else if(t!=='hidden'&&t!=='button'&&t!=='submit'&&t!=='reset'&&t!=='image'&&t!=='file'){
if(e.value!==e.defaultValue)return true;}
continue;}
if(e.isContentEditable&&(e.textContent||'').trim().length>0)return true;}
return false;}catch(x){return true;}}
function walk(w,depth){if(own(w))return true;if(depth>8)return true;
var n=0;try{n=w.frames.length}catch(x){return true}
for(var k=0;k<n;k++){var c=false;try{c=walk(w.frames[k],depth+1)}catch(x){c=true}
if(c)return true;}
return false;}
try{return walk(window,0)}catch(x){return true}})()"#;

/// A fresh, successful resume and a real probe are both mandatory. No suspended-state hint
/// can bypass this path; an unsupported/failed Resume is an unknown page, never permission to close.
fn resume_then_sleep_probe(
    resume: impl FnOnce() -> Result<()>,
    probe: impl FnOnce(Box<dyn FnOnce(bool)>),
    done: Box<dyn FnOnce(bool)>,
) {
    if resume().is_err() {
        done(true);
        return;
    }
    probe(done);
}

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

/// Takes the tab's waiting POST body, but only when it belongs to `navigating`, the address the new
/// controller is about to load; a body left over from a superseded navigation is dropped, never
/// sent to another address. Pure.
fn take_pending_post(
    pending_post: &mut HashMap<TabId, (Url, String)>,
    tab: TabId,
    navigating: Option<&Url>,
) -> Option<String> {
    let (url, body) = pending_post.remove(&tab)?;
    (navigating == Some(&url)).then_some(body)
}

/// Navigates a webview with a POST of form fields (`body`, already percent-encoded) to an https
/// address: a search with an engine that takes its query that way.
fn post_webview(
    webview: Option<&ICoreWebView2>,
    environment: Option<&ICoreWebView2Environment>,
    url: &Url,
    body: &str,
) -> Result<()> {
    if url.scheme() != "https" {
        return Err(Error::from(windows::Win32::Foundation::E_INVALIDARG));
    }
    let webview: ICoreWebView2_2 = webview.ok_or_else(|| Error::from(windows::Win32::Foundation::E_POINTER))?.cast()?;
    let environment: ICoreWebView2Environment2 =
        environment.ok_or_else(|| Error::from(windows::Win32::Foundation::E_POINTER))?.cast()?;
    let stream = unsafe { windows::Win32::UI::Shell::SHCreateMemStream(Some(body.as_bytes())) }
        .ok_or_else(|| Error::from(windows::Win32::Foundation::E_OUTOFMEMORY))?;
    let uri = HSTRING::from(url.as_str());
    let headers = HSTRING::from("Content-Type: application/x-www-form-urlencoded
");
    let request = unsafe {
        environment.CreateWebResourceRequest(PCWSTR(uri.as_ptr()), w!("POST"), &stream, PCWSTR(headers.as_ptr()))?
    };
    unsafe { webview.NavigateWithWebResourceRequest(&request) }
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
    private: bool,
) -> Result<WebView2Tab> {
    unsafe {
        controller.SetBounds(bounds)?;
        controller.SetIsVisible(visible)?;
        let webview = controller.CoreWebView2()?;
        if private {
            // A runtime that ignored the controller options must never leave a private tab in the
            // browsing profile.
            verify_private_profile(&webview)?;
        }
        let settings = webview.Settings()?;
        // Browsing tabs keep the page DevTools available in every build (the "Developer tools"
        // menu item and F12 / Ctrl+Shift+I open it). This is the DevTools UI only; it does not
        // touch the ad blocker's `CallDevToolsProtocolMethod` Fetch/Network session.
        settings.SetAreDevToolsEnabled(true)?;
        // --- layout (thread T6) --- the link-address overlay at the bottom of the page follows the
        // status bar setting (Bowser has no status bar of its own).
        settings.SetIsStatusBarEnabled(crate::layout_prefs::status_overlay_shown())?;
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
        // Password login messages are opt-in per active profile: the app supplies this callback
        // only when its vault opens successfully and the profile is not a guest.
        apply_controller_security_policy(|setting| match setting {
            ControllerSecuritySetting::WebMessages => settings.SetIsWebMessageEnabled(false),
            ControllerSecuritySetting::HostObjects => settings.SetAreHostObjectsAllowed(false),
        })?;
        // Private tabs never capture sign-ins, so their pages get no message channel at all.
        if let Some(login_callback) = events.login_message.clone().filter(|_| {
            crate::private::may_persist(crate::private::Store::SignInCapture, private, false)
        }) {
            settings.SetIsWebMessageEnabled(true)?;
            let mut web_messages_enabled = BOOL::default();
            settings.IsWebMessageEnabled(&mut web_messages_enabled)?;
            if !web_messages_enabled.as_bool() {
                return Err(Error::new(
                    windows::core::HRESULT(0x80070005u32 as i32),
                    "WebView2 login message handling could not be enabled",
                ));
            }

            // Never wait for this completion: configure_controller runs inside the controller's
            // creation callback, and WebView2 does not deliver a completion to a nested message
            // pump started from its own callback. Waiting here hung every non-guest tab before its
            // first navigation. WebView2 applies the script in call order, so it is in place for
            // the Navigate that follows. A synchronous failure still fails configuration.
            let script = CoTaskMemPWSTR::from(LOGIN_DETECTION_SCRIPT);
            let script_log = events.policy_event.clone();
            webview.AddScriptToExecuteOnDocumentCreated(
                *script.as_ref().as_pcwstr(),
                &AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
                    move |status, _id| {
                        if status.is_err() {
                            event(script_log.as_ref(), "login_script_failed");
                        }
                        Ok(())
                    },
                )),
            )?;

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
                    let json = Zeroizing::new(raw_message.owned_text());
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
        start_website_look(&webview);
        // Page tools (T7): Ctrl+scroll zoom and spatial navigation, as Settings > Page tools has them.
        let spatial_id = Rc::new(RefCell::new(None::<String>));
        apply_page_tools_to(&webview, &spatial_id);
        if let Some(level) = tracking_policy.level {
            let verified = apply_tracking_prevention(
                &webview,
                level,
                tracking_policy.enabled,
                events.policy_event.as_ref(),
            );
            if private && !verified {
                // Strict tracking prevention is part of what makes the tab private.
                return Err(Error::new(
                    windows::Win32::Foundation::E_ACCESSDENIED,
                    "WebView2 could not apply strict tracking prevention to a private tab",
                ));
            }
        } else if private {
            return Err(Error::from(windows::Win32::Foundation::E_ACCESSDENIED));
        }
        let mut token = 0;
        let allowed_file_for_handler = allowed_file.clone();
        let allowed_file_pending = Rc::new(Cell::new(allowed_file.is_some()));
        let block_tab = Rc::new(TabBlockState::default());
        let navigation_start_log = events.policy_event.clone();
        let navigation_started = events.navigation_started.clone();
        let link_navigation = events.link_navigation.clone(); // --- tab options ---
        let nav_blocker = blocker.clone();
        let nav_block_tab = block_tab.clone();
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
                    // A top-level navigation is not cancelled here. Cancelling a typed address or
                    // an ad-click landing left the tab where it was, with no site toggle that
                    // could bring the destination back. Iframes and new windows still are.
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
                            if let Some(sender) = _sender.as_ref() {
                                exit_native_picture_in_picture(sender);
                                // The destination host decides the renderer block list before the
                                // new document's first subresources run. top_host stays the page
                                // that is still showing until the navigation commits.
                                if let (Some(blocker), Some(host)) = (
                                    nav_blocker.as_ref(),
                                    parsed.as_ref().and_then(|url| url.host_str()),
                                ) {
                                    refresh_blocked_urls(
                                        sender,
                                        &nav_block_tab,
                                        blocker,
                                        private,
                                        Some(host),
                                    );
                                }
                            }
                            // --- tab options --- "Keep pinned tabs on their site": a followed link to another
                            // site opens in a new tab and this navigation is cancelled.
                            if let (Some(hook), Some(destination)) = (link_navigation.as_ref(), parsed.as_ref()) {
                                let (mut user, mut redirected) = (BOOL::default(), BOOL::default());
                                let _ = args.IsUserInitiated(&mut user);
                                let _ = args.IsRedirected(&mut redirected);
                                let mut kind = COREWEBVIEW2_NAVIGATION_KIND_NEW_DOCUMENT;
                                if let Ok(args3) = args.cast::<ICoreWebView2NavigationStartingEventArgs3>() {
                                    let _ = args3.NavigationKind(&mut kind);
                                }
                                if user.as_bool()
                                    && !redirected.as_bool()
                                    && kind == COREWEBVIEW2_NAVIGATION_KIND_NEW_DOCUMENT
                                    && hook(tab_id, destination)
                                {
                                    let _ = args.SetCancel(true);
                                    return Ok(());
                                }
                            }
                            // --- end tab options ---
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
        let frame_blocker = blocker.clone();
        let frame_block_tab = block_tab.clone();
        webview.add_FrameNavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    let parsed = if args.Uri(&mut uri).is_ok() {
                        Url::parse(&CoTaskMemPWSTR::from(uri).to_string()).ok()
                    } else {
                        None
                    };
                    let allowed = parsed
                        .as_ref()
                        .is_some_and(frame_navigation_scheme_allowed);
                    // blob:, data: and about:srcdoc frames are left alone: only an http(s) frame
                    // destination is ever checked against the blocker.
                    let blocked = allowed
                        && parsed.as_ref().is_some_and(|url| {
                            matches!(url.scheme(), "http" | "https") && {
                                let top_host = frame_block_tab.top_host.borrow();
                                frame_blocker.as_ref().is_some_and(|blocker| {
                                    let blocker = blocker.borrow();
                                    if private {
                                        blocker.should_block_private(url.as_str(), &top_host)
                                    } else {
                                        blocker.should_block(url.as_str(), &top_host)
                                    }
                                })
                            }
                        });
                    if !allowed || blocked {
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
        let content_blocker = blocker.clone();
        webview.add_ContentLoading(
            &ContentLoadingEventHandler::create(Box::new(move |sender, args| {
                if let Some(sender) = sender.as_ref() {
                    sync_top_host(sender, &content_block_tab);
                    refresh_force_dark(sender);
                    if let Some(blocker) = content_blocker.as_ref() {
                        refresh_blocked_urls(sender, &content_block_tab, blocker, private, None);
                        apply_cosmetic_css(sender, &content_block_tab, blocker, private);
                    }
                }
                if content_block_tab.count.replace(0) != 0
                    && let Some(callback) = content_blocked.as_ref()
                {
                    callback(tab_id, 0, String::new());
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
            let dom_block_tab = block_tab.clone();
            let dom_blocker = blocker.clone();
            webview2_2.add_DOMContentLoaded(
                &DOMContentLoadedEventHandler::create(Box::new(move |sender, args| {
                    // The page's own scripts have started by now: install the cosmetic style and
                    // the X promoted-post marker again (both idempotent) in case the early one ran
                    // before the document was ready.
                    if let (Some(sender), Some(blocker)) = (sender.as_ref(), dom_blocker.as_ref()) {
                        apply_cosmetic_css(sender, &dom_block_tab, blocker, private);
                    }
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
                            // A blocker SetCancel completes as OPERATION_CANCELED. The page that
                            // is still showing did not fail to load, so the security pill stays.
                            if navigation_failure_is_reportable(status) {
                                callback(tab_id, is_certificate_error(status));
                            }
                        }
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        let address_changed = events.address_changed.clone();
        let source_block_tab = block_tab.clone();
        let source_blocker = blocker.clone();
        webview.add_SourceChanged(
            &SourceChangedEventHandler::create(Box::new(move |sender, _args| {
                if let Some(sender) = sender.as_ref() {
                    sync_top_host(sender, &source_block_tab);
                    if let Some(blocker) = source_blocker.as_ref() {
                        refresh_blocked_urls(sender, &source_block_tab, blocker, private, None);
                    }
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

        // --- auto-hide (T11) --- a page element (a video) asks for full screen or leaves it. Only
        // installed when the app listens, so nothing runs otherwise.
        if let Some(fullscreen_changed) = events.fullscreen_changed.clone() {
            webview.add_ContainsFullScreenElementChanged(
                &webview2_com::ContainsFullScreenElementChangedEventHandler::create(Box::new(move |sender, _| {
                    if let Some(sender) = sender {
                        let mut on = BOOL(0);
                        if sender.ContainsFullScreenElement(&mut on).is_ok() {
                            fullscreen_changed(tab_id, on.as_bool(), current_web_url(&sender));
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }
        // --- end auto-hide ---

        let policy_event = events.policy_event.clone();
        let permission_store = permissions.clone();
        let permission_denied = events.permission_denied.clone();
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
                    let known = permission_for_kind(kind);
                    let stored = match (known, permission_store.as_ref()) {
                        (Some(permission), Some(store)) => store.borrow().allows(&uri, permission),
                        _ => false,
                    };
                    // A private tab has no stored allows: every request is denied.
                    let allowed = crate::private::permission_allowed(private, stored);
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
                    // Passive indicator and denied-request log only: the decision above is final and
                    // nothing prompts. Only the origin and kind leave this handler.
                    if !allowed
                        && let Some(permission) = known
                        && let Some(origin) = crate::permissions::origin_of(&uri)
                        && let Some(callback) = permission_denied.as_ref()
                    {
                        callback(tab_id, permission, origin);
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;

        // Downloads from a normal tab save to the download folder and are tracked; a private tab's
        // are always cancelled (engine/download.rs, `Source::private`).
        let download_hooks = super::download::DownloadHooks {
            event: events.download_event.clone(),
            plan: events.download_plan.clone(),
            policy: events.policy_event.clone(),
        };
        let download_source = super::download::Source {
            owner: super::download::tab_owner(tab_id.0),
            parent,
            private,
        };
        let webview4: ICoreWebView2_4 = webview.cast()?;
        webview4.add_DownloadStarting(
            &DownloadStartingEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    super::download::handle_starting(&args, &download_source, &download_hooks);
                }
                Ok(())
            })),
            &mut token,
        )?;

        let policy_event = events.policy_event.clone();
        let popup_callback = events.new_window_requested.clone();
        let popup_store = permissions.clone();
        let popup_denied = events.permission_denied.clone();
        let popup_blocker = blocker.clone();
        webview.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(move |sender, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    let mut initiated = BOOL::default();
                    let uri_result = args.Uri(&mut uri);
                    let initiated_result = args.IsUserInitiated(&mut initiated);
                    let uri_text = CoTaskMemPWSTR::from(uri).to_string();
                    let user_initiated = initiated_result.is_ok() && initiated.as_bool();
                    let target = if uri_result.is_ok() {
                        Url::parse(&uri_text).ok()
                    } else {
                        None
                    };
                    // The opener is the page showing now. Only a stored Allow for popups on its
                    // exact origin lets a script-opened window through.
                    let opener = sender.as_ref().and_then(current_web_url);
                    let stored_popups = !user_initiated
                        && match (opener.as_ref(), popup_store.as_ref()) {
                            (Some(opener), Some(store)) => {
                                store.borrow().allows(opener.as_str(), Permission::Popups)
                            }
                            _ => false,
                        };
                    // A private tab has no stored allows: a script-opened window is always blocked.
                    let site_allows = crate::private::permission_allowed(private, stored_popups);
                    // An ad target never opens a tab, whatever the gesture or stored allow: this is
                    // checked against the opener's host, not the (not yet existing) new tab's.
                    let target_blocked = target.as_ref().is_some_and(|url| {
                        matches!(url.scheme(), "http" | "https")
                            && opener.as_ref().and_then(|opener| opener.host_str()).is_some_and(
                                |host| {
                                    popup_blocker.as_ref().is_some_and(|blocker| {
                                        let blocker = blocker.borrow();
                                        if private {
                                            blocker.should_block_private(url.as_str(), host)
                                        } else {
                                            blocker.should_block(url.as_str(), host)
                                        }
                                    })
                                },
                            )
                    });
                    let decision = target
                        .as_ref()
                        .map(|url| popup_decision(url, user_initiated, site_allows, target_blocked))
                        .unwrap_or(PopupDecision::Block);
                    if decision == PopupDecision::NewTab {
                        let url = target.expect("new-tab decision requires a parsed target");
                        if let Some(callback) = popup_callback.as_ref() {
                            callback(tab_id, url);
                        }
                        if let Some(log) = policy_event.as_ref() {
                            log("popup_new_tab");
                        }
                    } else {
                        if let Some(log) = policy_event.as_ref() {
                            log("popup_blocked");
                        }
                        // A blocked script popup is listed on the Privacy dashboard (origin and
                        // kind only). A refused click, such as a mailto: link, is not a popup. An
                        // ad target blocked here is not a popup-permission denial either: nothing
                        // about permissions decided it.
                        if !target_blocked
                            && !user_initiated
                            && let Some(origin) = opener
                                .as_ref()
                                .and_then(|opener| crate::permissions::origin_of(opener.as_str()))
                            && let Some(callback) = popup_denied.as_ref()
                        {
                            callback(tab_id, Permission::Popups, origin);
                        }
                    }
                    let _ = args.SetHandled(true);
                }
                Ok(())
            })),
            &mut token,
        )?;

        // --- split view ---
        // "Open link in split view" after "Open link in new window" on an http(s) link. The pick
        // only reports the link; the window opens it later, outside this callback.
        if let (Some(open_in_split), Ok(webview11), Ok(environment9)) = (
            events.open_in_split.clone(),
            webview.cast::<ICoreWebView2_11>(),
            environment.cast::<ICoreWebView2Environment9>(),
        ) {
            webview11.add_ContextMenuRequested(
                &ContextMenuRequestedEventHandler::create(Box::new(move |_sender, args| {
                    let Some(args) = args else { return Ok(()) };
                    let target = args.ContextMenuTarget()?;
                    let mut has_link = BOOL::default();
                    target.HasLinkUri(&mut has_link)?;
                    if !has_link.as_bool() {
                        return Ok(());
                    }
                    let mut uri = PWSTR::null();
                    target.LinkUri(&mut uri)?;
                    let Some(link) = Url::parse(&CoTaskMemPWSTR::from(uri).to_string())
                        .ok()
                        .filter(|url| matches!(url.scheme(), "http" | "https"))
                    else {
                        return Ok(());
                    };
                    let item = environment9.CreateContextMenuItem(
                        w!("Open link in split view"),
                        None::<&IStream>,
                        COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND,
                    )?;
                    let picked = open_in_split.clone();
                    let mut item_token = 0i64;
                    item.add_CustomItemSelected(
                        &CustomItemSelectedEventHandler::create(Box::new(move |_item, _args| {
                            picked(tab_id, link.clone());
                            Ok(())
                        })),
                        &mut item_token,
                    )?;
                    let items = args.MenuItems()?;
                    let mut count = 0u32;
                    items.Count(&mut count)?;
                    let mut at = 0u32;
                    for index in 0..count {
                        let mut name = PWSTR::null();
                        if items.GetValueAtIndex(index).and_then(|entry| entry.Name(&mut name)).is_ok() {
                            let name = CoTaskMemPWSTR::from(name).to_string();
                            if name == "openLinkInNewTab" || name == "openLinkInNewWindow" {
                                at = index + 1;
                            }
                        }
                    }
                    items.InsertValueAtIndex(at, &item)?;
                    Ok(())
                })),
                &mut token,
            )?;
        }
        // --- end split view ---

        // --- address bar, search and bookmarks ---
        // "Search <engine> for <selection>" and "Search for this image with <engine>" in the page's
        // context menu. The labels and the action come from the window (it knows the engines).
        if let (Some(labels), Some(picked), Ok(webview11), Ok(environment9)) = (
            events.context_search.clone(),
            events.context_search_pick.clone(),
            webview.cast::<ICoreWebView2_11>(),
            environment.cast::<ICoreWebView2Environment9>(),
        ) {
            webview11.add_ContextMenuRequested(
                &ContextMenuRequestedEventHandler::create(Box::new(move |_sender, args| {
                    let Some(args) = args else { return Ok(()) };
                    let target = args.ContextMenuTarget()?;
                    let mut has_selection = BOOL::default();
                    target.HasSelection(&mut has_selection)?;
                    let mut selection = None;
                    if has_selection.as_bool() {
                        let mut text = PWSTR::null();
                        target.SelectionText(&mut text)?;
                        let text = CoTaskMemPWSTR::from(text).to_string();
                        let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                        let text: String = text.chars().take(256).collect();
                        if !text.is_empty() {
                            selection = Some(text);
                        }
                    }
                    let mut kind = COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND::default();
                    target.Kind(&mut kind)?;
                    let mut image = None;
                    if kind == COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_IMAGE {
                        let mut source = PWSTR::null();
                        target.SourceUri(&mut source)?;
                        image = Url::parse(&CoTaskMemPWSTR::from(source).to_string()).ok().filter(|url| matches!(url.scheme(), "http" | "https"));
                    }
                    let info = super::ContextSearchInfo { selection, image };
                    if info.selection.is_none() && info.image.is_none() {
                        return Ok(());
                    }
                    let names = labels(tab_id, &info);
                    let items = args.MenuItems()?;
                    let mut count = 0u32;
                    items.Count(&mut count)?;
                    let position = |wanted: &[&str], fallback: u32| {
                        let mut at = fallback;
                        for index in 0..count {
                            let mut name = PWSTR::null();
                            if items.GetValueAtIndex(index).and_then(|entry| entry.Name(&mut name)).is_ok()
                                && wanted.contains(&CoTaskMemPWSTR::from(name).to_string().as_str())
                            {
                                at = index + 1;
                            }
                        }
                        at
                    };
                    let add = |label: &str, pick: super::ContextSearchPick, after: &[&str]| -> windows::core::Result<()> {
                        let wide = HSTRING::from(label);
                        let item = environment9.CreateContextMenuItem(PCWSTR(wide.as_ptr()), None::<&IStream>, COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND)?;
                        let picked = picked.clone();
                        let mut item_token = 0i64;
                        item.add_CustomItemSelected(
                            &CustomItemSelectedEventHandler::create(Box::new(move |_item, _args| {
                                picked(tab_id, pick.clone());
                                Ok(())
                            })),
                            &mut item_token,
                        )?;
                        let at = position(after, 0);
                        items.InsertValueAtIndex(at, &item)
                    };
                    if let (Some(label), Some(text)) = (names.selection, info.selection) {
                        add(&label, super::ContextSearchPick::Selection(text), &["copy"])?;
                    }
                    if let (Some(label), Some(url)) = (names.image, info.image) {
                        add(&label, super::ContextSearchPick::Image(url), &["copyImage", "saveImageAs", "copyImageLocation"])?;
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }
        // --- end address bar, search and bookmarks ---

        // --- page tools (T7) ---
        // "Insert note" in the context menu of a text field. The labels and the action come from the app,
        // which knows the notes; the page is only told the text of the one the user picks, by a script
        // the app runs through `execute_script_for_origin`.
        if let (Some(labels), Some(picked), Ok(webview11), Ok(environment9)) = (
            events.context_notes.clone(),
            events.context_note_pick.clone(),
            webview.cast::<ICoreWebView2_11>(),
            environment.cast::<ICoreWebView2Environment9>(),
        ) {
            webview11.add_ContextMenuRequested(
                &ContextMenuRequestedEventHandler::create(Box::new(move |_sender, args| {
                    let Some(args) = args else { return Ok(()) };
                    let target = args.ContextMenuTarget()?;
                    let mut editable = BOOL::default();
                    target.IsEditable(&mut editable)?;
                    if !editable.as_bool() {
                        return Ok(());
                    }
                    let notes = labels(tab_id);
                    if notes.is_empty() {
                        return Ok(());
                    }
                    let parent = environment9.CreateContextMenuItem(
                        w!("Insert note"),
                        None::<&IStream>,
                        COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SUBMENU,
                    )?;
                    let children = parent.Children()?;
                    for (index, (note, label)) in notes.into_iter().enumerate() {
                        let wide = HSTRING::from(label);
                        let child = environment9.CreateContextMenuItem(
                            PCWSTR(wide.as_ptr()),
                            None::<&IStream>,
                            COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND,
                        )?;
                        let picked = picked.clone();
                        let mut item_token = 0i64;
                        child.add_CustomItemSelected(
                            &CustomItemSelectedEventHandler::create(Box::new(move |_item, _args| {
                                picked(tab_id, note);
                                Ok(())
                            })),
                            &mut item_token,
                        )?;
                        children.InsertValueAtIndex(index as u32, &child)?;
                    }
                    let items = args.MenuItems()?;
                    let mut count = 0u32;
                    items.Count(&mut count)?;
                    let mut at = count;
                    for index in 0..count {
                        let mut name = PWSTR::null();
                        if items.GetValueAtIndex(index).and_then(|entry| entry.Name(&mut name)).is_ok()
                            && CoTaskMemPWSTR::from(name).to_string() == "paste"
                        {
                            at = index + 1;
                        }
                    }
                    items.InsertValueAtIndex(at, &parent)
                })),
                &mut token,
            )?;
        }
        // --- end page tools ---

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
            let blocked_callback = events.blocked_changed.clone();
            let policy_log = events.policy_event.clone();
            // Listeners exist before Fetch.enable and Network.enable, which refresh sends.
            install_fetch_listener(&webview, &block_tab, blocker.clone(), private);
            install_loading_failed_listener(
                &webview,
                &block_tab,
                tab_id,
                blocked_callback.clone(),
                policy_log.clone(),
            );
            // Installed now, with the tab's still-empty top host, so every listed host is blocked
            // at the network layer before the first navigation commits.
            refresh_blocked_urls(&webview, &block_tab, &blocker, private, None);
            // The rules do not change after load, so neither do the request filters.
            let request_filters = blocker.borrow().request_filters();
            let response_environment = environment.clone();
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
                    let (hit, site_host) = REQUEST_URL.with(|buffer| {
                        let mut buffer = buffer.borrow_mut();
                        buffer.clear();
                        buffer.extend(
                            char::decode_utf16(uri.as_wide().iter().copied())
                                .map(|unit| unit.unwrap_or('\u{FFFD}')),
                        );
                        let hit = {
                            let site_host = request_tab.top_host.borrow();
                            if private {
                                blocker.borrow().block_hit_private(&buffer, &site_host)
                            } else {
                                blocker.borrow().block_hit(&buffer, &site_host)
                            }
                        };
                        let site_host = if hit != crate::blocker::BlockHit::None {
                            request_tab.top_host.borrow().clone()
                        } else {
                            String::new()
                        };
                        (hit, site_host)
                    });
                    drop(guard);
                    // Host rules and plain path rules are failed in the renderer by
                    // setBlockedURLs. Only requests the filters below let through get here: the
                    // substring rules, path rules whose case, `?` or `^` a URL pattern would get
                    // wrong, and worker requests. This 403 is the answer for those.
                    if hit != crate::blocker::BlockHit::None {
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
                            callback(tab_id, count, site_host);
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
            // which the older call does not see. The URIs are narrow (`request_filter_uris`): no
            // filter at all means no page request is routed through the UI thread.
            match webview.cast::<ICoreWebView2_22>() {
                Ok(webview22) => {
                    for (uri, kinds) in request_filter_uris(request_filters) {
                        let uri = HSTRING::from(uri);
                        for context in BLOCKABLE_CONTEXTS {
                            webview22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                                PCWSTR(uri.as_ptr()),
                                context,
                                kinds,
                            )?;
                        }
                    }
                }
                Err(_) => {
                    for uri in request_filters {
                        let uri = HSTRING::from(uri);
                        for context in BLOCKABLE_CONTEXTS {
                            webview.AddWebResourceRequestedFilter(PCWSTR(uri.as_ptr()), context)?;
                        }
                    }
                }
            }
        }

        let escape_pressed = events.escape_pressed.clone();
        let key_webview = webview.clone();
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
                    // Reader view leaves itself on Escape. The page still gets the key.
                    if is_escape_press(key, kind, ctrl, shift, alt)
                        && let Some(callback) = escape_pressed.as_ref()
                    {
                        callback(tab_id);
                    }
                    // A user can bind a gated command to any chord, so the gates are read for every key.
                    let gated = win32::gated_chords(parent);
                    if let Some(forward) = shortcut_forward_on_page(key, kind, ctrl, shift, alt, gated, || {
                        current_web_url(&key_webview).and_then(|url| url.host_str().map(str::to_owned))
                    }) {
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
        // Fires for our own SetZoomFactor calls and for the page's own zoom (Ctrl+wheel), so the
        // app sees every change.
        if let Some(callback) = events.zoom_changed.clone() {
            controller.add_ZoomFactorChanged(
                &ZoomFactorChangedEventHandler::create(Box::new(move |sender, _args| {
                    if let Some(sender) = sender {
                        let mut factor = 1.0_f64;
                        if sender.ZoomFactor(&mut factor).is_ok() {
                            callback(tab_id, crate::site_prefs::percent_of_factor(factor));
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }
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
            spatial_id,
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
fn tracking_prevention_label(
    level: COREWEBVIEW2_TRACKING_PREVENTION_LEVEL,
) -> Option<&'static str> {
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

/// A navigation Bowser cancelled is not a failed load. Real network and certificate
/// failures use other statuses and are still reported.
fn navigation_failure_is_reportable(status: COREWEBVIEW2_WEB_ERROR_STATUS) -> bool {
    status != COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED
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

/// Whether the requested level is in effect afterwards (read back from the profile).
fn apply_tracking_prevention(
    webview: &ICoreWebView2,
    requested: TrackingPreventionLevel,
    enabled: bool,
    log: Option<&Rc<dyn Fn(&'static str)>>,
) -> bool {
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
        return false;
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
            return false;
        }
    };
    let result = apply_tracking_prevention_transaction(
        tracking_prevention_value(requested),
        || {
            let mut level = COREWEBVIEW2_TRACKING_PREVENTION_LEVEL(0);
            unsafe { profile.PreferredTrackingPreventionLevel(&mut level) }?;
            Ok(level)
        },
        |level| unsafe { profile.SetPreferredTrackingPreventionLevel(level) },
        |name| event(log, name),
    );
    matches!(result, TrackingPolicyApplyResult::Verified)
}

/// Creates a controller in the shared in-memory private profile. WebView2 keeps an InPrivate
/// profile's cookies, storage and cache in memory and drops them when its last controller closes.
fn create_private_controller(
    environment: &ICoreWebView2Environment,
    parent: HWND,
    handler: &ICoreWebView2CreateCoreWebView2ControllerCompletedHandler,
) -> Result<()> {
    debug_assert!(crate::private::profile_name_valid());
    let environment10: ICoreWebView2Environment10 = environment.cast()?;
    let options = unsafe { environment10.CreateCoreWebView2ControllerOptions()? };
    let name: Vec<u16> = crate::private::PRIVATE_PROFILE
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        options.SetProfileName(PCWSTR(name.as_ptr()))?;
        options.SetIsInPrivateModeEnabled(true)?;
        environment10.CreateCoreWebView2ControllerWithOptions(parent, &options, handler)
    }
}

/// Confirms a private tab's webview really is in the InPrivate private profile.
fn verify_private_profile(webview: &ICoreWebView2) -> Result<()> {
    let webview13: ICoreWebView2_13 = webview.cast()?;
    let profile = unsafe { webview13.Profile()? };
    let mut name = PWSTR::null();
    let mut in_private = BOOL::default();
    unsafe {
        profile.ProfileName(&mut name)?;
        profile.IsInPrivateModeEnabled(&mut in_private)?;
    }
    let name = CoTaskMemPWSTR::from(name).to_string();
    if name.eq_ignore_ascii_case(crate::private::PRIVATE_PROFILE) && in_private.as_bool() {
        Ok(())
    } else {
        Err(Error::new(
            windows::Win32::Foundation::E_ACCESSDENIED,
            "WebView2 did not use the private profile for a private tab",
        ))
    }
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

/// One visibility change in a tab or pane switch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SwitchStep {
    Show(TabId),
    Hide(TabId),
}

/// Orders a tab switch so the next tab is shown before the previous one is hidden. Nothing is
/// hidden when the tab is already the active one or is already on screen in a split view.
fn switch_steps(previous: Option<TabId>, next: TabId, next_was_visible: bool) -> Vec<SwitchStep> {
    let mut steps = vec![SwitchStep::Show(next)];
    if let Some(previous) = previous {
        if previous != next && !next_was_visible {
            steps.push(SwitchStep::Hide(previous));
        }
    }
    steps
}

/// The split-view form of `switch_steps`: every next pane is shown before any old pane is hidden.
fn pane_switch_steps(next: &[TabId], old: &[TabId]) -> Vec<SwitchStep> {
    next.iter()
        .copied()
        .map(SwitchStep::Show)
        .chain(old.iter().copied().map(SwitchStep::Hide))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pending_post_body_goes_only_to_its_own_navigation() {
        let tab = TabId(7);
        let search = Url::parse("https://search.example/q").unwrap();
        let other = Url::parse("https://other.example/").unwrap();
        let mut posts = HashMap::new();
        posts.insert(tab, (search.clone(), "q=secret".to_owned()));
        assert_eq!(take_pending_post(&mut posts, tab, Some(&search)).as_deref(), Some("q=secret"));
        assert!(posts.is_empty(), "taken once");
        posts.insert(tab, (search.clone(), "q=secret".to_owned()));
        assert_eq!(take_pending_post(&mut posts, tab, Some(&other)), None, "never sent to another address");
        assert!(posts.is_empty(), "and dropped");
        posts.insert(tab, (search.clone(), "q=secret".to_owned()));
        assert_eq!(take_pending_post(&mut posts, tab, None), None);
        assert!(posts.is_empty());
        assert_eq!(take_pending_post(&mut posts, TabId(8), Some(&search)), None);
    }

    #[test]
    fn a_pending_post_body_is_bound_and_cleared_in_the_engine() {
        // The controller callback, close and shutdown are not unit-testable without WebView2, so
        // these source checks keep a queued search body from reaching another address or outliving
        // its tab.
        let source = include_str!("webview2.rs");
        let production = &source[..source.find("#[cfg(test)]").expect("test module")];
        for needle in [
            "take_pending_post(&mut core.borrow_mut().pending_post, tab_id, pending.as_ref())",
            "state.pending_post.insert(tab, (url.clone(), body.to_owned()))",
            "state.pending_post.clear();",
        ] {
            assert!(production.contains(needle), "missing in the engine: {needle}");
        }
        assert_eq!(
            production.matches("state.pending_post.remove(&tab_id);").count(),
            2,
            "a new navigation and a tab close each drop the waiting body"
        );
    }

    /// Hooks for the ignored live probe (`extension_probe_live`). They live here, not beside the
    /// production code, so the source-scanning tests below see the whole production region.
    impl WebView2Engine {
        /// Opens an extension page without showing a window.
        pub(in crate::engine) fn open_extension_page_hidden(
            &mut self,
            id: &str,
            folder: &Path,
            kind: crate::extensions::PageKind,
            done: Box<dyn FnOnce(std::result::Result<(), String>)>,
        ) {
            self.open_extension_page_with(id, folder, kind, "Bowser extension probe", false, done);
        }

        /// Runs a script in the open hosted page.
        pub(in crate::engine) fn extension_page_script(
            &self,
            script: &str,
            done: Box<dyn FnOnce(Option<String>)>,
        ) {
            let webview =
                self.core.borrow().extension_page.as_ref().map(|page| page.webview.clone());
            match webview {
                Some(webview) => super::super::extension_page::execute_script(&webview, script, done),
                None => done(None),
            }
        }
    }

    #[test]
    fn switching_tabs_shows_the_next_before_hiding_the_previous() {
        let steps = switch_steps(Some(TabId(1)), TabId(2), false);
        assert_eq!(steps, vec![SwitchStep::Show(TabId(2)), SwitchStep::Hide(TabId(1))]);
        let show = steps.iter().position(|s| *s == SwitchStep::Show(TabId(2))).unwrap();
        let hide = steps.iter().position(|s| *s == SwitchStep::Hide(TabId(1))).unwrap();
        assert!(show < hide);
    }

    #[test]
    fn switching_to_the_same_or_an_onscreen_tab_hides_nothing() {
        let same = switch_steps(Some(TabId(3)), TabId(3), false);
        assert_eq!(same, vec![SwitchStep::Show(TabId(3))]);
        let onscreen = switch_steps(Some(TabId(1)), TabId(2), true);
        assert_eq!(onscreen, vec![SwitchStep::Show(TabId(2))]);
        let first = switch_steps(None, TabId(2), false);
        assert_eq!(first, vec![SwitchStep::Show(TabId(2))]);
    }

    #[test]
    fn pane_switches_show_every_next_pane_before_hiding_old_ones() {
        let steps = pane_switch_steps(&[TabId(2), TabId(3)], &[TabId(1), TabId(4)]);
        assert_eq!(
            steps,
            vec![
                SwitchStep::Show(TabId(2)),
                SwitchStep::Show(TabId(3)),
                SwitchStep::Hide(TabId(1)),
                SwitchStep::Hide(TabId(4)),
            ]
        );
    }

    #[test]
    fn engine_never_blocks_on_a_webview2_completion() {
        // Tab controllers are configured inside WebView2's controller-created callback, where a
        // nested pump never receives the awaited completion: the login script wait hung every
        // non-guest tab before its first page. Completions must be handled asynchronously.
        let source = include_str!("webview2.rs");
        for blocking in [concat!("wait_for_async", "_operation"), concat!("wait_with", "_pump")] {
            assert!(!source.contains(blocking), "src/engine/webview2.rs uses {blocking}");
        }
    }

    #[test]
    fn private_tab_guards_stay_wired_into_the_engine() {
        // The controller code is not unit-testable without WebView2, so these source checks keep the
        // private-tab guarantees from being dropped by a refactor.
        let source = include_str!("webview2.rs");
        let production = &source[..source.find("#[cfg(test)]").expect("test module")];
        for needle in [
            // Created InPrivate, in the named private profile, and verified afterwards.
            "SetIsInPrivateModeEnabled(true)",
            "create_private_controller(&environment, parent, &handler)",
            "verify_private_profile(&webview)?",
            // Always-on blocker, forced Strict tracking prevention, no stored permission allows.
            "block_hit_private(&buffer, &site_host)",
            "should_block_private(url.as_str(), &top_host)",
            "crate::private::tracking_level(private, state.tracking_policy.level)",
            "crate::private::permission_allowed(private, stored)",
            "crate::private::permission_allowed(private, stored_popups)",
            // No sign-in capture channel, and never mistaken for the extensions' profile.
            "Store::SignInCapture, private, false",
            ".filter(|_| !private)",
            // Downloads: private tabs are cancelled inside the shared handler, which gets the flag.
            "super::download::handle_starting(&args, &download_source, &download_hooks)",
            "owner: super::download::tab_owner(tab_id.0),",
            // Fails closed without a private blocker.
            "state.private_blocker.is_none()",
        ] {
            assert!(production.contains(needle), "missing in the engine: {needle}");
        }
    }

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
    fn login_message_policy_requires_an_enabled_non_guest_vault() {
        assert!(password_login_messages_enabled(true, false));
        assert!(!password_login_messages_enabled(false, false));
        assert!(!password_login_messages_enabled(true, true));
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
    fn json_string_literal_escapes_the_characters_that_would_break_out() {
        assert_eq!(json_string_literal(".textads"), "\".textads\"");
        assert_eq!(json_string_literal("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_string_literal("a\\b"), "\"a\\\\b\"");
        assert_eq!(json_string_literal("a\nb"), "\"a\\nb\"");
        assert_eq!(json_string_literal("</style>"), "\"\\u003c/style>\"");
    }

    #[test]
    fn cosmetic_install_script_embeds_the_css_as_a_string_literal() {
        let script = cosmetic_install_script(".adbox.banner_ads.adsbox{display:none!important}");
        assert!(script.contains("bowser-cosmetic"));
        assert!(script.contains("\".adbox.banner_ads.adsbox{display:none!important}\""));
        assert!(script.contains("createElement('style')"));
    }

    #[test]
    fn cosmetic_remove_script_targets_the_same_element_id() {
        assert!(COSMETIC_REMOVE_SCRIPT.contains("bowser-cosmetic"));
        assert!(COSMETIC_REMOVE_SCRIPT.contains(".remove()"));
    }

    #[test]
    fn a_page_without_cosmetic_css_runs_a_script_only_to_remove_an_earlier_style() {
        let css = ".adbox{display:none!important}";
        assert_eq!(cosmetic_script(Some(css), false), Some(cosmetic_install_script(css)));
        assert_eq!(cosmetic_script(Some(css), true), Some(cosmetic_install_script(css)));
        assert_eq!(cosmetic_script(None, true).as_deref(), Some(COSMETIC_REMOVE_SCRIPT));
        assert_eq!(cosmetic_script(None, false), None);
    }

    #[test]
    fn page_requests_reach_the_matcher_only_through_narrow_filters() {
        let workers = COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_SHARED_WORKER
            | COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_SERVICE_WORKER;
        // No leftover rule: no page filter at all, only the worker requests.
        assert_eq!(request_filter_uris(Vec::new()), vec![("*".to_owned(), workers)]);
        // Leftover rules: their filters for the page, `*` for workers only.
        assert_eq!(
            request_filter_uris(vec!["*://*google.com/??????/*".to_owned()]),
            vec![
                (
                    "*://*google.com/??????/*".to_owned(),
                    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_DOCUMENT
                ),
                ("*".to_owned(), workers),
            ]
        );
        // A rule that can match anything keeps every request, as before.
        assert_eq!(
            request_filter_uris(vec!["*".to_owned()]),
            vec![("*".to_owned(), COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL)]
        );
        // The built-in list needs no page-wide `*`.
        let builtin = crate::blocker::BlockerState::new(crate::blocker::Blocker::from_lists(None), &[]);
        let uris = request_filter_uris(builtin.request_filters());
        assert!(
            uris.iter()
                .filter(|(_, kinds)| kinds.contains(COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_DOCUMENT))
                .all(|(uri, _)| uri != "*")
        );
    }

    #[test]
    fn the_request_filters_and_lazy_legacy_list_stay_wired_into_the_engine() {
        let source = include_str!("webview2.rs");
        let production = &source[..source.find("#[cfg(test)]").expect("test module")];
        for needle in [
            "blocker.borrow().request_filters()",
            "request_filter_uris(request_filters)",
            "let legacy = blocked_urls_legacy_params(&rejected);",
            "state.response_patterns(top_host, private)",
            "cosmetic_script(css.as_deref(), installed)",
        ] {
            assert!(production.contains(needle), "missing in the engine: {needle}");
        }
        assert!(!production.contains(concat!("AddWebResourceRequestedFilter(w!(", "\"*\")")));
        assert!(!production.contains(concat!("let queued_legacy", " =")));
    }

    #[test]
    fn blocked_urls_params_builds_the_devtools_json() {
        assert_eq!(blocked_urls_params(&[]), r#"{"urlPatterns":[]}"#);
        assert_eq!(
            blocked_urls_params(&["*://doubleclick.net/*".to_owned()]),
            r#"{"urlPatterns":[{"urlPattern":"*://doubleclick.net/*","block":true}]}"#
        );
        assert_eq!(blocked_urls_legacy_params(&[]), r#"{"urls":[]}"#);
        assert_eq!(
            blocked_urls_legacy_params(&["*://doubleclick.net/*".to_owned()]),
            r#"{"urls":["*://doubleclick.net/*"]}"#
        );
        assert!(!blocked_urls_params(&["*://doubleclick.net/*".to_owned()]).contains("\"urls\""));
        assert_eq!(fetch_enable_params(&[]), r#"{"patterns":[]}"#);
    }

    #[test]
    fn a_devtools_protocol_error_is_the_code_object_not_an_empty_success() {
        assert!(!devtools_protocol_error(""));
        assert!(!devtools_protocol_error("{}"));
        assert!(!devtools_protocol_error("{\"result\":{}}"));
        assert!(devtools_protocol_error(
            "{\"code\":-32601,\"message\":\"'Network.setBlockedURLs' wasn't found\"}"
        ));
    }

    #[test]
    fn fetch_enable_params_add_the_youtube_player_responses_at_the_response_stage() {
        let responses = &crate::blocker::youtube::RESPONSE_PATTERNS;
        assert_eq!(
            fetch_enable_params(responses),
            concat!(
                r#"{"patterns":["#,
                r#"{"urlPattern":"*://*.youtube.com/youtubei/v1/player\\?*","requestStage":"Response"},"#,
                r#"{"urlPattern":"*://*.youtube-nocookie.com/youtubei/v1/player\\?*","requestStage":"Response"},"#,
                r#"{"urlPattern":"*://*.youtube.com/watch\\?*","resourceType":"Document","requestStage":"Response"}]}"#,
            )
        );
        assert!(
            fetch_enable_params(responses)
                .starts_with(r#"{"patterns":[{"urlPattern":"*://*.youtube.com/"#)
        );
        assert!(!fetch_enable_params(responses).contains("Request\"}"));
    }

    #[test]
    fn a_response_stage_pause_is_never_mistaken_for_a_blocked_request() {
        let request = r#"{"requestId":"interception-job-1.0","request":{"url":"https://doubleclick.net/x","headers":{"responseStatusCode":"1"}},"resourceType":"Fetch"}"#;
        assert_eq!(paused_stage(request), PausedStage::Request);
        let response = r#"{"requestId":"interception-job-2.0","request":{"url":"https://www.youtube.com/youtubei/v1/player?prettyPrint=false","method":"POST","headers":{}},"frameId":"F","resourceType":"Fetch","responseStatusCode":200,"responseHeaders":[{"name":"content-type","value":"application/json; charset=UTF-8"},{"name":"content-encoding","value":"br"},{"name":"set-cookie","value":"YSC=a; Path=/"}],"networkId":"N"}"#;
        assert_eq!(
            paused_stage(response),
            PausedStage::Response(Some(PausedResponse {
                url: "https://www.youtube.com/youtubei/v1/player?prettyPrint=false".to_owned(),
                status: Some(200),
                headers: vec![
                    (
                        "content-type".to_owned(),
                        "application/json; charset=UTF-8".to_owned()
                    ),
                    ("content-encoding".to_owned(), "br".to_owned()),
                    ("set-cookie".to_owned(), "YSC=a; Path=/".to_owned()),
                ],
            }))
        );
        let failed = r#"{"requestId":"r","request":{"url":"https://www.youtube.com/watch?v=x"},"resourceType":"Document","responseErrorReason":"ConnectionReset"}"#;
        assert!(matches!(
            paused_stage(failed),
            PausedStage::Response(Some(PausedResponse { status: None, .. }))
        ));
        // Unreadable, but plainly a response: continued, not failed.
        assert_eq!(
            paused_stage(r#"{"requestId":"r","responseStatusCode":200,"#),
            PausedStage::Response(None)
        );
    }

    #[test]
    fn only_utf8_json_and_html_bodies_are_rewritten() {
        let headers = |value: &str| vec![("Content-Type".to_owned(), value.to_owned())];
        for ok in [
            "application/json; charset=UTF-8",
            "application/json",
            "text/html; charset=utf-8",
            "text/html; charset=\"utf-8\"",
        ] {
            assert!(utf8_text_response(&headers(ok)), "{ok}");
        }
        for not in [
            "text/html; charset=windows-1252",
            "application/x-protobuf",
            "video/mp4",
            "text/plain",
        ] {
            assert!(!utf8_text_response(&headers(not)), "{not}");
        }
        assert!(!utf8_text_response(&[]));
    }

    #[test]
    fn a_response_body_is_read_whether_or_not_devtools_base64_encoded_it() {
        assert_eq!(
            response_body_text(r#"{"body":"{\"adSlots\":[]}","base64Encoded":false}"#).as_deref(),
            Some(r#"{"adSlots":[]}"#)
        );
        assert_eq!(
            response_body_text(r#"{"body":"eyJhZFNsb3RzIjpbXX0=","base64Encoded":true}"#)
                .as_deref(),
            Some(r#"{"adSlots":[]}"#)
        );
        assert_eq!(
            response_body_text(r#"{"body":"/w==","base64Encoded":true}"#),
            None
        );
        assert_eq!(response_body_text(r#"{"base64Encoded":false}"#), None);
    }

    #[test]
    fn fulfill_params_keep_status_and_headers_but_drop_the_wire_encoding() {
        let headers = vec![
            (
                "content-type".to_owned(),
                "application/json; charset=UTF-8".to_owned(),
            ),
            ("Content-Encoding".to_owned(), "br".to_owned()),
            ("content-length".to_owned(), "999".to_owned()),
            ("set-cookie".to_owned(), "YSC=a; Path=/".to_owned()),
        ];
        assert_eq!(
            fulfill_params("job-1", 200, &headers, b"{}"),
            r#"{"requestId":"job-1","responseCode":200,"responseHeaders":[{"name":"content-type","value":"application/json; charset=UTF-8"},{"name":"set-cookie","value":"YSC=a; Path=/"}],"body":"e30="}"#
        );
    }

    #[test]
    fn the_player_response_rewrite_stays_wired_into_the_fetch_listener() {
        let source = include_str!("webview2.rs");
        let production = &source[..source.find("#[cfg(test)]").expect("test module")];
        for needle in [
            "if let PausedStage::Response(paused) = paused_stage(&json)",
            ".player_ads_rewrite(&paused.url, &top_host, private)",
            "fetch_enable_params(responses)",
            "Network.loadingFailed",
            "blockedReason",
            "enable_network_then_block",
        ] {
            assert!(
                production.contains(needle),
                "missing in the engine: {needle}"
            );
        }
        // Host rules are not paused, and a paused response is continued or rewritten
        // before any other pause is continued.
        assert!(!production.contains("fetch_fail"));
        assert!(!production.contains("fetch_armed"));
        assert!(!production.contains("document_blocked"));
        let response_branch = production.find("paused_stage(&json)").unwrap();
        let listener_continue = production.find("fetch_continue(&sender, &request_id)").unwrap();
        assert!(response_branch < listener_continue);
    }

    #[test]
    fn a_paused_fetch_event_yields_its_request_id_and_resource_type() {
        let json = r#"{"requestId":"interception-job-1.0","request":{"url":"https://doubleclick.net/favicon.ico?adblock_test=1"},"resourceType":"Fetch"}"#;
        assert_eq!(
            json_string_field(json, "requestId").as_deref(),
            Some("interception-job-1.0")
        );
        assert_eq!(json_string_field(json, "resourceType").as_deref(), Some("Fetch"));
        assert_eq!(json_string_field(json, "missing"), None);
        assert_ne!(json_string_field(json, "resourceType").as_deref(), Some("Document"));
    }

    #[test]
    fn popup_policy_contains_only_user_initiated_http_and_https() {
        let page = |text: &str| Url::parse(text).unwrap();
        assert_eq!(popup_decision(&page("https://example.com"), true, false, false), PopupDecision::NewTab);
        assert_eq!(popup_decision(&page("http://example.com"), true, false, false), PopupDecision::NewTab);
        assert_eq!(popup_decision(&page("https://example.com"), false, false, false), PopupDecision::Block);
        assert_eq!(popup_decision(&page("mailto:person@example.com"), true, false, false), PopupDecision::Block);
    }

    #[test]
    fn a_stored_popup_allow_opens_script_windows_but_only_as_http_or_https_tabs() {
        let page = |text: &str| Url::parse(text).unwrap();
        // The default stays deny: a script popup with no stored Allow is blocked.
        assert_eq!(popup_decision(&page("https://ads.example/x"), false, false, false), PopupDecision::Block);
        // An allowed site's script popup becomes a new tab.
        assert_eq!(popup_decision(&page("https://pay.example/x"), false, true, false), PopupDecision::NewTab);
        // The allow never widens the scheme gate.
        for value in ["javascript:alert(1)", "data:text/html,hi", "file:///C:/x", "about:blank", "bowser-ui://app/index.html", "mailto:a@b.test", "blob:https://a.test/1"] {
            assert_eq!(popup_decision(&page(value), false, true, false), PopupDecision::Block, "{value}");
            assert_eq!(popup_decision(&page(value), true, true, false), PopupDecision::Block, "{value}");
        }
    }

    #[test]
    fn a_blocked_target_never_opens_a_tab_whatever_the_gesture_or_allow() {
        let page = |text: &str| Url::parse(text).unwrap();
        // An ad target blocks even for an ordinary user-initiated click...
        assert_eq!(
            popup_decision(&page("https://ads.example/x"), true, false, true),
            PopupDecision::Block
        );
        // ...or a site with a stored popup allow.
        assert_eq!(
            popup_decision(&page("https://ads.example/x"), false, true, true),
            PopupDecision::Block
        );
        // An ordinary user-initiated https popup still opens.
        assert_eq!(
            popup_decision(&page("https://example.com/x"), true, false, false),
            PopupDecision::NewTab
        );
        // A script popup without a stored allow still blocks.
        assert_eq!(
            popup_decision(&page("https://example.com/x"), false, false, false),
            PopupDecision::Block
        );
        // An allowed popup of a non-ad URL still opens.
        assert_eq!(
            popup_decision(&page("https://example.com/x"), false, true, false),
            PopupDecision::NewTab
        );
    }

    #[test]
    fn a_cancelled_navigation_is_not_a_failed_load() {
        assert!(!navigation_failure_is_reportable(
            COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED
        ));
        assert!(navigation_failure_is_reportable(
            COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_IS_INVALID
        ));
        assert!(navigation_failure_is_reportable(
            COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_ABORTED
        ));
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
            "bowser-ui://app/index.html?view=toolbar",
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
    fn tabs_never_load_the_html_chrome_scheme() {
        let scheme = crate::chrome_host::assets::SCHEME;
        assert_eq!(scheme, "bowser-ui");
        for value in [
            crate::chrome_host::assets::START_URL,
            crate::chrome_host::assets::TABS_START_URL,
            "bowser-ui://app/",
            "bowser-ui://app/app.js",
            "BOWSER-UI://app/index.html",
            "bowser-ui://other/x",
            "bowser-ui:index.html",
        ] {
            let url = Url::parse(value).unwrap();
            assert_eq!(url.scheme(), scheme, "{value}");
            // Block arm, not the external-protocol fallback, even with a file grant present.
            assert_eq!(
                navigation_start_policy(&url, None),
                NavigationStartPolicy::Block,
                "{value} must be blocked in tabs"
            );
            assert_eq!(
                navigation_start_policy(&url, Some(&url)),
                NavigationStartPolicy::Block,
                "{value} must be blocked in tabs"
            );
            assert!(!navigation_scheme_allowed(&url), "{value}");
            assert!(!frame_navigation_scheme_allowed(&url), "{value}");
            assert_eq!(popup_decision(&url, true, true, false), PopupDecision::Block, "{value}");
        }
    }

    const HOSTED_ID: &str = "hekkainhmjhmmgdonhilehemnlbachak";

    /// A hosted page's grant is for the one URL Bowser built, once. The live probe observed
    /// `chrome-extension` as the scheme (docs/extensions.md); `extension` and `ms-browser-extension`
    /// stay refused even for the same id and path. None of this reaches a tab: see
    /// `extension_origin_schemes_are_not_loadable_in_tabs_today`.
    #[test]
    fn hosted_extension_page_grant_is_one_shot_and_exact_for_the_observed_scheme_only() {
        let granted = Url::parse(&format!("chrome-extension://{HOSTED_ID}/popup.html")).unwrap();
        let policy = HostedPagePolicy::new(&granted).expect("a Bowser-built page URL is grantable");
        for other in [
            format!("chrome-extension://{HOSTED_ID}/other.html"),
            format!("chrome-extension://{HOSTED_ID}/popup.html?x=1"),
            format!("chrome-extension://{HOSTED_ID}/popup.html#x"),
            "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/popup.html".to_owned(),
            format!("extension://{HOSTED_ID}/popup.html"),
            format!("ms-browser-extension://{HOSTED_ID}/popup.html"),
            "https://example.com/".to_owned(),
            "file:///C:/Windows/win.ini".to_owned(),
            "about:blank".to_owned(),
        ] {
            let other = Url::parse(&other).unwrap();
            assert!(!policy.allow_navigation(&other), "{other} must not use the grant");
        }
        // Refusals above did not use up the grant.
        assert!(policy.allow_navigation(&granted), "first navigation to the granted URL");
        assert!(!policy.allow_navigation(&granted), "the grant is one-shot");
    }

    #[test]
    fn hosted_extension_page_policy_cannot_be_built_for_any_other_scheme_or_shape() {
        for refused in [
            format!("extension://{HOSTED_ID}/popup.html"),
            format!("ms-browser-extension://{HOSTED_ID}/popup.html"),
            format!("https://{HOSTED_ID}/popup.html"),
            format!("chrome-extension://{HOSTED_ID}/"),
            format!("chrome-extension://{HOSTED_ID}/popup.html?x=1"),
            "chrome-extension://short/popup.html".to_owned(),
            "file:///C:/Windows/win.ini".to_owned(),
        ] {
            assert!(HostedPagePolicy::new(&Url::parse(&refused).unwrap()).is_none(), "{refused}");
        }
    }

    #[test]
    fn hosted_extension_page_frames_stay_inside_the_granted_extension() {
        let granted = Url::parse(&format!("chrome-extension://{HOSTED_ID}/popup.html")).unwrap();
        let policy = HostedPagePolicy::new(&granted).unwrap();
        for allowed in [
            "about:blank",
            "about:srcdoc",
            &format!("chrome-extension://{HOSTED_ID}/frame.html"),
        ] {
            assert!(policy.allow_frame(&Url::parse(allowed).unwrap()), "{allowed}");
        }
        for refused in [
            "https://example.com/",
            "http://127.0.0.1/",
            "data:text/html,x",
            "blob:chrome-extension://x/y",
            "file:///C:/Windows/win.ini",
            "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/frame.html",
            &format!("extension://{HOSTED_ID}/frame.html"),
            &format!("ms-browser-extension://{HOSTED_ID}/frame.html"),
        ] {
            assert!(!policy.allow_frame(&Url::parse(refused).unwrap()), "{refused}");
        }
    }

    /// Documents what tabs do with the schemes an extension origin might use. The live probe
    /// observed `chrome-extension` (docs/extensions.md) and Bowser still unblocks none of these for
    /// tabs: `ms-browser-extension` has its own Block arm, and the other two fall into the
    /// external-protocol path, which only cancels the OS launch (`LaunchingExternalUriScheme`).
    /// Nothing here lets a tab load one. Hosted pages use `HostedPagePolicy` instead.
    #[test]
    fn extension_origin_schemes_are_not_loadable_in_tabs_today() {
        for (value, expected) in [
            ("ms-browser-extension://abc/page.html", NavigationStartPolicy::Block),
            ("chrome-extension://abc/page.html", NavigationStartPolicy::ExternalProtocol),
            ("extension://abc/page.html", NavigationStartPolicy::ExternalProtocol),
        ] {
            let url = Url::parse(value).unwrap();
            for grant in [None, Some(&url)] {
                assert_eq!(navigation_start_policy(&url, grant), expected, "{value}");
            }
            assert!(!navigation_scheme_allowed(&url), "{value}");
            assert!(!frame_navigation_scheme_allowed(&url), "{value}");
            assert_eq!(popup_decision(&url, true, true, false), PopupDecision::Block, "{value}");
            assert_eq!(top_host_for_source(value), "", "{value}");
            assert!(web_url_from_source(value).is_none(), "{value}");
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

    /// The ungated filter: every setting-gated chord is off, as when the app cannot be reached.
    fn shortcut_forward(
        key: u32,
        kind: COREWEBVIEW2_KEY_EVENT_KIND,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> Option<ShortcutForward> {
        super::shortcut_forward(key, kind, ctrl, shift, alt, shortcuts::GatedChords::default())
    }

    #[test]
    fn gated_chords_are_swallowed_only_while_their_setting_is_on() {
        let down = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
        let on = shortcuts::GatedChords::new(true, true, true);
        // Ctrl+Shift+C and Ctrl+S reach the page unless the setting claims them.
        assert_eq!(shortcut_forward(0x43, down, true, true, false), None);
        assert_eq!(shortcut_forward(0x53, down, true, false, false), None);
        assert_eq!(
            super::shortcut_forward(0x43, down, true, true, false, on),
            forward(0x43, true, true, false)
        );
        assert_eq!(
            super::shortcut_forward(0x53, down, true, false, false, on),
            forward(0x53, true, false, false)
        );
        // Only a key press is claimed, and the setting claims nothing else.
        assert_eq!(
            super::shortcut_forward(0x43, COREWEBVIEW2_KEY_EVENT_KIND_KEY_UP, true, true, false, on),
            None
        );
        assert_eq!(super::shortcut_forward(0x43, down, true, false, false, on), None);
        // Ctrl+S is claimed only in the vertical layout.
        let horizontal = shortcuts::GatedChords::new(true, true, false);
        assert_eq!(super::shortcut_forward(0x53, down, true, false, false, horizontal), None);
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
                        // Chords the command registry adds for commands with no `Command` of their own
                        // (Ctrl+F1, the cheat sheet; page tools' capture and notes; F11 and Ctrl+F11 for
                        // full screen and auto-hide; Ctrl+F7, F8 and F9, the tilings) are forwarded too.
                        && (shortcuts::command_for(key, ctrl, shift, alt).is_some()
                            || [(0x42, true, false, false), (0x70, true, false, false), (0x53, true, true, false), (0x4F, true, true, false), (0x7A, false, false, false), (0x7A, true, false, false),
                                (0x76, true, false, false), (0x77, true, false, false), (0x78, true, false, false)]
                                .contains(&(key, ctrl, shift, alt)))
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
    fn shortcut_forward_swallows_the_dev_tools_chords() {
        let down = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
        // F12 and Ctrl+Shift+I are handled by Bowser (Developer tools), not forwarded to the page.
        assert_eq!(
            shortcut_forward(0x7B, down, false, false, false),
            forward(0x7B, false, false, false)
        );
        assert_eq!(
            shortcut_forward(0x49, down, true, true, false),
            forward(0x49, true, true, false)
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
            (0x7B, true, false, false),  // Ctrl+F12, not the DevTools chord
            (0x49, true, false, false),  // Ctrl+I, italics in editors
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
    fn refused_or_unsupported_resume_never_authorizes_discard_or_runs_a_probe() {
        let answered = Rc::new(std::cell::Cell::new(None));
        let answer = answered.clone();
        resume_then_sleep_probe(
            || Err(Error::from(windows::Win32::Foundation::E_NOINTERFACE)),
            |_| panic!("a failed resume cannot run a page probe"),
            Box::new(move |blocked| answer.set(Some(blocked))),
        );
        assert_eq!(answered.get(), Some(true));
    }

    #[test]
    fn a_suspension_hint_cannot_replace_a_fresh_unsaved_input_or_media_probe() {
        for result in ["true", "false", "null"] {
            let probes = std::cell::Cell::new(0);
            let answered = Rc::new(std::cell::Cell::new(None));
            let answer = answered.clone();
            resume_then_sleep_probe(
                || Ok(()),
                |done| {
                    probes.set(probes.get() + 1);
                    done(sleep_probe_blocks(true, result));
                },
                Box::new(move |blocked| answer.set(Some(blocked))),
            );
            assert_eq!(probes.get(), 1, "every successful resume must inspect the page");
            assert_eq!(answered.get(), Some(result != "false"));
        }
    }

    #[test]
    fn pending_fresh_sleep_probe_does_not_approve_discard_before_answering() {
        type Verdict = Box<dyn FnOnce(bool)>;
        let pending: RefCell<Option<Verdict>> = RefCell::new(None);
        let answered = Rc::new(std::cell::Cell::new(None));
        let answer = answered.clone();
        resume_then_sleep_probe(
            || Ok(()),
            |done| *pending.borrow_mut() = Some(done),
            Box::new(move |blocked| answer.set(Some(blocked))),
        );
        assert_eq!(answered.get(), None, "requesting a probe is not a successful verdict");
        pending.borrow_mut().take().unwrap()(true);
        assert_eq!(answered.get(), Some(true));
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

    #[test]
    fn extension_log_lines_carry_hresult_id_and_name() {
        use windows::core::HRESULT;
        assert_eq!(
            extension_log_line(
                "add",
                Some("ext-0123456789abcdef"),
                Some(HRESULT(0)),
                Some("ciocoifocbggchcecjgibbaeojegkpen"),
                Some("Bowser Probe MV3"),
            ),
            "extension call=add bowser=ext-0123456789abcdef result=ok hresult=0x00000000 id=ciocoifocbggchcecjgibbaeojegkpen name=\"Bowser Probe MV3\""
        );
        // E_ACCESSDENIED is what WebView2 documents for a reserved `_` name.
        assert_eq!(
            extension_log_line("add", None, Some(HRESULT(0x8007_0005_u32 as i32)), None, None),
            "extension call=add bowser=none result=failed hresult=0x80070005 id=none name=none"
        );
        // A completion that reported success without an extension has no HRESULT to report.
        assert_eq!(
            extension_log_line("add", None, None, None, None),
            "extension call=add bowser=none result=no_extension hresult=none id=none name=none"
        );
    }

    #[test]
    fn extension_log_lines_cannot_be_forged_by_a_name_or_id() {
        use windows::core::HRESULT;
        let line = extension_log_line(
            "remove",
            None,
            Some(HRESULT(0)),
            Some("id\r\nextension call=enable"),
            Some("a\nextension call=remove\u{202e}\"b\\"),
        );
        assert_eq!(line.lines().count(), 1, "{line:?}");
        assert!(!line.chars().any(|c| c.is_control() || crate::extensions::is_invisible_format(c)));
        assert!(line.ends_with("name=\"a?extension call=remove??b?\""), "{line}");
        assert!(line.contains("id=id??extension call=enable "), "{line}");
        let long = extension_log_line("add", None, Some(HRESULT(0)), None, Some(&"n".repeat(500)));
        assert!(long.len() < 200, "names are capped: {} bytes", long.len());
    }

    #[test]
    fn profile7_log_line_says_whether_the_runtime_supports_extensions() {
        use windows::core::HRESULT;
        assert_eq!(
            profile7_log_line(HRESULT(0)),
            "extension call=profile7_cast supported=true hresult=0x00000000"
        );
        // E_NOINTERFACE: the runtime has no ICoreWebView2Profile7.
        assert_eq!(
            profile7_log_line(HRESULT(0x8000_4002_u32 as i32)),
            "extension call=profile7_cast supported=false hresult=0x80004002"
        );
    }

    #[test]
    fn a_load_result_is_recorded_and_then_announced() {
        let id = "ext-00000000000000ab";
        let announced = std::cell::Cell::new(0);
        let seen_by_listener = std::cell::RefCell::new(None);
        record_load(id, Err("boom".to_owned()), || {
            announced.set(announced.get() + 1);
            // Windows catch up from the record, so it must already be written when they hear.
            *seen_by_listener.borrow_mut() = Some(crate::extensions::load_failure(id));
        });
        assert_eq!(announced.get(), 1);
        assert_eq!(seen_by_listener.into_inner(), Some(Some("boom".to_owned())));
        record_load(id, Ok(()), || announced.set(announced.get() + 1));
        assert_eq!(announced.get(), 2, "a success is announced too: the failure it clears left the toolbar");
        assert_eq!(crate::extensions::load_failure(id), None);
    }

    #[test]
    fn every_engine_load_result_is_announced_to_the_windows() {
        // The startup paths cannot run without WebView2; this keeps one of them from recording a
        // result without telling the windows. Calls: report_load, the profile refusal, the runtime floor.
        let source = include_str!("webview2.rs");
        let production = &source[..source.find("#[cfg(test)]").expect("test module")];
        assert_eq!(production.matches("crate::extensions::record_load_result(").count(), 1, "only record_load records");
        assert_eq!(production.matches("record_load(").count() - production.matches("fn record_load(").count(), 3);
    }

    #[test]
    fn runtime_floor_for_extensions_is_134_0_3124_44() {
        for version in [
            "134.0.3124.44",
            "134.0.3124.45",
            "134.0.3125.0",
            "135.0.0.0",
            "155.0.4283.45",
            " 155.0.4283.45\n",
            // Preview channels append a word to the version.
            "156.0.4300.0 beta",
        ] {
            assert!(runtime_supports_extensions(version), "{version:?}");
        }
        for version in [
            "134.0.3124.43",
            "134.0.3123.999",
            "133.9.9999.99",
            "120.0.2210.55",
            "",
            "garbage",
            "134.0.3124",
            "134.0.3124.44.1",
            "134.0.3124.x",
            "-1.0.0.0",
            "999999999999.0.0.0",
        ] {
            assert!(!runtime_supports_extensions(version), "{version:?}");
        }
    }

    #[test]
    fn runtime_floor_log_line_reports_the_version_and_the_decision() {
        assert_eq!(
            runtime_floor_log_line("155.0.4283.45", true),
            "extension call=runtime_floor version=155.0.4283.45 floor=134.0.3124.44 enabled=true"
        );
        assert_eq!(
            runtime_floor_log_line("120.0.2210.55", false),
            "extension call=runtime_floor version=120.0.2210.55 floor=134.0.3124.44 enabled=false"
        );
        // Text that is not a version is not echoed.
        assert_eq!(
            runtime_floor_log_line("evil\nline", false),
            "extension call=runtime_floor version=unreadable floor=134.0.3124.44 enabled=false"
        );
    }

    #[test]
    fn minimize_hide_does_not_mark_the_active_tab_for_the_30s_suspend_sweep() {
        let core = Rc::new(RefCell::new(EngineCore {
            parent: HWND::default(),
            bounds: RECT::default(),
            environment: None,
            allowed_file: None,
            events: EngineEvents::default(),
            tracking_policy: TrackingPolicy {
                level: Some(TrackingPreventionLevel::Balanced),
                enabled: true,
            },
            blocker: None,
            private_blocker: None,
            private_tabs: HashSet::new(),
            active_tab: Some(TabId(1)),
            visible_tabs: HashSet::from([TabId(1)]),
            pane_bounds: HashMap::new(),
            target: MemoryUsageTarget::Normal,
            tabs: HashMap::new(),
            known_tabs: HashSet::from([TabId(1), TabId(2)]),
            creating: HashSet::new(),
            prewarming: HashSet::new(),
            prewarm_waiting: None,
            waiting_controllers: VecDeque::new(),
            pending: HashMap::new(),
            pending_post: HashMap::new(),
            hidden_since: HashMap::from([(TabId(2), Instant::now() - Duration::from_secs(60))]),
            suspended: HashSet::new(),
            closed: false,
            browser_extensions: false,
            extensions_refused: None,
            extension_profile: None,
            extensions_loaded: HashMap::new(),
            startup_extensions: Vec::new(),
            known_extensions: Vec::new(),
            extension_ids_file: None,
            extension_ids: HashMap::new(),
            permissions: None,
            chrome_ui_scheme: false,
            extension_page: None,
        }));
        let mut engine = WebView2Engine { core: core.clone() };
        // On minimize, set_visible(false) is called on the engine:
        let _ = engine.set_visible(false);
        // Active tab must NOT be added to hidden_since:
        assert!(!core.borrow().hidden_since.contains_key(&TabId(1)));
        // Background tab that was already hidden remains in hidden_since:
        assert!(core.borrow().hidden_since.contains_key(&TabId(2)));

        // 30s suspend sweep runs:
        let _ = engine.suspend_hidden_tabs(30_000);
        // Active tab is not suspended:
        assert!(!core.borrow().suspended.contains(&TabId(1)));
    }
}
