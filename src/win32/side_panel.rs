//! Rail panel host. A row of tabs docks it on the saved `side_panel_position`; a visible vertical
//! tab bar puts it on the opposite edge ([`SidePanelPosition::placed`]). In frameless mode the open
//! panel keeps its width and slides over the page. The host window is stable while content changes.

use crate::{
    settings::{SidePanelPosition, TabLayout},
    theme,
    win32::glass,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::c_void,
    rc::Rc,
    time::Instant,
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, InvalidateRect, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY, TRANSPARENT},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{GetFocus, GetKeyState, ReleaseCapture, SetCapture, SetFocus},
            WindowsAndMessaging::{
                CREATESTRUCTW, CreateWindowExW, DefWindowProcW, GWLP_USERDATA,
                GetClientRect, GetWindowLongPtrW, IDC_ARROW, IDC_SIZEWE, LoadCursorW,
                RegisterClassW, SetCursor, SetWindowLongPtrW, SetWindowPos, ShowWindow, HWND_TOP, SW_HIDE,
                SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_CHAR, WM_ERASEBKGND,
                WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE,
                WM_NCDESTROY, WM_PAINT, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN,
            },
        },
    },
    core::{PCWSTR, w},
};

const CLASS: PCWSTR = w!("BowserSidePanelHost");
pub const TIMER_ID: usize = 0xB057;
pub const WM_RESIZED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x78;
const ANIMATION_MS: f32 = 220.0;
const FRAME_MS: u32 = 16;
const MIN_WIDTH_DIP: i32 = 280;
const MAX_WIDTH_DIP: i32 = 520;
const DEFAULT_WIDTH_DIP: i32 = 340;
const HEADER_DIP: i32 = 48;
const RESIZE_DIP: i32 = 6;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PanelId { Bookmarks, History, Customize, Menu, WebApp(u32) }

impl PanelId {
    fn title(self) -> String {
        match self { Self::Bookmarks => "Bookmarks".into(), Self::History => "History".into(), Self::Customize => "Customize".into(), Self::Menu => "Menu".into(), Self::WebApp(id) => format!("Web app {id}") }
    }
}

/// Paints inside the stable host. A returned native child is positioned within the content area.
pub trait PanelContent {
    /// The stable host HWND, available before this content becomes visible.
    fn on_host_created(&mut self, _host: HWND) {}
    /// Physical-pixel content bounds, refreshed on every host size/animation update.
    fn on_bounds_changed(&mut self, _bounds: RECT) {}
    fn paint(&mut self, _hdc: windows::Win32::Graphics::Gdi::HDC, _rect: RECT, _palette: &theme::Palette) {}
    fn on_mouse_move(&mut self, _x: i32, _y: i32) {}
    fn on_mouse_down(&mut self, _x: i32, _y: i32) {}
    fn on_mouse_up(&mut self, _x: i32, _y: i32) {}
    fn on_mouse_wheel(&mut self, _delta: i16) {}
    fn on_char(&mut self, _ch: char) {}
    fn on_key(&mut self, _key: u32, _shift: bool, _ctrl: bool) {}
    fn on_focus(&mut self, _focused: bool) {}
    // --- panel host (lane P) ---
    /// Called after the native side-panel host takes focus, so embedded views can receive it too.
    fn after_shown(&mut self) {}
    // --- end panel host ---
    fn min_width(&self) -> i32 { MIN_WIDTH_DIP }
    fn preferred_width(&self) -> i32 { DEFAULT_WIDTH_DIP }
    fn title(&self) -> &str;
    /// Whether the host paints its own title and close button above the content. HTML panels draw
    /// their own header, so a second native one would double it (V23).
    fn native_header(&self) -> bool { true }
    fn wants_native_child(&self) -> Option<HWND> { None }
}

pub type ContentFactory = Rc<dyn Fn() -> Box<dyn PanelContent>>;
thread_local! {
    static FACTORIES: RefCell<HashMap<PanelId, ContentFactory>> = RefCell::new(HashMap::new());
    static HOSTS: RefCell<HashMap<isize, isize>> = RefCell::new(HashMap::new());
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
    static GUEST_WIDTH: Cell<i32> = const { Cell::new(DEFAULT_WIDTH_DIP) };
    static STATUS: Cell<Option<&'static str>> = const { Cell::new(None) };
}

pub fn register_content(id: PanelId, factory: impl Fn() -> Box<dyn PanelContent> + 'static) {
    FACTORIES.with(|map| { map.borrow_mut().insert(id, Rc::new(factory)); });
}
pub fn unregister_content(id: PanelId) {
    FACTORIES.with(|map| { map.borrow_mut().remove(&id); });
}

pub fn close_content(owner: HWND, id: PanelId) {
    let active = HOSTS.with(|hosts| unsafe {
        hosts.borrow().get(&(owner.0 as isize)).copied().is_some_and(|raw| host(HWND(raw as *mut c_void)).is_some_and(|state| state.id == Some(id)))
    });
    if active { close(owner); }
}
pub fn last_status() -> Option<&'static str> { STATUS.with(Cell::get) }

#[cfg(test)]
#[derive(Default)]
struct PanelState {
    open: Option<PanelId>,
    host_generation: u8,
    host_created: bool,
}
#[cfg(test)]
impl PanelState {
    fn toggle(&mut self, id: PanelId) { if self.open == Some(id) { self.close(); } else { if !self.host_created { self.host_created = true; self.host_generation += 1; } self.open = Some(id); } }
    fn close(&mut self) { self.open = None; }
    fn escape(&mut self) -> bool { if self.open.is_some() { self.close(); true } else { false } }
}

struct Animation {
    from: i32,
    to: i32,
    started: Instant,
    /// `from`/`to` are x offsets from the resting edge when true, host widths when false.
    slide: bool,
}
pub struct SidePanelHost {
    owner: HWND,
    previous_focus: HWND,
    id: Option<PanelId>,
    content: Option<Box<dyn PanelContent>>,
    retired: Vec<Box<dyn PanelContent>>,
    retired_blur: Vec<bool>,
    width_dip: i32,
    width: i32,
    position: SidePanelPosition,
    /// Pixel shift from the resting edge while a frameless panel slides. `0` is at rest.
    offset: i32,
    /// The panel covers the page (frameless, or the panel bar is hidden) instead of narrowing it.
    floats: bool,
    client: RECT,
    resizing: bool,
    animation: Option<Animation>,
    /// The current content wants the native title row and close button.
    header: bool,
    edit_brush: windows::Win32::Graphics::Gdi::HBRUSH,
}

pub fn is_open(owner: HWND) -> bool {
    HOSTS.with(|hosts| unsafe { hosts.borrow().get(&(owner.0 as isize)).copied().is_some_and(|raw| host(HWND(raw as *mut c_void)).is_some_and(|h| h.id.is_some())) })
}

pub fn active_id(owner: HWND) -> Option<PanelId> {
    HOSTS.with(|hosts| unsafe {
        let raw = hosts.borrow().get(&(owner.0 as isize)).copied()?;
        host(HWND(raw as *mut c_void))?.id
    })
}

pub fn active_width_px(owner: HWND) -> i32 {
    HOSTS.with(|hosts| unsafe { hosts.borrow().get(&(owner.0 as isize)).copied().map_or(0, |raw| host(HWND(raw as *mut c_void)).map_or(0, |h| h.width)) })
}

pub fn active_position(owner: HWND) -> SidePanelPosition {
    HOSTS.with(|hosts| unsafe { hosts.borrow().get(&(owner.0 as isize)).copied().map_or(SidePanelPosition::Left, |raw| host(HWND(raw as *mut c_void)).map_or(SidePanelPosition::Left, |h| h.position)) })
}

pub fn open(owner: HWND, id: PanelId) -> bool {
    STATUS.with(|status| status.set(None));
    let Some(factory) = FACTORIES.with(|map| map.borrow().get(&id).cloned()) else {
        STATUS.with(|status| status.set(Some("This side panel has no registered content yet.")));
        return false;
    };
    let existing = HOSTS.with(|hosts| hosts.borrow().get(&(owner.0 as isize)).copied().unwrap_or(0));
    if existing != 0 {
        let hwnd = HWND(existing as *mut c_void);
        if let Some(state) = unsafe { host(hwnd) } {
            let content = factory();
            let mut content = content;
            content.on_host_created(hwnd);
            state.previous_focus = unsafe { GetFocus() };
            if let Some(previous) = state.content.as_mut() { previous.on_focus(false); }
            state.header = content.native_header();
            // --- window and panels (thread T9) --- a web panel keeps its own width; leaving one goes back to the shared width.
            switch_panel(&mut state.id, &mut state.width_dip, id, content.min_width(), panel_width_choice);
            // --- end window and panels ---
            if let Some(previous) = state.content.take() { state.retired.push(previous); state.retired_blur.push(false); }
            state.content = Some(content);
            if let Some(current) = state.content.as_mut() { current.on_focus(true); }
            let target = desired_width(state.width_dip, owner);
            state.floats = super::window::side_panel_floats(owner);
            let arm = if state.floats {
                state.width = target;
                begin_motion(state, true, state.offset, 0)
            } else {
                state.offset = 0;
                begin_motion(state, false, state.width, target)
            };
            raise(hwnd);
            unsafe { let _ = ShowWindow(hwnd, SW_SHOW); let _ = SetFocus(Some(hwnd)); }
            // --- panel host (lane P) ---
            if let Some(current) = state.content.as_mut() { current.after_shown(); }
            // --- end panel host ---
            sync(owner, base_viewport(owner), dpi(owner));
            invalidate(hwnd);
            release_motion(owner, hwnd, state, arm);
            return true;
        }
    }
    if !REGISTERED.with(Cell::get) && !register_class() { return false; }
    // The page area below the toolbar, not the whole client: a host created at y=0 painted its
    // title and wash over the toolbar, and left them there (V5).
    let client = super::window::page_viewport(owner).unwrap_or_else(|| base_viewport(owner));
    let inset = crate::chrome::layout::left_rail_inset(dpi(owner));
    let mut content = factory();
    let saved_width = crate::profile::active().ok().map(|profile| shared_width(&profile.settings_file())).unwrap_or_else(|| GUEST_WIDTH.with(Cell::get));
    let preferred = if saved_width == crate::settings::DEFAULT_SIDE_PANEL_WIDTH as i32 { content.preferred_width() } else { saved_width };
    let minimum = content.min_width().clamp(MIN_WIDTH_DIP, MAX_WIDTH_DIP);
    let width_dip = preferred.clamp(minimum, MAX_WIDTH_DIP);
    let width_dip = panel_width_choice(None, id).map_or(width_dip, |own| own.clamp(minimum, MAX_WIDTH_DIP)); // --- window and panels (thread T9) ---
    let Some(target_width) = panel_width_px(width_dip, dpi(owner), client.right-inset) else {
        STATUS.with(|status| status.set(Some("This window is too narrow to open a side panel while keeping the page readable.")));
        return false;
    };
    content.on_focus(true);
    let header = content.native_header();
    let position = placed_position();
    let floats = super::window::side_panel_floats(owner);
    let offset = if floats { hidden_offset(target_width, position) } else { 0 };
    let state = Box::new(SidePanelHost { owner, previous_focus: unsafe { GetFocus() }, id: Some(id), content: Some(content), retired: Vec::new(), retired_blur: Vec::new(), width_dip, width: if floats { target_width } else { 0 }, position, offset, floats, client, resizing: false, animation: None, header, edit_brush: unsafe { CreateSolidBrush(colorref(theme::palette(glass::appearance(), true).omnibox_fill)) } });
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None).unwrap_or_default().0 });
    let hwnd = unsafe { CreateWindowExW(Default::default(), CLASS, w!(""), WS_CHILD | WS_CLIPCHILDREN, panel_left(client, inset, 0, position), client.top, 1, client.bottom-client.top, Some(owner), None, Some(instance), Some(Box::into_raw(state).cast())) };
    let Ok(hwnd) = hwnd else { return false };
    HOSTS.with(|hosts| { hosts.borrow_mut().insert(owner.0 as isize, hwnd.0 as isize); });
    let arm = if let Some(state) = unsafe { host(hwnd) } {
        if let Some(content) = state.content.as_mut() { content.on_host_created(hwnd); }
        if state.floats { begin_motion(state, true, state.offset, 0) } else { begin_motion(state, false, state.width, target_width) }
    } else {
        false
    };
    // Place the first frame before the host is shown, so a frameless panel does not flash at the
    // resting edge and then jump off it.
    sync(owner, client, dpi(owner));
    raise(hwnd);
    unsafe { let _ = ShowWindow(hwnd, SW_SHOW); let _ = SetFocus(Some(hwnd)); }
    // --- panel host (lane P) ---
    if let Some(current) = unsafe { host(hwnd) }.and_then(|state| state.content.as_mut()) { current.after_shown(); }
    // --- end panel host ---
    if let Some(state) = unsafe { host(hwnd) } { release_motion(owner, hwnd, state, arm); }
    true
}

pub fn close(owner: HWND) {
    HOSTS.with(|hosts| {
        let raw = hosts.borrow().get(&(owner.0 as isize)).copied().unwrap_or(0);
        let hwnd = HWND(raw as *mut c_void);
        if let Some(state) = unsafe { host(hwnd) }.filter(|h| h.owner == owner) {
            state.id = None;
            if let Some(content) = state.content.take() { state.retired.push(content); state.retired_blur.push(true); }
            state.floats = super::window::side_panel_floats(owner);
            let arm = if state.floats {
                begin_motion(state, true, state.offset, hidden_offset(state.width, state.position))
            } else {
                begin_motion(state, false, state.width, 0)
            };
            let target = if state.previous_focus.0.is_null() { owner } else { state.previous_focus };
            unsafe { let _ = SetFocus(Some(target)); }
            release_motion(owner, hwnd, state, arm);
            invalidate(hwnd);
        }
    });
}

pub fn toggle(owner: HWND, id: PanelId) -> bool {
    STATUS.with(|status| status.set(None));
    let opened = HOSTS.with(|hosts| unsafe { hosts.borrow().get(&(owner.0 as isize)).copied().is_some_and(|raw| host(HWND(raw as *mut c_void)).is_some_and(|h| h.id == Some(id))) });
    if opened { close(owner); true } else { open(owner, id) }
}

pub fn sync(owner: HWND, viewport: RECT, window_dpi: u32) {
    HOSTS.with(|hosts| unsafe {
        let raw = hosts.borrow().get(&(owner.0 as isize)).copied().unwrap_or(0);
        let hwnd = HWND(raw as *mut c_void);
        let Some(state) = host(hwnd).filter(|h| h.owner == owner) else { return };
        let inset = crate::chrome::layout::left_rail_inset(window_dpi);
        let origin = panel_origin(viewport.left, inset);
        let floats = super::window::side_panel_floats(owner);
        if state.floats != floats {
            // A settings change snaps. The slide belongs to an open or a dismiss, not to the toggle.
            state.floats = floats;
            state.offset = 0;
            state.animation = None;
            if state.id.is_none() { state.width = 0; }
        }
        state.client = viewport;
        if state.id.is_some() {
            if let Some(width) = panel_width_px(state.width_dip, window_dpi, viewport.right-origin) {
                if state.floats {
                    let previous = state.width;
                    let started = state.animation.as_ref().map(|animation| (animation.slide, animation.to, animation.from));
                    state.width = width;
                    // The first sync can clamp the width the open animation was built for. Only a
                    // panel still parked fully off-screen follows that; a slide already underway does not.
                    if let Some((true, 0, from)) = started
                        && from == state.offset
                        && from.saturating_abs() == previous
                        && previous != width
                    {
                        let hidden = hidden_offset(width, state.position);
                        if let Some(animation) = &mut state.animation { animation.from = hidden; }
                        state.offset = hidden;
                    }
                } else {
                    state.offset = 0;
                    state.width = state.width.min(width);
                    if let Some(animation) = &mut state.animation {
                        if !animation.slide { animation.to = width; }
                    } else {
                        state.width = width;
                    }
                }
            } else {
                state.width = 0;
                state.offset = 0;
                state.id = None;
                if let Some(content) = state.content.take() { state.retired.push(content); state.retired_blur.push(true); }
                state.animation = None;
                arm_animation(owner);
                STATUS.with(|status| status.set(Some("The window is too narrow for a side panel and a readable page.")));
            }
        }
        place_panel(hwnd, state, window_dpi);
        if let Some(child) = state.content.as_ref().and_then(|c| c.wants_native_child()) {
            let header = scale(HEADER_DIP, window_dpi);
            let _ = SetWindowPos(child, None, scale(12, window_dpi), header, (state.width-scale(24, window_dpi)).max(0), (viewport.bottom-viewport.top-header).max(0), SWP_NOZORDER | SWP_NOACTIVATE);
        }
        let width = state.width.max(0);
        if let Some(content) = state.content.as_mut() {
            content.on_bounds_changed(RECT {
                left: 0,
                top: header_px(state.header, window_dpi),
                right: width,
                bottom: (viewport.bottom - viewport.top).max(0),
            });
        }
        if state.width == 0 && state.id.is_none() { let _ = ShowWindow(hwnd, SW_HIDE); }
    });
}

// --- layout (thread T6) ---
/// Moves an open docked panel to `position` at once (the settings page changed it). A panel opened
/// later reads the setting from the file as before.
pub fn set_position(owner: HWND, position: SidePanelPosition) {
    let moved = HOSTS.with(|hosts| unsafe {
        let raw = hosts.borrow().get(&(owner.0 as isize)).copied().unwrap_or(0);
        let hwnd = HWND(raw as *mut c_void);
        let Some(state) = host(hwnd).filter(|h| h.owner == owner) else { return false };
        if state.position == position { return false; }
        let old_hidden = hidden_offset(state.width, state.position);
        state.position = position;
        if state.floats {
            let new_hidden = hidden_offset(state.width, position);
            state.offset = mirrored_offset(state.offset, old_hidden, new_hidden);
            let offset = state.offset;
            let opening = state.id.is_some();
            if let Some(animation) = &mut state.animation
                && animation.slide
            {
                animation.from = offset;
                animation.to = if opening { 0 } else { new_hidden };
            }
        }
        true
    });
    if moved {
        sync(owner, base_viewport(owner), dpi(owner));
        let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::PostMessageW(Some(owner), WM_RESIZED, WPARAM(0), LPARAM(0)) };
    }
}
// --- end layout ---

pub fn adjust_viewport(viewport: RECT, owner: HWND) -> RECT {
    let width = active_width_px(owner);
    if width == 0 { return viewport; }
    reserved_viewport(viewport, crate::chrome::layout::left_rail_inset(dpi(owner)), width, active_position(owner))
}

/// The page area a panel of `width` leaves: left-docked, the panel starts at the rail inset or the
/// viewport edge (whichever is further right) and the page begins where the panel ends; right-docked,
/// the panel ends at the viewport's right edge and the page ends where the panel begins. Pure.
fn reserved_viewport(viewport: RECT, inset: i32, width: i32, position: SidePanelPosition) -> RECT {
    match position {
        SidePanelPosition::Left => RECT { left: panel_origin(viewport.left, inset) + width.max(0), ..viewport },
        SidePanelPosition::Right => RECT { right: viewport.right - width.max(0), ..viewport },
    }
}

/// Height of the native title row: none when the content draws its own header. Pure.
fn header_px(native_header: bool, dpi: u32) -> i32 { if native_header { scale(HEADER_DIP, dpi) } else { 0 } }

/// Puts the host above the tab page controllers, which are created later and so start on top.
fn raise(hwnd: HWND) {
    unsafe { let _ = SetWindowPos(hwnd, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE); }
}

pub fn timer(owner: HWND) -> bool {
    HOSTS.with(|hosts| unsafe {
        let raw = hosts.borrow().get(&(owner.0 as isize)).copied().unwrap_or(0);
        let hwnd = HWND(raw as *mut c_void);
        let Some(state) = host(hwnd).filter(|h| h.owner == owner) else { return false };
        let retired = std::mem::take(&mut state.retired);
        let retired_blur = std::mem::take(&mut state.retired_blur);
        for (mut content, blur) in retired.into_iter().zip(retired_blur) { if blur { content.on_focus(false); } }
        let Some((from, to, slide, started)) = state.animation.as_ref().map(|anim| (anim.from, anim.to, anim.slide, anim.started)) else { return !state.retired.is_empty() };
        let elapsed = Instant::now().saturating_duration_since(started).as_secs_f32() * 1000.0;
        // --- window and panels (thread T9) --- "Use animation" off: the panel snaps to its width.
        let t = if super::window::animations_enabled() { (elapsed / ANIMATION_MS).clamp(0.0, 1.0) } else { 1.0 };
        if slide { state.offset = slide_px(from, to, t); } else { state.width = animation_width(from, to, t); }
        if t >= 1.0 {
            if slide {
                state.offset = to;
                if state.id.is_none() { state.width = 0; state.offset = 0; }
            } else {
                state.width = to;
            }
            state.animation = None;
        }
        place_panel(hwnd, state, dpi(owner));
        if state.width == 0 && state.id.is_none() { let _ = ShowWindow(hwnd, SW_HIDE); }
        let _ = InvalidateRect(Some(hwnd), None, false);
        (state.animation.is_some() && animation_running(elapsed)) || !state.retired.is_empty()
    })
}

fn arm_animation(owner: HWND) { unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(owner), TIMER_ID, FRAME_MS, None); } }

/// Starts a docked width change (`slide` false) or a frameless edge slide. Returns false when the
/// panel snaps there, so the caller does not arm a timer: animation off, or nothing to move.
fn begin_motion(state: &mut SidePanelHost, slide: bool, from: i32, to: i32) -> bool {
    if !super::window::animations_enabled() || from == to {
        if slide {
            state.offset = to;
            if state.id.is_none() {
                state.width = 0;
                state.offset = 0;
            }
        } else {
            state.width = to;
            state.offset = 0;
        }
        state.animation = None;
        false
    } else {
        state.animation = Some(Animation { from, to, started: Instant::now(), slide });
        true
    }
}

fn flush_retired(state: &mut SidePanelHost) {
    let retired = std::mem::take(&mut state.retired);
    let blur = std::mem::take(&mut state.retired_blur);
    for (mut content, blur) in retired.into_iter().zip(blur) {
        if blur { content.on_focus(false); }
    }
}

/// Arms the frame timer, or settles at once when [`begin_motion`] snapped.
fn release_motion(owner: HWND, hwnd: HWND, state: &mut SidePanelHost, arm: bool) {
    if arm {
        arm_animation(owner);
        return;
    }
    flush_retired(state);
    if state.width == 0 && state.id.is_none() {
        unsafe { let _ = ShowWindow(hwnd, SW_HIDE); }
    }
    // A docked panel's width is the page's column. A floating one never changes it.
    if !state.floats {
        note_resized(owner);
    }
}

fn note_resized(owner: HWND) {
    let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::PostMessageW(Some(owner), WM_RESIZED, WPARAM(0), LPARAM(0)) };
}

fn place_panel(hwnd: HWND, state: &SidePanelHost, window_dpi: u32) {
    let inset = crate::chrome::layout::left_rail_inset(window_dpi);
    let offset = if state.floats { state.offset } else { 0 };
    let bounds = shifted_bounds(state.client, inset, state.width.max(0), state.position, offset);
    unsafe {
        let _ = SetWindowPos(hwnd, None, bounds.left, bounds.top, (bounds.right - bounds.left).max(0), (bounds.bottom - bounds.top).max(0), SWP_NOZORDER | SWP_NOACTIVATE);
    }
}
fn desired_width(width_dip: i32, owner: HWND) -> i32 {
    let mut rect = RECT::default(); unsafe { let _ = GetClientRect(owner, &mut rect); }
    let inset=crate::chrome::layout::left_rail_inset(dpi(owner));
    panel_width_px(width_dip, dpi(owner), (rect.right-rect.left-inset).max(0)).unwrap_or(0)
}
fn dpi(hwnd: HWND) -> u32 { unsafe { GetDpiForWindow(hwnd).max(96) } }
fn scale(value: i32, dpi: u32) -> i32 { value.saturating_mul(dpi as i32) / 96 }
fn base_viewport(owner: HWND) -> RECT { let mut r=RECT::default(); unsafe { let _=GetClientRect(owner,&mut r); } r }
fn invalidate(hwnd: HWND) { unsafe { let _=InvalidateRect(Some(hwnd),None,false); } }

fn register_class() -> bool {
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None).unwrap_or_default().0 });
    let class = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: instance, lpszClassName: CLASS, hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() }, ..Default::default() };
    let ok = unsafe { RegisterClassW(&class) != 0 };
    REGISTERED.with(|slot| slot.set(ok)); ok
}

unsafe fn host(hwnd: HWND) -> Option<&'static mut SidePanelHost> {
    if hwnd.0.is_null() { return None; }
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut SidePanelHost;
    unsafe { ptr.as_mut() }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { match msg {
        WM_NCCREATE => { let create=&*(lp.0 as *const CREATESTRUCTW); SetWindowLongPtrW(hwnd,GWLP_USERDATA,create.lpCreateParams as isize); DefWindowProcW(hwnd,msg,wp,lp) }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => { paint(hwnd); LRESULT(0) }
        WM_MOUSEMOVE => { if let Some(s)=host(hwnd) { let x=lp.0 as u16 as i16 as i32; let y=(lp.0>>16) as u16 as i16 as i32; if s.resizing { s.animation=None; s.offset=0; s.width=width_from_drag(s.owner,x,s.width,s.position); s.width_dip=(s.width.saturating_mul(96)/dpi(s.owner) as i32).clamp(MIN_WIDTH_DIP,MAX_WIDTH_DIP); place_panel(hwnd,s,dpi(s.owner)); let _=windows::Win32::UI::WindowsAndMessaging::PostMessageW(Some(s.owner),WM_RESIZED,WPARAM(0),LPARAM(0)); } else if over_grip(x,s.width,scale(RESIZE_DIP,dpi(hwnd)),s.position) { if let Ok(cursor)=LoadCursorW(None,IDC_SIZEWE) { let _=SetCursor(Some(cursor)); } } else if let Some(c)=s.content.as_mut() { c.on_mouse_move(x,y); } } LRESULT(0) }
        WM_LBUTTONDOWN => {
            let x=lp.0 as u16 as i16 as i32; let y=(lp.0>>16) as u16 as i16 as i32;
            let close_owner = if let Some(s)=host(hwnd) {
                if over_grip(x,s.width,scale(RESIZE_DIP,dpi(hwnd)),s.position) { s.resizing=true; let _=SetCapture(hwnd); None }
                else if s.header && y<scale(HEADER_DIP,dpi(hwnd)) && x>s.width-scale(36,dpi(hwnd)) { Some(s.owner) }
                else { if let Some(c)=s.content.as_mut() { c.on_mouse_down(x,y); } None }
            } else { None };
            if let Some(owner)=close_owner { close(owner); }
            LRESULT(0)
        }
        WM_LBUTTONUP => { if let Some(s)=host(hwnd) { let owner=s.owner; if s.resizing { s.resizing=false; let _=ReleaseCapture(); save_panel_width(s.id, owner, s.width_dip); } else if let Some(c)=s.content.as_mut() { c.on_mouse_up(lp.0 as u16 as i32,(lp.0>>16) as u16 as i32); } crate::win32::post(owner,crate::win32::WM_RAIL_CHANGED,WPARAM(0),LPARAM(0)); } LRESULT(0) }
        WM_MOUSEWHEEL => { if let Some(s)=host(hwnd) { if let Some(c)=s.content.as_mut() { c.on_mouse_wheel((wp.0>>16) as i16); } } LRESULT(0) }
        WM_CHAR => { if let Some(s)=host(hwnd) { let owner=s.owner; if let Some(c)=s.content.as_mut() { if let Some(ch)=char::from_u32(wp.0 as u32) { c.on_char(ch); } } crate::win32::post(owner,crate::win32::WM_RAIL_CHANGED,WPARAM(0),LPARAM(0)); } LRESULT(0) }
        WM_KEYDOWN => {
            if wp.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE.0 as usize {
                let owner=host(hwnd).map(|s|s.owner);
                if let Some(owner)=owner { close(owner); return LRESULT(0); }
            }
            if wp.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_TAB.0 as usize {
                let focus_target=host(hwnd).and_then(|s| s.content.as_ref().and_then(|c| c.wants_native_child()).map(|child| {
                    if GetKeyState(windows::Win32::UI::Input::KeyboardAndMouse::VK_SHIFT.0 as i32)<0 { s.owner } else { child }
                }));
                if let Some(target)=focus_target { let _=SetFocus(Some(target)); return LRESULT(0); }
            }
            if let Some(s)=host(hwnd) { let owner=s.owner; if let Some(c)=s.content.as_mut() { c.on_key(wp.0 as u32,GetKeyState(windows::Win32::UI::Input::KeyboardAndMouse::VK_SHIFT.0 as i32)<0,GetKeyState(windows::Win32::UI::Input::KeyboardAndMouse::VK_CONTROL.0 as i32)<0); } crate::win32::post(owner,crate::win32::WM_RAIL_CHANGED,WPARAM(0),LPARAM(0)); }
            DefWindowProcW(hwnd,msg,wp,lp)
        }
        WM_NCDESTROY => { if let Some(s)=host(hwnd) { let owner=s.owner.0 as isize; s.retired.clear(); s.retired_blur.clear(); let _=DeleteObject(s.edit_brush.into()); drop(Box::from_raw(s)); HOSTS.with(|hosts| { if hosts.borrow().get(&owner)==Some(&(hwnd.0 as isize)) { hosts.borrow_mut().remove(&owner); } }); } SetWindowLongPtrW(hwnd,GWLP_USERDATA,0); DefWindowProcW(hwnd,msg,wp,lp) }
        _ => DefWindowProcW(hwnd,msg,wp,lp),
    } }
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut ps=PAINTSTRUCT::default(); let dc=BeginPaint(hwnd,&mut ps); let Some(s)=host(hwnd) else { let _=EndPaint(hwnd,&ps); return; };
        let p = if s.floats { theme::palette(glass::appearance(), true) } else { glass::palette() }; let mut r=RECT::default(); let _=GetClientRect(hwnd,&mut r);
        let width = r.right - r.left;
        let height = r.bottom - r.top;
        let memory = CreateCompatibleDC(Some(dc));
        if memory.is_invalid() || width <= 0 || height <= 0 { let _=EndPaint(hwnd,&ps); return; }
        let info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: width, biHeight: -height, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
        let mut bits = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0) else { let _=DeleteDC(memory); let _=EndPaint(hwnd,&ps); return; };
        let previous = SelectObject(memory, bitmap.into());
        let brush=CreateSolidBrush(colorref(p.window)); let _=FillRect(memory,&r,brush); let _=DeleteObject(brush.into());
        SetBkMode(memory,TRANSPARENT); SetTextColor(memory,colorref(p.tab_text_active));
        let title=s.content.as_ref().map_or_else(|| s.id.map_or(String::new(),PanelId::title),|c| c.title().to_owned());
        // HTML content draws its own header; the host then only paints the backing fill.
        if s.header {
        if let Some(mut title16) = super::panel_widgets::nonempty_wide(&title) { let mut header=RECT{left:scale(16,dpi(hwnd)),top:0,right:r.right-scale(40,dpi(hwnd)),bottom:scale(HEADER_DIP,dpi(hwnd))}; let _=DrawTextW(memory,&mut title16,&mut header,windows::Win32::Graphics::Gdi::DT_LEFT|windows::Win32::Graphics::Gdi::DT_VCENTER|windows::Win32::Graphics::Gdi::DT_SINGLELINE); }
        let mut close_text="×".encode_utf16().collect::<Vec<_>>(); let mut close_rect=RECT{left:r.right-scale(36,dpi(hwnd)),top:0,right:r.right,bottom:scale(HEADER_DIP,dpi(hwnd))}; let _=DrawTextW(memory,&mut close_text,&mut close_rect,windows::Win32::Graphics::Gdi::DT_CENTER|windows::Win32::Graphics::Gdi::DT_VCENTER|windows::Win32::Graphics::Gdi::DT_SINGLELINE); }
        if let Some(content)=s.content.as_mut() { content.paint(memory,RECT{left:0,top:header_px(s.header,dpi(hwnd)),right:r.right,bottom:r.bottom},p); }
        if !bits.is_null() { for pixel in std::slice::from_raw_parts_mut(bits.cast::<u32>(), (width as usize).saturating_mul(height as usize)) { *pixel |= 0xFF00_0000; } }
        let _=BitBlt(dc,0,0,width,height,Some(memory),0,0,SRCCOPY);
        let _=SelectObject(memory,previous); let _=DeleteObject(bitmap.into()); let _=DeleteDC(memory);
        let _=EndPaint(hwnd,&ps);
    }
}

// --- window and panels (thread T9) --- a web panel's own width (src/panel_prefs.rs `width`).
/// The width a switch to `next` should take, or `None` to leave the host's width alone: a web panel's
/// own width (or the shared one when it has none), and the shared width when leaving a web panel.
fn panel_width_choice(previous: Option<PanelId>, next: PanelId) -> Option<i32> {
    let shared = || crate::profile::active().ok().map_or_else(|| GUEST_WIDTH.with(Cell::get), |profile| shared_width(&profile.settings_file()));
    match (previous, next) {
        (_, PanelId::WebApp(app)) => Some(crate::panel_prefs::current(app).width.map_or_else(shared, i32::from)),
        (Some(PanelId::WebApp(_)), _) => Some(shared()),
        _ => None,
    }
}

/// Switches the host's panel (`shown`) to `next` and sets its width: `choice` decides from the panel
/// shown until now and `next` (`panel_width_choice`), else the width stays, at least `min_width`.
/// W-6: the panel being left is read before `shown` changes; read after, a switch from a web panel
/// looked like a switch from `next` itself and kept the web panel's own width.
fn switch_panel(shown: &mut Option<PanelId>, width_dip: &mut i32, next: PanelId, min_width: i32, choice: impl Fn(Option<PanelId>, PanelId) -> Option<i32>) {
    let previous = shown.replace(next);
    let width = choice(previous, next).unwrap_or(*width_dip);
    *width_dip = width.max(min_width).clamp(MIN_WIDTH_DIP, MAX_WIDTH_DIP);
}

/// A drag of the panel's edge ended: a web panel remembers it for itself, every other panel for all.
fn save_panel_width(id: Option<PanelId>, owner: HWND, width: i32) {
    match id {
        Some(PanelId::WebApp(app)) => {
            let width = width.clamp(MIN_WIDTH_DIP, MAX_WIDTH_DIP) as u16;
            let _ = crate::panel_prefs::update(|file| {
                let mut prefs = file.get(app);
                prefs.width = Some(width);
                file.set(app, prefs);
            });
        }
        _ => save_width(owner, width),
    }
}
// --- end window and panels ---

/// The side panel layout saved in a settings file. `position` is the saved edge, not the live one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SavedLayout {
    width: i32,
    position: SidePanelPosition,
    tabs: TabLayout,
    tab_bar_visible: bool,
}

impl SavedLayout {
    /// The edge an open panel uses. Switching the tab edge does not change `position`.
    fn placed(self) -> SidePanelPosition {
        SidePanelPosition::placed(self.position, self.tabs, self.tab_bar_visible)
    }
}

/// What a cached layout was read at: the file's stamp and the `crate::settings::save_generation`.
type LayoutSeen = (Option<crate::panel_prefs::FileStamp>, u64);

/// A settings file's side panel layout, with what it was read at.
type CachedLayout = (std::path::PathBuf, LayoutSeen, SavedLayout);

thread_local! {
    /// The saved shared width and position, parsed once: opening or switching a panel must not wait
    /// on reading and parsing the whole settings file. A save by this process (this panel's resize,
    /// a Save or Reset on the settings page) moves the save generation on, and a write by another
    /// Bowser process on the profile changes the file's stamp, so either way the next ask reads it
    /// again instead of serving a stale width.
    static SAVED_LAYOUT: std::cell::RefCell<Option<CachedLayout>> = const { std::cell::RefCell::new(None) };
}

/// The side panel layout saved in `settings`, from memory while the file is unchanged.
fn saved_layout(settings: &std::path::Path) -> SavedLayout {
    saved_layout_at(settings, crate::settings::save_generation())
}

/// `saved_layout` once `generation` settings saves have happened in this process.
fn saved_layout_at(settings: &std::path::Path, generation: u64) -> SavedLayout {
    let stamp = (crate::panel_prefs::file_stamp(settings), generation);
    let cached = SAVED_LAYOUT.with(|slot| slot.borrow().as_ref().filter(|(path, seen, _)| path == settings && *seen == stamp).map(|(.., layout)| *layout));
    if let Some(layout) = cached {
        return layout;
    }
    let file = crate::settings::Settings::load(settings);
    let layout = SavedLayout { width: i32::from(file.side_panel_width), position: file.side_panel_position, tabs: file.tab_layout, tab_bar_visible: file.tab_bar_visible };
    SAVED_LAYOUT.with(|slot| *slot.borrow_mut() = Some((settings.to_owned(), stamp, layout)));
    layout
}

/// The shared side panel width saved in `settings`.
fn shared_width(settings: &std::path::Path) -> i32 {
    saved_layout(settings).width
}

fn save_width(_owner: HWND, width: i32) { let width=width.clamp(MIN_WIDTH_DIP,MAX_WIDTH_DIP); if let Ok(profile)=crate::profile::active() { if profile.is_guest() { GUEST_WIDTH.with(|v|v.set(width)); } else { let path=profile.settings_file(); let mut settings=crate::settings::Settings::load(&path); settings.side_panel_width=width as u16; let _=crate::settings::save(&path,&settings); } } else { GUEST_WIDTH.with(|v|v.set(width)); } }
fn panel_origin(viewport_left: i32, inset: i32) -> i32 { viewport_left.max(inset) }

/// The saved edge, with a visible vertical tab bar taking the opposite one. The file is unchanged.
fn placed_position() -> SidePanelPosition {
    crate::profile::active().ok().map(|profile| saved_layout(&profile.settings_file()).placed()).unwrap_or(SidePanelPosition::Left)
}

/// How far off its resting edge a floating panel of `width` starts. Left hides to the left.
fn hidden_offset(width: i32, position: SidePanelPosition) -> i32 {
    match position {
        SidePanelPosition::Left => -width.max(0),
        SidePanelPosition::Right => width.max(0),
    }
}

/// `offset` shifted from one edge's hidden distance to the other's, keeping the same fraction.
fn mirrored_offset(offset: i32, old_hidden: i32, new_hidden: i32) -> i32 {
    if old_hidden == 0 {
        return 0;
    }
    let value = i64::from(offset) * i64::from(new_hidden) / i64::from(old_hidden);
    i32::try_from(value).unwrap_or(new_hidden)
}

/// The host rectangle. `offset` slides a floating panel along x; `0` is the docked edge.
fn shifted_bounds(viewport: RECT, inset: i32, width: i32, position: SidePanelPosition, offset: i32) -> RECT {
    let base = panel_bounds(viewport, inset, width, position);
    RECT { left: base.left + offset, right: base.right + offset, ..base }
}

/// The host window's left edge for a panel of `width`, docked per `position`. Pure.
fn panel_left(viewport: RECT, inset: i32, width: i32, position: SidePanelPosition) -> i32 {
    match position {
        SidePanelPosition::Left => panel_origin(viewport.left, inset),
        SidePanelPosition::Right => viewport.right - width.max(0),
    }
}

/// Host-local x the resize grip sits at. Pure.
fn grip_edge(width: i32, resize_px: i32, position: SidePanelPosition) -> i32 {
    match position {
        SidePanelPosition::Left => width - resize_px,
        SidePanelPosition::Right => resize_px,
    }
}

/// Whether host-local `x` is over the resize grip. Pure.
fn over_grip(x: i32, width: i32, resize_px: i32, position: SidePanelPosition) -> bool {
    match position {
        SidePanelPosition::Left => x >= grip_edge(width, resize_px, position),
        SidePanelPosition::Right => x <= grip_edge(width, resize_px, position),
    }
}

/// The panel width (px, before DPI/available-space clamping) implied by dragging the grip to
/// host-local `x`, from a width of `before` px. Pure.
fn width_px_from_grip(x: i32, before: i32, position: SidePanelPosition) -> i32 {
    match position {
        SidePanelPosition::Left => x,
        SidePanelPosition::Right => before - x,
    }
}
pub(crate) fn should_dismiss_panel_accelerator(vk: u32, key_down: bool, open: bool, text_focused: bool) -> bool {
    vk == 0x1B && key_down && open && !text_focused
}
fn panel_bounds(viewport: RECT, inset: i32, width: i32, position: SidePanelPosition) -> RECT {
    let left = panel_left(viewport, inset, width, position);
    RECT { left, top: viewport.top, right: left + width.max(0), bottom: viewport.bottom }
}
fn width_from_drag(owner: HWND, x: i32, before_width: i32, position: SidePanelPosition) -> i32 { let mut r=RECT::default(); unsafe { let _=GetClientRect(owner,&mut r); } let inset=crate::chrome::layout::left_rail_inset(dpi(owner)); let viewport_left=HOSTS.with(|hosts| unsafe { let raw=hosts.borrow().get(&(owner.0 as isize)).copied().unwrap_or(0); host(HWND(raw as *mut c_void)).map_or(0,|state|state.client.left) }); let origin=panel_origin(viewport_left,inset); let px=width_px_from_grip(x,before_width,position); panel_width_px(px.saturating_mul(96)/dpi(owner) as i32,dpi(owner),(r.right-origin).max(0)).unwrap_or(0) }

pub fn panel_width_px(width_dip: i32, dpi: u32, available: i32) -> Option<i32> {
    let available=available.max(0); let min=scale(MIN_WIDTH_DIP,dpi.max(96));
    let max=(scale(MAX_WIDTH_DIP,dpi.max(96))).min((available as f32*0.45) as i32).min(available-scale(320,dpi.max(96)));
    if max<min { None } else { Some(scale(width_dip.clamp(MIN_WIDTH_DIP,MAX_WIDTH_DIP),dpi.max(96)).clamp(min,max)) }
}
#[cfg(test)]
fn reserve_viewport(viewport: RECT, panel_width: i32) -> RECT { RECT { left: viewport.left+panel_width.max(0), ..viewport } }
fn animation_width(from: i32, to: i32, t: f32) -> i32 {
    let t = t.clamp(0.0, 1.0);
    // Opening eases out; closing eases in.
    let eased = if to < from { t.powi(3) } else { 1.0 - (1.0 - t).powi(3) };
    (from as f32 + (to - from) as f32 * eased).round() as i32
}

/// Offset of a floating panel. `0` rests on its edge; a left panel hides at `-width` and a right
/// panel at `+width`. Moving toward 0 (opening) eases out. Moving away eases in. The sign of the
/// delta is not the direction: a right-edge reveal goes from positive to 0 and still eases out.
fn slide_px(from: i32, to: i32, t: f32) -> i32 {
    let t = t.clamp(0.0, 1.0);
    let eased = if to.saturating_abs() < from.saturating_abs() { 1.0 - (1.0 - t).powi(3) } else { t.powi(3) };
    (from as f32 + (to - from) as f32 * eased).round() as i32
}
fn animation_running(elapsed_ms: f32) -> bool { elapsed_ms < ANIMATION_MS }
fn colorref(c: theme::Rgba) -> windows::Win32::Foundation::COLORREF { windows::Win32::Foundation::COLORREF(u32::from(c.r)|(u32::from(c.g)<<8)|(u32::from(c.b)<<16)) }

#[cfg(test)]
mod tests {
    use super::*;

    /// W-6: from a widened web-app panel to Bookmarks (or History) the host goes back to the shared
    /// width, because the width rule sees the web app as the panel being left.
    #[test]
    fn leaving_a_web_panel_goes_back_to_the_shared_width() {
        const SHARED: i32 = 320;
        const OWN: i32 = 520;
        let seen = std::cell::RefCell::new(Vec::new());
        // `panel_width_choice` without the profile: a web panel's own width, the shared width when
        // leaving one, else nothing.
        let choice = |previous: Option<PanelId>, next: PanelId| {
            seen.borrow_mut().push((previous, next));
            match (previous, next) {
                (_, PanelId::WebApp(_)) => Some(OWN),
                (Some(PanelId::WebApp(_)), _) => Some(SHARED),
                _ => None,
            }
        };
        let (mut shown, mut width) = (Some(PanelId::Bookmarks), SHARED);
        switch_panel(&mut shown, &mut width, PanelId::WebApp(7), 0, choice);
        assert_eq!((shown, width), (Some(PanelId::WebApp(7)), OWN), "a web panel takes its own width");
        switch_panel(&mut shown, &mut width, PanelId::Bookmarks, 0, choice);
        assert_eq!((shown, width), (Some(PanelId::Bookmarks), SHARED), "leaving it: the shared width");
        assert_eq!(seen.borrow()[1], (Some(PanelId::WebApp(7)), PanelId::Bookmarks), "the rule saw the panel being left");
        switch_panel(&mut shown, &mut width, PanelId::History, 0, choice);
        assert_eq!(width, SHARED, "between shared panels the width stays");
        switch_panel(&mut shown, &mut width, PanelId::Customize, 400, choice);
        assert_eq!(width, 400, "but never below the content's minimum");
    }

    #[test]
    fn the_saved_layout_is_read_once_and_any_write_to_the_settings_makes_it_read_again() {
        let dir = std::env::temp_dir().join(format!("bowser-side-width-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.txt");
        let mut settings = crate::settings::Settings::default();
        settings.side_panel_width = 333;
        settings.side_panel_position = SidePanelPosition::Right;
        // Every save in this process moves the generation on (tests running alongside save too, so
        // only "it grew" is certain; below, explicit generations keep their saves out of the result).
        let before = crate::settings::save_generation();
        crate::settings::save(&path, &settings).unwrap();
        assert!(crate::settings::save_generation() > before, "a settings save invalidates the cached layout");
        assert_eq!(saved_layout_at(&path, 1), SavedLayout { width: 333, position: SidePanelPosition::Right, tabs: crate::settings::TabLayout::ToolbarTop, tab_bar_visible: true });
        // While nothing changed a panel switch does not parse the file: the copy in memory answers
        // (seen here by planting a different copy for the same stamp and generation).
        let planted = SavedLayout { width: 300, position: SidePanelPosition::Right, tabs: crate::settings::TabLayout::ToolbarTop, tab_bar_visible: true };
        SAVED_LAYOUT.with(|slot| *slot.borrow_mut() = Some((path.clone(), (crate::panel_prefs::file_stamp(&path), 1), planted)));
        assert_eq!(saved_layout_at(&path, 1).width, 300, "served from memory");
        // A settings-page Save or Reset: the generation moved, so the file is read again even when
        // its time and length could not show it.
        assert_eq!(saved_layout_at(&path, 2).width, 333, "a save in this process is never served stale");
        // Another Bowser process on the profile rewrites the file (generation unchanged here): its
        // stamp changed, so the new width is read.
        settings.side_panel_width = 410;
        settings.side_panel_position = SidePanelPosition::Left;
        crate::settings::save(&path, &settings).unwrap();
        assert_eq!(saved_layout_at(&path, 2), SavedLayout { width: 410, position: SidePanelPosition::Left, tabs: crate::settings::TabLayout::ToolbarTop, tab_bar_visible: true });
        // The real entry points agree, and a removed file means the default width.
        assert_eq!(shared_width(&path), 410);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(shared_width(&path), i32::from(crate::settings::DEFAULT_SIDE_PANEL_WIDTH));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn width_is_dpi_aware_and_preserves_a_useful_viewport() {
        assert_eq!(panel_width_px(360, 96, 1200), Some(360));
        assert_eq!(panel_width_px(360, 144, 1600), Some(540));
        assert_eq!(panel_width_px(520, 192, 1400), Some(630));
        assert_eq!(panel_width_px(280, 96, 640), Some(280));
        assert_eq!(panel_width_px(280, 96, 600), None);
        assert_eq!(panel_width_px(280, 96, 580), None);
    }

    #[test]
    fn viewport_reservation_starts_after_rail() {
        assert_eq!(reserve_viewport(RECT { left: 80, top: 90, right: 1200, bottom: 800 }, 340),
            RECT { left: 420, top: 90, right: 1200, bottom: 800 });
    }

    #[test]
    fn vertical_sidebar_places_panel_after_sidebar_and_reserves_its_width() {
        let viewport = RECT { left: 300, top: 80, right: 1600, bottom: 900 };
        let panel = panel_bounds(viewport, 44, 360, SidePanelPosition::Left);
        let page = reserve_viewport(viewport, panel.right - panel.left);
        assert_eq!(panel, RECT { left: 300, top: 80, right: 660, bottom: 900 });
        assert_eq!(page.left, panel.right);
    }

    #[test]
    fn page_is_rebounded_to_the_space_right_of_an_open_panel_and_restored_on_close() {
        let viewport = RECT { left: 34, top: 70, right: 1276, bottom: 796 };
        // Open at full width: the page starts where the panel ends (rail inset 34 + 340).
        assert_eq!(reserved_viewport(viewport, 34, 340, SidePanelPosition::Left), RECT { left: 374, ..viewport });
        // Mid animation and closed, the same formula follows the width back to the full page.
        assert_eq!(reserved_viewport(viewport, 34, 120, SidePanelPosition::Left).left, 154);
        assert_eq!(reserved_viewport(viewport, 34, 0, SidePanelPosition::Left), viewport);
        // A viewport that starts left of the rail inset still clears the rail.
        assert_eq!(reserved_viewport(RECT { left: 0, ..viewport }, 34, 340, SidePanelPosition::Left).left, 374);
        // A vertical sidebar pushes the viewport right of the inset: the panel sits after it.
        assert_eq!(reserved_viewport(RECT { left: 300, ..viewport }, 34, 340, SidePanelPosition::Left).left, 640);
        // Never negative width.
        assert_eq!(reserved_viewport(viewport, 34, -5, SidePanelPosition::Left), viewport);
    }

    #[test]
    fn right_docked_panel_sits_at_the_viewport_s_right_edge_and_the_page_ends_before_it() {
        let viewport = RECT { left: 34, top: 70, right: 1276, bottom: 796 };
        let panel = panel_bounds(viewport, 34, 340, SidePanelPosition::Right);
        assert_eq!(panel, RECT { left: 936, top: 70, right: 1276, bottom: 796 });
        let page = reserved_viewport(viewport, 34, 340, SidePanelPosition::Right);
        assert_eq!(page, RECT { right: 936, ..viewport });
        assert_eq!(page.right, panel.left);
        // Closed (width 0), the page regains the full viewport, same as left-docked.
        assert_eq!(reserved_viewport(viewport, 34, 0, SidePanelPosition::Right), viewport);
        assert_eq!(reserved_viewport(viewport, 34, -5, SidePanelPosition::Right), viewport, "never negative width");
    }

    #[test]
    fn the_resize_grip_and_drag_direction_mirror_for_a_right_docked_panel() {
        let resize_px = 6;
        // Left-docked: the grip is the panel's own right edge, dragging it sets the width directly.
        assert!(over_grip(354, 360, resize_px, SidePanelPosition::Left));
        assert!(!over_grip(300, 360, resize_px, SidePanelPosition::Left));
        assert_eq!(width_px_from_grip(400, 360, SidePanelPosition::Left), 400);
        // Right-docked: the grip is the panel's own left edge (host-local x near 0); dragging it
        // further left (a smaller, even negative, local x) grows the width from where it was.
        assert!(over_grip(2, 360, resize_px, SidePanelPosition::Right));
        assert!(!over_grip(60, 360, resize_px, SidePanelPosition::Right));
        assert_eq!(width_px_from_grip(-40, 360, SidePanelPosition::Right), 400);
        assert_eq!(width_px_from_grip(60, 360, SidePanelPosition::Right), 300);
    }

    #[test]
    fn html_content_has_no_native_header_row() {
        assert_eq!(header_px(true, 96), HEADER_DIP);
        assert_eq!(header_px(true, 192), HEADER_DIP * 2);
        assert_eq!(header_px(false, 96), 0);
        assert_eq!(header_px(false, 192), 0);
    }

    #[test]
    fn escape_accelerator_dismisses_only_an_open_panel_without_text_focus() {
        assert!(should_dismiss_panel_accelerator(0x1B, true, true, false));
        assert!(!should_dismiss_panel_accelerator(0x1B, true, false, false));
        assert!(!should_dismiss_panel_accelerator(0x1B, false, true, false));
        assert!(!should_dismiss_panel_accelerator(0x1B, true, true, true));
        assert!(!should_dismiss_panel_accelerator(0x41, true, true, false));
    }

    #[test]
    fn a_floating_panel_keeps_its_width_and_slides_in_from_the_outer_edge() {
        let viewport = RECT { left: 34, top: 70, right: 1276, bottom: 796 };
        let rest = panel_bounds(viewport, 34, 340, SidePanelPosition::Left);
        let hidden = shifted_bounds(viewport, 34, 340, SidePanelPosition::Left, hidden_offset(340, SidePanelPosition::Left));
        assert_eq!(hidden.right - hidden.left, 340, "the width stays constant");
        assert_eq!(hidden.right, rest.left, "a left panel starts fully past the left edge");
        let right = panel_bounds(viewport, 34, 340, SidePanelPosition::Right);
        let right_hidden = shifted_bounds(viewport, 34, 340, SidePanelPosition::Right, hidden_offset(340, SidePanelPosition::Right));
        assert_eq!(right_hidden.right - right_hidden.left, 340);
        assert_eq!(right_hidden.left, right.right, "a right panel starts fully past the right edge");
        // Switching edges mid-slide keeps the same fraction of the way out.
        assert_eq!(mirrored_offset(-170, -340, 340), 170);
        assert_eq!(mirrored_offset(0, -340, 340), 0);
    }

    #[test]
    fn a_vertical_tab_bar_in_the_file_places_the_panel_opposite_without_changing_the_saved_edge() {
        let dir = std::env::temp_dir().join(format!("bowser-side-place-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.txt");
        let mut settings = crate::settings::Settings::default();
        settings.tab_layout = crate::settings::TabLayout::Vertical;
        settings.side_panel_position = SidePanelPosition::Left;
        crate::settings::save(&path, &settings).unwrap();
        let saved = saved_layout_at(&path, crate::settings::save_generation());
        assert_eq!(saved.position, SidePanelPosition::Left, "the file still says left");
        assert_eq!(saved.placed(), SidePanelPosition::Right, "tabs on the left: the panel opens on the right");
        settings.tab_layout = crate::settings::TabLayout::VerticalRight;
        crate::settings::save(&path, &settings).unwrap();
        let moved = saved_layout_at(&path, crate::settings::save_generation());
        assert_eq!(moved.position, SidePanelPosition::Left, "switching the tab edge does not rewrite the panel edge");
        assert_eq!(moved.placed(), SidePanelPosition::Left);
        settings.tab_bar_visible = false;
        crate::settings::save(&path, &settings).unwrap();
        assert_eq!(saved_layout_at(&path, crate::settings::save_generation()).placed(), SidePanelPosition::Left, "a hidden tab bar keeps the saved edge");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn animation_has_eased_endpoints_and_settles() {
        assert_eq!(animation_width(200, 400, 0.0), 200);
        assert_eq!(animation_width(200, 400, 1.0), 400);
        assert!(animation_width(200, 400, 0.5) > 300, "opening eases out");
        assert!(animation_width(400, 200, 0.5) > 300, "closing eases in and has barely started");
        assert_eq!(slide_px(-400, 0, 0.0), -400);
        assert_eq!(slide_px(-400, 0, 1.0), 0);
        assert!(slide_px(-400, 0, 0.5) > -200, "opening from the left eases out");
        assert!(slide_px(0, -400, 0.5) > -200, "closing to the left eases in");
        assert!(slide_px(400, 0, 0.5) < 200, "a right-edge reveal eases out even though the offset falls");
        assert!(slide_px(0, 400, 0.5) < 200, "a right-edge hide eases in");
        assert!(animation_running(219.0));
        assert!(!animation_running(220.0));
    }

    #[test]
    fn toggle_switch_close_and_escape_state_machine() {
        let mut state = PanelState::default();
        state.toggle(PanelId::Bookmarks);
        let host = state.host_generation;
        state.toggle(PanelId::History);
        assert_eq!(state.open, Some(PanelId::History));
        assert_eq!(state.host_generation, host);
        assert!(state.escape());
        assert_eq!(state.open, None);
        state.toggle(PanelId::Bookmarks);
        state.close();
        assert!(!state.escape());
    }

    #[test]
    fn closing_host_with_empty_title_skips_the_draw_call() {
        let mut state = PanelState::default();
        state.toggle(PanelId::Bookmarks);
        state.close();
        let title = state.open.map_or_else(String::new, PanelId::title);
        assert!(super::super::panel_widgets::nonempty_wide(&title).is_none());
    }

    struct FakeContent;
    impl PanelContent for FakeContent {
        fn title(&self) -> &str { "Fake" }
    }

    #[test]
    fn registered_factory_constructs_content_lazily() {
        let created = std::rc::Rc::new(std::cell::Cell::new(0));
        let count = created.clone();
        register_content(PanelId::Customize, move || {
            count.set(count.get() + 1);
            Box::new(FakeContent)
        });
        assert_eq!(created.get(), 0);
        let factory = FACTORIES.with(|factories| factories.borrow().get(&PanelId::Customize).cloned()).unwrap();
        let content = factory();
        assert_eq!(created.get(), 1);
        assert_eq!(content.title(), "Fake");
    }
}
