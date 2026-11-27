//! Dragging a tab out of its window (docs/design/tab-drag.md, "Drag out into a window").
//!
//! A held tab released while it floats off the strip, and not over the strip or the page, leaves
//! this window:
//!
//! - over another Bowser window, it opens there as a new tab (the link handoff of
//!   `instance::deliver_to`) and that window comes forward;
//! - anywhere else (another app, the desktop, this window's toolbar), it opens in a new window whose
//!   tab strip starts under the pointer.
//!
//! Each window is a process of its own, so the tab moves as its address: the page loads again in
//! the other window and its back and forward history stays behind. A private tab never moves (a
//! new process would not be private). The only tab of a window moves into another window and then
//! this window closes; dropped anywhere else it stays.

use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
    Graphics::Gdi::ClientToScreen,
    UI::WindowsAndMessaging::{GetCursorPos, PostMessageW, WM_CLOSE},
};

use crate::app::{App, AppRef};

/// Where the pointer sits on the new window's tab strip (DIP from its top-left corner).
const GRAB_DIP: (i32, i32) = (120, 16);

fn contains(rect: RECT, point: POINT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

/// Whether screen `point` is over `owner`'s tab view or page.
fn over_strip_or_page(owner: HWND, point: POINT) -> bool {
    if super::window::tabs_host_screen(owner).is_some_and(|(strip, _)| contains(strip, point)) {
        return true;
    }
    let Some(mut page) = super::window::split_viewport(owner) else {
        return true;
    };
    let mut origin = POINT::default();
    if !unsafe { ClientToScreen(owner, &mut origin) }.as_bool() {
        return true;
    }
    page.left += origin.x;
    page.right += origin.x;
    page.top += origin.y;
    page.bottom += origin.y;
    contains(page, point)
}

/// Tab `raw` (a wire id) was released while floating. Moves it to another window when the pointer
/// is off the strip and the page; true when it moved (the caller then fades the ghost).
pub(super) fn release(owner: HWND, app: &AppRef, raw: u32) -> bool {
    let Some(id) = crate::chrome_host::tab_from_wire(raw, &App::tab_ids(app)) else {
        return false;
    };
    let mut cursor = POINT::default();
    if unsafe { GetCursorPos(&mut cursor) }.is_err() || over_strip_or_page(owner, cursor) || App::tab_private(app, id) {
        return false;
    }
    let Some(url) = App::tab_committed_url(app, id).filter(crate::profile::can_open_in_new_window) else {
        return false;
    };
    let only = App::all_tab_ids(app).len() <= 1;
    match crate::instance::bowser_window_at(cursor.x, cursor.y, owner) {
        Some(target) => {
            if !crate::instance::deliver_to(target, &[url]) {
                return false;
            }
            if only {
                unsafe {
                    let _ = PostMessageW(Some(owner), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            } else {
                let _ = App::close_tab(app, id);
            }
            true
        }
        None => {
            if only {
                return false;
            }
            let at = (
                cursor.x - super::window::split_px(owner, GRAB_DIP.0),
                cursor.y - super::window::split_px(owner, GRAB_DIP.1),
            );
            let launched = crate::profile::active().is_ok_and(|profile| crate::profile::spawn_window_with_url_at(&profile, &url, at));
            if launched {
                let _ = App::close_tab(app, id);
            }
            launched
        }
    }
}
