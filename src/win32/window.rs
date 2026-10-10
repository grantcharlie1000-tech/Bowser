use super::{
    INPUT_ID, WM_CARET_BLINK_TIMER, WM_HISTORY_BACK, WM_HISTORY_FORWARD, WM_IDLE_TRIM_TIMER,
    WM_NAVIGATION_PROGRESS, WM_SHORTCUT_KEY, WM_SUBMIT, WM_TABS_CHANGED, frame, glass,
};
use crate::{
    app::{App, AppRef, IdleTimerAction},
    chrome::{
        layout::{self, Hit, Layout, Metrics, Rect},
        paint::{self, ChromeSurface, ChromeView, OmniboxView, TabView},
    },
    favicon::{self, FaviconCache},
    omnibox::{self, Completion, History},
    shortcuts::{self, Command},
    tabs::{FaviconKey, TabId},
    theme::Palette,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, ClientToScreen, CreateRectRgn, DEFAULT_QUALITY, DeleteObject, EndPaint,
            FillRect, InvalidateRect, PAINTSTRUCT, SetWindowRgn,
        },
        System::LibraryLoader::GetModuleHandleW,
        System::SystemInformation::GetTickCount,
        UI::{
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
                DestroyWindow, DispatchMessageW, EN_CHANGE, ES_AUTOHSCROLL, GWLP_USERDATA,
                GetCaretBlinkTime, GetClientRect, GetMessageW, GetWindowLongPtrW,
                GetWindowTextLengthW, GetWindowTextW, HMENU, HTCAPTION, HTCLOSE, HTMAXBUTTON,
                HTMINBUTTON, HideCaret, IsZoomed, KillTimer, LoadCursorW, MSG, MoveWindow,
                NCCALCSIZE_PARAMS, PostQuitMessage, RegisterClassW, SC_CLOSE, SC_MAXIMIZE,
                SC_MINIMIZE, SC_RESTORE, SIZE_MINIMIZED, SM_CXFRAME, SM_CXPADDEDBORDER, SM_CYFRAME,
                SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW, SetTimer, SetWindowLongPtrW,
                SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_STYLE, WM_CHAR,
                WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_DPICHANGED,
                WM_DWMCOLORIZATIONCOLORCHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_KILLFOCUS,
                WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONUP, WM_MOUSEMOVE,
                WM_MOVE, WM_NCCALCSIZE, WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST,
                WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE,
                WM_NCMOUSEMOVE, WM_PAINT, WM_PASTE, WM_SETFOCUS, WM_SETTINGCHANGE, WM_SIZE,
                WM_SYSCOMMAND, WM_SYSKEYDOWN, WM_THEMECHANGED, WM_TIMER, WNDCLASSW, WS_CHILD,
                WS_CLIPCHILDREN, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
                WS_EX_TRANSPARENT, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE, WVR_REDRAW,
            },
        },
    },
    core::w,
};

const CLASS_NAME: windows::core::PCWSTR = w!("BowserNativeWindow");
const WM_MOUSELEAVE: u32 = 0x02A3;
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
    views: Vec<TabView<'static>>,
    favicons: FaviconCache,
    failed: Vec<FaviconKey>,
    edit_text: String,
    focused: bool,
    caret: bool,
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
            views: Vec::with_capacity(8),
            favicons: FaviconCache::default(),
            failed: Vec::new(),
            edit_text: String::with_capacity(256),
            focused: false,
            caret: true,
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
        }
    }
    fn refresh(&mut self, app: &AppRef) {
        let ids = App::tab_ids(app);
        let mut tabs = Vec::with_capacity(ids.len());
        for id in ids {
            let old = self.tabs.iter().find(|t| t.id == id);
            let title = App::tab_title(app, id).unwrap_or_default();
            let title = if title.trim().is_empty() {
                "New tab".to_owned()
            } else {
                title
            };
            let loading = App::tab_loading(app, id);
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
        self.views.clear();
        self.tabs = tabs;
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
        let i = self.tabs.iter().position(|t| t.id == active).unwrap_or(0);
        self.layout =
            layout::compute_with_active(w, h, self.dpi, self.tabs.len().max(1), i, maximised);
        self.surface.resize(w, h, self.dpi)
    }
    fn update_completion(&mut self) {
        self.completion =
            omnibox::inline_completion(&self.history, &self.edit_text, !self.suppress_completion);
        self.suppress_completion = false
    }
}
thread_local! {static WINDOWS:RefCell<HashMap<isize,WindowChrome>>=RefCell::new(HashMap::new());}
fn with_chrome<T>(hwnd: HWND, f: impl FnOnce(&mut WindowChrome) -> T) -> Option<T> {
    WINDOWS.with(|a| a.borrow_mut().get_mut(&(hwnd.0 as isize)).map(f))
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

pub fn run(app: App) -> windows::core::Result<()> {
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
        let hwnd = CreateWindowExW(
            Default::default(),
            CLASS_NAME,
            w!("Bowser - Search or enter address"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
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
        let _ = ShowWindow(hwnd, SW_SHOW);
        app.borrow().log.record("window_visible").ok();
        if !app.borrow().edit.is_invalid() {
            app.borrow().log.record("input_ready").ok();
        }
        if let Some(url) = app.borrow_mut().benchmark_url.take() {
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
                    Hit::Tab(_) | Hit::TabClose(_) | Hit::NewTab | Hit::Omnibox
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
            // Only the caption buttons are ours. Dragging the strip and the resize edges need the
            // default handler, which runs the system move and size loops.
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
                // Glass leaves the client untouched so DWM shows the acrylic through it.
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
                let _ = with_chrome(hwnd, |s| {
                    s.palette = glass::palette();
                    if message == WM_SETTINGCHANGE && wparam.0 == SPI_SETNONCLIENTMETRICS {
                        s.surface.rebuild_fonts();
                    }
                });
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
                    s.refresh(app);
                    WINDOWS.with(|all| {
                        all.borrow_mut().insert(hwnd.0 as isize, s);
                    });
                    let _ = SetFocus(Some(edit));
                    let _ = SetTimer(
                        Some(hwnd),
                        WM_CARET_BLINK_TIMER,
                        GetCaretBlinkTime().max(100),
                        None,
                    );
                }
                LRESULT(0)
            }
            WM_MOVE => {
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_SIZE => {
                if let Some(app) = app {
                    let min = wparam.0 as u32 == SIZE_MINIMIZED;
                    let mut r = RECT::default();
                    let _ = GetClientRect(hwnd, &mut r);
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
            windows::Win32::UI::WindowsAndMessaging::WM_CAPTURECHANGED
            | windows::Win32::UI::WindowsAndMessaging::WM_CANCELMODE => {
                let had_state = with_chrome(hwnd, |s| {
                    let had_drag = s.drag.take().is_some();
                    let had_press = s.pressed != Hit::None;
                    s.pressed = Hit::None;
                    had_drag || had_press
                })
                .unwrap_or(false);
                if had_state {
                    let _ = ReleaseCapture();
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let p = frame::point_from_lparam(lparam);
                match hit(hwnd, p.x, p.y) {
                    Hit::Tab(i) => {
                        if let Some(app) = app {
                            let ids = App::tab_ids(app);
                            if let Some(id) = ids.get(i).copied() {
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
                    h @ Hit::CaptionMinimize | h @ Hit::CaptionMaximize | h @ Hit::CaptionClose => {
                        set_pressed(hwnd, h);
                        let _ = SetCapture(hwnd);
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let p = frame::point_from_lparam(lparam);
                if let Some((id, from)) = with_chrome(hwnd, |s| s.drag.take()).flatten() {
                    let l = with_chrome(hwnd, |s| s.layout).unwrap();
                    if let Some(app) = app {
                        let to = layout::reorder_target(&l, from, p.x);
                        let _ = App::move_tab(app, id, to);
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
                                if let Some(id) = App::tab_ids(app).get(i).copied() {
                                    let _ = App::close_tab(app, id);
                                }
                            }
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
                        let ids = App::tab_ids(app);
                        if let Some(id) = ids.get(i) {
                            let _ = App::close_tab(app, *id);
                        }
                    }
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
                    schedule_idle_timer(hwnd, App::idle_timer_fired(app, idle));
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == WM_CARET_BLINK_TIMER => {
                if app.is_some_and(|a| GetFocus() == a.borrow().edit) {
                    // Only the caret changes, so repaint just its pixels.
                    let caret = with_chrome(hwnd, |s| {
                        s.caret = !s.caret;
                        s.surface.caret_rect()
                    })
                    .unwrap_or_default();
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
                } else {
                    let _ = KillTimer(Some(hwnd), WM_CARET_BLINK_TIMER);
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
                if let Some(app) = app {
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
    with_chrome(hwnd, |s| layout::hit_test(&s.layout, x, y)).unwrap_or(Hit::None)
}
fn rect_for(l: &Layout, h: Hit) -> Rect {
    match h {
        Hit::Tab(i) | Hit::TabClose(i) => l.tab(i).unwrap_or_default(),
        Hit::NewTab => l.new_tab_button,
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
        (o, s.layout)
    })
    .unwrap_or((Hit::None, layout::compute(0, 0, 96, 0, false)));
    if old != h {
        invalidate(hwnd, rect_for(&l, old));
        invalidate(hwnd, rect_for(&l, h));
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
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        if let Some(app) = app {
            let active = App::active_tab_id(app);
            let progress = App::tab_progress_percent(app, active);
            let focus = GetFocus() == app.borrow().edit;
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
                for t in &s.tabs {
                    let v = TabView {
                        title: &t.title,
                        loading: t.loading,
                        favicon: t.favicon.as_ref(),
                    };
                    s.views
                        .push(std::mem::transmute::<TabView<'_>, TabView<'static>>(v));
                }
                let active_i = s.tabs.iter().position(|t| t.id == active).unwrap_or(0);
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
                };
                let t0 = std::time::Instant::now();
                s.surface.paint(&view, dirty);
                let t1 = std::time::Instant::now();
                let mut origin = windows::Win32::Foundation::POINT { x: 0, y: 0 };
                let _ = ClientToScreen(hwnd, &mut origin);
                s.surface.present(s.overlay, origin);
                let t2 = std::time::Instant::now();
                if std::env::var_os("BOWSER_PAINT_LOG").is_some() {
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
                    // Solid mode owns every pixel. The overlaid chrome uses the matching solid
                    // palette, and this host fill keeps the viewport and row gaps seamless.
                    let fill = dirty.intersect(Rect::new(0, 0, s.client.0, s.client.1));
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
    unsafe {
        let _ = SetTimer(
            Some(hwnd),
            WM_CARET_BLINK_TIMER,
            GetCaretBlinkTime().max(100),
            None,
        );
    }
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
    }
    let _ = with_chrome(hwnd, |s| s.refresh(app));
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
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
            let _ = SetTimer(
                Some(main),
                WM_CARET_BLINK_TIMER,
                GetCaretBlinkTime().max(100),
                None,
            );
            let _ = with_chrome(main, |s| {
                s.focused = true;
                s.caret = true;
                s.update_completion()
            });
            invalidate_omni(main);
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
            let x = p.x - Metrics::new(dpi(main)).px(Metrics::OMNIBOX_TEXT_INSET);
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
