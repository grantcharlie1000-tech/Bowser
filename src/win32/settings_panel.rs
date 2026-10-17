//! The Settings window: a native, owned panel for the settings in the active profile's
//! `settings.txt`.
//!
//! Only the fallback: a window with the HTML chrome opens `bowser://settings` in a tab instead
//! (`settings_page.rs`, docs/design/settings-page.md). This window is used when the chrome runs
//! natively (HTML chrome off, no chrome environment, or the chrome failed), because only the
//! trusted chrome host may draw the settings page.
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
    settings::{
        self, Settings, parse_blocker_allow, parse_default_zoom, parse_sleep_after_secs,
        parse_theme_accent, parse_ui_font,
    },
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
const LAYOUT_HEADING: &str = "Browser layout";
const LAYOUT_HELP: &str =
    "Click to step through the layouts: toolbar above tabs, tabs above toolbar, single row or vertical sidebar. Sidebar width is measured in logical pixels.";
// --- wave-1 fixes --- this switch is only the None choice of the blocking level; it does not choose the level.
const BLOCKER_TOGGLE: &str = "Blocking on (off is the None level)";
const BLOCKER_HELP: &str =
    "Off blocks nothing. On uses the blocking level chosen on the settings page (Trackers, or Trackers and ads), on every site except the ones listed below.";
const ALLOW_LABEL: &str = "Allowed sites";
const ALLOW_HELP: &str = "Separate sites with commas, for example example.com, other.org.";
const TAB_GROUPS_HEADING: &str = "Tab groups";
const TAB_GROUPS_TOGGLE: &str = "Group tabs from the same site automatically";
const TAB_GROUPS_HELP: &str = "Tabs from one site share an island. Turn this off to stop new automatic groups. Groups you made by hand stay. Applies after a restart.";
const YOUTUBE_TOGGLE: &str = "Block YouTube video ads";
const APPEARANCE_HEADING: &str = "Appearance";
const APPEARANCE_HELP: &str = "Theme, accent, text size and font apply immediately.";
const ACCENT_LABEL: &str = "Accent colour (#rrggbb, blank for default)";
const ACCENT_ERROR: &str = "Enter a colour as #rrggbb, or leave it blank.";
const UI_FONT_LABEL: &str = "Font (blank for system font)";
const UI_FONT_ERROR: &str = "Enter a font name of up to 64 characters, with no control characters.";
const CHROME_HEADING: &str = "Toolbar & chrome";
/// --- settings slim --- only the omnibox's site-information and split buttons read this list (GDI chrome).
const TOOLBAR_HEADING: &str = "Toolbar buttons (only Site information and Split view act yet)";
const BEHAVIOUR_HEADING: &str = "Browsing behaviour";
const ZOOM_LABEL: &str = "Default zoom for new tabs (25-500%)";
const DEFAULT_BROWSER_BUTTON: &str = "Make Bowser the default browser";
const IDLE_STATUS: &str = "Search applies on the next search. Other changes need a restart.";
const SAVED_STATUS: &str = "Saved. Search applies now; restart Bowser for other changes.";
const RADIUS: i32 = 10;
/// Logical (96 DPI) client size. Every other measurement is scaled from these values.
const CLIENT_WIDTH: i32 = 600;
const CLIENT_HEIGHT: i32 = 1682;

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
const ID_THEME: i32 = 213;
const ID_THEME_ACCENT: i32 = 214;
const ID_FONT_SIZE: i32 = 215;
const ID_UI_FONT: i32 = 216;
const ID_BOOKMARKS_BAR: i32 = 217;
const ID_SIDE_PANEL_POSITION: i32 = 218;
const ID_CENTRED_ADDRESS_BAR: i32 = 219;
const ID_MINIMAL_URL: i32 = 220;
const ID_SHOW_LOAD_PROGRESS: i32 = 221;
const ID_FRAMELESS: i32 = 222;
const ID_ROUNDED_CONTENT_FRAME: i32 = 223;
const ID_SHOW_HOME_BUTTON: i32 = 224;
const ID_BOOKMARKS_BAR_TAB_GROUPS: i32 = 225;
const ID_TAB_PREVIEW_IMAGES: i32 = 226;
const ID_TAB_PREVIEW_MEMORY: i32 = 227;
const ID_TOOLBAR_BACK: i32 = 228;
const ID_TOOLBAR_FORWARD: i32 = 229;
const ID_TOOLBAR_RELOAD: i32 = 230;
const ID_TOOLBAR_SITE_INFO: i32 = 231;
const ID_TOOLBAR_BOOKMARK: i32 = 232;
const ID_TOOLBAR_NEWTAB: i32 = 233;
const ID_TOOLBAR_SPLIT: i32 = 234;
const ID_TOOLBAR_READER: i32 = 235;
const ID_DEFAULT_ZOOM: i32 = 236;
const ID_SHOW_ZOOM_INDICATOR: i32 = 237;
const ID_NEW_TAB_BESIDE_ACTIVE: i32 = 238;
const ID_CTRL_TAB_MRU: i32 = 239;
const ID_SHIFT_RIGHT_CLICK_CONTEXT: i32 = 240;
const ID_CTRL_SHIFT_C_COPY_LINK: i32 = 241;
const ID_CTRL_S_VERTICAL_TABS: i32 = 242;
const ID_PIN_NEW_TAB_GROUPS: i32 = 243;
const ID_SPLIT_EDGE_DROP: i32 = 244;
const ID_BLOCK_YOUTUBE_VIDEO_ADS: i32 = 245;
const ID_DEFAULT_BROWSER: i32 = 246;

/// Every owner-drawn checkbox-style control added for the new settings (plus the three that
/// already existed), so `draw_item` and `command` can recognise them with one guard instead of
/// repeating the id list. Order does not matter.
const CHECKLIST_IDS: [i32; 29] = [
    ID_SLEEP_ENABLED,
    ID_BLOCKER_ENABLED,
    ID_TAB_GROUPS,
    ID_BLOCK_YOUTUBE_VIDEO_ADS,
    ID_CENTRED_ADDRESS_BAR,
    ID_MINIMAL_URL,
    ID_SHOW_LOAD_PROGRESS,
    ID_FRAMELESS,
    ID_ROUNDED_CONTENT_FRAME,
    ID_SHOW_HOME_BUTTON,
    ID_BOOKMARKS_BAR_TAB_GROUPS,
    ID_TAB_PREVIEW_IMAGES,
    ID_TAB_PREVIEW_MEMORY,
    ID_SHOW_ZOOM_INDICATOR,
    ID_NEW_TAB_BESIDE_ACTIVE,
    ID_CTRL_TAB_MRU,
    ID_SHIFT_RIGHT_CLICK_CONTEXT,
    ID_CTRL_SHIFT_C_COPY_LINK,
    ID_CTRL_S_VERTICAL_TABS,
    ID_PIN_NEW_TAB_GROUPS,
    ID_SPLIT_EDGE_DROP,
    ID_TOOLBAR_BACK,
    ID_TOOLBAR_FORWARD,
    ID_TOOLBAR_RELOAD,
    ID_TOOLBAR_SITE_INFO,
    ID_TOOLBAR_BOOKMARK,
    ID_TOOLBAR_NEWTAB,
    ID_TOOLBAR_SPLIT,
    ID_TOOLBAR_READER,
];

/// The known toolbar button ids in their fixed catalog order, with the short label shown in the
/// "Toolbar buttons" checklist. This native window does not support drag-reordering, so the order
/// here (and the order `save_form` writes back) is always `settings::KNOWN_TOOLBAR_BUTTONS`.
const TOOLBAR_BUTTON_ROWS: [(i32, &str); 8] = [
    (ID_TOOLBAR_BACK, "Back"),
    (ID_TOOLBAR_FORWARD, "Forward"),
    (ID_TOOLBAR_RELOAD, "Reload"),
    (ID_TOOLBAR_SITE_INFO, "Site information"),
    (ID_TOOLBAR_BOOKMARK, "Bookmark"),
    (ID_TOOLBAR_NEWTAB, "New tab"),
    (ID_TOOLBAR_SPLIT, "Split view"),
    (ID_TOOLBAR_READER, "Reader view"),
];

/// The index into `Panel::toolbar_buttons` (and `settings::KNOWN_TOOLBAR_BUTTONS`) for a toolbar
/// button checkbox id, or `None` for any other id.
fn toolbar_button_index(id: i32) -> Option<usize> {
    TOOLBAR_BUTTON_ROWS.iter().position(|&(row_id, _)| row_id == id)
}

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
    theme: settings::Theme,
    font_size: settings::FontSize,
    bookmarks_bar: settings::BookmarksBar,
    side_panel_position: settings::SidePanelPosition,
    centred_address_bar: bool,
    minimal_url: bool,
    show_load_progress: bool,
    frameless: bool,
    rounded_content_frame: bool,
    show_home_button: bool,
    bookmarks_bar_tab_groups: bool,
    tab_preview_images: bool,
    tab_preview_memory: bool,
    show_zoom_indicator: bool,
    new_tab_beside_active: bool,
    ctrl_tab_mru: bool,
    shift_right_click_context: bool,
    ctrl_shift_c_copy_link: bool,
    ctrl_s_vertical_tabs: bool,
    pin_new_tab_groups: bool,
    split_edge_drop: bool,
    block_youtube_video_ads: bool,
    /// Which of `settings::KNOWN_TOOLBAR_BUTTONS` (same order) are checked.
    toolbar_buttons: [bool; 8],
    /// What the bottom status row shows for "is Bowser the default browser"; refreshed on open
    /// and after the default browser button is used, not on every paint.
    default_browser_status: String,
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
    youtube_toggle: RECT,
    appearance_heading: RECT,
    appearance_card: RECT,
    theme_button: RECT,
    font_size_button: RECT,
    accent_label: RECT,
    accent_field: RECT,
    accent_edit: RECT,
    ui_font_label: RECT,
    ui_font_field: RECT,
    ui_font_edit: RECT,
    appearance_help: RECT,
    chrome_heading: RECT,
    chrome_card: RECT,
    bookmarks_bar_button: RECT,
    side_panel_button: RECT,
    chrome_toggles: [RECT; 9],
    toolbar_heading: RECT,
    toolbar_card: RECT,
    toolbar_toggles: [RECT; 8],
    behaviour_heading: RECT,
    behaviour_card: RECT,
    zoom_label: RECT,
    zoom_field: RECT,
    zoom_edit: RECT,
    behaviour_toggles: [RECT; 8],
    default_browser_button: RECT,
    default_browser_status: RECT,
    status: RECT,
    open_file: RECT,
    reset: RECT,
    save: RECT,
    close: RECT,
}

/// Three equal-ish columns inside a card's usual 44-556 content width, used for the dense
/// checklists below. `col(0)`/`col(1)`/`col(2)` give each column's `(left, right)`.
fn checklist_column(index: usize) -> (i32, i32) {
    match index {
        0 => (44, 208),
        1 => (216, 380),
        _ => (388, 556),
    }
}

/// `N` checkbox rows packed three to a row, `row_height` tall with a 2px gap between rows,
/// starting at `top`. Row `i` is at `(i / 3, i % 3)` in the grid.
fn checklist_rows<const N: usize>(
    r: &impl Fn(i32, i32, i32, i32) -> RECT,
    top: i32,
    row_height: i32,
) -> [RECT; N] {
    std::array::from_fn(|i| {
        let (left, right) = checklist_column(i % 3);
        let row_top = top + (i as i32 / 3) * (row_height + 2);
        r(left, row_top, right, row_top + row_height)
    })
}

fn layout(dpi: u32) -> Layout {
    let s = |v: i32| scaled(v, dpi);
    let r = |l: i32, t: i32, right: i32, b: i32| rect(s(l), s(t), s(right), s(b));
    let sleep_field = r(224, 442, 328, 472);
    let allow_field = r(44, 626, 556, 656);
    let accent_field = r(388, 932, 556, 966);
    let ui_font_field = r(388, 972, 556, 1006);
    let zoom_field = r(388, 1412, 556, 1446);
    let chrome_toggles: [RECT; 9] = checklist_rows(&r, 1132, 26);
    let toolbar_toggles: [RECT; 8] = checklist_rows(&r, 1272, 26);
    let behaviour_toggles: [RECT; 8] = checklist_rows(&r, 1454, 26);
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
        // The blocker card grew by 30px (compared to before this window gained its other
        // settings) to fit the YouTube ad-block row, which shifts tab groups down by the same
        // amount; everything from "Appearance" on is new.
        blocker_heading: r(28, 494, 572, 516),
        blocker_card: r(28, 522, 572, 728),
        blocker_toggle: r(44, 534, 556, 558),
        blocker_help: r(44, 562, 556, 596),
        allow_label: r(44, 604, 556, 624),
        allow_field,
        allow_edit: inset(allow_field, s(8), s(5)),
        allow_help: r(44, 662, 556, 680),
        youtube_toggle: r(44, 700, 556, 724),
        tab_groups_heading: r(28, 736, 572, 758),
        tab_groups_card: r(28, 764, 572, 844),
        tab_groups_toggle: r(44, 776, 556, 800),
        tab_groups_help: r(44, 804, 556, 838),
        appearance_heading: r(28, 856, 572, 878),
        appearance_card: r(28, 882, 572, 1042),
        theme_button: r(44, 892, 286, 926),
        font_size_button: r(314, 892, 556, 926),
        accent_label: r(44, 932, 380, 966),
        accent_field,
        accent_edit: inset(accent_field, s(8), s(5)),
        ui_font_label: r(44, 972, 380, 1006),
        ui_font_field,
        ui_font_edit: inset(ui_font_field, s(8), s(5)),
        appearance_help: r(44, 1014, 556, 1032),
        chrome_heading: r(28, 1054, 572, 1076),
        chrome_card: r(28, 1080, 572, 1224),
        bookmarks_bar_button: r(44, 1090, 286, 1124),
        side_panel_button: r(314, 1090, 556, 1124),
        chrome_toggles,
        toolbar_heading: r(28, 1236, 572, 1258),
        toolbar_card: r(28, 1262, 572, 1364),
        toolbar_toggles,
        behaviour_heading: r(28, 1376, 572, 1398),
        behaviour_card: r(28, 1402, 572, 1546),
        zoom_label: r(44, 1412, 380, 1446),
        zoom_field,
        zoom_edit: inset(zoom_field, s(8), s(5)),
        behaviour_toggles,
        default_browser_button: r(44, 1562, 310, 1596),
        default_browser_status: r(324, 1562, 556, 1596),
        status: r(28, 1612, 572, 1632),
        open_file: r(28, 1640, 198, 1674),
        reset: r(210, 1640, 342, 1674),
        save: r(354, 1640, 458, 1674),
        close: r(470, 1640, 572, 1674),
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
            round_rect(
                dc,
                l.appearance_card,
                p.omnibox_fill,
                p.omnibox_border,
                scaled(1, dpi),
            );
            round_rect(
                dc,
                l.chrome_card,
                p.omnibox_fill,
                p.omnibox_border,
                scaled(1, dpi),
            );
            round_rect(
                dc,
                l.toolbar_card,
                p.omnibox_fill,
                p.omnibox_border,
                scaled(1, dpi),
            );
            round_rect(
                dc,
                l.behaviour_card,
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
            draw_text(
                dc,
                l.appearance_heading,
                APPEARANCE_HEADING,
                p.tab_text_active,
                single,
            );
            draw_text(
                dc,
                l.chrome_heading,
                CHROME_HEADING,
                p.tab_text_active,
                single,
            );
            draw_text(
                dc,
                l.toolbar_heading,
                TOOLBAR_HEADING,
                p.tab_text_active,
                single,
            );
            draw_text(
                dc,
                l.behaviour_heading,
                BEHAVIOUR_HEADING,
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
            draw_text(dc, l.accent_label, ACCENT_LABEL, p.omnibox_text, single);
            draw_text(dc, l.ui_font_label, UI_FONT_LABEL, p.omnibox_text, single);
            draw_text(
                dc,
                l.appearance_help,
                APPEARANCE_HELP,
                p.omnibox_placeholder,
                single,
            );
            draw_text(dc, l.zoom_label, ZOOM_LABEL, p.omnibox_text, single);
            draw_text(
                dc,
                l.default_browser_status,
                &panel.default_browser_status,
                p.omnibox_placeholder,
                single,
            );

            // The field frames sit outside the edit controls, so the teal ring is drawn here.
            for (id, field) in [
                (ID_SLEEP_SECS, l.sleep_field),
                (ID_BLOCKER_ALLOW, l.allow_field),
                (ID_SIDEBAR_WIDTH, l.layout_width),
                (ID_THEME_ACCENT, l.accent_field),
                (ID_UI_FONT, l.ui_font_field),
                (ID_DEFAULT_ZOOM, l.zoom_field),
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
            id if CHECKLIST_IDS.contains(&id) => {
                let checked = checklist_checked(panel, id);
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
            ID_SEARCH_ENGINE | ID_TAB_LAYOUT | ID_THEME | ID_FONT_SIZE | ID_BOOKMARKS_BAR
            | ID_SIDE_PANEL_POSITION => {
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
            ID_OPEN_FILE | ID_RESET | ID_SAVE | ID_CLOSE | ID_DEFAULT_BROWSER => {
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
                ID_BLOCK_YOUTUBE_VIDEO_ADS,
                YOUTUBE_TOGGLE,
                l.youtube_toggle,
                button,
                ButtonKind::Owner,
            ),
            (ID_THEME, "", l.theme_button, button, ButtonKind::Owner),
            (
                ID_FONT_SIZE,
                "",
                l.font_size_button,
                button,
                ButtonKind::Owner,
            ),
            (
                ID_THEME_ACCENT,
                "",
                l.accent_edit,
                edit_text,
                ButtonKind::Edit,
            ),
            (ID_UI_FONT, "", l.ui_font_edit, edit_text, ButtonKind::Edit),
            (
                ID_BOOKMARKS_BAR,
                "",
                l.bookmarks_bar_button,
                button,
                ButtonKind::Owner,
            ),
            (
                ID_SIDE_PANEL_POSITION,
                "",
                l.side_panel_button,
                button,
                ButtonKind::Owner,
            ),
            (
                ID_CENTRED_ADDRESS_BAR,
                "Centre the address bar",
                l.chrome_toggles[0],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_MINIMAL_URL,
                "Show a simplified address while unfocused",
                l.chrome_toggles[1],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_SHOW_LOAD_PROGRESS,
                "Show a loading progress bar",
                l.chrome_toggles[2],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_FRAMELESS,
                FRAMELESS_TOGGLE,
                l.chrome_toggles[3],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_ROUNDED_CONTENT_FRAME,
                "Rounded corners around the page",
                l.chrome_toggles[4],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_SHOW_HOME_BUTTON,
                "Show the Home button",
                l.chrome_toggles[5],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_BOOKMARKS_BAR_TAB_GROUPS,
                "Show tab groups in the bookmarks bar",
                l.chrome_toggles[6],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_TAB_PREVIEW_IMAGES,
                "Show a thumbnail in tab previews",
                l.chrome_toggles[7],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_TAB_PREVIEW_MEMORY,
                "Show memory use in tab previews",
                l.chrome_toggles[8],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_TOOLBAR_BACK,
                "Back",
                l.toolbar_toggles[0],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_TOOLBAR_FORWARD,
                "Forward",
                l.toolbar_toggles[1],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_TOOLBAR_RELOAD,
                "Reload",
                l.toolbar_toggles[2],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_TOOLBAR_SITE_INFO,
                "Site information",
                l.toolbar_toggles[3],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_TOOLBAR_BOOKMARK,
                "Bookmark",
                l.toolbar_toggles[4],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_TOOLBAR_NEWTAB,
                "New tab",
                l.toolbar_toggles[5],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_TOOLBAR_SPLIT,
                "Split view",
                l.toolbar_toggles[6],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_TOOLBAR_READER,
                "Reader view",
                l.toolbar_toggles[7],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_DEFAULT_ZOOM,
                "",
                l.zoom_edit,
                edit_number,
                ButtonKind::Edit,
            ),
            (
                ID_SHOW_ZOOM_INDICATOR,
                "Show the zoom level in the toolbar",
                l.behaviour_toggles[0],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_NEW_TAB_BESIDE_ACTIVE,
                "Open new tabs next to the active tab",
                l.behaviour_toggles[1],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_CTRL_TAB_MRU,
                "Ctrl+Tab cycles most-recently-used tabs first",
                l.behaviour_toggles[2],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_SHIFT_RIGHT_CLICK_CONTEXT,
                "Shift+Right Click always shows the system menu",
                l.behaviour_toggles[3],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_CTRL_SHIFT_C_COPY_LINK,
                "Ctrl+Shift+C copies the current page's address",
                l.behaviour_toggles[4],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_CTRL_S_VERTICAL_TABS,
                "Ctrl+S collapses the sidebar (vertical layout only)",
                l.behaviour_toggles[5],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_PIN_NEW_TAB_GROUPS,
                "Pin new tab groups to the bookmarks bar",
                l.behaviour_toggles[6],
                button,
                ButtonKind::Owner,
            ),
            (
                ID_SPLIT_EDGE_DROP,
                "Allow dropping a tab on the window edge to split",
                l.behaviour_toggles[7],
                button,
                ButtonKind::Owner,
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
            (
                ID_DEFAULT_BROWSER,
                DEFAULT_BROWSER_BUTTON,
                l.default_browser_button,
                button,
                ButtonKind::Owner,
            ),
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
        panel.theme = settings.theme;
        panel.font_size = settings.font_size;
        panel.bookmarks_bar = settings.bookmarks_bar;
        panel.side_panel_position = settings.side_panel_position;
        panel.centred_address_bar = settings.centred_address_bar;
        panel.minimal_url = settings.minimal_url;
        panel.show_load_progress = settings.show_load_progress;
        panel.frameless = settings.frameless;
        panel.rounded_content_frame = settings.rounded_content_frame;
        panel.show_home_button = settings.show_home_button;
        panel.bookmarks_bar_tab_groups = settings.bookmarks_bar_tab_groups;
        panel.tab_preview_images = settings.tab_preview_images;
        panel.tab_preview_memory = settings.tab_preview_memory;
        panel.show_zoom_indicator = settings.show_zoom_indicator;
        panel.new_tab_beside_active = settings.new_tab_beside_active;
        panel.ctrl_tab_mru = settings.ctrl_tab_mru;
        panel.shift_right_click_context = settings.shift_right_click_context;
        panel.ctrl_shift_c_copy_link = settings.ctrl_shift_c_copy_link;
        panel.ctrl_s_vertical_tabs = settings.ctrl_s_vertical_tabs;
        panel.pin_new_tab_groups = settings.pin_new_tab_groups;
        panel.split_edge_drop = settings.split_edge_drop;
        panel.block_youtube_video_ads = settings.block_youtube_video_ads;
        for (index, &known) in settings::KNOWN_TOOLBAR_BUTTONS.iter().enumerate() {
            panel.toolbar_buttons[index] = settings.toolbar_buttons.iter().any(|b| b == known);
        }
    }
    refresh_search_text(hwnd);
    refresh_layout_text(hwnd);
    refresh_theme_text(hwnd);
    refresh_font_size_text(hwnd);
    refresh_bookmarks_bar_text(hwnd);
    refresh_side_panel_text(hwnd);
    invalidate_child(hwnd, ID_SLEEP_ENABLED);
    invalidate_child(hwnd, ID_BLOCKER_ENABLED);
    invalidate_child(hwnd, ID_TAB_GROUPS);
    for &id in &CHECKLIST_IDS {
        invalidate_child(hwnd, id);
    }
    set_text(hwnd, ID_SLEEP_SECS, &settings.sleep_after_secs.to_string());
    set_text(hwnd, ID_SIDEBAR_WIDTH, &settings.sidebar_width.to_string());
    set_text(hwnd, ID_BLOCKER_ALLOW, &settings.blocker_allow.join(", "));
    set_text(hwnd, ID_THEME_ACCENT, settings.theme_accent.as_deref().unwrap_or(""));
    set_text(hwnd, ID_UI_FONT, &settings.ui_font);
    set_text(hwnd, ID_DEFAULT_ZOOM, &settings.default_zoom.to_string());
    invalidate(hwnd);
}

fn refresh_layout_text(hwnd: HWND) {
    let (text, width) = state(hwnd).map_or((layout_label(settings::TabLayout::default()), 260), |panel| {
        (layout_label(panel.tab_layout), panel.sidebar_width)
    });
    set_text(hwnd, ID_TAB_LAYOUT, text);
    set_text(hwnd, ID_SIDEBAR_WIDTH, &width.to_string());
}

/// The Browser layout button's text. Clicking it steps to the next layout.
fn layout_label(layout: settings::TabLayout) -> &'static str {
    match layout {
        settings::TabLayout::ToolbarTop => "Toolbar above tabs",
        settings::TabLayout::TabsTop => "Tabs above toolbar",
        settings::TabLayout::SingleRow => "Single row",
        settings::TabLayout::Vertical => "Vertical",
        // --- layout (thread T6) ---
        settings::TabLayout::VerticalRight => "Vertical, on the right",
        settings::TabLayout::TabsBottom => "Tabs along the bottom",
        // --- end layout ---
    }
}

/// The layout after `layout` in the settings order, wrapping round.
fn next_layout(layout: settings::TabLayout) -> settings::TabLayout {
    let all = settings::TabLayout::ALL;
    let index = all.iter().position(|candidate| *candidate == layout).unwrap_or(0);
    all[(index + 1) % all.len()]
}

/// The Theme button's text, with the label folded in since the button has no separate caption.
fn theme_label(theme: settings::Theme) -> &'static str {
    match theme {
        settings::Theme::Helium => "Theme: Helium colours",
        settings::Theme::System => "Theme: System colours",
    }
}

fn next_theme(theme: settings::Theme) -> settings::Theme {
    match theme {
        settings::Theme::Helium => settings::Theme::System,
        settings::Theme::System => settings::Theme::Helium,
    }
}

fn font_size_label(size: settings::FontSize) -> &'static str {
    match size {
        settings::FontSize::Small => "Text size: Small",
        settings::FontSize::Medium => "Text size: Medium (recommended)",
        settings::FontSize::Large => "Text size: Large",
    }
}

fn next_font_size(size: settings::FontSize) -> settings::FontSize {
    match size {
        settings::FontSize::Small => settings::FontSize::Medium,
        settings::FontSize::Medium => settings::FontSize::Large,
        settings::FontSize::Large => settings::FontSize::Small,
    }
}

fn bookmarks_bar_label(mode: settings::BookmarksBar) -> &'static str {
    match mode {
        settings::BookmarksBar::Always => "Bookmarks bar: Always show",
        settings::BookmarksBar::Never => "Bookmarks bar: Always hide",
        settings::BookmarksBar::NewTab => "Bookmarks bar: Only on the new tab",
    }
}

/// Cycles Never (the default) -> Always -> New tab -> Never, matching `next_layout`'s style.
fn next_bookmarks_bar(mode: settings::BookmarksBar) -> settings::BookmarksBar {
    match mode {
        settings::BookmarksBar::Never => settings::BookmarksBar::Always,
        settings::BookmarksBar::Always => settings::BookmarksBar::NewTab,
        settings::BookmarksBar::NewTab => settings::BookmarksBar::Never,
    }
}

fn side_panel_position_label(position: settings::SidePanelPosition) -> &'static str {
    match position {
        settings::SidePanelPosition::Left => "Helium panels: Left",
        settings::SidePanelPosition::Right => "Helium panels: Right",
    }
}

fn next_side_panel_position(position: settings::SidePanelPosition) -> settings::SidePanelPosition {
    match position {
        settings::SidePanelPosition::Left => settings::SidePanelPosition::Right,
        settings::SidePanelPosition::Right => settings::SidePanelPosition::Left,
    }
}

/// Whether the checkbox-style control `id` (one of `CHECKLIST_IDS`) is currently checked. `false`
/// for any other id.
fn checklist_checked(panel: &Panel, id: i32) -> bool {
    if let Some(index) = toolbar_button_index(id) {
        return panel.toolbar_buttons[index];
    }
    match id {
        ID_SLEEP_ENABLED => panel.sleep_enabled,
        ID_BLOCKER_ENABLED => panel.blocker_enabled,
        ID_TAB_GROUPS => panel.automatic_tab_grouping,
        ID_BLOCK_YOUTUBE_VIDEO_ADS => panel.block_youtube_video_ads,
        ID_CENTRED_ADDRESS_BAR => panel.centred_address_bar,
        ID_MINIMAL_URL => panel.minimal_url,
        ID_SHOW_LOAD_PROGRESS => panel.show_load_progress,
        ID_FRAMELESS => panel.frameless,
        ID_ROUNDED_CONTENT_FRAME => panel.rounded_content_frame,
        ID_SHOW_HOME_BUTTON => panel.show_home_button,
        ID_BOOKMARKS_BAR_TAB_GROUPS => panel.bookmarks_bar_tab_groups,
        ID_TAB_PREVIEW_IMAGES => panel.tab_preview_images,
        ID_TAB_PREVIEW_MEMORY => panel.tab_preview_memory,
        ID_SHOW_ZOOM_INDICATOR => panel.show_zoom_indicator,
        ID_NEW_TAB_BESIDE_ACTIVE => panel.new_tab_beside_active,
        ID_CTRL_TAB_MRU => panel.ctrl_tab_mru,
        ID_SHIFT_RIGHT_CLICK_CONTEXT => panel.shift_right_click_context,
        ID_CTRL_SHIFT_C_COPY_LINK => panel.ctrl_shift_c_copy_link,
        ID_CTRL_S_VERTICAL_TABS => panel.ctrl_s_vertical_tabs,
        ID_PIN_NEW_TAB_GROUPS => panel.pin_new_tab_groups,
        ID_SPLIT_EDGE_DROP => panel.split_edge_drop,
        _ => false,
    }
}

/// Sets the checkbox-style control `id`'s stored state. Does nothing for any other id.
fn set_checklist(panel: &mut Panel, id: i32, value: bool) {
    if let Some(index) = toolbar_button_index(id) {
        panel.toolbar_buttons[index] = value;
        return;
    }
    match id {
        ID_SLEEP_ENABLED => panel.sleep_enabled = value,
        ID_BLOCKER_ENABLED => panel.blocker_enabled = value,
        ID_TAB_GROUPS => panel.automatic_tab_grouping = value,
        ID_BLOCK_YOUTUBE_VIDEO_ADS => panel.block_youtube_video_ads = value,
        ID_CENTRED_ADDRESS_BAR => panel.centred_address_bar = value,
        ID_MINIMAL_URL => panel.minimal_url = value,
        ID_SHOW_LOAD_PROGRESS => panel.show_load_progress = value,
        ID_FRAMELESS => panel.frameless = value,
        ID_ROUNDED_CONTENT_FRAME => panel.rounded_content_frame = value,
        ID_SHOW_HOME_BUTTON => panel.show_home_button = value,
        ID_BOOKMARKS_BAR_TAB_GROUPS => panel.bookmarks_bar_tab_groups = value,
        ID_TAB_PREVIEW_IMAGES => panel.tab_preview_images = value,
        ID_TAB_PREVIEW_MEMORY => panel.tab_preview_memory = value,
        ID_SHOW_ZOOM_INDICATOR => panel.show_zoom_indicator = value,
        ID_NEW_TAB_BESIDE_ACTIVE => panel.new_tab_beside_active = value,
        ID_CTRL_TAB_MRU => panel.ctrl_tab_mru = value,
        ID_SHIFT_RIGHT_CLICK_CONTEXT => panel.shift_right_click_context = value,
        ID_CTRL_SHIFT_C_COPY_LINK => panel.ctrl_shift_c_copy_link = value,
        ID_CTRL_S_VERTICAL_TABS => panel.ctrl_s_vertical_tabs = value,
        ID_PIN_NEW_TAB_GROUPS => panel.pin_new_tab_groups = value,
        ID_SPLIT_EDGE_DROP => panel.split_edge_drop = value,
        _ => {}
    }
}

fn refresh_theme_text(hwnd: HWND) {
    let text = state(hwnd).map_or(theme_label(settings::Theme::default()), |panel| theme_label(panel.theme));
    set_text(hwnd, ID_THEME, text);
}

fn refresh_font_size_text(hwnd: HWND) {
    let text =
        state(hwnd).map_or(font_size_label(settings::FontSize::default()), |panel| font_size_label(panel.font_size));
    set_text(hwnd, ID_FONT_SIZE, text);
}

fn refresh_bookmarks_bar_text(hwnd: HWND) {
    let text = state(hwnd)
        .map_or(bookmarks_bar_label(settings::BookmarksBar::default()), |panel| {
            bookmarks_bar_label(panel.bookmarks_bar)
        });
    set_text(hwnd, ID_BOOKMARKS_BAR, text);
}

fn refresh_side_panel_text(hwnd: HWND) {
    let text = state(hwnd)
        .map_or(side_panel_position_label(settings::SidePanelPosition::default()), |panel| {
            side_panel_position_label(panel.side_panel_position)
        });
    set_text(hwnd, ID_SIDE_PANEL_POSITION, text);
}

/// Refreshes the read-only "is Bowser the default browser" status row. Reads the registry once,
/// so this is called on open and after the default browser button is used, not on every paint.
fn refresh_default_browser_status(hwnd: HWND) {
    let text = match crate::default_browser::is_http_default() {
        Some(true) => "Bowser is your default browser",
        Some(false) => "Bowser is not your default browser",
        None => "Could not check the default browser",
    };
    if let Some(panel) = state(hwnd) {
        panel.default_browser_status = text.to_owned();
    }
    invalidate(hwnd);
}

/// Registers Bowser and opens Windows' own default-apps picker, reporting the result on the main
/// status line like `open_file`'s error handling, then refreshes the read-only status row.
fn make_default_browser(hwnd: HWND) {
    match crate::default_browser::current_exe() {
        Some(exe) => match crate::default_browser::make_default(&exe) {
            Ok(()) => set_status(
                hwnd,
                "Opened Windows' default apps settings. Choose Bowser there to finish.",
                false,
            ),
            Err(message) => set_status(hwnd, message, true),
        },
        None => set_status(hwnd, "Bowser's own program path could not be found.", true),
    }
    refresh_default_browser_status(hwnd);
}

fn search_text(panel: &Panel) -> &'static str {
    search_selector_text(engine_list_in_use(), panel.search_engine, panel.search_fallback)
}

/// --- wave-1 fixes --- the frameless switch is saved but nothing acts on it yet (thread T11 owns it).
const FRAMELESS_TOGGLE: &str = "Hide toolbar and tabs until the pointer nears the edge (not active yet)";
/// Shown in the search selector once the settings page's engine list decides searches.
const SEARCH_MANAGED: &str = "Set in Settings > Search";

/// Whether the profile has the settings page's engine list, which decides searches instead of the
/// `search_engine` key this form edits.
fn engine_list_in_use() -> bool {
    crate::profile::active().is_ok_and(|profile| profile.root().join(crate::search_engines::FILE_NAME).exists())
}

/// The selector's text: the older engine choice, unless the engine list is in use (then the selector
/// says where the engine is chosen and does nothing).
fn search_selector_text(list_in_use: bool, engine: Option<SearchEngine>, fallback: &'static str) -> &'static str {
    if list_in_use {
        return SEARCH_MANAGED;
    }
    match engine {
        Some(engine) => engine.label(),
        None => fallback,
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
    // --- wave-1 fixes --- the engine list decides searches: this older choice would do nothing.
    if engine_list_in_use() {
        return;
    }
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

/// A snapshot of every owner-drawn control's stored state, read once at the top of `save_form` so
/// the rest of the function works with plain values instead of borrowing `Panel` repeatedly. The
/// plain `EDIT` controls (free text and numeric fields) are read separately with `window_text_of`,
/// since Windows already tracks their text for us.
struct FormState {
    sleep_enabled: bool,
    blocker_enabled: bool,
    search_engine: Option<SearchEngine>,
    automatic_tab_grouping: bool,
    tab_layout: settings::TabLayout,
    theme: settings::Theme,
    font_size: settings::FontSize,
    bookmarks_bar: settings::BookmarksBar,
    side_panel_position: settings::SidePanelPosition,
    centred_address_bar: bool,
    minimal_url: bool,
    show_load_progress: bool,
    frameless: bool,
    rounded_content_frame: bool,
    show_home_button: bool,
    bookmarks_bar_tab_groups: bool,
    tab_preview_images: bool,
    tab_preview_memory: bool,
    show_zoom_indicator: bool,
    new_tab_beside_active: bool,
    ctrl_tab_mru: bool,
    shift_right_click_context: bool,
    ctrl_shift_c_copy_link: bool,
    ctrl_s_vertical_tabs: bool,
    pin_new_tab_groups: bool,
    split_edge_drop: bool,
    block_youtube_video_ads: bool,
    toolbar_buttons: [bool; 8],
}

impl From<&Panel> for FormState {
    fn from(panel: &Panel) -> Self {
        Self {
            sleep_enabled: panel.sleep_enabled,
            blocker_enabled: panel.blocker_enabled,
            search_engine: panel.search_engine,
            automatic_tab_grouping: panel.automatic_tab_grouping,
            tab_layout: panel.tab_layout,
            theme: panel.theme,
            font_size: panel.font_size,
            bookmarks_bar: panel.bookmarks_bar,
            side_panel_position: panel.side_panel_position,
            centred_address_bar: panel.centred_address_bar,
            minimal_url: panel.minimal_url,
            show_load_progress: panel.show_load_progress,
            frameless: panel.frameless,
            rounded_content_frame: panel.rounded_content_frame,
            show_home_button: panel.show_home_button,
            bookmarks_bar_tab_groups: panel.bookmarks_bar_tab_groups,
            tab_preview_images: panel.tab_preview_images,
            tab_preview_memory: panel.tab_preview_memory,
            show_zoom_indicator: panel.show_zoom_indicator,
            new_tab_beside_active: panel.new_tab_beside_active,
            ctrl_tab_mru: panel.ctrl_tab_mru,
            shift_right_click_context: panel.shift_right_click_context,
            ctrl_shift_c_copy_link: panel.ctrl_shift_c_copy_link,
            ctrl_s_vertical_tabs: panel.ctrl_s_vertical_tabs,
            pin_new_tab_groups: panel.pin_new_tab_groups,
            split_edge_drop: panel.split_edge_drop,
            block_youtube_video_ads: panel.block_youtube_video_ads,
            toolbar_buttons: panel.toolbar_buttons,
        }
    }
}

fn save_form(hwnd: HWND) {
    let Some(path) = state(hwnd).map(|panel| panel.path.clone()) else {
        return;
    };
    let Some(form) = state(hwnd).map(|panel| FormState::from(&*panel)) else {
        return;
    };
    let sleep = parse_sleep_after_secs(&window_text_of(hwnd, ID_SLEEP_SECS));
    let allow = parse_blocker_allow(&window_text_of(hwnd, ID_BLOCKER_ALLOW));
    let sidebar_width = window_text_of(hwnd, ID_SIDEBAR_WIDTH)
        .parse::<u16>()
        .ok()
        .filter(|width| {
            (settings::MIN_SIDEBAR_WIDTH..=settings::MAX_SIDEBAR_WIDTH).contains(width)
        });
    let accent = parse_theme_accent(&window_text_of(hwnd, ID_THEME_ACCENT));
    let ui_font = parse_ui_font(&window_text_of(hwnd, ID_UI_FONT));
    let zoom = parse_default_zoom(&window_text_of(hwnd, ID_DEFAULT_ZOOM));

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
    let Some(theme_accent) = accent else {
        focus_field(hwnd, ID_THEME_ACCENT);
        set_status(hwnd, ACCENT_ERROR, true);
        return;
    };
    let Some(ui_font) = ui_font else {
        focus_field(hwnd, ID_UI_FONT);
        set_status(hwnd, UI_FONT_ERROR, true);
        return;
    };
    let default_zoom = match zoom {
        Ok(percent) => percent,
        Err(message) => {
            focus_field(hwnd, ID_DEFAULT_ZOOM);
            set_status(hwnd, message, true);
            return;
        }
    };
    // This native window has no drag-reordering, so the checked buttons are written back in their
    // fixed catalog order, matching `settings::KNOWN_TOOLBAR_BUTTONS`.
    let toolbar_buttons: Vec<String> = settings::KNOWN_TOOLBAR_BUTTONS
        .iter()
        .zip(form.toolbar_buttons)
        .filter(|(_, checked)| *checked)
        .map(|(&id, _)| id.to_owned())
        .collect();
    // The GDI form has no controls for these; the file's current values are kept.
    let current = Settings::load(&path);
    let settings = Settings {
        tab_layout: form.tab_layout,
        sidebar_width,
        sidebar_collapsed: current.sidebar_collapsed,
        side_panel_width: current.side_panel_width,
        sleep_enabled: form.sleep_enabled,
        sleep_after_secs,
        blocker_enabled: form.blocker_enabled,
        blocker_allow,
        search_engine: form.search_engine,
        automatic_tab_grouping: form.automatic_tab_grouping,
        efficiency_mode: current.efficiency_mode,
        tracking_prevention: current.tracking_prevention,
        theme: form.theme,
        theme_accent,
        toolbar_buttons,
        centred_address_bar: form.centred_address_bar,
        minimal_url: form.minimal_url,
        show_load_progress: form.show_load_progress,
        frameless: form.frameless,
        rounded_content_frame: form.rounded_content_frame,
        show_home_button: form.show_home_button,
        bookmarks_bar: form.bookmarks_bar,
        bookmarks_bar_tab_groups: form.bookmarks_bar_tab_groups,
        side_panel_position: form.side_panel_position,
        tab_preview_images: form.tab_preview_images,
        tab_preview_memory: form.tab_preview_memory,
        font_size: form.font_size,
        ui_font,
        default_zoom,
        show_zoom_indicator: form.show_zoom_indicator,
        new_tab_beside_active: form.new_tab_beside_active,
        ctrl_tab_mru: form.ctrl_tab_mru,
        shift_right_click_context: form.shift_right_click_context,
        ctrl_shift_c_copy_link: form.ctrl_shift_c_copy_link,
        ctrl_s_vertical_tabs: form.ctrl_s_vertical_tabs,
        pin_new_tab_groups: form.pin_new_tab_groups,
        split_edge_drop: form.split_edge_drop,
        block_youtube_video_ads: form.block_youtube_video_ads,
        // The GDI form has no controls for the download settings either.
        download_dir: current.download_dir,
        ask_download_location: current.ask_download_location,
        translate_offer: current.translate_offer,
        translate_auto: current.translate_auto,
        translate_target: current.translate_target,
        // --- privacy, history and website appearance (thread T3): kept as the file has them ---
        blocker_level: current.blocker_level,
        cookie_notices: current.cookie_notices,
        history_retention: current.history_retention,
        website_appearance: current.website_appearance,
        force_dark_websites: current.force_dark_websites,
        private_theme: current.private_theme,
        // --- end privacy, history and website appearance ---
        // The GDI form has no start page controls; the file's values are kept.
        start: current.start,
        // Address bar, search and bookmark options have no GDI controls; the file's values are kept.
        address: current.address,
        // The GDI form has no controls for the tab options (the HTML settings page has them).
        tabs: current.tabs,
        // --- layout (thread T6): the Layout block's other options have no GDI controls; kept as the file has them ---
        tab_bar_visible: current.tab_bar_visible,
        address_bar_position: current.address_bar_position,
        address_bar_visible: current.address_bar_visible,
        show_title_bar: current.show_title_bar,
        status_bar: current.status_bar,
        ui_density: current.ui_density,
        compact_menus: current.compact_menus,
        ui_zoom: current.ui_zoom,
        layout_saved: current.layout_saved,
        // --- end layout ---
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
                crate::app::App::apply_live_settings(&app, &settings);
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
        "Reset all settings to their defaults? How long history is kept stays as it is, as do comments and other lines in settings.txt.",
    ) {
        return;
    }
    let Some(path) = state(hwnd).map(|panel| panel.path.clone()) else {
        return;
    };
    // --- wave-1 fixes --- the same reset as the settings page: history retention is kept.
    let defaults = settings::reset_defaults(&Settings::load(&path));
    match settings::save(&path, &defaults) {
        Ok(()) => {
            populate(hwnd, &defaults);
            let app = state(hwnd).map(|panel| panel.app.clone());
            if let Some(app) = app {
                crate::app::App::apply_tab_layout_settings(&app, defaults.tab_layout, defaults.sidebar_width);
                crate::app::App::apply_live_settings(&app, &defaults);
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
    if let Err(message) = launch_settings_file(&path) {
        set_status(hwnd, message, true);
    }
}

/// Creates the commented starter file when none exists, then opens it in the default editor. The
/// error is a plain sentence for the status line or a message box. Shared by the native window and
/// the HTML Settings page.
pub(crate) fn launch_settings_file(path: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = settings::create_template_if_missing(path) {
        return Err(format!("The settings file could not be created ({error})."));
    }
    let wide = HSTRING::from(path.as_os_str());
    let result = unsafe { ShellExecuteW(None, w!("open"), &wide, None, None, SW_SHOWNORMAL) };
    // ShellExecuteW reports failure with a value of 32 or less.
    if (result.0 as isize) <= 32 {
        return Err("The settings file could not be opened.".to_owned());
    }
    Ok(())
}

/// Flips a checkbox's stored state and repaints it. Native clicks and Space both arrive here as
/// `BN_CLICKED`, so keyboard and mouse use the same path.
fn toggle(hwnd: HWND, id: i32) {
    if let Some(panel) = state(hwnd) {
        let current = checklist_checked(panel, id);
        set_checklist(panel, id, !current);
    }
    invalidate_child(hwnd, id);
}

fn command(hwnd: HWND, wparam: WPARAM) {
    let id = (wparam.0 & 0xFFFF) as i32;
    let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
    match (id, code) {
        (
            ID_SLEEP_SECS | ID_BLOCKER_ALLOW | ID_SIDEBAR_WIDTH | ID_THEME_ACCENT | ID_UI_FONT
            | ID_DEFAULT_ZOOM,
            EN_SETFOCUS,
        ) => {
            if let Some(panel) = state(hwnd) {
                panel.focus = id;
            }
            invalidate(hwnd);
        }
        (
            ID_SLEEP_SECS | ID_BLOCKER_ALLOW | ID_SIDEBAR_WIDTH | ID_THEME_ACCENT | ID_UI_FONT
            | ID_DEFAULT_ZOOM,
            EN_KILLFOCUS,
        ) => {
            if let Some(panel) = state(hwnd) {
                if panel.focus == id {
                    panel.focus = 0;
                }
            }
            invalidate(hwnd);
        }
        (id, BN_CLICKED) if CHECKLIST_IDS.contains(&id) => toggle(hwnd, id),
        (ID_SEARCH_ENGINE, BN_CLICKED) => choose_search_engine(hwnd),
        (ID_TAB_LAYOUT, BN_CLICKED) => {
            if let Some(panel) = state(hwnd) {
                panel.tab_layout = next_layout(panel.tab_layout);
            }
            refresh_layout_text(hwnd);
            invalidate_child(hwnd, ID_TAB_LAYOUT);
        }
        (ID_THEME, BN_CLICKED) => {
            if let Some(panel) = state(hwnd) {
                panel.theme = next_theme(panel.theme);
            }
            refresh_theme_text(hwnd);
            invalidate_child(hwnd, ID_THEME);
        }
        (ID_FONT_SIZE, BN_CLICKED) => {
            if let Some(panel) = state(hwnd) {
                panel.font_size = next_font_size(panel.font_size);
            }
            refresh_font_size_text(hwnd);
            invalidate_child(hwnd, ID_FONT_SIZE);
        }
        (ID_BOOKMARKS_BAR, BN_CLICKED) => {
            if let Some(panel) = state(hwnd) {
                panel.bookmarks_bar = next_bookmarks_bar(panel.bookmarks_bar);
            }
            refresh_bookmarks_bar_text(hwnd);
            invalidate_child(hwnd, ID_BOOKMARKS_BAR);
        }
        (ID_SIDE_PANEL_POSITION, BN_CLICKED) => {
            if let Some(panel) = state(hwnd) {
                panel.side_panel_position = next_side_panel_position(panel.side_panel_position);
            }
            refresh_side_panel_text(hwnd);
            invalidate_child(hwnd, ID_SIDE_PANEL_POSITION);
        }
        (ID_DEFAULT_BROWSER, BN_CLICKED) => make_default_browser(hwnd),
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
        // The native layout is fixed, so the window keeps its size and is only moved on screen.
        let (width, height) = (scaled(CLIENT_WIDTH, dpi), scaled(CLIENT_HEIGHT, dpi));
        let mut owner_rect = RECT::default();
        let _ = GetWindowRect(owner, &mut owner_rect);
        let placed = super::html_dialog::dialog_rect(owner_rect, super::html_dialog::work_area_of(owner), width, height, false);
        let (x, y, width, height) = (placed.left, placed.top, placed.right - placed.left, placed.bottom - placed.top);

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
        let mut toolbar_buttons = [false; 8];
        for (index, &known) in settings::KNOWN_TOOLBAR_BUTTONS.iter().enumerate() {
            toolbar_buttons[index] = initial.toolbar_buttons.iter().any(|b| b == known);
        }
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
            theme: initial.theme,
            font_size: initial.font_size,
            bookmarks_bar: initial.bookmarks_bar,
            side_panel_position: initial.side_panel_position,
            centred_address_bar: initial.centred_address_bar,
            minimal_url: initial.minimal_url,
            show_load_progress: initial.show_load_progress,
            frameless: initial.frameless,
            rounded_content_frame: initial.rounded_content_frame,
            show_home_button: initial.show_home_button,
            bookmarks_bar_tab_groups: initial.bookmarks_bar_tab_groups,
            tab_preview_images: initial.tab_preview_images,
            tab_preview_memory: initial.tab_preview_memory,
            show_zoom_indicator: initial.show_zoom_indicator,
            new_tab_beside_active: initial.new_tab_beside_active,
            ctrl_tab_mru: initial.ctrl_tab_mru,
            shift_right_click_context: initial.shift_right_click_context,
            ctrl_shift_c_copy_link: initial.ctrl_shift_c_copy_link,
            ctrl_s_vertical_tabs: initial.ctrl_s_vertical_tabs,
            pin_new_tab_groups: initial.pin_new_tab_groups,
            split_edge_drop: initial.split_edge_drop,
            block_youtube_video_ads: initial.block_youtube_video_ads,
            toolbar_buttons,
            default_browser_status: String::new(),
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
        refresh_default_browser_status(window);
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
            if matches!(
                id,
                ID_SLEEP_SECS | ID_BLOCKER_ALLOW | ID_SIDEBAR_WIDTH | ID_THEME_ACCENT | ID_UI_FONT
                    | ID_DEFAULT_ZOOM
            ) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// --- wave-1 fixes --- the native form does not claim more than it does.
    #[test]
    fn the_native_form_says_what_its_blocker_search_and_frameless_controls_do() {
        assert!(BLOCKER_TOGGLE.contains("None"), "the switch is the None level, not the whole level");
        assert!(BLOCKER_HELP.contains("blocking level"));
        assert!(FRAMELESS_TOGGLE.ends_with("(not active yet)"));
        assert!(TOOLBAR_HEADING.contains("only Site information and Split view act"), "the checklist says which buttons it moves");
        assert_eq!(search_selector_text(true, Some(SearchEngine::Google), SEARCH_DEFAULT), SEARCH_MANAGED, "the engine list decides");
        assert_eq!(search_selector_text(false, Some(SearchEngine::Google), SEARCH_DEFAULT), SearchEngine::Google.label());
        assert_eq!(search_selector_text(false, None, SEARCH_DEFAULT), SEARCH_DEFAULT);
    }

    #[test]
    fn theme_cycles_between_both_values() {
        assert_eq!(next_theme(settings::Theme::Helium), settings::Theme::System);
        assert_eq!(next_theme(settings::Theme::System), settings::Theme::Helium);
        assert_eq!(theme_label(settings::Theme::Helium), "Theme: Helium colours");
        assert_eq!(theme_label(settings::Theme::System), "Theme: System colours");
    }

    #[test]
    fn font_size_cycles_through_all_three_and_back() {
        use settings::FontSize::{Large, Medium, Small};
        assert_eq!(next_font_size(Small), Medium);
        assert_eq!(next_font_size(Medium), Large);
        assert_eq!(next_font_size(Large), Small);
        assert_eq!(font_size_label(Medium), "Text size: Medium (recommended)");
    }

    #[test]
    fn bookmarks_bar_cycles_from_the_default_and_labels_never_as_always_hide() {
        use settings::BookmarksBar::{Always, Never, NewTab};
        assert_eq!(settings::BookmarksBar::default(), Never);
        assert_eq!(next_bookmarks_bar(Never), Always);
        assert_eq!(next_bookmarks_bar(Always), NewTab);
        assert_eq!(next_bookmarks_bar(NewTab), Never);
        assert_eq!(bookmarks_bar_label(Never), "Bookmarks bar: Always hide");
        assert_eq!(bookmarks_bar_label(Always), "Bookmarks bar: Always show");
        assert_eq!(bookmarks_bar_label(NewTab), "Bookmarks bar: Only on the new tab");
    }

    #[test]
    fn side_panel_position_cycles_between_both_values() {
        use settings::SidePanelPosition::{Left, Right};
        assert_eq!(next_side_panel_position(Left), Right);
        assert_eq!(next_side_panel_position(Right), Left);
        assert_eq!(side_panel_position_label(Left), "Helium panels: Left");
    }

    #[test]
    fn toolbar_button_index_matches_the_known_catalog_order() {
        for (index, &(id, _)) in TOOLBAR_BUTTON_ROWS.iter().enumerate() {
            assert_eq!(toolbar_button_index(id), Some(index));
        }
        assert_eq!(toolbar_button_index(ID_SAVE), None);
        assert_eq!(TOOLBAR_BUTTON_ROWS.len(), settings::KNOWN_TOOLBAR_BUTTONS.len());
    }

    #[test]
    fn checklist_ids_has_no_duplicates() {
        let mut sorted = CHECKLIST_IDS.to_vec();
        sorted.sort_unstable();
        let mut deduped = sorted.clone();
        deduped.dedup();
        assert_eq!(sorted.len(), deduped.len());
    }
}
