use super::{
    INPUT_ID, WM_ANIMATION_TIMER, WM_CARET_BLINK_TIMER, WM_FOREGROUND_SYNC, WM_PANE_FOCUSED,
    WM_HISTORY_BACK, WM_HISTORY_FORWARD, WM_IDLE_TRIM_TIMER, WM_NAVIGATION_PROGRESS,
    WM_PROGRESS_TIMER, WM_SESSION_SAVE_TIMER, WM_SHORTCUT_KEY, WM_SUBMIT, WM_TABS_CHANGED, card, frame, glass, island_menu,
    menu, passwords_panel, settings_panel, show_error_message, site_info,
};
use super::menu::{Entry, Icon, Item, Menu, ProfileHeader};
use crate::{
    app::{App, AppRef, IdleTimerAction},
    chrome::{
        layout::{self, Hit, Layout, Metrics, Rect},
        motion::{Emphasis, Motion},
        paint::{self, ChromeSurface, ChromeView, OmniboxView, TabView},
        tab_card::Marker,
    },
    favicon::{self, FaviconCache},
    omnibox::{self, Completion, History},
    shortcuts::{self, Command},
    tab_groups::{self, StripSlot, TabGroupInput},
    tabs::{FaviconKey, TabId},
    theme::Palette,
};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
    time::Instant,
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, ClientToScreen, CreateRectRgn, CreateRoundRectRgn, DEFAULT_QUALITY,
            DeleteObject, EndPaint, FillRect, GetMonitorInfoW, InvalidateRect,
            MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, PAINTSTRUCT, SetWindowRgn,
        },
        System::LibraryLoader::GetModuleHandleW,
        System::SystemInformation::GetTickCount,
        System::Threading::GetCurrentProcessId,
        UI::{
            Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
            Controls::{EM_GETSEL, EM_SETMARGINS, EM_SETSEL},
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{
                GetCapture, GetFocus, GetKeyState, GetLastInputInfo, LASTINPUTINFO, ReleaseCapture,
                SetCapture, SetFocus, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent,
                VK_CONTROL, VK_MENU, VK_SHIFT,
            },
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::{
                CREATESTRUCTW, CS_DBLCLKS, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW,
                DestroyWindow, DispatchMessageW,
                EN_CHANGE, ES_AUTOHSCROLL, EVENT_SYSTEM_FOREGROUND, GW_OWNER, GWL_STYLE,
                GWLP_USERDATA, GetCaretBlinkTime, GetClientRect, GetForegroundWindow,
                GetMessageW, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
                GetWindowTextW, GetWindowThreadProcessId, HMENU, HTCAPTION, HTCLOSE, HTMAXBUTTON,
                HTMINBUTTON, HideCaret, IsIconic, IsZoomed, KillTimer, MINMAXINFO,
                LoadCursorW, SetCursor, IDC_SIZEWE, MSG,
                MoveWindow, NCCALCSIZE_PARAMS, PostQuitMessage, RegisterClassW, SC_CLOSE,
                SC_KEYMENU, SC_MAXIMIZE, SC_MINIMIZE, SC_MOVE, SC_RESTORE, SC_SIZE, SIZE_MINIMIZED, SM_CXFRAME,
                SM_CXPADDEDBORDER, SM_CYFRAME, SPI_GETCLIENTAREAANIMATION, SW_SHOW, SWP_NOACTIVATE,
                SWP_NOSIZE, SWP_NOZORDER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SendMessageW,
                SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
                SystemParametersInfoW, TranslateMessage, WINDOW_STYLE, WINEVENT_OUTOFCONTEXT, WM_CHAR,
                WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_DPICHANGED,
                WM_DWMCOLORIZATIONCOLORCHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN,
                WM_KILLFOCUS,
                WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONUP, WM_MOUSEMOVE,
                WM_MOVE, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST,
                WM_NCRBUTTONUP,
                WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE,
                WM_NCMOUSEMOVE, WM_NCPAINT, WM_PAINT, WM_PASTE, WM_RBUTTONUP, WM_SETFOCUS,
                WM_SETICON, WM_SETTEXT, WM_SETTINGCHANGE, WM_SIZE, WM_SYSCOMMAND, WM_SYSKEYDOWN,
                WM_THEMECHANGED, WM_TIMER, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN,
                WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
                WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME, WS_VISIBLE,
                WVR_REDRAW,
            },
        },
    },
    core::{BOOL, w},
};

const CLASS_NAME: windows::core::PCWSTR = w!("BowserNativeWindow");
const WM_NCUAHDRAWCAPTION: u32 = 0x00AE;
const WM_NCUAHDRAWFRAME: u32 = 0x00AF;
const WM_MOUSELEAVE: u32 = 0x02A3;
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
        if self.to < 100.0 { return Some(value); }
        let Some(started) = self.started else { return Some(value); };
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
        let Some(started) = self.started else { return Some(self.to) };
        let t = (now.saturating_duration_since(started).as_secs_f32() * 1000.0
            / Self::DURATION_MS)
            .clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        Some((self.from + (self.to - self.from) * eased).clamp(0.0, 100.0))
    }

    fn retarget(&mut self, tab: TabId, navigation_id: u64, target: u8, now: Instant, animate: bool) {
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
        if target == self.to { return; }
        self.from = self.value(now).unwrap_or(0.0);
        self.to = target;
        self.started = animate.then_some(now);
    }

    fn settled(&self, now: Instant) -> bool {
        self.value(now).is_some_and(|value| value >= self.to) && !self.running(now)
    }

    fn clear(&mut self) { *self = Self::default(); }
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
    history: History,
    urls: HashMap<TabId, String>,
    pending_history: HashMap<TabId, String>,
    hover: Hit,
    pressed: Hit,
    drag: Option<(TabId, usize)>,
    dpi: u32,
    motion: Motion,
    active_tab: Option<TabId>,
    progress: ProgressTransition,
    first_paint_logged: bool,
    split_active: bool,
    split_dragging: bool,
    split_divider: i32,
}
impl WindowChrome {
    fn new(width: i32, height: i32, dpi: u32, overlay: HWND) -> Self {
        Self {
            surface: ChromeSurface::new(width, height, dpi),
            overlay,
            client: (width, height),
            layout: layout::compute(width, height, dpi, 1, false),
            palette: glass::palette(),
            tabs: Vec::with_capacity(8),
            slots: Vec::with_capacity(8),
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
            history: History::default(),
            urls: HashMap::with_capacity(8),
            pending_history: HashMap::with_capacity(8),
            hover: Hit::None,
            pressed: Hit::None,
            drag: None,
            dpi,
            motion: Motion::new(!animations_enabled()),
            active_tab: None,
            progress: ProgressTransition::default(),
            first_paint_logged: false,
            split_active: false,
            split_dragging: false,
            split_divider: width / 2,
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
        let mut group_inputs = Vec::with_capacity(ids.len());
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
            if reproject {
                let domain = App::tab_committed_url(app, id)
                    .as_ref()
                    .and_then(tab_groups::DomainKey::from_url);
                group_inputs.push(TabGroupInput {
                    id,
                    domain,
                    pinned,
                    opener: App::tab_opener(app, id),
                });
            }
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
            );
        }
    }
    fn relayout(&mut self, w: i32, h: i32, dpi: u32, active: TabId, maximised: bool) {
        self.client = (w, h);
        self.dpi = dpi.max(96);
        let i = self
            .slots
            .iter()
            .position(|slot| {
                self.tabs
                    .get(slot.tab_index)
                    .is_some_and(|tab| tab.id == active)
            })
            .unwrap_or(0);
        self.layout =
            layout::compute_with_active(w, h, self.dpi, self.slots.len().max(1), i, maximised)
                .with_compact_slots(self.slots.iter().map(|slot| slot.marker_only));
        self.split_divider = self.layout.viewport.left + self.layout.viewport.width() / 2 - 3;
        self.surface.resize(w, h, self.dpi)
    }
    /// The tab under the pointer, by identity, so its emphasis follows it across reorders.
    fn hovered_tab(&self) -> Option<TabId> {
        match self.hover {
            Hit::Tab(i) | Hit::TabClose(i) => self
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
        self.motion.prune(now);
        let running = self.motion.step(now);
        for &id in self.motion.repaint() {
            if let Some(rect) = self.tab_rect(id) {
                invalidate(hwnd, rect);
            }
        }
        running
    }
    fn update_completion(&mut self) {
        self.completion =
            omnibox::inline_completion(&self.history, &self.edit_text, !self.suppress_completion);
        self.suppress_completion = false
    }
}
thread_local! {static WINDOWS:RefCell<HashMap<isize,WindowChrome>>=RefCell::new(HashMap::new());}
fn main_window_style() -> WINDOW_STYLE {
    WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_CLIPCHILDREN
}

fn with_chrome<T>(hwnd: HWND, f: impl FnOnce(&mut WindowChrome) -> T) -> Option<T> {
    WINDOWS.with(|a| a.borrow_mut().get_mut(&(hwnd.0 as isize)).map(f))
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
        let had_state = s.drag.take().is_some() || s.pressed != Hit::None || std::mem::take(&mut s.split_dragging);
        s.pressed = Hit::None;
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
        let _ = glass::apply(hwnd);
        app.borrow_mut().hwnd = hwnd;
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
        App::submit_initial_url(&app);
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
                let l = with_chrome(hwnd, |s| s.layout).unwrap_or_else(|| {
                    layout::compute(r.right, r.bottom, dpi(hwnd), 1, IsZoomed(hwnd).as_bool())
                });
                let ih = matches!(
                    layout::hit_test(&l, p.x, p.y),
                    Hit::Tab(_)
                        | Hit::TabClose(_)
                        | Hit::NewTab
                        | Hit::TabMenu
                        | Hit::MainMenu
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
                LRESULT(0)
            }
            WM_SIZE => {
                card::cancel(hwnd);
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
                        );
                    });
                    position_edit(hwnd);
                    let l = with_chrome(hwnd, |s| s.layout).unwrap();
                    if let Some(panes) = split_rects(hwnd, None) {
                        App::update_split_bounds(app, panes);
                    }
                    App::window_size_changed(
                        app,
                        min,
                        RECT {
                            left: l.viewport.left,
                            top: l.viewport.top,
                            right: l.viewport.right,
                            bottom: l.viewport.bottom,
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
                        );
                        s.refresh(app, true)
                    });
                    refresh_edit_font(hwnd, app);
                    position_edit(hwnd);
                    let l = with_chrome(hwnd, |s| s.layout).unwrap();
                    if let Some(panes) = split_rects(hwnd, None) {
                        App::update_split_bounds(app, panes);
                    }
                    App::window_size_changed(
                        app,
                        false,
                        RECT {
                            left: l.viewport.left,
                            top: l.viewport.top,
                            right: l.viewport.right,
                            bottom: l.viewport.bottom,
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
            WM_NCRBUTTONUP if wparam.0 == windows::Win32::UI::WindowsAndMessaging::HTCAPTION as usize => {
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
                        if let Some(target) = App::tab_progress_percent(app, App::active_tab_id(app)) {
                            let animate = animations_enabled();
                            with_chrome(hwnd, |s| {
                                s.progress.retarget(App::active_tab_id(app), navigation_id, target, Instant::now(), animate);
                            });
                            if animate {
                                let _ = SetTimer(Some(hwnd), WM_PROGRESS_TIMER, ANIMATION_FRAME_MS, None);
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
            WM_TABS_CHANGED => {
                if let Some(app) = app {
                    let (structural, persisted) = App::take_tab_change_flags();
                    if structural {
                        card::cancel_card(hwnd);
                    }
                    let previously_active = with_chrome(hwnd, |s| s.active_tab).flatten();
                    let strips = with_chrome(hwnd, |s| {
                        s.refresh(app, structural);
                        (s.layout.strip, s.layout.tab_row)
                    });
                    let active = App::active_tab_id(app);
                    if previously_active != Some(active) {
                        card::cancel_card(hwnd);
                        let value = App::tab_committed_url(app, active)
                            .map(|url| url.to_string())
                            .unwrap_or_default();
                        let edit = app.borrow().edit;
                        let _ = SetWindowTextW(edit, &windows::core::HSTRING::from(value));
                        if let Some((navigation_id, target)) = App::active_navigation_progress_identity(app) {
                            with_chrome(hwnd, |s| {
                                s.progress.clear();
                                s.progress.retarget(active, navigation_id, target, Instant::now(), false);
                            });
                        } else {
                            with_chrome(hwnd, |s| s.progress.clear());
                        }
                        let _ = KillTimer(Some(hwnd), WM_PROGRESS_TIMER);
                    }
                    card::sync(hwnd, app, |id| tab_anchor(hwnd, id));
                    island_menu::sync(hwnd, app, |id| bar_anchor(hwnd, id));
                    if structural {
                        position_edit(hwnd);
                        let _ = InvalidateRect(Some(hwnd), None, false);
                        arm_motion(hwnd);
                    } else if let Some((toolbar, tabs)) = strips {
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
            WM_SHORTCUT_KEY => {
                if let Some(app) = app {
                    let m = lparam.0 as u8;
                    dispatch(
                        hwnd,
                        app,
                        wparam.0 as u32,
                        m & 1 != 0,
                        m & 2 != 0,
                        m & 4 != 0,
                    );
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if GetCapture() == hwnd {
                    let dragging = with_chrome(hwnd, |s| s.split_dragging).unwrap_or(false);
                    if dragging
                        && let Some(panes) = split_rects(hwnd, Some(frame::point_from_lparam(lparam).x))
                        && let Some(app) = app
                    {
                        let divider = panes.0.right;
                        let _ = with_chrome(hwnd, |s| s.split_divider = divider);
                        App::update_split_bounds(app, panes);
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                } else {
                    let p = frame::point_from_lparam(lparam);
                    let h = hit(hwnd, p.x, p.y);
                    if h == Hit::SplitDivider {
                        let cursor = LoadCursorW(None, IDC_SIZEWE);
                        if let Ok(cursor) = cursor { let _ = SetCursor(Some(cursor)); }
                    }
                    set_hover(hwnd, h);
                    if let Some(app) = app {
                        card::hover_changed(hwnd, app, hit_tab(hwnd, h));
                    }
                    island_menu::bar_hover(
                        hwnd,
                        island_bar_at(hwnd, p.x, p.y).map(|bar| (bar.tab, bar.rect)),
                    );
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
            WM_MOUSELEAVE => {
                set_hover(hwnd, Hit::None);
                card::pointer_left(hwnd);
                island_menu::pointer_left(hwnd);
                LRESULT(0)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_CAPTURECHANGED => {
                card::cancel(hwnd);
                if clear_pointer_state(hwnd) {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_CANCELMODE => {
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
                // Pressing the strip dismisses a hover card; the press then acts on the strip.
                card::cancel(hwnd);
                match hit(hwnd, p.x, p.y) {
                    Hit::Tab(i) => {
                        if let Some(bar) = island_bar_at(hwnd, p.x, p.y) {
                            // Clicking an island's bar collapses it, or expands it when it is collapsed.
                            if let Some(app) = app {
                                let _ = App::set_tab_group_collapsed(app, bar.tab, !bar.collapsed);
                                let _ = InvalidateRect(Some(hwnd), None, false);
                            }
                        } else if let Some((id, collapsed)) = group_toggle_target(hwnd, i, p.x, p.y) {
                            if let Some(app) = app {
                                let _ = App::set_tab_group_collapsed(app, id, collapsed);
                                let _ = InvalidateRect(Some(hwnd), None, false);
                            }
                        } else if let Some(id) = collapsed_group_target(hwnd, i) {
                            if let Some(app) = app {
                                let _ = App::set_tab_group_collapsed(app, id, false);
                                let _ = App::select_tab(app, id);
                            }
                        } else if let Some(app) = app {
                            if let Some(id) = slot_tab_id(hwnd, i) {
                                let _ = App::select_tab(app, id);
                                let _ = with_chrome(hwnd, |s| {
                                    s.pressed = Hit::Tab(i);
                                    s.drag = Some((id, i));
                                });
                                let _ = SetCapture(hwnd);
                                invalidate_hit(hwnd, Hit::Tab(i));
                            }
                        }
                    }
                    Hit::TabClose(i) => {
                        set_pressed(hwnd, Hit::TabClose(i));
                        let _ = with_chrome(hwnd, |s| s.drag = None);
                        let _ = SetCapture(hwnd);
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
                            let button =
                                with_chrome(hwnd, |s| s.layout.site_info_button()).unwrap_or_default();
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
                            let pane = if p.x < left.right { crate::split_view::Pane::Left } else { crate::split_view::Pane::Right };
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
                let was_split_drag = with_chrome(hwnd, |s| std::mem::take(&mut s.split_dragging)).unwrap_or(false);
                if was_split_drag {
                    let _ = ReleaseCapture();
                } else if let Some((id, from)) = with_chrome(hwnd, |s| s.drag.take()).flatten() {
                    let l = with_chrome(hwnd, |s| s.layout).unwrap();
                    if let Some(app) = app {
                        let to = layout::reorder_target(&l, from, p.x);
                        let model_to = slot_model_index(hwnd, to).unwrap_or(to);
                        let _ = App::move_tab(app, id, model_to);
                    }
                    set_pressed(hwnd, Hit::None);
                    let _ = ReleaseCapture();
                } else {
                    let was = with_chrome(hwnd, |s| s.pressed).unwrap_or(Hit::None);
                    set_pressed(hwnd, Hit::None);
                    let _ = ReleaseCapture();
                    if was == hit(hwnd, p.x, p.y) {
                        if let Hit::TabClose(i) = was {
                            if let Some(app) = app {
                                if let Some(id) = slot_tab_id(hwnd, i) {
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
                if hit(hwnd, p.x, p.y) == Hit::CaptionArea {
                    toggle_max(hwnd)
                }
                LRESULT(0)
            }
            WM_MBUTTONUP => {
                let p = frame::point_from_lparam(lparam);
                if let Hit::Tab(i) | Hit::TabClose(i) = hit(hwnd, p.x, p.y) {
                    card::cancel(hwnd);
                    if let Some(app) = app {
                        if let Some(id) = slot_tab_id(hwnd, i) {
                            let _ = App::close_tab(app, id);
                        }
                    }
                }
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                let p = frame::point_from_lparam(lparam);
                if let Hit::Tab(i) | Hit::TabClose(i) = hit(hwnd, p.x, p.y)
                    && let Some(app) = app
                    && let Some(id) = slot_tab_id(hwnd, i)
                {
                    card::cancel(hwnd);
                    show_tab_context_menu(hwnd, app, id, p.x, p.y);
                }
                LRESULT(0)
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
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
                    island_menu::timer(hwnd, wparam.0, app, |id| bar_anchor(hwnd, id));
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
            WM_TIMER if wparam.0 == WM_PROGRESS_TIMER => {
                let now = Instant::now();
                let (running, active_matches) = with_chrome(hwnd, |s| {
                    let active = app.map(App::active_tab_id);
                    (s.progress.running(now), active == s.progress.tab)
                }).unwrap_or((false, false));
                if active_matches { invalidate_omni(hwnd); }
                if !running || !active_matches {
                    let _ = KillTimer(Some(hwnd), WM_PROGRESS_TIMER);
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
                let _ = KillTimer(Some(hwnd), WM_SESSION_SAVE_TIMER);
                card::cancel(hwnd);
                if let Some(app) = app {
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
                let _ = KillTimer(Some(hwnd), WM_IDLE_TRIM_TIMER);
                let _ = KillTimer(Some(hwnd), WM_CARET_BLINK_TIMER);
                let _ = KillTimer(Some(hwnd), WM_ANIMATION_TIMER);
                let _ = KillTimer(Some(hwnd), WM_PROGRESS_TIMER);
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
        let hit = match layout::hit_test(&s.layout, x, y) {
            Hit::TabClose(index) if s.slots.get(index).is_some_and(|slot| slot.marker_only) => {
                Hit::Tab(index)
            }
            // A group's leading tab carries the marker box, which pushes its close target right.
            Hit::Tab(index) | Hit::TabClose(index) if leader_has_marker(s, index) => {
                let close = s
                    .layout
                    .tab(index)
                    .map(|island| s.layout.group_leader_parts(island).close);
                if close.is_some_and(|close| close.contains(x, y)) {
                    Hit::TabClose(index)
                } else {
                    Hit::Tab(index)
                }
            }
            hit => hit,
        };
        if hit == Hit::Viewport
            && s.split_active
            && layout::split_pane_layout(
                s.layout.viewport,
                s.split_divider,
                s.layout.metrics.px(220),
                s.layout.metrics.px(6),
            )
            .is_some_and(|panes| panes.divider.contains(x, y))
        {
            return Hit::SplitDivider;
        }
        hit
    }).unwrap_or(Hit::None)
}
/// The tab a strip hit lands on. Its close button counts as the tab itself for hover.
fn hit_tab(hwnd: HWND, h: Hit) -> Option<TabId> {
    match h {
        Hit::Tab(i) | Hit::TabClose(i) => slot_tab_id(hwnd, i),
        _ => None,
    }
}
/// A tab's visible rectangle in main-client coordinates, or `None` when it is scrolled out.
fn tab_anchor(hwnd: HWND, id: TabId) -> Option<Rect> {
    with_chrome(hwnd, |s| s.tab_rect(id)).flatten()
}
/// Whether the slot at `index` leads a group and so draws a marker box at its leading edge. Compact
/// islands have no room for one.
fn leader_has_marker(s: &WindowChrome, index: usize) -> bool {
    s.slots.get(index).is_some_and(|slot| {
        slot.group_control
            && !slot.marker_only
            && s.layout
                .tab(index)
                .is_some_and(|island| island.width() >= s.layout.metrics.px(Metrics::TITLE_MIN_TAB_WIDTH))
    })
}
/// A domain island's leading bar, found from the strip the chrome last laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
struct IslandBar {
    /// The island's leader, the tab whose slot carries the bar.
    tab: TabId,
    /// The bar in main-client coordinates.
    rect: Rect,
    collapsed: bool,
}
/// The island bar under a client point, if the point is on one. Only domain islands have bars that
/// open the menu or collapse; opener-only islands are not included.
fn island_bar_at(hwnd: HWND, x: i32, y: i32) -> Option<IslandBar> {
    with_chrome(hwnd, |s| {
        let m = Metrics::new(s.dpi);
        s.slots.iter().enumerate().find_map(|(index, slot)| {
            if !slot.group_control || slot.domain.is_none() {
                return None;
            }
            let bar = island_bar_rect(s, index, m)?;
            let hit = Rect::new(bar.left - m.px(3), bar.top, bar.right + m.px(3), bar.bottom);
            let tab = s.tabs.get(slot.tab_index).map(|tab| tab.id)?;
            hit.contains(x, y).then_some(IslandBar {
                tab,
                rect: hit,
                collapsed: slot.collapsed,
            })
        })
    })
    .flatten()
}
/// The island bar's rectangle for a tab, in main-client coordinates, or `None` when that tab does
/// not lead a visible domain island.
fn bar_anchor(hwnd: HWND, id: TabId) -> Option<Rect> {
    with_chrome(hwnd, |s| {
        let m = Metrics::new(s.dpi);
        s.slots.iter().enumerate().find_map(|(index, slot)| {
            if !slot.group_control || slot.domain.is_none() {
                return None;
            }
            if s.tabs.get(slot.tab_index).map(|tab| tab.id) != Some(id) {
                return None;
            }
            let bar = island_bar_rect(s, index, m)?;
            Some(Rect::new(bar.left - m.px(3), bar.top, bar.right + m.px(3), bar.bottom))
        })
    })
    .flatten()
}
/// The clickable marker for the visible slot at `index`, placed from its island with the same rules
/// the painter uses: the bar of a compact marker-only slot, or the marker box at the leading edge of
/// a group's leading tab. `None` for a compact island, where no marker is drawn.
fn island_bar_rect(s: &WindowChrome, index: usize, m: Metrics) -> Option<Rect> {
    let island = s.layout.tab(index)?;
    if s.slots.get(index).is_some_and(|slot| slot.marker_only) {
        return Some(paint::marker_only_bar(island, m));
    }
    leader_has_marker(s, index).then(|| paint::group_marker_box(island, m))
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
fn group_toggle_target(hwnd: HWND, slot_index: usize, x: i32, y: i32) -> Option<(TabId, bool)> {
    with_chrome(hwnd, |s| {
        let slot = s.slots.get(slot_index)?;
        if !slot.group_control {
            return None;
        }
        let tab = s.tabs.get(slot.tab_index)?;
        let island = s.layout.tab(slot_index)?;
        if slot.domain.is_none() {
            return None;
        }
        let favicon = if leader_has_marker(s, slot_index) {
            s.layout.group_leader_parts(island).favicon
        } else {
            s.layout.tab_parts(island).favicon
        };
        favicon.contains(x, y).then_some((tab.id, !slot.collapsed))
    })
    .flatten()
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

fn copy_text_to_clipboard(hwnd: HWND, text: &str) -> bool {
    use windows::Win32::{
        Foundation::{GlobalFree, HANDLE},
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
        },
    };

    let wide = text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
    unsafe {
        if OpenClipboard(Some(hwnd)).is_err() {
            return false;
        }
        if EmptyClipboard().is_err() {
            let _ = CloseClipboard();
            return false;
        }
        let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * std::mem::size_of::<u16>()) else {
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
        let _ = GlobalUnlock(memory);
        if SetClipboardData(13, Some(HANDLE(memory.0))).is_err() {
            let _ = GlobalFree(Some(memory));
            let _ = CloseClipboard();
            return false;
        }
        let _ = CloseClipboard();
        true
    }
}

fn split_rects(
    hwnd: HWND,
    divider_left: Option<i32>,
) -> Option<(RECT, RECT)> {
    let (layout, saved_divider) = with_chrome(hwnd, |state| (state.layout, state.split_divider))?;
    let divider = divider_left.unwrap_or(saved_divider);
    let panes = layout::split_pane_layout(
        layout.viewport,
        divider,
        layout.metrics.px(220),
        layout.metrics.px(6),
    )?;
    Some((
        RECT { left: panes.left.left, top: panes.left.top, right: panes.left.right, bottom: panes.left.bottom },
        RECT { left: panes.right.left, top: panes.right.top, right: panes.right.right, bottom: panes.right.bottom },
    ))
}

fn split_tab_rows(app: &AppRef, skip: Option<TabId>) -> Vec<Entry> {
    App::tab_ids(app)
        .into_iter()
        .filter(|id| Some(*id) != skip)
        .map(|id| {
            let title = App::tab_title(app, id).filter(|title| !title.trim().is_empty()).unwrap_or_else(|| "New tab".to_owned());
            let favicon = App::tab_favicon(app, id).and_then(|bytes| {
                crate::favicon::decode(&bytes, 24)
            });
            Entry::Item(Item::new(id.0 as usize + 100, title.clone(), Icon::Grid).rich_row(title, favicon))
        })
        .collect()
}

fn show_split_menu(hwnd: HWND, app: &AppRef) {
    const OPEN_BASE: usize = 100;
    const SWAP: usize = 1;
    const CLOSE_LEFT: usize = 2;
    const CLOSE_RIGHT: usize = 3;
    const REPLACE: usize = 4;
    const OPEN_NEW: usize = 5;
    let split = App::split_view(app);
    let entries = if let Some(split) = split {
        vec![
            Entry::Item(Item::new(SWAP, "Swap panes", Icon::Grid)),
            Entry::Item(Item::new(CLOSE_LEFT, "Close left pane", Icon::Close)),
            Entry::Item(Item::new(CLOSE_RIGHT, "Close right pane", Icon::Close)),
            Entry::Item(Item::new(REPLACE, "Replace pane with tab…", Icon::Grid)
                .submenu_with(split_tab_rows(app, Some(split.other_tab())))),
        ]
    } else {
        let active = App::active_tab_id(app);
        let mut entries = vec![Entry::Item(Item::new(OPEN_NEW, "Open split view with a new tab", Icon::Plus))];
        entries.push(Entry::Separator);
        entries.push(Entry::Item(Item::new(OPEN_BASE, "Open split view with…", Icon::Grid)
            .submenu_with(split_tab_rows(app, Some(active)))));
        entries
    };
    let mut point = main_menu_anchor(hwnd);
    unsafe { let _ = ClientToScreen(hwnd, &mut point); }
    let command = menu::track(hwnd, point, Menu {
        header: None,
        entries,
        footer: "Split view".to_owned(),
    }).map_or(0, |chosen| chosen.id);
    if let Some(split) = split {
        match command {
            SWAP => { let _ = App::swap_split_panes(app); }
            CLOSE_LEFT => { let _ = App::close_split_pane(app, crate::split_view::Pane::Left); }
            CLOSE_RIGHT => { let _ = App::close_split_pane(app, crate::split_view::Pane::Right); }
            value if value >= OPEN_BASE => { let _ = App::replace_focused_split_pane(app, TabId((value - OPEN_BASE) as u32)); }
            _ => {}
        }
        let _ = split;
    } else {
        let active = App::active_tab_id(app);
        if command == OPEN_NEW {
            let new = App::new_tab(app);
            if let Some(bounds) = split_rects(hwnd, None) {
                let _ = App::open_split_view(app, active, new, crate::split_view::Pane::Right, bounds);
            }
        } else if command >= OPEN_BASE {
            let other = TabId((command - OPEN_BASE) as u32);
            if let Some(bounds) = split_rects(hwnd, None) {
                let _ = App::open_split_view(app, active, other, crate::split_view::Pane::Right, bounds);
            }
        }
    }
    unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
}

fn show_tab_context_menu(hwnd: HWND, app: &AppRef, id: TabId, x: i32, y: i32) {
    const NEW_RIGHT: usize = 1;
    const TAB_SPLIT_VIEW_HOOK: usize = 2;
    const GROUP: usize = 3;
    const GROUP_NEW: usize = 30;
    const GROUP_AUTO: usize = 31;
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
    let host = url.as_ref().and_then(|address| address.host_str()).map(str::to_owned);
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
    let mute_site = site_ids.iter().any(|candidate| !App::tab_muted(app, *candidate));
    let can_mute_tab = App::tab_supports_muting(app, id);
    let can_mute_site = !site_ids.is_empty()
        && site_ids.iter().all(|candidate| App::tab_supports_muting(app, *candidate));
    let mut entries = vec![
        Entry::Item(Item::new(NEW_RIGHT, "New tab to the right", Icon::Plus)),
        Entry::Item(Item::new(TAB_SPLIT_VIEW_HOOK, "New split view with current tab", Icon::Grid)
            .disabled_if(App::tab_count(app) < 2 || App::split_view(app).is_some())),
        Entry::Separator,
    ];
    if let Some(key) = &key {
        entries.push(Entry::Item(
            Item::new(GROUP, "Add tab to group", Icon::Grid).submenu_with(vec![
                Entry::Item(Item::new(GROUP_NEW, "New group for this site", Icon::Plus)),
                Entry::Item(Item::new(GROUP_AUTO, "Group automatically", Icon::Grid)),
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
                if collapsed { "Expand group" } else { "Collapse group" },
                Icon::None,
            )
            .disabled_if(!has_group),
        ));
    }
    entries.push(Entry::Separator);
    entries.extend([
        Entry::Item(Item::new(COPY_URL, "Copy URL", Icon::None).disabled_if(url.is_none())),
        Entry::Item(Item::new(RELOAD, "Reload", Icon::None)),
        Entry::Item(Item::new(DUPLICATE, "Duplicate tab", Icon::Plus).disabled_if(url.is_none())),
        Entry::Item(Item::new(PIN, if pinned { "Unpin tab" } else { "Pin tab" }, Icon::None).checked(pinned)),
        Entry::Item(
            Item::new(MUTE_TAB, if muted { "Unmute tab" } else { "Mute tab" }, Icon::None)
                .checked(muted)
                .disabled_if(!can_mute_tab),
        ),
        Entry::Item(
            Item::new(MUTE_SITE, if mute_site { "Mute site" } else { "Unmute site" }, Icon::None)
                .disabled_if(!can_mute_site),
        ),
        Entry::Item(Item::new(HIBERNATE, "Hibernate tab", Icon::None).disabled_if(!can_hibernate)),
        Entry::Item(
            Item::new(MOVE_WINDOW, "Move tab to new window", Icon::None)
                .disabled_if(url.is_none()),
        ),
    ]);
    entries.push(Entry::Separator);
    entries.extend([
        Entry::Item(Item::new(CLOSE, "Close tab", Icon::Close)),
        Entry::Item(Item::new(CLOSE_OTHERS, "Close other tabs", Icon::Close).disabled_if(ids.len() <= 1)),
        Entry::Item(Item::new(CLOSE_LEFT, "Close tabs to the left", Icon::Close).disabled_if(index == 0)),
        Entry::Item(Item::new(CLOSE_RIGHT, "Close tabs to the right", Icon::Close).disabled_if(index + 1 == ids.len())),
    ]);
    let mut point = windows::Win32::Foundation::POINT { x, y };
    unsafe {
        let _ = ClientToScreen(hwnd, &mut point);
        let command = menu::track_below(
            hwnd,
            point,
            Menu {
                header: None,
                entries,
                footer: "Actions for this tab".to_owned(),
            },
        )
        .map_or(0, |chosen| chosen.id);
        match command {
            NEW_RIGHT => {
                let created = App::new_tab(app);
                let current = App::tab_ids(app);
                let after = if pinned {
                    current.iter().take_while(|candidate| App::tab_pinned(app, **candidate)).count()
                } else {
                    current.iter().position(|candidate| *candidate == id).unwrap_or(index) + 1
                };
                let _ = App::move_tab(app, created, after);
                focus_omni(hwnd, app, true);
            }
            TAB_SPLIT_VIEW_HOOK => {
                let active = App::active_tab_id(app);
                let companion = if id != active {
                    Some(active)
                } else {
                    App::tab_ids(app).into_iter().find(|candidate| *candidate != id)
                };
                if let (Some(companion), Some(bounds)) = (companion, split_rects(hwnd, None)) {
                    let _ = App::open_split_view(app, id, companion, crate::split_view::Pane::Left, bounds);
                }
            }
            GROUP_NEW => {
                let _ = App::set_tab_group_mode(app, id, tab_groups::GroupMode::Always);
            }
            GROUP_AUTO => {
                let _ = App::set_tab_group_mode(app, id, tab_groups::GroupMode::Auto);
            }
            value if value == GROUP_AUTO + 1 && has_group => {
                let _ = App::set_tab_group_collapsed(app, id, !collapsed);
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
                    let after = current.iter().position(|candidate| *candidate == id).unwrap_or(index) + 1;
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
                    let launched = profile.is_ok_and(|profile| {
                        crate::profile::spawn_window_with_url(&profile, url)
                    });
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
    }
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
            show_profile_notice(hwnd, "Bowser could not find this user's LocalAppData directory.");
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
                .profile_badge(metadata.avatar_glyph(name), metadata.accent_colour())
                .checked(self.is_current(name)),
            ));
        }
        entries.push(Entry::Separator);
        entries.push(Entry::Item(Item::new(PROFILE_GUEST, "Guest window", Icon::Person)));
        entries.push(Entry::Item(Item::new(
            PROFILE_NEW,
            "New profile",
            Icon::Plus,
        )));
        entries.push(Entry::Item(Item::new(20, "Choose avatar", Icon::Person).submenu_with(
            ["initial", "leaf", "star", "wave", "moon"].iter().enumerate().map(|(index, id)| {
                Entry::Item(Item::new(PROFILE_AVATAR_BASE + index, avatar_label(id), Icon::Person)
                    .profile_badge(avatar_label(id), self.current_metadata.accent_colour())
                    .checked(self.current_metadata.avatar == *id))
            }).collect(),
        )));
        entries.push(Entry::Item(Item::new(21, "Choose accent colour", Icon::Grid).submenu_with(
            ["blue", "violet", "teal", "amber", "rose"].iter().enumerate().map(|(index, id)| {
                let mut metadata = self.current_metadata.clone();
                metadata.accent = (*id).to_owned();
                Entry::Item(Item::new(PROFILE_ACCENT_BASE + index, accent_label(id), Icon::Grid)
                    .profile_badge(metadata.avatar_glyph(self.current.listed_name()), metadata.accent_colour())
                    .checked(self.current_metadata.accent == *id))
            }).collect(),
        )));
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
                )));
            }
        }
        let renamable: Vec<usize> = self.names.iter().enumerate()
            .filter(|(_, name)| !name.eq_ignore_ascii_case(profile::DEFAULT_NAME) && !self.is_current(name))
            .map(|(index, _)| index).collect();
        if !renamable.is_empty() {
            entries.push(Entry::SectionHeader("Rename profile".to_owned()));
            for index in renamable {
                entries.push(Entry::Item(Item::new(PROFILE_RENAME_BASE + index,
                    Self::label(&self.names[index]).to_string(), Icon::Person)));
            }
        }
        entries
    }

    /// Runs the command a row of [`Self::append_to`] returned. Other ids are ignored.
    fn run(&self, command: usize, hwnd: HWND) {
        use crate::profile::{self, Profile};
        match command {
            id if (PROFILE_RENAME_BASE..PROFILE_RENAME_BASE + PROFILE_MAX_LISTED).contains(&id) => {
                if let Some(name) = self.names.get(id - PROFILE_RENAME_BASE)
                    .filter(|name| !self.is_current(name))
                    && let Some(new_name) = super::profile_rename::prompt(hwnd, name)
                    && let Err(error) = profile::rename(&self.app_data, name, &new_name, &self.current)
                { show_profile_notice(hwnd, &format!("The profile could not be renamed. {error}")); }
            }
            PROFILE_GUEST => {
                if !profile::spawn_guest_window() { show_profile_notice(hwnd, "Bowser could not start a guest window."); }
            }
            PROFILE_NEW => match profile::create_auto(&self.app_data) {
                Ok(created) => open_profile_window(&created, hwnd),
                Err(error) => show_profile_notice(hwnd, &format!("The new profile was not created. {error}")),
            },
            id if (PROFILE_SWITCH_BASE..PROFILE_SWITCH_BASE + PROFILE_MAX_LISTED).contains(&id) => {
                if let Some(name) = self
                    .names
                    .get(id - PROFILE_SWITCH_BASE)
                    .filter(|name| !self.is_current(name))
                {
                    match Profile::for_listed_name(&self.app_data, name) {
                        Ok(target) => {
                            if let Err(error) = profile::remember_last_profile(&self.app_data, name) {
                                show_profile_notice(hwnd, &error.to_string());
                            } else { open_profile_window(&target, hwnd); }
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
                    if let Err(error) = metadata.save(&self.current) { show_profile_notice(hwnd, &error.to_string()); }
                }
            }
            id if (PROFILE_ACCENT_BASE..PROFILE_ACCENT_BASE + 5).contains(&id) => {
                let choices = ["blue", "violet", "teal", "amber", "rose"];
                if let Some(accent) = choices.get(id - PROFILE_ACCENT_BASE) {
                    let mut metadata = self.current_metadata.clone();
                    metadata.accent = (*accent).to_owned();
                    if let Err(error) = metadata.save(&self.current) { show_profile_notice(hwnd, &error.to_string()); }
                }
            }
            id if (PROFILE_DELETE_BASE..PROFILE_DELETE_BASE + PROFILE_MAX_LISTED).contains(&id) => {
                if let Some(name) = self.names.get(id - PROFILE_DELETE_BASE) {
                    if confirm_profile_delete(hwnd, name) {
                        if let Err(error) = profile::delete(&self.app_data, name, &self.current) {
                            show_profile_notice(hwnd, &format!("The profile \"{name}\" was not deleted. {error}"));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn avatar_label(id: &str) -> &'static str {
    match id { "leaf" => "Leaf", "star" => "Star", "wave" => "Wave", "moon" => "Moon", _ => "Initial" }
}

fn accent_label(id: &str) -> &'static str {
    match id { "violet" => "Violet", "teal" => "Teal", "amber" => "Amber", "rose" => "Rose", _ => "Blue" }
}

fn show_profile_notice(hwnd: HWND, message: &str) {
    let mut point = main_menu_anchor(hwnd);
    unsafe { let _ = ClientToScreen(hwnd, &mut point); }
    let _ = menu::track(hwnd, point, Menu {
        header: None,
        entries: vec![
            Entry::SectionHeader(message.to_owned()),
            Entry::Item(Item::new(1, "Close", Icon::Close)),
        ],
        footer: "Profile action".to_owned(),
    });
}

fn confirm_profile_delete(hwnd: HWND, name: &str) -> bool {
    let mut point = main_menu_anchor(hwnd);
    unsafe { let _ = ClientToScreen(hwnd, &mut point); }
    menu::track(hwnd, point, Menu {
        header: None,
        entries: vec![
            Entry::SectionHeader(format!("Delete profile {name}?")),
            Entry::Item(Item::new(2, "Delete profile", Icon::Close)),
            Entry::Item(Item::new(3, "Keep profile", Icon::Person)),
        ],
        footer: "Cookies, saved passwords, settings and session data will be removed".to_owned(),
    }).is_some_and(|chosen| chosen.id == 2)
}

fn show_profile_menu(hwnd: HWND) {
    let Some(profiles) = ProfileMenu::load(hwnd) else {
        return;
    };
    let mut point = main_menu_anchor(hwnd);
    unsafe {
        let _ = ClientToScreen(hwnd, &mut point);
    };
    let menu = Menu {
        header: None,
        entries: profiles.entries(),
        footer: "Switch, create, rename, or delete".to_owned(),
    };
    if let Some(chosen) = menu::track(hwnd, point, menu) {
        profiles.run(chosen.id, hwnd);
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

    let profiles = ProfileMenu::load(hwnd);
    let mut entries = vec![
        Entry::Item(Item::new(NEW_TAB, "New tab", Icon::Plus).shortcut("Ctrl+T")),
        Entry::Separator,
    ];
    if let Some(profiles) = &profiles {
        let name = ProfileMenu::label(profiles.current.listed_name()).to_string();
        entries.push(Entry::Item(
            Item::new(PROFILE, format!("Profile: {name}"), Icon::Person)
                .submenu_with(profiles.entries()),
        ));
    }
    entries.push(Entry::Item(
        Item::new(EXTENSIONS, "Extensions…", Icon::Grid).shortcut("Ctrl+Shift+E"),
    ));
    entries.push(Entry::Item(
        Item::new(PASSWORDS, "Passwords & passkeys", Icon::Key)
            .shortcut("Ctrl+,")
            .disabled_if(crate::profile::active().is_ok_and(|profile| profile.is_guest())),
    ));
    entries.push(Entry::Separator);
    let blocker = Item::new(BLOCKER, "Block ads & trackers", Icon::Shield)
        .shortcut("Ctrl+Shift+B")
        .described("Block ads & trackers on this site (Ctrl+Shift+B)");
    entries.push(Entry::Item(match App::blocker_active_for_site(app) {
        Some(active) => blocker.checked(active),
        None => blocker.disabled(),
    }));
    entries.push(Entry::Separator);
    entries.push(Entry::Item(Item::new(SETTINGS, "Settings…", Icon::Gear)));
    entries.push(Entry::Separator);
    entries.push(Entry::Item(
        Item::new(EXIT, "Exit", Icon::Close).shortcut("Alt+F4"),
    ));
    let menu = Menu {
        header: Some(ProfileHeader {
            profile: profiles
                .as_ref()
                .map_or_else(|| "Bowser".to_owned(), |p| ProfileMenu::label(p.current.listed_name()).to_string()),
            caption: "Current profile".to_owned(),
            avatar: profiles.as_ref().map_or_else(|| "B".to_owned(), |p| p.current_metadata.avatar_glyph(p.current.listed_name())),
            accent: profiles.as_ref().map_or_else(|| crate::profile::ProfileMetadata::default().accent_colour(), |p| p.current_metadata.accent_colour()),
        }),
        entries,
        footer: format!("Bowser {}", env!("CARGO_PKG_VERSION")),
    };

    // Right-aligned under the button, so the menu grows leftwards and stays on screen.
    let mut point = main_menu_anchor(hwnd);
    set_pressed(hwnd, Hit::MainMenu);
    unsafe {
        let _ = ClientToScreen(hwnd, &mut point);
    }
    let chosen = menu::track(hwnd, point, menu);
    set_pressed(hwnd, Hit::None);

    if let Some(chosen) = chosen {
        match chosen.id {
            NEW_TAB => {
                App::new_tab(app);
                focus_omni(hwnd, app, true);
            }
            PROFILE => {}
            EXTENSIONS => show_extensions_menu(hwnd, app),
            PASSWORDS => {
                if let Err(error) = passwords_panel::open(hwnd) {
                    show_error_message(&format!(
                        "Passwords & Passkeys could not be opened: {error}"
                    ));
                }
            }
            BLOCKER => App::toggle_blocker(app),
            SETTINGS => {
                if let Err(error) = settings_panel::open(hwnd) {
                    show_error_message(&format!("Settings could not be opened: {error}"));
                }
            }
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
                || (PROFILE_RENAME_BASE..PROFILE_RENAME_BASE + PROFILE_MAX_LISTED).contains(&id)
                || id == PROFILE_GUEST => {
                    if let Some(profiles) = &profiles { profiles.run(id, hwnd); }
                }
            PROFILE_NEW => {
                if let Some(profiles) = &profiles {
                    profiles.run(PROFILE_NEW, hwnd);
                }
            }
            _ => {}
        }
    }
    let _ = with_chrome(hwnd, |s| s.refresh(app, true));
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
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
                .disabled_if(!maximized && !minimized),
        ),
        Entry::Item(
            Item::new(SC_MOVE as usize, "Move", Icon::None).disabled_if(maximized || minimized),
        ),
        Entry::Item(
            Item::new(SC_SIZE as usize, "Size", Icon::None).disabled_if(maximized || minimized),
        ),
        Entry::Item(
            Item::new(SC_MINIMIZE as usize, "Minimize", Icon::None).disabled_if(minimized),
        ),
        Entry::Item(
            Item::new(SC_MAXIMIZE as usize, "Maximize", Icon::None).disabled_if(maximized),
        ),
        Entry::Separator,
        Entry::Item(Item::new(SC_CLOSE as usize, "Close", Icon::Close).shortcut("Alt+F4")),
    ];
    let chosen = menu::track_below(
        hwnd,
        point,
        Menu {
            header: None,
            entries,
            footer: "Window commands".to_owned(),
        },
    );
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
}

/// Ctrl+Shift+E: themed menu for this profile's unpacked extensions.
fn show_extensions_menu(hwnd: HWND, app: &AppRef) {
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
            Item::new(0, "No extensions installed", Icon::None).disabled(),
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
            Item::new(TOGGLE_BASE + index, label, Icon::Grid).checked(record.enabled),
        ));
    }
    let remove_entries: Vec<Entry> = records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            Entry::Item(Item::new(
                REMOVE_BASE + index,
                record.name.clone(),
                Icon::Close,
            ))
        })
        .collect();
    entries.push(Entry::Separator);
    let remove = Item::new(REMOVE_MENU, "Remove extension…", Icon::Close);
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
    )));

    let mut point = main_menu_anchor(hwnd);
    unsafe {
        let _ = ClientToScreen(hwnd, &mut point);
    };
    let command = menu::track(
        hwnd,
        point,
        Menu {
            header: None,
            entries,
            footer: "Unpacked extensions".to_owned(),
        },
    )
    .map_or(0, |chosen| chosen.id);

    match command {
        INSTALL => install_extension(hwnd, app, &profile),
        id if (TOGGLE_BASE..TOGGLE_BASE + MAX_LISTED).contains(&id) => {
            if let Some(record) = records.get(id - TOGGLE_BASE) {
                toggle_extension(app, &profile, record);
            }
        }
        id if (REMOVE_BASE..REMOVE_BASE + MAX_LISTED).contains(&id) => {
            if let Some(record) = records.get(id - REMOVE_BASE) {
                remove_extension(app, &profile, record);
            }
        }
        _ => {}
    }
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
            Item::new(index + 1, label.clone(), if id == active { Icon::Check } else { Icon::None })
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
    let command = menu::track_below(
        hwnd,
        point,
        Menu {
            header: None,
            entries,
            footer: "Select a tab".to_owned(),
        },
    )
    .map_or(0, |chosen| chosen.id);
    {
        if let Some(id) = command
            .checked_sub(1)
            .and_then(|index| ids.get(index))
            .copied()
        {
            let _ = App::select_tab(app, id);
        }
    }
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
        Hit::Tab(i) | Hit::TabClose(i) => l.tab(i).unwrap_or_default(),
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
        Hit::Tab(i) | Hit::TabClose(i) => {
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
        (Hit::Tab(_) | Hit::TabClose(_), None) => Hit::None,
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
    let (old, l) = with_chrome(hwnd, |s| {
        let o = s.hover;
        s.hover = h;
        s.retarget_motion(Instant::now());
        (o, s.layout)
    })
    .unwrap_or_else(|| (Hit::None, layout::compute(0, 0, 96, 0, false)));
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
        (o, s.layout)
    })
    .unwrap_or_else(|| (Hit::None, layout::compute(0, 0, 96, 0, false)));
    if old != h {
        invalidate(hwnd, rect_for(&l, old));
        invalidate(hwnd, rect_for(&l, h));
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
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        if let Some(app) = app {
            let active = App::active_tab_id(app);
            let split_view = App::split_view(app);
            let active_has_controller = split_view.map_or_else(
                || App::active_tab_has_controller(app),
                |split| App::tab_has_controller(app, split.left) || App::tab_has_controller(app, split.right),
            );
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
                    let v = TabView {
                        title: &t.title,
                        loading: t.loading,
                        favicon: t.favicon.as_deref(),
                        emphasis: s
                            .motion
                            .emphasis(t.id)
                            .unwrap_or_else(|| Emphasis::resting(false, false, now)),
                        // A marker-only slot is not a member island, so it joins no group pill.
                        group_size: (!slot.marker_only)
                            .then(|| slot.group_key.as_ref().map(|_| slot.group_size))
                            .flatten(),
                        group_control: slot.group_control && !slot.marker_only,
                        marker: t.marker,
                        group_colour: slot.island_colour,
                        marker_only: slot.marker_only,
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
                if progress_target.is_none() {
                    s.progress.clear();
                }
                let progress = match (progress_target, s.progress.tab) {
                    (Some(target), Some(tab)) if tab == active => s.progress.paint_value(now).or(Some(target as f32)),
                    (Some(target), _) => Some(target as f32),
                    (None, _) => None,
                };
                let view = ChromeView {
                    layout: l,
                    palette: p,
                    tabs: views,
                    active: active_i,
                    hover: s.hover,
                    pressed: s.pressed,
                    caption_maximised: IsZoomed(hwnd).as_bool(),
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
                };
                let page_regions = if active_has_controller {
                    if let Some(split) = split_view {
                        layout::split_pane_layout(
                            s.layout.viewport,
                            s.split_divider,
                            s.layout.metrics.px(220),
                            s.layout.metrics.px(6),
                        )
                        .map(|panes| (s.layout.viewport, Some(panes), split.focused == crate::split_view::Pane::Left))
                    } else {
                        Some((s.layout.viewport, None, false))
                    }
                } else {
                    None
                };
                s.surface.set_page_regions(page_regions);
                let t0 = std::time::Instant::now();
                s.surface.paint(&view, dirty);
                let t1 = std::time::Instant::now();
                let mut origin = windows::Win32::Foundation::POINT { x: 0, y: 0 };
                let _ = ClientToScreen(hwnd, &mut origin);
                s.surface.present(s.overlay, origin, dirty);
                let t2 = std::time::Instant::now();
                if !s.first_paint_logged {
                    s.first_paint_logged = true;
                    let _ = app.borrow().log.record("chrome_painted");
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
                && with_chrome(hwnd, |s| s.progress.tab == Some(active) && s.progress.settled(now))
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
        Command::OpenPasswords => {
            if let Err(error) = passwords_panel::open(hwnd) {
                show_error_message(&format!(
                    "Passwords & Passkeys could not be opened: {error}"
                ));
            }
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
    std::thread::spawn(move || {
        if profile.history_enabled() {
            let _ = crate::productivity::add_history_entry(&profile.history_file(), &url, &title);
        }
    });
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
        assert!(progress.paint_value(start + Duration::from_millis(300)).unwrap() > 100.0);
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
            s.relayout(1000, 700, dpi, TabId(0), false);
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
