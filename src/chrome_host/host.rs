//! Isolated WebView2 host for the HTML chrome toolbar (see docs/html-chrome-protocol.md).
//!
//! One controller in its own environment (exclusive user-data folder, never shared with browsing
//! profiles). The page can reach nothing except the embedded assets served through the `bowser-ui`
//! custom scheme, and its only channel back to Rust is the string bridge in `protocol.rs`.

use std::{
    cell::RefCell,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    rc::{Rc, Weak},
};

use url::Url;
use webview2_com::{
    CoTaskMemPWSTR, CoreWebView2CustomSchemeRegistration, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    DownloadStartingEventHandler, FrameCreatedEventHandler, FrameNavigationStartingEventHandler,
    IFixedEnvironmentOptions4,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler,
    PermissionRequestedEventHandler, ProcessFailedEventHandler, WebMessageReceivedEventHandler,
    WebResourceRequestedEventHandler, Microsoft::Web::WebView2::Win32::*,
};
use windows::{
    Win32::{
        Foundation::{E_FAIL, E_POINTER, HWND, RECT},
        System::Com::IStream,
        UI::Shell::SHCreateMemStream,
    },
    core::{Error, Interface, PCWSTR, PWSTR, Result},
};

use super::{
    assets::{self, Asset, RESPONSE_HEADERS, START_URL, SCHEME},
    protocol::{Command, parse_command},
};

const TRANSPARENT: COREWEBVIEW2_COLOR = COREWEBVIEW2_COLOR {
    A: 0,
    R: 0,
    G: 0,
    B: 0,
};

type ReadyCallback = Box<dyn FnOnce(Result<ChromeHost>)>;

/// Everything the creation callbacks need. Cheap to clone; all fields are shared or Copy.
#[derive(Clone)]
struct Setup {
    parent: HWND,
    bounds: RECT,
    on_command: Rc<dyn Fn(Command)>,
    on_failed: Rc<dyn Fn()>,
    ready: Rc<RefCell<Option<ReadyCallback>>>,
}

struct State {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
    bounds: RECT,
    /// What the caller last asked for. Defaults to visible; only shown once the page has said `ready`.
    wanted_visible: bool,
    /// Set when the page posts `ready`.
    revealed: bool,
    closed: bool,
    removers: Vec<Box<dyn FnOnce()>>,
}

/// The HTML chrome WebView2 controller. Clones share one controller.
#[derive(Clone)]
pub struct ChromeHost {
    state: Rc<RefCell<State>>,
}

impl ChromeHost {
    /// Starts creating the chrome environment and controller. `on_ready` runs once, with the host
    /// or the reason creation failed. The controller stays hidden until the page posts `ready`.
    /// `user_data_dir` must be the chrome-only folder; it is verified against WebView2's report.
    pub fn begin_create(
        parent: HWND,
        bounds: RECT,
        user_data_dir: &Path,
        on_command: Rc<dyn Fn(Command)>,
        on_failed: Rc<dyn Fn()>,
        on_ready: impl FnOnce(Result<ChromeHost>) + 'static,
    ) -> Result<()> {
        std::fs::create_dir_all(user_data_dir).map_err(|error| {
            fail(&format!("Could not create isolated chrome profile: {error}"))
        })?;
        let profile: Vec<u16> = user_data_dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let expected = std::fs::canonicalize(user_data_dir)
            .unwrap_or_else(|_| user_data_dir.to_path_buf());

        let options = CoreWebView2EnvironmentOptions::default();
        let options4: IFixedEnvironmentOptions4 = options.into();
        let scheme: ICoreWebView2CustomSchemeRegistration =
            CoreWebView2CustomSchemeRegistration::new(SCHEME.to_owned()).into();
        unsafe {
            scheme.SetTreatAsSecure(true)?;
            scheme.SetHasAuthorityComponent(true)?;
            // The generated wrapper method is private (the macro drops the trait's visibility), so
            // the COM slot is called through the vtable; the signature is the same one webview2-com
            // uses in its own tests.
            let schemes = [scheme.as_raw()];
            (Interface::vtable(&options4).SetCustomSchemeRegistrations)(
                Interface::as_raw(&options4),
                1,
                schemes.as_ptr(),
            )
            .ok()?;
        }
        let options1: ICoreWebView2EnvironmentOptions = options4.cast()?;
        let options2: ICoreWebView2EnvironmentOptions2 = options4.cast()?;
        unsafe { options2.SetExclusiveUserDataFolderAccess(true)? };

        let setup = Setup {
            parent,
            bounds,
            on_command,
            on_failed,
            ready: Rc::new(RefCell::new(Some(Box::new(on_ready)))),
        };
        let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
            move |status, environment| {
                let result = match (status, environment) {
                    (Ok(()), Some(environment)) => {
                        verify_user_data_folder(&environment, &expected)
                            .and_then(|()| create_controller(&environment, setup.clone()))
                    }
                    (Err(error), _) => Err(error),
                    _ => Err(Error::from(E_POINTER)),
                };
                if let Err(error) = result {
                    finish(&setup.ready, Err(error));
                }
                Ok(())
            },
        ));
        unsafe {
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                PCWSTR(profile.as_ptr()),
                &options1,
                &handler,
            )
        }
    }

    pub fn set_bounds(&self, bounds: RECT) -> Result<()> {
        let mut state = self.state.borrow_mut();
        if state.closed {
            return Ok(());
        }
        state.bounds = bounds;
        unsafe { state.controller.SetBounds(bounds) }
    }

    /// Sends one JSON message to the page. Dropped silently once the host is closed.
    pub fn post_json(&self, json: &str) -> Result<()> {
        let state = self.state.borrow();
        if state.closed {
            return Ok(());
        }
        let message = wide(json);
        unsafe { state.webview.PostWebMessageAsJson(PCWSTR(message.as_ptr())) }
    }

    /// Moves keyboard focus into the chrome page (programmatic move).
    pub fn focus(&self) -> Result<()> {
        let state = self.state.borrow();
        if state.closed {
            return Ok(());
        }
        unsafe {
            state
                .controller
                .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)
        }
    }

    /// Records the caller's visibility. The controller is only shown once the page is ready.
    pub fn set_visible(&self, visible: bool) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.wanted_visible = visible;
        if state.closed || !state.revealed {
            return Ok(());
        }
        unsafe { state.controller.SetIsVisible(visible) }
    }

    pub fn close(&self) {
        let mut state = self.state.borrow_mut();
        close_state(&mut state);
    }
}

impl Drop for ChromeHost {
    fn drop(&mut self) {
        if Rc::strong_count(&self.state) == 1 {
            self.close();
        }
    }
}

fn close_state(state: &mut State) {
    if state.closed {
        return;
    }
    state.closed = true;
    for remove in state.removers.drain(..) {
        remove();
    }
    let _ = unsafe { state.controller.Close() };
}

fn fail(message: &str) -> Error {
    Error::new(E_FAIL, message)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Runs the one-shot ready callback, if it has not run yet.
fn finish(ready: &Rc<RefCell<Option<ReadyCallback>>>, result: Result<ChromeHost>) {
    let callback = ready.borrow_mut().take();
    if let Some(callback) = callback {
        callback(result);
    }
}

fn verify_user_data_folder(environment: &ICoreWebView2Environment, expected: &Path) -> Result<()> {
    let environment7: ICoreWebView2Environment7 = environment.cast()?;
    let mut actual_path = PWSTR::null();
    unsafe { environment7.UserDataFolder(&mut actual_path)? };
    let actual = CoTaskMemPWSTR::from(actual_path).to_string();
    let actual = std::fs::canonicalize(&actual).unwrap_or_else(|_| PathBuf::from(&actual));
    if actual
        .to_string_lossy()
        .eq_ignore_ascii_case(expected.to_string_lossy().as_ref())
    {
        Ok(())
    } else {
        Err(fail("WebView2 did not use the isolated chrome profile directory"))
    }
}

fn create_controller(environment: &ICoreWebView2Environment, setup: Setup) -> Result<()> {
    let environment10: ICoreWebView2Environment10 = environment.cast()?;
    let options = unsafe { environment10.CreateCoreWebView2ControllerOptions()? };
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
            bounds: setup.bounds,
            wanted_visible: true,
            revealed: false,
            closed: false,
            removers: Vec::new(),
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
        controller.SetBounds(setup.bounds)?;
        controller.SetIsVisible(false)?;
        if !background_at_creation {
            let controller2: ICoreWebView2Controller2 = controller.cast()?;
            controller2.SetDefaultBackgroundColor(TRANSPARENT)?;
        }
        if let Ok(controller4) = controller.cast::<ICoreWebView2Controller4>() {
            controller4.SetAllowExternalDrop(false)?;
        }
    }
    configure_settings(&webview)?;
    install_handlers(host, environment, setup, &webview)?;
    let start = wide(START_URL);
    unsafe { webview.Navigate(PCWSTR(start.as_ptr())) }
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

    unsafe {
        // Top-level navigation: only the chrome start URL may load.
        webview.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    let allowed = args.Uri(&mut uri).is_ok()
                        && is_start_url(&CoTaskMemPWSTR::from(uri).to_string());
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
            &ProcessFailedEventHandler::create(Box::new(move |_sender, _args| {
                on_failed();
                Ok(())
            })),
            &mut token,
        )?;
        let w = webview.clone();
        removers.push(Box::new(move || {
            let _ = w.remove_ProcessFailed(token);
        }));

        // The bridge: top-level page only, string payloads only.
        let on_command = setup.on_command.clone();
        let weak_bridge = weak.clone();
        webview.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else {
                    return Ok(());
                };
                let mut source = PWSTR::null();
                if sender.Source(&mut source).is_err() {
                    return Ok(());
                }
                if !is_start_url(&CoTaskMemPWSTR::from(source).to_string()) {
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
                if matches!(command, Command::Ready) {
                    reveal(&weak_bridge);
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

        // Every request is answered here: embedded asset or 404. Nothing reaches the network.
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
        let filter = wide("*");
        webview.AddWebResourceRequestedFilter(
            PCWSTR(filter.as_ptr()),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
        )?;
    }

    host.state.borrow_mut().removers.append(&mut removers);
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

/// The page said `ready`: show the controller if the caller wants it visible.
fn reveal(weak: &Weak<RefCell<State>>) {
    let Some(shared) = weak.upgrade() else {
        return;
    };
    let mut state = shared.borrow_mut();
    state.revealed = true;
    if !state.closed && state.wanted_visible {
        let _ = unsafe { state.controller.SetIsVisible(true) };
    }
}

/// Exact match against the chrome start URL. Nothing else may load in the chrome webview.
fn is_start_url(uri: &str) -> bool {
    uri == START_URL
}

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
    fn start_url_must_match_exactly() {
        assert!(is_start_url(START_URL));
        assert!(is_start_url("bowser-ui://app/index.html"));
        for uri in [
            "",
            " bowser-ui://app/index.html",
            "bowser-ui://app/index.html?x=1",
            "bowser-ui://app/index.html#top",
            "bowser-ui://app/",
            "bowser-ui://app/app.js",
            "BOWSER-UI://app/index.html",
            "bowser-ui://APP/index.html",
            "bowser-ui://app/index.html/",
            "https://example.com/",
        ] {
            assert!(!is_start_url(uri), "{uri:?} should not be the start URL");
        }
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

    #[test]
    fn unknown_app_paths_are_not_assets() {
        let path = request_path("bowser-ui://app/missing.html").expect("app host");
        assert_eq!(path, "/missing.html");
        assert!(assets::lookup(&path).is_none());
        let page = request_path(START_URL).and_then(|path| assets::lookup(&path));
        assert!(page.is_some());
    }
}
