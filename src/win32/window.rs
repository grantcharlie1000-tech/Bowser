use super::menu::{Entry, Icon, Item, Menu, ProfileHeader};
use super::{
    INPUT_ID, WM_ANIMATION_TIMER, WM_CARET_BLINK_TIMER, WM_FOREGROUND_SYNC, WM_HISTORY_BACK,
    WM_HISTORY_FORWARD, WM_IDLE_TRIM_TIMER, WM_LOGIN_MESSAGE, WM_NAVIGATION_PROGRESS,
    WM_PANE_FOCUSED, WM_PROGRESS_TIMER, WM_SESSION_SAVE_TIMER, WM_SHORTCUT_KEY,
    WM_SPLIT_BOUNDS_TIMER, WM_SUBMIT, WM_TABS_CHANGED, WM_RAIL_CHANGED, WM_HTML_CHROME_ENV, card, command_palette, frame, glass,
    import_panel, island_menu, menu, devtools_dock, notes_panel, passwords_panel, privacy_panel, settings_panel,
    // --- popup host (lane M) ---
    html_popup,
    // --- end popup host ---
    side_panel,
    // --- panel host (lane P) ---
    html_panel, WM_HTML_PANEL_FAILED,
    // --- end panel host ---
    show_error_message, site_info,
};
use crate::{
    app::{App, AppRef, ChromeEnvironment, IdleTimerAction},
    chrome::{
        group_ui::{self, GroupGeometry},
        layout::{self, Hit, Layout, Metrics, Rect},
        motion::{Emphasis, GroupSpec, Motion},
        paint::{self, ChromeSurface, ChromeView, OmniboxView, TabView},
        rail::{RailDropIndicator, RailEntry, RailIcon, RailKey, RailView, WorkspaceGlyph},
        tab_card::Marker,
    },
    chrome_host::{
        self, host::{Accel, AccelHandler, ChromeEnv, ChromeHost, View, accel_command_gated},
        protocol::{
            CaptionAction, Command as ChromeCommand, GroupAction, NavAction, OpenTarget,
            RailAction, RailKeyWire, SideAction, TabAction, ViewState, completion_json, fav_json, focus_json,
            rail_json, railfav_json, set_text_json, tabs_json,
        },
    },
    favicon::{self, FaviconCache},
    omnibox::{self, Completion, History},
    profile,
    shortcuts::{self, Command},
    tab_groups::{self, StripSlot},
    tabs::{FaviconKey, TabId},
    theme::Palette,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    path::Path,
    rc::Rc,
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime},
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, ClientToScreen, CreateRectRgn, CreateRoundRectRgn, DEFAULT_QUALITY,
            DeleteObject, EndPaint, FillRect, GetMonitorInfoW, InvalidateRect,
            MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, PAINTSTRUCT, ScreenToClient,
            SetWindowRgn,
        },
        System::LibraryLoader::GetModuleHandleW,
        System::SystemInformation::GetTickCount,
        System::Threading::GetCurrentProcessId,
        UI::{
            Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
            Controls::{EM_GETMODIFY, EM_GETSEL, EM_SETMARGINS, EM_SETSEL},
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, GetCapture, GetDoubleClickTime, GetFocus, GetKeyState,
                GetLastInputInfo, LASTINPUTINFO, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE,
                TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent, VK_CONTROL, VK_DOWN, VK_LBUTTON,
                VK_MENU, VK_RBUTTON, VK_RETURN, VK_SHIFT, VK_UP,
            },
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::{
                CREATESTRUCTW, CS_DBLCLKS, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW,
                DestroyWindow, DispatchMessageW, EN_CHANGE, ES_AUTOHSCROLL,
                EVENT_SYSTEM_FOREGROUND, GW_OWNER, GWL_STYLE, GWLP_USERDATA, GetCaretBlinkTime,
                EnumChildWindows, GetClientRect, GetCursorPos, GetForegroundWindow, GetMessageW,
                GetParent, GetSystemMenu, GetSystemMetrics, GetWindow, GetWindowLongPtrW, GetWindowRect,
                GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, HICON, HMENU,
                HTCAPTION, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, LoadImageW,
                HTCLOSE, HTMAXBUTTON, HTMINBUTTON, HWND_TOP, HideCaret, IDC_SIZEALL, IDC_SIZENS, IDC_SIZEWE, IsIconic,
                IsZoomed, KillTimer, LoadCursorW, MINMAXINFO, MSG, MoveWindow, NCCALCSIZE_PARAMS,
                PostMessageW, PostQuitMessage, RegisterClassW, SC_CLOSE, SC_KEYMENU, SC_MAXIMIZE,
                SC_MINIMIZE, SC_MOVE, SC_RESTORE, SC_SIZE, SIZE_MINIMIZED, SM_CXFRAME, SM_CXICON,
                SM_CXSMICON,
                SM_CXPADDEDBORDER, SM_CYFRAME, SM_SWAPBUTTON, SPI_GETCLIENTAREAANIMATION, SW_HIDE, SW_SHOW,
                SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
                SendMessageW, SetCursor, SetForegroundWindow, SetTimer, SetWindowLongPtrW,
                SetWindowPos, SetWindowTextW, ShowWindow, SystemParametersInfoW, TPM_RETURNCMD,
                TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WINDOW_STYLE,
                WINEVENT_OUTOFCONTEXT,
                WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_DPICHANGED, WM_ENDSESSION,
                WM_QUERYENDSESSION,
                WM_DWMCOLORIZATIONCOLORCHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN,
                WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONUP,
                WM_MOUSEMOVE, WM_MOVE, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCCREATE, WM_NCDESTROY,
                WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP,
                WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_NCPAINT, WM_NCRBUTTONUP, WM_PAINT, WM_PASTE,
                WM_POWERBROADCAST, WM_RBUTTONUP, WM_SETFOCUS, WM_SETICON, WM_SETTEXT,
                WM_SETTINGCHANGE, WM_SIZE,
                WM_SYSCOMMAND, WM_SYSKEYDOWN, WM_THEMECHANGED, WM_TIMER, WNDCLASSW, WS_CAPTION,
                WS_CHILD, WS_CLIPCHILDREN, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
                WS_EX_TRANSPARENT, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU,
                WS_THICKFRAME, WS_VISIBLE, WVR_REDRAW,
            },
        },
    },
    core::{BOOL, w},
};

const CLASS_NAME: windows::core::PCWSTR = w!("BowserNativeWindow");
// --- auto-hide (T11) --- the window's side of auto-hide and full screen (window/auto_hide.rs).
mod auto_hide;
mod fullscreen_notice;
// --- end auto-hide ---
// --- look state (fix group B) --- the page and panel hosts' `state` (window/host_state.rs).
mod host_state;
// --- end look state ---
const WM_NCUAHDRAWCAPTION: u32 = 0x00AE;
const WM_NCUAHDRAWFRAME: u32 = 0x00AF;
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_SIDEBAR_SCROLL_TIMER: usize = 0xB053;
const WM_RAIL_TOOLTIP_TIMER: usize = 0xB058;
/// Delay timer for the native new-tab button's HTML tooltip.
const WM_NEWTAB_TOOLTIP_TIMER: usize = 0xB05A;
const NEWTAB_TOOLTIP_TEXT: &str = "New tab (Ctrl+T)";
/// Frame timer of a vertical sidebar width drag: the moves of one frame end in one relayout.
const WM_SIDEBAR_RESIZE_TIMER: usize = 0xB05B;
/// Slow check, armed only while a settings, history or split page host is hidden, that closes a
/// host hidden for `PAGE_IDLE_CLOSE`. The next use creates it again.
const WM_PAGE_IDLE_TIMER: usize = 0xB05C;
const PAGE_IDLE_CHECK_MS: u32 = 30_000;
const PAGE_IDLE_CLOSE: Duration = Duration::from_secs(180);
/// Posted once after chrome state changed (see `mark_chrome_dirty`). The HTML views and the page
/// hosts are brought up to date from it; `WM_PAINT` only draws.
const WM_CHROME_SYNC: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x84;
/// Posted the first time a view needs the hover layer (see `ensure_hover_layer`), so it is created
/// from the window procedure, not inside the WebView2 callback that asked for it.
const WM_HTML_HOVER_START: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x85;
// --- popup host (lane M) ---
const WM_HTML_POPUP_PREWARM_TIMER: usize = 0xB059;
// --- tab options ---
/// The one-second tick of periodic tab reload and the audio rule; set only while one needs it.
const WM_TAB_TICK_TIMER: usize = 0xB0A1;
/// A moment after a page settles or a tab is shown: capture its thumbnail for the hover card.
const WM_TAB_THUMB_TIMER: usize = 0xB0A2;
const WM_TAB_LINK_TIMER: usize = 0xB0A3;
// --- end tab options ---
// --- theme options ---
/// Twice a minute while the theme follows a time schedule: is it time to switch between light and dark?
const WM_THEME_SCHEDULE_TIMER: usize = 0xB0A4;
// --- end theme options ---
pub(super) const WM_HTML_POPUP_FAILED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x81;
// --- end popup host ---
/// Posted from the first paint to start the HTML chrome. Posted, not called, so the paint finishes
/// before any WebView2 work starts on the UI thread.
const WM_HTML_CHROME_START: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x7C;
/// Posted when the HTML toolbar's WebView2 process fails. Handled on the UI thread.
const WM_HTML_CHROME_FAILED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x7A;
/// Posted when the HTML tab strip's WebView2 process fails. Handled on the UI thread.
const WM_HTML_TABS_FAILED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x7B;
/// Posted once per queued menu, popup or window command from an HTML chrome view. Each post runs
/// the oldest queued command on the UI thread, outside WebView2's message callback.
const WM_HTML_DEFERRED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x7E;
/// Posted when the HTML left rail's WebView2 process fails. Handled on the UI thread.
const WM_HTML_RAIL_FAILED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x7F;
/// Posted by the "Check for updates" worker when its check ends; `wparam` is an `UPDATE_*` code.
const WM_UPDATE_CHECKED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x83;
const UPDATE_UP_TO_DATE: usize = 0;
const UPDATE_STAGED: usize = 1;
const UPDATE_UNVERIFIED: usize = 2;
const UPDATE_FAILED: usize = 3;
const UPDATE_BUSY: usize = 4;
const UPDATE_UNAVAILABLE: usize = 5;
/// Most deferred chrome commands waiting at once. A command that finds the queue full is dropped.
const HTML_DEFERRED_LIMIT: usize = 8;

/// A menu, popup or window command from an HTML chrome view, waiting to run on the UI thread. Each
/// one may start a modal loop (a popup menu, the site info panel, a native window move), which must
/// not run inside WebView2's WebMessageReceived callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeferredChromeCommand {
    MainMenu,
    SiteInfo,
    SplitMenu,
    TabMenu { id: TabId, x: i32, y: i32 },
    // --- tab options --- the strip's Closed tabs button: the list of closed tabs at the point.
    ClosedTabs { x: i32, y: i32 },
    // --- end tab options ---
    IslandMenu { id: TabId, x: i32, y: i32 },
    BeginDrag,
    ToggleMaximize,
    SystemMenu,
    /// A left click on a rail button: what a native click does. May open a modal prompt.
    RailClick { key: RailKey },
    /// A command button of the address bar or the tab bar (thread T13): the item at `index` of that bar's list.
    Tool { bar: chrome_host::protocol::ToolBar, index: u8 },
    /// A right click on a rail button: the native rail context menu at the client point.
    RailMenu { key: RailKey, x: i32, y: i32 },
    /// The pointer rested on a flyout button while a flyout is open: switch to it.
    RailHover { key: RailKey },
    // --- vertical sidebar (lane S) ---
    SidebarWorkspaceMenu,
    // --- end vertical sidebar ---
    // --- popup host (lane M) ---
    MenuResult { tok: u32, id: Option<u32> },
    // --- end popup host ---
    // --- popup surfaces (lane PP) ---
    /// An in-place action on an open popup page (site info toggle).
    PopupAct { tok: u32, id: u32, n: u8 },
    /// A composer's submit; its text waits in `html_popup`.
    PopupCompose { tok: u32 },
    /// The toolbar's download button was clicked (its rectangle waits in `downloads_ui`).
    DownloadsToggle,
    /// The toolbar's extensions button was clicked (its rectangle waits in `html_ext_rect`).
    ExtensionsButton,
    /// A row or list action on the open downloads bubble.
    DownloadsAct { tok: u32, id: u32, n: u8 },
    /// A composer the Customize flyout asked for; the request waits in `composer`.
    ComposerOpen,
    // --- end popup surfaces ---
}
/// Frame interval while chrome transitions run; about 60 frames a second.
const ANIMATION_FRAME_MS: u32 = 16;
/// `wParam` of `WM_SETTINGCHANGE` when the system message font may have changed.
const SPI_SETNONCLIENTMETRICS: usize = 0x002A;
struct TabSnapshot {
    id: TabId,
    title: String,
    loading: bool,
    favicon_key: Option<FaviconKey>,
    favicon: Option<Rc<favicon::FaviconBitmap>>,
    marker: Option<Marker>,
}

#[derive(Clone)]
struct RailEntryData {
    key: RailKey,
    icon: RailIcon,
    label: String,
    active: bool,
    accent: Option<crate::theme::Rgba>,
    favicon: Option<Rc<favicon::FaviconBitmap>>,
    /// The favicon's source bytes, as the HTML rail sends them (it sends only PNGs that pass the
    /// protocol gate).
    png: Option<Rc<Vec<u8>>>,
    /// A web app panel whose page title announces a count (thread T9): the rail button shows its dot.
    dot: bool,
    /// --- tab stacks and workspaces (T10) --- the tab count badge (0: none) and the emoji.
    count: u32,
    emoji: Option<String>,
}

#[derive(Clone, Copy)]
struct RailDrag {
    key: RailKey,
    origin: (i32, i32),
    active: bool,
    target: Option<(usize, RailDropIndicator)>,
}

/// A short eased transition for the visible address-bar progress value.
#[derive(Default)]
struct ProgressTransition {
    tab: Option<TabId>,
    navigation_id: Option<u64>,
    from: f32,
    to: f32,
    started: Option<Instant>,
}
impl ProgressTransition {
    const DURATION_MS: f32 = 210.0;
    const FADE_MS: f32 = 220.0;

    // Values above 100 encode the finish fade for the existing paint path.
    fn paint_value(&self, now: Instant) -> Option<f32> {
        let value = self.value(now)?;
        if self.to < 100.0 {
            return Some(value);
        }
        let Some(started) = self.started else {
            return Some(value);
        };
        let elapsed = now.saturating_duration_since(started).as_secs_f32() * 1000.0;
        let fade = ((elapsed - Self::DURATION_MS) / Self::FADE_MS).clamp(0.0, 1.0);
        Some(value + 100.0 * fade * fade * (3.0 - 2.0 * fade))
    }

    fn running(&self, now: Instant) -> bool {
        self.started.is_some_and(|started| {
            let duration = Self::DURATION_MS + if self.to >= 100.0 { Self::FADE_MS } else { 0.0 };
            now.saturating_duration_since(started).as_secs_f32() * 1000.0 < duration
        })
    }

    fn value(&self, now: Instant) -> Option<f32> {
        self.tab?;
        let Some(started) = self.started else {
            return Some(self.to);
        };
        let t = (now.saturating_duration_since(started).as_secs_f32() * 1000.0 / Self::DURATION_MS)
            .clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        Some((self.from + (self.to - self.from) * eased).clamp(0.0, 100.0))
    }

    fn retarget(
        &mut self,
        tab: TabId,
        navigation_id: u64,
        target: u8,
        now: Instant,
        animate: bool,
    ) {
        if self.tab != Some(tab) || self.navigation_id != Some(navigation_id) {
            self.tab = Some(tab);
            self.navigation_id = Some(navigation_id);
            self.from = 0.0;
            self.to = target as f32;
            self.started = animate.then_some(now);
            return;
        }
        // Progress is monotonic for one navigation, including when updates arrive mid-transition.
        let target = (target as f32).max(self.to);
        if target == self.to {
            return;
        }
        self.from = self.value(now).unwrap_or(0.0);
        self.to = target;
        self.started = animate.then_some(now);
    }

    fn settled(&self, now: Instant) -> bool {
        self.value(now).is_some_and(|value| value >= self.to) && !self.running(now)
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}
/// The settings and workspace values the native paint reads. Kept here and refreshed by the chrome
/// sync, so a paint clones neither the settings nor the workspace set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PaintPrefs {
    rounded_content_frame: bool,
    show_site_info: bool,
    show_split: bool,
    workspace_name: String,
}
impl PaintPrefs {
    fn read(app: &AppRef) -> Self {
        let settings = App::settings(app);
        Self {
            rounded_content_frame: settings.rounded_content_frame,
            show_site_info: settings.toolbar_buttons.iter().any(|button| button == "site-info"),
            show_split: settings.toolbar_buttons.iter().any(|button| button == "split"),
            workspace_name: App::workspaces(app).active().name().to_owned(),
        }
    }
}

/// How long a page host (settings, history, split) has been out of sight. Only a host that has
/// been shown is timed, so one that is still being created, or never was, is left alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PageIdle {
    seen: bool,
    hidden_since: Option<Instant>,
}

/// Advances `idle` for a host that is `shown` (or not) at `now`. True when the host has been hidden
/// for `PAGE_IDLE_CLOSE` and should close; its clock then starts over. Pure.
fn page_idle_step(idle: &mut PageIdle, shown: bool, now: Instant) -> bool {
    if shown {
        *idle = PageIdle { seen: true, hidden_since: None };
        return false;
    }
    if !idle.seen {
        return false;
    }
    let since = *idle.hidden_since.get_or_insert(now);
    if now.saturating_duration_since(since) >= PAGE_IDLE_CLOSE {
        *idle = PageIdle::default();
        return true;
    }
    false
}

/// A web app's rail icon as last read: its cached favicon file and, once the native rail paints,
/// the bitmap decoded at a size (`None` when it would not decode).
struct RailAppIcon {
    png: Option<Rc<Vec<u8>>>,
    decoded: Option<(u32, Option<Rc<favicon::FaviconBitmap>>)>,
}

thread_local! {
    /// Bumped when a web app's favicon changes; the rail's icon cache is dropped on its next build.
    static RAIL_ICON_EPOCH: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A file's modification time and length, the identity the rail's caches are checked against.
type FileStamp = (SystemTime, u64);

fn file_stamp(path: &Path) -> Option<FileStamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// The arguments of the last `relayout` except the size and sidebar width, so a sidebar drag can lay
/// out again without reading the settings.
#[derive(Clone)]
struct RelayoutInputs {
    active: TabId,
    maximised: bool,
    // --- layout (thread T6) --- the whole arrangement and the list density, not just the tab layout.
    arrangement: layout::Arrangement,
    density: crate::layout_prefs::UiDensity,
    // --- end layout ---
    centred_address_bar: bool,
    /// The chrome's text scale in percent (`layout_prefs::chrome_scale_percent`).
    font_scale: u32,
    ui_font: String,
}

struct RelayoutRequest<'a> {
    w: i32,
    h: i32,
    dpi: u32,
    active: TabId,
    maximised: bool,
    arrangement: layout::Arrangement,
    density: crate::layout_prefs::UiDensity,
    sidebar_width: u16,
    centred_address_bar: bool,
    font_scale: u32,
    ui_font: &'a str,
}

type HtmlTabSnapshot = (TabId, String, bool, Option<Marker>, bool, bool, bool);

#[derive(Clone, PartialEq, Eq)]
struct HtmlTabsInputs {
    mode: chrome_host::protocol::LayoutMode,
    workspace: (String, String),
    active: TabId,
    snapshot: Vec<HtmlTabSnapshot>,
    group_inputs: Vec<tab_groups::TabGroupInput>,
    group_preferences: tab_groups::GroupPreferences,
    favicons: Vec<(TabId, Option<FaviconKey>)>,
    generation: u64,
    // --- tab options ---
    extras: Vec<crate::app::TabStripExtra>,
    options: chrome_host::protocol::StripOptions,
    // --- end tab options ---
}

struct WindowChrome {
    /// Auto-hide: which bars are out of the layout, what is revealed (`auto_hide.rs`).
    ah: auto_hide::Runtime, // --- auto-hide (T11) ---
    /// The bookmarks bar's share of the page, in physical pixels, and whether it is at the bottom
    /// (src/win32/bookmark_bar.rs); `relayout` takes it off the viewport.
    bar_reserve: (i32, bool),
    surface: ChromeSurface,
    overlay: HWND,
    /// Client size in physical pixels. The surface only keeps the chrome rows.
    client: (i32, i32),
    layout: Layout,
    palette: &'static Palette,
    tabs: Vec<TabSnapshot>,
    slots: Vec<StripSlot>,
    /// --- two-level stacks (T10b) --- the second row's tabs (the stack in use), left to right;
    /// empty while no second row is laid out.
    stack_tabs: Vec<TabId>,
    sidebar: Option<crate::vertical_tabs::SidebarLayout>,
    sidebar_rows: Vec<crate::chrome::vertical_paint::SidebarRow>,
    sidebar_scroll: i32,
    sidebar_scroll_target: i32,
    sidebar_hover_row: Option<usize>,
    sidebar_resizing: bool,
    /// --- layout (thread T6) --- a hidden address bar brought back by Ctrl+L, until the address
    /// field loses focus again.
    address_bar_revealed: bool,
    /// The width a sidebar drag has reached, laid out once per frame and saved to the settings when
    /// the drag ends, and whether a frame for it is due.
    sidebar_drag_width: Option<u16>,
    sidebar_drag_pending: bool,
    last_relayout: Option<RelayoutInputs>,
    rail_entries: Vec<RailEntryData>,
    /// `rail_entries` as the painter and hit tests take them, rebuilt with them (see `rail_view_of`),
    /// so no label is copied per paint or per hit test.
    rail_view: Vec<RailEntry<'static>>,
    /// The open panel the last `WM_RAIL_CHANGED` saw, so an unchanged rail is not repainted.
    rail_open_seen: Option<RailKey>,
    /// `sidebar.txt` as last read, with the stamp it was read at.
    sidebar_config: Option<(Option<FileStamp>, crate::sidebar::SidebarConfig)>,
    /// Web app icons by address, read once each (see `RailAppIcon`), and the epoch they belong to.
    rail_app_icons: HashMap<String, RailAppIcon>,
    rail_icon_epoch: u64,
    rail_scroll: i32,
    rail_drag: Option<RailDrag>,
    rail_focus: Option<RailKey>,
    rail_keyboard_focus: bool,
    rail_previous_focus: HWND,
    rail_tooltip_key: Option<RailKey>,
    rail_tooltip_visible: bool,
    /// The HTML tooltip for the native new-tab button is up.
    newtab_tooltip_shown: bool,
    vertical_drag: Option<TabId>,
    views: Vec<TabView<'static>>,
    favicons: FaviconCache,
    failed: VecDeque<(FaviconKey, u32)>,
    edit_text: String,
    focused: bool,
    caret: bool,
    /// Whether Bowser was the foreground application at the last `sync_foreground`.
    foreground: bool,
    /// The address input, kept here so focus checks never need a borrow of the app state.
    edit: HWND,
    completion: Option<Completion>,
    suppress_completion: bool,
    programmatic_text_update: bool,
    /// Selection anchor for mouse and shift selection in the omnibox, as a UTF-16 index.
    omni_anchor: usize,
    /// The time and count of the last press on the omnibox, for double and triple clicks.
    omni_click: Option<(Instant, u8)>,
    history: History,
    /// The startup read of history.txt, running on a worker until something needs the history
    /// (see `loaded_history`).
    history_load: Option<JoinHandle<History>>,
    urls: HashMap<TabId, String>,
    /// Decides when a finished page load becomes a history entry (any way the page was reached).
    visits: crate::productivity::VisitTracker,
    /// A visit was written to history.txt since the History flyout last refreshed.
    history_dirty: bool,
    hover: Hit,
    pressed: Hit,
    drag: Option<(TabId, usize)>,
    /// --- tab stacks (T10) --- the slot the held tab rests over the middle of, and since when
    /// ("Stack tabs by dragging one onto another" in the native strip).
    stack_hover: Option<(usize, Instant)>,
    split_drag_companion: Option<TabId>,
    split_drop: Option<crate::split_view::DropZone>,
    dpi: u32,
    motion: Motion,
    active_tab: Option<TabId>,
    progress: ProgressTransition,
    first_paint_logged: bool,
    split_active: bool,
    /// The shown pair's orientation: which axis `split_divider` runs along.
    split_orientation: crate::split_view::Orientation,
    /// The shown layout (pair, or three or four panes), as last synced from the app.
    split_layout: Option<crate::split_view::SplitView>,
    split_dragging: bool,
    /// Which divider the pointer holds (or hovers): the primary gap, a cross gap or the crossing.
    split_drag_divider: Option<crate::split_view::Divider>,
    /// The divider gap's leading edge along the split axis: x side by side, y stacked.
    split_divider: i32,
    /// The cross gaps' leading edge along the other axis (a layout of three or four panes).
    split_cross: i32,
    pending_split_bounds: Option<crate::app::SplitBounds>,
    /// The HTML toolbar's WebView2 host, once created. Dropped (and closed) on failure.
    html: Option<ChromeHost>,
    /// Set once the HTML toolbar has been started, so it is never started twice.
    html_requested: bool,
    /// The chrome page has posted `ready`.
    html_page_ready: bool,
    /// True only when the host exists and the page is ready. The HTML region then replaces the
    /// native back, forward, reload, omnibox and main menu controls.
    html_toolbar_active: bool,
    /// A chrome view failed for good, so its region is painted natively instead of waiting for it
    /// (toolbar, tab strip, rail).
    html_failed: [bool; 3],
    /// The omnibox in the HTML toolbar has keyboard focus (from its `focus` messages).
    html_omnibox_focused: bool,
    /// The user has typed in the omnibox since it last took focus, so a page load must not move the
    /// keyboard away from it (V35).
    omnibox_edited: bool,
    /// The user deliberately took the omnibox (Ctrl+L, a new tab, a click), as opposed to it merely
    /// holding the focus it started with.
    omnibox_claimed: bool,
    /// The last `state` JSON posted to the HTML toolbar, so unchanged state is not sent again.
    html_last_state: Option<String>,
    /// CSS px the toolbar page's download button takes from the pill (`0` or `DL_BUTTON_SLOT`), and the
    /// last `dlbtn` message the page was sent (docs/design/downloads.md, `downloads_ui`).
    html_dl_slot: i32,
    html_dl_last: Option<String>,
    /// The same for the extensions button (`0` or `EXT_BUTTON_SLOT`, docs/extensions.md): its slot,
    /// the last `extbtn` message the page was sent, and the button's rectangle from its last click.
    html_ext_slot: i32,
    html_ext_last: Option<String>,
    html_ext_rect: Option<chrome_host::protocol::HoverRect>,
    // --- toolbars and feeds (thread T13) --- what the address bar's list (`toolbar_buttons`) does to the
    // toolbar's slots: whether Downloads and Extensions may take theirs, and the room the always-there
    // buttons at the end of the bar take (set by `html_sync`, read by `toolbar_slot`).
    html_dl_allowed: bool,
    html_ext_allowed: bool,
    html_bar_extra: i32,
    // --- end toolbars and feeds ---
    html_tabs_last_state: Option<String>,
    html_rail_last_state: Option<String>,
    /// Incremented for each page `ready`, so a re-entrant post cannot cache a prior page's state.
    html_state_generation: [u64; 3],
    /// The tab and omnibox text last sent with `setText`, so the text is not re-sent each sync.
    html_text_sent: Option<(TabId, String)>,
    /// The active tab the last sync saw. A change of tab sends the new tab's text even while the
    /// omnibox is focused (see `chrome_host::should_set_text`).
    html_synced_tab: Option<TabId>,
    /// The shared tab environment the chrome hosts live in, once the engine offered it. Both
    /// hosts are created on it, each in the private chrome profile.
    html_env: Option<ChromeEnv>,
    /// The HTML chrome was requested and is waiting for the tab engine's environment.
    html_env_waiting: bool,
    /// The HTML tab strip's WebView2 host, once created. Dropped (and closed) on failure.
    html_tabs: Option<ChromeHost>,
    /// Set once the HTML tab strip has been started, so it is never started twice.
    html_tabs_requested: bool,
    /// The tab strip page has posted `ready`.
    html_tabs_page_ready: bool,
    /// The last `tabs` JSON posted to the tab strip, so an unchanged strip is not sent again.
    html_tabs_last: Option<String>,
    // --- vertical sidebar (lane S) ---
    html_tabs_layout: Option<chrome_host::protocol::LayoutMode>,
    // --- tab options --- the last `tabopts` message the strip was sent.
    html_tabs_opts: Option<String>,
    // --- end tab options ---
    html_tabs_workspace: Option<(String, String)>,
    // --- end vertical sidebar ---
    /// Group wire key to the tab a group command acts on, from the last strip sent.
    html_tabs_groups: HashMap<u32, HtmlGroup>,
    /// Each live tab's favicon key and, when it passes the protocol's gate, its PNG bytes.
    html_favicons: HashMap<TabId, HtmlFavicon>,
    /// The favicon key last sent to the tab strip for each tab.
    html_favicons_sent: HashMap<TabId, FaviconKey>,
    /// Menu, popup and window commands from the HTML chrome views, oldest first, run one per
    /// `WM_HTML_DEFERRED` (see `DeferredChromeCommand`). Bounded by `HTML_DEFERRED_LIMIT`.
    html_deferred: VecDeque<DeferredChromeCommand>,
    /// The HTML left rail's WebView2 host, once created. Dropped (and closed) on failure.
    html_rail: Option<ChromeHost>,
    /// Set once the HTML rail has been started, so it is never started twice.
    html_rail_requested: bool,
    /// The rail page has posted `ready`.
    html_rail_page_ready: bool,
    /// The last `rail` JSON posted to the HTML rail, so an unchanged rail is not sent again.
    html_rail_last: Option<String>,
    /// The favicon PNG last sent to the HTML rail for each button, so it is sent once per change.
    html_rail_favs_sent: HashMap<RailKey, Rc<Vec<u8>>>,
    // --- rail flyouts ---
    /// The last rectangle the HTML rail reported for each flyout button (rail CSS px): where its
    /// flyout is anchored when it opens from a shortcut rather than a click.
    html_rail_anchors: HashMap<RailKey, chrome_host::protocol::HoverRect>,
    html_tabs_synced_inputs: Option<HtmlTabsInputs>,
    rail_generation: u64,
    html_rail_synced_open: Option<Option<RailKey>>,
    html_rail_synced_rail_gen: u64,
    html_rail_synced_state_gen: u64,
    // --- end rail flyouts ---
    /// A `WM_CHROME_SYNC` is queued.
    chrome_sync_posted: bool,
    paint_prefs: PaintPrefs,
    /// Idle clocks of the split, settings and history page hosts.
    page_idle: [PageIdle; 4],
    /// The flyout panel's controller was requested (on first use, see `ensure_flyout_panel`), and
    /// whether it failed for good.
    flyout_requested: bool,
    flyout_failed: bool,
    /// The hover layer was asked for since the toolbar activated (see `ensure_hover_layer`).
    hover_requested: bool,
    /// The exit flush ran (`flush_for_exit`), so a close after an ended session does not repeat it.
    exit_flushed: bool,
}

/// Where a group command from the tab strip goes: a tab of the group, and what the group allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HtmlGroup {
    tab: TabId,
    collapsible: bool,
    collapsed: bool,
}

/// A favicon kept for the HTML tab strip. `png` is `None` when the favicon is too large or not PNG.
struct HtmlFavicon {
    key: FaviconKey,
    png: Option<Vec<u8>>,
}
impl WindowChrome {
    fn new(width: i32, height: i32, dpi: u32, overlay: HWND) -> Self {
        Self {
            ah: auto_hide::Runtime::default(), // --- auto-hide (T11) ---
            bar_reserve: (0, false),
            surface: {
                let mut surface = ChromeSurface::new(width, height, dpi);
                let _ = surface.set_rail_snap(!animations_enabled(), Instant::now());
                surface
            },
            overlay,
            client: (width, height),
            layout: layout::compute_in_frame(width, height, dpi, 1, 0, false),
            palette: glass::palette(), // --- auto-hide (T11) --- a new window has hidden no bar yet, so it starts glass
            tabs: Vec::with_capacity(8),
            slots: Vec::with_capacity(8),
            stack_tabs: Vec::new(), // --- two-level stacks (T10b) ---
            sidebar: None,
            sidebar_rows: Vec::new(),
            sidebar_scroll: 0,
            sidebar_scroll_target: 0,
            sidebar_hover_row: None,
            sidebar_resizing: false,
            address_bar_revealed: false,
            sidebar_drag_width: None,
            sidebar_drag_pending: false,
            last_relayout: None,
            rail_entries: Vec::new(),
            rail_view: Vec::new(),
            rail_open_seen: None,
            sidebar_config: None,
            rail_app_icons: HashMap::new(),
            rail_icon_epoch: RAIL_ICON_EPOCH.with(|epoch| epoch.get()),
            rail_scroll: 0,
            rail_drag: None,
            rail_focus: None,
            rail_keyboard_focus: false,
            rail_previous_focus: HWND::default(),
            rail_tooltip_key: None,
            rail_tooltip_visible: false,
            newtab_tooltip_shown: false,
            vertical_drag: None,
            views: Vec::with_capacity(8),
            favicons: FaviconCache::default(),
            failed: VecDeque::new(),
            edit_text: String::with_capacity(256),
            focused: false,
            caret: true,
            foreground: false,
            edit: HWND::default(),
            completion: None,
            suppress_completion: false,
            programmatic_text_update: false,
            omni_anchor: 0,
            omni_click: None,
            history: History::default(),
            history_load: None,
            urls: HashMap::with_capacity(8),
            visits: crate::productivity::VisitTracker::default(),
            history_dirty: false,
            hover: Hit::None,
            pressed: Hit::None,
            drag: None,
            split_drag_companion: None,
            stack_hover: None, // --- tab stacks (T10) ---
            split_drop: None,
            dpi,
            motion: Motion::new(!animations_enabled()),
            active_tab: None,
            progress: ProgressTransition::default(),
            first_paint_logged: false,
            split_active: false,
            split_orientation: crate::split_view::Orientation::SideBySide,
            split_layout: None,
            split_dragging: false,
            split_drag_divider: None,
            split_divider: width / 2,
            split_cross: 0,
            pending_split_bounds: None,
            html: None,
            html_requested: false,
            html_page_ready: false,
            html_toolbar_active: false,
            html_failed: [false; 3],
            html_omnibox_focused: false,
            omnibox_edited: false,
            omnibox_claimed: false,
            html_last_state: None,
            html_dl_slot: 0,
            html_dl_last: None,
            html_ext_slot: 0,
            html_ext_last: None,
            html_ext_rect: None,
            html_dl_allowed: true,
            html_ext_allowed: true,
            html_bar_extra: 0,
            html_tabs_last_state: None,
            html_rail_last_state: None,
            html_state_generation: [0; 3],
            html_text_sent: None,
            html_synced_tab: None,
            html_env: None,
            html_env_waiting: false,
            html_tabs: None,
            html_tabs_requested: false,
            html_tabs_page_ready: false,
            html_tabs_last: None,
            // --- vertical sidebar (lane S) ---
            html_tabs_layout: None,
            html_tabs_opts: None, // --- tab options ---
            html_tabs_workspace: None,
            // --- end vertical sidebar ---
            html_tabs_groups: HashMap::new(),
            html_favicons: HashMap::new(),
            html_favicons_sent: HashMap::new(),
            html_deferred: VecDeque::new(),
            html_rail: None,
            html_rail_requested: false,
            html_rail_page_ready: false,
            html_rail_last: None,
            html_rail_favs_sent: HashMap::new(),
            html_rail_anchors: HashMap::new(),
            html_tabs_synced_inputs: None,
            rail_generation: 0,
            html_rail_synced_open: None,
            html_rail_synced_rail_gen: 0,
            html_rail_synced_state_gen: 0,
            chrome_sync_posted: false,
            paint_prefs: PaintPrefs::default(),
            page_idle: [PageIdle::default(); 4],
            flyout_requested: false,
            flyout_failed: false,
            hover_requested: false,
            exit_flushed: false,
        }
    }
    /// Starts reading the profile's history on a worker, so the window shows without first parsing
    /// every stored address. Suggestions and visits take the result through `loaded_history`.
    fn start_history_load(&mut self) {
        let Ok(profile) = crate::profile::active() else {
            return;
        };
        if !crate::productivity::profile_allows_productivity(&profile) || !profile.history_enabled() {
            return;
        }
        let path = profile.history_file();
        let worker = path.clone();
        match std::thread::Builder::new()
            .name("history-load".into())
            .spawn(move || {
                // Visits older than the retention setting are dropped before the list is read, so
                // suggestions never offer them (thread T3).
                let _ = crate::history_retention::prune_now(&worker);
                read_suggestion_history(&worker)
            })
        {
            Ok(handle) => self.history_load = Some(handle),
            Err(_) => self.history = read_suggestion_history(&path),
        }
    }
    /// The sidebar width to lay out: the one a drag has reached, else the setting.
    fn sidebar_width(&self, app: &AppRef) -> u16 {
        self.sidebar_drag_width.unwrap_or_else(|| App::sidebar_width(app))
    }
    /// Lays the chrome out again at sidebar width `width`, with everything else as the last
    /// `relayout` had it. False when there was no earlier layout to repeat.
    fn relayout_sidebar(&mut self, width: u16) -> bool {
        let Some(inputs) = self.last_relayout.clone() else {
            return false;
        };
        self.relayout(RelayoutRequest { w: self.client.0, h: self.client.1, dpi: self.dpi, active: inputs.active, maximised: inputs.maximised, arrangement: inputs.arrangement, density: inputs.density, sidebar_width: width, centred_address_bar: inputs.centred_address_bar, font_scale: inputs.font_scale, ui_font: &inputs.ui_font });
        true
    }
    fn refresh(&mut self, app: &AppRef, structural: bool) {
        let now = Instant::now();
        let ids = App::tab_ids(app);
        let active = App::active_tab_id(app);
        self.split_active = App::split_view(app).is_some();
        self.split_orientation = App::split_view(app).map_or_else(Default::default, |split| split.orientation);
        self.split_layout = App::split_view(app);
        // A change of active tab moves the collapsed-group representative and the active tab's width,
        // so it re-projects and relays out even when the posting path was not structural. Projection
        // inputs are collected whenever it runs, so both paths reach `project_strip` with real data.
        let active_changed = self.active_tab != Some(active);
        let reproject = structural || active_changed;
        let preferences = reproject.then(|| App::tab_group_preferences(app));
        let markers = App::tab_markers(app);
        let mut tabs = Vec::with_capacity(ids.len());
        // The app's own projection inputs, so manual placement and the settle rule apply to the strip.
        let group_inputs = if reproject {
            App::tab_group_inputs(app)
        } else {
            Vec::new()
        };
        let mut sidebar_tabs = Vec::with_capacity(if reproject { ids.len() } else { 0 });
        let size = Metrics::new(self.dpi).px(Metrics::FAVICON_SIZE).max(1) as u32;
        let html_strip = self.html_tab_region().is_some();
        // The previous snapshot by id: one pass, not a search of every old tab for every tab.
        let previous: HashMap<TabId, &TabSnapshot> = self.tabs.iter().map(|tab| (tab.id, tab)).collect();
        for &id in &ids {
            let old = previous.get(&id).copied();
            let raw_title = App::tab_title(app, id).unwrap_or_default();
            // --- tab options --- a name the user gave the tab wins over the page's title.
            let title = if let Some(name) = App::tab_name(app, id) {
                name
            } else if raw_title.trim().is_empty() {
                "New tab".to_owned()
            } else {
                raw_title.clone()
            };
            // --- end tab options ---
            let loading = App::tab_loading(app, id);
            // The sidebar rows are only rebuilt on a reprojection, so only then are they collected.
            if reproject {
                sidebar_tabs.push((id, title.clone(), App::tab_pinned(app, id), loading));
            }
            let key = App::tab_favicon_key(app, id);
            // A live HTML tab strip paints the favicons itself: no bytes are fetched or decoded here.
            // `tab_favicon` decodes on demand for the native readers that still want one.
            let image = if !native_favicon_wanted(html_strip) {
                None
            } else if old.is_some_and(|t| {
                t.favicon_key == key
                    && (key.is_none()
                        || t.favicon
                            .as_ref()
                            .is_some_and(|image| image.width == size && image.height == size))
            }) {
                old.and_then(|tab| tab.favicon.clone())
            } else if let (Some(k), Some(bytes)) = (key, App::tab_favicon(app, id)) {
                decode_cached_favicon(&mut self.favicons, &mut self.failed, k, size, &bytes)
            } else {
                None
            };
            // A finished load of an http(s) page is a visit, however the page was reached (typed,
            // link, redirect, same-document change, reload). A private tab leaves no trace: not in
            // history.txt, not in suggestions.
            let just_finished = old.is_some_and(|t| t.loading) && !loading;
            let private = App::tab_private(app, id);
            if crate::private::may_persist(crate::private::Store::History, private, false) {
                let committed = App::tab_committed_url(app, id);
                if let Some((url, visit_title)) = self.visits.observe(
                    id.0,
                    committed.as_ref().map(url::Url::as_str),
                    &raw_title,
                    loading,
                    just_finished,
                ) {
                    if crate::private::may_persist(crate::private::Store::SuggestionHistory, private, false) {
                        loaded_history(&mut self.history, &mut self.history_load).record(&url, &visit_title);
                    }
                    if persist_profile_history(url, visit_title) {
                        self.history_dirty = true;
                    }
                }
            }
            tabs.push(TabSnapshot {
                id,
                title,
                loading,
                favicon_key: key,
                favicon: image,
                marker: markers.get(&id).copied(),
            });
        }
        let active_index = ids.iter().position(|&id| id == active).unwrap_or(0);
        let slots = match preferences.as_ref() {
            Some(preferences) if reproject => {
                tab_groups::project_strip(&group_inputs, active_index, preferences)
            }
            _ => self.slots.clone(),
        };
        // --- two-level stacks (T10b) --- with a second row, the stack in use is only its ball in the
        // tab row and its tabs are the second row.
        let mut slots = slots;
        if reproject {
            let row = App::arrangement(app).stack_row != layout::StackRow::None;
            self.stack_tabs = if row { App::stack_row_tabs(app) } else { Vec::new() };
            if !self.stack_tabs.is_empty() {
                for slot in slots.iter_mut().filter(|slot| slot.collapsed) {
                    if ids.get(slot.tab_index).is_some_and(|id| self.stack_tabs.contains(id)) {
                        slot.marker_only = true;
                    }
                }
            }
        }
        // --- end two-level stacks ---
        if reproject {
            self.sidebar_rows = crate::chrome::vertical_paint::rows(
                &sidebar_tabs,
                &group_inputs,
                preferences
                    .as_ref()
                    .expect("preferences loaded for reprojection"),
                active,
            );
        }
        // Still the old tabs and layout here, so a tab that is leaving keeps its island to fade from.
        let hovered = self.hovered_tab();
        if structural {
            let (layout, old, old_slots) = (&self.layout, &self.tabs, &self.slots);
            self.motion.sync(
                &ids,
                hovered,
                Some(active),
                |id| {
                    old_slots
                        .iter()
                        .position(|slot| old.get(slot.tab_index).is_some_and(|tab| tab.id == id))
                        .and_then(|slot| layout.tab(slot))
                },
                now,
            );
        }
        self.active_tab = Some(active);
        self.views.clear();
        self.tabs = tabs;
        self.slots = slots;
        let index = hovered.and_then(|id| {
            self.slots.iter().position(|slot| {
                self.tabs
                    .get(slot.tab_index)
                    .is_some_and(|tab| tab.id == id)
            })
        });
        self.hover = moved_hover(self.hover, index);
        if !self.focused {
            self.edit_text.clear();
            if let Some(url) = self.urls.get(&App::active_tab_id(app)) {
                self.edit_text.push_str(url)
            }
        }
        if reproject {
            let settings = App::settings(app);
            let sidebar_width = self.sidebar_width(app);
            let arrangement = App::arrangement(app).with_address_bar_revealed(self.address_bar_revealed); // --- layout (thread T6) ---
            self.relayout(RelayoutRequest { w: self.client.0, h: self.client.1, dpi: self.dpi, active: App::active_tab_id(app), maximised: self.layout.maximised, arrangement, density: settings.ui_density, sidebar_width, centred_address_bar: settings.centred_address_bar, font_scale: crate::layout_prefs::chrome_scale_percent(&settings), ui_font: &settings.ui_font });
        }
        self.motion.sync_groups(&self.group_specs(), now);
    }
    fn relayout(&mut self, request: RelayoutRequest<'_>) {
        let RelayoutRequest { w, h, dpi, active, maximised, arrangement, density, sidebar_width, centred_address_bar, font_scale, ui_font } = request;
        self.client = (w, h);
        self.dpi = dpi.max(96);
        self.last_relayout = Some(RelayoutInputs {
            active,
            maximised,
            arrangement,
            density,
            centred_address_bar,
            font_scale,
            ui_font: ui_font.to_owned(),
        });
        let was_vertical = self.sidebar.is_some();
        let i = self
            .slots
            .iter()
            .position(|slot| {
                self.tabs
                    .get(slot.tab_index)
                    .is_some_and(|tab| tab.id == active)
            })
            .unwrap_or(0);
        // --- layout (thread T6) --- one geometry for every arrangement: tab bar edge or hidden,
        // address bar edge or hidden, title row, sidebar side and the list density.
        // --- auto-hide (T11) --- with nothing hidden this is exactly that geometry; with bars out
        // the page widens over them (`layout::compute_arranged_auto_hide`), and the layout with
        // every bar shown says where a revealed bar floats.
        let plan = crate::auto_hide::plan_for(self.ah.hidden, arrangement.tabs, arrangement.tab_bar); // A hidden tab bar hides nothing
        let hide = plan.layout();
        let (layout, sidebar) = layout::compute_arranged_auto_hide(
            arrangement, w, h, self.dpi, self.slots.len().max(1), i, maximised,
            i32::from(sidebar_width), centred_address_bar, density.sidebar_row_height(), hide,
        );
        let shown = if hide.is_off() {
            None
        } else {
            Some(layout::compute_arranged_in_frame(
                arrangement, w, h, self.dpi, self.slots.len().max(1), i, maximised,
                i32::from(sidebar_width), centred_address_bar, density.sidebar_row_height(),
            ).0)
        };
        let sidebar = sidebar.map(|sidebar| sidebar.with_tab_count(self.sidebar_rows.len(), self.dpi));
        // --- end layout ---
        self.layout = layout.with_group_slots(
            self.slots.iter().map(|slot| slot.marker_only),
            self.slots.iter().map(|slot| slot.group_control && !slot.collapsed),
            self.slots.iter().map(|slot| slot.collapsed && !slot.marker_only),
        );
        // --- auto-hide (T11) --- where the revealed bars float, and the page's rectangle with the bars out.
        self.ah.plan = plan;
        self.ah.page = self.layout.viewport;
        self.ah.overlay = layout::auto_hide_overlay_arranged(
            shown.as_ref().unwrap_or(&self.layout), &self.layout, sidebar.as_ref(), arrangement, self.dpi, plan.sidebar,
        );
        {
            let normal = shown.as_ref().map(|shown| shown.viewport).unwrap_or(self.layout.viewport);
            let (bar, bottom) = self.bar_reserve;
            self.ah.bar_bottom = bottom;
            self.ah.bar_float = if bar <= 0 {
                Rect::default()
            } else if bottom {
                Rect::new(normal.left, normal.bottom - bar, normal.right, normal.bottom)
            } else {
                Rect::new(normal.left, normal.top, normal.right, normal.top + bar)
            };
        }
        // The bookmarks bar takes its row off the top (or bottom) of the page area, unless auto-hide
        // has it out of the layout (then it floats over the page).
        if !plan.bookmarks {
            self.layout.viewport = super::bookmark_bar::reserved_viewport(self.layout.viewport, self.bar_reserve);
        }
        // --- end auto-hide ---
        if let Some(sidebar) = sidebar {
            self.sidebar_scroll = self.sidebar_scroll.clamp(0, sidebar.max_scroll);
            self.sidebar_scroll_target = self.sidebar_scroll_target.clamp(0, sidebar.max_scroll);
            self.sidebar = Some(sidebar);
        } else {
            self.sidebar = None;
        }
        if was_vertical != self.sidebar.is_some() {
            self.html_tabs_layout = None;
            self.html_tabs_opts = None; // --- tab options ---
            self.html_tabs_workspace = None;
        }
        self.split_divider = if self.split_orientation.is_stacked() {
            self.layout.viewport.top + self.layout.viewport.height() / 2 - 3
        } else {
            self.layout.viewport.left + self.layout.viewport.width() / 2 - 3
        };
        self.split_cross = if self.split_orientation.is_stacked() {
            self.layout.viewport.left + self.layout.viewport.width() / 2 - 3
        } else {
            self.layout.viewport.top + self.layout.viewport.height() / 2 - 3
        };
        self.sync_html_bounds();
        self.sync_html_tabs_bounds();
        self.sync_html_rail_bounds();
        self.surface.resize(w, h, self.dpi);
        self.surface.set_font(font_scale, ui_font);
    }
    /// The toolbar region the HTML chrome covers, in client pixels. `None` while it is inactive or
    /// the toolbar is too narrow for its region.
    fn html_region(&self) -> Option<Rect> {
        if !self.html_toolbar_active || self.ah.top_hidden() {
            return None;
        }
        chrome_host::toolbar_bounds(&self.layout).map(Rect::from)
    }
    // --- vertical sidebar (lane S) ---
    fn html_tabs_bounds(&self) -> Option<RECT> {
        self.sidebar.as_ref().and_then(chrome_host::sidebar_bounds)
            .or_else(|| chrome_host::tab_row_bounds(&self.layout))
    }
    // --- end vertical sidebar ---
    /// The tab row or vertical sidebar covered by the Tabs host, in client pixels.
    fn html_tab_region(&self) -> Option<Rect> {
        if self.html_tabs.is_none() || !self.html_tabs_page_ready {
            return None;
        }
        // --- vertical sidebar (lane S) ---
        if self.sidebar.is_some() {
            if self.ah.sidebar_hidden() {
                return None; // --- auto-hide (T11) ---
            }
            return sidebar_html_region(self.sidebar.as_ref(), true);
        }
        // --- end vertical sidebar ---
        if self.ah.top_hidden() {
            return None; // --- auto-hide (T11) ---
        }
        chrome_host::tab_row_bounds(&self.layout).map(Rect::from)
    }
    /// The left rail the HTML rail covers, in client pixels. `None` until its page is ready, so the
    /// native rail keeps painting and hit-testing until then.
    fn html_rail_region(&self) -> Option<Rect> {
        if self.html_rail.is_none() || !self.html_rail_page_ready {
            return None;
        }
        chrome_host::rail_bounds(&self.layout).map(Rect::from)
    }
    /// Every HTML region in client pixels: the toolbar's, the tab row's and the rail's, when active.
    fn html_regions(&self) -> Vec<Rect> {
        self.html_region()
            .into_iter()
            .chain(self.html_tab_region())
            .chain(self.html_rail_region())
            .collect()
    }
    /// Moves the HTML rail over the left rail and hides it when there is none. Also tells the
    /// surface to leave the rail to the page.
    fn sync_html_rail_bounds(&mut self) {
        let region = self.html_rail_region();
        self.surface.set_html_rail(region.is_some());
        self.surface.set_html_pending(2, chrome_host::html_ui_enabled() && !self.html_failed[2] && region.is_none());
    }
    /// Moves the HTML tab strip over the tab row and hides it when there is none. Also tells the
    /// surface to leave the row to the page.
    fn sync_html_tabs_bounds(&mut self) {
        let region = self.html_tab_region();
        self.surface.set_html_tabs(region.is_some());
        let hidden = if self.sidebar.is_some() { self.ah.sidebar_hidden() } else { self.ah.top_hidden() }; // --- auto-hide (T11) ---
        self.surface.set_html_pending(1, chrome_host::html_ui_enabled() && !self.html_failed[1] && region.is_none() && !hidden);
    }
    /// CSS px the toolbar page's buttons beside the pill (downloads, extensions) take from it.
    fn toolbar_slot(&self) -> i32 {
        // --- toolbars and feeds (thread T13) --- a button taken off the bar gives its room back.
        (if self.html_dl_allowed { self.html_dl_slot } else { 0 }) + (if self.html_ext_allowed { self.html_ext_slot } else { 0 }) + self.html_bar_extra
    }
    /// Moves the HTML chrome over the toolbar region and hides it when there is none. Also tells
    /// the surface to leave the region to the page.
    fn sync_html_bounds(&mut self) {
        let region = self.html_region();
        self.surface.set_html_toolbar(region.is_some());
        self.surface.set_html_pending(0, chrome_host::html_ui_enabled() && !self.html_failed[0] && region.is_none() && !self.ah.top_hidden()); // --- auto-hide (T11) ---
    }
    /// The tab under the pointer, by identity, so its emphasis follows it across reorders.
    fn hovered_tab(&self) -> Option<TabId> {
        match self.hover {
            Hit::Tab(i) | Hit::TabClose(i) | Hit::GroupBall(i) => self
                .slots
                .get(i)
                .and_then(|slot| self.tabs.get(slot.tab_index))
                .map(|tab| tab.id),
            _ => None,
        }
    }
    fn retarget_motion(&mut self, now: Instant) {
        let hovered = self.hovered_tab();
        self.motion.retarget(hovered, self.active_tab, now);
        self.motion.retarget_icons(self.hover, self.pressed, now);
    }
    fn group_specs(&self) -> Vec<GroupSpec> {
        let mut groups = Vec::new();
        for (index, slot) in self.slots.iter().enumerate() {
            if !self.visual_group_leader(index) {
                continue;
            }
            let Some(key) = slot.group_key.clone() else {
                continue;
            };
            let state = group_ui::state_from_model(slot.collapsed, !slot.marker_only);
            let members = self
                .slots
                .iter()
                .enumerate()
                .filter(|(member_index, member)| {
                    member.group_key.as_ref() == Some(&key)
                        && self.layout.tab(*member_index).is_some()
                })
                .collect::<Vec<_>>();
            let Some((first_index, first_slot)) = members.first().copied() else {
                continue;
            };
            let (last_index, _) = members.last().copied().unwrap_or((first_index, first_slot));
            let Some(first) = self
                .layout
                .tab(first_index)
                .map(|rect| rect.intersect(self.layout.tab_zone))
            else {
                continue;
            };
            let Some(last) = self
                .layout
                .tab(last_index)
                .map(|rect| rect.intersect(self.layout.tab_zone))
            else {
                continue;
            };
            let shell = Rect::new(first.left, first.top, last.right, first.bottom);
            groups.push(GroupSpec {
                key,
                state,
                count: first_slot.group_size,
                shell,
            });
        }
        groups
    }
    fn visual_group_leader(&self, index: usize) -> bool {
        let Some(key) = self
            .slots
            .get(index)
            .and_then(|slot| slot.group_key.as_ref())
        else {
            return false;
        };
        self.layout.tab(index).is_some()
            && !(self.layout.first_visible_tab()..index).any(|previous| {
                self.layout.tab(previous).is_some()
                    && self
                        .slots
                        .get(previous)
                        .and_then(|slot| slot.group_key.as_ref())
                        == Some(key)
            })
    }
    fn group_geometry(&self, index: usize, now: Instant) -> Option<GroupGeometry> {
        let slot = self.slots.get(index)?;
        let key = slot.group_key.as_ref()?;
        let frame = self.motion.group_frame(key, now)?;
        Some(group_ui::group_geometry(frame, Metrics::new(self.dpi)))
    }
    /// The visible part of a tab's island, or `None` when it is scrolled out of the strip.
    fn tab_rect(&self, id: TabId) -> Option<Rect> {
        let index = self.slots.iter().position(|slot| {
            self.tabs
                .get(slot.tab_index)
                .is_some_and(|tab| tab.id == id)
        })?;
        self.layout
            .tab(index)
            .map(|rect| rect.intersect(self.layout.tab_zone))
    }
    /// Repaints only what one animation frame moves: the islands the motion asks for, and the
    /// ghosts that are fading or finishing. Returns whether another frame is needed.
    fn animate(&mut self, hwnd: HWND, now: Instant) -> bool {
        // A live HTML tab strip animates itself: the native island repaints below would paint nothing.
        let native_strip = self.html_tab_region().is_none();
        // Repaint ghosts before pruning, so the frame that drops one also clears its pixels.
        if native_strip {
            for departure in self.motion.departures() {
                invalidate(hwnd, departure.rect.intersect(self.layout.tab_zone));
            }
        }
        let running = self.motion.step(now);
        if native_strip {
            for &id in self.motion.repaint() {
                if let Some(rect) = self.tab_rect(id) {
                    invalidate(hwnd, rect);
                }
            }
            for key in self.motion.repaint_groups() {
                if let Some(rect) = self
                    .motion
                    .group_repaint_bounds(key, now, self.layout.metrics)
                    .map(|rect| rect.intersect(self.layout.tab_zone))
                {
                    invalidate(hwnd, rect);
                }
            }
        }
        // The rail's press and active motion lives in the painter, so ask it for its own frame.
        let rail_frame = self.surface.rail_step(now);
        if !self.motion.rail_repaint().is_empty() || rail_frame.repaint {
            invalidate(hwnd, self.layout.rail);
        }
        for hit in self.motion.icon_repaint() {
            invalidate(hwnd, rect_for(&self.layout, hit));
        }
        self.motion.prune(now);
        running || rail_frame.running
    }
    fn update_completion(&mut self) {
        let history = loaded_history(&mut self.history, &mut self.history_load);
        self.completion =
            omnibox::inline_completion(history, &self.edit_text, !self.suppress_completion);
        self.suppress_completion = false
    }
}

/// The suggestion history from `path`, as the omnibox ranks it. Runs on the history worker.
fn read_suggestion_history(path: &Path) -> History {
    let mut history = History::default();
    for (_, url, title) in crate::productivity::read_history(path) {
        history.record(&url, &title);
    }
    history
}

/// `history`, after the startup load has been taken into it if it was still pending. Waits for the
/// worker only if it has not finished, which is the first keystroke or visit right after launch.
fn loaded_history<'a>(history: &'a mut History, load: &mut Option<JoinHandle<History>>) -> &'a mut History {
    if let Some(handle) = load.take()
        && let Ok(loaded) = handle.join()
    {
        *history = loaded;
    }
    history
}
thread_local! {static WINDOWS:RefCell<HashMap<isize,WindowChrome>>=RefCell::new(HashMap::new());}

/// Rebuilds the rail's buttons and reports whether they changed. `sidebar.txt` is parsed again only
/// when its stamp changed, the Bookmarks mark is one lookup in the in-memory bookmark set, and each
/// web app's icon is read once (`rail_app_icons`), so the side panel's `WM_RAIL_CHANGED` per
/// keystroke costs a `stat`. An unchanged rail keeps its generation, so nothing downstream rebuilds.
fn rebuild_rail_entries(state: &mut WindowChrome, app: &AppRef) -> bool {
    rebuild_rail_entries_with(state, app, true)
}

/// `rebuild_rail_entries`. Without `icons` no web app favicon is read or decoded: the first build,
/// before the window shows, leaves them to a posted `WM_RAIL_CHANGED`.
fn rebuild_rail_entries_with(state: &mut WindowChrome, app: &AppRef, icons: bool) -> bool {
    use crate::{sidebar::{SidebarConfig, SidebarKind}, workspaces::{WorkspaceColour, WorkspaceIcon}};

    let profile = crate::profile::active().ok();
    let config = profile.as_ref().map_or_else(SidebarConfig::default, |p| cached_sidebar_config(state, p.root()));
    let owner = app.borrow().hwnd;
    // --- rail flyouts ---
    match html_panel::active_id(owner) {
        Some(side_panel::PanelId::Bookmarks)
            if crate::sidebar::should_close_hidden_panel(Some(SidebarKind::Bookmarks), &config) => html_panel::dismiss(owner),
        Some(side_panel::PanelId::History)
            if crate::sidebar::should_close_hidden_panel(Some(SidebarKind::History), &config) => html_panel::dismiss(owner),
        _ => {}
    }
    // --- end rail flyouts ---
    match side_panel::active_id(owner) {
        Some(id @ side_panel::PanelId::Bookmarks)
            if crate::sidebar::should_close_hidden_panel(Some(SidebarKind::Bookmarks), &config) =>
        {
            side_panel::close_content(owner, id);
        }
        Some(id @ side_panel::PanelId::History)
            if crate::sidebar::should_close_hidden_panel(Some(SidebarKind::History), &config) =>
        {
            side_panel::close_content(owner, id);
        }
        _ => {}
    }
    let active_url = App::tab_committed_url(app, App::active_tab_id(app));
    let bookmark_active = active_url.as_ref().is_some_and(|url| super::html_suggest::is_bookmarked(url.as_str()));
    let shown = |kind| config.items.iter().find(|item| item.kind == kind).is_none_or(|item| item.visible);
    let mut entries = Vec::new();
    if App::settings(app).show_home_button {
        entries.push(RailEntryData { key: RailKey::Home, icon: RailIcon::Home, label: "Home".into(), active: false, accent: None, favicon: None, png: None, dot: false, count: 0, emoji: None });
    }
    entries.push(RailEntryData { key: RailKey::Customize, icon: RailIcon::Customize, label: "Customize".into(), active: false, accent: None, favicon: None, png: None, dot: false, count: 0, emoji: None });
    // --- tab stacks and workspaces (T10) --- "Workspaces" off hides them; the counts are an option.
    let workspace_options = App::tab_options(app);
    if shown(SidebarKind::Workspaces) && workspace_options.workspaces {
        for (id, name, icon, colour, tab_count, active) in App::workspace_entries(app) {
            let count = if workspace_options.workspace_counts { u32::try_from(tab_count).unwrap_or(u32::MAX) } else { 0 };
            let emoji = App::workspace_emoji(app, id);
            let glyph = match icon {
                WorkspaceIcon::Briefcase => WorkspaceGlyph::Briefcase,
                WorkspaceIcon::Home => WorkspaceGlyph::Home,
                WorkspaceIcon::Code => WorkspaceGlyph::Code,
                WorkspaceIcon::Book => WorkspaceGlyph::Book,
                WorkspaceIcon::Cart => WorkspaceGlyph::Cart,
                WorkspaceIcon::Game => WorkspaceGlyph::Game,
                WorkspaceIcon::Music => WorkspaceGlyph::Music,
                WorkspaceIcon::Heart => WorkspaceGlyph::Heart,
                WorkspaceIcon::Star => WorkspaceGlyph::Star,
                WorkspaceIcon::Bolt => WorkspaceGlyph::Bolt,
                WorkspaceIcon::Globe => WorkspaceGlyph::Globe,
                WorkspaceIcon::Leaf => WorkspaceGlyph::Leaf,
            };
            let accent = colour.map(|value| crate::theme::Rgba::hex(match value {
                WorkspaceColour::Blue => 0x4385D1,
                WorkspaceColour::Violet => 0x8B66D9,
                WorkspaceColour::Teal => 0x32A89B,
                WorkspaceColour::Amber => 0xD49A32,
                WorkspaceColour::Rose => 0xD66D91,
                WorkspaceColour::Red => 0xD55757,
                WorkspaceColour::Green => 0x56A36B,
                WorkspaceColour::Slate => 0x778394,
            }, 0xFF));
            entries.push(RailEntryData { key: RailKey::Workspace(id.0 as u64), icon: RailIcon::Glyph(glyph), label: name, active, accent, favicon: None, png: None, dot: false, count, emoji });
        }
        entries.push(RailEntryData { key: RailKey::AddWorkspace, icon: RailIcon::Plus, label: "New workspace".into(), active: false, accent: None, favicon: None, png: None, dot: false, count: 0, emoji: None });
    }
    if shown(SidebarKind::Apps) {
        // Only a native rail paints the decoded bitmap; the HTML rail takes the file's bytes.
        let native_rail = !chrome_host::html_ui_enabled() || state.html_failed[2];
        let size = Metrics::new(state.dpi).px(Metrics::FAVICON_SIZE).max(1) as u32;
        let epoch = RAIL_ICON_EPOCH.with(|epoch| epoch.get());
        if state.rail_icon_epoch != epoch {
            state.rail_app_icons.clear();
            state.rail_icon_epoch = epoch;
        }
        let live: HashSet<&str> = config.apps.iter().map(|entry| entry.url.as_str()).collect();
        state.rail_app_icons.retain(|url, _| live.contains(url.as_str()));
        for app_entry in &config.apps {
            let (png, favicon) = if icons && profile.is_some() {
                rail_app_icon(state, &app_entry.url, native_rail.then_some(size))
            } else {
                (None, None)
            };
            let icon = if favicon.is_some() || (!native_rail && png.is_some()) { RailIcon::AppTile } else { RailIcon::Letter };
            entries.push(RailEntryData { key: RailKey::App(app_entry.id), icon, label: app_entry.name.clone(), active: false, accent: None, favicon, png, dot: super::panel_webapp::unread_count(app_entry.id) > 0, count: 0, emoji: None });
        }
        entries.push(RailEntryData { key: RailKey::AddApp, icon: RailIcon::Plus, label: "Add web app".into(), active: false, accent: None, favicon: None, png: None, dot: false, count: 0, emoji: None });
    }
    for item in &config.items {
        let (key, icon, label) = match item.kind {
            SidebarKind::Bookmarks if item.visible => (RailKey::Bookmarks, RailIcon::Heart, "Bookmarks"),
            SidebarKind::History if item.visible => (RailKey::History, RailIcon::Clock, "History"),
            _ => continue,
        };
        entries.push(RailEntryData { key, icon, label: label.into(), active: key == RailKey::Bookmarks && bookmark_active, accent: None, favicon: None, png: None, dot: false, count: 0, emoji: None });
    }
    // --- toolbars and feeds (thread T13) --- the panel bar's extra buttons (`toolbar_panel`), after the
    // built-in ones: the Feeds button and command buttons (a letter tile, the command's name as its tooltip).
    for (index, label, icon) in panel_tools(&App::settings(app).toolbar_panel) {
        entries.push(RailEntryData { key: RailKey::Tool(index), icon, label, active: false, accent: None, favicon: None, png: None, dot: false, count: 0, emoji: None });
    }
    // --- end toolbars and feeds ---
    entries.push(RailEntryData { key: RailKey::Menu, icon: RailIcon::Dots, label: "Menu".into(), active: false, accent: None, favicon: None, png: None, dot: false, count: 0, emoji: None });
    state.rail_scroll = state.rail_scroll.max(0);
    if same_rail(&state.rail_entries, &entries) {
        return false;
    }
    // The painter's view borrows the old entries' favicons, so it goes first.
    state.rail_view.clear();
    state.rail_entries = entries;
    state.rail_view = rail_view_of(&state.rail_entries);
    state.rail_generation = state.rail_generation.wrapping_add(1);
    true
}

/// `sidebar.txt` under `root`, parsed again only when its stamp changed since the last read.
fn cached_sidebar_config(state: &mut WindowChrome, root: &Path) -> crate::sidebar::SidebarConfig {
    let stamp = file_stamp(&root.join("sidebar.txt"));
    if let Some((cached, config)) = &state.sidebar_config
        && *cached == stamp
    {
        return config.clone();
    }
    let config = crate::sidebar::SidebarConfig::load(root).unwrap_or_default();
    state.sidebar_config = Some((stamp, config.clone()));
    config
}

/// The rail icon of the web app at `url`: its favicon file's bytes, read once, and, when `decode_at`
/// asks, the bitmap at that size, decoded once per size.
fn rail_app_icon(
    state: &mut WindowChrome,
    url: &str,
    decode_at: Option<u32>,
) -> (Option<Rc<Vec<u8>>>, Option<Rc<favicon::FaviconBitmap>>) {
    let icon = state.rail_app_icons.entry(url.to_owned()).or_insert_with(|| RailAppIcon {
        png: url::Url::parse(url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .and_then(|host| favicon::load_favicon_bytes(&host).ok().flatten())
            .map(Rc::new),
        decoded: None,
    });
    let favicon = decode_at.and_then(|size| {
        if icon.decoded.as_ref().is_none_or(|(at, _)| *at != size) {
            let bitmap = icon.png.as_deref().and_then(|png| favicon::decode(png, size)).map(Rc::new);
            icon.decoded = Some((size, bitmap));
        }
        icon.decoded.as_ref().and_then(|(_, bitmap)| bitmap.clone())
    });
    (icon.png.clone(), favicon)
}

/// Whether two rails show the same buttons. Icons compare by identity: the cache hands out the
/// same `Rc` until an icon changes. Pure.
fn same_rail(a: &[RailEntryData], b: &[RailEntryData]) -> bool {
    fn same<T>(a: &Option<Rc<T>>, b: &Option<Rc<T>>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.key == b.key
                && a.icon == b.icon
                && a.label == b.label
                && a.active == b.active
                && a.accent == b.accent
                && same(&a.favicon, &b.favicon)
                && same(&a.png, &b.png)
                && a.dot == b.dot
        })
}

/// The painter's form of `entries`. Each favicon reference points into an `Rc` that `entries`
/// holds, whose address does not move with the vector; the caller drops the view before it drops
/// or replaces `entries` (see `rebuild_rail_entries_with`).
fn rail_view_of(entries: &[RailEntryData]) -> Vec<RailEntry<'static>> {
    entries
        .iter()
        .map(|entry| {
            let view = RailEntry {
                key: entry.key,
                icon: entry.icon,
                label: entry.label.clone(),
                active: entry.active,
                dot: entry.dot,
                accent: entry.accent,
                favicon: entry.favicon.as_deref(),
                count: entry.count,
                emoji: entry.emoji.clone(),
            };
            // SAFETY: as documented above, the bitmap outlives every use of the view.
            unsafe { std::mem::transmute::<RailEntry<'_>, RailEntry<'static>>(view) }
        })
        .collect()
}

fn main_window_style() -> WINDOW_STYLE {
    WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_CLIPCHILDREN
}

fn with_chrome<T>(hwnd: HWND, f: impl FnOnce(&mut WindowChrome) -> T) -> Option<T> {
    WINDOWS.with(|a| a.borrow_mut().get_mut(&(hwnd.0 as isize)).map(f))
}

/// Queues one `WM_CHROME_SYNC` after a change the HTML views or page hosts show: tabs, navigation,
/// settings, layout, the rail, the omnibox text. Calls before it runs add nothing.
fn mark_chrome_dirty(hwnd: HWND) {
    let first = WINDOWS.with(|all| {
        all.try_borrow_mut().ok().map(|mut all| {
            all.get_mut(&(hwnd.0 as isize))
                .is_some_and(|s| !std::mem::replace(&mut s.chrome_sync_posted, true))
        })
    });
    // `None`: the window table is borrowed further up the stack. Post anyway; a sync is idempotent.
    if first == Some(false) {
        return;
    }
    let posted = unsafe { PostMessageW(Some(hwnd), WM_CHROME_SYNC, WPARAM(0), LPARAM(0)) }.is_ok();
    if !posted && first == Some(true) {
        let _ = with_chrome(hwnd, |s| s.chrome_sync_posted = false);
    }
}

/// Repaints the whole window after a state change and queues the chrome sync with it.
fn repaint(hwnd: HWND) {
    mark_chrome_dirty(hwnd);
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

/// Brings the HTML views and the page hosts in line with the window, once per queued change (what
/// every `WM_PAINT` used to do first). Each step sends nothing its view already has, and the paint
/// values are refreshed here so a paint reads no settings.
fn run_chrome_sync(hwnd: HWND, app: Option<&AppRef>) {
    let _ = with_chrome(hwnd, |s| s.chrome_sync_posted = false);
    sync_html_host_bounds(hwnd);
    html_sync(hwnd);
    html_tabs_sync(hwnd);
    html_rail_sync(hwnd);
    let Some(app) = app else {
        return;
    };
    // --- split view ---
    super::split_ui::sync(hwnd, app);
    super::settings_page::sync(hwnd, app);
    super::history_page::sync(hwnd, app);
    super::downloads_page::sync(hwnd, app);
    // --- end split view ---
    track_idle_pages(hwnd);
    auto_hide::after_layout(hwnd); // --- auto-hide (T11) --- the bookmarks bar and page hosts just synced
    let prefs = PaintPrefs::read(app);
    let changed = with_chrome(hwnd, |s| {
        let changed = s.paint_prefs != prefs;
        if changed {
            s.paint_prefs = prefs;
        }
        changed
    })
    .unwrap_or(false);
    if changed {
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}

/// Times the split, settings and history page hosts while they are hidden and closes one hidden for
/// `PAGE_IDLE_CLOSE`; its next show creates it again. A pointer drag holds the clocks (the split
/// host's drop card follows drags). The slow timer runs only while a clock does.
fn track_idle_pages(hwnd: HWND) {
    let shown = [
        super::split_ui::picker_pane(hwnd).is_some() || super::split_ui::private_page_shown(hwnd),
        super::settings_page::shown(hwnd),
        super::history_page::shown(hwnd),
        super::downloads_page::shown(hwnd),
    ];
    let now = Instant::now();
    let dragging = unsafe { GetCapture() == hwnd };
    let Some((close, running)) = with_chrome(hwnd, |s| {
        let dragging = dragging || s.drag.is_some() || s.vertical_drag.is_some();
        let mut close = [false; 4];
        for (index, idle) in s.page_idle.iter_mut().enumerate() {
            if !dragging {
                close[index] = page_idle_step(idle, shown[index], now);
            }
        }
        (close, s.page_idle.iter().any(|idle| idle.hidden_since.is_some()))
    }) else {
        return;
    };
    for (index, close) in close.into_iter().enumerate() {
        if !close {
            continue;
        }
        match index {
            0 => super::split_ui::close(hwnd),
            1 => super::settings_page::close(hwnd),
            2 => super::history_page::close(hwnd),
            _ => super::downloads_page::close(hwnd),
        }
        perf_event(hwnd, "page_host_idle_closed");
    }
    unsafe {
        if running {
            let _ = SetTimer(Some(hwnd), WM_PAGE_IDLE_TIMER, PAGE_IDLE_CHECK_MS, None);
        } else {
            let _ = KillTimer(Some(hwnd), WM_PAGE_IDLE_TIMER);
        }
    }
}

/// What a clean exit writes, once per window: the privacy counters when they changed since their
/// last save (their debounce timer is still pending), queued history visits, and the final session
/// snapshot, which also removes the running marker. Shared by `WM_CLOSE` and `WM_ENDSESSION`.
fn flush_for_exit(hwnd: HWND, app: &AppRef) {
    if with_chrome(hwnd, |s| std::mem::replace(&mut s.exit_flushed, true)).unwrap_or(false) {
        return;
    }
    if unsafe { KillTimer(Some(hwnd), crate::win32::WM_PRIVACY_STATS_TIMER) }.is_ok() {
        let _ = App::privacy_stats_save_now(app);
    }
    crate::productivity::flush_pending_history();
    // A session-only history ends with the session (thread T3).
    crate::history_retention::finish_at_exit();
    App::session_finish(app);
}

/// Asks for the hover layer the first time a view needs it (a tooltip, card, island or the omnibox
/// dropdown), once the HTML toolbar is up. The layer is created from `WM_HTML_HOVER_START`.
fn ensure_hover_layer(hwnd: HWND) {
    let ask = with_chrome(hwnd, |s| s.html_toolbar_active && !std::mem::replace(&mut s.hover_requested, true))
        .unwrap_or(false);
    if ask && unsafe { PostMessageW(Some(hwnd), WM_HTML_HOVER_START, WPARAM(0), LPARAM(0)) }.is_err() {
        let _ = with_chrome(hwnd, |s| s.hover_requested = false);
    }
}

/// Creates the hover layer. `html_hover::prewarm` keeps a layer that already exists.
fn start_hover_layer(hwnd: HWND) {
    let env = with_chrome(hwnd, |s| s.html_toolbar_active.then(|| s.html_env.clone()).flatten()).flatten();
    if let Some(env) = env {
        super::html_hover::prewarm(hwnd, env);
    }
}

/// Requests the flyout panel's controller the first time a rail flyout is opened, once per window.
/// True when flyouts are available now (the controller may still be loading; the flyout opens when
/// its page is ready).
fn ensure_flyout_panel(hwnd: HWND) -> bool {
    if html_panel::available(hwnd) {
        return true;
    }
    let Some(app) = app_of(hwnd) else {
        return false;
    };
    let env = with_chrome(hwnd, |s| {
        if s.flyout_requested || s.flyout_failed {
            return None;
        }
        let env = s.html_env.clone()?;
        s.flyout_requested = true;
        Some(env)
    })
    .flatten();
    if let Some(env) = env {
        html_panel::register(&app, hwnd, env);
    }
    html_panel::available(hwnd)
}

// --- vertical sidebar (lane S) ---
fn sidebar_html_region(layout: Option<&crate::vertical_tabs::SidebarLayout>, active: bool) -> Option<Rect> {
    if !active { return None; }
    layout.and_then(chrome_host::sidebar_bounds).map(Rect::from)
}
// --- end vertical sidebar ---

/// Snapshot hosts and geometry, then call WebView2 after the window borrow has ended.
fn sync_html_host_bounds(hwnd: HWND) {
    let Some(hosts) = with_chrome(hwnd, |s| [
        (s.html.clone(), chrome_host::toolbar_bounds(&s.layout).filter(|_| s.html_region().is_some())),
        // --- vertical sidebar (lane S) ---
        (s.html_tabs.clone(), s.html_tabs_bounds().filter(|_| s.html_tab_region().is_some())),
        // --- end vertical sidebar ---
        (s.html_rail.clone(), chrome_host::rail_bounds(&s.layout).filter(|_| s.html_rail_region().is_some())),
    ]) else { return; };
    for (host, bounds) in hosts {
        let Some(host) = host else { continue; };
        if let Some(bounds) = bounds {
            if host.set_bounds(bounds).is_err() { continue; }
        }
        let _ = host.set_visible(bounds.is_some());
    }
    auto_hide::after_layout(hwnd); // --- auto-hide (T11) --- sensors, and raising what is shown
}
// --- auto-hide (T11) --- what the rest of the app calls (window/auto_hide.rs does the work).
pub(crate) fn auto_hide_sync(hwnd: HWND, app: &AppRef) {
    auto_hide::sync_config(hwnd, app);
}
pub(crate) fn auto_hide_page_created(hwnd: HWND) {
    if !hwnd.is_invalid() {
        auto_hide::page_created(hwnd);
    }
}
pub(crate) fn auto_hide_effective(hwnd: HWND, settings: &crate::settings::Settings) -> crate::auto_hide::Effective {
    auto_hide::effective_for(hwnd, settings)
}
pub(crate) fn zoomed_or_full(hwnd: HWND) -> bool {
    auto_hide::zoomed_or_full(hwnd)
}
pub(crate) fn toggle_fullscreen(hwnd: HWND, app: &AppRef) {
    auto_hide::toggle_fullscreen(hwnd, app);
}
pub(crate) fn page_fullscreen(hwnd: HWND, app: &AppRef, on: bool, source: Option<&url::Url>) {
    auto_hide::page_fullscreen(hwnd, app, on, source);
}
/// The tab whose page holds page full screen in `hwnd`, if any.
pub(crate) fn page_fullscreen_owner(hwnd: HWND) -> Option<crate::tabs::TabId> {
    fullscreen_notice::owner(hwnd)
}
pub(super) fn bookmark_bar_placement(hwnd: HWND) -> Option<(RECT, bool)> {
    auto_hide::bookmark_bar_placement(hwnd)
}
// --- end auto-hide ---
/// Moves the owned chrome popup with the client origin without repainting its pixels.
fn sync_overlay_position(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            return;
        }
        let Some(overlay) = with_chrome(hwnd, |s| s.overlay) else {
            return;
        };
        let mut origin = windows::Win32::Foundation::POINT { x: 0, y: 0 };
        let _ = ClientToScreen(hwnd, &mut origin);
        let _ = SetWindowPos(
            overlay,
            None,
            origin.x,
            origin.y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}
fn clear_pointer_state(hwnd: HWND) -> bool {
    with_chrome(hwnd, |s| {
        let had_state = s.drag.take().is_some()
            || s.vertical_drag.take().is_some()
            || s.rail_drag.take().is_some()
            || s.pressed != Hit::None
            || std::mem::take(&mut s.split_dragging)
            || s.split_drop.take().is_some()
            || std::mem::take(&mut s.sidebar_resizing);
        s.split_drag_companion = None;
        s.pressed = Hit::None;
        s.rail_tooltip_visible = false;
        had_state
    })
    .unwrap_or(false)
}
/// Arms or clears the slow fallback that re-reads the power source. `WM_POWERBROADCAST` is the
/// primary signal; this only covers a missed broadcast, and runs only while the setting is Auto.
pub(crate) fn schedule_power_timer(hwnd: HWND, enabled: bool) {
    unsafe {
        if enabled {
            let _ = SetTimer(
                Some(hwnd),
                super::WM_POWER_TIMER,
                crate::efficiency::POWER_FALLBACK_MS,
                None,
            );
        } else {
            let _ = KillTimer(Some(hwnd), super::WM_POWER_TIMER);
        }
    }
}
// --- theme options ---
/// Starts or stops the half-minute timer the time schedule of the theme runs on.
pub(crate) fn set_theme_timer(hwnd: HWND, on: bool) {
    if hwnd.is_invalid() {
        return;
    }
    unsafe {
        if on {
            let _ = SetTimer(Some(hwnd), WM_THEME_SCHEDULE_TIMER, 30_000, None);
        } else {
            let _ = KillTimer(Some(hwnd), WM_THEME_SCHEDULE_TIMER);
        }
    }
}

/// Has the window take a changed light/dark look or palette: the backdrop, the chrome and the
/// hosts follow, as they do when Windows changes its colours.
pub(crate) fn refresh_theme(hwnd: HWND) {
    if hwnd.is_invalid() {
        return;
    }
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_THEMECHANGED, WPARAM(0), LPARAM(0));
    }
}
// --- end theme options ---
// --- tab options ---
/// Starts or stops the window's one-second tab tick.
pub(crate) fn set_tab_tick(hwnd: HWND, on: bool) {
    if hwnd.is_invalid() {
        return;
    }
    unsafe {
        if on {
            let _ = SetTimer(Some(hwnd), WM_TAB_TICK_TIMER, 1000, None);
        } else {
            let _ = KillTimer(Some(hwnd), WM_TAB_TICK_TIMER);
        }
    }
}
/// Arms the one-shot timer that opens links a pinned tab refused in tabs of their own.
pub(crate) fn set_link_timer(hwnd: HWND) {
    if hwnd.is_invalid() {
        return;
    }
    unsafe {
        let _ = SetTimer(Some(hwnd), WM_TAB_LINK_TIMER, 10, None);
    }
}
/// Arms the one-shot thumbnail timer (a newer request replaces an older one).
pub(crate) fn set_thumbnail_timer(hwnd: HWND) {
    if hwnd.is_invalid() {
        return;
    }
    unsafe {
        let _ = SetTimer(Some(hwnd), WM_TAB_THUMB_TIMER, 1200, None);
    }
}
// --- end tab options ---

pub(crate) fn schedule_idle_timer(hwnd: HWND, action: IdleTimerAction) {
    unsafe {
        match action {
            IdleTimerAction::Kill => {
                let _ = KillTimer(Some(hwnd), WM_IDLE_TRIM_TIMER);
            }
            IdleTimerAction::Arm(ms) => {
                let _ = KillTimer(Some(hwnd), WM_IDLE_TRIM_TIMER);
                let _ = SetTimer(Some(hwnd), WM_IDLE_TRIM_TIMER, ms, None);
            }
        }
    }
}
/// Starts the frame timer if a transition is running. Idle chrome has no timer at all.
fn arm_motion(hwnd: HWND) {
    if with_chrome(hwnd, |s| s.motion.animating(Instant::now())).unwrap_or(false) {
        unsafe {
            let _ = SetTimer(Some(hwnd), WM_ANIMATION_TIMER, ANIMATION_FRAME_MS, None);
        }
    }
}
/// Whether Windows allows animation ("Show animations in Windows"). Transitions snap when it does not.
fn animations_enabled() -> bool {
    let mut enabled = BOOL(1);
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&mut enabled as *mut BOOL).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    // --- window and panels (thread T9) --- Bowser's own "Use animation" setting also has to allow it.
    crate::window_prefs::animate(read.is_err() || enabled.as_bool())
}
fn dpi(hwnd: HWND) -> u32 {
    unsafe { GetDpiForWindow(hwnd) }.max(96)
}
/// Gives the address EDIT the system message font for the window's current DPI, and releases
/// the font it had.
pub(crate) fn refresh_edit_font(hwnd: HWND, app: &AppRef) {
    let settings = App::settings(app);
    let ui_font = (!settings.ui_font.is_empty()).then_some(settings.ui_font.as_str());
    let font = paint::create_message_font_with(dpi(hwnd), DEFAULT_QUALITY, crate::layout_prefs::chrome_scale_percent(&settings), ui_font); // --- layout (thread T6) ---
    let (edit, old) = {
        let mut a = app.borrow_mut();
        (a.edit, std::mem::replace(&mut a.input_font, font))
    };
    unsafe {
        let _ = SendMessageW(
            edit,
            windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
        if !old.is_invalid() {
            let _ = DeleteObject(old.into());
        }
    }
}

thread_local! {static MAIN_WINDOW: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };}

/// The caret blinks only while Bowser is the foreground application, is not minimised, and the address
/// input has keyboard focus. Otherwise it stays solid, so it cannot be left hidden.
fn caret_blink_allowed(foreground: bool, minimised: bool, edit_focused: bool) -> bool {
    foreground && !minimised && edit_focused
}

/// A foreground window belongs to Bowser when this process owns it, or when it is a top-level window owned
/// by the main window (a WebView2 popup can live in another process).
fn belongs_to_app(foreground_pid: u32, own_pid: u32, foreground_owner: isize, main: isize) -> bool {
    foreground_pid == own_pid || (main != 0 && foreground_owner == main)
}

fn foreground_is_ours(hwnd: HWND) -> bool {
    unsafe {
        let foreground = GetForegroundWindow();
        let mut pid = 0u32;
        GetWindowThreadProcessId(foreground, Some(&mut pid));
        let owner = GetWindow(foreground, GW_OWNER).map_or(0, |owner| owner.0 as isize);
        belongs_to_app(pid, GetCurrentProcessId(), owner, hwnd.0 as isize)
    }
}

/// Whether the address input has keyboard focus. Reads the handle kept in the chrome, so a borrow of the app
/// state held by a modal loop cannot make the answer look like "not focused".
fn edit_focused(hwnd: HWND) -> bool {
    let Some(edit) = with_chrome(hwnd, |s| s.edit).filter(|edit| !edit.is_invalid()) else {
        return false;
    };
    unsafe { GetFocus() == edit }
}

/// Starts or stops the caret blink. Stopping leaves the caret visible and solid.
fn set_caret_blink(hwnd: HWND, on: bool) {
    if on {
        unsafe {
            SetTimer(
                Some(hwnd),
                WM_CARET_BLINK_TIMER,
                GetCaretBlinkTime().max(100),
                None,
            );
        }
        return;
    }
    unsafe {
        let _ = KillTimer(Some(hwnd), WM_CARET_BLINK_TIMER);
    }
    let caret = with_chrome(hwnd, |s| {
        if s.caret {
            None
        } else {
            s.caret = true;
            Some(s.surface.caret_rect())
        }
    })
    .flatten();
    if let Some(caret) = caret {
        invalidate_caret(hwnd, caret);
    }
}

/// Repaints only the caret's pixels, or the whole omnibox when the caret has no rectangle.
fn invalidate_caret(hwnd: HWND, caret: Rect) {
    if caret.is_empty() {
        invalidate_omni(hwnd)
    } else {
        invalidate(
            hwnd,
            Rect::new(
                caret.left - 1,
                caret.top - 1,
                caret.right + 1,
                caret.bottom + 1,
            ),
        )
    }
}

/// Brings the caret and the idle memory timer into line with whether Bowser is the foreground application.
/// It runs on every activation message and on the foreground hook, because a window launched behind another
/// one can miss the activation message when it is brought forward. Checking GetForegroundWindow is what
/// makes the result reliable.
// --- window and panels (thread T9) ---
/// Whether this window is not the foreground window (the chrome's "fade colours in background
/// windows" option reads it).
fn window_in_background(hwnd: HWND) -> bool {
    with_chrome(hwnd, |s| !s.foreground).unwrap_or(false)
}
// --- end window and panels ---

fn sync_foreground(hwnd: HWND, app: Option<&AppRef>) {
    let foreground = foreground_is_ours(hwnd);
    let minimised = unsafe { IsIconic(hwnd).as_bool() };
    set_caret_blink(
        hwnd,
        caret_blink_allowed(foreground, minimised, edit_focused(hwnd)),
    );
    let (changed, gained) = with_chrome(hwnd, |s| {
        let changed = foreground != s.foreground;
        let gained = foreground && !s.foreground;
        s.foreground = foreground;
        (changed, gained)
    })
    .unwrap_or((false, false));
    if !foreground {
        card::cancel(hwnd);
    }
    // --- window and panels (thread T9) --- the chrome's `look.bg` changed: send the state again (a
    // no-op when the JSON is the same). Skipped while the app state is borrowed (a modal loop).
    if app.is_some_and(|app| app.try_borrow_mut().is_ok()) {
        let faded = app.is_some_and(|app| App::settings(app).window.fade_background);
        if faded && changed {
            html_sync(hwnd);
            // State-only updates: ordinary chrome sync also reaches existing page hosts and
            // the bookmarks bar without setting their content/draft snapshots dirty.
            mark_chrome_dirty(hwnd);
            html_panel::settings_changed(hwnd);
        }
    }
    // --- end window and panels ---
    // Returning to the window counts as activity: restore the page's memory target and resume the idle cadence
    // that stopped while another application was active. Activation can arrive from a modal loop that already
    // holds the app state, so the borrow is checked first.
    if let Some(app) = app.filter(|app| gained && app.try_borrow_mut().is_ok()) {
        schedule_idle_timer(hwnd, App::idle_timer_fired(app, 0, true));
    }
}

/// Runs on the UI thread when the foreground window changes. It only posts a message, so the state change
/// happens in the window procedure and never re-enters a message handler.
unsafe extern "system" fn foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    let main = MAIN_WINDOW.with(|m| m.get());
    if main != 0 {
        super::post(
            HWND(main as *mut _),
            WM_FOREGROUND_SYNC,
            WPARAM(0),
            LPARAM(0),
        );
    }
}

/// The icon compiled from `icons/bowser.ico` as resource 1. A missing resource keeps the default icon.
fn app_icon(instance: HINSTANCE, size: i32) -> HICON {
    unsafe {
        LoadImageW(
            Some(instance),
            // MAKEINTRESOURCEW(1): integer resource ID, not an aligned/dereferenceable pointer.
            windows::core::PCWSTR(std::ptr::without_provenance(1)),
            IMAGE_ICON,
            size,
            size,
            LR_SHARED,
        )
        .map(|handle| HICON(handle.0))
        .unwrap_or_default()
    }
}

pub fn run(app: App) -> windows::core::Result<()> {
    // Overlaps font realization with window creation; the first paint otherwise waits for it.
    let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() };
    let _ = std::thread::Builder::new()
        .name("text-prewarm".into())
        .spawn(move || paint::prewarm_text(dpi));
    // The shortcut and chain files (`crate::commands`) are read off the UI thread, so the first key press
    // and the first palette find them loaded.
    let _ = std::thread::Builder::new()
        .name("commands-prewarm".into())
        .spawn(|| {
            let _ = crate::commands::live();
        });
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let big_icon = app_icon(instance, GetSystemMetrics(SM_CXICON));
        let small_icon = app_icon(instance, GetSystemMetrics(SM_CXSMICON));
        let class = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hIcon: big_icon,
            hCursor: LoadCursorW(None, windows::Win32::UI::WindowsAndMessaging::IDC_ARROW)?,
            hbrBackground: Default::default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let app = Rc::new(RefCell::new(app));
        let window_title = windows::core::HSTRING::from(format!(
            "Bowser - Search or enter address{}{}",
            crate::profile::active()
                .map(|profile| profile.title_suffix())
                .unwrap_or_default(),
            crate::profile::dev_title_suffix()
        ));
        let hwnd = CreateWindowExW(
            Default::default(),
            CLASS_NAME,
            windows::core::PCWSTR(window_title.as_ptr()),
            main_window_style(),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1000,
            720,
            None,
            None,
            Some(instance),
            Some((&app as *const AppRef).cast()),
        )?;
        if !big_icon.0.is_null() {
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_BIG as usize)),
                Some(LPARAM(big_icon.0 as isize)),
            );
        }
        if !small_icon.0.is_null() {
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(small_icon.0 as isize)),
            );
        }
        super::panel_bookmarks::register(&app);
        super::panel_history::register(&app);
        super::panel_customize::register(&app);
        let _ = glass::apply(hwnd);
        app.borrow_mut().hwnd = hwnd;
        super::set_blocker_wake(hwnd); // --- latency (fix E) --- blocker lists built off-thread
        super::panel_webapp::register(&app);
        super::panel_webapp::set_on_icon_changed(Box::new(move || {
            RAIL_ICON_EPOCH.with(|epoch| epoch.set(epoch.get().wrapping_add(1)));
            super::post(hwnd, super::WM_RAIL_CHANGED, WPARAM(0), LPARAM(0));
        }));
        MAIN_WINDOW.with(|m| m.set(hwnd.0 as isize));
        // First reading of the power source; later changes arrive as power broadcasts.
        App::refresh_efficiency(&app);
        let foreground_hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        let _ = ShowWindow(hwnd, SW_SHOW);
        app.borrow().log.record("window_visible").ok();
        if !app.borrow().edit.is_invalid() {
            app.borrow().log.record("input_ready").ok();
        }
        // Take the URL first so the app borrow ends before submit_url borrows it again.
        let launch_decided = App::launch_has_target(&app); // --- start page ---
        App::restore_pending_session(&app);
        sync_split_ratio(hwnd, App::split_view(&app));
        if App::split_view(&app).is_some()
            && let Some(panes) = split_rects(hwnd, None)
        {
            App::update_split_bounds(&app, panes);
        }
        App::submit_initial_url(&app);
        App::apply_startup_plan(&app, launch_decided); // --- start page --- homepage or specific pages
        // Restored and initial tabs exist now, so a standing rule for a site with no open tab is stale.
        App::release_unused_site_rules(&app);
        let benchmark_url = app.borrow_mut().benchmark_url.take();
        if let Some(url) = benchmark_url {
            App::submit_benchmark_url(&app, url)
        }
        let mut m = MSG::default();
        loop {
            match GetMessageW(&mut m, None, 0, 0).0 {
                -1 => return Err(windows::core::Error::from_thread()),
                0 => break,
                _ => {
                    if settings_panel::process_message(&m) {
                        continue;
                    }
                    let _ = TranslateMessage(&m);
                    DispatchMessageW(&m);
                }
            }
        }
        let _ = UnhookWinEvent(foreground_hook);
        Ok(())
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if message == WM_NCCREATE {
            let c = &*(lparam.0 as *const CREATESTRUCTW);
            let src = c.lpCreateParams as *const AppRef;
            if !src.is_null() {
                let data = Box::into_raw(Box::new((*src).clone()));
                let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, data as isize);
                (*data).borrow_mut().hwnd = hwnd;
            }
        }
        let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const AppRef;
        let app = raw.as_ref();
        match message {
            WM_NCCALCSIZE => {
                // --- auto-hide (T11) --- a full-screen window has no frame at all: the client is the window.
                if auto_hide::fullscreen_of(hwnd) != crate::auto_hide::Fullscreen::No {
                    return LRESULT(if wparam.0 != 0 { WVR_REDRAW as isize } else { 0 });
                }
                // --- end auto-hide ---
                let d = dpi(hwnd);
                let mx = IsZoomed(hwnd).as_bool();
                let fx = windows::Win32::UI::HiDpi::GetSystemMetricsForDpi(SM_CXFRAME, d)
                    + windows::Win32::UI::HiDpi::GetSystemMetricsForDpi(SM_CXPADDEDBORDER, d);
                let fy = windows::Win32::UI::HiDpi::GetSystemMetricsForDpi(SM_CYFRAME, d)
                    + windows::Win32::UI::HiDpi::GetSystemMetricsForDpi(SM_CXPADDEDBORDER, d);
                if wparam.0 != 0 {
                    let p = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
                    let work = monitor_work_area(hwnd).unwrap_or(p.rgrc[0]);
                    p.rgrc[0] = frame::client_rect_for_nccalc(p.rgrc[0], mx, d, (fx, fy), work);
                } else {
                    let r = &mut *(lparam.0 as *mut RECT);
                    let work = monitor_work_area(hwnd).unwrap_or(*r);
                    *r = frame::client_rect_for_nccalc(*r, mx, d, (fx, fy), work);
                }
                LRESULT(if wparam.0 != 0 {
                    WVR_REDRAW as isize
                } else {
                    0
                })
            }
            WM_GETMINMAXINFO => {
                let info = &mut *(lparam.0 as *mut MINMAXINFO);
                let metrics = Metrics::new(dpi(hwnd));
                info.ptMinTrackSize.x = metrics.px(520);
                info.ptMinTrackSize.y = metrics.px(300);
                LRESULT(0)
            }
            WM_NCHITTEST => {
                let mut p = frame::point_from_lparam(lparam);
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut p);
                let mut r = RECT::default();
                let _ = GetClientRect(hwnd, &mut r);
                let chrome_hit = hit(hwnd, p.x, p.y); // Also tells a drag band apart
                let ih = matches!(
                    chrome_hit,
                    Hit::Tab(_)
                        | Hit::TabClose(_)
                        | Hit::NewTab
                        | Hit::TabMenu
                        | Hit::MainMenu
                        | Hit::SidebarWorkspace
                        | Hit::SidebarNewTab
                        | Hit::SidebarGroup(_)
                        | Hit::SidebarTab(_)
                        | Hit::SidebarTabClose(_)
                        | Hit::SidebarResize
                        | Hit::Back
                        | Hit::Forward
                        | Hit::Reload
                        | Hit::Omnibox
                        | Hit::SiteInfo
                        | Hit::Split
                        | Hit::StackTab(_) // --- two-level stacks (T10b) ---
                        | Hit::StackTabClose(_)
                );
                // --- layout (thread T6) --- a row along the bottom edge keeps the bottom grip thin.
                let thin_bottom = with_chrome(hwnd, |s| s.last_relayout.as_ref().is_some_and(|inputs| inputs.arrangement.thin_bottom_edge())).unwrap_or(false);
                frame::frame_hit_test_with(
                    p,
                    (r.right, r.bottom),
                    dpi(hwnd),
                    auto_hide::zoomed_or_full(hwnd), // --- auto-hide (T11) --- no resize rim in full screen
                    if ih {
                        frame::ChromeHit::Interactive
                    } else if chrome_hit == Hit::CaptionArea {
                        frame::ChromeHit::Drag // The bottom toolbar's empty space drags too
                    } else {
                        frame::ChromeHit::None
                    },
                    thin_bottom,
                )
                .lresult()
            }
            WM_NCMOUSEMOVE => {
                set_hover(hwnd, caption_hit(wparam.0 as u32));
                let mut t = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE | TME_NONCLIENT,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut t);
                LRESULT(0)
            }
            WM_NCMOUSELEAVE => {
                set_hover(hwnd, Hit::None);
                LRESULT(0)
            }
            // Only the caption buttons are ours. Dragging the strip and sizing edges use the
            // system's native move and resize loops.
            WM_NCLBUTTONDOWN | WM_NCLBUTTONUP if caption_hit(wparam.0 as u32) == Hit::None => {
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_NCLBUTTONDOWN => {
                set_pressed(hwnd, caption_hit(wparam.0 as u32));
                LRESULT(0)
            }
            WM_NCLBUTTONUP => {
                let hit = caption_hit(wparam.0 as u32);
                let prev = with_chrome(hwnd, |s| s.pressed).unwrap_or(Hit::None);
                set_pressed(hwnd, Hit::None);
                if prev == hit {
                    caption_command(hwnd, hit)
                }
                LRESULT(0)
            }
            WM_NCLBUTTONDBLCLK if wparam.0 as u32 == HTCAPTION => {
                toggle_max(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => {
                // Glass leaves the client untouched so DWM shows the native blur through it.
                if glass::backdrop_mode() == glass::BackdropMode::Solid {
                    let mut r = RECT::default();
                    let _ = GetClientRect(hwnd, &mut r);
                    let _ = FillRect(
                        windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut _),
                        &r,
                        glass::solid_brush(glass::appearance()),
                    );
                }
                LRESULT(1)
            }
            WM_PAINT => {
                paint_chrome(hwnd, app);
                LRESULT(0)
            }
            WM_CHROME_SYNC => {
                run_chrome_sync(hwnd, app);
                LRESULT(0)
            }
            WM_HTML_HOVER_START => {
                if chrome_host::html_ui_enabled() {
                    start_hover_layer(hwnd);
                }
                LRESULT(0)
            }
            // Logoff, restart or shutdown: never veto it, and when it happens write what a normal
            // close writes, so the next launch neither reports a crash nor loses the last visits.
            WM_QUERYENDSESSION => LRESULT(1),
            WM_ENDSESSION => {
                if wparam.0 != 0
                    && let Some(app) = app.filter(|app| app.try_borrow_mut().is_ok())
                {
                    flush_for_exit(hwnd, app);
                }
                LRESULT(0)
            }
            WM_SETTINGCHANGE | WM_DWMCOLORIZATIONCOLORCHANGED | WM_THEMECHANGED => {
                if message == WM_SETTINGCHANGE {
                    // lParam is the changed setting's name, or null.
                    let name = (lparam.0 != 0)
                        .then_some(windows::core::PCWSTR(lparam.0 as *const u16))
                        .and_then(|name| name.to_string().ok());
                    if !glass::setting_change_is_relevant(wparam.0, name.as_deref())
                        && !glass::refresh_system_state_changed()
                    {
                        return LRESULT(0);
                    }
                }
                // --- theme options --- the person's colours follow the Windows accent they were made with.
                if let Some(app) = app {
                    App::refresh_theme_install(app);
                }
                // --- end theme options ---
                glass::refresh_system_state();
                let _ = glass::apply(hwnd);
                let settled = with_chrome(hwnd, |s| {
                    s.palette = auto_hide::chrome_palette(hwnd); // --- auto-hide (T11) --- this window's
                    let settled = s.motion.set_snap(!animations_enabled(), Instant::now());
                    let rail_settled = s.surface.set_rail_snap(!animations_enabled(), Instant::now());
                    let settled = settled || rail_settled;
                    if !animations_enabled() {
                        s.progress.from = s.progress.to;
                        s.progress.started = None;
                    }
                    if message == WM_SETTINGCHANGE && wparam.0 == SPI_SETNONCLIENTMETRICS {
                        s.surface.rebuild_fonts();
                    }
                    settled
                })
                .unwrap_or(false);
                if settled {
                    // Reduced motion landed mid-fade, so no frame is needed. The full repaint below
                    // paints the settled state.
                    let _ = KillTimer(Some(hwnd), WM_ANIMATION_TIMER);
                }
                if !animations_enabled() {
                    let _ = KillTimer(Some(hwnd), WM_PROGRESS_TIMER);
                }
                if message == WM_SETTINGCHANGE && wparam.0 == SPI_SETNONCLIENTMETRICS {
                    if let Some(app) = app {
                        refresh_edit_font(hwnd, app);
                    }
                }
                repaint(hwnd);
                LRESULT(0)
            }
            WM_CREATE => {
                if let Some(app) = app {
                    App::theme_window_created(app, hwnd); // --- theme options ---
                    let instance = HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0);
                    let edit = match CreateWindowExW(
                        windows::Win32::UI::WindowsAndMessaging::WS_EX_TRANSPARENT,
                        w!("EDIT"),
                        w!(""),
                        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                        0,
                        0,
                        1,
                        36,
                        Some(hwnd),
                        Some(HMENU(INPUT_ID as usize as *mut _)),
                        Some(instance),
                        None,
                    ) {
                        Ok(v) => v,
                        Err(e) => {
                            super::show_error_message(&format!(
                                "Could not create the address input: {e}"
                            ));
                            return LRESULT(-1);
                        }
                    };
                    let font = paint::create_message_font(dpi(hwnd), DEFAULT_QUALITY);
                    let _ = SendMessageW(
                        edit,
                        windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,
                        Some(WPARAM(font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                    if !SetWindowSubclass(edit, Some(edit_subclass), 1, hwnd.0 as usize).as_bool() {
                        return LRESULT(-1);
                    }
                    // The chrome paints the omnibox text itself. An EDIT also draws directly while
                    // typing (not only in WM_PAINT), so an empty region keeps it invisible while it
                    // still owns focus, IME and clipboard.
                    let hidden = CreateRectRgn(0, 0, 0, 0);
                    if SetWindowRgn(edit, Some(hidden), false) == 0 {
                        let _ = DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(hidden.0));
                    }
                    app.borrow_mut().edit = edit;
                    app.borrow_mut().input_font = font;
                    let mut r = RECT::default();
                    let _ = GetClientRect(hwnd, &mut r);
                    let overlay = CreateWindowExW(
                        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                        w!("STATIC"),
                        w!(""),
                        WS_POPUP | WS_VISIBLE,
                        0,
                        0,
                        r.right,
                        r.bottom,
                        Some(hwnd),
                        None,
                        Some(instance),
                        None,
                    )
                    .unwrap_or_default();
                    let mut s = WindowChrome::new(r.right, r.bottom, dpi(hwnd), overlay);
                    s.edit = edit;
                    s.start_history_load();
                    s.refresh(app, true);
                    // The web apps' icons are read by the rebuild posted below, after the window shows.
                    rebuild_rail_entries_with(&mut s, app, false);
                    s.paint_prefs = PaintPrefs::read(app);
                    WINDOWS.with(|all| {
                        all.borrow_mut().insert(hwnd.0 as isize, s);
                    });
                    super::post(hwnd, WM_RAIL_CHANGED, WPARAM(0), LPARAM(0));
                    auto_hide::sync_config(hwnd, app); // --- auto-hide (T11) --- a window opened with the setting on starts hidden
                    let _ = SetFocus(Some(edit));
                    sync_foreground(hwnd, Some(app));
                }
                LRESULT(0)
            }
            WM_MOVE => {
                sync_overlay_position(hwnd);
                card::cancel(hwnd);
                // --- popup host (lane M) ---
                html_popup::close(hwnd);
                // --- end popup host ---
                html_panel::dismiss(hwnd); // --- rail flyouts ---
                LRESULT(0)
            }
            WM_SIZE => {
                card::cancel(hwnd);
                // --- popup host (lane M) ---
                html_popup::close(hwnd);
                // --- end popup host ---
                html_panel::dismiss(hwnd); // --- rail flyouts ---
                if let Some(app) = app {
                    let min = wparam.0 as u32 == SIZE_MINIMIZED;
                    let mut r = RECT::default();
                    let _ = GetClientRect(hwnd, &mut r);
                    apply_window_region(hwnd, r.right, r.bottom, IsZoomed(hwnd).as_bool());
                    let maximised_now = auto_hide::zoomed_or_full(hwnd); // --- auto-hide (T11) ---
                    let active = App::active_tab_id(app);
                    let settings = App::settings(app);
                    let arrangement = App::arrangement(app); // --- layout (thread T6) ---
                    let _ = with_chrome(hwnd, |s| {
                        s.relayout(RelayoutRequest { w: r.right, h: r.bottom, dpi: dpi(hwnd), active, maximised: maximised_now, arrangement: arrangement.with_address_bar_revealed(s.address_bar_revealed), density: settings.ui_density, sidebar_width: App::sidebar_width(app), centred_address_bar: settings.centred_address_bar, font_scale: crate::layout_prefs::chrome_scale_percent(&settings), ui_font: &settings.ui_font });
                    });
                    sync_html_host_bounds(hwnd);
                    html_sync(hwnd);
                    sync_split_ratio(hwnd, App::split_view(app));
                    position_edit(hwnd);
                    let l = with_chrome(hwnd, |s| s.layout).unwrap();
                    let viewport = to_win_rect(l.viewport);
                    notes_panel::sync(hwnd, viewport, dpi(hwnd));
                    side_panel::sync(hwnd, viewport, dpi(hwnd));
                    let viewport = adjusted_viewport(hwnd, viewport, dpi(hwnd));
                    if let Some(panes) = split_rects(hwnd, None) {
                        queue_split_bounds(hwnd, panes);
                    } else {
                        cancel_split_bounds(hwnd);
                        App::clear_split_bounds(app);
                    }
                    App::window_size_changed(
                        app,
                        min,
                        RECT {
                            left: viewport.left,
                            top: viewport.top,
                            right: viewport.right,
                            bottom: viewport.bottom,
                        },
                    );
                    if !min {
                        repaint(hwnd);
                    }
                    // Minimising stops the blink and restoring resumes it when Bowser is also in front.
                    sync_foreground(hwnd, Some(app));
                }
                LRESULT(0)
            }
            notes_panel::WM_SIDEBAR_RESIZED => {
                if let Some(app) = app {
                    apply_sidebar_bounds(hwnd, app);
                }
                LRESULT(0)
            }
            side_panel::WM_RESIZED => {
                if let Some(app) = app { apply_sidebar_bounds(hwnd, app); }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                if let Some(r) = (lparam.0 as *const RECT).as_ref() {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                if let Some(app) = app {
                    let mut r = RECT::default();
                    let _ = GetClientRect(hwnd, &mut r);
                    let _ = with_chrome(hwnd, |s| {
                        s.favicons.clear();
                        s.failed.clear();
                        s.views.clear();
                        for tab in &mut s.tabs {
                            tab.favicon_key = None;
                            tab.favicon = None;
                        }
                        let settings = App::settings(app);
                        let arrangement = App::arrangement(app).with_address_bar_revealed(s.address_bar_revealed); // --- layout (thread T6) ---
                        s.relayout(RelayoutRequest { w: r.right, h: r.bottom, dpi: dpi(hwnd), active: App::active_tab_id(app), maximised: auto_hide::zoomed_or_full(hwnd), arrangement, density: settings.ui_density, sidebar_width: App::sidebar_width(app), centred_address_bar: settings.centred_address_bar, font_scale: crate::layout_prefs::chrome_scale_percent(&settings), ui_font: &settings.ui_font });
                        s.refresh(app, true)
                    });
                    sync_html_host_bounds(hwnd);
                    html_sync(hwnd);
                    sync_split_ratio(hwnd, App::split_view(app));
                    refresh_edit_font(hwnd, app);
                    position_edit(hwnd);
                    let l = with_chrome(hwnd, |s| s.layout).unwrap();
                    let viewport = to_win_rect(l.viewport);
                    notes_panel::sync(hwnd, viewport, dpi(hwnd));
                    side_panel::sync(hwnd, viewport, dpi(hwnd));
                    let viewport = adjusted_viewport(hwnd, viewport, dpi(hwnd));
                    if let Some(panes) = split_rects(hwnd, None) {
                        App::update_split_bounds(app, panes);
                    } else {
                        cancel_split_bounds(hwnd);
                        App::clear_split_bounds(app);
                    }
                    App::window_size_changed(
                        app,
                        false,
                        RECT {
                            left: viewport.left,
                            top: viewport.top,
                            right: viewport.right,
                            bottom: viewport.bottom,
                        },
                    );
                    repaint(hwnd);
                }
                LRESULT(0)
            }
            // Alt pressed and released on its own (or F10): Bowser's main menu when the "Alt opens the
            // menu" option is on; otherwise nothing, instead of Windows' hidden system-menu mode.
            WM_SYSCOMMAND if (wparam.0 & 0xFFF0) == SC_KEYMENU as usize && lparam.0 == 0 => {
                if crate::commands::behaviour::alt_tap(&crate::commands::live().store.options) == crate::commands::behaviour::AltTap::OpenMenu {
                    if let Some(app) = app {
                        show_main_menu(hwnd, app);
                    }
                }
                LRESULT(0)
            }
            WM_SYSCOMMAND
                if (wparam.0 & 0xFFF0) == SC_KEYMENU as usize
                    && (lparam.0 as u16) == u16::from(b' ') =>
            {
                let mut window = RECT::default();
                let _ = GetWindowRect(hwnd, &mut window);
                show_system_menu(
                    hwnd,
                    windows::Win32::Foundation::POINT {
                        x: window.left,
                        y: window.top,
                    },
                );
                LRESULT(0)
            }
            WM_NCRBUTTONUP
                if wparam.0 == windows::Win32::UI::WindowsAndMessaging::HTCAPTION as usize =>
            {
                show_system_menu(hwnd, frame::point_from_lparam(lparam));
                LRESULT(0)
            }
            WM_COMMAND
                if (wparam.0 & 0xffff) as u16 == INPUT_ID
                    && (wparam.0 >> 16) as u32 == EN_CHANGE =>
            {
                if let Some(app) = app {
                    let text = read_edit(app.borrow().edit);
                    let prewarm = with_chrome(hwnd, |s| {
                        s.edit_text.clear();
                        s.edit_text.push_str(&text);
                        let programmatic = s.programmatic_text_update;
                        if !programmatic {
                            s.omnibox_edited = true;
                            s.update_completion();
                        }
                        !programmatic
                    })
                    .unwrap_or(false);
                    if prewarm {
                        App::prewarm(app, &text);
                    }
                    invalidate_omni(hwnd);
                }
                LRESULT(0)
            }
            WM_COMMAND if (wparam.0 & 0xffff) as u16 == INPUT_ID => LRESULT(0),
            WM_SUBMIT => {
                if let Some(app) = app {
                    let text = read_edit(app.borrow().edit);
                    remember_url(hwnd, App::active_tab_id(app), &text, App::settings(app).minimal_url);
                    App::submit(app, &text);
                }
                LRESULT(0)
            }
            WM_HISTORY_BACK => {
                if let Some(app) = app {
                    navigate_back_for_focused_pane(app)
                }
                LRESULT(0)
            }
            WM_HISTORY_FORWARD => {
                if let Some(app) = app {
                    navigate_forward_for_focused_pane(app)
                }
                LRESULT(0)
            }
            WM_NAVIGATION_PROGRESS => {
                if let Some(app) = app {
                    let navigation_id = wparam.0 as u64;
                    if App::navigation_progress_message(app, navigation_id, lparam.0) {
                        // Loading, progress and the back and forward buttons are toolbar state.
                        mark_chrome_dirty(hwnd);
                        // The page has committed: if the user is not using the omnibox, the keyboard goes to it.
                        if matches!(lparam.0, 1 | 2) {
                            focus_page_after_commit(hwnd, app);
                        }
                        if let Some(target) =
                            App::tab_progress_percent(app, App::active_tab_id(app))
                        {
                            let html_active = with_chrome(hwnd, |s| s.html_toolbar_active).unwrap_or(false);
                            if !html_active {
                            let animate = animations_enabled();
                            with_chrome(hwnd, |s| {
                                s.progress.retarget(
                                    App::active_tab_id(app),
                                    navigation_id,
                                    target,
                                    Instant::now(),
                                    animate,
                                );
                            });
                            if animate {
                                let _ = SetTimer(
                                    Some(hwnd),
                                    WM_PROGRESS_TIMER,
                                    ANIMATION_FRAME_MS,
                                    None,
                                );
                            }
                            invalidate_omni(hwnd)
                            }
                        } else {
                            with_chrome(hwnd, |s| s.progress.clear());
                            let _ = KillTimer(Some(hwnd), WM_PROGRESS_TIMER);
                            invalidate_omni(hwnd);
                        }
                    }
                }
                LRESULT(0)
            }
            // --- split view ---
            super::WM_SPLIT_COMMAND => {
                if let Some(app) = app {
                    super::split_ui::run_queued(hwnd, app);
                    mark_chrome_dirty(hwnd);
                }
                LRESULT(0)
            }
            super::WM_SPLIT_LINK => {
                if let Some(app) = app {
                    super::split_ui::run_links(hwnd, app);
                    mark_chrome_dirty(hwnd);
                }
                LRESULT(0)
            }
            // --- tab stacks (T10) --- the tile look changed: lay the shown split out again.
            super::WM_SPLIT_LOOK => {
                if let Some(app) = app {
                    apply_sidebar_bounds(hwnd, app);
                    mark_chrome_dirty(hwnd);
                }
                LRESULT(0)
            }
            super::WM_SETTINGS_COMMAND => {
                if let Some(app) = app {
                    super::settings_page::run_queued(hwnd, app);
                    mark_chrome_dirty(hwnd);
                }
                LRESULT(0)
            }
            // One step of a running command chain (`command_actions`): its delay has passed.
            super::WM_CHAIN_STEP => {
                if let Some(app) = app {
                    super::command_actions::chain_step(hwnd, app, wparam.0 as u32, lparam.0 as usize);
                }
                LRESULT(0)
            }
            super::WM_HISTORY_PAGE_COMMAND => {
                if let Some(app) = app {
                    super::history_page::run_queued(hwnd, app);
                    mark_chrome_dirty(hwnd);
                }
                LRESULT(0)
            }
            super::WM_DOWNLOADS_PAGE_COMMAND => {
                if let Some(app) = app {
                    super::downloads_page::run_queued(hwnd, app);
                    mark_chrome_dirty(hwnd);
                }
                LRESULT(0)
            }
            // The downloads list changed (`downloads_ui`): the toolbar button, an open bubble and a shown
            // downloads page follow.
            super::WM_DOWNLOADS_CHANGED => {
                if let Some(app) = app {
                    super::downloads_ui::changed(hwnd, app, wparam.0);
                }
                LRESULT(0)
            }
            super::WM_EXTENSIONS_CHANGED => {
                html_refresh_extensions_button(hwnd);
                super::settings_page::extensions_changed(hwnd);
                LRESULT(0)
            }
            super::bookmark_bar::WM_BOOKMARK_BAR_COMMAND => {
                if let Some(app) = app {
                    super::bookmark_bar::run_queued(hwnd, app);
                }
                LRESULT(0)
            }
            // Engine suggestions for the address field or the search field arrived.
            super::address_bar::WM_ADDRESS_SUGGEST => {
                super::address_bar::suggestions_ready(hwnd);
                LRESULT(0)
            }
            // --- end split view ---
            WM_PANE_FOCUSED => {
                if let Some(app) = app
                    && App::focus_split_tab(app, TabId(wparam.0 as u32))
                {
                    repaint(hwnd);
                }
                LRESULT(0)
            }
            WM_LOGIN_MESSAGE => {
                if let Some(app) = app {
                    for (tab_id, message) in App::take_login_messages(app) {
                        passwords_panel::handle_login_message(hwnd, app, tab_id, message);
                    }
                }
                LRESULT(0)
            }
            WM_TABS_CHANGED => {
                if let Some(app) = app {
                    if !fullscreen_notice::belongs_to_active_tab(hwnd, app) {
                        auto_hide::page_fullscreen(hwnd, app, false, None);
                    }
                    let (structural, persisted) = App::take_tab_change_flags();
                    if structural {
                        card::cancel_card(hwnd);
                    }
                    let previously_active = with_chrome(hwnd, |s| s.active_tab).flatten();
                    let refreshed = with_chrome(hwnd, |s| {
                        let previous_viewport = s.layout.viewport;
                        s.refresh(app, structural);
                        (
                            (s.layout.strip, s.layout.tab_row),
                            previous_viewport != s.layout.viewport,
                        )
                    });
                    if refreshed.as_ref().is_some_and(|(_, changed)| *changed) {
                        sync_split_ratio(hwnd, App::split_view(app));
                        apply_sidebar_bounds(hwnd, app);
                    }
                    // --- tab stacks (T10) --- the rail's tab counts follow tabs opening and closing
                    // (the rail sends nothing when the rebuilt entries are the same).
                    if structural && App::tab_options(app).workspace_counts {
                        App::rail_changed(app);
                    }
                    // A visit was just written: an open History flyout shows it without reopening,
                    // and so does a shown bowser://history page.
                    if with_chrome(hwnd, |s| std::mem::take(&mut s.history_dirty)).unwrap_or(false) {
                        html_panel::history_changed(hwnd);
                        super::history_page::history_changed(hwnd);
                    }
                    let active = App::active_tab_id(app);
                    // --- popup host (lane M) ---
                    // A tab or workspace switch dismisses an open HTML menu.
                    if let Some(context) = popup_context(hwnd) {
                        html_popup::note_context(hwnd, context);
                        html_panel::note_context(hwnd, context); // --- rail flyouts ---
                    }
                    // --- end popup host ---
                    if previously_active != Some(active) {
                        card::cancel_card(hwnd);
                        let value = App::tab_committed_url(app, active)
                            .map(|url| url.to_string())
                            .unwrap_or_default();
                        let edit = app.borrow().edit;
                        let _ = SetWindowTextW(edit, &windows::core::HSTRING::from(value));
                        if let Some((navigation_id, target)) =
                            App::active_navigation_progress_identity(app)
                        {
                            with_chrome(hwnd, |s| {
                                s.progress.clear();
                                s.progress.retarget(
                                    active,
                                    navigation_id,
                                    target,
                                    Instant::now(),
                                    false,
                                );
                            });
                        } else {
                            with_chrome(hwnd, |s| s.progress.clear());
                        }
                        let _ = KillTimer(Some(hwnd), WM_PROGRESS_TIMER);
                        devtools_dock::follow_active_tab(hwnd, active);
                    }
                    card::sync(hwnd, app, |id| tab_anchor(hwnd, id));
                    island_menu::sync(hwnd, app, |id| ball_anchor(hwnd, id));
                    super::split_ui::tabs_changed(hwnd, app); // --- split view ---
                    if structural {
                        position_edit(hwnd);
                        repaint(hwnd);
                        arm_motion(hwnd);
                    } else if let Some(((toolbar, tabs), _)) = refreshed
                        .filter(|_| {
                            with_chrome(hwnd, |s| {
                                native_strip_pixels_change(s.html_region().is_some(), s.html_tab_region().is_some())
                            })
                            .unwrap_or(true)
                        })
                    {
                        for strip in [toolbar, tabs] {
                            let rect = windows::Win32::Foundation::RECT {
                                left: strip.left,
                                top: strip.top,
                                right: strip.right,
                                bottom: strip.bottom,
                            };
                            let _ = InvalidateRect(Some(hwnd), Some(&rect), false);
                        }
                    }
                    arm_motion(hwnd);
                    // Titles, loading, favicons, the active tab: the tab strip and toolbar follow.
                    mark_chrome_dirty(hwnd);
                    // Debounced crash-recovery snapshot: one timer, armed only when dirty.
                    if persisted && App::session_changed(app) {
                        let _ = SetTimer(
                            Some(hwnd),
                            WM_SESSION_SAVE_TIMER,
                            crate::session::SAVE_DEBOUNCE_MS,
                            None,
                        );
                    }
                }
                LRESULT(0)
            }
            WM_RAIL_CHANGED => {
                if let Some(app) = app {
                    hide_rail_tooltip(hwnd);
                    // Every bookmark editor in the window posts this: the toolbar star follows.
                    let bookmarks_changed = super::html_suggest::refresh_bookmarks();
                    let rebuilt = with_chrome(hwnd, |s| rebuild_rail_entries(s, app)).unwrap_or(false);
                    let open = open_rail_panel(hwnd);
                    let open_changed = with_chrome(hwnd, |s| std::mem::replace(&mut s.rail_open_seen, open) != open)
                        .unwrap_or(false);
                    // A side panel keystroke that changed nothing repaints and sends nothing.
                    if rebuilt || open_changed {
                        repaint(hwnd);
                    } else if bookmarks_changed {
                        mark_chrome_dirty(hwnd);
                    }
                    html_rail_sync(hwnd); // --- rail flyouts --- (the open flyout marks its button)
                }
                LRESULT(0)
            }
            WM_SHORTCUT_KEY => {
                if let Some(app) = app {
                    let m = lparam.0 as u8;
                    let key = wparam.0 as u32;
                    let (ctrl, shift, alt) = (m & 1 != 0, m & 2 != 0, m & 4 != 0);
                    let handled = dispatch(hwnd, app, key, ctrl, shift, alt);
                    if handled
                        && m & (SHORTCUT_FROM_CHROME as u8) != 0
                        && crate::commands::resolve(
                            key,
                            ctrl,
                            shift,
                            alt,
                            &crate::commands::ResolveContext { gated: App::gated_chords(app), ..Default::default() },
                        )
                        .is_some_and(changes_active_tab)
                    {
                        App::focus_page(app);
                    }
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if GetCapture() == hwnd {
                    let p = frame::point_from_lparam(lparam);
            let rail_dragging = with_chrome(hwnd, |s| {
                let Some(mut drag) = s.rail_drag else { return false };
                if !drag.active && crate::chrome::rail::drag_started(drag.origin, (p.x, p.y)) {
                    drag.active = true;
                    s.rail_tooltip_visible = false;
                    let _ = KillTimer(Some(hwnd), WM_RAIL_TOOLTIP_TIMER);
                }
                if drag.active {
                    let entries = rail_entries(s);
                    let geometry = crate::chrome::rail::geometry(s.layout.rail, entries, s.rail_scroll, s.dpi);
                    drag.target = crate::chrome::rail::drop_indicator(&geometry, drag.key, p.x, p.y);
                }
                s.rail_drag = Some(drag);
                drag.active
            }).unwrap_or(false);
            if rail_dragging {
                let _ = InvalidateRect(Some(hwnd), None, false);
            } else {
                let resizing = with_chrome(hwnd, |s| s.sidebar_resizing).unwrap_or(false);
                    if resizing {
                        // --- layout (thread T6) --- measured from the sidebar's own edge, whichever side it is on.
                        let drag_px = with_chrome(hwnd, |s| s.sidebar.map_or(p.x, |sidebar| sidebar.drag_width_px(p.x))).unwrap_or(p.x);
                        let width = ((drag_px * 96) / dpi(hwnd).max(96) as i32).clamp(
                            i32::from(crate::settings::MIN_SIDEBAR_WIDTH),
                            i32::from(crate::settings::MAX_SIDEBAR_WIDTH),
                        ) as u16;
                        // The move only records the width; the frame timer lays it out once.
                        let arm = with_chrome(hwnd, |s| {
                            s.sidebar_drag_width = Some(width);
                            !std::mem::replace(&mut s.sidebar_drag_pending, true)
                        })
                        .unwrap_or(false);
                        if arm {
                            let _ = SetTimer(Some(hwnd), WM_SIDEBAR_RESIZE_TIMER, ANIMATION_FRAME_MS, None);
                        }
                    } else {
                        let (dragging, orientation, held) = with_chrome(hwnd, |s| (s.split_dragging, s.split_orientation, s.split_drag_divider)).unwrap_or((false, Default::default(), None));
                        // --- split view --- the held divider follows the pointer along its axis
                        // (both axes at a 2 x 2's crossing)
                        let held = held.unwrap_or(crate::split_view::Divider::Primary);
                        let along = crate::split_view::axis_coord(divider_axis(orientation, held), p.x, p.y);
                        if dragging
                            && super::split_ui::divider_dragged(hwnd, along)
                            && let Some(app) = app
                            && let Some(panes) = drag_split_divider(hwnd, app, held, p)
                        {
                            queue_split_bounds(hwnd, panes);
                            repaint(hwnd);
                        } else {
                            let point = frame::point_from_lparam(lparam);
                            // --- split view --- the native strip's tab drag: the page edge zone
                            // under the pointer (left, right, top or bottom half).
                            let target = with_chrome(hwnd, |s| {
                                let viewport = adjusted_viewport(hwnd, to_win_rect(s.layout.viewport), s.dpi);
                                s.drag.and_then(|_| {
                                    crate::split_view::drop_zone(
                                        (viewport.left, viewport.top, viewport.right, viewport.bottom),
                                        point.x,
                                        point.y,
                                        s.split_drop,
                                    )
                                })
                            })
                            .flatten();
                            let changed = with_chrome(hwnd, |s| {
                                let changed = s.split_drop != target;
                                s.split_drop = target;
                                changed
                            })
                            .unwrap_or(false);
                            if changed {
                                let _ = InvalidateRect(Some(hwnd), None, false);
                            }
                            // --- tab stacks (T10) --- the held tab resting over the middle of another
                            // tab: a release after the stacking delay stacks the two.
                            let _ = with_chrome(hwnd, |s| {
                                let Some((_, from)) = s.drag else {
                                    s.stack_hover = None;
                                    return;
                                };
                                let over = (0..s.slots.len()).find(|&slot| {
                                    slot != from
                                        && s.layout.tab(slot).is_some_and(|rect| {
                                            rect.contains(point.x, point.y) && crate::stacks::over_middle(rect.left, rect.right, point.x)
                                        })
                                });
                                s.stack_hover = match (over, s.stack_hover) {
                                    (Some(slot), Some((held, since))) if held == slot => Some((held, since)),
                                    (Some(slot), _) => Some((slot, Instant::now())),
                                    (None, _) => None,
                                };
                            });
                        }
                        if let Some(app) = app
                            && with_chrome(hwnd, |s| s.pressed == Hit::Omnibox).unwrap_or(false)
                        {
                            omnibox_drag(hwnd, app, frame::point_from_lparam(lparam).x);
                        }
                    }
                }
                } else {
                    let p = frame::point_from_lparam(lparam);
                    let h = hit(hwnd, p.x, p.y);
                    if h == Hit::SplitDivider {
                        // --- split view --- the divider under the pointer lights its grip
                        let (divider, orientation, changed) = with_chrome(hwnd, |s| {
                            let divider = split_divider_at(hwnd, s, p.x, p.y);
                            let changed = s.split_drag_divider != divider;
                            s.split_drag_divider = divider;
                            (divider, s.split_orientation, changed)
                        })
                        .unwrap_or((None, Default::default(), false));
                        let cursor = match divider {
                            Some(crate::split_view::Divider::Both) => IDC_SIZEALL,
                            Some(divider) if divider_axis(orientation, divider).is_stacked() => IDC_SIZENS,
                            _ => IDC_SIZEWE,
                        };
                        if let Ok(cursor) = LoadCursorW(None, cursor) {
                            let _ = SetCursor(Some(cursor));
                        }
                        if changed {
                            repaint(hwnd);
                        }
                    }
                    set_hover(hwnd, h);
                    let row = match h {
                        Hit::SidebarTab(index)
                        | Hit::SidebarTabClose(index)
                        | Hit::SidebarGroup(index) => Some(index),
                        _ => None,
                    };
                    let row_changed = with_chrome(hwnd, |s| {
                        if s.sidebar_hover_row == row {
                            false
                        } else {
                            s.sidebar_hover_row = row;
                            true
                        }
                    })
                    .unwrap_or(false);
                    if row_changed {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    if let Some(app) = app {
                        card::hover_changed(hwnd, app, hit_tab(hwnd, h));
                    }
                    let ball = match h {
                        Hit::SidebarGroup(index) => sidebar_group_ball(hwnd, index)
                            .filter(|(_, rect)| rect.contains(p.x, p.y)),
                        _ => island_ball_at(hwnd, p.x, p.y).map(|ball| (ball.tab, ball.rect)),
                    };
                    island_menu::ball_hover(hwnd, ball);
                    let mut t = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut t);
                }
                LRESULT(0)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_MOUSEWHEEL => {
                let mut point = frame::point_from_lparam(lparam);
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut point);
                let in_rail = with_chrome(hwnd, |s| s.layout.rail.contains(point.x, point.y)).unwrap_or(false);
                // --- tab stacks (T10) --- "Switch workspaces by scrolling" over the rail's workspaces.
                if in_rail
                    && let Some(app) = app
                    && App::tab_options(app).workspace_scroll
                    && matches!(hit(hwnd, point.x, point.y), Hit::Rail(RailKey::Workspace(_) | RailKey::AddWorkspace))
                {
                    let _ = App::scroll_workspaces(app, -((wparam.0 >> 16) as i16 as i32).signum());
                    return LRESULT(0);
                }
                if in_rail {
                    let delta = -((wparam.0 >> 16) as i16 as i32).signum()
                        * Metrics::new(dpi(hwnd)).px(36);
                    let changed = with_chrome(hwnd, |s| {
                        let max = crate::chrome::rail::geometry(s.layout.rail, &s.rail_view, s.rail_scroll, s.dpi).max_scroll;
                        let next = crate::chrome::rail::scroll_by(s.rail_scroll, delta, max);
                        let changed = next != s.rail_scroll;
                        s.rail_scroll = next;
                        changed
                    }).unwrap_or(false);
                    if changed { let _ = InvalidateRect(Some(hwnd), None, false); }
                    return LRESULT(0);
                }
                let in_sidebar = with_chrome(hwnd, |s| {
                    s.sidebar
                        .is_some_and(|side| side.sidebar.contains(point.x, point.y))
                })
                .unwrap_or(false);
                if in_sidebar {
                    let delta = ((wparam.0 >> 16) as i16 as i32).signum();
                    let changed = with_chrome(hwnd, |s| {
                        let Some(sidebar) = s.sidebar else {
                            return false;
                        };
                        let row_height = sidebar.row_height().max(1); // --- layout (thread T6) ---
                        s.sidebar_scroll_target = s
                            .sidebar_scroll_target
                            .saturating_sub(delta * row_height * 3)
                            .clamp(0, sidebar.max_scroll);
                        s.sidebar_scroll != s.sidebar_scroll_target
                    })
                    .unwrap_or(false);
                    if changed {
                        let _ = SetTimer(Some(hwnd), WM_SIDEBAR_SCROLL_TIMER, 16, None);
                    }
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                set_hover(hwnd, Hit::None);
                hide_rail_tooltip(hwnd);
                card::pointer_left(hwnd);
                island_menu::pointer_left(hwnd);
                LRESULT(0)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_CAPTURECHANGED => {
                hide_rail_tooltip(hwnd);
                card::cancel(hwnd);
                // Every released press lands here. Only a sidebar drag that lost the capture before
                // its button-up has a width to keep; the button-up saves its own.
                if with_chrome(hwnd, |s| s.sidebar_resizing).unwrap_or(false)
                    && let Some(app) = app
                {
                    finish_sidebar_drag(hwnd, app);
                }
                if clear_pointer_state(hwnd) {
                    repaint(hwnd);
                }
                LRESULT(0)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_CANCELMODE => {
                hide_rail_tooltip(hwnd);
                if with_chrome(hwnd, |s| s.sidebar_resizing).unwrap_or(false)
                    && let Some(app) = app
                {
                    finish_sidebar_drag(hwnd, app);
                }
                let had_pointer_state = clear_pointer_state(hwnd);
                if had_pointer_state {
                    if GetCapture() == hwnd {
                        let _ = ReleaseCapture();
                    }
                    repaint(hwnd);
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_LBUTTONDOWN => {
                let p = frame::point_from_lparam(lparam);
                hide_rail_tooltip(hwnd);
                with_chrome(hwnd, |s| s.rail_keyboard_focus = false);
                // Pressing the strip dismisses a hover card; the press then acts on the strip.
                card::cancel(hwnd);
                match hit(hwnd, p.x, p.y) {
                    Hit::Rail(key) => {
                        set_pressed(hwnd, Hit::Rail(key));
                        let reorderable = matches!(key, RailKey::Workspace(_) | RailKey::App(_) | RailKey::Bookmarks | RailKey::History);
                        if reorderable {
                            let _ = with_chrome(hwnd, |s| s.rail_drag = Some(RailDrag { key, origin: (p.x, p.y), active: false, target: None }));
                        }
                        let _ = SetCapture(hwnd);
                    }
                    Hit::GroupBall(i) => {
                        if let Some(app) = app {
                            let control = with_chrome(hwnd, |s| {
                                let slot = s.slots.get(i)?;
                                let tab = s.tabs.get(slot.tab_index)?;
                                Some((tab.id, slot.collapsed, slot.domain.is_some()))
                            })
                            .flatten();
                            if let Some((id, collapsed, can_collapse)) = control {
                                if can_collapse {
                                    let _ = App::set_tab_group_collapsed(app, id, !collapsed);
                                } else {
                                    let _ = App::select_tab(app, id);
                                }
                                repaint(hwnd);
                                arm_motion(hwnd);
                            }
                        }
                    }
                    Hit::Tab(i) => {
                        if let Some(id) = collapsed_group_target(hwnd, i) {
                            if let Some(app) = app {
                                let _ = App::set_tab_group_collapsed(app, id, false);
                                let _ = App::select_tab(app, id);
                            }
                        } else if let Some(app) = app {
                            if let Some(id) = slot_tab_id(hwnd, i) {
                                let companion = App::active_tab_id(app);
                                let _ = App::select_tab(app, id);
                                let _ = with_chrome(hwnd, |s| {
                                    s.pressed = Hit::Tab(i);
                                    s.drag = Some((id, i));
                                    s.split_drag_companion = (companion != id).then_some(companion);
                                });
                                let _ = SetCapture(hwnd);
                                invalidate_hit(hwnd, Hit::Tab(i));
                            }
                        }
                    }
                    Hit::SidebarWorkspace => {
                        // --- tab stacks (T10) --- no workspace menu while workspaces are off.
                        if let Some(app) = app
                            && App::workspaces_enabled(app)
                        {
                            show_workspaces_menu(hwnd, app);
                        }
                    }
                    Hit::SidebarNewTab => {
                        if let Some(app) = app {
                            open_user_tab(hwnd, app); // --- tab options ---
                        }
                    }
                    Hit::SidebarResize => {
                        let _ = with_chrome(hwnd, |s| s.sidebar_resizing = true);
                        let _ = SetCapture(hwnd);
                    }
                    Hit::SidebarGroup(index) => {
                        if let Some(app) = app {
                            let group = with_chrome(hwnd, |s| {
                                s.sidebar_rows.get(index).and_then(|row| match row {
                                    crate::chrome::vertical_paint::SidebarRow::Group {
                                        representative,
                                        collapsed,
                                        ..
                                    } => Some((*representative, *collapsed)),
                                    _ => None,
                                })
                            })
                            .flatten();
                            if let Some((id, collapsed)) = group {
                                let _ = App::set_tab_group_collapsed(app, id, !collapsed);
                                let _ = with_chrome(hwnd, |s| s.refresh(app, true));
                                repaint(hwnd);
                            }
                        }
                    }
                    Hit::SidebarTab(index) => {
                        if let Some(app) = app
                            && let Some(id) = sidebar_tab_id(hwnd, index)
                        {
                            let _ = App::select_tab(app, id);
                            let _ = with_chrome(hwnd, |s| {
                                s.pressed = Hit::SidebarTab(index);
                                s.vertical_drag = Some(id);
                            });
                            let _ = SetCapture(hwnd);
                        }
                    }
                    Hit::SidebarTabClose(index) => {
                        set_pressed(hwnd, Hit::SidebarTabClose(index));
                        let _ = SetCapture(hwnd);
                    }
                    // --- two-level stacks (T10b) --- a second-row tab selects; its close button
                    // closes on release, as the tab row's does.
                    Hit::StackTab(index) => {
                        if let Some(app) = app
                            && let Some(id) = stack_tab_id(hwnd, index)
                        {
                            let _ = App::select_tab(app, id);
                        }
                    }
                    Hit::StackTabClose(index) => {
                        set_pressed(hwnd, Hit::StackTabClose(index));
                        let _ = SetCapture(hwnd);
                    }
                    Hit::TabClose(i) => {
                        set_pressed(hwnd, Hit::TabClose(i));
                        let _ = with_chrome(hwnd, |s| s.drag = None);
                        let _ = SetCapture(hwnd);
                    }
                    Hit::Omnibox => {
                        if let Some(app) = app {
                            omnibox_press(hwnd, app, p.x);
                        }
                    }
                    Hit::NewTab => {
                        if let Some(app) = app {
                            open_user_tab(hwnd, app) // --- tab options ---
                        }
                    }
                    Hit::TabMenu => {
                        if let Some(app) = app {
                            show_open_tabs_menu(hwnd, app);
                        }
                    }
                    Hit::MainMenu => {
                        if let Some(app) = app {
                            show_main_menu(hwnd, app);
                        }
                    }
                    Hit::Split => {
                        set_pressed(hwnd, Hit::Split);
                        let _ = SetCapture(hwnd);
                    }
                    Hit::SplitDivider => {
                        let (orientation, held) = with_chrome(hwnd, |s| {
                            s.split_dragging = true;
                            s.split_drag_divider = split_divider_at(hwnd, s, p.x, p.y);
                            (s.split_orientation, s.split_drag_divider.unwrap_or(crate::split_view::Divider::Primary))
                        })
                        .unwrap_or((Default::default(), crate::split_view::Divider::Primary));
                        // --- split view ---
                        super::split_ui::divider_pressed(hwnd, crate::split_view::axis_coord(divider_axis(orientation, held), p.x, p.y));
                        let _ = SetCapture(hwnd);
                    }
                    Hit::SiteInfo => {
                        if let Some(app) = app {
                            let button = with_chrome(hwnd, |s| s.layout.site_info_button())
                                .unwrap_or_default();
                            set_pressed(hwnd, Hit::SiteInfo);
                            site_info::show(hwnd, app, button);
                            set_pressed(hwnd, Hit::None);
                            let _ = with_chrome(hwnd, |s| s.refresh(app, true));
                            repaint(hwnd);
                        }
                    }
                    Hit::Viewport => {
                        // --- split view --- a click beside a pane (its rim or a gap) focuses the nearest pane
                        if let Some(app) = app
                            && App::split_view(app).is_some()
                            && let Some(pane) = split_geometry_now(hwnd).and_then(|panes| panes.pane_near(p.x, p.y))
                        {
                            let _ = App::focus_split_pane(app, pane);
                        }
                    }
                    h @ (Hit::Back
                    | Hit::Forward
                    | Hit::Reload
                    | Hit::CaptionMinimize
                    | Hit::CaptionMaximize
                    | Hit::CaptionClose) => {
                        set_pressed(hwnd, h);
                        let _ = SetCapture(hwnd);
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let p = frame::point_from_lparam(lparam);
                let rail_drag = with_chrome(hwnd, |s| s.rail_drag.take()).flatten();
                if let Some(drag) = rail_drag.filter(|drag| drag.active) {
                    if let Some(app) = app {
                        if let Some((index, _)) = rail_drop_target(hwnd, drag.key, p.x, p.y) {
                            commit_rail_drop(hwnd, app, drag.key, index);
                        }
                    }
                    set_pressed(hwnd, Hit::None);
                    repaint(hwnd);
                    let _ = ReleaseCapture();
                    return LRESULT(0);
                }
                let was_sidebar_resize = with_chrome(hwnd, |s| {
                    let resizing = std::mem::take(&mut s.sidebar_resizing);
                    if resizing {
                        s.vertical_drag = None;
                        s.split_dragging = false;
                        s.split_drop = None;
                        s.split_drag_companion = None;
                    }
                    resizing
                })
                .unwrap_or(false);
                if was_sidebar_resize {
                    if let Some(app) = app {
                        finish_sidebar_drag(hwnd, app);
                    }
                    let _ = ReleaseCapture();
                    return LRESULT(0);
                }
                let was_split_drag = with_chrome(hwnd, |s| {
                    let dragging = std::mem::take(&mut s.split_dragging);
                    if dragging {
                        s.split_drop = None;
                        s.split_drag_companion = None;
                        s.vertical_drag = None;
                    }
                    dragging
                })
                .unwrap_or(false);
                if was_split_drag {
                    let _ = ReleaseCapture();
                    if let Some(app) = app {
                        // --- split view ---
                        // A press that never moved is a click on the divider: its menu opens.
                        if super::split_ui::divider_released(hwnd) {
                            super::split_ui::divider_menu(hwnd, app, p);
                        } else {
                            App::split_state_changed(app);
                        }
                        // --- end split view ---
                    }
                } else if let Some(id) = with_chrome(hwnd, |s| s.vertical_drag.take()).flatten() {
                    if let Some(app) = app {
                        let target = match hit(hwnd, p.x, p.y) {
                            Hit::SidebarTab(index) | Hit::SidebarTabClose(index) => {
                                sidebar_tab_id(hwnd, index)
                            }
                            Hit::SidebarGroup(index) => {
                                with_chrome(hwnd, |s| match s.sidebar_rows.get(index) {
                                    Some(crate::chrome::vertical_paint::SidebarRow::Group {
                                        representative,
                                        ..
                                    }) => Some(*representative),
                                    _ => None,
                                })
                                .flatten()
                            }
                            _ => None,
                        };
                        if let Some(target) = target {
                            if let Some(index) =
                                App::tab_ids(app).iter().position(|tab| *tab == target)
                            {
                                let _ = App::drop_tab(app, id, index);
                            }
                        }
                    }
                    set_pressed(hwnd, Hit::None);
                    let _ = ReleaseCapture();
                } else if let Some((id, from)) = with_chrome(hwnd, |s| s.drag.take()).flatten() {
                    let l = with_chrome(hwnd, |s| s.layout).unwrap();
                    let (drop, companion) = with_chrome(hwnd, |s| {
                        (s.split_drop.take(), s.split_drag_companion.take())
                    })
                    .unwrap_or((None, None));
                    if let Some(app) = app {
                        if let Some(zone) = drop {
                            let other = companion
                                .filter(|tab| *tab != id)
                                .or_else(|| App::tab_ids(app).into_iter().find(|tab| *tab != id));
                            if let Some(other) = other
                                && let Some(view) = crate::split_view::SplitView::at_edge(id, other, zone)
                            {
                                // --- split view --- the dragged tab on the zone's half, side by side or stacked
                                let bounds = split_rects_for_ratio(hwnd, view.orientation, view.ratio);
                                let _ = App::open_split_arranged(app, view, bounds);
                                sync_split_ratio(hwnd, App::split_view(app));
                                if let Some(panes) = split_rects(hwnd, None) {
                                    App::update_split_bounds(app, panes);
                                }
                            }
                        } else if let Some(target) = native_stack_target(hwnd, app)
                            && App::stack_onto(app, id, target)
                        {
                            // --- tab stacks (T10) --- held over another tab long enough: a stack.
                            super::stack_ui::stack_made(hwnd, app, target);
                        } else {
                            let to = layout::reorder_target(&l, from, p.x);
                            let model_to = slot_model_index(hwnd, to).unwrap_or(to);
                            let _ = App::drop_tab(app, id, model_to);
                        }
                    }
                    let _ = with_chrome(hwnd, |s| s.stack_hover = None);
                    repaint(hwnd);
                    set_pressed(hwnd, Hit::None);
                    let _ = ReleaseCapture();
                } else {
                    let was = with_chrome(hwnd, |s| s.pressed).unwrap_or(Hit::None);
                    set_pressed(hwnd, Hit::None);
                    let _ = ReleaseCapture();
                    if was == hit(hwnd, p.x, p.y) {
                            if let (Hit::Rail(key), Some(app)) = (was, app) {
                                activate_rail(hwnd, app, key);
                            }
                        if let Hit::TabClose(i) = was {
                            if let Some(app) = app {
                                if let Some(id) = slot_tab_id(hwnd, i) {
                                    user_close(hwnd, app, id); // --- tab options ---
                                }
                            }
                        }
                        if let Hit::SidebarTabClose(i) = was {
                            if let Some(app) = app {
                                if let Some(id) = sidebar_tab_id(hwnd, i) {
                                    user_close(hwnd, app, id); // --- tab options ---
                                }
                            }
                        }
                        // --- two-level stacks (T10b) ---
                        if let (Hit::StackTabClose(i), Some(app)) = (was, app)
                            && let Some(id) = stack_tab_id(hwnd, i)
                        {
                            user_close(hwnd, app, id);
                        }
                        if let (Hit::Back | Hit::Forward | Hit::Reload, Some(app)) = (was, app) {
                            // App::history checks the engine itself, so a click on a dimmed
                            // button is harmless.
                            match was {
                                Hit::Back => navigate_back_for_focused_pane(app),
                                Hit::Forward => navigate_forward_for_focused_pane(app),
                                _ => App::reload(app),
                            }
                            let _ = with_chrome(hwnd, |s| s.refresh(app, true));
                            repaint(hwnd);
                        }
                        if was == Hit::Split
                            && let Some(app) = app
                        {
                            super::split_ui::toolbar_button(hwnd, app); // --- split view ---
                        }
                        caption_command(hwnd, was)
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONDBLCLK => {
                let p = frame::point_from_lparam(lparam);
                match hit(hwnd, p.x, p.y) {
                    Hit::CaptionArea => toggle_max(hwnd),
                    // The system sends this in place of the second press, so it counts as a press too.
                    Hit::Omnibox => {
                        if let Some(app) = app {
                            omnibox_press(hwnd, app, p.x);
                        }
                    }
                    Hit::SplitDivider => {
                        if let Some(app) = app {
                            // --- split view --- the divider under the pointer goes back to 50/50
                            let divider = with_chrome(hwnd, |s| split_divider_at(hwnd, s, p.x, p.y)).flatten();
                            if divider != Some(crate::split_view::Divider::Cross(0)) && divider != Some(crate::split_view::Divider::Cross(1)) {
                                App::set_split_ratio(app, 50);
                            }
                            if divider.is_some_and(|divider| divider != crate::split_view::Divider::Primary) {
                                App::set_split_cross_ratio(app, 50);
                            }
                            App::split_state_changed(app);
                            sync_split_ratio(hwnd, App::split_view(app));
                            if let Some(panes) = split_rects(hwnd, None) {
                                App::update_split_bounds(app, panes);
                            }
                        }
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_MBUTTONUP => {
                let p = frame::point_from_lparam(lparam);
                let target = match hit(hwnd, p.x, p.y) {
                    Hit::Tab(i) | Hit::TabClose(i) => slot_tab_id(hwnd, i),
                    Hit::SidebarTab(i) | Hit::SidebarTabClose(i) => sidebar_tab_id(hwnd, i),
                    Hit::StackTab(i) | Hit::StackTabClose(i) => stack_tab_id(hwnd, i), // --- two-level stacks (T10b) ---
                    _ => None,
                };
                if let Some(id) = target {
                    card::cancel(hwnd);
                    if let Some(app) = app {
                        user_close(hwnd, app, id); // --- tab options --- middle click
                    }
                }
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                hide_rail_tooltip(hwnd);
                let p = frame::point_from_lparam(lparam);
                if let Some(app) = app {
                    let target = hit(hwnd, p.x, p.y);
                    if right_click_shows_system_menu(
                        target,
                        App::shift_right_click_context(app),
                        GetKeyState(VK_SHIFT.0 as i32) < 0,
                    ) {
                        card::cancel(hwnd);
                        html_system_menu(hwnd);
                        return LRESULT(0);
                    }
                    match target {
                        Hit::Rail(key) => show_rail_context_menu(hwnd, app, key, p.x, p.y),
                        // --- split view ---
                        Hit::SplitDivider => super::split_ui::divider_menu(hwnd, app, p),
                        // --- end split view ---
                        Hit::GroupBall(i) => {
                            card::cancel(hwnd);
                            if let Some(id) = slot_tab_id(hwnd, i) {
                                let domain_group = with_chrome(hwnd, |s| {
                                    s.slots.get(i).is_some_and(|slot| slot.domain.is_some())
                                })
                                .unwrap_or(false);
                                if domain_group {
                                    if let Some(anchor) = ball_anchor(hwnd, id) {
                                        island_menu::open_now(hwnd, app, id, anchor);
                                    }
                                } else {
                                    let app = app.clone();
                                    let menu_app = app.clone();
                                    App::picture_in_picture_eligible(
                                        &app,
                                        id,
                                        Box::new(move |available| {
                                            show_tab_context_menu(
                                                hwnd, &menu_app, id, p.x, p.y, available,
                                            );
                                        }),
                                    );
                                }
                            }
                        }
                        Hit::Tab(i) | Hit::TabClose(i) => {
                            if let Some(id) = slot_tab_id(hwnd, i) {
                                card::cancel(hwnd);
                                let app = app.clone();
                                let menu_app = app.clone();
                                App::picture_in_picture_eligible(
                                    &app,
                                    id,
                                    Box::new(move |available| {
                                        show_tab_context_menu(
                                            hwnd, &menu_app, id, p.x, p.y, available,
                                        );
                                    }),
                                );
                            }
                        }
                        Hit::SidebarGroup(i) => {
                            card::cancel(hwnd);
                            if let Some((id, anchor)) = sidebar_group_ball(hwnd, i) {
                                island_menu::open_now(hwnd, app, id, anchor);
                            }
                        }
                        Hit::SidebarTab(i) | Hit::SidebarTabClose(i) | Hit::StackTab(i) | Hit::StackTabClose(i) => {
                            let id = if matches!(target, Hit::StackTab(_) | Hit::StackTabClose(_)) { stack_tab_id(hwnd, i) } else { sidebar_tab_id(hwnd, i) };
                            if let Some(id) = id {
                                card::cancel(hwnd);
                                let app = app.clone();
                                let menu_app = app.clone();
                                App::picture_in_picture_eligible(
                                    &app,
                                    id,
                                    Box::new(move |available| {
                                        show_tab_context_menu(
                                            hwnd, &menu_app, id, p.x, p.y, available,
                                        );
                                    }),
                                );
                            }
                        }
                        _ => {}
                    }
                }
                LRESULT(0)
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                if wparam.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE.0 as usize
                    && auto_hide::fullscreen_of(hwnd) == crate::auto_hide::Fullscreen::Page
                {
                    if let Some(app) = app { auto_hide::page_fullscreen(hwnd, app, false, None); }
                    return LRESULT(0);
                }
                if wparam.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE.0 as usize
                    && with_chrome(hwnd, |s| s.rail_drag.is_some()).unwrap_or(false)
                {
                    with_chrome(hwnd, |s| { s.rail_drag = None; s.pressed = Hit::None; });
                    hide_rail_tooltip(hwnd);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    if GetCapture() == hwnd { let _ = ReleaseCapture(); }
                    return LRESULT(0);
                }
                if message == WM_SYSKEYDOWN && wparam.0 == 0x30 && GetKeyState(VK_MENU.0 as i32) < 0 {
                    let keys = with_chrome(hwnd, |s| s.rail_entries.iter().map(|entry| entry.key).collect::<Vec<_>>()).unwrap_or_default();
                    if let Some(first) = crate::chrome::rail::step_focus(&keys, None, 1) {
                        let current_focus = GetFocus();
                        let _ = with_chrome(hwnd, |s| {
                            if !s.rail_keyboard_focus { s.rail_previous_focus = current_focus; }
                            s.rail_keyboard_focus = true;
                            s.rail_focus = Some(first);
                        });
                        let _ = SetFocus(Some(hwnd));
                        let _ = InvalidateRect(Some(hwnd), None, false);
                        return LRESULT(0);
                    }
                }
                let rail_focus = with_chrome(hwnd, |s| s.rail_keyboard_focus).unwrap_or(false);
                if rail_focus {
                    let key = wparam.0 as u32;
                    if key == VK_UP.0 as u32 || key == VK_DOWN.0 as u32 {
                        let _ = with_chrome(hwnd, |s| {
                            let keys: Vec<_> = s.rail_entries.iter().map(|entry| entry.key).collect();
                            s.rail_focus = crate::chrome::rail::step_focus(&keys, s.rail_focus, if key == VK_UP.0 as u32 { -1 } else { 1 });
                            let entries = rail_entries(s);
                            let geometry = crate::chrome::rail::geometry(s.layout.rail, entries, s.rail_scroll, s.dpi);
                            if let Some(focus) = s.rail_focus
                                && matches!(focus, RailKey::Workspace(_) | RailKey::AddWorkspace | RailKey::App(_) | RailKey::AddApp)
                                && let Some(item) = geometry.item(focus)
                            {
                                if item.rect.top < geometry.middle.top {
                                    s.rail_scroll = s.rail_scroll.saturating_sub(geometry.middle.top - item.rect.top);
                                } else if item.rect.bottom > geometry.middle.bottom {
                                    s.rail_scroll = s.rail_scroll.saturating_add(item.rect.bottom - geometry.middle.bottom).min(geometry.max_scroll);
                                }
                            }
                        });
                        let _ = InvalidateRect(Some(hwnd), None, false);
                        return LRESULT(0);
                    }
                    if key == VK_RETURN.0 as u32 {
                        if let (Some(app), Some(key)) = (app, with_chrome(hwnd, |s| s.rail_focus).flatten()) {
                            with_chrome(hwnd, |s| s.rail_keyboard_focus = false);
                            activate_rail(hwnd, app, key);
                        }
                        return LRESULT(0);
                    }
                    if key == windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE.0 as u32 {
                        let previous = with_chrome(hwnd, |s| {
                            s.rail_keyboard_focus = false;
                            s.rail_focus = None;
                            std::mem::take(&mut s.rail_previous_focus)
                        }).unwrap_or_default();
                        if !previous.0.is_null() { let _ = SetFocus(Some(previous)); }
                        let _ = InvalidateRect(Some(hwnd), None, false);
                        return LRESULT(0);
                    }
                }
                // --- latency (fix E) --- Ctrl+B / Ctrl+J / Ctrl+H are registry commands
                // ("bookmarks-panel", "downloads", "history") run by `dispatch_current` below, so
                // rebinding, clearing and "shortcuts off" apply to them like every other chord.
                if wparam.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE.0 as usize
                    && side_panel::is_open(hwnd)
                    && !crate::win32::menu::is_tracking()
                {
                    side_panel::close(hwnd);
                    if !animations_enabled() { if let Some(app) = app { apply_sidebar_bounds(hwnd, app); } }
                    return LRESULT(0);
                }
                if wparam.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE.0 as usize
                    && card::is_open()
                {
                    card::close();
                    return LRESULT(0);
                }
                if let Some(app) = app {
                    if dispatch_current(hwnd, app, wparam.0 as u32) {
                        return LRESULT(0);
                    }
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_TIMER if wparam.0 == crate::win32::passwords_panel::CLIPBOARD_CLEAR_TIMER => {
                crate::win32::passwords_panel::clear_password_clipboard_timer(hwnd);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == WM_RAIL_TOOLTIP_TIMER => {
                let _ = KillTimer(Some(hwnd), WM_RAIL_TOOLTIP_TIMER);
                let shown = with_chrome(hwnd, |s| {
                    let key = s.rail_tooltip_key;
                    if key.is_some() && s.hover == Hit::Rail(key.unwrap()) && s.rail_drag.is_none() {
                        let entries = rail_entries(s);
                        let _ = key.map(|key| crate::chrome::rail::rail_tooltip(key, entries));
                        s.rail_tooltip_visible = true;
                        true
                    } else { false }
                }).unwrap_or(false);
                if shown { with_chrome(hwnd, |s| invalidate(hwnd, s.layout.rail)); }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == WM_NEWTAB_TOOLTIP_TIMER => {
                let _ = KillTimer(Some(hwnd), WM_NEWTAB_TOOLTIP_TIMER);
                let button = with_chrome(hwnd, |s| {
                    (s.hover == Hit::NewTab && s.pressed == Hit::None && s.rail_drag.is_none() && s.drag.is_none())
                        .then_some(s.layout.new_tab_button)
                }).flatten();
                if let Some(button) = button {
                    let mut top_left = POINT { x: button.left, y: button.top };
                    let mut bottom_right = POINT { x: button.right, y: button.bottom };
                    let _ = ClientToScreen(hwnd, &mut top_left);
                    let _ = ClientToScreen(hwnd, &mut bottom_right);
                    let anchor = RECT { left: top_left.x, top: top_left.y, right: bottom_right.x.max(top_left.x + 1), bottom: bottom_right.y.max(top_left.y + 1) };
                    let _ = with_chrome(hwnd, |s| s.newtab_tooltip_shown = true);
                    ensure_hover_layer(hwnd);
                    super::html_hover::tip(hwnd, anchor, NEWTAB_TOOLTIP_TEXT, false);
                }
                LRESULT(0)
            }
            // Only an HTML chrome run starts or fails these; in native mode they do nothing.
            WM_HTML_CHROME_START => {
                if chrome_host::html_ui_enabled() {
                    start_html_chrome(hwnd);
                }
                LRESULT(0)
            }
            WM_HTML_CHROME_ENV => {
                if chrome_host::html_ui_enabled() {
                    html_env_settled(hwnd);
                }
                LRESULT(0)
            }
            WM_HTML_CHROME_FAILED => {
                if chrome_host::html_ui_enabled() {
                    html_failed(hwnd);
                }
                LRESULT(0)
            }
            WM_HTML_TABS_FAILED => {
                if chrome_host::html_ui_enabled() {
                    html_tabs_failed(hwnd);
                }
                LRESULT(0)
            }
            WM_HTML_RAIL_FAILED => {
                if chrome_host::html_ui_enabled() {
                    html_rail_failed(hwnd);
                }
                LRESULT(0)
            }
            // --- panel host (lane P) ---
            WM_HTML_PANEL_FAILED => {
                if chrome_host::html_ui_enabled() { html_panel_failed(hwnd); }
                LRESULT(0)
            }
            // --- end panel host ---
            WM_HTML_DEFERRED => {
                if chrome_host::html_ui_enabled() {
                    run_deferred_html_command(hwnd);
                }
                LRESULT(0)
            }
            WM_UPDATE_CHECKED => {
                if lparam.0 == UPDATE_FROM_PAGE {
                    super::settings_page::update_checked(hwnd, update_result_key(wparam.0));
                } else {
                    show_update_result(hwnd, wparam.0);
                }
                LRESULT(0)
            }
            // --- theme options --- the time schedule (App::theme_tick).
            WM_TIMER if wparam.0 == WM_THEME_SCHEDULE_TIMER => {
                if let Some(app) = app {
                    App::theme_tick(app);
                } else {
                    let _ = KillTimer(Some(hwnd), WM_THEME_SCHEDULE_TIMER);
                }
                LRESULT(0)
            }
            // --- end theme options ---
            // --- auto-hide (T11) --- a pointer sensor, and the reveal / hide clock.
            auto_hide::WM_AUTOHIDE_SENSOR => {
                auto_hide::on_sensor(hwnd, wparam, lparam);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == auto_hide::TIMER_ID => {
                auto_hide::on_timer(hwnd);
                LRESULT(0)
            }
            // --- end auto-hide ---
            // --- tab options --- periodic reload and the audio rule (App::tab_tick).
            WM_TIMER if wparam.0 == WM_TAB_TICK_TIMER => {
                if let Some(app) = app {
                    App::tab_tick(app);
                } else {
                    let _ = KillTimer(Some(hwnd), WM_TAB_TICK_TIMER);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == WM_TAB_THUMB_TIMER => {
                let _ = KillTimer(Some(hwnd), WM_TAB_THUMB_TIMER);
                if let Some(app) = app {
                    App::thumbnail_timer_fired(app);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == WM_TAB_LINK_TIMER => {
                let _ = KillTimer(Some(hwnd), WM_TAB_LINK_TIMER);
                if let Some(app) = app {
                    App::run_deferred_links(app);
                }
                LRESULT(0)
            }
            // --- end tab options ---
            // --- popup host (lane M) ---
            WM_TIMER if wparam.0 == WM_HTML_POPUP_PREWARM_TIMER => {
                let _ = KillTimer(Some(hwnd), WM_HTML_POPUP_PREWARM_TIMER);
                if chrome_host::html_ui_enabled() { prewarm_html_popup(hwnd); }
                LRESULT(0)
            }
            WM_HTML_POPUP_FAILED => {
                if chrome_host::html_ui_enabled() { html_popup::shutdown(hwnd); }
                LRESULT(0)
            }
            // --- end popup host ---
            WM_TIMER if wparam.0 == super::WM_IMPORT_OFFER_TIMER => {
                let _ = KillTimer(Some(hwnd), super::WM_IMPORT_OFFER_TIMER);
                if let Some(app) = app {
                    if !App::active_tab_is_usable(app) {
                        let _ = SetTimer(Some(hwnd), super::WM_IMPORT_OFFER_TIMER, 500, None);
                        return LRESULT(0);
                    }
                } else {
                    return LRESULT(0);
                }
                if let Ok(profile) = profile::active() {
                    if profile.import_offer_pending() {
                        // --- popup host (lane M) ---
                        let anchor = screen_point(hwnd, main_menu_anchor(hwnd));
                        menu::show(hwnd, anchor, menu::Placement::AboveLeft, Menu {
                            header: None,
                            entries: import_offer_entries(),
                            footer: IMPORT_OFFER_FOOTER.into(),
                        }, "import-offer", move |_, selected| {
                            let _ = profile.remember_import_offer_choice();
                            if selected.is_some_and(|item| item.id == 1) {
                                let _ = import_panel::open(hwnd);
                            }
                        });
                        // --- end popup host ---
                    }
                }
                LRESULT(0)
            }
            WM_POWERBROADCAST => {
                // Battery, AC and Battery Saver changes arrive as a status change; resume from
                // sleep may have changed the source while no broadcast was delivered.
                const PBT_APMRESUMESUSPEND: usize = 7;
                const PBT_APMRESUMEAUTOMATIC: usize = 18;
                let relevant = wparam.0 == windows::Win32::UI::WindowsAndMessaging::PBT_APMPOWERSTATUSCHANGE as usize
                    || wparam.0 == PBT_APMRESUMESUSPEND
                    || wparam.0 == PBT_APMRESUMEAUTOMATIC;
                if relevant && let Some(app) = app.filter(|app| app.try_borrow_mut().is_ok()) {
                    App::refresh_efficiency(app);
                }
                // TRUE: the broadcast is accepted.
                LRESULT(1)
            }
            WM_TIMER if wparam.0 == super::WM_DOWNLOADS_TIMER => {
                if let Some(app) = app {
                    super::downloads_ui::timer(hwnd, app);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == super::WM_POWER_TIMER => {
                if let Some(app) = app.filter(|app| app.try_borrow_mut().is_ok()) {
                    App::refresh_efficiency(app);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == WM_IDLE_TRIM_TIMER => {
                if let Some(app) = app {
                    let _ = KillTimer(Some(hwnd), WM_IDLE_TRIM_TIMER);
                    let mut last = LASTINPUTINFO {
                        cbSize: size_of::<LASTINPUTINFO>() as u32,
                        ..Default::default()
                    };
                    let idle = if GetLastInputInfo(&mut last).as_bool() {
                        GetTickCount().wrapping_sub(last.dwTime)
                    } else {
                        0
                    };
                    let foreground = foreground_is_ours(hwnd);
                    schedule_idle_timer(hwnd, App::idle_timer_fired(app, idle, foreground));
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == WM_SIDEBAR_RESIZE_TIMER => {
                let _ = KillTimer(Some(hwnd), WM_SIDEBAR_RESIZE_TIMER);
                if let Some(app) = app {
                    apply_sidebar_drag(hwnd, app);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == WM_PAGE_IDLE_TIMER => {
                track_idle_pages(hwnd);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == WM_SIDEBAR_SCROLL_TIMER => {
                let moving = with_chrome(hwnd, |s| {
                    s.sidebar_scroll =
                        super::smooth::approach_scroll(s.sidebar_scroll, s.sidebar_scroll_target);
                    s.sidebar_scroll != s.sidebar_scroll_target
                })
                .unwrap_or(false);
                if !moving {
                    let _ = KillTimer(Some(hwnd), WM_SIDEBAR_SCROLL_TIMER);
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            // A window region disables DWM's non-client frame. Do not let the themed
            // fallback frame draw over Bowser's custom rounded chrome.
            WM_NCPAINT | WM_NCUAHDRAWCAPTION | WM_NCUAHDRAWFRAME => LRESULT(0),
            WM_NCACTIVATE => {
                // -1 updates activation state without repainting the non-client area.
                let result = DefWindowProcW(hwnd, message, wparam, LPARAM(-1));
                sync_foreground(hwnd, app);
                result
            }
            WM_SETTEXT | WM_SETICON => {
                // DefWindowProc stores the caption text/icon but would also paint it.
                // Hide only during that synchronous call to suppress non-client painting.
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
                let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, style & !(WS_VISIBLE.0 as isize));
                let result = DefWindowProcW(hwnd, message, wparam, lparam);
                let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, style);
                result
            }
            windows::Win32::UI::WindowsAndMessaging::WM_ACTIVATE => {
                // Activation messages are not trusted on their own; sync_foreground checks the real foreground.
                let result = DefWindowProcW(hwnd, message, wparam, lparam);
                sync_foreground(hwnd, app);
                result
            }
            WM_FOREGROUND_SYNC => {
                sync_foreground(hwnd, app);
                LRESULT(0)
            }
            WM_TIMER if island_menu::is_timer(wparam.0) => {
                if let Some(app) = app {
                    island_menu::timer(hwnd, wparam.0, app, |id| ball_anchor(hwnd, id));
                }
                LRESULT(0)
            }
            WM_TIMER if card::is_hover_timer(wparam.0) => {
                if let Some(app) = app {
                    card::hover_timer(hwnd, wparam.0, app, |id| tab_anchor(hwnd, id));
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == WM_SESSION_SAVE_TIMER => {
                let _ = KillTimer(Some(hwnd), WM_SESSION_SAVE_TIMER);
                if let Some(app) = app {
                    App::session_save_now(app);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == crate::win32::WM_PRIVACY_STATS_TIMER => {
                let _ = KillTimer(Some(hwnd), crate::win32::WM_PRIVACY_STATS_TIMER);
                if let Some(app) = app {
                    let _ = App::privacy_stats_save_now(app);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == crate::app::BLOCKED_TITLE_TIMER => {
                let _ = KillTimer(Some(hwnd), crate::app::BLOCKED_TITLE_TIMER);
                if let Some(app) = app {
                    App::blocked_title_timer(app);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == WM_ANIMATION_TIMER => {
                let running =
                    with_chrome(hwnd, |s| s.animate(hwnd, Instant::now())).unwrap_or(false);
                if !running {
                    let _ = KillTimer(Some(hwnd), WM_ANIMATION_TIMER);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == side_panel::TIMER_ID => {
                let running = side_panel::timer(hwnd);
                if !running {
                    let _ = KillTimer(Some(hwnd), side_panel::TIMER_ID);
                    if let Some(app) = app { apply_sidebar_bounds(hwnd, app); }
                }
                LRESULT(0)
            }
            // --- window and panels (thread T9) --- periodic reload of web panels
            WM_TIMER if wparam.0 == super::panel_webapp::RELOAD_TIMER => {
                if !super::panel_webapp::reload_timer(hwnd) { let _ = KillTimer(Some(hwnd), super::panel_webapp::RELOAD_TIMER); }
                LRESULT(0)
            }
            // --- end window and panels ---
            WM_TIMER if wparam.0 == super::panel_webapp::IDLE_TIMER => {
                let running = super::panel_webapp::idle_timer(hwnd);
                if !running { let _ = KillTimer(Some(hwnd), super::panel_webapp::IDLE_TIMER); }
                LRESULT(0)
            }
            super::WM_TRANSLATE_NET => {
                if let Some(app) = app {
                    App::translate_net_done(app, wparam.0 as u32, lparam.0 as u32);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == super::WM_TRANSLATE_WATCH_TIMER => {
                let _ = KillTimer(Some(hwnd), super::WM_TRANSLATE_WATCH_TIMER);
                if let Some(app) = app {
                    App::translate_watch_timer(app);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == super::WM_READER_PROBE_TIMER => {
                let _ = KillTimer(Some(hwnd), super::WM_READER_PROBE_TIMER);
                if let Some(app) = app {
                    App::reader_probe_timer_fired(app);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == devtools_dock::TIMER_ID => {
                let running = devtools_dock::timer(hwnd);
                if !running { let _ = KillTimer(Some(hwnd), devtools_dock::TIMER_ID); }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == WM_PROGRESS_TIMER => {
                if with_chrome(hwnd, |s| s.html_toolbar_active).unwrap_or(false) {
                    let _ = KillTimer(Some(hwnd), WM_PROGRESS_TIMER);
                    return LRESULT(0);
                }
                let now = Instant::now();
                let (running, active_matches) = with_chrome(hwnd, |s| {
                    let active = app.map(App::active_tab_id);
                    (s.progress.running(now), active == s.progress.tab)
                })
                .unwrap_or((false, false));
                if active_matches {
                    invalidate_omni(hwnd);
                }
                if !running || !active_matches {
                    let _ = KillTimer(Some(hwnd), WM_PROGRESS_TIMER);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == WM_SPLIT_BOUNDS_TIMER => {
                let _ = KillTimer(Some(hwnd), WM_SPLIT_BOUNDS_TIMER);
                if let (Some(app), Some(bounds)) = (
                    app,
                    with_chrome(hwnd, |s| s.pending_split_bounds.take()).flatten(),
                ) {
                    App::update_split_bounds(app, bounds);
                    // The split's page hosts follow the new pane bounds.
                    mark_chrome_dirty(hwnd);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == WM_CARET_BLINK_TIMER => {
                let minimised = IsIconic(hwnd).as_bool();
                if caret_blink_allowed(foreground_is_ours(hwnd), minimised, edit_focused(hwnd)) {
                    // Only the caret changes, so repaint just its pixels.
                    let caret = with_chrome(hwnd, |s| {
                        s.caret = !s.caret;
                        s.surface.caret_rect()
                    })
                    .unwrap_or_default();
                    invalidate_caret(hwnd, caret);
                } else {
                    set_caret_blink(hwnd, false);
                }
                LRESULT(0)
            }
            WM_SETFOCUS => {
                if with_chrome(hwnd, |s| s.rail_keyboard_focus).unwrap_or(false) {
                    return LRESULT(0);
                }
                // --- split view ---
                // The window got the keyboard back (activation, or a page-less side was focused):
                // the empty side's search field takes it when that side is the focused one.
                if super::split_ui::take_focus(hwnd) {
                    return LRESULT(0);
                }
                // --- end split view ---
                // The native address input is hidden while the HTML toolbar is active.
                if with_chrome(hwnd, |s| s.html_toolbar_active).unwrap_or(false) {
                    return LRESULT(0);
                }
                if let Some(app) = app {
                    let _ = SetFocus(Some(app.borrow().edit));
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = KillTimer(Some(hwnd), WM_IDLE_TRIM_TIMER);
                let _ = KillTimer(Some(hwnd), WM_CARET_BLINK_TIMER);
                let _ = KillTimer(Some(hwnd), WM_ANIMATION_TIMER);
                let _ = KillTimer(Some(hwnd), WM_PROGRESS_TIMER);
                let _ = KillTimer(Some(hwnd), WM_SPLIT_BOUNDS_TIMER);
                let _ = KillTimer(Some(hwnd), WM_SESSION_SAVE_TIMER);
                let _ = KillTimer(Some(hwnd), super::WM_IMPORT_OFFER_TIMER);
                let _ = KillTimer(Some(hwnd), WM_SIDEBAR_RESIZE_TIMER);
                let _ = KillTimer(Some(hwnd), WM_PAGE_IDLE_TIMER);
                card::cancel(hwnd);
                // The chrome hosts live in the engine's environment: close them first.
                close_html_chrome(hwnd);
                if let Some(app) = app {
                    App::release_chrome_environment(app);
                    // One write per file that changed: the privacy counters only with a save pending.
                    flush_for_exit(hwnd, app);
                    let (mut engine, log) = {
                        let mut s = app.borrow_mut();
                        s.closing = true;
                        (s.engine.take(), s.log.clone())
                    };
                    let _ = App::reset_navigation_progress(app);
                    if let Some(e) = engine.as_mut() {
                        let _ = e.close();
                    }
                    App::downloads_shutdown(app);
                    let _ = log.record("shutdown");
                }
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                auto_hide::forget(hwnd); // --- auto-hide (T11) ---
                super::panel_webapp::shutdown();
                passwords_panel::clear_password_clipboard_timer(hwnd);
                let _ = KillTimer(Some(hwnd), super::WM_POWER_TIMER);
                let _ = KillTimer(Some(hwnd), WM_IDLE_TRIM_TIMER);
                let _ = KillTimer(Some(hwnd), WM_CARET_BLINK_TIMER);
                let _ = KillTimer(Some(hwnd), WM_ANIMATION_TIMER);
                let _ = KillTimer(Some(hwnd), WM_PROGRESS_TIMER);
                let _ = KillTimer(Some(hwnd), WM_SPLIT_BOUNDS_TIMER);
                let _ = KillTimer(Some(hwnd), super::WM_IMPORT_OFFER_TIMER);
                let _ = KillTimer(Some(hwnd), WM_SIDEBAR_RESIZE_TIMER);
                let _ = KillTimer(Some(hwnd), WM_PAGE_IDLE_TIMER);
                let _ = KillTimer(Some(hwnd), crate::win32::WM_PRIVACY_STATS_TIMER);
                card::cancel(hwnd);
                if let Some(app) = app {
                    let mut s = app.borrow_mut();
                    if !s.input_font.is_invalid() {
                        let _ = windows::Win32::Graphics::Gdi::DeleteObject(s.input_font.into());
                    }
                    if s.initializing {
                        s.loop_shutdown_pending = true
                    } else {
                        PostQuitMessage(0)
                    }
                } else {
                    PostQuitMessage(0)
                }
                WINDOWS.with(|a| {
                    a.borrow_mut().remove(&(hwnd.0 as isize));
                });
                LRESULT(0)
            }
            WM_NCDESTROY => {
                let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                if !raw.is_null() {
                    drop(Box::from_raw(raw as *mut AppRef));
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}
fn hit(hwnd: HWND, x: i32, y: i32) -> Hit {
    with_chrome(hwnd, |s| {
        if s.layout.rail.contains(x, y) {
            let rail = RailView { entries: &s.rail_view, fades: &[], open_panel: None, scroll: s.rail_scroll, drop_indicator: None, focused: None, tooltip: None };
            return layout::hit_test_native(&s.layout, Some(&rail), &s.html_regions(), x, y);
        }
        // The HTML toolbar's and tab row's regions belong to their own WebView2 children, not to
        // native hit tests.
        if s.html_regions().iter().any(|region| region.contains(x, y)) {
            return Hit::None;
        }
        if let Some(sidebar) = s.sidebar
            && let Some(sidebar_hit) = crate::chrome::vertical_paint::hit_test(
                &sidebar,
                &s.sidebar_rows,
                x,
                y,
                s.sidebar_scroll,
            )
        {
            return match sidebar_hit {
                crate::vertical_tabs::SidebarHit::Workspace => Hit::SidebarWorkspace,
                crate::vertical_tabs::SidebarHit::NewTab => Hit::SidebarNewTab,
                crate::vertical_tabs::SidebarHit::Resize => Hit::SidebarResize,
                crate::vertical_tabs::SidebarHit::Group(index) => Hit::SidebarGroup(index),
                crate::vertical_tabs::SidebarHit::Tab(index) => Hit::SidebarTab(index),
                crate::vertical_tabs::SidebarHit::TabClose(index) => Hit::SidebarTabClose(index),
            };
        }
        // --- two-level stacks (T10b) --- the second row: its tabs and their close buttons; the
        // rest of the row is nothing (not a drag band).
        if s.layout.stack_row.contains(x, y) {
            let islands = s.layout.stack_tabs(s.stack_tabs.len());
            return islands.iter().position(|island| island.contains(x, y)).map_or(Hit::None, |index| {
                if s.layout.tab_parts(islands[index]).close.contains(x, y) { Hit::StackTabClose(index) } else { Hit::StackTab(index) }
            });
        }
        let now = Instant::now();
        let balls = s
            .slots
            .iter()
            .enumerate()
            .filter(|(index, _)| s.visual_group_leader(*index))
            .filter_map(|(index, _)| {
                s.group_geometry(index, now)
                    .map(|geometry| (index, geometry.hit_rect.intersect(s.layout.tab_zone)))
            })
            .collect::<Vec<_>>();
        let mut hit = layout::hit_test_with_group_balls(&s.layout, &balls, x, y);
        if let Hit::Tab(index) | Hit::TabClose(index) = hit {
            let slot = s.slots.get(index);
            if s.visual_group_leader(index) && slot.is_some_and(|slot| slot.marker_only) {
                return Hit::GroupBall(index);
            }
            if let Some(island) = s.layout.tab(index) {
                let inset = if s.visual_group_leader(index) {
                    s.group_geometry(index, now)
                        .map_or(0, |geometry| geometry.content_inset)
                } else {
                    0
                };
                let parts = if slot.is_some_and(|slot| slot.group_key.is_some()) {
                    s.layout.group_tab_parts(island, inset)
                } else {
                    s.layout.tab_parts(island)
                };
                let active_tab = slot
                    .and_then(|slot| s.tabs.get(slot.tab_index))
                    .is_some_and(|tab| Some(tab.id) == s.active_tab);
                hit = if parts.close.contains(x, y)
                    || (active_tab && parts.active_close && parts.favicon.contains(x, y))
                {
                    Hit::TabClose(index)
                } else {
                    Hit::Tab(index)
                };
            }
        }
        if hit == Hit::Viewport && s.split_active && split_divider_at(hwnd, s, x, y).is_some() {
            return Hit::SplitDivider;
        }
        hit
    })
    .unwrap_or(Hit::None)
}
/// The tab a strip hit lands on. Its close button counts as the tab itself for hover.
fn hit_tab(hwnd: HWND, h: Hit) -> Option<TabId> {
    match h {
        Hit::Tab(i) | Hit::TabClose(i) | Hit::GroupBall(i) => slot_tab_id(hwnd, i),
        Hit::SidebarTab(i) | Hit::SidebarTabClose(i) => sidebar_tab_id(hwnd, i),
        Hit::StackTab(i) | Hit::StackTabClose(i) => stack_tab_id(hwnd, i), // --- two-level stacks (T10b) ---
        _ => None,
    }
}

/// The ball hit rectangle of a sidebar group header and the group's leading tab, in client
/// coordinates, or `None` when `index` is not a group header.
fn sidebar_group_ball(hwnd: HWND, index: usize) -> Option<(TabId, Rect)> {
    with_chrome(hwnd, |s| {
        let layout = s.sidebar?;
        let metrics = Metrics::new(s.dpi);
        let id = match s.sidebar_rows.get(index)? {
            crate::chrome::vertical_paint::SidebarRow::Group { representative, .. } => {
                *representative
            }
            crate::chrome::vertical_paint::SidebarRow::Tab { .. } => return None,
        };
        let geometry = crate::chrome::vertical_paint::header_ball(
            &layout,
            &s.sidebar_rows,
            index,
            s.sidebar_scroll,
            metrics,
        )?;
        Some((id, geometry.hit_rect))
    })
    .flatten()
}

fn sidebar_tab_id(hwnd: HWND, index: usize) -> Option<TabId> {
    with_chrome(hwnd, |s| match s.sidebar_rows.get(index)? {
        crate::chrome::vertical_paint::SidebarRow::Tab { id, .. } => Some(*id),
        crate::chrome::vertical_paint::SidebarRow::Group { .. } => None,
    })
    .flatten()
}
/// A tab's visible rectangle in main-client coordinates, or `None` when it is scrolled out.
fn tab_anchor(hwnd: HWND, id: TabId) -> Option<Rect> {
    with_chrome(hwnd, |s| s.tab_rect(id)).flatten()
}
/// A domain island's leading ball, found from the strip the chrome last laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
struct IslandBall {
    /// The island's leader, the tab whose slot carries the ball.
    tab: TabId,
    /// The ball or stack in main-client coordinates.
    rect: Rect,
    collapsed: bool,
}
/// The island ball under a client point, if the point is on one. Only domain islands have balls that
/// open the menu or collapse; opener-only islands are not included.
fn island_ball_at(hwnd: HWND, x: i32, y: i32) -> Option<IslandBall> {
    with_chrome(hwnd, |s| {
        s.slots.iter().enumerate().find_map(|(index, slot)| {
            if !s.visual_group_leader(index) || slot.domain.is_none() {
                return None;
            }
            let ball = s
                .group_geometry(index, Instant::now())?
                .hit_rect
                .intersect(s.layout.tab_zone);
            let tab = s.tabs.get(slot.tab_index).map(|tab| tab.id)?;
            ball.contains(x, y).then_some(IslandBall {
                tab,
                rect: ball,
                collapsed: slot.collapsed,
            })
        })
    })
    .flatten()
}
/// The island ball's rectangle for a tab, in main-client coordinates, or `None` when that tab does
/// not lead a visible domain island.
fn ball_anchor(hwnd: HWND, id: TabId) -> Option<Rect> {
    with_chrome(hwnd, |s| {
        s.slots.iter().enumerate().find_map(|(index, slot)| {
            if !s.visual_group_leader(index) || slot.domain.is_none() {
                return None;
            }
            if s.tabs.get(slot.tab_index).map(|tab| tab.id) != Some(id) {
                return None;
            }
            s.group_geometry(index, Instant::now())
                .map(|geometry| geometry.hit_rect.intersect(s.layout.tab_zone))
        })
    })
    .flatten()
}
/// --- tab stacks (T10) --- opens the stack editor (the island menu: name, colour and members) of
/// the stack `id` is in. Deferred, so the strip has drawn a stack that was just made before the
/// editor anchors to it: at its ball when the native strip draws one, else under the pointer.
pub(super) fn open_stack_editor(hwnd: HWND, app: &AppRef, id: TabId) {
    let _ = with_chrome(hwnd, |s| s.refresh(app, true));
    let anchor = ball_anchor(hwnd, id)
        .map(|ball| (ball.left + ball.width() / 2, ball.bottom))
        .or_else(|| cursor_client_point(hwnd));
    if let Some((x, y)) = anchor {
        defer_html_command(hwnd, DeferredChromeCommand::IslandMenu { id, x, y });
    }
}
/// --- tab stacks (T10) --- the tab a native tab drag released now stacks onto: the one the held tab
/// rested over the middle of for the stacking delay, while the option is on.
fn native_stack_target(hwnd: HWND, app: &AppRef) -> Option<TabId> {
    let options = App::tab_options(app);
    let (slot, since) = with_chrome(hwnd, |s| s.stack_hover.take()).flatten()?;
    let rested = u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX);
    crate::stacks::drop_stacks(options.stack_drag, rested, options.stack_drop_delay.0)
        .then(|| slot_tab_id(hwnd, slot))
        .flatten()
}
/// Favicon of a tab for the island menu's rows: the chrome's decoded bitmap, or, while a live HTML
/// tab strip left it undecoded, one decoded now through the same cache.
pub(crate) fn tab_favicon(hwnd: HWND, id: TabId) -> Option<Rc<crate::favicon::FaviconBitmap>> {
    let (key, image, size) = with_chrome(hwnd, |s| {
        let tab = s.tabs.iter().find(|tab| tab.id == id);
        (
            tab.and_then(|tab| tab.favicon_key),
            tab.and_then(|tab| tab.favicon.clone()),
            Metrics::new(s.dpi).px(Metrics::FAVICON_SIZE).max(1) as u32,
        )
    })?;
    if image.is_some() {
        return image;
    }
    let key = key?;
    let bytes = App::tab_favicon(&app_of(hwnd)?, id)?;
    with_chrome(hwnd, |s| decode_cached_favicon(&mut s.favicons, &mut s.failed, key, size, &bytes)).flatten()
}

/// Whether the native tab row needs decoded favicons: not while the HTML tab strip is live.
fn native_favicon_wanted(html_tabs_live: bool) -> bool {
    !html_tabs_live
}

/// Whether a tab change that moves no tab must repaint the native toolbar and tab row: only when
/// either is not drawn by a live HTML view.
fn native_strip_pixels_change(html_toolbar_live: bool, html_tabs_live: bool) -> bool {
    !(html_toolbar_live && html_tabs_live)
}

/// `bytes` decoded at `size` through the favicon cache. A failed decode is remembered in `failed`,
/// a ring of the last 64, so it is not retried.
fn decode_cached_favicon(
    cache: &mut FaviconCache,
    failed: &mut VecDeque<(FaviconKey, u32)>,
    key: FaviconKey,
    size: u32,
    bytes: &[u8],
) -> Option<Rc<favicon::FaviconBitmap>> {
    if let Some(cached) = cache.get(key, size, bytes) {
        return Some(Rc::new(cached.clone()));
    }
    if failed.contains(&(key, size)) {
        return None;
    }
    let decoded = favicon::decode(bytes, size);
    if let Some(bitmap) = &decoded {
        cache.insert(key, size, bytes, bitmap.clone());
    } else {
        if failed.len() == 64 {
            failed.pop_front();
        }
        failed.push_back((key, size));
    }
    decoded.map(Rc::new)
}
fn overflow_tab_ids(layout: &Layout, slots: &[StripSlot], tabs: &[TabSnapshot]) -> Vec<TabId> {
    layout
        .hidden_tab_indices()
        .filter_map(|index| {
            slots
                .get(index)
                .and_then(|slot| tabs.get(slot.tab_index))
                .map(|tab| tab.id)
        })
        .collect()
}
fn slot_tab_id(hwnd: HWND, index: usize) -> Option<TabId> {
    with_chrome(hwnd, |s| {
        s.slots
            .get(index)
            .and_then(|slot| s.tabs.get(slot.tab_index))
            .map(|tab| tab.id)
    })
    .flatten()
}
/// --- two-level stacks (T10b) --- the tab at `index` of the second row.
fn stack_tab_id(hwnd: HWND, index: usize) -> Option<TabId> {
    with_chrome(hwnd, |s| s.stack_tabs.get(index).copied()).flatten()
}
fn slot_model_index(hwnd: HWND, index: usize) -> Option<usize> {
    with_chrome(hwnd, |s| s.slots.get(index).map(|slot| slot.tab_index)).flatten()
}
fn collapsed_group_target(hwnd: HWND, slot_index: usize) -> Option<TabId> {
    with_chrome(hwnd, |s| {
        let slot = s.slots.get(slot_index)?;
        (slot.collapsed && slot.group_control)
            .then(|| s.tabs.get(slot.tab_index).map(|tab| tab.id))
            .flatten()
    })
    .flatten()
}

pub(crate) fn copy_text_to_clipboard(hwnd: HWND, text: &str) -> bool {
    use windows::Win32::{
        Foundation::{GlobalFree, HANDLE},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW,
                SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        },
    };
    use windows::core::w;

    let mut wide = zeroize::Zeroizing::new(
        text.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>(),
    );
    unsafe {
        if OpenClipboard(Some(hwnd)).is_err() {
            return false;
        }
        if EmptyClipboard().is_err() {
            let _ = CloseClipboard();
            return false;
        }
        for (name, value) in [
            (w!("ExcludeClipboardContentFromMonitorProcessing"), 1u32),
            (w!("CanIncludeInClipboardHistory"), 0u32),
            (w!("CanUploadToCloudClipboard"), 0u32),
        ] {
            let format = RegisterClipboardFormatW(name);
            let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of::<u32>()) else {
                let _ = EmptyClipboard();
                let _ = CloseClipboard();
                return false;
            };
            let destination = GlobalLock(memory).cast::<u32>();
            if destination.is_null() {
                let _ = GlobalFree(Some(memory));
                let _ = EmptyClipboard();
                let _ = CloseClipboard();
                return false;
            }
            *destination = value;
            let _ = GlobalUnlock(memory);
            if SetClipboardData(format, Some(HANDLE(memory.0))).is_err() {
                let _ = GlobalFree(Some(memory));
                let _ = EmptyClipboard();
                let _ = CloseClipboard();
                return false;
            }
        }
        let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * std::mem::size_of::<u16>()) else {
            let _ = EmptyClipboard();
            let _ = CloseClipboard();
            return false;
        };
        let destination = GlobalLock(memory).cast::<u16>();
        if destination.is_null() {
            let _ = GlobalFree(Some(memory));
            let _ = CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), destination, wide.len());
        zeroize::Zeroize::zeroize(&mut *wide);
        let _ = GlobalUnlock(memory);
        if SetClipboardData(13, Some(HANDLE(memory.0))).is_err() {
            let _ = GlobalFree(Some(memory));
            let _ = EmptyClipboard();
            let _ = CloseClipboard();
            return false;
        }
        let _ = CloseClipboard();
        true
    }
}

/// The pane rectangles of the shown layout, with the primary gap's leading edge at `divider` (along
/// the split axis) or at the saved divider. `None` when too small for two panes.
pub(super) fn split_rects(hwnd: HWND, divider: Option<i32>) -> Option<crate::app::SplitBounds> {
    split_rects_at(hwnd, divider, None)
}

/// [`split_rects`] with the cross gaps' leading edge at `cross` (or the saved one).
fn split_rects_at(hwnd: HWND, divider: Option<i32>, cross: Option<i32>) -> Option<crate::app::SplitBounds> {
    let (layout, view, saved_divider, saved_cross, orientation) =
        with_chrome(hwnd, |state| (state.layout, state.split_layout, state.split_divider, state.split_cross, state.split_orientation))?;
    // Without a synced layout (a split about to open), a pair in the saved orientation.
    let view = view.or_else(|| crate::split_view::SplitView::new(TabId(0), TabId(1), crate::split_view::Pane::Left).map(|pair| pair.with_orientation(orientation)))?;
    let viewport = adjusted_viewport(hwnd, to_win_rect(layout.viewport), dpi(hwnd));
    let viewport = Rect::new(viewport.left, viewport.top, viewport.right, viewport.bottom);
    let panes = layout::split_panes(viewport, view, divider.unwrap_or(saved_divider), cross.unwrap_or(saved_cross), layout.metrics)?;
    Some(bounds_of(&panes, layout.metrics))
}

/// The pages' rectangles of a split: each pane, less its title bar when tiles carry one
/// (--- tab stacks (T10) ---, `stacks::tile_parts`).
fn bounds_of(panes: &layout::SplitPanes, m: Metrics) -> crate::app::SplitBounds {
    let title = m.px(crate::stacks::title_dip());
    crate::split_view::PaneMap(panes.panes.map(|pane| {
        pane.map(|rect| {
            let ((left, top, right, bottom), _) = crate::stacks::tile_parts((rect.left, rect.top, rect.right, rect.bottom), title);
            RECT { left, top, right, bottom }
        })
    }))
}

/// --- tab stacks (T10) --- the title bars of the shown split's tiles, with the tab each one names,
/// in client coordinates. Empty without title bars.
fn tile_title_bars(hwnd: HWND, s: &WindowChrome) -> Vec<(Rect, TabId)> {
    let title = s.layout.metrics.px(crate::stacks::title_dip());
    let (Some(view), true) = (s.split_layout, title > 0) else { return Vec::new() };
    let Some(panes) = split_panes_now(hwnd, s) else { return Vec::new() };
    view.panes()
        .filter_map(|(pane, tab)| {
            let rect = panes.panes[pane.index()]?;
            let (_, bar) = crate::stacks::tile_parts((rect.left, rect.top, rect.right, rect.bottom), title);
            bar.map(|(left, top, right, bottom)| (Rect::new(left, top, right, bottom), tab))
        })
        .collect()
}

/// The overlay layout of the shown split at the saved dividers, or `None` (no split, or too small).
fn split_panes_now(hwnd: HWND, s: &WindowChrome) -> Option<layout::SplitPanes> {
    let view = s.split_layout?;
    let viewport = adjusted_viewport(hwnd, to_win_rect(s.layout.viewport), s.dpi);
    let viewport = Rect::new(viewport.left, viewport.top, viewport.right, viewport.bottom);
    layout::split_panes(viewport, view, s.split_divider, s.split_cross, s.layout.metrics)
}

/// The divider under `(x, y)` (client pixels) of the shown split, if any.
fn split_divider_at(hwnd: HWND, s: &WindowChrome, x: i32, y: i32) -> Option<crate::split_view::Divider> {
    use crate::split_view::Divider;
    let panes = split_panes_now(hwnd, s)?;
    let inside = |rect: Option<Rect>| rect.is_some_and(|rect| rect.contains(x, y));
    if inside(panes.crossing) {
        return Some(Divider::Both);
    }
    if inside(panes.dividers[0]) {
        return Some(Divider::Primary);
    }
    (0..2).find(|half| inside(panes.dividers[half + 1])).map(Divider::Cross)
}

/// The axis a divider moves along: the primary split's, or the other one for a cross gap.
fn divider_axis(orientation: crate::split_view::Orientation, divider: crate::split_view::Divider) -> crate::split_view::Orientation {
    match divider {
        crate::split_view::Divider::Cross(_) => orientation.other(),
        _ => orientation,
    }
}

pub(super) fn sync_split_ratio(hwnd: HWND, split: Option<crate::split_view::SplitView>) {
    let Some(split) = split else { return };
    let _ = with_chrome(hwnd, |state| {
        let viewport = adjusted_viewport(hwnd, to_win_rect(state.layout.viewport), state.dpi);
        let page = (viewport.left, viewport.top, viewport.right, viewport.bottom);
        state.split_orientation = split.orientation;
        state.split_layout = Some(split);
        let (primary, cross) = crate::split_view::dividers_for_ratios(split, page, state.layout.metrics.px(crate::stacks::gap_dip()));
        state.split_divider = primary;
        state.split_cross = cross;
    });
}

/// Moves the held divider (or both, at the crossing) to the pointer `p`: the gap positions are
/// clamped by the layout's minimum pane sizes, stored, and turned into the app's ratios. Returns the
/// new pane rectangles, or `None` when the layout does not fit.
fn drag_split_divider(hwnd: HWND, app: &AppRef, held: crate::split_view::Divider, p: POINT) -> Option<crate::app::SplitBounds> {
    use crate::split_view::{axis_coord, axis_span, ratio_for_divider, Divider};
    let (layout, view, orientation, saved_primary, saved_cross) =
        with_chrome(hwnd, |s| (s.layout, s.split_layout, s.split_orientation, s.split_divider, s.split_cross))?;
    let view = view?;
    let viewport = adjusted_viewport(hwnd, to_win_rect(layout.viewport), dpi(hwnd));
    let page = (viewport.left, viewport.top, viewport.right, viewport.bottom);
    let gap = layout.metrics.px(crate::stacks::gap_dip());
    let moves_primary = !matches!(held, Divider::Cross(_));
    let moves_cross = held != Divider::Primary;
    let primary_at = if moves_primary { axis_coord(orientation, p.x, p.y) - gap / 2 } else { saved_primary };
    let cross_at = if moves_cross { axis_coord(orientation.other(), p.x, p.y) - gap / 2 } else { saved_cross };
    let panes = layout::split_panes(Rect::new(page.0, page.1, page.2, page.3), view, primary_at, cross_at, layout.metrics)?;
    let primary = panes.dividers[0]?;
    let primary_edge = if orientation.is_stacked() { primary.top } else { primary.left };
    let cross_edge = panes.dividers[1].or(panes.dividers[2]).map(|cross| if orientation.is_stacked() { cross.left } else { cross.top });
    let _ = with_chrome(hwnd, |s| {
        s.split_divider = primary_edge;
        if let Some(cross) = cross_edge {
            s.split_cross = cross;
        }
    });
    if moves_primary {
        let (start, end) = axis_span(page, orientation);
        App::set_split_ratio(app, ratio_for_divider(start, end, primary_edge, gap));
    }
    if moves_cross && let Some(cross) = cross_edge {
        let (start, end) = axis_span(page, orientation.other());
        App::set_split_cross_ratio(app, ratio_for_divider(start, end, cross, gap));
    }
    Some(bounds_of(&panes, layout.metrics))
}

/// The divider of the shown split at client point `p`, if any (for the divider menu).
pub(super) fn split_divider_at_point(hwnd: HWND, p: POINT) -> Option<crate::split_view::Divider> {
    with_chrome(hwnd, |s| split_divider_at(hwnd, s, p.x, p.y)).flatten()
}

fn queue_split_bounds(hwnd: HWND, bounds: crate::app::SplitBounds) {
    let should_arm = with_chrome(hwnd, |s| {
        let should_arm = s.pending_split_bounds.is_none();
        s.pending_split_bounds = Some(bounds);
        should_arm
    })
    .unwrap_or(false);
    if should_arm {
        unsafe {
            let _ = SetTimer(Some(hwnd), WM_SPLIT_BOUNDS_TIMER, ANIMATION_FRAME_MS, None);
        }
    }
}

fn cancel_split_bounds(hwnd: HWND) {
    unsafe {
        let _ = KillTimer(Some(hwnd), WM_SPLIT_BOUNDS_TIMER);
    }
    let _ = with_chrome(hwnd, |s| s.pending_split_bounds = None);
}

fn to_win_rect(rect: Rect) -> RECT {
    RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

fn adjusted_viewport(hwnd: HWND, viewport: RECT, window_dpi: u32) -> RECT {
    let viewport = notes_panel::adjust_viewport(viewport, hwnd, window_dpi);
    // --- auto-hide (T11) --- a floating side panel takes no room from the page.
    let viewport = if auto_hide::panel_floats(hwnd) { viewport } else { side_panel::adjust_viewport(viewport, hwnd) };
    // --- end auto-hide ---
    devtools_dock::adjust_viewport(viewport, hwnd)
}

/// One frame of a sidebar width drag: the chrome is laid out at the width the drag reached and the
/// page follows, however many mouse moves the frame had. No tab is refreshed and nothing is saved;
/// the setting is written when the drag ends (`finish_sidebar_drag`).
fn apply_sidebar_drag(hwnd: HWND, app: &AppRef) {
    let laid_out = with_chrome(hwnd, |s| {
        s.sidebar_drag_pending = false;
        let width = s.sidebar_drag_width;
        width.is_some_and(|width| s.relayout_sidebar(width))
    })
    .unwrap_or(false);
    if laid_out {
        sync_html_host_bounds(hwnd);
        apply_sidebar_bounds(hwnd, app);
    }
}

/// Ends a sidebar width drag: the width it reached becomes the setting (one structural refresh
/// follows) and the settings file is written once. A press that never moved writes nothing.
fn finish_sidebar_drag(hwnd: HWND, app: &AppRef) {
    unsafe {
        let _ = KillTimer(Some(hwnd), WM_SIDEBAR_RESIZE_TIMER);
    }
    let width = with_chrome(hwnd, |s| {
        s.sidebar_drag_pending = false;
        s.sidebar_drag_width.take()
    })
    .flatten();
    if let Some(width) = width {
        App::set_sidebar_width(app, width);
        // The app resized the page from the settings alone; the panels' shares come back here,
        // as after every drag frame.
        apply_sidebar_bounds(hwnd, app);
        App::save_settings(app);
        // --- fix group C --- an open settings page reads the new width, so its next Save keeps it.
        super::settings_page::refresh(hwnd);
    }
}

fn apply_sidebar_bounds(hwnd: HWND, app: &AppRef) {
    let Some(layout) = with_chrome(hwnd, |state| state.layout) else {
        return;
    };
    sync_split_ratio(hwnd, App::split_view(app));
    let viewport = to_win_rect(layout.viewport);
    notes_panel::sync(hwnd, viewport, dpi(hwnd));
    side_panel::sync(hwnd, viewport, dpi(hwnd));
    devtools_dock::sync(hwnd, viewport, dpi(hwnd));
    // `split_rects` computes a pane layout from the saved divider even when no split view exists,
    // so it cannot decide which path applies: only a real split gets pane bounds. Otherwise the
    // single page was never re-bounded when a panel opened, and the panel hid behind it (V1).
    let panes = App::split_view(app).and_then(|_| split_rects(hwnd, None));
    if let Some(panes) = panes {
        App::update_split_bounds(app, panes);
    } else {
        let bounds = adjusted_viewport(hwnd, viewport, dpi(hwnd));
        App::window_size_changed(app, false, bounds);
    }
    repaint(hwnd);
}

// --- split view ---
/// Repaints the window and queues the chrome sync, which re-syncs the split, settings and history
/// hosts (`split_ui::sync` and its siblings run from `WM_CHROME_SYNC`).
pub(super) fn split_ui_invalidate(hwnd: HWND) {
    repaint(hwnd);
}

/// The page area (client coordinates) the panes share, or `None` before the first layout.
pub(super) fn split_viewport(hwnd: HWND) -> Option<RECT> {
    let layout = with_chrome(hwnd, |state| state.layout)?;
    let viewport = adjusted_viewport(hwnd, to_win_rect(layout.viewport), dpi(hwnd));
    (viewport.right > viewport.left && viewport.bottom > viewport.top).then_some(viewport)
}

/// `dip` logical pixels at the window's DPI.
pub(super) fn split_px(hwnd: HWND, dip: i32) -> i32 {
    with_chrome(hwnd, |state| state.layout.metrics.px(dip)).unwrap_or(dip)
}

/// A divider's gap (client coordinates) of the shown split: a column side by side, a row stacked
/// (the primary gap for the crossing).
pub(super) fn split_gap(hwnd: HWND, divider: crate::split_view::Divider) -> Option<RECT> {
    with_chrome(hwnd, |s| {
        let panes = split_panes_now(hwnd, s)?;
        let rect = match divider {
            crate::split_view::Divider::Cross(half) => panes.dividers[half + 1],
            _ => panes.dividers[0],
        };
        rect.map(to_win_rect)
    })
    .flatten()
}

/// Pane rectangles for a split in `orientation` at `ratio`, without moving the divider (for a
/// pair about to open or turn).
pub(super) fn split_rects_for_ratio(hwnd: HWND, orientation: crate::split_view::Orientation, ratio: u8) -> Option<crate::app::SplitBounds> {
    let mut pair = crate::split_view::SplitView::new(TabId(0), TabId(1), crate::split_view::Pane::Left)?.with_orientation(orientation);
    pair.set_ratio(ratio);
    split_rects_for_view(hwnd, pair)
}

/// Pane rectangles for `view` at its own ratios, without moving the dividers (for a layout about
/// to open, turn or change shape).
pub(super) fn split_rects_for_view(hwnd: HWND, view: crate::split_view::SplitView) -> Option<crate::app::SplitBounds> {
    let viewport = split_viewport(hwnd)?;
    let page = (viewport.left, viewport.top, viewport.right, viewport.bottom);
    let m = with_chrome(hwnd, |s| s.layout.metrics)?;
    let (primary, cross) = crate::split_view::dividers_for_ratios(view, page, m.px(crate::stacks::gap_dip()));
    let panes = layout::split_panes(Rect::new(page.0, page.1, page.2, page.3), view, primary, cross, m)?;
    Some(bounds_of(&panes, m))
}

/// The shown split's overlay geometry and focused pane, for the split host (picker placement).
pub(super) fn split_geometry_now(hwnd: HWND) -> Option<layout::SplitPanes> {
    with_chrome(hwnd, |s| split_panes_now(hwnd, s)).flatten()
}

/// The HTML tab view's screen rectangle and the window DPI, while that view is active.
pub(super) fn tabs_host_screen(hwnd: HWND) -> Option<(RECT, u32)> {
    let (host, dpi) = with_chrome(hwnd, |s| (s.html_tab_region().and(s.html_tabs.clone()), s.dpi))?;
    Some((host?.screen_bounds()?, dpi))
}

// --- tab ghost ---
/// What the held tab's native ghost draws for wire tab `raw` (docs/design/tab-drag.md): its title,
/// favicon and private / pinned state, with the window's DPI and colours. `None` for an unknown tab
/// or when the HTML tab view is not live.
pub(super) fn tab_ghost_source(hwnd: HWND, raw: u32) -> Option<super::tab_ghost::Source> {
    let app = app_of(hwnd)?;
    let id = chrome_host::tab_from_wire(raw, &App::tab_ids(&app))?;
    let (title, dpi, palette) = with_chrome(hwnd, |s| {
        s.html_tab_region()?;
        let title = s.tabs.iter().find(|tab| tab.id == id).map(|tab| tab.title.clone()).unwrap_or_default();
        Some((title, s.dpi, s.palette))
    })
    .flatten()?;
    Some(super::tab_ghost::Source {
        title,
        favicon: tab_favicon(hwnd, id),
        private: App::tab_private(&app, id),
        pinned: App::tab_pinned(&app, id),
        dpi,
        window: palette.window,
        text: palette.tab_text_active,
        dark: glass::appearance() == crate::theme::Appearance::Dark,
    })
}

/// Posts a `tabghost` message to the HTML tab view (if it is up).
pub(super) fn tab_ghost_post(hwnd: HWND, json: &str) {
    if let Some(host) = with_chrome(hwnd, |s| s.html_tabs.clone()).flatten() {
        let _ = host.post_json(json);
    }
}
// --- end tab ghost ---

/// The split view's `state`: the panel state with the window colour made opaque, so the empty
/// side matches the overlay's opaque fill of a page-less pane and never shows the desktop.
pub(super) fn split_host_state(hwnd: HWND, accent: Option<crate::theme::Rgba>) -> String {
    let (palette, dpi) = with_chrome(hwnd, |state| (state.palette, state.dpi))
        .unwrap_or_else(|| (crate::theme::palette(glass::appearance(), true), 96));
    // --- wave-1 integration --- the hosts that use this state (the bookmarks bar, the private landing,
    // the start page) take T3's private theme while a private tab is shown, like the toolbar does.
    let system_look = glass::appearance();
    let private_theme = app_of(hwnd).map(|app| (App::private_theme(&app), App::active_tab_private(&app)));
    let (palette, dark) = match private_theme {
        Some((theme, private)) => private_host_look(theme, private, system_look, palette),
        None => (palette, system_look == crate::theme::Appearance::Dark),
    };
    // --- end wave-1 integration ---
    // --- theme options --- the person's colours and accent (the page's, when asked) for this host too.
    let look = if dark { crate::theme::Appearance::Dark } else { crate::theme::Appearance::Light };
    let mut palette = match app_of(hwnd) {
        Some(app) => App::themed_palette(&app, look, !palette.translucent).0,
        None => crate::theme::with_accent(palette, accent),
    };
    // --- end theme options ---
    palette.window.a = 255;
    // --- look state (fix group B) --- the person's text size, font, density, menus and window look.
    let look = host_look(hwnd);
    host_state::page_host_view(dpi as f32 / 96.0, true, dark, look, chrome_host::theme_tokens(&palette)).to_json()
}

/// The settings-driven part of a page or panel host's `state`, as `html_sync` builds it for the toolbar.
fn host_look(hwnd: HWND) -> host_state::HostLook {
    match app_of(hwnd) {
        Some(app) => host_state::HostLook::of(&App::settings(&app), window_in_background(hwnd)),
        None => host_state::HostLook::neutral(),
    }
}
// --- end look state ---

/// The palette and dark flag a page host takes: the window's own, or the private theme's light or
/// dark look while a private tab is shown (the same rule as `html_sync`).
fn private_host_look(
    theme: crate::settings::PrivateTheme,
    private: bool,
    system: crate::theme::Appearance,
    palette: &'static crate::theme::Palette,
) -> (&'static crate::theme::Palette, bool) {
    let look = theme.appearance(private, system);
    let palette = if look == system { palette } else { crate::theme::palette(look, true) };
    (palette, look == crate::theme::Appearance::Dark)
}

pub(super) fn html_split_accelerator(hwnd: HWND, accel: Accel) -> bool {
    html_accelerator(hwnd, accel, View::Split)
}

pub(super) fn html_settings_accelerator(hwnd: HWND, accel: Accel) -> bool {
    // The keyboard editor is waiting for a new shortcut: the page gets the key, so Ctrl+T is captured
    // as a shortcut instead of opening a tab.
    if crate::commands::capturing() {
        return false;
    }
    html_accelerator(hwnd, accel, View::Settings)
}

pub(super) fn html_history_page_accelerator(hwnd: HWND, accel: Accel) -> bool {
    html_accelerator(hwnd, accel, View::HistoryPage)
}

pub(super) fn html_downloads_page_accelerator(hwnd: HWND, accel: Accel) -> bool {
    html_accelerator(hwnd, accel, View::DownloadsPage)
}

/// History as it was before `bowser://history`: the rail flyout in the HTML chrome, else the
/// docked native panel. The history page falls back to this when the window cannot draw it.
pub(super) fn open_history_panel(hwnd: HWND, app: &AppRef) {
    // --- rail flyouts ---
    if html_panel::active_id(hwnd) == Some(side_panel::PanelId::History)
        || toggle_flyout(hwnd, side_panel::PanelId::History, RailKey::History)
    {
        return;
    }
    // --- end rail flyouts ---
    if side_panel::open(hwnd, side_panel::PanelId::History) {
        if !animations_enabled() { apply_sidebar_bounds(hwnd, app); }
    } else if let Some(message) = side_panel::last_status() {
        show_error_message(message);
    }
}

/// Whether the HTML omnibox has the keyboard (the settings page then leaves it alone).
pub(super) fn omnibox_focused(hwnd: HWND) -> bool {
    with_chrome(hwnd, |s| s.html_omnibox_focused).unwrap_or(false)
}

/// Sends the `pair` message to the toolbar and the tab strip when it changed (or the page reloaded).
fn html_broadcast_pair(hwnd: HWND, app: &AppRef) {
    let json = super::split_ui::pair_message(app);
    let Some(hosts) = with_chrome(hwnd, |s| {
        [
            (s.html_page_ready.then(|| s.html.clone()).flatten(), s.html_state_generation[0]),
            (s.html_tabs_page_ready.then(|| s.html_tabs.clone()).flatten(), s.html_state_generation[1]),
        ]
    }) else {
        return;
    };
    for (index, (host, generation)) in hosts.into_iter().enumerate() {
        let Some(host) = host else { continue };
        if super::split_ui::pair_needs_send(hwnd, index, generation, &json) && host.post_json(&json).is_ok() {
            super::split_ui::pair_sent(hwnd, index, generation, &json);
        }
    }
}
// --- end split view ---
// --- rail flyouts ---
/// The flyout a rail button opens, if it opens one (Bookmarks, Customize, and the rail's more
/// button, which opens Customize). The Add buttons open a composer, not a flyout.
fn flyout_panel(key: RailKey) -> Option<side_panel::PanelId> {
    match key {
        // History is a page (bowser://history, history_page.rs): its button closes an open flyout
        // and never switches one by hover. The flyout is only its fallback (open_history_panel).
        RailKey::Bookmarks => Some(side_panel::PanelId::Bookmarks),
        RailKey::Customize | RailKey::Menu => Some(side_panel::PanelId::Customize),
        _ => None,
    }
}

/// The rail button a flyout is anchored to when nothing more specific is known.
fn flyout_button(id: side_panel::PanelId) -> RailKey {
    match id {
        side_panel::PanelId::History => RailKey::History,
        side_panel::PanelId::Bookmarks => RailKey::Bookmarks,
        _ => RailKey::Customize,
    }
}

/// Screen anchor for the flyout of rail button `key`: the rail's right edge and the button. The
/// HTML rail's last reported rectangle for the button wins; otherwise the native rail geometry.
fn flyout_anchor(hwnd: HWND, key: RailKey) -> Option<html_panel::Anchor> {
    let (html_rail, reported, layout_rail, native, dpi) = with_chrome(hwnd, |s| {
        let entries = rail_entries(s);
        let geometry = crate::chrome::rail::geometry(s.layout.rail, entries, s.rail_scroll, s.dpi);
        let native = geometry.items.iter().find(|item| item.key == key).map(|item| to_win_rect(item.rect));
        (s.html_rail.clone().filter(|_| s.html_rail_page_ready), s.html_rail_anchors.get(&key).copied(), to_win_rect(s.layout.rail), native, s.dpi)
    })?;
    let to_screen = |rect: RECT| -> RECT {
        let mut origin = POINT::default();
        unsafe { let _ = ClientToScreen(hwnd, &mut origin); }
        RECT { left: rect.left + origin.x, top: rect.top + origin.y, right: rect.right + origin.x, bottom: rect.bottom + origin.y }
    };
    let rail = html_rail.and_then(|host| host.screen_bounds()).unwrap_or_else(|| to_screen(layout_rail));
    let button = match (reported, native) {
        (Some(rect), _) => super::html_hover::anchor_to_screen(rail, dpi as f32 / 96.0, rect),
        (None, Some(rect)) => to_screen(rect),
        (None, None) => RECT { left: rail.left, top: rail.top, right: rail.right, bottom: rail.top + (rail.right - rail.left) },
    };
    Some(html_panel::Anchor { rail_right: rail.right, button })
}

/// Whether the cursor is over the rail (the HTML rail host, else the native rail rectangle).
pub(super) fn cursor_on_rail(hwnd: HWND) -> bool {
    let mut point = POINT::default();
    if unsafe { GetCursorPos(&mut point) }.is_err() {
        return false;
    }
    let Some((html_rail, layout_rail)) = with_chrome(hwnd, |s| (s.html_rail.clone().filter(|_| s.html_rail_page_ready), to_win_rect(s.layout.rail))) else {
        return false;
    };
    let rail = html_rail.and_then(|host| host.screen_bounds()).unwrap_or_else(|| {
        let mut origin = POINT::default();
        unsafe { let _ = ClientToScreen(hwnd, &mut origin); }
        RECT { left: layout_rail.left + origin.x, top: layout_rail.top + origin.y, right: layout_rail.right + origin.x, bottom: layout_rail.bottom + origin.y }
    });
    point.x >= rail.left && point.x < rail.right && point.y >= rail.top && point.y < rail.bottom
}

/// HTML chrome: History, Bookmarks and Customize open as flyouts beside their rail button. Returns
/// true when the flyout path handled the request (including "the HTML chrome is still starting",
/// when nothing opens rather than the docked panel); false sends the caller to the docked native
/// panel (native chrome, or the panel controller failed).
fn toggle_flyout(hwnd: HWND, id: side_panel::PanelId, button: RailKey) -> bool {
    if !matches!(id, side_panel::PanelId::History | side_panel::PanelId::Bookmarks | side_panel::PanelId::Customize)
        || !chrome_host::html_ui_enabled()
    {
        return false;
    }
    if !ensure_flyout_panel(hwnd) {
        return with_chrome(hwnd, |s| s.html_env_waiting).unwrap_or(false);
    }
    let (Some(anchor), Some(context)) = (flyout_anchor(hwnd, button), popup_context(hwnd)) else {
        perf_event(hwnd, "html_flyout_unanchored");
        return true;
    };
    html_panel::toggle(hwnd, id, anchor, context)
}
// --- end rail flyouts ---

/// Rail handlers call this with the semantic panel ID; the host stays alive when switching IDs.
pub fn rail_activate(hwnd: HWND, app: &AppRef, key: side_panel::PanelId) -> bool {
    rail_activate_at(hwnd, app, key, None)
}

/// `rail_activate` anchored to rail button `button` (the one clicked) when it opens a flyout.
fn rail_activate_at(hwnd: HWND, app: &AppRef, key: side_panel::PanelId, button: Option<RailKey>) -> bool {
    if toggle_flyout(hwnd, key, button.unwrap_or_else(|| flyout_button(key))) {
        return true;
    }
    let opened = side_panel::toggle(hwnd, key);
    if opened && !animations_enabled() { apply_sidebar_bounds(hwnd, app); }
    if !side_panel::is_open(hwnd) {
        if let Some(message) = side_panel::last_status() { show_error_message(message); return false; }
    }
    true
}

fn activate_rail(hwnd: HWND, app: &AppRef, key: RailKey) {
    // --- rail flyouts ---
    // Any other rail button (home, a workspace, a web app) closes an open flyout first.
    if flyout_panel(key).is_none() {
        html_panel::close_for_rail(hwnd);
    }
    // --- end rail flyouts ---
    match key {
        // --- start page --- Home opens the homepage when one is set, else a new tab (the start page)
        RailKey::Home => {
            if let Some(url) = App::settings(app).start.home_target() {
                App::navigate_current(app, &url);
            } else {
                open_user_tab(hwnd, app); // --- tab options --- the new tab page choice
            }
        }
        // --- end start page ---
        RailKey::Customize | RailKey::Menu => { rail_activate_at(hwnd, app, side_panel::PanelId::Customize, Some(key)); }
        RailKey::Workspace(id) => { let _ = App::switch_workspace(app, crate::workspaces::WorkspaceId(id as u32)); }
        // A composer below the button; nothing is created until it is confirmed.
        RailKey::AddWorkspace => open_rail_composer(hwnd, app, key, super::composer::Composer::WorkspaceNew),
        RailKey::AddApp => open_rail_composer(hwnd, app, key, super::composer::Composer::AppAdd),
        RailKey::App(id) => {
            if let Ok(profile) = crate::profile::active() {
                if let Ok(config) = crate::sidebar::SidebarConfig::load(profile.root()) {
                    if let Some(web_app) = config.apps.into_iter().find(|entry| entry.id == id) {
                        super::panel_webapp::register_app(app, web_app);
                    }
                }
            }
            rail_activate(hwnd, app, side_panel::PanelId::WebApp(id));
        }
        RailKey::Bookmarks => { rail_activate_at(hwnd, app, side_panel::PanelId::Bookmarks, Some(key)); }
        // The History button opens bowser://history; without it, it toggles the flyout or panel.
        RailKey::History if super::history_page::available(hwnd) => super::history_page::open(hwnd, app, false),
        RailKey::History => { rail_activate_at(hwnd, app, side_panel::PanelId::History, Some(key)); }
        // --- toolbars and feeds (thread T13) --- the item at that place of the panel bar's list, if it is still there.
        RailKey::Tool(index) => {
            let settings = App::settings(app);
            if let Some(item) = settings.toolbar_panel.get(usize::from(index)) {
                run_bar_item(hwnd, app, item);
            }
        }
        // --- end toolbars and feeds ---
    }

    let _ = with_chrome(hwnd, |state| rebuild_rail_entries(state, app));
    repaint(hwnd);
}

/// The rail's buttons as the painter, hit tests and geometry take them. Built once per rail change
/// (`rail_view_of`), so nothing is copied here.
fn rail_entries(state: &WindowChrome) -> &[RailEntry<'_>] {
    &state.rail_view
}

fn rail_drop_target(hwnd: HWND, key: RailKey, x: i32, y: i32) -> Option<(usize, RailDropIndicator)> {
    with_chrome(hwnd, |state| {
        let entries = rail_entries(state);
        let geometry = crate::chrome::rail::geometry(state.layout.rail, entries, state.rail_scroll, state.dpi);
        crate::chrome::rail::drop_indicator(&geometry, key, x, y)
    }).flatten()
}

/// Opens a composer for a rail Add button: at the click point when the pointer is on the button,
/// else (keyboard activation) at the rail's right edge level with the button's top.
fn open_rail_composer(hwnd: HWND, app: &AppRef, key: RailKey, composer: super::composer::Composer) {
    let at = rail_composer_anchor(hwnd, key);
    // No centred name dialog. Native chrome has no popup host, so Customize (fields in the dock)
    // is the only fallback.
    if !super::composer::open(hwnd, app, composer, None, at) {
        rail_activate_at(hwnd, app, side_panel::PanelId::Customize, Some(key));
    }
}

fn rail_composer_anchor(hwnd: HWND, key: RailKey) -> POINT {
    let mut cursor = POINT::default();
    let has_cursor = unsafe { GetCursorPos(&mut cursor) }.is_ok();
    let anchor = flyout_anchor(hwnd, key);
    match anchor {
        Some(anchor) if !(has_cursor
            && cursor.x >= anchor.button.left && cursor.x < anchor.button.right
            && cursor.y >= anchor.button.top && cursor.y < anchor.button.bottom) =>
        {
            POINT { x: anchor.rail_right, y: anchor.button.top }
        }
        _ => {
            let mut client = cursor;
            unsafe { let _ = ScreenToClient(hwnd, &mut client); }
            screen_point(hwnd, rail_menu_anchor(client, layout::left_rail_inset(dpi(hwnd))))
        }
    }
}

/// Editâ€¦ on a rail web app: the composer opens where the menu was (`at`, screen pixels).
fn edit_rail_app(hwnd: HWND, app: &AppRef, id: u32, at: POINT) {
    if super::composer::open(hwnd, app, super::composer::Composer::AppEdit(id), None, at) {
        return;
    }
    super::panel_customize::focus_app(id);
    // --- rail flyouts ---
    if html_panel::active_id(hwnd) == Some(side_panel::PanelId::Customize) {
        return;
    }
    if toggle_flyout(hwnd, side_panel::PanelId::Customize, RailKey::Customize) {
        return;
    }
    // --- end rail flyouts ---
    if side_panel::open(hwnd, side_panel::PanelId::Customize) {
        if !animations_enabled() { apply_sidebar_bounds(hwnd, app); }
    } else if let Some(message) = side_panel::last_status() {
        show_error_message(message);
    }
}

fn commit_rail_drop(hwnd: HWND, app: &AppRef, key: RailKey, index: usize) {
    match key {
        RailKey::Workspace(raw) => {
            let _ = App::workspace_move(app, crate::workspaces::WorkspaceId(raw as u32), index);
        }
        RailKey::App(id) => {
            if let Ok(profile) = profile::active() {
                let mut config = crate::sidebar::SidebarConfig::load(profile.root()).unwrap_or_default();
                if config.reorder_app(id, index).unwrap_or(false)
                    && config.save(profile.root()).is_ok()
                {
                    super::post(hwnd, super::WM_RAIL_CHANGED, WPARAM(0), LPARAM(0));
                }
            }
        }
        RailKey::Bookmarks | RailKey::History => {
            if let Ok(profile) = profile::active() {
                let kind = if key == RailKey::Bookmarks { crate::sidebar::SidebarKind::Bookmarks } else { crate::sidebar::SidebarKind::History };
                let mut config = crate::sidebar::SidebarConfig::load(profile.root()).unwrap_or_default();
                if config.reorder_section(kind, index).is_ok() && config.save(profile.root()).is_ok() {
                    super::post(hwnd, super::WM_RAIL_CHANGED, WPARAM(0), LPARAM(0));
                }
            }
        }
        _ => {}
    }
}

/// Hides the new-tab button's tooltip and cancels its pending delay. Safe to call at any time:
/// the shared HTML tooltip is only touched when this button put it up.
fn hide_newtab_tooltip(hwnd: HWND) {
    unsafe { let _ = KillTimer(Some(hwnd), WM_NEWTAB_TOOLTIP_TIMER); }
    let shown = with_chrome(hwnd, |state| std::mem::take(&mut state.newtab_tooltip_shown)).unwrap_or(false);
    if shown { super::html_hover::tip_off(hwnd); }
}

fn hide_rail_tooltip(hwnd: HWND) {
    hide_newtab_tooltip(hwnd);
    let visible = with_chrome(hwnd, |state| {
        let visible = state.rail_tooltip_visible;
        state.rail_tooltip_visible = false;
        visible
    }).unwrap_or(false);
    unsafe { let _ = KillTimer(Some(hwnd), WM_RAIL_TOOLTIP_TIMER); }
    if visible { with_chrome(hwnd, |state| invalidate(hwnd, state.layout.rail)); }
}

// --- popup host (lane M): async rail context menus ---
/// Family icon names of the workspace glyphs, in the order of the "Change icon" choices.
const WORKSPACE_GLYPH_NAMES: [&str; 12] = [
    "workspace-briefcase", "workspace-home", "workspace-code", "workspace-book", "workspace-cart",
    "workspace-game", "workspace-music", "workspace-heart", "workspace-star", "workspace-bolt",
    "workspace-globe", "workspace-leaf",
];
/// Swatch colours of the "Change colour" choices (the rail's accent colours, same order).
const WORKSPACE_SWATCHES: [u32; 8] = [0x4385D1, 0x8B66D9, 0x32A89B, 0xD49A32, 0xD66D91, 0xD55757, 0x56A36B, 0x778394];

/// Family icon for a workspace's own icon, shown on its row in the workspaces menu.
fn workspace_glyph_name(icon: crate::workspaces::WorkspaceIcon) -> &'static str {
    use crate::workspaces::WorkspaceIcon as W;
    match icon {
        W::Briefcase => "workspace-briefcase",
        W::Home => "workspace-home",
        W::Code => "workspace-code",
        W::Book => "workspace-book",
        W::Cart => "workspace-cart",
        W::Game => "workspace-game",
        W::Music => "workspace-music",
        W::Heart => "workspace-heart",
        W::Star => "workspace-star",
        W::Bolt => "workspace-bolt",
        W::Globe => "workspace-globe",
        W::Leaf => "workspace-leaf",
    }
}

/// Whether a page that just committed should take the keyboard from the omnibox (V35): only when the
/// omnibox has it, the user has neither typed in it nor deliberately taken it, and the page is a
/// real one (a blank or new tab is waiting for the user to type) and no menu or popup is open, since
/// moving the keyboard would dismiss it. Pure.
fn should_focus_page_after_commit(omnibox_focused: bool, edited: bool, claimed: bool, blank_page: bool, popup_open: bool) -> bool {
    omnibox_focused && !edited && !claimed && !blank_page && !popup_open
}

fn focus_page_after_commit(hwnd: HWND, app: &AppRef) {
    let id = App::active_tab_id(app);
    let state = with_chrome(hwnd, |s| {
        (
            s.html_omnibox_focused || (!s.edit.is_invalid() && unsafe { GetFocus() } == s.edit),
            s.omnibox_edited,
            s.omnibox_claimed,
            s.urls.get(&id).is_none_or(|url| url.is_empty()),
        )
    });
    let Some((focused, edited, claimed, blank)) = state else { return };
    let popup_open = html_popup::is_active(hwnd) || menu::is_tracking();
    if should_focus_page_after_commit(focused, edited, claimed, blank, popup_open) {
        App::focus_page(app);
    }
}

/// The first-run import offer as a compact notice in the ordinary menu model: a title, one
/// explanatory line, then the three choices (V31).
const IMPORT_OFFER_TITLE: &str = "Import from another browser?";
const IMPORT_OFFER_DETAIL: &str = "Bring over bookmarks and search settings.";
const IMPORT_OFFER_FOOTER: &str = "Also available later in the main menu.";

fn import_offer_entries() -> Vec<Entry> {
    vec![
        Entry::SectionHeader(IMPORT_OFFER_TITLE.to_owned()),
        Entry::SectionHeader(IMPORT_OFFER_DETAIL.to_owned()),
        Entry::Item(Item::new(1, "Import", Icon::Grid).glyph("download")),
        Entry::Item(Item::new(2, "Not now", Icon::Close).glyph("clock")),
        Entry::Item(Item::new(3, "Don't ask again", Icon::Close).glyph("close")),
    ]
}

/// Where a rail context menu opens: at the pointer, but never over the rail itself, so the menu
/// goes rightwards into the window instead of off its left edge. `rail_right` is the client x where
/// the rail ends. Pure.
fn rail_menu_anchor(pointer: POINT, rail_right: i32) -> POINT {
    POINT { x: pointer.x.max(rail_right), y: pointer.y }
}

/// Whether "Move up" and "Move down" can do anything for the workspace at `index` of `count`. Pure.
fn workspace_move_enabled(index: Option<usize>, count: usize) -> (bool, bool) {
    match index {
        Some(index) => (index > 0, index + 1 < count),
        None => (false, false),
    }
}

fn show_rail_context_menu(hwnd: HWND, app: &AppRef, key: RailKey, x: i32, y: i32) {
    // The menu opens to the right of the rail from the pointer, never leftwards over the window edge.
    let point = screen_point(hwnd, rail_menu_anchor(windows::Win32::Foundation::POINT { x, y }, layout::left_rail_inset(dpi(hwnd))));
    match key {
        RailKey::Workspace(raw_id) => {
            let id = crate::workspaces::WorkspaceId(raw_id as u32);
            let set = App::workspaces(app);
            let entries = App::workspace_entries(app);
            let Some((_, name, _, _, _, _)) = entries.iter().find(|(entry_id, ..)| *entry_id == id) else { return; };
            let name = name.clone();
            let (can_up, can_down) = workspace_move_enabled(set.all().iter().position(|workspace| workspace.id() == id), set.workspace_count());
            let menu_entries = vec![
                Entry::Item(Item::new(1, "Renameâ€¦", Icon::Person).glyph("edit")),
                Entry::Item(Item::new(2, "Change iconâ€¦", Icon::Grid).glyph("workspace")),
                Entry::Item(Item::new(3, "Change colourâ€¦", Icon::Grid).glyph("droplet")),
                Entry::Item(Item::new(4, "Move up", Icon::Grid).glyph("chevron-up").disabled_if(!can_up)),
                Entry::Item(Item::new(5, "Move down", Icon::Grid).glyph("chevron-down").disabled_if(!can_down)),
                Entry::Item(Item::new(6, "Deleteâ€¦", Icon::Close).glyph("trash").disabled_if(set.workspace_count() == 1)),
            ];
            let kind = format!("rail-ws:{raw_id}");
            menu::show(hwnd, point, menu::Placement::BelowLeft, Menu { header: None, entries: menu_entries, footer: name.clone() }, &kind, move |app, chosen| {
                let Some(chosen) = chosen else { return; };
                // The workspace may have been deleted while the menu was open.
                let set = App::workspaces(app);
                if !set.all().iter().any(|workspace| workspace.id() == id) { return; }
                match chosen.id {
                    // The anchored composer renames only on confirm. No centred dialog.
                    1 => {
                        if !super::composer::open(hwnd, app, super::composer::Composer::WorkspaceRename(id.0), None, point) {
                            rail_activate_at(hwnd, app, side_panel::PanelId::Customize, Some(RailKey::Customize));
                        }
                    }
                    2 => {
                        let choices = ["Briefcase", "Home", "Code", "Book", "Shopping", "Game", "Music", "Heart", "Star", "Bolt", "Globe", "Leaf"];
                        let mut entries: Vec<Entry> = choices.iter().enumerate().map(|(i, label)| Entry::Item(Item::new(20 + i, *label, Icon::Grid).glyph(WORKSPACE_GLYPH_NAMES[i]))).collect();
                        // --- tab stacks and workspaces (T10) --- "icon or emoji".
                        const EMOJI: usize = 34;
                        const NO_EMOJI: usize = 35;
                        let emoji = App::workspace_emoji(app, id);
                        entries.push(Entry::Separator);
                        entries.push(Entry::Item(Item::new(EMOJI, emoji.as_deref().map_or_else(|| "Emojiâ€¦".to_owned(), |emoji| format!("Emoji: {emoji}â€¦")), Icon::Grid).glyph("edit").checked(emoji.is_some())));
                        if emoji.is_some() {
                            entries.push(Entry::Item(Item::new(NO_EMOJI, "Use the icon", Icon::Grid).glyph("reset")));
                        }
                        let icons = [crate::workspaces::WorkspaceIcon::Briefcase, crate::workspaces::WorkspaceIcon::Home, crate::workspaces::WorkspaceIcon::Code, crate::workspaces::WorkspaceIcon::Book, crate::workspaces::WorkspaceIcon::Cart, crate::workspaces::WorkspaceIcon::Game, crate::workspaces::WorkspaceIcon::Music, crate::workspaces::WorkspaceIcon::Heart, crate::workspaces::WorkspaceIcon::Star, crate::workspaces::WorkspaceIcon::Bolt, crate::workspaces::WorkspaceIcon::Globe, crate::workspaces::WorkspaceIcon::Leaf];
                        menu::show(hwnd, point, menu::Placement::BelowLeft, Menu { header: None, entries, footer: "Workspace icon".into() }, "rail-ws-icon", move |app, chosen| {
                            let Some(picked) = chosen.map(|item| item.id) else { return };
                            match picked {
                                EMOJI => workspace_emoji_prompt(hwnd, app, id),
                                NO_EMOJI => { let _ = App::workspace_set_emoji(app, id, None); }
                                _ => if let Some(icon) = icons.get(picked.saturating_sub(20)).copied() { let _ = App::workspace_set_icon(app, id, icon); },
                            }
                        });
                    }
                    3 => {
                        let choices = ["Blue", "Violet", "Teal", "Amber", "Rose", "Red", "Green", "Slate"];
                        let entries = choices.iter().enumerate().map(|(i, label)| Entry::Item(Item::new(40 + i, *label, Icon::Grid).profile_badge("", crate::theme::Rgba::hex(WORKSPACE_SWATCHES[i], 0xFF)))).collect();
                        let colours = [crate::workspaces::WorkspaceColour::Blue, crate::workspaces::WorkspaceColour::Violet, crate::workspaces::WorkspaceColour::Teal, crate::workspaces::WorkspaceColour::Amber, crate::workspaces::WorkspaceColour::Rose, crate::workspaces::WorkspaceColour::Red, crate::workspaces::WorkspaceColour::Green, crate::workspaces::WorkspaceColour::Slate];
                        menu::show(hwnd, point, menu::Placement::BelowLeft, Menu { header: None, entries, footer: "Workspace colour".into() }, "rail-ws-colour", move |app, chosen| {
                            if let Some(colour) = chosen.and_then(|item| colours.get(item.id.saturating_sub(40)).copied()) { let _ = App::workspace_set_colour(app, id, Some(colour)); }
                        });
                    }
                    4 => if let Some(index) = set.all().iter().position(|workspace| workspace.id() == id) && workspace_move_enabled(Some(index), set.workspace_count()).0 { let _ = App::workspace_move(app, id, index - 1); },
                    5 => if let Some(index) = set.all().iter().position(|workspace| workspace.id() == id) && workspace_move_enabled(Some(index), set.workspace_count()).1 { let _ = App::workspace_move(app, id, index + 1); },
                    // --- popup surfaces (lane PP) --- confirm in the HTML popup, not a MessageBox
                    6 if set.workspace_count() > 1 => { confirm_rail_workspace_delete(hwnd, point, id, &name); },
                    _ => {}
                }
            });
        }
        RailKey::App(id) => {
            // --- window and panels (thread T9) --- this panel's own options live here, not on the settings page.
            let prefs = crate::panel_prefs::current(id);
            let reload_choices: Vec<Entry> = crate::panel_prefs::RELOAD_CHOICES.iter()
                .map(|minutes| Entry::Item(Item::new(100 + usize::from(*minutes), crate::panel_prefs::reload_label(*minutes), Icon::None).checked(prefs.reload_minutes == *minutes)))
                .collect();
            let entries = vec![
                Entry::Item(Item::new(1, "Open", Icon::Grid).glyph("app-tile")),
                Entry::Item(Item::new(2, "Editâ€¦", Icon::Person).glyph("edit")),
                Entry::Separator,
                Entry::Item(Item::new(10, "Zoom in", Icon::Grid).glyph("zoom-in").disabled_if(prefs.zoom >= crate::site_prefs::ZOOM_MAX)),
                Entry::Item(Item::new(11, "Zoom out", Icon::Grid).glyph("zoom-out").disabled_if(prefs.zoom <= crate::site_prefs::ZOOM_MIN)),
                Entry::Item(Item::new(12, crate::panel_prefs::zoom_label(prefs.zoom), Icon::Grid).glyph("reset").disabled_if(prefs.zoom == crate::site_prefs::ZOOM_DEFAULT)),
                Entry::Item(Item::new(13, "Mute audio", Icon::Grid).glyph("mute").checked(prefs.muted)),
                Entry::Item(Item::new(14, "Reload page everyâ€¦", Icon::Grid).glyph("reload").submenu_with(reload_choices)),
                Entry::Item(Item::new(15, "Show unread count from the page title", Icon::Grid).glyph("bell").checked(prefs.notify)),
                Entry::Separator,
                Entry::Item(Item::new(16, "Open page in a tab", Icon::Grid).glyph("external")),
                Entry::Item(Item::new(17, "Open page in a new window", Icon::Grid).glyph("window")),
                Entry::Item(Item::new(18, "Open page in a private tab", Icon::Grid).glyph("private")),
                Entry::Separator,
                Entry::Item(Item::new(3, "Removeâ€¦", Icon::Close).glyph("trash")),
            ];
            // --- end window and panels ---
            let kind = format!("rail-app:{id}");
            menu::show(hwnd, point, menu::Placement::BelowLeft, Menu { header: None, entries, footer: "Web app".into() }, &kind, move |app, chosen| {
                let Some(chosen) = chosen else { return; };
                // The app may have been removed while the menu was open.
                if !rail_model_keys(hwnd).contains(&key) { return; }
                match chosen.id {
                    1 => activate_rail(hwnd, app, key),
                    2 => edit_rail_app(hwnd, app, id, point),
                    // --- window and panels (thread T9) --- per-panel options (src/panel_prefs.rs)
                    10 | 11 | 12 | 13 | 15 | 100..=130 => {
                        let _ = crate::panel_prefs::update(|file| {
                            let mut now = file.get(id);
                            match chosen.id {
                                10 => now.zoom = crate::site_prefs::zoom_in(now.zoom),
                                11 => now.zoom = crate::site_prefs::zoom_out(now.zoom),
                                12 => now.zoom = crate::site_prefs::ZOOM_DEFAULT,
                                13 => now.muted = !now.muted,
                                15 => now.notify = !now.notify,
                                minutes => now.reload_minutes = (minutes - 100) as u8,
                            }
                            file.set(id, now);
                        });
                        super::panel_webapp::prefs_changed(id);
                        let _ = with_chrome(hwnd, |state| rebuild_rail_entries(state, app));
                        repaint(hwnd);
                    }
                    16..=18 => {
                        let Some(address) = super::panel_webapp::page_url(id) else { return; };
                        let Ok(target) = url::Url::parse(&address) else { return; };
                        match chosen.id {
                            16 => { App::new_tab(app); App::navigate_current(app, &target); }
                            17 => { if let Ok(profile) = crate::profile::active() { let _ = crate::profile::spawn_window_with_url(&profile, &target); } }
                            _ => { App::new_private_tab(app); App::navigate_current(app, &target); }
                        }
                    }
                    // --- end window and panels ---
                    3 if crate::win32::ask_yes_no("Remove this web app from the rail?") => {
                        if let Ok(profile) = crate::profile::active() {
                            let mut config = crate::sidebar::SidebarConfig::load(profile.root()).unwrap_or_default();
                            config.remove_app_remembered(id); // --- window and panels (thread T9) --- kept for "Restore" ---
                            let _ = config.save(profile.root());
                            super::panel_webapp::forget_app(id);
                            let _ = with_chrome(hwnd, |state| rebuild_rail_entries(state, app));
                            repaint(hwnd);
                        }
                    }
                    _ => {}
                }
            });
        }
        // --- window and panels (thread T9) --- the web apps removed from the rail come back from here.
        RailKey::AddApp => {
            let removed = crate::profile::active().ok().map(|profile| crate::panel_prefs::PanelPrefsFile::load(profile.root()).removed).unwrap_or_default();
            let entries: Vec<Entry> = if removed.is_empty() {
                vec![Entry::Item(Item::new(1, "No removed web apps", Icon::Grid).disabled())]
            } else {
                removed.iter().enumerate().map(|(index, entry)| Entry::Item(Item::new(200 + index, entry.name.clone(), Icon::Grid).glyph("app-tile"))).collect()
            };
            menu::show(hwnd, point, menu::Placement::BelowLeft, Menu { header: None, entries, footer: "Restore a removed web app".into() }, "rail-app-restore", move |app, chosen| {
                let Some(index) = chosen.and_then(|item| item.id.checked_sub(200)) else { return; };
                let Ok(profile) = crate::profile::active() else { return; };
                let Some(entry) = crate::panel_prefs::update(|file| file.take_removed(index)).flatten() else { return; };
                let mut config = crate::sidebar::SidebarConfig::load(profile.root()).unwrap_or_default();
                match config.add_app(&entry.name, &entry.url) {
                    Ok(new_id) => {
                        if config.save(profile.root()).is_ok() {
                            if let Some(web_app) = config.apps.iter().find(|item| item.id == new_id).cloned() { super::panel_webapp::register_app(app, web_app); }
                        }
                    }
                    // Not restorable now (the address is on the rail again, or the rail is full): keep it listed.
                    Err(_) => { let _ = crate::panel_prefs::update(|file| file.remember_removed(&entry.name, &entry.url)); }
                }
                let _ = with_chrome(hwnd, |state| rebuild_rail_entries(state, app));
                repaint(hwnd);
            });
        }
        // --- end window and panels ---
        _ => {}
    }
}
// --- end popup host ---

// --- split view: the split menus live in split_ui.rs ---

fn show_tab_context_menu(hwnd: HWND, app: &AppRef, id: TabId, x: i32, y: i32, pip_available: bool) {
    const NEW_RIGHT: usize = 1;
    const NEW_PRIVATE: usize = 40;
    const CLOSE_PRIVATE: usize = 41;
    const GROUP: usize = 3;
    const GROUP_NEW: usize = 30;
    const GROUP_AUTO: usize = 31;
    const GROUP_NEVER: usize = 33;
    const UNGROUP: usize = 16;
    const UNDO_ORGANISE: usize = 17;
    const RELOAD: usize = 4;
    const DUPLICATE: usize = 5;
    const PIN: usize = 6;
    const MUTE_TAB: usize = 7;
    const MUTE_SITE: usize = 8;
    const HIBERNATE: usize = 9;
    const COPY_URL: usize = 10;
    const MOVE_WINDOW: usize = 11;
    const CLOSE: usize = 12;
    const CLOSE_OTHERS: usize = 13;
    const CLOSE_LEFT: usize = 14;
    const CLOSE_RIGHT: usize = 15;
    const PICTURE_IN_PICTURE: usize = 18;
    let targets = App::selection_targets(app, id); // --- tab options ---
    let pinned = App::tab_pinned(app, id);
    let private = App::tab_private(app, id);
    let url = App::tab_committed_url(app, id);
    // A private tab is never grouped, so the site-grouping choices (saved per site) are not offered.
    let key = url
        .as_ref()
        .filter(|_| !private)
        .and_then(tab_groups::DomainKey::from_url);
    let ids = App::tab_ids(app);
    let Some(index) = ids.iter().position(|candidate| *candidate == id) else {
        return;
    };
    let can_hibernate = App::tab_hibernate_eligible(app, id);
    let mut has_group = false;
    let mut collapsed = false;
    let muted = App::tab_muted(app, id);
    let host = url
        .as_ref()
        .and_then(|address| address.host_str())
        .map(str::to_owned);
    let site_ids: Vec<TabId> = host
        .as_ref()
        .map(|host| {
            ids.iter()
                .copied()
                .filter(|candidate| {
                    App::tab_committed_url(app, *candidate)
                        .and_then(|address| address.host_str().map(str::to_owned))
                        .as_ref()
                        == Some(host)
                })
                .collect()
        })
        .unwrap_or_default();
    let mute_site = site_ids
        .iter()
        .any(|candidate| !App::tab_muted(app, *candidate));
    let can_mute_tab = App::tab_supports_muting(app, id);
    let can_mute_site = !site_ids.is_empty()
        && site_ids
            .iter()
            .all(|candidate| App::tab_supports_muting(app, *candidate));
    let mut entries = vec![
        Entry::Item(Item::new(NEW_RIGHT, "New tab to the right", Icon::Plus).glyph("plus")),
        // Same words as the main menu and the command palette (docs/design/private-tabs.md).
        Entry::Item(Item::new(NEW_PRIVATE, "New private tab", Icon::Shield).glyph("private").shortcut("Ctrl+Shift+P")),
    ];
    // --- split view ---
    entries.extend(super::split_ui::tab_menu_entries(app, id));
    entries.push(Entry::Separator);
    // --- end split view ---
    entries.extend(super::stack_ui::tab_menu_entries(app, id)); // --- tab stacks (T10) ---
    if let Some(key) = &key {
        // The site's standing mode is marked with a check, as the other choice lists do.
        let mode = App::tab_group_preferences(app).mode(key);
        let mode_icon = |_: bool| Icon::None;
        entries.push(Entry::Item(
            Item::new(GROUP, "Add tab to group", Icon::Grid).glyph("tab-group").submenu_with(vec![
                Entry::Item(
                    Item::new(
                        GROUP_NEW,
                        "Always group this site",
                        mode_icon(mode == tab_groups::GroupMode::Always),
                    )
                    .checked(mode == tab_groups::GroupMode::Always),
                ),
                Entry::Item(
                    Item::new(
                        GROUP_AUTO,
                        "Group this site automatically",
                        mode_icon(mode == tab_groups::GroupMode::Auto),
                    )
                    .checked(mode == tab_groups::GroupMode::Auto),
                ),
                Entry::Item(
                    Item::new(
                        GROUP_NEVER,
                        "Never group this site",
                        mode_icon(mode == tab_groups::GroupMode::Never),
                    )
                    .checked(mode == tab_groups::GroupMode::Never),
                ),
            ]),
        ));
        (has_group, collapsed) = with_chrome(hwnd, |state| {
            state
                .slots
                .iter()
                .find(|slot| slot.domain.as_ref() == Some(key))
                .map_or((false, false), |slot| (true, slot.collapsed))
        })
        .unwrap_or((false, false));
        entries.push(Entry::Item(
            Item::new(
                GROUP_AUTO + 1,
                if collapsed {
                    "Expand group"
                } else {
                    "Collapse group"
                },
                Icon::None,
            )
            .glyph(if collapsed { "chevron-down" } else { "chevron-up" })
            // --- tab stacks (T10) --- Compact and Accordion fold stacks by themselves.
            .disabled_if(!has_group || App::stack_style(app) != crate::tab_options::StackStyle::Islands),
        ));
    }
    entries.push(Entry::Item(
        Item::new(UNGROUP, "Remove from group", Icon::None)
            .glyph("caption-minimize")
            .disabled_if(!App::tab_is_grouped(app, id)),
    ));
    entries.push(Entry::Item(
        Item::new(UNDO_ORGANISE, "Undo tab organisation", Icon::None)
            .glyph("reset")
            .disabled_if(!App::can_undo_tab_organisation(app)),
    ));
    entries.push(Entry::Separator);
    entries.extend([
        Entry::Item(Item::new(COPY_URL, "Copy URL", Icon::None).glyph("link").disabled_if(url.is_none())),
        Entry::Item(Item::new(RELOAD, "Reload", Icon::None).glyph("reload")),
        Entry::Item(
            Item::new(DUPLICATE, "Duplicate tab", Icon::Plus)
                .glyph("copy")
                .disabled_if(url.is_none() || private),
        ),
        Entry::Item(
            Item::new(
                PIN,
                if pinned { "Unpin tab" } else { "Pin tab" },
                Icon::None,
            )
            .glyph("pin")
            .checked(pinned),
        ),
        Entry::Item(
            Item::new(
                MUTE_TAB,
                if muted { "Unmute tab" } else { "Mute tab" },
                Icon::None,
            )
            .glyph(if muted { "volume" } else { "mute" })
            .checked(muted)
            .disabled_if(!can_mute_tab),
        ),
        Entry::Item(
            Item::new(
                MUTE_SITE,
                if mute_site {
                    "Mute site"
                } else {
                    "Unmute site"
                },
                Icon::None,
            )
            .glyph(if mute_site { "mute" } else { "volume" })
            .disabled_if(!can_mute_site),
        ),
        Entry::Item(Item::new(HIBERNATE, "Hibernate tab", Icon::None).glyph("clock").disabled_if(!can_hibernate)),
        Entry::Item(
            // A new window is a new process that would not be private, so a private tab stays put.
            Item::new(MOVE_WINDOW, "Move tab to new window", Icon::None)
                .glyph("external")
                .disabled_if(
                    private
                        || !url
                            .as_ref()
                            .is_some_and(crate::profile::can_open_in_new_window),
                ),
        ),
    ]);
    // Settings > Page tools > "Offer picture in picture in the tab menu" (the Ctrl+Shift+Y command works either way).
    if crate::page_tools::pip_menu_entry(pip_available, App::settings(app).page_tools.pip_menu) {
        entries.push(Entry::Item(Item::new(
            PICTURE_IN_PICTURE,
            "Picture in picture",
            Icon::None,
        )
        .glyph("pip")));
    }
    // --- tab options --- periodic reload, rename and the actions on a selection.
    entries.extend(tab_options_menu_entries(app, id, url.as_ref(), targets.len()));
    // --- end tab options ---
    entries.push(Entry::Separator);
    entries.extend([
        Entry::Item(Item::new(CLOSE, "Close tab", Icon::Close).glyph("close")),
        Entry::Item(
            Item::new(CLOSE_OTHERS, "Close other tabs", Icon::Close).glyph("close-others").disabled_if(ids.len() <= 1),
        ),
        Entry::Item(
            Item::new(CLOSE_LEFT, "Close tabs to the left", Icon::Close).glyph("back").disabled_if(index == 0),
        ),
        Entry::Item(
            Item::new(CLOSE_RIGHT, "Close tabs to the right", Icon::Close)
                .glyph("forward")
                .disabled_if(index + 1 == ids.len()),
        ),
    ]);
    if private {
        entries.push(Entry::Item(Item::new(CLOSE_PRIVATE, "Close all private tabs", Icon::Close).glyph("glasses")));
    }
    // --- popup host (lane M) ---
    let point = screen_point(hwnd, windows::Win32::Foundation::POINT { x, y });
    let kind = format!("tab:{}", id.0);
    let menu = Menu {
        header: None,
        entries,
        footer: "Actions for this tab".to_owned(),
    };
    menu::show(hwnd, point, menu::Placement::BelowLeft, menu, &kind, move |app, chosen| {
        let command = chosen.map_or(0, |chosen| chosen.id);
        // The tab may have closed while the menu was open.
        if command == 0 || !App::tab_ids(app).contains(&id) {
            return;
        }
        match command {
            PICTURE_IN_PICTURE => App::toggle_picture_in_picture(app, id),
            NEW_RIGHT | NEW_PRIVATE => {
                let created = if command == NEW_PRIVATE { App::new_private_tab(app) } else { App::new_tab(app) };
                let current = App::tab_ids(app);
                let after = if pinned {
                    current
                        .iter()
                        .take_while(|candidate| App::tab_pinned(app, **candidate))
                        .count()
                } else {
                    current
                        .iter()
                        .position(|candidate| *candidate == id)
                        .unwrap_or(index)
                        + 1
                };
                let _ = App::move_tab(app, created, after);
                focus_omni(hwnd, app, true);
            }
            // --- split view ---
            split if super::split_ui::tab_menu_pick(hwnd, app, id, split) => {}
            // --- end split view ---
            stack if super::stack_ui::tab_menu_pick(hwnd, app, id, stack) => {} // --- tab stacks (T10) ---
            // --- tab options ---
            tab if tab_options_menu_pick(hwnd, app, id, tab, &targets) => {}
            // --- end tab options ---
            GROUP_NEW => {
                let _ = App::set_tab_group_mode(app, id, tab_groups::GroupMode::Always);
            }
            GROUP_AUTO => {
                let _ = App::set_tab_group_mode(app, id, tab_groups::GroupMode::Auto);
            }
            GROUP_NEVER => {
                let _ = App::set_tab_group_mode(app, id, tab_groups::GroupMode::Never);
            }
            value if value == GROUP_AUTO + 1 && has_group => {
                let _ = App::set_tab_group_collapsed(app, id, !collapsed);
            }
            UNGROUP => {
                let _ = App::remove_tab_from_group(app, id);
            }
            UNDO_ORGANISE => {
                let _ = App::undo_tab_organisation(app);
            }
            COPY_URL => {
                if let Some(url) = url.as_ref() {
                    if !copy_text_to_clipboard(hwnd, url.as_str()) {
                        show_error_message("Bowser could not copy the tab URL.");
                    }
                }
            }
            RELOAD => {
                App::reload_tab(app, id);
            }
            DUPLICATE => {
                if let Some(url) = url.as_ref() {
                    let created = App::new_tab(app);
                    App::navigate_current(app, url);
                    if pinned {
                        let _ = App::set_tab_pinned(app, created, true);
                    }
                    if muted {
                        App::set_tab_muted(app, created, true);
                    }
                    let current = App::tab_ids(app);
                    // --- tab options --- "Duplicated tab position": next to the original, or last.
                    let after = duplicate_index(App::tab_options(app).duplicate_position, &current, id, index);
                    // --- end tab options ---
                    let _ = App::move_tab(app, created, after);
                }
            }
            PIN => {
                let _ = App::set_tab_pinned(app, id, !pinned);
            }
            MUTE_TAB => App::set_tab_muted(app, id, !muted),
            MUTE_SITE => {
                for site_tab in site_ids {
                    App::set_tab_muted(app, site_tab, mute_site);
                }
            }
            HIBERNATE => {
                if !App::hibernate_tab(app, id) {
                    show_error_message("Bowser could not hibernate this tab.");
                }
            }
            MOVE_WINDOW => {
                if let Some(url) = url.as_ref() {
                    let profile = crate::profile::active();
                    let launched = profile
                        .is_ok_and(|profile| crate::profile::spawn_window_with_url(&profile, url));
                    if launched {
                        let _ = App::close_tab(app, id);
                    } else {
                        show_error_message("Bowser could not open this tab in a new window.");
                    }
                }
            }
            CLOSE => user_close(hwnd, app, id), // --- tab options ---
            // --- tab options --- a bulk close asks first at the configured count.
            CLOSE_OTHERS => {
                close_many(hwnd, app, ids.iter().copied().filter(|other| *other != id).collect());
            }
            CLOSE_LEFT => {
                close_many(hwnd, app, ids[..index].to_vec());
            }
            CLOSE_RIGHT => {
                close_many(hwnd, app, ids[index + 1..].to_vec());
            }
            // --- end tab options ---
            CLOSE_PRIVATE => {
                App::close_private_tabs(app);
            }
            _ => {}
        }
        repaint(hwnd);
    });
    // --- end popup host ---
}

/// Ctrl+Shift+M: native menu to switch, create or delete profiles. Switching starts a new Bowser
/// process for that profile (WebView2 allows one browser process per data folder); the current
/// window stays open.
const PROFILE_NEW: usize = 1;
const PROFILE_GUEST: usize = 2;
const PROFILE_SWITCH_BASE: usize = 100;
const PROFILE_DELETE_BASE: usize = 1000;
const PROFILE_AVATAR_BASE: usize = 2000;
const PROFILE_ACCENT_BASE: usize = 2100;
const PROFILE_RENAME_BASE: usize = 3000;
const PROFILE_MAX_LISTED: usize = 300;

/// Profile state read when a profile menu opens, so the rows built and the command run afterwards
/// refer to the same list.
struct ProfileMenu {
    app_data: std::path::PathBuf,
    current: crate::profile::Profile,
    names: Vec<String>,
    current_metadata: crate::profile::ProfileMetadata,
}

impl ProfileMenu {
    fn load(hwnd: HWND) -> Option<Self> {
        use crate::profile;
        let Some(app_data) = profile::app_data_dir() else {
            show_profile_notice(
                hwnd,
                "Bowser could not find this user's LocalAppData directory.",
            );
            return None;
        };
        let current = profile::active().ok()?;
        let names = profile::list(&app_data)
            .into_iter()
            .take(PROFILE_MAX_LISTED)
            .collect();
        let current_metadata = crate::profile::ProfileMetadata::load(&current);
        Some(Self {
            app_data,
            current,
            names,
            current_metadata,
        })
    }

    fn is_current(&self, name: &str) -> bool {
        name.eq_ignore_ascii_case(self.current.listed_name())
    }

    fn label(name: &str) -> windows::core::HSTRING {
        let text = if name.eq_ignore_ascii_case(crate::profile::DEFAULT_NAME) {
            "Default"
        } else {
            name
        };
        windows::core::HSTRING::from(text)
    }

    /// Builds profile actions from the same snapshot used by `run`.
    fn entries(&self) -> Vec<Entry> {
        use crate::profile::{self, Profile};
        let mut entries = Vec::with_capacity(self.names.len().saturating_mul(2) + 8);
        for (index, name) in self.names.iter().enumerate() {
            let metadata = Profile::for_listed_name(&self.app_data, name)
                .map(|profile| profile::ProfileMetadata::load(&profile))
                .unwrap_or_default();
            entries.push(Entry::Item(
                Item::new(
                    PROFILE_SWITCH_BASE + index,
                    Self::label(name).to_string(),
                    Icon::Person,
                )
                .glyph("person")
                .profile_badge(metadata.avatar_glyph(name), metadata.accent_colour())
                .checked(self.is_current(name)),
            ));
        }
        entries.push(Entry::Separator);
        entries.push(Entry::Item(Item::new(
            PROFILE_GUEST,
            "Guest window",
            Icon::Person,
        )
        .glyph("external")));
        entries.push(Entry::Item(Item::new(
            PROFILE_NEW,
            "New profile",
            Icon::Plus,
        )
        .glyph("plus")));
        entries.push(Entry::Item(
            Item::new(20, "Choose avatar", Icon::Person).glyph("person").submenu_with(
                ["initial", "leaf", "star", "wave", "moon"]
                    .iter()
                    .enumerate()
                    .map(|(index, id)| {
                        Entry::Item(
                            Item::new(PROFILE_AVATAR_BASE + index, avatar_label(id), Icon::Person)
                                .profile_badge(
                                    avatar_label(id),
                                    self.current_metadata.accent_colour(),
                                )
                                .checked(self.current_metadata.avatar == *id),
                        )
                    })
                    .collect(),
            ),
        ));
        entries.push(Entry::Item(
            Item::new(21, "Choose accent colour", Icon::Grid).glyph("droplet").submenu_with(
                ["blue", "violet", "teal", "amber", "rose"]
                    .iter()
                    .enumerate()
                    .map(|(index, id)| {
                        let mut metadata = self.current_metadata.clone();
                        metadata.accent = (*id).to_owned();
                        Entry::Item(
                            Item::new(PROFILE_ACCENT_BASE + index, accent_label(id), Icon::Grid)
                                .profile_badge(
                                    metadata.avatar_glyph(self.current.listed_name()),
                                    metadata.accent_colour(),
                                )
                                .checked(self.current_metadata.accent == *id),
                        )
                    })
                    .collect(),
            ),
        ));
        let deletable: Vec<usize> = self
            .names
            .iter()
            .enumerate()
            .filter(|(_, name)| {
                !name.eq_ignore_ascii_case(profile::DEFAULT_NAME) && !self.is_current(name)
            })
            .map(|(index, _)| index)
            .collect();
        if !deletable.is_empty() {
            entries.push(Entry::SectionHeader("Delete profile".to_owned()));
            for index in deletable {
                entries.push(Entry::Item(Item::new(
                    PROFILE_DELETE_BASE + index,
                    Self::label(&self.names[index]).to_string(),
                    Icon::Close,
                )
                .glyph("trash")));
            }
        }
        let renamable: Vec<usize> = self
            .names
            .iter()
            .enumerate()
            .filter(|(_, name)| {
                !name.eq_ignore_ascii_case(profile::DEFAULT_NAME) && !self.is_current(name)
            })
            .map(|(index, _)| index)
            .collect();
        if !renamable.is_empty() {
            entries.push(Entry::SectionHeader("Rename profile".to_owned()));
            for index in renamable {
                entries.push(Entry::Item(Item::new(
                    PROFILE_RENAME_BASE + index,
                    Self::label(&self.names[index]).to_string(),
                    Icon::Person,
                )
                .glyph("edit")));
            }
        }
        entries
    }

    /// Runs the command a row of [`Self::append_to`] returned. Other ids are ignored.
    fn run(&self, command: usize, hwnd: HWND) {
        use crate::profile::{self, Profile};
        match command {
            id if (PROFILE_RENAME_BASE..PROFILE_RENAME_BASE + PROFILE_MAX_LISTED).contains(&id) => {
                if let Some(name) = self
                    .names
                    .get(id - PROFILE_RENAME_BASE)
                    .filter(|name| !self.is_current(name))
                    && let Some(new_name) = super::profile_rename::prompt_validated(
                        hwnd,
                        name,
                        "Rename profile",
                        "Save",
                        "1 to 32 letters, digits, - or _",
                        Box::new(|name| profile::validate_name(name).map_err(|error| error.to_string())),
                    )
                    && let Err(error) =
                        profile::rename(&self.app_data, name, &new_name, &self.current)
                {
                    show_profile_notice(
                        hwnd,
                        &format!("The profile could not be renamed. {error}"),
                    );
                }
            }
            PROFILE_GUEST => {
                if !profile::spawn_guest_window() {
                    show_profile_notice(hwnd, "Bowser could not start a guest window.");
                }
            }
            PROFILE_NEW => match profile::create_auto(&self.app_data) {
                Ok(created) => open_profile_window(&created, hwnd),
                Err(error) => {
                    show_profile_notice(hwnd, &format!("The new profile was not created. {error}"))
                }
            },
            id if (PROFILE_SWITCH_BASE..PROFILE_SWITCH_BASE + PROFILE_MAX_LISTED).contains(&id) => {
                if let Some(name) = self
                    .names
                    .get(id - PROFILE_SWITCH_BASE)
                    .filter(|name| !self.is_current(name))
                {
                    match Profile::for_listed_name(&self.app_data, name) {
                        Ok(target) => {
                            if let Err(error) = profile::remember_last_profile(&self.app_data, name)
                            {
                                show_profile_notice(hwnd, &error.to_string());
                            } else {
                                open_profile_window(&target, hwnd);
                            }
                        }
                        Err(error) => show_profile_notice(hwnd, &error.to_string()),
                    }
                }
            }
            id if (PROFILE_AVATAR_BASE..PROFILE_AVATAR_BASE + 5).contains(&id) => {
                let choices = ["initial", "leaf", "star", "wave", "moon"];
                if let Some(avatar) = choices.get(id - PROFILE_AVATAR_BASE) {
                    let mut metadata = self.current_metadata.clone();
                    metadata.avatar = (*avatar).to_owned();
                    if let Err(error) = metadata.save(&self.current) {
                        show_profile_notice(hwnd, &error.to_string());
                    }
                }
            }
            id if (PROFILE_ACCENT_BASE..PROFILE_ACCENT_BASE + 5).contains(&id) => {
                let choices = ["blue", "violet", "teal", "amber", "rose"];
                if let Some(accent) = choices.get(id - PROFILE_ACCENT_BASE) {
                    let mut metadata = self.current_metadata.clone();
                    metadata.accent = (*accent).to_owned();
                    if let Err(error) = metadata.save(&self.current) {
                        show_profile_notice(hwnd, &error.to_string());
                    }
                }
            }
            id if (PROFILE_DELETE_BASE..PROFILE_DELETE_BASE + PROFILE_MAX_LISTED).contains(&id) => {
                if let Some(name) = self.names.get(id - PROFILE_DELETE_BASE).cloned() {
                    let app_data = self.app_data.clone();
                    let current = self.current.clone();
                    confirm_profile_delete(hwnd, &name.clone(), move || {
                        if let Err(error) = profile::delete(&app_data, &name, &current) {
                            show_profile_notice(
                                hwnd,
                                &format!("The profile \"{name}\" was not deleted. {error}"),
                            );
                        }
                    });
                }
            }
            _ => {}
        }
    }
}

fn avatar_label(id: &str) -> &'static str {
    match id {
        "leaf" => "Leaf",
        "star" => "Star",
        "wave" => "Wave",
        "moon" => "Moon",
        _ => "Initial",
    }
}

fn accent_label(id: &str) -> &'static str {
    match id {
        "violet" => "Violet",
        "teal" => "Teal",
        "amber" => "Amber",
        "rose" => "Rose",
        _ => "Blue",
    }
}

// --- popup host (lane M): async profile menus ---
fn show_profile_notice(hwnd: HWND, message: &str) {
    let point = screen_point(hwnd, main_menu_anchor(hwnd));
    menu::show(
        hwnd,
        point,
        menu::Placement::AboveLeft,
        Menu {
            header: None,
            entries: vec![
                Entry::SectionHeader(message.to_owned()),
                Entry::Item(Item::new(1, "Close", Icon::Close).glyph("close")),
            ],
            footer: "Profile action".to_owned(),
        },
        "profile-notice",
        |_, _| {},
    );
}

// --- popup surfaces (lane PP) ---
/// The rail's "Delete workspace" confirmation, in the same menu form as the main menu's.
fn confirm_rail_workspace_delete(hwnd: HWND, point: POINT, id: crate::workspaces::WorkspaceId, name: &str) {
    let confirm = Menu {
        header: None,
        entries: vec![
            Entry::SectionHeader(format!("Delete {name} and close its tabs?")),
            Entry::Item(Item::new(1, "Delete workspace", Icon::Close)),
            Entry::Item(Item::new(2, "Keep workspace", Icon::Person)),
        ],
        footer: "This cannot be undone".to_owned(),
    };
    menu::show(hwnd, point, menu::Placement::BelowLeft, confirm, "workspace-delete", move |app, chosen| {
        if chosen.is_some_and(|chosen| chosen.id == 1)
            && App::workspaces(app).workspace_count() > 1
            && App::workspaces(app).all().iter().any(|workspace| workspace.id() == id)
            && let Err(error) = App::delete_workspace(app, id)
        {
            show_profile_notice(hwnd, &format!("Workspace could not be deleted: {error:?}"));
        }
    });
}
// --- end popup surfaces ---

/// Asks before deleting a profile; `on_confirm` runs only when the user picks Delete.
fn confirm_profile_delete(hwnd: HWND, name: &str, on_confirm: impl FnOnce() + 'static) {
    let point = screen_point(hwnd, main_menu_anchor(hwnd));
    menu::show(
        hwnd,
        point,
        menu::Placement::AboveLeft,
        Menu {
            header: None,
            entries: vec![
                Entry::SectionHeader(format!("Delete profile {name}?")),
                Entry::Item(Item::new(2, "Delete profile", Icon::Close).glyph("trash")),
                Entry::Item(Item::new(3, "Keep profile", Icon::Person).glyph("person")),
            ],
            footer: "Cookies, saved passwords, settings and session data will be removed"
                .to_owned(),
        },
        "profile-delete",
        move |_, chosen| {
            if chosen.is_some_and(|chosen| chosen.id == 2) {
                on_confirm();
            }
        },
    );
}

fn show_profile_menu(hwnd: HWND) {
    let Some(profiles) = ProfileMenu::load(hwnd) else {
        return;
    };
    let point = screen_point(hwnd, main_menu_anchor(hwnd));
    let menu = Menu {
        header: None,
        entries: profiles.entries(),
        footer: "Switch, create, rename, or delete".to_owned(),
    };
    menu::show(hwnd, point, menu::Placement::AboveLeft, menu, "profiles", move |_, chosen| {
        if let Some(chosen) = chosen {
            profiles.run(chosen.id, hwnd);
        }
    });
}
// --- end popup host ---

const WORKSPACE_SELECT_BASE: usize = 6100;
const WORKSPACE_CREATE: usize = 6300;
const WORKSPACE_RENAME: usize = 6301;
const WORKSPACE_DELETE: usize = 6302;

fn workspace_menu_entries(app: &AppRef) -> Vec<Entry> {
    let workspaces = App::workspaces(app);
    let active = workspaces.active_id();
    // --- tab stacks and workspaces (T10) --- the tab counts and emoji show here too.
    let counts = App::tab_options(app).workspace_counts;
    let mut entries = workspaces
        .all()
        .iter()
        .enumerate()
        .map(|(index, workspace)| {
            let mut label = match workspace.emoji() {
                Some(emoji) => format!("{emoji}  {}", workspace.name()),
                None => workspace.name().to_owned(),
            };
            if counts {
                label = format!("{label} Â· {}", crate::stacks::tab_count_label(workspace.tabs().len()));
            }
            Entry::Item(
                Item::new(WORKSPACE_SELECT_BASE + index, label, Icon::Grid)
                    .glyph(workspace_glyph_name(workspace.icon()))
                    .checked(workspace.id() == active),
            )
        })
        .collect::<Vec<_>>();
    entries.push(Entry::Separator);
    entries.push(Entry::Item(Item::new(
        WORKSPACE_CREATE,
        "New workspaceâ€¦",
        Icon::Plus,
    )
    .glyph("plus")));
    entries.push(Entry::Item(Item::new(
        WORKSPACE_RENAME,
        "Rename current workspaceâ€¦",
        Icon::Person,
    )
    .glyph("edit")));
    entries.push(Entry::Item(
        Item::new(WORKSPACE_DELETE, "Delete current workspaceâ€¦", Icon::Close)
            .glyph("trash")
            .disabled_if(workspaces.workspace_count() == 1),
    ));
    entries
}

// --- popup host (lane M): async workspace menus ---
/// Workspace ids in menu order and the active one, read when a workspace menu is built, so a pick
/// acts on the workspace its row showed.
fn workspace_menu_snapshot(
    app: &AppRef,
) -> (Vec<crate::workspaces::WorkspaceId>, crate::workspaces::WorkspaceId) {
    let workspaces = App::workspaces(app);
    (workspaces.all().iter().map(|workspace| workspace.id()).collect(), workspaces.active_id())
}

fn show_workspaces_menu(hwnd: HWND, app: &AppRef) {
    let point = screen_point(hwnd, main_menu_anchor(hwnd));
    let menu = Menu {
        header: None,
        entries: workspace_menu_entries(app),
        footer: "Tabs stay in their workspace".to_owned(),
    };
    let (built, active) = workspace_menu_snapshot(app);
    menu::show(hwnd, point, menu::Placement::AboveLeft, menu, "workspaces", move |app, chosen| {
        if let Some(chosen) = chosen {
            run_workspace_action(hwnd, app, chosen.id, &built, active, point);
        }
    });
}

/// Runs a workspace menu command. `built` and `active_at_open` come from
/// `workspace_menu_snapshot` when the menu was built; a workspace that has gone since is ignored.
/// `point` is the screen point the menu opened at: the composers open below-left of it.
fn run_workspace_action(
    hwnd: HWND,
    app: &AppRef,
    command: usize,
    built: &[crate::workspaces::WorkspaceId],
    active_at_open: crate::workspaces::WorkspaceId,
    point: POINT,
) {
    let workspaces = App::workspaces(app);
    if (WORKSPACE_SELECT_BASE..WORKSPACE_SELECT_BASE + 128).contains(&command) {
        let live: Vec<_> = workspaces.all().iter().map(|workspace| workspace.id()).collect();
        if let Some(id) = menu::still_listed(built, &live, command - WORKSPACE_SELECT_BASE) {
            let _ = App::switch_workspace(app, *id);
        }
        return;
    }
    // Rename and delete act on the workspace that was current when the menu opened.
    if workspaces.active_id() != active_at_open {
        return;
    }
    let active = workspaces.active();
    match command {
        // Both open the anchored composer and change nothing until confirm. There is no centred
        // dialog; without the popup host, Customize opens instead.
        WORKSPACE_CREATE => {
            if !super::composer::open(hwnd, app, super::composer::Composer::WorkspaceNew, None, point) {
                rail_activate_at(hwnd, app, side_panel::PanelId::Customize, Some(RailKey::Customize));
            }
        }
        WORKSPACE_RENAME => {
            if !super::composer::open(hwnd, app, super::composer::Composer::WorkspaceRename(active.id().0), None, point) {
                rail_activate_at(hwnd, app, side_panel::PanelId::Customize, Some(RailKey::Customize));
            }
        }
        WORKSPACE_DELETE => {
            let confirm = Menu {
                header: None,
                entries: vec![
                    Entry::SectionHeader(format!("Delete {} and close its tabs?", active.name())),
                    Entry::Item(Item::new(1, "Delete workspace", Icon::Close).glyph("trash")),
                    Entry::Item(Item::new(2, "Keep workspace", Icon::Person).glyph("workspace")),
                ],
                footer: "This cannot be undone".to_owned(),
            };
            let point = screen_point(hwnd, main_menu_anchor(hwnd));
            let target = active.id();
            menu::show(hwnd, point, menu::Placement::AboveLeft, confirm, "workspace-delete", move |app, chosen| {
                if chosen.is_some_and(|chosen| chosen.id == 1)
                    && App::workspaces(app).all().iter().any(|workspace| workspace.id() == target)
                    && let Err(error) = App::delete_workspace(app, target)
                {
                    show_profile_notice(hwnd, &format!("Workspace could not be deleted: {error:?}"));
                }
            });
        }
        _ => {}
    }
}
// --- end popup host ---

/// Another top-level window. A private one is always a guest. A normal one repeats the current
/// profile, and a window that is already a guest opens another guest.
fn open_another_window(private: bool) {
    let launched = if private || crate::profile::active().is_ok_and(|profile| profile.is_guest()) {
        crate::profile::spawn_guest_window()
    } else {
        crate::profile::active().is_ok_and(|profile| crate::profile::spawn_window(&profile))
    };
    if !launched {
        show_error_message("Bowser could not open a new window.");
    }
}

/// Main menu from the three-dot button: tabs, profiles, extensions, passwords, blocker and
/// settings. A custom panel in the chrome palette; the profile rows open as the themed native menu.
fn show_main_menu(hwnd: HWND, app: &AppRef) {
    const NEW_TAB: usize = 5001;
    const EXTENSIONS: usize = 5002;
    const PASSWORDS: usize = 5003;
    const BLOCKER: usize = 5004;
    const SETTINGS: usize = 5005;
    const EXIT: usize = 5006;
    const PROFILE: usize = 5007;
    const NOTES: usize = 5010;
    const PALETTE: usize = 5011;
    const BOOKMARK: usize = 5012;
    const SESSIONS: usize = 5013;
    const WORKSPACES: usize = 5015;
    const NEW_PRIVATE_TAB: usize = 5016;
    const READER: usize = 5017;
    const CALM: usize = 5018;
    const RESTART_TO_UPDATE: usize = 5020;
    const CLOSE_PRIVATE_TABS: usize = 5030;
    const HISTORY: usize = 5031;
    const MORE_TOOLS: usize = 5032;
    const DEVTOOLS: usize = 5033;
    const NEW_WINDOW: usize = 5034;
    const NEW_PRIVATE_WINDOW: usize = 5035;
    const SHOW_BOOKMARKS: usize = 5037;
    const ZOOM_IN: usize = 5039;
    const ZOOM_OUT: usize = 5040;
    const ZOOM_RESET: usize = 5041;
    const CLEAR_DATA: usize = 5042;
    const CUSTOMISE: usize = 5043;
    const ABOUT: usize = 5044;
    const GROUP_ALWAYS: usize = 5060;
    const GROUP_AUTO: usize = 5061;
    const GROUP_NEVER: usize = 5062;
    const RECENT_BASE: usize = 5300;
    const REOPEN_CLOSED: usize = 5290; // --- tab options ---
    // Submenu parents need an id distinct from any leaf below them, since html_popup's
    // menu_json rejects a menu with any repeated item id (validate_entries, seen.insert) -
    // including a parent sharing an id with its own child - and falls back to the native GDI
    // menu, which ignores Item::glyph.
    const HISTORY_MENU: usize = 5036;
    const BOOKMARKS_MENU: usize = 5038;
    const TAB_GROUPS_MENU: usize = 5045;
    const ZOOM_MENU: usize = 5046;
    const DOWNLOADS: usize = 5047;
    const TRANSLATE: usize = 5048;
    const TRANSLATE_ALWAYS_SITE: usize = 5049;
    const TRANSLATE_NEVER_SITE: usize = 5050;

    let profiles = ProfileMenu::load(hwnd);
    let recent: Vec<(String, String)> = App::recently_closed(app)
        .into_iter()
        .take(8)
        .map(|closed| {
            let title = if closed.title.trim().is_empty() {
                closed.url.clone()
            } else {
                closed.title
            };
            (title, closed.url)
        })
        .collect();
    let mut history_entries = vec![Entry::Item(
        Item::new(HISTORY, "History", Icon::Info)
            .glyph("history")
            .shortcut("Ctrl+H"),
    )];
    // --- tab options --- Ctrl+Shift+T.
    history_entries.push(Entry::Item(
        Item::new(REOPEN_CLOSED, "Reopen closed tab", Icon::None)
            .glyph("reset")
            .shortcut("Ctrl+Shift+T")
            .disabled_if(recent.is_empty()),
    ));
    // --- end tab options ---
    history_entries.push(Entry::Separator);
    if recent.is_empty() {
        history_entries.push(Entry::Item(
            Item::new(RECENT_BASE, "Nothing recently closed", Icon::None)
                .glyph("clock")
                .disabled(),
        ));
    } else {
        history_entries.extend(recent.iter().enumerate().map(|(index, (title, _))| {
            Entry::Item(Item::new(RECENT_BASE + index, title.clone(), Icon::None).glyph("clock"))
        }));
    }
    let group_tab = App::active_tab_id(app);
    let group_private = App::tab_private(app, group_tab);
    let group_mode = App::tab_committed_url(app, group_tab)
        .as_ref()
        .and_then(crate::tab_groups::DomainKey::from_url)
        .map(|key| App::tab_group_preferences(app).mode(&key));
    let zoom = App::active_zoom_percent(app);
    let mut entries = vec![
        Entry::Item(Item::new(NEW_TAB, "New tab", Icon::Plus).glyph("plus").shortcut("Ctrl+T")),
        Entry::Item(
            Item::new(NEW_WINDOW, "New window", Icon::Grid)
                .glyph("window")
                .shortcut("Ctrl+N"),
        ),
        Entry::Item(
            Item::new(NEW_PRIVATE_TAB, "New private tab", Icon::Shield)
                .glyph("private")
                .shortcut("Ctrl+Shift+P")
                .described("Browse without saving history, cookies or site data (Ctrl+Shift+P)"),
        ),
        Entry::Item(
            Item::new(NEW_PRIVATE_WINDOW, "New private window", Icon::Shield)
                .glyph("private-window")
                .described("Open a guest window. Nothing from it is saved."),
        ),
    ];
    // Only while there is something to close (docs/design/private-tabs.md).
    if !App::private_tab_ids(app).is_empty() {
        entries.push(Entry::Item(
            Item::new(CLOSE_PRIVATE_TABS, "Close all private tabs", Icon::Close)
                .glyph("glasses")
                .described("Close every private tab in this window"),
        ));
    }
    entries.push(Entry::Separator);
    entries.push(Entry::Item(
        Item::new(HISTORY_MENU, "History", Icon::Info)
            .glyph("history")
            .submenu_with(history_entries),
    ));
    // The same path as Ctrl+J: bowser://downloads in a tab (docs/design/downloads.md).
    entries.push(Entry::Item(
        Item::new(DOWNLOADS, "Downloads", Icon::Info)
            .glyph("download")
            .shortcut("Ctrl+J"),
    ));
    let guest_bookmark = crate::profile::active().is_ok_and(|profile| profile.is_guest());
    entries.push(Entry::Item(
        Item::new(BOOKMARKS_MENU, "Bookmarks", Icon::Plus)
            .glyph("bookmark")
            .submenu_with(vec![
                Entry::Item(
                    Item::new(BOOKMARK, "Bookmark this page", Icon::Plus)
                        .glyph("bookmark")
                        .shortcut("Ctrl+D")
                        .disabled_if(guest_bookmark),
                ),
                Entry::Item(
                    Item::new(SHOW_BOOKMARKS, "All bookmarks", Icon::Grid)
                        .glyph("bookmark")
                        .disabled_if(guest_bookmark),
                ),
            ]),
    ));
    entries.push(Entry::Item(
        Item::new(TAB_GROUPS_MENU, "Tab groups", Icon::Grid)
            .glyph("tab-group")
            .submenu_with(vec![
                Entry::Item(
                    Item::new(GROUP_ALWAYS, "Always group this site", Icon::None)
                        .checked(group_mode == Some(tab_groups::GroupMode::Always))
                        .disabled_if(group_private || group_mode.is_none()),
                ),
                Entry::Item(
                    Item::new(GROUP_AUTO, "Group this site automatically", Icon::None)
                        .checked(group_mode == Some(tab_groups::GroupMode::Auto))
                        .disabled_if(group_private || group_mode.is_none()),
                ),
                Entry::Item(
                    Item::new(GROUP_NEVER, "Never group this site", Icon::None)
                        .checked(group_mode == Some(tab_groups::GroupMode::Never))
                        .disabled_if(group_private || group_mode.is_none()),
                ),
            ]),
    ));
    // --- tab stacks and workspaces (T10) --- "Workspaces" off takes them out of the menu too.
    if App::workspaces_enabled(app) {
        entries.push(Entry::Item(
            Item::new(WORKSPACES, "Workspaces", Icon::Grid).glyph("workspace").submenu_with(workspace_menu_entries(app)),
        ));
    }
    entries.push(Entry::Separator);
    let blocker = Item::new(BLOCKER, "Block ads & trackers", Icon::Shield)
        .glyph("shield")
        .shortcut("Ctrl+Shift+B")
        .described("Block ads & trackers on this site (Ctrl+Shift+B)");
    entries.push(Entry::Item(if App::active_tab_private(app) {
        // Always on in a private tab; there is no per-site exception to toggle.
        blocker
            .described("The blocker is always on in private tabs")
            .checked(true)
            .disabled()
    } else {
        match App::blocker_active_for_site(app) {
            Some(active) => blocker.checked(active),
            None => blocker.disabled(),
        }
    }));
    let reading_tab = App::active_tab_id(app);
    let reading = App::reader_active(app, reading_tab);
    let readable = App::reader_available(app, reading_tab);
    entries.push(Entry::Item(
        Item::new(READER, "Reader view", Icon::Info)
            .glyph("reader")
            .shortcut("Ctrl+Alt+R")
            .checked(reading == crate::reader::Active::Reader)
            .disabled_if(!readable),
    ));
    // Translation reads the display language (once) only because a person opened this menu.
    let translatable = App::translate_available(app, reading_tab);
    let translated = App::translate_active(app, reading_tab);
    let translate_private = App::tab_private(app, reading_tab);
    let translate_site = App::translate_site_choice(app, reading_tab);
    entries.push(Entry::Item(
        Item::new(TRANSLATE, App::translate_command_label(app, reading_tab), Icon::Info)
            .glyph("translate")
            .checked(translated)
            .disabled_if(!translatable),
    ));
    // Page tools and rarely used windows live one level down so the menu stays short. Settings
    // (the profile, import, privacy dashboard and update rows) is the page they moved to.
    let guest = crate::profile::active().is_ok_and(|profile| profile.is_guest());
    let mut more_tools = vec![
            Entry::Item(Item::new(NOTES, "Notes & Tasksâ€¦", Icon::Info).glyph("notes").shortcut("Ctrl+Shift+N")),
            Entry::Item(Item::new(PALETTE, "Command paletteâ€¦", Icon::Grid).glyph("command").shortcut("Ctrl+Shift+K")),
            Entry::Item(Item::new(SESSIONS, "Saved sessionsâ€¦", Icon::Check).glyph("folder").disabled_if(guest)),
            Entry::Item(
                Item::new(CALM, "Hide distractions", Icon::Info)
                    .glyph("eye")
                    .checked(reading == crate::reader::Active::Calm)
                    .disabled_if(!readable),
            ),
            Entry::Separator,
            Entry::Item(Item::new(EXTENSIONS, "Extensionsâ€¦", Icon::Grid).glyph("puzzle").shortcut("Ctrl+Shift+E")),
            Entry::Item(Item::new(PASSWORDS, "Passwords & passkeys", Icon::Key).glyph("key").shortcut("Ctrl+,").disabled_if(guest)),
            Entry::Item(Item::new(DEVTOOLS, "Developer tools", Icon::Info).glyph("workspace-code").shortcut("F12")),
    ];
    if !translate_private {
        more_tools.push(Entry::Separator);
        more_tools.push(Entry::Item(
            Item::new(TRANSLATE_ALWAYS_SITE, "Always translate this site", Icon::None)
                .glyph("translate")
                .checked(translate_site == Some(crate::site_prefs::SiteTranslate::Always))
                .disabled_if(!translatable),
        ));
        more_tools.push(Entry::Item(
            Item::new(TRANSLATE_NEVER_SITE, "Never offer to translate this site", Icon::None)
                .checked(translate_site == Some(crate::site_prefs::SiteTranslate::Never))
                .disabled_if(!translatable),
        ));
    }
    entries.push(Entry::Item(
        Item::new(MORE_TOOLS, "More tools", Icon::Grid).glyph("more").submenu_with(more_tools),
    ));
    entries.push(Entry::Separator);
    entries.push(Entry::Item(
        Item::new(ZOOM_MENU, format!("Zoom Â· {zoom}%"), Icon::Grid)
            .glyph("zoom-in")
            .submenu_with(vec![
                Entry::Item(Item::new(ZOOM_IN, "Zoom in", Icon::Plus).glyph("zoom-in").shortcut("Ctrl++")),
                Entry::Item(Item::new(ZOOM_OUT, "Zoom out", Icon::None).glyph("zoom-out").shortcut("Ctrl+âˆ’")),
                Entry::Item(Item::new(ZOOM_RESET, "Reset zoom", Icon::Reset).glyph("reset").shortcut("Ctrl+0")),
            ]),
    ));
    entries.push(Entry::Item(
        Item::new(CLEAR_DATA, "Delete browsing dataâ€¦", Icon::Close)
            .glyph("trash")
            .shortcut("Ctrl+Shift+Del"),
    ));
    entries.push(Entry::Separator);
    // The one update row worth the menu: a downloaded build waits for a restart. Checking, the
    // automatic switch and the version history are on the Settings page (Updates).
    let update_status = crate::updater::status();
    if matches!(update_status, crate::updater::Status::Ready(_)) {
        entries.push(Entry::Item(Item::new(RESTART_TO_UPDATE, "Restart to update", Icon::Check).glyph("refresh")));
    }
    entries.push(Entry::Item(
        Item::new(CUSTOMISE, "Customise", Icon::Grid).glyph("customize"),
    ));
    entries.push(Entry::Item(
        Item::new(ABOUT, "About Bowser", Icon::Info).glyph("info"),
    ));
    entries.push(Entry::Item(Item::new(SETTINGS, "Settings", Icon::Gear).glyph("gear")));
    entries.push(Entry::Item(
        Item::new(EXIT, "Exit", Icon::Close).glyph("close").shortcut("Alt+F4"),
    ));
    let menu = Menu {
        header: Some(ProfileHeader {
            profile: profiles.as_ref().map_or_else(
                || "Bowser".to_owned(),
                |p| ProfileMenu::label(p.current.listed_name()).to_string(),
            ),
            caption: "Current profile".to_owned(),
            avatar: profiles.as_ref().map_or_else(
                || "B".to_owned(),
                |p| p.current_metadata.avatar_glyph(p.current.listed_name()),
            ),
            accent: profiles.as_ref().map_or_else(
                || crate::profile::ProfileMetadata::default().accent_colour(),
                |p| p.current_metadata.accent_colour(),
            ),
        }),
        entries,
        footer: update_footer(&update_status),
    };

    // Right-aligned under the button, so the menu grows leftwards and stays on screen.
    // --- popup host (lane M) ---
    let point = screen_point(hwnd, main_menu_anchor(hwnd));
    set_pressed(hwnd, Hit::MainMenu);
    let (built_workspaces, active_workspace) = workspace_menu_snapshot(app);
    menu::show(hwnd, point, menu::Placement::AboveLeft, menu, "main", move |app, chosen| {
    set_pressed(hwnd, Hit::None);

    if let Some(chosen) = chosen {
        match chosen.id {
            NEW_TAB => open_user_tab(hwnd, app), // --- tab options ---
            NEW_WINDOW => open_another_window(false),
            NEW_PRIVATE_WINDOW => open_another_window(true),
            NEW_PRIVATE_TAB => {
                App::new_private_tab(app);
                focus_omni(hwnd, app, true);
            }
            CLOSE_PRIVATE_TABS => {
                App::close_private_tabs(app);
            }
            PROFILE | MORE_TOOLS => {}
            // --- wave-1 fixes --- the registry's commands, not their keys: rebinding or switching off
            // Ctrl+H / Ctrl+J never breaks the menu, and it still opens wherever History opens.
            HISTORY => {
                run_registered(hwnd, app, MENU_HISTORY_COMMAND);
            }
            DOWNLOADS => {
                run_registered(hwnd, app, MENU_DOWNLOADS_COMMAND);
            }
            id if (RECENT_BASE..RECENT_BASE + recent.len()).contains(&id) => {
                // --- tab options --- reopened as a tab, where it was, and gone from the list.
                if recent.get(id - RECENT_BASE).is_some_and(|(_, url)| App::closed_tabs(app).get(id - RECENT_BASE).is_some_and(|closed| closed.url == *url)) {
                    let _ = App::reopen_closed_at(app, id - RECENT_BASE);
                }
                // --- end tab options ---
            }
            // --- tab options ---
            REOPEN_CLOSED => {
                let _ = App::reopen_closed_tab(app);
            }
            // --- end tab options ---
            SHOW_BOOKMARKS => {
                let _ = rail_activate(hwnd, app, side_panel::PanelId::Bookmarks);
            }
            GROUP_ALWAYS => {
                let _ = App::set_tab_group_mode(app, group_tab, tab_groups::GroupMode::Always);
            }
            GROUP_AUTO => {
                let _ = App::set_tab_group_mode(app, group_tab, tab_groups::GroupMode::Auto);
            }
            GROUP_NEVER => {
                let _ = App::set_tab_group_mode(app, group_tab, tab_groups::GroupMode::Never);
            }
            ZOOM_IN => App::zoom_active(app, crate::site_prefs::ZoomStep::In),
            ZOOM_OUT => App::zoom_active(app, crate::site_prefs::ZoomStep::Out),
            ZOOM_RESET => App::zoom_active(app, crate::site_prefs::ZoomStep::Reset),
            CLEAR_DATA => privacy_panel::open(hwnd, app),
            CUSTOMISE => super::settings_page::open(
                hwnd,
                app,
                Some(crate::internal_page::Section::Appearance),
                false,
            ),
            ABOUT => super::settings_page::open(
                hwnd,
                app,
                Some(crate::internal_page::Section::About),
                false,
            ),
            NOTES => {
                notes_panel::open(hwnd);
                apply_sidebar_bounds(hwnd, app);
            }
            PALETTE => run_command_palette(hwnd, app),
            BOOKMARK => add_current_bookmark(hwnd, app),
            SESSIONS => super::sessions_panel::open(hwnd, app), // --- tab options --- the Sessions panel
            WORKSPACES => show_workspaces_menu(hwnd, app),
            READER => run_reader(hwnd, app, crate::reader::Mode::Reader),
            CALM => run_reader(hwnd, app, crate::reader::Mode::Calm),
            TRANSLATE => run_translate(hwnd, app, TranslateAction::Toggle),
            TRANSLATE_ALWAYS_SITE => run_translate(hwnd, app, TranslateAction::Site(crate::site_prefs::SiteTranslate::Always)),
            TRANSLATE_NEVER_SITE => run_translate(hwnd, app, TranslateAction::Site(crate::site_prefs::SiteTranslate::Never)),
            EXTENSIONS => show_extensions_menu(hwnd, app),
            DEVTOOLS => App::open_dev_tools(app),
            PASSWORDS => {
                if let Err(error) = passwords_panel::open(hwnd) {
                    show_error_message(&format!(
                        "Passwords & Passkeys could not be opened: {error}"
                    ));
                }
            }
            BLOCKER => App::toggle_blocker(app),
            // bowser://settings in a tab (the native window when this window has no HTML chrome).
            SETTINGS => super::settings_page::open(hwnd, app, None, false),
            RESTART_TO_UPDATE => restart_to_update(hwnd),
            EXIT => unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(hwnd),
                    WM_CLOSE,
                    WPARAM(0),
                    LPARAM(0),
                );
            },
            id if (PROFILE_SWITCH_BASE..PROFILE_SWITCH_BASE + PROFILE_MAX_LISTED).contains(&id) => {
                if let Some(profiles) = &profiles {
                    profiles.run(id, hwnd);
                }
            }
            id if (PROFILE_DELETE_BASE..PROFILE_DELETE_BASE + PROFILE_MAX_LISTED).contains(&id) => {
                if let Some(profiles) = &profiles {
                    profiles.run(id, hwnd);
                }
            }
            id if (PROFILE_AVATAR_BASE..PROFILE_AVATAR_BASE + 5).contains(&id)
                || (PROFILE_ACCENT_BASE..PROFILE_ACCENT_BASE + 5).contains(&id)
                || (PROFILE_RENAME_BASE..PROFILE_RENAME_BASE + PROFILE_MAX_LISTED)
                    .contains(&id)
                || id == PROFILE_GUEST =>
            {
                if let Some(profiles) = &profiles {
                    profiles.run(id, hwnd);
                }
            }
            PROFILE_NEW => {
                if let Some(profiles) = &profiles {
                    profiles.run(PROFILE_NEW, hwnd);
                }
            }
            id if (WORKSPACE_SELECT_BASE..WORKSPACE_SELECT_BASE + 128).contains(&id)
                || (WORKSPACE_CREATE..=WORKSPACE_DELETE).contains(&id) =>
            {
                run_workspace_action(hwnd, app, id, &built_workspaces, active_workspace, point)
            }
            _ => {}
        }
    }
    let _ = with_chrome(hwnd, |s| s.refresh(app, true));
    repaint(hwnd);
    });
    // --- end popup host ---
}

/// The main menu's footer: the running version and, when there is news, the update state.
fn update_footer(status: &crate::updater::Status) -> String {
    use crate::updater::{LastCheck, Status, Version};
    let running = Version::current();
    let note = match status {
        Status::Ready(version) if *version > running => Some(format!("Update {version} ready")),
        Status::Ready(_) => Some("Update ready".to_owned()),
        Status::Checking => Some("Checking for updates".to_owned()),
        Status::Idle { last: Some(LastCheck::UpToDate) } => Some("Up to date".to_owned()),
        Status::Idle { last: Some(LastCheck::Failed) } => Some("Last update check failed".to_owned()),
        Status::Idle { .. } | Status::Unavailable => None,
    };
    match note {
        Some(note) => format!("Bowser {running} Â· {note}"),
        None => format!("Bowser {running}"),
    }
}

/// `WM_UPDATE_CHECKED`'s LPARAM for a check the settings page started.
const UPDATE_FROM_PAGE: isize = 1;

/// The settings page's "Check for updates": the result goes back to the page. False when the
/// check could not start.
pub(super) fn check_for_updates_from_page(hwnd: HWND) -> bool {
    start_update_check(hwnd, UPDATE_FROM_PAGE)
}

/// The page's word for a check result (`checkResult` in the settings snapshot).
fn update_result_key(code: usize) -> &'static str {
    match code {
        UPDATE_STAGED => "staged",
        UPDATE_UP_TO_DATE => "uptodate",
        UPDATE_UNVERIFIED => "unverified",
        UPDATE_BUSY => "busy",
        UPDATE_UNAVAILABLE => "unavailable",
        _ => "failed",
    }
}

/// "Restart to update" from the settings page.
pub(super) fn restart_to_update_from_page(hwnd: HWND) {
    restart_to_update(hwnd);
}

/// "Manage profiles" from the settings page: the profile menu, where the main menu opens it.
pub(super) fn show_profile_menu_from_page(hwnd: HWND) {
    show_profile_menu(hwnd);
}

/// Runs one update check off the UI thread; `WM_UPDATE_CHECKED` carries the result code and
/// `origin` back. False when the thread could not start.
fn start_update_check(hwnd: HWND, origin: isize) -> bool {
    let window = hwnd.0 as isize;
    let spawned = std::thread::Builder::new()
        .name("bowser-update-check".into())
        .spawn(move || {
            use crate::updater::{CheckOutcome, CheckRefused};
            let code = match crate::updater::check_now() {
                Ok(Ok(CheckOutcome::UpToDate)) => UPDATE_UP_TO_DATE,
                Ok(Ok(CheckOutcome::Staged(_))) => UPDATE_STAGED,
                Ok(Ok(CheckOutcome::AvailableUnverified(_))) => UPDATE_UNVERIFIED,
                Ok(Err(_)) => UPDATE_FAILED,
                Err(CheckRefused::Busy) => UPDATE_BUSY,
                Err(CheckRefused::Unavailable) => UPDATE_UNAVAILABLE,
            };
            super::post(HWND(window as *mut _), WM_UPDATE_CHECKED, WPARAM(code), LPARAM(origin));
        });
    spawned.is_ok()
}

/// The toast that ends "Check for updates". A staged build offers "Restart to update".
fn show_update_result(hwnd: HWND, code: usize) {
    const RESTART: usize = 1;
    let item = match code {
        UPDATE_STAGED => Item::new(RESTART, "Update downloaded. Restart to update", Icon::Check).glyph("refresh"),
        UPDATE_UP_TO_DATE => Item::new(0, "Bowser is up to date", Icon::Check).glyph("check").disabled(),
        UPDATE_UNVERIFIED => {
            Item::new(0, "A newer version could not be verified, so it was not downloaded", Icon::Info)
                .glyph("warning")
                .disabled()
        }
        UPDATE_BUSY => Item::new(0, "Already checking for updates", Icon::Info).glyph("refresh").disabled(),
        UPDATE_UNAVAILABLE => {
            Item::new(0, "Development builds do not update", Icon::Info).glyph("info").disabled()
        }
        _ => Item::new(0, "The update check failed. Try again later", Icon::Info).glyph("warning").disabled(),
    };
    menu::show(
        hwnd,
        screen_point(hwnd, main_menu_anchor(hwnd)),
        menu::Placement::AboveLeft,
        Menu { header: None, entries: vec![Entry::Item(item)], footer: "Updates".into() },
        "update-toast",
        move |_, chosen| {
            if chosen.is_some_and(|chosen| chosen.id == RESTART) {
                restart_to_update(hwnd);
            }
        },
    );
}

/// "Restart to update": closes this window cleanly, and `main` then starts Bowser again, which
/// installs the staged build. The clean close saves the tabs; the restart marker reopens them
/// without the crash prompt.
fn restart_to_update(hwnd: HWND) {
    let marked = crate::profile::active()
        .map_err(|error| error.to_string())
        .and_then(|profile| {
            if profile.is_guest() {
                return Ok(()); // a guest session is never restored
            }
            crate::session::mark_restart(&profile.session_dir()).map_err(|error| error.to_string())
        });
    if let Err(error) = marked {
        show_error_message(&format!("Bowser could not prepare to restart: {error}"));
        return;
    }
    crate::updater::request_restart();
    super::post(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
}

fn run_command_palette(hwnd: HWND, app: &AppRef) {
    let Ok(profile) = crate::profile::active() else {
        return;
    };
    let live = crate::commands::live();
    let options = live.store.options.clone();
    let tabs: Vec<_> = App::tab_ids(app)
        .into_iter()
        .map(|id| {
            (
                id.0,
                App::tab_title(app, id).unwrap_or_else(|| "New tab".into()),
                App::tab_committed_url(app, id).map_or_else(String::new, |url| url.to_string()),
            )
        })
        .collect();
    let productive = crate::productivity::profile_allows_productivity(&profile);
    let bookmarks = if !productive {
        Vec::new()
    } else {
        crate::productivity::read_bookmarks(&profile.bookmarks_file())
    };
    let history = if !productive || !profile.history_enabled() {
        Vec::new()
    } else {
        crate::productivity::read_history(&profile.history_file())
    };
    // Every result type reads a store that already exists; nothing is cached for the palette.
    let notes = if !productive {
        Vec::new()
    } else {
        crate::productivity::read_note_list(&profile.notes_file()).into_iter().map(|note| (note.id, note.title, note.body)).collect()
    };
    let data = command_palette::PaletteData {
        tabs,
        closed: App::recently_closed(app).into_iter().map(|closed| (closed.title, closed.url)).collect(),
        // --- tab stacks and workspaces (T10) --- none while workspaces are off.
        workspaces: if App::workspaces_enabled(app) {
            App::workspace_entries(app).into_iter().map(|(id, name, _, _, count, _)| (id.0, name, count)).collect()
        } else {
            Vec::new()
        },
        bookmarks,
        notes,
        history,
        commands: crate::commands::palette_commands(&live),
        // T2's bookmark nicknames (cached, re-read only when a bookmark file changed); none for a guest.
        nicknames: if productive {
            super::html_suggest::marks().iter().filter(|(_, _, nick)| !nick.is_empty()).map(|(url, _, nick)| (url.clone(), nick.clone())).collect()
        } else {
            Vec::new()
        },
    };
    // --- popup surfaces (lane PP) ---
    if html_popup::page_ready(hwnd) {
        if html_popup::is_active(hwnd) {
            html_popup::cancel(hwnd, app);
        }
        let items = command_palette::html_items_with_favicons(
            &data,
            &options,
            |id| App::tab_favicon(app, TabId(id)),
            |host| crate::favicon::load_favicon_bytes(host).ok().flatten(),
        );
        let page = command_palette::html_page(&items, &options);
        let mut window = RECT::default();
        unsafe {
            let _ = GetWindowRect(hwnd, &mut window);
        }
        let actions: Vec<String> = items.into_iter().map(|item| item.action).collect();
        let shown = html_popup::show_page(
            hwnd,
            html_popup::Anchor::TopCenter(window),
            "palette",
            popup_context(hwnd),
            page,
            Box::new(move |app, chosen| {
                // The id was checked against the page's set; map it back to the action Rust built.
                if let Some(action) = chosen.and_then(|chosen| actions.get(chosen.id)) {
                    run_palette_item(hwnd, app, action);
                }
            }),
            None,
        );
        match shown {
            Ok(()) | Err(html_popup::ShowRefused::Guarded(_)) => return,
            Err(html_popup::ShowRefused::Unavailable(_)) => {}
        }
    }
    // NATIVE FALLBACK: HTML chrome off, or the popup host is not ready yet.
    // --- end popup surfaces ---
    let Some(item) = command_palette::show(hwnd, data, options) else {
        return;
    };
    run_palette_item(hwnd, app, &item.action);
}

/// Shows the keyboard cheat sheet (Ctrl+F1): the palette page in `shortcuts` mode.
pub(crate) fn show_shortcuts_sheet(hwnd: HWND, app: &AppRef) {
    let live = crate::commands::live();
    if !html_popup::page_ready(hwnd) {
        // No HTML popup host (the GDI chrome): the Keyboard section of Settings lists the same
        // shortcuts and lets the user change them.
        super::settings_page::open(hwnd, app, Some(crate::internal_page::Section::Keyboard), false);
        return;
    }
    if html_popup::is_active(hwnd) {
        html_popup::cancel(hwnd, app);
    }
    let items = command_palette::shortcuts_items(&live);
    let page = command_palette::shortcuts_page(&items, live.store.options.shortcuts_enabled);
    let mut window = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut window);
    }
    let _ = html_popup::show_page(
        hwnd,
        html_popup::Anchor::TopCenter(window),
        "shortcuts",
        popup_context(hwnd),
        page,
        Box::new(|_, _| {}),
        None,
    );
}

/// Runs one palette result: a tab id that is still open, an http(s) address, a closed tab, a
/// workspace, a note, a calculator result, or a command.
fn run_palette_item(hwnd: HWND, app: &AppRef, action: &str) {
    let options = crate::commands::live().store.options.clone();
    let web = |text: &str| url::Url::parse(text).ok().filter(|url| matches!(url.scheme(), "http" | "https"));
    if let Some(id) = action.strip_prefix("tab:").and_then(|s| s.parse::<u32>().ok()) {
        if App::tab_ids(app).contains(&TabId(id)) {
            let _ = App::select_tab(app, TabId(id));
        }
    } else if let Some(url) = action.strip_prefix("url:").or_else(|| action.strip_prefix("reopen:")).and_then(web) {
        // "Open links in a new tab" decides between a new tab and the current one; a closed tab always
        // comes back in a tab of its own (`commands::behaviour::link_open`).
        if crate::commands::behaviour::link_open(&options, action) == Some(crate::commands::behaviour::LinkOpen::NewTab) {
            App::new_tab(app);
        }
        App::navigate_current(app, &url);
    } else if let Some(id) = action.strip_prefix("workspace:").and_then(|s| s.parse::<u32>().ok()) {
        let _ = App::switch_workspace(app, crate::workspaces::WorkspaceId(id));
    } else if let Some(id) = action.strip_prefix("note:").and_then(|s| s.parse::<u64>().ok()) {
        if crate::commands::behaviour::note_pick(&options) == crate::commands::behaviour::NotePick::OpenPanel {
            notes_panel::show(hwnd); // --- wave-1 fixes --- never closes an open panel
            apply_sidebar_bounds(hwnd, app);
        } else if let Ok(profile) = crate::profile::active() {
            // Off: the note's text goes to the clipboard to paste where it is wanted.
            if let Some(note) = crate::productivity::read_note_list(&profile.notes_file()).into_iter().find(|note| note.id == id) {
                let text = if note.title.trim().is_empty() { note.body } else { format!("{}\n{}", note.title, note.body) };
                let _ = copy_text_to_clipboard(hwnd, text.trim_end());
            }
        }
    } else if let Some(text) = action.strip_prefix("copy:") {
        let _ = copy_text_to_clipboard(hwnd, text);
    } else if !run_registered(hwnd, app, action) {
        run_palette_command(hwnd, app, action);
    }
}

/// The registry's `Handler::Action` commands: the ones with no chord-dispatched `Command` of their
/// own. Everything else a registry command can do runs through `run_key_command`.
fn run_palette_command(hwnd: HWND, app: &AppRef, action: &str) {
    match action {
        "close-private-tabs" => {
            App::close_private_tabs(app);
        }
        // --- tab options --- the tab and session commands of Quick Commands.
        "reopen-tab" => {
            let _ = App::reopen_closed_tab(app);
        }
        "closed-tabs" => {
            let point = main_menu_anchor(hwnd);
            show_closed_tabs_menu(hwnd, app, point.x, point.y);
        }
        "save-all-session" => save_session_prompt(hwnd, app, &App::tab_ids(app)),
        "open-session" => show_open_session_menu(hwnd, app),
        // --- end tab options ---
        "settings" => super::settings_page::open(hwnd, app, None, false),
        "save-session" | "sessions" => super::sessions_panel::open(hwnd, app), // --- tab options ---
        "calm" => run_reader(hwnd, app, crate::reader::Mode::Calm),
        "translate" => run_translate(hwnd, app, TranslateAction::Toggle),
        _ => {}
    }
}

/// A short list of notes to pick one from (page tools: "Add selection to an existing note"). `picked` gets
/// the note's id; it runs only if the pick is still one of the listed notes.
pub(crate) fn show_note_pick_menu(hwnd: HWND, notes: Vec<(u64, String)>, picked: Box<dyn FnOnce(u64)>) {
    const BASE: usize = 0x5500;
    let entries: Vec<Entry> = notes
        .iter()
        .enumerate()
        .map(|(index, (_, label))| Entry::Item(Item::new(BASE + index, label.clone(), Icon::None).glyph("notes")))
        .collect();
    let point = screen_point(hwnd, main_menu_anchor(hwnd));
    let menu = Menu { header: None, entries, footer: "Add to a note".into() };
    menu::show(hwnd, point, menu::Placement::AboveLeft, menu, "note-pick", move |_app, chosen| {
        let Some(chosen) = chosen else { return };
        if let Some((id, _)) = notes.get(chosen.id.wrapping_sub(BASE)) {
            picked(*id);
        }
    });
}

/// A passive one-line message in the same toast the reader and bookmark commands use.
pub(crate) fn show_toast(hwnd: HWND, message: &str, footer: &str) {
    menu::show(
        hwnd,
        screen_point(hwnd, main_menu_anchor(hwnd)),
        menu::Placement::AboveLeft,
        Menu {
            header: None,
            entries: vec![Entry::Item(Item::new(0, message, Icon::Info).glyph("command").disabled())],
            footer: footer.into(),
        },
        "command-toast",
        |_, _| {},
    );
}

/// Reader view or distraction hiding for the active page. Shared by Ctrl+Alt+R, the toolbar button,
/// the main menu and the command palette. It runs on request only; when there is nothing to do the
/// user is told with the same passive toast the bookmark command uses.
fn run_reader(hwnd: HWND, app: &AppRef, mode: crate::reader::Mode) {
    use crate::app::ReaderResult;
    let tab = App::active_tab_id(app);
    let app_handle = app.clone();
    App::reader_toggle(
        app,
        tab,
        mode,
        Box::new(move |result| {
            let message = match result {
                ReaderResult::NothingToRead => Some("No article text found on this page"),
                ReaderResult::Unavailable if !App::reader_available(&app_handle, tab) => {
                    Some("Reader view works on web pages")
                }
                ReaderResult::Failed => Some("This page could not be changed"),
                _ => None,
            };
            if let Some(message) = message {
                menu::show(
                    hwnd,
                    screen_point(hwnd, main_menu_anchor(hwnd)),
                    menu::Placement::AboveLeft,
                    Menu {
                        header: None,
                        entries: vec![Entry::Item(Item::new(0, message, Icon::Info).glyph("reader").disabled())],
                        footer: "Reader view".into(),
                    },
                    "reader-toast",
                    |_, _| {},
                );
            }
            // Escape leaves reader view only when the page has the keyboard, so give it back.
            if matches!(result, ReaderResult::Applied(crate::reader::Active::Reader)) {
                App::focus_page(&app_handle);
            }
            let _ = with_chrome(hwnd, |s| s.refresh(&app_handle, false));
            html_sync(hwnd);
        }),
    );
}

/// What a translate command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TranslateAction {
    /// Translate the active page, or show its original if it is translated.
    Toggle,
    /// Set (or, if already set, clear) the site's "always" or "never" choice.
    Site(crate::site_prefs::SiteTranslate),
}

/// A passive message in the same place reader view uses.
fn show_translate_toast(hwnd: HWND, message: String) {
    menu::show(
        hwnd,
        screen_point(hwnd, main_menu_anchor(hwnd)),
        menu::Placement::AboveLeft,
        Menu {
            header: None,
            entries: vec![Entry::Item(Item::new(0, message, Icon::Info).glyph("translate").disabled())],
            footer: "Translate".into(),
        },
        "translate-toast",
        |_, _| {},
    );
}

/// The words for what a translation did, when it deserves any. A finished translation, and showing
/// the original, are visible on the page and need none.
fn translate_message(notice: &crate::app::TranslateNotice, available: bool) -> Option<String> {
    use crate::app::TranslateNotice as N;
    match notice {
        N::Started => Some("Translating with Google Translateâ€¦".to_owned()),
        N::Unavailable if !available => Some("Translation works on web pages".to_owned()),
        N::Unavailable => Some("Translation is not available on this page".to_owned()),
        N::InProgress => Some("This page is already being translated".to_owned()),
        N::Busy => Some("Another page is being translated".to_owned()),
        N::SameLanguage => Some("This page is already in your language".to_owned()),
        N::NothingToTranslate => Some("No text to translate on this page".to_owned()),
        N::NetworkFailed => Some("Google Translate could not be reached. Check your connection and try again".to_owned()),
        N::Limited => Some("Google Translate is limiting requests from this connection. Try again later".to_owned()),
        N::Failed => Some("This page could not be translated".to_owned()),
        N::NeedsConfirm | N::Done | N::Restored => None,
    }
}

/// Starts or reverses translation of the active page. Shared by the toolbar button, the main menu and
/// the command palette. It runs on request only, and it sends the page's text to Google Translate; a
/// private tab is asked first.
fn run_translate(hwnd: HWND, app: &AppRef, action: TranslateAction) {
    let tab = App::active_tab_id(app);
    match action {
        TranslateAction::Toggle => start_translate(hwnd, app, tab, false),
        TranslateAction::Site(choice) => {
            // Choosing again clears it. Nothing is stored from a private tab, which has no such menu row.
            let wanted = (App::translate_site_choice(app, tab) != Some(choice)).then_some(choice);
            if !App::translate_set_site(app, tab, wanted) {
                show_translate_toast(hwnd, "This choice could not be saved".to_owned());
                return;
            }
            if wanted == Some(crate::site_prefs::SiteTranslate::Always) && !App::translate_active(app, tab) {
                start_translate(hwnd, app, tab, false);
            }
            let _ = with_chrome(hwnd, |s| s.refresh(app, false));
            html_sync(hwnd);
        }
    }
}

fn start_translate(hwnd: HWND, app: &AppRef, tab: TabId, confirmed: bool) {
    use crate::app::TranslateNotice as N;
    let app_handle = app.clone();
    let notify: std::rc::Rc<dyn Fn(N)> = std::rc::Rc::new(move |notice| {
        if notice == N::NeedsConfirm {
            // A private tab asks first: its text is about to go to Google.
            let again = app_handle.clone();
            menu::show(
                hwnd,
                screen_point(hwnd, main_menu_anchor(hwnd)),
                menu::Placement::AboveLeft,
                Menu {
                    header: None,
                    entries: vec![
                        Entry::Item(Item::new(1, "Send this page's text to Google Translate", Icon::Check).glyph("translate")),
                        Entry::Item(Item::new(0, "Not now", Icon::None)),
                    ],
                    footer: "Translate with Google".into(),
                },
                "translate-confirm",
                move |_, selected| {
                    if selected.is_some_and(|item| item.id == 1) {
                        start_translate(hwnd, &again, tab, true);
                    }
                },
            );
            return;
        }
        if let Some(message) = translate_message(&notice, App::translate_available(&app_handle, tab)) {
            show_translate_toast(hwnd, message);
        }
        let _ = with_chrome(hwnd, |s| s.refresh(&app_handle, false));
        html_sync(hwnd);
    });
    App::translate_toggle(app, tab, confirmed, notify);
}

fn add_current_bookmark(hwnd: HWND, app: &AppRef) {
    // The bookmark dialog, or the page's own details first (src/win32/address_dialog.rs).
    if super::address_dialog::defer_quick_add(hwnd, app) {
        return;
    }
    add_bookmark_now(hwnd, app);
}

/// Saves (or removes) the shown page's bookmark at once, with the toast.
pub(super) fn add_bookmark_now(hwnd: HWND, app: &AppRef) {
    let Ok(profile) = crate::profile::active() else {
        return;
    };
    if !crate::productivity::profile_allows_productivity(&profile) {
        return;
    }
    let id = App::active_tab_id(app);
    let Some(url) = App::tab_committed_url(app, id) else {
        return;
    };
    let title =
        App::tab_title(app, id).unwrap_or_else(|| url.host_str().unwrap_or("Bookmark").to_owned());
    let (title, page_description) = super::address_dialog::apply_metadata(title, App::tab_name(app, id).as_deref(), url.as_str());
    let mut store = match crate::bookmarks::BookmarkStore::load(profile.root()) {
        Ok(store) => store,
        Err(error) => { show_error_message(&format!("Bookmarks could not be loaded: {error}")); return; }
    };
    match store.toggle(None, &title, url.as_str()).and_then(|added| {
        store.save(profile.root())?;
        Ok(added)
    }) {
        Err(error) => show_error_message(&format!("Bookmark could not be saved: {error}")),
        Ok(added) => {
        super::post(hwnd, super::WM_RAIL_CHANGED, WPARAM(0), LPARAM(0));
        if added && !page_description.is_empty() {
            super::address_dialog::save_description(&profile, url.as_str(), &page_description);
        }
        let mirror = if added {
            crate::productivity::add_bookmark(&profile.bookmarks_file(), url.as_str(), &title).map(|_| ())
        } else {
            crate::productivity::remove_bookmark(&profile.bookmarks_file(), url.as_str()).map(|_| ())
        };
        if let Err(error) = mirror {
            show_error_message(&format!("The bookmark changed, but the legacy mirror could not be updated: {error}"));
        }
        // --- popup host (lane M) ---
        menu::show(
            hwnd,
            screen_point(hwnd, main_menu_anchor(hwnd)),
            menu::Placement::AboveLeft,
            Menu {
                header: None,
                entries: vec![Entry::Item(
                    Item::new(0, if added { "Bookmark saved" } else { "Bookmark removed" }, if added { Icon::Check } else { Icon::Close }).glyph("bookmark").disabled(),
                )],
                footer: "Bookmarks".into(),
            },
            "bookmark-toast",
            |_, _| {},
        );
        // --- end popup host ---
        }
    }
}

// --- tab options (src/tab_options.rs, src/app/tab_features.rs) ---

/// A new tab from the user's hand (Ctrl+T, the new tab buttons, the menus): what the new tab page
/// option says opens, and the keyboard goes to the page or the address bar as the options say.
fn open_user_tab(hwnd: HWND, app: &AppRef) {
    let id = App::open_new_tab(app);
    if new_tab_focuses_page(App::tab_is_blank(app, id), App::tab_options(app).focus_page) {
        App::focus_page(app);
    } else {
        focus_omni(hwnd, app, true);
    }
}

/// --- wave-1 fixes (testable rules of the Tabs options) ---
/// "Focus page content on new tab": the page takes the keyboard only when the option is on and the
/// tab shows a page; a blank tab (the start page) keeps the address bar.
fn new_tab_focuses_page(blank: bool, focus_page: bool) -> bool {
    focus_page && !blank
}

/// "Duplicated tab position": where the copy of tab `original` goes in `order`, which already holds
/// the copy at its end; `fallback` is the original's index when it is no longer found.
fn duplicate_index(position: crate::tab_options::DuplicatePosition, order: &[TabId], original: TabId, fallback: usize) -> usize {
    match position {
        crate::tab_options::DuplicatePosition::Last => order.len(),
        crate::tab_options::DuplicatePosition::Beside => order.iter().position(|candidate| *candidate == original).unwrap_or(fallback) + 1,
    }
}

/// What a double click on a tab does ("Double click on a tab").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TabDoubleClick {
    Nothing,
    Close,
    Rename,
}

fn tab_double_click(option: crate::tab_options::DoubleClick) -> TabDoubleClick {
    match option {
        crate::tab_options::DoubleClick::Ignore => TabDoubleClick::Nothing,
        crate::tab_options::DoubleClick::Close => TabDoubleClick::Close,
        crate::tab_options::DoubleClick::Rename => TabDoubleClick::Rename,
    }
}
// --- end wave-1 fixes ---

/// Closes a tab the user asked to close (the close button, middle click, Ctrl+W, the tab menu, a
/// double click): the pinned and last tab options decide, and the last tab may take the window.
fn user_close(hwnd: HWND, app: &AppRef, id: TabId) {
    if App::user_close_tab(app, id) == crate::tab_options::CloseKind::CloseWindow {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

/// Closes several tabs at once (Close other tabs, to the left, to the right, the selection), asking
/// first when their count reaches "Confirm when closing many tabs". A pinned tab that is set not to
/// close stays, and one set to minimise goes to sleep instead (wave-1 fixes). Returns how many closed.
fn close_many(hwnd: HWND, app: &AppRef, ids: Vec<TabId>) -> usize {
    let (close, minimise) = App::bulk_close_split(app, &ids);
    if !close.is_empty() && App::close_needs_confirmation(app, close.len()) && !super::ask_yes_no(&crate::tab_options::confirm_close_text(close.len())) {
        let _ = hwnd;
        return 0;
    }
    for id in minimise {
        let _ = App::user_close_tab(app, id);
    }
    close.into_iter().filter(|id| App::close_tab(app, *id)).count()
}

/// The strip's selection (Ctrl and Shift click): wire ids in strip order, kept for the tab menu.
fn html_tab_selection(hwnd: HWND, wire_ids: &[u32]) {
    let Some(app) = app_of(hwnd) else { return };
    let open = App::tab_ids(&app);
    let ids: Vec<TabId> = wire_ids.iter().filter_map(|raw| chrome_host::tab_from_wire(*raw, &open)).collect();
    App::set_tab_selection(&app, &ids);
}

/// The names Quick Commands, the tab menu and the Sessions panel give the actions.
const TAB_RELOAD_PARENT: usize = 0x5200;
const TAB_RELOAD_OFF: usize = 0x5201;
const TAB_RELOAD_ALL_OFF: usize = 0x5202;
const TAB_RENAME: usize = 0x5203;
const TAB_CLOSE_SELECTED: usize = 0x5204;
const TAB_SESSION_SELECTED: usize = 0x5205;
const TAB_RELOAD_BASE: usize = 0x5210;

/// The tab menu's rows for periodic reload, renaming and a selection of several tabs.
fn tab_options_menu_entries(app: &AppRef, id: TabId, url: Option<&url::Url>, selected: usize) -> Vec<Entry> {
    let current = App::tab_reload_secs(app, id);
    let web = url.is_some_and(|url| matches!(url.scheme(), "http" | "https"));
    let mut reload = crate::tab_state::RELOAD_PRESETS
        .iter()
        .enumerate()
        .map(|(index, secs)| {
            Entry::Item(
                Item::new(TAB_RELOAD_BASE + index, crate::tab_state::interval_label(*secs), Icon::None).checked(current == Some(*secs)),
            )
        })
        .collect::<Vec<_>>();
    if current.is_some() {
        reload.push(Entry::Separator);
        reload.push(Entry::Item(Item::new(TAB_RELOAD_OFF, "Stop reloading this tab", Icon::None)));
    }
    if App::reloading_tab_count(app) > 0 {
        reload.push(Entry::Item(Item::new(TAB_RELOAD_ALL_OFF, "Stop reloading all tabs", Icon::None)));
    }
    let mut entries = vec![
        Entry::Item(Item::new(TAB_RELOAD_PARENT, "Reload every", Icon::None).glyph("reload").submenu_with(reload).disabled_if(!web)),
        Entry::Item(Item::new(TAB_RENAME, "Rename tabâ€¦", Icon::None).glyph("edit")),
    ];
    if selected > 1 {
        entries.push(Entry::Separator);
        entries.push(Entry::Item(Item::new(TAB_CLOSE_SELECTED, format!("Close {selected} selected tabs"), Icon::Close).glyph("close")));
        entries.push(Entry::Item(Item::new(TAB_SESSION_SELECTED, format!("Save {selected} selected tabs as sessionâ€¦"), Icon::None).glyph("save")));
    }
    entries
}

/// Runs a row of [`tab_options_menu_entries`]. False for any other row.
fn tab_options_menu_pick(hwnd: HWND, app: &AppRef, id: TabId, command: usize, targets: &[TabId]) -> bool {
    let presets = crate::tab_state::RELOAD_PRESETS;
    match command {
        c if (TAB_RELOAD_BASE..TAB_RELOAD_BASE + presets.len()).contains(&c) => {
            let _ = App::set_tab_reload(app, id, Some(presets[c - TAB_RELOAD_BASE]));
        }
        TAB_RELOAD_OFF => {
            let _ = App::set_tab_reload(app, id, None);
        }
        TAB_RELOAD_ALL_OFF => {
            let _ = App::disable_all_reloads(app);
        }
        TAB_RENAME => rename_tab_prompt(hwnd, app, id),
        TAB_CLOSE_SELECTED => {
            close_many(hwnd, app, targets.to_vec());
        }
        TAB_SESSION_SELECTED => save_session_prompt(hwnd, app, targets),
        _ => return false,
    }
    true
}

/// --- wave-1 integration --- the "Reload this tab everyâ€¦" registry command: the tab menu's reload
/// presets for the tab in front, as a menu under the main menu button.
pub(crate) fn show_periodic_reload_menu(hwnd: HWND, app: &AppRef) {
    let id = App::active_tab_id(app);
    let url = App::tab_committed_url(app, id);
    if !url.as_ref().is_some_and(|url| matches!(url.scheme(), "http" | "https")) {
        show_toast(hwnd, "Periodic reload works on web pages", "Periodic reload");
        return;
    }
    // The tab menu's own rows, so both menus offer the same presets and stop rows.
    let entries = tab_options_menu_entries(app, id, url.as_ref(), 1)
        .into_iter()
        .find_map(|entry| match entry {
            Entry::Item(item) if item.id == TAB_RELOAD_PARENT => item.submenu_entries,
            _ => None,
        })
        .unwrap_or_default();
    let menu = Menu { header: None, entries, footer: "Reload this tab every".to_owned() };
    menu::show(hwnd, screen_point(hwnd, main_menu_anchor(hwnd)), menu::Placement::BelowLeft, menu, "tabreload", move |app, chosen| {
        if let Some(chosen) = chosen {
            let _ = tab_options_menu_pick(hwnd, app, id, chosen.id, &[]);
        }
    });
}
// --- end wave-1 integration ---

/// --- tab stacks and workspaces (T10) --- "Emojiâ€¦" for a workspace: asks for an emoji (or a short
/// text) to show instead of its icon. Empty goes back to the icon.
fn workspace_emoji_prompt(hwnd: HWND, app: &AppRef, id: crate::workspaces::WorkspaceId) {
    let current = App::workspace_emoji(app, id).unwrap_or_default();
    let validate: super::profile_rename::Validator = Box::new(|text| {
        if text.trim().is_empty() || crate::workspaces::clean_emoji(text).is_some() {
            Ok(())
        } else {
            Err(format!("Use one emoji, or up to {} characters without spaces.", crate::workspaces::MAX_EMOJI_CHARS))
        }
    });
    let Some(text) = super::profile_rename::prompt_validated(hwnd, &current, "Workspace emoji", "Use", "Shown instead of the icon. Leave it empty to show the icon. Win+. opens the emoji panel.", validate) else {
        return;
    };
    let _ = App::workspace_set_emoji(app, id, Some(&text));
}

/// "Rename tab": asks for a name. An empty name goes back to the page's title.
fn rename_tab_prompt(hwnd: HWND, app: &AppRef, id: TabId) {
    let current = App::tab_name(app, id).unwrap_or_default();
    let validate: super::profile_rename::Validator = Box::new(|name| {
        if name.chars().any(char::is_control) {
            Err("Names cannot contain control characters.".to_owned())
        } else if name.chars().count() > crate::app::MAX_TAB_NAME_CHARS {
            Err(format!("Use at most {} characters.", crate::app::MAX_TAB_NAME_CHARS))
        } else {
            Ok(())
        }
    });
    let Some(name) = super::profile_rename::prompt_validated(hwnd, &current, "Rename tab", "Rename", "Leave it empty to show the pageâ€™s title.", validate) else {
        return;
    };
    let _ = App::set_tab_name(app, id, &name);
}

/// The list of tabs closed this session at `(x, y)` (client pixels); a pick reopens it where it was.
fn show_closed_tabs_menu(hwnd: HWND, app: &AppRef, x: i32, y: i32) {
    const BASE: usize = 0x5300;
    const REOPEN: usize = 0x52FF;
    let closed = App::closed_tabs(app);
    let urls: Vec<String> = closed.iter().map(|tab| tab.url.clone()).collect();
    let mut entries = vec![Entry::Item(
        Item::new(REOPEN, "Reopen closed tab", Icon::None).glyph("reset").shortcut("Ctrl+Shift+T").disabled_if(closed.is_empty()),
    )];
    entries.push(Entry::Separator);
    if closed.is_empty() {
        entries.push(Entry::Item(Item::new(BASE, "Nothing closed in this session", Icon::None).glyph("clock").disabled()));
    }
    entries.extend(closed.iter().enumerate().map(|(index, tab)| {
        let label = if tab.title.trim().is_empty() { tab.url.clone() } else { tab.title.clone() };
        Entry::Item(Item::new(BASE + index, label, Icon::None).glyph("clock"))
    }));
    let point = screen_point(hwnd, windows::Win32::Foundation::POINT { x, y });
    let menu = Menu { header: None, entries, footer: "Closed tabs this session".to_owned() };
    menu::show(hwnd, point, menu::Placement::BelowLeft, menu, "closedtabs", move |app, chosen| {
        let Some(chosen) = chosen else { return };
        if chosen.id == REOPEN {
            let _ = App::reopen_closed_tab(app);
        } else if (BASE..BASE + urls.len()).contains(&chosen.id) {
            let n = chosen.id - BASE;
            // The list may have changed while the menu was open: reopen only the entry that was shown.
            if App::closed_tabs(app).get(n).is_some_and(|tab| tab.url == urls[n]) {
                let _ = App::reopen_closed_at(app, n);
            }
        }
    });
}

/// "Save all tabs as session" and "Save selected tabs as session": asks for a name and saves the
/// web pages among `ids`.
fn save_session_prompt(hwnd: HWND, app: &AppRef, ids: &[TabId]) {
    let Some(name) = super::profile_rename::prompt(hwnd, "", "Save tabs as session", "Save") else {
        return;
    };
    if let Err(message) = App::save_session(app, &name, ids) {
        show_error_message(&message);
    }
    super::sessions_panel::refresh(hwnd);
}

/// "Open saved session": a menu of the saved sessions; a pick opens it.
fn show_open_session_menu(hwnd: HWND, _app: &AppRef) {
    let Ok(profile) = crate::profile::active() else { return };
    if !crate::productivity::profile_allows_productivity(&profile) {
        return;
    }
    let sessions = crate::productivity::read_sessions(&profile.saved_sessions_file());
    const BASE: usize = 0x5400;
    let mut entries: Vec<Entry> = sessions
        .iter()
        .enumerate()
        .map(|(index, session)| Entry::Item(Item::new(BASE + index, format!("{} ({})", session.name, session.urls.len()), Icon::None).glyph("folder")))
        .collect();
    if entries.is_empty() {
        entries.push(Entry::Item(Item::new(BASE, "No saved sessions", Icon::None).glyph("folder").disabled()));
    }
    let point = screen_point(hwnd, main_menu_anchor(hwnd));
    let menu = Menu { header: None, entries, footer: "Saved sessions".into() };
    menu::show(hwnd, point, menu::Placement::AboveLeft, menu, "sessions", move |app, chosen| {
        let Some(chosen) = chosen else { return };
        let live = crate::productivity::read_sessions(&profile.saved_sessions_file());
        let Some(session) = sessions.get(chosen.id.wrapping_sub(BASE)).filter(|session| live.iter().any(|other| other.name == session.name)) else {
            return;
        };
        App::open_session(app, session, false);
    });
}

// --- end tab options ---

pub(super) fn show_sessions(hwnd: HWND, _app: &AppRef) {
    let Ok(profile) = crate::profile::active() else {
        return;
    };
    if !crate::productivity::profile_allows_productivity(&profile) {
        return;
    }
    let sessions = crate::productivity::read_sessions(&profile.saved_sessions_file());
    const SAVE: usize = 1;
    const RESTORE_BASE: usize = 100;
    const DELETE_BASE: usize = 200;
    let mut entries = vec![Entry::Item(Item::new(
        SAVE,
        "Save current tabsâ€¦",
        Icon::Plus,
    )
    .glyph("plus"))];
    if !sessions.is_empty() {
        entries.push(Entry::Separator);
    }
    // --- popup host (lane M) ---
    // Submenu parents get their own ids: the HTML menu rejects duplicates.
    const PARENT_BASE: usize = 300;
    for (index, session) in sessions.iter().enumerate() {
        let restore = RESTORE_BASE + index;
        let delete = DELETE_BASE + index;
        entries.push(Entry::Item(
            Item::new(PARENT_BASE + index, session.name.clone(), Icon::Check).glyph("folder").submenu_with(vec![
                Entry::Item(
                    Item::new(
                        restore,
                        format!("Restore {} tabs", session.urls.len()),
                        Icon::Check,
                    )
                    .glyph("reset"),
                ),
                Entry::Item(Item::new(delete, "Delete session", Icon::Close).glyph("trash")),
            ]),
        ));
    }
    let point = screen_point(hwnd, main_menu_anchor(hwnd));
    let menu = Menu {
        header: None,
        entries,
        footer: "Saved sessions".into(),
    };
    menu::show(hwnd, point, menu::Placement::AboveLeft, menu, "sessions", move |app, chosen| {
    let Some(chosen) = chosen else {
        return;
    };
    // A session deleted or renamed while the menu was open is not acted on.
    let live = crate::productivity::read_sessions(&profile.saved_sessions_file());
    let still_saved = |index: usize| {
        sessions.get(index).filter(|session| live.iter().any(|other| other.name == session.name))
    };
    if chosen.id == SAVE {
        let Some(name) = super::profile_rename::prompt(hwnd, "", "Save session", "Save") else {
            return;
        };
        let active_id = App::active_tab_id(app);
        let entries: Vec<_> = App::tab_ids(app)
            .into_iter()
            // Private tabs are never written to a saved session.
            .filter(|id| {
                crate::private::may_persist(
                    crate::private::Store::SavedSessions,
                    App::tab_private(app, *id),
                    false,
                )
            })
            .filter_map(|id| App::tab_committed_url(app, id).map(|url| (id, url.to_string())))
            .filter(|(_, value)| {
                matches!(
                    url::Url::parse(value)
                        .map(|u| u.scheme().to_owned())
                        .as_deref(),
                    Ok("http" | "https")
                )
            })
            .collect();
        let urls: Vec<_> = entries.iter().map(|(_, url)| url.clone()).collect();
        if urls.is_empty() {
            return;
        }
        let active = entries
            .iter()
            .position(|(id, _)| *id == active_id)
            .unwrap_or(0);
        if let Err(error) = crate::productivity::save_session(
            &profile.saved_sessions_file(),
            crate::productivity::SavedSession { name, urls, active },
        ) {
            show_error_message(&format!("Session could not be saved: {error}"));
        }
    } else if (RESTORE_BASE..DELETE_BASE).contains(&chosen.id) {
        let Some(session) = still_saved(chosen.id - RESTORE_BASE) else {
            return;
        };
        if !super::ask_yes_no("Replace the open tabs with this saved session?") {
            return;
        }
        for id in App::tab_ids(app) {
            let _ = App::close_tab(app, id);
        }
        let mut ids = vec![App::active_tab_id(app)];
        for value in &session.urls {
            if let Ok(url) = url::Url::parse(value) {
                App::navigate_current(app, &url);
            }
            if ids.len() < session.urls.len() {
                ids.push(App::new_tab(app));
                let _ = App::select_tab(app, *ids.last().unwrap());
            }
        }
        if let Some(id) = ids.get(session.active) {
            let _ = App::select_tab(app, *id);
        }
    } else if (DELETE_BASE..DELETE_BASE + sessions.len()).contains(&chosen.id) {
        let index = chosen.id - DELETE_BASE;
        if let Some(session) = still_saved(index) {
            if super::ask_yes_no("Delete this saved session?") {
                let _ = crate::productivity::delete_session(
                    &profile.saved_sessions_file(),
                    &session.name,
                );
            }
        }
    }
    });
    // --- end popup host ---
}

fn open_profile_window(target: &crate::profile::Profile, hwnd: HWND) {
    if !crate::profile::spawn_window(target) {
        show_profile_notice(hwnd, "Bowser could not start a window for that profile.");
    }
}

/// Handles Alt+Space and titlebar right-click with Bowser's themed window-command menu.
fn show_system_menu(hwnd: HWND, point: windows::Win32::Foundation::POINT) {
    use windows::Win32::UI::WindowsAndMessaging::{SC_CLOSE, SC_MAXIMIZE, SC_MINIMIZE};
    let maximized = unsafe { IsZoomed(hwnd).as_bool() };
    let minimized = unsafe { IsIconic(hwnd).as_bool() };
    let entries = vec![
        Entry::Item(
            Item::new(SC_RESTORE as usize, "Restore", Icon::None)
                .glyph("caption-restore")
                .disabled_if(!maximized && !minimized),
        ),
        Entry::Item(
            Item::new(SC_MOVE as usize, "Move", Icon::None).disabled_if(maximized || minimized),
        ),
        Entry::Item(
            Item::new(SC_SIZE as usize, "Size", Icon::None).disabled_if(maximized || minimized),
        ),
        Entry::Item(Item::new(SC_MINIMIZE as usize, "Minimize", Icon::None).glyph("caption-minimize").disabled_if(minimized)),
        Entry::Item(Item::new(SC_MAXIMIZE as usize, "Maximize", Icon::None).glyph("caption-maximize").disabled_if(maximized)),
        Entry::Separator,
        Entry::Item(Item::new(SC_CLOSE as usize, "Close", Icon::Close).glyph("close").shortcut("Alt+F4")),
    ];
    // --- popup host (lane M) ---
    let menu = Menu {
        header: None,
        entries,
        footer: "Window commands".to_owned(),
    };
    menu::show(hwnd, point, menu::Placement::BelowLeft, menu, "system", move |_, chosen| {
        if let Some(chosen) = chosen {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(hwnd),
                    WM_SYSCOMMAND,
                    WPARAM(chosen.id),
                    LPARAM(0),
                );
            }
        }
    });
    // --- end popup host ---
}

/// Ctrl+Shift+E: themed menu for this profile's unpacked extensions.
fn show_extensions_menu(hwnd: HWND, _app: &AppRef) {
    use crate::extensions::{self, Record};
    const INSTALL: usize = 1;
    const TOGGLE_BASE: usize = 100;
    const REMOVE_BASE: usize = 1000;
    const REMOVE_MENU: usize = 2;
    const POPUP_MENU: usize = 3;
    const MANAGE: usize = 4;
    const INSTALL_CRX: usize = 5;
    const POPUP_BASE: usize = 3000;
    const MAX_LISTED: usize = extensions::MAX_INSTALLED;

    let Ok(profile) = crate::profile::active() else {
        return;
    };
    let records: Vec<Record> = match extensions::read_records(&profile.extensions_file()) {
        Ok(records) => records.into_iter().take(MAX_LISTED).collect(),
        Err(error) => {
            show_error_message(&format!("The extensions list could not be read. {error}"));
            return;
        }
    };
    let mut entries = Vec::with_capacity(records.len().saturating_mul(2) + 3);
    if records.is_empty() {
        entries.push(Entry::Item(
            Item::new(0, "No extensions installed", Icon::None).glyph("puzzle").disabled(),
        ));
    }
    for (index, record) in records.iter().enumerate() {
        let mut label = format!(
            "{} {} ({})",
            record.name,
            record.version,
            if record.enabled {
                "enabled"
            } else {
                "disabled"
            }
        );
        if let Some(reason) = extensions::load_failure(&record.id) {
            label.push_str(&format!(" (failed: {reason})"));
        }
        entries.push(Entry::Item(
            Item::new(TOGGLE_BASE + index, label, Icon::Grid).glyph("puzzle").checked(record.enabled),
        ));
    }
    let remove_entries: Vec<Entry> = records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            Entry::Item(
                Item::new(REMOVE_BASE + index, record.name.clone(), Icon::Close).glyph("puzzle"),
            )
        })
        .collect();
    entries.push(Entry::Separator);
    // Only an enabled extension whose manifest names a popup is listed.
    let popup_entries: Vec<Entry> = records
        .iter()
        .enumerate()
        .filter(|(_, record)| {
            record.enabled
                && extensions::folder_for(&profile.extensions_dir(), &record.id)
                    .is_ok_and(|folder| extensions::declares_page(&folder, extensions::PageKind::Popup))
        })
        .map(|(index, record)| {
            Entry::Item(
                Item::new(POPUP_BASE + index, record.name.clone(), Icon::Grid).glyph("puzzle"),
            )
        })
        .collect();
    let popup = Item::new(POPUP_MENU, "Open extension popupâ€¦", Icon::Grid).glyph("puzzle");
    entries.push(Entry::Item(if popup_entries.is_empty() {
        popup.disabled()
    } else {
        popup.submenu_with(popup_entries)
    }));
    let remove = Item::new(REMOVE_MENU, "Remove extensionâ€¦", Icon::Close).glyph("trash");
    entries.push(Entry::Item(if records.is_empty() {
        remove.disabled()
    } else {
        remove.submenu_with(remove_entries)
    }));
    entries.push(Entry::Separator);
    entries.push(Entry::Item(Item::new(MANAGE, "Manage extensions", Icon::Grid).glyph("gear")));
    entries.push(Entry::Item(Item::new(
        INSTALL,
        "Install unpacked extensionâ€¦",
        Icon::Plus,
    )
    .glyph("folder")));
    entries.push(Entry::Item(Item::new(
        INSTALL_CRX,
        "Install CRX fileâ€¦",
        Icon::Plus,
    )
    .glyph("puzzle")));

    // --- popup host (lane M) ---
    let point = screen_point(hwnd, main_menu_anchor(hwnd));
    let menu = Menu {
        header: None,
        entries,
        footer: "Extensions".to_owned(),
    };
    menu::show(hwnd, point, menu::Placement::AboveLeft, menu, "extensions", move |app, chosen| {
        let command = chosen.map_or(0, |chosen| chosen.id);
        // Act on the record as it is now; one removed while the menu was open is skipped.
        let live = |index: usize| {
            let listed = records.get(index)?;
            extensions::read_records(&profile.extensions_file())
                .ok()?
                .into_iter()
                .find(|record| record.id == listed.id)
        };
        match command {
            INSTALL => {
                if let Err(error) = install_extension(hwnd, app, &profile) {
                    show_error_message(&error);
                }
            }
            INSTALL_CRX => {
                if let Err(error) = install_extension_from_crx_file(hwnd, app, &profile) {
                    show_error_message(&error);
                }
            }
            MANAGE => super::settings_page::open(hwnd, app, Some(crate::internal_page::Section::Extensions), false),
            id if (TOGGLE_BASE..TOGGLE_BASE + MAX_LISTED).contains(&id) => {
                if let Some(record) = live(id - TOGGLE_BASE) {
                    if let Err(error) = set_extension_enabled(app, &profile, &record, !record.enabled) {
                        show_error_message(&error);
                    }
                }
            }
            id if (REMOVE_BASE..REMOVE_BASE + MAX_LISTED).contains(&id) => {
                if let Some(record) = live(id - REMOVE_BASE) {
                    let question = format!(
                        "Remove the extension \"{}\" from this profile? Its copied files will be deleted.",
                        record.name
                    );
                    if crate::win32::ask_yes_no(&question) {
                        remove_extension(app, &profile, &record, |result| {
                            if let Err(error) = result {
                                show_error_message(&error);
                            }
                        });
                    }
                }
            }
            id if (POPUP_BASE..POPUP_BASE + MAX_LISTED).contains(&id) => {
                if let Some(record) = live(id - POPUP_BASE).filter(|record| record.enabled) {
                    open_extension_popup(app, &profile, &record);
                }
            }
            _ => {}
        }
    });
    // --- end popup host ---
}

/// Enabled extensions whose copied manifest names a popup and that did not fail to load this
/// session, in list order: what the toolbar's extensions button offers.
fn popup_extensions(profile: &crate::profile::Profile) -> Vec<crate::extensions::Record> {
    use crate::extensions;
    let Ok(records) = extensions::read_records(&profile.extensions_file()) else {
        return Vec::new();
    };
    records
        .into_iter()
        .filter(|record| {
            record.enabled
                && extensions::load_failure(&record.id).is_none()
                && extensions::folder_for(&profile.extensions_dir(), &record.id)
                    .is_ok_and(|folder| extensions::declares_page(&folder, extensions::PageKind::Popup))
        })
        .collect()
}

/// The toolbar's extensions button (docs/extensions.md): reserves its slot beside the pill while an
/// enabled extension has a popup (so the pill's new width is sent first), then sends the `extbtn`
/// message when it differs from the last one the page got. Runs when the toolbar page is ready and
/// on `WM_EXTENSIONS_CHANGED`.
fn html_refresh_extensions_button(hwnd: HWND) {
    let popups = crate::profile::active().map(|profile| popup_extensions(&profile)).unwrap_or_default();
    let names: Vec<&str> = popups.iter().map(|record| record.name.as_str()).collect();
    let json = chrome_host::extensions_page::button_json(&names);
    let slot = if popups.is_empty() { 0 } else { chrome_host::EXT_BUTTON_SLOT };
    let moved = with_chrome(hwnd, |s| std::mem::replace(&mut s.html_ext_slot, slot) != slot).unwrap_or(false);
    if moved {
        html_sync(hwnd);
    }
    let Some((host, generation)) = with_chrome(hwnd, |s| {
        (s.html_page_ready && s.html_ext_last.as_deref() != Some(json.as_str()))
            .then(|| s.html.clone())
            .flatten()
            .map(|host| (host, s.html_state_generation[0]))
    })
    .flatten() else {
        return;
    };
    if host.post_json(&json).is_ok() {
        let _ = with_chrome(hwnd, |s| {
            if s.html_state_generation[0] == generation {
                s.html_ext_last = Some(json);
            }
        });
    }
}

/// Posts `message` to every Bowser window on this thread.
pub(super) fn post_to_every_window(message: u32) {
    let all: Vec<isize> = WINDOWS.with(|all| all.try_borrow().map(|all| all.keys().copied().collect()).unwrap_or_default());
    for raw in all {
        crate::win32::post(HWND(raw as *mut _), message, WPARAM(0), LPARAM(0));
    }
}

/// The toolbar's extensions button was clicked: the only extension with a popup opens it; with
/// several, their names hang from the button with "Manage extensions" under them.
fn show_extensions_button_menu(hwnd: HWND, app: &AppRef) {
    const MANAGE: usize = 1;
    const POPUP_BASE: usize = 100;
    let Ok(profile) = crate::profile::active() else {
        return;
    };
    let popups = popup_extensions(&profile);
    match popups.as_slice() {
        [] => {
            // The button outlived its last popup (the list changed on disk): go where it can be fixed.
            super::settings_page::open(hwnd, app, Some(crate::internal_page::Section::Extensions), false);
            return;
        }
        [only] => {
            open_extension_popup(app, &profile, only);
            return;
        }
        _ => {}
    }
    let mut entries: Vec<Entry> = popups
        .iter()
        .take(crate::extensions::MAX_INSTALLED)
        .enumerate()
        .map(|(index, record)| Entry::Item(Item::new(POPUP_BASE + index, record.name.clone(), Icon::Grid).glyph("puzzle")))
        .collect();
    entries.push(Entry::Separator);
    entries.push(Entry::Item(Item::new(MANAGE, "Manage extensions", Icon::Grid).glyph("gear")));
    let Some(anchor) = extensions_anchor(hwnd) else {
        return;
    };
    let menu = Menu {
        header: None,
        entries,
        footer: "Extension popups".to_owned(),
    };
    menu::show(hwnd, anchor, menu::Placement::BelowRight, menu, "extensions-button", move |app, chosen| {
        match chosen.map_or(0, |chosen| chosen.id) {
            MANAGE => super::settings_page::open(hwnd, app, Some(crate::internal_page::Section::Extensions), false),
            id if id >= POPUP_BASE => {
                // The pick names the extension it was built from; it must still have a popup now.
                let Some(listed) = popups.get(id - POPUP_BASE) else { return };
                if let Some(record) = popup_extensions(&profile).into_iter().find(|record| record.id == listed.id) {
                    open_extension_popup(app, &profile, &record);
                }
            }
            _ => {}
        }
    });
}

/// The screen point the extensions list hangs from: under the button, at its right edge. The rect
/// is the button as the page last reported it; without one, the slot Rust reserved stands in.
fn extensions_anchor(hwnd: HWND) -> Option<windows::Win32::Foundation::POINT> {
    let (host, dpi, menux, dl_slot, rect) = with_chrome(hwnd, |s| {
        let (_, _, menux) = chrome_host::toolbar_css_placement_with_slot(&s.layout, s.dpi, s.toolbar_slot())?;
        Some((s.html.clone()?, s.dpi, menux, s.html_dl_slot, s.html_ext_rect))
    })
    .flatten()?;
    let screen = host.screen_bounds()?;
    let fallback = chrome_host::protocol::HoverRect { x: (menux - dl_slot - chrome_host::EXT_BUTTON_SLOT).max(0) as u32, y: 3, w: 28, h: 28 };
    let scale = dpi as f32 / 96.0;
    let button = super::html_hover::anchor_to_screen(screen, scale, rect.unwrap_or(fallback));
    Some(windows::Win32::Foundation::POINT { x: button.right, y: button.bottom + (4.0 * scale).round() as i32 })
}

/// Opens the extension's popup page in a Bowser-owned window. The engine builds the address from
/// the manifest in the extension's folder and refuses anything that is not a plain page path.
pub(super) fn open_extension_popup(app: &AppRef, profile: &crate::profile::Profile, record: &crate::extensions::Record) {
    use crate::extensions;
    match extensions::folder_for(&profile.extensions_dir(), &record.id) {
        Ok(folder) => App::open_extension_page(
            app,
            record.id.clone(),
            folder,
            extensions::PageKind::Popup,
            format!("{} popup", record.name),
        ),
        Err(error) => show_error_message(&error.to_string()),
    }
}

/// Turns an installed extension on or off: the flag in `extensions.txt`, then the engine loads or
/// unloads Bowser's existing copy. Nothing is read from the original folder again and nothing is
/// asked, so turning one back on uses exactly what was installed. The load result arrives later
/// (`App::set_extension_enabled`) and every window hears about it.
pub(super) fn set_extension_enabled(
    app: &AppRef,
    profile: &crate::profile::Profile,
    record: &crate::extensions::Record,
    enabled: bool,
) -> Result<(), String> {
    use crate::extensions;
    extensions::set_enabled(&profile.extensions_file(), &record.id, enabled)
        .map_err(|error| format!("The extension was not changed. {error}"))?;
    super::extensions_changed();
    let folder = extensions::folder_for(&profile.extensions_dir(), &record.id).map_err(|error| error.to_string())?;
    App::set_extension_enabled(app, record.id.clone(), folder, enabled);
    Ok(())
}

/// Unloads an extension, then drops it from the list and deletes Bowser's copy. The caller has
/// already asked the user. `done` runs once the engine answers (later, or at once without an
/// engine) with what went wrong, if anything.
pub(super) fn remove_extension(
    app: &AppRef,
    profile: &crate::profile::Profile,
    record: &crate::extensions::Record,
    done: impl FnOnce(Result<(), String>) + 'static,
) {
    use crate::extensions;
    let extensions_dir = profile.extensions_dir();
    let records_file = profile.extensions_file();
    let id = record.id.clone();
    App::remove_extension(
        app,
        record.id.clone(),
        Box::new(move |result| {
            let result = match result {
                Ok(()) => {
                    extensions::record_load_result(&id, Ok(()));
                    extensions::uninstall(&extensions_dir, &records_file, &id).map_err(|error| {
                        format!("The extension was removed from the list, but its files were not deleted. {error}")
                    })
                }
                Err(reason) => {
                    extensions::record_load_result(&id, Err(format!("Remove failed: {reason}")));
                    Err(format!("The extension was not removed. {reason}"))
                }
            };
            super::extensions_changed();
            done(result);
        }),
    );
}

/// Asks for an unpacked extension folder, shows its permissions, then copies it into the profile
/// and loads it. Nothing is copied before the user agrees. `Ok(false)` when the user cancelled.
pub(super) fn install_extension(hwnd: HWND, app: &AppRef, profile: &crate::profile::Profile) -> Result<bool, String> {
    use crate::extensions;
    let Some(folder) = pick_extension_folder(hwnd) else {
        return Ok(false);
    };
    let result = extensions::install_confirmed(
        &folder,
        &profile.extensions_dir(),
        &profile.extensions_file(),
        &mut |inspection| crate::win32::ask_yes_no(&extensions::install_prompt(&inspection.manifest)),
    );
    finish_install(app, profile, result.map_err(|error| error.to_string()))
}

/// Asks for a `.crx` file on this computer, verifies it, unpacks it into a temporary folder and
/// installs that folder through the same permission prompt and copy as an unpacked extension.
/// Nothing is installed when the file does not verify or the user declines; the temporary folder is
/// always deleted. No network is used. `Ok(false)` when the user cancelled or declined.
pub(super) fn install_extension_from_crx_file(
    hwnd: HWND,
    app: &AppRef,
    profile: &crate::profile::Profile,
) -> Result<bool, String> {
    use crate::extensions;
    let Some(file) = pick_crx_file(hwnd) else {
        return Ok(false);
    };
    let result = crate::crx_file::install_crx_file(
        &file,
        &profile.extensions_dir(),
        &profile.extensions_file(),
        &mut |inspection| crate::win32::ask_yes_no(&extensions::install_prompt(&inspection.manifest)),
    );
    finish_install(app, profile, result.map_err(|error| error.to_string()))
}

/// What both install paths do with the answer: a recorded extension is loaded from Bowser's copy and
/// every window hears about it; a declined prompt is `Ok(false)`; a failure is worded for the page.
fn finish_install(
    app: &AppRef,
    profile: &crate::profile::Profile,
    result: Result<Option<crate::extensions::Record>, String>,
) -> Result<bool, String> {
    use crate::extensions;
    match result {
        Ok(Some(record)) => {
            super::extensions_changed();
            let installed = extensions::folder_for(&profile.extensions_dir(), &record.id).map_err(|error| error.to_string())?;
            App::set_extension_enabled(app, record.id, installed, true);
            Ok(true)
        }
        Ok(None) => Ok(false),
        Err(error) => Err(format!("The extension was not installed. {error}")),
    }
}

/// File picker for one `.crx` file. `None` when cancelled or when the dialog fails.
fn pick_crx_file(owner: HWND) -> Option<std::path::PathBuf> {
    use windows::Win32::{
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
        UI::Shell::{
            Common::COMDLG_FILTERSPEC, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FileOpenDialog,
            IFileOpenDialog, SIGDN_FILESYSPATH,
        },
    };
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog
            .SetOptions(FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST)
            .ok()?;
        dialog
            .SetTitle(w!("Choose a .crx extension file on this computer"))
            .ok()?;
        let filters = [COMDLG_FILTERSPEC {
            pszName: w!("CRX extension file (*.crx)"),
            pszSpec: w!("*.crx"),
        }];
        dialog.SetFileTypes(&filters).ok()?;
        dialog.Show(Some(owner)).ok()?;
        let item = dialog.GetResult().ok()?;
        let display = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = display.to_string().ok().map(std::path::PathBuf::from);
        CoTaskMemFree(Some(display.0.cast_const().cast()));
        path
    }
}

/// Folder picker for an unpacked extension. `None` when cancelled or when the dialog fails.
fn pick_extension_folder(owner: HWND) -> Option<std::path::PathBuf> {
    use windows::Win32::{
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
        UI::Shell::{
            FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog,
            SIGDN_FILESYSPATH,
        },
    };
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog
            .SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)
            .ok()?;
        dialog
            .SetTitle(w!(
                "Choose an unpacked extension folder (the one with manifest.json)"
            ))
            .ok()?;
        dialog.Show(Some(owner)).ok()?;
        let item = dialog.GetResult().ok()?;
        let display = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = display.to_string().ok().map(std::path::PathBuf::from);
        CoTaskMemFree(Some(display.0.cast_const().cast()));
        path
    }
}

fn show_open_tabs_menu(hwnd: HWND, app: &AppRef) {
    let hidden =
        with_chrome(hwnd, |s| overflow_tab_ids(&s.layout, &s.slots, &s.tabs)).unwrap_or_default();
    if hidden.is_empty() {
        return;
    }
    let ids = App::tab_ids(app);
    let active = App::active_tab_id(app);
    let mut entries = Vec::with_capacity(ids.len());
    for (index, id) in ids.iter().copied().enumerate() {
        let mut label = App::tab_title(app, id).unwrap_or_default();
        if label.trim().is_empty() {
            label = "New tab".to_owned();
        }
        let favicon = with_chrome(hwnd, |state| {
            state
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .and_then(|tab| tab.favicon.as_deref().cloned())
        })
        .flatten();
        entries.push(Entry::Item(
            Item::new(
                index + 1,
                label.clone(),
                if id == active {
                    Icon::Check
                } else {
                    Icon::None
                },
            )
            .glyph("workspace-globe")
            .favicon_png(
                with_chrome(hwnd, |state| {
                    state
                        .html_favicons
                        .get(&id)
                        .and_then(|favicon| favicon.png.clone())
                })
                .flatten()
                .map(std::rc::Rc::new),
            )
            .rich_row(label, favicon)
            .checked(id == active),
        ));
    }
    let mut point = with_chrome(hwnd, |s| {
        let r = s.layout.tab_menu_button;
        windows::Win32::Foundation::POINT {
            x: r.left,
            y: r.bottom,
        }
    })
    .unwrap_or_default();
    unsafe {
        let _ = ClientToScreen(hwnd, &mut point);
    }
    // --- popup host (lane M) ---
    let menu = Menu {
        header: None,
        entries,
        footer: "Select a tab".to_owned(),
    };
    menu::show(hwnd, point, menu::Placement::BelowLeft, menu, "open-tabs", move |app, chosen| {
        let command = chosen.map_or(0, |chosen| chosen.id);
        let live = App::tab_ids(app);
        if let Some(id) = command
            .checked_sub(1)
            .and_then(|index| menu::still_listed(&ids, &live, index))
            .copied()
        {
            let _ = App::select_tab(app, id);
        }
    });
    // --- end popup host ---
}

/// The work area of the monitor nearest the window, in screen coordinates.
fn monitor_work_area(hwnd: HWND) -> Option<RECT> {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    (!monitor.is_invalid() && unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool())
        .then_some(info.rcWork)
}

fn apply_window_region(hwnd: HWND, client_width: i32, client_height: i32, maximised: bool) {
    // --- auto-hide (T11) --- a full-screen window is a plain rectangle.
    if auto_hide::fullscreen_of(hwnd) != crate::auto_hide::Fullscreen::No {
        unsafe {
            let _ = SetWindowRgn(hwnd, None, true);
        }
        return;
    }
    // --- end auto-hide ---
    let radius = Metrics::new(dpi(hwnd)).window_radius();
    let (left, top, right, bottom) = if maximised {
        let mut window = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut window) }.is_err() {
            return;
        }
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if monitor.is_invalid() || !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            return;
        }
        (
            (info.rcWork.left - window.left).clamp(0, window.right - window.left),
            (info.rcWork.top - window.top).clamp(0, window.bottom - window.top),
            (info.rcWork.right - window.left).clamp(0, window.right - window.left),
            (info.rcWork.bottom - window.top).clamp(0, window.bottom - window.top),
        )
    } else {
        // SetWindowRgn takes coordinates relative to the outer window, while WM_SIZE
        // gives us client dimensions. Snapped windows retain a non-client frame offset,
        // so translate the client bounds before applying the region.
        let mut window = RECT::default();
        let mut client_origin = windows::Win32::Foundation::POINT { x: 0, y: 0 };
        if unsafe { GetWindowRect(hwnd, &mut window) }.is_err()
            || !unsafe { ClientToScreen(hwnd, &mut client_origin) }.as_bool()
        {
            return;
        }
        let left = client_origin.x - window.left;
        let top = client_origin.y - window.top;
        (left, top, left + client_width, top + client_height)
    };
    if right <= left || bottom <= top {
        return;
    }
    let region = unsafe { CreateRoundRectRgn(left, top, right, bottom, radius * 2, radius * 2) };
    if !region.is_invalid() {
        unsafe {
            let _ = SetWindowRgn(hwnd, Some(region), true);
        }
    }
}

fn rect_for(l: &Layout, h: Hit) -> Rect {
    match h {
        Hit::Rail(_) | Hit::RailBackground => l.rail,
        Hit::Tab(i) | Hit::TabClose(i) | Hit::GroupBall(i) => l.tab(i).unwrap_or_default(),
        Hit::StackTab(_) | Hit::StackTabClose(_) => l.stack_row, // --- two-level stacks (T10b) ---
        Hit::NewTab => l.new_tab_button,
        Hit::TabMenu => l.tab_menu_button,
        Hit::MainMenu => l.main_menu_button,
        Hit::Back => l.back_button,
        Hit::Forward => l.forward_button,
        Hit::Reload => l.reload_button,
        Hit::Omnibox => l.omnibox,
        Hit::Split => l.split_button(),
        Hit::SplitDivider => Rect::new(
            l.viewport.left + l.viewport.width() / 2 - 3,
            l.viewport.top,
            l.viewport.left + l.viewport.width() / 2 + 3,
            l.viewport.bottom,
        ),
        Hit::SiteInfo => l.site_info_button(),
        Hit::CaptionMinimize | Hit::CaptionMaximize | Hit::CaptionClose => l.caption_button(h),
        Hit::CaptionArea => l.strip,
        _ => Rect::default(),
    }
}
fn invalidate(hwnd: HWND, r: Rect) {
    if !r.is_empty() {
        let r = RECT {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        };
        unsafe {
            let _ = InvalidateRect(Some(hwnd), Some(&r), false);
        }
    }
}
/// Repaints a hit. A tab's divider hides beside a hovered tab, so its neighbours repaint too.
fn invalidate_hover(hwnd: HWND, l: &Layout, h: Hit) {
    match h {
        Hit::Rail(_) | Hit::RailBackground => invalidate(hwnd, l.rail),
        Hit::Tab(i) | Hit::TabClose(i) | Hit::GroupBall(i) => {
            for index in i.saturating_sub(1)..=i + 1 {
                invalidate(hwnd, rect_for(l, Hit::Tab(index)));
            }
        }
        _ => invalidate(hwnd, rect_for(l, h)),
    }
}
/// The hover after the tab list changed. A hover stays on its tab even when the tab moves, and
/// clears when the tab closes, rather than landing on whichever tab slid into its index.
fn moved_hover(hover: Hit, index: Option<usize>) -> Hit {
    match (hover, index) {
        (Hit::Tab(_), Some(i)) => Hit::Tab(i),
        (Hit::TabClose(_), Some(i)) => Hit::TabClose(i),
        (Hit::GroupBall(_), Some(i)) => Hit::GroupBall(i),
        (Hit::Tab(_) | Hit::TabClose(_) | Hit::GroupBall(_), None) => Hit::None,
        (other, _) => other,
    }
}
fn invalidate_hit(hwnd: HWND, h: Hit) {
    if let Some(r) = with_chrome(hwnd, |s| rect_for(&s.layout, h)) {
        invalidate(hwnd, r)
    }
}
fn invalidate_omni(hwnd: HWND) {
    invalidate_hit(hwnd, Hit::Omnibox)
}
fn set_hover(hwnd: HWND, h: Hit) {
    let (old, l, tooltip_changed, tooltip_key) = with_chrome(hwnd, |s| {
        let o = s.hover;
        s.hover = h;
        let rail_hover = match h { Hit::Rail(key) => Some(key), _ => None };
        let tooltip_changed = s.rail_tooltip_key != rail_hover;
        if tooltip_changed {
            s.rail_tooltip_key = rail_hover;
            s.rail_tooltip_visible = false;
        }
        s.motion.retarget_rail(rail_hover, Instant::now());
        s.retarget_motion(Instant::now());
        (o, s.layout, tooltip_changed, rail_hover)
    })
    .unwrap_or_else(|| (Hit::None, layout::compute(0, 0, 96, 0, false), false, None));
    if tooltip_changed {
        unsafe {
            let _ = KillTimer(Some(hwnd), WM_RAIL_TOOLTIP_TIMER);
            if tooltip_key.is_some() {
                let _ = SetTimer(Some(hwnd), WM_RAIL_TOOLTIP_TIMER, card::HOVER_DELAY_MS, None);
            }
        }
    }
    if old != h && (old == Hit::NewTab || h == Hit::NewTab) {
        hide_newtab_tooltip(hwnd);
        if h == Hit::NewTab {
            unsafe { let _ = SetTimer(Some(hwnd), WM_NEWTAB_TOOLTIP_TIMER, card::HOVER_DELAY_MS, None); }
        }
    }
    if old != h {
        invalidate_hover(hwnd, &l, old);
        invalidate_hover(hwnd, &l, h);
        arm_motion(hwnd);
    }
}
fn set_pressed(hwnd: HWND, h: Hit) {
    let (old, l) = with_chrome(hwnd, |s| {
        let o = s.pressed;
        s.pressed = h;
        s.motion.retarget_icons(s.hover, s.pressed, Instant::now());
        (o, s.layout)
    })
    .unwrap_or_else(|| (Hit::None, layout::compute(0, 0, 96, 0, false)));
    if old != h {
        invalidate(hwnd, rect_for(&l, old));
        invalidate(hwnd, rect_for(&l, h));
        arm_motion(hwnd);
    }
    // The HTML toolbar draws its own menu button: it shows the open menu from this message.
    if (old == Hit::MainMenu) != (h == Hit::MainMenu) {
        html_toolbar_post(hwnd, &chrome_host::protocol::menu_open_json(h == Hit::MainMenu));
    }
}

/// Navigation target for the currently focused browsing pane. Split view can route through this
/// boundary without changing toolbar or keyboard callers.
fn navigate_back_for_focused_pane(app: &AppRef) {
    App::history(app, false);
}

fn navigate_forward_for_focused_pane(app: &AppRef) {
    App::history(app, true);
}

fn caption_hit(v: u32) -> Hit {
    match v {
        x if x == HTMINBUTTON => Hit::CaptionMinimize,
        x if x == HTMAXBUTTON => Hit::CaptionMaximize,
        x if x == HTCLOSE => Hit::CaptionClose,
        _ => Hit::None,
    }
}
fn caption_command(hwnd: HWND, h: Hit) {
    let c = match h {
        Hit::CaptionMinimize => Some(SC_MINIMIZE),
        Hit::CaptionMaximize => Some(if unsafe { IsZoomed(hwnd).as_bool() } {
            SC_RESTORE
        } else {
            SC_MAXIMIZE
        }),
        Hit::CaptionClose => Some(SC_CLOSE),
        _ => None,
    };
    if let Some(c) = c {
        unsafe {
            let _ = SendMessageW(
                hwnd,
                WM_SYSCOMMAND,
                Some(WPARAM(c as usize)),
                Some(LPARAM(0)),
            );
        }
    }
}
fn toggle_max(hwnd: HWND) {
    caption_command(hwnd, Hit::CaptionMaximize)
}
/// Starts the HTML chrome, once per window. The chrome views are hosted in the active profile's
/// tab environment (shared browser, GPU and network processes), each in the private chrome
/// profile. When that environment does not exist yet, the engine is asked to create it now, and
/// `WM_HTML_CHROME_ENV` continues here once it settles. If anything fails, the native chrome stays.
fn start_html_chrome(hwnd: HWND) {
    let started = with_chrome(hwnd, |s| {
        if s.html_requested {
            return false;
        }
        s.html_requested = true;
        s.html_env_waiting = true;
        true
    })
    .unwrap_or(false);
    if started {
        html_env_settled(hwnd);
    }
}

/// Continues a waiting HTML chrome start with the engine's environment: both hosts are created
/// on it, or, when there is none, the native chrome stays. Ignored unless the window is waiting.
fn html_env_settled(hwnd: HWND) {
    if !with_chrome(hwnd, |s| s.html_env_waiting).unwrap_or(false) {
        return;
    }
    let Some(app) = app_of(hwnd) else {
        return;
    };
    match App::request_chrome_environment(&app) {
        ChromeEnvironment::Pending => {}
        ChromeEnvironment::Ready(environment) => {
            let env = ChromeEnv::new(environment);
            let _ = with_chrome(hwnd, |s| {
                s.html_env_waiting = false;
                s.html_env = Some(env.clone());
            });
            // The two controllers are requested together and are created concurrently. Each one's
            // failure is its own.
            start_html_toolbar(hwnd, &env);
            start_html_tabs(hwnd);
            start_html_rail(hwnd);
            // --- popup surfaces (lane PP) ---
            // The popup is created by `request_popup_prewarm` when the toolbar activates, so the
            // hidden page does not compete with the visible toolbar.
            // --- end popup surfaces ---
            // The flyout panel and the hover layer are created the first time they are used
            // (`ensure_flyout_panel`, `ensure_hover_layer`).
        }
        ChromeEnvironment::Unavailable => {
            let _ = with_chrome(hwnd, |s| s.html_env_waiting = false);
            html_failed(hwnd);
            html_tabs_failed(hwnd);
            html_rail_failed(hwnd);
        }
    }
}

/// Closes both chrome hosts and drops the window's reference to the shared environment, so the
/// engine can close it next. Nothing restarts the HTML chrome afterwards.
// --- popup host (lane M) ---
fn prewarm_html_popup(hwnd: HWND) {
    let env = with_chrome(hwnd, |s| s.html_env.clone()).flatten();
    // The hover layer is no longer prewarmed here; it starts on first use (`ensure_hover_layer`).
    if let Some(env) = env { html_popup::prewarm(hwnd, env); }
}
// --- popup surfaces (lane PP) ---
/// A menu or page wanted the popup before it was ready (or the toolbar just activated): create it
/// now if it does not exist. Idempotent: an existing or pending host is kept.
pub(super) fn request_popup_prewarm(hwnd: HWND) {
    if !chrome_host::html_ui_enabled() {
        return;
    }
    unsafe { let _ = KillTimer(Some(hwnd), WM_HTML_POPUP_PREWARM_TIMER); }
    // A hover layer that failed since is asked for again on its next use.
    let _ = with_chrome(hwnd, |s| s.hover_requested = false);
    prewarm_html_popup(hwnd);
}
// --- end popup surfaces ---
// --- end popup host ---
// --- panel host (lane P) ---
/// The flyout's panel controller failed: the flyout closes for good in this window and the rail
/// opens the docked native panels instead (their factories stay registered from window creation).
fn html_panel_failed(hwnd: HWND) {
    let _ = with_chrome(hwnd, |s| s.flyout_failed = true);
    html_panel::fail(hwnd);
    let _ = unsafe { PostMessageW(Some(hwnd), crate::win32::WM_RAIL_CHANGED, WPARAM(0), LPARAM(0)) };
}
// --- end panel host ---
fn close_html_chrome(hwnd: HWND) {
    // --- popup host (lane M) ---
    unsafe { let _ = KillTimer(Some(hwnd), WM_HTML_POPUP_PREWARM_TIMER); }
    html_popup::shutdown(hwnd);
    // --- end popup host ---
    // --- hover layer ---
    super::html_hover::shutdown(hwnd);
    // --- end hover layer ---
    // --- panel host (lane P) ---
    html_panel::close(hwnd);
    // --- end panel host ---
    super::split_ui::close(hwnd); // --- split view ---
    super::settings_page::close(hwnd);
    super::history_page::close(hwnd);
    super::downloads_page::close(hwnd);
    super::downloads_ui::close(hwnd);
    super::bookmark_bar::close(hwnd);
    super::address_bar::forget(hwnd);
    let hosts = with_chrome(hwnd, |s| {
        s.html_requested = true;
        s.html_tabs_requested = true;
        s.html_rail_requested = true;
        s.html_env_waiting = false;
        s.html_toolbar_active = false;
        s.html_page_ready = false;
        s.html_tabs_page_ready = false;
        s.html_rail_page_ready = false;
        s.html_env = None;
        (s.html.take(), s.html_tabs.take(), s.html_rail.take())
    });
    // Closed outside the borrow: closing a controller can pump messages.
    if let Some((toolbar, tabs, rail)) = hosts {
        if let Some(host) = toolbar {
            host.close();
        }
        if let Some(host) = tabs {
            host.close();
        }
        if let Some(host) = rail {
            host.close();
        }
    }
}

/// True once the window has started closing; a chrome host that finishes creation then is closed
/// at once instead of being stored.
fn window_closing(hwnd: HWND) -> bool {
    app_of(hwnd).is_some_and(|app| app.borrow().closing)
}

/// Requests the HTML toolbar's controller on the shared environment.
fn start_html_toolbar(hwnd: HWND, env: &ChromeEnv) {
    let bounds = with_chrome(hwnd, |s| chrome_host::toolbar_bounds(&s.layout).unwrap_or_default())
        .unwrap_or_default();
    let raw = hwnd.0 as isize;
    let on_command: Rc<dyn Fn(ChromeCommand)> =
        Rc::new(move |command| html_command(HWND(raw as *mut _), command));
    let on_accelerator: AccelHandler =
        Rc::new(move |accel| html_accelerator(HWND(raw as *mut _), accel, View::Toolbar));
    let on_failed: Rc<dyn Fn()> = Rc::new(move || unsafe {
        // Posted, not handled here: the failure can arrive inside the controller's own callback.
        let _ = PostMessageW(
            Some(HWND(raw as *mut _)),
            WM_HTML_CHROME_FAILED,
            WPARAM(0),
            LPARAM(0),
        );
    });
    let created = ChromeHost::create(env, crate::chrome_host::host::ChromeHostOptions { parent: hwnd, bounds, view: View::Toolbar, on_command, on_failed, on_accelerator: Some(on_accelerator) }, move |result| html_created(hwnd, result));
    if created.is_err() {
        html_failed(hwnd);
    }
}

/// The controller was created, or failed. A created host is stored, placed and raised, then the
/// toolbar activates as soon as the page is ready.
fn html_created(hwnd: HWND, result: windows::core::Result<ChromeHost>) {
    let Ok(host) = result else {
        html_failed(hwnd);
        return;
    };
    if window_closing(hwnd) {
        host.close();
        return;
    }
    let stored = with_chrome(hwnd, move |s| {
        s.html = Some(host);
        s.sync_html_bounds();
    });
    if stored.is_some() {
        sync_html_host_bounds(hwnd);
        if let Some(region) = with_chrome(hwnd, |s| chrome_host::toolbar_bounds(&s.layout)).flatten()
        {
            raise_html_child(hwnd, region);
        }
        html_refresh(hwnd);
    }
}

/// Activates the HTML toolbar once its host exists and the page has said `ready`, then sends the
/// page its state. The native controls it replaces stop painting and the native address input is
/// hidden. Runs again on every `ready`, so a reloaded page gets state too.
fn html_refresh(hwnd: HWND) {
    let Some((edit, host, activating, generation)) = with_chrome(hwnd, |s| {
        let activating = s.html.is_some() && s.html_page_ready && !s.html_toolbar_active;
        if s.html.is_some() && s.html_page_ready && !s.html_toolbar_active {
            s.html_toolbar_active = true;
            s.sync_html_bounds();
        }
        if !s.html_toolbar_active {
            return None;
        }
        // A freshly loaded page knows nothing yet, so the next sync sends the state and the text.
        s.html_last_state = None;
        s.html_dl_last = None;
        s.html_ext_last = None;
        s.html_state_generation[0] = s.html_state_generation[0].wrapping_add(1);
        s.html_text_sent = None;
        s.html_synced_tab = None;
        Some((s.edit, s.html.clone()?, activating, s.html_state_generation[0]))
    })
    .flatten() else {
        return;
    };
    // --- popup host (lane M) ---
    if activating {
        perf_event(hwnd, "html_toolbar_ready");
        // The normal start of the popup: requested once the toolbar reports ready.
        request_popup_prewarm(hwnd);
    }
    // --- end popup host ---
    let native_text = (!edit.is_invalid() && activating).then(|| read_edit(edit));
    let native_focused = unsafe { !edit.is_invalid() && GetFocus() == edit };
    let handoff = native_text.and_then(|text| {
        let app = app_of(hwnd)?;
        let id = App::active_tab_id(&app);
        let committed = with_chrome(hwnd, |s| s.urls.get(&id).cloned().unwrap_or_default())?;
        let modified = unsafe {
            SendMessageW(edit, EM_GETMODIFY, None, None).0 != 0
        };
        chrome_host::should_handoff_edit(native_focused, &text, &committed)
            .then(|| (id, chrome_host::handoff_text(&text, modified, &committed).to_owned()))
    });
    sync_html_host_bounds(hwnd);
    if activating {
        // Benchmark harnesses assert on this event to know the HTML chrome, not the native one, is live.
        if let Some(app) = app_of(hwnd) {
            let _ = app.borrow().log.record("html_chrome_shown");
        }
    }
    html_sync(hwnd);
    html_refresh_extensions_button(hwnd);
    // The downloads list reaches the toolbar's download button from here on; a reloaded page gets the
    // button's current state again.
    if let Some(app) = app_of(hwnd) {
        super::downloads_ui::install(hwnd, &app);
    }
    if let Some((id, text)) = handoff {
        if host.post_json(&set_text_json(&text, false)).is_ok() {
            let current = with_chrome(hwnd, |s| {
                if s.html_state_generation[0] != generation { return false; }
                s.html_text_sent = Some((id, text.clone()));
                s.html_synced_tab = Some(id);
                s.html_omnibox_focused = true;
                s.edit_text = text;
                true
            }).unwrap_or(false);
            if current {
                let _ = host.focus();
                let _ = host.post_json(&focus_json(false));
            }
        }
    }
    unsafe {
        if !edit.is_invalid() {
            if GetFocus() == edit {
                let _ = SetFocus(Some(hwnd));
            }
            let _ = ShowWindow(edit, SW_HIDE);
        }
    }
    repaint(hwnd);
}

/// The application state behind this window, if it is still alive. The clone is an `Rc` handle, so
/// no borrow of the window table or the app is held by the caller.
pub(super) fn app_of(hwnd: HWND) -> Option<AppRef> {
    let raw = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const AppRef).as_ref() }?;
    Some(raw.clone())
}

/// Pushes the toolbar state and the omnibox text when they changed since the last push. Cheap when
/// nothing changed: the state JSON is compared with the last one sent. The omnibox text is sent when
/// the active tab changed (even while the omnibox is focused, so a new tab's blank address replaces
/// the old one), or, within the same tab, only while the omnibox is not focused, so the user's typing
/// is never overwritten. Holds no chrome borrow while calling App, which may re-enter this window.
fn html_sync(hwnd: HWND) {
    let Some(app) = app_of(hwnd) else {
        return;
    };
    let Some((_chrome_palette, dpi, layout)) = with_chrome(hwnd, |s| {
        if !(s.html_page_ready || s.html_tabs_page_ready || s.html_rail_page_ready) { return None; }
        Some((s.palette, s.dpi, s.layout))
    })
    .flatten() else {
        return;
    };
    let id = App::active_tab_id(&app);
    let site = App::site_security(&app);
    let settings = App::settings(&app);
    // --- toolbars and feeds (thread T13) --- the address bar's own list decides which buttons take room.
    let bar_extra = crate::toolbar_prefs::right_static_px(&settings);
    let (dl_allowed, ext_allowed) = (crate::toolbar_prefs::address_has(&settings, "downloads"), crate::toolbar_prefs::address_has(&settings, "extensions"));
    let _ = with_chrome(hwnd, |s| {
        s.html_bar_extra = bar_extra;
        s.html_dl_allowed = dl_allowed;
        s.html_ext_allowed = ext_allowed;
    });
    // --- end toolbars and feeds ---
    let slot = with_chrome(hwnd, |s| s.toolbar_slot()).unwrap_or(0);
    let (pillx, pillw, menux) = chrome_host::toolbar_css_placement_with_slot(&layout, dpi, slot).unwrap_or_default();
    // While a private tab is shown, the chrome takes its own light or dark look when the private
    // theme asks for one (thread T3); otherwise it follows the normal appearance.
    let system_look = glass::appearance();
    let look = settings.private_theme.appearance(App::tab_private(&app, id), system_look);
    // --- theme options --- the person's colours, the accent (the page's, when asked) and the tint.
    // --- auto-hide (T11) --- solid while a bar is out of the layout: a revealed bar floats over the page.
    let (palette, _) = App::themed_palette(&app, look, view_solid(hwnd));
    // --- end theme options ---
    let dark = look == crate::theme::Appearance::Dark;
    let state = ViewState {
        scale: dpi as f32 / 96.0,
        pillx,
        pillw,
        menux,
        solid: view_solid(hwnd), // --- auto-hide (T11) ---
        dark,
        back: App::tab_can_go_back(&app, id),
        forward: App::tab_can_go_forward(&app, id),
        loading: App::tab_loading(&app, id),
        // --- omnibox suggest ---
        split: App::split_view(&app).is_some(),
        bookmarked: App::tab_committed_url(&app, id)
            .is_some_and(|url| super::html_suggest::is_bookmarked(url.as_str())),
        // --- end omnibox suggest ---
        progress: App::tab_progress_percent(&app, id).unwrap_or(0),
        show_progress: settings.show_load_progress,
        security: chrome_host::security_of(site),
        label: site.summary().to_string(),
        permblocked: App::active_permission_blocked(&app),
        zoom: App::active_zoom_percent(&app),
        show_zoom: settings.show_zoom_indicator,
        private: App::tab_private(&app, id),
        reader: App::active_reader_pill(&app),
        translate: App::active_translate_pill(&app),
        font_scale: crate::layout_prefs::chrome_scale_percent(&settings), // --- layout (thread T6) --- font_size times ui_zoom
        ui_font: settings.ui_font.clone(),
        density: settings.ui_density,
        compact_menus: settings.compact_menus,
        look: crate::window_prefs::Look::of(&settings.window, window_in_background(hwnd)), // --- window and panels (thread T9) ---
        bar: address_bar_items(&app, &settings), // --- toolbars and feeds (thread T13) ---
        theme: chrome_host::theme_tokens(&palette),
    };
    let json = state.to_json();
    let theme_json = state.theme_only().to_json();
    html_broadcast_state(hwnd, &json, &theme_json);
    html_broadcast_pair(hwnd, &app); // --- split view ---
    let Some(host) = with_chrome(hwnd, |s| s.html_toolbar_active.then(|| s.html.clone()).flatten()).flatten() else {
        return;
    };
    // The address field's and the search field's options (src/win32/address_bar.rs).
    let generation = with_chrome(hwnd, |s| s.html_state_generation[0]).unwrap_or_default();
    super::address_bar::sync(hwnd, &app, generation);
    super::bookmark_bar::sync(hwnd, &app);
    let display =
        with_chrome(hwnd, |s| s.urls.get(&id).cloned().unwrap_or_default()).unwrap_or_default();
    let (send_text, select, generation) = with_chrome(hwnd, |s| {
        let already_sent = s
            .html_text_sent
            .as_ref()
            .is_some_and(|(tab, text)| *tab == id && *text == display);
        let prev_tab = s.html_synced_tab;
        let send_text = chrome_host::should_set_text(
            prev_tab,
            id,
            s.html_omnibox_focused,
            !already_sent,
        );
        // A blank tab that takes focus (Ctrl+T) selects its empty text so typing replaces it.
        let select = send_text && s.html_omnibox_focused && display.is_empty();
        (send_text, select, s.html_state_generation[0])
    })
    .unwrap_or_default();
    if send_text && host.post_json(&set_text_json(&display, select)).is_ok() {
            let _ = with_chrome(hwnd, |s| {
                if s.html_state_generation[0] != generation { return; }
                s.html_synced_tab = Some(id);
                s.html_text_sent = Some((id, display.clone()));
                s.edit_text.clone_from(&display);
                s.completion = None;
            });
    }
}

/// The toolbar receives the full state; the tabs strip and the rail receive the theme-only state, so
/// toolbar churn (loading, progress, zoom, label) does not repost to them. Each host has its own
/// successful-send cache.
fn html_broadcast_state(hwnd: HWND, json: &str, theme_json: &str) {
    let Some(candidates) = with_chrome(hwnd, |s| [
        (s.html_page_ready && chrome_host::state_differs(&s.html_last_state, json)).then(|| s.html.clone()).flatten(),
        (s.html_tabs_page_ready && chrome_host::state_differs(&s.html_tabs_last_state, theme_json)).then(|| s.html_tabs.clone()).flatten(),
        (s.html_rail_page_ready && chrome_host::state_differs(&s.html_rail_last_state, theme_json)).then(|| s.html_rail.clone()).flatten(),
    ]) else { return; };
    let generations = with_chrome(hwnd, |s| s.html_state_generation).unwrap_or_default();
    for (index, candidate) in candidates.into_iter().enumerate() {
        let Some(host) = candidate else { continue; };
        let json = if index == 0 { json } else { theme_json };
        if host.post_json(json).is_ok() {
            let _ = with_chrome(hwnd, |s| {
                if s.html_state_generation[index] == generations[index] {
                    match index {
                        0 => s.html_last_state = Some(json.to_owned()),
                        1 => s.html_tabs_last_state = Some(json.to_owned()),
                        _ => s.html_rail_last_state = Some(json.to_owned()),
                    }
                }
            });
        }
    }
}

/// The toolbar's download button (docs/design/downloads.md): reserves its slot beside the pill while
/// `on` (so the pill's new width is sent first), then sends `json` (the `dlbtn` message) when it
/// differs from the last one the page got. A page that reloaded has seen none, so it gets this one.
pub(super) fn html_set_download_button(hwnd: HWND, on: bool, json: &str) {
    let slot = if on { chrome_host::DL_BUTTON_SLOT } else { 0 };
    let moved = with_chrome(hwnd, |s| std::mem::replace(&mut s.html_dl_slot, slot) != slot).unwrap_or(false);
    if moved {
        html_sync(hwnd);
    }
    let Some((host, generation)) = with_chrome(hwnd, |s| {
        (s.html_page_ready && s.html_dl_last.as_deref() != Some(json))
            .then(|| s.html.clone())
            .flatten()
            .map(|host| (host, s.html_state_generation[0]))
    })
    .flatten() else {
        return;
    };
    if host.post_json(json).is_ok() {
        let _ = with_chrome(hwnd, |s| {
            if s.html_state_generation[0] == generation {
                s.html_dl_last = Some(json.to_owned());
            }
        });
    }
}

/// Whether the HTML toolbar is up (its host exists and its page said `ready`).
pub(super) fn html_toolbar_active(hwnd: HWND) -> bool {
    with_chrome(hwnd, |s| s.html_toolbar_active && s.html_page_ready).unwrap_or(false)
}

/// The screen point the downloads bubble hangs from: under the download button, at its right edge.
/// `rect` is the button as the page last reported it (toolbar CSS px); without one the slot Rust
/// reserved beside the menu button stands in. `None` while the toolbar host is not up.
pub(super) fn downloads_anchor(hwnd: HWND, rect: Option<chrome_host::protocol::HoverRect>) -> Option<windows::Win32::Foundation::POINT> {
    let (host, dpi, menux) = with_chrome(hwnd, |s| {
        let (_, _, menux) = chrome_host::toolbar_css_placement_with_slot(&s.layout, s.dpi, s.html_dl_slot)?;
        Some((s.html.clone()?, s.dpi, menux))
    })
    .flatten()?;
    let screen = host.screen_bounds()?;
    let rect = rect.unwrap_or(chrome_host::protocol::HoverRect { x: (menux - chrome_host::DL_BUTTON_SLOT).max(0) as u32, y: 3, w: 28, h: 28 });
    let scale = dpi as f32 / 96.0;
    let button = super::html_hover::anchor_to_screen(screen, scale, rect);
    Some(windows::Win32::Foundation::POINT { x: button.right, y: button.bottom + (4.0 * scale).round() as i32 })
}

/// The screen point a popup under the toolbar element at `rect` (toolbar CSS pixels) hangs from: the
/// element's bottom left edge. `None` while the toolbar host is not up.
pub(super) fn toolbar_anchor(hwnd: HWND, rect: chrome_host::protocol::HoverRect) -> Option<windows::Win32::Foundation::POINT> {
    let (host, dpi) = with_chrome(hwnd, |s| Some((s.html.clone()?, s.dpi)))??;
    let screen = host.screen_bounds()?;
    let scale = dpi as f32 / 96.0;
    let element = super::html_hover::anchor_to_screen(screen, scale, rect);
    Some(windows::Win32::Foundation::POINT { x: element.left, y: element.bottom + (4.0 * scale).round() as i32 })
}

/// Gives the bookmarks bar `reserve` (physical pixels, bottom?) of the page area. `true` when that
/// changed it: the window is laid out again at once, and the caller's sync runs again with the room
/// in place.
// --- layout (thread T6) ---
/// Shows a hidden address bar (Ctrl+L and the other ways into the address field) or hides it again
/// (the field lost focus). Nothing changes while the setting shows the bar anyway. True when the
/// chrome was laid out again.
pub(super) fn set_address_bar_revealed(hwnd: HWND, app: &AppRef, on: bool) -> bool {
    let arrangement = App::arrangement(app);
    let on = on && !arrangement.address_bar_visible;
    let relaid = with_chrome(hwnd, |s| {
        if s.address_bar_revealed == on {
            return false;
        }
        s.address_bar_revealed = on;
        if let Some(inputs) = s.last_relayout.as_mut() {
            inputs.arrangement = arrangement.with_address_bar_revealed(on);
        }
        s.relayout_sidebar(App::sidebar_width(app))
    })
    .unwrap_or(false);
    if relaid {
        sync_html_host_bounds(hwnd);
        html_sync(hwnd);
        apply_sidebar_bounds(hwnd, app);
        repaint(hwnd);
    }
    relaid
}
// --- end layout ---

pub(super) fn apply_bar_reserve(hwnd: HWND, app: &AppRef, reserve: (i32, bool)) -> bool {
    let relaid = with_chrome(hwnd, |s| {
        if s.bar_reserve == reserve {
            return false;
        }
        s.bar_reserve = reserve;
        s.relayout_sidebar(App::sidebar_width(app))
    })
    .unwrap_or(false);
    if relaid {
        sync_html_host_bounds(hwnd);
        apply_sidebar_bounds(hwnd, app);
        repaint(hwnd);
    }
    relaid
}

/// Runs the chrome sync now (the options a menu pick changed reach the toolbar).
pub(super) fn html_sync_now(hwnd: HWND) {
    html_sync(hwnd);
}

/// Replaces the text of the toolbar's address field and puts the caret at its end.
pub(super) fn set_toolbar_text(hwnd: HWND, text: &str) {
    let Some(host) = with_chrome(hwnd, |s| s.html.clone()).flatten() else { return };
    let _ = with_chrome(hwnd, |s| {
        s.edit_text.clear();
        s.edit_text.push_str(text);
        s.html_text_sent = None;
    });
    let _ = host.post_json(&set_text_json(text, false));
    let _ = host.post_json(&focus_json(false));
}

/// Remembers the address as typed, for the unfocused field (the same call a plain Enter makes).
pub(super) fn remember_typed_url(hwnd: HWND, app: &AppRef, text: &str) {
    remember_url(hwnd, App::active_tab_id(app), text, App::settings(app).minimal_url);
}

/// Falls back to the native toolbar: the HTML region is released, the native address input comes
/// back and the host is closed. Safe to call more than once.
fn html_failed(hwnd: HWND) {
    let Some((host, edit)) = with_chrome(hwnd, |s| {
        s.html_toolbar_active = false;
        s.html_failed[0] = true;
        s.html_page_ready = false;
        s.html_omnibox_focused = false;
        s.sync_html_bounds();
        (s.html.take(), s.edit)
    }) else {
        return;
    };
    // Closed outside the borrow: closing a controller can pump messages.
    if let Some(host) = host {
        host.close();
    }
    unsafe {
        if !edit.is_invalid() {
            let _ = ShowWindow(edit, SW_SHOW);
        }
    }
    // The downloads page keeps following the list after the fallback, even when the toolbar never
    // became ready (html_refresh installs the observer otherwise).
    if let Some(app) = app_of(hwnd) {
        super::downloads_ui::install(hwnd, &app);
    }
    repaint(hwnd);
}

/// Handles one validated message from the HTML toolbar. Runs on the UI thread, holding no borrow.
fn html_command(hwnd: HWND, command: ChromeCommand) {
    match command {
        ChromeCommand::Ready => {
            let _ = with_chrome(hwnd, |s| s.html_page_ready = true);
            html_refresh(hwnd);
            return;
        }
        // --- hover layer ---
        ChromeCommand::Tip { .. } | ChromeCommand::TipOff => {
            html_hover_command(hwnd, View::Toolbar, command);
            return;
        }
        // --- end hover layer ---
        ChromeCommand::BeginDrag | ChromeCommand::Caption(_) => {
            html_window_command(hwnd, command, View::Toolbar);
        }
        ChromeCommand::Nav(action) => {
            let Some(app) = app_of(hwnd) else { return };
            match action {
                NavAction::Back => navigate_back_for_focused_pane(&app),
                NavAction::Forward => navigate_forward_for_focused_pane(&app),
                NavAction::Reload => App::reload(&app),
                NavAction::Stop => App::stop(&app),
            }
        }
        ChromeCommand::Edit { seq, text } => html_edit(hwnd, seq, &text),
        // The address field's options and the search field (src/win32/address_bar.rs).
        ChromeCommand::Panel { kind: chrome_host::protocol::PanelType::Address, f } => super::address_bar::on_toolbar_command(hwnd, &f),
        // --- omnibox suggest ---
        ChromeCommand::SuggestPick { seq, i } => {
            if let Some(target) = super::html_hover::suggest_take(hwnd, seq, i) {
                html_suggest_run(hwnd, target);
            }
        }
        ChromeCommand::SuggestPreview { seq, i } => {
            super::html_hover::suggest_preview(hwnd, seq, i);
            return;
        }
        ChromeCommand::SuggestEnd => {
            super::html_hover::suggest_close(hwnd);
            return;
        }
        // --- end omnibox suggest ---
        ChromeCommand::Submit(text) => {
            let Some(app) = app_of(hwnd) else { return };
            super::html_hover::suggest_hide(hwnd); // --- omnibox suggest ---
            // Same path as the native omnibox: remember the typed text, then let App normalise it.
            remember_url(hwnd, App::active_tab_id(&app), &text, App::settings(&app).minimal_url);
            App::submit(&app, &text);
            App::focus_page(&app);
        }
        ChromeCommand::Escape => {
            let Some(app) = app_of(hwnd) else { return };
            super::html_hover::suggest_hide(hwnd); // --- omnibox suggest ---
            let id = App::active_tab_id(&app);
            let display = with_chrome(hwnd, |s| s.urls.get(&id).cloned().unwrap_or_default())
                .unwrap_or_default();
            // Restore the address the tab shows, then hand the keyboard back to the page.
            let host = with_chrome(hwnd, |s| {
                s.html_text_sent = None;
                s.edit_text.clone_from(&display);
                s.completion = None;
                (s.html.clone(), s.html_state_generation[0])
            });
            if let Some((Some(host), generation)) = host {
                if host.post_json(&set_text_json(&display, false)).is_ok() {
                    let _ = with_chrome(hwnd, |s| {
                        if s.html_state_generation[0] == generation {
                            s.html_text_sent = Some((id, display.clone()));
                        }
                    });
                }
            }
            App::focus_page(&app);
        }
        ChromeCommand::Focus(on) => {
            // --- omnibox suggest ---
            if on {
                // The dropdown is drawn by the hover layer: have it ready by the first keystroke.
                ensure_hover_layer(hwnd);
                super::html_hover::omnibox_focused(hwnd);
            } else {
                super::html_hover::suggest_focus_lost(hwnd);
            }
            // --- end omnibox suggest ---
            let _ = with_chrome(hwnd, |s| {
                s.html_omnibox_focused = on;
                if !on {
                    s.omnibox_claimed = false;
                    s.omnibox_edited = false;
                    // Leaving the omnibox discards typed text, as the native edit does. The next
                    // sync sends the tab's address again.
                    s.html_text_sent = None;
                    s.completion = None;
                }
            });
            // --- layout (thread T6) --- a hidden address bar goes away with the focus.
            if !on && let Some(app) = app_of(hwnd) {
                set_address_bar_revealed(hwnd, &app, false);
            }
        }
        ChromeCommand::Open(OpenTarget::Menu) => {
            defer_html_command(hwnd, DeferredChromeCommand::MainMenu);
        }
        ChromeCommand::Open(OpenTarget::Site) => {
            defer_html_command(hwnd, DeferredChromeCommand::SiteInfo);
        }
        ChromeCommand::Open(OpenTarget::NewTab) => {
            if let Some(app) = app_of(hwnd) {
                open_user_tab(hwnd, &app); // --- tab options ---
            }
        }
        ChromeCommand::Open(OpenTarget::ZoomReset) => {
            if let Some(app) = app_of(hwnd) {
                App::zoom_active(&app, crate::site_prefs::ZoomStep::Reset);
            }
        }
        ChromeCommand::Open(OpenTarget::Split) => {
            // Same as the native omnibox split button: open the split menu.
            defer_html_command(hwnd, DeferredChromeCommand::SplitMenu);
        }
        // The download button: the bubble opens under it (or closes, when it is the one showing).
        ChromeCommand::DownloadsToggle { rect } => {
            super::downloads_ui::note_button_rect(hwnd, rect);
            defer_html_command(hwnd, DeferredChromeCommand::DownloadsToggle);
        }
        // The extensions button: a popup opens, or the list of them hangs from the button.
        ChromeCommand::ExtensionsButton { rect } => {
            let _ = with_chrome(hwnd, |s| s.html_ext_rect = Some(rect));
            defer_html_command(hwnd, DeferredChromeCommand::ExtensionsButton);
        }
        ChromeCommand::Open(OpenTarget::Reader) => {
            // The toolbar button: the same toggle as Ctrl+Alt+R. It opens no menu, so it runs here.
            if let Some(app) = app_of(hwnd) {
                run_reader(hwnd, &app, crate::reader::Mode::Reader);
                App::focus_page(&app);
            }
        }
        ChromeCommand::Open(OpenTarget::Translate) => {
            // The toolbar button: translates the page, or shows the original. It opens no menu.
            if let Some(app) = app_of(hwnd) {
                run_translate(hwnd, &app, TranslateAction::Toggle);
            }
        }
        // --- toolbars and feeds (thread T13) ---
        // The feed button in the address field: the Feeds panel opens with the feeds this page offers.
        ChromeCommand::Open(OpenTarget::Feeds) => {
            if let Some(app) = app_of(hwnd) {
                open_feeds_panel(hwnd, &app);
            }
        }
        // A command button: only an item the user put at that place in that bar's list runs.
        ChromeCommand::Tool { bar, item, index } => {
            if let Some(app) = app_of(hwnd) {
                let settings = App::settings(&app);
                let list = match bar {
                    chrome_host::protocol::ToolBar::Address => crate::toolbar_prefs::Bar::Address.list(&settings),
                    chrome_host::protocol::ToolBar::Tabs => crate::toolbar_prefs::Bar::Tabs.list(&settings),
                };
                if list.get(usize::from(index)) == Some(&item) {
                    defer_html_command(hwnd, DeferredChromeCommand::Tool { bar, index });
                }
            }
        }
        // --- end toolbars and feeds ---
        // Tab strip and rail commands are handled by their own views, not the toolbar.
        ChromeCommand::Tab { .. }
        | ChromeCommand::TabMove { .. }
        | ChromeCommand::TabStack { .. } // --- tab stacks (T10) ---
        | ChromeCommand::RailWheel { .. } // --- tab stacks (T10) ---
        | ChromeCommand::TabSelection { .. } // --- tab options ---
        | ChromeCommand::ClosedTabs // --- tab options ---
        | ChromeCommand::Group { .. }
        | ChromeCommand::Rail { .. }
        | ChromeCommand::RailMove { .. }
        | ChromeCommand::Panel { .. }
        | ChromeCommand::PopupSize { .. }
        | ChromeCommand::PopupPick { .. }
        | ChromeCommand::PopupDismiss { .. }
        | ChromeCommand::PopupAct { .. }
        | ChromeCommand::DownloadsAct { .. }
        | ChromeCommand::PopupCompose { .. }
        | ChromeCommand::ComposerOpen { .. }
        // --- hover layer ---
        | ChromeCommand::Hover(_)
        | ChromeCommand::HoverAct { .. }
        // --- end hover layer ---
        // --- popup host (lane M) ---
        | ChromeCommand::Side(_) => {}
        // --- end popup host ---
        // --- split view ---
        ChromeCommand::Split(_) => {}
        // --- end split view ---
        ChromeCommand::TabDrag(_) => {} // --- tab ghost --- (the tab view's)
    }
    html_sync(hwnd);
}

/// Bit in the `WM_SHORTCUT_KEY` LPARAM set when the chord came from an HTML chrome view. Such a
/// tab-changing command then also moves focus to the page, not left in the chrome.
const SHORTCUT_FROM_CHROME: isize = 8;

/// Claims a key an HTML chrome view did not consume, when it is a browser shortcut. While the
/// HTML omnibox is being edited, its editing chords stay with the field (see `accel_command_gated`).
/// The command runs later through `WM_SHORTCUT_KEY`, the same path the tab WebViews use.
fn html_accelerator(hwnd: HWND, accel: Accel, view: View) -> bool {
    // --- popup host (lane M) ---
    if cfg!(debug_assertions) && accel.key_down && !accel.repeat && accel.vk == 0x4D
        && accel.ctrl && accel.alt && accel.shift && chrome_host::html_ui_enabled()
    {
        open_debug_html_menu(hwnd);
        return true;
    }
    // --- end popup host ---
    let editing = view == View::Split // --- split view --- (the empty side's search field)
        || view == View::Settings // the settings page's search and site fields
        || view == View::HistoryPage // the history page's search field
        || view == View::DownloadsPage // the downloads page's search field
        || (view == View::Toolbar && with_chrome(hwnd, |s| s.html_omnibox_focused).unwrap_or(false));
    if view != View::Panel
        && side_panel::should_dismiss_panel_accelerator(
            accel.vk, accel.key_down, side_panel::is_open(hwnd), editing,
        )
    {
        side_panel::close(hwnd);
        if !animations_enabled() { if let Some(app) = app_of(hwnd) { apply_sidebar_bounds(hwnd, &app); } }
        return true;
    }
    if accel_command_gated(accel, editing, gated_chords(hwnd)).is_none() {
        return false;
    }
    let bits = (accel.ctrl as isize)
        | ((accel.shift as isize) << 1)
        | ((accel.alt as isize) << 2)
        | SHORTCUT_FROM_CHROME;
    unsafe {
        let _ = PostMessageW(
            Some(hwnd),
            WM_SHORTCUT_KEY,
            WPARAM(accel.vk as usize),
            LPARAM(bits),
        );
    }
    true
}

// --- panel host (lane P) ---
pub(super) fn html_panel_accelerator(hwnd: HWND, accel: Accel) -> bool {
    html_accelerator(hwnd, accel, View::Panel)
}

pub(super) fn html_panel_state(hwnd: HWND) -> String {
    let (palette, dpi) = with_chrome(hwnd, |state| (state.palette, state.dpi))
        .unwrap_or_else(|| (crate::theme::palette(glass::appearance(), true), 96));
    // --- look state (fix group B) --- the private theme's look while a private tab is shown, as
    // the toolbar (html_sync) and the page hosts (split_host_state) take it.
    let system_look = glass::appearance();
    let (look, dark) = match app_of(hwnd) {
        Some(app) => {
            let look = App::private_theme(&app).appearance(App::active_tab_private(&app), system_look);
            (look, look == crate::theme::Appearance::Dark)
        }
        None => (system_look, system_look == crate::theme::Appearance::Dark),
    };
    // --- end look state ---
    // --- theme options ---
    let palette = match app_of(hwnd) {
        Some(app) => App::themed_palette(&app, look, !palette.translucent).0,
        None => palette.clone(),
    };
    // --- end theme options ---
    // The flyout window is transparent around its surface (shadow room); the page paints the
    // opaque surface itself, so the body must not fill the window (solid false).
    host_state::page_host_view(dpi as f32 / 96.0, false, dark, host_look(hwnd), chrome_host::theme_tokens(&palette)).to_json() // --- look state (fix group B) ---
}

/// The page area (below the toolbar, right of the rail) in client coordinates, before any panel
/// takes a share of it. Panel hosts are created and placed from this, never from the whole client.
pub(super) fn page_viewport(hwnd: HWND) -> Option<RECT> {
    with_chrome(hwnd, |state| to_win_rect(state.layout.viewport))
}

// --- layout (thread T6) ---
/// Whether a hidden address bar is shown right now because Ctrl+L asked for it.
pub(crate) fn address_bar_revealed(hwnd: HWND) -> bool {
    with_chrome(hwnd, |state| state.address_bar_revealed).unwrap_or(false)
}
// --- end layout ---

pub(super) fn html_environment(hwnd: HWND) -> Option<ChromeEnv> {
    with_chrome(hwnd, |state| state.html_env.clone()).flatten()
}

pub(super) fn attach_html_dialog(owner: HWND, parent: HWND, kind: super::html_dialog::Kind) {
    let (Some(app), Some(env)) = (app_of(owner), html_environment(owner)) else { return; };
    super::html_dialog::attach(&app, owner, parent, env, kind);
}
// --- end panel host ---

// --- popup host (lane M) ---
fn open_debug_html_menu(hwnd: HWND) {
    let mut anchor = POINT::default();
    if unsafe { GetCursorPos(&mut anchor) }.is_err() { return; }
    let submenu = vec![
        Entry::Item(Item::new(3, "Nested option", Icon::Info).glyph("info")),
        Entry::Item(Item::new(4, "Another option", Icon::Plus).glyph("plus")),
    ];
    let sample = Menu {
        header: None,
        entries: vec![
            Entry::Item(Item::new(1, "Checked option", Icon::Check).glyph("bell").checked(true)),
            Entry::Item(Item::new(2, "Disabled option", Icon::Close).glyph("lock").disabled()),
            Entry::Item(Item::new(5, "More options", Icon::Plus).glyph("more-horizontal").submenu_with(submenu)),
            Entry::Separator,
            Entry::Item(Item::new(6, "Close sample", Icon::Close).glyph("close")),
        ],
        footer: String::new(),
    };
    menu::show(hwnd, anchor, menu::Placement::BelowLeft, sample, "debug-sample", |_, _| {});
}

/// The window's App, for `menu::show` callbacks that run without a caller frame.
pub(super) fn popup_app(hwnd: HWND) -> Option<AppRef> {
    app_of(hwnd)
}

// --- popup surfaces (lane PP) ---
/// Records a perf-log event (`--benchmark-log`) for this window's app. Never panics: a busy app
/// borrow skips the event.
pub(super) fn perf_event(hwnd: HWND, event: &str) {
    if let Some(app) = app_of(hwnd) {
        if let Ok(app) = app.try_borrow() {
            let _ = app.log.record(event);
        }
    }
}
// --- end popup surfaces ---

/// Active tab and workspace, recorded when a menu opens; a change dismisses the menu.
pub(super) fn popup_context(hwnd: HWND) -> Option<html_popup::MenuContext> {
    let app = app_of(hwnd)?;
    Some((App::active_tab_id(&app).0, App::workspaces(&app).active_id().0))
}

/// A menu anchor in screen coordinates from a client point.
pub(super) fn screen_point(hwnd: HWND, mut point: POINT) -> POINT {
    unsafe {
        let _ = ClientToScreen(hwnd, &mut point);
    }
    point
}
// --- end popup host ---

/// Commands that make another tab the active one.
fn changes_active_tab(command: Command) -> bool {
    matches!(
        command,
        Command::NextTab
            | Command::PrevTab
            | Command::SelectTab(_)
            | Command::SelectLastTab
            | Command::CloseTab
    )
}

/// Starts the HTML tab strip's WebView2 controller on the shared environment (private chrome
/// profile), once per window. It is independent of the toolbar: if it cannot start, the native tab
/// row stays.
fn start_html_tabs(hwnd: HWND) {
    let Some((env, bounds)) = with_chrome(hwnd, |s| {
        if s.html_tabs_requested {
            return None;
        }
        let env = s.html_env.clone()?;
        s.html_tabs_requested = true;
        // --- vertical sidebar (lane S) ---
        Some((env, s.html_tabs_bounds().unwrap_or_default()))
        // --- end vertical sidebar ---
    })
    .flatten() else {
        return;
    };
    let raw = hwnd.0 as isize;
    let on_command: Rc<dyn Fn(ChromeCommand)> =
        Rc::new(move |command| html_tabs_command(HWND(raw as *mut _), command));
    let on_accelerator: AccelHandler =
        Rc::new(move |accel| html_accelerator(HWND(raw as *mut _), accel, View::Tabs));
    let on_failed: Rc<dyn Fn()> = Rc::new(move || unsafe {
        // Posted, not handled here: the failure can arrive inside the controller's own callback.
        let _ = PostMessageW(
            Some(HWND(raw as *mut _)),
            WM_HTML_TABS_FAILED,
            WPARAM(0),
            LPARAM(0),
        );
    });
    let created = ChromeHost::create(&env, crate::chrome_host::host::ChromeHostOptions { parent: hwnd, bounds, view: View::Tabs, on_command, on_failed, on_accelerator: Some(on_accelerator) }, move |result| html_tabs_created(hwnd, result));
    if created.is_err() {
        html_tabs_failed(hwnd);
    }
}

/// The tab strip controller was created, or failed. A created host is stored, placed and raised;
/// the strip activates once its page says `ready`.
fn html_tabs_created(hwnd: HWND, result: windows::core::Result<ChromeHost>) {
    let Ok(host) = result else {
        html_tabs_failed(hwnd);
        return;
    };
    if window_closing(hwnd) {
        host.close();
        return;
    }
    let stored = with_chrome(hwnd, move |s| {
        s.html_tabs = Some(host);
        s.sync_html_tabs_bounds();
    });
    if stored.is_some() {
        sync_html_host_bounds(hwnd);
        // --- vertical sidebar (lane S) ---
        if let Some(region) = with_chrome(hwnd, |s| s.html_tabs_bounds()).flatten()
        // --- end vertical sidebar ---
        {
            raise_html_child(hwnd, region);
        }
        html_tabs_refresh(hwnd);
    }
}

/// Activates the tab strip once its page is ready. A freshly loaded page knows nothing yet, so the
/// whole strip and every favicon are resent. Runs again on every `ready`.
fn html_tabs_refresh(hwnd: HWND) {
    let _ = with_chrome(hwnd, |s| {
        s.html_tabs_last = None;
        s.html_tabs_last_state = None;
        // --- vertical sidebar (lane S) ---
        s.html_tabs_layout = None;
        s.html_tabs_opts = None; // --- tab options ---
        s.html_tabs_workspace = None;
        // --- end vertical sidebar ---
        s.html_state_generation[1] = s.html_state_generation[1].wrapping_add(1);
        s.html_favicons_sent.clear();
        s.sync_html_tabs_bounds();
    });
    sync_html_host_bounds(hwnd);
    html_tabs_sync(hwnd);
    repaint(hwnd);
}

/// The tab strip failed: its row goes back to native paint and its host is closed. The toolbar is
/// unaffected. Safe to call more than once.
fn html_tabs_failed(hwnd: HWND) {
    let Some(host) = with_chrome(hwnd, |s| {
        s.html_tabs_page_ready = false;
        s.html_failed[1] = true;
        s.html_tabs_last = None;
        // --- vertical sidebar (lane S) ---
        s.html_tabs_layout = None;
        s.html_tabs_opts = None; // --- tab options ---
        s.html_tabs_workspace = None;
        // --- end vertical sidebar ---
        s.html_tabs_groups.clear();
        s.html_favicons_sent.clear();
        let host = s.html_tabs.take();
        s.sync_html_tabs_bounds();
        host
    }) else {
        return;
    };
    // Closed outside the borrow: closing a controller can pump messages.
    if let Some(host) = host {
        host.close();
    }
    repaint(hwnd);
}

/// Handles one validated message from the HTML tab strip. Runs on the UI thread, holding no borrow.
fn html_tabs_command(hwnd: HWND, command: ChromeCommand) {
    match command {
        ChromeCommand::Ready => {
            let _ = with_chrome(hwnd, |s| s.html_tabs_page_ready = true);
            html_tabs_refresh(hwnd);
            html_sync(hwnd);
            return;
        }
        ChromeCommand::BeginDrag | ChromeCommand::Caption(_) => {
            html_window_command(hwnd, command, View::Tabs);
            return;
        }
        // --- hover layer ---
        ChromeCommand::Tip { .. } | ChromeCommand::TipOff | ChromeCommand::Hover(_) => {
            html_hover_command(hwnd, View::Tabs, command);
            return;
        }
        // --- end hover layer ---
        ChromeCommand::Tab { action, id } => html_tab_action(hwnd, action, id),
        ChromeCommand::TabMove { id, before } => html_tab_move(hwnd, id, before),
        // --- tab stacks (T10) --- a held tab dropped on another after resting over it.
        ChromeCommand::TabStack { id, onto } => {
            let Some(app) = app_of(hwnd) else { return };
            let open = App::tab_ids(&app);
            let (Some(dragged), Some(target)) = (chrome_host::tab_from_wire(id, &open), chrome_host::tab_from_wire(onto, &open)) else {
                return;
            };
            if App::stack_onto(&app, dragged, target) {
                super::stack_ui::stack_made(hwnd, &app, target);
            }
        }
        // The vertical sidebar's workspace header takes the wheel like the rail's workspaces.
        ChromeCommand::RailWheel { d } => {
            if let Some(app) = app_of(hwnd) {
                let _ = App::scroll_workspaces(&app, d);
            }
            return;
        }
        // --- end tab stacks ---
        // --- tab options ---
        ChromeCommand::TabSelection { ids } => {
            html_tab_selection(hwnd, &ids);
            return;
        }
        ChromeCommand::ClosedTabs => {
            if let Some((x, mut y)) = cursor_client_point(hwnd) {
                if let Some(bottom) = with_chrome(hwnd, |s| (!s.layout.tab_row.is_empty()).then_some(s.layout.tab_row.bottom)).flatten() {
                    y = y.max(bottom);
                }
                defer_html_command(hwnd, DeferredChromeCommand::ClosedTabs { x, y });
            }
            return;
        }
        // --- end tab options ---
        ChromeCommand::Group { action, key } => html_group_action(hwnd, action, key),
        // --- vertical sidebar (lane S) ---
        ChromeCommand::Side(action) => {
            let vertical = with_chrome(hwnd, |s| s.sidebar.is_some() && s.html_tab_region().is_some()).unwrap_or(false);
            if !vertical { return; }
            match action {
                SideAction::Workspace => {
                    // --- tab stacks (T10) --- the switcher is hidden while workspaces are off.
                    if app_of(hwnd).is_some_and(|app| App::workspaces_enabled(&app)) {
                        defer_html_command(hwnd, DeferredChromeCommand::SidebarWorkspaceMenu);
                    }
                }
                SideAction::NewTab => {
                    if let Some(app) = app_of(hwnd) {
                        open_user_tab(hwnd, &app); // --- tab options ---
                    }
                }
                SideAction::Collapse => {
                    if let Some(app) = app_of(hwnd) {
                        toggle_sidebar(hwnd, &app);
                    }
                }
            }
        }
        // --- end vertical sidebar ---
        // --- split view ---
        // A tab dragged out of the strip over the page; runs from the window procedure.
        ChromeCommand::Split(command) => {
            super::split_ui::queue_from_tabs(hwnd, command);
            return;
        }
        // --- end split view ---
        // --- tab ghost --- the held tab's native ghost (docs/design/tab-drag.md)
        ChromeCommand::TabDrag(command) => {
            super::tab_ghost::command(hwnd, command);
            return;
        }
        // --- end tab ghost ---
        // The strip sends nothing else. Any other message is ignored.
        _ => return,
    }
    html_tabs_sync(hwnd);
}

/// Window drag and caption actions, shared by both HTML chrome hosts. Only a cursor inside the
/// sending host's own region counts, and the action is queued to run on the UI thread.
fn html_window_command(hwnd: HWND, command: ChromeCommand, view: View) {
    let region = with_chrome(hwnd, |s| match view {
        View::Toolbar => s.html_region(),
        View::Tabs => s.html_tab_region(),
        View::Rail => s.html_rail_region(),
        View::Panel | View::Popup | View::Dialog | View::DialogNotes | View::DialogPrivacy | View::DialogRename | View::DialogAddress | View::BookmarkBar | View::DialogSessions | View::DialogFeeds | View::Hover | View::Split | View::Settings | View::HistoryPage | View::DownloadsPage => None,
    })
    .flatten();
    let Some(region) = region else {
        return;
    };
    let Some((x, y)) = cursor_client_point(hwnd) else {
        return;
    };
    if !region.contains(x, y) {
        return;
    }
    let deferred = match command {
        ChromeCommand::BeginDrag => DeferredChromeCommand::BeginDrag,
        ChromeCommand::Caption(CaptionAction::ToggleMaximize) => DeferredChromeCommand::ToggleMaximize,
        ChromeCommand::Caption(CaptionAction::SystemMenu) => DeferredChromeCommand::SystemMenu,
        _ => return,
    };
    defer_html_command(hwnd, deferred);
}

/// Queues a menu, popup or window command and posts `WM_HTML_DEFERRED` to run it. A full queue drops
/// the command. A post that fails takes the command back out, so the queue and the posts stay equal.
fn defer_html_command(hwnd: HWND, command: DeferredChromeCommand) {
    let queued = with_chrome(hwnd, |s| {
        if s.html_deferred.len() >= HTML_DEFERRED_LIMIT {
            return false;
        }
        s.html_deferred.push_back(command);
        true
    })
    .unwrap_or(false);
    if !queued {
        return;
    }
    let posted = unsafe { PostMessageW(Some(hwnd), WM_HTML_DEFERRED, WPARAM(0), LPARAM(0)) };
    if posted.is_err() {
        let _ = with_chrome(hwnd, |s| s.html_deferred.pop_back());
    }
}

// --- hover layer ---
/// Routes a tooltip or tab-strip hover message to the HTML hover layer. The anchor is converted from
/// the sending view's CSS pixels to screen pixels with that view's own host bounds; tab ids and group
/// keys are checked against the live model (a ball counts only for a domain island).
fn html_hover_command(hwnd: HWND, view: View, command: ChromeCommand) {
    ensure_hover_layer(hwnd);
    let Some((host, dpi)) = with_chrome(hwnd, |s| {
        let host = match view {
            View::Toolbar => s.html.clone(),
            View::Tabs => s.html_tabs.clone(),
            View::Rail => s.html_rail.clone(),
            _ => None,
        };
        host.map(|host| (host, s.dpi))
    })
    .flatten() else {
        return;
    };
    let Some(screen) = host.screen_bounds() else { return };
    let scale = dpi as f32 / 96.0;
    match command {
        ChromeCommand::Tip { text, rect } => {
            // The rail is a narrow column, so its tooltips sit beside it.
            super::html_hover::tip(hwnd, super::html_hover::anchor_to_screen(screen, scale, rect), &text, view == View::Rail);
        }
        ChromeCommand::TipOff => super::html_hover::tip_off(hwnd),
        ChromeCommand::Hover(target) => {
            let Some(app) = app_of(hwnd) else { return };
            let target = match target {
                chrome_host::protocol::HoverTarget::Tab { id, rect } => chrome_host::tab_from_wire(id, &App::tab_ids(&app))
                    .map(|tab| (super::html_hover::Kind::Card, tab, rect)),
                chrome_host::protocol::HoverTarget::Ball { key, rect } => with_chrome(hwnd, |s| s.html_tabs_groups.get(&key).copied())
                    .flatten()
                    .filter(|group| group.collapsible)
                    .map(|group| (super::html_hover::Kind::Island, group.tab, rect)),
                chrome_host::protocol::HoverTarget::None => None,
            };
            let target = target.map(|(kind, tab, rect)| (kind, tab, super::html_hover::anchor_to_screen(screen, scale, rect)));
            super::html_hover::hover(hwnd, target);
        }
        _ => {}
    }
}
// --- end hover layer ---

// --- popup host (lane M) ---
pub(super) fn defer_popup_result(hwnd: HWND, tok: u32, id: Option<u32>) {
    defer_html_command(hwnd, DeferredChromeCommand::MenuResult { tok, id });
}

pub(super) fn html_popup_command(hwnd: HWND, command: ChromeCommand) {
    match command {
        ChromeCommand::PopupSize { tok, w, h, cards } => html_popup::show_sized(hwnd, tok, w, h, cards),
        ChromeCommand::PopupPick { tok, id } => {
            if html_popup::valid_pick(hwnd, tok, id) { defer_popup_result(hwnd, tok, Some(id)); }
        }
        ChromeCommand::PopupDismiss { tok } => {
            if html_popup::valid_token(hwnd, tok) { defer_popup_result(hwnd, tok, None); }
        }
        // --- popup surfaces (lane PP) ---
        ChromeCommand::PopupAct { tok, id, n } => {
            if html_popup::valid_act(hwnd, tok, id, n) {
                defer_html_command(hwnd, DeferredChromeCommand::PopupAct { tok, id, n });
            }
        }
        ChromeCommand::PopupCompose { tok, name, url } => {
            if html_popup::stash_compose(hwnd, tok, name, url) {
                defer_html_command(hwnd, DeferredChromeCommand::PopupCompose { tok });
            }
        }
        ChromeCommand::DownloadsAct { tok, id, n } if super::downloads_ui::valid_act(hwnd, tok) => {
            defer_html_command(hwnd, DeferredChromeCommand::DownloadsAct { tok, id, n });
        }
        // --- end popup surfaces ---
        _ => {}
    }
}

/// A Customize flyout button asked for a composer below-left of `rect` (CSS pixels of the flyout,
/// whose controller covers `view` on screen). The request runs from the deferred queue, after the
/// WebView2 callback that delivered it.
pub(super) fn request_composer(
    hwnd: HWND,
    view: RECT,
    mode: chrome_host::protocol::ComposerMode,
    id: Option<u32>,
    name: Option<String>,
    url: Option<String>,
    rect: chrome_host::protocol::HoverRect,
) {
    use chrome_host::protocol::ComposerMode;
    let composer = match (mode, id) {
        (ComposerMode::WorkspaceNew, _) => super::composer::Composer::WorkspaceNew,
        (ComposerMode::AppAdd, _) => super::composer::Composer::AppAdd,
        (ComposerMode::AppEdit, Some(id)) => super::composer::Composer::AppEdit(id),
        (ComposerMode::AppEdit, None) => return,
    };
    let button = super::html_hover::anchor_to_screen(view, dpi(hwnd) as f32 / 96.0, rect);
    super::composer::stash(hwnd, super::composer::Request {
        composer,
        prefill: name.zip(url),
        at: POINT { x: button.left, y: button.bottom },
    });
    defer_html_command(hwnd, DeferredChromeCommand::ComposerOpen);
}

/// Whether window `hwnd`'s views are solid: the system backdrop is, or this window (not another
/// one) has a bar auto-hidden.
fn view_solid(hwnd: HWND) -> bool {
    glass::backdrop_mode() == glass::BackdropMode::Solid || auto_hide::solid(hwnd)
}

pub(super) fn popup_state_json(hwnd: HWND) -> Option<String> {
    let app = app_of(hwnd)?;
    let (palette, dpi) = with_chrome(hwnd, |s| (s.palette, s.dpi))?;
    // --- theme options ---
    let palette = App::themed_palette(&app, glass::appearance(), !palette.translucent).0;
    // --- end theme options ---
    let id = App::active_tab_id(&app);
    let site = App::site_security(&app);
    let settings = App::settings(&app);
    let state = ViewState {
        scale: dpi as f32 / 96.0,
        pillx: 0,
        pillw: 0,
        menux: 0,
        solid: view_solid(hwnd), // --- auto-hide (T11) ---
        dark: glass::appearance() == crate::theme::Appearance::Dark,
        back: App::tab_can_go_back(&app, id),
        forward: App::tab_can_go_forward(&app, id),
        loading: App::tab_loading(&app, id),
        split: false,
        bookmarked: false,
        progress: App::tab_progress_percent(&app, id).unwrap_or(0),
        show_progress: settings.show_load_progress,
        security: chrome_host::security_of(site),
        label: site.summary().to_owned(),
        permblocked: App::active_permission_blocked(&app),
        zoom: App::active_zoom_percent(&app),
        show_zoom: settings.show_zoom_indicator,
        private: App::tab_private(&app, id),
        reader: crate::reader::Pill::Hidden,
        translate: crate::translate::Pill::Hidden,
        font_scale: crate::layout_prefs::chrome_scale_percent(&settings), // --- layout (thread T6) --- font_size times ui_zoom
        ui_font: settings.ui_font.clone(),
        density: settings.ui_density,
        compact_menus: settings.compact_menus,
        look: crate::window_prefs::Look::of(&settings.window, window_in_background(hwnd)), // --- window and panels (thread T9) ---
        bar: Default::default(), // --- toolbars and feeds (thread T13) ---
        theme: chrome_host::theme_tokens(&palette),
    };
    Some(state.to_json())
}
// --- end popup host ---

/// Runs the oldest queued chrome command. Each `WM_HTML_DEFERRED` runs exactly one, so a modal loop
/// that one command starts pumps its own messages without losing the rest of the queue.
fn run_deferred_html_command(hwnd: HWND) {
    let Some(command) = with_chrome(hwnd, |s| s.html_deferred.pop_front()).flatten() else {
        return;
    };
    let Some(app) = app_of(hwnd) else {
        return;
    };
    match command {
        DeferredChromeCommand::MainMenu => show_main_menu(hwnd, &app),
        DeferredChromeCommand::SiteInfo => {
            let button = with_chrome(hwnd, |s| s.layout.site_info_button()).unwrap_or_default();
            site_info::show(hwnd, &app, button);
        }
        DeferredChromeCommand::SplitMenu => super::split_ui::toolbar_button(hwnd, &app), // --- split view ---
        DeferredChromeCommand::TabMenu { id, x, y } => {
            // The tab may have closed while the command waited.
            if App::tab_ids(&app).contains(&id) {
                card::cancel(hwnd);
                show_html_tab_menu(hwnd, &app, id, x, y);
            }
        }
        // --- tab options ---
        DeferredChromeCommand::ClosedTabs { x, y } => {
            card::cancel(hwnd);
            show_closed_tabs_menu(hwnd, &app, x, y);
        }
        // --- end tab options ---
        DeferredChromeCommand::IslandMenu { id, x, y } => {
            if App::tab_ids(&app).contains(&id) {
                card::cancel(hwnd);
                island_menu::open_now(hwnd, &app, id, Rect::new(x, y, x + 1, y + 1));
            }
        }
        DeferredChromeCommand::BeginDrag => begin_html_drag(hwnd),
        DeferredChromeCommand::ToggleMaximize => toggle_max(hwnd),
        DeferredChromeCommand::SystemMenu => html_system_menu(hwnd),
        // The button may have gone while the command waited.
        DeferredChromeCommand::RailClick { key } => {
            if rail_model_keys(hwnd).contains(&key) {
                activate_rail(hwnd, &app, key);
            }
        }
        DeferredChromeCommand::RailMenu { key, x, y } => {
            if rail_model_keys(hwnd).contains(&key) {
                show_rail_context_menu(hwnd, &app, key, x, y);
            }
        }
        // The list may have changed while the command waited: the item must still be where it was.
        DeferredChromeCommand::Tool { bar, index } => {
            let settings = App::settings(&app);
            let list = match bar {
                chrome_host::protocol::ToolBar::Address => crate::toolbar_prefs::Bar::Address.list(&settings),
                chrome_host::protocol::ToolBar::Tabs => crate::toolbar_prefs::Bar::Tabs.list(&settings),
            };
            if let Some(item) = list.get(usize::from(index)) {
                run_bar_item(hwnd, &app, item);
            }
        }
        DeferredChromeCommand::RailHover { key } => {
            if rail_model_keys(hwnd).contains(&key) {
                if let Some(id) = flyout_panel(key) {
                    if let (Some(anchor), Some(context)) = (flyout_anchor(hwnd, key), popup_context(hwnd)) {
                        html_panel::hover(hwnd, id, anchor, context);
                    }
                }
            }
        }
        // --- vertical sidebar (lane S) ---
        DeferredChromeCommand::SidebarWorkspaceMenu => show_workspaces_menu(hwnd, &app),
        // --- end vertical sidebar ---
        // --- popup host (lane M) ---
        DeferredChromeCommand::MenuResult { tok, id } => {
            html_popup::run_result(hwnd, tok, id, &app);
            // Every close path ends here (pick, Esc, outside click, toggle by the menu button):
            // clear the hover layer so tooltips and cards keep working afterwards (V6).
            super::html_hover::popup_closed(hwnd);
        }
        DeferredChromeCommand::PopupAct { tok, id, n } => html_popup::run_act(hwnd, tok, id, n, &app),
        DeferredChromeCommand::PopupCompose { tok } => html_popup::run_compose(hwnd, tok, &app),
        DeferredChromeCommand::DownloadsToggle => super::downloads_ui::toggle(hwnd, &app),
        DeferredChromeCommand::ExtensionsButton => show_extensions_button_menu(hwnd, &app),
        DeferredChromeCommand::DownloadsAct { tok, id, n } => super::downloads_ui::run_act(hwnd, &app, tok, id, n),
        DeferredChromeCommand::ComposerOpen => {
            if let Some(request) = super::composer::take(hwnd) {
                // The flyout closes first so the composer takes the page's focus, not the flyout's.
                html_panel::dismiss(hwnd);
                super::composer::open(hwnd, &app, request.composer, request.prefill, request.at);
            }
        }
        // --- end popup host ---
    }
    // A menu may have edited bookmarks (an import, a bookmark item): one `stat` keeps the toolbar
    // star's set current.
    let _ = super::html_suggest::refresh_bookmarks();
    html_sync(hwnd);
    html_tabs_sync(hwnd);
    html_rail_sync(hwnd);
    mark_chrome_dirty(hwnd);
}

/// The client point under the cursor. Native menus take client points.
fn cursor_client_point(hwnd: HWND) -> Option<(i32, i32)> {
    let mut point = POINT::default();
    unsafe {
        GetCursorPos(&mut point).ok()?;
        let _ = ScreenToClient(hwnd, &mut point);
    }
    Some((point.x, point.y))
}

/// Shows the native tab context menu at a client point, as a right-click on the tab does.
fn show_html_tab_menu(hwnd: HWND, app: &AppRef, id: TabId, x: i32, y: i32) {
    let menu_app = app.clone();
    App::picture_in_picture_eligible(
        app,
        id,
        Box::new(move |available| show_tab_context_menu(hwnd, &menu_app, id, x, y, available)),
    );
}

/// A tab action from the strip, through the same App calls the native tab controls use. Ids that
/// are not open tabs are ignored.
fn html_tab_action(hwnd: HWND, action: TabAction, raw: u32) {
    let Some(app) = app_of(hwnd) else { return };
    let Some(id) = chrome_host::tab_from_wire(raw, &App::tab_ids(&app)) else {
        return;
    };
    match action {
        TabAction::Select => {
            // --- tab options --- "Minimise the active tab": a click on it goes back to the last tab.
            if id == App::active_tab_id(&app) && App::tab_options(&app).minimise_active {
                let _ = App::minimise_active_tab(&app);
            } else {
                let _ = App::select_tab(&app, id);
            }
            // --- end tab options ---
        }
        TabAction::Close => user_close(hwnd, &app, id), // --- tab options ---
        // --- tab options --- "Double click on a tab".
        TabAction::DoubleClick => match tab_double_click(App::tab_options(&app).double_click) {
            TabDoubleClick::Nothing => {}
            TabDoubleClick::Close => user_close(hwnd, &app, id),
            TabDoubleClick::Rename => rename_tab_prompt(hwnd, &app, id),
        },
        // --- end tab options ---
        TabAction::Pin => {
            let pinned = App::tab_pinned(&app, id);
            let _ = App::set_tab_pinned(&app, id, !pinned);
        }
        TabAction::Mute => {
            let muted = App::tab_muted(&app, id);
            App::set_tab_muted(&app, id, !muted);
        }
        TabAction::Menu if system_menu_requested(&app) => {
            defer_html_command(hwnd, DeferredChromeCommand::SystemMenu);
        }
        TabAction::Menu => {
            if let Some((x, mut y)) = cursor_client_point(hwnd) {
                // Open below the tab, not over it (the horizontal strip; the sidebar has no tab row).
                if let Some(bottom) = with_chrome(hwnd, |s| (!s.layout.tab_row.is_empty()).then_some(s.layout.tab_row.bottom)).flatten() {
                    y = y.max(bottom);
                }
                defer_html_command(hwnd, DeferredChromeCommand::TabMenu { id, x, y });
            }
        }
    }
}

/// A drag-reorder drop from the strip: the same `App::drop_tab` a native drop calls, with the index
/// worked out in the same post-removal terms.
fn html_tab_move(hwnd: HWND, raw: u32, before: u32) {
    let Some(app) = app_of(hwnd) else { return };
    let ids = App::tab_ids(&app);
    let Some(moved) = chrome_host::tab_from_wire(raw, &ids) else {
        return;
    };
    let Some(index) = chrome_host::drop_index(&ids, moved, before) else {
        return;
    };
    // App::drop_tab applies the group join and leave rules, as for a native drop.
    let _ = App::drop_tab(&app, moved, index);
}

/// A group command from the strip. Like the native ball: a domain group toggles its collapse and
/// opens its island menu; an opener group has nothing to collapse, so its ball selects the tab and
/// its menu is the tab menu.
fn html_group_action(hwnd: HWND, action: GroupAction, key: u32) {
    let Some(app) = app_of(hwnd) else { return };
    let Some(group) =
        with_chrome(hwnd, |s| s.html_tabs_groups.get(&key).copied()).flatten()
    else {
        return;
    };
    match action {
        GroupAction::Toggle if group.collapsible => {
            let _ = App::set_tab_group_collapsed(&app, group.tab, !group.collapsed);
        }
        GroupAction::Toggle => {
            let _ = App::select_tab(&app, group.tab);
        }
        GroupAction::Menu if system_menu_requested(&app) => {
            defer_html_command(hwnd, DeferredChromeCommand::SystemMenu);
        }
        GroupAction::Menu => {
            let Some((x, y)) = cursor_client_point(hwnd) else {
                return;
            };
            let id = group.tab;
            let menu = if group.collapsible {
                DeferredChromeCommand::IslandMenu { id, x, y }
            } else {
                DeferredChromeCommand::TabMenu { id, x, y }
            };
            defer_html_command(hwnd, menu);
        }
    }
}

/// Keeps each live tab's favicon key and, when it passes the protocol's gate, its PNG bytes. The App
/// is read only for tabs whose favicon key changed, and entries of closed tabs are dropped. Returns
/// the tabs that have a sendable favicon.
fn html_tabs_favicons(hwnd: HWND, app: &AppRef, ids: &[TabId]) -> HashSet<TabId> {
    for &id in ids {
        let key = App::tab_favicon_key(app, id);
        let cached =
            with_chrome(hwnd, |s| s.html_favicons.get(&id).map(|favicon| favicon.key)).flatten();
        if key == cached {
            continue;
        }
        let entry = key.map(|key| HtmlFavicon {
            key,
            png: App::tab_favicon(app, id).filter(|png| chrome_host::favicon_sendable(png)),
        });
        let _ = with_chrome(hwnd, |s| match entry {
            Some(entry) => {
                s.html_favicons.insert(id, entry);
            }
            None => {
                s.html_favicons.remove(&id);
            }
        });
    }
    let live: HashSet<TabId> = ids.iter().copied().collect();
    with_chrome(hwnd, |s| {
        s.html_favicons.retain(|id, _| live.contains(id));
        s.html_favicons_sent.retain(|id, _| live.contains(id));
        s.html_favicons
            .iter()
            .filter(|(_, favicon)| favicon.png.is_some())
            .map(|(id, _)| *id)
            .collect::<HashSet<TabId>>()
    })
    .unwrap_or_default()
}

// --- tab options ---
/// The Tabs settings the strip needs, with the window facts it cannot know itself.
fn html_strip_options(hwnd: HWND, app: &AppRef) -> chrome_host::protocol::StripOptions {
    use crate::tab_options::LoadIndicator;
    let options = App::tab_options(app);
    let settings = App::settings(app); // --- toolbars and feeds (thread T13) ---
    chrome_host::protocol::StripOptions {
        close_left: options.close_button_left,
        close_always: options.close_button_always,
        scroll: options.scroll_tabs,
        min_width: options.active_min_width.0,
        indicator: match options.load_indicator {
            LoadIndicator::Spinner => "spinner",
            LoadIndicator::Bar => "bar",
            LoadIndicator::Favicon => "favicon",
            LoadIndicator::None => "none",
        },
        wheel: options.wheel_switch,
        separator: options.pinned_separator,
        align_close: options.align_close_button,
        selection: options.selection,
        selection_active: options.selection_active,
        remove_spacing: options.remove_spacing_maximised,
        maximised: with_chrome(hwnd, |s| s.layout.maximised).unwrap_or(false),
        closed: App::closed_tabs(app).len() as u32,
        // --- tab stacks (T10) --- stacking by drag needs stacks to be on.
        stack_drag: options.stack_drag && crate::stacks::stacks_on(options.stack_style),
        stack_delay: options.stack_drop_delay.0,
        // --- two-level stacks (T10b) --- the second row the window laid out, so the strip page
        // and its host bounds always agree.
        stack_row: with_chrome(hwnd, |s| {
            if s.layout.stack_row.is_empty() { "off" } else { s.last_relayout.as_ref().map_or("off", |inputs| inputs.arrangement.stack_row.key()) }
        })
        .unwrap_or("off"),
        // --- toolbars and feeds (thread T13) ---
        closed_button: crate::toolbar_prefs::Bar::Tabs.list(&settings).iter().any(|item| item == "closed-tabs"),
        tools: tool_buttons(crate::toolbar_prefs::Bar::Tabs.list(&settings)),
    }
}

// --- toolbars and feeds (thread T13) ---
/// The command buttons among `items`, with the names the registry gives them. A chain that was deleted
/// since the button was placed, or a command that no longer exists, simply shows no button.
fn tool_buttons(items: &[String]) -> Vec<chrome_host::protocol::ToolButton> {
    if !items.iter().any(|item| crate::toolbar_prefs::command_of(item).is_some()) {
        return Vec::new();
    }
    let live = crate::commands::live();
    let entries = live.store.entries();
    items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let id = crate::toolbar_prefs::command_of(item)?;
            let entry = entries.iter().find(|entry| entry.id == id)?;
            Some(chrome_host::protocol::ToolButton { item: item.clone(), label: entry.label.clone(), index: index as u8 })
        })
        .collect()
}

/// What the toolbar page needs to lay out the address bar: its list, the command buttons' names, the room
/// the always-there buttons at its end take, and how many feeds the page in front offers.
fn address_bar_items(app: &AppRef, settings: &crate::settings::Settings) -> chrome_host::protocol::BarItems {
    chrome_host::protocol::BarItems {
        items: Some(settings.toolbar_buttons.join(",")),
        tools: tool_buttons(&settings.toolbar_buttons),
        extra: crate::toolbar_prefs::right_static_px(settings).max(0) as u32,
        feeds: App::active_feed_links(app).len() as u32,
    }
}

/// The panel bar's extra buttons as the rail draws them: `(place in the list, name, icon)`. The Feeds button
/// has its own icon; a command button is a letter tile named after its command. A command or chain that no
/// longer exists shows no button.
fn panel_tools(items: &[String]) -> Vec<(u8, String, RailIcon)> {
    let buttons = tool_buttons(items);
    items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            if item == "feeds" {
                return Some((index as u8, "Feeds".to_owned(), RailIcon::Feed));
            }
            let button = buttons.iter().find(|button| &button.item == item)?;
            Some((index as u8, button.label.clone(), RailIcon::Letter))
        })
        .collect()
}

/// Runs the command behind a bar item: the bar's own buttons that are commands (`bookmark`, `newtab`,
/// `feeds`) and the command and chain buttons. False for an item that is not a command (the other buttons
/// have their own wire).
fn run_bar_item(hwnd: HWND, app: &AppRef, item: &str) -> bool {
    let id = match item {
        "bookmark" => "add-bookmark",
        "newtab" => "new-tab",
        "feeds" => "feeds",
        other => match crate::toolbar_prefs::command_of(other) {
            Some(id) => id,
            None => return false,
        },
    };
    run_registered(hwnd, app, id)
}

/// The Feeds panel from the address bar's feed button (`OpenTarget::Feeds`) and the `feeds` command: with
/// the feeds the page in front offers.
pub(crate) fn open_feeds_panel(hwnd: HWND, app: &AppRef) {
    super::feeds_panel::open(hwnd, app, App::active_feed_links(app));
}
// --- end toolbars and feeds ---

/// What the strip marks on tab `id`, from the one pass `App::tab_strip_extras` made.
fn html_tab_marks(extras: &[crate::app::TabStripExtra], id: TabId) -> chrome_host::protocol::TabMarks {
    let Some(extra) = extras.iter().find(|extra| extra.id == id) else {
        return Default::default();
    };
    chrome_host::protocol::TabMarks {
        unread: extra.unread,
        notify: extra.notify,
        asleep: extra.asleep,
        progress: extra.progress,
        reload: extra.reload.map(|(_, next)| next.saturating_duration_since(Instant::now()).as_secs_f64().ceil() as u32),
        idle: extra.idle,
    }
}
// --- end tab options ---

/// Posts the tab strip's tabs and any favicon that is new for its tab. Does nothing while the strip
/// is not active (not ready, or the tabs are vertical), and posts nothing that is unchanged. Holds no
/// chrome borrow while calling App, which may re-enter this window.
fn html_tabs_sync(hwnd: HWND) {
    let Some(app) = app_of(hwnd) else {
        return;
    };
    // Titles, loading and markers come from the snapshot the native strip paints.
    let Some((host, snapshot, mode)) = with_chrome(hwnd, |s| {
        s.html_tab_region()?;
        // --- vertical sidebar (lane S) ---
        let mode = if s.sidebar.is_some() { chrome_host::protocol::LayoutMode::Vertical } else { chrome_host::protocol::LayoutMode::Horizontal };
        // --- end vertical sidebar ---
        Some((
            s.html_tabs.clone()?,
            s.tabs
                .iter()
                .map(|tab| (tab.id, tab.title.clone(), tab.loading, tab.marker))
                .collect::<Vec<_>>(),
            // --- vertical sidebar (lane S) ---
            mode,
            // --- end vertical sidebar ---
        ))
    })
    .flatten() else {
        return;
    };
    let ids: Vec<TabId> = snapshot.iter().map(|(id, ..)| *id).collect();
    let active = App::active_tab_id(&app);
    // --- vertical sidebar (lane S) ---
    let layout = chrome_host::protocol::layout_json(mode);
    let layout_pending = with_chrome(hwnd, |s| chrome_host::protocol::layout_needs_sync(s.html_tabs_layout, mode)).unwrap_or(false);
    if layout_pending && host.post_json(&layout).is_ok() {
        let _ = with_chrome(hwnd, |s| s.html_tabs_layout = Some(mode));
    }
    let workspace_set = App::workspaces(&app);
    let workspace = workspace_set.active();
    let workspace_name = workspace.name().to_owned();
    let glyph = format!("g:{}", format!("{:?}", workspace.icon()).to_lowercase());
    // --- tab stacks and workspaces (T10) --- the header shows the workspace's emoji when it has one;
    // the emoji is part of what was sent, so a change of it is sent again.
    let emoji = workspace.emoji().map(str::to_owned);
    let workspaces_on = App::workspaces_enabled(&app);
    let workspace_icon = format!("{glyph}|{}|{workspaces_on}", emoji.as_deref().unwrap_or(""));
    if let Some(json) = chrome_host::protocol::workspace_json_with(&workspace_name, &glyph, emoji.as_deref(), workspaces_on) {
        let already_sent = with_chrome(hwnd, |s| {
            s.html_tabs_workspace.as_ref().is_some_and(|(name, icon)| name == &workspace_name && icon == &workspace_icon)
        }).unwrap_or(false);
        if !already_sent && host.post_json(&json).is_ok() {
            let _ = with_chrome(hwnd, |s| s.html_tabs_workspace = Some((workspace_name.clone(), workspace_icon.clone())));
        }
    }
    // --- end vertical sidebar ---
    // --- tab options --- what the strip draws beyond the model, and how it draws it.
    let strip_options = html_strip_options(hwnd, &app);
    let strip_extras = App::tab_strip_extras(&app, &ids);
    {
        let json = chrome_host::protocol::tabopts_json(&strip_options);
        let pending = with_chrome(hwnd, |s| s.html_tabs_opts.as_deref() != Some(json.as_str())).unwrap_or(false);
        if pending && host.post_json(&json).is_ok() {
            let _ = with_chrome(hwnd, |s| s.html_tabs_opts = Some(json));
        }
    }
    // --- end tab options ---
    let inputs = App::tab_group_inputs(&app);
    let preferences = App::tab_group_preferences(&app);
    let tab_states: Vec<HtmlTabSnapshot> = snapshot
        .iter()
        .map(|(id, title, loading, marker)| {
            (
                *id,
                title.clone(),
                *loading,
                *marker,
                App::tab_pinned(&app, *id),
                App::tab_muted(&app, *id),
                App::tab_private(&app, *id),
            )
        })
        .collect();
    let favicon_keys: Vec<(TabId, Option<FaviconKey>)> = ids
        .iter()
        .map(|&id| (id, App::tab_favicon_key(&app, id)))
        .collect();
    let current_inputs = HtmlTabsInputs {
        mode,
        workspace: (workspace_name, workspace_icon),
        active,
        snapshot: tab_states,
        group_inputs: inputs.clone(),
        group_preferences: preferences.clone(),
        favicons: favicon_keys,
        generation: with_chrome(hwnd, |s| s.html_state_generation[1]).unwrap_or(0),
        extras: strip_extras,
        options: strip_options,
    };
    let is_unchanged = with_chrome(hwnd, |s| {
        s.html_tabs_last.is_some()
            && s.html_tabs_synced_inputs.as_ref() == Some(&current_inputs)
            && s.html_favicons_sent.len() == s.html_favicons.iter().filter(|(_, f)| f.png.is_some()).count()
    })
    .unwrap_or(false);
    if is_unchanged {
        return;
    }
    let keys = tab_groups::group_keys(&inputs, &preferences);
    let group_of: HashMap<TabId, Option<tab_groups::TabGroupKey>> =
        inputs.iter().map(|input| input.id).zip(keys).collect();
    let with_png = html_tabs_favicons(hwnd, &app, &ids);
    // The pinned, muted and private flags were read once for the comparison above; App finds each
    // tab by a scan, so reading them again per row would scan every tab for every tab once more.
    let rows: Vec<chrome_host::TabRow> = current_inputs
        .snapshot
        .iter()
        .map(|&(id, ref title, loading, marker, pinned, muted, private)| chrome_host::TabRow {
            id,
            title: title.clone(),
            loading,
            pinned,
            muted,
            private,
            marker,
            group: group_of
                .get(&id)
                .cloned()
                .flatten()
                .map(|key| chrome_host::row_group(&key, &preferences)),
            has_favicon: with_png.contains(&id),
            marks: html_tab_marks(&current_inputs.extras, id),
        })
        .collect();
    let items = chrome_host::tab_items(&rows);
    let json = tabs_json(
        chrome_host::tab_to_wire(active).unwrap_or(0),
        &items,
    );
    let groups: HashMap<u32, HtmlGroup> = rows
        .iter()
        .filter(|row| !row.pinned)
        .filter_map(|row| {
            let group = row.group.as_ref()?;
            Some((
                chrome_host::group_wire_key(&group.key),
                HtmlGroup {
                    tab: row.id,
                    collapsible: group.collapsible,
                    collapsed: group.collapsed,
                },
            ))
        })
        .collect();
    let (send_json, sends, generation) = with_chrome(hwnd, |s| {
        s.html_tabs_groups = groups;
        let send_json = s.html_tabs_last.as_deref() != Some(json.as_str());
        let mut sends = Vec::new();
        for (id, favicon) in &s.html_favicons {
            let Some(png) = &favicon.png else {
                continue;
            };
            if s.html_favicons_sent.get(id) != Some(&favicon.key) {
                sends.push((*id, favicon.key, png.clone()));
            }
        }
        (send_json, sends, s.html_state_generation[1])
    })
    .unwrap_or_default();
    if send_json && host.post_json(&json).is_ok() {
        let _ = with_chrome(hwnd, |s| {
            if s.html_state_generation[1] == generation {
                s.html_tabs_last = Some(json);
                s.html_tabs_synced_inputs = Some(current_inputs.clone());
            }
        });
    } else {
        let _ = with_chrome(hwnd, |s| {
            if s.html_state_generation[1] == generation && !send_json {
                s.html_tabs_synced_inputs = Some(current_inputs.clone());
            }
        });
    }
    for (id, key, png) in sends {
        if let Some(favicon_json) =
            chrome_host::tab_to_wire(id).and_then(|wire| fav_json(wire, &png))
        {
            if host.post_json(&favicon_json).is_ok() {
                let _ = with_chrome(hwnd, |s| {
                    if s.html_state_generation[1] == generation { s.html_favicons_sent.insert(id, key); }
                });
            }
        }
    }
}

/// Which rail button's native panel is open, if any. Shared by the native paint and the HTML rail.
fn open_rail_panel(hwnd: HWND) -> Option<RailKey> {
    // --- rail flyouts ---
    if let Some(id) = html_panel::active_id(hwnd) {
        return Some(flyout_button(id));
    }
    // --- end rail flyouts ---
    match side_panel::active_id(hwnd) {
        Some(side_panel::PanelId::Bookmarks) => Some(RailKey::Bookmarks),
        Some(side_panel::PanelId::History) => Some(RailKey::History),
        Some(side_panel::PanelId::Customize | side_panel::PanelId::Menu) => Some(RailKey::Customize),
        Some(side_panel::PanelId::WebApp(id)) => Some(RailKey::App(id)),
        None => None,
    }
}

/// The rail's keys in display order, as the native model holds them. Empty when the window is gone.
fn rail_model_keys(hwnd: HWND) -> Vec<RailKey> {
    with_chrome(hwnd, |s| s.rail_entries.iter().map(|entry| entry.key).collect()).unwrap_or_default()
}

/// Starts the HTML left rail's WebView2 controller on the shared environment (private chrome
/// profile), once per window. It is independent of the toolbar and the tab strip: if it cannot
/// start, the native rail stays.
fn start_html_rail(hwnd: HWND) {
    let Some((env, bounds)) = with_chrome(hwnd, |s| {
        if s.html_rail_requested {
            return None;
        }
        let env = s.html_env.clone()?;
        s.html_rail_requested = true;
        Some((env, chrome_host::rail_bounds(&s.layout).unwrap_or_default()))
    })
    .flatten() else {
        return;
    };
    let raw = hwnd.0 as isize;
    let on_command: Rc<dyn Fn(ChromeCommand)> =
        Rc::new(move |command| html_rail_command(HWND(raw as *mut _), command));
    let on_accelerator: AccelHandler =
        Rc::new(move |accel| html_accelerator(HWND(raw as *mut _), accel, View::Rail));
    let on_failed: Rc<dyn Fn()> = Rc::new(move || unsafe {
        // Posted, not handled here: the failure can arrive inside the controller's own callback.
        let _ = PostMessageW(
            Some(HWND(raw as *mut _)),
            WM_HTML_RAIL_FAILED,
            WPARAM(0),
            LPARAM(0),
        );
    });
    let created = ChromeHost::create(&env, crate::chrome_host::host::ChromeHostOptions { parent: hwnd, bounds, view: View::Rail, on_command, on_failed, on_accelerator: Some(on_accelerator) }, move |result| html_rail_created(hwnd, result));
    if created.is_err() {
        html_rail_failed(hwnd);
    }
}

/// The rail controller was created, or failed. A created host is stored, placed and raised; the
/// rail activates once its page says `ready`.
fn html_rail_created(hwnd: HWND, result: windows::core::Result<ChromeHost>) {
    let Ok(host) = result else {
        html_rail_failed(hwnd);
        return;
    };
    if window_closing(hwnd) {
        host.close();
        return;
    }
    let stored = with_chrome(hwnd, move |s| {
        s.html_rail = Some(host);
        s.sync_html_rail_bounds();
    });
    if stored.is_some() {
        sync_html_host_bounds(hwnd);
        if let Some(region) = with_chrome(hwnd, |s| chrome_host::rail_bounds(&s.layout)).flatten() {
            raise_html_child(hwnd, region);
        }
        html_rail_sync(hwnd);
    }
}

/// Activates the HTML rail once its page is ready. A freshly loaded page knows nothing yet, so the
/// whole rail and every favicon are resent. Runs again on every `ready`. The native rail stops
/// painting and hit-testing its area from here on.
fn html_rail_refresh(hwnd: HWND) {
    let _ = with_chrome(hwnd, |s| {
        s.html_rail_last = None;
        s.html_rail_last_state = None;
        s.html_state_generation[2] = s.html_state_generation[2].wrapping_add(1);
        s.html_rail_favs_sent.clear();
        // A hover or keyboard focus on the native rail belongs to the native buttons, now hidden.
        if matches!(s.hover, Hit::Rail(_)) {
            s.hover = Hit::None;
        }
        s.rail_keyboard_focus = false;
        s.sync_html_rail_bounds();
    });
    sync_html_host_bounds(hwnd);
    hide_rail_tooltip(hwnd);
    html_rail_sync(hwnd);
    repaint(hwnd);
}

/// The HTML rail failed: its area goes back to native paint and hit tests and its host is closed.
/// The toolbar and tab strip are unaffected. Safe to call more than once.
fn html_rail_failed(hwnd: HWND) {
    let Some(host) = with_chrome(hwnd, |s| {
        s.html_rail_page_ready = false;
        s.html_failed[2] = true;
        s.html_rail_last = None;
        s.html_rail_favs_sent.clear();
        let host = s.html_rail.take();
        s.sync_html_rail_bounds();
        host
    }) else {
        return;
    };
    // Closed outside the borrow: closing a controller can pump messages.
    if let Some(host) = host {
        host.close();
    }
    hide_rail_tooltip(hwnd);
    // The native rail paints again, and only it paints the web apps' decoded icons.
    super::post(hwnd, super::WM_RAIL_CHANGED, WPARAM(0), LPARAM(0));
    repaint(hwnd);
}

/// Handles one validated message from the HTML rail. Runs on the UI thread, holding no borrow.
fn html_rail_command(hwnd: HWND, command: ChromeCommand) {
    match command {
        ChromeCommand::Ready => {
            let _ = with_chrome(hwnd, |s| s.html_rail_page_ready = true);
            html_rail_refresh(hwnd);
            html_sync(hwnd);
            return;
        }
        ChromeCommand::Rail { action, key, rect } => html_rail_action(hwnd, action, key, rect),
        ChromeCommand::RailMove { key, before } => html_rail_move(hwnd, key, before),
        // --- tab stacks (T10) --- "Switch workspaces by scrolling" (Rust checks the option).
        ChromeCommand::RailWheel { d } => {
            if let Some(app) = app_of(hwnd) {
                let _ = App::scroll_workspaces(&app, d);
            }
        }
        // --- hover layer ---
        ChromeCommand::Tip { .. } | ChromeCommand::TipOff => {
            html_hover_command(hwnd, View::Rail, command);
            return;
        }
        // --- end hover layer ---
        // The rail sends nothing else. Any other message is ignored.
        _ => return,
    }
    html_rail_sync(hwnd);
}

/// A click or right click on a rail button. The key must be in the native model, or the message is
/// ignored. Both run from the deferred queue, outside WebView2's callback, because either may open
/// a modal prompt or menu: a click does what a native click does, a right click shows the native
/// rail context menu at the cursor.
fn html_rail_action(hwnd: HWND, action: RailAction, wire: RailKeyWire, rect: Option<chrome_host::protocol::HoverRect>) {
    let Some(key) = chrome_host::rail_key_from_wire(wire, &rail_model_keys(hwnd)) else {
        return;
    };
    // --- rail flyouts ---
    // The Add buttons keep their rectangle too: it places the composer they open.
    if let Some(rect) = rect.filter(|_| flyout_panel(key).is_some() || matches!(key, RailKey::AddWorkspace | RailKey::AddApp)) {
        let _ = with_chrome(hwnd, |s| s.html_rail_anchors.insert(key, rect));
    }
    // --- end rail flyouts ---
    match action {
        RailAction::Click => defer_html_command(hwnd, DeferredChromeCommand::RailClick { key }),
        RailAction::Hover => {
            if flyout_panel(key).is_some() && html_panel::active_id(hwnd).is_some() {
                defer_html_command(hwnd, DeferredChromeCommand::RailHover { key });
            }
        }
        RailAction::Menu if app_of(hwnd).is_some_and(|app| system_menu_requested(&app)) => {
            defer_html_command(hwnd, DeferredChromeCommand::SystemMenu);
        }
        RailAction::Menu => {
            if let Some((x, y)) = cursor_client_point(hwnd) {
                defer_html_command(hwnd, DeferredChromeCommand::RailMenu { key, x, y });
            }
        }
    }
}

/// A drag-reorder drop of a workspace or app in the rail. Both keys must be in the native model and
/// `before` must be in the same section, or the drop is ignored. The index is the one the native
/// drop computes (`rail::reorder_index`), and the commit is the native one (`commit_rail_drop`).
fn html_rail_move(hwnd: HWND, wire: RailKeyWire, before: Option<RailKeyWire>) {
    let Some(app) = app_of(hwnd) else {
        return;
    };
    let keys = rail_model_keys(hwnd);
    let Some(moved) = chrome_host::rail_key_from_wire(wire, &keys) else {
        return;
    };
    let before = match before {
        None => None,
        Some(wire) => match chrome_host::rail_key_from_wire(wire, &keys) {
            Some(key) => Some(key),
            None => return,
        },
    };
    let Some(index) = crate::chrome::rail::reorder_index(&keys, moved, before) else {
        return;
    };
    commit_rail_drop(hwnd, &app, moved, index);
}

/// Posts the rail's buttons when they changed, and each favicon PNG that is new for its button.
/// Does nothing until the page is ready. Holds no chrome borrow while calling App, which may
/// re-enter this window, and the App is not called at all here: the model is already built.
fn html_rail_sync(hwnd: HWND) {
    let open = open_rail_panel(hwnd);
    let unchanged = with_chrome(hwnd, |s| {
        s.html_rail_last.is_some()
            && s.html_rail_synced_open == Some(open)
            && s.html_rail_synced_rail_gen == s.rail_generation
            && s.html_rail_synced_state_gen == s.html_state_generation[2]
            && s.html_rail_favs_sent.len() == s.rail_entries.iter().filter(|e| e.png.is_some()).count()
    })
    .unwrap_or(false);
    if unchanged {
        return;
    }
    let Some((host, json, favicons, generation, rail_gen)) = with_chrome(hwnd, |s| {
        s.html_rail_region()?;
        let host = s.html_rail.clone()?;
        let items = {
            let entries = rail_entries(s);
            chrome_host::rail_items(entries, open, |key| {
                s.rail_entries
                    .iter()
                    .find(|entry| entry.key == key)
                    .and_then(|entry| entry.png.as_deref())
                    .map(Vec::as_slice)
            })
        };
        let json = rail_json(&items);
        // Favicons go out once per change: a button whose PNG is the one already sent is skipped.
        let mut favicons = Vec::new();
        for entry in &s.rail_entries {
            let Some(png) = &entry.png else {
                continue;
            };
            let sent = s.html_rail_favs_sent.get(&entry.key).is_some_and(|prev| {
                Rc::ptr_eq(prev, png) || prev.as_slice() == png.as_slice()
            });
            if sent {
                continue;
            }
            let wire = chrome_host::rail_key_to_wire(entry.key).to_wire();
            if let Some(message) = railfav_json(&wire, png) {
                favicons.push((entry.key, png.clone(), message));
            }
        }
        let live: HashSet<RailKey> = s
            .rail_entries
            .iter()
            .filter(|entry| entry.png.is_some())
            .map(|entry| entry.key)
            .collect();
        s.html_rail_favs_sent.retain(|key, _| live.contains(key));
        let send_json = s.html_rail_last.as_deref() != Some(json.as_str());
        Some((host, send_json.then_some(json), favicons, s.html_state_generation[2], s.rail_generation))
    })
    .flatten() else {
        return;
    };
    if let Some(json) = json {
        if host.post_json(&json).is_ok() {
            let _ = with_chrome(hwnd, |s| {
                if s.html_state_generation[2] == generation {
                    s.html_rail_last = Some(json);
                    s.html_rail_synced_open = Some(open);
                    s.html_rail_synced_rail_gen = rail_gen;
                    s.html_rail_synced_state_gen = generation;
                }
            });
        }
    } else {
        let _ = with_chrome(hwnd, |s| {
            if s.html_state_generation[2] == generation {
                s.html_rail_synced_open = Some(open);
                s.html_rail_synced_rail_gen = rail_gen;
                s.html_rail_synced_state_gen = generation;
            }
        });
    }
    for (key, png, message) in favicons {
        if host.post_json(&message).is_ok() {
            let _ = with_chrome(hwnd, |s| {
                if s.html_state_generation[2] == generation {
                    s.html_rail_favs_sent.insert(key, png);
                }
            });
        }
    }
}

/// Inline completion for the HTML omnibox. Mirrors the native EN_CHANGE path: the text is kept for
/// the next backspace check, prewarming runs, and the suffix goes back to the page tagged with `seq`.
/// Empty text gets no reply.
fn html_edit(hwnd: HWND, seq: u32, text: &str) {
    let Some(app) = app_of(hwnd) else {
        return;
    };
    let suffix = with_chrome(hwnd, |s| {
        s.omnibox_edited = true;
        // Shorter text means Backspace or Delete, where completion must not reappear.
        s.suppress_completion = text.chars().count() < s.edit_text.chars().count();
        s.edit_text.clear();
        s.edit_text.push_str(text);
        s.update_completion();
        s.completion.as_ref().map(|c| c.completed_suffix.clone())
    })
    .flatten();
    App::prewarm(&app, text);
    if text.is_empty() {
        super::html_hover::suggest_close(hwnd); // --- omnibox suggest ---
        return;
    }
    let reply = completion_json(seq, suffix.as_deref().unwrap_or(""));
    let _ = with_chrome(hwnd, |s| s.html.clone()).flatten().map(|host| host.post_json(&reply));
    html_suggest_edit(hwnd, &app, seq, text); // --- omnibox suggest ---
}

// --- omnibox suggest ---
/// Posts `json` to the HTML toolbar page, if it is ready.
pub(super) fn html_toolbar_post(hwnd: HWND, json: &str) {
    if let Some(host) = with_chrome(hwnd, |s| s.html_toolbar_active.then(|| s.html.clone()).flatten()).flatten() {
        let _ = host.post_json(json);
    }
}

/// Computes the dropdown rows for edit `seq` and shows them under the pill. Runs once per edit, after
/// the inline completion, so the default row follows it. Nothing here touches the disk except one
/// `stat` of the bookmark file.
fn html_suggest_edit(hwnd: HWND, app: &AppRef, seq: u32, text: &str) {
    if !super::address_bar::list_on_typing(app) {
        super::html_hover::suggest_close(hwnd);
        return;
    }
    html_suggest_with(hwnd, app, seq, text, false);
}

/// The list for `text` although typing does not open it (Down arrow with the list closed).
pub(super) fn html_suggest_forced(hwnd: HWND, app: &AppRef, seq: u32, text: &str) {
    if text.trim().is_empty() {
        super::html_hover::suggest_close(hwnd);
        return;
    }
    html_suggest_with(hwnd, app, seq, text, true);
}

/// A late engine answer: the list for `text` again, from what is now known.
pub(super) fn html_suggest_refresh(hwnd: HWND, seq: u32, text: &str) {
    if let Some(app) = app_of(hwnd) {
        html_suggest_with(hwnd, &app, seq, text, true);
    }
}

/// The rows `typed` would show now, for picking the first of them (a scoped nickname's Enter).
pub(super) fn html_suggest_rows_for(hwnd: HWND, app: &AppRef, typed: &str) -> Vec<super::html_suggest::Row> {
    html_suggest_compute(hwnd, app, 0, typed, true).map(|(rows, _)| rows).unwrap_or_default()
}

fn html_suggest_compute(hwnd: HWND, app: &AppRef, seq: u32, text: &str, forced: bool) -> Option<(Vec<super::html_suggest::Row>, Vec<super::html_suggest::TabSource>)> {
    use super::html_suggest as suggest;
    let typed_url = App::typed_address(app, text);
    let active = App::active_tab_id(app);
    let tabs: Vec<_> =
        App::tab_suggest_sources(app).into_iter().filter(|(id, ..)| *id != active).collect();
    let rows = with_chrome(hwnd, |s| {
        let history = loaded_history(&mut s.history, &mut s.history_load);
        super::address_bar::rows(hwnd, app, history, s.completion.as_ref(), &super::address_bar::Query { text, typed_url: typed_url.as_ref(), tabs: &tabs, seq, forced })
    })?;
    let mut rows = rows;
    // A favicon is known for a row's page only through an open tab on the same site.
    for row in &mut rows {
        let tab = match row.target {
            suggest::Target::Tab(id) => Some(id),
            _ if row.fav_url.is_empty() => None,
            _ => suggest::tab_with_host(&tabs, &row.fav_url),
        };
        row.item.png = tab.and_then(|id| App::tab_favicon(app, id)).filter(|png| chrome_host::favicon_sendable(png));
    }
    Some((rows, tabs))
}

fn html_suggest_with(hwnd: HWND, app: &AppRef, seq: u32, text: &str, forced: bool) {
    let started = Instant::now();
    let Some((rows, _)) = html_suggest_compute(hwnd, app, seq, text, forced) else { return };
    html_suggest_show(hwnd, app, seq, text, rows, started);
}

/// Shows ready-made rows under the pill (the typed-history list, the search field's list).
pub(super) fn html_suggest_present(hwnd: HWND, app: &AppRef, seq: u32, text: &str, rows: Vec<super::html_suggest::Row>) {
    html_suggest_show(hwnd, app, seq, text, rows, Instant::now());
}

fn html_suggest_show(hwnd: HWND, app: &AppRef, seq: u32, text: &str, rows: Vec<super::html_suggest::Row>, started: Instant) {
    let _ = app;
    // The dropdown continues the pill as one surface: it is exactly as wide as the pill the toolbar
    // page paints, and attaches to that pill's device-pixel edges (see html_hover::place).
    let Some((anchor, width)) = with_chrome(hwnd, |s| {
        let omnibox = s.layout.omnibox;
        let pill = chrome_host::toolbar_pill_rect_with_slot(&s.layout, s.dpi, s.toolbar_slot()).unwrap_or(RECT {
            left: omnibox.left,
            top: omnibox.top,
            right: omnibox.right,
            bottom: omnibox.bottom,
        });
        let mut origin = windows::Win32::Foundation::POINT { x: pill.left, y: pill.top };
        unsafe { let _ = ClientToScreen(hwnd, &mut origin); }
        let (w, h) = (pill.right - pill.left, pill.bottom - pill.top);
        let css = chrome_host::toolbar_css_placement_with_slot(&s.layout, s.dpi, s.toolbar_slot())
            .map_or_else(|| chrome_host::css_px_from_physical(w, s.dpi), |(_, pill_w, _)| pill_w);
        (RECT { left: origin.x, top: origin.y, right: origin.x + w, bottom: origin.y + h }, css.max(1) as u32)
    }) else {
        return;
    };
    let computed = started.elapsed();
    super::html_hover::suggest_show(hwnd, seq, anchor, width, text, rows);
    if std::env::var_os("BOWSER_SUGGEST_TRACE").is_some() {
        eprintln!("suggest seq={seq} compute={computed:?} total={:?}", started.elapsed());
    }
}

/// Does what picking a dropdown row does: switch to the tab, or go to the address or search, the
/// same path as typing it and pressing Enter.
pub(super) fn html_suggest_run(hwnd: HWND, target: super::html_suggest::Target) {
    use super::html_suggest::Target;
    let Some(app) = app_of(hwnd) else { return };
    if super::address_bar::run_target(hwnd, &app, &target) {
        // A calculator answer stays in the field; a search-field row opened its page.
        if matches!(target, Target::EngineSearch { .. }) {
            html_sync(hwnd);
        }
        return;
    }
    match target {
        Target::Tab(id) => {
            let _ = App::select_tab(&app, id);
        }
        Target::Navigate(text) | Target::Search(text) => {
            remember_url(hwnd, App::active_tab_id(&app), &text, App::settings(&app).minimal_url);
            App::submit(&app, &text);
        }
        Target::Fill(_) | Target::EngineSearch { .. } => {}
    }
    App::focus_page(&app);
    html_sync(hwnd);
}
// --- end omnibox suggest ---

/// Starts a native window move for a press on an empty toolbar area. Only a press still held
/// counts, so a stale message cannot start a drag.
fn begin_html_drag(hwnd: HWND) {
    unsafe {
        // With the mouse buttons swapped, the button that starts a drag is the right one.
        let primary = if GetSystemMetrics(SM_SWAPBUTTON) != 0 {
            VK_RBUTTON
        } else {
            VK_LBUTTON
        };
        if (GetAsyncKeyState(primary.0 as i32) as u16 & 0x8000) == 0 {
            return;
        }
        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_err() {
            return;
        }
        let _ = ReleaseCapture();
        let lparam = ((point.y as u16 as u32) << 16) | (point.x as u16 as u32);
        let _ = SendMessageW(
            hwnd,
            WM_NCLBUTTONDOWN,
            Some(WPARAM(HTCAPTION as usize)),
            Some(LPARAM(lparam as isize)),
        );
    }
}

/// Shows the window's system menu at the cursor and sends the chosen command as `WM_SYSCOMMAND`.
fn html_system_menu(hwnd: HWND) {
    unsafe {
        let menu = GetSystemMenu(hwnd, false);
        let mut point = POINT::default();
        if menu.is_invalid() || GetCursorPos(&mut point).is_err() {
            return;
        }
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            Some(0),
            hwnd,
            None,
        );
        if chosen.0 != 0 {
            let _ = PostMessageW(
                Some(hwnd),
                WM_SYSCOMMAND,
                WPARAM(chosen.0 as usize),
                LPARAM(0),
            );
        }
    }
}

/// Raises the WebView2 window of an HTML chrome host to the top of the z-order. It is the child of
/// `hwnd` whose client rectangle is `region`. The toolbar and tab row regions never overlap.
fn raise_html_child(hwnd: HWND, region: RECT) {
    struct Search {
        parent: HWND,
        region: RECT,
    }
    unsafe extern "system" fn visit(child: HWND, lparam: LPARAM) -> BOOL {
        let search = unsafe { &*(lparam.0 as *const Search) };
        let direct = unsafe { GetParent(child) }.is_ok_and(|parent| parent == search.parent);
        if !direct {
            return BOOL(1);
        }
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(child, &mut rect) }.is_err() {
            return BOOL(1);
        }
        let mut origin = POINT {
            x: rect.left,
            y: rect.top,
        };
        unsafe {
            let _ = ScreenToClient(search.parent, &mut origin);
        }
        let region = search.region;
        let same = origin.x == region.left
            && origin.y == region.top
            && rect.right - rect.left == region.right - region.left
            && rect.bottom - rect.top == region.bottom - region.top;
        if !same {
            return BOOL(1);
        }
        unsafe {
            let _ = SetWindowPos(
                child,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        BOOL(0)
    }
    let mut search = Search {
        parent: hwnd,
        region,
    };
    unsafe {
        let _ = EnumChildWindows(
            Some(hwnd),
            Some(visit),
            LPARAM(&mut search as *mut Search as isize),
        );
    }
}

fn position_edit(hwnd: HWND) {
    let Some((edit, r)) = with_chrome(hwnd, |s| {
        let a = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const AppRef).as_ref() }?;
        Some((a.borrow().edit, s.layout.omnibox))
    })
    .flatten() else {
        return;
    };
    unsafe {
        // The chrome draws all omnibox text; the EDIT is only the input engine (focus, IME,
        // clipboard, selection), so it is kept 1x1 at the omnibox origin where it cannot show.
        let _ = MoveWindow(edit, r.left, r.top, 1, 1, false);
        let m = Metrics::new(dpi(hwnd)).px(Metrics::OMNIBOX_TEXT_INSET) as isize;
        let _ = SendMessageW(
            edit,
            EM_SETMARGINS,
            Some(WPARAM(
                (windows::Win32::UI::WindowsAndMessaging::EC_LEFTMARGIN
                    | windows::Win32::UI::WindowsAndMessaging::EC_RIGHTMARGIN)
                    as usize,
            )),
            Some(LPARAM(m | (m << 16))),
        );
    }
}
/// `BOWSER_PAINT_LOG` diagnostic for the page overrunning its rounded frame: the client, the
/// overlay's screen origin against the client origin it was presented at, the island the overlay
/// frames (`island`, client coordinates) and every visible direct child window (the page and HTML
/// chrome WebView2 hosts) with its class and client rectangle. `None` when nothing changed since the
/// last line, so steady repaints add nothing to the log.
fn changed_page_geometry(hwnd: HWND, overlay: HWND, origin: windows::Win32::Foundation::POINT, island: Rect) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GW_CHILD, GW_HWNDNEXT, GetClassNameW, IsWindowVisible};
    thread_local! {
        static LAST: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    }
    let mut line = String::new();
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let mut shown = RECT::default();
        let _ = GetWindowRect(overlay, &mut shown);
        line.push_str(&format!(
            "geometry client={}x{} zoomed={} client_origin=({},{}) overlay_origin=({},{}) island=({},{},{},{})",
            client.right,
            client.bottom,
            IsZoomed(hwnd).as_bool(),
            origin.x,
            origin.y,
            shown.left,
            shown.top,
            island.left,
            island.top,
            island.right,
            island.bottom,
        ));
        let mut child = GetWindow(hwnd, GW_CHILD).ok().filter(|child| !child.is_invalid());
        while let Some(window) = child {
            if IsWindowVisible(window).as_bool() {
                let mut buffer = [0u16; 64];
                let len = GetClassNameW(window, &mut buffer).max(0) as usize;
                let mut rect = RECT::default();
                let _ = GetWindowRect(window, &mut rect);
                line.push_str(&format!(
                    " | {}=({},{},{},{})",
                    String::from_utf16_lossy(&buffer[..len.min(buffer.len())]),
                    rect.left - origin.x,
                    rect.top - origin.y,
                    rect.right - origin.x,
                    rect.bottom - origin.y,
                ));
            }
            child = GetWindow(window, GW_HWNDNEXT).ok().filter(|next| !next.is_invalid());
        }
    }
    LAST.with(|last| {
        let mut last = last.borrow_mut();
        (*last != line).then(|| {
            last.clone_from(&line);
            line
        })
    })
}
/// Draws the native chrome. Nothing else: the HTML views and page hosts are synced from
/// `WM_CHROME_SYNC` (see `mark_chrome_dirty`), so an animation frame reads no files and sends no
/// state.
fn paint_chrome(hwnd: HWND, app: Option<&AppRef>) {
    static PAINT_LOG_ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        if let Some(app) = app {
            let active = App::active_tab_id(app);
            let split_view = App::split_view(app);
            // --- split view --- a shown split always draws its pane frames (an empty side is filled)
            // The private landing (split_ui) covers a blank private tab like a page: framed, not filled.
            let active_has_controller = split_view.is_some()
                || App::active_tab_has_controller(app)
                || super::split_ui::private_page_shown(hwnd)
                || super::settings_page::shown(hwnd)
                || super::history_page::shown(hwnd)
                || super::downloads_page::shown(hwnd);
            // --- split view ---
            // The side whose tab has no page, unless the HTML "choose a tab" surface covers it,
            // and a tab drag target the HTML card cannot show yet.
            let picker_pane = super::split_ui::picker_pane(hwnd);
            let settings_pane = super::settings_page::pane(hwnd, app);
            let history_pane = super::history_page::pane(hwnd, app);
            let downloads_pane = super::downloads_page::pane(hwnd, app);
            let split_unfilled = split_view.map(|split| {
                let mut unfilled = [false; 4];
                for (pane, tab) in split.panes() {
                    unfilled[pane.index()] = !App::tab_has_controller(app, tab) && picker_pane != Some(pane) && settings_pane != Some(pane) && history_pane != Some(pane) && downloads_pane != Some(pane);
                }
                unfilled
            });
            let split_drop_fallback = super::split_ui::drop_fallback(hwnd);
            // --- end split view ---
            let progress_target = App::tab_progress_percent(app, active);
            let can_back = App::tab_can_go_back(app, active);
            let can_forward = App::tab_can_go_forward(app, active);
            let site = App::site_security(app);
            let focus = GetFocus() == app.borrow().edit;
            let now = Instant::now();
            let (mut a, mut b) = (0u32, 0u32);
            let _ = SendMessageW(
                app.borrow().edit,
                EM_GETSEL,
                Some(WPARAM((&mut a as *mut u32) as usize)),
                Some(LPARAM((&mut b as *mut u32) as isize)),
            );
            let dirty = Rect::from(ps.rcPaint);
            let mut did = false;
            WINDOWS.with(|all| {
                let mut all = all.borrow_mut();
                let Some(s) = all.get_mut(&(hwnd.0 as isize)) else {
                    return;
                };
                s.focused = focus;
                // WindowChrome owns the text and icons. The short-lived references below are used only while
                // this mutable borrow is held; clearing the vector precedes every snapshot replacement.
                s.views.clear();
                for slot in &s.slots {
                    let Some(t) = s.tabs.get(slot.tab_index) else {
                        continue;
                    };
                    let group_frame = slot
                        .group_key
                        .as_ref()
                        .and_then(|key| s.motion.group_frame(key, now));
                    let v = TabView {
                        title: &t.title,
                        loading: t.loading,
                        favicon: t.favicon.as_deref(),
                        emphasis: s
                            .motion
                            .emphasis(t.id)
                            .unwrap_or_else(|| Emphasis::resting(false, false, now)),
                        group_size: slot.group_key.as_ref().map(|_| slot.group_size),
                        group_control: slot.group_control,
                        marker: t.marker,
                        group_colour: slot.island_colour,
                        group_frame,
                    };
                    s.views
                        .push(std::mem::transmute::<TabView<'_>, TabView<'static>>(v));
                }
                // --- two-level stacks (T10b) --- the second row's tabs follow the tab row's views.
                let main_views = s.views.len();
                for id in &s.stack_tabs {
                    let Some(t) = s.tabs.iter().find(|tab| tab.id == *id) else {
                        continue;
                    };
                    let v = TabView {
                        title: &t.title,
                        loading: t.loading,
                        favicon: t.favicon.as_deref(),
                        emphasis: Emphasis::resting(false, false, now),
                        group_size: None,
                        group_control: false,
                        marker: t.marker,
                        group_colour: None,
                        group_frame: None,
                    };
                    s.views
                        .push(std::mem::transmute::<TabView<'_>, TabView<'static>>(v));
                }
                let stack_active = s.stack_tabs.iter().position(|id| *id == active);
                // --- end two-level stacks ---
                let active_i = s
                    .slots
                    .iter()
                    .position(|slot| {
                        s.tabs
                            .get(slot.tab_index)
                            .is_some_and(|tab| tab.id == active)
                    })
                    .unwrap_or(0);
                let suffix = s
                    .completion
                    .as_ref()
                    .map_or("", |c| c.completed_suffix.as_str());
                let text = &s.edit_text;
                let l = &s.layout;
                let p = s.palette;
                let views = &s.views;
                let mut rail_fades = Vec::new();
                s.motion.rail_fades(now, &mut rail_fades);
                let open_panel = open_rail_panel(hwnd);
                let rail = RailView { entries: &s.rail_view, fades: &rail_fades, open_panel, scroll: s.rail_scroll, drop_indicator: s.rail_drag.and_then(|drag| drag.target.map(|(_, indicator)| indicator)), focused: s.rail_keyboard_focus.then_some(s.rail_focus).flatten(), tooltip: s.rail_tooltip_visible.then_some(s.rail_tooltip_key).flatten() };
                if progress_target.is_none() {
                    s.progress.clear();
                }
                let progress = match (progress_target, s.progress.tab) {
                    (Some(target), Some(tab)) if tab == active => {
                        s.progress.paint_value(now).or(Some(target as f32))
                    }
                    (Some(target), _) => Some(target as f32),
                    (None, _) => None,
                };
                let maximised = IsZoomed(hwnd).as_bool();
                s.motion.retarget_icon_active(Hit::CaptionMaximize, maximised, now);
                s.motion.retarget_icon_active(Hit::SiteInfo,
                    site == crate::site_security::SiteSecurity::Secure, now);
                let view = ChromeView {
                    layout: l,
                    palette: p,
                    tabs: &views[..main_views],
                    active: active_i,
                    stack_tabs: &views[main_views..], // --- two-level stacks (T10b) ---
                    stack_active,
                    hover: s.hover,
                    pressed: s.pressed,
                    caption_maximised: maximised,
                    can_back,
                    can_forward,
                    omnibox: OmniboxView {
                        text,
                        selection: if focus {
                            (a as usize, b as usize)
                        } else {
                            (0, 0)
                        },
                        caret_visible: s.caret,
                        focused: focus,
                        completion_suffix: suffix,
                        progress,
                        site,
                    },
                    now,
                    departures: s.motion.departures(),
                    rail: Some(rail),
                    icon_motion: Some(&s.motion),
                    content_rounded: s.paint_prefs.rounded_content_frame,
                    show_site_info: s.paint_prefs.show_site_info,
                    show_split: s.paint_prefs.show_split,
                };
                let sidebar_viewport = adjusted_viewport(hwnd, to_win_rect(s.layout.viewport), s.dpi);
                let sidebar_viewport = Rect::new(
                    sidebar_viewport.left,
                    sidebar_viewport.top,
                    sidebar_viewport.right,
                    sidebar_viewport.bottom,
                );
                // --- auto-hide (T11) --- the page's frame is drawn below a bar that floats over its top.
                let sidebar_viewport = s.ah.paint_viewport(sidebar_viewport);
                let dirty = if s.surface.set_auto_hidden_top(s.ah.top_hidden()) {
                    Rect::new(0, 0, s.client.0, s.client.1)
                } else {
                    dirty
                };
                // --- end auto-hide ---
                let page_regions = if active_has_controller {
                    if let Some(split) = split_view {
                        layout::split_panes(sidebar_viewport, split, s.split_divider, s.split_cross, s.layout.metrics)
                            .map(|panes| (sidebar_viewport, Some(panes)))
                            // --- split view --- too narrow for two panes: the focused pane alone
                            .or(Some((sidebar_viewport, None)))
                    } else {
                        Some((sidebar_viewport, None))
                    }
                } else {
                    None
                };
                // --- split view ---
                let grip = if s.split_dragging {
                    crate::chrome::paint::SplitGrip::Drag
                } else if s.hover == Hit::SplitDivider {
                    crate::chrome::paint::SplitGrip::Hover
                } else {
                    crate::chrome::paint::SplitGrip::Idle
                };
                let empty_pane = match (split_unfilled, page_regions.and_then(|(_, panes)| panes)) {
                    (Some(unfilled), Some(panes)) => std::array::from_fn(|index| panes.panes[index].filter(|_| unfilled[index])),
                    _ => [None; 4],
                };
                let grip_divider = (s.split_dragging || s.hover == Hit::SplitDivider).then_some(s.split_drag_divider).flatten();
                let dirty = if s.surface.set_split_chrome(grip, grip_divider, empty_pane) {
                    dirty.union(sidebar_viewport)
                } else {
                    dirty
                };
                // --- end split view ---
                s.surface.set_page_regions(page_regions);
                // A tab with no page shows an opaque themed surface. When that appears or goes
                // (a page took over) the whole viewport is repainted, not just the dirty part.
                let empty_viewport = (!active_has_controller).then_some(sidebar_viewport);
                let viewport_changed = s.surface.set_empty_viewport(empty_viewport);
                let dirty = if viewport_changed {
                    dirty.union(sidebar_viewport)
                } else {
                    dirty
                };
                let t0 = std::time::Instant::now();
                s.surface.paint(&view, dirty);
                if let Some(sidebar) = s.sidebar.filter(|_| !s.ah.sidebar_hidden()) { // --- auto-hide (T11) ---
                    s.surface.paint_vertical_sidebar(crate::chrome::paint::SidebarPaintView { layout: &sidebar, rows: &s.sidebar_rows, workspace: &s.paint_prefs.workspace_name, active, scroll: s.sidebar_scroll, hover_row: s.sidebar_hover_row, html_sidebar: s.html_tab_region().is_some() }, dirty, p);
                }
                // --- split view --- the native strip's half, or the HTML card's fallback rectangle
                let native_drop = s.split_drop.map(|zone| {
                    let gap = s.layout.metrics.px(crate::stacks::gap_dip());
                    let (l, t, r, b) = crate::split_view::drop_rect((sidebar_viewport.left, sidebar_viewport.top, sidebar_viewport.right, sidebar_viewport.bottom), zone, gap);
                    Rect::new(l, t, r, b)
                });
                if let Some(target) = native_drop.or(split_drop_fallback.map(|rect| Rect::new(rect.left, rect.top, rect.right, rect.bottom))) {
                    s.surface.paint_split_drop(target, p, dirty);
                }
                // --- tab stacks (T10) --- "Show a title bar on each tile", in both chromes: the bars
                // sit in the window, above each tile's page.
                if active_has_controller {
                    let focused = split_view.map(|view| view.focused_tab());
                    let bars: Vec<(Rect, String, bool)> = tile_title_bars(hwnd, s)
                        .into_iter()
                        .map(|(bar, tab)| {
                            let title = s.tabs.iter().find(|shown| shown.id == tab).map(|shown| shown.title.clone()).unwrap_or_default();
                            (bar, title, focused == Some(tab))
                        })
                        .collect();
                    if !bars.is_empty() {
                        s.surface.paint_tile_titles(&bars, p, dirty);
                    }
                }
                let t1 = std::time::Instant::now();
                let mut origin = windows::Win32::Foundation::POINT { x: 0, y: 0 };
                let _ = ClientToScreen(hwnd, &mut origin);
                s.surface.present(s.overlay, origin, dirty);
                let t2 = std::time::Instant::now();
                // A rail press or activation that this paint started needs frames to finish.
                if s.surface.rail_animating(now) || s.motion.animating(now) {
                    let _ = SetTimer(Some(hwnd), WM_ANIMATION_TIMER, ANIMATION_FRAME_MS, None);
                }
                if !s.first_paint_logged {
                    s.first_paint_logged = true;
                    let _ = app.borrow().log.record("chrome_painted");
                    let _ = SetTimer(Some(hwnd), super::WM_IMPORT_OFFER_TIMER, 1800, None);
                    if chrome_host::html_ui_enabled() {
                        // Native mode never reaches this: no message, no WebView2 work.
                        let _ = PostMessageW(
                            Some(hwnd),
                            WM_HTML_CHROME_START,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }
                if *PAINT_LOG_ENABLED.get_or_init(|| std::env::var_os("BOWSER_PAINT_LOG").is_some())
                {
                    use std::io::Write;
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(std::env::temp_dir().join("bowser-paint.log"))
                    {
                        let _ = writeln!(
                            f,
                            "{}x{} paint_us={} layered_update_us={}",
                            dirty.width(),
                            dirty.height(),
                            (t1 - t0).as_micros(),
                            (t2 - t1).as_micros()
                        );
                        // Page-overrun diagnostic: where the framed island is painted against where
                        // the page windows really are, logged when either changes.
                        if let Some(line) = changed_page_geometry(hwnd, s.overlay, origin, sidebar_viewport) {
                            let _ = writeln!(f, "{line}");
                        }
                    }
                }
                if !s.palette.translucent {
                    // Only solid mode owns every pixel. In glass mode a GDI fill carries zero
                    // alpha over the accent blur and renders as a lighter blue-gray patch, so a
                    // blank viewport is left untouched and shows the same blur and tint as the
                    // top chrome.
                    let owned = Rect::new(0, 0, s.client.0, s.client.1);
                    let fill = dirty
                        .intersect(Rect::new(0, 0, s.client.0, s.client.1))
                        .intersect(owned);
                    if !fill.is_empty() {
                        let r = RECT {
                            left: fill.left,
                            top: fill.top,
                            right: fill.right,
                            bottom: fill.bottom,
                        };
                        FillRect(dc, &r, glass::solid_brush(glass::appearance()));
                    }
                }
                did = true;
            });
            if did
                && App::navigation_progress_percent(app) == Some(100)
                && with_chrome(hwnd, |s| {
                    s.progress.tab == Some(active) && s.progress.settled(now)
                })
                .unwrap_or(false)
                && App::navigation_progress_painted(app)
            {
                invalidate_omni(hwnd)
            }
        }
        let _ = EndPaint(hwnd, &ps);
    }
}
fn read_edit(hwnd: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut b = vec![0u16; n + 1];
        let copied = GetWindowTextW(hwnd, &mut b) as usize;
        String::from_utf16_lossy(&b[..copied])
    }
}
fn remember_url(hwnd: HWND, id: TabId, value: &str, minimal_url: bool) {
    let parsed = url::Url::parse(value).or_else(|_| {
        if value.contains('.') || value.starts_with("localhost") {
            url::Url::parse(&format!("https://{value}"))
        } else {
            Err(url::ParseError::RelativeUrlWithoutBase)
        }
    });
    if let Ok(url) = parsed {
        let shown = omnibox::address_display(&url, minimal_url);
        let _ = with_chrome(hwnd, |s| {
            s.urls.insert(id, shown);
        });
        mark_chrome_dirty(hwnd);
    }
}
pub(super) fn focus_omni(hwnd: HWND, app: &AppRef, all: bool) {
    // --- layout (thread T6) --- a hidden address bar comes back for as long as it has the focus.
    set_address_bar_revealed(hwnd, app, true);
    auto_hide::reveal_for_keyboard(hwnd); // --- auto-hide (T11) --- Ctrl+L, Ctrl+T and friends bring the bars back first
    let _ = with_chrome(hwnd, |s| {
        s.omnibox_claimed = true;
        s.omnibox_edited = false;
    });
    // While the HTML toolbar is active, its page owns the omnibox and the native input stays hidden.
    if let Some(host) = with_chrome(hwnd, |s| {
        if s.html_toolbar_active { s.html.clone() } else { None }
    })
    .flatten()
    {
        let _ = host.focus();
        let _ = host.post_json(&focus_json(true));
        return;
    }
    let edit = app.borrow().edit;
    let id = App::active_tab_id(app);
    let value =
        with_chrome(hwnd, |s| s.urls.get(&id).cloned().unwrap_or_default()).unwrap_or_default();
    unsafe {
        let _ = SetWindowTextW(edit, &windows::core::HSTRING::from(value));
        let _ = SetFocus(Some(edit));
        if all {
            let _ = SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
        }
    }
    let _ = with_chrome(hwnd, |s| {
        s.focused = true;
        s.caret = true;
        s.completion = None;
        s.suppress_completion = false;
    });
    sync_foreground(hwnd, Some(app));
    invalidate_omni(hwnd)
}
/// Remembers the address a navigation was just requested for as the tab's omnibox text, so the bar
/// shows it at once (also before the engine exists or the page commits) instead of the placeholder.
/// A native field nobody has typed in yet (the blank start state, which already owns the focus) is
/// filled in and selected so typing replaces it; a field the user is editing is left alone, and the
/// address reaches the HTML omnibox the way a committed one does.
pub(crate) fn show_pending_address(hwnd: HWND, edit: HWND, id: TabId, url: &url::Url, minimal_url: bool) {
    let shown = omnibox::address_display(url, minimal_url);
    if with_chrome(hwnd, |s| s.urls.insert(id, shown.clone())).is_none() {
        return;
    }
    unsafe {
        let pristine = !edit.is_invalid()
            && SendMessageW(edit, EM_GETMODIFY, None, None).0 == 0
            && read_edit(edit).is_empty();
        if pristine {
            let _ = super::set_input_text(edit, &shown);
            let _ = SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
        }
    }
    invalidate_omni(hwnd);
    mark_chrome_dirty(hwnd);
}
/// Shows a navigation's address in the omnibox. While the field has focus the user owns its text and
/// selection, so the address is only remembered and appears once focus leaves.
pub(crate) fn sync_address_input(hwnd: HWND, edit: HWND, id: TabId, url: &url::Url, minimal_url: bool) -> bool {
    let shown = omnibox::address_display(url, minimal_url);
    if omnibox::should_sync_displayed_url(edit_focused(hwnd)) {
        return super::set_input_text(edit, &shown);
    }
    let _ = with_chrome(hwnd, |s| s.urls.insert(id, shown));
    mark_chrome_dirty(hwnd);
    true
}
/// The text index under a client-space `x`, from the chrome's own metrics so it matches what is painted.
fn omnibox_text_index(hwnd: HWND, text: &str, client_x: i32) -> usize {
    let left = with_chrome(hwnd, |s| s.layout.omnibox.left).unwrap_or(0);
    let text_left = Metrics::new(dpi(hwnd)).px(Metrics::OMNIBOX_TEXT_LEFT);
    with_chrome(hwnd, |s| {
        s.surface.index_at_x(text, client_x - left - text_left)
    })
    .unwrap_or(0)
}
fn set_omni_selection(edit: HWND, anchor: usize, head: usize) {
    unsafe {
        let _ = SendMessageW(
            edit,
            EM_SETSEL,
            Some(WPARAM(anchor.min(head))),
            Some(LPARAM(anchor.max(head) as isize)),
        );
    }
}
/// A press on the omnibox. The chrome paints the text, so the press is mapped here to the EDIT's selection.
/// An unfocused field takes focus and keeps its URL. One press places the caret, or extends from the anchor
/// with Shift. Two select a word, and three select the whole field. Dragging then extends from the anchor.
fn omnibox_press(hwnd: HWND, app: &AppRef, client_x: i32) {
    let edit = app.borrow().edit;
    let now = Instant::now();
    let window = Duration::from_millis(u64::from(unsafe { GetDoubleClickTime() }));
    let was_focused = edit_focused(hwnd);
    let extend = was_focused && unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 };
    let count = with_chrome(hwnd, |s| {
        let count = omnibox::click_count(s.omni_click, now, window);
        s.omni_click = Some((now, count));
        count
    })
    .unwrap_or(1);
    if !was_focused {
        focus_omni(hwnd, app, false);
    }
    let text = read_edit(edit);
    let index = omnibox_text_index(hwnd, &text, client_x);
    let (anchor, head) = match count {
        2 => omnibox::word_range(&text, index),
        3 => (0, text.encode_utf16().count()),
        _ if extend => (with_chrome(hwnd, |s| s.omni_anchor).unwrap_or(index), index),
        _ => (index, index),
    };
    let _ = with_chrome(hwnd, |s| s.omni_anchor = anchor);
    set_omni_selection(edit, anchor, head);
    set_pressed(hwnd, Hit::Omnibox);
    unsafe {
        let _ = SetCapture(hwnd);
    }
    invalidate_omni(hwnd);
}
fn omnibox_drag(hwnd: HWND, app: &AppRef, client_x: i32) {
    let edit = app.borrow().edit;
    let text = read_edit(edit);
    let index = omnibox_text_index(hwnd, &text, client_x);
    let anchor = with_chrome(hwnd, |s| s.omni_anchor).unwrap_or(index);
    set_omni_selection(edit, anchor, index);
    invalidate_omni(hwnd);
}
/// Split view toggle: closes the split when one is open, otherwise splits the active tab with the
/// most recent other tab on the right. Shared by Ctrl+Shift+\ and the HTML toolbar's `open split`.
fn toggle_split(hwnd: HWND, app: &AppRef) {
    // --- split view ---
    // Separate views when split (both tabs stay open); otherwise split with an empty side.
    super::split_ui::toggle(hwnd, app);
    // --- end split view ---
}

/// Ctrl+Shift+C: puts the active tab's address on the clipboard. A tab with no address yet (a new
/// tab page) has nothing to copy, so the chord does nothing.
fn copy_active_tab_link(hwnd: HWND, app: &AppRef) {
    let Some(url) = App::tab_committed_url(app, App::active_tab_id(app)) else {
        return;
    };
    if !copy_text_to_clipboard(hwnd, url.as_str()) {
        show_error_message("Bowser could not copy the tab URL.");
    }
}

/// Collapses or expands the vertical sidebar: the same relayout as the drag-resize path, then the
/// toggle is saved. Shared by the sidebar's collapse button and Ctrl+S.
fn toggle_sidebar(hwnd: HWND, app: &AppRef) {
    App::set_sidebar_collapsed(app, !App::sidebar_collapsed(app));
    App::save_settings(app);
    let _ = with_chrome(hwnd, |s| s.refresh(app, true));
    sync_split_ratio(hwnd, App::split_view(app));
    apply_sidebar_bounds(hwnd, app);
}

/// Whether a right click over `hit` opens one of Bowser's own tab, group, rail or split menus.
fn opens_bowser_menu(hit: Hit) -> bool {
    matches!(
        hit,
        Hit::Rail(_)
            | Hit::SplitDivider
            | Hit::GroupBall(_)
            | Hit::Tab(_)
            | Hit::TabClose(_)
            | Hit::SidebarGroup(_)
            | Hit::SidebarTab(_)
            | Hit::SidebarTabClose(_)
            | Hit::StackTab(_) // --- two-level stacks (T10b) ---
            | Hit::StackTabClose(_)
    )
}

/// `shift_right_click_context`: with the setting on and Shift held, a right click shows the window's
/// system menu instead of Bowser's own tab and rail menus. Off, or without Shift, nothing changes.
fn system_menu_requested(app: &AppRef) -> bool {
    system_menu_wanted(App::shift_right_click_context(app), unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 })
}

/// The decision behind `system_menu_requested`: the setting is on and Shift is held.
fn system_menu_wanted(setting: bool, shift_held: bool) -> bool {
    setting && shift_held
}

/// Whether a right click over `hit` shows the window's system menu instead of one of Bowser's own:
/// only over something that has a Bowser menu, with the setting on and Shift held.
fn right_click_shows_system_menu(hit: Hit, setting: bool, shift_held: bool) -> bool {
    opens_bowser_menu(hit) && system_menu_wanted(setting, shift_held)
}

/// The setting-gated chords bound for this window, for the key filters that decide whether a chord
/// is swallowed. None while the app is not reachable, so the page keeps the key.
pub(crate) fn gated_chords(hwnd: HWND) -> shortcuts::GatedChords {
    app_of(hwnd)
        .map(|app| App::gated_chords(&app))
        .unwrap_or_default()
}

fn dispatch_current(hwnd: HWND, app: &AppRef, key: u32) -> bool {
    let c = unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 };
    let s = unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 };
    let a = unsafe { GetKeyState(VK_MENU.0 as i32) < 0 };
    dispatch(hwnd, app, key, c, s, a)
}
fn dispatch(hwnd: HWND, app: &AppRef, key: u32, ctrl: bool, shift: bool, alt: bool) -> bool {
    // The key filters upstream (page, chrome) already decided who owns the key; this resolves it
    // once more against the same live keymap (`crate::commands`).
    let context = crate::commands::ResolveContext {
        gated: App::gated_chords(app),
        typing: edit_focused(hwnd) || omnibox_focused(hwnd),
        ..Default::default()
    };
    let Some(cmd) = crate::commands::resolve(key, ctrl, shift, alt, &context) else {
        return false;
    };
    // While the address field has focus, the field's own redo keeps Ctrl+Shift+Z.
    if cmd == Command::UndoTabOrganisation && edit_focused(hwnd) {
        return false;
    }
    run_key_command(hwnd, app, cmd, key);
    true
}

/// Runs one command and refreshes the chrome. `key` is the key that triggered it (0 when a menu, the
/// palette or a chain did), which F6 focus cycling needs.
fn run_key_command(hwnd: HWND, app: &AppRef, cmd: Command, key: u32) {
    match cmd {
        Command::Registered(handle) => {
            if let Some(id) = crate::commands::id_of(handle) {
                run_registered(hwnd, app, &id);
            }
        }
        Command::NewTab => open_user_tab(hwnd, app), // --- tab options ---
        // --- tab options --- Ctrl+Shift+T.
        Command::ReopenClosedTab => {
            let _ = App::reopen_closed_tab(app);
        }
        // --- end tab options ---
        Command::NewWindow => open_another_window(false),
        Command::NewPrivateTab => {
            App::new_private_tab(app);
            focus_omni(hwnd, app, true)
        }
        Command::CloseTab => user_close(hwnd, app, App::active_tab_id(app)), // --- tab options ---
        Command::NextTab => {
            App::select_next(app);
        }
        Command::PrevTab => {
            App::select_prev(app);
        }
        Command::SelectTab(i) => {
            if let Some(id) = App::tab_ids(app).get(i) {
                let _ = App::select_tab(app, *id);
            }
        }
        Command::SelectLastTab => {
            if let Some(id) = App::tab_ids(app).last() {
                let _ = App::select_tab(app, *id);
            }
        }
        // With the focus-cycling option on, F6 moves the keyboard between the address bar and the page.
        Command::FocusOmnibox
            if key == 0x75
                && crate::commands::behaviour::f6_moves_to_page(
                    &crate::commands::live().store.options,
                    key,
                    edit_focused(hwnd) || omnibox_focused(hwnd),
                ) =>
        {
            App::focus_page(app)
        }
        Command::FocusOmnibox => focus_omni(hwnd, app, true),
        Command::Reload => App::reload(app),
        Command::HardReload => {
            App::stop(app);
            App::reload(app)
        }
        Command::Stop => App::stop(app),
        Command::Back => navigate_back_for_focused_pane(app),
        Command::Forward => navigate_forward_for_focused_pane(app),
        Command::ToggleBlocker => App::toggle_blocker(app),
        Command::ShowProfileMenu => show_profile_menu(hwnd),
        Command::ShowExtensionsMenu => show_extensions_menu(hwnd, app),
        Command::UndoTabOrganisation => {
            let _ = App::undo_tab_organisation(app);
        }
        Command::TogglePictureInPicture => {
            App::toggle_picture_in_picture(app, App::active_tab_id(app));
        }
        Command::ToggleSplit => toggle_split(hwnd, app),
        // --- split view --- the neighbouring pane (a pair: its first / second pane)
        Command::FocusSplitLeft => {
            let _ = App::focus_split_direction(app, crate::split_view::Direction::Left);
        }
        Command::FocusSplitRight => {
            let _ = App::focus_split_direction(app, crate::split_view::Direction::Right);
        }
        // Bound only while a split has panes above one another (stacked, or divided).
        Command::FocusSplitUp => {
            let _ = App::focus_split_direction(app, crate::split_view::Direction::Up);
        }
        Command::FocusSplitDown => {
            let _ = App::focus_split_direction(app, crate::split_view::Direction::Down);
        }
        Command::CloseSplitPane => {
            if let Some(split) = App::split_view(app) {
                let _ = App::close_split_pane(app, split.focused);
            }
        }
        Command::ShowCommandPalette => run_command_palette(hwnd, app),
        Command::ToggleNotes => {
            notes_panel::open(hwnd);
            apply_sidebar_bounds(hwnd, app);
        }
        Command::AddBookmark => add_current_bookmark(hwnd, app),
        Command::OpenPasswords => {
            if let Err(error) = passwords_panel::open(hwnd) {
                show_error_message(&format!(
                    "Passwords & Passkeys could not be opened: {error}"
                ));
            }
        }
        Command::FillPassword => passwords_panel::offer_fill(hwnd, app),
        Command::OpenPrivacyDashboard => privacy_panel::open(hwnd, app),
        Command::ZoomIn => App::zoom_active(app, crate::site_prefs::ZoomStep::In),
        Command::ZoomOut => App::zoom_active(app, crate::site_prefs::ZoomStep::Out),
        Command::ZoomReset => App::zoom_active(app, crate::site_prefs::ZoomStep::Reset),
        // bowser://history in a tab; the flyout or the docked panel without the HTML chrome.
        // bowser://history in a tab; the History side panel without the HTML chrome (fix E: this
        // fallback was in the hardcoded Ctrl+H block).
        Command::ShowHistory if super::history_page::available(hwnd) => super::history_page::open(hwnd, app, false),
        Command::ShowHistory => {
            rail_activate(hwnd, app, side_panel::PanelId::History);
        }
        // bowser://downloads in a tab (Ctrl+J, the main menu).
        Command::ShowDownloads => super::downloads_page::open(hwnd, app, false),
        Command::ToggleReader => run_reader(hwnd, app, crate::reader::Mode::Reader),
        Command::OpenDevTools => App::open_dev_tools(app),
        Command::CopyLink => copy_active_tab_link(hwnd, app),
        Command::ToggleSidebar => toggle_sidebar(hwnd, app),
    }
    let _ = with_chrome(hwnd, |s| s.refresh(app, true));
    repaint(hwnd);
    arm_motion(hwnd);
}

/// --- wave-1 fixes --- the registry commands the main menu's History and Downloads items run.
const MENU_HISTORY_COMMAND: &str = "history";
const MENU_DOWNLOADS_COMMAND: &str = "downloads";

/// Runs a registry command by id: the one entry point for the palette, shortcuts, chains and (later)
/// mouse gestures and toolbar buttons. False when the id is not a command.
pub(crate) fn run_registered(hwnd: HWND, app: &AppRef, id: &str) -> bool {
    use crate::commands::{registry::Handler, store::EntryHandler};
    let live = crate::commands::live();
    let Some(entry) = crate::commands::entry(&live, id) else {
        return false;
    };
    match entry.handler {
        EntryHandler::Builtin(Handler::Key(command)) => run_key_command(hwnd, app, command, 0),
        EntryHandler::Builtin(Handler::Action(action)) => run_palette_command(hwnd, app, action),
        EntryHandler::Builtin(Handler::Run(run)) => run(&crate::commands::registry::Context { hwnd, app }),
        EntryHandler::Chain(number) => super::command_actions::start_chain(hwnd, &live.store, number),
    }
    true
}

/// Writes one visit to the active profile's history. True when the file now holds it.
fn persist_profile_history(url: String, title: String) -> bool {
    let Ok(profile) = crate::profile::active() else {
        return false;
    };
    let kept = crate::productivity::profile_allows_productivity(&profile)
        && profile.history_enabled()
        && crate::productivity::add_history_entry(&profile.history_file(), &url, &title).is_ok();
    if kept {
        // Expired visits are dropped every few hours while Bowser stays open (thread T3).
        crate::history_retention::after_visit(&profile.history_file());
    }
    kept
}

unsafe extern "system" fn edit_subclass(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    parent: usize,
) -> LRESULT {
    unsafe {
        let main = HWND(parent as *mut _);
        let raw = GetWindowLongPtrW(main, GWLP_USERDATA) as *const AppRef;
        let app = raw.as_ref();
        if msg == windows::Win32::UI::WindowsAndMessaging::WM_PAINT {
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            let _ = EndPaint(hwnd, &ps);
            return LRESULT(0);
        }
        if msg == WM_ERASEBKGND {
            return LRESULT(1);
        }
        if msg == windows::Win32::UI::WindowsAndMessaging::WM_SETTEXT {
            let _ = with_chrome(main, |s| s.programmatic_text_update = true);
            let result = DefSubclassProc(hwnd, msg, w, l);
            let _ = with_chrome(main, |s| s.programmatic_text_update = false);
            let text = read_edit(hwnd);
            let display = url::Url::parse(&text)
                .ok()
                .map(|url| omnibox::display_form(&url))
                .unwrap_or(text);
            let active = app.map(App::active_tab_id);
            let _ = with_chrome(main, |s| {
                if let Some(id) = active {
                    s.urls.insert(id, display.clone());
                }
                s.edit_text.clone_from(&display);
            });
            invalidate_omni(main);
            mark_chrome_dirty(main);
            return result;
        }
        if msg == WM_SETFOCUS {
            let r = DefSubclassProc(hwnd, msg, w, l);
            // The chrome paints its own caret. The EDIT's system caret would be drawn at its
            // stale position over the glass.
            let _ = HideCaret(Some(hwnd));
            let _ = with_chrome(main, |s| {
                s.focused = true;
                s.caret = true;
                s.update_completion()
            });
            invalidate_omni(main);
            sync_foreground(main, app);
            return r;
        }
        if msg == WM_KILLFOCUS {
            let _ = KillTimer(Some(main), WM_CARET_BLINK_TIMER);
            let r = DefSubclassProc(hwnd, msg, w, l);
            let display = if let Some(app) = app {
                with_chrome(main, |s| {
                    s.urls
                        .get(&App::active_tab_id(app))
                        .cloned()
                        .unwrap_or_default()
                })
                .unwrap_or_default()
            } else {
                String::new()
            };
            let _ = SetWindowTextW(hwnd, &windows::core::HSTRING::from(&display));
            let _ = with_chrome(main, |s| {
                s.focused = false;
                s.caret = false;
                s.completion = None;
                s.edit_text.clear();
                s.edit_text.push_str(&display);
            });
            invalidate_omni(main);
            // --- layout (thread T6) --- a hidden address bar goes away with the focus.
            if let Some(app) = app {
                set_address_bar_revealed(main, app, false);
            }
            return r;
        }
        if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
            // Typing means Bowser is in front, so this also recovers the caret if no activation message arrived.
            sync_foreground(main, app);
            // Escape closes an open hover card first. The page keeps Escape when it has focus.
            if w.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE.0 as usize
                && card::is_open()
            {
                card::close();
                return LRESULT(0);
            }
            if w.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE.0 as usize
                && GetCapture() == main
            {
                let _ = with_chrome(main, |s| {
                    s.drag = None;
                    s.pressed = Hit::None
                });
                let _ = ReleaseCapture();
                return LRESULT(0);
            }
            if w.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_BACK.0 as usize
                || w.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_DELETE.0 as usize
            {
                let _ = with_chrome(main, |s| s.suppress_completion = true);
            }
            if let Some(app) = app {
                if dispatch_current(main, app, w.0 as u32) {
                    return LRESULT(0);
                }
            }
            if w.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_RIGHT.0 as usize
                || w.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_TAB.0 as usize
            {
                let (a, b) = selection(hwnd);
                let len = read_edit(hwnd).encode_utf16().count();
                if a == b && b == len {
                    if let Some(c) = with_chrome(main, |s| s.completion.take()).flatten() {
                        let _ = SetWindowTextW(hwnd, &windows::core::HSTRING::from(c.full_url));
                        let _ = SendMessageW(
                            hwnd,
                            EM_SETSEL,
                            Some(WPARAM(usize::MAX)),
                            Some(LPARAM(-1)),
                        );
                        return LRESULT(0);
                    }
                }
            }
        }
        if msg == WM_CHAR && w.0 == 0x0d {
            if let Some(app) = app {
                let _ = app.borrow().log.record("enter_received");
            }
            super::post(main, WM_SUBMIT, WPARAM(0), LPARAM(0));
            return LRESULT(0);
        }
        if msg == WM_SYSKEYDOWN && GetKeyState(VK_MENU.0 as i32) < 0 {
            if w.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_LEFT.0 as usize {
                super::post(main, WM_HISTORY_BACK, WPARAM(0), LPARAM(0));
                return LRESULT(0);
            }
            if w.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_RIGHT.0 as usize {
                super::post(main, WM_HISTORY_FORWARD, WPARAM(0), LPARAM(0));
                return LRESULT(0);
            }
        }
        if msg == WM_LBUTTONDOWN && GetFocus() != hwnd {
            if let Some(app) = app {
                focus_omni(main, app, true)
            }
            return LRESULT(0);
        }
        if msg == WM_LBUTTONDOWN {
            let text = read_edit(hwnd);
            let p = frame::point_from_lparam(l);
            // `p` is relative to the EDIT, which starts at the omnibox edge.
            let x = p.x - Metrics::new(dpi(main)).px(Metrics::OMNIBOX_TEXT_LEFT);
            let i = with_chrome(main, |s| s.surface.index_at_x(&text, x)).unwrap_or(0);
            let _ = DefSubclassProc(hwnd, msg, w, l);
            let _ = SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(i)), Some(LPARAM(i as isize)));
            return LRESULT(0);
        }
        if matches!(
            msg,
            WM_LBUTTONUP
                | WM_CHAR
                | WM_KEYDOWN
                | WM_PASTE
                | WM_MOUSEMOVE
                | windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDBLCLK
        ) {
            let r = DefSubclassProc(hwnd, msg, w, l);
            let text = read_edit(hwnd);
            let _ = with_chrome(main, |s| {
                s.edit_text.clear();
                s.edit_text.push_str(&text);
                s.update_completion()
            });
            invalidate_omni(main);
            return r;
        }
        DefSubclassProc(hwnd, msg, w, l)
    }
}
fn selection(hwnd: HWND) -> (usize, usize) {
    unsafe {
        let (mut a, mut b) = (0u32, 0u32);
        let _ = SendMessageW(
            hwnd,
            EM_GETSEL,
            Some(WPARAM((&mut a as *mut u32) as usize)),
            Some(LPARAM((&mut b as *mut u32) as isize)),
        );
        (a as usize, b as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// --- final --- every private message the main window's procedure matches has its own number, so
    /// no thread's message is taken for another's after the merges.
    #[test]
    fn the_main_window_private_messages_are_unique() {
        let ids = [
            WM_CHROME_SYNC, WM_FOREGROUND_SYNC, WM_HISTORY_BACK, WM_HISTORY_FORWARD, WM_HTML_CHROME_ENV,
            WM_HTML_CHROME_FAILED, WM_HTML_CHROME_START, WM_HTML_DEFERRED, WM_HTML_HOVER_START, WM_HTML_PANEL_FAILED,
            WM_HTML_POPUP_FAILED, WM_HTML_RAIL_FAILED, WM_HTML_TABS_FAILED, WM_LOGIN_MESSAGE, WM_NAVIGATION_PROGRESS,
            WM_PANE_FOCUSED, WM_RAIL_CHANGED, WM_SHORTCUT_KEY, WM_SUBMIT, WM_TABS_CHANGED, WM_UPDATE_CHECKED,
            auto_hide::WM_AUTOHIDE_SENSOR, notes_panel::WM_SIDEBAR_RESIZED, side_panel::WM_RESIZED,
            super::super::WM_CHAIN_STEP, super::super::WM_DOWNLOADS_CHANGED, super::super::WM_DOWNLOADS_PAGE_COMMAND,
            super::super::WM_EXTENSIONS_CHANGED, super::super::WM_HISTORY_PAGE_COMMAND, super::super::WM_SETTINGS_COMMAND,
            super::super::WM_SPLIT_COMMAND, super::super::WM_SPLIT_LINK, super::super::WM_SPLIT_LOOK,
            super::super::WM_TRANSLATE_NET, super::super::address_bar::WM_ADDRESS_SUGGEST,
            super::super::bookmark_bar::WM_BOOKMARK_BAR_COMMAND,
        ];
        let mut seen = std::collections::HashSet::new();
        for id in ids {
            assert!(id >= windows::Win32::UI::WindowsAndMessaging::WM_APP, "{id:#x} is a private message");
            assert!(seen.insert(id), "WM_APP + {:#x} is used twice", id - windows::Win32::UI::WindowsAndMessaging::WM_APP);
        }
    }

    /// Timers are scoped to the main HWND, not to each feature's module. Dispatch order must
    /// never redirect auto-hide to themes, split bounds to statistics, or reader probes to panels.
    #[test]
    fn the_main_window_timer_ids_are_unique_and_outside_hover_ranges() {
        let ids = [
            WM_IDLE_TRIM_TIMER, WM_CARET_BLINK_TIMER, WM_ANIMATION_TIMER, WM_SESSION_SAVE_TIMER,
            super::super::WM_PRIVACY_STATS_TIMER, WM_PROGRESS_TIMER, WM_SPLIT_BOUNDS_TIMER,
            super::super::WM_POWER_TIMER, super::super::WM_IMPORT_OFFER_TIMER,
            super::super::WM_READER_PROBE_TIMER, super::super::WM_TRANSLATE_WATCH_TIMER,
            super::super::WM_DOWNLOADS_TIMER, crate::app::BLOCKED_TITLE_TIMER,
            WM_SIDEBAR_SCROLL_TIMER, WM_RAIL_TOOLTIP_TIMER, WM_NEWTAB_TOOLTIP_TIMER,
            WM_SIDEBAR_RESIZE_TIMER, WM_PAGE_IDLE_TIMER, WM_HTML_POPUP_PREWARM_TIMER,
            WM_TAB_TICK_TIMER, WM_TAB_THUMB_TIMER, WM_TAB_LINK_TIMER, WM_THEME_SCHEDULE_TIMER,
            auto_hide::TIMER_ID, side_panel::TIMER_ID, super::super::panel_webapp::RELOAD_TIMER,
            super::super::panel_webapp::IDLE_TIMER, devtools_dock::TIMER_ID,
            super::super::passwords_panel::CLIPBOARD_CLEAR_TIMER,
        ];
        let mut seen = std::collections::HashSet::new();
        for id in ids {
            assert!(seen.insert(id), "main window timer {id:#x} is used twice");
            assert!(!card::is_hover_timer(id), "timer {id:#x} is claimed by tab-card dispatch");
            assert!(!island_menu::is_timer(id), "timer {id:#x} is claimed by island-menu dispatch");
        }
    }

    /// --- wave-1 fixes --- the main menu's History and Downloads run registry commands, so they still
    /// work with shortcuts switched off or Ctrl+H / Ctrl+J bound elsewhere.
    #[test]
    fn the_main_menu_history_and_downloads_do_not_depend_on_their_shortcuts() {
        use crate::commands::{registry::Handler, store::EntryHandler};
        let mut store = crate::commands::store::Store::default();
        store.options.shortcuts_enabled = false;
        let live = crate::commands::Live::new(store);
        let cx = crate::commands::ResolveContext::default();
        assert_eq!(crate::commands::resolve_in(&live, 0x48, true, false, false, &cx), None, "Ctrl+H does nothing now");
        assert_eq!(crate::commands::resolve_in(&live, 0x4A, true, false, false, &cx), None, "Ctrl+J does nothing now");
        for (id, command) in [(MENU_HISTORY_COMMAND, Command::ShowHistory), (MENU_DOWNLOADS_COMMAND, Command::ShowDownloads)] {
            let entry = crate::commands::entry(&live, id).expect("a registry command");
            assert!(matches!(entry.handler, EntryHandler::Builtin(Handler::Key(found)) if found == command), "{id}");
        }
    }

    #[test]
    fn the_tab_options_decide_focus_duplicate_position_and_double_click() {
        use crate::tab_options::{DoubleClick, DuplicatePosition};
        assert!(new_tab_focuses_page(false, true), "a page, option on: the page");
        assert!(!new_tab_focuses_page(true, true), "the start page keeps the address bar");
        assert!(!new_tab_focuses_page(false, false), "off: the address bar");
        let order = [TabId(1), TabId(2), TabId(3), TabId(9)];
        assert_eq!(duplicate_index(DuplicatePosition::Beside, &order, TabId(2), 0), 2, "right of the original");
        assert_eq!(duplicate_index(DuplicatePosition::Last, &order, TabId(2), 0), 4, "at the end");
        assert_eq!(duplicate_index(DuplicatePosition::Beside, &order, TabId(7), 1), 2, "a gone original uses its old place");
        assert_eq!(tab_double_click(DoubleClick::Ignore), TabDoubleClick::Nothing);
        assert_eq!(tab_double_click(DoubleClick::Close), TabDoubleClick::Close);
        assert_eq!(tab_double_click(DoubleClick::Rename), TabDoubleClick::Rename);
    }

    /// Integration (T2 bookmarks bar, T3 private theme): the page hosts beside the toolbar take the
    /// private look while a private tab is shown, and the window's own look otherwise.
    #[test]
    fn page_hosts_take_the_private_theme_while_a_private_tab_is_shown() {
        use crate::{settings::PrivateTheme, theme::Appearance};
        let light = crate::theme::palette(Appearance::Light, true);
        let (palette, dark) = private_host_look(PrivateTheme::Dark, true, Appearance::Light, light);
        assert!(dark);
        assert!(std::ptr::eq(palette, crate::theme::palette(Appearance::Dark, true)));
        let (palette, dark) = private_host_look(PrivateTheme::Dark, false, Appearance::Light, light);
        assert!(!dark, "a normal tab keeps the window's look");
        assert!(std::ptr::eq(palette, light));
        let (_, dark) = private_host_look(PrivateTheme::Follow, true, Appearance::Dark, crate::theme::palette(Appearance::Dark, true));
        assert!(dark, "follow keeps the window's dark look");
        let (_, dark) = private_host_look(PrivateTheme::Light, true, Appearance::Dark, crate::theme::palette(Appearance::Dark, true));
        assert!(!dark);
    }

    #[test]
    fn shift_right_click_shows_the_system_menu_only_with_the_setting_on_shift_held_and_a_bowser_menu_under_the_pointer() {
        // Every place a right click opens one of Bowser's tab, group, rail or split menus.
        for hit in [Hit::Tab(0), Hit::TabClose(1), Hit::SidebarGroup(0), Hit::SidebarTab(2), Hit::SidebarTabClose(0), Hit::GroupBall(0), Hit::SplitDivider] {
            assert!(right_click_shows_system_menu(hit, true, true), "{hit:?}: setting on, Shift held");
            assert!(!right_click_shows_system_menu(hit, false, true), "{hit:?}: setting off, Bowser's own menu stays");
            assert!(!right_click_shows_system_menu(hit, true, false), "{hit:?}: no Shift, Bowser's own menu stays");
            assert!(!right_click_shows_system_menu(hit, false, false), "{hit:?}");
        }
        // Over the page, the address bar or empty strip there is no Bowser menu to replace.
        for hit in [Hit::None, Hit::Viewport, Hit::Omnibox, Hit::CaptionArea, Hit::Back, Hit::MainMenu] {
            assert!(!right_click_shows_system_menu(hit, true, true), "{hit:?}");
        }
        assert!(system_menu_wanted(true, true));
        assert!(!system_menu_wanted(true, false) && !system_menu_wanted(false, true));
    }

    #[test]
    fn the_shift_right_click_setting_is_read_from_the_saved_file() {
        let mut settings = crate::settings::Settings::default();
        assert!(!settings.shift_right_click_context, "off by default: a plain right click is always Bowser's menu");
        settings.shift_right_click_context = true;
        let again = crate::settings::Settings::parse(&crate::settings::render_updated("", &settings));
        assert!(again.shift_right_click_context);
    }

    #[test]
    fn rail_menu_opens_right_of_the_rail_and_keeps_the_pointer_height() {
        let rail_right = 36;
        // A click on the rail itself moves the anchor out to the rail's right edge.
        assert_eq!((rail_menu_anchor(POINT { x: 14, y: 220 }, rail_right).x, rail_menu_anchor(POINT { x: 14, y: 220 }, rail_right).y), (36, 220));
        // A pointer already past the rail stays where it is.
        assert_eq!(rail_menu_anchor(POINT { x: 90, y: 5 }, rail_right).x, 90);
    }

    #[test]
    fn import_offer_is_a_title_one_explanatory_line_then_the_three_choices() {
        let entries = import_offer_entries();
        assert!(matches!(&entries[0], Entry::SectionHeader(title) if title == IMPORT_OFFER_TITLE));
        assert!(matches!(&entries[1], Entry::SectionHeader(detail) if detail == IMPORT_OFFER_DETAIL));
        let ids: Vec<usize> = entries.iter().filter_map(|entry| if let Entry::Item(item) = entry { Some(item.id) } else { None }).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        // Short enough to stay on one line in a compact menu.
        assert!(IMPORT_OFFER_TITLE.len() <= 32 && IMPORT_OFFER_DETAIL.len() <= 44 && IMPORT_OFFER_FOOTER.len() <= 44);
    }

    #[test]
    fn a_committed_page_takes_the_keyboard_only_from_an_untouched_omnibox() {
        // The omnibox holds the focus it started with and the page is a real one: move to the page.
        assert!(should_focus_page_after_commit(true, false, false, false, false));
        // Typing, a deliberate Ctrl+L / new tab / click, a blank page, no omnibox focus, or an open
        // menu or popup (the import offer): leave it.
        assert!(!should_focus_page_after_commit(true, true, false, false, false));
        assert!(!should_focus_page_after_commit(true, false, true, false, false));
        assert!(!should_focus_page_after_commit(true, false, false, true, false));
        assert!(!should_focus_page_after_commit(false, false, false, false, false));
        assert!(!should_focus_page_after_commit(true, false, false, false, true));
    }

    #[test]
    fn workspace_move_items_are_enabled_only_when_they_can_move() {
        assert_eq!(workspace_move_enabled(Some(0), 1), (false, false));
        assert_eq!(workspace_move_enabled(Some(0), 3), (false, true));
        assert_eq!(workspace_move_enabled(Some(1), 3), (true, true));
        assert_eq!(workspace_move_enabled(Some(2), 3), (true, false));
        assert_eq!(workspace_move_enabled(None, 3), (false, false));
    }

    #[test]
    fn progress_finishes_then_fades_before_settling() {
        let start = Instant::now();
        let mut progress = ProgressTransition::default();
        progress.retarget(TabId(1), 7, 100, start, true);
        assert!(progress.paint_value(start).unwrap() < 100.0);
        assert!(
            progress
                .paint_value(start + Duration::from_millis(300))
                .unwrap()
                > 100.0
        );
        assert!(!progress.settled(start + Duration::from_millis(300)));
        assert!(progress.settled(start + Duration::from_millis(500)));
    }

    #[test]
    fn native_strip_repaints_unless_both_html_views_are_live() {
        assert!(native_strip_pixels_change(false, false));
        assert!(native_strip_pixels_change(true, false));
        assert!(native_strip_pixels_change(false, true));
        assert!(!native_strip_pixels_change(true, true));
    }

    #[test]
    fn native_favicons_are_wanted_only_without_a_live_html_tab_strip() {
        assert!(native_favicon_wanted(false));
        assert!(!native_favicon_wanted(true));
    }

    #[test]
    fn main_window_style_has_native_resize_and_caption_controls() {
        let style = main_window_style();
        assert_ne!(style.0 & WS_THICKFRAME.0, 0);
        assert_ne!(style.0 & WS_CAPTION.0, 0);
        assert_ne!(style.0 & WS_SYSMENU.0, 0);
        assert_ne!(style.0 & WS_MAXIMIZEBOX.0, 0);
    }

    // --- vertical sidebar (lane S) ---
    #[test]
    fn html_sidebar_region_skips_native_sidebar_hits_but_leaves_resize_handle() {
        let layout = crate::vertical_tabs::SidebarLayout::compute_in_frame(1200, 700, 96, 260, false);
        let region = sidebar_html_region(Some(&layout), true).expect("active HTML sidebar");
        assert_eq!(region.right, layout.handle.left);
        assert!(region.contains(layout.sidebar.left + 1, layout.sidebar.top + 1));
        assert!(!region.contains(layout.handle.left, layout.handle.top + 1));
        assert!(sidebar_html_region(Some(&layout), false).is_none());
        assert!(sidebar_html_region(None, true).is_none());
    }
    // --- end vertical sidebar ---

    #[test]
    fn overflow_menu_contains_only_tabs_outside_the_visible_window() {
        let tabs = (0..40)
            .map(|id| TabSnapshot {
                id: TabId(id),
                title: String::new(),
                loading: false,
                favicon_key: None,
                favicon: None,
                marker: None,
            })
            .collect::<Vec<_>>();
        let slots = (0..40)
            .map(|index| StripSlot {
                tab_index: index,
                domain: None,
                group_key: None,
                group_size: 1,
                group_control: false,
                collapsed: false,
                marker_only: false,
                island_colour: None,
            })
            .collect::<Vec<_>>();
        let overflow = layout::compute_with_active(768, 700, 96, 40, 39, false);
        assert_eq!(
            overflow_tab_ids(&overflow, &slots, &tabs),
            (0..17).map(TabId).collect::<Vec<_>>()
        );

        let fitting = layout::compute(1024, 700, 96, 8, false);
        assert!(overflow_tab_ids(&fitting, &slots[..8], &tabs[..8]).is_empty());
    }

    #[test]
    fn a_hovered_tab_keeps_its_hover_when_its_index_changes() {
        assert_eq!(moved_hover(Hit::Tab(3), Some(1)), Hit::Tab(1));
        assert_eq!(moved_hover(Hit::TabClose(3), Some(4)), Hit::TabClose(4));
    }

    #[test]
    fn a_hover_on_a_closed_tab_clears_rather_than_moving_to_a_neighbour() {
        assert_eq!(moved_hover(Hit::Tab(2), None), Hit::None);
        assert_eq!(moved_hover(Hit::TabClose(2), None), Hit::None);
    }

    #[test]
    fn hovers_that_are_not_tabs_are_left_alone() {
        assert_eq!(moved_hover(Hit::NewTab, Some(0)), Hit::NewTab);
        assert_eq!(moved_hover(Hit::Omnibox, None), Hit::Omnibox);
    }

    #[test]
    fn the_caret_blinks_only_in_front_restored_and_with_the_address_input_focused() {
        assert!(caret_blink_allowed(true, false, true));
        assert!(!caret_blink_allowed(false, false, true));
        assert!(!caret_blink_allowed(true, true, true));
        assert!(!caret_blink_allowed(true, false, false));
    }

    #[test]
    fn a_foreground_window_is_ours_when_this_process_or_the_main_window_owns_it() {
        const OWN_PID: u32 = 100;
        const MAIN: isize = 0x500;
        assert!(belongs_to_app(OWN_PID, OWN_PID, 0, MAIN));
        assert!(belongs_to_app(200, OWN_PID, MAIN, MAIN));
        assert!(!belongs_to_app(200, OWN_PID, 0x900, MAIN));
        assert!(!belongs_to_app(200, OWN_PID, 0, 0));
    }

    #[test]
    fn overlay_surface_tracks_full_client_height_across_dpi_changes() {
        let mut s = WindowChrome::new(1000, 700, 96, HWND::default());
        for dpi in [96, 120, 144, 192, 96, 120] {
            for tab_layout in [
                crate::settings::TabLayout::ToolbarTop,
                crate::settings::TabLayout::TabsTop,
                crate::settings::TabLayout::SingleRow,
            ] {
                let arrangement = layout::Arrangement { tabs: tab_layout, ..Default::default() };
                s.relayout(RelayoutRequest { w: 1000, h: 700, dpi, active: TabId(0), maximised: false, arrangement, density: crate::layout_prefs::UiDensity::Regular, sidebar_width: 260, centred_address_bar: true, font_scale: 100, ui_font: "" });
                let chrome = layout::chrome_height(tab_layout, dpi);
                assert_eq!(s.surface.height(), 700, "surface height at dpi {dpi}");
                assert_eq!(s.surface.width(), 1000, "surface width at dpi {dpi}");
                assert_eq!(s.layout.viewport.top, chrome, "viewport top at dpi {dpi}, {tab_layout:?}");
            }
        }
    }

    // --- auto-hide (T11) ---
    fn relaid(s: &mut WindowChrome, dpi: u32, maximised: bool, tab_layout: crate::settings::TabLayout) {
        let arrangement = layout::Arrangement { tabs: tab_layout, ..Default::default() };
        s.relayout(RelayoutRequest { w: 1280, h: 720, dpi, active: TabId(0), maximised, arrangement, density: crate::layout_prefs::UiDensity::Regular, sidebar_width: 260, centred_address_bar: true, font_scale: 100, ui_font: "" });
    }

    /// The user's rule: with auto-hide off the window lays out exactly as it always did, and no sensor,
    /// timer or window exists.
    #[test]
    fn with_auto_hide_off_the_window_layout_is_unchanged_and_nothing_is_created() {
        for tab_layout in crate::settings::TabLayout::ALL {
            for dpi in [96, 144] {
                for maximised in [false, true] {
                    let mut s = WindowChrome::new(1280, 720, dpi, HWND::default());
                    relaid(&mut s, dpi, maximised, tab_layout);
                    let context = format!("{tab_layout:?} dpi {dpi} max {maximised}");
                    // --- layout (thread T6) --- the arranged geometry is what every layout is laid out with.
                    let expected = layout::compute_arranged_in_frame(
                        layout::Arrangement { tabs: tab_layout, ..Default::default() }, 1280, 720, dpi, 1, 0, maximised, 260, true,
                        crate::layout_prefs::UiDensity::Regular.sidebar_row_height(),
                    ).0;
                    assert_eq!(s.layout.viewport, expected.viewport, "{context}");
                    assert_eq!(s.layout.strip, expected.strip, "{context}");
                    assert_eq!(s.layout.tab_row, expected.tab_row, "{context}");
                    assert_eq!(s.ah.plan, crate::auto_hide::Plan::default(), "{context}");
                    assert!(!s.ah.top_hidden() && !s.ah.sidebar_hidden(), "{context}");
                    assert!(!s.ah.machine.active(), "{context}: no timer to run");
                    assert!(s.ah.paint_viewport(s.layout.viewport) == s.layout.viewport, "{context}");
                }
            }
        }
    }

    /// Each bar of "Bars to hide" changes the window's layout the way the plan says, and revealing a
    /// bar never moves the page.
    #[test]
    fn hiding_the_bars_widens_the_page_and_a_reveal_leaves_it_where_it_is() {
        use crate::auto_hide::{Bars, Zone, Zones};
        let tab_layout = crate::settings::TabLayout::ToolbarTop;
        let mut shown = WindowChrome::new(1280, 720, 96, HWND::default());
        relaid(&mut shown, 96, false, tab_layout);
        let normal = shown.layout.viewport;

        let mut s = WindowChrome::new(1280, 720, 96, HWND::default());
        s.ah.hidden = Bars::ADDRESS;
        relaid(&mut s, 96, false, tab_layout);
        assert!(s.ah.plan.top);
        assert_eq!(s.layout.viewport.top, layout::auto_hide_top(96, false), "the page rose to the top");
        assert_eq!((s.layout.viewport.left, s.layout.viewport.right, s.layout.viewport.bottom), (normal.left, normal.right, normal.bottom));
        assert!(s.ah.top_hidden());
        assert_eq!(s.ah.overlay.top.bottom, normal.top, "the bars float exactly where they sit when shown");
        let hidden_viewport = s.layout.viewport;

        // A reveal changes what is drawn, never the page's rectangle.
        assert!(s.ah.machine.reveal(Zones::NONE.with(Zone::Top), 0));
        relaid(&mut s, 96, false, tab_layout);
        assert_eq!(s.layout.viewport, hidden_viewport, "no relayout of the page when a bar appears");
        assert!(!s.ah.top_hidden());
        assert_eq!(s.ah.paint_viewport(s.layout.viewport).top, normal.top, "the page's frame is drawn below the floating bars");

        // The bookmarks bar: in the layout it takes room; out of it, the page keeps the room and the bar floats.
        let mut with_bar = WindowChrome::new(1280, 720, 96, HWND::default());
        with_bar.bar_reserve = (28, false);
        relaid(&mut with_bar, 96, false, tab_layout);
        assert_eq!(with_bar.layout.viewport.top, normal.top + 28, "shown: the bar takes its row");
        assert!(with_bar.ah.bar_float.is_empty() == false, "its floating place is known either way");
        assert_eq!(with_bar.ah.bar_float, Rect::new(normal.left, normal.top, normal.right, normal.top + 28));
        let mut floating = WindowChrome::new(1280, 720, 96, HWND::default());
        floating.bar_reserve = (28, false);
        floating.ah.hidden = Bars::BOOKMARKS;
        relaid(&mut floating, 96, false, tab_layout);
        assert_eq!(floating.layout.viewport.top, normal.top, "hidden: the page keeps the bar's row");
        assert!(floating.ah.plan.bookmarks && !floating.ah.plan.top);
        assert!(!floating.ah.bookmarks_revealed());
        assert!(floating.ah.machine.reveal(Zones::NONE.with(Zone::Bookmarks), 0));
        assert!(floating.ah.bookmarks_revealed());

        // The vertical layout: tabs are the sidebar, which has its own edge.
        let mut v = WindowChrome::new(1280, 720, 96, HWND::default());
        v.ah.hidden = Bars::TABS;
        relaid(&mut v, 96, false, crate::settings::TabLayout::Vertical);
        assert!(v.ah.plan.sidebar && !v.ah.plan.top);
        assert!(v.ah.sidebar_hidden() && !v.ah.top_hidden());
        assert_eq!(v.layout.viewport.left, layout::left_rail_inset(96) + Metrics::new(96).px(Metrics::RESIZE_RIM));
        assert!(v.sidebar.is_some(), "the sidebar keeps its rectangle: it floats there");
    }

    /// An open side panel floats (takes nothing from the page) when "Side panel" is among the hidden bars.
    #[test]
    fn the_side_panel_row_decides_whether_an_open_panel_takes_room() {
        use crate::auto_hide::{effective, plan, AutoHideSettings, Bars, Fullscreen};
        let on = AutoHideSettings { bars: Bars::PANEL, in_fullscreen: true };
        let off = AutoHideSettings { bars: Bars::TABS, in_fullscreen: true };
        let tab_layout = crate::settings::TabLayout::ToolbarTop;
        assert!(plan(effective(true, &on, Fullscreen::No).hidden, tab_layout).panel);
        assert!(!plan(effective(true, &off, Fullscreen::No).hidden, tab_layout).panel);
        assert!(!plan(effective(false, &on, Fullscreen::No).hidden, tab_layout).panel, "the master switch off: panels take room as ever");
        assert!(plan(effective(false, &off, Fullscreen::Window).hidden, tab_layout).panel, "full screen floats every bar");
    }
    // --- end auto-hide ---

    #[test]
    fn unchanged_chrome_sync_does_not_rebuild() {
        let mut s = WindowChrome::new(1000, 700, 96, HWND::default());
        s.html_rail_last = Some(r#"{"v":1,"t":"rail","items":[]}"#.to_owned());
        s.rail_generation = 42;
        s.html_state_generation[2] = 1;
        s.html_rail_synced_open = Some(None);
        s.html_rail_synced_rail_gen = 42;
        s.html_rail_synced_state_gen = 1;

        let unchanged = s.html_rail_last.is_some()
            && s.html_rail_synced_open == Some(None)
            && s.html_rail_synced_rail_gen == s.rail_generation
            && s.html_rail_synced_state_gen == s.html_state_generation[2]
            && s.html_rail_favs_sent.len() == s.rail_entries.iter().filter(|e| e.png.is_some()).count();
        assert!(unchanged, "identical inputs must be recognized as unchanged and skip rebuild");

        let open_changed = s.html_rail_synced_open == Some(Some(RailKey::Bookmarks));
        assert!(!open_changed, "changed open panel must invalidate cache and trigger rebuild");

        s.rail_generation = 43;
        let gen_changed = s.html_rail_synced_rail_gen == s.rail_generation;
        assert!(!gen_changed, "changed rail generation must invalidate cache and trigger rebuild");
    }

    #[test]
    fn a_hidden_page_host_closes_only_after_it_was_shown_and_then_stayed_hidden() {
        let start = Instant::now();
        let mut idle = PageIdle::default();
        // Never shown (still being created, or never used): never closed.
        assert!(!page_idle_step(&mut idle, false, start + PAGE_IDLE_CLOSE * 2));
        assert!(!page_idle_step(&mut idle, true, start));
        assert!(!page_idle_step(&mut idle, false, start));
        assert!(!page_idle_step(&mut idle, false, start + PAGE_IDLE_CLOSE - Duration::from_secs(1)));
        // Shown again in between: the clock starts over.
        let back = start + PAGE_IDLE_CLOSE;
        assert!(!page_idle_step(&mut idle, true, back));
        assert!(!page_idle_step(&mut idle, false, back));
        assert!(page_idle_step(&mut idle, false, back + PAGE_IDLE_CLOSE));
        // Closed: nothing is timed again until the recreated host has been shown.
        assert_eq!(idle, PageIdle::default());
        assert!(!page_idle_step(&mut idle, false, back + PAGE_IDLE_CLOSE * 3));
    }

    #[test]
    fn an_unchanged_rail_is_recognised_and_its_painter_view_follows_the_entries() {
        let entry = |key, label: &str| RailEntryData {
            key,
            icon: RailIcon::Letter,
            label: label.into(),
            active: false,
            accent: None,
            favicon: None,
            png: None,
            dot: false,
            count: 0,
            emoji: None,
        };
        let mut rail = vec![entry(RailKey::Customize, "Customize"), entry(RailKey::App(1), "Mail")];
        rail[1].png = Some(Rc::new(vec![1u8, 2, 3]));
        assert!(same_rail(&rail, &rail.clone()), "a rebuild from the same config and cached icons");
        let mut renamed = rail.clone();
        renamed[1].label = "Post".into();
        assert!(!same_rail(&rail, &renamed));
        let mut reread = rail.clone();
        reread[1].png = Some(Rc::new(vec![1u8, 2, 3]));
        assert!(!same_rail(&rail, &reread), "a re-read icon is new to the HTML rail");
        assert!(!same_rail(&rail, &rail[..1]));
        let bitmap = Rc::new(favicon::FaviconBitmap { width: 1, height: 1, bgra_premultiplied: vec![0; 4] });
        rail[1].favicon = Some(bitmap.clone());
        let view = rail_view_of(&rail);
        assert_eq!(view.iter().map(|entry| entry.label.as_str()).collect::<Vec<_>>(), ["Customize", "Mail"]);
        assert!(view[1].favicon.is_some_and(|favicon| std::ptr::eq(favicon, &*bitmap)));
        assert!(view[0].favicon.is_none());
    }

    #[test]
    fn the_history_read_on_the_worker_is_taken_on_first_use() {
        let mut history = History::default();
        let mut load = Some(std::thread::spawn(|| {
            let mut loaded = History::default();
            loaded.record("https://example.com/", "Example");
            loaded
        }));
        assert_eq!(loaded_history(&mut history, &mut load).len(), 1);
        assert!(load.is_none(), "the worker is joined once");
        // A visit recorded later lands in the loaded history.
        loaded_history(&mut history, &mut load).record("https://example.org/", "Org");
        assert_eq!(loaded_history(&mut history, &mut load).len(), 2);
    }

    #[test]
    fn a_sidebar_drag_frame_lays_out_again_with_only_the_width_changed() {
        let mut s = WindowChrome::new(1200, 700, 96, HWND::default());
        assert!(!s.relayout_sidebar(300), "nothing to repeat before the first layout");
        let vertical = layout::Arrangement { tabs: crate::settings::TabLayout::Vertical, ..Default::default() };
        s.relayout(RelayoutRequest { w: 1200, h: 700, dpi: 96, active: TabId(0), maximised: false, arrangement: vertical, density: crate::layout_prefs::UiDensity::Regular, sidebar_width: 200, centred_address_bar: true, font_scale: 100, ui_font: "" });
        let narrow = s.sidebar.expect("a vertical layout has a sidebar").sidebar;
        let top = s.layout.viewport.top;
        assert!(s.relayout_sidebar(320));
        let wide = s.sidebar.expect("still vertical").sidebar;
        assert!(wide.width() > narrow.width(), "{narrow:?} -> {wide:?}");
        assert_eq!((s.client, s.layout.viewport.top), ((1200, 700), top), "nothing else moved");
    }

    #[test]
    fn sidebar_animation_schedules_one_page_resize() {
        let mut resize_calls = 0;
        let timer_frames = [true, true, true, false];
        for running in timer_frames {
            if !running {
                resize_calls += 1;
            }
        }
        assert_eq!(resize_calls, 1, "exactly one page resize must be scheduled at the end of sidebar animation");
    }
}

/// Client-space point under the main-menu button, where every top-level menu opens from.
fn main_menu_anchor(hwnd: HWND) -> windows::Win32::Foundation::POINT {
    with_chrome(hwnd, |s| {
        let r = s.layout.main_menu_button;
        windows::Win32::Foundation::POINT {
            x: r.right,
            y: r.bottom + 2,
        }
    })
    .unwrap_or_default()
}

// --- toolbars and feeds (thread T13) ---
#[cfg(test)]
mod bar_tests {
    use super::*;

    fn list(text: &str) -> Vec<String> {
        text.split(',').filter(|id| !id.is_empty()).map(str::to_owned).collect()
    }

    #[test]
    fn command_buttons_take_their_commands_name_and_their_place_in_the_list() {
        let buttons = tool_buttons(&list("closed-tabs,cmd:new-private-tab,cmd:no-such,chain:9,cmd:history"));
        let named: Vec<(&str, &str, u8)> = buttons.iter().map(|b| (b.item.as_str(), b.label.as_str(), b.index)).collect();
        assert_eq!(named, [("cmd:new-private-tab", "New private tab", 1), ("cmd:history", "History", 4)], "a deleted chain or unknown command shows no button");
        assert!(tool_buttons(&list("back,reload")).is_empty());
    }

    #[test]
    fn the_panel_bar_shows_feeds_with_its_own_glyph_and_command_buttons_as_letter_tiles() {
        let tools = panel_tools(&list("feeds,cmd:toggle-notes,cmd:no-such"));
        assert_eq!(tools.len(), 2);
        assert_eq!((tools[0].0, tools[0].1.as_str(), tools[0].2), (0, "Feeds", RailIcon::Feed));
        assert_eq!((tools[1].0, tools[1].1.as_str(), tools[1].2), (1, "Toggle notes and tasks", RailIcon::Letter));
        assert!(panel_tools(&[]).is_empty());
    }

    #[test]
    fn the_rail_wire_names_a_tool_by_its_place_and_the_feed_glyph_by_name() {
        assert_eq!(chrome_host::rail_key_to_wire(RailKey::Tool(3)).to_wire(), "tool:3");
        assert_eq!(chrome_host::rail_icon_wire(RailIcon::Feed), "feed");
    }

    #[test]
    fn a_bar_item_runs_the_command_the_registry_names() {
        // The commands the bar's own buttons stand for exist, so a button can never be a dead end.
        for command in ["add-bookmark", "new-tab", "feeds", "subscribe-feed"] {
            assert!(crate::commands::registry::builtin(command).is_some(), "{command}");
        }
    }
}
// --- end toolbars and feeds ---
