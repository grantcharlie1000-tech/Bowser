//! Fetching a filter list the user asked for, and checking any list before Bowser keeps it.
//!
//! This is the only network use in the blocker and it runs only when the user adds a list by URL
//! or presses update on one, never at startup and never on a timer. Nothing here knows a list
//! address: the URL always comes from the user's own input.
//!
//! The request is HTTPS only, follows at most three redirects (each one checked again), refuses
//! addresses that are not public hosts (IP literals, `localhost`, single-label and local names),
//! stops after [`MAX_LIST_BYTES`] bytes or [`TOTAL_SECONDS`] seconds, and never sends cookies or
//! credentials. The answer must then pass [`validate_list`] before anything is stored. Meant for a
//! background thread: it blocks until the answer is in.

use std::{
    ffi::c_void,
    time::{Duration, Instant},
};

use url::Url;
use windows::{
    Win32::Networking::WinHttp::{
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_COOKIES, WINHTTP_DISABLE_REDIRECTS, WINHTTP_FLAG_SECURE,
        WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_LOCATION, WINHTTP_QUERY_STATUS_CODE,
        WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
        WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
    },
    core::{HSTRING, PCWSTR, w},
};

use super::{MAX_USER_LIST_BYTES, parse_line};

/// Largest list accepted, in bytes (the same cap as a list read from disk).
pub const MAX_LIST_BYTES: usize = MAX_USER_LIST_BYTES;
/// Longest address accepted, in bytes.
pub const MAX_URL_BYTES: usize = 2048;
/// Wall-clock limit for the whole fetch, redirects included.
pub const TOTAL_SECONDS: u64 = 60;
const MAX_REDIRECTS: usize = 3;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FetchError {
    /// The address is not one Bowser will fetch; the string says why.
    BadUrl(&'static str),
    /// Could not connect, or the answer did not arrive in time.
    Network,
    /// The server answered with this status instead of 200.
    Status(u32),
    TooLarge,
    /// A redirect went somewhere Bowser will not follow.
    BadRedirect,
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadUrl(why) => f.write_str(why),
            Self::Network => f.write_str("Could not reach that address. Check it and try again."),
            Self::Status(code) => write!(f, "The server answered {code}, not a list."),
            Self::TooLarge => f.write_str("That list is larger than Bowser keeps (8 MB)."),
            Self::BadRedirect => f.write_str("That address redirects somewhere Bowser will not follow."),
        }
    }
}

/// Parses a list address typed by the user: `https` only, a public host name, no credentials, the
/// default port, at most [`MAX_URL_BYTES`] bytes.
pub fn parse_https_url(text: &str) -> Result<Url, FetchError> {
    let text = text.trim();
    if text.len() > MAX_URL_BYTES || text.chars().any(char::is_control) {
        return Err(FetchError::BadUrl("That is not a web address."));
    }
    let url = Url::parse(text).map_err(|_| FetchError::BadUrl("That is not a web address."))?;
    check_url(&url)?;
    Ok(url)
}

fn check_url(url: &Url) -> Result<(), FetchError> {
    if url.scheme() != "https" {
        return Err(FetchError::BadUrl("Lists are only fetched over https."));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(FetchError::BadUrl("An address with a user name or password is not accepted."));
    }
    if url.port().is_some() {
        return Err(FetchError::BadUrl("An address with its own port is not accepted."));
    }
    if url.fragment().is_some() {
        return Err(FetchError::BadUrl("Remove the part after # from the address."));
    }
    match url.host() {
        Some(url::Host::Domain(host)) if public_host_name(host) => Ok(()),
        _ => Err(FetchError::BadUrl("Only public web sites are fetched, not local or numeric addresses.")),
    }
}

/// A name that can only be a public host: at least two labels, no local-network suffix.
fn public_host_name(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if !host.contains('.') || host.len() > 253 {
        return false;
    }
    const PRIVATE_SUFFIXES: [&str; 9] =
        [".local", ".localhost", ".internal", ".lan", ".home", ".corp", ".intranet", ".home.arpa", ".test"];
    if PRIVATE_SUFFIXES.iter().any(|suffix| host.ends_with(suffix)) || host == "localhost" {
        return false;
    }
    // A name that is all digits and dots (or hex like 0x7f.1) is an address in disguise.
    let numeric_like = host.split('.').all(|label| {
        !label.is_empty() && (label.bytes().all(|b| b.is_ascii_digit()) || label.starts_with("0x"))
    });
    !numeric_like
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

/// Fetches `url` and returns the body's bytes.
pub fn fetch_list(url: &Url) -> Result<Vec<u8>, FetchError> {
    let started = Instant::now();
    let mut current = url.clone();
    for _ in 0..=MAX_REDIRECTS {
        check_url(&current)?;
        match get_once(&current, started)? {
            Reply::Body(bytes) => return Ok(bytes),
            Reply::Redirect(location) => {
                let next = current.join(&location).map_err(|_| FetchError::BadRedirect)?;
                check_url(&next).map_err(|_| FetchError::BadRedirect)?;
                current = next;
            }
        }
    }
    Err(FetchError::BadRedirect)
}

enum Reply {
    Body(Vec<u8>),
    Redirect(String),
}

fn get_once(url: &Url, started: Instant) -> Result<Reply, FetchError> {
    let host = url.host_str().ok_or(FetchError::Network)?;
    let mut path = url.path().to_owned();
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    let agent = HSTRING::from(concat!("Bowser/", env!("CARGO_PKG_VERSION")));
    let session = Handle(unsafe { WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0) });
    if session.0.is_null() {
        return Err(FetchError::Network);
    }
    unsafe { WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 30_000) }.map_err(|_| FetchError::Network)?;
    let connection = Handle(unsafe { WinHttpConnect(session.0, &HSTRING::from(host), 443, 0) });
    if connection.0.is_null() {
        return Err(FetchError::Network);
    }
    let request = Handle(unsafe {
        WinHttpOpenRequest(connection.0, w!("GET"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE)
    });
    if request.0.is_null() {
        return Err(FetchError::Network);
    }
    let disable = WINHTTP_DISABLE_REDIRECTS | WINHTTP_DISABLE_COOKIES;
    unsafe { WinHttpSetOption(Some(request.0 as *const c_void), WINHTTP_OPTION_DISABLE_FEATURE, Some(&disable.to_ne_bytes())) }
        .map_err(|_| FetchError::Network)?;
    let headers: Vec<u16> = "Accept: text/plain, */*;q=0.5\r\n".encode_utf16().collect();
    unsafe { WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0) }.map_err(|_| FetchError::Network)?;
    unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) }.map_err(|_| FetchError::Network)?;
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
    .map_err(|_| FetchError::Network)?;
    if matches!(status, 301 | 302 | 303 | 307 | 308) {
        return Ok(Reply::Redirect(location(request.0)?));
    }
    if status != 200 {
        return Err(FetchError::Status(status));
    }
    let mut body = Vec::new();
    let mut buffer = vec![0u8; 32 * 1024];
    loop {
        if started.elapsed() > Duration::from_secs(TOTAL_SECONDS) {
            return Err(FetchError::Network);
        }
        let mut read = 0u32;
        unsafe { WinHttpReadData(request.0, buffer.as_mut_ptr() as *mut c_void, buffer.len() as u32, &mut read) }
            .map_err(|_| FetchError::Network)?;
        if read == 0 {
            return Ok(Reply::Body(body));
        }
        if body.len() + read as usize > MAX_LIST_BYTES {
            return Err(FetchError::TooLarge);
        }
        body.extend_from_slice(&buffer[..read as usize]);
    }
}

fn location(request: *mut c_void) -> Result<String, FetchError> {
    let mut buffer = vec![0u16; 2048];
    let mut size = (buffer.len() * 2) as u32;
    unsafe {
        WinHttpQueryHeaders(request, WINHTTP_QUERY_LOCATION, PCWSTR::null(), Some(buffer.as_mut_ptr() as *mut c_void), &mut size, std::ptr::null_mut())
    }
    .map_err(|_| FetchError::BadRedirect)?;
    let units = (size as usize / 2).min(buffer.len());
    String::from_utf16(&buffer[..units]).map_err(|_| FetchError::BadRedirect)
}

/// Why a block of text is not kept as a filter list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListError {
    TooLarge,
    NotText,
    /// The text looks like a web page, not a list.
    Html,
    /// No line in it is a rule Bowser can use.
    NoRules,
}

impl std::fmt::Display for ListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooLarge => "That list is larger than Bowser keeps (8 MB).",
            Self::NotText => "That is not a text list.",
            Self::Html => "That address is a web page, not a filter list.",
            Self::NoRules => "That list has no rules Bowser can use.",
        })
    }
}

/// Checks `bytes` before they are stored as a list: valid UTF-8 text without NUL bytes, not an HTML
/// page, and containing at least one rule `cookies` (for a cookie-notice list) or the request and
/// element-hiding matcher understands. Returns the text.
pub fn validate_list(bytes: &[u8], cookie_list: bool) -> Result<String, ListError> {
    if bytes.len() > MAX_LIST_BYTES {
        return Err(ListError::TooLarge);
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if bytes.contains(&0) {
        return Err(ListError::NotText);
    }
    let text = String::from_utf8(bytes.to_vec()).map_err(|_| ListError::NotText)?;
    let head: String = text.chars().take(1024).collect::<String>().to_ascii_lowercase();
    if head.trim_start().starts_with("<!doctype") || head.contains("<html") || head.contains("<head") || head.contains("<body") {
        return Err(ListError::Html);
    }
    let usable = if cookie_list {
        super::cookies::CookieRules::default().add_list(&text) > 0
    } else {
        text.lines().any(|line| parse_line(line).is_some())
    };
    if usable { Ok(text) } else { Err(ListError::NoRules) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_https_addresses_are_accepted() {
        for good in [
            "https://easylist.to/easylist/easylist.txt",
            "https://raw.githubusercontent.com/a/b/main/list.txt?x=1",
            "  https://secure.fanboy.co.nz/fanboy-cookiemonster.txt ",
        ] {
            assert!(parse_https_url(good).is_ok(), "{good}");
        }
        for bad in [
            "http://easylist.to/easylist.txt",
            "ftp://easylist.to/x",
            "file:///C:/list.txt",
            "https://localhost/list.txt",
            "https://127.0.0.1/list.txt",
            "https://192.168.1.1/list.txt",
            "https://[::1]/list.txt",
            "https://2130706433/list.txt",
            "https://0x7f.1/list.txt",
            "https://intranet/list.txt",
            "https://printer.local/list.txt",
            "https://nas.home.arpa/list.txt",
            "https://user:pass@easylist.to/x.txt",
            "https://easylist.to:8443/x.txt",
            "https://easylist.to/x.txt#frag",
            "https://",
            "not a url",
            "",
            "javascript:alert(1)",
        ] {
            assert!(parse_https_url(bad).is_err(), "{bad}");
        }
        assert!(parse_https_url(&format!("https://example.com/{}", "a".repeat(MAX_URL_BYTES))).is_err());
        assert!(parse_https_url("https://example.com/a\nb").is_err());
    }

    #[test]
    fn a_list_is_text_with_at_least_one_usable_rule() {
        let list = "[Adblock Plus 2.0]\n! Title: x\n||ads.example^\n##.banner\n";
        assert_eq!(validate_list(list.as_bytes(), false).as_deref(), Ok(list));
        let with_bom = [b"\xEF\xBB\xBF".as_slice(), list.as_bytes()].concat();
        assert_eq!(validate_list(&with_bom, false).as_deref(), Ok(list));
        assert_eq!(validate_list(b"! only comments\n", false), Err(ListError::NoRules));
        assert_eq!(validate_list(b"", false), Err(ListError::NoRules));
        assert_eq!(validate_list(b"||ads.example^\0\n", false), Err(ListError::NotText));
        assert_eq!(validate_list(&[0xFF, 0xFE, b'a'], false), Err(ListError::NotText));
        assert_eq!(validate_list(b"<!DOCTYPE html><html><body>nope</body></html>", false), Err(ListError::Html));
        assert_eq!(validate_list(b"||ads.example^\n<html>", false), Err(ListError::Html));
        assert_eq!(validate_list(&vec![b'a'; MAX_LIST_BYTES + 1], false), Err(ListError::TooLarge));
    }

    #[test]
    fn a_cookie_list_needs_an_element_hiding_rule_it_can_keep() {
        assert!(validate_list(b"example.com##.cookie-banner\n", true).is_ok());
        assert_eq!(validate_list(b"||cookiebot.com^\n", true), Err(ListError::NoRules));
        assert_eq!(validate_list(b"##body\n", true), Err(ListError::NoRules), "a rule that could blank the page is not a rule");
    }
}
