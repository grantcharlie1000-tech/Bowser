//! Hover card for a tab: a native popup owned by the main window. It shows the tab's group marker
//! swatches and the other tabs in its group, and it never activates, so focus stays in the main
//! window.
//!
//! The main window feeds this module pointer movement and its one-shot hover timer. Clicks inside
//! the card call back into `App`, and the main window calls [`sync`] on every tab change so the card
//! follows the tab list. Everything runs on the UI thread, and no `STATE` borrow is held across a
//! Win32 call, because those calls can send messages straight back into this module.

use std::cell::{Cell, RefCell};

use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, ClientToScreen, CreateRoundRectRgn, CreateSolidBrush, DEFAULT_QUALITY,
            DRAW_TEXT_FORMAT, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE,
            DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRgn, HDC, HGDIOBJ,
            InvalidateRect, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, SetWindowRgn,
            TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClientRect,
                GetCursorPos, GetWindowLongPtrW, IDC_ARROW, KillTimer, LoadCursorW, RegisterClassW,
                SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, SetTimer, SetWindowPos,
                ShowWindow, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
                WM_NCDESTROY, WM_PAINT, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
                WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, w},
};

use super::{frame, glass, hover_square, island_menu, smooth};
use crate::{
    app::{App, AppRef},
    chrome::{
        layout::{Metrics, Rect},
        paint::create_message_font,
        tab_card::{self, CardHit, CardLayout, CardModel, MARKERS},
    },
    tabs::TabId,
    theme::{self, MARKER_COLOURS, Palette, Rgba},
};

const CLASS_NAME: PCWSTR = w!("BowserTabCard");
/// How long the pointer must rest on a tab before its card opens.
pub(crate) const HOVER_DELAY_MS: u32 = 3000;
const HOVER_TIMER_BASE: usize = 0x10_000;
/// One-shot leave check, re-armed only while the pointer is inside the card's square but off the card.
const WATCH_TIMER: usize = 0x7002;
const WATCH_INTERVAL_MS: u32 = 150;
/// `WM_MOUSEACTIVATE` result that keeps the card from taking activation on click.
const MA_NOACTIVATE: isize = 3;
const WM_MOUSELEAVE: u32 = 0x02A3;
/// Logical width of the marker ring drawn around the marker on the card.
const RING_WIDTH: i32 = 2;
/// Logical diameter of the favicon placeholder dot on a member row.
const ROW_DOT: i32 = 7;
const ROW_GAP: i32 = 6;

#[derive(Default)]
struct State {
    /// The tab under the pointer, whether or not a card may open for it. Changes only when the
    /// pointer moves to another tab, so repeated mouse moves never restart the hover timer.
    target: Option<TabId>,
    hover: HoverGeneration,
    open: Option<Open>,
}

#[derive(Default)]
struct HoverGeneration {
    generation: usize,
    timer_id: Option<usize>,
}

impl HoverGeneration {
    fn arm(&mut self) -> usize {
        self.generation = self.generation.wrapping_add(1);
        let id = HOVER_TIMER_BASE.wrapping_add(self.generation);
        self.timer_id = Some(id);
        id
    }

    fn cancel(&mut self) -> Option<usize> {
        self.generation = self.generation.wrapping_add(1);
        self.timer_id.take()
    }

    fn is_current(&self, id: usize) -> bool {
        self.timer_id == Some(id)
    }
}

struct Open {
    main: HWND,
    hwnd: HWND,
    tab: TabId,
    /// Card rectangle in screen coordinates, for the pointer-in-card test.
    screen: RECT,
    /// The tab and gap and card, in screen coordinates. The card stays open while the pointer is in it.
    square: RECT,
    model: CardModel,
    rows: Vec<TabId>,
    /// Layout in card-client coordinates.
    layout: CardLayout,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    static CLASS_REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// The tab under the pointer changed to `target` (`None` when it is over no tab). Closes any open
/// card and, for a tab that is not active, starts the hover timer.
pub(crate) fn hover_changed(main: HWND, app: &AppRef, target: Option<TabId>) {
    // The pointer is on the strip, so the strip reports its own leave. No re-check is needed meanwhile.
    stop_watch(main);
    // Empty strip inside the open card's square keeps the card. Leaving the square closes it.
    if target.is_none() && matches!(pointer_position(), Some((_, true))) {
        return;
    }
    let (changed, old_timer, new_timer) = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.target == target {
            (false, None, None)
        } else {
            state.target = target;
            let old_timer = state.hover.cancel();
            let new_timer = target
                .filter(|tab| App::active_tab_id(app) != *tab)
                .map(|_| state.hover.arm());
            (true, old_timer, new_timer)
        }
    });
    if !changed {
        return;
    }
    close();
    if let Some(timer) = old_timer {
        unsafe {
            let _ = KillTimer(Some(main), timer);
        }
    }
    // The active tab gets no card: its marker is already on the strip, and a card over the page
    // after every click on the current tab would be noise.
    if let Some(tab) = target
        && App::active_tab_id(app) != tab
    {
        if let Some(timer) = new_timer {
            unsafe {
                let _ = SetTimer(Some(main), timer, HOVER_DELAY_MS, None);
            }
        }
    }
}

pub(crate) fn is_hover_timer(timer: usize) -> bool {
    timer >= HOVER_TIMER_BASE || timer == WATCH_TIMER
}

/// Invalidates any queued hover timer and closes the card. Used on tab switches, drags and
/// deactivation so a stale timer cannot reopen a card after the interaction ended.
pub(crate) fn cancel(main: HWND) {
    cancel_card(main);
    // --- hover layer ---
    super::html_hover::cancel(main);
    // --- end hover layer ---
    // Island menus share the strip's clicks, drags and window changes, so they are cancelled too.
    island_menu::cancel(main);
}

/// Cancels the hover card and its timer only. Tab-list changes use this, because an island menu
/// follows its island through [`island_menu::sync`] instead of closing.
pub(crate) fn cancel_card(main: HWND) {
    // --- hover layer ---
    super::html_hover::cancel_card(main);
    // --- end hover layer ---
    let timer = STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.target = None;
        state.hover.cancel()
    });
    if let Some(timer) = timer {
        unsafe {
            let _ = KillTimer(Some(main), timer);
        }
    }
    close();
}

/// The hover timer fired: opens the card for the tab still under the pointer. `anchor` gives the
/// tab's rectangle in main-client coordinates, or `None` when it is not visible.
pub(crate) fn hover_timer(
    main: HWND,
    timer: usize,
    app: &AppRef,
    anchor: impl Fn(TabId) -> Option<Rect>,
) {
    if timer == WATCH_TIMER {
        // One-shot: stop it, then let the check arm it again while the pointer stays in the square.
        stop_watch(main);
        check_leave(main);
        return;
    }
    let target = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if !state.hover.is_current(timer) {
            return None;
        }
        state.hover.cancel();
        state.target
    });
    if target.is_none() {
        return;
    }
    unsafe {
        let _ = KillTimer(Some(main), timer);
    }
    let Some(tab) = target else {
        return;
    };
    if App::active_tab_id(app) == tab || island_menu::is_open() {
        return;
    }
    if let Some(rect) = anchor(tab) {
        show(main, app, tab, rect);
    }
}

/// The main window's pointer left it. Moving onto the page takes the pointer out of the window without
/// leaving the card's square, so the card decides from the pointer's position rather than from this
/// message alone.
pub(crate) fn pointer_left(main: HWND) {
    check_leave(main);
}

/// Keeps the card in step with the tab list: it follows its tab's position, refreshes its contents,
/// and closes when the tab is gone.
pub(crate) fn sync(main: HWND, app: &AppRef, anchor: impl Fn(TabId) -> Option<Rect>) {
    // --- hover layer ---
    super::html_hover::sync(main, app);
    // --- end hover layer ---
    let Some(tab) = STATE.with(|state| state.borrow().open.as_ref().map(|open| open.tab)) else {
        return;
    };
    match anchor(tab) {
        Some(rect) => show(main, app, tab, rect),
        None => close(),
    }
}

pub(crate) fn is_open() -> bool {
    STATE.with(|state| state.borrow().open.is_some())
}

/// Destroys the open card, if any.
pub(crate) fn close() {
    let open = STATE.with(|state| state.borrow_mut().open.take());
    if let Some(open) = open {
        unsafe {
            let _ = KillTimer(Some(open.main), WATCH_TIMER);
            let _ = DestroyWindow(open.hwnd);
        }
    }
}

/// Closes the card and forgets the hovered tab, so the next pointer move onto a tab starts a new
/// hover.
fn dismiss(main: HWND) {
    let timer = STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.target = None;
        state.hover.cancel()
    });
    close();
    if let Some(timer) = timer {
        unsafe {
            let _ = KillTimer(Some(main), timer);
        }
    }
}

#[cfg(test)]
mod hover_generation_tests {
    use super::*;

    #[test]
    fn preview_delay_is_three_seconds() {
        assert_eq!(HOVER_DELAY_MS, 3000);
    }

    #[test]
    fn rearming_invalidates_the_previous_timer_generation() {
        let mut hover = HoverGeneration::default();
        let first = hover.arm();
        let second = hover.arm();
        assert_ne!(first, second);
        assert!(!hover.is_current(first));
        assert!(hover.is_current(second));
    }

    #[test]
    fn cancel_invalidates_a_queued_timer() {
        let mut hover = HoverGeneration::default();
        let timer = hover.arm();
        assert_eq!(hover.cancel(), Some(timer));
        assert!(!hover.is_current(timer));
    }
}

/// Opens the card for `tab`, or moves and refreshes it when it is already open. Nothing is
/// activated: the window is shown with `SW_SHOWNOACTIVATE`.
fn show(main: HWND, app: &AppRef, tab: TabId, anchor: Rect) {
    let Some((model, rows)) = App::tab_card_model(app, tab) else {
        close();
        return;
    };
    let mut client = RECT::default();
    let dpi = unsafe {
        let _ = GetClientRect(main, &mut client);
        GetDpiForWindow(main).max(96)
    };
    let layout = tab_card::layout(anchor, client.right, dpi, rows.len());
    let card = layout.card;
    let mut origin = POINT::default();
    unsafe {
        let _ = ClientToScreen(main, &mut origin);
    }
    let screen = RECT {
        left: origin.x + card.left,
        top: origin.y + card.top,
        right: origin.x + card.right,
        bottom: origin.y + card.bottom,
    };
    let trigger = RECT {
        left: origin.x + anchor.left,
        top: origin.y + anchor.top,
        right: origin.x + anchor.right,
        bottom: origin.y + anchor.bottom,
    };
    let square = hover_square::square(
        trigger,
        screen,
        Metrics::new(dpi).px(hover_square::TOLERANCE),
    );
    let existing = STATE.with(|state| state.borrow().open.as_ref().map(|open| open.hwnd));
    let (hwnd, created) = match existing {
        Some(hwnd) => {
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    screen.left,
                    screen.top,
                    screen.right - screen.left,
                    screen.bottom - screen.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            (hwnd, false)
        }
        None => match create(main, &screen) {
            Some(hwnd) => (hwnd, true),
            None => return,
        },
    };
    shape(hwnd, screen, dpi);
    let open = Open {
        main,
        hwnd,
        tab,
        screen,
        square,
        model,
        rows,
        layout: localise(&layout, card.left, card.top),
    };
    STATE.with(|state| state.borrow_mut().open = Some(open));
    unsafe {
        if created {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        } else {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}

fn create(main: HWND, screen: &RECT) -> Option<HWND> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None).ok()?.0);
        if !CLASS_REGISTERED.with(Cell::get) {
            let class = WNDCLASSW {
                lpfnWndProc: Some(card_proc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: CLASS_NAME,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return None;
            }
            CLASS_REGISTERED.with(|registered| registered.set(true));
        }
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            CLASS_NAME,
            w!(""),
            WS_POPUP,
            screen.left,
            screen.top,
            screen.right - screen.left,
            screen.bottom - screen.top,
            Some(main),
            None,
            Some(instance),
            None,
        )
        .ok()
    }
}

/// Clips the window to the rounded card. The system owns the region once `SetWindowRgn` succeeds,
/// so a new one is made for every size.
fn shape(hwnd: HWND, screen: RECT, dpi: u32) {
    let (width, height) = (screen.right - screen.left, screen.bottom - screen.top);
    let radius = Metrics::new(dpi).px(tab_card::CARD_RADIUS);
    unsafe {
        let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2);
        if SetWindowRgn(hwnd, Some(region), true) == 0 {
            let _ = DeleteObject(HGDIOBJ(region.0));
        }
    }
}

fn localise(layout: &CardLayout, dx: i32, dy: i32) -> CardLayout {
    let shift = move |r: Rect| Rect::new(r.left - dx, r.top - dy, r.right - dx, r.bottom - dy);
    CardLayout {
        card: shift(layout.card),
        swatches: layout.swatches.map(shift),
        new_marker: shift(layout.new_marker),
        title: shift(layout.title),
        domain: shift(layout.domain),
        rows: layout.rows.iter().copied().map(shift).collect(),
        hidden_rows: layout.hidden_rows,
        more: layout.more.map(shift),
    }
}

/// Where the pointer is relative to the open card: over the card, and inside its square. `None` when
/// no card is open or the pointer cannot be read.
fn pointer_position() -> Option<(bool, bool)> {
    let mut cursor = POINT::default();
    if unsafe { GetCursorPos(&mut cursor) }.is_err() {
        return None;
    }
    STATE.with(|state| {
        state.borrow().open.as_ref().map(|open| {
            (
                hover_square::contains(open.screen, cursor),
                hover_square::contains(open.square, cursor),
            )
        })
    })
}

/// Stops the pending leave check, if one is armed.
fn stop_watch(main: HWND) {
    unsafe {
        let _ = KillTimer(Some(main), WATCH_TIMER);
    }
}

/// Decides, from where the pointer is now, whether the open card stays, waits for another check in
/// the gap, or closes. Closing lets a return to the tab start a fresh hover.
fn check_leave(main: HWND) {
    if !is_open() {
        stop_watch(main);
        return;
    }
    let (in_card, in_square) = pointer_position().unwrap_or((false, false));
    match hover_square::on_leave(in_card, in_square, false) {
        hover_square::Leave::Keep => stop_watch(main),
        hover_square::Leave::Watch => unsafe {
            let _ = SetTimer(Some(main), WATCH_TIMER, WATCH_INTERVAL_MS, None);
        },
        hover_square::Leave::Close => dismiss(main),
    }
}

/// The pointer left the card. It stays while the pointer is in the square, such as in the gap.
fn card_left(hwnd: HWND) {
    let main = STATE.with(|state| {
        state
            .borrow()
            .open
            .as_ref()
            .filter(|open| open.hwnd == hwnd)
            .map(|open| open.main)
    });
    if let Some(main) = main {
        check_leave(main);
    }
}

/// Runs the action for a click on the card. The main window is found through its user data, the
/// same way the main window's own handlers reach `App`.
fn click(hwnd: HWND, x: i32, y: i32) {
    let target = STATE.with(|state| {
        let state = state.borrow();
        let open = state.open.as_ref().filter(|open| open.hwnd == hwnd)?;
        Some((
            open.main,
            open.tab,
            open.rows.clone(),
            tab_card::hit_test(&open.layout, x, y),
        ))
    });
    let Some((main, tab, rows, hit)) = target else {
        return;
    };
    let app = unsafe {
        (GetWindowLongPtrW(main, GWLP_USERDATA) as *const AppRef)
            .as_ref()
            .cloned()
    };
    let Some(app) = app else {
        return;
    };
    match hit {
        CardHit::Swatch(index) => {
            let current = App::tab_marker(&app, tab);
            App::set_tab_marker(&app, tab, tab_card::toggle_marker(current, MARKERS[index]));
        }
        CardHit::NewMarker => {
            let current = App::tab_marker(&app, tab);
            App::set_tab_marker(&app, tab, tab_card::next_marker(current));
        }
        CardHit::Row(index) => {
            if let Some(&row) = rows.get(index) {
                dismiss(main);
                App::select_tab(&app, row);
            }
        }
        CardHit::None => {}
    }
}

/// Forgets the card when its window goes away, however that happened.
fn forget(hwnd: HWND) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.open.as_ref().is_some_and(|open| open.hwnd == hwnd) {
            state.open = None;
        }
    });
}

unsafe extern "system" fn card_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE),
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut track);
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                card_left(hwnd);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let p = frame::point_from_lparam(lparam);
                click(hwnd, p.x, p.y);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                forget(hwnd);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

fn palette() -> &'static Palette {
    // The card is opaque: it is a popup over the page, not part of the glass chrome.
    theme::palette(glass::appearance(), true)
}

fn colorref(colour: Rgba) -> COLORREF {
    COLORREF(u32::from(colour.r) | (u32::from(colour.g) << 8) | (u32::from(colour.b) << 16))
}

fn to_rect(rect: Rect) -> RECT {
    RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

unsafe fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        STATE.with(|state| {
            if let Some(open) = state
                .borrow()
                .open
                .as_ref()
                .filter(|open| open.hwnd == hwnd)
            {
                draw(dc, hwnd, open);
            }
        });
        let _ = EndPaint(hwnd, &ps);
    }
}

unsafe fn draw(dc: HDC, hwnd: HWND, open: &Open) {
    unsafe {
        let dpi = GetDpiForWindow(hwnd).max(96);
        let m = Metrics::new(dpi);
        let palette = palette();
        let layout = &open.layout;
        let (width, height) = (layout.card.width(), layout.card.height());
        let client = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };

        let background = CreateSolidBrush(colorref(palette.window));
        FillRect(dc, &client, background);
        let outline = CreateRoundRectRgn(
            0,
            0,
            width + 1,
            height + 1,
            m.px(tab_card::CARD_RADIUS) * 2,
            m.px(tab_card::CARD_RADIUS) * 2,
        );
        let border = CreateSolidBrush(colorref(palette.hairline));
        let _ = FrameRgn(dc, outline, border, 1, 1);
        let _ = DeleteObject(HGDIOBJ(outline.0));
        let _ = DeleteObject(background.into());
        let _ = DeleteObject(border.into());

        let font = create_message_font(dpi, DEFAULT_QUALITY);
        let old_font = SelectObject(dc, font.into());
        SetBkMode(dc, TRANSPARENT);

        let ring = m.px(RING_WIDTH).max(1);
        for (index, swatch) in layout.swatches.iter().enumerate() {
            let rect = to_rect(*swatch);
            fill_ellipse(dc, rect, MARKER_COLOURS[index]);
            if open.model.marker == Some(MARKERS[index]) {
                let grow = ring + 1;
                let outer = RECT {
                    left: rect.left - grow,
                    top: rect.top - grow,
                    right: rect.right + grow,
                    bottom: rect.bottom + grow,
                };
                stroke_ellipse(dc, outer, palette.tab_text_active, ring);
            }
        }
        let plus = to_rect(layout.new_marker);
        stroke_round_rect(dc, plus, m.px(6), palette.hairline, 1);
        draw_text(
            dc,
            plus,
            "+",
            palette.tab_text_inactive,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );

        draw_text(
            dc,
            to_rect(layout.title),
            &open.model.title,
            palette.tab_text_active,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        if let Some(domain) = &open.model.domain {
            draw_text(
                dc,
                to_rect(layout.domain),
                domain,
                palette.tab_text_inactive,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
        }

        let dot = m.px(ROW_DOT).max(3);
        let gap = m.px(ROW_GAP);
        for (index, row_rect) in layout.rows.iter().enumerate() {
            let Some(row) = open.model.rows.get(index) else {
                continue;
            };
            let rect = to_rect(*row_rect);
            let centre = (rect.top + rect.bottom) / 2;
            let favicon = RECT {
                left: rect.left,
                top: centre - dot / 2,
                right: rect.left + dot,
                bottom: centre - dot / 2 + dot,
            };
            let dot_colour = if row.favicon_present {
                palette.tab_text_inactive
            } else {
                palette.tab_active_border
            };
            fill_ellipse(dc, favicon, dot_colour);

            let text = RECT {
                left: favicon.right + gap,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            };
            let colour = if row.active {
                palette.tab_text_active
            } else {
                palette.tab_text_inactive
            };
            draw_text(
                dc,
                text,
                &row.title,
                colour,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
            if row.linked {
                let underline = m.px(2).max(1);
                draw_line(
                    dc,
                    text.left,
                    rect.bottom - underline,
                    text.right,
                    rect.bottom - underline,
                    palette.tab_group_accent,
                    underline,
                );
            }
        }

        if let Some(more) = layout.more {
            let text = format!("+{} more", layout.hidden_rows);
            draw_text(
                dc,
                to_rect(more),
                &text,
                palette.tab_text_inactive,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        }

        SelectObject(dc, old_font);
        let _ = DeleteObject(font.into());
    }
}

unsafe fn fill_ellipse(dc: HDC, rect: RECT, fill: Rgba) {
    unsafe { smooth::fill_ellipse(dc, rect, fill) }
}

unsafe fn stroke_ellipse(dc: HDC, rect: RECT, colour: Rgba, width: i32) {
    unsafe { smooth::stroke_ellipse(dc, rect, colour, width) }
}

unsafe fn stroke_round_rect(dc: HDC, rect: RECT, radius: i32, colour: Rgba, width: i32) {
    unsafe { smooth::stroke_round_rect(dc, rect, radius, colour, width) }
}

unsafe fn draw_line(dc: HDC, x0: i32, y0: i32, x1: i32, y1: i32, colour: Rgba, width: i32) {
    unsafe {
        smooth::stroke_line(
            dc,
            POINT { x: x0, y: y0 },
            POINT { x: x1, y: y1 },
            colour,
            width,
        )
    }
}

unsafe fn draw_text(dc: HDC, bounds: RECT, text: &str, colour: Rgba, format: DRAW_TEXT_FORMAT) {
    unsafe {
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        if wide.is_empty() {
            return;
        }
        let mut bounds = bounds;
        SetTextColor(dc, colorref(colour));
        DrawTextW(dc, &mut wide, &mut bounds, format);
    }
}
