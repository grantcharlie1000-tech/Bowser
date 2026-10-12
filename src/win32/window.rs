use super::menu::{Entry, Icon, Item, Menu, ProfileHeader};
use super::{
    INPUT_ID, WM_ANIMATION_TIMER, WM_CARET_BLINK_TIMER, WM_FOREGROUND_SYNC, WM_HISTORY_BACK,
    WM_HISTORY_FORWARD, WM_IDLE_TRIM_TIMER, WM_LOGIN_MESSAGE, WM_NAVIGATION_PROGRESS,
    WM_PANE_FOCUSED, WM_PROGRESS_TIMER, WM_SESSION_SAVE_TIMER, WM_SHORTCUT_KEY,
    WM_SPLIT_BOUNDS_TIMER, WM_SUBMIT, WM_TABS_CHANGED, WM_RAIL_CHANGED, WM_HTML_CHROME_ENV, card, command_palette, frame, glass,
    import_panel, island_menu, menu, notes_panel, passwords_panel, privacy_panel, settings_panel,
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
        self, host::{Accel, AccelHandler, ChromeEnv, ChromeHost, View, accel_command},
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
    rc::Rc,
    time::{Duration, Instant},
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
            Controls::{EM_GETSEL, EM_SETMARGINS, EM_SETSEL},
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
                GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, HMENU, HTCAPTION,
                HTCLOSE, HTMAXBUTTON, HTMINBUTTON, HWND_TOP, HideCaret, IDC_SIZEWE, IsIconic,
                IsZoomed, KillTimer, LoadCursorW, MINMAXINFO, MSG, MoveWindow, NCCALCSIZE_PARAMS,
                PostMessageW, PostQuitMessage, RegisterClassW, SC_CLOSE, SC_KEYMENU, SC_MAXIMIZE,
                SC_MINIMIZE, SC_MOVE, SC_RESTORE, SC_SIZE, SIZE_MINIMIZED, SM_CXFRAME,
                SM_CXPADDEDBORDER, SM_CYFRAME, SM_SWAPBUTTON, SPI_GETCLIENTAREAANIMATION, SW_HIDE, SW_SHOW,
                SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
                SendMessageW, SetCursor, SetForegroundWindow, SetTimer, SetWindowLongPtrW,
                SetWindowPos, SetWindowTextW, ShowWindow, SystemParametersInfoW, TPM_RETURNCMD,
                TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WINDOW_STYLE,
                WINEVENT_OUTOFCONTEXT,
                WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_DPICHANGED,
                WM_DWMCOLORIZATIONCOLORCHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN,
                WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONUP,
                WM_MOUSEMOVE, WM_MOVE, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCCREATE, WM_NCDESTROY,
                WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP,
                WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_NCPAINT, WM_NCRBUTTONUP, WM_PAINT, WM_PASTE,
                WM_RBUTTONUP, WM_SETFOCUS, WM_SETICON, WM_SETTEXT, WM_SETTINGCHANGE, WM_SIZE,
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
const WM_NCUAHDRAWCAPTION: u32 = 0x00AE;
const WM_NCUAHDRAWFRAME: u32 = 0x00AF;
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_SIDEBAR_SCROLL_TIMER: usize = 0xB053;
const WM_RAIL_TOOLTIP_TIMER: usize = 0xB058;
// --- popup host (lane M) ---
const WM_HTML_POPUP_PREWARM_TIMER: usize = 0xB059;
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
    IslandMenu { id: TabId, x: i32, y: i32 },
    BeginDrag,
    ToggleMaximize,
    SystemMenu,
    /// A left click on a rail button: what a native click does. May open a modal prompt.
    RailClick { key: RailKey },
    /// A right click on a rail button: the native rail context menu at the client point.
    RailMenu { key: RailKey, x: i32, y: i32 },
    // --- vertical sidebar (lane S) ---
    SidebarWorkspaceMenu,
    // --- end vertical sidebar ---
    // --- popup host (lane M) ---
    MenuResult { tok: u32, id: Option<u32> },
    // --- end popup host ---
    // --- popup surfaces (lane PP) ---
    /// An in-place action on an open popup page (site info toggle).
    PopupAct { tok: u32, id: u32, n: u8 },
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
struct WindowChrome {
    surface: ChromeSurface,
    overlay: HWND,
    /// Client size in physical pixels. The surface only keeps the chrome rows.
    client: (i32, i32),
    layout: Layout,
    palette: &'static Palette,
    tabs: Vec<TabSnapshot>,
    slots: Vec<StripSlot>,
    sidebar: Option<crate::vertical_tabs::SidebarLayout>,
    sidebar_rows: Vec<crate::chrome::vertical_paint::SidebarRow>,
    sidebar_scroll: i32,
    sidebar_scroll_target: i32,
    sidebar_hover_row: Option<usize>,
    sidebar_resizing: bool,
    rail_entries: Vec<RailEntryData>,
    rail_scroll: i32,
    rail_drag: Option<RailDrag>,
    rail_focus: Option<RailKey>,
    rail_keyboard_focus: bool,
    rail_previous_focus: HWND,
    rail_tooltip_key: Option<RailKey>,
    rail_tooltip_visible: bool,
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
    urls: HashMap<TabId, String>,
    pending_history: HashMap<TabId, String>,
    hover: Hit,
    pressed: Hit,
    drag: Option<(TabId, usize)>,
    split_drag_companion: Option<TabId>,
    split_drop: Option<crate::split_view::Pane>,
    dpi: u32,
    motion: Motion,
    active_tab: Option<TabId>,
    progress: ProgressTransition,
    first_paint_logged: bool,
    split_active: bool,
    split_dragging: bool,
    split_divider: i32,
    pending_split_bounds: Option<(RECT, RECT)>,
    /// The HTML toolbar's WebView2 host, once created. Dropped (and closed) on failure.
    html: Option<ChromeHost>,
    /// Set once the HTML toolbar has been started, so it is never started twice.
    html_requested: bool,
    /// The chrome page has posted `ready`.
    html_page_ready: bool,
    /// True only when the host exists and the page is ready. The HTML region then replaces the
    /// native back, forward, reload, omnibox and main menu controls.
    html_toolbar_active: bool,
    /// The omnibox in the HTML toolbar has keyboard focus (from its `focus` messages).
    html_omnibox_focused: bool,
    /// The last `state` JSON posted to the HTML toolbar, so unchanged state is not sent again.
    html_last_state: Option<String>,
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
        let mut history = History::default();
        if let Ok(profile) = crate::profile::active() {
            if crate::productivity::profile_allows_productivity(&profile)
                && profile.history_enabled()
            {
                for (_, url, title) in crate::productivity::read_history(&profile.history_file()) {
                    history.record(&url, &title);
                }
            }
        }
        Self {
            surface: {
                let mut surface = ChromeSurface::new(width, height, dpi);
                let _ = surface.set_rail_snap(!animations_enabled(), Instant::now());
                surface
            },
            overlay,
            client: (width, height),
            layout: layout::compute_in_frame(width, height, dpi, 1, 0, false),
            palette: glass::palette(),
            tabs: Vec::with_capacity(8),
            slots: Vec::with_capacity(8),
            sidebar: None,
            sidebar_rows: Vec::new(),
            sidebar_scroll: 0,
            sidebar_scroll_target: 0,
            sidebar_hover_row: None,
            sidebar_resizing: false,
            rail_entries: Vec::new(),
            rail_scroll: 0,
            rail_drag: None,
            rail_focus: None,
            rail_keyboard_focus: false,
            rail_previous_focus: HWND::default(),
            rail_tooltip_key: None,
            rail_tooltip_visible: false,
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
            history,
            urls: HashMap::with_capacity(8),
            pending_history: HashMap::with_capacity(8),
            hover: Hit::None,
            pressed: Hit::None,
            drag: None,
            split_drag_companion: None,
            split_drop: None,
            dpi,
            motion: Motion::new(!animations_enabled()),
            active_tab: None,
            progress: ProgressTransition::default(),
            first_paint_logged: false,
            split_active: false,
            split_dragging: false,
            split_divider: width / 2,
            pending_split_bounds: None,
            html: None,
            html_requested: false,
            html_page_ready: false,
            html_toolbar_active: false,
            html_omnibox_focused: false,
            html_last_state: None,
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
        }
    }
    fn refresh(&mut self, app: &AppRef, structural: bool) {
        let now = Instant::now();
        let ids = App::tab_ids(app);
        let active = App::active_tab_id(app);
        self.split_active = App::split_view(app).is_some();
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
        let mut sidebar_tabs = Vec::with_capacity(ids.len());
        let size = Metrics::new(self.dpi).px(Metrics::FAVICON_SIZE).max(1) as u32;
        for &id in &ids {
            let old = self.tabs.iter().find(|t| t.id == id);
            let title = App::tab_title(app, id).unwrap_or_default();
            let title = if title.trim().is_empty() {
                "New tab".to_owned()
            } else {
                title
            };
            let loading = App::tab_loading(app, id);
            let pinned = App::tab_pinned(app, id);
            sidebar_tabs.push((id, title.clone(), pinned, loading));
            let key = App::tab_favicon_key(app, id);
            let image = if old.is_some_and(|t| {
                t.favicon_key == key
                    && (key.is_none()
                        || t.favicon
                            .as_ref()
                            .is_some_and(|image| image.width == size && image.height == size))
            }) {
                old.and_then(|tab| tab.favicon.clone())
            } else if let (Some(k), Some(bytes)) = (key, App::tab_favicon(app, id)) {
                if let Some(cached) = self.favicons.get(k, size, &bytes) {
                    Some(Rc::new(cached.clone()))
                } else if self.failed.contains(&(k, size)) {
                    None
                } else {
                    let decoded = favicon::decode(&bytes, size);
                    if let Some(bitmap) = &decoded {
                        self.favicons.insert(k, size, &bytes, bitmap.clone());
                    } else {
                        if self.failed.len() == 64 {
                            self.failed.pop_front();
                        }
                        self.failed.push_back((k, size));
                    }
                    decoded.map(Rc::new)
                }
            } else {
                None
            };
            if old.is_some_and(|t| t.loading) && !loading {
                if let Some(url) = self.pending_history.remove(&id) {
                    self.history.record(&url, &title);
                    persist_profile_history(url, title.clone());
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
            self.relayout(
                self.client.0,
                self.client.1,
                self.dpi,
                App::active_tab_id(app),
                self.layout.maximised,
                App::tab_layout(app),
                App::sidebar_width(app),
            );
        }
        self.motion.sync_groups(&self.group_specs(), now);
    }
    fn relayout(
        &mut self,
        w: i32,
        h: i32,
        dpi: u32,
        active: TabId,
        maximised: bool,
        tab_layout: crate::settings::TabLayout,
        sidebar_width: u16,
    ) {
        self.client = (w, h);
        self.dpi = dpi.max(96);
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
        let (layout, sidebar) = if tab_layout == crate::settings::TabLayout::Vertical {
            let (layout, sidebar) = layout::compute_vertical_in_frame(
                w, h, self.dpi, self.slots.len().max(1), i, maximised,
                i32::from(sidebar_width),
            );
            (layout, Some(sidebar.with_tab_count(self.sidebar_rows.len(), self.dpi)))
        } else {
            (layout::compute_in_frame(w, h, self.dpi, self.slots.len().max(1), i, maximised), None)
        };
        self.layout = layout.with_group_slots(
            self.slots.iter().map(|slot| slot.marker_only),
            self.slots.iter().map(|slot| slot.group_control && !slot.collapsed),
            self.slots.iter().map(|slot| slot.collapsed && !slot.marker_only),
        );
        if let Some(sidebar) = sidebar {
            self.sidebar_scroll = self.sidebar_scroll.clamp(0, sidebar.max_scroll);
            self.sidebar_scroll_target = self.sidebar_scroll_target.clamp(0, sidebar.max_scroll);
            self.sidebar = Some(sidebar);
        } else {
            self.sidebar = None;
        }
        if was_vertical != self.sidebar.is_some() {
            self.html_tabs_layout = None;
            self.html_tabs_workspace = None;
        }
        self.split_divider = self.layout.viewport.left + self.layout.viewport.width() / 2 - 3;
        self.sync_html_bounds();
        self.sync_html_tabs_bounds();
        self.sync_html_rail_bounds();
        self.surface.resize(w, h, self.dpi)
    }
    /// The toolbar region the HTML chrome covers, in client pixels. `None` while it is inactive or
    /// the toolbar is too narrow for its region.
    fn html_region(&self) -> Option<Rect> {
        if !self.html_toolbar_active {
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
            return sidebar_html_region(self.sidebar.as_ref(), true);
        }
        // --- end vertical sidebar ---
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
    }
    /// Moves the HTML tab strip over the tab row and hides it when there is none. Also tells the
    /// surface to leave the row to the page.
    fn sync_html_tabs_bounds(&mut self) {
        let region = self.html_tab_region();
        self.surface.set_html_tabs(region.is_some());
    }
    /// Moves the HTML chrome over the toolbar region and hides it when there is none. Also tells
    /// the surface to leave the region to the page.
    fn sync_html_bounds(&mut self) {
        let region = self.html_region();
        self.surface.set_html_toolbar(region.is_some());
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
        // Repaint ghosts before pruning, so the frame that drops one also clears its pixels.
        for departure in self.motion.departures() {
            invalidate(hwnd, departure.rect.intersect(self.layout.tab_zone));
        }
        let running = self.motion.step(now);
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
        self.completion =
            omnibox::inline_completion(&self.history, &self.edit_text, !self.suppress_completion);
        self.suppress_completion = false
    }
}
thread_local! {static WINDOWS:RefCell<HashMap<isize,WindowChrome>>=RefCell::new(HashMap::new());}

fn rebuild_rail_entries(state: &mut WindowChrome, app: &AppRef) {
    use crate::{sidebar::{SidebarConfig, SidebarKind}, workspaces::{WorkspaceColour, WorkspaceIcon}};

    let profile = crate::profile::active().ok();
    let config = profile.as_ref().map_or_else(SidebarConfig::default, |p| {
        SidebarConfig::load(p.root()).unwrap_or_default()
    });
    let owner = app.borrow().hwnd;
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
    let bookmark_active = profile.as_ref().zip(active_url.as_ref()).is_some_and(|(profile, url)| {
        crate::bookmarks::BookmarkStore::load(profile.root()).is_ok_and(|store| store.is_bookmarked(url.as_str()))
    });
    let shown = |kind| config.items.iter().find(|item| item.kind == kind).is_none_or(|item| item.visible);
    let mut entries = vec![
        RailEntryData { key: RailKey::Home, icon: RailIcon::Home, label: "Home".into(), active: false, accent: None, favicon: None, png: None },
        RailEntryData { key: RailKey::Customize, icon: RailIcon::Customize, label: "Customize".into(), active: false, accent: None, favicon: None, png: None },
    ];
    if shown(SidebarKind::Workspaces) {
        for (id, name, icon, colour, _, active) in App::workspace_entries(app) {
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
            entries.push(RailEntryData { key: RailKey::Workspace(id.0 as u64), icon: RailIcon::Glyph(glyph), label: name, active, accent, favicon: None, png: None });
        }
        entries.push(RailEntryData { key: RailKey::AddWorkspace, icon: RailIcon::Plus, label: "New workspace".into(), active: false, accent: None, favicon: None, png: None });
    }
    if shown(SidebarKind::Apps) {
        for app_entry in &config.apps {
            let bytes: Option<Vec<u8>> = profile.as_ref().and_then(|_| {
                let host = url::Url::parse(&app_entry.url).ok()?.host_str()?.to_owned();
                favicon::load_favicon_bytes(&host).ok()?
            });
            let favicon = bytes.as_deref().and_then(|bytes| {
                favicon::decode(bytes, Metrics::new(state.dpi).px(Metrics::FAVICON_SIZE).max(1) as u32).map(Rc::new)
            });
            entries.push(RailEntryData { key: RailKey::App(app_entry.id), icon: if favicon.is_some() { RailIcon::AppTile } else { RailIcon::Letter }, label: app_entry.name.clone(), active: false, accent: None, favicon, png: bytes.map(Rc::new) });
        }
        entries.push(RailEntryData { key: RailKey::AddApp, icon: RailIcon::Plus, label: "Add web app".into(), active: false, accent: None, favicon: None, png: None });
    }
    for item in &config.items {
        let (key, icon, label) = match item.kind {
            SidebarKind::Bookmarks if item.visible => (RailKey::Bookmarks, RailIcon::Heart, "Bookmarks"),
            SidebarKind::History if item.visible => (RailKey::History, RailIcon::Clock, "History"),
            _ => continue,
        };
        entries.push(RailEntryData { key, icon, label: label.into(), active: key == RailKey::Bookmarks && bookmark_active, accent: None, favicon: None, png: None });
    }
    entries.push(RailEntryData { key: RailKey::Menu, icon: RailIcon::Dots, label: "Menu".into(), active: false, accent: None, favicon: None, png: None });
    state.rail_entries = entries;
    state.rail_scroll = state.rail_scroll.max(0);
}

fn main_window_style() -> WINDOW_STYLE {
    WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_CLIPCHILDREN
}

fn with_chrome<T>(hwnd: HWND, f: impl FnOnce(&mut WindowChrome) -> T) -> Option<T> {
    WINDOWS.with(|a| a.borrow_mut().get_mut(&(hwnd.0 as isize)).map(f))
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
}
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
    read.is_err() || enabled.as_bool()
}
fn dpi(hwnd: HWND) -> u32 {
    unsafe { GetDpiForWindow(hwnd) }.max(96)
}
/// Gives the address EDIT the system message font for the window's current DPI, and releases
/// the font it had.
fn refresh_edit_font(hwnd: HWND, app: &AppRef) {
    let font = paint::create_message_font(dpi(hwnd), DEFAULT_QUALITY);
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
fn sync_foreground(hwnd: HWND, app: Option<&AppRef>) {
    let foreground = foreground_is_ours(hwnd);
    let minimised = unsafe { IsIconic(hwnd).as_bool() };
    set_caret_blink(
        hwnd,
        caret_blink_allowed(foreground, minimised, edit_focused(hwnd)),
    );
    let gained = with_chrome(hwnd, |s| {
        let gained = foreground && !s.foreground;
        s.foreground = foreground;
        gained
    })
    .unwrap_or(false);
    if !foreground {
        card::cancel(hwnd);
    }
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

pub fn run(app: App) -> windows::core::Result<()> {
    // Overlaps font realization with window creation; the first paint otherwise waits for it.
    let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() };
    let _ = std::thread::Builder::new()
        .name("text-prewarm".into())
        .spawn(move || paint::prewarm_text(dpi));
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let class = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
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
            "Bowser - Search or enter address{}",
            crate::profile::active()
                .map(|profile| profile.title_suffix())
                .unwrap_or_default()
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
        super::panel_bookmarks::register(&app);
        super::panel_history::register(&app);
        super::panel_customize::register(&app);
        let _ = glass::apply(hwnd);
        app.borrow_mut().hwnd = hwnd;
        super::panel_webapp::register(&app);
        super::panel_webapp::set_on_icon_changed(Box::new(move || {
            super::post(hwnd, super::WM_RAIL_CHANGED, WPARAM(0), LPARAM(0));
        }));
        MAIN_WINDOW.with(|m| m.set(hwnd.0 as isize));
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
        App::restore_pending_session(&app);
        sync_split_ratio(hwnd, App::split_view(&app));
        if App::split_view(&app).is_some()
            && let Some(panes) = split_rects(hwnd, None)
        {
            App::update_split_bounds(&app, panes);
        }
        App::submit_initial_url(&app);
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
                let ih = matches!(
                    hit(hwnd, p.x, p.y),
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
                );
                frame::frame_hit_test(
                    p,
                    (r.right, r.bottom),
                    dpi(hwnd),
                    IsZoomed(hwnd).as_bool(),
                    if ih {
                        frame::ChromeHit::Interactive
                    } else {
                        frame::ChromeHit::None
                    },
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
            WM_SETTINGCHANGE | WM_DWMCOLORIZATIONCOLORCHANGED | WM_THEMECHANGED => {
                glass::refresh_system_state();
                let _ = glass::apply(hwnd);
                let settled = with_chrome(hwnd, |s| {
                    s.palette = glass::palette();
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
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_CREATE => {
                if let Some(app) = app {
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
                    s.refresh(app, true);
                    rebuild_rail_entries(&mut s, app);
                    WINDOWS.with(|all| {
                        all.borrow_mut().insert(hwnd.0 as isize, s);
                    });
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
                LRESULT(0)
            }
            WM_SIZE => {
                card::cancel(hwnd);
                // --- popup host (lane M) ---
                html_popup::close(hwnd);
                // --- end popup host ---
                if let Some(app) = app {
                    let min = wparam.0 as u32 == SIZE_MINIMIZED;
                    let mut r = RECT::default();
                    let _ = GetClientRect(hwnd, &mut r);
                    apply_window_region(hwnd, r.right, r.bottom, IsZoomed(hwnd).as_bool());
                    let active = App::active_tab_id(app);
                    let _ = with_chrome(hwnd, |s| {
                        s.relayout(
                            r.right,
                            r.bottom,
                            dpi(hwnd),
                            active,
                            IsZoomed(hwnd).as_bool(),
                            App::tab_layout(app),
                            App::sidebar_width(app),
                        );
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
                        let _ = InvalidateRect(Some(hwnd), None, false);
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
                        s.relayout(
                            r.right,
                            r.bottom,
                            dpi(hwnd),
                            App::active_tab_id(app),
                            IsZoomed(hwnd).as_bool(),
                            App::tab_layout(app),
                            App::sidebar_width(app),
                        );
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
                    let _ = InvalidateRect(Some(hwnd), None, false);
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
                    remember_url(hwnd, App::active_tab_id(app), &text);
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
                        if let Some(target) =
                            App::tab_progress_percent(app, App::active_tab_id(app))
                        {
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
                        } else {
                            with_chrome(hwnd, |s| s.progress.clear());
                            let _ = KillTimer(Some(hwnd), WM_PROGRESS_TIMER);
                            invalidate_omni(hwnd);
                        }
                    }
                }
                LRESULT(0)
            }
            WM_PANE_FOCUSED => {
                if let Some(app) = app
                    && App::focus_split_tab(app, TabId(wparam.0 as u32))
                {
                    let _ = InvalidateRect(Some(hwnd), None, false);
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
                    let active = App::active_tab_id(app);
                    // --- popup host (lane M) ---
                    // A tab or workspace switch dismisses an open HTML menu.
                    if let Some(context) = popup_context(hwnd) { html_popup::note_context(hwnd, context); }
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
                    }
                    card::sync(hwnd, app, |id| tab_anchor(hwnd, id));
                    island_menu::sync(hwnd, app, |id| ball_anchor(hwnd, id));
                    if structural {
                        position_edit(hwnd);
                        let _ = InvalidateRect(Some(hwnd), None, false);
                        arm_motion(hwnd);
                    } else if let Some(((toolbar, tabs), _)) = refreshed {
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
                    let _ = with_chrome(hwnd, |s| rebuild_rail_entries(s, app));
                    let _ = InvalidateRect(Some(hwnd), None, false);
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
                        && shortcuts::command_for(key, ctrl, shift, alt)
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
                    let geometry = crate::chrome::rail::geometry(s.layout.rail, &entries, s.rail_scroll, s.dpi);
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
                        if let Some(app) = app {
                            let width = ((p.x * 96) / dpi(hwnd).max(96) as i32).clamp(
                                i32::from(crate::settings::MIN_SIDEBAR_WIDTH),
                                i32::from(crate::settings::MAX_SIDEBAR_WIDTH),
                            );
                            App::set_sidebar_width(app, width as u16);
                            let _ = with_chrome(hwnd, |s| s.refresh(app, true));
                            sync_split_ratio(hwnd, App::split_view(app));
                            apply_sidebar_bounds(hwnd, app);
                        }
                    } else {
                        let dragging = with_chrome(hwnd, |s| s.split_dragging).unwrap_or(false);
                        if dragging
                            && let Some(panes) = split_rects(hwnd, Some(p.x))
                            && let Some(app) = app
                        {
                            let divider = panes.0.right;
                            let _ = with_chrome(hwnd, |s| s.split_divider = divider);
                            if let Some(viewport) = with_chrome(hwnd, |s| s.layout.viewport) {
                                let viewport = adjusted_viewport(hwnd, to_win_rect(viewport), dpi(hwnd));
                                let ratio = (((divider + 3 - viewport.left) * 100)
                                    / (viewport.right - viewport.left).max(1))
                                .clamp(20, 80) as u8;
                                App::set_split_ratio(app, ratio);
                            }
                            queue_split_bounds(hwnd, panes);
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        } else {
                            let point = frame::point_from_lparam(lparam);
                            let target = with_chrome(hwnd, |s| {
                                let dragging_tab = s.drag.is_some();
                                let viewport = adjusted_viewport(hwnd, to_win_rect(s.layout.viewport), s.dpi);
                                let viewport = Rect::new(
                                    viewport.left,
                                    viewport.top,
                                    viewport.right,
                                    viewport.bottom,
                                );
                                (dragging_tab && viewport.contains(point.x, point.y)).then_some(
                                    if point.x < viewport.left + viewport.width() / 2 {
                                        crate::split_view::Pane::Left
                                    } else {
                                        crate::split_view::Pane::Right
                                    },
                                )
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
                        let cursor = LoadCursorW(None, IDC_SIZEWE);
                        if let Ok(cursor) = cursor {
                            let _ = SetCursor(Some(cursor));
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
                if in_rail {
                    let delta = -((wparam.0 >> 16) as i16 as i32).signum()
                        * Metrics::new(dpi(hwnd)).px(36);
                    let changed = with_chrome(hwnd, |s| {
                        let entries = s.rail_entries.iter().map(|entry| RailEntry {
                            key: entry.key, icon: entry.icon, label: entry.label.clone(), active: entry.active,
                            dot: false, accent: entry.accent, favicon: entry.favicon.as_deref(),
                        }).collect::<Vec<_>>();
                        let max = crate::chrome::rail::geometry(s.layout.rail, &entries, s.rail_scroll, s.dpi).max_scroll;
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
                        let row_height = crate::chrome::layout::Metrics::new(s.dpi)
                            .px(crate::vertical_tabs::TAB_ROW_HEIGHT)
                            .max(1);
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
                if let Some(app) = app {
                    App::save_settings(app);
                }
                if clear_pointer_state(hwnd) {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_CANCELMODE => {
                hide_rail_tooltip(hwnd);
                let had_pointer_state = clear_pointer_state(hwnd);
                if had_pointer_state {
                    if GetCapture() == hwnd {
                        let _ = ReleaseCapture();
                    }
                    let _ = InvalidateRect(Some(hwnd), None, false);
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
                                let _ = InvalidateRect(Some(hwnd), None, false);
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
                        if let Some(app) = app {
                            show_workspaces_menu(hwnd, app);
                        }
                    }
                    Hit::SidebarNewTab => {
                        if let Some(app) = app {
                            App::new_tab(app);
                            focus_omni(hwnd, app, true);
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
                                let _ = InvalidateRect(Some(hwnd), None, false);
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
                            App::new_tab(app);
                            focus_omni(hwnd, app, true)
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
                        let _ = with_chrome(hwnd, |s| s.split_dragging = true);
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
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        }
                    }
                    Hit::Viewport => {
                        if let Some(app) = app
                            && App::split_view(app).is_some()
                            && let Some((left, _right)) = split_rects(hwnd, None)
                        {
                            let pane = if p.x < left.right {
                                crate::split_view::Pane::Left
                            } else {
                                crate::split_view::Pane::Right
                            };
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
                    let _ = InvalidateRect(Some(hwnd), None, false);
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
                        App::save_settings(app);
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
                        App::split_state_changed(app);
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
                        if let Some(pane) = drop {
                            let other = companion
                                .filter(|tab| *tab != id)
                                .or_else(|| App::tab_ids(app).into_iter().find(|tab| *tab != id));
                            if let Some(other) = other
                                && let Some(bounds) = split_rects(hwnd, None)
                            {
                                let (left, right) = match pane {
                                    crate::split_view::Pane::Left => (id, other),
                                    crate::split_view::Pane::Right => (other, id),
                                };
                                let _ = App::open_split_view(app, left, right, pane, bounds);
                                sync_split_ratio(hwnd, App::split_view(app));
                                if let Some(panes) = split_rects(hwnd, None) {
                                    App::update_split_bounds(app, panes);
                                }
                            }
                        } else {
                            let to = layout::reorder_target(&l, from, p.x);
                            let model_to = slot_model_index(hwnd, to).unwrap_or(to);
                            let _ = App::drop_tab(app, id, model_to);
                        }
                    }
                    let _ = InvalidateRect(Some(hwnd), None, false);
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
                                    let _ = App::close_tab(app, id);
                                }
                            }
                        }
                        if let Hit::SidebarTabClose(i) = was {
                            if let Some(app) = app {
                                if let Some(id) = sidebar_tab_id(hwnd, i) {
                                    let _ = App::close_tab(app, id);
                                }
                            }
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
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        }
                        if was == Hit::Split
                            && let Some(app) = app
                        {
                            show_split_menu(hwnd, app);
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
                            App::set_split_ratio(app, 50);
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
                    _ => None,
                };
                if let Some(id) = target {
                    card::cancel(hwnd);
                    if let Some(app) = app {
                        let _ = App::close_tab(app, id);
                    }
                }
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                hide_rail_tooltip(hwnd);
                let p = frame::point_from_lparam(lparam);
                if let Some(app) = app {
                    match hit(hwnd, p.x, p.y) {
                        Hit::Rail(key) => show_rail_context_menu(hwnd, app, key, p.x, p.y),
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
                        Hit::SidebarTab(i) | Hit::SidebarTabClose(i) => {
                            if let Some(id) = sidebar_tab_id(hwnd, i) {
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
                            let geometry = crate::chrome::rail::geometry(s.layout.rail, &entries, s.rail_scroll, s.dpi);
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
                            std::mem::replace(&mut s.rail_previous_focus, HWND::default())
                        }).unwrap_or_default();
                        if !previous.0.is_null() { let _ = SetFocus(Some(previous)); }
                        let _ = InvalidateRect(Some(hwnd), None, false);
                        return LRESULT(0);
                    }
                }
                if GetKeyState(VK_CONTROL.0 as i32) < 0 && GetKeyState(VK_SHIFT.0 as i32) >= 0 {
                    if let Some(app) = app {
                        match wparam.0 as u32 {
                            0x42 => { rail_activate(hwnd, app, side_panel::PanelId::Bookmarks); return LRESULT(0); }
                            0x48 => { rail_activate(hwnd, app, side_panel::PanelId::History); return LRESULT(0); }
                            _ => {}
                        }
                    }
                }
                if wparam.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE.0 as usize
                    && side_panel::is_open(hwnd)
                    && !crate::win32::menu::is_tracking()
                {
                    side_panel::close(hwnd);
                    if let Some(app) = app { apply_sidebar_bounds(hwnd, app); }
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
                        let _ = key.map(|key| crate::chrome::rail::rail_tooltip(key, &entries));
                        s.rail_tooltip_visible = true;
                        true
                    } else { false }
                }).unwrap_or(false);
                if shown { with_chrome(hwnd, |s| invalidate(hwnd, s.layout.rail)); }
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
                            entries: vec![
                                Entry::Item(Item::new(1, "Import", Icon::Grid).glyph("download")),
                                Entry::Item(Item::new(2, "Not now", Icon::Close).glyph("clock")),
                                Entry::Item(Item::new(3, "Don't ask again", Icon::Close).glyph("close")),
                            ],
                            footer: "Import bookmarks and supported search settings from another browser. You can also import later from the main menu.".into(),
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
                if let Some(app) = app { apply_sidebar_bounds(hwnd, app); }
                if !running { let _ = KillTimer(Some(hwnd), side_panel::TIMER_ID); }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == WM_PROGRESS_TIMER => {
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
                let _ = KillTimer(Some(hwnd), crate::win32::WM_PRIVACY_STATS_TIMER);
                let _ = KillTimer(Some(hwnd), super::WM_IMPORT_OFFER_TIMER);
                card::cancel(hwnd);
                // The chrome hosts live in the engine's environment: close them first.
                close_html_chrome(hwnd);
                if let Some(app) = app {
                    App::release_chrome_environment(app);
                    let _ = App::privacy_stats_save_now(app);
                    App::session_finish(app);
                    let (mut engine, log) = {
                        let mut s = app.borrow_mut();
                        s.closing = true;
                        (s.engine.take(), s.log.clone())
                    };
                    let _ = App::reset_navigation_progress(app);
                    if let Some(e) = engine.as_mut() {
                        let _ = e.close();
                    }
                    let _ = log.record("shutdown");
                }
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                super::panel_webapp::shutdown();
                passwords_panel::clear_password_clipboard_timer(hwnd);
                let _ = KillTimer(Some(hwnd), WM_IDLE_TRIM_TIMER);
                let _ = KillTimer(Some(hwnd), WM_CARET_BLINK_TIMER);
                let _ = KillTimer(Some(hwnd), WM_ANIMATION_TIMER);
                let _ = KillTimer(Some(hwnd), WM_PROGRESS_TIMER);
                let _ = KillTimer(Some(hwnd), WM_SPLIT_BOUNDS_TIMER);
                let _ = KillTimer(Some(hwnd), super::WM_IMPORT_OFFER_TIMER);
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
            let entries = s.rail_entries.iter().map(|entry| RailEntry {
                key: entry.key, icon: entry.icon, label: entry.label.clone(), active: entry.active,
                dot: false, accent: entry.accent, favicon: entry.favicon.as_deref(),
            }).collect::<Vec<_>>();
            let rail = RailView { entries: &entries, fades: &[], open_panel: None, scroll: s.rail_scroll, drop_indicator: None, focused: None, tooltip: None };
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
        if hit == Hit::Viewport
            && s.split_active
            && layout::split_pane_layout(
                {
                    let viewport = adjusted_viewport(hwnd, to_win_rect(s.layout.viewport), s.dpi);
                    Rect::new(viewport.left, viewport.top, viewport.right, viewport.bottom)
                },
                s.split_divider,
                s.layout.metrics.px(220),
                s.layout.metrics.px(6),
            )
            .is_some_and(|panes| panes.divider.contains(x, y))
        {
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
/// Favicon of a tab as the chrome last decoded it, for the island menu's rows.
pub(crate) fn tab_favicon(hwnd: HWND, id: TabId) -> Option<Rc<crate::favicon::FaviconBitmap>> {
    with_chrome(hwnd, |s| {
        s.tabs
            .iter()
            .find(|tab| tab.id == id)
            .and_then(|tab| tab.favicon.clone())
    })
    .flatten()
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

fn split_rects(hwnd: HWND, divider_left: Option<i32>) -> Option<(RECT, RECT)> {
    let (layout, saved_divider) = with_chrome(hwnd, |state| (state.layout, state.split_divider))?;
    let divider = divider_left.unwrap_or(saved_divider);
    let viewport = adjusted_viewport(hwnd, to_win_rect(layout.viewport), dpi(hwnd));
    let viewport = Rect::new(viewport.left, viewport.top, viewport.right, viewport.bottom);
    let panes = layout::split_pane_layout(
        viewport,
        divider,
        layout.metrics.px(220),
        layout.metrics.px(6),
    )?;
    Some((
        RECT {
            left: panes.left.left,
            top: panes.left.top,
            right: panes.left.right,
            bottom: panes.left.bottom,
        },
        RECT {
            left: panes.right.left,
            top: panes.right.top,
            right: panes.right.right,
            bottom: panes.right.bottom,
        },
    ))
}

fn sync_split_ratio(hwnd: HWND, split: Option<crate::split_view::SplitView>) {
    let Some(split) = split else { return };
    let _ = with_chrome(hwnd, |state| {
        let viewport = adjusted_viewport(hwnd, to_win_rect(state.layout.viewport), state.dpi);
        state.split_divider = viewport.left
            + (viewport.right - viewport.left) * i32::from(split.ratio) / 100
            - state.layout.metrics.px(3);
    });
}

fn queue_split_bounds(hwnd: HWND, bounds: (RECT, RECT)) {
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
    side_panel::adjust_viewport(viewport, hwnd)
}

fn apply_sidebar_bounds(hwnd: HWND, app: &AppRef) {
    let Some(layout) = with_chrome(hwnd, |state| state.layout) else {
        return;
    };
    sync_split_ratio(hwnd, App::split_view(app));
    let viewport = to_win_rect(layout.viewport);
    notes_panel::sync(hwnd, viewport, dpi(hwnd));
    side_panel::sync(hwnd, viewport, dpi(hwnd));
    if let Some(panes) = split_rects(hwnd, None) {
        App::update_split_bounds(app, panes);
    } else {
        let bounds = adjusted_viewport(hwnd, viewport, dpi(hwnd));
        App::window_size_changed(app, false, bounds);
    }
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
}

/// Rail handlers call this with the semantic panel ID; the host stays alive when switching IDs.
pub fn rail_activate(hwnd: HWND, app: &AppRef, key: side_panel::PanelId) -> bool {
    let opened = side_panel::toggle(hwnd, key);
    if opened { apply_sidebar_bounds(hwnd, app); }
    if !side_panel::is_open(hwnd) {
        if let Some(message) = side_panel::last_status() { show_error_message(message); return false; }
    }
    true
}

fn activate_rail(hwnd: HWND, app: &AppRef, key: RailKey) {
    match key {
        RailKey::Home => { App::new_tab(app); focus_omni(hwnd, app, true); }
        RailKey::Customize | RailKey::Menu | RailKey::AddApp => { rail_activate(hwnd, app, side_panel::PanelId::Customize); }
        RailKey::Workspace(id) => { let _ = App::switch_workspace(app, crate::workspaces::WorkspaceId(id as u32)); }
        RailKey::AddWorkspace => {
            let count = App::workspace_entries(app).len();
            let name = format!("Workspace {}", count + 1);
            if let Ok(id) = App::workspace_create(app, &name) {
                if let Some(renamed) = super::profile_rename::prompt(hwnd, &name, "New workspace", "Create") {
                    let _ = App::rename_workspace(app, id, &renamed);
                }
            }
        }
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
        RailKey::Bookmarks => { rail_activate(hwnd, app, side_panel::PanelId::Bookmarks); }
        RailKey::History => { rail_activate(hwnd, app, side_panel::PanelId::History); }
    }
    let _ = with_chrome(hwnd, |state| rebuild_rail_entries(state, app));
    unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
}

fn rail_entries(state: &WindowChrome) -> Vec<RailEntry<'_>> {
    state.rail_entries.iter().map(|entry| RailEntry {
        key: entry.key,
        icon: entry.icon,
        label: entry.label.clone(),
        active: entry.active,
        dot: false,
        accent: entry.accent,
        favicon: entry.favicon.as_deref(),
    }).collect()
}

fn rail_drop_target(hwnd: HWND, key: RailKey, x: i32, y: i32) -> Option<(usize, RailDropIndicator)> {
    with_chrome(hwnd, |state| {
        let entries = rail_entries(state);
        let geometry = crate::chrome::rail::geometry(state.layout.rail, &entries, state.rail_scroll, state.dpi);
        crate::chrome::rail::drop_indicator(&geometry, key, x, y)
    }).flatten()
}

fn edit_rail_app(hwnd: HWND, app: &AppRef, id: u32) {
    super::panel_customize::focus_app(id);
    if side_panel::open(hwnd, side_panel::PanelId::Customize) {
        apply_sidebar_bounds(hwnd, app);
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

fn hide_rail_tooltip(hwnd: HWND) {
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

fn show_rail_context_menu(hwnd: HWND, app: &AppRef, key: RailKey, x: i32, y: i32) {
    let point = screen_point(hwnd, windows::Win32::Foundation::POINT { x, y });
    match key {
        RailKey::Workspace(raw_id) => {
            let id = crate::workspaces::WorkspaceId(raw_id as u32);
            let set = App::workspaces(app);
            let entries = App::workspace_entries(app);
            let Some((_, name, _, _, _, _)) = entries.iter().find(|(entry_id, ..)| *entry_id == id) else { return; };
            let name = name.clone();
            let menu_entries = vec![
                Entry::Item(Item::new(1, "Rename…", Icon::Person).glyph("edit")),
                Entry::Item(Item::new(2, "Change icon…", Icon::Grid).glyph("workspace")),
                Entry::Item(Item::new(3, "Change colour…", Icon::Grid).glyph("droplet")),
                Entry::Item(Item::new(4, "Move up", Icon::Grid).glyph("chevron-up")),
                Entry::Item(Item::new(5, "Move down", Icon::Grid).glyph("chevron-down")),
                Entry::Item(Item::new(6, "Delete…", Icon::Close).glyph("trash").disabled_if(set.workspace_count() == 1)),
            ];
            let kind = format!("rail-ws:{raw_id}");
            menu::show(hwnd, point, menu::Placement::AboveLeft, Menu { header: None, entries: menu_entries, footer: name.clone() }, &kind, move |app, chosen| {
                let Some(chosen) = chosen else { return; };
                // The workspace may have been deleted while the menu was open.
                let set = App::workspaces(app);
                if !set.all().iter().any(|workspace| workspace.id() == id) { return; }
                match chosen.id {
                    1 => if let Some(value) = super::profile_rename::prompt(hwnd, &name, "Rename workspace", "Save") { let _ = App::rename_workspace(app, id, &value); },
                    2 => {
                        let choices = ["Briefcase", "Home", "Code", "Book", "Shopping", "Game", "Music", "Heart", "Star", "Bolt", "Globe", "Leaf"];
                        let entries = choices.iter().enumerate().map(|(i, label)| Entry::Item(Item::new(20 + i, *label, Icon::Grid).glyph(WORKSPACE_GLYPH_NAMES[i]))).collect();
                        let icons = [crate::workspaces::WorkspaceIcon::Briefcase, crate::workspaces::WorkspaceIcon::Home, crate::workspaces::WorkspaceIcon::Code, crate::workspaces::WorkspaceIcon::Book, crate::workspaces::WorkspaceIcon::Cart, crate::workspaces::WorkspaceIcon::Game, crate::workspaces::WorkspaceIcon::Music, crate::workspaces::WorkspaceIcon::Heart, crate::workspaces::WorkspaceIcon::Star, crate::workspaces::WorkspaceIcon::Bolt, crate::workspaces::WorkspaceIcon::Globe, crate::workspaces::WorkspaceIcon::Leaf];
                        menu::show(hwnd, point, menu::Placement::AboveLeft, Menu { header: None, entries, footer: "Workspace icon".into() }, "rail-ws-icon", move |app, chosen| {
                            if let Some(icon) = chosen.and_then(|item| icons.get(item.id.saturating_sub(20)).copied()) { let _ = App::workspace_set_icon(app, id, icon); }
                        });
                    }
                    3 => {
                        let choices = ["Blue", "Violet", "Teal", "Amber", "Rose", "Red", "Green", "Slate"];
                        let entries = choices.iter().enumerate().map(|(i, label)| Entry::Item(Item::new(40 + i, *label, Icon::Grid).profile_badge("", crate::theme::Rgba::hex(WORKSPACE_SWATCHES[i], 0xFF)))).collect();
                        let colours = [crate::workspaces::WorkspaceColour::Blue, crate::workspaces::WorkspaceColour::Violet, crate::workspaces::WorkspaceColour::Teal, crate::workspaces::WorkspaceColour::Amber, crate::workspaces::WorkspaceColour::Rose, crate::workspaces::WorkspaceColour::Red, crate::workspaces::WorkspaceColour::Green, crate::workspaces::WorkspaceColour::Slate];
                        menu::show(hwnd, point, menu::Placement::AboveLeft, Menu { header: None, entries, footer: "Workspace colour".into() }, "rail-ws-colour", move |app, chosen| {
                            if let Some(colour) = chosen.and_then(|item| colours.get(item.id.saturating_sub(40)).copied()) { let _ = App::workspace_set_colour(app, id, Some(colour)); }
                        });
                    }
                    4 => if let Some(index) = set.all().iter().position(|workspace| workspace.id() == id) { let _ = App::workspace_move(app, id, index.saturating_sub(1)); },
                    5 => if let Some(index) = set.all().iter().position(|workspace| workspace.id() == id) { let _ = App::workspace_move(app, id, index + 1); },
                    // --- popup surfaces (lane PP) --- confirm in the HTML popup, not a MessageBox
                    6 => if set.workspace_count() > 1 { confirm_rail_workspace_delete(hwnd, point, id, &name); },
                    _ => {}
                }
            });
        }
        RailKey::App(id) => {
            let entries = vec![Entry::Item(Item::new(1, "Open", Icon::Grid).glyph("app-tile")), Entry::Item(Item::new(2, "Edit…", Icon::Person).glyph("edit")), Entry::Item(Item::new(3, "Remove…", Icon::Close).glyph("trash"))];
            let kind = format!("rail-app:{id}");
            menu::show(hwnd, point, menu::Placement::AboveLeft, Menu { header: None, entries, footer: "Web app".into() }, &kind, move |app, chosen| {
                let Some(chosen) = chosen else { return; };
                // The app may have been removed while the menu was open.
                if !rail_model_keys(hwnd).contains(&key) { return; }
                match chosen.id {
                    1 => activate_rail(hwnd, app, key),
                    2 => edit_rail_app(hwnd, app, id),
                    3 if crate::win32::ask_yes_no("Remove this web app from the rail?") => {
                        if let Ok(profile) = crate::profile::active() {
                            let mut config = crate::sidebar::SidebarConfig::load(profile.root()).unwrap_or_default();
                            config.remove_app(id);
                            let _ = config.save(profile.root());
                            super::panel_webapp::forget_app(id);
                            let _ = with_chrome(hwnd, |state| rebuild_rail_entries(state, app));
                            unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
                        }
                    }
                    _ => {}
                }
            });
        }
        _ => {}
    }
}
// --- end popup host ---

fn split_tab_rows(app: &AppRef, skip: Option<TabId>) -> Vec<Entry> {
    App::tab_ids(app)
        .into_iter()
        .filter(|id| Some(*id) != skip)
        .map(|id| {
            let title = App::tab_title(app, id)
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| "New tab".to_owned());
            let favicon =
                App::tab_favicon(app, id).and_then(|bytes| crate::favicon::decode(&bytes, 24));
            Entry::Item(
                Item::new(id.0 as usize + 100, title.clone(), Icon::Grid)
                    .glyph("workspace-globe")
                    .favicon_png(
                        App::tab_favicon(app, id)
                            .filter(|bytes| bytes.len() <= chrome_host::protocol::MAX_FAVICON_BYTES)
                            .map(std::rc::Rc::new),
                    )
                    .rich_row(title, favicon),
            )
        })
        .collect()
}

// --- popup host (lane M): async split menu ---
fn show_split_menu(hwnd: HWND, app: &AppRef) {
    const OPEN_BASE: usize = 100;
    const SWAP: usize = 1;
    const CLOSE_LEFT: usize = 2;
    const CLOSE_RIGHT: usize = 3;
    const REPLACE: usize = 4;
    const OPEN_NEW: usize = 5;
    // Submenu parents need ids of their own: the HTML menu rejects duplicate ids.
    const OPEN_WITH: usize = 6;
    let split = App::split_view(app);
    let entries = if let Some(split) = split {
        vec![
            Entry::Item(Item::new(SWAP, "Swap panes", Icon::Grid).glyph("reload")),
            Entry::Item(Item::new(CLOSE_LEFT, "Close left pane", Icon::Close).glyph("back")),
            Entry::Item(Item::new(CLOSE_RIGHT, "Close right pane", Icon::Close).glyph("forward")),
            Entry::Item(
                Item::new(REPLACE, "Replace pane with tab…", Icon::Grid)
                    .glyph("split")
                    .submenu_with(split_tab_rows(app, Some(split.other_tab()))),
            ),
        ]
    } else {
        let active = App::active_tab_id(app);
        let mut entries = vec![Entry::Item(Item::new(
            OPEN_NEW,
            "Open split view with a new tab",
            Icon::Plus,
        )
        .glyph("plus"))];
        entries.push(Entry::Separator);
        entries.push(Entry::Item(
            Item::new(OPEN_WITH, "Open split view with…", Icon::Grid)
                .glyph("split")
                .submenu_with(split_tab_rows(app, Some(active))),
        ));
        entries
    };
    let point = screen_point(hwnd, main_menu_anchor(hwnd));
    let was_split = split.is_some();
    menu::show(
        hwnd,
        point,
        menu::Placement::AboveLeft,
        Menu {
            header: None,
            entries,
            footer: "Split view".to_owned(),
        },
        "split",
        move |app, chosen| {
            let command = chosen.map_or(0, |chosen| chosen.id);
            // The split may have opened or closed, or the picked tab gone, while the menu was open.
            let picked_tab = (command >= OPEN_BASE)
                .then(|| TabId((command - OPEN_BASE) as u32))
                .filter(|tab| App::tab_ids(app).contains(tab));
            if command >= OPEN_BASE && picked_tab.is_none() {
                return;
            }
            if App::split_view(app).is_some() != was_split {
                return;
            }
            if was_split {
                match command {
                    SWAP => {
                        let _ = App::swap_split_panes(app);
                    }
                    CLOSE_LEFT => {
                        let _ = App::close_split_pane(app, crate::split_view::Pane::Left);
                    }
                    CLOSE_RIGHT => {
                        let _ = App::close_split_pane(app, crate::split_view::Pane::Right);
                    }
                    _ => {
                        if let Some(tab) = picked_tab {
                            let _ = App::replace_focused_split_pane(app, tab);
                        }
                    }
                }
            } else {
                let active = App::active_tab_id(app);
                if command == OPEN_NEW {
                    let new = App::new_tab(app);
                    if let Some(bounds) = split_rects(hwnd, None) {
                        let _ = App::open_split_view(
                            app,
                            active,
                            new,
                            crate::split_view::Pane::Right,
                            bounds,
                        );
                    }
                } else if let Some(other) = picked_tab.filter(|other| *other != active) {
                    if let Some(bounds) = split_rects(hwnd, None) {
                        let _ = App::open_split_view(
                            app,
                            active,
                            other,
                            crate::split_view::Pane::Right,
                            bounds,
                        );
                    }
                }
            }
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        },
    );
}
// --- end popup host ---

fn show_tab_context_menu(hwnd: HWND, app: &AppRef, id: TabId, x: i32, y: i32, pip_available: bool) {
    const NEW_RIGHT: usize = 1;
    const TAB_SPLIT_VIEW_HOOK: usize = 2;
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
    let pinned = App::tab_pinned(app, id);
    let url = App::tab_committed_url(app, id);
    let key = url.as_ref().and_then(tab_groups::DomainKey::from_url);
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
        Entry::Item(
            Item::new(
                TAB_SPLIT_VIEW_HOOK,
                "New split view with current tab",
                Icon::Grid,
            )
            .glyph("split")
            .disabled_if(App::tab_count(app) < 2 || App::split_view(app).is_some()),
        ),
        Entry::Separator,
    ];
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
            .disabled_if(!has_group),
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
        Entry::Item(Item::new(DUPLICATE, "Duplicate tab", Icon::Plus).glyph("copy").disabled_if(url.is_none())),
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
            .glyph("workspace-globe")
            .disabled_if(!can_mute_site),
        ),
        Entry::Item(Item::new(HIBERNATE, "Hibernate tab", Icon::None).glyph("clock").disabled_if(!can_hibernate)),
        Entry::Item(
            Item::new(MOVE_WINDOW, "Move tab to new window", Icon::None).glyph("external").disabled_if(url.is_none()),
        ),
    ]);
    if pip_available {
        entries.push(Entry::Item(Item::new(
            PICTURE_IN_PICTURE,
            "Picture in picture",
            Icon::None,
        )
        .glyph("pip")));
    }
    entries.push(Entry::Separator);
    entries.extend([
        Entry::Item(Item::new(CLOSE, "Close tab", Icon::Close).glyph("close")),
        Entry::Item(
            Item::new(CLOSE_OTHERS, "Close other tabs", Icon::Close).glyph("trash").disabled_if(ids.len() <= 1),
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
    // --- popup host (lane M) ---
    let point = screen_point(hwnd, windows::Win32::Foundation::POINT { x, y });
    let kind = format!("tab:{}", id.0);
    let menu = Menu {
        header: None,
        entries,
        footer: "Actions for this tab".to_owned(),
    };
    menu::show(hwnd, point, menu::Placement::BelowLeft, menu, &kind, move |app, chosen| unsafe {
        let command = chosen.map_or(0, |chosen| chosen.id);
        // The tab may have closed while the menu was open.
        if command == 0 || !App::tab_ids(app).contains(&id) {
            return;
        }
        match command {
            PICTURE_IN_PICTURE => App::toggle_picture_in_picture(app, id),
            NEW_RIGHT => {
                let created = App::new_tab(app);
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
            TAB_SPLIT_VIEW_HOOK => {
                let active = App::active_tab_id(app);
                let companion = if id != active {
                    Some(active)
                } else {
                    App::tab_ids(app)
                        .into_iter()
                        .find(|candidate| *candidate != id)
                };
                if let (Some(companion), Some(bounds)) = (companion, split_rects(hwnd, None)) {
                    let _ = App::open_split_view(
                        app,
                        id,
                        companion,
                        crate::split_view::Pane::Left,
                        bounds,
                    );
                }
            }
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
                    let after = current
                        .iter()
                        .position(|candidate| *candidate == id)
                        .unwrap_or(index)
                        + 1;
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
            CLOSE => {
                let _ = App::close_tab(app, id);
            }
            CLOSE_OTHERS => {
                for other in ids.iter().copied().filter(|other| *other != id) {
                    let _ = App::close_tab(app, other);
                }
            }
            CLOSE_LEFT => {
                for left in ids[..index].iter().copied() {
                    let _ = App::close_tab(app, left);
                }
            }
            CLOSE_RIGHT => {
                for right in ids[index + 1..].iter().copied() {
                    let _ = App::close_tab(app, right);
                }
            }
            _ => {}
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
    });
    // --- end popup host ---
}

/// Ctrl+Shift+P: native menu to switch, create or delete profiles. Switching starts a new Bowser
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
    menu::show(hwnd, point, menu::Placement::AboveLeft, confirm, "workspace-delete", move |app, chosen| {
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
    let mut entries = workspaces
        .all()
        .iter()
        .enumerate()
        .map(|(index, workspace)| {
            Entry::Item(
                Item::new(WORKSPACE_SELECT_BASE + index, workspace.name(), Icon::Grid)
                    .glyph(workspace_glyph_name(workspace.icon()))
                    .checked(workspace.id() == active),
            )
        })
        .collect::<Vec<_>>();
    entries.push(Entry::Separator);
    entries.push(Entry::Item(Item::new(
        WORKSPACE_CREATE,
        "New workspace…",
        Icon::Plus,
    )
    .glyph("plus")));
    entries.push(Entry::Item(Item::new(
        WORKSPACE_RENAME,
        "Rename current workspace…",
        Icon::Person,
    )
    .glyph("edit")));
    entries.push(Entry::Item(
        Item::new(WORKSPACE_DELETE, "Delete current workspace…", Icon::Close)
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
            run_workspace_action(hwnd, app, chosen.id, &built, active);
        }
    });
}

/// Runs a workspace menu command. `built` and `active_at_open` come from
/// `workspace_menu_snapshot` when the menu was built; a workspace that has gone since is ignored.
fn run_workspace_action(
    hwnd: HWND,
    app: &AppRef,
    command: usize,
    built: &[crate::workspaces::WorkspaceId],
    active_at_open: crate::workspaces::WorkspaceId,
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
        WORKSPACE_CREATE => {
            if let Some(name) = super::profile_rename::prompt(hwnd, "", "New workspace", "Create") {
                if let Err(error) = App::create_workspace(app, &name) {
                    show_profile_notice(
                        hwnd,
                        &format!("Workspace could not be created: {error:?}"),
                    );
                }
            }
        }
        WORKSPACE_RENAME => {
            if let Some(name) =
                super::profile_rename::prompt(hwnd, active.name(), "Rename workspace", "Save")
                && let Err(error) = App::rename_workspace(app, active.id(), &name)
            {
                show_profile_notice(hwnd, &format!("Workspace could not be renamed: {error:?}"));
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
    const IMPORT: usize = 5008;
    const PRIVACY: usize = 5009;
    const NOTES: usize = 5010;
    const PALETTE: usize = 5011;
    const BOOKMARK: usize = 5012;
    const SESSIONS: usize = 5013;
    const TAB_LAYOUT: usize = 5014;
    const WORKSPACES: usize = 5015;

    let profiles = ProfileMenu::load(hwnd);
    let mut entries = vec![
        Entry::Item(Item::new(NEW_TAB, "New tab", Icon::Plus).glyph("plus").shortcut("Ctrl+T")),
        Entry::Separator,
    ];
    if let Some(profiles) = &profiles {
        let name = ProfileMenu::label(profiles.current.listed_name()).to_string();
        entries.push(Entry::Item(
            Item::new(PROFILE, format!("Profile: {name}"), Icon::Person)
                .glyph("person")
                .submenu_with(profiles.entries()),
        ));
    }
    entries.push(Entry::Item(
        Item::new(NOTES, "Notes & Tasks…", Icon::Info).glyph("notes").shortcut("Ctrl+Shift+N"),
    ));
    entries.push(Entry::Item(
        Item::new(PALETTE, "Command palette…", Icon::Grid).glyph("command").shortcut("Ctrl+Shift+K"),
    ));
    entries.push(Entry::Item(
        Item::new(BOOKMARK, "Bookmark this page", Icon::Plus)
            .glyph("bookmark")
            .shortcut("Ctrl+D")
            .disabled_if(crate::profile::active().is_ok_and(|profile| profile.is_guest())),
    ));
    entries.push(Entry::Item(
        Item::new(SESSIONS, "Saved sessions…", Icon::Check)
            .glyph("folder")
            .disabled_if(crate::profile::active().is_ok_and(|profile| profile.is_guest())),
    ));
    entries.push(Entry::Item(
        Item::new(EXTENSIONS, "Extensions…", Icon::Grid).glyph("puzzle").shortcut("Ctrl+Shift+E"),
    ));
    entries.push(Entry::Item(
        Item::new(PASSWORDS, "Passwords & passkeys", Icon::Key)
            .glyph("key")
            .shortcut("Ctrl+,")
            .disabled_if(crate::profile::active().is_ok_and(|profile| profile.is_guest())),
    ));
    entries.push(Entry::Item(Item::new(
        IMPORT,
        "Import from another browser…",
        Icon::Grid,
    )
    .glyph("download")));
    entries.push(Entry::Separator);
    entries.push(Entry::Item(
        Item::new(TAB_LAYOUT, "Vertical tabs", Icon::Grid)
            .glyph("sidebar")
            .shortcut("Ctrl+Shift+V")
            .checked(App::tab_layout(app) == crate::settings::TabLayout::Vertical),
    ));
    entries.push(Entry::Item(
        Item::new(WORKSPACES, "Workspaces", Icon::Grid).glyph("workspace").submenu_with(workspace_menu_entries(app)),
    ));
    entries.push(Entry::Separator);
    let blocker = Item::new(BLOCKER, "Block ads & trackers", Icon::Shield)
        .glyph("shield")
        .shortcut("Ctrl+Shift+B")
        .described("Block ads & trackers on this site (Ctrl+Shift+B)");
    entries.push(Entry::Item(match App::blocker_active_for_site(app) {
        Some(active) => blocker.checked(active),
        None => blocker.disabled(),
    }));
    entries.push(Entry::Separator);
    entries.push(Entry::Item(Item::new(SETTINGS, "Settings…", Icon::Gear).glyph("gear")));
    entries.push(Entry::Item(
        Item::new(PRIVACY, "Privacy dashboard", Icon::Shield).glyph("eye").shortcut("Ctrl+Shift+Del"),
    ));
    entries.push(Entry::Separator);
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
        footer: format!("Bowser {}", env!("CARGO_PKG_VERSION")),
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
            NEW_TAB => {
                App::new_tab(app);
                focus_omni(hwnd, app, true);
            }
            PROFILE => {}
            NOTES => {
                notes_panel::open(hwnd);
                apply_sidebar_bounds(hwnd, app);
            }
            PALETTE => run_command_palette(hwnd, app),
            BOOKMARK => add_current_bookmark(hwnd, app),
            SESSIONS => show_sessions(hwnd, app),
            TAB_LAYOUT => {
                let _ = App::toggle_tab_layout(app);
                let _ = with_chrome(hwnd, |s| s.refresh(app, true));
                sync_split_ratio(hwnd, App::split_view(app));
                apply_sidebar_bounds(hwnd, app);
                html_tabs_sync(hwnd);
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            WORKSPACES => show_workspaces_menu(hwnd, app),
            EXTENSIONS => show_extensions_menu(hwnd, app),
            PASSWORDS => {
                if let Err(error) = passwords_panel::open(hwnd) {
                    show_error_message(&format!(
                        "Passwords & Passkeys could not be opened: {error}"
                    ));
                }
            }
            IMPORT => {
                if let Err(error) = import_panel::open(hwnd) {
                    show_error_message(&format!("Browser import could not be opened: {error}"));
                }
            }
            BLOCKER => App::toggle_blocker(app),
            SETTINGS => {
                if let Err(error) = settings_panel::open(hwnd, app) {
                    show_error_message(&format!("Settings could not be opened: {error}"));
                }
            }
            PRIVACY => privacy_panel::open(hwnd, app),
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
                run_workspace_action(hwnd, app, id, &built_workspaces, active_workspace)
            }
            _ => {}
        }
    }
    let _ = with_chrome(hwnd, |s| s.refresh(app, true));
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
    });
    // --- end popup host ---
}

fn run_command_palette(hwnd: HWND, app: &AppRef) {
    let Ok(profile) = crate::profile::active() else {
        return;
    };
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
    let bookmarks = if !crate::productivity::profile_allows_productivity(&profile) {
        Vec::new()
    } else {
        crate::productivity::read_bookmarks(&profile.bookmarks_file())
    };
    let history = if !crate::productivity::profile_allows_productivity(&profile)
        || !profile.history_enabled()
    {
        Vec::new()
    } else {
        crate::productivity::read_history(&profile.history_file())
    };
    let commands = crate::productivity::browser_commands();
    // --- popup surfaces (lane PP) ---
    if html_popup::page_ready(hwnd) {
        if html_popup::is_active(hwnd) {
            html_popup::cancel(hwnd, app);
        }
        let items = command_palette::html_items(&tabs, &bookmarks, &history, &commands);
        let page = command_palette::html_page(&items);
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
    let Some(item) = command_palette::show(hwnd, tabs, bookmarks, history, commands) else {
        return;
    };
    run_palette_item(hwnd, app, &item.action);
}

/// Runs one palette result: a tab id that is still open, an http(s) address, or a command.
fn run_palette_item(hwnd: HWND, app: &AppRef, action: &str) {
    if let Some(id) = action.strip_prefix("tab:").and_then(|s| s.parse::<u32>().ok()) {
        if App::tab_ids(app).contains(&TabId(id)) {
            let _ = App::select_tab(app, TabId(id));
        }
    } else if let Some(url) = action
        .strip_prefix("url:")
        .and_then(|s| url::Url::parse(s).ok())
        .filter(|url| matches!(url.scheme(), "http" | "https"))
    {
        App::navigate_current(app, &url);
    } else {
        run_palette_command(hwnd, app, action);
    }
}

fn run_palette_command(hwnd: HWND, app: &AppRef, action: &str) {
    match action {
        "new-tab" => {
            App::new_tab(app);
            focus_omni(hwnd, app, true);
        }
        "close-tab" => {
            let _ = App::close_tab(app, App::active_tab_id(app));
        }
        "settings" => {
            let _ = settings_panel::open(hwnd, app);
        }
        "toggle-sidebar" => {
            notes_panel::open(hwnd);
            apply_sidebar_bounds(hwnd, app);
        }
        "passwords" => {
            let _ = passwords_panel::open(hwnd);
        }
        "profiles" => show_profile_menu(hwnd),
        "save-session" | "sessions" => show_sessions(hwnd, app),
        "add-bookmark" => add_current_bookmark(hwnd, app),
        "reload" => App::reload(app),
        "back" => navigate_back_for_focused_pane(app),
        "forward" => navigate_forward_for_focused_pane(app),
        "extensions" => show_extensions_menu(hwnd, app),
        _ => {}
    }
}

fn add_current_bookmark(hwnd: HWND, app: &AppRef) {
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

fn show_sessions(hwnd: HWND, _app: &AppRef) {
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
        "Save current tabs…",
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
    let remove = Item::new(REMOVE_MENU, "Remove extension…", Icon::Close).glyph("trash");
    entries.push(Entry::Item(if records.is_empty() {
        remove.disabled()
    } else {
        remove.submenu_with(remove_entries)
    }));
    entries.push(Entry::Separator);
    entries.push(Entry::Item(Item::new(
        INSTALL,
        "Install unpacked extension…",
        Icon::Plus,
    )
    .glyph("folder")));

    // --- popup host (lane M) ---
    let point = screen_point(hwnd, main_menu_anchor(hwnd));
    let menu = Menu {
        header: None,
        entries,
        footer: "Unpacked extensions".to_owned(),
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
            INSTALL => install_extension(hwnd, app, &profile),
            id if (TOGGLE_BASE..TOGGLE_BASE + MAX_LISTED).contains(&id) => {
                if let Some(record) = live(id - TOGGLE_BASE) {
                    toggle_extension(app, &profile, &record);
                }
            }
            id if (REMOVE_BASE..REMOVE_BASE + MAX_LISTED).contains(&id) => {
                if let Some(record) = live(id - REMOVE_BASE) {
                    remove_extension(app, &profile, &record);
                }
            }
            _ => {}
        }
    });
    // --- end popup host ---
}

fn toggle_extension(
    app: &AppRef,
    profile: &crate::profile::Profile,
    record: &crate::extensions::Record,
) {
    use crate::extensions;
    let enabled = !record.enabled;
    if let Err(error) = extensions::set_enabled(&profile.extensions_file(), &record.id, enabled) {
        show_error_message(&format!("The extension was not changed. {error}"));
        return;
    }
    match extensions::folder_for(&profile.extensions_dir(), &record.id) {
        Ok(folder) => App::set_extension_enabled(app, record.id.clone(), folder, enabled),
        Err(error) => show_error_message(&error.to_string()),
    }
}

fn remove_extension(
    app: &AppRef,
    profile: &crate::profile::Profile,
    record: &crate::extensions::Record,
) {
    use crate::extensions;
    let question = format!(
        "Remove the extension \"{}\" from this profile? Its copied files will be deleted.",
        record.name
    );
    if !crate::win32::ask_yes_no(&question) {
        return;
    }
    let extensions_dir = profile.extensions_dir();
    let records_file = profile.extensions_file();
    let id = record.id.clone();
    App::remove_extension(
        app,
        record.id.clone(),
        Box::new(move |result| match result {
            Ok(()) => {
                extensions::record_load_result(&id, Ok(()));
                if let Err(error) = extensions::uninstall(&extensions_dir, &records_file, &id) {
                    show_error_message(&format!(
                        "The extension was removed from the list, but its files were not deleted. {error}"
                    ));
                }
            }
            Err(reason) => {
                extensions::record_load_result(&id, Err(format!("Remove failed: {reason}")));
            }
        }),
    );
}

/// Asks for an unpacked extension folder, shows its permissions, then copies it into the profile
/// and loads it. Nothing is copied before the user agrees.
fn install_extension(hwnd: HWND, app: &AppRef, profile: &crate::profile::Profile) {
    use crate::extensions;
    let Some(folder) = pick_extension_folder(hwnd) else {
        return;
    };
    let result = extensions::install_confirmed(
        &folder,
        &profile.extensions_dir(),
        &profile.extensions_file(),
        &mut |inspection| {
            let question = format!(
                "Install \"{}\" version {}?\n\n{}\n\nOnly install extensions you trust. An extension can read and change the pages it has access to.",
                inspection.manifest.name,
                inspection.manifest.version,
                extensions::permission_summary(&inspection.manifest)
            );
            crate::win32::ask_yes_no(&question)
        },
    );
    match result {
        Ok(Some(record)) => match extensions::folder_for(&profile.extensions_dir(), &record.id) {
            Ok(installed) => App::set_extension_enabled(app, record.id, installed, true),
            Err(error) => show_error_message(&error.to_string()),
        },
        Ok(None) => {}
        Err(error) => show_error_message(&format!("The extension was not installed. {error}")),
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
            // --- panel host (lane P) ---
            html_panel::register(&app, hwnd, env.clone());
            // --- end panel host ---
            // The two controllers are requested together and are created concurrently. Each one's
            // failure is its own.
            start_html_toolbar(hwnd, &env);
            start_html_tabs(hwnd);
            start_html_rail(hwnd);
            // --- popup surfaces (lane PP) ---
            // Requested last, with the visible chrome: the chrome controllers share one renderer,
            // so the hidden popup page is ready about when the toolbar is, instead of a separate
            // (slow, on a loaded machine) start 2 s after it. It stays suspended while hidden.
            html_popup::prewarm(hwnd, env.clone());
            // --- end popup surfaces ---
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
    // --- hover layer --- (kept on its original schedule: once the toolbar is active)
    if with_chrome(hwnd, |s| s.html_toolbar_active).unwrap_or(false) {
        if let Some(env) = env.clone() { super::html_hover::prewarm(hwnd, env); }
    }
    // --- end hover layer ---
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
    prewarm_html_popup(hwnd);
}
// --- end popup surfaces ---
// --- end popup host ---
// --- panel host (lane P) ---
fn html_panel_failed(hwnd: HWND) {
    let active = side_panel::active_id(hwnd);
    html_panel::fail(hwnd);
    if let Some(app) = app_of(hwnd) {
        super::panel_history::register(&app);
        super::panel_bookmarks::register(&app);
        super::panel_customize::register(&app);
    }
    if let Some(id @ (side_panel::PanelId::History | side_panel::PanelId::Bookmarks | side_panel::PanelId::Customize)) = active {
        side_panel::close_content(hwnd, id);
    }
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
    let created = ChromeHost::create(
        env,
        hwnd,
        bounds,
        View::Toolbar,
        on_command,
        on_failed,
        Some(on_accelerator),
        move |result| html_created(hwnd, result),
    );
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
        // Normally created with the other chrome controllers; this only covers a host that
        // failed and was shut down.
        request_popup_prewarm(hwnd);
    }
    // --- end popup host ---
    let native_text = (!edit.is_invalid() && activating).then(|| read_edit(edit));
    let native_focused = unsafe { !edit.is_invalid() && GetFocus() == edit };
    let handoff = native_text.and_then(|text| {
        let app = app_of(hwnd)?;
        let id = App::active_tab_id(&app);
        let committed = with_chrome(hwnd, |s| s.urls.get(&id).cloned().unwrap_or_default())?;
        chrome_host::should_handoff_edit(native_focused, &text, &committed)
            .then_some((id, text))
    });
    sync_html_host_bounds(hwnd);
    if activating {
        // Benchmark harnesses assert on this event to know the HTML chrome, not the native one, is live.
        if let Some(app) = app_of(hwnd) {
            let _ = app.borrow().log.record("html_chrome_shown");
        }
    }
    html_sync(hwnd);
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
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

/// The application state behind this window, if it is still alive. The clone is an `Rc` handle, so
/// no borrow of the window table or the app is held by the caller.
fn app_of(hwnd: HWND) -> Option<AppRef> {
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
    let Some((palette, dpi, layout)) = with_chrome(hwnd, |s| {
        if !(s.html_page_ready || s.html_tabs_page_ready || s.html_rail_page_ready) { return None; }
        Some((s.palette, s.dpi, s.layout))
    })
    .flatten() else {
        return;
    };
    let id = App::active_tab_id(&app);
    let site = App::site_security(&app);
    let (pillx, pillw, menux) = chrome_host::toolbar_css_placement(&layout, dpi).unwrap_or_default();
    let state = ViewState {
        scale: dpi as f32 / 96.0,
        pillx,
        pillw,
        menux,
        solid: glass::backdrop_mode() == glass::BackdropMode::Solid,
        dark: glass::appearance() == crate::theme::Appearance::Dark,
        back: App::tab_can_go_back(&app, id),
        forward: App::tab_can_go_forward(&app, id),
        loading: App::tab_loading(&app, id),
        // --- omnibox suggest ---
        split: App::split_view(&app).is_some(),
        bookmarked: App::tab_committed_url(&app, id)
            .is_some_and(|url| super::html_suggest::is_bookmarked(url.as_str())),
        // --- end omnibox suggest ---
        progress: App::tab_progress_percent(&app, id).unwrap_or(0),
        security: chrome_host::security_of(site),
        label: site.summary().to_string(),
        permblocked: App::active_permission_blocked(&app),
        theme: chrome_host::theme_tokens(palette),
    };
    let json = state.to_json();
    html_broadcast_state(hwnd, &json);
    let Some(host) = with_chrome(hwnd, |s| s.html_toolbar_active.then(|| s.html.clone()).flatten()).flatten() else {
        return;
    };
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
    if send_text {
        if host.post_json(&set_text_json(&display, select)).is_ok() {
            let _ = with_chrome(hwnd, |s| {
                if s.html_state_generation[0] != generation { return; }
                s.html_synced_tab = Some(id);
                s.html_text_sent = Some((id, display.clone()));
                s.edit_text.clone_from(&display);
                s.completion = None;
            });
        }
    }
}

/// Each ready page receives the same state; each host has its own successful-send cache.
fn html_broadcast_state(hwnd: HWND, json: &str) {
    let Some(candidates) = with_chrome(hwnd, |s| [
        (s.html_page_ready && chrome_host::state_differs(&s.html_last_state, json)).then(|| s.html.clone()).flatten(),
        (s.html_tabs_page_ready && chrome_host::state_differs(&s.html_tabs_last_state, json)).then(|| s.html_tabs.clone()).flatten(),
        (s.html_rail_page_ready && chrome_host::state_differs(&s.html_rail_last_state, json)).then(|| s.html_rail.clone()).flatten(),
    ]) else { return; };
    let generations = with_chrome(hwnd, |s| s.html_state_generation).unwrap_or_default();
    for (index, candidate) in candidates.into_iter().enumerate() {
        let Some(host) = candidate else { continue; };
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

/// Falls back to the native toolbar: the HTML region is released, the native address input comes
/// back and the host is closed. Safe to call more than once.
fn html_failed(hwnd: HWND) {
    let Some((host, edit)) = with_chrome(hwnd, |s| {
        s.html_toolbar_active = false;
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
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
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
            super::html_hover::suggest_hide(hwnd);
            return;
        }
        // --- end omnibox suggest ---
        ChromeCommand::Submit(text) => {
            let Some(app) = app_of(hwnd) else { return };
            super::html_hover::suggest_hide(hwnd); // --- omnibox suggest ---
            // Same path as the native omnibox: remember the typed text, then let App normalise it.
            remember_url(hwnd, App::active_tab_id(&app), &text);
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
            if !on {
                super::html_hover::suggest_focus_lost(hwnd);
            }
            // --- end omnibox suggest ---
            let _ = with_chrome(hwnd, |s| {
                s.html_omnibox_focused = on;
                if !on {
                    // Leaving the omnibox discards typed text, as the native edit does. The next
                    // sync sends the tab's address again.
                    s.html_text_sent = None;
                    s.completion = None;
                }
            });
        }
        ChromeCommand::Open(OpenTarget::Menu) => {
            defer_html_command(hwnd, DeferredChromeCommand::MainMenu);
        }
        ChromeCommand::Open(OpenTarget::Site) => {
            defer_html_command(hwnd, DeferredChromeCommand::SiteInfo);
        }
        ChromeCommand::Open(OpenTarget::NewTab) => {
            if let Some(app) = app_of(hwnd) {
                App::new_tab(&app);
                focus_omni(hwnd, &app, true);
            }
        }
        ChromeCommand::Open(OpenTarget::Split) => {
            // Same as the native omnibox split button: open the split menu.
            defer_html_command(hwnd, DeferredChromeCommand::SplitMenu);
        }
        // Tab strip and rail commands are handled by their own views, not the toolbar.
        ChromeCommand::Tab { .. }
        | ChromeCommand::TabMove { .. }
        | ChromeCommand::Group { .. }
        | ChromeCommand::Rail { .. }
        | ChromeCommand::RailMove { .. }
        | ChromeCommand::Panel { .. }
        | ChromeCommand::PopupSize { .. }
        | ChromeCommand::PopupPick { .. }
        | ChromeCommand::PopupDismiss { .. }
        | ChromeCommand::PopupAct { .. }
        // --- hover layer ---
        | ChromeCommand::Hover(_)
        | ChromeCommand::HoverAct { .. }
        // --- end hover layer ---
        // --- popup host (lane M) ---
        | ChromeCommand::Side(_) => {}
        // --- end popup host ---
    }
    html_sync(hwnd);
}

/// Bit in the `WM_SHORTCUT_KEY` LPARAM set when the chord came from an HTML chrome view. Such a
/// tab-changing command then also moves focus to the page, not left in the chrome.
const SHORTCUT_FROM_CHROME: isize = 8;

/// Claims a key an HTML chrome view did not consume, when it is a browser shortcut. While the
/// HTML omnibox is being edited, its editing chords stay with the field (see `accel_command`).
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
    let editing =
        view == View::Toolbar && with_chrome(hwnd, |s| s.html_omnibox_focused).unwrap_or(false);
    if view != View::Panel
        && side_panel::should_dismiss_panel_accelerator(
            accel.vk, accel.key_down, side_panel::is_open(hwnd), editing,
        )
    {
        side_panel::close(hwnd);
        if let Some(app) = app_of(hwnd) { apply_sidebar_bounds(hwnd, &app); }
        return true;
    }
    if accel_command(accel, editing).is_none() {
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
    ViewState {
        scale: dpi as f32 / 96.0,
        pillx: 0,
        pillw: 0,
        menux: 0,
        solid: true,
        dark: glass::appearance() == crate::theme::Appearance::Dark,
        back: false,
        forward: false,
        loading: false,
        split: false,
        bookmarked: false,
        progress: 0,
        security: crate::chrome_host::protocol::Security::None,
        label: String::new(),
        permblocked: false,
        theme: chrome_host::theme_tokens(&palette),
    }.to_json()
}

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
fn screen_point(hwnd: HWND, mut point: POINT) -> POINT {
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
    let created = ChromeHost::create(
        &env,
        hwnd,
        bounds,
        View::Tabs,
        on_command,
        on_failed,
        Some(on_accelerator),
        move |result| html_tabs_created(hwnd, result),
    );
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
        s.html_tabs_workspace = None;
        // --- end vertical sidebar ---
        s.html_state_generation[1] = s.html_state_generation[1].wrapping_add(1);
        s.html_favicons_sent.clear();
        s.sync_html_tabs_bounds();
    });
    sync_html_host_bounds(hwnd);
    html_tabs_sync(hwnd);
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

/// The tab strip failed: its row goes back to native paint and its host is closed. The toolbar is
/// unaffected. Safe to call more than once.
fn html_tabs_failed(hwnd: HWND) {
    let Some(host) = with_chrome(hwnd, |s| {
        s.html_tabs_page_ready = false;
        s.html_tabs_last = None;
        // --- vertical sidebar (lane S) ---
        s.html_tabs_layout = None;
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
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
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
        ChromeCommand::Group { action, key } => html_group_action(hwnd, action, key),
        // --- vertical sidebar (lane S) ---
        ChromeCommand::Side(action) => {
            let vertical = with_chrome(hwnd, |s| s.sidebar.is_some() && s.html_tab_region().is_some()).unwrap_or(false);
            if !vertical { return; }
            match action {
                SideAction::Workspace => defer_html_command(hwnd, DeferredChromeCommand::SidebarWorkspaceMenu),
                SideAction::NewTab => {
                    if let Some(app) = app_of(hwnd) {
                        App::new_tab(&app);
                        focus_omni(hwnd, &app, true);
                    }
                }
            }
        }
        // --- end vertical sidebar ---
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
        View::Panel | View::Popup | View::Dialog | View::DialogNotes | View::DialogSettings | View::DialogPrivacy | View::DialogRename | View::Hover => None,
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
        ChromeCommand::PopupSize { tok, w, h } => html_popup::show_sized(hwnd, tok, w, h),
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
        // --- end popup surfaces ---
        _ => {}
    }
}

pub(super) fn popup_state_json(hwnd: HWND) -> Option<String> {
    let app = app_of(hwnd)?;
    let (palette, dpi) = with_chrome(hwnd, |s| (s.palette, s.dpi))?;
    let id = App::active_tab_id(&app);
    let site = App::site_security(&app);
    let state = ViewState {
        scale: dpi as f32 / 96.0,
        pillx: 0,
        pillw: 0,
        menux: 0,
        solid: glass::backdrop_mode() == glass::BackdropMode::Solid,
        dark: glass::appearance() == crate::theme::Appearance::Dark,
        back: App::tab_can_go_back(&app, id),
        forward: App::tab_can_go_forward(&app, id),
        loading: App::tab_loading(&app, id),
        split: false,
        bookmarked: false,
        progress: App::tab_progress_percent(&app, id).unwrap_or(0),
        security: chrome_host::security_of(site),
        label: site.summary().to_owned(),
        permblocked: App::active_permission_blocked(&app),
        theme: chrome_host::theme_tokens(palette),
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
        DeferredChromeCommand::SplitMenu => show_split_menu(hwnd, &app),
        DeferredChromeCommand::TabMenu { id, x, y } => {
            // The tab may have closed while the command waited.
            if App::tab_ids(&app).contains(&id) {
                card::cancel(hwnd);
                show_html_tab_menu(hwnd, &app, id, x, y);
            }
        }
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
        // --- vertical sidebar (lane S) ---
        DeferredChromeCommand::SidebarWorkspaceMenu => show_workspaces_menu(hwnd, &app),
        // --- end vertical sidebar ---
        // --- popup host (lane M) ---
        DeferredChromeCommand::MenuResult { tok, id } => html_popup::run_result(hwnd, tok, id, &app),
        DeferredChromeCommand::PopupAct { tok, id, n } => html_popup::run_act(hwnd, tok, id, n, &app),
        // --- end popup host ---
    }
    html_sync(hwnd);
    html_tabs_sync(hwnd);
    html_rail_sync(hwnd);
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
            let _ = App::select_tab(&app, id);
        }
        TabAction::Close => {
            let _ = App::close_tab(&app, id);
        }
        TabAction::Pin => {
            let pinned = App::tab_pinned(&app, id);
            let _ = App::set_tab_pinned(&app, id, !pinned);
        }
        TabAction::Mute => {
            let muted = App::tab_muted(&app, id);
            App::set_tab_muted(&app, id, !muted);
        }
        TabAction::Menu => {
            if let Some((x, y)) = cursor_client_point(hwnd) {
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
    let workspace_icon = format!("g:{}", format!("{:?}", workspace.icon()).to_lowercase());
    if let Some(json) = chrome_host::protocol::workspace_json(&workspace_name, &workspace_icon) {
        let already_sent = with_chrome(hwnd, |s| {
            s.html_tabs_workspace.as_ref().is_some_and(|(name, icon)| name == &workspace_name && icon == &workspace_icon)
        }).unwrap_or(false);
        if !already_sent && host.post_json(&json).is_ok() {
            let _ = with_chrome(hwnd, |s| s.html_tabs_workspace = Some((workspace_name, workspace_icon)));
        }
    }
    // --- end vertical sidebar ---
    let inputs = App::tab_group_inputs(&app);
    let preferences = App::tab_group_preferences(&app);
    let keys = tab_groups::group_keys(&inputs, &preferences);
    let group_of: HashMap<TabId, Option<tab_groups::TabGroupKey>> =
        inputs.iter().map(|input| input.id).zip(keys).collect();
    let with_png = html_tabs_favicons(hwnd, &app, &ids);
    let rows: Vec<chrome_host::TabRow> = snapshot
        .into_iter()
        .map(|(id, title, loading, marker)| chrome_host::TabRow {
            id,
            title,
            loading,
            pinned: App::tab_pinned(&app, id),
            muted: App::tab_muted(&app, id),
            marker,
            group: group_of
                .get(&id)
                .cloned()
                .flatten()
                .map(|key| chrome_host::row_group(&key, &preferences)),
            has_favicon: with_png.contains(&id),
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
            if s.html_state_generation[1] == generation { s.html_tabs_last = Some(json); }
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
    let created = ChromeHost::create(
        &env,
        hwnd,
        bounds,
        View::Rail,
        on_command,
        on_failed,
        Some(on_accelerator),
        move |result| html_rail_created(hwnd, result),
    );
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
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

/// The HTML rail failed: its area goes back to native paint and hit tests and its host is closed.
/// The toolbar and tab strip are unaffected. Safe to call more than once.
fn html_rail_failed(hwnd: HWND) {
    let Some(host) = with_chrome(hwnd, |s| {
        s.html_rail_page_ready = false;
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
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
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
        ChromeCommand::Rail { action, key } => html_rail_action(hwnd, action, key),
        ChromeCommand::RailMove { key, before } => html_rail_move(hwnd, key, before),
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
fn html_rail_action(hwnd: HWND, action: RailAction, wire: RailKeyWire) {
    let Some(key) = chrome_host::rail_key_from_wire(wire, &rail_model_keys(hwnd)) else {
        return;
    };
    match action {
        RailAction::Click => defer_html_command(hwnd, DeferredChromeCommand::RailClick { key }),
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
    let Some((host, json, favicons, generation)) = with_chrome(hwnd, |s| {
        s.html_rail_region()?;
        let host = s.html_rail.clone()?;
        let items = {
            let entries = rail_entries(s);
            chrome_host::rail_items(&entries, open, |key| {
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
        Some((host, send_json.then_some(json), favicons, s.html_state_generation[2]))
    })
    .flatten() else {
        return;
    };
    if let Some(json) = json {
        if host.post_json(&json).is_ok() {
            let _ = with_chrome(hwnd, |s| {
                if s.html_state_generation[2] == generation { s.html_rail_last = Some(json); }
            });
        }
    }
    for (key, png, message) in favicons {
        if host.post_json(&message).is_ok() {
            let _ = with_chrome(hwnd, |s| {
                if s.html_state_generation[2] == generation { s.html_rail_favs_sent.insert(key, png); }
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
        super::html_hover::suggest_hide(hwnd); // --- omnibox suggest ---
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
    use super::html_suggest::{self as suggest, Target};
    let started = Instant::now();
    let typed_url = App::typed_address(app, text);
    let active = App::active_tab_id(app);
    let tabs: Vec<_> =
        App::tab_suggest_sources(app).into_iter().filter(|(id, ..)| *id != active).collect();
    let bookmarks = suggest::bookmarks();
    let Some(mut rows) = with_chrome(hwnd, |s| {
        suggest::rows(&s.history, s.completion.as_ref(), text, typed_url.as_ref(), &tabs, &bookmarks)
    }) else {
        return;
    };
    // A favicon is known for a row's page only through an open tab on the same site.
    for row in &mut rows {
        let tab = match row.target {
            Target::Tab(id) => Some(id),
            _ if row.fav_url.is_empty() => None,
            _ => suggest::tab_with_host(&tabs, &row.fav_url),
        };
        row.item.png = tab.and_then(|id| App::tab_favicon(app, id)).filter(|png| chrome_host::favicon_sendable(png));
    }
    let Some((anchor, width)) = with_chrome(hwnd, |s| {
        let omnibox = s.layout.omnibox;
        let mut origin = windows::Win32::Foundation::POINT { x: omnibox.left, y: omnibox.top };
        unsafe { let _ = ClientToScreen(hwnd, &mut origin); }
        let css = chrome_host::css_px_from_physical(omnibox.width(), s.dpi).max(0) as u32;
        (RECT { left: origin.x, top: origin.y, right: origin.x + omnibox.width(), bottom: origin.y + omnibox.height() }, css.max(480))
    }) else {
        return;
    };
    let computed = started.elapsed();
    super::html_hover::suggest_show(hwnd, seq, anchor, width, rows);
    if std::env::var_os("BOWSER_SUGGEST_TRACE").is_some() {
        eprintln!("suggest seq={seq} compute={computed:?} total={:?}", started.elapsed());
    }
}

/// Does what picking a dropdown row does: switch to the tab, or go to the address or search, the
/// same path as typing it and pressing Enter.
pub(super) fn html_suggest_run(hwnd: HWND, target: super::html_suggest::Target) {
    use super::html_suggest::Target;
    let Some(app) = app_of(hwnd) else { return };
    match target {
        Target::Tab(id) => {
            let _ = App::select_tab(&app, id);
        }
        Target::Navigate(text) | Target::Search(text) => {
            remember_url(hwnd, App::active_tab_id(&app), &text);
            App::submit(&app, &text);
        }
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
fn paint_chrome(hwnd: HWND, app: Option<&AppRef>) {
    static PAINT_LOG_ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    // The HTML chrome views have no paint of their own here; this is where their state is refreshed.
    sync_html_host_bounds(hwnd);
    html_sync(hwnd);
    html_tabs_sync(hwnd);
    html_rail_sync(hwnd);
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        if let Some(app) = app {
            let active = App::active_tab_id(app);
            let split_view = App::split_view(app);
            let active_has_controller = split_view.map_or_else(
                || App::active_tab_has_controller(app),
                |split| {
                    App::tab_has_controller(app, split.left)
                        || App::tab_has_controller(app, split.right)
                },
            );
            let progress_target = App::tab_progress_percent(app, active);
            let can_back = App::tab_can_go_back(app, active);
            let can_forward = App::tab_can_go_forward(app, active);
            let site = App::site_security(app);
            let workspace_name = App::workspaces(app).active().name().to_owned();
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
                let rail_entries = s.rail_entries.iter().map(|entry| RailEntry {
                    key: entry.key,
                    icon: entry.icon,
                    label: entry.label.clone(),
                    active: entry.active,
                    dot: false,
                    accent: entry.accent,
                    favicon: entry.favicon.as_deref(),
                }).collect::<Vec<_>>();
                let mut rail_fades = Vec::new();
                s.motion.rail_fades(now, &mut rail_fades);
                let open_panel = open_rail_panel(hwnd);
                let rail = RailView { entries: &rail_entries, fades: &rail_fades, open_panel, scroll: s.rail_scroll, drop_indicator: s.rail_drag.and_then(|drag| drag.target.map(|(_, indicator)| indicator)), focused: s.rail_keyboard_focus.then_some(s.rail_focus).flatten(), tooltip: s.rail_tooltip_visible.then_some(s.rail_tooltip_key).flatten() };
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
                    tabs: views,
                    active: active_i,
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
                };
                let sidebar_viewport = adjusted_viewport(hwnd, to_win_rect(s.layout.viewport), s.dpi);
                let sidebar_viewport = Rect::new(
                    sidebar_viewport.left,
                    sidebar_viewport.top,
                    sidebar_viewport.right,
                    sidebar_viewport.bottom,
                );
                let page_regions = if active_has_controller {
                    if let Some(split) = split_view {
                        layout::split_pane_layout(
                            sidebar_viewport,
                            s.split_divider,
                            s.layout.metrics.px(220),
                            s.layout.metrics.px(6),
                        )
                        .map(|panes| {
                            (
                                sidebar_viewport,
                                Some(panes),
                                split.focused == crate::split_view::Pane::Left,
                            )
                        })
                    } else {
                        Some((sidebar_viewport, None, false))
                    }
                } else {
                    None
                };
                s.surface.set_page_regions(page_regions);
                let t0 = std::time::Instant::now();
                s.surface.paint(&view, dirty);
                if let Some(sidebar) = s.sidebar {
                    s.surface.paint_vertical_sidebar(
                        &sidebar,
                        &s.sidebar_rows,
                        &workspace_name,
                        active,
                        s.sidebar_scroll,
                        s.sidebar_hover_row,
                        // --- vertical sidebar (lane S) ---
                        s.html_tab_region().is_some(),
                        // --- end vertical sidebar ---
                        dirty,
                        p,
                    );
                }
                if let Some(pane) = s.split_drop {
                    s.surface.paint_split_drop(sidebar_viewport, pane, p, dirty);
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
fn remember_url(hwnd: HWND, id: TabId, value: &str) {
    let parsed = url::Url::parse(value).or_else(|_| {
        if value.contains('.') || value.starts_with("localhost") {
            url::Url::parse(&format!("https://{value}"))
        } else {
            Err(url::ParseError::RelativeUrlWithoutBase)
        }
    });
    if let Ok(url) = parsed {
        let shown = omnibox::display_form(&url);
        let canonical = url.to_string();
        let _ = with_chrome(hwnd, |s| {
            s.urls.insert(id, shown);
            s.pending_history.insert(id, canonical);
        });
    }
}
fn focus_omni(hwnd: HWND, app: &AppRef, all: bool) {
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
/// Shows a navigation's address in the omnibox. While the field has focus the user owns its text and
/// selection, so the address is only remembered and appears once focus leaves.
pub(crate) fn sync_address_input(hwnd: HWND, edit: HWND, id: TabId, url: &url::Url) -> bool {
    if omnibox::should_sync_displayed_url(edit_focused(hwnd)) {
        return super::set_input_text(edit, url.as_str());
    }
    let shown = omnibox::display_form(url);
    let _ = with_chrome(hwnd, |s| s.urls.insert(id, shown));
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
    if let Some(split) = App::split_view(app) {
        let _ = App::close_split_pane(app, split.focused);
    } else {
        let tabs = App::tab_ids(app);
        let active = App::active_tab_id(app);
        if let Some(other) = tabs.into_iter().find(|id| *id != active)
            && let Some(bounds) = split_rects(hwnd, None)
        {
            let _ = App::open_split_view(
                app,
                active,
                other,
                crate::split_view::Pane::Right,
                bounds,
            );
        }
    }
}

fn dispatch_current(hwnd: HWND, app: &AppRef, key: u32) -> bool {
    let c = unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 };
    let s = unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 };
    let a = unsafe { GetKeyState(VK_MENU.0 as i32) < 0 };
    dispatch(hwnd, app, key, c, s, a)
}
fn dispatch(hwnd: HWND, app: &AppRef, key: u32, ctrl: bool, shift: bool, alt: bool) -> bool {
    let Some(cmd) = shortcuts::command_for(key, ctrl, shift, alt) else {
        return false;
    };
    // While the address field has focus, the field's own redo keeps Ctrl+Shift+Z.
    if cmd == Command::UndoTabOrganisation && edit_focused(hwnd) {
        return false;
    }
    match cmd {
        Command::NewTab => {
            App::new_tab(app);
            focus_omni(hwnd, app, true)
        }
        Command::CloseTab => {
            let _ = App::close_tab(app, App::active_tab_id(app));
        }
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
        Command::FocusSplitLeft => {
            let _ = App::focus_split_pane(app, crate::split_view::Pane::Left);
        }
        Command::FocusSplitRight => {
            let _ = App::focus_split_pane(app, crate::split_view::Pane::Right);
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
        Command::ToggleTabLayout => {
            let _ = App::toggle_tab_layout(app);
            let _ = with_chrome(hwnd, |s| s.refresh(app, true));
            sync_split_ratio(hwnd, App::split_view(app));
            apply_sidebar_bounds(hwnd, app);
            html_tabs_sync(hwnd);
        }
        Command::OpenPasswords => {
            if let Err(error) = passwords_panel::open(hwnd) {
                show_error_message(&format!(
                    "Passwords & Passkeys could not be opened: {error}"
                ));
            }
        }
        Command::FillPassword => passwords_panel::offer_fill(hwnd, app),
        Command::OpenPrivacyDashboard => privacy_panel::open(hwnd, app),
        Command::ShowHistory => {
            side_panel::open(hwnd, side_panel::PanelId::History);
            apply_sidebar_bounds(hwnd, app);
        }
    }
    let _ = with_chrome(hwnd, |s| s.refresh(app, true));
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
    arm_motion(hwnd);
    true
}

fn persist_profile_history(url: String, title: String) {
    let Ok(profile) = crate::profile::active() else {
        return;
    };
    if crate::productivity::profile_allows_productivity(&profile) && profile.history_enabled() {
        let _ = crate::productivity::add_history_entry(&profile.history_file(), &url, &title);
    }
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
        // Ctrl+Shift+V is the tab layout toggle. TranslateMessage still queues its Ctrl+V character
        // (SYN), which an EDIT would treat as a paste.
        if msg == WM_CHAR && w.0 == 0x16 && GetKeyState(VK_SHIFT.0 as i32) < 0 {
            return LRESULT(0);
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
            (0..15).map(TabId).collect::<Vec<_>>()
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
            s.relayout(
                1000,
                700,
                dpi,
                TabId(0),
                false,
                crate::settings::TabLayout::Horizontal,
                260,
            );
            let chrome = layout::chrome_height(dpi);
            assert_eq!(s.surface.height(), 700, "surface height at dpi {dpi}");
            assert_eq!(s.surface.width(), 1000, "surface width at dpi {dpi}");
            assert_eq!(s.layout.viewport.top, chrome, "viewport top at dpi {dpi}");
        }
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
