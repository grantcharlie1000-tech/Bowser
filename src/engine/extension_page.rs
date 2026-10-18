//! A Bowser-owned window that hosts one extension page (an action popup or an options page).
//!
//! WebView2 draws no extension UI, so Bowser opens the page itself: a top-level window with one
//! WebView2 controller in the shared environment, on the default (non-private) profile, so the page
//! shares the profile's extension storage and runtime with the extension's worker.
//!
//! What the page gets: no web-message channel and no host objects (Bowser has no message handler
//! here, and both settings are verified off), denied permissions and downloads, no unowned new windows, no
//! external-protocol launches, and same-extension navigation after the initial granted URL (see
//! `HostedPagePolicy`). What it does not get from Bowser: the blocker, the privacy counters, tab
//! history or any tab id. Whether the page has `chrome.*` is the runtime's decision.

use std::{cell::Cell, cell::RefCell, rc::Rc};

use url::Url;
use webview2_com::{
    CoTaskMemPWSTR, ContextMenuRequestedEventHandler, CustomItemSelectedEventHandler,
    CreateCoreWebView2ControllerCompletedHandler, DownloadStartingEventHandler,
    Microsoft::Web::WebView2::Win32::*, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, PermissionRequestedEventHandler,
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, Com::IStream},
        UI::WindowsAndMessaging::{
            CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
            GetClientRect, GetWindowLongPtrW, IDC_ARROW, LoadCursorW, RegisterClassW, SW_SHOW,
            SetWindowLongPtrW, ShowWindow, WINDOW_EX_STYLE, WM_NCDESTROY, WM_SIZE, WNDCLASSW,
            WS_OVERLAPPEDWINDOW,
        },
    },
    core::{BOOL, Error, HSTRING, Interface, PWSTR, Result, w},
};

use super::webview2::{
    ControllerSecuritySetting, ExternalSchemeHandler, HostedPagePolicy,
    apply_controller_security_policy,
};

const WINDOW_WIDTH: i32 = 440;
const WINDOW_HEIGHT: i32 = 640;
const MAX_TITLE_CHARS: usize = 128;

thread_local! {
    static CLASS_REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// What the window owns. It lives in `GWLP_USERDATA` from the moment the controller exists until
/// the window is destroyed.
struct WindowState {
    controller: ICoreWebView2Controller,
    alive: Rc<Cell<bool>>,
}

/// The engine's handle on an open hosted page.
pub(super) struct HostedPage {
    hwnd: HWND,
    alive: Rc<Cell<bool>>,
    #[cfg(test)]
    pub(super) webview: ICoreWebView2,
}

impl HostedPage {
    /// Closes the window if it is still open. The flag, not `IsWindow`, says so: a destroyed
    /// window's handle can be reused by an unrelated window.
    pub(super) fn close(&self) {
        if self.alive.get() {
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

unsafe extern "system" fn page_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_SIZE => {
            let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const WindowState;
            if let Some(state) = unsafe { state.as_ref() } {
                let mut bounds = RECT::default();
                if unsafe { GetClientRect(hwnd, &mut bounds) }.is_ok() {
                    let _ = unsafe { state.controller.SetBounds(bounds) };
                }
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let raw = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) } as *mut WindowState;
            if !raw.is_null() {
                let state = unsafe { Box::from_raw(raw) };
                state.alive.set(false);
                let _ = unsafe { state.controller.Close() };
            }
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn clean_title(title: &str) -> String {
    title
        .chars()
        .take(MAX_TITLE_CHARS)
        .map(|c| {
            if c.is_control() || crate::extensions::is_invisible_format(c) {
                '?'
            } else {
                c
            }
        })
        .collect()
}

fn create_window(title: &str) -> Result<HWND> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        if !CLASS_REGISTERED.with(Cell::get) {
            let class = WNDCLASSW {
                lpfnWndProc: Some(page_proc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: w!("BowserExtensionPage"),
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return Err(Error::from_thread());
            }
            CLASS_REGISTERED.with(|registered| registered.set(true));
        }
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("BowserExtensionPage"),
            &HSTRING::from(clean_title(title)),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
            None,
            None,
            Some(instance),
            None,
        )
    }
}

/// Opens `url` in a new window. `show` is false only for the live probe, which must not put a
/// window on the desktop. `done` runs once, on the UI thread, when the page has started loading or
/// failed.
pub(super) fn open(
    environment: &ICoreWebView2Environment,
    url: Url,
    title: &str,
    show: bool,
    done: Box<dyn FnOnce(std::result::Result<HostedPage, String>)>,
) {
    let Some(policy) = HostedPagePolicy::new(&url) else {
        done(Err(
            "Bowser will not open that address as an extension page.".to_owned(),
        ));
        return;
    };
    let hwnd = match create_window(title) {
        Ok(hwnd) => hwnd,
        Err(error) => {
            done(Err(format!(
                "The extension window could not be created. {error}"
            )));
            return;
        }
    };
    let slot = Rc::new(RefCell::new(Some(done)));
    let completion = slot.clone();
    let policy = Rc::new(policy);
    let environment_for_menu = environment.clone();
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |status, controller| {
            let result = match (status, controller) {
                (Ok(()), Some(controller)) => {
                    let configured = configure(hwnd, &controller, &url, policy, show, &environment_for_menu);
                    if configured.is_err() {
                        let _ = unsafe { controller.Close() };
                    }
                    configured.map_err(|error| format!("{error}"))
                }
                (Err(error), _) => Err(format!("{error}")),
                _ => Err("WebView2 did not return a controller.".to_owned()),
            };
            if result.is_err() {
                let _ = unsafe { DestroyWindow(hwnd) };
            }
            if let Some(done) = completion.borrow_mut().take() {
                done(result);
            }
            Ok(())
        },
    ));
    if let Err(error) = unsafe { environment.CreateCoreWebView2Controller(hwnd, &handler) } {
        let _ = unsafe { DestroyWindow(hwnd) };
        if let Some(done) = slot.borrow_mut().take() {
            done(Err(format!("{error}")));
        }
    }
}

fn configure(
    hwnd: HWND,
    controller: &ICoreWebView2Controller,
    url: &Url,
    policy: Rc<HostedPagePolicy>,
    show: bool,
    environment: &ICoreWebView2Environment,
) -> Result<HostedPage> {
    unsafe {
        let mut bounds = RECT::default();
        GetClientRect(hwnd, &mut bounds)?;
        controller.SetBounds(bounds)?;
        controller.SetIsVisible(true)?;
        let webview = controller.CoreWebView2()?;
        let settings = webview.Settings()?;
        apply_controller_security_policy(|setting| match setting {
            ControllerSecuritySetting::WebMessages => settings.SetIsWebMessageEnabled(false),
            ControllerSecuritySetting::HostObjects => settings.SetAreHostObjectsAllowed(false),
        })?;
        // Fail closed: the page must have neither channel to Bowser.
        let mut enabled = BOOL::default();
        settings.IsWebMessageEnabled(&mut enabled)?;
        let mut host_objects = BOOL::default();
        settings.AreHostObjectsAllowed(&mut host_objects)?;
        if enabled.as_bool() || host_objects.as_bool() {
            return Err(Error::new(
                windows::Win32::Foundation::E_ACCESSDENIED,
                "WebView2 did not turn off the message channel for an extension page",
            ));
        }

        // Never register `WebMessageReceived`, `FrameWebMessageReceived` or a host object on this
        // WebView. Observed on runtime 155.0.4283.45 (see docs/extensions.md): `window.chrome.webview`
        // still exists in the page, but with these settings off a `postMessage` from it reached no
        // host handler, and no host object was callable. That is only because nothing is
        // registered here: the object is inert, not a sandbox. A handler added here would be
        // reachable by the extension's page script.
        #[cfg(test)]
        observe::attach(&webview)?;

        let mut token = 0;
        let navigation_policy = policy.clone();
        webview.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    let parsed = if args.Uri(&mut uri).is_ok() {
                        Url::parse(&CoTaskMemPWSTR::from(uri).to_string()).ok()
                    } else {
                        None
                    };
                    if !parsed.is_some_and(|uri| navigation_policy.allow_navigation(&uri)) {
                        let _ = args.SetCancel(true);
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        let frame_policy = policy.clone();
        webview.add_FrameNavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    let parsed = if args.Uri(&mut uri).is_ok() {
                        Url::parse(&CoTaskMemPWSTR::from(uri).to_string()).ok()
                    } else {
                        None
                    };
                    if !parsed.is_some_and(|uri| frame_policy.allow_frame(&uri)) {
                        let _ = args.SetCancel(true);
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        // A native, user-picked way to follow a real signed-page link even when its ordinary
        // click handler depends on chrome.tabs.create, which WebView2 cannot map to Bowser tabs.
        // This grants no website API and never reads a guessed dashboard path.
        let link_policy = policy.clone();
        if let (Ok(webview11), Ok(environment9)) = (webview.cast::<ICoreWebView2_11>(), environment.cast::<ICoreWebView2Environment9>()) {
            webview11.add_ContextMenuRequested(&ContextMenuRequestedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else { return Ok(()) };
                let target = args.ContextMenuTarget()?; let mut has_link = BOOL::default(); target.HasLinkUri(&mut has_link)?;
                if !has_link.as_bool() { return Ok(()); }
                let mut raw = PWSTR::null(); target.LinkUri(&mut raw)?;
                let Some(link) = Url::parse(&CoTaskMemPWSTR::from(raw).to_string()).ok().filter(|url| link_policy.allow_new_window(url)) else { return Ok(()) };
                let item = environment9.CreateContextMenuItem(w!("Open link in this extension window"), None::<&IStream>, COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND)?;
                let navigation_policy = link_policy.clone(); let mut selected = 0;
                item.add_CustomItemSelected(&CustomItemSelectedEventHandler::create(Box::new(move |_sender, _args| {
                    let mut raw = PWSTR::null(); sender.Source(&mut raw)?;
                    let source = Url::parse(&CoTaskMemPWSTR::from(raw).to_string()).ok();
                    if source.as_ref().is_some_and(|source| navigation_policy.allow_new_window(source)) && navigation_policy.allow_new_window(&link) {
                        let address = CoTaskMemPWSTR::from(link.as_str()); sender.Navigate(*address.as_ref().as_pcwstr())?;
                    }
                    Ok(())
                })), &mut selected)?;
                args.MenuItems()?.InsertValueAtIndex(0, &item)?; Ok(())
            })), &mut token)?;
        }
        webview.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(move |sender, args| {
                if let Some(args) = args {
                    // Always suppress an unowned native window. A signed extension's own link
                    // may instead navigate this same no-bridge host, using the normal policy.
                    args.SetHandled(true)?;
                    let mut uri = PWSTR::null();
                    let uri = args.Uri(&mut uri).ok().and_then(|()| Url::parse(&CoTaskMemPWSTR::from(uri).to_string()).ok());
                    let allowed = uri.as_ref().is_some_and(|uri| policy.allow_new_window(uri));
                    #[cfg(test)] println!("EXTENSION_PAGE_NEW_WINDOW uri={uri:?} same_extension_reuse_allowed={allowed}");
                    if let (true, Some(uri), Some(sender)) = (allowed, uri, sender) {
                        let address = CoTaskMemPWSTR::from(uri.as_str()); sender.Navigate(*address.as_ref().as_pcwstr())?;
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        webview.add_PermissionRequested(
            &PermissionRequestedEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let _ = args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY);
                    if let Ok(args3) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() {
                        let _ = args3.SetSavesInProfile(false);
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        let webview4: ICoreWebView2_4 = webview.cast()?;
        webview4.add_DownloadStarting(
            &DownloadStartingEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let _ = args.SetCancel(true);
                }
                Ok(())
            })),
            &mut token,
        )?;
        let webview18: ICoreWebView2_18 = webview.cast()?;
        let external: ICoreWebView2LaunchingExternalUriSchemeEventHandler =
            ExternalSchemeHandler(RefCell::new(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let _ = args.SetCancel(true);
                }
                Ok(())
            })))
            .into();
        webview18.add_LaunchingExternalUriScheme(&external, &mut token)?;

        let alive = Rc::new(Cell::new(true));
        let state = Box::new(WindowState {
            controller: controller.clone(),
            alive: alive.clone(),
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
        let page = HostedPage {
            hwnd,
            alive,
            #[cfg(test)]
            webview: webview.clone(),
        };
        if show {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        let address = CoTaskMemPWSTR::from(url.as_str());
        if let Err(error) = webview.Navigate(*address.as_ref().as_pcwstr()) {
            page.close();
            return Err(error);
        }
        Ok(page)
    }
}

/// Test only. The live probe's observer: it records every web message the hosted WebView delivers
/// to the host, so the probe can say whether `window.chrome.webview.postMessage` goes anywhere. It
/// is compiled out of the product and only records; it never answers or acts on a message.
#[cfg(test)]
pub(super) mod observe {
    use std::{cell::Cell, cell::RefCell};

    use webview2_com::{CoTaskMemPWSTR, WebMessageReceivedEventHandler};
    use windows::{Win32::Foundation::E_FAIL, core::{PWSTR, Result}};

    use super::ICoreWebView2;

    thread_local! {
        static RECEIVED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static POSITIVE_CONTROL: Cell<bool> = const { Cell::new(false) };
    }

    /// Every message the host side saw since the last `take_received`.
    pub(in crate::engine) fn take_received() -> Vec<String> {
        RECEIVED.with(|received| std::mem::take(&mut *received.borrow_mut()))
    }

    /// Turns web messages back on for the next hosted page, to prove the observer can see one.
    pub(in crate::engine) fn positive_control(on: bool) {
        POSITIVE_CONTROL.with(|control| control.set(on));
    }

    pub(super) fn attach(webview: &ICoreWebView2) -> Result<()> {
        let mut token = 0;
        unsafe {
            webview.add_WebMessageReceived(
                &WebMessageReceivedEventHandler::create(Box::new(|_sender, args| {
                    if let Some(args) = args {
                        let mut text = PWSTR::null();
                        let message = if args.TryGetWebMessageAsString(&mut text).is_ok() {
                            CoTaskMemPWSTR::from(text).to_string()
                        } else {
                            let mut json = PWSTR::null();
                            let _ = args.WebMessageAsJson(&mut json);
                            format!("json:{}", CoTaskMemPWSTR::from(json))
                        };
                        RECEIVED.with(|received| received.borrow_mut().push(message));
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            if POSITIVE_CONTROL.with(Cell::get) {
                webview
                    .Settings()?
                    .SetIsWebMessageEnabled(true)
                    .map_err(|_| windows::core::Error::from(E_FAIL))?;
            }
        }
        Ok(())
    }
}

/// Test only: runs a script in the hosted page and hands back the JSON text of its result. The
/// live probe uses it to read what the page reports and to click its button; production code has no
/// script channel into these pages.
#[cfg(test)]
pub(super) fn execute_script(
    webview: &ICoreWebView2,
    script: &str,
    done: Box<dyn FnOnce(Option<String>)>,
) {
    use webview2_com::ExecuteScriptCompletedHandler;
    let slot = Rc::new(RefCell::new(Some(done)));
    let completion = slot.clone();
    let handler = ExecuteScriptCompletedHandler::create(Box::new(move |status, value| {
        let answer = status.is_ok().then(|| value.to_string());
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

#[cfg(test)]
mod tests {
    /// The production part of this file: everything before the test-only observer.
    fn production() -> &'static str {
        let source = include_str!("extension_page.rs");
        &source[..source
            .find(concat!("/// Test only. The live probe's ", "observer"))
            .expect("test-only observer marker")]
    }

    #[test]
    fn hosted_page_production_code_registers_no_channel_to_the_host() {
        // `window.chrome.webview` exists in the page even with both settings off (observed in the
        // live probe), so the only thing keeping it inert is that nothing on the host side is
        // registered. Keep it that way: no web-message handler, no frame handler, no host object.
        let production = production();
        for forbidden in [
            concat!("add_WebMessage", "Received"),
            concat!("add_FrameWebMessage", "Received"),
            concat!("AddHostObject", "ToScript"),
            concat!("add_Frame", "Created"),
            concat!("PostWebMessage", "As"),
            concat!("SetIsWebMessageEnabled", "(true"),
            concat!("SetAreHostObjectsAllowed", "(true"),
        ] {
            assert!(
                !production.contains(forbidden),
                "src/engine/extension_page.rs production code uses {forbidden}"
            );
        }
        // The observer that proved this is compiled out of the product. The
        // checkout may use CRLF; the gate is the source order, not the newline byte.
        let source = include_str!("extension_page.rs").replace("\r\n", "\n");
        assert!(source.contains(concat!("#[cfg(test)]\npub(super) mod ", "observe")));
        assert!(source.contains(concat!("#[cfg(test)]\n        observe", "::attach")));
    }

    #[test]
    fn hosted_page_keeps_both_channels_off_and_checks_them() {
        let production = production();
        for needle in [
            "SetIsWebMessageEnabled(false)",
            "SetAreHostObjectsAllowed(false)",
            "IsWebMessageEnabled(&mut enabled)",
            "AreHostObjectsAllowed(&mut host_objects)",
        ] {
            assert!(production.contains(needle), "missing in extension_page.rs: {needle}");
        }
    }
}
