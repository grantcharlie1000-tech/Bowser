use super::{
    INPUT_ID, WM_ANIMATION_TIMER, WM_CARET_BLINK_TIMER, WM_FOREGROUND_SYNC, WM_HISTORY_BACK,
    WM_HISTORY_FORWARD, WM_IDLE_TRIM_TIMER, WM_NAVIGATION_PROGRESS, WM_SESSION_SAVE_TIMER,
    WM_SHORTCUT_KEY, WM_SUBMIT, WM_TABS_CHANGED, frame, glass, passwords_panel, show_error_message,
};
use crate::{
    app::{App, AppRef, IdleTimerAction},
    chrome::{
        layout::{self, Hit, Layout, Metrics, Rect},
        motion::{Emphasis, Motion},
        paint::{self, ChromeSurface, ChromeView, OmniboxView, TabView},
    },
    favicon::{self, FaviconCache},
    omnibox::{self, Completion, History},
    shortcuts::{self, Command},
    tab_groups::{self, StripSlot, TabGroupInput},
    tabs::{FaviconKey, TabId},
    theme::Palette,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Instant};
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
                AppendMenuW, CREATESTRUCTW, CS_DBLCLKS, CW_USEDEFAULT, CreatePopupMenu,
                CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW,
                EN_CHANGE, ES_AUTOHSCROLL, EVENT_SYSTEM_FOREGROUND, GW_OWNER, GWL_STYLE,
                GWLP_USERDATA, GetCaretBlinkTime, GetClientRect, GetCursorPos, GetForegroundWindow,
                GetMessageW, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
                GetWindowTextW, GetWindowThreadProcessId, HMENU, HTBOTTOM, HTBOTTOMLEFT,
                HTBOTTOMRIGHT, HTCAPTION, HTCLOSE, HTLEFT, HTMAXBUTTON, HTMINBUTTON, HTRIGHT,
                HTTOP, HTTOPLEFT, HTTOPRIGHT, HideCaret, IsIconic, IsZoomed, KillTimer,
                LoadCursorW, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG,
                MoveWindow, NCCALCSIZE_PARAMS, PostQuitMessage, RegisterClassW, SC_CLOSE,
                SC_MAXIMIZE, SC_MINIMIZE, SC_RESTORE, SIZE_MINIMIZED, SM_CXFRAME,
                SM_CXPADDEDBORDER, SM_CYFRAME, SPI_GETCLIENTAREAANIMATION, SW_SHOW, SWP_NOACTIVATE,
                SWP_NOSIZE, SWP_NOZORDER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SendMessageW,
                SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
                SystemParametersInfoW, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
                TranslateMessage, WINDOW_STYLE, WINEVENT_OUTOFCONTEXT, WM_CHAR, WM_CLOSE,
                WM_COMMAND, WM_CREATE, WM_DESTROY, WM_DPICHANGED, WM_DWMCOLORIZATIONCOLORCHANGED,
                WM_ERASEBKGND, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
                WM_LBUTTONUP, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOVE, WM_NCACTIVATE, WM_NCCALCSIZE,
                WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN,
                WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_NCPAINT, WM_PAINT, WM_PASTE,
                WM_RBUTTONUP, WM_SETFOCUS, WM_SETICON, WM_SETTEXT, WM_SETTINGCHANGE, WM_SIZE,
                WM_SYSCOMMAND, WM_SYSKEYDOWN, WM_THEMECHANGED, WM_TIMER, WNDCLASSW, WS_CAPTION,
                WS_CHILD, WS_CLIPCHILDREN, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
                WS_EX_TRANSPARENT, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU,
                WS_VISIBLE, WVR_REDRAW,
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
    favicon: Option<favicon::FaviconBitmap>,
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
    failed: Vec<FaviconKey>,
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
    first_paint_logged: bool,
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
            failed: Vec::new(),
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
            first_paint_logged: false,
        }
    }
    fn refresh(&mut self, app: &AppRef) {
        let now = Instant::now();
        let ids = App::tab_ids(app);
        let active = App::active_tab_id(app);
        let preferences = App::tab_group_preferences(app);
        let mut tabs = Vec::with_capacity(ids.len());
        let mut group_inputs = Vec::with_capacity(ids.len());
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
            let domain = App::tab_committed_url(app, id)
                .as_ref()
                .and_then(tab_groups::DomainKey::from_url);
            group_inputs.push(TabGroupInput { domain, pinned });
            let bytes = App::tab_favicon(app, id);
            let key = bytes.as_deref().map(favicon::key_for);
            let image = match (key, bytes.as_deref(), old) {
                (Some(k), _, Some(t)) if t.favicon_key == Some(k) => t.favicon.clone(),
                (Some(k), Some(b), _) => {
                    if let Some(x) = self.favicons.get(k, b) {
                        Some(x.clone())
                    } else if self.failed.contains(&k) {
                        None
                    } else {
                        let size = Metrics::new(self.dpi).px(Metrics::FAVICON_SIZE).max(1);
                        let d = favicon::decode(b, size as u32);
                        if let Some(x) = &d {
                            self.favicons.insert(k, b, x.clone())
                        } else {
                            self.failed.push(k)
                        }
                        d
                    }
                }
                _ => None,
            };
            if old.is_some_and(|t| t.loading) && !loading {
                if let Some(url) = self.pending_history.remove(&id) {
                    self.history.record(&url, &title)
                }
            }
            tabs.push(TabSnapshot {
                id,
                title,
                loading,
                favicon_key: key,
                favicon: image,
            });
        }
        let active_index = ids.iter().position(|&id| id == active).unwrap_or(0);
        let slots = tab_groups::project_strip(&group_inputs, active_index, &preferences);
        // Still the old tabs and layout here, so a tab that is leaving keeps its island to fade from.
        let hovered = self.hovered_tab();
        {
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
        self.relayout(
            self.client.0,
            self.client.1,
            self.dpi,
            App::active_tab_id(app),
            self.layout.maximised,
        );
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
            layout::compute_with_active(w, h, self.dpi, self.slots.len().max(1), i, maximised);
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
#[derive(Clone, Copy)]
struct ResizeEdges {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}
struct ResizeState {
    edges: ResizeEdges,
    start_cursor: windows::Win32::Foundation::POINT,
    start_rect: RECT,
    min_width: i32,
    min_height: i32,
}
thread_local! {static RESIZING:RefCell<HashMap<isize,ResizeState>>=RefCell::new(HashMap::new());}

fn resize_edges(hit: usize) -> Option<ResizeEdges> {
    let (left, right, top, bottom) = match hit as u32 {
        x if x == HTLEFT as u32 => (true, false, false, false),
        x if x == HTRIGHT as u32 => (false, true, false, false),
        x if x == HTTOP as u32 => (false, false, true, false),
        x if x == HTBOTTOM as u32 => (false, false, false, true),
        x if x == HTTOPLEFT as u32 => (true, false, true, false),
        x if x == HTTOPRIGHT as u32 => (false, true, true, false),
        x if x == HTBOTTOMLEFT as u32 => (true, false, false, true),
        x if x == HTBOTTOMRIGHT as u32 => (false, true, false, true),
        _ => return None,
    };
    Some(ResizeEdges {
        left,
        right,
        top,
        bottom,
    })
}

fn begin_resize(hwnd: HWND, hit: usize) -> bool {
    let Some(edges) = resize_edges(hit) else {
        return false;
    };
    if unsafe { IsZoomed(hwnd).as_bool() } {
        return false;
    }
    let mut start_rect = RECT::default();
    let mut start_cursor = windows::Win32::Foundation::POINT::default();
    if unsafe { GetWindowRect(hwnd, &mut start_rect) }.is_err()
        || unsafe { GetCursorPos(&mut start_cursor) }.is_err()
    {
        return false;
    }
    let metrics = Metrics::new(dpi(hwnd));
    let min_width = metrics.px(520);
    let min_height = metrics.px(300);
    unsafe {
        let _ = SetCapture(hwnd);
    }
    if unsafe { GetCapture() } != hwnd {
        return false;
    }
    RESIZING.with(|states| {
        states.borrow_mut().insert(
            hwnd.0 as isize,
            ResizeState {
                edges,
                start_cursor,
                start_rect,
                min_width,
                min_height,
            },
        );
    });
    true
}

fn resize_window(hwnd: HWND) {
    let state = RESIZING.with(|states| {
        states.borrow().get(&(hwnd.0 as isize)).map(|state| {
            (
                state.edges,
                state.start_cursor,
                state.start_rect,
                state.min_width,
                state.min_height,
            )
        })
    });
    let Some((edges, start_cursor, start_rect, min_width, min_height)) = state else {
        return;
    };
    let mut cursor = windows::Win32::Foundation::POINT::default();
    if unsafe { GetCursorPos(&mut cursor) }.is_err() {
        return;
    }
    let dx = cursor.x - start_cursor.x;
    let dy = cursor.y - start_cursor.y;
    let mut rect = start_rect;
    if edges.left {
        rect.left = (start_rect.left + dx).min(start_rect.right - min_width);
    }
    if edges.right {
        rect.right = (start_rect.right + dx).max(start_rect.left + min_width);
    }
    if edges.top {
        rect.top = (start_rect.top + dy).min(start_rect.bottom - min_height);
    }
    if edges.bottom {
        rect.bottom = (start_rect.bottom + dy).max(start_rect.top + min_height);
    }
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
}

fn finish_resize(hwnd: HWND) -> bool {
    RESIZING.with(|states| states.borrow_mut().remove(&(hwnd.0 as isize)).is_some())
}
fn is_resizing(hwnd: HWND) -> bool {
    RESIZING.with(|states| states.borrow().contains_key(&(hwnd.0 as isize)))
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
        let had_state = s.drag.take().is_some() || s.pressed != Hit::None;
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
            // The system sizing frame also forces WS_EX_WINDOWEDGE on this HWND. Bowser
            // handles the resize loop itself so its rounded region can stay edge-free.
            WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_CLIPCHILDREN,
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
                    p.rgrc[0] = frame::client_rect_for_nccalc(p.rgrc[0], mx, d, (fx, fy));
                } else {
                    let r = &mut *(lparam.0 as *mut RECT);
                    *r = frame::client_rect_for_nccalc(*r, mx, d, (fx, fy));
                }
                LRESULT(if wparam.0 != 0 {
                    WVR_REDRAW as isize
                } else {
                    0
                })
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
                        | Hit::Back
                        | Hit::Forward
                        | Hit::Reload
                        | Hit::Omnibox
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
            WM_NCLBUTTONDOWN if begin_resize(hwnd, wparam.0) => LRESULT(0),
            // Only the caption buttons are ours. Dragging the strip and the resize edges need the
            // default handler, which runs the system move loop. Resize edges are handled above.
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
                        layout::chrome_height(dpi(hwnd)),
                        Some(hwnd),
                        None,
                        Some(instance),
                        None,
                    )
                    .unwrap_or_default();
                    let mut s = WindowChrome::new(r.right, r.bottom, dpi(hwnd), overlay);
                    s.edit = edit;
                    s.refresh(app);
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
                LRESULT(0)
            }
            WM_SIZE => {
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
                        s.refresh(app)
                    });
                    refresh_edit_font(hwnd, app);
                    position_edit(hwnd);
                    let l = with_chrome(hwnd, |s| s.layout).unwrap();
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
            WM_COMMAND
                if (wparam.0 & 0xffff) as u16 == INPUT_ID
                    && (wparam.0 >> 16) as u32 == EN_CHANGE =>
            {
                if let Some(app) = app {
                    let text = read_edit(app.borrow().edit);
                    let prewarm = with_chrome(hwnd, |s| {
                        s.edit_text.clear();
                        s.edit_text.push_str(&text);
                        s.update_completion();
                        !s.programmatic_text_update
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
                    App::history(app, false)
                }
                LRESULT(0)
            }
            WM_HISTORY_FORWARD => {
                if let Some(app) = app {
                    App::history(app, true)
                }
                LRESULT(0)
            }
            WM_NAVIGATION_PROGRESS => {
                if let Some(app) = app {
                    if App::navigation_progress_message(app, wparam.0 as u64, lparam.0) {
                        invalidate_omni(hwnd)
                    }
                }
                LRESULT(0)
            }
            WM_TABS_CHANGED => {
                if let Some(app) = app {
                    let _ = with_chrome(hwnd, |s| s.refresh(app));
                    position_edit(hwnd);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    arm_motion(hwnd);
                    // Debounced crash-recovery snapshot: one timer, armed only when dirty.
                    if App::session_changed(app) {
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
                if is_resizing(hwnd) {
                    resize_window(hwnd);
                    return LRESULT(0);
                }
                if GetCapture() == hwnd {
                } else {
                    let p = frame::point_from_lparam(lparam);
                    let h = hit(hwnd, p.x, p.y);
                    set_hover(hwnd, h);
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
                LRESULT(0)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_CAPTURECHANGED => {
                finish_resize(hwnd);
                if clear_pointer_state(hwnd) {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_CANCELMODE => {
                let was_resizing = finish_resize(hwnd);
                let had_pointer_state = clear_pointer_state(hwnd);
                if was_resizing || had_pointer_state {
                    if GetCapture() == hwnd {
                        let _ = ReleaseCapture();
                    }
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_LBUTTONDOWN => {
                let p = frame::point_from_lparam(lparam);
                match hit(hwnd, p.x, p.y) {
                    Hit::Tab(i) => {
                        if let Some((id, collapsed)) = group_toggle_target(hwnd, i, p.x, p.y) {
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
                if finish_resize(hwnd) {
                    let _ = ReleaseCapture();
                    return LRESULT(0);
                }
                let p = frame::point_from_lparam(lparam);
                if let Some((id, from)) = with_chrome(hwnd, |s| s.drag.take()).flatten() {
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
                                Hit::Back => App::history(app, false),
                                Hit::Forward => App::history(app, true),
                                _ => App::reload(app),
                            }
                            let _ = with_chrome(hwnd, |s| s.refresh(app));
                            let _ = InvalidateRect(Some(hwnd), None, false);
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
                    show_tab_context_menu(hwnd, app, id, p.x, p.y);
                }
                LRESULT(0)
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
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
                let _ = KillTimer(Some(hwnd), WM_SESSION_SAVE_TIMER);
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
                finish_resize(hwnd);
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
    with_chrome(hwnd, |s| layout::hit_test(&s.layout, x, y)).unwrap_or(Hit::None)
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
        s.layout
            .tab_parts(island)
            .favicon
            .contains(x, y)
            .then_some((tab.id, !slot.collapsed))
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
fn show_tab_context_menu(hwnd: HWND, app: &AppRef, id: TabId, x: i32, y: i32) {
    const PIN: usize = 1;
    const AUTO: usize = 2;
    const ALWAYS: usize = 3;
    const NEVER: usize = 4;
    const COLLAPSE: usize = 5;

    let menu = unsafe { CreatePopupMenu() };
    let Ok(menu) = menu else { return };
    let pinned = App::tab_pinned(app, id);
    let _ = unsafe {
        AppendMenuW(
            menu,
            MF_STRING,
            PIN,
            if pinned {
                w!("Unpin tab")
            } else {
                w!("Pin tab")
            },
        )
    };
    let url = App::tab_committed_url(app, id);
    let key = url.as_ref().and_then(tab_groups::DomainKey::from_url);
    let mut has_group = false;
    let mut collapsed = false;
    if let Some(key) = &key {
        let _ = unsafe { AppendMenuW(menu, MF_STRING, AUTO, w!("Group this site automatically")) };
        let _ = unsafe { AppendMenuW(menu, MF_STRING, ALWAYS, w!("Always group this site")) };
        let _ = unsafe { AppendMenuW(menu, MF_STRING, NEVER, w!("Never group this site")) };
        (has_group, collapsed) = with_chrome(hwnd, |state| {
            state
                .slots
                .iter()
                .find(|slot| slot.domain.as_ref() == Some(key))
                .map_or((false, false), |slot| (true, slot.collapsed))
        })
        .unwrap_or((false, false));
        if has_group {
            let _ = unsafe {
                AppendMenuW(
                    menu,
                    MF_STRING,
                    COLLAPSE,
                    if collapsed {
                        w!("Expand group")
                    } else {
                        w!("Collapse group")
                    },
                )
            };
        }
    }
    let mut point = windows::Win32::Foundation::POINT { x, y };
    unsafe {
        let _ = ClientToScreen(hwnd, &mut point);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            None,
            hwnd,
            None,
        )
        .0 as usize;
        let _ = DestroyMenu(menu);
        match command {
            PIN => {
                let _ = App::set_tab_pinned(app, id, !pinned);
            }
            AUTO => {
                let _ = App::set_tab_group_mode(app, id, tab_groups::GroupMode::Auto);
            }
            ALWAYS => {
                let _ = App::set_tab_group_mode(app, id, tab_groups::GroupMode::Always);
            }
            NEVER => {
                let _ = App::set_tab_group_mode(app, id, tab_groups::GroupMode::Never);
            }
            COLLAPSE if has_group => {
                let _ = App::set_tab_group_collapsed(app, id, !collapsed);
            }
            _ => {}
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

/// Ctrl+Shift+P: native menu to switch, create or delete profiles. Switching starts a new Bowser
/// process for that profile (WebView2 allows one browser process per data folder); the current
/// window stays open.
fn show_profile_menu(hwnd: HWND) {
    use crate::profile::{self, Profile};
    const NEW: usize = 1;
    const SWITCH_BASE: usize = 100;
    const DELETE_BASE: usize = 1000;
    const MAX_LISTED: usize = 300;

    let Some(app_data) = profile::app_data_dir() else {
        show_error_message("Bowser could not find this user's LocalAppData directory.");
        return;
    };
    let Ok(current) = profile::active() else {
        return;
    };
    let names: Vec<String> = profile::list(&app_data)
        .into_iter()
        .take(MAX_LISTED)
        .collect();
    let current_name = current.listed_name().to_owned();
    let is_current = |name: &str| name.eq_ignore_ascii_case(&current_name);
    let is_default = |name: &str| name.eq_ignore_ascii_case(profile::DEFAULT_NAME);
    let label = |name: &str| {
        let text = if is_default(name) { "Default" } else { name };
        windows::core::HSTRING::from(text)
    };

    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        return;
    };
    for (index, name) in names.iter().enumerate() {
        let flags = if is_current(name) {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let text = label(name);
        let _ = unsafe {
            AppendMenuW(
                menu,
                flags,
                SWITCH_BASE + index,
                windows::core::PCWSTR(text.as_ptr()),
            )
        };
    }
    let _ = unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, windows::core::PCWSTR::null()) };
    let _ = unsafe { AppendMenuW(menu, MF_STRING, NEW, w!("New profile")) };
    let deletable: Vec<usize> = names
        .iter()
        .enumerate()
        .filter(|(_, name)| !is_default(name) && !is_current(name))
        .map(|(index, _)| index)
        .collect();
    if let Ok(submenu) = unsafe { CreatePopupMenu() } {
        for &index in &deletable {
            let text = label(&names[index]);
            let _ = unsafe {
                AppendMenuW(
                    submenu,
                    MF_STRING,
                    DELETE_BASE + index,
                    windows::core::PCWSTR(text.as_ptr()),
                )
            };
        }
        let flags = if deletable.is_empty() {
            MF_POPUP | MF_GRAYED
        } else {
            MF_POPUP
        };
        // The submenu handle is owned by `menu` and is destroyed with it.
        let _ = unsafe { AppendMenuW(menu, flags, submenu.0 as usize, w!("Delete profile…")) };
    }

    let mut point = windows::Win32::Foundation::POINT { x: 8, y: 8 };
    let command = unsafe {
        let _ = ClientToScreen(hwnd, &mut point);
        TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            None,
            hwnd,
            None,
        )
        .0 as usize
    };
    let _ = unsafe { DestroyMenu(menu) };

    match command {
        NEW => match profile::create_auto(&app_data) {
            Ok(created) => open_profile_window(&created),
            Err(error) => show_error_message(&format!("The new profile was not created. {error}")),
        },
        id if (SWITCH_BASE..SWITCH_BASE + MAX_LISTED).contains(&id) => {
            if let Some(name) = names.get(id - SWITCH_BASE).filter(|name| !is_current(name)) {
                match Profile::for_listed_name(&app_data, name) {
                    Ok(target) => open_profile_window(&target),
                    Err(error) => show_error_message(&error.to_string()),
                }
            }
        }
        id if (DELETE_BASE..DELETE_BASE + MAX_LISTED).contains(&id) => {
            if let Some(name) = names.get(id - DELETE_BASE) {
                let question = format!(
                    "Delete the Bowser profile \"{name}\"? Its cookies, site data, saved passwords, settings and session will be permanently removed."
                );
                if crate::win32::ask_yes_no(&question) {
                    if let Err(error) = profile::delete(&app_data, name, &current) {
                        show_error_message(&format!(
                            "The profile \"{name}\" was not deleted. {error}"
                        ));
                    }
                }
            }
        }
        _ => {}
    }
}

fn open_profile_window(target: &crate::profile::Profile) {
    if !crate::profile::spawn_window(target) {
        show_error_message("Bowser could not start a window for that profile.");
    }
}

/// Ctrl+Shift+E: native menu for this profile's unpacked extensions. Each row toggles the
/// extension; failures from the last load show in the row. Removal is under a submenu.
fn show_extensions_menu(hwnd: HWND, app: &AppRef) {
    use crate::extensions::{self, Record};
    const INSTALL: usize = 1;
    const TOGGLE_BASE: usize = 100;
    const REMOVE_BASE: usize = 1000;
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
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        return;
    };
    if records.is_empty() {
        let _ = unsafe {
            AppendMenuW(
                menu,
                MF_STRING | MF_GRAYED,
                0,
                w!("No extensions installed"),
            )
        };
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
        let flags = if record.enabled {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let text = windows::core::HSTRING::from(label);
        let _ = unsafe {
            AppendMenuW(
                menu,
                flags,
                TOGGLE_BASE + index,
                windows::core::PCWSTR(text.as_ptr()),
            )
        };
    }
    let _ = unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, windows::core::PCWSTR::null()) };
    if let Ok(submenu) = unsafe { CreatePopupMenu() } {
        for (index, record) in records.iter().enumerate() {
            let text = windows::core::HSTRING::from(record.name.as_str());
            let _ = unsafe {
                AppendMenuW(
                    submenu,
                    MF_STRING,
                    REMOVE_BASE + index,
                    windows::core::PCWSTR(text.as_ptr()),
                )
            };
        }
        let flags = if records.is_empty() {
            MF_POPUP | MF_GRAYED
        } else {
            MF_POPUP
        };
        // The submenu handle is owned by `menu` and is destroyed with it.
        let _ = unsafe { AppendMenuW(menu, flags, submenu.0 as usize, w!("Remove extension…")) };
    }
    let _ = unsafe { AppendMenuW(menu, MF_STRING, INSTALL, w!("Install unpacked extension…")) };

    let mut point = windows::Win32::Foundation::POINT { x: 8, y: 8 };
    let command = unsafe {
        let _ = ClientToScreen(hwnd, &mut point);
        TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            None,
            hwnd,
            None,
        )
        .0 as usize
    };
    let _ = unsafe { DestroyMenu(menu) };

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
    let inspection = match extensions::inspect_source(&folder) {
        Ok(inspection) => inspection,
        Err(error) => {
            show_error_message(&error.to_string());
            return;
        }
    };
    let question = format!(
        "Install \"{}\" version {}?\n\n{}\n\nOnly install extensions you trust. An extension can read and change the pages it has access to.",
        inspection.manifest.name,
        inspection.manifest.version,
        extensions::permission_summary(&inspection.manifest)
    );
    if !crate::win32::ask_yes_no(&question) {
        return;
    }
    match extensions::install(
        &folder,
        &profile.extensions_dir(),
        &profile.extensions_file(),
    ) {
        Ok(record) => match extensions::folder_for(&profile.extensions_dir(), &record.id) {
            Ok(installed) => App::set_extension_enabled(app, record.id, installed, true),
            Err(error) => show_error_message(&error.to_string()),
        },
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
    let ids = App::tab_ids(app);
    if ids.is_empty() {
        return;
    }
    let menu = unsafe { CreatePopupMenu() };
    let Ok(menu) = menu else { return };
    let mut labels = Vec::with_capacity(ids.len());
    for (index, id) in ids.iter().copied().enumerate() {
        let mut label = App::tab_title(app, id).unwrap_or_default();
        if label.trim().is_empty() {
            label = "New tab".to_owned();
        }
        let wide = label.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let _ = unsafe {
            AppendMenuW(
                menu,
                MF_STRING,
                index as usize + 1,
                windows::core::PCWSTR(wide.as_ptr()),
            )
        };
        labels.push(wide);
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
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            None,
            hwnd,
            None,
        )
        .0 as usize;
        let _ = DestroyMenu(menu);
        if let Some(id) = command
            .checked_sub(1)
            .and_then(|index| ids.get(index))
            .copied()
        {
            let _ = App::select_tab(app, id);
        }
    }
    let _ = labels;
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
        (0, 0, client_width, client_height)
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
        Hit::Back => l.back_button,
        Hit::Forward => l.forward_button,
        Hit::Reload => l.reload_button,
        Hit::Omnibox => l.omnibox,
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
    .unwrap_or((Hit::None, layout::compute(0, 0, 96, 0, false)));
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
    .unwrap_or((Hit::None, layout::compute(0, 0, 96, 0, false)));
    if old != h {
        invalidate(hwnd, rect_for(&l, old));
        invalidate(hwnd, rect_for(&l, h));
    }
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
            let active_has_controller = App::active_tab_has_controller(app);
            let progress = App::tab_progress_percent(app, active);
            let can_back = App::can_go_back(app);
            let can_forward = App::can_go_forward(app);
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
                        title: if slot.collapsed {
                            slot.domain
                                .as_ref()
                                .map_or(&t.title, |domain| domain.as_str())
                        } else {
                            &t.title
                        },
                        loading: t.loading,
                        favicon: t.favicon.as_ref(),
                        emphasis: s
                            .motion
                            .emphasis(t.id)
                            .unwrap_or_else(|| Emphasis::resting(false, false, now)),
                        group_size: slot.domain.as_ref().map(|_| slot.group_size),
                        group_control: slot.group_control,
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
                    },
                    now,
                    departures: s.motion.departures(),
                };
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
                if !s.palette.translucent || !active_has_controller {
                    // Solid mode owns every pixel. In glass mode an untouched viewport is
                    // transparent too, so back blank tabs with the page surface to avoid
                    // revealing arbitrary windows behind Bowser.
                    let owned = if s.palette.translucent {
                        s.layout.viewport
                    } else {
                        Rect::new(0, 0, s.client.0, s.client.1)
                    };
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
            if did && App::navigation_progress_painted(app) {
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
        Command::Back => App::history(app, false),
        Command::Forward => App::history(app, true),
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
    let _ = with_chrome(hwnd, |s| s.refresh(app));
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
    arm_motion(hwnd);
    true
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
                s.update_completion();
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
    fn overlay_surface_tracks_chrome_height_across_dpi_changes() {
        let mut s = WindowChrome::new(1000, 700, 96, HWND::default());
        for dpi in [96, 120, 144, 192, 96, 120] {
            s.relayout(1000, 700, dpi, TabId(0), false);
            let chrome = layout::chrome_height(dpi);
            assert_eq!(s.surface.height(), chrome, "surface height at dpi {dpi}");
            assert_eq!(s.surface.width(), 1000, "surface width at dpi {dpi}");
            assert_eq!(s.layout.viewport.top, chrome, "viewport top at dpi {dpi}");
        }
    }
}
