//! Tab island menu: a native popup under a domain island's bar, opened by hovering the bar. It holds
//! the island's name field, colour swatches, member tabs, and a close button for the whole island.
//!
//! The popup is owned by the main window and is activated only by a click inside it, so hovering
//! never takes focus from the address field. The name is a native EDIT child, which gives real caret,
//! selection, clipboard, and IME handling. Enter commits the name and closes; Escape reverts it and
//! closes. Opening waits [`HOVER_DELAY_MS`] after the pointer reaches the bar. The menu stays open
//! while the pointer is inside its square (the bar, the gap, and the popup) or a field in it has
//! focus. Outside the square it closes, and a one-shot check re-runs while the pointer sits in the gap.
//!
//! Everything runs on the UI thread, and no `STATE` borrow is held across a Win32 call, because
//! those calls can send messages straight back into this module.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            AC_SRC_ALPHA, AC_SRC_OVER, AlphaBlend, BI_RGB, BITMAPINFO, BITMAPINFOHEADER,
            BLENDFUNCTION, BeginPaint, CreateCompatibleDC, CreateDIBSection, CreateRoundRectRgn,
            CreateSolidBrush, DEFAULT_QUALITY, DIB_RGB_COLORS, DRAW_TEXT_FORMAT, DT_CENTER,
            DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC,
            DeleteObject, DrawTextW, EndPaint, FillRect, FrameRgn, HBRUSH, HDC, HFONT, HGDIOBJ,
            InvalidateRect, PAINTSTRUCT, SelectObject, SetBkColor, SetBkMode, SetTextColor,
            SetWindowRgn, TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{
                GetFocus, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_ESCAPE,
                VK_RETURN,
            },
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, ES_AUTOHSCROLL, GetClientRect,
                GetCursorPos, GetWindowTextLengthW, GetWindowTextW, IDC_ARROW, KillTimer,
                LoadCursorW, PostMessageW, RegisterClassW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
                SWP_NOZORDER, SetForegroundWindow, SetTimer, SetWindowPos, SetWindowTextW,
                ShowWindow, WM_APP, WM_CHAR, WM_CTLCOLOREDIT, WM_ERASEBKGND, WM_KEYDOWN,
                WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT, WM_SETFONT, WNDCLASSW,
                WS_CHILD, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
            },
        },
    },
    core::{PCWSTR, w},
};

use super::{frame, glass, hover_square, smooth};
use crate::{
    app::{App, AppRef},
    chrome::{
        island_menu::{self as menu, IslandHit, IslandLayout, IslandModel},
        layout::{Metrics, Rect},
        paint::create_message_font,
        tab_card::NEW_TAB_TITLE,
    },
    favicon::FaviconBitmap,
    tab_groups,
    tabs::TabId,
    theme::{self, ISLAND_COLOURS, Palette, Rgba},
};

const CLASS_NAME: PCWSTR = w!("BowserIslandMenu");
/// How long the pointer must rest on a bar before its menu opens.
pub(crate) const HOVER_DELAY_MS: u32 = 300;
/// Re-check interval while the pointer is in the square but off the menu, such as in the gap.
const CHECK_INTERVAL_MS: u32 = 150;
/// One-shot timers. Ids stay below the tab card's hover timer range.
const OPEN_TIMER: usize = 0x7000;
const CLOSE_TIMER: usize = 0x7001;
/// `EN_KILLFOCUS` notification code, sent from the name field when it loses the keyboard.
const EN_KILLFOCUS: u32 = 0x0200;
/// Posted from the name field to the popup: `wparam` is 1 to commit the name and 0 to revert it.
const WM_FINISH: u32 = WM_APP + 0x31;
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_COMMAND: u32 = 0x0111;
/// `EN_CHANGE` notification code, sent in the high word of `WM_COMMAND` from an EDIT.
const EN_CHANGE: u32 = 0x0300;
/// Subclass id for the name field.
const EDIT_SUBCLASS: usize = 0x7A11;
const FAVICON_SLOT: i32 = 16;
const ROW_DOT: i32 = 7;
const ROW_GAP: i32 = 6;
const RING_WIDTH: i32 = 2;
const CLEAR_STROKE: i32 = 2;

#[derive(Default)]
struct State {
    /// The bar under the pointer: its leader tab and bar rectangle in main-client coordinates.
    bar: Option<(TabId, Rect)>,
    open: Option<Open>,
}

struct Open {
    main: HWND,
    hwnd: HWND,
    edit: HWND,
    app: AppRef,
    tab: TabId,
    /// Popup rectangle in screen coordinates, for the pointer-in-popup test.
    screen: RECT,
    /// The bar and the gap and the popup, in screen coordinates. The menu stays open inside it.
    square: RECT,
    model: IslandModel,
    /// Member tab ids, in the same order as `model.rows`.
    ids: Vec<TabId>,
    /// Favicons for `ids`, as the chrome last decoded them.
    favicons: Vec<Option<Rc<FaviconBitmap>>>,
    /// Layout in popup-client coordinates.
    layout: IslandLayout,
    /// The name as stored when the popup last refreshed. The draft in `edit` is compared with it.
    committed: Option<String>,
    font: HFONT,
    field_brush: HBRUSH,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    static CLASS_REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// The pointer is over the bar of the domain island led by `bar.0`, or over no bar (`None`). The
/// menu opens after [`HOVER_DELAY_MS`] for the bar the pointer rests on, and closes after the
/// pointer leaves both the bar and the menu.
pub(crate) fn bar_hover(main: HWND, bar: Option<(TabId, Rect)>) {
    let (same_bar, open_tab) = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let same = state.bar.map(|(tab, _)| tab) == bar.map(|(tab, _)| tab);
        state.bar = bar;
        (same, state.open.as_ref().map(|open| open.tab))
    });
    if same_bar {
        return;
    }
    unsafe {
        let _ = KillTimer(Some(main), OPEN_TIMER);
    }
    match bar {
        Some((tab, _)) => {
            unsafe {
                let _ = KillTimer(Some(main), CLOSE_TIMER);
            }
            if open_tab == Some(tab) {
                // Back on the bar of the open menu: keep it open.
                return;
            }
            unsafe {
                let _ = SetTimer(Some(main), OPEN_TIMER, HOVER_DELAY_MS, None);
            }
        }
        None => check_leave(main),
    }
}

/// Timer handler. The open timer shows the menu for the bar still under the pointer; the close
/// timer closes it when the pointer is over neither the bar nor the menu. `anchor` gives the bar's
/// current rectangle for a tab, or `None` when that bar is scrolled out of view.
pub(crate) fn timer(
    main: HWND,
    timer: usize,
    app: &AppRef,
    anchor: impl Fn(TabId) -> Option<Rect>,
) {
    unsafe {
        let _ = KillTimer(Some(main), timer);
    }
    match timer {
        OPEN_TIMER => {
            let bar = STATE.with(|state| state.borrow().bar);
            let Some((tab, _)) = bar else {
                return;
            };
            if let Some(rect) = anchor(tab) {
                show(main, app, tab, rect);
            }
        }
        CLOSE_TIMER => check_leave(main),
        _ => {}
    }
}

pub(crate) fn is_timer(timer: usize) -> bool {
    timer == OPEN_TIMER || timer == CLOSE_TIMER
}

/// The main window's pointer left it. Moving onto the page or onto the menu takes the pointer out of
/// the window, so the menu decides from the pointer's position.
pub(crate) fn pointer_left(main: HWND) {
    STATE.with(|state| state.borrow_mut().bar = None);
    check_leave(main);
}

/// Where the pointer is relative to the open menu: over the menu, and inside its square. `None` when
/// no menu is open or the pointer cannot be read.
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

/// Decides, from where the pointer and the keyboard focus are now, whether the open menu stays, waits
/// for another check in the gap, or closes. A focused name field keeps it open.
fn check_leave(main: HWND) {
    let Some(edit) = STATE.with(|state| state.borrow().open.as_ref().map(|open| open.edit)) else {
        unsafe {
            let _ = KillTimer(Some(main), CLOSE_TIMER);
        }
        return;
    };
    let (in_menu, in_square) = pointer_position().unwrap_or((false, false));
    let focused = unsafe { GetFocus() } == edit;
    match hover_square::on_leave(in_menu, in_square, focused) {
        hover_square::Leave::Keep => unsafe {
            let _ = KillTimer(Some(main), CLOSE_TIMER);
        },
        hover_square::Leave::Watch => unsafe {
            let _ = SetTimer(Some(main), CLOSE_TIMER, CHECK_INTERVAL_MS, None);
        },
        hover_square::Leave::Close => finish(true),
    }
}

/// Keeps the menu in step with the tab list: it follows the island's bar, refreshes its contents,
/// and closes when the bar is gone.
pub(crate) fn sync(main: HWND, app: &AppRef, anchor: impl Fn(TabId) -> Option<Rect>) {
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

/// Stops every timer and closes the menu at once. Used for clicks, drags, and card-level cancels.
pub(crate) fn cancel(main: HWND) {
    // The hovered bar is kept, so a small move on the same bar does not reopen the menu straight
    // after a click. Leaving the bar clears it as usual.
    unsafe {
        let _ = KillTimer(Some(main), OPEN_TIMER);
        let _ = KillTimer(Some(main), CLOSE_TIMER);
    }
    close();
}

/// Commits the draft name and destroys the menu, if one is open.
pub(crate) fn close() {
    finish(true);
}

/// Takes the open menu out of state, commits its name when `commit` is set, and destroys it. The
/// state borrow is released before any Win32 call.
fn finish(commit: bool) {
    let open = STATE.with(|state| state.borrow_mut().open.take());
    let Some(open) = open else {
        return;
    };
    if commit {
        commit_name(&open);
    }
    // Only a menu that held keyboard focus gives it back, so closing on a hover-out never pulls
    // focus away from another application.
    let had_focus = unsafe { GetFocus() } == open.edit;
    unsafe {
        let _ = DestroyWindow(open.hwnd);
        let _ = DeleteObject(open.font.into());
        let _ = DeleteObject(open.field_brush.into());
        if had_focus {
            let _ = SetForegroundWindow(open.main);
        }
    }
}

fn commit_name(open: &Open) {
    let draft = edit_text(open.edit);
    let cleaned = tab_groups::clean_name(&draft);
    if cleaned != open.committed {
        let _ = App::set_tab_group_name(&open.app, open.tab, &draft);
    }
}

fn edit_text(edit: HWND) -> String {
    unsafe {
        let length = GetWindowTextLengthW(edit).max(0) as usize;
        let mut units = vec![0u16; length + 1];
        let written = GetWindowTextW(edit, &mut units).max(0) as usize;
        String::from_utf16_lossy(&units[..written])
    }
}

/// Opens the menu for `tab`'s island, or refreshes it in place when it is already open for that
/// island (keeping any text the user is typing). Nothing is activated.
fn show(main: HWND, app: &AppRef, tab: TabId, anchor: Rect) {
    let Some((model, ids)) = App::island_menu(app, tab) else {
        close();
        return;
    };
    let favicons: Vec<_> = ids
        .iter()
        .map(|&id| crate::win32::window::tab_favicon(main, id))
        .collect();
    let mut client = RECT::default();
    let dpi = unsafe {
        let _ = GetClientRect(main, &mut client);
        GetDpiForWindow(main).max(96)
    };
    let layout = menu::layout(anchor, client.right, dpi, model.rows.len());
    let card = layout.card;
    let mut origin = POINT::default();
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(main, &mut origin);
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

    let existing = STATE.with(|state| {
        state
            .borrow()
            .open
            .as_ref()
            .filter(|open| open.tab == tab)
            .map(|open| (open.hwnd, open.edit))
    });
    if let Some((hwnd, edit)) = existing {
        STATE.with(|state| {
            if let Some(open) = state.borrow_mut().open.as_mut() {
                open.screen = screen;
                open.square = square;
                open.committed = model.name.clone();
                open.model = model.clone();
                open.ids = ids;
                open.favicons = favicons;
                open.layout = layout.translated(card.left, card.top);
            }
        });
        place(hwnd, edit, screen, &layout.translated(card.left, card.top));
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        return;
    }

    // A different island's menu commits its draft and closes before the new one opens.
    finish(true);
    let Some(hwnd) = create(main, &screen) else {
        return;
    };
    let Some(edit) = create_edit(hwnd, dpi, &model) else {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return;
    };
    let font = create_message_font(dpi, DEFAULT_QUALITY);
    let field_brush = unsafe { CreateSolidBrush(colorref(field_fill(palette()))) };
    let local = layout.translated(card.left, card.top);
    shape(hwnd, screen, dpi);
    place(hwnd, edit, screen, &local);
    let open = Open {
        main,
        hwnd,
        edit,
        app: app.clone(),
        tab,
        screen,
        square,
        committed: model.name.clone(),
        model,
        ids,
        favicons,
        layout: local,
        font,
        field_brush,
    };
    STATE.with(|state| state.borrow_mut().open = Some(open));
    // The popup and the hover card should not overlap, so the card closes when the menu opens.
    crate::win32::card::close();
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
}

fn place(hwnd: HWND, edit: HWND, screen: RECT, local: &IslandLayout) {
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
        let _ = SetWindowPos(
            edit,
            None,
            local.edit.left,
            local.edit.top,
            local.edit.width(),
            local.edit.height(),
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

fn create(main: HWND, screen: &RECT) -> Option<HWND> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None).ok()?.0);
        if !CLASS_REGISTERED.with(Cell::get) {
            // The default WM_MOUSEACTIVATE lets a click activate the popup, so the name field can
            // take keyboard focus. Hovering never activates it.
            let class = WNDCLASSW {
                lpfnWndProc: Some(island_proc),
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
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            CLASS_NAME,
            w!(""),
            WS_POPUP | WS_CLIPCHILDREN,
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

/// The name field: a native single-line EDIT, subclassed so Enter and Escape reach the popup.
fn create_edit(popup: HWND, dpi: u32, model: &IslandModel) -> Option<HWND> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None).ok()?.0);
        let edit = CreateWindowExW(
            windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
            w!("EDIT"),
            w!(""),
            WS_CHILD
                | WS_VISIBLE
                | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            0,
            0,
            0,
            0,
            Some(popup),
            None,
            Some(instance),
            None,
        )
        .ok()?;
        let font = create_message_font(dpi, DEFAULT_QUALITY);
        set_font(edit, font);
        let name = model.name.clone().unwrap_or_default();
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = SetWindowTextW(edit, PCWSTR(wide.as_ptr()));
        let placeholder: Vec<u16> = menu::PLACEHOLDER
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // EM_SETCUEBANNER with wParam 1 keeps the placeholder visible while the field has focus.
        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            edit,
            0x1501,
            Some(WPARAM(1)),
            Some(LPARAM(placeholder.as_ptr() as isize)),
        );
        if !SetWindowSubclass(edit, Some(edit_proc), EDIT_SUBCLASS, popup.0 as usize).as_bool() {
            let _ = DestroyWindow(edit);
            return None;
        }
        Some(edit)
    }
}

unsafe fn set_font(edit: HWND, font: HFONT) {
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            edit,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
}

/// Clips the popup to the rounded card, as the hover card does.
fn shape(hwnd: HWND, screen: RECT, dpi: u32) {
    let (width, height) = (screen.right - screen.left, screen.bottom - screen.top);
    let radius = Metrics::new(dpi).px(menu::CARD_RADIUS);
    unsafe {
        let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2);
        if SetWindowRgn(hwnd, Some(region), true) == 0 {
            let _ = DeleteObject(HGDIOBJ(region.0));
        }
    }
}

/// Subclass for the name field. Enter and Escape are posted to the popup, which commits or reverts
/// and closes. Their character messages are swallowed so the field does not beep.
unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    popup: usize,
) -> LRESULT {
    unsafe {
        let popup = HWND(popup as *mut std::ffi::c_void);
        match message {
            WM_KEYDOWN if wparam.0 == VK_RETURN.0 as usize => {
                let _ = PostMessageW(Some(popup), WM_FINISH, WPARAM(1), LPARAM(0));
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
                let _ = PostMessageW(Some(popup), WM_FINISH, WPARAM(0), LPARAM(0));
                LRESULT(0)
            }
            WM_CHAR if wparam.0 == 0x0D || wparam.0 == 0x1B => LRESULT(0),
            windows::Win32::UI::WindowsAndMessaging::WM_NCDESTROY => {
                let _ = RemoveWindowSubclass(hwnd, Some(edit_proc), EDIT_SUBCLASS);
                DefSubclassProc(hwnd, message, wparam, lparam)
            }
            _ => DefSubclassProc(hwnd, message, wparam, lparam),
        }
    }
}

/// Runs the action for a click inside the popup.
fn click(hwnd: HWND, x: i32, y: i32) {
    let target = STATE.with(|state| {
        let state = state.borrow();
        let open = state.open.as_ref().filter(|open| open.hwnd == hwnd)?;
        Some((
            open.app.clone(),
            open.tab,
            open.ids.clone(),
            open.edit,
            menu::hit_test(&open.layout, x, y),
        ))
    });
    let Some((app, tab, ids, edit, hit)) = target else {
        return;
    };
    match hit {
        IslandHit::Field => unsafe {
            let _ = SetFocus(Some(edit));
        },
        IslandHit::Clear => unsafe {
            let _ = SetWindowTextW(edit, PCWSTR::null());
            let _ = SetFocus(Some(edit));
            let _ = InvalidateRect(Some(hwnd), None, false);
        },
        IslandHit::Swatch(slot) => {
            let _ = App::set_tab_group_colour(&app, tab, menu::swatch_colour(slot));
        }
        IslandHit::Row(index) => {
            if let Some(&row) = ids.get(index) {
                finish(true);
                let _ = App::select_tab(&app, row);
            }
        }
        IslandHit::Close => {
            finish(true);
            App::close_tab_island(&app, tab);
        }
        IslandHit::None => {}
    }
}

/// The main window that owns the open popup `hwnd`, which is where its timers live.
fn main_of(hwnd: HWND) -> Option<HWND> {
    STATE.with(|state| {
        state
            .borrow()
            .open
            .as_ref()
            .filter(|open| open.hwnd == hwnd)
            .map(|open| open.main)
    })
}

/// The pointer left the popup. It stays while the pointer is in the square, such as in the gap.
fn popup_left(hwnd: HWND) {
    if let Some(main) = main_of(hwnd) {
        check_leave(main);
    }
}

fn popup_entered(hwnd: HWND) {
    let main = STATE.with(|state| {
        state
            .borrow()
            .open
            .as_ref()
            .filter(|open| open.hwnd == hwnd)
            .map(|open| open.main)
    });
    if let Some(main) = main {
        unsafe {
            let _ = KillTimer(Some(main), CLOSE_TIMER);
        }
    }
}

/// Forgets the menu when its window goes away, however that happened.
fn forget(hwnd: HWND) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.open.as_ref().is_some_and(|open| open.hwnd == hwnd) {
            state.open = None;
        }
    });
}

unsafe extern "system" fn island_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_CTLCOLOREDIT => {
                let dc = HDC(wparam.0 as *mut std::ffi::c_void);
                let brush = STATE.with(|state| {
                    state
                        .borrow()
                        .open
                        .as_ref()
                        .filter(|open| open.hwnd == hwnd)
                        .map(|open| open.field_brush)
                });
                match brush {
                    Some(brush) => {
                        let palette = palette();
                        SetTextColor(dc, colorref(palette.omnibox_text));
                        SetBkColor(dc, colorref(field_fill(palette)));
                        LRESULT(brush.0 as isize)
                    }
                    None => DefWindowProcW(hwnd, message, wparam, lparam),
                }
            }
            WM_MOUSEMOVE => {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut track);
                popup_entered(hwnd);
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                popup_left(hwnd);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let p = frame::point_from_lparam(lparam);
                click(hwnd, p.x, p.y);
                LRESULT(0)
            }
            // The name field notifies its parent on every change, so the clear button can appear
            // or disappear with the text.
            WM_COMMAND if (wparam.0 >> 16) as u32 == EN_CHANGE => {
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            // Losing the keyboard can leave the pointer outside the square, so the menu re-checks on
            // the owner's timer rather than closing from inside the field's notification.
            WM_COMMAND if (wparam.0 >> 16) as u32 == EN_KILLFOCUS => {
                if let Some(main) = main_of(hwnd) {
                    let _ = SetTimer(Some(main), CLOSE_TIMER, CHECK_INTERVAL_MS, None);
                }
                LRESULT(0)
            }
            WM_FINISH => {
                finish(wparam.0 != 0);
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
    // The menu is opaque: it is a popup over the page, not part of the glass chrome.
    theme::palette(glass::appearance(), true)
}

/// `from` moved `amount` of the way toward `to`, opaque.
fn blend(from: Rgba, to: Rgba, amount: f32) -> Rgba {
    let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * amount).round() as u8;
    Rgba {
        r: mix(from.r, to.r),
        g: mix(from.g, to.g),
        b: mix(from.b, to.b),
        a: 255,
    }
}

fn is_dark(palette: &Palette) -> bool {
    let w = palette.window;
    (u32::from(w.r) * 30 + u32::from(w.g) * 59 + u32::from(w.b) * 11) / 100 < 128
}

/// The card's fill: lifted a little from the window so the popup separates from the page.
fn panel_fill(palette: &Palette) -> Rgba {
    blend(palette.window, palette.tab_text_active, 0.07)
}

/// The card's outline: a clear contrasting line.
fn panel_border(palette: &Palette) -> Rgba {
    blend(panel_fill(palette), palette.tab_text_active, 0.40)
}

/// The name field's fill: a recessed well in dark themes, a white field in light ones.
fn field_fill(palette: &Palette) -> Rgba {
    let panel = panel_fill(palette);
    if is_dark(palette) {
        blend(panel, Rgba::hex(0x000000, 255), 0.35)
    } else {
        Rgba::hex(0xFFFFFF, 255)
    }
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

        let background = CreateSolidBrush(colorref(panel_fill(palette)));
        FillRect(dc, &client, background);
        let outline = CreateRoundRectRgn(
            0,
            0,
            width + 1,
            height + 1,
            m.px(menu::CARD_RADIUS) * 2,
            m.px(menu::CARD_RADIUS) * 2,
        );
        let border = CreateSolidBrush(colorref(panel_border(palette)));
        let _ = FrameRgn(dc, outline, border, 1, 1);
        let _ = DeleteObject(HGDIOBJ(outline.0));
        let _ = DeleteObject(background.into());
        let _ = DeleteObject(border.into());

        let font = create_message_font(dpi, DEFAULT_QUALITY);
        let old_font = SelectObject(dc, font.into());
        SetBkMode(dc, TRANSPARENT);

        // Name field: rounded, with a focus ring when the EDIT owns the keyboard.
        let field = to_rect(layout.field);
        let focused = GetFocus() == open.edit;
        smooth::fill_round_rect(dc, field, m.px(8) * 2, field_fill(palette));
        let (ring, ring_width) = if focused {
            (palette.omnibox_focus_border, 2)
        } else {
            (blend(panel_fill(palette), palette.tab_text_active, 0.45), 1)
        };
        smooth::stroke_round_rect(dc, field, m.px(8) * 2, ring, ring_width);
        // The placeholder is the EDIT's own cue banner, so only the clear button is drawn here.
        if !edit_text(open.edit).is_empty() {
            draw_clear(dc, to_rect(layout.clear), palette.tab_text_inactive, m);
        }

        for (slot, swatch) in layout.swatches.iter().enumerate() {
            let rect = to_rect(*swatch);
            let fill = if slot == 0 {
                palette.tab_group_accent
            } else {
                ISLAND_COLOURS[slot - 1]
            };
            smooth::fill_ellipse(dc, rect, fill);
            if menu::swatch_slot(open.model.colour) == slot {
                let grow = m.px(RING_WIDTH).max(1) + 1;
                let outer = RECT {
                    left: rect.left - grow,
                    top: rect.top - grow,
                    right: rect.right + grow,
                    bottom: rect.bottom + grow,
                };
                smooth::stroke_ellipse(dc, outer, palette.tab_text_active, m.px(RING_WIDTH).max(1));
            }
        }

        for (index, row_rect) in layout.rows.iter().enumerate() {
            let Some(row) = open.model.rows.get(index) else {
                continue;
            };
            let rect = to_rect(*row_rect);
            let centre = (rect.top + rect.bottom) / 2;
            let icon = RECT {
                left: rect.left,
                top: centre - FAVICON_SLOT / 2,
                right: rect.left + FAVICON_SLOT,
                bottom: centre + FAVICON_SLOT / 2,
            };
            match open.favicons.get(index).and_then(|image| image.as_deref()) {
                Some(image) => blit_favicon(dc, icon, image),
                None => {
                    let dot = m.px(ROW_DOT).max(3);
                    let dot_rect = RECT {
                        left: rect.left + (FAVICON_SLOT - dot) / 2,
                        top: centre - dot / 2,
                        right: rect.left + (FAVICON_SLOT - dot) / 2 + dot,
                        bottom: centre - dot / 2 + dot,
                    };
                    smooth::fill_ellipse(dc, dot_rect, palette.tab_text_inactive);
                }
            }
            let text = RECT {
                left: icon.right + m.px(ROW_GAP),
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            };
            let colour = if row.active {
                palette.tab_text_active
            } else {
                palette.tab_text_inactive
            };
            let title = if row.title.trim().is_empty() {
                NEW_TAB_TITLE
            } else {
                row.title.as_str()
            };
            draw_text(
                dc,
                text,
                title,
                colour,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
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

        let button = to_rect(layout.close);
        smooth::fill_round_rect(dc, button, m.px(8) * 2, palette.tab_hover);
        draw_text(
            dc,
            button,
            &open.model.close_label(),
            palette.tab_text_active,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );

        SelectObject(dc, old_font);
        let _ = DeleteObject(font.into());
    }
}

/// A small "x" in the clear button.
unsafe fn draw_clear(dc: HDC, rect: RECT, colour: Rgba, m: Metrics) {
    unsafe {
        let inset = m.px(4);
        let width = m.px(CLEAR_STROKE).max(1);
        let (a, b) = (
            POINT {
                x: rect.left + inset,
                y: rect.top + inset,
            },
            POINT {
                x: rect.right - inset,
                y: rect.bottom - inset,
            },
        );
        smooth::stroke_line(dc, a, b, colour, width);
        smooth::stroke_line(
            dc,
            POINT {
                x: rect.right - inset,
                y: rect.top + inset,
            },
            windows::Win32::Foundation::POINT {
                x: rect.left + inset,
                y: rect.bottom - inset,
            },
            colour,
            width,
        );
    }
}

/// Draws a premultiplied BGRA favicon into `rect`, scaled to fit.
unsafe fn blit_favicon(dc: HDC, rect: RECT, image: &FaviconBitmap) {
    unsafe {
        let (width, height) = (image.width as i32, image.height as i32);
        if width <= 0
            || height <= 0
            || image.bgra_premultiplied.len() != (width * height * 4) as usize
        {
            return;
        }
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
        if bitmap.is_invalid() || bits.is_null() {
            let _ = DeleteDC(memory);
            return;
        }
        std::ptr::copy_nonoverlapping(
            image.bgra_premultiplied.as_ptr(),
            bits.cast::<u8>(),
            image.bgra_premultiplied.len(),
        );
        let original = SelectObject(memory, bitmap.into());
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = AlphaBlend(
            dc,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            memory,
            0,
            0,
            width,
            height,
            blend,
        );
        SelectObject(memory, original);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
    }
}

unsafe fn draw_text(dc: HDC, bounds: RECT, text: &str, colour: Rgba, format: DRAW_TEXT_FORMAT) {
    unsafe {
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut bounds = bounds;
        SetTextColor(dc, colorref(colour));
        DrawTextW(dc, &mut wide, &mut bounds, format);
    }
}
