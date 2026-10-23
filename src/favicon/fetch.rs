//! Anonymous, same-origin favicon GETs. No document, scripts, cookies, credentials, redirects,
//! proxies or third-party favicon service. Shares the blocker's DNS/peer and deadline guards.

use crate::blocker::fetch::{
    Deadline, FetchError, Handle, bound_timeouts, peer_watch, resolve_public_by,
};
use std::{
    ffi::c_void,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, Ordering},
    },
};
use url::Url;
use windows::{
    Win32::Networking::WinHttp::*,
    core::{HSTRING, PCWSTR, w},
};

pub(super) fn allowed(url: &Url) -> bool {
    // Auto-refresh never probes local services or custom ports, even for a restored intranet tab.
    crate::feeds::fetch::check_url(url).is_ok()
}

pub(super) fn fetch(url: &Url, deadline: Deadline) -> Option<Vec<u8>> {
    get(url, deadline).ok()
}

/// Do not bypass a proxy, PAC or WPAD configuration for implicit startup requests. Cache use is
/// independent of this gate. Unknown routing fails closed, before even resolving a browsing host.
fn routing_is_direct() -> bool {
    use windows::Win32::Foundation::{GlobalFree, HGLOBAL};
    let mut user = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default();
    let user_ok = unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut user) }.is_ok();
    let mut machine = WINHTTP_PROXY_INFO::default();
    let machine_ok = unsafe { WinHttpGetDefaultProxyConfiguration(&mut machine) }.is_ok();
    let direct = direct_configuration(
        user_ok,
        machine_ok,
        user.fAutoDetect.as_bool(),
        !user.lpszAutoConfigUrl.is_null(),
        !user.lpszProxy.is_null(),
        machine.dwAccessType == WINHTTP_ACCESS_TYPE_NO_PROXY,
        !machine.lpszProxy.is_null(),
    );
    for pointer in [
        user.lpszAutoConfigUrl,
        user.lpszProxy,
        user.lpszProxyBypass,
        machine.lpszProxy,
        machine.lpszProxyBypass,
    ] {
        if !pointer.is_null() {
            unsafe {
                let _ = GlobalFree(Some(HGLOBAL(pointer.0.cast())));
            }
        }
    }
    direct
}

fn direct_configuration(
    user_ok: bool,
    machine_ok: bool,
    autodetect: bool,
    pac: bool,
    user_proxy: bool,
    machine_direct: bool,
    machine_proxy: bool,
) -> bool {
    user_ok && machine_ok && !autodetect && !pac && !user_proxy && machine_direct && !machine_proxy
}

struct RequestState {
    handle: AtomicPtr<c_void>,
    refused: AtomicBool,
}

impl RequestState {
    fn close_once(&self) {
        let handle = self.handle.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !handle.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(handle);
            }
        }
    }
}

/// The request owns one Arc; its callback owns another until the final HANDLE_CLOSING notification.
/// Closing from a synchronous informational callback is explicitly supported by WinHTTP:
/// https://learn.microsoft.com/en-us/windows/win32/winhttp/concurrency-in-winhttp
struct CheckedRequest(Arc<RequestState>);

impl CheckedRequest {
    fn new(handle: *mut c_void) -> Result<Self, FetchError> {
        if handle.is_null() {
            return Err(FetchError::Network);
        }
        let request = Self(Arc::new(RequestState {
            handle: AtomicPtr::new(handle),
            refused: AtomicBool::new(false),
        }));
        let context = request.context();
        unsafe {
            WinHttpSetOption(
                Some(handle),
                WINHTTP_OPTION_CONTEXT_VALUE,
                Some(&context.to_ne_bytes()),
            )
        }
        .map_err(|_| FetchError::Network)?;
        let retained = Arc::into_raw(request.0.clone());
        // WINHTTP_CALLBACK_FLAG_HANDLES is this OR; windows-rs exposes only the two statuses.
        let handles =
            WINHTTP_CALLBACK_STATUS_HANDLE_CREATED | WINHTTP_CALLBACK_STATUS_HANDLE_CLOSING;
        let previous = unsafe {
            WinHttpSetStatusCallback(handle, Some(checked_peer), 0x0000_000f | handles, 0)
        };
        if previous.map(|callback| callback as usize) == Some(usize::MAX) {
            // No callback was installed, so there will be no closing notification to release this.
            unsafe {
                drop(Arc::from_raw(retained));
            }
            return Err(FetchError::Network);
        }
        Ok(request)
    }

    fn context(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
    fn refused(&self) -> bool {
        self.0.refused.load(Ordering::SeqCst)
    }
    fn handle(&self) -> Result<*mut c_void, FetchError> {
        let handle = self.0.handle.load(Ordering::SeqCst);
        if handle.is_null() {
            Err(FetchError::PrivateAddress)
        } else {
            Ok(handle)
        }
    }
}

impl Drop for CheckedRequest {
    fn drop(&mut self) {
        self.0.close_once();
    }
}

unsafe extern "system" fn checked_peer(
    handle: *mut c_void,
    context: usize,
    status: u32,
    info: *mut c_void,
    length: u32,
) {
    if context == 0 {
        return;
    }
    if status == WINHTTP_CALLBACK_STATUS_HANDLE_CLOSING {
        // WinHTTP guarantees this is the final notification for the handle. Other informational
        // callbacks may outlive close, so the callback reference must not be freed earlier.
        unsafe {
            drop(Arc::from_raw(context as *const RequestState));
        }
        return;
    }
    let state = unsafe { &*(context as *const RequestState) };
    unsafe {
        peer_watch(
            handle,
            &state.refused as *const AtomicBool as usize,
            status,
            info,
            length,
        );
    }
    if state.refused.load(Ordering::SeqCst) {
        state.close_once();
    }
}

fn get(url: &Url, deadline: Deadline) -> Result<Vec<u8>, FetchError> {
    if !allowed(url) || url.query().is_some() || url.fragment().is_some() {
        return Err(FetchError::BadUrl("not an automatic favicon address"));
    }
    if !routing_is_direct() {
        return Err(FetchError::Network);
    }
    let host = url.host_str().ok_or(FetchError::Network)?;
    let vetted = resolve_public_by(host, deadline)?;
    let target = crate::blocker::fetch::connect_target(url, &vetted).ok_or(FetchError::Network)?;
    let secure = url.scheme() == "https";
    let agent = HSTRING::from(concat!("Bowser/", env!("CARGO_PKG_VERSION")));
    let session = Handle(unsafe {
        WinHttpOpen(
            &agent,
            WINHTTP_ACCESS_TYPE_NO_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        )
    });
    if session.0.is_null() {
        return Err(FetchError::Network);
    }
    const TIMEOUTS: [i32; 4] = [2_000, 2_000, 2_000, 3_000];
    bound_timeouts(session.0, deadline, TIMEOUTS, true)?;
    let connection = Handle(unsafe {
        WinHttpConnect(
            session.0,
            &HSTRING::from(target),
            if secure { 443 } else { 80 },
            0,
        )
    });
    if connection.0.is_null() {
        return Err(FetchError::Network);
    }
    let flags = if secure {
        WINHTTP_FLAG_SECURE
    } else {
        Default::default()
    };
    let request = CheckedRequest::new(unsafe {
        WinHttpOpenRequest(
            connection.0,
            w!("GET"),
            &HSTRING::from(url.path()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        )
    })?;
    let disable =
        WINHTTP_DISABLE_REDIRECTS | WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION;
    unsafe {
        WinHttpSetOption(
            Some(request.handle()?),
            WINHTTP_OPTION_DISABLE_FEATURE,
            Some(&disable.to_ne_bytes()),
        )
    }
    .map_err(|_| FetchError::Network)?;
    if !secure {
        // HTTP connects to the vetted IP, avoiding a second DNS lookup before sending the request.
        let header: Vec<u16> = format!("Host: {host}\r\n").encode_utf16().collect();
        unsafe {
            WinHttpAddRequestHeaders(
                request.handle()?,
                &header,
                WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE,
            )
        }
        .map_err(|_| FetchError::Network)?;
    }
    let headers: Vec<u16> = "Accept: image/png, image/x-icon, image/vnd.microsoft.icon\r\n"
        .encode_utf16()
        .collect();
    bound_timeouts(request.handle()?, deadline, TIMEOUTS, true)?;
    let sent = unsafe {
        WinHttpSendRequest(
            request.handle()?,
            Some(&headers),
            None,
            0,
            0,
            request.context(),
        )
    };
    if request.refused() {
        return Err(FetchError::PrivateAddress);
    }
    sent.map_err(|_| FetchError::Network)?;
    bound_timeouts(request.handle()?, deadline, TIMEOUTS, false)?;
    let received = unsafe { WinHttpReceiveResponse(request.handle()?, std::ptr::null_mut()) };
    if request.refused() {
        return Err(FetchError::PrivateAddress);
    }
    received.map_err(|_| FetchError::Network)?;
    deadline.remaining()?;
    let mut status = 0u32;
    let mut size = 4u32;
    unsafe {
        WinHttpQueryHeaders(
            request.handle()?,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut size,
            std::ptr::null_mut(),
        )
    }
    .map_err(|_| FetchError::Network)?;
    if status != 200 {
        return Err(FetchError::Status(status));
    }
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        bound_timeouts(request.handle()?, deadline, TIMEOUTS, false)?;
        let mut read = 0;
        let result = unsafe {
            WinHttpReadData(
                request.handle()?,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut read,
            )
        };
        if request.refused() {
            return Err(FetchError::PrivateAddress);
        }
        result.map_err(|_| FetchError::Network)?;
        if read == 0 {
            return Ok(bytes);
        }
        if bytes.len() + read as usize > super::MAX_FAVICON_FILE_BYTES {
            return Err(FetchError::TooLarge);
        }
        bytes.extend_from_slice(&buffer[..read as usize]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_requests_refuse_private_credentials_schemes_and_custom_ports() {
        for text in [
            "http://localhost/",
            "http://127.0.0.1/",
            "http://[::1]/",
            "https://printer.local/",
            "https://intranet/",
            "https://a.example:8443/",
            "https://user:pw@a.example/",
            "file:///C:/secret",
            "bowser://settings",
        ] {
            assert!(!allowed(&Url::parse(text).unwrap()), "{text}");
        }
        assert!(allowed(
            &Url::parse("https://a.example/favicon.ico").unwrap()
        ));
        assert!(allowed(
            &Url::parse("http://a.example/favicon.png").unwrap()
        ));
    }

    #[test]
    fn background_routing_requires_explicit_direct_user_and_machine_configuration() {
        assert!(direct_configuration(
            true, true, false, false, false, true, false
        ));
        for state in [
            [false, true, false, false, false, true, false],
            [true, false, false, false, false, true, false],
            [true, true, true, false, false, true, false],
            [true, true, false, true, false, true, false],
            [true, true, false, false, true, true, false],
            [true, true, false, false, false, false, false],
            [true, true, false, false, false, true, true],
        ] {
            let [
                user_ok,
                machine_ok,
                autodetect,
                pac,
                user_proxy,
                machine_direct,
                machine_proxy,
            ] = state;
            assert!(!direct_configuration(
                user_ok,
                machine_ok,
                autodetect,
                pac,
                user_proxy,
                machine_direct,
                machine_proxy
            ));
        }
    }

    /// Handle creation does not connect. These tests simulate peer notifications on a real request
    /// before SendRequest; no DNS lookup, network request, proxy change or browser profile is used.
    fn unused_request() -> (Handle, Handle, CheckedRequest) {
        let session = Handle(unsafe {
            WinHttpOpen(
                w!("Bowser favicon test"),
                WINHTTP_ACCESS_TYPE_NO_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            )
        });
        assert!(!session.0.is_null());
        let connection = Handle(unsafe { WinHttpConnect(session.0, w!("example.com"), 443, 0) });
        assert!(!connection.0.is_null());
        let request = CheckedRequest::new(unsafe {
            WinHttpOpenRequest(
                connection.0,
                w!("GET"),
                w!("/favicon.ico"),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        })
        .unwrap();
        (session, connection, request)
    }

    fn wait_for_callback_release(state: &Arc<RequestState>) {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while Arc::strong_count(state) != 1 && std::time::Instant::now() < until {
            std::thread::yield_now();
        }
        assert_eq!(
            Arc::strong_count(state),
            1,
            "the final HANDLE_CLOSING callback releases its reference"
        );
    }

    #[test]
    fn private_peer_notification_closes_before_send_without_handle_reuse_or_context_loss() {
        let (_session, _connection, request) = unused_request();
        let state = request.0.clone();
        for (address, allowed) in [("93.184.216.34", true), ("127.0.0.1", false)] {
            let mut wide: Vec<u16> = address.encode_utf16().chain(Some(0)).collect();
            let raw = request.handle().unwrap();
            unsafe {
                checked_peer(
                    raw,
                    request.context(),
                    WINHTTP_CALLBACK_STATUS_CONNECTING_TO_SERVER,
                    wide.as_mut_ptr().cast(),
                    (wide.len() * 2) as u32,
                );
            }
            assert_eq!(request.handle().is_ok(), allowed);
        }
        assert!(request.refused());
        assert!(
            request.handle().is_err(),
            "SendRequest cannot obtain the refused handle"
        );
        drop(request);
        state.close_once(); // Closing twice cannot touch a recycled WinHTTP handle.
        wait_for_callback_release(&state);
    }

    #[test]
    fn closing_an_unused_request_releases_the_retained_callback_context() {
        let (_session, _connection, request) = unused_request();
        let state = request.0.clone();
        assert_eq!(Arc::strong_count(&state), 3);
        drop(request);
        wait_for_callback_release(&state);
        assert!(state.handle.load(Ordering::SeqCst).is_null());
        assert!(!state.refused.load(Ordering::SeqCst));
    }
}
