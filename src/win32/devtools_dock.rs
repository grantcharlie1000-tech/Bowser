//! Docked Developer Tools pane, right of the page.
//!
//! `webview2-com` 0.39.1 only exposes `OpenDevToolsWindow()`: it takes no parent and always creates
//! its own top-level window, and there is no way to close it either. Microsoft has no plans to add a
//! dock API (WebView2Feedback #2615, closed not planned), and opening a remote-debugging port to host
//! a second DevTools frontend is out of scope here. So this module works with what exists: it calls
//! `OpenDevToolsWindow()` as before, diffs the system's top-level windows for the new "DevTools"
//! window WebView2's browser process just created (that window belongs to msedgewebview2.exe, never
//! to Bowser's own process, so it is found by class and title, never by process id), strips its popup
//! styling, reparents it into a `WS_CHILD` container docked on the right, and later asks it to close
//! by posting `WM_CLOSE` (the only way to ask, since there is no API for it). If the window cannot be
//! found within a few retries, the floating window WebView2 already created is left alone and the
//! page keeps its full width.
//!
//! One container (and one resize grip) per Bowser window. Each tab that has opened DevTools gets its
//! own reparented child window, kept alive in that container even while hidden; only the active tab's
//! child is shown and sized, so switching tabs hides the previous one and shows the new tab's (or
//! collapses the dock if the new tab never opened DevTools).

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::c_void,
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, InvalidateRect,
            PAINTSTRUCT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{ReleaseCapture, SetCapture},
            WindowsAndMessaging::{
                CREATESTRUCTW, CreateWindowExW, DefWindowProcW, EnumWindows, GWL_STYLE,
                GWLP_USERDATA, GetClassNameW, GetClientRect, GetWindowLongPtrW, GetWindowTextW,
                HWND_TOP, IDC_ARROW, IDC_SIZEWE, KillTimer, LoadCursorW, PostMessageW,
                RegisterClassW, SW_HIDE, SW_SHOW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
                SWP_NOSIZE, SWP_NOZORDER, SetCursor, SetParent, SetTimer, SetWindowLongPtrW,
                SetWindowPos, ShowWindow, WM_CLOSE, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP,
                WM_MOUSEMOVE, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WNDCLASSW, WS_CAPTION,
                WS_CHILD, WS_CLIPCHILDREN, WS_POPUP, WS_THICKFRAME,
            },
        },
    },
    core::{BOOL, PCWSTR, w},
};

use crate::{tabs::TabId, theme, win32::glass};

const CLASS: PCWSTR = w!("BowserDevToolsDock");
const MIN_WIDTH_DIP: i32 = 280;
const MAX_WIDTH_DIP: i32 = 520;
const DEFAULT_WIDTH_DIP: i32 = 360;
const GRIP_DIP: i32 = 6;
/// Discovery polls at this interval, for up to `DISCOVERY_ATTEMPTS` tries (~1.5s total), before
/// giving up and leaving the floating window WebView2 already created alone.
const DISCOVERY_MS: u32 = 50;
const DISCOVERY_ATTEMPTS: u32 = 30;
pub(crate) const TIMER_ID: usize = 0xB061;

/// WebView2's standalone DevTools window: `Chrome_WidgetWin_1`, titled "DevTools - ...".
const DEVTOOLS_CLASS: &str = "Chrome_WidgetWin_1";
const DEVTOOLS_TITLE_PREFIX: &str = "DevTools";

struct Pending {
    tab: TabId,
    before: Vec<isize>,
    attempts: u32,
}

struct Dock {
    owner: HWND,
    /// Reparented DevTools windows, one per tab that has opened them, kept alive while hidden.
    panes: HashMap<TabId, HWND>,
    /// The tab whose pane is currently shown and sized (at most one).
    shown: Option<TabId>,
    /// The tab devtools_dock believes is active right now, kept in step by `follow_active_tab` and
    /// by `discover` (which only ever targets the active tab). A pane whose discovery finishes after
    /// the user has switched away is filed under its tab but left hidden.
    active: Option<TabId>,
    width_dip: i32,
    width: i32,
    client: RECT,
    resizing: bool,
    pending: Option<Pending>,
}

thread_local! {
    static HOSTS: RefCell<HashMap<isize, isize>> = RefCell::new(HashMap::new());
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
    /// The last width the user dragged to, remembered in memory only (no settings persistence).
    static WIDTH_DIP: Cell<i32> = const { Cell::new(DEFAULT_WIDTH_DIP) };
}

// ----- entry points called from outside this module -----

/// Starts (or resumes) the search for the DevTools window `OpenDevToolsWindow` just asked
/// WebView2's browser process to create, for `tab` (always the active tab), so it can be docked once
/// found. `before` is the top-level window snapshot taken right before that call.
pub(crate) fn discover(owner: HWND, tab: TabId, before: Vec<isize>) {
    let Some(hwnd) = ensure_container(owner) else {
        return;
    };
    raise(hwnd);
    let found = find_new_devtools_window(&before);
    let Some(dock) = (unsafe { host(hwnd) }) else {
        return;
    };
    dock.active = Some(tab);
    match found {
        Some(devtools) => finish_open(hwnd, dock, tab, devtools),
        None => {
            dock.pending = Some(Pending {
                tab,
                before,
                attempts: 0,
            });
            unsafe {
                let _ = SetTimer(Some(owner), TIMER_ID, DISCOVERY_MS, None);
            }
        }
    }
}

/// A discovery retry tick. Returns whether to keep the timer running.
pub(crate) fn timer(owner: HWND) -> bool {
    let Some(hwnd) = lookup(owner) else {
        return false;
    };
    let Some(dock) = (unsafe { host(hwnd) }) else {
        return false;
    };
    let Some(pending) = dock.pending.take() else {
        return false;
    };
    if let Some(devtools) = find_new_devtools_window(&pending.before) {
        finish_open(hwnd, dock, pending.tab, devtools);
        return false;
    }
    if pending.attempts + 1 >= DISCOVERY_ATTEMPTS {
        // Gave up: the floating window WebView2 created (if any) is left as-is.
        return false;
    }
    dock.pending = Some(Pending {
        attempts: pending.attempts + 1,
        ..pending
    });
    true
}

/// True and closes `tab`'s pane (or cancels its pending discovery) when it was open; false when
/// there was nothing to close, so the caller knows to open a new one instead.
pub(crate) fn close_if_open(owner: HWND, tab: TabId) -> bool {
    let Some(hwnd) = lookup(owner) else {
        return false;
    };
    let Some(dock) = (unsafe { host(hwnd) }) else {
        return false;
    };
    if dock.pending.as_ref().is_some_and(|pending| pending.tab == tab) {
        dock.pending = None;
        unsafe {
            let _ = KillTimer(Some(owner), TIMER_ID);
        }
        return true;
    }
    let Some(child) = dock.panes.remove(&tab) else {
        return false;
    };
    unsafe {
        let _ = PostMessageW(Some(child), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
    if dock.shown == Some(tab) {
        dock.shown = None;
        dock.width = 0;
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        notify_resized(owner);
    }
    true
}

/// The active tab changed: shows that tab's docked pane if it has one, hides the one that was shown.
pub(crate) fn follow_active_tab(owner: HWND, tab: TabId) {
    let Some(hwnd) = lookup(owner) else {
        return;
    };
    let Some(dock) = (unsafe { host(hwnd) }) else {
        return;
    };
    dock.active = Some(tab);
    if dock.shown == Some(tab) {
        return;
    }
    if let Some(previous) = dock.shown.take().and_then(|old| dock.panes.get(&old).copied()) {
        unsafe {
            let _ = ShowWindow(previous, SW_HIDE);
        }
    }
    if dock.panes.contains_key(&tab) {
        show_pane(hwnd, dock, tab);
    } else {
        dock.width = 0;
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
    notify_resized(owner);
}

/// The tab closed: drops its tracked pane (asking it to close, if it was open) and any pending
/// discovery for it.
pub(crate) fn tab_closed(owner: HWND, tab: TabId) {
    let Some(hwnd) = lookup(owner) else {
        return;
    };
    let Some(dock) = (unsafe { host(hwnd) }) else {
        return;
    };
    if dock.pending.as_ref().is_some_and(|pending| pending.tab == tab) {
        dock.pending = None;
        unsafe {
            let _ = KillTimer(Some(owner), TIMER_ID);
        }
    }
    if let Some(child) = dock.panes.remove(&tab) {
        unsafe {
            let _ = PostMessageW(Some(child), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
    if dock.shown == Some(tab) {
        dock.shown = None;
        dock.width = 0;
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        notify_resized(owner);
    }
}

/// Snapshot of every top-level window right now, taken right before `OpenDevToolsWindow` so the new
/// DevTools window can be found by diffing against it afterwards.
pub(crate) fn snapshot_top_level() -> Vec<isize> {
    let mut found = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(std::ptr::addr_of_mut!(found) as isize));
    }
    found
}

/// The page area a dock of `width` px leaves: it sits flush against the viewport's right edge and the
/// page ends where it begins. Called from `window::adjusted_viewport`.
pub(crate) fn adjust_viewport(viewport: RECT, owner: HWND) -> RECT {
    let width = shown_width(owner);
    if width == 0 {
        return viewport;
    }
    reserved_viewport(viewport, width)
}

/// Repositions the container (and its shown child, if any) to `viewport`. Called from
/// `window::apply_sidebar_bounds`, beside `side_panel::sync`.
pub(crate) fn sync(owner: HWND, viewport: RECT, window_dpi: u32) {
    let Some(hwnd) = lookup(owner) else {
        return;
    };
    let Some(dock) = (unsafe { host(hwnd) }) else {
        return;
    };
    dock.client = viewport;
    if dock.shown.is_some() {
        match width_px(dock.width_dip, window_dpi, (viewport.right - viewport.left).max(0)) {
            Some(width) => dock.width = width,
            None => {
                dock.shown = None;
                dock.width = 0;
            }
        }
    }
    let visible = dock.shown.is_some() && dock.width > 0;
    let left = viewport.right - dock.width.max(0);
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            left,
            viewport.top,
            dock.width.max(0),
            (viewport.bottom - viewport.top).max(0),
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    layout_shown_child(hwnd, dock);
    unsafe {
        let _ = ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
    }
}

// ----- internals -----

fn notify_resized(owner: HWND) {
    crate::win32::post(owner, super::side_panel::WM_RESIZED, WPARAM(0), LPARAM(0));
}

fn lookup(owner: HWND) -> Option<HWND> {
    let raw = HOSTS.with(|hosts| hosts.borrow().get(&(owner.0 as isize)).copied())?;
    (raw != 0).then(|| HWND(raw as *mut c_void))
}

fn shown_width(owner: HWND) -> i32 {
    let Some(hwnd) = lookup(owner) else { return 0 };
    unsafe {
        host(hwnd).map_or(0, |dock| if dock.shown.is_some() { dock.width.max(0) } else { 0 })
    }
}

/// The page area a dock of `width` px leaves. Pure.
fn reserved_viewport(viewport: RECT, width: i32) -> RECT {
    RECT { right: viewport.right - width.max(0), ..viewport }
}

/// The dock width in px for `width_dip` at `dpi`, fit into `available` px. Same policy as the side
/// panel's (`side_panel::panel_width_px`), since the two size themselves the same way: never below
/// `MIN_WIDTH_DIP`, never above `MAX_WIDTH_DIP` or 45% of `available`, and never leaving less than
/// 320 dip for the page. `None` when the window is too narrow for both. Pure.
fn width_px(width_dip: i32, dpi: u32, available: i32) -> Option<i32> {
    super::side_panel::panel_width_px(width_dip, dpi, available)
}

fn finish_open(hwnd: HWND, dock: &mut Dock, tab: TabId, devtools: HWND) {
    reparent(hwnd, devtools);
    dock.panes.insert(tab, devtools);
    if dock.active == Some(tab) {
        show_pane(hwnd, dock, tab);
        notify_resized(dock.owner);
    }
}

fn show_pane(hwnd: HWND, dock: &mut Dock, tab: TabId) {
    dock.shown = Some(tab);
    if let Some(width) = width_px(dock.width_dip, dpi(hwnd), (dock.client.right - dock.client.left).max(0)) {
        dock.width = width;
    }
    let left = dock.client.right - dock.width.max(0);
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOP),
            left,
            dock.client.top,
            dock.width.max(0),
            (dock.client.bottom - dock.client.top).max(0),
            SWP_NOACTIVATE,
        );
        let _ = ShowWindow(hwnd, SW_SHOW);
        if let Some(&child) = dock.panes.get(&tab) {
            let _ = ShowWindow(child, SW_SHOW);
        }
    }
    layout_shown_child(hwnd, dock);
}

fn layout_shown_child(hwnd: HWND, dock: &Dock) {
    if let Some(tab) = dock.shown
        && let Some(&child) = dock.panes.get(&tab)
    {
        let grip = scale(GRIP_DIP, hwnd);
        let width = (dock.width - grip).max(0);
        let height = (dock.client.bottom - dock.client.top).max(0);
        unsafe {
            let _ = SetWindowPos(child, None, grip, 0, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}

/// Strips the floating window's popup chrome and makes it a child of `container`, filling it (minus
/// the grip strip, positioned separately by `layout_shown_child`).
fn reparent(container: HWND, devtools: HWND) {
    unsafe {
        let style = GetWindowLongPtrW(devtools, GWL_STYLE);
        let style = (style
            & !(WS_POPUP.0 as isize)
            & !(WS_CAPTION.0 as isize)
            & !(WS_THICKFRAME.0 as isize))
            | WS_CHILD.0 as isize;
        let _ = SetWindowLongPtrW(devtools, GWL_STYLE, style);
        let _ = SetParent(devtools, Some(container));
        let _ = SetWindowPos(
            devtools,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let found = unsafe { &mut *(lparam.0 as *mut Vec<isize>) };
    found.push(hwnd.0 as isize);
    BOOL(1)
}

fn window_class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) }.max(0) as usize;
    String::from_utf16_lossy(&buffer[..len])
}

fn window_title(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let len = unsafe { GetWindowTextW(hwnd, &mut buffer) }.max(0) as usize;
    String::from_utf16_lossy(&buffer[..len])
}

fn is_devtools_window(hwnd: HWND) -> bool {
    window_class_name(hwnd) == DEVTOOLS_CLASS && window_title(hwnd).starts_with(DEVTOOLS_TITLE_PREFIX)
}

/// A window present now but not in `before`, matching WebView2's standalone DevTools window. Never
/// matched by process id: the window belongs to msedgewebview2.exe, not Bowser.
fn find_new_devtools_window(before: &[isize]) -> Option<HWND> {
    snapshot_top_level()
        .into_iter()
        .filter(|raw| !before.contains(raw))
        .map(|raw| HWND(raw as *mut c_void))
        .find(|&hwnd| is_devtools_window(hwnd))
}

fn ensure_container(owner: HWND) -> Option<HWND> {
    if let Some(hwnd) = lookup(owner) {
        return Some(hwnd);
    }
    if !REGISTERED.with(Cell::get) && !register_class() {
        return None;
    }
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(owner, &mut client);
    }
    let state = Box::new(Dock {
        owner,
        panes: HashMap::new(),
        shown: None,
        active: None,
        width_dip: WIDTH_DIP.with(Cell::get),
        width: 0,
        client,
        resizing: false,
        pending: None,
    });
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None).unwrap_or_default().0 });
    let hwnd = unsafe {
        CreateWindowExW(
            Default::default(),
            CLASS,
            w!(""),
            WS_CHILD | WS_CLIPCHILDREN,
            client.right,
            client.top,
            0,
            (client.bottom - client.top).max(1),
            Some(owner),
            None,
            Some(instance),
            Some(Box::into_raw(state).cast()),
        )
    };
    let Ok(hwnd) = hwnd else { return None };
    HOSTS.with(|hosts| {
        hosts.borrow_mut().insert(owner.0 as isize, hwnd.0 as isize);
    });
    Some(hwnd)
}

fn register_class() -> bool {
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None).unwrap_or_default().0 });
    let class = WNDCLASSW {
        lpfnWndProc: Some(proc),
        hInstance: instance,
        lpszClassName: CLASS,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
        ..Default::default()
    };
    let ok = unsafe { RegisterClassW(&class) != 0 };
    REGISTERED.with(|slot| slot.set(ok));
    ok
}

/// Puts the container above the tab page controllers, which are created later and so start on top.
fn raise(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(hwnd, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// The container width (px, before clamping) implied by dragging the grip to client-local `x`, from
/// a width of `before_width` px: the grip sits at the dock's own left edge, so moving it left (a
/// smaller, even negative, local x as the capture keeps delivering moves past the edge) grows the
/// width. Pure.
fn width_px_from_grip(x: i32, before_width: i32) -> i32 {
    before_width - x
}

fn width_from_drag(owner: HWND, x: i32, before_width: i32) -> i32 {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(owner, &mut r);
    }
    let window_dpi = dpi(owner);
    let px = width_px_from_grip(x, before_width);
    width_px(px.saturating_mul(96) / window_dpi as i32, window_dpi, (r.right - r.left).max(0)).unwrap_or(0)
}

fn dpi(hwnd: HWND) -> u32 {
    unsafe { GetDpiForWindow(hwnd).max(96) }
}
fn scale(value: i32, hwnd: HWND) -> i32 {
    value.saturating_mul(dpi(hwnd) as i32) / 96
}

unsafe fn host(hwnd: HWND) -> Option<&'static mut Dock> {
    if hwnd.0.is_null() {
        return None;
    }
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Dock;
    unsafe { ptr.as_mut() }
}

fn colorref(c: theme::Rgba) -> windows::Win32::Foundation::COLORREF {
    windows::Win32::Foundation::COLORREF(u32::from(c.r) | (u32::from(c.g) << 8) | (u32::from(c.b) << 16))
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let p = theme::palette(glass::appearance(), true);
        let brush = CreateSolidBrush(colorref(p.window));
        FillRect(dc, &client, brush);
        let _ = DeleteObject(brush.into());
        let grip = RECT { right: scale(GRIP_DIP, hwnd), ..client };
        let grip_brush = CreateSolidBrush(colorref(p.hairline));
        FillRect(dc, &grip, grip_brush);
        let _ = DeleteObject(grip_brush.into());
        let _ = EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_NCCREATE => {
                let create = &*(lp.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let x = lp.0 as u16 as i16 as i32;
                if let Some(dock) = host(hwnd) {
                    if dock.resizing {
                        dock.width = width_from_drag(dock.owner, x, dock.width);
                        dock.width_dip = (dock.width.saturating_mul(96) / dpi(hwnd) as i32)
                            .clamp(MIN_WIDTH_DIP, MAX_WIDTH_DIP);
                        let left = dock.client.right - dock.width.max(0);
                        let _ = SetWindowPos(
                            hwnd,
                            None,
                            left,
                            dock.client.top,
                            dock.width.max(0),
                            (dock.client.bottom - dock.client.top).max(0),
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                        layout_shown_child(hwnd, dock);
                        let _ = InvalidateRect(Some(hwnd), None, false);
                        notify_resized(dock.owner);
                    } else if x < scale(GRIP_DIP, hwnd)
                        && let Ok(cursor) = LoadCursorW(None, IDC_SIZEWE)
                    {
                        let _ = SetCursor(Some(cursor));
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let x = lp.0 as u16 as i16 as i32;
                if let Some(dock) = host(hwnd)
                    && x < scale(GRIP_DIP, hwnd)
                {
                    dock.resizing = true;
                    let _ = SetCapture(hwnd);
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                if let Some(dock) = host(hwnd)
                    && dock.resizing
                {
                    dock.resizing = false;
                    WIDTH_DIP.with(|w| w.set(dock.width_dip));
                    let _ = ReleaseCapture();
                }
                LRESULT(0)
            }
            WM_NCDESTROY => {
                if let Some(dock) = host(hwnd) {
                    let owner = dock.owner.0 as isize;
                    drop(Box::from_raw(dock));
                    HOSTS.with(|hosts| {
                        if hosts.borrow().get(&owner) == Some(&(hwnd.0 as isize)) {
                            hosts.borrow_mut().remove(&owner);
                        }
                    });
                }
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_viewport_leaves_the_dock_s_width_on_the_right() {
        let viewport = RECT { left: 0, top: 70, right: 1600, bottom: 900 };
        assert_eq!(reserved_viewport(viewport, 360), RECT { right: 1240, ..viewport });
        // Closed (width 0) or a negative width (never produced, but defensive), the page regains
        // the full viewport.
        assert_eq!(reserved_viewport(viewport, 0), viewport);
        assert_eq!(reserved_viewport(viewport, -5), viewport);
    }

    #[test]
    fn width_clamp_matches_the_side_panel_s_policy() {
        // Within range: used as-is.
        assert_eq!(width_px(360, 96, 1200), Some(360));
        // Below the minimum: clamped up.
        assert_eq!(width_px(140, 96, 1200), Some(MIN_WIDTH_DIP));
        // Above the maximum: clamped down.
        assert_eq!(width_px(900, 96, 2000), Some(MAX_WIDTH_DIP));
        // At most 45% of the available width.
        assert_eq!(width_px(520, 96, 1000), Some(450));
        // DPI-aware.
        assert_eq!(width_px(360, 144, 1800), Some(540));
        // Too narrow to fit the minimum dock and still leave the page usable: no width fits.
        assert_eq!(width_px(280, 96, 580), None);
    }

    #[test]
    fn grip_drag_grows_the_dock_when_dragged_toward_the_page() {
        // The grip sits at the dock's own left edge; dragging it left (smaller, even negative,
        // local x once capture keeps delivering moves past the edge) grows the width.
        assert_eq!(width_px_from_grip(-40, 360), 400);
        assert_eq!(width_px_from_grip(60, 360), 300);
        assert_eq!(width_px_from_grip(0, 360), 360);
    }
}
