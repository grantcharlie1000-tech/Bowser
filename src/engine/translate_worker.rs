//! The hidden page that runs the translation engine (see `crate::translate`).
//!
//! It exists only while the user is translating: the engine creates it on a translate command and
//! closes it after [`crate::translate::WORKER_IDLE_SECS`] of no use. It is one more WebView2
//! controller in the tab environment, but in its own in-memory profile, never visible, with no tab
//! blocker and no extension. It loads exactly one page, `worker.html`, from a folder mapped to the
//! virtual host `bowser-translate.invalid`; the page's content policy lets it read that folder and
//! nothing else, and every other request, navigation, frame, window, permission, download and
//! external launch is refused here as well. Its only channel is the web-message bridge: JSON text in,
//! JSON text out. Messages are accepted only from that host's page, size-capped, and handed to the
//! app as raw text for `crate::translate::parse_worker_message`; the tabs have no such channel.

use std::{cell::RefCell, path::Path, rc::Rc};

use url::Url;
use webview2_com::{
    CoTaskMemPWSTR, CreateCoreWebView2ControllerCompletedHandler, DownloadStartingEventHandler,
    Microsoft::Web::WebView2::Win32::*, NavigationStartingEventHandler, NewWindowRequestedEventHandler,
    PermissionRequestedEventHandler, ProcessFailedEventHandler, WebMessageReceivedEventHandler,
    WebResourceRequestedEventHandler,
};
use windows::{
    Win32::{
        Foundation::{E_POINTER, HWND, RECT},
        System::Com::IStream,
    },
    core::{Error, Interface, PCWSTR, PWSTR, Result},
};

use super::webview2::ExternalSchemeHandler;
use crate::translate::{MAX_ANSWER_BYTES, WORKER_HOST, is_worker_url};

/// The worker's own in-memory WebView2 profile: not the browsing profile, not the private-tab
/// profile, not the chrome's.
const PROFILE: &str = "BowserTranslate";
const START_URL: &str = "https://bowser-translate.invalid/worker.html";

/// A running worker page. Dropping it does not close it; call [`Worker::close`].
pub(super) struct Worker {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

impl Worker {
    /// Sends one JSON message to the page. `false` when it could not be delivered.
    pub(super) fn post(&self, json: &str) -> bool {
        let message: Vec<u16> = json.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { self.webview.PostWebMessageAsJson(PCWSTR(message.as_ptr())) }.is_ok()
    }

    /// Closes the page and its renderer.
    pub(super) fn close(&self) {
        let _ = unsafe { self.controller.Close() };
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn uri_of(read: impl FnOnce(&mut PWSTR) -> Result<()>) -> Option<Url> {
    let mut raw = PWSTR::null();
    read(&mut raw).ok()?;
    Url::parse(&CoTaskMemPWSTR::from(raw).to_string()).ok()
}

/// Starts creating the worker page for `root` (the cache folder, which must already hold
/// `worker.html`). `done` runs once, on the UI thread, with the worker or the reason it failed.
/// `on_message` gets the raw text of each message the worker page posts; `on_failed` runs if the
/// page's process fails afterwards.
pub(super) fn open(
    environment: &ICoreWebView2Environment,
    parent: HWND,
    root: &Path,
    on_message: Rc<dyn Fn(String)>,
    on_failed: Rc<dyn Fn()>,
    done: Box<dyn FnOnce(std::result::Result<Worker, String>)>,
) {
    let slot = Rc::new(RefCell::new(Some(done)));
    let finish = {
        let slot = slot.clone();
        move |result: std::result::Result<Worker, String>| {
            if let Some(done) = slot.borrow_mut().take() {
                done(result);
            }
        }
    };
    let options = match unsafe {
        environment
            .cast::<ICoreWebView2Environment10>()
            .and_then(|environment10| environment10.CreateCoreWebView2ControllerOptions())
    } {
        Ok(options) => options,
        Err(error) => return finish(Err(error.to_string())),
    };
    let profile = wide(PROFILE);
    // An in-memory profile of its own: nothing the page does is kept, and it shares nothing with a
    // tab. Both are required; a runtime that refuses either fails the worker, not the browser.
    if let Err(error) = unsafe {
        options
            .SetProfileName(PCWSTR(profile.as_ptr()))
            .and_then(|()| options.SetIsInPrivateModeEnabled(true))
    } {
        return finish(Err(error.to_string()));
    }
    let env = environment.clone();
    let folder = root.to_path_buf();
    let finish_in_handler = finish.clone();
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |status, controller| {
        let result = match (status, controller) {
            (Ok(()), Some(controller)) => configure(&env, &controller, &folder, on_message.clone(), on_failed.clone())
                .inspect_err(|_| {
                    let _ = unsafe { controller.Close() };
                })
                .map_err(|error| error.to_string()),
            (Err(error), _) => Err(error.to_string()),
            _ => Err(Error::from(E_POINTER).to_string()),
        };
        finish_in_handler(result);
        Ok(())
    }));
    let environment10: ICoreWebView2Environment10 = match environment.cast() {
        Ok(environment10) => environment10,
        Err(error) => return finish(Err(error.to_string())),
    };
    if let Err(error) = unsafe { environment10.CreateCoreWebView2ControllerWithOptions(parent, &options, &handler) } {
        finish(Err(error.to_string()));
    }
}

fn configure(
    environment: &ICoreWebView2Environment,
    controller: &ICoreWebView2Controller,
    folder: &Path,
    on_message: Rc<dyn Fn(String)>,
    on_failed: Rc<dyn Fn()>,
) -> Result<Worker> {
    unsafe {
        // Never shown: no size, not visible. It still runs; a hidden page is only throttled.
        controller.SetBounds(RECT { left: 0, top: 0, right: 1, bottom: 1 })?;
        controller.SetIsVisible(false)?;
        let webview = controller.CoreWebView2()?;
        let settings = webview.Settings()?;
        settings.SetIsWebMessageEnabled(true)?;
        settings.SetAreHostObjectsAllowed(false)?;
        settings.SetAreDefaultContextMenusEnabled(false)?;
        settings.SetIsStatusBarEnabled(false)?;
        settings.SetIsZoomControlEnabled(false)?;
        settings.SetAreDefaultScriptDialogsEnabled(false)?;
        settings.SetAreDevToolsEnabled(cfg!(debug_assertions))?;
        if let Ok(settings3) = settings.cast::<ICoreWebView2Settings3>() {
            settings3.SetAreBrowserAcceleratorKeysEnabled(false)?;
        }
        if let Ok(settings4) = settings.cast::<ICoreWebView2Settings4>() {
            settings4.SetIsPasswordAutosaveEnabled(false)?;
        }
        if let Ok(settings6) = settings.cast::<ICoreWebView2Settings6>() {
            settings6.SetIsGeneralAutofillEnabled(false)?;
        }

        // Only the worker's own origin may load or navigate; everything else is cancelled before
        // it starts. The first navigation below is the one allowed address.
        let mut token = 0;
        webview.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let allowed = uri_of(|raw| args.Uri(raw)).is_some_and(|uri| is_worker_url(&uri) && uri.path() == "/worker.html");
                    if !allowed {
                        let _ = args.SetCancel(true);
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        // The page has no frames and its policy forbids them; a frame navigation is always refused.
        webview.add_FrameNavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let _ = args.SetCancel(true);
                }
                Ok(())
            })),
            &mut token,
        )?;
        webview.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(move |_sender, args| {
                if let Some(args) = args {
                    let _ = args.SetHandled(true);
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
        let external: ICoreWebView2LaunchingExternalUriSchemeEventHandler = ExternalSchemeHandler(RefCell::new(Box::new(move |_sender, args| {
            if let Some(args) = args {
                let _ = args.SetCancel(true);
            }
            Ok(())
        })))
        .into();
        webview18.add_LaunchingExternalUriScheme(&external, &mut token)?;
        webview.add_ProcessFailed(
            &ProcessFailedEventHandler::create(Box::new(move |_sender, _args| {
                on_failed();
                Ok(())
            })),
            &mut token,
        )?;

        // A second wall behind the page's own policy: any request that is not for the worker's
        // virtual host is answered 403 here and never reaches the network.
        let env = environment.clone();
        webview.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_sender, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                let uri = args.Request().ok().and_then(|request| uri_of(|raw| request.Uri(raw)));
                if uri.is_some_and(|uri| uri.host_str() == Some(WORKER_HOST)) {
                    return Ok(());
                }
                let reason = wide("Forbidden");
                let headers = wide("");
                let response = env.CreateWebResourceResponse(None::<&IStream>, 403, PCWSTR(reason.as_ptr()), PCWSTR(headers.as_ptr()))?;
                args.SetResponse(&response)
            })),
            &mut token,
        )?;
        let filter = wide("*");
        webview.AddWebResourceRequestedFilter(PCWSTR(filter.as_ptr()), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?;

        // The bridge. The sender's page and the message's own source must both be the worker's
        // host, the payload must be a string, and anything oversized is dropped unread.
        webview.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else {
                    return Ok(());
                };
                let from_worker = uri_of(|raw| sender.Source(raw)).is_some_and(|uri| is_worker_url(&uri))
                    && uri_of(|raw| args.Source(raw)).is_some_and(|uri| is_worker_url(&uri));
                if !from_worker {
                    return Ok(());
                }
                let mut raw = PWSTR::null();
                if args.TryGetWebMessageAsString(&mut raw).is_err() {
                    return Ok(());
                }
                let text = CoTaskMemPWSTR::from(raw).to_string();
                if text.len() <= MAX_ANSWER_BYTES {
                    on_message(text);
                }
                Ok(())
            })),
            &mut token,
        )?;

        // Serve the cache folder at the virtual host. DENY_CORS: only the page's own origin reads it.
        let webview3: ICoreWebView2_3 = webview.cast()?;
        let host = wide(WORKER_HOST);
        let folder = wide(&folder.to_string_lossy());
        webview3.SetVirtualHostNameToFolderMapping(PCWSTR(host.as_ptr()), PCWSTR(folder.as_ptr()), COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS)?;
        let start = wide(START_URL);
        webview.Navigate(PCWSTR(start.as_ptr()))?;
        Ok(Worker { controller: controller.clone(), webview })
    }
}
