//! Shared building blocks for the rail panels (Bookmarks, History, Customize).
//!
//! The list math here is pure: scrolling, visible rows, hit-testing, keyboard movement, drag
//! drop slots and time formatting. It is unit-tested without a window. The GDI helpers at the
//! bottom draw text, favicon bitmaps and letter tiles, and they only run on the UI thread.

use std::ops::Range;

use windows::Win32::{
    Foundation::{COLORREF, RECT},
    Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DRAW_TEXT_FORMAT, DeleteDC,
        DeleteObject, DrawTextW, HDC, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, StretchDIBits, SRCCOPY, DT_CENTER, DT_END_ELLIPSIS,
        DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
    },
};

use crate::{
    favicon::{self, FaviconBitmap},
    theme::{self, Palette, Rgba},
};

/// Keys the list panels act on. Text entry stays with the native EDIT, so plain characters are
/// never mapped here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Enter,
    Delete,
    F2,
    Escape,
    /// Collapses a folder, or moves to its parent.
    Left,
    /// Expands a folder, or moves to its first child.
    Right,
}

/// Maps a Win32 virtual-key code to a panel key. Unknown codes return `None`.
pub(crate) fn key_from_vk(vk: u16) -> Option<Key> {
    Some(match vk {
        0x26 => Key::Up,
        0x28 => Key::Down,
        0x24 => Key::Home,
        0x23 => Key::End,
        0x21 => Key::PageUp,
        0x22 => Key::PageDown,
        0x0D => Key::Enter,
        0x2E => Key::Delete,
        0x71 => Key::F2,
        0x1B => Key::Escape,
        0x25 => Key::Left,
        0x27 => Key::Right,
        _ => return None,
    })
}

/// The row a movement key selects, clamped to the list. `None` when the key does not move the
/// selection or the list is empty. With no selection, Down starts at the top and Up at the bottom.
pub(crate) fn moved_selection(
    key: Key,
    current: Option<usize>,
    len: usize,
    page: usize,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let last = len - 1;
    let page = page.max(1);
    Some(match key {
        Key::Down => current.map_or(0, |i| (i + 1).min(last)),
        Key::Up => current.map_or(last, |i| i.saturating_sub(1)),
        Key::Home => 0,
        Key::End => last,
        Key::PageDown => current.map_or(0, |i| (i + page).min(last)),
        Key::PageUp => current.map_or(0, |i| i.saturating_sub(page)),
        _ => return None,
    })
}

/// Row geometry in device pixels. `row` is the height of one row and `height` the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Metrics {
    pub row: i32,
    pub height: i32,
}

pub(crate) fn content_height(count: usize, m: Metrics) -> i32 {
    (count as i64 * m.row as i64).min(i32::MAX as i64) as i32
}

pub(crate) fn max_scroll(count: usize, m: Metrics) -> i32 {
    (content_height(count, m) - m.height).max(0)
}

pub(crate) fn clamp_scroll(scroll: i32, count: usize, m: Metrics) -> i32 {
    scroll.clamp(0, max_scroll(count, m))
}

/// Rows that touch the viewport at `scroll`, including partly visible rows at either edge.
pub(crate) fn visible_range(scroll: i32, count: usize, m: Metrics) -> Range<usize> {
    if count == 0 || m.row <= 0 || m.height <= 0 {
        return 0..0;
    }
    let first = (scroll.max(0) / m.row) as usize;
    let bottom = scroll.max(0) as i64 + m.height as i64;
    let end = ((bottom + m.row as i64 - 1) / m.row as i64) as usize;
    first.min(count)..end.min(count)
}

/// The scroll position that brings `index` fully into view, with the minimum movement.
pub(crate) fn scroll_to_show(scroll: i32, index: usize, count: usize, m: Metrics) -> i32 {
    let top = index as i32 * m.row;
    let bottom = top + m.row;
    let next = if top < scroll {
        top
    } else if bottom > scroll + m.height {
        bottom - m.height
    } else {
        scroll
    };
    clamp_scroll(next, count, m)
}

/// The row under a viewport-relative `y`, or `None` in the gap below the last row.
pub(crate) fn row_at(y: i32, scroll: i32, count: usize, m: Metrics) -> Option<usize> {
    if y < 0 || y >= m.height || m.row <= 0 {
        return None;
    }
    let index = ((y as i64 + scroll as i64) / m.row as i64) as usize;
    (index < count).then_some(index)
}

/// Viewport-relative top edge of row `index`.
pub(crate) fn row_top(index: usize, scroll: i32, m: Metrics) -> i32 {
    index as i32 * m.row - scroll
}

pub(crate) fn contains(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}


/// Where a dragged item lands relative to the row under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Before,
    After,
    /// Inside a folder row: append as its last child.
    Into,
}

/// Classifies the pointer's `y` inside a row of height `row`. The outer quarters insert beside
/// the row. The middle inserts into a folder, and splits in half for a link.
pub(crate) fn slot_in_row(y_in_row: i32, row: i32, is_folder: bool) -> Slot {
    let edge = (row / 4).max(1);
    if y_in_row < edge {
        Slot::Before
    } else if y_in_row >= row - edge {
        Slot::After
    } else if is_folder {
        Slot::Into
    } else if y_in_row < row / 2 {
        Slot::Before
    } else {
        Slot::After
    }
}

/// The insert index `BookmarkStore::move_item` expects: a position among the destination's
/// children with the dragged item removed. `siblings` lists the destination's children in order.
/// `target` is the row beside which the item lands, or `None` when it appends.
pub(crate) fn insert_index(siblings: &[u64], dragged: u64, target: Option<u64>, slot: Slot) -> usize {
    let without: Vec<u64> = siblings.iter().copied().filter(|id| *id != dragged).collect();
    match (target, slot) {
        (Some(target), Slot::Before) => without
            .iter()
            .position(|id| *id == target)
            .unwrap_or(without.len()),
        (Some(target), Slot::After) => without
            .iter()
            .position(|id| *id == target)
            .map_or(without.len(), |p| p + 1),
        _ => without.len(),
    }
}

/// Upper-case first letter of a title for the letter tile fallback. Symbols and empty titles
/// fall back to a bullet so the tile is never blank.
pub(crate) fn letter_tile(title: &str) -> char {
    title
        .chars()
        .find(|c| c.is_alphanumeric())
        .map_or('\u{2022}', |c| c.to_uppercase().next().unwrap_or(c))
}

/// Host shown under a link: the URL's host without a leading `www.`. Unparseable input yields
/// an empty string.
pub(crate) fn host_label(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .map(|h| h.strip_prefix("www.").unwrap_or(&h).to_owned())
        .unwrap_or_default()
}

/// Days since 1970-01-01 for a proleptic Gregorian date.
pub(crate) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Offset in seconds from UTC to local time, from two readings of the same instant. Each reading
/// is `(year, month, day, hour, minute, second)`.
pub(crate) fn local_offset_secs(local: [i64; 6], utc: [i64; 6]) -> i64 {
    let as_secs = |t: [i64; 6]| {
        days_from_civil(t[0], t[1], t[2]) * 86400 + t[3] * 3600 + t[4] * 60 + t[5]
    };
    as_secs(local) - as_secs(utc)
}

/// `HH:MM` for a Unix timestamp shifted by `offset_secs`.
pub(crate) fn clock_label(timestamp: u64, offset_secs: i64) -> String {
    let local = (timestamp as i64 + offset_secs).max(0);
    let secs_of_day = local.rem_euclid(86400);
    format!("{:02}:{:02}", secs_of_day / 3600, secs_of_day % 3600 / 60)
}

/// Shifts a Unix timestamp by a local offset, so that day-bucketing by `timestamp / 86400` groups
/// by local calendar day.
pub(crate) fn shift_to_local(timestamp: u64, offset_secs: i64) -> u64 {
    (timestamp as i64 + offset_secs).max(0) as u64
}

/// Accent colour for a letter tile, picked from the host so the same site keeps its colour.
pub(crate) fn tile_colour(host: &str) -> Rgba {
    let hash = host.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    theme::MARKER_COLOURS[hash as usize % theme::MARKER_COLOURS.len()]
}

/// Destructive text colour on light surfaces.
pub(crate) const DANGER: Rgba = Rgba::hex(0xC42B1C, 0xFF);

/// Destructive text colour for `palette`. The light-surface red is too dim on a dark panel, so a
/// lighter red of the same hue stands in there.
pub(crate) fn danger(palette: &Palette) -> Rgba {
    let w = palette.window;
    let luma = (w.r as u32 * 299 + w.g as u32 * 587 + w.b as u32 * 114) / 1000;
    if luma < 128 { Rgba::hex(0xFF8A7A, 0xFF) } else { DANGER }
}

pub(crate) fn colorref(c: Rgba) -> COLORREF {
    COLORREF(c.r as u32 | (c.g as u32) << 8 | (c.b as u32) << 16)
}

/// `c` laid over `over` as an opaque colour. GDI and EDIT controls drop alpha, and GDI+ fills write
/// it into a surface that is already glass, so a translucent token would draw as solid white.
fn flatten(c: Rgba, over: Rgba) -> Rgba {
    let a = c.a as u32;
    let mix = |fg: u8, bg: u8| ((fg as u32 * a + bg as u32 * (255 - a) + 127) / 255) as u8;
    Rgba {
        r: mix(c.r, over.r),
        g: mix(c.g, over.g),
        b: mix(c.b, over.b),
        a: 0xFF,
    }
}

/// The palette with every token opaque. Each translucent token is laid over the window tint, which
/// is the colour the host paints behind the content. Opaque tokens are unchanged.
pub(crate) fn opaque(palette: &Palette) -> Palette {
    let mut out = palette.clone();
    out.translucent = false;
    out.window = Rgba { a: 0xFF, ..palette.window };
    let base = out.window;
    for token in [
        &mut out.tab_inactive,
        &mut out.tab_hover,
        &mut out.tab_active,
        &mut out.tab_active_border,
        &mut out.tab_group_accent,
        &mut out.tab_text_inactive,
        &mut out.tab_text_active,
        &mut out.omnibox_fill,
        &mut out.omnibox_border,
        &mut out.omnibox_focus_border,
        &mut out.omnibox_selection,
        &mut out.omnibox_text,
        &mut out.omnibox_placeholder,
        &mut out.omnibox_completion,
        &mut out.omnibox_caret,
        &mut out.progress_fill,
        &mut out.caption_glyph,
        &mut out.caption_hover,
        &mut out.close_hover,
        &mut out.close_glyph_hover,
        &mut out.hairline,
    ] {
        *token = flatten(*token, base);
    }
    out
}

pub(crate) fn scale(value: i32, dpi: u32) -> i32 {
    value * dpi.max(96) as i32 / 96
}

pub(crate) fn nonempty_wide(value: &str) -> Option<Vec<u16>> {
    let units = value.encode_utf16().collect::<Vec<_>>();
    (!units.is_empty()).then_some(units)
}

/// Draws one line of text clipped to `bounds`. `ellipsis` trims the end instead of wrapping.
pub(crate) unsafe fn draw_text(
    dc: HDC,
    mut bounds: RECT,
    value: &str,
    colour: Rgba,
    align_centre: bool,
    ellipsis: bool,
) {
    if value.is_empty() { return; }
    let Some(mut units) = nonempty_wide(value) else { return };
    let mut flags: DRAW_TEXT_FORMAT = DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX;
    flags |= if align_centre { DT_CENTER } else { DT_LEFT };
    if ellipsis {
        flags |= DT_END_ELLIPSIS;
    }
    unsafe {
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, colorref(colour));
        let _ = DrawTextW(dc, &mut units, &mut bounds, flags);
    }
}


/// Draws a letter tile: a rounded square in the host's colour with the title's first letter. It is
/// the fallback whenever a favicon is missing or undecoded.
pub(crate) unsafe fn letter_tile_at(dc: HDC, rect: RECT, title: &str, host: &str) {
    let letter = letter_tile(title).to_string();
    unsafe {
        super::smooth::fill_round_rect(dc, rect, (rect.right - rect.left) / 3, tile_colour(host));
        draw_text(dc, rect, &letter, Rgba::hex(0xFFFFFF, 0xFF), true, false);
    }
}

/// Draws a favicon bitmap scaled into `rect`. Bitmaps that fail the size check draw nothing.
pub(crate) unsafe fn blit_favicon(dc: HDC, rect: RECT, image: &FaviconBitmap) {
    let (width, height) = (image.width as i32, image.height as i32);
    if width <= 0 || height <= 0 || image.bgra_premultiplied.len() != (width * height * 4) as usize {
        return;
    }
    unsafe {
        let memory = CreateCompatibleDC(Some(dc));
        if memory.is_invalid() {
            return;
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(memory);
            return;
        };
        if !bits.is_null() {
            std::ptr::copy_nonoverlapping(
                image.bgra_premultiplied.as_ptr(),
                bits.cast::<u8>(),
                image.bgra_premultiplied.len(),
            );
        }
        let previous = SelectObject(memory, bitmap.into());
        let _ = StretchDIBits(
            dc,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            0,
            0,
            width,
            height,
            Some(bits as *const std::ffi::c_void),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
        SelectObject(memory, previous);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
    }
}


// ---- Win32 plumbing -------------------------------------------------------------------------
//
// The host gives content a paint DC and forwards mouse, wheel, char and key input. It positions
// at most one native child, so each panel creates its own EDITs under the host window. Their
// subclass routes Enter, Escape, arrows, Tab and text changes back to the panel. The host is
// subclassed too, so the EDITs take the palette's fill and text colours.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::{
        ANTIALIASED_QUALITY, HBRUSH, HFONT, InvalidateRect, SetBkColor, WindowFromDC,
    },
    UI::{
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::{SetFocus, VK_SHIFT, VK_TAB},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, ES_AUTOHSCROLL, GetParent, GetWindowTextLengthW,
            GetWindowTextW, SW_HIDE, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SendMessageW,
            SetWindowPos, SetWindowTextW, ShowWindow, WINDOW_STYLE, WM_CHAR, WM_CLEAR,
            WM_CTLCOLOREDIT, WM_CUT, WM_KEYDOWN, WM_PASTE, WM_RBUTTONUP, WM_SETFONT, WM_UNDO,
            WS_CHILD,
        },
    },
};
use windows::Win32::UI::Controls::EM_SETCUEBANNER;
use windows::core::{PCWSTR, w};

use super::menu;
use crate::chrome::paint::create_message_font;

/// Receives right-button releases on a panel host. The host does not forward them, so the host
/// subclass looks the sink up by window.
pub(crate) trait PointerSink {
    fn right_click(&mut self, x: i32, y: i32);
}

thread_local! {
    static RIGHT_CLICK: RefCell<HashMap<isize, *mut dyn PointerSink>> = RefCell::new(HashMap::new());
}

/// Routes right-button releases on `host` to `sink` until [`unregister_right_click`].
pub(crate) fn register_right_click(host: HWND, sink: *mut dyn PointerSink) {
    RIGHT_CLICK.with(|map| map.borrow_mut().insert(host.0 as isize, sink));
}

pub(crate) fn unregister_right_click(host: HWND) {
    RIGHT_CLICK.with(|map| map.borrow_mut().remove(&(host.0 as isize)));
}

/// Subclass ids. Each subclass is keyed by its (procedure, id) pair.
const EDIT_SUBCLASS: usize = 0x9B01;
const HOST_SUBCLASS: usize = 0x9B02;

/// What an EDIT reports to its panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditEvent {
    /// The text may have changed. The sink compares it with what it last saw.
    Changed,
    Key(Key),
    Tab { back: bool },
}

/// The panel side of its EDIT controls. `field` is the index the panel gave when it created the
/// EDIT. A key the sink returns `true` for is consumed, so the EDIT does not also edit text.
pub(crate) trait EditSink {
    fn edit_event(&mut self, field: usize, event: EditEvent) -> bool;
}

/// Heap record that the EDIT's subclass reads. It is freed when the field is destroyed.
struct Route {
    sink: *mut dyn EditSink,
    field: usize,
}

/// One native EDIT under a panel's host window.
pub(crate) struct EditField {
    pub hwnd: HWND,
    route: *mut Route,
    placed: Option<RECT>,
}

impl EditField {
    /// Creates a child EDIT under `host` that reports to `sink`.
    pub(crate) fn new(host: HWND, sink: *mut dyn EditSink, field: usize, font: HFONT) -> Self {
        let route = Box::into_raw(Box::new(Route { sink, field }));
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("EDIT"),
                PCWSTR::null(),
                // Created hidden. The first `place` call shows it at its laid-out rectangle.
                WS_CHILD | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                0,
                0,
                1,
                1,
                Some(host),
                None,
                None,
                None,
            )
            .unwrap_or_default()
        };
        if hwnd.is_invalid() {
            drop(unsafe { Box::from_raw(route) });
            return Self {
                hwnd: HWND::default(),
                route: std::ptr::null_mut(),
                placed: None,
            };
        }
        unsafe {
            let _ = SetWindowSubclass(hwnd, Some(edit_proc), EDIT_SUBCLASS, route as usize);
            SendMessageW(hwnd, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
        }
        Self {
            hwnd,
            route,
            placed: None,
        }
    }

    /// Moves the EDIT to `rect` in host coordinates, or hides it for `None`. Win32 is only called
    /// when the placement changed.
    pub(crate) fn place(&mut self, rect: Option<RECT>) {
        if self.hwnd.is_invalid() || self.placed == rect {
            return;
        }
        self.placed = rect;
        unsafe {
            match rect {
                Some(rect) => {
                    let _ = SetWindowPos(
                        self.hwnd,
                        None,
                        rect.left,
                        rect.top,
                        (rect.right - rect.left).max(0),
                        (rect.bottom - rect.top).max(0),
                        SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                }
                None => {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
            }
        }
    }

    pub(crate) fn set_font(&self, font: HFONT) {
        if !self.hwnd.is_invalid() {
            unsafe { SendMessageW(self.hwnd, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1))); }
        }
    }

    pub(crate) fn text(&self) -> String {
        if self.hwnd.is_invalid() {
            return String::new();
        }
        unsafe {
            let len = GetWindowTextLengthW(self.hwnd).max(0) as usize;
            let mut buffer = vec![0u16; len + 1];
            let count = GetWindowTextW(self.hwnd, &mut buffer).max(0) as usize;
            String::from_utf16_lossy(&buffer[..count])
        }
    }

    /// Replaces the text. This sends WM_SETTEXT, which does not count as an edit, so the sink is not
    /// told about it.
    pub(crate) fn set_text(&self, value: &str) {
        if self.hwnd.is_invalid() {
            return;
        }
        let wide: Vec<u16> = value.encode_utf16().chain([0]).collect();
        unsafe {
            let _ = SetWindowTextW(self.hwnd, PCWSTR(wide.as_ptr()));
        }
    }

    /// Greyed placeholder shown while the EDIT is empty, even when it has focus.
    pub(crate) fn set_cue(&self, text: &str) {
        if self.hwnd.is_invalid() {
            return;
        }
        let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
        unsafe {
            SendMessageW(
                self.hwnd,
                EM_SETCUEBANNER,
                Some(WPARAM(1)),
                Some(LPARAM(wide.as_ptr() as isize)),
            );
        }
    }

    pub(crate) fn focus(&self) {
        if !self.hwnd.is_invalid() {
            unsafe {
                let _ = SetFocus(Some(self.hwnd));
            }
        }
    }

}

impl Drop for EditField {
    fn drop(&mut self) {
        if !self.hwnd.is_invalid() {
            unsafe {
                let _ = RemoveWindowSubclass(self.hwnd, Some(edit_proc), EDIT_SUBCLASS);
                let _ = DestroyWindow(self.hwnd);
            }
        }
        if !self.route.is_null() {
            drop(unsafe { Box::from_raw(self.route) });
        }
    }
}

/// Virtual-key event for an EDIT: Tab (with Shift as `back`) or a panel key.
fn edit_key_event(vk: u32, shift: bool) -> Option<EditEvent> {
    if vk == VK_TAB.0 as u32 {
        return Some(EditEvent::Tab { back: shift });
    }
    key_from_vk(vk as u16).map(EditEvent::Key)
}

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let route = data as *mut Route;
    if !route.is_null() {
        let route = unsafe { &*route };
        match message {
            WM_KEYDOWN => {
                let shift = unsafe {
                    windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(VK_SHIFT.0 as i32) < 0
                };
                if let Some(event) = edit_key_event(wparam.0 as u32, shift) {
                    let handled = unsafe { (*route.sink).edit_event(route.field, event) };
                    if handled {
                        return LRESULT(0);
                    }
                }
            }
            WM_CHAR if wparam.0 == 0x0D || wparam.0 == 0x1B => {
                // Enter and Escape are handled on key-down; swallow their character too.
                return LRESULT(0);
            }
            _ => {}
        }
    }
    let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    if !route.is_null()
        && matches!(message, WM_KEYDOWN | WM_CHAR | WM_PASTE | WM_CUT | WM_CLEAR | WM_UNDO)
    {
        let route = unsafe { &*route };
        unsafe { (*route.sink).edit_event(route.field, EditEvent::Changed) };
    }
    result
}

/// Colours EDIT controls to match the palette. Filled on each paint by [`Surface::learn`].
#[derive(Clone, Copy, Debug, Default)]
struct EditStyle {
    fill: u32,
    text: u32,
    brush: isize,
}

thread_local! {
    static EDIT_STYLE: Cell<EditStyle> = const { Cell::new(EditStyle { fill: 0, text: 0, brush: 0 }) };
}

unsafe extern "system" fn host_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if message == WM_CTLCOLOREDIT {
        let style = EDIT_STYLE.with(Cell::get);
        if style.brush != 0 {
            let dc = HDC(wparam.0 as *mut std::ffi::c_void);
            unsafe {
                SetBkColor(dc, COLORREF(style.fill));
                SetTextColor(dc, COLORREF(style.text));
            }
            return LRESULT(style.brush);
        }
    }
    if message == WM_RBUTTONUP {
        let sink = RIGHT_CLICK.with(|map| map.borrow().get(&(hwnd.0 as isize)).copied());
        if let Some(sink) = sink {
            let x = lparam.0 as u16 as i16 as i32;
            let y = (lparam.0 >> 16) as u16 as i16 as i32;
            unsafe { (*sink).right_click(x, y) };
            return LRESULT(0);
        }
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

/// Where a panel paints and receives input. It is learned from the paint DC, because the host
/// does not pass its window to content.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Surface {
    pub host: HWND,
    /// The content rectangle in host client coordinates, from the last paint.
    pub origin: RECT,
    pub dpi: u32,
}

impl Surface {
    /// Attaches the surface to its stable host before painting, since memory DCs have no HWND.
    pub(crate) fn attach(&mut self, host: HWND) {
        if host.is_invalid() { return; }
        self.host = host;
        self.dpi = unsafe { GetDpiForWindow(host).max(96) };
        unsafe { let _ = SetWindowSubclass(host, Some(host_proc), HOST_SUBCLASS, 0); }
    }

    /// Records the host, the content rectangle and the DPI from a paint, installs the host subclass
    /// once, and refreshes the EDIT colours from the palette.
    pub(crate) fn learn(&mut self, dc: HDC, rect: RECT, palette: &crate::theme::Palette) {
        let host = unsafe { WindowFromDC(dc) };
        if host != self.host && !host.is_invalid() {
            self.attach(host);
        }
        self.origin = rect;
        self.dpi = unsafe { GetDpiForWindow(self.host).max(96) };
        // EDIT colours come from the flattened palette, so a translucent fill reads as its tint.
        let flat = opaque(palette);
        let fill = colorref(flat.omnibox_fill).0;
        let text = colorref(flat.omnibox_text).0;
        EDIT_STYLE.with(|cell| {
            let mut style = cell.get();
            if style.fill != fill || style.brush == 0 {
                if style.brush != 0 {
                    unsafe {
                        let _ = DeleteObject(HBRUSH(style.brush as *mut std::ffi::c_void).into());
                    }
                }
                let brush = unsafe { windows::Win32::Graphics::Gdi::CreateSolidBrush(COLORREF(fill)) };
                style = EditStyle {
                    fill,
                    text,
                    brush: brush.0 as isize,
                };
            } else {
                style.text = text;
            }
            cell.set(style);
        });
    }

    /// Converts a content-relative rectangle to host client coordinates.
    pub(crate) fn to_host(self, rect: RECT) -> RECT {
        RECT {
            left: rect.left + self.origin.left,
            top: rect.top + self.origin.top,
            right: rect.right + self.origin.left,
            bottom: rect.bottom + self.origin.top,
        }
    }

    /// Converts a host client point to content coordinates.
    pub(crate) fn to_content(self, x: i32, y: i32) -> (i32, i32) {
        (x - self.origin.left, y - self.origin.top)
    }

    pub(crate) fn px(&self, dip: i32) -> i32 {
        scale(dip, self.dpi)
    }

    /// The owner window: the host's parent, which is the Bowser window.
    pub(crate) fn owner(&self) -> HWND {
        unsafe { GetParent(self.host).unwrap_or_default() }
    }

    pub(crate) fn invalidate(&self) {
        if !self.host.is_invalid() {
            unsafe {
                let _ = InvalidateRect(Some(self.host), Some(&self.origin), false);
            }
        }
    }

    pub(crate) fn focus_host(&self) {
        if !self.host.is_invalid() {
            unsafe {
                let _ = SetFocus(Some(self.host));
            }
        }
    }

    /// Screen position of the pointer, for menus.
    pub(crate) fn cursor(&self) -> windows::Win32::Foundation::POINT {
        let mut point = windows::Win32::Foundation::POINT::default();
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point);
        }
        point
    }
}

/// Narrower than this the panel is still animating open. Its layout math goes negative there, and the
/// rounded-rect drawing rejects a negative box, so the panel skips painting until it has room.
const MIN_PAINT_DIP: i32 = 160;

/// True while the panel is too narrow to lay out. Call after [`Surface::learn`], which records the width.
pub(crate) fn too_narrow(surface: &Surface) -> bool {
    surface.origin.right - surface.origin.left < scale(MIN_PAINT_DIP, surface.dpi)
}

/// Font for the panel's text at `dpi`. Owned by the caller, which deletes it. Grayscale antialiasing:
/// ClearType writes coloured fringes into the glass surface, which show as dark boxes behind text.
pub(crate) fn panel_font(dpi: u32) -> HFONT {
    create_message_font(dpi, ANTIALIASED_QUALITY)
}

/// Favicon bitmaps for rows, decoded once per host and size. A missing icon is cached as `None`,
/// so the letter tile shows without reading the disk on every paint.
#[derive(Default)]
pub(crate) struct IconCache {
    icons: HashMap<String, Option<Rc<FaviconBitmap>>>,
}

impl IconCache {
    pub(crate) fn get(&mut self, host: &str, px: u32) -> Option<Rc<FaviconBitmap>> {
        if host.is_empty() {
            return None;
        }
        self.icons
            .entry(host.to_owned())
            .or_insert_with(|| {
                favicon::load_favicon_bytes(host)
                    .ok()
                    .flatten()
                    .and_then(|bytes| favicon::decode(&bytes, px))
                    .map(Rc::new)
            })
            .clone()
    }

}

/// Shows a context menu under the pointer and returns the chosen item id.
pub(crate) fn context_menu(owner: HWND, anchor: windows::Win32::Foundation::POINT, entries: Vec<menu::Entry>) -> Option<usize> {
    menu::track_below(
        owner,
        anchor,
        menu::Menu {
            header: None,
            entries,
            footer: String::new(),
        },
    )
    .map(|chosen| chosen.id)
}

pub(crate) fn item(id: usize, label: impl Into<String>, icon: menu::Icon) -> menu::Entry {
    menu::Entry::Item(menu::Item::new(id, label, icon))
}

pub(crate) fn divider() -> menu::Entry {
    menu::Entry::Separator
}

/// Sends one typed character to an EDIT, as if the user had pressed the key.
pub(crate) fn type_into(edit: HWND, ch: char) {
    if !edit.is_invalid() {
        unsafe {
            SendMessageW(edit, WM_CHAR, Some(WPARAM(ch as usize)), Some(LPARAM(0)));
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_panel_title_is_skipped_when_host_closes() {
        assert!(nonempty_wide("").is_none());
        assert_eq!(nonempty_wide("Bookmarks"), Some("Bookmarks".encode_utf16().collect()));
    }

    fn m(row: i32, height: i32) -> Metrics {
        Metrics { row, height }
    }

    #[test]
    fn keyboard_movement_clamps_and_starts_from_the_edges() {
        assert_eq!(moved_selection(Key::Down, None, 3, 4), Some(0));
        assert_eq!(moved_selection(Key::Up, None, 3, 4), Some(2));
        assert_eq!(moved_selection(Key::Down, Some(2), 3, 4), Some(2));
        assert_eq!(moved_selection(Key::Up, Some(0), 3, 4), Some(0));
        assert_eq!(moved_selection(Key::Home, Some(2), 3, 4), Some(0));
        assert_eq!(moved_selection(Key::End, Some(0), 3, 4), Some(2));
        assert_eq!(moved_selection(Key::PageDown, Some(1), 10, 4), Some(5));
        assert_eq!(moved_selection(Key::PageUp, Some(2), 10, 4), Some(0));
        assert_eq!(moved_selection(Key::Enter, Some(1), 3, 4), None);
        assert_eq!(moved_selection(Key::Down, None, 0, 4), None);
    }

    #[test]
    fn virtual_keys_map_to_panel_keys() {
        assert_eq!(key_from_vk(0x26), Some(Key::Up));
        assert_eq!(key_from_vk(0x71), Some(Key::F2));
        assert_eq!(key_from_vk(0x2E), Some(Key::Delete));
        assert_eq!(key_from_vk(0x41), None);
    }

    #[test]
    fn visible_range_includes_partial_rows_at_both_edges() {
        assert_eq!(visible_range(0, 10, m(20, 50)), 0..3);
        assert_eq!(visible_range(30, 10, m(20, 50)), 1..4);
        assert_eq!(visible_range(0, 2, m(20, 50)), 0..2);
        assert_eq!(visible_range(0, 0, m(20, 50)), 0..0);
    }

    #[test]
    fn scroll_is_clamped_and_follows_the_selection_minimally() {
        assert_eq!(max_scroll(10, m(20, 50)), 150);
        assert_eq!(max_scroll(1, m(20, 50)), 0);
        assert_eq!(clamp_scroll(-5, 10, m(20, 50)), 0);
        assert_eq!(clamp_scroll(999, 10, m(20, 50)), 150);
        assert_eq!(scroll_to_show(0, 1, 10, m(20, 50)), 0);
        // Row 4 spans 80..100, so a 50px viewport must scroll to 50 to show its bottom edge.
        assert_eq!(scroll_to_show(0, 4, 10, m(20, 50)), 50);
        assert_eq!(scroll_to_show(100, 2, 10, m(20, 50)), 40);
        assert_eq!(scroll_to_show(120, 9, 10, m(20, 50)), 150);
    }

    #[test]
    fn row_hit_testing_uses_scroll_and_ignores_the_gap() {
        assert_eq!(row_at(5, 0, 10, m(20, 50)), Some(0));
        assert_eq!(row_at(25, 30, 10, m(20, 50)), Some(2));
        assert_eq!(row_at(49, 0, 2, m(20, 50)), None);
        assert_eq!(row_at(-1, 0, 10, m(20, 50)), None);
        assert_eq!(row_top(3, 30, m(20, 50)), 30);
    }

    #[test]
    fn rect_containment_is_half_open() {
        let rect = RECT { left: 8, top: 0, right: 20, bottom: 10 };
        assert!(contains(rect, 8, 0));
        assert!(contains(rect, 19, 9));
        assert!(!contains(rect, 20, 5));
        assert!(!contains(rect, 5, 5));
    }

    #[test]
    fn drop_slots_split_row_edges_and_folder_middles() {
        assert_eq!(slot_in_row(1, 40, false), Slot::Before);
        assert_eq!(slot_in_row(39, 40, false), Slot::After);
        assert_eq!(slot_in_row(15, 40, false), Slot::Before);
        assert_eq!(slot_in_row(25, 40, false), Slot::After);
        assert_eq!(slot_in_row(20, 40, true), Slot::Into);
        assert_eq!(slot_in_row(2, 40, true), Slot::Before);
    }

    #[test]
    fn insert_index_counts_siblings_without_the_dragged_item() {
        // Children of the destination, in order: 1, 2 (dragged), 3, 4.
        let siblings = [1, 2, 3, 4];
        assert_eq!(insert_index(&siblings, 2, Some(1), Slot::Before), 0);
        assert_eq!(insert_index(&siblings, 2, Some(3), Slot::Before), 1);
        assert_eq!(insert_index(&siblings, 2, Some(3), Slot::After), 2);
        assert_eq!(insert_index(&siblings, 2, Some(4), Slot::After), 3);
        assert_eq!(insert_index(&siblings, 9, None, Slot::Into), 4);
    }

    #[test]
    fn letter_tiles_and_host_labels_never_come_out_blank() {
        assert_eq!(letter_tile("github"), 'G');
        assert_eq!(letter_tile("  élan"), 'É');
        assert_eq!(letter_tile("...!"), '\u{2022}');
        assert_eq!(letter_tile(""), '\u{2022}');
        assert_eq!(host_label("https://www.Example.com/a?b=c"), "example.com");
        assert_eq!(host_label("https://docs.rs/x"), "docs.rs");
        assert_eq!(host_label("not a url"), "");
    }

    #[test]
    fn civil_days_match_known_epoch_values() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11017);
        assert_eq!(days_from_civil(2026, 10, 12), 20738);
    }

    #[test]
    fn local_offset_is_the_difference_between_two_readings() {
        // 2026-10-12 12:00 UTC is 14:00 at UTC+2.
        let utc = [2026, 10, 12, 12, 0, 0];
        let local = [2026, 10, 12, 14, 0, 0];
        assert_eq!(local_offset_secs(local, utc), 7200);
        // Across midnight: 23:00 UTC on the 12th is 01:00 on the 13th at UTC+2.
        assert_eq!(
            local_offset_secs([2026, 10, 13, 1, 0, 0], [2026, 10, 12, 23, 0, 0]),
            7200
        );
    }

    #[test]
    fn clock_and_local_day_shift_follow_the_offset() {
        let noon = days_from_civil(2026, 10, 10) as u64 * 86400 + 12 * 3600;
        assert_eq!(clock_label(noon, 0), "12:00");
        assert_eq!(clock_label(noon, -3 * 3600), "09:00");
        assert_eq!(clock_label(noon, 14 * 3600 + 1800 + 86400), "02:30");
        // 23:30 UTC on the 11th is already the 12th at UTC+1, so it groups with local day 12.
        let late = days_from_civil(2026, 10, 11) as u64 * 86400 + 23 * 3600 + 1800;
        assert_eq!(shift_to_local(late, 3600) / 86400, days_from_civil(2026, 10, 12) as u64);
        assert_eq!(late / 86400, days_from_civil(2026, 10, 11) as u64);
        assert_eq!(shift_to_local(10, -3600), 0);
    }

    #[test]
    fn tile_colour_is_stable_per_host() {
        assert_eq!(tile_colour("example.com"), tile_colour("example.com"));
    }

    #[test]
    fn translucent_tokens_flatten_over_the_window_tint() {
        let white = Rgba::hex(0xFFFFFF, 0xFF);
        let black = Rgba::hex(0x000000, 0xFF);
        // Opaque colours pass through unchanged.
        assert_eq!(flatten(white, black), white);
        // Half-white over black is mid grey, and the result is always opaque.
        let half = flatten(Rgba::hex(0xFFFFFF, 0x80), black);
        assert_eq!((half.r, half.g, half.b, half.a), (128, 128, 128, 0xFF));
        // A panel palette has no translucent token left after flattening.
        let palette = opaque(theme::palette(theme::Appearance::Dark, false));
        assert!(!palette.translucent);
        for token in [
            palette.tab_inactive,
            palette.tab_hover,
            palette.tab_active,
            palette.omnibox_fill,
            palette.hairline,
        ] {
            assert_eq!(token.a, 0xFF);
        }
        assert_eq!(palette.window.a, 0xFF);
    }

    #[test]
    fn danger_text_is_lighter_on_dark_surfaces_only() {
        let dark = theme::palette(theme::Appearance::Dark, true);
        let light = theme::palette(theme::Appearance::Light, true);
        assert_eq!(danger(light), DANGER);
        assert_ne!(danger(dark), DANGER);
    }

    #[test]
    fn scale_is_dpi_aware_and_never_below_96() {
        assert_eq!(scale(20, 96), 20);
        assert_eq!(scale(20, 144), 30);
        assert_eq!(scale(20, 0), 20);
    }

}
