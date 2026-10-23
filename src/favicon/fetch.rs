//! Anonymous, same-origin favicon GETs. No document, scripts, cookies, credentials, redirects,
//! proxies or third-party favicon service. Shares the blocker's DNS/peer and deadline guards.

use std::{ffi::c_void, sync::atomic::{AtomicBool, Ordering}};
use url::Url;
use windows::{core::{HSTRING, PCWSTR, w}, Win32::Networking::WinHttp::*};
use crate::blocker::fetch::{Deadline, FetchError, Handle, bound_timeouts, peer_watch, resolve_public_by};

pub(super) fn allowed(url: &Url) -> bool {
    // Auto-refresh never probes local services or custom ports, even for a restored intranet tab.
    crate::feeds::fetch::check_url(url).is_ok()
}

pub(super) fn fetch(url: &Url, deadline: Deadline) -> Option<Vec<u8>> {
    get(url, deadline).ok()
}

fn get(url: &Url, deadline: Deadline) -> Result<Vec<u8>, FetchError> {
    if !allowed(url) || url.query().is_some() || url.fragment().is_some() {
        return Err(FetchError::BadUrl("not an automatic favicon address"));
    }
    let host = url.host_str().ok_or(FetchError::Network)?;
    let vetted = resolve_public_by(host, deadline)?;
    let target = crate::blocker::fetch::connect_target(url, &vetted).ok_or(FetchError::Network)?;
    let refused = AtomicBool::new(false);
    let secure = url.scheme() == "https";
    let agent = HSTRING::from(concat!("Bowser/", env!("CARGO_PKG_VERSION")));
    let session = Handle(unsafe { WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_NO_PROXY, PCWSTR::null(), PCWSTR::null(), 0) });
    if session.0.is_null() { return Err(FetchError::Network); }
    // Fail closed if WinHTTP cannot install the peer check.
    if unsafe { WinHttpSetStatusCallback(session.0, Some(peer_watch), 0x0000_000f, 0) }.map(|callback| callback as usize) == Some(usize::MAX) {
        return Err(FetchError::Network);
    }
    const TIMEOUTS: [i32; 4] = [2_000, 2_000, 2_000, 3_000];
    bound_timeouts(session.0, deadline, TIMEOUTS, true)?;
    let connection = Handle(unsafe { WinHttpConnect(session.0, &HSTRING::from(target), if secure { 443 } else { 80 }, 0) });
    if connection.0.is_null() { return Err(FetchError::Network); }
    let flags = if secure { WINHTTP_FLAG_SECURE } else { Default::default() };
    let request = Handle(unsafe { WinHttpOpenRequest(connection.0, w!("GET"), &HSTRING::from(url.path()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), flags) });
    if request.0.is_null() { return Err(FetchError::Network); }
    let disable = WINHTTP_DISABLE_REDIRECTS | WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION;
    unsafe { WinHttpSetOption(Some(request.0 as *const c_void), WINHTTP_OPTION_DISABLE_FEATURE, Some(&disable.to_ne_bytes())) }.map_err(|_| FetchError::Network)?;
    if !secure {
        // HTTP connects to the vetted IP, avoiding a second DNS lookup before sending the request.
        let header: Vec<u16> = format!("Host: {host}\r\n").encode_utf16().collect();
        unsafe { WinHttpAddRequestHeaders(request.0, &header, WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE) }.map_err(|_| FetchError::Network)?;
    }
    let headers: Vec<u16> = "Accept: image/png, image/x-icon, image/vnd.microsoft.icon\r\n".encode_utf16().collect();
    bound_timeouts(request.0, deadline, TIMEOUTS, true)?;
    let sent = unsafe { WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, &refused as *const AtomicBool as usize) };
    if refused.load(Ordering::SeqCst) { return Err(FetchError::PrivateAddress); }
    sent.map_err(|_| FetchError::Network)?;
    bound_timeouts(request.0, deadline, TIMEOUTS, false)?;
    let received = unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) };
    if refused.load(Ordering::SeqCst) { return Err(FetchError::PrivateAddress); }
    received.map_err(|_| FetchError::Network)?;
    deadline.remaining()?;
    let mut status = 0u32;
    let mut size = 4u32;
    unsafe { WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some(&mut status as *mut u32 as *mut c_void), &mut size, std::ptr::null_mut()) }.map_err(|_| FetchError::Network)?;
    if status != 200 { return Err(FetchError::Status(status)); }
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        bound_timeouts(request.0, deadline, TIMEOUTS, false)?;
        let mut read = 0;
        unsafe { WinHttpReadData(request.0, buffer.as_mut_ptr().cast(), buffer.len() as u32, &mut read) }.map_err(|_| FetchError::Network)?;
        if read == 0 { return Ok(bytes); }
        if bytes.len() + read as usize > super::MAX_FAVICON_FILE_BYTES { return Err(FetchError::TooLarge); }
        bytes.extend_from_slice(&buffer[..read as usize]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_requests_refuse_private_credentials_schemes_and_custom_ports() {
        for text in ["http://localhost/", "http://127.0.0.1/", "http://[::1]/", "https://printer.local/", "https://intranet/", "https://a.example:8443/", "https://user:pw@a.example/", "file:///C:/secret", "bowser://settings"] {
            assert!(!allowed(&Url::parse(text).unwrap()), "{text}");
        }
        assert!(allowed(&Url::parse("https://a.example/favicon.ico").unwrap()));
        assert!(allowed(&Url::parse("http://a.example/favicon.png").unwrap()));
    }
}
