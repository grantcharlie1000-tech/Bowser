//! The rail flyout window: History, Bookmarks and Customize open beside their rail button as a
//! popover over the page instead of a docked side panel (docs/design/flyouts.md).
//!
//! One owned, activating `WS_POPUP` per browser window, created hidden with the chrome controllers.
//! It hosts the panel controller (`html_panel.rs`, `View::Panel`). The page draws the surface, its
//! caret and its shadow, so the window is the surface inflated by [`SHADOW`] and is transparent
//! outside it: the DWM frame is extended over the whole client area, so DWM honours the WebView's
//! per-pixel alpha and the shadow is never clipped by a window region. Nothing here re-bounds the
//! page: the flyout only shows, moves and hides.

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, GW_OWNER, GetWindow, HWND_TOP, KillTimer, PostMessageW,
            RegisterClassW, SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SetForegroundWindow, SetTimer,
            SetWindowPos, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_APP, WM_ERASEBKGND, WM_TIMER,
            WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
        },
    },
    core::{PCWSTR, w},
};

const CLASS_NAME: PCWSTR = w!("BowserFlyout");
/// Posted to the flyout window when it lost activation; handled outside `WM_ACTIVATE`, so the
/// panel runtime is never re-entered from inside a `SetForegroundWindow` call.
const WM_FLYOUT_DEACTIVATED: u32 = WM_APP + 0x91;
const SHOW_FALLBACK_TIMER: usize = 0xF1;
/// If the page has not said it painted by then, the window is shown anyway.
const SHOW_FALLBACK_MS: u32 = 300;
const RAIL_SETTLE_TIMER: usize = 0xF2;
/// After a press on the rail took the activation, the flyout waits this long for the rail click.
pub(crate) const RAIL_SETTLE_MS: u32 = 1500;

/// Room around the surface for its `--bw-sh-flyout` shadow (0 8px 16px, 0 0 2px), in DIP. The left
/// side faces the rail: it only holds the caret's 4 px tip, so the window never covers the rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Margins {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
pub(crate) const SHADOW: Margins = Margins { left: 4, top: 8, right: 16, bottom: 24 };
/// Rail host's right edge to the surface's left edge, in DIP (the caret bridges it). Small enough
/// that the surface covers the page frame's edge instead of leaving a bright sliver beside it.
const GAP: i32 = 4;
/// Distance kept from the owner window's right edge and the band's top and bottom, in DIP.
const EDGE: i32 = 8;
/// Below this owner width (DIP) the surface takes the whole width right of the rail.
const NARROW: i32 = 500;
/// The surface's 44 px header is centred on the button.
const HEADER_CENTRE: i32 = 22;
/// The caret is hidden when its centre would be this close to the surface's top or bottom (DIP).
const CARET_KEEP: i32 = 18;

/// The surface's preferred size in DIP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Size {
    pub width: i32,
    pub height: i32,
}
pub(crate) const LIST_SIZE: Size = Size { width: 360, height: 600 };
pub(crate) const CUSTOMIZE_SIZE: Size = Size { width: 380, height: 640 };

/// Where the flyout goes, in screen pixels, and where its caret points (DIP from the surface top,
/// `-1` for none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    pub window: RECT,
    pub surface: RECT,
    pub caret: i32,
}

fn px(value: i32, dpi: u32) -> i32 {
    (value * dpi.max(1) as i32 + 48) / 96
}

fn dip(value: i32, dpi: u32) -> i32 {
    (value * 96 + dpi.max(1) as i32 / 2) / dpi.max(1) as i32
}

/// Places a flyout of `size` beside the rail: left edge `GAP` right of `rail_right`, header centred
/// on `button`, never wider than the owner's client area (`owner`, screen) nor taller than the band
/// shared by the owner and the monitor work area. All rectangles are screen pixels at `dpi`.
pub(crate) fn place(rail_right: i32, button: RECT, owner: RECT, work: RECT, dpi: u32, size: Size) -> Placement {
    let left = rail_right + px(GAP, dpi);
    let room = (owner.right - px(EDGE, dpi) - left).max(1);
    let width = if dip(owner.right - owner.left, dpi) < NARROW { room } else { px(size.width, dpi).min(room) };
    let band_top = owner.top.max(work.top) + px(EDGE, dpi);
    let band_bottom = (owner.bottom.min(work.bottom) - px(EDGE, dpi)).max(band_top + 1);
    let height = px(size.height, dpi).min(band_bottom - band_top);
    let centre = (button.top + button.bottom) / 2;
    let top = (centre - px(HEADER_CENTRE, dpi)).clamp(band_top, band_bottom - height);
    let offset = centre - top;
    let keep = px(CARET_KEEP, dpi);
    let caret = if offset >= keep && offset <= height - keep { dip(offset, dpi) } else { -1 };
    let surface = RECT { left, top, right: left + width, bottom: top + height };
    let window = RECT {
        left: surface.left - px(SHADOW.left, dpi),
        top: surface.top - px(SHADOW.top, dpi),
        right: surface.right + px(SHADOW.right, dpi),
        bottom: surface.bottom + px(SHADOW.bottom, dpi),
    };
    Placement { window, surface, caret }
}

/// Creates the hidden flyout window owned by `owner`.
pub(crate) fn create(owner: HWND) -> Option<HWND> {
    register_class()?;
    let instance = unsafe { GetModuleHandleW(None).ok()? };
    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS_NAME,
            w!(""),
            WS_POPUP,
            0,
            0,
            360,
            600,
            Some(owner),
            None,
            Some(instance.into()),
            None,
        )
    }
    .ok()?;
    // "Sheet of glass": the whole client area is DWM frame, so pixels the WebView leaves
    // transparent (the shadow room) show what is behind the window.
    let margins = [-1_i32; 4];
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(window, margins.as_ptr());
    }
    Some(window)
}

/// Moves and sizes the window without showing or activating it.
pub(crate) fn set_bounds(window: HWND, bounds: RECT) {
    unsafe {
        let _ = SetWindowPos(
            window,
            Some(HWND_TOP),
            bounds.left,
            bounds.top,
            (bounds.right - bounds.left).max(1),
            (bounds.bottom - bounds.top).max(1),
            SWP_NOACTIVATE,
        );
    }
}

/// Shows and activates the window (an activating popup, so its search field takes typing).
pub(crate) fn show(window: HWND) {
    cancel_show_fallback(window);
    unsafe {
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
    }
}

pub(crate) fn hide(window: HWND) {
    cancel_show_fallback(window);
    unsafe {
        let _ = ShowWindow(window, SW_HIDE);
    }
}

/// Shows the window after `SHOW_FALLBACK_MS` unless the page reports that it painted first.
pub(crate) fn arm_show_fallback(window: HWND) {
    unsafe {
        let _ = SetTimer(Some(window), SHOW_FALLBACK_TIMER, SHOW_FALLBACK_MS, None);
    }
}

/// Starts the wait for the rail command after a rail press (see `html_panel::flyout_deactivated`).
pub(crate) fn arm_rail_settle(window: HWND) {
    unsafe {
        let _ = SetTimer(Some(window), RAIL_SETTLE_TIMER, RAIL_SETTLE_MS, None);
    }
}

fn cancel_show_fallback(window: HWND) {
    unsafe {
        let _ = KillTimer(Some(window), SHOW_FALLBACK_TIMER);
    }
}

fn register_class() -> Option<()> {
    thread_local! { static REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
    if REGISTERED.with(std::cell::Cell::get) {
        return Some(());
    }
    let instance = unsafe { GetModuleHandleW(None).ok()? };
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance.into(),
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        return None;
    }
    REGISTERED.with(|registered| registered.set(true));
    Some(())
}

fn owner_of(window: HWND) -> Option<HWND> {
    unsafe { GetWindow(window, GW_OWNER) }.ok().filter(|owner| !owner.is_invalid())
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_ACTIVATE if (wparam.0 & 0xffff) as u32 == WA_INACTIVE => {
            unsafe {
                let _ = PostMessageW(Some(window), WM_FLYOUT_DEACTIVATED, WPARAM(0), lparam);
            }
            LRESULT(0)
        }
        WM_FLYOUT_DEACTIVATED => {
            if let Some(owner) = owner_of(window) {
                super::html_panel::flyout_deactivated(owner);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == SHOW_FALLBACK_TIMER => {
            cancel_show_fallback(window);
            if let Some(owner) = owner_of(window) {
                super::html_panel::flyout_painted(owner);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == RAIL_SETTLE_TIMER => {
            unsafe {
                let _ = KillTimer(Some(window), RAIL_SETTLE_TIMER);
            }
            if let Some(owner) = owner_of(window) {
                super::html_panel::flyout_rail_settled(owner);
            }
            LRESULT(0)
        }
        // The WebView covers the whole client area; nothing is painted underneath it.
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmExtendFrameIntoClientArea(hwnd: HWND, margins: *const i32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: RECT = RECT { left: 0, top: 0, right: 1920, bottom: 1040 };
    const OWNER: RECT = RECT { left: 40, top: 30, right: 1320, bottom: 830 };

    fn button(y: i32) -> RECT {
        RECT { left: 44, top: y, right: 70, bottom: y + 26 }
    }

    #[test]
    fn sits_beside_the_rail_with_the_header_centred_on_the_button() {
        let placed = place(74, button(70), OWNER, WORK, 96, LIST_SIZE);
        assert_eq!(placed.surface, RECT { left: 78, top: 61, right: 438, bottom: 661 });
        // Shadow room: 4 left (caret side; the window starts at the rail's edge), 8 top, 16 right,
        // 24 bottom.
        assert_eq!(placed.window, RECT { left: 74, top: 53, right: 454, bottom: 685 });
        assert_eq!(placed.caret, 22, "caret points at the button centre");
    }

    #[test]
    fn a_button_low_in_the_rail_keeps_the_flyout_inside_the_window_and_the_caret_on_it() {
        let placed = place(74, button(780), OWNER, WORK, 96, LIST_SIZE);
        assert_eq!(placed.surface.bottom, OWNER.bottom - 8);
        assert_eq!(placed.surface.bottom - placed.surface.top, 600);
        assert_eq!(placed.caret, 793 - placed.surface.top);
    }

    #[test]
    fn height_is_capped_by_the_window_and_the_work_area() {
        let short = RECT { left: 40, top: 30, right: 1320, bottom: 530 };
        let placed = place(74, button(70), short, WORK, 96, CUSTOMIZE_SIZE);
        assert_eq!((placed.surface.top, placed.surface.bottom), (38, 522));
        let low_work = RECT { bottom: 400, ..WORK };
        let capped = place(74, button(70), OWNER, low_work, 96, LIST_SIZE);
        assert_eq!((capped.surface.top, capped.surface.bottom), (38, 392));
    }

    #[test]
    fn width_never_passes_the_window_and_fills_it_when_narrow() {
        let fits = RECT { left: 40, top: 30, right: 540, bottom: 830 };
        let placed = place(74, button(70), fits, WORK, 96, CUSTOMIZE_SIZE);
        assert_eq!(placed.surface.right - placed.surface.left, 380);
        let narrow = RECT { right: 480, ..fits };
        let full = place(74, button(70), narrow, WORK, 96, LIST_SIZE);
        assert_eq!(full.surface.right, 480 - 8, "full width minus the rail");
        let tight = RECT { right: 400, ..fits };
        assert_eq!(place(74, button(70), tight, WORK, 96, LIST_SIZE).surface.right, 392);
    }

    #[test]
    fn scales_with_dpi_and_reports_the_caret_in_dip() {
        let owner = RECT { left: 0, top: 0, right: 2400, bottom: 1500 };
        let work = RECT { left: 0, top: 0, right: 2560, bottom: 1560 };
        let placed = place(60, RECT { left: 6, top: 120, right: 45, bottom: 159 }, owner, work, 144, LIST_SIZE);
        assert_eq!(placed.surface.left, 66);
        assert_eq!(placed.surface.right - placed.surface.left, 540);
        assert_eq!(placed.surface.bottom - placed.surface.top, 900);
        assert_eq!(placed.window.left, 60);
        assert_eq!(placed.window.bottom - placed.surface.bottom, 36);
        assert_eq!(placed.caret, 22);
    }

    #[test]
    fn a_caret_that_would_land_in_a_corner_is_hidden() {
        // The surface is pushed down to the band's top; the button sits above it.
        let placed = place(74, button(10), OWNER, WORK, 96, LIST_SIZE);
        assert_eq!(placed.surface.top, 38);
        assert_eq!(placed.caret, -1);
    }
}
