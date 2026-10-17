//! Security surface for page full screen: an owned native window, never page HTML or script.
//! It remains visible until page full screen ends. Identity comes only from the committed top-level
//! WebView2 Source and uses ASCII origin serialization (no credentials, path, page title or Unicode host tricks).

use std::{cell::RefCell, collections::HashMap};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, GetWindowRect, HWND_TOP, SendMessageW, SetWindowPos,
                SWP_NOACTIVATE, SWP_SHOWWINDOW, WINDOW_STYLE, WM_SETFONT, WS_BORDER,
                WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
    core::{HSTRING, w},
};
use crate::{app::{App, AppRef}, tabs::TabId};

thread_local! {
    static NOTICES: RefCell<HashMap<isize, (HWND, TabId)>> = RefCell::new(HashMap::new());
}

/// Opaque or oversized identities cannot be represented safely by this small browser-owned
/// surface. In that case the caller keeps ordinary browser chrome instead of full-monitor mode.
fn notice_text(url: &url::Url) -> Option<String> {
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    let origin = url.origin().ascii_serialization();
    if !origin.is_ascii() || origin.len() > 300 {
        return None;
    }
    Some(format!("Bowser full screen\nTop-level page at {origin}\nPress Esc to exit full screen"))
}

/// Displays a persistent owned popup above WebView2 without stealing the page's keyboard.
/// Returning false is fail-closed: callers must restore ordinary visible browser chrome.
pub(super) fn show(owner: HWND, app: &AppRef, source: Option<&url::Url>) -> bool {
    hide(owner);
    let tab = App::active_tab_id(app);
    let Some(text) = source.and_then(notice_text) else { return false };
    unsafe {
        let mut bounds = RECT::default();
        if GetWindowRect(owner, &mut bounds).is_err() { return false; }
        let scale = GetDpiForWindow(owner).max(96) as i32;
        let dip = |n: i32| n * scale / 96;
        let width = dip(800).min((bounds.right - bounds.left - dip(32)).max(0));
        if width < dip(240) { return false; }
        // Explicitly break long ASCII hosts, rather than let STATIC clip a security-critical
        // suffix. Twenty DIP per ASCII character is deliberately wider than the stock GUI font.
        let columns = ((width - dip(32)) / dip(20)).max(1) as usize;
        let lines: Vec<&str> = text.lines().flat_map(|line| {
            line.as_bytes().chunks(columns).map(|bytes| std::str::from_utf8(bytes).expect("ASCII notice"))
        }).collect();
        let height = dip(24 * lines.len() as i32 + 16);
        if bounds.bottom - bounds.top < height + dip(32) { return false; }
        let text = lines.join("\n");
        let Ok(instance) = GetModuleHandleW(None) else { return false };
        // STATIC's SS_CENTER is 0x0001. A plain native control deliberately avoids a page-host
        // startup dependency: even a failed HTML chrome cannot suppress this security notice.
        let window = CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            w!("STATIC"),
            &HSTRING::from(text),
            WS_POPUP | WS_BORDER | WINDOW_STYLE(0x0001),
            bounds.left + (bounds.right - bounds.left - width) / 2,
            bounds.top + dip(16), width, height,
            Some(owner), None, Some(instance.into()), None,
        );
        let Ok(window) = window else { return false };
        let font = GetStockObject(DEFAULT_GUI_FONT);
        let _ = SendMessageW(window, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
        if SetWindowPos(window, Some(HWND_TOP), 0, 0, 0, 0,
            SWP_NOACTIVATE | SWP_SHOWWINDOW
                | windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE).is_err() {
            let _ = DestroyWindow(window);
            return false;
        }
        NOTICES.with(|notices| notices.borrow_mut().insert(owner.0 as isize, (window, tab)));
        true
    }
}

pub(super) fn hide(owner: HWND) {
    let window = NOTICES.with(|notices| notices.borrow_mut().remove(&(owner.0 as isize)));
    if let Some((window, _)) = window { unsafe { let _ = DestroyWindow(window); } }
}

/// Tab switching must never leave a previous site's identity over a different page.
pub(super) fn belongs_to_active_tab(owner: HWND, app: &AppRef) -> bool {
    let active = App::active_tab_id(app);
    NOTICES.with(|notices| notices.borrow().get(&(owner.0 as isize))
        .is_none_or(|(_, tab)| *tab == active))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_uses_only_ascii_origin_and_always_names_browser_and_exit() {
        let url = url::Url::parse("https://user:secret@bücher.example:8443/path?fake=trusted#title").unwrap();
        let text = notice_text(&url).unwrap();
        assert_eq!(text, "Bowser full screen\nTop-level page at https://xn--bcher-kva.example:8443\nPress Esc to exit full screen");
        for secret in ["user", "secret", "/path", "fake", "title", "bücher"] {
            assert!(!text.contains(secret));
        }
    }

    #[test]
    fn opaque_local_and_oversized_origins_fail_closed() {
        for text in ["about:blank", "data:text/html,hello", "file:///C:/local.html"] {
            assert!(notice_text(&url::Url::parse(text).unwrap()).is_none());
        }
        let long = format!("https://{}.example/", "a".repeat(301));
        if let Ok(url) = url::Url::parse(&long) {
            assert!(notice_text(&url).is_none());
        } // A parser rejecting the oversized host is already fail-closed.
    }

    #[test]
    fn default_ports_and_ipv6_have_unambiguous_identity() {
        assert!(notice_text(&url::Url::parse("https://example.com:443/").unwrap()).unwrap()
            .contains("Top-level page at https://example.com\n"));
        assert!(notice_text(&url::Url::parse("http://[2001:db8::1]:8080/").unwrap()).unwrap()
            .contains("Top-level page at http://[2001:db8::1]:8080\n"));
    }
}
