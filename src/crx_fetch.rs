//! Turns an extension id, or a Chrome Web Store URL, into verified CRX bytes and a stamped unpack
//! folder. Library only: no menu, no host message, no `AddBrowserExtension`, nothing loaded.
//!
//! The protocol is Chromium's `ExtensionDownloader`: one HTTPS GET to
//! `https://clients2.google.com/service/update2/crx` asks for an *update manifest* (`<gupdate>` XML),
//! and the CRX is then downloaded from the `codebase` URL inside it. A store URL is parsed for its
//! 32-character `a`-`p` id and is never fetched itself.
//!
//! The manifest is read the way `extensions/browser/updater/safe_manifest_parser.cc` reads it, but
//! only for what is needed: the `<app appid>`, the `<updatecheck status>` and its `codebase`. The app
//! id must be the requested one. HTTP 204, an empty body or `status="noupdate"` mean no package. The
//! codebase must be HTTPS with a host and no credentials. A package offered only as `puff` is
//! refused with a clear error: this module cannot apply it, so the request asks for `crx3` only.
//!
//! Redirects, on the manifest and on the CRX, are followed the way `updater.rs` follows them: at most
//! five hops, each re-validated as plain HTTPS, never downgraded. Only the manifest request has a
//! fixed host; the CRX is accepted on its signature and id, not on where it came from, so the
//! codebase host needs no allowlist.
//!
//! The manifest is capped at [`MAX_MANIFEST_BYTES`]. The CRX stops at [`MAX_DOWNLOAD_BYTES`] (the
//! unpack total plus the CRX header cap): a declared `Content-Length` over it is refused before
//! reading, and an undeclared or lying body is cut off while it is read, never buffered past the cap.
//!
//! After the bytes are in memory: [`crate::crx::verify_crx3`] with the requested id (the developer
//! proof must derive that id; no Google publisher pin is required), then
//! [`crate::crx_unpack::unpack_stamped`] into an empty folder the caller created. Verification runs
//! before any file is written. If the unpack or the stamp fails, `unpack_stamped` removes every file
//! it created, so only the caller's empty folder is left. Nothing here deletes anything else.
//!
//! Identity: the request carries `prod=chromiumcrx`, Chromium's value for a build without Google
//! branding (`chromecrx` is Chrome's and is never sent), the `X-Goog-Update-*` protocol headers with
//! the updater name `chromiumcrx-<prodversion>`, and the user agent `Bowser-CrxFetch/<version>`. It
//! never claims to be Google Chrome.

use std::{
    ffi::c_void, fmt, path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use url::{Position, Url};
use windows::{
    Win32::Networking::WinHttp::{
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ACCESS_TYPE_NO_PROXY,
        WINHTTP_DISABLE_REDIRECTS, WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS,
        WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER,
        WINHTTP_QUERY_LOCATION, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
        WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
        WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
    },
    core::{HSTRING, PCWSTR, w},
};

use crate::{
    crx::{self, CrxError},
    crx_unpack::{self, StampError, StampedUnpack},
};

/// Most bytes a download may deliver: the unpack total (128 MiB) plus the CRX header cap (16 MiB).
pub const MAX_DOWNLOAD_BYTES: u64 = crx_unpack::MAX_TOTAL_BYTES + crx::MAX_HEADER_LEN as u64;

const UPDATE_HOST: &str = "clients2.google.com";
const UPDATE_PATH: &str = "/service/update2/crx";
/// Chromium's `kCrx` for a build without Google branding. Not Chrome.
const PROD: &str = "chromiumcrx";
/// Bowser has no release channel in Chromium's sense.
const PRODCHANNEL: &str = "unknown";
const LANG: &str = "en-US";
/// The `v=` Chromium sends for an extension that is not installed.
const NEW_INSTALL_VERSION: &str = "0.0.0.0";
/// An update manifest is a few hundred bytes. This is a generous ceiling, not a target.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const GUPDATE_NAMESPACE: &str = "http://www.google.com/update2/response";
const MAX_REDIRECTS: u32 = 5;
const ID_LEN: usize = 32;
/// How much of an error body is kept for diagnostics.
const ERROR_BODY_BYTES: usize = 200;
const CHROME_WEB_STORE_HOST: &str = "chromewebstore.google.com";
const OLD_STORE_HOST: &str = "chrome.google.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// Not exactly 32 characters of `a`-`p`.
    InvalidId,
    /// Not a Chrome Web Store extension URL this module understands.
    InvalidStoreUrl(&'static str),
    /// `prodversion` is not one to four dot-separated digit groups.
    InvalidProdVersion,
    Cancelled,
    /// A redirect left HTTPS, carried credentials, or was unusable.
    BadRedirect(&'static str),
    TooManyRedirects,
    /// The body is, or declares itself, larger than the cap.
    Oversize,
    /// The final status was not 200. `body` is at most [`ERROR_BODY_BYTES`] bytes of the response.
    Http { status: u32, body: Vec<u8> },
    /// The service offers no package for this id: HTTP 204, an empty body or `status="noupdate"`.
    NoUpdate,
    /// The update manifest is malformed or not acceptable. The text says why.
    Manifest(String),
    /// The manifest's `<app appid>` is not the id that was requested.
    AppIdMismatch,
    /// The only package offered is `puff`, which this module cannot apply.
    PuffOnly,
    Network(String),
    Verify(CrxError),
    Unpack(StampError),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId => f.write_str("extension id must be 32 characters a-p"),
            Self::InvalidStoreUrl(why) => write!(f, "not a Chrome Web Store extension URL: {why}"),
            Self::InvalidProdVersion => f.write_str("runtime version is not dotted digits"),
            Self::Cancelled => f.write_str("download cancelled; nothing was installed"),
            Self::BadRedirect(why) => write!(f, "refused redirect: {why}"),
            Self::TooManyRedirects => f.write_str("too many redirects"),
            Self::Oversize => f.write_str("download exceeds the size limit"),
            Self::Http { status, .. } => write!(f, "the store answered HTTP status {status}"),
            Self::NoUpdate => f.write_str("the store offers no package for this extension"),
            Self::Manifest(why) => write!(f, "update manifest refused: {why}"),
            Self::AppIdMismatch => f.write_str("update manifest is for a different extension id"),
            Self::PuffOnly => {
                f.write_str("the store offers only a puff package, which Bowser cannot apply")
            }
            Self::Network(why) => write!(f, "network error: {why}"),
            Self::Verify(error) => write!(f, "CRX verification failed: {error}"),
            Self::Unpack(error) => write!(f, "unpack failed: {error}"),
        }
    }
}

impl std::error::Error for FetchError {}

impl From<CrxError> for FetchError {
    fn from(error: CrxError) -> Self {
        Self::Verify(error)
    }
}

impl From<StampError> for FetchError {
    fn from(error: StampError) -> Self {
        Self::Unpack(error)
    }
}

// ---------------------------------------------------------------------------------------------
// Input parsing
// ---------------------------------------------------------------------------------------------

fn is_extension_id(text: &str) -> bool {
    text.len() == ID_LEN && text.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// Accepts exactly 32 characters from `a` to `p`. No trimming, no case folding.
pub fn parse_extension_id(text: &str) -> Result<String, FetchError> {
    if is_extension_id(text) {
        Ok(text.to_owned())
    } else {
        Err(FetchError::InvalidId)
    }
}

/// True when `text`, after one round of percent decoding, holds a run of 32 or more `a`-`p`
/// characters: something that could be read as a second extension id.
fn holds_id_like_run(text: &str) -> bool {
    url::form_urlencoded::parse(text.as_bytes()).any(|(key, value)| {
        [key, value].iter().any(|part| {
            let mut run = 0;
            part.bytes().any(|b| {
                run = if (b'a'..=b'p').contains(&b) { run + 1 } else { 0 };
                run >= ID_LEN
            })
        })
    })
}

/// Parses `https://chromewebstore.google.com/detail/[slug/]<id>` and the older
/// `https://chrome.google.com/webstore/detail/[slug/]<id>`, with an optional harmless query. The URL
/// is only read, never fetched. Any other host, scheme, port, credentials, path shape, uppercase or
/// malformed id, or a second id-like run in the slug, query or fragment is refused.
pub fn parse_store_url(text: &str) -> Result<String, FetchError> {
    let bad = FetchError::InvalidStoreUrl;
    let url = Url::parse(text).map_err(|_| bad("not a URL"))?;
    if url.scheme() != "https" {
        return Err(bad("not https"));
    }
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return Err(bad("credentials or port"));
    }
    let prefix: &[&str] = match url.host_str() {
        Some(CHROME_WEB_STORE_HOST) => &["detail"],
        Some(OLD_STORE_HOST) => &["webstore", "detail"],
        _ => return Err(bad("not a Chrome Web Store host")),
    };
    let mut segments: Vec<&str> = url.path_segments().map_or_else(Vec::new, Iterator::collect);
    if segments.last() == Some(&"") {
        segments.pop();
    }
    if segments.len() <= prefix.len() || segments[..prefix.len()] != *prefix {
        return Err(bad("not an extension detail path"));
    }
    let rest = &segments[prefix.len()..];
    let id = match rest {
        [id] => id,
        [slug, id] if !slug.is_empty() && !holds_id_like_run(slug) => id,
        _ => return Err(bad("unexpected path")),
    };
    if !is_extension_id(id) {
        return Err(bad("path does not end in a 32-character a-p id"));
    }
    if [url.query(), url.fragment()]
        .into_iter()
        .flatten()
        .any(holds_id_like_run)
    {
        return Err(bad("a second id in the query or fragment"));
    }
    Ok((*id).to_owned())
}

/// A bare id, or (when the text has a scheme) a Web Store URL.
pub fn parse_request(input: &str) -> Result<String, FetchError> {
    if input.contains("://") {
        parse_store_url(input)
    } else {
        parse_extension_id(input)
    }
}

/// One to four dot-separated groups of one to nine digits, as in a Chromium version.
fn valid_prodversion(text: &str) -> bool {
    let groups: Vec<&str> = text.split('.').collect();
    (1..=4).contains(&groups.len())
        && groups
            .iter()
            .all(|g| !g.is_empty() && g.len() <= 9 && g.bytes().all(|b| b.is_ascii_digit()))
}

// ---------------------------------------------------------------------------------------------
// Endpoint and request URL
// ---------------------------------------------------------------------------------------------

/// Where requests may go. Production has exactly one value, [`Endpoint::production`]: HTTPS to
/// `clients2.google.com`. The plain-HTTP loopback value exists only in test builds.
#[derive(Debug, Clone)]
struct Endpoint {
    secure: bool,
    host: String,
    port: u16,
}

impl Endpoint {
    fn production() -> Self {
        Self {
            secure: true,
            host: UPDATE_HOST.to_owned(),
            port: 443,
        }
    }

    #[cfg(test)]
    fn loopback(port: u16) -> Self {
        Self {
            secure: false,
            host: "127.0.0.1".to_owned(),
            port,
        }
    }

    fn scheme(&self) -> &'static str {
        if self.secure { "https" } else { "http" }
    }
}

fn arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86") {
        "x86"
    } else {
        "x64"
    }
}

/// The update-service request for one id: a GET, like Chromium's.
///
/// The keys follow Chromium's `UpdateQueryParams::Get(CRX)` (`components/update_client/
/// update_query_params.cc`: `os=%s&arch=%s&prod=%s` + delegate extras + `&acceptformat=...`), the
/// extras from `ChromeUpdateQueryParamsDelegate::GetExtraParams()` (`&prodchannel=`, `&prodversion=`,
/// `&lang=`), and the `x=` value from `ManifestFetchData::AddExtension` (`id=`, `v=`, `uc`, joined by
/// `&` and escaped as one value). `prod` is `chromiumcrx`, the value an unbranded Chromium build
/// sends (`chromecrx` is the Google Chrome build's). `v=0.0.0.0` is what Chromium sends for an
/// extension that is not installed yet. `prodversion` is the browser's version in Chromium; here it
/// is the WebView2 runtime that will run the extension.
///
/// `acceptformat` is `crx3`, not Chromium's `crx3,puff`, since this module cannot apply a `puff`
/// package and should not advertise one. The server answers with an update manifest, never the CRX.
fn request_url(
    endpoint: &Endpoint,
    id: &str,
    prodversion: &str,
) -> Result<Url, FetchError> {
    if !is_extension_id(id) {
        return Err(FetchError::InvalidId);
    }
    if !valid_prodversion(prodversion) {
        return Err(FetchError::InvalidProdVersion);
    }
    let mut url = Url::parse(&format!(
        "{}://{}:{}{UPDATE_PATH}",
        endpoint.scheme(),
        endpoint.host,
        endpoint.port
    ))
    .map_err(|_| FetchError::Network("bad endpoint".into()))?;
    url.query_pairs_mut()
        .append_pair("os", "win")
        .append_pair("arch", arch())
        .append_pair("prod", PROD)
        .append_pair("prodchannel", PRODCHANNEL)
        .append_pair("prodversion", prodversion)
        .append_pair("lang", LANG)
        .append_pair("acceptformat", "crx3")
        .append_pair("x", &format!("id={id}&v={NEW_INSTALL_VERSION}&uc"));
    Ok(url)
}

/// The protocol headers `ExtensionDownloader::CreateManifestLoader` adds for Web Store update URLs:
/// the updater name is `<prod>-<prodversion>` (here `chromiumcrx-...`, not a browser User-Agent), the
/// app id list, and `fg` because the user asked for this install. Both values were validated by
/// [`request_url`], so neither can carry a line break.
fn update_headers(id: &str, prodversion: &str) -> String {
    format!(
        "X-Goog-Update-Updater: {PROD}-{prodversion}\r\nX-Goog-Update-AppId: {id}\r\nX-Goog-Update-Interactivity: fg\r\n"
    )
}

/// Resolves a redirect target. It must stay on the endpoint's scheme (HTTPS in production), carry no
/// credentials and have a host. A plain-HTTP test endpoint also keeps the redirect on its own host,
/// so a loopback test can never be sent to a real server.
fn redirect_target(endpoint: &Endpoint, current: &Url, location: &str) -> Result<Url, FetchError> {
    let next = current
        .join(location)
        .map_err(|_| FetchError::BadRedirect("unparseable Location"))?;
    if next.scheme() != endpoint.scheme() {
        return Err(FetchError::BadRedirect("not the endpoint's scheme"));
    }
    if next.host_str().is_none_or(str::is_empty) {
        return Err(FetchError::BadRedirect("no host"));
    }
    if !next.username().is_empty() || next.password().is_some() {
        return Err(FetchError::BadRedirect("credentials in URL"));
    }
    if !endpoint.secure && next.host_str() != Some(endpoint.host.as_str()) {
        return Err(FetchError::BadRedirect("left the test host"));
    }
    Ok(next)
}

// ---------------------------------------------------------------------------------------------
// WinHTTP (same pattern as `updater.rs`: manual redirects, same timeouts, capped streaming read)
// ---------------------------------------------------------------------------------------------

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the handle came from WinHTTP and is closed exactly once, here.
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn net(what: &str, error: windows::core::Error) -> FetchError {
    FetchError::Network(format!("{what}: {error}"))
}

fn status(request: *mut c_void) -> Result<u32, FetchError> {
    let mut code = 0u32;
    let mut len = 4u32;
    // SAFETY: `code` is a 4-byte buffer and `len` says so.
    unsafe {
        WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut code as *mut u32 as *mut c_void),
            &mut len,
            std::ptr::null_mut(),
        )
    }
    .map_err(|e| net("status", e))?;
    Ok(code)
}

fn location(request: *mut c_void) -> Result<String, FetchError> {
    let mut len = 0u32;
    // SAFETY: a null buffer asks for the required length.
    let _ = unsafe {
        WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_LOCATION,
            PCWSTR::null(),
            None,
            &mut len,
            std::ptr::null_mut(),
        )
    };
    if len == 0 || len > 8192 {
        return Err(FetchError::BadRedirect("no usable Location"));
    }
    let mut buffer = vec![0u16; len as usize / 2 + 1];
    // SAFETY: `buffer` holds at least `len` bytes.
    unsafe {
        WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_LOCATION,
            PCWSTR::null(),
            Some(buffer.as_mut_ptr() as *mut c_void),
            &mut len,
            std::ptr::null_mut(),
        )
    }
    .map_err(|e| net("location", e))?;
    Ok(String::from_utf16_lossy(&buffer[..len as usize / 2]))
}

fn read_chunk(request: *mut c_void, buffer: &mut [u8]) -> Result<usize, FetchError> {
    let mut read = 0u32;
    // SAFETY: `buffer` is valid for `buffer.len()` bytes.
    unsafe {
        WinHttpReadData(
            request,
            buffer.as_mut_ptr() as *mut c_void,
            buffer.len() as u32,
            &mut read,
        )
    }
    .map_err(|e| net("read", e))?;
    Ok(read as usize)
}

/// At most [`ERROR_BODY_BYTES`] bytes of the response body, for error reports.
fn read_error_body(request: *mut c_void) -> Vec<u8> {
    let mut body = vec![0u8; ERROR_BODY_BYTES];
    let mut filled = 0;
    while filled < body.len() {
        match read_chunk(request, &mut body[filled..]) {
            Ok(0) | Err(_) => break,
            Ok(read) => filled += read,
        }
    }
    body.truncate(filled);
    body
}

/// GETs `start` from the endpoint's own host with the update protocol headers, handing body chunks
/// to `sink`. This is the manifest request: the only one whose host is fixed.
#[cfg(test)]
fn get(
    endpoint: &Endpoint,
    start: &Url,
    headers: &str,
    max_bytes: u64,
    sink: &mut dyn FnMut(&[u8]),
) -> Result<(), FetchError> {
    get_cancellable(endpoint, start, headers, max_bytes, None, sink)
}

fn get_cancellable(
    endpoint: &Endpoint,
    start: &Url,
    headers: &str,
    max_bytes: u64,
    cancelled: Option<&AtomicBool>,
    sink: &mut dyn FnMut(&[u8]),
) -> Result<(), FetchError> {
    if start.host_str() != Some(endpoint.host.as_str()) {
        return Err(FetchError::BadRedirect("request is not for the endpoint"));
    }
    transfer_cancellable(endpoint, start, Some(headers), max_bytes, cancelled, sink)
}

/// GETs `start`, handing body chunks to `sink`. `headers` (CRLF-terminated lines) go only to the
/// endpoint's own host. Fails on a scheme other than the endpoint's, a redirect that
/// [`redirect_target`] refuses, more than five hops, a declared body over `max_bytes`, a body that
/// grows past `max_bytes` while read (no chunk past the cap is passed on), or any final status but 200.
#[cfg(test)]
fn transfer(
    endpoint: &Endpoint,
    start: &Url,
    headers: Option<&str>,
    max_bytes: u64,
    sink: &mut dyn FnMut(&[u8]),
) -> Result<(), FetchError> {
    transfer_cancellable(endpoint, start, headers, max_bytes, None, sink)
}

fn transfer_cancellable(
    endpoint: &Endpoint, start: &Url, headers: Option<&str>, max_bytes: u64,
    cancelled: Option<&AtomicBool>, sink: &mut dyn FnMut(&[u8]),
) -> Result<(), FetchError> {
    let check_cancelled = || if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) { Err(FetchError::Cancelled) } else { Ok(()) };
    check_cancelled()?;
    if start.scheme() != endpoint.scheme() {
        return Err(FetchError::BadRedirect("request is not for the endpoint"));
    }
    let agent = HSTRING::from(concat!("Bowser-CrxFetch/", env!("CARGO_PKG_VERSION")));
    let access = if endpoint.secure {
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY
    } else {
        // Loopback test traffic must not be handed to a system proxy.
        WINHTTP_ACCESS_TYPE_NO_PROXY
    };
    // SAFETY: plain FFI call with valid arguments.
    let session = Handle(unsafe { WinHttpOpen(&agent, access, PCWSTR::null(), PCWSTR::null(), 0) });
    if session.0.is_null() {
        return Err(FetchError::Network("WinHttpOpen failed".into()));
    }
    // SAFETY: `session` is a live session handle.
    unsafe { WinHttpSetTimeouts(session.0, 10_000, 10_000, 30_000, 30_000) }
        .map_err(|e| net("timeouts", e))?;
    let request_flags = if endpoint.secure {
        WINHTTP_FLAG_SECURE
    } else {
        WINHTTP_OPEN_REQUEST_FLAGS(0)
    };
    let mut url = start.clone();
    for _hop in 0..=MAX_REDIRECTS {
        check_cancelled()?;
        let host = HSTRING::from(url.host_str().unwrap_or_default());
        let port = url.port_or_known_default().unwrap_or(endpoint.port);
        let object = HSTRING::from(&url[Position::BeforePath..Position::AfterQuery]);
        // SAFETY: `session` is live; `host` outlives the call.
        let connection = Handle(unsafe { WinHttpConnect(session.0, &host, port, 0) });
        if connection.0.is_null() {
            return Err(FetchError::Network("connect failed".into()));
        }
        // SAFETY: `connection` is live; `object` outlives the call.
        let request = Handle(unsafe {
            WinHttpOpenRequest(
                connection.0,
                w!("GET"),
                &object,
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                request_flags,
            )
        });
        if request.0.is_null() {
            return Err(FetchError::Network("open request failed".into()));
        }
        // SAFETY: `request` is live; the option value is a 4-byte flag.
        unsafe {
            WinHttpSetOption(
                Some(request.0 as *const c_void),
                WINHTTP_OPTION_DISABLE_FEATURE,
                Some(&WINHTTP_DISABLE_REDIRECTS.to_ne_bytes()),
            )
        }
        .map_err(|e| net("disable redirects", e))?;
        let wide_headers: Option<Vec<u16>> = headers
            .filter(|_| url.host_str() == Some(endpoint.host.as_str()))
            .map(|text| text.encode_utf16().collect());
        // SAFETY: `request` is live; `wide_headers` outlives the call; there is no body.
        unsafe { WinHttpSendRequest(request.0, wide_headers.as_deref(), None, 0, 0, 0) }
            .map_err(|e| net("send", e))?;
        // SAFETY: `request` is live and was sent.
        unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) }
            .map_err(|e| net("receive", e))?;
        let http_status = status(request.0)?;
        if std::env::var_os("BOWSER_EXTENSION_TRACE").is_some() {
            eprintln!("CRX_FETCH host={} http_status={http_status}", url.host_str().unwrap_or_default());
        }
        match http_status {
            301 | 302 | 303 | 307 | 308 => {
                url = redirect_target(endpoint, &url, &location(request.0)?)?;
                continue;
            }
            200 => {}
            other => {
                return Err(FetchError::Http {
                    status: other,
                    body: read_error_body(request.0),
                });
            }
        }
        let mut declared = 0u32;
        let mut len = 4u32;
        // SAFETY: `declared` is a 4-byte buffer and `len` says so.
        let has_length = unsafe {
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some(&mut declared as *mut u32 as *mut c_void),
                &mut len,
                std::ptr::null_mut(),
            )
        }
        .is_ok();
        if has_length && u64::from(declared) > max_bytes {
            return Err(FetchError::Oversize);
        }
        let mut buffer = vec![0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            check_cancelled()?;
            let read = read_chunk(request.0, &mut buffer)?;
            check_cancelled()?;
            if read == 0 {
                return Ok(());
            }
            total += read as u64;
            if total > max_bytes {
                return Err(FetchError::Oversize);
            }
            sink(&buffer[..read]);
        }
    }
    Err(FetchError::TooManyRedirects)
}

// ---------------------------------------------------------------------------------------------
// Update manifest
// ---------------------------------------------------------------------------------------------

/// What the manifest offers: where the CRX is.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Offer {
    codebase: Url,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TagKind {
    Start,
    Empty,
    End,
}

#[derive(Debug, Clone)]
struct Tag {
    /// The element name without any namespace prefix.
    name: String,
    kind: TagKind,
    attrs: Vec<(String, String)>,
}

impl Tag {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

struct App {
    tag: Tag,
    updatechecks: Vec<Tag>,
}

fn manifest_error(why: &str) -> FetchError {
    FetchError::Manifest(why.to_owned())
}

/// Decodes the five predefined XML entities and numeric character references. Anything else after
/// an `&` is refused, which also refuses a custom entity.
fn unescape(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let end = tail.find(';')?;
        let entity = &tail[..end];
        out.push(match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let digits = entity.strip_prefix('#')?;
                let code = match digits.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                    None => digits.parse().ok()?,
                };
                char::from_u32(code).filter(|c| !c.is_control())?
            }
        });
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// Parses one tag body (the text between `<` and `>`).
fn parse_tag(body: &str) -> Result<Tag, FetchError> {
    let bad = || manifest_error("malformed XML tag");
    let (kind, body) = if let Some(rest) = body.strip_prefix('/') {
        (TagKind::End, rest)
    } else if let Some(rest) = body.strip_suffix('/') {
        (TagKind::Empty, rest)
    } else {
        (TagKind::Start, body)
    };
    let body = body.trim();
    let name_end = body
        .find(|c: char| c.is_ascii_whitespace())
        .unwrap_or(body.len());
    let (qualified, mut rest) = body.split_at(name_end);
    let name = qualified.rsplit(':').next().unwrap_or_default();
    if name.is_empty() || qualified.contains(['"', '\'', '=']) {
        return Err(bad());
    }
    let mut attrs = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let eq = rest.find('=').ok_or_else(bad)?;
        let key = rest[..eq].trim();
        if key.is_empty() || key.contains(|c: char| c.is_ascii_whitespace() || c == '"' || c == '\'')
        {
            return Err(bad());
        }
        rest = rest[eq + 1..].trim_start();
        let quote = rest
            .chars()
            .next()
            .filter(|c| *c == '"' || *c == '\'')
            .ok_or_else(bad)?;
        let close = rest[1..].find(quote).ok_or_else(bad)? + 1;
        let value = unescape(&rest[1..close]).ok_or_else(bad)?;
        // `xmlns` declarations keep their key; any other prefix (`ns:status`) is dropped.
        let key = if key.starts_with("xmlns") {
            key
        } else {
            key.rsplit(':').next().unwrap_or(key)
        };
        attrs.push((key.to_owned(), value));
        rest = &rest[close + 1..];
    }
    if kind == TagKind::End && !attrs.is_empty() {
        return Err(bad());
    }
    Ok(Tag {
        name: name.to_owned(),
        kind,
        attrs,
    })
}

/// Splits `xml` into tags; text is ignored. The XML declaration, processing instructions and
/// comments are skipped. A DOCTYPE, an entity declaration or CDATA is refused outright.
fn scan_tags(xml: &str) -> Result<Vec<Tag>, FetchError> {
    let mut tags = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find('<') {
        rest = &rest[at..];
        if let Some(after) = rest.strip_prefix("<?") {
            let end = after
                .find("?>")
                .ok_or_else(|| manifest_error("unterminated <?"))?;
            rest = &after[end + 2..];
        } else if let Some(after) = rest.strip_prefix("<!--") {
            let end = after
                .find("-->")
                .ok_or_else(|| manifest_error("unterminated comment"))?;
            rest = &after[end + 3..];
        } else if rest.starts_with("<!") {
            return Err(manifest_error("DOCTYPE, entity or CDATA is not accepted"));
        } else {
            // The tag ends at the first `>` outside a quoted value.
            let mut quote = None;
            let mut end = None;
            for (index, c) in rest.char_indices().skip(1) {
                match (quote, c) {
                    (None, '>') => {
                        end = Some(index);
                        break;
                    }
                    (None, '"' | '\'') => quote = Some(c),
                    (Some(q), c) if c == q => quote = None,
                    (None, '<') => return Err(manifest_error("malformed XML tag")),
                    _ => {}
                }
            }
            let end = end.ok_or_else(|| manifest_error("unterminated tag"))?;
            tags.push(parse_tag(&rest[1..end])?);
            rest = &rest[end + 1..];
        }
    }
    Ok(tags)
}

/// A short, printable rendering of a server-supplied word, for an error message.
fn printable(text: &str) -> String {
    text.chars().filter(char::is_ascii_graphic).take(40).collect()
}

/// Validates the `codebase` URL against the endpoint's rules: the endpoint's scheme (HTTPS in
/// production), a host, no credentials, and on a plain-HTTP test endpoint that same host only.
fn codebase_url(endpoint: &Endpoint, text: &str) -> Result<Url, FetchError> {
    if text.is_empty() {
        return Err(manifest_error("empty codebase"));
    }
    let url = Url::parse(text).map_err(|_| manifest_error("codebase is not a URL"))?;
    if url.scheme() != endpoint.scheme() {
        return Err(manifest_error("codebase is not https"));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(manifest_error("codebase has no host"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(manifest_error("codebase has credentials"));
    }
    if !endpoint.secure && url.host_str() != Some(endpoint.host.as_str()) {
        return Err(manifest_error("codebase left the test host"));
    }
    Ok(url)
}

/// Reads an update manifest for `id`, as `safe_manifest_parser.cc` does but for three things only:
/// `<app appid status>` and `<updatecheck status codebase format>` inside a `<gupdate>` 2.0 root.
/// The version, hash, size and the rest are ignored: the CRX is accepted on its signature and id.
fn parse_update_manifest(endpoint: &Endpoint, xml: &[u8], id: &str) -> Result<Offer, FetchError> {
    let xml = std::str::from_utf8(xml).map_err(|_| manifest_error("not UTF-8"))?;
    let xml = xml.strip_prefix('\u{feff}').unwrap_or(xml);
    if xml.trim().is_empty() {
        return Err(FetchError::NoUpdate);
    }

    let mut stack: Vec<String> = Vec::new();
    let mut apps: Vec<App> = Vec::new();
    for tag in scan_tags(xml)? {
        if tag.kind == TagKind::End {
            if stack.pop().as_deref() != Some(tag.name.as_str()) {
                return Err(manifest_error("mismatched XML tags"));
            }
            continue;
        }
        if stack.is_empty() {
            let namespaced = tag
                .attrs
                .iter()
                .any(|(key, value)| key.starts_with("xmlns") && value == GUPDATE_NAMESPACE);
            if tag.name != "gupdate" || !namespaced || tag.attr("protocol") != Some("2.0") {
                return Err(manifest_error("not a gupdate 2.0 manifest"));
            }
        } else if tag.name == "app" && stack.len() == 1 {
            apps.push(App {
                tag: tag.clone(),
                updatechecks: Vec::new(),
            });
        } else if tag.name == "updatecheck" && stack.len() == 2 && stack[1] == "app" {
            if let Some(app) = apps.last_mut() {
                app.updatechecks.push(tag.clone());
            }
        }
        if tag.kind == TagKind::Start {
            stack.push(tag.name);
        }
    }
    if !stack.is_empty() {
        return Err(manifest_error("unterminated XML"));
    }

    if apps.iter().any(|app| app.tag.attr("appid") != Some(id)) {
        return Err(FetchError::AppIdMismatch);
    }
    let app = match apps.as_slice() {
        [] => return Err(manifest_error("no app in the manifest")),
        [app] => app,
        _ => return Err(manifest_error("more than one app in the manifest")),
    };
    match app.tag.attr("status") {
        None | Some("" | "ok") => {}
        Some(other) => return Err(manifest_error(&format!("app status {}", printable(other)))),
    }
    let [check] = app.updatechecks.as_slice() else {
        return Err(manifest_error("expected exactly one updatecheck"));
    };
    match check.attr("status") {
        Some("noupdate") => return Err(FetchError::NoUpdate),
        None | Some("" | "ok") => {}
        Some(other) => {
            return Err(manifest_error(&format!("updatecheck status {}", printable(other))));
        }
    }
    let codebase = codebase_url(endpoint, check.attr("codebase").unwrap_or_default())?;
    // Chromium's parser has no notion of `puff`; a package is called puff here when the check names
    // that format without `crx3`, or the codebase file is a `.puff`.
    let formats = check.attr("format").unwrap_or_default().to_ascii_lowercase();
    let names = |wanted: &str| formats.split(',').any(|f| f.trim() == wanted);
    let puff_path = codebase.path().to_ascii_lowercase().ends_with(".puff");
    if puff_path || (names("puff") && !names("crx3")) {
        return Err(FetchError::PuffOnly);
    }
    Ok(Offer { codebase })
}

/// GETs the update manifest for `id`. Any status but 200 is a [`FetchError::Http`], 204 included.
#[cfg(test)]
fn fetch_manifest_bytes(
    endpoint: &Endpoint,
    id: &str,
    prodversion: &str,
) -> Result<Vec<u8>, FetchError> {
    fetch_manifest_bytes_cancellable(endpoint, id, prodversion, None)
}

fn fetch_manifest_bytes_cancellable(
    endpoint: &Endpoint, id: &str, prodversion: &str, cancelled: Option<&AtomicBool>,
) -> Result<Vec<u8>, FetchError> {
    let url = request_url(endpoint, id, prodversion)?;
    let mut bytes = Vec::new();
    get_cancellable(
        endpoint,
        &url,
        &update_headers(id, prodversion),
        MAX_MANIFEST_BYTES,
        cancelled,
        &mut |chunk| bytes.extend_from_slice(chunk),
    )?;
    Ok(bytes)
}

/// Asks the update service what it offers for `id`: the manifest request, then the manifest rules.
fn fetch_offer_cancellable(endpoint: &Endpoint, id: &str, prodversion: &str, cancelled: Option<&AtomicBool>) -> Result<Offer, FetchError> {
    let manifest = match fetch_manifest_bytes_cancellable(endpoint, id, prodversion, cancelled) {
        Err(FetchError::Http { status: 204, .. }) => return Err(FetchError::NoUpdate),
        other => other?,
    };
    parse_update_manifest(endpoint, &manifest, id)
}

/// Downloads `url` (a validated codebase), holding at most `max_bytes` in memory.
#[cfg(test)]
fn download_bytes(endpoint: &Endpoint, url: &Url, max_bytes: u64) -> Result<Vec<u8>, FetchError> {
    download_bytes_cancellable(endpoint, url, max_bytes, None)
}

fn download_bytes_cancellable(endpoint: &Endpoint, url: &Url, max_bytes: u64, cancelled: Option<&AtomicBool>) -> Result<Vec<u8>, FetchError> {
    let mut bytes = Vec::new();
    transfer_cancellable(endpoint, url, None, max_bytes, cancelled, &mut |chunk| {
        bytes.extend_from_slice(chunk)
    })?;
    Ok(bytes)
}

/// Manifest, then CRX: downloads the CRX for `id` via `endpoint`, holding at most `max_bytes`.
fn download_from(
    endpoint: &Endpoint,
    id: &str,
    prodversion: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, FetchError> {
    download_from_cancellable(endpoint, id, prodversion, max_bytes, None)
}

fn download_from_cancellable(
    endpoint: &Endpoint, id: &str, prodversion: &str, max_bytes: u64, cancelled: Option<&AtomicBool>,
) -> Result<Vec<u8>, FetchError> {
    let offer = fetch_offer_cancellable(endpoint, id, prodversion, cancelled)?;
    download_bytes_cancellable(endpoint, &offer.codebase, max_bytes, cancelled)
}

// ---------------------------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------------------------

/// Downloads the CRX for `id`: the update manifest from the Google update service, then the
/// manifest's codebase. Not yet verified.
pub fn download_crx(id: &str, prodversion: &str) -> Result<Vec<u8>, FetchError> {
    download_from(&Endpoint::production(), id, prodversion, MAX_DOWNLOAD_BYTES)
}

/// Verifies `crx` for `id` and unpacks it, stamped with its developer key, into `dest`: a folder the
/// caller just created and that is still empty. Verification comes first, so a CRX that does not
/// verify writes nothing. If the unpack or the stamp fails, the files it created are removed; a
/// destination that already holds something is refused and left untouched.
pub fn verify_and_unpack(crx: &[u8], id: &str, dest: &Path) -> Result<StampedUnpack, FetchError> {
    let verified = crx::verify_crx3(crx, id)?;
    Ok(crx_unpack::unpack_stamped(&verified, dest)?)
}

fn fetch_from(
    endpoint: &Endpoint,
    input: &str,
    prodversion: &str,
    dest: &Path,
) -> Result<StampedUnpack, FetchError> {
    let id = parse_request(input)?;
    let bytes = download_from(endpoint, &id, prodversion, MAX_DOWNLOAD_BYTES)?;
    verify_and_unpack(&bytes, &id, dest)
}

/// Extension id or Chrome Web Store URL in; verified, stamped unpack folder out. `prodversion` is
/// the WebView2 runtime version (dotted digits). The returned [`StampedUnpack`] holds the id.
pub fn fetch_and_unpack(
    input: &str,
    prodversion: &str,
    dest: &Path,
) -> Result<StampedUnpack, FetchError> {
    fetch_from(&Endpoint::production(), input, prodversion, dest)
}

/// The same verified pipeline with cooperative cancellation between network operations/chunks.
/// A blocked WinHTTP call still obeys its timeout. Cancellation is checked again before unpack;
/// the Store caller checks it after unpack and never commits a cancelled review.
pub fn fetch_and_unpack_cancellable(
    input: &str, prodversion: &str, dest: &Path, cancelled: &AtomicBool,
) -> Result<StampedUnpack, FetchError> {
    let id = parse_request(input)?;
    let bytes = download_from_cancellable(&Endpoint::production(), &id, prodversion, MAX_DOWNLOAD_BYTES, Some(cancelled))?;
    if cancelled.load(Ordering::Acquire) { return Err(FetchError::Cancelled); }
    verify_and_unpack(&bytes, &id, dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::{Cursor, Read, Write},
        net::{SocketAddr, TcpListener, TcpStream},
        path::PathBuf,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        },
        thread::JoinHandle,
        time::{Duration, Instant},
    };

    const ID: &str = "cjpalhdlnbpafiamejdnhcphjbkeiagm";
    const OTHER_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const PRODVERSION: &str = "141.0.3537.71";

    // ---- id parser -------------------------------------------------------------------------

    #[test]
    fn id_parser_accepts_exactly_thirty_two_a_to_p() {
        assert_eq!(parse_extension_id(ID).as_deref(), Ok(ID));
        assert_eq!(parse_extension_id("abcdefghijklmnopabcdefghijklmnop").as_deref(), Ok("abcdefghijklmnopabcdefghijklmnop"));
    }

    #[test]
    fn id_parser_rejects_everything_else() {
        let bad = [
            "",
            &ID[..31],
            &format!("{ID}a"),
            &ID.to_uppercase(),
            "qjpalhdlnbpafiamejdnhcphjbkeiagm",          // q is outside a-p
            "0jpalhdlnbpafiamejdnhcphjbkeiagm",           // digit
            &format!(" {ID}"),
            &format!("{ID}\n"),
            &format!("{ID}/"),
            &format!("/{}", &ID[1..]),
            &format!("../{}", &ID[3..]),
            &format!("{ID}&{OTHER_ID}"),
            &format!("{ID}{OTHER_ID}"),
            "cjpalhdlnbpafiamejdnhcphjbkeiag\u{e9}",
        ];
        for text in bad {
            assert_eq!(parse_extension_id(text), Err(FetchError::InvalidId), "{text:?}");
        }
    }

    // ---- store URL parser ------------------------------------------------------------------

    #[test]
    fn store_url_parser_reads_the_current_store_form() {
        for text in [
            format!("https://chromewebstore.google.com/detail/ublock-origin/{ID}"),
            format!("https://chromewebstore.google.com/detail/ublock-origin/{ID}?hl=en&authuser=0"),
            format!("https://chromewebstore.google.com/detail/ublock-origin/{ID}/"),
            format!("https://chromewebstore.google.com/detail/{ID}"),
            format!("https://CHROMEWEBSTORE.google.com/detail/%C3%BCblock/{ID}#reviews"),
            format!("https://chromewebstore.google.com:443/detail/x/{ID}"),
        ] {
            assert_eq!(parse_store_url(&text).as_deref(), Ok(ID), "{text}");
        }
    }

    #[test]
    fn store_url_parser_reads_the_older_chrome_google_com_form() {
        for text in [
            format!("https://chrome.google.com/webstore/detail/ublock-origin/{ID}"),
            format!("https://chrome.google.com/webstore/detail/ublock-origin/{ID}?hl=en-GB"),
            format!("https://chrome.google.com/webstore/detail/{ID}"),
        ] {
            assert_eq!(parse_store_url(&text).as_deref(), Ok(ID), "{text}");
        }
    }

    #[test]
    fn store_url_parser_rejects_other_hosts_schemes_and_ports() {
        for text in [
            format!("http://chromewebstore.google.com/detail/x/{ID}"),
            format!("https://evil.example/detail/x/{ID}"),
            format!("https://chromewebstore.google.com.evil.example/detail/x/{ID}"),
            format!("https://evilchromewebstore.google.com/detail/x/{ID}"),
            format!("https://www.chromewebstore.google.com/detail/x/{ID}"),
            format!("https://chromewebstore.google.com./detail/x/{ID}"),
            format!("https://chromewebstore.google.com@evil.example/detail/x/{ID}"),
            format!("https://user:pw@chromewebstore.google.com/detail/x/{ID}"),
            format!("https://chromewebstore.google.com:8443/detail/x/{ID}"),
            format!("https://clients2.google.com/service/update2/crx?x=id%3D{ID}%26uc"),
            format!("https://google.com/webstore/detail/x/{ID}"),
            format!("https://microsoftedge.microsoft.com/addons/detail/x/{ID}"),
            format!("ftp://chromewebstore.google.com/detail/x/{ID}"),
            format!("javascript:alert(1)//chromewebstore.google.com/detail/x/{ID}"),
            format!("chromewebstore.google.com/detail/x/{ID}"),
        ] {
            assert!(
                matches!(parse_store_url(&text), Err(FetchError::InvalidStoreUrl(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn store_url_parser_rejects_wrong_paths_and_ids() {
        for text in [
            "https://chromewebstore.google.com/".to_owned(),
            "https://chromewebstore.google.com/category/extensions".to_owned(),
            "https://chromewebstore.google.com/search/ublock".to_owned(),
            "https://chromewebstore.google.com/detail/".to_owned(),
            "https://chromewebstore.google.com/detail/ublock-origin".to_owned(),
            format!("https://chromewebstore.google.com/webstore/detail/x/{ID}"),
            format!("https://chrome.google.com/detail/x/{ID}"),
            format!("https://chrome.google.com/webstore/x/{ID}"),
            format!("https://chrome.google.com/webstore/category/{ID}"),
            format!("https://chromewebstore.google.com/detail/x/{ID}/reviews"),
            format!("https://chromewebstore.google.com/detail/x/y/{ID}"),
            format!("https://chromewebstore.google.com/detail//{ID}"),
            format!("https://chromewebstore.google.com/detail/x/{}", ID.to_uppercase()),
            format!("https://chromewebstore.google.com/detail/x/{}", &ID[..31]),
            format!("https://chromewebstore.google.com/detail/x/{ID}a"),
            format!("https://chromewebstore.google.com/detail/x/{ID}%2F"),
        ] {
            assert!(
                matches!(parse_store_url(&text), Err(FetchError::InvalidStoreUrl(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn store_url_parser_rejects_a_second_id_anywhere() {
        for text in [
            format!("https://chromewebstore.google.com/detail/x/{ID}?id={OTHER_ID}"),
            format!("https://chromewebstore.google.com/detail/x/{ID}?hl=en&{OTHER_ID}"),
            format!("https://chromewebstore.google.com/detail/x/{ID}?next=%2Fdetail%2Fx%2F{OTHER_ID}"),
            format!("https://chromewebstore.google.com/detail/x/{ID}?{OTHER_ID}={OTHER_ID}"),
            format!("https://chromewebstore.google.com/detail/x/{ID}#{OTHER_ID}"),
            format!("https://chromewebstore.google.com/detail/{OTHER_ID}/{ID}"),
            format!("https://chromewebstore.google.com/detail/%61aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/{ID}"),
            format!("https://chrome.google.com/webstore/detail/{OTHER_ID}/{ID}"),
            format!("https://chrome.google.com/webstore/detail/x/{ID}?id={ID}"),
        ] {
            assert!(
                matches!(parse_store_url(&text), Err(FetchError::InvalidStoreUrl(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn request_parser_takes_an_id_or_a_store_url_and_nothing_else() {
        assert_eq!(parse_request(ID).as_deref(), Ok(ID));
        assert_eq!(
            parse_request(&format!("https://chromewebstore.google.com/detail/x/{ID}")).as_deref(),
            Ok(ID)
        );
        assert_eq!(parse_request("ublock origin"), Err(FetchError::InvalidId));
        assert_eq!(parse_request(""), Err(FetchError::InvalidId));
        assert!(matches!(
            parse_request("https://example.com/"),
            Err(FetchError::InvalidStoreUrl(_))
        ));
    }

    // ---- request URL and redirects ---------------------------------------------------------

    fn query_of(url: &Url) -> Vec<(String, String)> {
        url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
    }

    #[test]
    fn production_request_goes_only_to_the_update_host_over_https() {
        let url = request_url(&Endpoint::production(), ID, PRODVERSION).unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("clients2.google.com"));
        assert_eq!(url.path(), "/service/update2/crx");
        assert_eq!(url.port(), None);
        let query = query_of(&url);
        let get = |key: &str| query.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        assert_eq!(get("prod"), Some("chromiumcrx"));
        assert_eq!(get("prodversion"), Some(PRODVERSION));
        assert_eq!(get("prodchannel"), Some("unknown"));
        assert_eq!(get("lang"), Some("en-US"));
        assert_eq!(get("acceptformat"), Some("crx3"));
        assert_eq!(get("x"), Some(format!("id={ID}&v=0.0.0.0&uc").as_str()));
        assert!(url.query().unwrap().contains(&format!("x=id%3D{ID}%26v%3D0.0.0.0%26uc")));
        assert!(!url.as_str().to_ascii_lowercase().contains("prod=chrome&"));
        assert!(!url.as_str().contains("chromecrx"));
        assert_eq!(get("response"), None, "response=redirect is not Chromium's");
        assert!(!url.as_str().contains("redirect"));
    }

    #[test]
    fn update_headers_are_the_chromium_protocol_headers_with_the_unbranded_updater() {
        let headers = update_headers(ID, PRODVERSION);
        assert_eq!(
            headers,
            format!(
                "X-Goog-Update-Updater: chromiumcrx-{PRODVERSION}\r\nX-Goog-Update-AppId: {ID}\r\nX-Goog-Update-Interactivity: fg\r\n"
            )
        );
        assert!(!headers.to_ascii_lowercase().contains("chromecrx"));
        assert!(!headers.to_ascii_lowercase().contains("user-agent"));
    }

    #[test]
    fn request_url_refuses_a_bad_id_or_a_bad_prodversion() {
        let endpoint = Endpoint::production();
        assert_eq!(request_url(&endpoint, "short", PRODVERSION).unwrap_err(), FetchError::InvalidId);
        assert_eq!(
            request_url(&endpoint, ID, "1.2&x=id%3D").unwrap_err(),
            FetchError::InvalidProdVersion
        );
        assert_eq!(request_url(&endpoint, ID, "").unwrap_err(), FetchError::InvalidProdVersion);
        assert_eq!(request_url(&endpoint, ID, "1.2.3.4.5").unwrap_err(), FetchError::InvalidProdVersion);
    }

    #[test]
    fn production_redirects_must_stay_on_plain_https() {
        let endpoint = Endpoint::production();
        let current = request_url(&endpoint, ID, PRODVERSION).unwrap();
        for location in [
            "http://clients2.google.com/crx",
            "http://clients2.googleusercontent.com/crx/blobs/x",
            "ftp://clients2.google.com/x",
            "https://user:pw@clients2.googleusercontent.com/x",
            "https://",
            "javascript:alert(1)",
        ] {
            assert!(redirect_target(&endpoint, &current, location).is_err(), "{location}");
        }
        // Like `updater.rs`: any HTTPS host is followed. The CRX is accepted on its signature and id.
        let next = redirect_target(&endpoint, &current, "https://clients2.googleusercontent.com/crx/blobs/x").unwrap();
        assert_eq!(next.host_str(), Some("clients2.googleusercontent.com"));
        let relative = redirect_target(&endpoint, &current, "/other?a=1").unwrap();
        assert_eq!(relative.host_str(), Some("clients2.google.com"));
    }

    #[test]
    fn loopback_endpoint_cannot_be_redirected_to_a_real_host() {
        let endpoint = Endpoint::loopback(4242);
        let current = request_url(&endpoint, ID, PRODVERSION).unwrap();
        assert_eq!(current.host_str(), Some("127.0.0.1"));
        for location in [
            "http://clients2.google.com/service/update2/crx",
            "https://clients2.google.com/service/update2/crx",
            "http://example.com/",
            "http://127.0.0.2:4242/",
        ] {
            assert!(redirect_target(&endpoint, &current, location).is_err(), "{location}");
        }
        assert!(redirect_target(&endpoint, &current, "http://127.0.0.1:4242/next").is_ok());
        assert!(redirect_target(&endpoint, &current, "/next").is_ok());
    }

    #[test]
    fn a_request_for_another_host_is_refused_before_connecting() {
        let endpoint = Endpoint::loopback(4242);
        let url = Url::parse("http://clients2.google.com/service/update2/crx").unwrap();
        let result = get(&endpoint, &url, "", 1024, &mut |_| panic!("must not read"));
        assert!(matches!(result, Err(FetchError::BadRedirect(_))));
        let production = Endpoint::production();
        let plain = Url::parse("http://clients2.google.com/service/update2/crx").unwrap();
        assert!(matches!(
            get(&production, &plain, "", 1024, &mut |_| panic!("must not read")),
            Err(FetchError::BadRedirect(_))
        ));
        // The CRX transfer is not host-pinned but still keeps the endpoint's scheme.
        assert!(matches!(
            transfer(&production, &plain, None, 1024, &mut |_| panic!("must not read")),
            Err(FetchError::BadRedirect(_))
        ));
    }

    #[test]
    fn production_manifest_host_is_only_clients2_google_com() {
        assert_eq!(UPDATE_HOST, "clients2.google.com");
        let production = Endpoint::production();
        assert!(production.secure);
        assert_eq!((production.host.as_str(), production.port), ("clients2.google.com", 443));
        // The test endpoint is plain HTTP on 127.0.0.1 and cannot be pointed at the real host: not as
        // a request, a redirect or a manifest codebase.
        let test = Endpoint::loopback(4242);
        assert_eq!(test.host, "127.0.0.1");
        assert!(!test.secure);
        let real = Url::parse("https://clients2.google.com/service/update2/crx").unwrap();
        assert!(matches!(
            get(&test, &real, "", 1024, &mut |_| panic!("must not read")),
            Err(FetchError::BadRedirect(_))
        ));
        assert!(matches!(
            transfer(&test, &real, None, 1024, &mut |_| panic!("must not read")),
            Err(FetchError::BadRedirect(_))
        ));
        assert!(matches!(
            codebase_url(&test, "https://clients2.google.com/crx"),
            Err(FetchError::Manifest(_))
        ));
        assert!(matches!(
            codebase_url(&test, "http://clients2.google.com/crx"),
            Err(FetchError::Manifest(_))
        ));
    }

    // ---- update manifest -------------------------------------------------------------------

    const CODEBASE: &str =
        "https://clients2.googleusercontent.com/crx/blobs/AaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaZw/extension_1_2.crx";

    fn manifest_with(app: &str, check: &str) -> String {
        format!(
            "<?xml version='1.0' encoding='UTF-8'?>\n<gupdate xmlns=\"{GUPDATE_NAMESPACE}\" protocol=\"2.0\">\n  <daystart elapsed_seconds=\"3600\"/>\n  <app {app}>\n    <updatecheck {check}/>\n  </app>\n</gupdate>"
        )
    }

    fn good_manifest(id: &str, codebase: &str) -> String {
        manifest_with(
            &format!("appid=\"{id}\" cohort=\"1:\" cohortname=\"Stable\" status=\"ok\""),
            &format!("codebase=\"{codebase}\" fp=\"1.abc\" hash_sha256=\"00\" size=\"123\" status=\"ok\" version=\"1.2\""),
        )
    }

    #[test]
    fn a_manifest_with_the_requested_id_and_an_https_codebase_is_accepted() {
        let offer = parse_update_manifest(
            &Endpoint::production(),
            good_manifest(ID, CODEBASE).as_bytes(),
            ID,
        )
        .unwrap();
        assert_eq!(offer.codebase.as_str(), CODEBASE);
    }

    #[test]
    fn a_manifest_codebase_with_escaped_ampersands_is_decoded() {
        let manifest = good_manifest(ID, "https://dl.example/crx?a=1&amp;b=2");
        let offer = parse_update_manifest(&Endpoint::production(), manifest.as_bytes(), ID).unwrap();
        assert_eq!(offer.codebase.query(), Some("a=1&b=2"));
    }

    #[test]
    fn a_manifest_with_a_prefix_namespace_and_single_quotes_is_accepted() {
        let manifest = format!(
            "<g:gupdate xmlns:g='{GUPDATE_NAMESPACE}' protocol='2.0'><g:app appid='{ID}' status='ok'><g:updatecheck codebase='{CODEBASE}' status='ok'/></g:app></g:gupdate>"
        );
        assert!(parse_update_manifest(&Endpoint::production(), manifest.as_bytes(), ID).is_ok());
    }

    #[test]
    fn a_noupdate_status_and_an_empty_answer_mean_no_package() {
        let production = Endpoint::production();
        let noupdate = manifest_with(
            &format!("appid=\"{ID}\" status=\"ok\""),
            "status=\"noupdate\" info=\"bandwidth limit\"",
        );
        assert_eq!(parse_update_manifest(&production, noupdate.as_bytes(), ID), Err(FetchError::NoUpdate));
        assert_eq!(parse_update_manifest(&production, b"", ID), Err(FetchError::NoUpdate));
        assert_eq!(parse_update_manifest(&production, b" \r\n", ID), Err(FetchError::NoUpdate));
    }

    #[test]
    fn a_manifest_for_a_different_app_id_is_refused() {
        let production = Endpoint::production();
        let other = good_manifest(OTHER_ID, CODEBASE);
        assert_eq!(parse_update_manifest(&production, other.as_bytes(), ID), Err(FetchError::AppIdMismatch));
        // One matching app does not excuse a second, foreign one.
        let mixed = format!(
            "<gupdate xmlns=\"{GUPDATE_NAMESPACE}\" protocol=\"2.0\"><app appid=\"{ID}\"><updatecheck codebase=\"{CODEBASE}\"/></app><app appid=\"{OTHER_ID}\"><updatecheck codebase=\"{CODEBASE}\"/></app></gupdate>"
        );
        assert_eq!(parse_update_manifest(&production, mixed.as_bytes(), ID), Err(FetchError::AppIdMismatch));
        // Case is not folded.
        let upper = good_manifest(&ID.to_uppercase(), CODEBASE);
        assert_eq!(parse_update_manifest(&production, upper.as_bytes(), ID), Err(FetchError::AppIdMismatch));
    }

    #[test]
    fn a_codebase_that_is_not_plain_https_with_a_host_is_refused() {
        let production = Endpoint::production();
        for codebase in [
            "http://clients2.googleusercontent.com/crx/x.crx",
            "ftp://example.com/x.crx",
            "https://user:pw@example.com/x.crx",
            "https://user@example.com/x.crx",
            "https://",
            "https://:443/x.crx",
            "//example.com/x.crx",
            "/crx/x.crx",
            "javascript:alert(1)",
            "file:///c:/x.crx",
            "",
        ] {
            let manifest = good_manifest(ID, codebase);
            assert!(
                matches!(
                    parse_update_manifest(&production, manifest.as_bytes(), ID),
                    Err(FetchError::Manifest(_))
                ),
                "{codebase:?}"
            );
        }
        // No codebase attribute at all.
        let missing = manifest_with(&format!("appid=\"{ID}\""), "status=\"ok\" version=\"1\"");
        assert!(matches!(
            parse_update_manifest(&production, missing.as_bytes(), ID),
            Err(FetchError::Manifest(_))
        ));
    }

    #[test]
    fn a_puff_only_package_is_refused_with_its_own_error() {
        let production = Endpoint::production();
        for check in [
            format!("codebase=\"{CODEBASE}\" format=\"puff\""),
            format!("codebase=\"{CODEBASE}\" format=\"PUFF\" status=\"ok\""),
            "codebase=\"https://dl.example/blobs/x.puff\" status=\"ok\"".to_owned(),
        ] {
            let manifest = manifest_with(&format!("appid=\"{ID}\""), &check);
            assert_eq!(
                parse_update_manifest(&production, manifest.as_bytes(), ID),
                Err(FetchError::PuffOnly),
                "{check}"
            );
        }
        assert!(FetchError::PuffOnly.to_string().contains("puff"));
        // A crx3 package offered next to puff is fine.
        let both = manifest_with(
            &format!("appid=\"{ID}\""),
            &format!("codebase=\"{CODEBASE}\" format=\"crx3,puff\""),
        );
        assert!(parse_update_manifest(&production, both.as_bytes(), ID).is_ok());
    }

    #[test]
    fn a_malformed_or_hostile_manifest_is_refused() {
        let production = Endpoint::production();
        let good = good_manifest(ID, CODEBASE);
        let cases: Vec<(&str, String)> = vec![
            ("not xml", "hello".to_owned()),
            ("a crx", "Cr24\u{3}\u{0}".to_owned()),
            ("wrong root", good.replace("gupdate", "update")),
            ("wrong namespace", good.replace(GUPDATE_NAMESPACE, "http://example.com/ns")),
            ("wrong protocol", good.replace("protocol=\"2.0\"", "protocol=\"3.0\"")),
            ("no protocol", good.replace("protocol=\"2.0\"", "")),
            ("doctype", format!("<!DOCTYPE gupdate [<!ENTITY x \"y\">]>{good}")),
            ("cdata", good.replace("<daystart elapsed_seconds=\"3600\"/>", "<![CDATA[x]]>")),
            ("custom entity", good.replace("1.2", "&xxe;")),
            ("unterminated", good.replace("</gupdate>", "")),
            ("mismatched", good.replace("</app>", "</apps>")),
            ("unquoted attribute", good.replace("status=\"ok\" version=\"1.2\"", "status=ok")),
            ("no app", format!("<gupdate xmlns=\"{GUPDATE_NAMESPACE}\" protocol=\"2.0\"/>")),
            ("app status error", good.replacen("status=\"ok\"", "status=\"error-unknownApplication\"", 1)),
            ("updatecheck error", good.replace("size=\"123\" status=\"ok\"", "size=\"123\" status=\"error-hash\"")),
            (
                "two updatechecks",
                good.replace("</app>", &format!("<updatecheck codebase=\"{CODEBASE}\"/></app>")),
            ),
            (
                "app outside the root",
                format!("<gupdate xmlns=\"{GUPDATE_NAMESPACE}\" protocol=\"2.0\"><x><app appid=\"{ID}\"><updatecheck codebase=\"{CODEBASE}\"/></app></x></gupdate>"),
            ),
        ];
        for (what, xml) in cases {
            assert!(
                matches!(
                    parse_update_manifest(&production, xml.as_bytes(), ID),
                    Err(FetchError::Manifest(_) | FetchError::AppIdMismatch)
                ),
                "{what}"
            );
        }
        assert!(matches!(
            parse_update_manifest(&production, &[0xff, 0xfe, 0x00], ID),
            Err(FetchError::Manifest(_))
        ));
    }

    #[test]
    fn download_cap_is_unpack_total_plus_header_cap() {
        assert_eq!(MAX_DOWNLOAD_BYTES, (128 + 16) * 1024 * 1024);
    }

    // ---- loopback server -------------------------------------------------------------------

    /// A one-thread HTTP server on 127.0.0.1. `handler` gets the connection index, the request head
    /// and the stream. The accept loop ends when the server is dropped.
    struct Server {
        addr: SocketAddr,
        stop: Arc<AtomicBool>,
        requests: Arc<Mutex<Vec<String>>>,
        thread: Option<JoinHandle<()>>,
    }

    impl Server {
        fn start(handler: impl Fn(usize, &str, &mut TcpStream) + Send + 'static) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
            let addr = listener.local_addr().unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let thread = {
                let (stop, requests) = (stop.clone(), requests.clone());
                std::thread::spawn(move || {
                    for (index, stream) in listener.incoming().enumerate() {
                        if stop.load(Ordering::SeqCst) {
                            return;
                        }
                        let Ok(mut stream) = stream else { continue };
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                        let mut head = Vec::new();
                        let mut byte = [0u8; 1];
                        while !head.ends_with(b"\r\n\r\n") && head.len() < 16 * 1024 {
                            match stream.read(&mut byte) {
                                Ok(1) => head.push(byte[0]),
                                _ => break,
                            }
                        }
                        let head = String::from_utf8_lossy(&head).into_owned();
                        requests.lock().unwrap().push(head.clone());
                        handler(index, &head, &mut stream);
                    }
                })
            };
            Self { addr, stop, requests, thread: Some(thread) }
        }

        fn endpoint(&self) -> Endpoint {
            Endpoint::loopback(self.addr.port())
        }

        fn requests(&self) -> Vec<String> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            let _ = TcpStream::connect(self.addr);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn respond(stream: &mut TcpStream, status_line: &str, headers: &[&str], body: &[u8]) {
        let mut head = format!("HTTP/1.1 {status_line}\r\nConnection: close\r\n");
        for header in headers {
            head.push_str(header);
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body);
        let _ = stream.flush();
    }

    fn ok_with_length(stream: &mut TcpStream, body: &[u8]) {
        respond(stream, "200 OK", &[&format!("Content-Length: {}", body.len())], body);
    }

    // ---- download behaviour ----------------------------------------------------------------

    /// A URL on the loopback server, as a validated codebase would be.
    fn url_on(server: &Server, path: &str) -> Url {
        Url::parse(&format!("http://{}{path}", server.addr)).unwrap()
    }

    fn manifest_response(stream: &mut TcpStream, xml: &str) {
        respond(
            stream,
            "200 OK",
            &[
                "Content-Type: text/xml; charset=UTF-8",
                &format!("Content-Length: {}", xml.len()),
            ],
            xml.as_bytes(),
        );
    }

    #[test]
    fn cancelled_download_does_not_make_a_network_request() {
        let server = Server::start(|_, _, _| panic!("cancelled request reached server"));
        let cancelled = AtomicBool::new(true);
        assert_eq!(
            fetch_manifest_bytes_cancellable(&server.endpoint(), ID, PRODVERSION, Some(&cancelled)),
            Err(FetchError::Cancelled)
        );
        assert!(server.requests().is_empty());
    }

    #[test]
    fn cancellation_during_response_does_not_deliver_body_to_caller() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let server = Server::start(move |_, _, stream| {
            flag.store(true, Ordering::Release);
            ok_with_length(stream, b"untrusted response bytes");
        });
        let mut delivered = Vec::new();
        assert_eq!(
            transfer_cancellable(&server.endpoint(), &url_on(&server, "/crx"), None,
                1024, Some(&cancelled), &mut |chunk| delivered.extend_from_slice(chunk)),
            Err(FetchError::Cancelled)
        );
        assert!(delivered.is_empty());
        assert_eq!(server.requests().len(), 1);
    }

    #[test]
    fn the_manifest_request_is_an_honest_get_with_the_protocol_headers() {
        let server = Server::start(|_, _, stream| {
            manifest_response(stream, &good_manifest(ID, "http://127.0.0.1:1/never"))
        });
        let bytes = fetch_manifest_bytes(&server.endpoint(), ID, PRODVERSION).unwrap();
        assert!(String::from_utf8(bytes).unwrap().contains("<gupdate"));

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        let head = &requests[0];
        let line = head.lines().next().unwrap();
        assert!(line.starts_with("GET /service/update2/crx?"), "{line}");
        assert!(line.contains("prod=chromiumcrx"), "{line}");
        assert!(line.contains("acceptformat=crx3"), "{line}");
        assert!(line.contains(&format!("x=id%3D{ID}%26v%3D0.0.0.0%26uc")), "{line}");
        assert!(!line.contains("response="), "{line}");
        let lower = head.to_ascii_lowercase();
        assert!(lower.contains(&format!("x-goog-update-updater: chromiumcrx-{PRODVERSION}\r\n")), "{head}");
        assert!(lower.contains(&format!("x-goog-update-appid: {ID}\r\n")), "{head}");
        assert!(lower.contains("x-goog-update-interactivity: fg\r\n"), "{head}");
        let agent = lower.lines().find(|l| l.starts_with("user-agent:")).expect("a user agent");
        assert!(agent.contains("bowser-crxfetch/"), "{agent}");
        assert!(!lower.contains("chrome/") && !agent.contains("chrome"), "{head}");
        assert!(!lower.contains("chromecrx"), "{head}");
        assert!(!lower.contains("cookie:") && !lower.contains("authorization:"), "{head}");
    }

    #[test]
    fn a_manifest_over_the_cap_is_refused() {
        let server = Server::start(|_, _, stream| {
            let body = vec![b' '; MAX_MANIFEST_BYTES as usize + 1];
            ok_with_length(stream, &body);
        });
        assert_eq!(
            fetch_manifest_bytes(&server.endpoint(), ID, PRODVERSION),
            Err(FetchError::Oversize)
        );
    }

    #[test]
    fn the_manifest_then_the_crx_are_both_fetched_from_the_loopback_server() {
        let crx_body: Vec<u8> = (0..200_000u32).map(|i| i as u8).collect();
        let served = crx_body.clone();
        let server = Server::start(move |_, head, stream| {
            if head.starts_with("GET /service/update2/crx?") {
                let codebase = head
                    .lines()
                    .find_map(|l| l.strip_prefix("Host: "))
                    .map(|host| format!("http://{host}/blobs/ext.crx"))
                    .expect("a Host header");
                manifest_response(stream, &good_manifest(ID, &codebase));
            } else {
                assert!(head.starts_with("GET /blobs/ext.crx "), "{head}");
                ok_with_length(stream, &served);
            }
        });
        let got = download_from(&server.endpoint(), ID, PRODVERSION, 1 << 20).unwrap();
        assert_eq!(got, crx_body);

        let requests = server.requests();
        assert_eq!(requests.len(), 2, "one manifest request, one CRX request");
        assert!(requests[0].starts_with("GET /service/update2/crx?"));
        assert!(requests[1].starts_with("GET /blobs/ext.crx "));
        // Every request went to 127.0.0.1; the CRX request carries no update-protocol headers.
        for head in &requests {
            let host = head.lines().find_map(|l| l.strip_prefix("Host: ")).unwrap();
            assert!(host.starts_with("127.0.0.1:"), "{host}");
        }
        assert!(!requests[1].to_ascii_lowercase().contains("x-goog-update"), "{}", requests[1]);
    }

    #[test]
    fn a_codebase_on_another_host_is_refused_without_connecting() {
        let server = Server::start(|_, _, stream| {
            manifest_response(stream, &good_manifest(ID, "http://127.0.0.2:1/x.crx"))
        });
        assert!(matches!(
            download_from(&server.endpoint(), ID, PRODVERSION, 1 << 20),
            Err(FetchError::Manifest(_))
        ));
        assert_eq!(server.requests().len(), 1, "only the manifest was requested");
    }

    #[test]
    fn a_manifest_for_another_id_or_without_an_update_stops_before_any_crx_request() {
        let wrong = Server::start(|_, _, stream| {
            manifest_response(stream, &good_manifest(OTHER_ID, "http://127.0.0.1:1/x.crx"))
        });
        assert_eq!(
            download_from(&wrong.endpoint(), ID, PRODVERSION, 1 << 20),
            Err(FetchError::AppIdMismatch)
        );
        assert_eq!(wrong.requests().len(), 1);
        let none = Server::start(|_, _, stream| respond(stream, "204 No Content", &[], b""));
        assert_eq!(
            download_from(&none.endpoint(), ID, PRODVERSION, 1 << 20),
            Err(FetchError::NoUpdate)
        );
        assert_eq!(none.requests().len(), 1);
        let noupdate = Server::start(|_, _, stream| {
            manifest_response(
                stream,
                &manifest_with(&format!("appid=\"{ID}\" status=\"ok\""), "status=\"noupdate\""),
            )
        });
        assert_eq!(
            download_from(&noupdate.endpoint(), ID, PRODVERSION, 1 << 20),
            Err(FetchError::NoUpdate)
        );
    }

    #[test]
    fn download_refuses_a_declared_length_over_the_cap_without_reading() {
        let server = Server::start(|_, _, stream| {
            respond(stream, "200 OK", &["Content-Length: 10000000"], b"short");
        });
        let result = download_bytes(&server.endpoint(), &url_on(&server, "/crx"), 1000);
        assert_eq!(result, Err(FetchError::Oversize));
    }

    #[test]
    fn download_stops_reading_a_stream_that_outgrows_the_cap() {
        const TOTAL: u64 = 256 * 1024 * 1024;
        let written = Arc::new(AtomicU64::new(0));
        let finished = Arc::new(AtomicBool::new(false));
        let (w, f) = (written.clone(), finished.clone());
        let server = Server::start(move |_, _, stream| {
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
            let chunk = vec![0x5au8; 1 << 20];
            while w.load(Ordering::SeqCst) < TOTAL {
                if stream.write_all(&chunk).is_err() {
                    break;
                }
                w.fetch_add(chunk.len() as u64, Ordering::SeqCst);
            }
            f.store(true, Ordering::SeqCst);
        });
        let mut delivered = 0u64;
        let url = url_on(&server, "/crx");
        let result = transfer(&server.endpoint(), &url, None, 1 << 20, &mut |chunk| {
            delivered += chunk.len() as u64
        });
        assert_eq!(result, Err(FetchError::Oversize));
        assert!(delivered <= 1 << 20, "passed {delivered} bytes past the cap");

        let deadline = Instant::now() + Duration::from_secs(20);
        while !finished.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(finished.load(Ordering::SeqCst), "server never saw the client stop");
        assert!(
            written.load(Ordering::SeqCst) < TOTAL,
            "client drained the whole {TOTAL}-byte stream"
        );
        drop(server);
    }

    #[test]
    fn download_into_memory_never_exceeds_the_cap() {
        let server = Server::start(|_, _, stream| {
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
            let chunk = [7u8; 8192];
            for _ in 0..64 {
                if stream.write_all(&chunk).is_err() {
                    return;
                }
            }
        });
        let url = url_on(&server, "/crx");
        assert_eq!(download_bytes(&server.endpoint(), &url, 100_000), Err(FetchError::Oversize));
        // The same stream under a larger cap arrives whole: the cap is exact, not a blanket refusal.
        let server = Server::start(|_, _, stream| {
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
            let _ = stream.write_all(&[7u8; 8192 * 4]);
        });
        let url = url_on(&server, "/crx");
        let got = download_bytes(&server.endpoint(), &url, 8192 * 4).unwrap();
        assert_eq!(got.len(), 8192 * 4);
        assert_eq!(download_bytes(&server.endpoint(), &url, 8192 * 4 - 1), Err(FetchError::Oversize));
    }

    #[test]
    fn a_refusal_reports_its_status_and_at_most_two_hundred_body_bytes() {
        let server = Server::start(|_, _, stream| {
            let body = vec![b'x'; 5000];
            respond(
                stream,
                "403 Forbidden",
                &[&format!("Content-Length: {}", body.len())],
                &body,
            );
        });
        match fetch_manifest_bytes(&server.endpoint(), ID, PRODVERSION) {
            Err(FetchError::Http { status, body }) => {
                assert_eq!(status, 403);
                assert_eq!(body.len(), 200);
            }
            other => panic!("{other:?}"),
        }
        // 204 is the raw status at this level and "no update" one level up.
        let server = Server::start(|_, _, stream| respond(stream, "204 No Content", &[], b""));
        match fetch_manifest_bytes(&server.endpoint(), ID, PRODVERSION) {
            Err(FetchError::Http { status, body }) => {
                assert_eq!(status, 204);
                assert!(body.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn download_follows_a_redirect_on_the_same_host() {
        let server = Server::start(|index, head, stream| {
            if index == 0 {
                assert!(head.starts_with("GET /crx "), "{head}");
                respond(stream, "302 Found", &["Location: /blob/1"], b"");
            } else {
                assert!(head.starts_with("GET /blob/1 "), "{head}");
                ok_with_length(stream, b"final");
            }
        });
        let url = url_on(&server, "/crx");
        assert_eq!(download_bytes(&server.endpoint(), &url, 1024).unwrap(), b"final");
        assert_eq!(server.requests().len(), 2);
    }

    #[test]
    fn the_manifest_request_itself_follows_a_redirect_on_its_host() {
        let server = Server::start(|index, head, stream| {
            if index == 0 {
                respond(stream, "302 Found", &["Location: /service/update2/crx2"], b"");
            } else {
                assert!(head.starts_with("GET /service/update2/crx2 "), "{head}");
                manifest_response(stream, &good_manifest(ID, "http://127.0.0.1:1/x.crx"));
            }
        });
        assert!(fetch_manifest_bytes(&server.endpoint(), ID, PRODVERSION).is_ok());
    }

    #[test]
    fn download_gives_up_after_five_redirects() {
        let hops = Arc::new(AtomicUsize::new(0));
        let counter = hops.clone();
        let server = Server::start(move |_, _, stream| {
            counter.fetch_add(1, Ordering::SeqCst);
            respond(stream, "302 Found", &["Location: /again"], b"");
        });
        let url = url_on(&server, "/crx");
        assert_eq!(
            download_bytes(&server.endpoint(), &url, 1024),
            Err(FetchError::TooManyRedirects)
        );
        assert_eq!(hops.load(Ordering::SeqCst), 6, "the first request plus five hops");
    }

    #[test]
    fn download_refuses_a_redirect_to_the_real_update_host() {
        // The loopback server tries to send the client to clients2.google.com. The client must refuse
        // before connecting, so this test cannot reach the network.
        for target in [
            "http://clients2.google.com/service/update2/crx",
            "https://clients2.google.com/service/update2/crx",
        ] {
            let location = format!("Location: {target}");
            let server = Server::start(move |_, _, stream| respond(stream, "302 Found", &[&location], b""));
            assert!(matches!(
                fetch_manifest_bytes(&server.endpoint(), ID, PRODVERSION),
                Err(FetchError::BadRedirect(_))
            ));
            assert!(matches!(
                download_bytes(&server.endpoint(), &url_on(&server, "/crx"), 1024),
                Err(FetchError::BadRedirect(_))
            ));
            assert_eq!(server.requests().len(), 2);
        }
    }

    // ---- verify and unpack -----------------------------------------------------------------

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "bowser-crx-fetch-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir(&path).expect("create scratch folder");
            Self(path)
        }

        fn entries(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(&self.0)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o644);
        for (name, contents) in files {
            zip.start_file(*name, options).unwrap();
            zip.write_all(contents.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    const MANIFEST: &str = "{\"manifest_version\":3,\"name\":\"Demo\",\"version\":\"1.0\"}";

    fn good_crx() -> (Vec<u8>, String, Vec<u8>) {
        crate::crx::tests::signed_crx_for_tests(&zip_of(&[
            ("manifest.json", MANIFEST),
            ("background.js", "console.log(1);"),
        ]))
    }

    #[test]
    fn verify_and_unpack_stamps_the_folder_and_returns_the_id() {
        let (crx, id, spki) = good_crx();
        let scratch = Scratch::new();
        let stamped = verify_and_unpack(&crx, &id, &scratch.0).unwrap();
        assert_eq!(stamped.id, id);
        assert_eq!(stamped.summary.files, 2);
        assert_eq!(scratch.entries(), ["background.js", "manifest.json"]);
        let manifest = fs::read_to_string(scratch.0.join("manifest.json")).unwrap();
        assert!(manifest.contains("\"key\":\""), "{manifest}");
        assert_eq!(crate::crx::extension_id_from_public_key(&spki).as_deref(), Some(id.as_str()));
    }

    #[test]
    fn a_crx_for_another_id_writes_nothing() {
        let (crx, id, _) = good_crx();
        assert_ne!(id, OTHER_ID);
        let scratch = Scratch::new();
        let result = verify_and_unpack(&crx, OTHER_ID, &scratch.0);
        assert_eq!(result, Err(FetchError::Verify(CrxError::IdMismatch)));
        assert!(scratch.entries().is_empty());
    }

    #[test]
    fn a_tampered_archive_writes_nothing() {
        let (mut crx, id, _) = good_crx();
        let last = crx.len() - 1;
        crx[last] ^= 0xff;
        let scratch = Scratch::new();
        assert!(matches!(
            verify_and_unpack(&crx, &id, &scratch.0),
            Err(FetchError::Verify(_))
        ));
        assert!(scratch.entries().is_empty());
    }

    #[test]
    fn a_failed_stamp_removes_what_was_unpacked() {
        // A manifest with a comment cannot take a key, so the stamp fails after the files exist.
        let archive = zip_of(&[
            ("manifest.json", "{ // not plain JSON\n\"manifest_version\":3}"),
            ("background.js", "1"),
            ("lib/a.js", "2"),
        ]);
        let (crx, id, _) = crate::crx::tests::signed_crx_for_tests(&archive);
        let scratch = Scratch::new();
        let result = verify_and_unpack(&crx, &id, &scratch.0);
        assert!(matches!(result, Err(FetchError::Unpack(StampError::ManifestKey(_)))), "{result:?}");
        assert!(scratch.entries().is_empty(), "{:?}", scratch.entries());
    }

    #[test]
    fn a_package_without_a_manifest_leaves_the_folder_empty() {
        let (crx, id, _) = crate::crx::tests::signed_crx_for_tests(&zip_of(&[("a.js", "1")]));
        let scratch = Scratch::new();
        assert_eq!(
            verify_and_unpack(&crx, &id, &scratch.0),
            Err(FetchError::Unpack(StampError::ManifestMissing))
        );
        assert!(scratch.entries().is_empty());
    }

    #[test]
    fn a_folder_that_already_holds_something_is_refused_and_untouched() {
        let (crx, id, _) = good_crx();
        let scratch = Scratch::new();
        fs::write(scratch.0.join("keep.txt"), "mine").unwrap();
        let result = verify_and_unpack(&crx, &id, &scratch.0);
        assert!(matches!(result, Err(FetchError::Unpack(StampError::Unpack(_)))), "{result:?}");
        assert_eq!(scratch.entries(), ["keep.txt"]);
        assert_eq!(fs::read_to_string(scratch.0.join("keep.txt")).unwrap(), "mine");
    }

    /// A loopback "store": the update manifest for `manifest_id` on the update path, pointing at
    /// `/blobs/ext.crx` on the same server, which serves `crx`.
    fn store_server(manifest_id: &str, crx: Vec<u8>) -> Server {
        let manifest_id = manifest_id.to_owned();
        Server::start(move |_, head, stream| {
            if head.starts_with("GET /service/update2/crx?") {
                let host = head.lines().find_map(|l| l.strip_prefix("Host: ")).unwrap();
                let codebase = format!("http://{host}/blobs/ext.crx");
                manifest_response(stream, &good_manifest(&manifest_id, &codebase));
            } else {
                ok_with_length(stream, &crx);
            }
        })
    }

    #[test]
    fn fetch_downloads_verifies_and_unpacks_from_a_loopback_server() {
        let (crx, id, _) = good_crx();
        let server = store_server(&id, crx);
        let scratch = Scratch::new();
        let url = format!("https://chromewebstore.google.com/detail/demo/{id}?hl=en");
        let stamped = fetch_from(&server.endpoint(), &url, PRODVERSION, &scratch.0).unwrap();
        assert_eq!(stamped.id, id);
        assert_eq!(scratch.entries(), ["background.js", "manifest.json"]);
        let requests = server.requests();
        assert_eq!(requests.len(), 2, "manifest and CRX; the store URL itself is never fetched");
        assert!(requests[0].contains(&format!("x=id%3D{id}%26v%3D0.0.0.0%26uc")));
        assert!(requests[1].starts_with("GET /blobs/ext.crx "));
    }

    #[test]
    fn fetch_with_a_bad_signature_deletes_the_files_and_errors() {
        let (mut crx, id, _) = good_crx();
        let last = crx.len() - 1;
        crx[last] ^= 0xff;
        let server = store_server(&id, crx);
        let scratch = Scratch::new();
        let result = fetch_from(&server.endpoint(), &id, PRODVERSION, &scratch.0);
        assert!(matches!(result, Err(FetchError::Verify(_))), "{result:?}");
        assert!(scratch.entries().is_empty(), "{:?}", scratch.entries());
    }

    #[test]
    fn fetch_of_a_crx_signed_for_another_id_writes_nothing() {
        let (crx, id, _) = good_crx();
        let server = store_server(&id, crx);
        let scratch = Scratch::new();
        // The manifest and the CRX agree with each other, but not with the id that was asked for.
        let result = fetch_from(&server.endpoint(), OTHER_ID, PRODVERSION, &scratch.0);
        assert_eq!(result, Err(FetchError::AppIdMismatch));
        assert!(scratch.entries().is_empty());
        // A manifest that names the requested id but offers someone else's CRX fails verification.
        let (other_crx, _, _) = good_crx();
        let server = store_server(OTHER_ID, other_crx);
        let result = fetch_from(&server.endpoint(), OTHER_ID, PRODVERSION, &scratch.0);
        assert_eq!(result, Err(FetchError::Verify(CrxError::IdMismatch)));
        assert!(scratch.entries().is_empty());
    }

    #[test]
    fn fetch_of_a_body_that_is_not_a_crx_writes_nothing() {
        let server = store_server(ID, b"<html>not a crx</html>".to_vec());
        let scratch = Scratch::new();
        let result = fetch_from(&server.endpoint(), ID, PRODVERSION, &scratch.0);
        assert_eq!(result, Err(FetchError::Verify(CrxError::BadMagic)));
        assert!(scratch.entries().is_empty());
    }

    #[test]
    fn fetch_of_a_bad_input_never_touches_the_network() {
        let server = Server::start(|_, _, stream| ok_with_length(stream, b""));
        let scratch = Scratch::new();
        for input in ["", "UBLOCK", "https://example.com/detail/x/cjpalhdlnbpafiamejdnhcphjbkeiagm"] {
            assert!(fetch_from(&server.endpoint(), input, PRODVERSION, &scratch.0).is_err());
        }
        assert!(server.requests().is_empty());
    }

    /// Explicit network diagnostic, not a compatibility verdict. Uses the installed runtime, never
    /// a Chrome version or user-agent. Each selected current listing is requested exactly once.
    #[test]
    #[ignore = "live network and installed WebView2 runtime; run explicitly on a disposable runner"]
    fn live_current_mv3_store_downloads() {
        use webview2_com::{CoTaskMemPWSTR, Microsoft::Web::WebView2::Win32::GetAvailableCoreWebView2BrowserVersionString};
        use windows::core::PWSTR;
        let mut raw = PWSTR::null();
        unsafe { GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut raw) }
            .expect("installed WebView2 runtime required for an accurate protocol version");
        let runtime = CoTaskMemPWSTR::from(raw).to_string();
        let version = runtime.split_whitespace().next().expect("runtime version");
        assert!(valid_prodversion(version), "unusable detected runtime version");
        println!("STORE_PROBE runtime={version} product={PROD} user_agent=Bowser-CrxFetch/{}", env!("CARGO_PKG_VERSION"));
        let endpoint = Endpoint::production();
        for (name, id) in [
            ("ublock-origin-lite", "ddkjiahejlhfcafbddmgiahcphecmpfh"),
            ("bitwarden", "nngceckbapebfimnlniiiahkandclblb"),
            ("dark-reader", "eimadpbcbfnmbkopoojfekhnkhdbieeh"),
        ] {
            let manifest = match fetch_manifest_bytes(&endpoint, id, version) {
                Ok(bytes) => bytes,
                Err(error) => {
                    println!("STORE_PROBE name={name} id={id} stage=manifest result={error}");
                    continue;
                }
            };
            let offer = match parse_update_manifest(&endpoint, &manifest, id) {
                Ok(offer) => offer,
                Err(error) => {
                    println!("STORE_PROBE name={name} id={id} http_status=200 stage=updatecheck result={error}");
                    continue;
                }
            };
            println!("STORE_PROBE name={name} id={id} http_status=200 updatecheck=ok");
            let bytes = match download_bytes(&endpoint, &offer.codebase, MAX_DOWNLOAD_BYTES) {
                Ok(bytes) => bytes,
                Err(error) => {
                    println!("STORE_PROBE name={name} id={id} stage=download result={error}");
                    continue;
                }
            };
            let scratch = Scratch::new();
            match verify_and_unpack(&bytes, id, &scratch.0) {
                Ok(stamped) => match crate::extensions::inspect_source(&scratch.0) {
                    Ok(inspection) => println!("STORE_PROBE name={name} id={} stage=verified version={} manifest_version={} files={} bytes={} loaded=false", stamped.id, inspection.manifest.version, inspection.manifest.manifest_version, inspection.files, inspection.bytes),
                    Err(error) => println!("STORE_PROBE name={name} id={id} stage=inspection result={error}"),
                },
                Err(error) => println!("STORE_PROBE name={name} id={id} stage=verification result={error}"),
            }
        }
    }

    // ---- live attempt (run by hand, once) --------------------------------------------------

    /// One real attempt for uBlock Origin: the update manifest, then (if it offers one) the CRX, into a
    /// temp folder that is deleted. Network, so it is ignored: run it by hand with
    /// `CRX_FETCH_LIVE_PRODVERSION=<runtime version>`. It never loads the extension. Do not loop it.
    #[test]
    #[ignore = "live network: one real manifest request to clients2.google.com"]
    fn live_ublock_origin_attempt() {
        let prodversion = std::env::var("CRX_FETCH_LIVE_PRODVERSION").expect("set CRX_FETCH_LIVE_PRODVERSION");
        let id = "cjpalhdlnbpafiamejdnhcphjbkeiagm";
        let endpoint = Endpoint::production();
        let manifest = match fetch_manifest_bytes(&endpoint, id, &prodversion) {
            Ok(bytes) => bytes,
            Err(FetchError::Http { status, body }) => {
                eprintln!("LIVE: manifest request answered HTTP status {status}, {} body bytes", body.len());
                if !body.is_empty() {
                    eprintln!("LIVE: body: {:?}", String::from_utf8_lossy(&body));
                }
                return;
            }
            Err(other) => {
                eprintln!("LIVE: manifest request failed before a verdict: {other}");
                return;
            }
        };
        eprintln!("LIVE: manifest HTTP status 200, {} bytes", manifest.len());
        let offer = match parse_update_manifest(&endpoint, &manifest, id) {
            Ok(offer) => offer,
            Err(error) => {
                let head = &manifest[..manifest.len().min(400)];
                eprintln!("LIVE: manifest says: {error}");
                eprintln!("LIVE: manifest head: {:?}", String::from_utf8_lossy(head));
                return;
            }
        };
        eprintln!("LIVE: manifest offers a codebase on host {:?}", offer.codebase.host_str());
        let bytes = match download_bytes(&endpoint, &offer.codebase, MAX_DOWNLOAD_BYTES) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("LIVE: codebase download failed: {error}");
                return;
            }
        };
        eprintln!("LIVE: codebase HTTP status 200, {} bytes", bytes.len());
        let scratch = Scratch::new();
        let dest = scratch.0.join("unpacked");
        fs::create_dir(&dest).unwrap();
        match verify_and_unpack(&bytes, id, &dest) {
            Ok(stamped) => {
                let manifest = fs::read_to_string(dest.join("manifest.json")).unwrap_or_default();
                let name = manifest
                    .split("\"name\"")
                    .nth(1)
                    .and_then(|rest| rest.split('"').nth(1))
                    .unwrap_or("?")
                    .to_owned();
                eprintln!("LIVE: verified and unpacked: id {}, manifest name {name:?}, files {}", stamped.id, stamped.summary.files);
            }
            Err(error) => eprintln!("LIVE: downloaded but did not verify/unpack: {error}"),
        }
    }
}
