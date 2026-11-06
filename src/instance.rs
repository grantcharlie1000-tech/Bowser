//! One running window per profile for external links.
//!
//! Windows starts a new process for a link or a file (`"<exe>" "%1"`). When this profile already
//! has a window, that process hands the address over and exits. It does not open another window
//! and it does not read or write the session. An explicit `--new-window` launch is a separate
//! choice and is not handled here.
//!
//! The address travels in `WM_COPYDATA`. The target window is the top-level `BowserNativeWindow`
//! that claimed this profile's directory. The sender, which the shell just started, brings that
//! window forward.

use std::{path::Path, thread, time::Duration};

use url::Url;
use windows::{
    Win32::{
        Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM},
        System::DataExchange::COPYDATASTRUCT,
        UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetForegroundWindow, GetPropW, IsIconic, RemovePropW,
            SMTO_ABORTIFHUNG, SMTO_BLOCK, SW_RESTORE, SW_SHOW, SendMessageTimeoutW,
            SetForegroundWindow, SetPropW, ShowWindow, WM_COPYDATA,
        },
    },
    core::{BOOL, w},
};

use crate::performance::MAX_OPEN_URLS;

/// `dwData` for a Bowser link handoff. Anything else is ignored.
pub const FORWARD_MAGIC: usize = 0x4277_4F70;
const PROFILE_PROP: windows::core::PCWSTR = w!("BowserProfileToken");
const WINDOW_CLASS: &str = "BowserNativeWindow";
/// How long a handoff waits for the window. The marker is taken just before the window exists, so
/// a link that arrives during startup has to wait out that gap. A window that is already up is
/// found on the first try.
pub const DELIVER_ATTEMPTS: u32 = 50;
pub const DELIVER_WAIT_MS: u64 = 100;
const SEND_TIMEOUT_MS: u32 = 2_000;
const MAX_FORWARD_BYTES: usize = 96 * 1024;
const HEADER: &str = "bowser-open 1\n";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliverError {
    /// The addresses are not a handoff this window will open.
    Rejected,
    /// No living window of this profile accepted them.
    Unavailable,
}

/// Stable identity of a profile directory. Case and slash differences name the same profile.
pub fn profile_token(dir: &Path) -> u64 {
    let path = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut key = path.to_string_lossy().replace('/', "\\");
    if let Some(rest) = key.strip_prefix(r"\\?\") {
        key = rest.to_owned();
    }
    key.make_ascii_lowercase();
    while key.ends_with('\\') {
        key.pop();
    }
    let mut hash = 0xcbf29ce484222325u64;
    for byte in key.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    if hash == 0 { 1 } else { hash }
}

/// An address a handoff may open: the same http(s) and file rule as a launch argument, with no
/// credentials and no line breaks that would split the message.
pub fn acceptable_launch_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "file")
        && url.username().is_empty()
        && url.password().is_none()
        && !url
            .as_str()
            .bytes()
            .any(|byte| matches!(byte, b'\n' | b'\r' | 0))
}

/// The bytes `WM_COPYDATA` carries. `None` when nothing in `urls` may be opened.
pub fn encode_urls(urls: &[Url]) -> Option<Vec<u8>> {
    let mut body = String::from(HEADER);
    let mut kept = 0usize;
    for url in urls {
        if !acceptable_launch_url(url) {
            continue;
        }
        if kept == MAX_OPEN_URLS {
            break;
        }
        body.push_str(url.as_str());
        body.push('\n');
        kept += 1;
    }
    if kept == 0 || body.len() > MAX_FORWARD_BYTES {
        return None;
    }
    Some(body.into_bytes())
}

/// Addresses from a handoff payload. `None` when the bytes are not a handoff (a session file, a
/// truncated message, a foreign scheme). At most [`MAX_OPEN_URLS`] addresses are returned.
pub fn decode_urls(bytes: &[u8]) -> Option<Vec<Url>> {
    if bytes.is_empty() || bytes.len() > MAX_FORWARD_BYTES || bytes.contains(&0) {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.split_inclusive('\n');
    if lines.next()? != HEADER {
        return None;
    }
    let mut urls = Vec::new();
    for line in lines {
        let line = line.strip_suffix('\n')?;
        if line.is_empty() {
            return None;
        }
        let url = Url::parse(line).ok()?;
        if !acceptable_launch_url(&url) {
            return None;
        }
        if urls.len() == MAX_OPEN_URLS {
            break;
        }
        urls.push(url);
    }
    if urls.is_empty() { None } else { Some(urls) }
}

/// The window that should open a handoff: the foreground one when it belongs to this profile,
/// otherwise the first.
pub fn choose_target(windows: &[isize], foreground: Option<isize>) -> Option<isize> {
    foreground
        .filter(|foreground| windows.contains(foreground))
        .or_else(|| windows.first().copied())
}

/// Tries `find` until one of its windows accepts `send`, or the attempts run out. A failed send
/// tries the next window of that attempt, then waits. A successful send happens once.
pub fn deliver_with(
    attempts: u32,
    mut find: impl FnMut() -> Vec<isize>,
    mut foreground: impl FnMut() -> Option<isize>,
    mut send: impl FnMut(isize) -> bool,
    mut wait: impl FnMut(),
) -> Result<isize, DeliverError> {
    let attempts = attempts.max(1);
    for attempt in 0..attempts {
        let windows = find();
        let mut ordered = Vec::with_capacity(windows.len());
        if let Some(foreground) = choose_target(&windows, foreground()) {
            ordered.push(foreground);
        }
        for window in windows {
            if !ordered.contains(&window) {
                ordered.push(window);
            }
        }
        for window in ordered {
            if send(window) {
                return Ok(window);
            }
        }
        if attempt + 1 != attempts {
            wait();
        }
    }
    Err(DeliverError::Unavailable)
}

/// Records `hwnd` as a window of the profile at `dir`.
pub fn claim_window(hwnd: HWND, dir: &Path) {
    let token = profile_token(dir);
    unsafe {
        let _ = SetPropW(hwnd, PROFILE_PROP, Some(handle_of(token)));
    }
}

pub fn release_window(hwnd: HWND) {
    unsafe {
        let _ = RemovePropW(hwnd, PROFILE_PROP);
    }
}

/// Whether any top-level Bowser window has claimed `dir`.
pub fn profile_is_open(dir: &Path) -> bool {
    !find_windows(profile_token(dir)).is_empty()
}

/// Hands `urls` to a living window of `dir` and asks that window to the foreground.
pub fn deliver(dir: &Path, urls: &[Url]) -> Result<(), DeliverError> {
    let bytes = encode_urls(urls).ok_or(DeliverError::Rejected)?;
    let token = profile_token(dir);
    let hwnd = deliver_with(
        DELIVER_ATTEMPTS,
        || find_windows(token),
        || Some(unsafe { GetForegroundWindow() }.0 as isize),
        |hwnd| send_copydata(hwnd, &bytes),
        || thread::sleep(Duration::from_millis(DELIVER_WAIT_MS)),
    )?;
    focus(HWND(hwnd as *mut std::ffi::c_void));
    Ok(())
}

/// URLs from a `WM_COPYDATA` lparam, when it is a Bowser handoff.
///
/// # Safety
/// `lparam` must be the `lParam` of a `WM_COPYDATA` the system delivered to this window.
pub unsafe fn urls_from_copydata(lparam: isize) -> Option<Vec<Url>> {
    if lparam == 0 {
        return None;
    }
    let data = unsafe { &*(lparam as *const COPYDATASTRUCT) };
    if data.dwData != FORWARD_MAGIC || data.lpData.is_null() || data.cbData == 0 {
        return None;
    }
    let len = data.cbData as usize;
    if len > MAX_FORWARD_BYTES {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(data.lpData as *const u8, len) };
    decode_urls(bytes)
}

fn handle_of(token: u64) -> HANDLE {
    HANDLE(token as usize as *mut std::ffi::c_void)
}

fn token_of(handle: HANDLE) -> Option<u64> {
    let raw = handle.0 as usize as u64;
    (raw != 0).then_some(raw)
}

fn is_bowser_window(hwnd: HWND) -> bool {
    let mut name = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut name) };
    if n <= 0 {
        return false;
    }
    String::from_utf16_lossy(&name[..n as usize]) == WINDOW_CLASS
}

fn window_token(hwnd: HWND) -> Option<u64> {
    if !is_bowser_window(hwnd) {
        return None;
    }
    token_of(unsafe { GetPropW(hwnd, PROFILE_PROP) })
}

struct FindState {
    token: u64,
    windows: Vec<isize>,
}

unsafe extern "system" fn enum_profile(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = unsafe { &mut *(lparam.0 as *mut FindState) };
    if window_token(hwnd) == Some(state.token) {
        state.windows.push(hwnd.0 as isize);
    }
    BOOL(1)
}

fn find_windows(token: u64) -> Vec<isize> {
    let mut state = FindState {
        token,
        windows: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_profile),
            LPARAM(&mut state as *mut FindState as isize),
        );
    }
    state.windows
}

fn send_copydata(hwnd: isize, bytes: &[u8]) -> bool {
    if hwnd == 0 || bytes.is_empty() || bytes.len() > u32::MAX as usize {
        return false;
    }
    let mut copy = COPYDATASTRUCT {
        dwData: FORWARD_MAGIC,
        cbData: bytes.len() as u32,
        lpData: bytes.as_ptr() as *mut std::ffi::c_void,
    };
    let mut result = 0usize;
    unsafe {
        let sent = SendMessageTimeoutW(
            HWND(hwnd as *mut std::ffi::c_void),
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&mut copy as *mut COPYDATASTRUCT as isize),
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            SEND_TIMEOUT_MS,
            Some(&mut result),
        );
        sent != LRESULT(0) && result == 1
    }
}

fn focus(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn profile_tokens_ignore_case_and_slashes_and_differ_by_directory() {
        assert_eq!(
            profile_token(Path::new(r"C:\Users\A\Bowser")),
            profile_token(Path::new("c:/users/a/bowser"))
        );
        assert_eq!(
            profile_token(Path::new(r"C:\Users\A\Bowser\")),
            profile_token(Path::new(r"C:\Users\A\Bowser"))
        );
        assert_ne!(
            profile_token(Path::new(r"C:\Users\A\Bowser")),
            profile_token(Path::new(r"C:\Users\A\Bowser-Dev"))
        );
    }

    #[test]
    fn a_handoff_round_trips_http_and_file_addresses_and_rejects_a_session() {
        let urls = vec![
            url("https://example.com/path?q=1#top"),
            url("http://127.0.0.1:8080/"),
            url("file:///C:/Users/me/page.html"),
        ];
        let bytes = encode_urls(&urls).unwrap();
        assert_eq!(decode_urls(&bytes).unwrap(), urls);
        assert!(decode_urls(b"bowser-session 2\nactive 0\ntab\t0\thttps://a.test/\tA\n").is_none());
        assert!(decode_urls(b"bowser-open 1\n").is_none());
        assert!(decode_urls(b"bowser-open 1\njavascript:alert(1)\n").is_none());
        assert!(decode_urls(b"bowser-open 1\nhttps://user:secret@example.com/\n").is_none());
        assert!(encode_urls(&[url("javascript:alert(1)")]).is_none());
    }

    #[test]
    fn a_handoff_keeps_at_most_the_launch_limit() {
        let urls: Vec<Url> = (0..MAX_OPEN_URLS + 4)
            .map(|n| url(&format!("https://t{n}.example/")))
            .collect();
        let decoded = decode_urls(&encode_urls(&urls).unwrap()).unwrap();
        assert_eq!(decoded.len(), MAX_OPEN_URLS);
        assert_eq!(decoded[0].as_str(), "https://t0.example/");
        assert_eq!(
            decoded.last().unwrap().as_str(),
            &format!("https://t{}.example/", MAX_OPEN_URLS - 1)
        );
    }

    #[test]
    fn the_foreground_window_is_chosen_when_it_belongs_to_the_profile() {
        assert_eq!(choose_target(&[2, 5, 9], Some(5)), Some(5));
        assert_eq!(choose_target(&[2, 5, 9], Some(7)), Some(2));
        assert_eq!(choose_target(&[], Some(7)), None);
    }

    #[test]
    fn delivery_waits_for_the_window_and_sends_once() {
        let mut seen = 0u32;
        let mut sends = 0u32;
        let hwnd = deliver_with(
            5,
            || {
                seen += 1;
                if seen < 3 { Vec::new() } else { vec![4, 8] }
            },
            || Some(8),
            |hwnd| {
                sends += 1;
                assert_eq!(
                    hwnd, 8,
                    "the foreground window is the one that receives the link"
                );
                true
            },
            || {},
        )
        .unwrap();
        assert_eq!(hwnd, 8);
        assert_eq!(sends, 1);
        assert_eq!(seen, 3);
    }

    #[test]
    fn a_failed_send_tries_another_window_and_does_not_report_success_twice() {
        let mut sends = Vec::new();
        let hwnd = deliver_with(
            3,
            || vec![1, 2],
            || Some(1),
            |hwnd| {
                sends.push(hwnd);
                hwnd == 2
            },
            || {},
        )
        .unwrap();
        assert_eq!(hwnd, 2);
        assert_eq!(sends, [1, 2]);
    }

    #[test]
    fn delivery_fails_when_no_window_appears() {
        let mut waits = 0u32;
        let error = deliver_with(3, Vec::new, || None, |_| false, || waits += 1).unwrap_err();
        assert_eq!(error, DeliverError::Unavailable);
        assert_eq!(waits, 2, "the last attempt does not wait");
    }
}
