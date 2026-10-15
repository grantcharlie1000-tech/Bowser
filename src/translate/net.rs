//! The one network call translation makes: an HTTPS POST of a form to Google Translate. WinHTTP,
//! HTTPS only, redirects refused, short timeouts and a capped answer. Meant for a background thread:
//! it blocks until the answer is in. Nothing here knows about pages or languages.

use std::ffi::c_void;

use windows::{
    Win32::Networking::WinHttp::{
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_REDIRECTS, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_DISABLE_FEATURE,
        WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
        WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
    },
    core::{HSTRING, PCWSTR, w},
};

/// Why a request did not produce an answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetError {
    /// Could not connect, send, or read the answer in time.
    Network,
    /// The server answered with this status instead of 200.
    Status(u32),
    /// The answer is larger than the cap.
    TooLarge,
}

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// POSTs `body` (already `application/x-www-form-urlencoded`) to `https://{host}{path}` and returns
/// the answer's bytes, at most `max_answer` of them.
pub fn post_form(host: &str, path: &str, body: &[u8], max_answer: usize) -> Result<Vec<u8>, NetError> {
    let agent = HSTRING::from(concat!("Bowser/", env!("CARGO_PKG_VERSION")));
    let session = Handle(unsafe { WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0) });
    if session.0.is_null() {
        return Err(NetError::Network);
    }
    unsafe { WinHttpSetTimeouts(session.0, 10_000, 10_000, 20_000, 30_000) }.map_err(|_| NetError::Network)?;
    let connection = Handle(unsafe { WinHttpConnect(session.0, &HSTRING::from(host), 443, 0) });
    if connection.0.is_null() {
        return Err(NetError::Network);
    }
    let request = Handle(unsafe {
        WinHttpOpenRequest(connection.0, w!("POST"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE)
    });
    if request.0.is_null() {
        return Err(NetError::Network);
    }
    unsafe { WinHttpSetOption(Some(request.0 as *const c_void), WINHTTP_OPTION_DISABLE_FEATURE, Some(&WINHTTP_DISABLE_REDIRECTS.to_ne_bytes())) }
        .map_err(|_| NetError::Network)?;
    let headers: Vec<u16> = "Content-Type: application/x-www-form-urlencoded; charset=UTF-8\r\n".encode_utf16().collect();
    let length = u32::try_from(body.len()).map_err(|_| NetError::TooLarge)?;
    unsafe { WinHttpSendRequest(request.0, Some(&headers), Some(body.as_ptr() as *const c_void), length, length, 0) }
        .map_err(|_| NetError::Network)?;
    unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) }.map_err(|_| NetError::Network)?;
    let mut status = 0u32;
    let mut size = 4u32;
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut size,
            std::ptr::null_mut(),
        )
    }
    .map_err(|_| NetError::Network)?;
    if status != 200 {
        return Err(NetError::Status(status));
    }
    let mut answer = Vec::new();
    let mut buffer = vec![0u8; 32 * 1024];
    loop {
        let mut read = 0u32;
        unsafe { WinHttpReadData(request.0, buffer.as_mut_ptr() as *mut c_void, buffer.len() as u32, &mut read) }.map_err(|_| NetError::Network)?;
        if read == 0 {
            return Ok(answer);
        }
        if answer.len() + read as usize > max_answer {
            return Err(NetError::TooLarge);
        }
        answer.extend_from_slice(&buffer[..read as usize]);
    }
}
