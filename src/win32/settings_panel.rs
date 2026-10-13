//! The Settings window: a native, owned panel for the settings in the active profile's
//! `settings.txt`.
//!
//! Inputs are real Win32 children, so Tab, Space and screen readers work as usual. Checkboxes and
//! buttons are owner-drawn in the solid palette, with a teal focus ring. Cards, headings and status
//! text are painted by the window itself. `WS_CLIPCHILDREN` keeps the two layers from overdrawing.
//! The message loop must pass keyboard messages through `process_message` first, so Tab, Shift+Tab,
//! Escape and Enter behave.

use std::{cell::Cell, ffi::c_void, io, path::PathBuf};

use windows::{
    Win32::{
        Foundation::{COLORREF, E_FAIL, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CLEARTYPE_QUALITY, CreateFontIndirectW, CreateSolidBrush, DRAW_TEXT_FORMAT,
            DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
            DT_WORDBREAK, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect, GetObjectW,
            HBRUSH, HDC, HFONT, InvalidateRect, LOGFONTW, PAINTSTRUCT, SelectObject, SetBkColor,
            SetBkMode, SetTextColor, TRANSPARENT,
        },
        UI::{
            Controls::{DRAWITEMSTRUCT, ODS_FOCUS, ODS_SELECTED},
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{GetFocus, SetFocus, VK_ESCAPE, VK_RETURN},
            Shell::ShellExecuteW,
            WindowsAndMessaging::{
                BN_CLICKED, BS_OWNERDRAW, CREATESTRUCTW, CreateWindowExW, DefWindowProcW,
                DestroyWindow, EN_KILLFOCUS, EN_SETFOCUS, ES_AUTOHSCROLL, ES_NUMBER, GWLP_USERDATA,
                GetClientRect, GetDlgCtrlID, GetDlgItem, GetWindowLongPtrW, GetWindowRect,
                GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IDCANCEL, IDYES, IsChild,
                IsDialogMessageW, IsWindow, LoadCursorW, MB_ICONQUESTION, MB_YESNO, MSG,
                MessageBoxW, RegisterClassW, SW_SHOW, SW_SHOWNORMAL, SendMessageW,
                SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW, ShowWindow, WINDOW_STYLE,
                WM_CLOSE, WM_COMMAND, WM_CREATE, WM_CTLCOLOREDIT, WM_DRAWITEM, WM_ERASEBKGND,
                WM_KEYDOWN, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_SETFONT, WNDCLASSW, WS_CHILD,
                WS_CLIPCHILDREN, WS_EX_CONTROLPARENT, WS_POPUP, WS_TABSTOP, WS_VISIBLE,
            },
        },
    },
    core::{HSTRING, PCWSTR, w},
};

use super::menu::{self, Entry, Icon, Item, Menu};
use crate::{
    navigation::{SearchEngine, SearchProvider},
    settings::{self, Settings, parse_blocker_allow, parse_sleep_after_secs},
    theme::{self, Rgba},
    win32::{glass, smooth},
};

const CLASS_NAME: PCWSTR = w!("BowserSettingsPanel");
const TITLE: &str = "Settings";
const SUBTITLE: &str = "Changes are saved to this profile's settings.txt.";
const SEARCH_HEADING: &str = "Search";
const SEARCH_LABEL: &str = "Search engine";
const SEARCH_HELP: &str =
    "Optimised: DuckDuckGo (no ads) for web, Google for images. Type `img ` before a query for images.";
/// Shown in the selector when no engine is chosen and no `search-provider.txt` is in use.
const SEARCH_DEFAULT: &str = "Optimised (default)";
const SEARCH_CUSTOM: &str = "Custom (search-provider.txt)";
const SLEEP_HEADING: &str = "Tab sleep";
const SLEEP_TOGGLE: &str = "Sleep hidden tabs after a delay";
const SLEEP_HELP: &str = "Hidden tabs are discarded to free memory once they have been hidden for this long. Under memory pressure, the longest-hidden tabs are discarded regardless.";
const SLEEP_LABEL: &str = "Sleep after (seconds)";
const SLEEP_UNIT: &str = "30 to 604800 seconds (7 days)";
const BLOCKER_HEADING: &str = "Ad and tracker blocker";
const LAYOUT_HEADING: &str = "Tab layout";
const LAYOUT_HELP: &str =
    "Choose a horizontal strip or vertical sidebar. Sidebar width is measured in logical pixels.";
const BLOCKER_TOGGLE: &str = "Block ads and trackers";
const BLOCKER_HELP: &str =
    "Blocks known ad and tracking requests on every site except the ones listed below.";
const ALLOW_LABEL: &str = "Allowed sites";
const ALLOW_HELP: &str = "Separate sites with commas, for example example.com, other.org.";
const TAB_GROUPS_HEADING: &str = "Tab groups";
const TAB_GROUPS_TOGGLE: &str = "Group tabs from the same site automatically";
const TAB_GROUPS_HELP: &str = "Tabs from one site share an island. Turn this off to stop new automatic groups. Groups you made by hand stay. Applies after a restart.";
const IDLE_STATUS: &str = "Search applies on the next search. Other changes need a restart.";
const SAVED_STATUS: &str = "Saved. Search applies now; restart Bowser for other changes.";
const RADIUS: i32 = 10;
/// Logical (96 DPI) client size. Every other measurement is scaled from these values.
const CLIENT_WIDTH: i32 = 600;
const CLIENT_HEIGHT: i32 = 892;

const ID_SLEEP_ENABLED: i32 = 201;
const ID_SLEEP_SECS: i32 = 202;
const ID_BLOCKER_ENABLED: i32 = 203;
const ID_BLOCKER_ALLOW: i32 = 204;
const ID_OPEN_FILE: i32 = 205;
const ID_RESET: i32 = 206;
const ID_SAVE: i32 = 207;
const ID_CLOSE: i32 = 208;
const ID_SEARCH_ENGINE: i32 = 209;
const ID_TAB_GROUPS: i32 = 210;
const ID_TAB_LAYOUT: i32 = 211;
const ID_SIDEBAR_WIDTH: i32 = 212;

thread_local! {
    /// The open window, so reopening the menu item focuses it rather than stacking another.
    static OPEN: Cell<isize> = const { Cell::new(0) };
    static CLASS_REGISTERED: Cell<bool> = const { Cell::new(false) };
    /// The panel pointer that `WM_NCCREATE` took ownership of, or 0. Lets `open` tell whether a
    /// failed `CreateWindowExW` already freed the panel through `WM_NCDESTROY`.
    static ADOPTED: Cell<usize> = const { Cell::new(0) };
}

/// GDI objects the window draws with. Created once per window and freed when it is dropped.
struct Resources {
    font: HFONT,
    heading: HFONT,
    section: HFONT,
    window: HBRUSH,
    card: HBRUSH,
    accent: HBRUSH,
    focus: HBRUSH,
    pressed: HBRUSH,
    border: HBRUSH,
}

impl Resources {
    unsafe fn new(dpi: u32, palette: &theme::Palette) -> Self {
        unsafe {
            let font = crate::chrome::paint::create_message_font(dpi, CLEARTYPE_QUALITY);
            Self {
                font,
                heading: derived_font(font, 13, 600),
                section: derived_font(font, 11, 600),
                window: CreateSolidBrush(colorref(palette.window)),
                card: CreateSolidBrush(colorref(palette.omnibox_fill)),
                accent: CreateSolidBrush(colorref(palette.progress_fill)),
                focus: CreateSolidBrush(colorref(palette.omnibox_focus_border)),
                pressed: CreateSolidBrush(colorref(palette.tab_active)),
                border: CreateSolidBrush(colorref(palette.omnibox_border)),
            }
        }
    }
}

impl Drop for Resources {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.font.into());
            let _ = DeleteObject(self.heading.into());
            let _ = DeleteObject(self.section.into());
            for brush in [
                self.window,
                self.card,
                self.accent,
                self.focus,
                self.pressed,
                self.border,
            ] {
                let _ = DeleteObject(brush.into());
            }
        }
    }
}

struct Panel {
    app: crate::app::AppRef,
    path: PathBuf,
    palette: &'static theme::Palette,
    res: Resources,
    /// Checkbox state is kept here, not in the control: owner-drawn buttons do not track
    /// `BM_GETCHECK`/`BM_SETCHECK`. Native clicks and Space still arrive as `BN_CLICKED`.
    sleep_enabled: bool,
    blocker_enabled: bool,
    automatic_tab_grouping: bool,
    /// The saved engine choice, or `None` when searches use `search-provider.txt` or the default.
    search_engine: Option<SearchEngine>,
    tab_layout: settings::TabLayout,
    sidebar_width: u16,
    /// What the selector shows when `search_engine` is `None`.
    search_fallback: &'static str,
    /// Control ID of the text field that has keyboard focus, or 0. Drives the teal ring.
    focus: i32,
    status: String,
    status_error: bool,
}

/// Logical positions. Every rectangle is scaled from these values with the window's DPI.
struct Layout {
    title: RECT,
    subtitle: RECT,
    search_heading: RECT,
    search_card: RECT,
    search_label: RECT,
    search_button: RECT,
    search_help: RECT,
    layout_heading: RECT,
    layout_card: RECT,
    layout_toggle: RECT,
    layout_width_label: RECT,
    layout_width: RECT,
    layout_help: RECT,
    sleep_heading: RECT,
    sleep_card: RECT,
    sleep_toggle: RECT,
    sleep_help: RECT,
    sleep_label: RECT,
    sleep_field: RECT,
    sleep_edit: RECT,
    sleep_unit: RECT,
    blocker_heading: RECT,
    blocker_card: RECT,
    blocker_toggle: RECT,
    blocker_help: RECT,
    allow_label: RECT,
    allow_field: RECT,
    allow_edit: RECT,
    allow_help: RECT,
    tab_groups_heading: RECT,
    tab_groups_card: RECT,
    tab_groups_toggle: RECT,
    tab_groups_help: RECT,
    status: RECT,
    open_file: RECT,
    reset: RECT,
    save: RECT,
    close: RECT,
}

fn layout(dpi: u32) -> Layout {
    let s = |v: i32| scaled(v, dpi);
    let r = |l: i32, t: i32, right: i32, b: i32| rect(s(l), s(t), s(right), s(b));
    let sleep_field = r(224, 442, 328, 472);
    let allow_field = r(44, 626, 556, 656);
    Layout {
        title: r(28, 20, 572, 56),
        subtitle: r(28, 56, 572, 76),
        search_heading: r(28, 92, 572, 114),
        search_card: r(28, 118, 572, 214),
        search_label: r(44, 132, 220, 158),
        search_button: r(224, 128, 420, 162),
        search_help: r(44, 170, 556, 204),
        layout_heading: r(28, 226, 572, 248),
        layout_card: r(28, 252, 572, 324),
        layout_toggle: r(44, 262, 286, 296),
        layout_width_label: r(314, 262, 440, 296),
        layout_width: r(448, 262, 548, 296),
        layout_help: r(44, 300, 556, 318),
        sleep_heading: r(28, 336, 572, 358),
        sleep_card: r(28, 364, 572, 486),
        sleep_toggle: r(44, 376, 556, 400),
        sleep_help: r(44, 404, 556, 438),
        sleep_label: r(44, 446, 220, 468),
        sleep_field,
        sleep_edit: inset(sleep_field, s(8), s(5)),
        sleep_unit: r(340, 446, 556, 468),
        blocker_heading: r(28, 494, 572, 516),
        blocker_card: r(28, 522, 572, 698),
        blocker_toggle: r(44, 534, 556, 558),
        blocker_help: r(44, 562, 556, 596),
        allow_label: r(44, 604, 556, 624),
        allow_field,
        allow_edit: inset(allow_field, s(8), s(5)),
        allow_help: r(44, 662, 556, 680),
        tab_groups_heading: r(28, 706, 572, 728),
        tab_groups_card: r(28, 734, 572, 814),
        tab_groups_toggle: r(44, 746, 556, 770),
        tab_groups_help: r(44, 774, 556, 808),
        status: r(28, 822, 572, 842),
        open_file: r(28, 850, 198, 884),
        reset: r(210, 850, 342, 884),
        save: r(354, 850, 458, 884),
        close: r(470, 850, 572, 884),
    }
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

fn inset(r: RECT, dx: i32, dy: i32) -> RECT {
    rect(r.left + dx, r.top + dy, r.right - dx, r.bottom - dy)
}

fn scaled(v: i32, dpi: u32) -> i32 {
    (v as i64 * dpi as i64 / 96) as i32
}

fn colorref(colour: Rgba) -> COLORREF {
    COLORREF(u32::from(colour.r) | (u32::from(colour.g) << 8) | (u32::from(colour.b) << 16))
}

fn to_wide_z(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn dpi_of(hwnd: HWND) -> u32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 96 } else { dpi }
}

/// The body font with its height scaled by `tenths / 10` and the given weight, for headings.
unsafe fn derived_font(base: HFONT, tenths: i32, weight: i32) -> HFONT {
    unsafe {
        let mut logfont = LOGFONTW::default();
        GetObjectW(
            base.into(),
            size_of::<LOGFONTW>() as i32,
            Some((&mut logfont as *mut LOGFONTW).cast()),
        );
        logfont.lfHeight = logfont.lfHeight * tenths / 10;
        logfont.lfWeight = weight;
        CreateFontIndirectW(&logfont)
    }
}

/// Opaque palette for the window. The glass palettes are translucent and only suit the chrome.
fn palette() -> &'static theme::Palette {
    theme::palette(glass::appearance(), true)
}

/// The panel state, valid from `WM_NCCREATE` until `WM_NCDESTROY`. Only used on the UI thread from
/// this window's own messages, and never held across a call that can re-enter this window.
fn state<'a>(hwnd: HWND) -> Option<&'a mut Panel> {
    unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Panel).as_mut() }
}

/// Rounded panel or control: an antialiased fill with an antialiased border of `width` pixels.
unsafe fn round_rect(dc: HDC, bounds: RECT, fill: Rgba, border: Rgba, width: i32) {
    unsafe {
        smooth::fill_round_rect(dc, bounds, RADIUS, fill);
        smooth::stroke_round_rect(dc, bounds, RADIUS, border, width);
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

fn window_text(hwnd: HWND) -> String {
    unsafe {
        let length = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut buffer = vec![0u16; length + 1];
        let copied = GetWindowTextW(hwnd, &mut buffer).max(0) as usize;
        String::from_utf16_lossy(&buffer[..copied])
    }
}

fn child_of(hwnd: HWND, id: i32) -> Option<HWND> {
    unsafe { GetDlgItem(Some(hwnd), id).ok() }
}

unsafe fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        if let Some(panel) = state(hwnd) {
            let mut client = RECT::default();
            let _ = GetClientRect(hwnd, &mut client);
            let dpi = dpi_of(hwnd);
            let l = layout(dpi);
            let p = panel.palette;
            let res = &panel.res;
            FillRect(dc, &client, res.window);
            FrameRect(dc, &client, res.border);
            SetBkMode(dc, TRANSPARENT);

            round_rect(
                dc,
                l.search_card,
                p.omnibox_fill,
                p.omnibox_border,
                scaled(1, dpi),
            );
            round_rect(
                dc,
                l.layout_card,
                p.omnibox_fill,
                p.omnibox_border,
                scaled(1, dpi),
            );
            round_rect(
                dc,
                l.sleep_card,
                p.omnibox_fill,
                p.omnibox_border,
                scaled(1, dpi),
            );
            round_rect(
                dc,
                l.blocker_card,
                p.omnibox_fill,
                p.omnibox_border,
                scaled(1, dpi),
            );
            round_rect(
                dc,
                l.tab_groups_card,
                p.omnibox_fill,
                p.omnibox_border,
                scaled(1, dpi),
            );

            let single = DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX;
            let wrapped = DT_LEFT | DT_WORDBREAK | DT_NOPREFIX;
            let old = SelectObject(dc, res.heading.into());
            draw_text(dc, l.title, TITLE, p.tab_text_active, single);
            SelectObject(dc, res.section.into());
            draw_text(
                dc,
                l.search_heading,
                SEARCH_HEADING,
                p.tab_text_active,
                single,
            );
            draw_text(
                dc,
                l.layout_heading,
                LAYOUT_HEADING,
                p.tab_text_active,
                single,
            );
            draw_text(
                dc,
                l.sleep_heading,
                SLEEP_HEADING,
                p.tab_text_active,
                single,
            );
            draw_text(
                dc,
                l.blocker_heading,
                BLOCKER_HEADING,
                p.tab_text_active,
                single,
            );
            draw_text(
                dc,
                l.tab_groups_heading,
                TAB_GROUPS_HEADING,
                p.tab_text_active,
                single,
            );
            SelectObject(dc, res.font.into());
            draw_text(dc, l.subtitle, SUBTITLE, p.omnibox_placeholder, single);
            draw_text(dc, l.search_label, SEARCH_LABEL, p.omnibox_text, single);
            draw_text(
                dc,
                l.search_help,
                SEARCH_HELP,
                p.omnibox_placeholder,
                wrapped,
            );
            draw_text(
                dc,
                l.layout_help,
                LAYOUT_HELP,
                p.omnibox_placeholder,
                single,
            );
            draw_text(
                dc,
                l.layout_width_label,
                "Sidebar width",
                p.omnibox_text,
                single,
            );
            draw_text(dc, l.sleep_help, SLEEP_HELP, p.omnibox_placeholder, wrapped);
            draw_text(dc, l.sleep_label, SLEEP_LABEL, p.omnibox_text, single);
            draw_text(dc, l.sleep_unit, SLEEP_UNIT, p.omnibox_placeholder, single);
            draw_text(
                dc,
                l.blocker_help,
                BLOCKER_HELP,
                p.omnibox_placeholder,
                wrapped,
            );
            draw_text(dc, l.allow_label, ALLOW_LABEL, p.omnibox_text, single);
            draw_text(dc, l.allow_help, ALLOW_HELP, p.omnibox_placeholder, wrapped);
            draw_text(
                dc,
                l.tab_groups_help,
                TAB_GROUPS_HELP,
                p.omnibox_placeholder,
                wrapped,
            );

            // The field frames sit outside the edit controls, so the teal ring is drawn here.
            for (id, field) in [
                (ID_SLEEP_SECS, l.sleep_field),
                (ID_BLOCKER_ALLOW, l.allow_field),
                (ID_SIDEBAR_WIDTH, l.layout_width),
            ] {
                let focused = panel.focus == id;
                let (border, width) = if focused {
                    (p.omnibox_focus_border, scaled(2, dpi))
                } else {
                    (p.omnibox_border, scaled(1, dpi))
                };
                round_rect(dc, field, p.window, border, width);
            }

            let status_colour = if panel.status_error {
                p.close_hover
            } else {
                p.omnibox_text
            };
            draw_text(dc, l.status, &panel.status, status_colour, single);
            SelectObject(dc, old);
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

/// Owner-drawn checkboxes and buttons. Their state lives in the control, so the drawing reads it.
unsafe fn draw_item(hwnd: HWND, lparam: LPARAM) {
    unsafe {
        let item = &*(lparam.0 as *const DRAWITEMSTRUCT);
        let Some(panel) = state(hwnd) else {
            return;
        };
        let dpi = dpi_of(hwnd);
        let dc = item.hDC;
        let rc = item.rcItem;
        let focused = item.itemState.0 & ODS_FOCUS.0 != 0;
        let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
        let p = panel.palette;
        let res = &panel.res;
        let text = window_text(item.hwndItem);
        SelectObject(dc, res.font.into());
        SetBkMode(dc, TRANSPARENT);
        let id = item.CtlID as i32;
        match id {
            ID_SLEEP_ENABLED | ID_BLOCKER_ENABLED | ID_TAB_GROUPS => {
                let checked = match id {
                    ID_SLEEP_ENABLED => panel.sleep_enabled,
                    ID_BLOCKER_ENABLED => panel.blocker_enabled,
                    _ => panel.automatic_tab_grouping,
                };
                let s = |v: i32| scaled(v, dpi);
                FillRect(dc, &rc, res.card);
                let mid = (rc.top + rc.bottom) / 2;
                let boxed = rect(rc.left + s(4), mid - s(9), rc.left + s(22), mid + s(9));
                let (fill, border) = if checked {
                    (p.omnibox_focus_border, p.omnibox_focus_border)
                } else {
                    (p.window, p.omnibox_border)
                };
                round_rect(dc, boxed, fill, border, s(1));
                if checked {
                    let mark = |from: POINT, to: POINT| {
                        smooth::stroke_line(dc, from, to, p.window, s(2).max(1));
                    };
                    mark(
                        POINT {
                            x: boxed.left + s(4),
                            y: boxed.top + s(9),
                        },
                        POINT {
                            x: boxed.left + s(8),
                            y: boxed.top + s(13),
                        },
                    );
                    mark(
                        POINT {
                            x: boxed.left + s(8),
                            y: boxed.top + s(13),
                        },
                        POINT {
                            x: boxed.left + s(14),
                            y: boxed.top + s(5),
                        },
                    );
                }
                let label = rect(rc.left + s(30), rc.top, rc.right, rc.bottom);
                draw_text(
                    dc,
                    label,
                    &text,
                    p.omnibox_text,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
                if focused {
                    FrameRect(dc, &rc, res.focus);
                }
            }
            ID_SEARCH_ENGINE | ID_TAB_LAYOUT => {
                let s = |v: i32| scaled(v, dpi);
                // Sits on the card, so clear the corners before the rounded field.
                FillRect(dc, &rc, res.card);
                let (border, width) = if focused {
                    (p.omnibox_focus_border, s(2))
                } else {
                    (p.omnibox_border, s(1))
                };
                let fill = if pressed { p.tab_active } else { p.window };
                round_rect(dc, rc, fill, border, width);
                draw_text(
                    dc,
                    rect(rc.left + s(12), rc.top, rc.right - s(32), rc.bottom),
                    &text,
                    p.omnibox_text,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
                );
                let (right, middle) = (rc.right - s(16), (rc.top + rc.bottom) / 2);
                let chevron = |from: POINT, to: POINT| {
                    smooth::stroke_line(dc, from, to, p.omnibox_text, s(2).max(1));
                };
                chevron(
                    POINT {
                        x: right - s(5),
                        y: middle - s(2),
                    },
                    POINT {
                        x: right,
                        y: middle + s(3),
                    },
                );
                chevron(
                    POINT {
                        x: right,
                        y: middle + s(3),
                    },
                    POINT {
                        x: right + s(5),
                        y: middle - s(2),
                    },
                );
            }
            ID_OPEN_FILE | ID_RESET | ID_SAVE | ID_CLOSE => {
                let primary = id == ID_SAVE;
                let (fill, text_colour) = if primary {
                    (p.progress_fill, p.window)
                } else if pressed {
                    (p.tab_active, p.omnibox_text)
                } else {
                    (p.omnibox_fill, p.omnibox_text)
                };
                let (border, width) = if focused {
                    (p.omnibox_focus_border, scaled(2, dpi))
                } else {
                    (p.omnibox_border, scaled(1, dpi))
                };
                // Buttons sit on the window background, so clear the corners before the rounded fill.
                FillRect(dc, &rc, res.window);
                round_rect(dc, rc, fill, border, width);
                draw_text(
                    dc,
                    rc,
                    &text,
                    text_colour,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
            }
            _ => {}
        }
    }
}

unsafe fn create_children(hwnd: HWND) -> windows::core::Result<()> {
    unsafe {
        let instance = HINSTANCE(windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?.0);
        let l = layout(dpi_of(hwnd));
        let font = state(hwnd).map_or(HFONT::default(), |panel| panel.res.font);
        let button = WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP;
        let edit_number = WINDOW_STYLE((ES_AUTOHSCROLL | ES_NUMBER) as u32) | WS_TABSTOP;
        let edit_text = WINDOW_STYLE(ES_AUTOHSCROLL as u32) | WS_TABSTOP;
        let children = [
            (
                ID_SEARCH_ENGINE,
                "",
                l.search_button,
                button,
                ButtonKind::Owner,
            ),
            (
                ID_TAB_LAYOUT,
                "",
                l.layout_toggle,
                button,
                ButtonKind::Owner,
            ),
            (
                ID_SIDEBAR_WIDTH,
                "",
                l.layout_width,
                edit_number,
                ButtonKind::Edit,
            ),
            (
                ID_SLEEP_ENABLED,
                SLEEP_TOGGLE,
                l.sleep_toggle,
                button,
                ButtonKind::Owner,
            ),
            (
                ID_SLEEP_SECS,
                "",
                l.sleep_edit,
                edit_number,
                ButtonKind::Edit,
            ),
            (
                ID_BLOCKER_ENABLED,
                BLOCKER_TOGGLE,
                l.blocker_toggle,
                button,
                ButtonKind::Owner,
            ),
            (
                ID_TAB_GROUPS,
                TAB_GROUPS_TOGGLE,
                l.tab_groups_toggle,
                button,
                ButtonKind::Owner,
            ),
            (
                ID_BLOCKER_ALLOW,
                "",
                l.allow_edit,
                edit_text,
                ButtonKind::Edit,
            ),
            (
                ID_OPEN_FILE,
                "Open settings file",
                l.open_file,
                button,
                ButtonKind::Owner,
            ),
            (
                ID_RESET,
                "Reset to defaults",
                l.reset,
                button,
                ButtonKind::Owner,
            ),
            (ID_SAVE, "Save", l.save, button, ButtonKind::Owner),
            (ID_CLOSE, "Close", l.close, button, ButtonKind::Owner),
        ];
        for (id, text, bounds, style, kind) in children {
            let class = match kind {
                ButtonKind::Owner => w!("BUTTON"),
                ButtonKind::Edit => w!("EDIT"),
            };
            let wide = to_wide_z(text);
            let child = CreateWindowExW(
                Default::default(),
                class,
                PCWSTR(wide.as_ptr()),
                WS_CHILD | WS_VISIBLE | style,
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                Some(hwnd),
                Some(HMENU(id as usize as *mut _)),
                Some(instance),
                None,
            )?;
            SendMessageW(
                child,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum ButtonKind {
    Owner,
    Edit,
}

fn set_text(hwnd: HWND, id: i32, text: &str) {
    if let Some(child) = child_of(hwnd, id) {
        let wide = to_wide_z(text);
        unsafe {
            let _ = SetWindowTextW(child, PCWSTR(wide.as_ptr()));
        }
    }
}

/// Shows `settings` in the controls, replacing whatever the user typed.
fn populate(hwnd: HWND, settings: &Settings) {
    if let Some(panel) = state(hwnd) {
        panel.sleep_enabled = settings.sleep_enabled;
        panel.blocker_enabled = settings.blocker_enabled;
        panel.automatic_tab_grouping = settings.automatic_tab_grouping;
        panel.search_engine = settings.search_engine;
        panel.tab_layout = settings.tab_layout;
        panel.sidebar_width = settings.sidebar_width;
    }
    refresh_search_text(hwnd);
    refresh_layout_text(hwnd);
    invalidate_child(hwnd, ID_SLEEP_ENABLED);
    invalidate_child(hwnd, ID_BLOCKER_ENABLED);
    invalidate_child(hwnd, ID_TAB_GROUPS);
    set_text(hwnd, ID_SLEEP_SECS, &settings.sleep_after_secs.to_string());
    set_text(hwnd, ID_SIDEBAR_WIDTH, &settings.sidebar_width.to_string());
    set_text(hwnd, ID_BLOCKER_ALLOW, &settings.blocker_allow.join(", "));
    invalidate(hwnd);
}

fn refresh_layout_text(hwnd: HWND) {
    let (text, width) = state(hwnd).map_or(("Horizontal tabs", 260), |panel| {
        (
            match panel.tab_layout {
                settings::TabLayout::Horizontal => "Horizontal tabs",
                settings::TabLayout::Vertical => "Vertical tabs",
            },
            panel.sidebar_width,
        )
    });
    set_text(hwnd, ID_TAB_LAYOUT, text);
    set_text(hwnd, ID_SIDEBAR_WIDTH, &width.to_string());
}

fn search_text(panel: &Panel) -> &'static str {
    match panel.search_engine {
        Some(engine) => engine.label(),
        None => panel.search_fallback,
    }
}

/// Copies the selector's current choice into its text. Owner-drawn, so the text is what it shows.
fn refresh_search_text(hwnd: HWND) {
    let text = state(hwnd).map_or("", |panel| search_text(panel));
    set_text(hwnd, ID_SEARCH_ENGINE, text);
}

fn engine_description(engine: SearchEngine) -> &'static str {
    match engine {
        SearchEngine::Optimised => "DuckDuckGo (no ads) for web, Google for images.",
        SearchEngine::Google => "Searches go to google.com.",
        SearchEngine::DuckDuckGo => "Searches go to duckduckgo.com.",
    }
}

/// Opens the themed engine list under the selector. The list runs its own message loop, so the
/// panel is read before it opens and changed only after it closes. Nothing is saved until Save.
fn choose_search_engine(hwnd: HWND) {
    let Some(button) = child_of(hwnd, ID_SEARCH_ENGINE) else {
        return;
    };
    let mut bounds = RECT::default();
    unsafe {
        let _ = GetWindowRect(button, &mut bounds);
    }
    let current = state(hwnd).and_then(|panel| panel.search_engine);
    let entries = SearchEngine::ALL
        .iter()
        .enumerate()
        .map(|(index, &engine)| {
            let selected = current == Some(engine);
            let icon = if selected { Icon::Check } else { Icon::None };
            Entry::Item(
                Item::new(index, engine.label(), icon)
                    .checked(selected)
                    .described(engine_description(engine)),
            )
        })
        .collect();
    let menu = Menu {
        header: None,
        entries,
        footer: "Used for address bar searches.".to_owned(),
    };
    let anchor = POINT {
        x: bounds.left,
        y: bounds.bottom + scaled(4, dpi_of(hwnd)),
    };
    if let Some(chosen) = menu::track_below(hwnd, anchor, menu) {
        if let Some(&engine) = SearchEngine::ALL.get(chosen.id) {
            if let Some(panel) = state(hwnd) {
                panel.search_engine = Some(engine);
            }
            refresh_search_text(hwnd);
            set_status(
                hwnd,
                "Not saved yet. Press Save to keep this search engine.",
                false,
            );
        }
    }
}

fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn invalidate_child(hwnd: HWND, id: i32) {
    if let Some(child) = child_of(hwnd, id) {
        unsafe {
            let _ = InvalidateRect(Some(child), None, false);
        }
    }
}

fn set_status(hwnd: HWND, text: impl Into<String>, error: bool) {
    if let Some(panel) = state(hwnd) {
        panel.status = text.into();
        panel.status_error = error;
    }
    invalidate(hwnd);
}

fn focus_field(hwnd: HWND, id: i32) {
    if let Some(child) = child_of(hwnd, id) {
        unsafe {
            let _ = SetFocus(Some(child));
        }
    }
}

/// Explains a failed write in plain words. Nothing is saved when this is shown.
fn save_error(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::InvalidData => format!("{error}. Nothing was saved."),
        _ => format!("Settings could not be saved ({error}). Nothing was saved."),
    }
}

fn save_form(hwnd: HWND) {
    let Some(path) = state(hwnd).map(|panel| panel.path.clone()) else {
        return;
    };
    let (sleep_enabled, blocker_enabled, search_engine, automatic_tab_grouping, tab_layout) =
        state(hwnd).map_or(
            (true, true, None, true, settings::TabLayout::Horizontal),
            |panel| {
                (
                    panel.sleep_enabled,
                    panel.blocker_enabled,
                    panel.search_engine,
                    panel.automatic_tab_grouping,
                    panel.tab_layout,
                )
            },
        );
    let sleep = parse_sleep_after_secs(&window_text_of(hwnd, ID_SLEEP_SECS));
    let allow = parse_blocker_allow(&window_text_of(hwnd, ID_BLOCKER_ALLOW));
    let sidebar_width = window_text_of(hwnd, ID_SIDEBAR_WIDTH)
        .parse::<u16>()
        .ok()
        .filter(|width| {
            (settings::MIN_SIDEBAR_WIDTH..=settings::MAX_SIDEBAR_WIDTH).contains(width)
        });
    let Some(sidebar_width) = sidebar_width else {
        focus_field(hwnd, ID_SIDEBAR_WIDTH);
        set_status(hwnd, "Sidebar width must be between 180 and 360.", true);
        return;
    };
    let (sleep_after_secs, blocker_allow) = match (sleep, allow) {
        (Ok(secs), Ok(sites)) => (secs, sites),
        (Err(message), _) => {
            focus_field(hwnd, ID_SLEEP_SECS);
            set_status(hwnd, message, true);
            return;
        }
        (_, Err(message)) => {
            focus_field(hwnd, ID_BLOCKER_ALLOW);
            set_status(hwnd, message, true);
            return;
        }
    };
    // The GDI form has no controls for these; the file's current values are kept.
    let current = Settings::load(&path);
    let settings = Settings {
        efficiency_mode: current.efficiency_mode,
        tracking_prevention: current.tracking_prevention,
        tab_layout,
        sidebar_width,
        sidebar_collapsed: current.sidebar_collapsed,
        side_panel_width: current.side_panel_width,
        sleep_enabled,
        sleep_after_secs,
        blocker_enabled,
        blocker_allow,
        search_engine,
        automatic_tab_grouping,
    };
    match settings::save(&path, &settings) {
        Ok(()) => {
            populate(hwnd, &settings);
            let app = state(hwnd).map(|panel| panel.app.clone());
            if let Some(app) = app {
                crate::app::App::apply_tab_layout_settings(
                    &app,
                    settings.tab_layout,
                    settings.sidebar_width,
                );
            }
            set_status(hwnd, SAVED_STATUS, false);
        }
        Err(error) => set_status(hwnd, save_error(&error), true),
    }
}

fn window_text_of(hwnd: HWND, id: i32) -> String {
    child_of(hwnd, id).map_or_else(String::new, window_text)
}

fn reset(hwnd: HWND) {
    if !confirm(
        hwnd,
        "Reset all settings to their defaults? Comments and other lines in settings.txt are kept.",
    ) {
        return;
    }
    let Some(path) = state(hwnd).map(|panel| panel.path.clone()) else {
        return;
    };
    match settings::save(&path, &Settings::default()) {
        Ok(()) => {
            populate(hwnd, &Settings::default());
            let app = state(hwnd).map(|panel| panel.app.clone());
            if let Some(app) = app {
                crate::app::App::apply_tab_layout_settings(
                    &app,
                    Settings::default().tab_layout,
                    Settings::default().sidebar_width,
                );
            }
            set_status(
                hwnd,
                "Defaults saved. Search applies now; restart Bowser for other changes.",
                false,
            );
        }
        Err(error) => set_status(hwnd, save_error(&error), true),
    }
}

/// Native Yes/No question owned by the settings window. `true` for Yes.
fn confirm(hwnd: HWND, message: &str) -> bool {
    let text = to_wide_z(message);
    unsafe {
        MessageBoxW(
            Some(hwnd),
            PCWSTR(text.as_ptr()),
            w!("Bowser"),
            MB_YESNO | MB_ICONQUESTION,
        ) == IDYES
    }
}

fn open_file(hwnd: HWND) {
    let Some(path) = state(hwnd).map(|panel| panel.path.clone()) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = settings::create_template_if_missing(&path) {
        set_status(
            hwnd,
            format!("The settings file could not be created ({error})."),
            true,
        );
        return;
    }
    let wide = HSTRING::from(path.as_os_str());
    let result = unsafe { ShellExecuteW(None, w!("open"), &wide, None, None, SW_SHOWNORMAL) };
    // ShellExecuteW reports failure with a value of 32 or less.
    if (result.0 as isize) <= 32 {
        set_status(hwnd, "The settings file could not be opened.", true);
    }
}

/// Flips a checkbox's stored state and repaints it. Native clicks and Space both arrive here as
/// `BN_CLICKED`, so keyboard and mouse use the same path.
fn toggle(hwnd: HWND, id: i32) {
    if let Some(panel) = state(hwnd) {
        match id {
            ID_SLEEP_ENABLED => panel.sleep_enabled = !panel.sleep_enabled,
            ID_BLOCKER_ENABLED => panel.blocker_enabled = !panel.blocker_enabled,
            _ => panel.automatic_tab_grouping = !panel.automatic_tab_grouping,
        }
    }
    invalidate_child(hwnd, id);
}

fn command(hwnd: HWND, wparam: WPARAM) {
    let id = (wparam.0 & 0xFFFF) as i32;
    let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
    match (id, code) {
        (ID_SLEEP_ENABLED | ID_BLOCKER_ENABLED | ID_TAB_GROUPS, BN_CLICKED) => toggle(hwnd, id),
        (ID_SLEEP_SECS | ID_BLOCKER_ALLOW | ID_SIDEBAR_WIDTH, EN_SETFOCUS) => {
            if let Some(panel) = state(hwnd) {
                panel.focus = id;
            }
            invalidate(hwnd);
        }
        (ID_SLEEP_SECS | ID_BLOCKER_ALLOW | ID_SIDEBAR_WIDTH, EN_KILLFOCUS) => {
            if let Some(panel) = state(hwnd) {
                if panel.focus == id {
                    panel.focus = 0;
                }
            }
            invalidate(hwnd);
        }
        (ID_SEARCH_ENGINE, BN_CLICKED) => choose_search_engine(hwnd),
        (ID_TAB_LAYOUT, BN_CLICKED) => {
            if let Some(panel) = state(hwnd) {
                panel.tab_layout = match panel.tab_layout {
                    settings::TabLayout::Horizontal => settings::TabLayout::Vertical,
                    settings::TabLayout::Vertical => settings::TabLayout::Horizontal,
                };
            }
            refresh_layout_text(hwnd);
            invalidate_child(hwnd, ID_TAB_LAYOUT);
        }
        (ID_OPEN_FILE, BN_CLICKED) => open_file(hwnd),
        (ID_RESET, BN_CLICKED) => reset(hwnd),
        (ID_SAVE, BN_CLICKED) => save_form(hwnd),
        (ID_CLOSE, BN_CLICKED) => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        // Dialog navigation sends IDCANCEL for Escape on a button; close either way.
        (id, _) if id == IDCANCEL.0 => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        _ => {}
    }
}

/// Opens the Settings window over `owner`, or focuses the one already open. The settings file is
/// the active profile's, read when the window opens. Unreadable files show the reason in the status
/// line and leave the controls at defaults, so Save reports the problem instead of overwriting it.
pub(crate) fn open(owner: HWND, app: &crate::app::AppRef) -> windows::core::Result<()> {
    unsafe {
        let existing = OPEN.with(Cell::get);
        if existing != 0 && IsWindow(Some(HWND(existing as *mut c_void))).as_bool() {
            let existing = HWND(existing as *mut c_void);
            let _ = ShowWindow(existing, SW_SHOW);
            let _ = SetForegroundWindow(existing);
            return Ok(());
        }

        let profile = crate::profile::active()
            .map_err(|_| windows::core::Error::new(E_FAIL, "Bowser's profile is not ready."))?;
        let path = profile.settings_file();
        let loaded = settings::read_strict(&path);

        let instance = HINSTANCE(windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?.0);
        if !CLASS_REGISTERED.with(Cell::get) {
            let class = WNDCLASSW {
                lpfnWndProc: Some(settings_proc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: CLASS_NAME,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return Err(windows::core::Error::from_thread());
            }
            CLASS_REGISTERED.with(|registered| registered.set(true));
        }

        let dpi = dpi_of(owner);
        let (width, height) = (scaled(CLIENT_WIDTH, dpi), scaled(CLIENT_HEIGHT, dpi));
        let mut owner_rect = RECT::default();
        let _ = GetWindowRect(owner, &mut owner_rect);
        let x = owner_rect.left + (owner_rect.right - owner_rect.left - width) / 2;
        let y = owner_rect.top + (owner_rect.bottom - owner_rect.top - height) / 2;

        let palette = palette();
        let initial = match &loaded {
            Ok(settings) => settings.clone(),
            Err(_) => Settings::default(),
        };
        // With no engine chosen, the selector names what searches really use.
        let search_fallback =
            if SearchProvider::from_config(None, &profile.search_provider_file()).is_default() {
                SEARCH_DEFAULT
            } else {
                SEARCH_CUSTOM
            };
        let panel = Box::into_raw(Box::new(Panel {
            app: app.clone(),
            path,
            palette,
            res: Resources::new(dpi, palette),
            sleep_enabled: initial.sleep_enabled,
            blocker_enabled: initial.blocker_enabled,
            automatic_tab_grouping: initial.automatic_tab_grouping,
            search_engine: initial.search_engine,
            tab_layout: initial.tab_layout,
            sidebar_width: initial.sidebar_width,
            search_fallback,
            focus: 0,
            status: IDLE_STATUS.to_owned(),
            status_error: false,
        }));
        ADOPTED.with(|adopted| adopted.set(0));
        let window = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            CLASS_NAME,
            w!("Settings"),
            WS_POPUP | WS_CLIPCHILDREN,
            x,
            y,
            width,
            height,
            Some(owner),
            None,
            Some(instance),
            Some(panel.cast()),
        );
        let adopted = ADOPTED.with(|adopted| adopted.replace(0));
        let window = match window {
            Ok(window) => window,
            Err(error) => {
                // Exactly one owner frees the panel. Once WM_NCCREATE has adopted it, WM_NCDESTROY
                // frees it during the failed creation, so only an unadopted pointer is freed here.
                if adopted != panel as usize {
                    drop(Box::from_raw(panel));
                }
                return Err(error);
            }
        };
        OPEN.with(|open| open.set(window.0 as isize));
        super::window::attach_html_dialog(owner, window, super::html_dialog::Kind::Settings);

        match loaded {
            Ok(settings) => populate(window, &settings),
            Err(error) => {
                populate(window, &Settings::default());
                set_status(
                    window,
                    format!(
                        "settings.txt could not be read ({error}). Open the settings file to fix it, then reopen Settings."
                    ),
                    true,
                );
            }
        }
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        if let Some(first) = child_of(window, ID_SLEEP_ENABLED) {
            let _ = SetFocus(Some(first));
        }
        Ok(())
    }
}

/// Routes keyboard input for the open Settings window. Returns `true` when the message was fully
/// handled, so the caller must skip `TranslateMessage` and `DispatchMessage` for it.
///
/// Escape closes the window. Enter in a text field saves. Everything else, including Tab and
/// Shift+Tab between controls, goes through `IsDialogMessageW`. Messages for other windows return
/// `false` and take the normal path.
pub(crate) fn process_message(msg: &MSG) -> bool {
    let window = OPEN.with(Cell::get);
    if window == 0 {
        return false;
    }
    let hwnd = HWND(window as *mut c_void);
    let ours = msg.hwnd == hwnd || unsafe { IsChild(hwnd, msg.hwnd) }.as_bool();
    if !ours {
        return false;
    }
    if msg.message == WM_KEYDOWN {
        let key = msg.wParam.0 as u16;
        if key == VK_ESCAPE.0 {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return true;
        }
        if key == VK_RETURN.0 {
            let focus = unsafe { GetFocus() };
            let id = unsafe { GetDlgCtrlID(focus) };
            if id == ID_SLEEP_SECS || id == ID_BLOCKER_ALLOW || id == ID_SIDEBAR_WIDTH {
                save_form(hwnd);
                return true;
            }
        }
    }
    unsafe { IsDialogMessageW(hwnd, msg as *const MSG) }.as_bool()
}

unsafe extern "system" fn settings_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_NCCREATE => {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                ADOPTED.with(|adopted| adopted.set(create.lpCreateParams as usize));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_CREATE => match create_children(hwnd) {
                Ok(()) => LRESULT(0),
                Err(_) => LRESULT(-1),
            },
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_DRAWITEM => {
                draw_item(hwnd, lparam);
                LRESULT(1)
            }
            WM_CTLCOLOREDIT => match state(hwnd) {
                Some(panel) => {
                    let dc = HDC(wparam.0 as *mut c_void);
                    SetTextColor(dc, colorref(panel.palette.omnibox_text));
                    SetBkColor(dc, colorref(panel.palette.window));
                    LRESULT(panel.res.window.0 as isize)
                }
                None => DefWindowProcW(hwnd, message, wparam, lparam),
            },
            WM_COMMAND => {
                command(hwnd, wparam);
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                super::html_dialog::close(hwnd);
                OPEN.with(|open| {
                    if open.get() == hwnd.0 as isize {
                        open.set(0);
                    }
                });
                let panel = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut Panel;
                if !panel.is_null() {
                    drop(Box::from_raw(panel));
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}
