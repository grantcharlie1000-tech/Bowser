use std::{cell::RefCell, os::windows::ffi::OsStrExt, path::Path, rc::Rc};

use url::Url;
use webview2_com::{
    AcceleratorKeyPressedEventHandler, CoTaskMemPWSTR, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    DocumentTitleChangedEventHandler, DownloadStartingEventHandler,
    Microsoft::Web::WebView2::Win32::*, NavigationCompletedEventHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler,
    PermissionRequestedEventHandler, ProcessFailedEventHandler, SourceChangedEventHandler,
};
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        UI::{
            Input::KeyboardAndMouse::{GetKeyState, VK_MENU},
            WindowsAndMessaging::SetWindowTextW,
        },
    },
    core::{BOOL, Error, Interface, PCWSTR, PWSTR, Ref, Result, implement},
};

use super::{
    EngineAdapter, EngineEvents, MemoryTargetApply, MemoryUsageTarget, TrackingPreventionLevel,
};

#[derive(Debug)]
pub struct RuntimeUnavailable(pub String);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PopupDecision {
    SameView,
    Block,
}

pub fn popup_decision(uri: &Url, user_initiated: bool) -> PopupDecision {
    if user_initiated && matches!(uri.scheme(), "http" | "https") {
        PopupDecision::SameView
    } else {
        PopupDecision::Block
    }
}

fn history_action(key: u32, kind: COREWEBVIEW2_KEY_EVENT_KIND, alt_pressed: bool) -> Option<bool> {
    if !alt_pressed
        || !matches!(
            kind,
            COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN | COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
        )
    {
        return None;
    }
    match key as u16 {
        0x25 => Some(false),
        0x27 => Some(true),
        _ => None,
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NavigationStartPolicy {
    Allow,
    Block,
    ExternalProtocol,
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

fn web_url_from_source(source: &str) -> Option<Url> {
    let url = Url::parse(source).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}

fn current_web_url(webview: &ICoreWebView2) -> Option<Url> {
    let mut source = PWSTR::null();
    unsafe { webview.Source(&mut source) }.ok()?;
    web_url_from_source(&CoTaskMemPWSTR::from(source).to_string())
}

impl std::fmt::Display for RuntimeUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub struct WebView2Engine {
    environment: Option<ICoreWebView2Environment>,
    controller: Option<ICoreWebView2Controller>,
    webview: Option<ICoreWebView2>,
    allowed_file: Option<Url>,
    memory_target_api: Option<MemoryTargetApi>,
}

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
                let completion_ctl = completion_env.clone();
                let completion_create = completion_env.clone();
                let handler_events = event_handlers.clone();
                let requested_tracking_prevention = TrackingPolicy {
                    level: tracking_prevention,
                    enabled: tracking_prevention_enabled,
                };
                let controller_log = event_handlers.policy_event.clone();
                let allowed_file = allowed_file.clone();
                let controller_environment = environment.clone();
                let controller_handler = CreateCoreWebView2ControllerCompletedHandler::create(
                    Box::new(move |status, controller| {
                        if let Some(log) = controller_log.as_ref() {
                            log("webview_controller_result");
                        }
                        let controller = match (status, controller) {
                            (Ok(()), Some(controller)) => controller,
                            (Err(error), _) => {
                                if let Some(f) = completion_ctl.borrow_mut().take() {
                                    f(Err(error));
                                }
                                return Ok(());
                            }
                            _ => {
                                if let Some(f) = completion_ctl.borrow_mut().take() {
                                    f(Err(Error::from(windows::Win32::Foundation::E_POINTER)));
                                }
                                return Ok(());
                            }
                        };
                        let controller_for_cleanup = controller.clone();
                        if let Some(log) = controller_log.as_ref() {
                            log("webview_configure_started");
                        }
                        let configured = unsafe {
                            configure_controller(
                                &controller_environment,
                                controller,
                                parent,
                                bounds,
                                allowed_file,
                                handler_events,
                                requested_tracking_prevention,
                            )
                        };
                        if let Some(log) = controller_log.as_ref() {
                            log("webview_configure_finished");
                        }
                        if configured.is_err() {
                            let _ = unsafe { controller_for_cleanup.Close() };
                        }
                        if let Some(f) = completion_ctl.borrow_mut().take() {
                            f(configured);
                        }
                        Ok(())
                    }),
                );
                if let Some(log) = event_handlers.policy_event.as_ref() {
                    log("webview_controller_requested");
                }
                if let Err(error) =
                    unsafe { environment.CreateCoreWebView2Controller(parent, &controller_handler) }
                {
                    if let Some(log) = event_handlers.policy_event.as_ref() {
                        log("webview_controller_request_failed");
                    }
                    if let Some(f) = completion_create.borrow_mut().take() {
                        f(Err(error));
                    }
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
    fn navigate(&mut self, url: &Url) -> Result<()> {
        if !navigation_scheme_allowed(url) && self.allowed_file.as_ref() != Some(url) {
            return Err(Error::from(windows::Win32::Foundation::E_INVALIDARG));
        }
        let url = CoTaskMemPWSTR::from(url.as_str());
        unsafe {
            self.webview
                .as_ref()
                .ok_or_else(|| Error::from(windows::Win32::Foundation::E_POINTER))?
                .Navigate(*url.as_ref().as_pcwstr())
        }
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
        unsafe {
            self.controller
                .as_ref()
                .ok_or_else(|| Error::from(windows::Win32::Foundation::E_POINTER))?
                .SetBounds(bounds)
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

unsafe fn configure_controller(
    environment: &ICoreWebView2Environment,
    controller: ICoreWebView2Controller,
    parent: HWND,
    bounds: RECT,
    allowed_file: Option<Url>,
    events: EngineEvents,
    tracking_policy: TrackingPolicy,
) -> Result<WebView2Engine> {
    unsafe {
        controller.SetBounds(bounds)?;
        controller.SetIsVisible(true)?;
        let webview = controller.CoreWebView2()?;
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
                    match parsed
                        .as_ref()
                        .map(|url| navigation_start_policy(url, allowed_file_for_handler.as_ref()))
                    {
                        Some(NavigationStartPolicy::Allow) => {
                            if let Some(callback) = navigation_started.as_ref() {
                                callback();
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
        let navigation_log = events.policy_event.clone();
        let completed_address_changed = events.address_changed.clone();
        webview.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
                if let Some(args) = args {
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
                            if let (Some(sender), Some(callback)) =
                                (sender.as_ref(), completed_address_changed.as_ref())
                            {
                                if let Some(url) = current_web_url(sender) {
                                    callback(url);
                                }
                            }
                        }
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        let address_changed = events.address_changed.clone();
        webview.add_SourceChanged(
            &SourceChangedEventHandler::create(Box::new(move |sender, _args| {
                if let (Some(sender), Some(callback)) = (sender, address_changed.as_ref()) {
                    if let Some(url) = current_web_url(&sender) {
                        callback(url);
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;

        let title_parent = parent;
        let page_ready = events.page_usable.clone();
        webview.add_DocumentTitleChanged(
            &DocumentTitleChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(sender) = sender {
                    let mut title = PWSTR::null();
                    if sender.DocumentTitle(&mut title).is_ok() {
                        let title = CoTaskMemPWSTR::from(title);
                        let mut text: Vec<u16> = title
                            .to_string()
                            .encode_utf16()
                            .chain(std::iter::once(0))
                            .collect();
                        let _ = SetWindowTextW(title_parent, PCWSTR(text.as_mut_ptr()));
                        if title.to_string().contains("BOWSER_BENCH_READY") {
                            if let Some(callback) = page_ready.as_ref() {
                                callback();
                            }
                        }
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;

        let policy_event = events.policy_event.clone();
        webview.add_PermissionRequested(
            &PermissionRequestedEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let _ = args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY);
                }
                if let Some(log) = policy_event.as_ref() {
                    log("permission_denied");
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
        webview.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(move |sender, args| {
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
                    if decision == PopupDecision::SameView {
                        let url = target.expect("same-view decision requires a parsed target");
                        let text = CoTaskMemPWSTR::from(url.as_str());
                        if let Some(sender) = sender.as_ref() {
                            let _ = sender.Navigate(*text.as_ref().as_pcwstr());
                        }
                        if let Some(log) = policy_event.as_ref() {
                            log("popup_same_view");
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

        controller.add_AcceleratorKeyPressed(
            &AcceleratorKeyPressedEventHandler::create(Box::new(move |sender, args| {
                if let Some(args) = args {
                    let mut key = 0u32;
                    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                    let _ = args.VirtualKey(&mut key);
                    let _ = args.KeyEventKind(&mut kind);
                    let action = history_action(key, kind, GetKeyState(VK_MENU.0 as i32) < 0);
                    if let Some(forward) = action {
                        let view = sender
                            .as_ref()
                            .and_then(|controller| controller.CoreWebView2().ok());
                        let handled = match (forward, view.as_ref()) {
                            (false, Some(view)) => {
                                let mut can_go = BOOL::default();
                                if view.CanGoBack(&mut can_go).is_ok() && can_go.as_bool() {
                                    let _ = view.GoBack();
                                    true
                                } else {
                                    false
                                }
                            }
                            (true, Some(view)) => {
                                let mut can_go = BOOL::default();
                                if view.CanGoForward(&mut can_go).is_ok() && can_go.as_bool() {
                                    let _ = view.GoForward();
                                    true
                                } else {
                                    false
                                }
                            }
                            _ => false,
                        };
                        if handled {
                            let _ = args.SetHandled(true);
                        }
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;

        if let Some(callback) = events.failed.as_ref() {
            let failure_callback = callback.clone();
            webview.add_ProcessFailed(
                &ProcessFailedEventHandler::create(Box::new(move |_sender, _args| {
                    failure_callback(Error::from(windows::Win32::Foundation::E_FAIL));
                    Ok(())
                })),
                &mut token,
            )?;
        }
        Ok(WebView2Engine {
            environment: Some(environment.clone()),
            controller: Some(controller),
            webview: Some(webview),
            allowed_file,
            memory_target_api: None,
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

fn event(log: Option<&Rc<dyn Fn(&'static str)>>, name: &'static str) {
    if let Some(log) = log {
        log(name);
    }
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
            PopupDecision::SameView
        );
        assert_eq!(
            popup_decision(&Url::parse("http://example.com").unwrap(), true),
            PopupDecision::SameView
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
    fn history_shortcuts_accept_alt_keydown_and_system_keydown_only() {
        for kind in [
            COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
            COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
        ] {
            assert_eq!(history_action(0x25, kind, true), Some(false));
            assert_eq!(history_action(0x27, kind, true), Some(true));
        }
        assert_eq!(
            history_action(0x25, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, false),
            None
        );
        assert_eq!(
            history_action(0x41, COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN, true),
            None
        );
        assert_eq!(
            history_action(0x25, COREWEBVIEW2_KEY_EVENT_KIND_KEY_UP, true),
            None
        );
    }
}
