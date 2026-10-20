//! Thin native windows over the HTML chrome and the page along the window's resize edges.
//!
//! The HTML hosts and the page are child windows of another process, and a child window wins every
//! hit test over its own pixels: where a child reaches into a resize band (the tab row of `TabsTop`
//! or the toolbar of `ToolbarTop` at the top, the rail on the left, the page on the right and at the
//! bottom) the frame's `WM_NCHITTEST` is never asked, so only the sliver outside the child could be
//! grabbed there. Each grip here sits above the children on exactly the part of a resize band they
//! cover (`frame::edge_grip_rects`). `HTTRANSPARENT` would not do: the search then goes on to the
//! child underneath, not to the frame. So a grip asks the frame for its hit code (one of the eight
//! resize codes), shows that resize cursor, and hands a press to the frame as the matching
//! non-client press, which runs the system's own sizing loop. No grip exists while the window is
//! maximised or full screen.

use std::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, ClientToScreen, EndPaint, PAINTSTRUCT, ScreenToClient};
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CreateWindowExW, DefWindowProcW, GetClientRect, GetCursorPos, GetParent, HTBOTTOM, HTBOTTOMLEFT,
    HTBOTTOMRIGHT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, HWND_TOP, IDC_ARROW, IDC_SIZENESW, IDC_SIZENS,
    IDC_SIZENWSE, IDC_SIZEWE, LoadCursorW, MA_NOACTIVATE, RegisterClassW, SW_HIDE,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SendMessageW, SetCursor, SetWindowPos, ShowWindow, WM_ERASEBKGND, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_PAINT, WM_SETCURSOR,
    WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS,
};
use windows::core::{PCWSTR, w};

const GRIP_CLASS: PCWSTR = w!("BowserFrameGrip");

thread_local! {
    /// The grip windows of each top-level window, made when first needed and kept.
    static GRIPS: RefCell<HashMap<isize, Vec<HWND>>> = RefCell::new(HashMap::new());
}

/// Puts one grip over each part of a resize band that `hosts` cover, above every other child, and
/// hides the rest. `thin_bottom` is the layout's thin bottom grip (a row along the bottom edge).
pub(super) fn sync(hwnd: HWND, hosts: &[RECT], thin_bottom: bool) {
    let mut client = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut client) }.is_err() {
        return;
    }
    let maximised = super::auto_hide::zoomed_or_full(hwnd);
    let rects = crate::win32::frame::edge_grip_rects((client.right, client.bottom), super::dpi(hwnd), maximised, thin_bottom, hosts);
    let have = GRIPS.with(|g| g.borrow().get(&(hwnd.0 as isize)).cloned().unwrap_or_default());
    if rects.is_empty() && have.is_empty() {
        return;
    }
    let mut windows = have;
    while windows.len() < rects.len() {
        let Some(grip) = create(hwnd) else { break };
        windows.push(grip);
    }
    for (index, window) in windows.iter().enumerate() {
        unsafe {
            match rects.get(index) {
                Some(r) => {
                    let _ = SetWindowPos(
                        *window,
                        Some(HWND_TOP),
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                }
                None => {
                    let _ = ShowWindow(*window, SW_HIDE);
                }
            }
        }
    }
    GRIPS.with(|g| g.borrow_mut().insert(hwnd.0 as isize, windows));
}

/// Brings the shown grips back above the child windows, after a page window was created on top.
pub(super) fn raise(hwnd: HWND) {
    let windows = GRIPS.with(|g| g.borrow().get(&(hwnd.0 as isize)).cloned().unwrap_or_default());
    for window in windows {
        unsafe {
            if windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(window).as_bool() {
                let _ = SetWindowPos(
                    window,
                    Some(HWND_TOP),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE,
                );
            }
        }
    }
}

/// Forgets the grips of a window being destroyed (Windows destroys the child windows themselves).
pub(super) fn forget(hwnd: HWND) {
    GRIPS.with(|g| g.borrow_mut().remove(&(hwnd.0 as isize)));
}

fn create(owner: HWND) -> Option<HWND> {
    if !register_class() {
        return None;
    }
    let instance = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }.ok().map(|m| m.into());
    unsafe {
        CreateWindowExW(Default::default(), GRIP_CLASS, w!(""), WS_CHILD | WS_CLIPSIBLINGS, 0, 0, 1, 1, Some(owner), None, instance, None)
    }
    .ok()
}

fn register_class() -> bool {
    thread_local! { static DONE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) }; }
    if let Some(done) = DONE.with(|d| d.get()) {
        return done;
    }
    let instance = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }.map(|m| m.into()).unwrap_or_default();
    let class = WNDCLASSW { style: CS_DBLCLKS, lpfnWndProc: Some(grip_proc), hInstance: instance, lpszClassName: GRIP_CLASS, ..Default::default() };
    let ok = unsafe { RegisterClassW(&class) } != 0;
    DONE.with(|d| d.set(Some(ok)));
    ok
}

/// The frame's hit code at a grip's client point `lp`, as `WM_NCHITTEST` answers it for the screen
/// point, with the screen point packed for a non-client message.
unsafe fn frame_hit(hwnd: HWND, lp: LPARAM) -> Option<(HWND, u32, LPARAM)> {
    unsafe {
        let parent = GetParent(hwnd).ok()?;
        let mut p = POINT { x: (lp.0 as u16 as i16) as i32, y: ((lp.0 >> 16) as u16 as i16) as i32 };
        let _ = ClientToScreen(hwnd, &mut p);
        let screen = LPARAM((((p.y as u16 as u32) << 16) | (p.x as u16 as u32)) as isize);
        let code = SendMessageW(parent, WM_NCHITTEST, None, Some(screen)).0 as u32;
        Some((parent, code, screen))
    }
}

fn is_resize(code: u32) -> bool {
    matches!(code, HTTOP | HTTOPLEFT | HTTOPRIGHT | HTLEFT | HTRIGHT | HTBOTTOM | HTBOTTOMLEFT | HTBOTTOMRIGHT)
}

/// The resize cursor for a frame hit code; the arrow for anything else.
fn cursor_for(code: Option<u32>) -> PCWSTR {
    match code {
        Some(HTTOPLEFT | HTBOTTOMRIGHT) => IDC_SIZENWSE,
        Some(HTTOPRIGHT | HTBOTTOMLEFT) => IDC_SIZENESW,
        Some(HTTOP | HTBOTTOM) => IDC_SIZENS,
        Some(HTLEFT | HTRIGHT) => IDC_SIZEWE,
        _ => IDC_ARROW,
    }
}

/// A grip: takes the frame's resize hit for its pixels and paints nothing.
unsafe extern "system" fn grip_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_SETCURSOR => {
                let mut cursor = POINT::default();
                let _ = GetCursorPos(&mut cursor);
                let _ = ScreenToClient(hwnd, &mut cursor);
                let local = LPARAM((((cursor.y as u16 as u32) << 16) | (cursor.x as u16 as u32)) as isize);
                let shape = cursor_for(frame_hit(hwnd, local).map(|(_, code, _)| code));
                if let Ok(cursor) = LoadCursorW(None, shape) {
                    SetCursor(Some(cursor));
                }
                LRESULT(1)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                // The frame sizes (or, double-clicked, fills the height) as from its own edge.
                if let Some((parent, code, screen)) = frame_hit(hwnd, lp)
                    && is_resize(code)
                {
                    let message = if msg == WM_LBUTTONDOWN { WM_NCLBUTTONDOWN } else { WM_NCLBUTTONDBLCLK };
                    let _ = ReleaseCapture();
                    let _ = SendMessageW(parent, message, Some(WPARAM(code as usize)), Some(screen));
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let _ = BeginPaint(hwnd, &mut ps);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{HTCAPTION, HTCLIENT, HTCLOSE, HTMAXBUTTON, HTMINBUTTON};

    #[test]
    fn only_the_resize_codes_are_handed_to_the_frame() {
        for code in [HTTOP, HTTOPLEFT, HTTOPRIGHT, HTLEFT, HTRIGHT, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT] {
            assert!(is_resize(code), "{code}");
            assert_ne!(cursor_for(Some(code)), IDC_ARROW, "{code}");
        }
        for code in [HTCLIENT, HTCAPTION, HTCLOSE, HTMAXBUTTON, HTMINBUTTON] {
            assert!(!is_resize(code), "{code}");
            assert_eq!(cursor_for(Some(code)), IDC_ARROW, "{code}");
        }
        assert_eq!(cursor_for(Some(HTLEFT)), IDC_SIZEWE);
        assert_eq!(cursor_for(Some(HTBOTTOMRIGHT)), IDC_SIZENWSE);
        assert_eq!(cursor_for(Some(HTBOTTOMLEFT)), IDC_SIZENESW);
    }
}
