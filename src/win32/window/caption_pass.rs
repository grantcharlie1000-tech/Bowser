//! Lets the top-level window answer for the caption buttons when a child covers them.
//!
//! Child windows win hit-testing over their own pixels. In a right-hand tab sidebar, and whenever
//! auto-hide floats the page under the top strip, the minimise, maximise and close buttons are
//! painted on the click-through overlay but the click lands in the page or the sidebar. Returning
//! `HTTRANSPARENT` from those children continues the search until the frame's `WM_NCHITTEST` runs,
//! which is what Windows 11 Snap Layouts require for the maximise button. Resize grips and the
//! auto-hide sensors are left alone: they already forward the edge, and a transparent hit there
//! would fall through to the page instead of the frame.

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetPropW, GetWindow, GW_CHILD, GW_HWNDNEXT, HTTRANSPARENT, RemovePropW, SetPropW, WM_NCDESTROY,
    WM_NCHITTEST,
};
use windows::core::{PCWSTR, w};

const SUBCLASS: usize = 0xC4;
const MARK: PCWSTR = w!("BowserCaptionHit");

pub(super) fn sync(parent: HWND) {
    if parent.is_invalid() {
        return;
    }
    let mut child = unsafe { GetWindow(parent, GW_CHILD) }.ok();
    while let Some(window) = child {
        walk(window, parent);
        child = unsafe { GetWindow(window, GW_HWNDNEXT) }.ok();
    }
}

fn walk(window: HWND, parent: HWND) {
    consider(window, parent);
    let mut child = unsafe { GetWindow(window, GW_CHILD) }.ok();
    while let Some(next) = child {
        walk(next, parent);
        child = unsafe { GetWindow(next, GW_HWNDNEXT) }.ok();
    }
}

fn consider(window: HWND, parent: HWND) {
    if window.is_invalid() || skipped(window) {
        return;
    }
    if !unsafe { GetPropW(window, MARK) }.0.is_null() {
        return;
    }
    let ok = unsafe { SetWindowSubclass(window, Some(proc), SUBCLASS, parent.0 as usize) }.as_bool();
    if ok {
        let _ = unsafe { SetPropW(window, MARK, Some(HANDLE(std::ptr::dangling_mut()))) };
    }
}

fn skipped(window: HWND) -> bool {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(window, &mut buf) };
    if n <= 0 {
        return false;
    }
    let name = String::from_utf16_lossy(&buf[..n as usize]);
    matches!(name.as_str(), "BowserFrameGrip" | "BowserAutoHideSensor" | "Edit")
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, parent_bits: usize) -> LRESULT {
    unsafe {
        if msg == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(proc), SUBCLASS);
            let _ = RemovePropW(hwnd, MARK);
            return DefSubclassProc(hwnd, msg, wp, lp);
        }
        if msg == WM_NCHITTEST {
            let parent = HWND(parent_bits as *mut _);
            if super::caption_buttons_live(parent) {
                let mut point = super::super::frame::point_from_lparam(lp);
                if ScreenToClient(parent, &mut point).as_bool() && super::caption_point_is_button(parent, point.x, point.y) {
                    return LRESULT(HTTRANSPARENT as isize);
                }
            }
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}
