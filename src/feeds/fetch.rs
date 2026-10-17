//! Fetching a feed the user subscribed to (or typed an address for). This is the whole of the feed
//! reader's network use: it runs only when the user subscribes, previews or refreshes, never at startup
//! and never on a timer, and it asks nobody but the feed's own server.
//!
//! The request is a plain GET over WinHTTP (the same API translation uses) for an `http` or `https`
//! address with no credentials and no port of its own. It follows at most three redirects, each one
//! checked again and never from `https` down to `http`; it refuses hosts that are not public names
//! (`localhost`, single-label and local names, IP literals); it resolves the name itself and refuses
//! any non-public address; and WinHTTP then reports every address it really connects to, so a name
//! that changes its answer between the two lookups (DNS rebinding) ends the hop and its answer is
//! thrown away. Those checks are the blocker's own (`crate::blocker::fetch`), shared, not copied.
//! The connection is direct (never through a proxy), sends no cookies and no credentials, stops after
//! [`MAX_BYTES`] bytes or [`TOTAL_SECONDS`] seconds. Meant for a background thread: it blocks.

use std::{
    ffi::c_void,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use url::Url;
use windows::{
    Win32::Networking::WinHttp::{
        WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_ADDREQ_FLAG_ADD, WINHTTP_ADDREQ_FLAG_REPLACE, WINHTTP_DISABLE_COOKIES, WINHTTP_DISABLE_REDIRECTS,
        WINHTTP_FLAG_SECURE, WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpAddRequestHeaders,
        WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
        WinHttpSetOption, WinHttpSetStatusCallback,
    },
    core::{HSTRING, PCWSTR, w},
};

use crate::blocker::fetch::{Deadline, FetchError, Handle, bound_timeouts, connect_target, location, peer_watch, public_host_name, resolve_public_by};

/// Largest answer accepted, in bytes.
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
/// Longest address accepted, in bytes.
pub const MAX_URL_BYTES: usize = 2048;
/// Wall-clock limit for the whole fetch, redirects included.
pub const TOTAL_SECONDS: u64 = 25;
const MAX_REDIRECTS: usize = 3;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FeedFetchError {
    /// The address is not one Bowser will fetch; the string says why.
    BadUrl(&'static str),
    /// Could not connect, or the answer did not arrive in time.
    Network,
    /// The server answered with this status instead of 200.
    Status(u32),
    TooLarge,
    /// A redirect went somewhere Bowser will not follow.
    BadRedirect,
    /// The host resolves, or connected, to an address that is not on the public internet.
    PrivateAddress,
}

impl std::fmt::Display for FeedFetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadUrl(why) => f.write_str(why),
            Self::Network => f.write_str("Could not reach that address. Check it and try again."),
            Self::Status(code) => write!(f, "The server answered {code}, not a feed."),
            Self::TooLarge => f.write_str("That answer is larger than Bowser reads (2 MB)."),
            Self::BadRedirect => f.write_str("That address redirects somewhere Bowser will not follow."),
            Self::PrivateAddress => f.write_str("That address points at a machine that is not on the public internet, so Bowser did not fetch it."),
        }
    }
}

impl From<FetchError> for FeedFetchError {
    fn from(error: FetchError) -> Self {
        match error {
            FetchError::PrivateAddress => Self::PrivateAddress,
            FetchError::TooLarge => Self::TooLarge,
            FetchError::BadRedirect => Self::BadRedirect,
            FetchError::Status(code) => Self::Status(code),
            FetchError::BadUrl(_) | FetchError::Network => Self::Network,
        }
    }
}

/// What a fetch returns: the address the answer really came from (after redirects) and its bytes.
#[derive(Debug)]
pub struct Fetched {
    pub url: Url,
    pub bytes: Vec<u8>,
}

/// Parses an address typed by the user (or listed in an OPML file): `http` or `https`, a public host
/// name, no credentials, the default port, no fragment, at most [`MAX_URL_BYTES`] bytes.
pub fn parse_url(text: &str) -> Result<Url, FeedFetchError> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_URL_BYTES || text.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(FeedFetchError::BadUrl("That is not a web address."));
    }
    // A bare "example.com/feed" is what people type: read it as https.
    let with_scheme = if text.contains("://") { text.to_owned() } else { format!("https://{text}") };
    let url = Url::parse(&with_scheme).map_err(|_| FeedFetchError::BadUrl("That is not a web address."))?;
    check_url(&url)?;
    Ok(url)
}

/// The rules every hop must meet.
pub fn check_url(url: &Url) -> Result<(), FeedFetchError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(FeedFetchError::BadUrl("Feeds are only fetched over http or https."));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(FeedFetchError::BadUrl("An address with a user name or password is not accepted."));
    }
    if url.port().is_some() {
        return Err(FeedFetchError::BadUrl("An address with its own port is not accepted."));
    }
    match url.host() {
        Some(url::Host::Domain(host)) if public_host_name(host) => Ok(()),
        _ => Err(FeedFetchError::BadUrl("Only public web sites are fetched, not local or numeric addresses.")),
    }
}

/// Fetches `url` and returns the body's bytes and the final address.
pub fn fetch(url: &Url) -> Result<Fetched, FeedFetchError> {
    let deadline = Deadline::after(Instant::now(), TOTAL_SECONDS);
    let mut current = url.clone();
    for _ in 0..=MAX_REDIRECTS {
        check_url(&current)?;
        match get_once(&current, deadline)? {
            Reply::Body(bytes) => return Ok(Fetched { url: current, bytes }),
            Reply::Redirect(target) => {
                let next = current.join(&target).map_err(|_| FeedFetchError::BadRedirect)?;
                check_url(&next).map_err(|_| FeedFetchError::BadRedirect)?;
                if !redirect_allowed(&current, &next) {
                    return Err(FeedFetchError::BadRedirect);
                }
                current = next;
            }
        }
    }
    Err(FeedFetchError::BadRedirect)
}

/// A hop may stay on its scheme or go from `http` up to `https`; never from `https` down to `http`.
pub fn redirect_allowed(from: &Url, to: &Url) -> bool {
    !(from.scheme() == "https" && to.scheme() == "http")
}

enum Reply {
    Body(Vec<u8>),
    Redirect(String),
}

/// WinHTTP's usual limits for a feed fetch (resolve, connect, send, receive), in milliseconds.
const FEED_TIMEOUTS: [i32; 4] = [8_000, 8_000, 10_000, 15_000];

fn get_once(url: &Url, deadline: Deadline) -> Result<Reply, FeedFetchError> {
    let host = url.host_str().ok_or(FeedFetchError::Network)?;
    // First check: every address the name resolves to must be public. A plain http hop then connects to
    // one of those very addresses (no second lookup to rebind); https keeps the name for its certificate.
    let vetted = resolve_public_by(host, deadline)?;
    let target = connect_target(url, &vetted).ok_or(FeedFetchError::Network)?;
    // Second check, below: every address WinHTTP actually resolves and connects to.
    let refused = AtomicBool::new(false);
    let mut path = url.path().to_owned();
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    let secure = url.scheme() == "https";
    let agent = HSTRING::from(concat!("Bowser/", env!("CARGO_PKG_VERSION")));
    let session = Handle(unsafe { WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_NO_PROXY, PCWSTR::null(), PCWSTR::null(), 0) });
    if session.0.is_null() {
        return Err(FeedFetchError::Network);
    }
    unsafe {
        let _ = WinHttpSetStatusCallback(session.0, Some(peer_watch), 0x0000_000f, 0);
    }
    bound_timeouts(session.0, deadline, FEED_TIMEOUTS, true)?;
    let connection = Handle(unsafe { WinHttpConnect(session.0, &HSTRING::from(target), if secure { 443 } else { 80 }, 0) });
    if connection.0.is_null() {
        return Err(FeedFetchError::Network);
    }
    let flags = if secure { WINHTTP_FLAG_SECURE } else { Default::default() };
    let request = Handle(unsafe { WinHttpOpenRequest(connection.0, w!("GET"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), flags) });
    if request.0.is_null() {
        return Err(FeedFetchError::Network);
    }
    let disable = WINHTTP_DISABLE_REDIRECTS | WINHTTP_DISABLE_COOKIES;
    unsafe { WinHttpSetOption(Some(request.0 as *const c_void), WINHTTP_OPTION_DISABLE_FEATURE, Some(&disable.to_ne_bytes())) }
        .map_err(|_| FeedFetchError::Network)?;
    if !secure {
        // Connected by address: the server still needs the name it is asked for.
        let host_header: Vec<u16> = format!("Host: {host}\r\n").encode_utf16().collect();
        unsafe { WinHttpAddRequestHeaders(request.0, &host_header, WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE) }.map_err(|_| FeedFetchError::Network)?;
    }
    let headers: Vec<u16> = "Accept: application/rss+xml, application/atom+xml, application/xml;q=0.9, text/xml;q=0.9, text/html;q=0.6, */*;q=0.1\r\n"
        .encode_utf16()
        .collect();
    let context = &refused as *const AtomicBool as usize;
    bound_timeouts(request.0, deadline, FEED_TIMEOUTS, true)?;
    let sent = unsafe { WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, context) };
    // A non-public peer ends the hop whatever else happened; its answer is never read.
    if refused.load(Ordering::SeqCst) {
        return Err(FeedFetchError::PrivateAddress);
    }
    sent.map_err(|_| FeedFetchError::Network)?;
    bound_timeouts(request.0, deadline, FEED_TIMEOUTS, false)?;
    let received = unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) };
    if refused.load(Ordering::SeqCst) {
        return Err(FeedFetchError::PrivateAddress);
    }
    received.map_err(|_| FeedFetchError::Network)?;
    deadline.remaining()?;
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
    .map_err(|_| FeedFetchError::Network)?;
    if matches!(status, 301 | 302 | 303 | 307 | 308) {
        return Ok(Reply::Redirect(location(request.0).map_err(FeedFetchError::from)?));
    }
    if status != 200 {
        return Err(FeedFetchError::Status(status));
    }
    let mut body = Vec::new();
    let mut buffer = vec![0u8; 32 * 1024];
    loop {
        bound_timeouts(request.0, deadline, FEED_TIMEOUTS, false)?;
        let mut read = 0u32;
        unsafe { WinHttpReadData(request.0, buffer.as_mut_ptr() as *mut c_void, buffer.len() as u32, &mut read) }.map_err(|_| FeedFetchError::Network)?;
        if read == 0 {
            return Ok(Reply::Body(body));
        }
        if body.len() + read as usize > MAX_BYTES {
            return Err(FeedFetchError::TooLarge);
        }
        body.extend_from_slice(&buffer[..read as usize]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_http_or_https_on_a_public_name_only() {
        assert_eq!(parse_url("https://example.com/feed.xml").unwrap().as_str(), "https://example.com/feed.xml");
        assert_eq!(parse_url("http://example.com/rss").unwrap().scheme(), "http");
        assert_eq!(parse_url("  example.com/feed  ").unwrap().as_str(), "https://example.com/feed", "a bare address is read as https");
        for bad in [
            "", "   ", "ftp://example.com/x", "file:///c:/windows/win.ini", "javascript:alert(1)", "data:text/xml,<a/>",
            "https://user:pw@example.com/x", "https://example.com:8080/x", "https://example.com:443x/x",
            "http://localhost/x", "https://intranet/x", "https://printer.local/x", "https://nas.lan/x",
            "http://127.0.0.1/x", "http://10.0.0.1/x", "http://192.168.1.1/x", "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/x", "http://[fe80::1]/x", "http://2130706433/x", "http://0x7f.1/x", "https://a b.example/x",
        ] {
            assert!(parse_url(bad).is_err(), "{bad:?} must be refused");
        }
        assert!(parse_url(&format!("https://example.com/{}", "a".repeat(MAX_URL_BYTES))).is_err());
        assert!(parse_url("https://exa\u{7}mple.com/").is_err());
    }

    #[test]
    fn a_redirect_may_not_leave_https_for_http() {
        let https = Url::parse("https://a.example/").unwrap();
        let http = Url::parse("http://b.example/").unwrap();
        assert!(redirect_allowed(&http, &https));
        assert!(redirect_allowed(&https, &https));
        assert!(redirect_allowed(&http, &http));
        assert!(!redirect_allowed(&https, &http));
    }

    #[test]
    fn every_hop_goes_through_the_same_checks() {
        // A public name whose redirect target is local is refused by check_url, which `fetch` applies to
        // each hop before connecting; the checks themselves are the ones above.
        for target in ["http://localhost/x", "http://127.0.0.1/feed", "https://user:pw@a.example/", "ftp://a.example/", "https://a.example:99/"] {
            assert!(check_url(&Url::parse(target).unwrap()).is_err(), "{target}");
        }
        assert!(check_url(&Url::parse("https://a.example/feed").unwrap()).is_ok());
    }

    #[test]
    fn fetch_refuses_a_private_target_before_any_connection() {
        // No network is used: the address is refused by the pure checks.
        assert!(matches!(fetch(&Url::parse("http://127.0.0.1/x").unwrap()), Err(FeedFetchError::BadUrl(_))));
        assert!(matches!(fetch(&Url::parse("http://localhost/x").unwrap()), Err(FeedFetchError::BadUrl(_))));
        assert!(matches!(fetch(&Url::parse("file:///c:/x").unwrap()), Err(FeedFetchError::BadUrl(_))));
    }
}
