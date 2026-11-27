//! Cut-out split panes (docs/design/split-view.md, "Cut-out panes").
//!
//! A blank pane the person chose to leave empty (`App::split_cut_out`) becomes a hole in the
//! window: Bowser sets a window region on the main window and on its chrome overlay that leaves the
//! pane out, so the window behind Bowser shows through and takes the clicks there. A hole that
//! touches the page's left, right or bottom edge runs on to the window's edge, so a window placed
//! there sits flush with the screen corner. The toolbar is never cut.
//!
//! [`sync`] runs from every split sync (each chrome paint and tab change). It sets a region only
//! when the holes or the window's size change, and removes it when no hole is left.

use std::{cell::RefCell, collections::HashMap};

use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::{ClientToScreen, CombineRgn, CreateRectRgn, DeleteObject, SetWindowRgn, HRGN, RGN_DIFF},
    UI::WindowsAndMessaging::{GetClientRect, GetWindowRect, IsWindow},
};

use crate::app::{App, AppRef};

/// A page edge closer than this to the window edge (DIP) is the window's edge for a hole.
const EDGE_REACH_DIP: i32 = 24;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Applied {
    holes: Vec<(i32, i32, i32, i32)>,
    window: (i32, i32),
    client: (i32, i32, i32, i32),
    overlay: isize,
}

thread_local! {
    static APPLIED: RefCell<HashMap<isize, Applied>> = RefCell::new(HashMap::new());
}

/// The holes in client coordinates, each grown to the client edge on a side where the page meets
/// the window edge (`reach` px or less apart). `client` is the client size.
pub(crate) fn hole_rects(holes: &[RECT], page: RECT, client: (i32, i32), reach: i32) -> Vec<RECT> {
    holes
        .iter()
        .map(|hole| {
            let mut out = *hole;
            if hole.left <= page.left + 1 && page.left <= reach {
                out.left = 0;
            }
            if hole.right >= page.right - 1 && client.0 - page.right <= reach {
                out.right = client.0;
            }
            if hole.bottom >= page.bottom - 1 && client.1 - page.bottom <= reach {
                out.bottom = client.1;
            }
            out
        })
        .filter(|hole| hole.right > hole.left && hole.bottom > hole.top)
        .collect()
}

/// A rectangle `(0, 0, width, height)` minus `holes` moved by `offset`. A hole that ends at the
/// client edge is taken to the window edge (`size`).
fn region(size: (i32, i32), client: (i32, i32), offset: (i32, i32), holes: &[RECT]) -> Option<HRGN> {
    unsafe {
        let whole = CreateRectRgn(0, 0, size.0, size.1);
        if whole.is_invalid() {
            return None;
        }
        for hole in holes {
            let left = if hole.left <= 0 { 0 } else { hole.left + offset.0 };
            let right = if hole.right >= client.0 { size.0 } else { hole.right + offset.0 };
            let bottom = if hole.bottom >= client.1 { size.1 } else { hole.bottom + offset.1 };
            let cut = CreateRectRgn(left, hole.top + offset.1, right, bottom);
            if !cut.is_invalid() {
                CombineRgn(Some(whole), Some(whole), Some(cut), RGN_DIFF);
                let _ = DeleteObject(cut.into());
            }
        }
        Some(whole)
    }
}

fn set_region(hwnd: HWND, region: Option<HRGN>) {
    if hwnd.is_invalid() || !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        if let Some(region) = region {
            unsafe {
                let _ = DeleteObject(region.into());
            }
        }
        return;
    }
    // The system owns a region it accepted.
    unsafe {
        SetWindowRgn(hwnd, region, true);
    }
}

/// Brings the window's region in line with the cut-out panes of the shown split.
pub(crate) fn sync(owner: HWND, app: &AppRef) {
    let holes: Vec<RECT> = App::split_view(app)
        .zip(App::split_bounds(app))
        .map(|(split, bounds)| {
            split
                .panes()
                .filter(|(_, tab)| App::split_cut_out(app, *tab))
                .filter_map(|(pane, _)| bounds.get(pane))
                .collect()
        })
        .unwrap_or_default();
    let key = owner.0 as isize;
    if holes.is_empty() {
        let had = APPLIED.with(|all| all.borrow_mut().remove(&key));
        if let Some(had) = had {
            set_region(owner, None);
            set_region(HWND(had.overlay as _), None);
        }
        return;
    }
    let (Some(page), Some(overlay)) = (super::window::split_viewport(owner), super::window::overlay_of(owner)) else {
        return;
    };
    let mut window = RECT::default();
    let mut client = RECT::default();
    let mut origin = POINT::default();
    unsafe {
        if GetWindowRect(owner, &mut window).is_err() || GetClientRect(owner, &mut client).is_err() || !ClientToScreen(owner, &mut origin).as_bool() {
            return;
        }
    }
    let client_size = (client.right - client.left, client.bottom - client.top);
    let holes = hole_rects(&holes, page, client_size, super::window::split_px(owner, EDGE_REACH_DIP));
    let applied = Applied {
        holes: holes.iter().map(|r| (r.left, r.top, r.right, r.bottom)).collect(),
        window: (window.right - window.left, window.bottom - window.top),
        client: (origin.x - window.left, origin.y - window.top, client_size.0, client_size.1),
        overlay: overlay.0 as isize,
    };
    if APPLIED.with(|all| all.borrow().get(&key) == Some(&applied)) {
        return;
    }
    let offset = (applied.client.0, applied.client.1);
    set_region(owner, region(applied.window, client_size, offset, &holes));
    set_region(overlay, region(client_size, client_size, (0, 0), &holes));
    APPLIED.with(|all| all.borrow_mut().insert(key, applied));
}

/// Forgets a closed window.
pub(crate) fn forget(owner: HWND) {
    APPLIED.with(|all| all.borrow_mut().remove(&(owner.0 as isize)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }

    #[test]
    fn a_hole_at_the_page_edge_runs_to_the_window_edge_but_never_into_the_toolbar() {
        let page = rect(6, 80, 1914, 1074);
        let client = (1920, 1080);
        // Bottom-left quarter: grows left and down, not up.
        let holes = hole_rects(&[rect(6, 581, 956, 1074)], page, client, 24);
        assert_eq!(holes, [rect(0, 581, 956, 1080)]);
        // Top-right quarter: grows right only.
        let holes = hole_rects(&[rect(964, 80, 1914, 573)], page, client, 24);
        assert_eq!(holes, [rect(964, 80, 1920, 573)]);
    }

    #[test]
    fn a_page_far_from_the_window_edge_keeps_the_sidebar() {
        // A vertical tab sidebar 240 px wide sits left of the page: it is not cut.
        let page = rect(240, 40, 1914, 1074);
        let holes = hole_rects(&[rect(240, 561, 1074, 1074)], page, (1920, 1080), 24);
        assert_eq!(holes, [rect(240, 561, 1074, 1080)]);
    }
}
