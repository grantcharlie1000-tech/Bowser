//! Isolated WebView2 host for pinned web apps.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    rc::Rc,
};

use url::Url;
use webview2_com::{
    CoTaskMemPWSTR, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    DocumentTitleChangedEventHandler, DownloadStartingEventHandler, FaviconChangedEventHandler,
    GetFaviconCompletedHandler, NavigationCompletedEventHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, PermissionRequestedEventHandler,
    ProcessFailedEventHandler, Microsoft::Web::WebView2::Win32::*,
};
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        System::Com::IStream,
    },
    core::{BOOL, Error, Interface, PCWSTR, PWSTR, Result},
};

type TitleChangedCallback = Rc<dyn Fn(&str, String)>;
type FaviconChangedCallback = Rc<dyn Fn(&str, Option<Url>, Option<Vec<u8>>)>;
type NavigationCallback = Rc<dyn Fn(&str, AppNavigationState)>;
type FailureCallback = Rc<dyn Fn(&str, String)>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppNavigationState {
    Loading,
    Ready,
    Failed,
}

#[derive(Clone, Default)]
pub struct WebAppEvents {
    pub title_changed: Option<TitleChangedCallback>,
    pub favicon_changed: Option<FaviconChangedCallback>,
    pub navigation_state: Option<NavigationCallback>,
    pub failed: Option<FailureCallback>,
    pub on_open_in_tab: Option<Rc<dyn Fn(Url)>>,
}

struct WebAppView {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
    remove_handlers: Vec<Box<dyn Fn()>>,
}

struct HostState {
    parent: HWND,
    bounds: RECT,
    environment: ICoreWebView2Environment,
    current_app: Option<String>,
    apps: HashMap<String, WebAppView>,
    events: WebAppEvents,
    closed: bool,
}

/// One secondary environment shared by all pinned web apps. Web apps do not use Bowser's
/// WebMessageReceived bridge or any native script injection.
#[derive(Clone)]
pub struct WebAppHost {
    state: Rc<RefCell<HostState>>,
}

impl WebAppHost {
    /// Starts the shared WebApps environment. Call only when the first app panel is opened.
    pub fn begin_create<F>(
        parent: HWND,
        bounds: RECT,
        web_apps_user_data_dir: &Path,
        events: WebAppEvents,
        on_ready: F,
    ) -> Result<()>
    where
        F: FnOnce(Result<Self>) + 'static,
    {
        std::fs::create_dir_all(web_apps_user_data_dir).map_err(|error| {
            Error::new(
                windows::core::HRESULT(0x80070005u32 as i32),
                &format!("Could not create isolated WebApps profile: {error}"),
            )
        })?;
        let profile: Vec<u16> = web_apps_user_data_dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let expected = std::fs::canonicalize(web_apps_user_data_dir)
            .unwrap_or_else(|_| web_apps_user_data_dir.to_path_buf());
        let options = CoreWebView2EnvironmentOptions::default();
        let options2: ICoreWebView2EnvironmentOptions2 = options.into();
        unsafe { options2.SetExclusiveUserDataFolderAccess(true)?; }
        let options1: ICoreWebView2EnvironmentOptions = options2.cast()?;
        let completion = Rc::new(RefCell::new(Some(on_ready)));
        let done = completion.clone();
        let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
            move |status, environment| {
                let result = match (status, environment) {
                    (Ok(()), Some(environment)) => {
                        let path_result = (|| -> Result<()> {
                            let environment7: ICoreWebView2Environment7 = environment.cast()?;
                            let mut actual_path = PWSTR::null();
                            unsafe { environment7.UserDataFolder(&mut actual_path)?; }
                            let actual = CoTaskMemPWSTR::from(actual_path).to_string();
                            let actual = std::fs::canonicalize(&actual)
                                .unwrap_or_else(|_| PathBuf::from(&actual));
                            if !actual.to_string_lossy().eq_ignore_ascii_case(
                                expected.to_string_lossy().as_ref(),
                            ) {
                                return Err(Error::new(
                                    windows::core::HRESULT(0x80070005u32 as i32),
                                    "WebView2 did not use the isolated WebApps profile directory",
                                ));
                            }
                            Ok(())
                        })();
                        path_result.map(|()| WebAppHost {
                            state: Rc::new(RefCell::new(HostState {
                                parent,
                                bounds,
                                environment,
                                current_app: None,
                                apps: HashMap::new(),
                                events: events.clone(),
                                closed: false,
                            })),
                        })
                    }
                    (Err(error), _) => Err(error),
                    _ => Err(Error::from(windows::Win32::Foundation::E_POINTER)),
                };
                if let Some(callback) = done.borrow_mut().take() {
                    callback(result);
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

    /// Shows an existing app controller or creates it once, then loads its initial URL.
    pub fn open_app(&self, app_id: &str, url: &str) -> Result<()> {
        let url = validate_app_url(url).map_err(|_| Error::new(
            windows::core::HRESULT(0x80070057u32 as i32), "Invalid web app URL"))?;
        {
            let mut state = self.state.borrow_mut();
            if state.closed {
                return Err(Error::from(windows::Win32::Foundation::E_FAIL));
            }
            for (id, app) in &state.apps {
                unsafe { app.controller.SetIsVisible(id == app_id)?; }
            }
            state.current_app = Some(app_id.to_owned());
            if let Some(app) = state.apps.get(app_id) {
                unsafe { app.controller.SetBounds(state.bounds)?; }
                let mut current = PWSTR::null();
                unsafe { app.webview.Source(&mut current)?; }
                let current = CoTaskMemPWSTR::from(current).to_string();
                if current.is_empty() {
                    unsafe { app.webview.Navigate(PCWSTR::from_raw(wide(&url.to_string()).as_ptr()))?; }
                }
                return Ok(());
            }
        }
        let (environment, parent, bounds, weak, app_id, origin) = {
            let state = self.state.borrow();
            (state.environment.clone(), state.parent, state.bounds, Rc::downgrade(&self.state), app_id.to_owned(), url)
        };
        let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |status, controller| {
            let controller = match (status, controller) {
                (Ok(()), Some(controller)) => controller,
                _ => {
                    if let Some(state) = weak.upgrade() {
                        if let Some(callback) = state.borrow().events.failed.as_ref() {
                            callback(&app_id, "The web app could not be opened.".to_owned());
                        }
                    }
                    return Ok(());
                }
            };
            let Some(state_rc) = weak.upgrade() else { let _ = unsafe { controller.Close() }; return Ok(()); };
            let (events, origin_for_handlers, env) = {
                let state = state_rc.borrow();
                (state.events.clone(), origin.clone(), state.environment.clone())
            };
            match unsafe { configure_web_app_controller(&env, controller.clone(), parent, bounds, &app_id, origin_for_handlers, events) } {
                Ok(app) => {
                    let mut state = state_rc.borrow_mut();
                    if state.closed {
                        app.close();
                    } else {
                        let visible = state.current_app.as_deref() == Some(app_id.as_str());
                        // Re-apply the current bounds: the rect captured when creation began may be
                        // stale (e.g. the panel finished animating open while the controller was
                        // still being built), which would otherwise leave the page at 0x0.
                        let _ = unsafe { app.controller.SetBounds(state.bounds) };
                        let _ = unsafe { app.controller.SetIsVisible(visible) };
                        state.apps.insert(app_id, app);
                    }
                }
                Err(error) => {
                    if let Some(callback) = state_rc.borrow().events.failed.as_ref() {
                        callback(&app_id, format!("The web app could not be configured: {error}"));
                    }
                    let _ = unsafe { controller.Close() };
                }
            }
            Ok(())
        }));
        unsafe { environment.CreateCoreWebView2Controller(parent, &handler) }
    }

    pub fn set_bounds(&self, bounds: RECT) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.bounds = bounds;
        for app in state.apps.values() {
            unsafe { app.controller.SetBounds(bounds)?; }
        }
        Ok(())
    }

    /// Moves every controller to `bounds` and makes `app_id` the visible one, hiding the rest.
    /// Idempotent: safe to call on every bounds change, not just when the active app changes.
    pub fn show_app(&self, app_id: &str, bounds: RECT) -> Result<()> {
        let mut state = self.state.borrow_mut();
        if state.closed {
            return Ok(());
        }
        state.bounds = bounds;
        state.current_app = Some(app_id.to_owned());
        for (id, app) in &state.apps {
            unsafe {
                app.controller.SetBounds(bounds)?;
                app.controller.SetIsVisible(id == app_id)?;
            }
        }
        Ok(())
    }

    pub fn hide_all(&self) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.current_app = None;
        for app in state.apps.values() {
            unsafe { app.controller.SetIsVisible(false)?; }
        }
        Ok(())
    }

    /// Retries the current document for an app after a load error.
    pub fn retry_app(&self, app_id: &str) -> Result<()> {
        let state = self.state.borrow();
        let app = state.apps.get(app_id).ok_or_else(|| {
            Error::new(
                windows::core::HRESULT(0x80070002u32 as i32),
                "The web app is not open",
            )
        })?;
        unsafe { app.webview.Reload() }
    }

    pub fn navigate_app(&self, app_id: &str, url: &str) -> Result<()> {
        let url = validate_app_url(url).map_err(|_| Error::new(
            windows::core::HRESULT(0x80070057u32 as i32), "Invalid web app URL"))?;
        let state = self.state.borrow();
        let app = state.apps.get(app_id).ok_or_else(|| Error::new(
            windows::core::HRESULT(0x80070002u32 as i32), "The web app is not open"))?;
        let target = wide(url.as_str());
        unsafe { app.webview.Navigate(PCWSTR::from_raw(target.as_ptr())) }
    }

    pub fn go_back(&self, app_id: &str) -> Result<()> {
        let state = self.state.borrow();
        let app = state.apps.get(app_id).ok_or_else(|| Error::new(
            windows::core::HRESULT(0x80070002u32 as i32), "The web app is not open"))?;
        unsafe { app.webview.GoBack() }
    }

    pub fn stop_app(&self, app_id: &str) -> Result<()> {
        let state = self.state.borrow();
        let app = state.apps.get(app_id).ok_or_else(|| Error::new(
            windows::core::HRESULT(0x80070002u32 as i32), "The web app is not open"))?;
        unsafe { app.webview.Stop() }
    }

    pub fn current_url(&self, app_id: &str) -> Option<String> {
        let state = self.state.borrow();
        let app = state.apps.get(app_id)?;
        let mut source = PWSTR::null();
        unsafe { app.webview.Source(&mut source).ok()?; }
        let value = CoTaskMemPWSTR::from(source).to_string();
        (!value.is_empty()).then_some(value)
    }

    pub fn remove_app(&self, app_id: &str) {
        let mut state = self.state.borrow_mut();
        if state.current_app.as_deref() == Some(app_id) { state.current_app = None; }
        if let Some(app) = state.apps.remove(app_id) { app.close(); }
    }

    pub fn close(&self) {
        let mut state = self.state.borrow_mut();
        if state.closed { return; }
        state.closed = true;
        state.current_app = None;
        for (_, app) in state.apps.drain() { app.close(); }
    }
}

impl Drop for WebAppHost {
    fn drop(&mut self) {
        if Rc::strong_count(&self.state) == 1 {
            self.close();
        }
    }
}

impl WebAppView {
    fn close(mut self) {
        for remove in self.remove_handlers.drain(..) { remove(); }
        let _ = unsafe { self.controller.Close() };
    }
}

unsafe fn configure_web_app_controller(
    _environment: &ICoreWebView2Environment,
    controller: ICoreWebView2Controller,
    _parent: HWND,
    bounds: RECT,
    app_id: &str,
    origin: Url,
    events: WebAppEvents,
) -> Result<WebAppView> {
    let webview = unsafe { controller.CoreWebView2()? };
    unsafe {
        controller.SetBounds(bounds)?;
        controller.SetIsVisible(true)?;
        let settings = webview.Settings()?;
        settings.SetIsWebMessageEnabled(false)?;
        settings.SetAreHostObjectsAllowed(false)?;
        settings.SetAreDefaultContextMenusEnabled(false)?;
        settings.SetIsStatusBarEnabled(false)?;
        settings.SetAreDevToolsEnabled(cfg!(debug_assertions))?;
        let settings4: ICoreWebView2Settings4 = settings.cast()?;
        settings4.SetIsPasswordAutosaveEnabled(false)?;
        let settings6: ICoreWebView2Settings6 = settings.cast()?;
        settings6.SetIsGeneralAutofillEnabled(false)?;
    }
    let mut removers: Vec<Box<dyn Fn()>> = Vec::new();
    let mut token = 0;
    let app_id_owned = app_id.to_owned();
    let open_in_tab = events.on_open_in_tab.clone();
    let navigation_state_started = events.navigation_state.clone();
    let navigation_origin = origin.clone();
    unsafe {
        webview.add_NavigationStarting(&NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
            if let Some(args) = args {
                let mut uri = PWSTR::null();
                let target = if args.Uri(&mut uri).is_ok() { Url::parse(&CoTaskMemPWSTR::from(uri).to_string()).ok() } else { None };
                match target.as_ref().map(|target| navigation_policy(&navigation_origin, target)) {
                    Some(NavigationPolicy::Allow) => if let Some(callback) = navigation_state_started.as_ref() { callback(&app_id_owned, AppNavigationState::Loading); },
                    Some(NavigationPolicy::OpenInTab) => {
                        if let (Some(callback), Some(url)) = (open_in_tab.as_ref(), target) { callback(url); }
                        let _ = args.SetCancel(true);
                    }
                    _ => { let _ = args.SetCancel(true); }
                }
            }
            Ok(())
        })), &mut token)?;
        let event_webview = webview.clone();
        removers.push(Box::new(move || { let _ = event_webview.remove_NavigationStarting(token); }));

        let navigation_state = events.navigation_state.clone();
        let failed = events.failed.clone();
        let completed_id = app_id.to_owned();
        let completed_webview = webview.clone();
        token = 0;
        webview.add_NavigationCompleted(&NavigationCompletedEventHandler::create(Box::new(move |_sender, args| {
            if let Some(args) = args {
                let mut success = BOOL::default();
                let _ = args.IsSuccess(&mut success);
                if success.as_bool() {
                    if let Some(callback) = navigation_state.as_ref() { callback(&completed_id, AppNavigationState::Ready); }
                } else {
                    let mut status = COREWEBVIEW2_WEB_ERROR_STATUS(0);
                    let _ = args.WebErrorStatus(&mut status);
                    let message = format!("The page failed to load (WebView2 error {}).", status.0);
                    if let Some(callback) = failed.as_ref() { callback(&completed_id, message); }
                    if let Some(callback) = navigation_state.as_ref() { callback(&completed_id, AppNavigationState::Failed); }
                }
                let _ = &completed_webview;
            }
            Ok(())
        })), &mut token)?;
        let event_webview = webview.clone();
        removers.push(Box::new(move || { let _ = event_webview.remove_NavigationCompleted(token); }));

        if let Some(callback) = events.title_changed.clone() {
            let id = app_id.to_owned(); token = 0;
            webview.add_DocumentTitleChanged(&DocumentTitleChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(sender) = sender {
                    let mut title = PWSTR::null();
                    if sender.DocumentTitle(&mut title).is_ok() { callback(&id, CoTaskMemPWSTR::from(title).to_string()); }
                }
                Ok(())
            })), &mut token)?;
            let event_webview = webview.clone();
            removers.push(Box::new(move || { let _ = event_webview.remove_DocumentTitleChanged(token); }));
        }

        let webview15: Option<ICoreWebView2_15> = webview.cast().ok();
        if let (Some(webview15_ref), Some(callback)) = (webview15.as_ref(), events.favicon_changed.clone()) {
            let id = app_id.to_owned();
            let favicon_webview = webview.clone(); token = 0;
            webview15_ref.add_FaviconChanged(&FaviconChangedEventHandler::create(Box::new(move |sender, _| {
                let mut uri = PWSTR::null();
                let favicon_url = sender.as_ref().and_then(|sender| sender.cast::<ICoreWebView2_15>().ok()).and_then(|sender| sender.FaviconUri(&mut uri).ok()).and_then(|_| Url::parse(&CoTaskMemPWSTR::from(uri).to_string()).ok());
                if let Some(sender) = sender {
                    if let Ok(webview15) = sender.cast::<ICoreWebView2_15>() {
                        let callback = callback.clone(); let id = id.clone(); let url = favicon_url.clone();
                        let done = GetFaviconCompletedHandler::create(Box::new(move |status, stream| {
                            let bytes = if status.is_ok() { stream.and_then(read_icon_stream) } else { None };
                            callback(&id, url, bytes);
                            Ok(())
                        }));
                        let _ = webview15.GetFavicon(COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, &done);
                    }
                }
                Ok(())
            })), &mut token)?;
            removers.push(Box::new(move || { if let Ok(v15) = favicon_webview.cast::<ICoreWebView2_15>() { let _ = v15.remove_FaviconChanged(token); } }));
        }
        webview.add_PermissionRequested(&PermissionRequestedEventHandler::create(Box::new(move |_sender, args| {
            if let Some(args) = args { let _ = args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY); if let Ok(args3) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() { let _ = args3.SetSavesInProfile(false); } }
            Ok(())
        })), &mut token)?;
        let event_webview = webview.clone(); let permission_token = token;
        removers.push(Box::new(move || { let _ = event_webview.remove_PermissionRequested(permission_token); }));

        let webview4: ICoreWebView2_4 = webview.cast()?;
        webview4.add_DownloadStarting(&DownloadStartingEventHandler::create(Box::new(move |_sender, args| {
            if let Some(args) = args { let _ = args.SetCancel(true); }
            Ok(())
        })), &mut token)?;
        let download_webview = webview.clone(); let download_token = token;
        removers.push(Box::new(move || { if let Ok(v4) = download_webview.cast::<ICoreWebView2_4>() { let _ = v4.remove_DownloadStarting(download_token); } }));

        let popup_callback = events.on_open_in_tab.clone();
        webview.add_NewWindowRequested(&NewWindowRequestedEventHandler::create(Box::new(move |_sender, args| {
            if let Some(args) = args {
                let mut uri = PWSTR::null();
                if args.Uri(&mut uri).is_ok() {
                    if let Ok(url) = Url::parse(&CoTaskMemPWSTR::from(uri).to_string()) {
                        if matches!(url.scheme(), "http" | "https") { if let Some(callback) = popup_callback.as_ref() { callback(url); } }
                    }
                }
                let _ = args.SetHandled(true);
            }
            Ok(())
        })), &mut token)?;
        let popup_webview = webview.clone(); let popup_token = token;
        removers.push(Box::new(move || { let _ = popup_webview.remove_NewWindowRequested(popup_token); }));

        let id = app_id.to_owned(); let failure = events.failed.clone(); let process_navigation_state = events.navigation_state.clone(); let retry = Rc::new(Cell::new(false)); let retry_state = retry.clone(); let failed_webview = webview.clone();
        webview.add_ProcessFailed(&ProcessFailedEventHandler::create(Box::new(move |_sender, _args| {
            let retrying_again = retry_state.replace(true);
            if !retrying_again {
                failed_webview.Reload()?;
            } else {
                if let Some(callback) = failure.as_ref() {
                    callback(&id, "The web app stopped responding. Close and reopen it to try again.".to_owned());
                }
                if let Some(callback) = process_navigation_state.as_ref() {
                    callback(&id, AppNavigationState::Failed);
                }
            }
            Ok(())
        })), &mut token)?;
        let failed_webview = webview.clone(); let failed_token = token;
        removers.push(Box::new(move || { let _ = failed_webview.remove_ProcessFailed(failed_token); }));

        let initial_url = wide(origin.as_str());
        webview.Navigate(PCWSTR::from_raw(initial_url.as_ptr()))?;
        Ok(WebAppView { controller, webview, remove_handlers: removers })
    }
}

fn wide(value: &str) -> Vec<u16> { value.encode_utf16().chain(std::iter::once(0)).collect() }

fn read_icon_stream(stream: IStream) -> Option<Vec<u8>> {
    let mut stat = windows::Win32::System::Com::STATSTG::default();
    unsafe { stream.Stat(&mut stat, windows::Win32::System::Com::STATFLAG_NONAME).ok()?; }
    let size = usize::try_from(stat.cbSize).ok()?;
    if size == 0 || size > 4 * 1024 * 1024 { return None; }
    let mut bytes = vec![0; size]; let mut read = 0u32;
    unsafe { stream.Read(bytes.as_mut_ptr().cast(), size as u32, Some(&mut read)).ok().ok()?; }
    bytes.truncate(read as usize);
    Some(bytes)
}

pub const MAX_APP_URL_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationPolicy {
    Allow,
    OpenInTab,
    Block,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppUrlError {
    TooLong,
    Invalid,
    UnsupportedScheme,
    Credentials,
    MissingHost,
}

pub fn validate_app_url(input: &str) -> std::result::Result<Url, AppUrlError> {
    if input.len() > MAX_APP_URL_BYTES {
        return Err(AppUrlError::TooLong);
    }
    let url = Url::parse(input).map_err(|_| AppUrlError::Invalid)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppUrlError::UnsupportedScheme);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppUrlError::Credentials);
    }
    if url.host_str().is_none() {
        return Err(AppUrlError::MissingHost);
    }
    Ok(url)
}

const AUTH_REDIRECT_HOSTS: &[&str] = &[
    "accounts.google.com",
    "login.microsoftonline.com",
    "appleid.apple.com",
    "auth.openai.com",
];

pub fn navigation_policy(app_origin_url: &Url, target_url: &Url) -> NavigationPolicy {
    if !matches!(target_url.scheme(), "http" | "https") || target_url.host_str().is_none() {
        return NavigationPolicy::Block;
    }
    let Some(app_host) = app_origin_url.host_str() else {
        return NavigationPolicy::Block;
    };
    let Some(target_host) = target_url.host_str() else {
        return NavigationPolicy::Block;
    };
    if same_registrable_domain(app_host, target_host)
        || AUTH_REDIRECT_HOSTS.iter().any(|allowed| {
            target_host.eq_ignore_ascii_case(allowed)
                || target_host
                    .to_ascii_lowercase()
                    .strip_suffix(allowed)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        })
    {
        NavigationPolicy::Allow
    } else {
        NavigationPolicy::OpenInTab
    }
}

fn same_registrable_domain(left: &str, right: &str) -> bool {
    match (psl::domain_str(left), psl::domain_str(right)) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
        _ => left.eq_ignore_ascii_case(right),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_only_http_urls_without_credentials() {
        assert_eq!(
            validate_app_url("https://spotify.com/").unwrap().host_str(),
            Some("spotify.com")
        );
        for input in [
            "file:///secret",
            "javascript:alert(1)",
            "data:text/html,x",
            "https://user:pass@example.com",
            "https://",
        ] {
            assert!(validate_app_url(input).is_err(), "accepted {input}");
        }
    }

    #[test]
    fn validates_length_limit() {
        let too_long = format!("https://example.com/{}", "a".repeat(MAX_APP_URL_BYTES));
        assert!(validate_app_url(&too_long).is_err());
    }

    #[test]
    fn same_registrable_site_and_subdomains_are_allowed() {
        let app = Url::parse("https://open.spotify.com").unwrap();
        for target in [
            "https://spotify.com",
            "https://accounts.spotify.com",
            "https://open.spotify.com/intl",
        ] {
            assert_eq!(
                navigation_policy(&app, &Url::parse(target).unwrap()),
                NavigationPolicy::Allow
            );
        }
    }

    #[test]
    fn cross_site_targets_open_in_browser_tab_and_lookalikes_do_not_match() {
        let app = Url::parse("https://spotify.com").unwrap();
        for target in [
            "https://example.com",
            "https://spotify.com.evil.com",
            "https://evilspotify.com",
        ] {
            assert_eq!(
                navigation_policy(&app, &Url::parse(target).unwrap()),
                NavigationPolicy::OpenInTab
            );
        }
    }

    #[test]
    fn known_auth_redirect_hosts_are_allowed() {
        let app = Url::parse("https://notion.so").unwrap();
        for target in [
            "https://accounts.google.com/signin",
            "https://login.microsoftonline.com/",
            "https://appleid.apple.com/auth",
        ] {
            assert_eq!(
                navigation_policy(&app, &Url::parse(target).unwrap()),
                NavigationPolicy::Allow
            );
        }
    }

    #[test]
    fn non_web_schemes_are_blocked() {
        let app = Url::parse("https://chatgpt.com").unwrap();
        for target in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,hi",
            "bowser://settings",
        ] {
            assert_eq!(
                navigation_policy(&app, &Url::parse(target).unwrap()),
                NavigationPolicy::Block
            );
        }
    }
}
