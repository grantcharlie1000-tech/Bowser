//! The window's side of auto-hide and full screen (thread T11, docs/design/t11-auto-hide.md).
//!
//! `crate::auto_hide` holds the rules and `chrome::layout` the geometry; this module wires them to the
//! window: which bars are out of the layout right now, the pointer sensors, the reveal and hide timing,
//! F11 and page full screen. Nothing here runs, and no window or timer exists, while auto-hide is off.
//!
//! How a bar floats. The page is laid out as if the hidden bars were gone (`WindowChrome::layout.viewport`)
//! and is never resized when a bar appears. A revealed bar is the same HTML view in the same place it has
//! with auto-hide off, raised above the page's window (`ChromeHost::raise_to_top`), with the window's
//! solid palette so it does not show the page through itself.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromPoint,
    MonitorFromWindow, PAINTSTRUCT, ScreenToClient,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CreateWindowExW, DefWindowProcW, GWLP_USERDATA, GetCursorPos, GetParent, GetPropW, GetWindowLongPtrW,
    GetWindowPlacement, HWND_TOP, IDC_ARROW, LoadCursorW, PostMessageW, RegisterClassW, RemovePropW, SW_HIDE, SW_RESTORE,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SetCursor, SetPropW, SetTimer, SetWindowLongPtrW,
    SetWindowPlacement, SetWindowPos, ShowWindow, WINDOWPLACEMENT, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_PAINT,
    WM_SETCURSOR, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_CLIPSIBLINGS, WS_THICKFRAME, GWL_STYLE,
};
use windows::core::{PCWSTR, w};

use crate::app::AppRef;
use crate::auto_hide::{self as rules, Bars, Fullscreen, Machine, Plan, Tick, Zone, Zones};
use crate::chrome::layout::{AutoHideOverlay, Metrics, Rect as LRect};

use super::{
    WM_MOUSELEAVE, WindowChrome, apply_sidebar_bounds, repaint, sync_html_host_bounds, with_chrome,
};

/// Posted by a sensor window to its parent: `wParam` is the zone ([`zone_code`]) and `lParam` 1 for the
/// pointer entering, 0 for leaving; a `wParam` of [`RAISE`] asks for the sensors and shown bars to be
/// put back above the page windows.
pub(super) const WM_AUTOHIDE_SENSOR: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0xA1;
/// Timer while a reveal is pending or bars are shown. Never armed otherwise.
pub(super) const TIMER_ID: usize = 0xB0A5;
const RAISE: usize = 0xFF;
const SENSOR_CLASS: PCWSTR = w!("BowserAutoHideSensor");
const PROP_FULLSCREEN: PCWSTR = w!("BowserFullscreen");
const PROP_PANEL_FLOATS: PCWSTR = w!("BowserPanelFloats");

/// Whether any bar is auto-hidden: the chrome then paints and themes itself solid, because a revealed
/// bar floats over a page and must not let it show through the window's glass.
static SOLID: AtomicBool = AtomicBool::new(false);

pub(super) fn solid() -> bool {
    SOLID.load(Ordering::Relaxed)
}

/// The palette the window paints and themes its chrome with: the glass palette, or the solid one while
/// auto-hide has a bar out of the layout.
pub(super) fn chrome_palette() -> &'static crate::theme::Palette {
    if solid() {
        crate::theme::palette(super::glass::appearance(), true)
    } else {
        super::glass::palette()
    }
}

fn now_ms() -> u64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
}

/// The window's auto-hide state, one per window.
#[derive(Default)]
pub(super) struct Runtime {
    /// The bars out of the layout now (`rules::effective`), and whether they may be revealed.
    pub hidden: Bars,
    pub reveal_allowed: bool,
    pub machine: Machine,
    /// What the hidden bars take out of the layout, as of the last `relayout`.
    pub plan: Plan,
    /// The page rectangle with the hidden bars out, before the bookmarks bar's share: sensors sit on it.
    pub page: LRect,
    /// Where the bookmarks bar floats when it is taken out of the layout; empty when there is none.
    pub bar_float: LRect,
    pub bar_bottom: bool,
    pub overlay: AutoHideOverlay,
    sensors: Vec<(Zone, HWND)>,
    timer: bool,
    ticks: u32,
}

impl Runtime {
    pub fn top_hidden(&self) -> bool {
        self.plan.top && !self.machine.shown().contains(Zone::Top)
    }

    pub fn sidebar_hidden(&self) -> bool {
        self.plan.sidebar && !self.machine.shown().contains(Zone::Sidebar)
    }

    /// The bookmarks bar floats now (taken out of the layout and revealed), alone or with the top chrome.
    pub fn bookmarks_revealed(&self) -> bool {
        let shown = self.machine.shown();
        shown.contains(Zone::Bookmarks) || (shown.contains(Zone::Top) && rules::bookmarks_ride_with_top(self.plan, self.bar_bottom))
    }

    /// `viewport` narrowed to what is not covered by a revealed bar: the edge the page's rounded
    /// frame is drawn at while a bar floats over its top or left.
    pub fn paint_viewport(&self, viewport: LRect) -> LRect {
        let mut v = viewport;
        if self.plan.top && self.machine.shown().contains(Zone::Top) {
            v.top = v.top.max(self.overlay.top.bottom).min(v.bottom);
            // --- layout (thread T6) --- rows along the bottom edge float over the page's bottom.
            if !self.overlay.bottom.is_empty() {
                v.bottom = v.bottom.min(self.overlay.bottom.top).max(v.top);
            }
        }
        if self.plan.sidebar && self.machine.shown().contains(Zone::Sidebar) {
            // --- layout (thread T6) --- on whichever side the sidebar is docked.
            if self.overlay.sidebar.left <= v.left {
                v.left = v.left.max(self.overlay.sidebar.right).min(v.right);
            } else {
                v.right = v.right.min(self.overlay.sidebar.left).max(v.left);
            }
        }
        v
    }

    /// The pointer is still on something a shown bar occupies, with a margin around it.
    fn covers(&self, x: i32, y: i32, margin: i32) -> bool {
        let grow = |r: LRect| LRect::new(r.left - margin, r.top - margin, r.right + margin, r.bottom + margin);
        let shown = self.machine.shown();
        let mut inside = false;
        if shown.contains(Zone::Top) {
            let mut area = self.overlay.top;
            if rules::bookmarks_ride_with_top(self.plan, self.bar_bottom) {
                area = area.union(self.bar_float);
            }
            inside |= grow(area).contains(x, y);
            // --- layout (thread T6) --- the rows along the bottom edge reveal with the top chrome.
            if !self.overlay.bottom.is_empty() {
                inside |= grow(self.overlay.bottom).contains(x, y);
            }
        }
        if shown.contains(Zone::Sidebar) {
            inside |= grow(self.overlay.sidebar).contains(x, y);
        }
        if shown.contains(Zone::Bookmarks) {
            inside |= grow(self.bar_float).contains(x, y);
        }
        inside
    }

    /// Something keeps the bars up whatever the pointer does.
    fn held(&self, hwnd: HWND, s: &WindowChrome) -> bool {
        s.html_omnibox_focused
            || s.focused
            || s.drag.is_some()
            || s.vertical_drag.is_some()
            || s.split_dragging
            || !s.foreground
            || super::html_popup::is_active(hwnd)
            || super::super::menu::is_tracking()
            || super::super::island_menu::is_open()
    }
}

// ----- state shared outside the window's borrow -----

/// The window's full-screen state. Read from places that cannot borrow the window's chrome (layout
/// maths run inside such borrows), so it lives in a window property.
pub(crate) fn fullscreen_of(hwnd: HWND) -> Fullscreen {
    let bits = unsafe { GetPropW(hwnd, PROP_FULLSCREEN) }.0 as usize;
    if bits & 2 != 0 {
        Fullscreen::Page
    } else if bits & 1 != 0 {
        Fullscreen::Window
    } else {
        Fullscreen::No
    }
}

fn fullscreen_bits(hwnd: HWND) -> usize {
    unsafe { GetPropW(hwnd, PROP_FULLSCREEN) }.0 as usize
}

fn set_fullscreen_bits(hwnd: HWND, bits: usize) {
    unsafe {
        if bits == 0 {
            let _ = RemovePropW(hwnd, PROP_FULLSCREEN);
        } else {
            let _ = SetPropW(hwnd, PROP_FULLSCREEN, Some(HANDLE(bits as *mut _)));
        }
    }
}

/// Whether the window is maximised or full screen: both have no resize rim and no window frame.
pub(crate) fn zoomed_or_full(hwnd: HWND) -> bool {
    let zoomed = unsafe { windows::Win32::UI::WindowsAndMessaging::IsZoomed(hwnd).as_bool() };
    zoomed || fullscreen_bits(hwnd) != 0
}

/// An open side panel floats over the page instead of taking room from it (Settings > Auto-hide > Bars to hide).
pub(crate) fn panel_floats(hwnd: HWND) -> bool {
    !unsafe { GetPropW(hwnd, PROP_PANEL_FLOATS) }.0.is_null()
}

fn set_panel_floats(hwnd: HWND, on: bool) {
    unsafe {
        if on {
            let _ = SetPropW(hwnd, PROP_PANEL_FLOATS, Some(HANDLE(std::ptr::dangling_mut())));
        } else {
            let _ = RemovePropW(hwnd, PROP_PANEL_FLOATS);
        }
    }
}

/// The bars that auto-hide now, from the app's settings and the window's full-screen state. The same
/// answer the app uses to size a page before the window has laid itself out.
pub(crate) fn effective_for(hwnd: HWND, settings: &crate::settings::Settings) -> rules::Effective {
    rules::effective(settings.frameless, &settings.auto_hide, fullscreen_of(hwnd))
}

// ----- settings and layout -----

/// Takes the settings and the full-screen state into the window: which bars are out of the layout,
/// whether they may come back, the solid palette. Lays the window out again when any of it changed.
/// Called at window creation, after every settings change and every full-screen change.
pub(super) fn sync_config(hwnd: HWND, app: &AppRef) {
    let settings = crate::app::App::settings(app);
    let tab_layout = settings.tab_layout;
    let effective = effective_for(hwnd, &settings);
    let plan = rules::plan_for(effective.hidden, tab_layout, settings.tab_bar_visible); // A hidden tab bar hides nothing
    set_panel_floats(hwnd, plan.panel);
    let was_solid = solid();
    let now_solid = !effective.hidden.is_empty();
    SOLID.store(now_solid, Ordering::Relaxed);
    let changed = with_chrome(hwnd, |s| {
        let before = (s.ah.hidden, s.ah.reveal_allowed);
        s.ah.hidden = effective.hidden;
        s.ah.reveal_allowed = effective.reveal;
        if !effective.reveal || effective.hidden.is_empty() {
            s.ah.machine.reset();
        }
        if was_solid != now_solid {
            s.palette = chrome_palette();
            s.html_last_state = None;
            s.html_tabs_last_state = None;
            s.html_rail_last_state = None;
        }
        before != (s.ah.hidden, s.ah.reveal_allowed) || was_solid != now_solid
    })
    .unwrap_or(false);
    if !changed {
        return;
    }
    relayout_all(hwnd, app);
    ensure_timer(hwnd);
}

/// Lays the window out again with the current bars and puts every view and page where it goes.
fn relayout_all(hwnd: HWND, app: &AppRef) {
    let relaid = with_chrome(hwnd, |s| s.relayout_sidebar(App_sidebar_width(app))).unwrap_or(false);
    if relaid {
        sync_html_host_bounds(hwnd);
        apply_sidebar_bounds(hwnd, app);
        repaint(hwnd);
    }
}

#[allow(non_snake_case)]
fn App_sidebar_width(app: &AppRef) -> u16 {
    crate::app::App::sidebar_width(app)
}

/// Where the bookmarks bar goes while auto-hide has it out of the layout: its floating rectangle and
/// whether it is revealed. `None` when auto-hide does not hide it (the bar sits in the layout as usual).
pub(super) fn bookmark_bar_placement(hwnd: HWND) -> Option<(RECT, bool)> {
    with_chrome(hwnd, |s| {
        (s.ah.plan.bookmarks && !s.ah.bar_float.is_empty()).then(|| {
            let r = s.ah.bar_float;
            (RECT { left: r.left, top: r.top, right: r.right, bottom: r.bottom }, s.ah.bookmarks_revealed())
        })
    })
    .flatten()
}

// ----- reveal and hide -----

/// The pointer rested on a sensor, left one, or a page window was created and everything must go back
/// on top. `wparam`/`lparam` are the sensor window's message ([`WM_AUTOHIDE_SENSOR`]).
pub(super) fn on_sensor(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    if wparam.0 == RAISE {
        raise_all(hwnd);
        return;
    }
    let Some(zone) = zone_from_code(wparam.0) else { return };
    let started = with_chrome(hwnd, |s| {
        if !s.ah.reveal_allowed {
            return false;
        }
        if lparam.0 != 0 {
            s.ah.machine.enter_sensor(zone, now_ms());
        } else {
            s.ah.machine.leave_sensor(zone);
        }
        true
    })
    .unwrap_or(false);
    if started {
        ensure_timer(hwnd);
    }
}

/// The 50 ms tick: completes a reveal, and hides the bars once the pointer has been away long enough.
pub(super) fn on_timer(hwnd: HWND) {
    let mut p = POINT::default();
    let have_pointer = unsafe { GetCursorPos(&mut p) }.is_ok() && unsafe { ScreenToClient(hwnd, &mut p) }.as_bool();
    let changed = with_chrome(hwnd, |s| {
        let margin = Metrics::new(s.dpi).px(rules::HIDE_MARGIN_DIP);
        let tick = Tick {
            pointer_over_bars: have_pointer && s.ah.covers(p.x, p.y, margin),
            held: s.ah.held(hwnd, s),
        };
        s.ah.ticks = s.ah.ticks.wrapping_add(1);
        s.ah.machine.tick(now_ms(), tick)
    })
    .unwrap_or(false);
    if changed {
        apply_shown(hwnd);
    } else if with_chrome(hwnd, |s| !s.ah.machine.shown().is_empty() && s.ah.ticks % 4 == 0).unwrap_or(false) {
        raise_all(hwnd); // a page created while a bar is up starts above it
    }
    ensure_timer(hwnd);
}

/// Starts or stops the tick: it runs only while a reveal is pending or bars are shown.
pub(super) fn ensure_timer(hwnd: HWND) {
    let want = with_chrome(hwnd, |s| s.ah.machine.active()).unwrap_or(false);
    let have = with_chrome(hwnd, |s| std::mem::replace(&mut s.ah.timer, want)).unwrap_or(false);
    if want && !have {
        unsafe {
            let _ = SetTimer(Some(hwnd), TIMER_ID, rules::POLL_MS, None);
        }
    } else if !want && have {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(hwnd), TIMER_ID);
        }
    }
}

/// Brings every hidden bar back now, for the keyboard (Ctrl+L, Ctrl+T, ...): no delay, and they stay
/// while the address bar has focus. Call it before focusing anything in a hidden bar.
pub(super) fn reveal_for_keyboard(hwnd: HWND) {
    let changed = with_chrome(hwnd, |s| {
        if !s.ah.reveal_allowed || s.ah.hidden.is_empty() {
            return None;
        }
        let present = rules::zones(s.ah.plan, !s.ah.bar_float.is_empty(), s.ah.bar_bottom);
        Some(s.ah.machine.reveal(present, now_ms()))
    })
    .flatten();
    if changed == Some(true) {
        apply_shown(hwnd);
    }
    if changed.is_some() {
        ensure_timer(hwnd);
    }
}

/// Puts the views, the sensors and the painting in line with which zones are shown now.
fn apply_shown(hwnd: HWND) {
    let _ = with_chrome(hwnd, |s| {
        s.sync_html_bounds();
        s.sync_html_tabs_bounds();
    });
    sync_html_host_bounds(hwnd);
    raise_all(hwnd);
    repaint(hwnd);
}

/// Raises the views of the bars that are shown, then the sensors of the ones that are not, above the
/// page windows (which are created later and so start on top). Cheap: a few `SetWindowPos` calls.
fn raise_all(hwnd: HWND) {
    let hosts = with_chrome(hwnd, |s| {
        if s.ah.hidden.is_empty() {
            return None;
        }
        Some((s.html.clone().filter(|_| !s.ah.top_hidden()), s.html_tabs.clone().filter(|_| !(s.ah.top_hidden() && s.sidebar.is_none()) && !(s.sidebar.is_some() && s.ah.sidebar_hidden()))))
    })
    .flatten();
    let Some((toolbar, tabs)) = hosts else {
        sync_sensors(hwnd);
        return;
    };
    if let Some(host) = toolbar {
        host.raise_to_top();
    }
    if let Some(host) = tabs {
        host.raise_to_top();
    }
    sync_sensors(hwnd);
}

/// Called at the end of every layout: places the sensors and raises what is shown. Does nothing, and
/// creates nothing, while auto-hide is off.
pub(super) fn after_layout(hwnd: HWND) {
    let idle = with_chrome(hwnd, |s| s.ah.hidden.is_empty() && s.ah.sensors.is_empty()).unwrap_or(true);
    if !idle {
        raise_all(hwnd);
    }
}

// ----- sensors -----

fn zone_code(zone: Zone) -> usize {
    match zone {
        Zone::Top => 1,
        Zone::Sidebar => 2,
        Zone::Bookmarks => 3,
    }
}

fn zone_from_code(code: usize) -> Option<Zone> {
    match code {
        1 => Some(Zone::Top),
        2 => Some(Zone::Sidebar),
        3 => Some(Zone::Bookmarks),
        _ => None,
    }
}

/// Puts one strip of the page at the edge of each hidden bar that is not shown, and takes down the
/// rest. The strips are the only windows auto-hide adds, made when first needed (after first paint,
/// never at launch) and kept.
fn sync_sensors(hwnd: HWND) {
    let want = with_chrome(hwnd, |s| {
        if !s.ah.reveal_allowed || s.ah.hidden.is_empty() {
            return (Zones::NONE, LRect::default(), LRect::default(), false, 96, false, false);
        }
        let present = rules::zones(s.ah.plan, !s.ah.bar_float.is_empty(), s.ah.bar_bottom);
        let mut waiting = Zones::NONE;
        for zone in [Zone::Top, Zone::Sidebar, Zone::Bookmarks] {
            if present.contains(zone) && !s.ah.machine.shown().contains(zone) {
                waiting = waiting.with(zone);
            }
        }
        // A right sidebar comes back from the right edge.
        let sidebar_right = s.sidebar.is_some_and(|sidebar| sidebar.side == crate::settings::SidePanelPosition::Right);
        (waiting, s.ah.page, s.ah.bar_float, s.ah.bar_bottom, s.dpi, true, sidebar_right)
    })
    .unwrap_or((Zones::NONE, LRect::default(), LRect::default(), false, 96, false, false));
    let (zones, page, bar, bottom, dpi_now, active, sidebar_right) = want;
    if !active && with_chrome(hwnd, |s| s.ah.sensors.is_empty()).unwrap_or(true) {
        return;
    }
    // A strip at a screen edge with no monitor beyond it is one pixel; anywhere else 4 DIP.
    let thin_top = zoomed_or_full(hwnd) && no_monitor_above(hwnd, page);
    let strips = rules::sensors_sided(zones, page, bar, bottom, dpi_now, thin_top, false, sidebar_right);
    // Hide every existing sensor whose zone is not wanted now.
    let existing = with_chrome(hwnd, |s| s.ah.sensors.clone()).unwrap_or_default();
    for (zone, window) in &existing {
        if !strips.iter().any(|strip| strip.zone == *zone) {
            unsafe {
                let _ = ShowWindow(*window, SW_HIDE);
            }
        }
    }
    for strip in strips {
        let Some(window) = sensor_window(hwnd, strip.zone) else { continue };
        let r = strip.rect;
        unsafe {
            let _ = SetWindowPos(
                window,
                Some(HWND_TOP),
                r.left,
                r.top,
                r.width().max(1),
                r.height().max(1),
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }
}

fn no_monitor_above(hwnd: HWND, page: LRect) -> bool {
    let mut p = POINT { x: (page.left + page.right) / 2, y: 0 };
    if !unsafe { windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut p) }.as_bool() {
        return false;
    }
    p.y -= 1;
    unsafe { MonitorFromPoint(p, MONITOR_DEFAULTTONULL) }.is_invalid()
}

fn sensor_window(owner: HWND, zone: Zone) -> Option<HWND> {
    if let Some(found) = with_chrome(owner, |s| s.ah.sensors.iter().find(|(z, _)| *z == zone).map(|(_, w)| *w)).flatten() {
        return Some(found);
    }
    if !register_sensor_class() {
        return None;
    }
    let instance = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }.ok().map(|m| m.into());
    let window = unsafe {
        CreateWindowExW(Default::default(), SENSOR_CLASS, w!(""), WS_CHILD | WS_CLIPSIBLINGS, 0, 0, 1, 1, Some(owner), None, instance, None)
    }
    .ok()?;
    unsafe {
        SetWindowLongPtrW(window, GWLP_USERDATA, zone_code(zone) as isize);
    }
    let _ = with_chrome(owner, |s| s.ah.sensors.push((zone, window)));
    Some(window)
}

fn register_sensor_class() -> bool {
    thread_local! { static DONE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) }; }
    if let Some(done) = DONE.with(|d| d.get()) {
        return done;
    }
    let instance = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }.map(|m| m.into()).unwrap_or_default();
    let class = WNDCLASSW { style: CS_DBLCLKS, lpfnWndProc: Some(sensor_proc), hInstance: instance, lpszClassName: SENSOR_CLASS, ..Default::default() };
    let ok = unsafe { RegisterClassW(&class) } != 0;
    DONE.with(|d| d.set(Some(ok)));
    ok
}

/// A sensor window: invisible, never takes focus or activation, and tells its parent when the pointer
/// rests on it. Bit 8 of its user data remembers that the pointer is inside, so a move posts once.
unsafe extern "system" fn sensor_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let data = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        let zone = (data & 0xFF) as usize;
        let post = |entered: bool| {
            if let Ok(parent) = GetParent(hwnd) {
                let _ = PostMessageW(Some(parent), WM_AUTOHIDE_SENSOR, WPARAM(zone), LPARAM(entered as isize));
            }
        };
        match msg {
            WM_MOUSEMOVE => {
                if data & 0x100 == 0 {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, data | 0x100);
                    let mut track = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut track);
                    post(true);
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, data & !0x100);
                post(false);
                LRESULT(0)
            }
            WM_MOUSEACTIVATE => LRESULT(3), // MA_NOACTIVATE
            WM_SETCURSOR => {
                if let Ok(arrow) = LoadCursorW(None, IDC_ARROW) {
                    SetCursor(Some(arrow));
                }
                LRESULT(1)
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

/// Asks the window to put the sensors and shown bars above a page window that was just created.
pub(crate) fn page_created(hwnd: HWND) {
    super::super::post(hwnd, WM_AUTOHIDE_SENSOR, WPARAM(RAISE), LPARAM(0));
}

// ----- full screen -----

/// What the window was before F11, to put back on exit.
#[derive(Clone, Copy)]
struct Saved {
    placement: WINDOWPLACEMENT,
    style: isize,
}

thread_local! {
    static SAVED: RefCell<HashMap<isize, Saved>> = RefCell::new(HashMap::new());
}

/// F11: the window fills its monitor with no frame, or goes back to what it was.
pub(crate) fn toggle_fullscreen(hwnd: HWND, app: &AppRef) {
    if fullscreen_bits(hwnd) & 1 != 0 {
        leave_window(hwnd, app, false);
    } else {
        enter_window(hwnd, app, false);
    }
}

/// A page element (a video) went full screen or came back. The window fills its monitor too, every
/// bar hides and nothing reveals them; Esc, which the page handles, ends it.
pub(crate) fn page_fullscreen(hwnd: HWND, app: &AppRef, on: bool, source: Option<&url::Url>) {
    let bits = fullscreen_bits(hwnd);
    // Repeated events must not overwrite the saved pre-fullscreen window placement.
    if on == (bits & 2 != 0) { return; }
    if on {
        let by_page = bits & 1 == 0;
        // Bit 2 is the page; bit 4 remembers that the window went full screen only for it.
        set_fullscreen_bits(hwnd, bits | 2 | if by_page { 4 } else { 0 });
        if by_page {
            enter_window(hwnd, app, true);
        } else {
            sync_config(hwnd, app);
        }
        // Browser-owned, persistent identity/exit surface. If it cannot be displayed, keep
        // browser chrome visible rather than enter an unlabelled page full-screen state.
        if !super::fullscreen_notice::show(hwnd, app, source) {
            page_fullscreen(hwnd, app, false, None);
        }
    } else {
        super::fullscreen_notice::hide(hwnd);
        let had_window_for_page = bits & 4 != 0;
        set_fullscreen_bits(hwnd, bits & !2 & !4);
        if had_window_for_page {
            leave_window(hwnd, app, true);
        } else {
            sync_config(hwnd, app);
        }
    }
}

fn enter_window(hwnd: HWND, app: &AppRef, for_page: bool) {
    unsafe {
        let mut placement = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
        if GetWindowPlacement(hwnd, &mut placement).is_err() {
            return;
        }
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if monitor.is_invalid() || !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        SAVED.with(|saved| saved.borrow_mut().insert(hwnd.0 as isize, Saved { placement, style }));
        if !for_page {
            set_fullscreen_bits(hwnd, fullscreen_bits(hwnd) | 1);
        }
        // Restored first: a maximised window is laid out from its work area, not the monitor.
        if windows::Win32::UI::WindowsAndMessaging::IsZoomed(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        SetWindowLongPtrW(hwnd, GWL_STYLE, style & !((WS_CAPTION.0 | WS_THICKFRAME.0) as isize));
        sync_config(hwnd, app);
        let r = info.rcMonitor;
        let _ = SetWindowPos(hwnd, Some(HWND_TOP), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_FRAMECHANGED | SWP_NOACTIVATE);
    }
}

fn leave_window(hwnd: HWND, app: &AppRef, for_page: bool) {
    let saved = SAVED.with(|saved| saved.borrow_mut().remove(&(hwnd.0 as isize)));
    unsafe {
        if let Some(saved) = saved {
            SetWindowLongPtrW(hwnd, GWL_STYLE, saved.style);
        }
        let bits = fullscreen_bits(hwnd) & !1;
        set_fullscreen_bits(hwnd, if for_page { bits & !4 } else { bits });
        sync_config(hwnd, app);
        if let Some(saved) = saved {
            let _ = SetWindowPlacement(hwnd, &saved.placement);
        }
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }
}

/// The window is going away: forget what it saved.
pub(crate) fn forget(hwnd: HWND) {
    super::fullscreen_notice::hide(hwnd);
    SAVED.with(|saved| saved.borrow_mut().remove(&(hwnd.0 as isize)));
}
