//! The main ⋮ menu: a borderless popup drawn in the chrome palette, run as a synchronous modal loop
//! that returns the chosen command. Layout, hit testing, keyboard stepping and placement are pure
//! functions so they can be tested without a window.
//!
//! The popup is owned by the main window and takes capture while open. Any press outside it, Escape,
//! losing activation or losing capture dismisses it. Focus and capture are restored on every exit.

use std::cell::Cell;

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, BitBlt, CLEARTYPE_QUALITY, CreateCompatibleBitmap, CreateCompatibleDC,
            CreateFontIndirectW, CreateRoundRectRgn, CreateSolidBrush, DRAW_TEXT_FORMAT,
            DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER,
            DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetMonitorInfoW, GetObjectW,
            HDC, HFONT, LOGFONTW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
            PAINTSTRUCT, SRCCOPY, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{
                GetCapture, GetFocus, ReleaseCapture, SetCapture, SetFocus, VK_DOWN, VK_END,
                VK_ESCAPE, VK_HOME, VK_RETURN, VK_RIGHT, VK_SPACE, VK_UP,
            },
            WindowsAndMessaging::{
                CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                GWLP_USERDATA, GetClientRect, GetForegroundWindow, GetMessageW, GetWindowLongPtrW,
                GetWindowRect, IDC_ARROW, IsWindow, LoadCursorW, MSG, PostQuitMessage,
                RegisterClassW, SW_SHOW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow,
                WA_INACTIVE, WM_ACTIVATE, WM_CAPTURECHANGED, WM_ERASEBKGND, WM_KEYDOWN,
                WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE, WM_PAINT, WM_RBUTTONDOWN,
                WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, w},
};

use crate::{
    theme::{self, Rgba},
    win32::{glass, smooth},
};

const CLASS_NAME: PCWSTR = w!("BowserMainMenu");
/// Logical (96 DPI) measurements. Every size is scaled from these by the owner's DPI.
const WIDTH: i32 = 320;
/// Right-hand column for shortcuts and the submenu chevron. Wide enough for "Ctrl+Shift+B".
const HINT_WIDTH: i32 = 100;
const HEADER_HEIGHT: i32 = 72;
const ROW_HEIGHT: i32 = 34;
const SEPARATOR_HEIGHT: i32 = 9;
const FOOTER_HEIGHT: i32 = 32;
const LIST_GAP: i32 = 4;
const PAD: i32 = 8;
const RADIUS: i32 = 12;
const ROW_RADIUS: i32 = 6;
const AVATAR: i32 = 36;

thread_local! {
    static CLASS_REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// Monochrome glyph drawn with pen strokes, so it follows the palette and the DPI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    None,
    Plus,
    Person,
    Grid,
    Key,
    Shield,
    Gear,
    Close,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    pub id: usize,
    pub label: String,
    /// Muted hint drawn right-aligned, such as "Ctrl+T".
    pub shortcut: Option<&'static str>,
    pub icon: Icon,
    pub checked: bool,
    pub enabled: bool,
    /// Opens a flyout rather than running a command, so it shows a chevron instead of a shortcut.
    pub submenu: bool,
    /// Full wording shown in the footer while the row is highlighted, for labels kept short.
    pub description: Option<&'static str>,
}

impl Item {
    pub fn new(id: usize, label: impl Into<String>, icon: Icon) -> Self {
        Self {
            id,
            label: label.into(),
            shortcut: None,
            icon,
            checked: false,
            enabled: true,
            submenu: false,
            description: None,
        }
    }

    pub fn shortcut(mut self, shortcut: &'static str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    pub fn described(mut self, description: &'static str) -> Self {
        self.description = Some(description);
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn submenu(mut self) -> Self {
        self.submenu = true;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    Item(Item),
    Separator,
}

/// Everything the popup shows. The profile name feeds the avatar and header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Menu {
    pub profile: String,
    pub entries: Vec<Entry>,
    /// Non-interactive text at the bottom, such as the version.
    pub footer: String,
}

/// The command the user picked and the row it came from, in screen coordinates, so a flyout can
/// open beside that row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Chosen {
    pub id: usize,
    pub row: RECT,
}

fn selectable(entry: &Entry) -> bool {
    matches!(entry, Entry::Item(item) if item.enabled)
}

/// Next selectable entry after `from` in the given direction, wrapping at the ends. With no
/// selection, Down starts at the first entry and Up at the last. `None` when nothing can be chosen.
pub(crate) fn step(entries: &[Entry], from: Option<usize>, forward: bool) -> Option<usize> {
    let count = entries.len() as isize;
    if count == 0 {
        return None;
    }
    let start = match from {
        Some(index) => index as isize,
        None if forward => -1,
        None => count,
    };
    (1..=count).find_map(|k| {
        let offset = if forward { k } else { -k };
        let index = (start + offset).rem_euclid(count) as usize;
        selectable(&entries[index]).then_some(index)
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Row {
    rect: RECT,
    entry: usize,
    selectable: bool,
}

/// Pixel geometry for one DPI, relative to the popup's client area.
#[derive(Clone, Debug, PartialEq)]
struct Layout {
    width: i32,
    height: i32,
    header: RECT,
    rows: Vec<Row>,
    footer: RECT,
}

fn scaled(logical: i32, dpi: u32) -> i32 {
    (i64::from(logical) * i64::from(dpi) / 96) as i32
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

fn contains(r: RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

impl Layout {
    fn new(menu: &Menu, dpi: u32) -> Self {
        let s = |v: i32| scaled(v, dpi);
        let width = s(WIDTH);
        let header = rect(0, 0, width, s(HEADER_HEIGHT));
        let mut y = header.bottom + s(LIST_GAP);
        let mut rows = Vec::with_capacity(menu.entries.len());
        for (entry, kind) in menu.entries.iter().enumerate() {
            let height = match kind {
                Entry::Item(_) => s(ROW_HEIGHT),
                Entry::Separator => s(SEPARATOR_HEIGHT),
            };
            rows.push(Row {
                rect: rect(0, y, width, y + height),
                entry,
                selectable: selectable(kind),
            });
            y += height;
        }
        let footer = rect(
            0,
            y + s(LIST_GAP),
            width,
            y + s(LIST_GAP) + s(FOOTER_HEIGHT),
        );
        Self {
            width,
            height: footer.bottom,
            header,
            rows,
            footer,
        }
    }

    /// Entry index of the selectable row under the point, if any.
    fn entry_at(&self, x: i32, y: i32) -> Option<usize> {
        self.rows
            .iter()
            .find(|row| row.selectable && contains(row.rect, x, y))
            .map(|row| row.entry)
    }

    fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.width && y < self.height
    }
}

/// Top-left corner for a popup whose top-right corner is `anchor`, moved to keep it inside `work`.
pub(crate) fn place(anchor: POINT, width: i32, height: i32, work: RECT) -> POINT {
    let x = (anchor.x - width).clamp(work.left, (work.right - width).max(work.left));
    let y = if anchor.y + height <= work.bottom {
        anchor.y
    } else {
        (work.bottom - height).max(work.top)
    };
    POINT { x, y }
}

fn offset(r: RECT, origin: POINT) -> RECT {
    rect(
        r.left + origin.x,
        r.top + origin.y,
        r.right + origin.x,
        r.bottom + origin.y,
    )
}

/// Everything the window procedure needs. Lives in a `Box` from creation until the modal loop ends.
/// Interaction state is in `Cell`s so the loop and the procedure can both read it through `&`.
struct State {
    menu: Menu,
    layout: Layout,
    body: HFONT,
    heading: HFONT,
    /// Entry under the pointer or chosen with the arrow keys.
    current: Cell<Option<usize>>,
    /// Entry the left button went down on, so only a matching release activates.
    pressed: Cell<Option<usize>>,
    chosen: Cell<Option<usize>>,
    done: Cell<bool>,
}

/// Shows the menu with its top-right corner at `anchor` (screen coordinates) and returns the item
/// the user activated, or `None` when it was dismissed. Blocks until then.
pub(crate) fn track(owner: HWND, anchor: POINT, menu: Menu) -> Option<Chosen> {
    let dpi = dpi_of(owner);
    let layout = Layout::new(&menu, dpi);
    let origin = place(anchor, layout.width, layout.height, work_area(anchor));

    let body = crate::chrome::paint::create_message_font(dpi, CLEARTYPE_QUALITY);
    let heading = unsafe { create_heading_font(body) };
    let state = Box::into_raw(Box::new(State {
        menu,
        layout,
        body,
        heading,
        current: Cell::new(None),
        pressed: Cell::new(None),
        chosen: Cell::new(None),
        done: Cell::new(false),
    }));
    let prior_focus = unsafe { GetFocus() };
    let owner_rect = unsafe { window_rect(owner) };
    let window = unsafe { create_window(owner, origin, &*state, state.cast_const().cast()) };
    let Some(window) = window else {
        unsafe { free_state(state) };
        return None;
    };

    unsafe {
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(window));
        let _ = SetCapture(window);
    }

    // A modal loop, as TrackPopupMenu runs its own. Messages for other windows are dispatched too.
    let state_ref = unsafe { &*state };
    while !state_ref.done.get() {
        let mut msg = MSG::default();
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 <= 0 {
            // WM_QUIT (0) or failure (-1): leave the loop and let the caller's loop see the quit.
            if got.0 == 0 {
                unsafe { PostQuitMessage(msg.wParam.0 as i32) };
            }
            break;
        }
        unsafe {
            DispatchMessageW(&msg);
        }
        // The owner was destroyed, moved or resized (its messages ran through the loop above):
        // the menu no longer sits under its anchor, so it is dismissed.
        if unsafe { owner_changed(owner, owner_rect) } {
            state_ref.done.set(true);
        }
    }

    let result = state_ref.chosen.get().and_then(|index| {
        let row = state_ref
            .layout
            .rows
            .iter()
            .find(|row| row.entry == index)?;
        match &state_ref.menu.entries[index] {
            Entry::Item(item) => Some(Chosen {
                id: item.id,
                row: offset(row.rect, origin),
            }),
            Entry::Separator => None,
        }
    });

    // Focus only follows the menu while Bowser is still in front. After a dismissal by another
    // app taking activation, focus stays where that app put it: no foreground stealing.
    let foreground = unsafe { GetForegroundWindow() };
    let app_in_front = foreground == window || foreground == owner;
    unsafe {
        if GetCapture() == window {
            let _ = ReleaseCapture();
        }
        if IsWindow(Some(window)).as_bool() {
            let _ = DestroyWindow(window);
        }
        free_state(state);
        if app_in_front && IsWindow(Some(owner)).as_bool() {
            if foreground == window {
                let _ = SetForegroundWindow(owner);
            }
            if !prior_focus.is_invalid() && IsWindow(Some(prior_focus)).as_bool() {
                let _ = SetFocus(Some(prior_focus));
            }
        }
    }
    result
}

unsafe fn window_rect(hwnd: HWND) -> RECT {
    unsafe {
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        rect
    }
}

/// True when the owner is gone or has moved or resized since the menu opened.
unsafe fn owner_changed(owner: HWND, before: RECT) -> bool {
    unsafe { !IsWindow(Some(owner)).as_bool() || window_rect(owner) != before }
}

/// Frees the popup state and its fonts. The window must already be destroyed.
unsafe fn free_state(state: *mut State) {
    unsafe {
        let state = Box::from_raw(state);
        let _ = DeleteObject(state.body.into());
        let _ = DeleteObject(state.heading.into());
    }
}

unsafe fn create_window(
    owner: HWND,
    origin: POINT,
    state: &State,
    params: *const std::ffi::c_void,
) -> Option<HWND> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None).ok()?.0);
        if !CLASS_REGISTERED.with(Cell::get) {
            let class = WNDCLASSW {
                lpfnWndProc: Some(menu_proc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_ARROW).ok()?,
                lpszClassName: CLASS_NAME,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return None;
            }
            CLASS_REGISTERED.with(|registered| registered.set(true));
        }
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS_NAME,
            w!("Bowser menu"),
            WS_POPUP,
            origin.x,
            origin.y,
            state.layout.width,
            state.layout.height,
            Some(owner),
            None,
            Some(instance),
            Some(params),
        )
        .ok()?;
        // The region takes ownership of the handle.
        let radius = scaled(RADIUS, dpi_of(owner)) * 2;
        let region = CreateRoundRectRgn(
            0,
            0,
            state.layout.width + 1,
            state.layout.height + 1,
            radius,
            radius,
        );
        let _ = SetWindowRgn(window, Some(region), false);
        Some(window)
    }
}

/// The body font made larger and semibold, for the profile name and avatar initial.
unsafe fn create_heading_font(body: HFONT) -> HFONT {
    unsafe {
        let mut logfont = LOGFONTW::default();
        GetObjectW(
            body.into(),
            size_of::<LOGFONTW>() as i32,
            Some((&mut logfont as *mut LOGFONTW).cast()),
        );
        logfont.lfHeight = logfont.lfHeight * 13 / 10;
        logfont.lfWeight = 600;
        CreateFontIndirectW(&logfont)
    }
}

fn dpi_of(hwnd: HWND) -> u32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 96 } else { dpi }
}

/// Work area of the monitor nearest the point, so the menu stays clear of the taskbar.
fn work_area(anchor: POINT) -> RECT {
    unsafe {
        let monitor = MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            info.rcWork
        } else {
            rect(0, 0, 1920, 1080)
        }
    }
}

/// The popup state, valid from `WM_NCCREATE` until the modal loop frees it.
fn state<'a>(hwnd: HWND) -> Option<&'a State> {
    unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State).as_ref() }
}

/// Client-space point from `WM_*MOUSE*` parameters, with the signed coordinates capture can produce.
fn point(lparam: LPARAM) -> (i32, i32) {
    (
        (lparam.0 & 0xFFFF) as i16 as i32,
        ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    )
}

unsafe extern "system" fn menu_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_NCCREATE => {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if let Some(state) = state(hwnd) {
                    let (x, y) = point(lparam);
                    let hover = state.layout.entry_at(x, y);
                    if hover != state.current.get() {
                        state.current.set(hover);
                        invalidate(hwnd);
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                if let Some(state) = state(hwnd) {
                    let (x, y) = point(lparam);
                    if state.layout.inside(x, y) {
                        state.pressed.set(state.layout.entry_at(x, y));
                        invalidate(hwnd);
                    } else {
                        state.done.set(true);
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                if let Some(state) = state(hwnd) {
                    let (x, y) = point(lparam);
                    let released = state.layout.entry_at(x, y);
                    if state.layout.inside(x, y)
                        && released.is_some()
                        && released == state.pressed.get()
                    {
                        state.chosen.set(released);
                        state.done.set(true);
                    }
                    state.pressed.set(None);
                    invalidate(hwnd);
                }
                LRESULT(0)
            }
            WM_RBUTTONDOWN => {
                if let Some(state) = state(hwnd) {
                    let (x, y) = point(lparam);
                    if !state.layout.inside(x, y) {
                        state.done.set(true);
                    }
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                if let Some(state) = state(hwnd) {
                    let key = wparam.0;
                    let is = |vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| {
                        key == vk.0 as usize
                    };
                    if is(VK_ESCAPE) {
                        state.done.set(true);
                    } else if is(VK_DOWN) || is(VK_UP) {
                        let next = step(&state.menu.entries, state.current.get(), is(VK_DOWN));
                        if next != state.current.get() {
                            state.current.set(next);
                            invalidate(hwnd);
                        }
                    } else if is(VK_HOME) || is(VK_END) {
                        let next = step(&state.menu.entries, None, is(VK_HOME));
                        if next != state.current.get() {
                            state.current.set(next);
                            invalidate(hwnd);
                        }
                    } else if is(VK_RETURN) || is(VK_SPACE) {
                        if let Some(index) = state.current.get() {
                            state.chosen.set(Some(index));
                            state.done.set(true);
                        }
                    } else if is(VK_RIGHT) {
                        // Right opens a flyout row, as in native menus; other rows ignore it.
                        if let Some(index) = state.current.get() {
                            if matches!(&state.menu.entries[index], Entry::Item(item) if item.submenu)
                            {
                                state.chosen.set(Some(index));
                                state.done.set(true);
                            }
                        }
                    }
                }
                LRESULT(0)
            }
            WM_ACTIVATE => {
                // Low word of WPARAM is the activation state; any other window taking focus dismisses.
                if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                    if let Some(state) = state(hwnd) {
                        state.done.set(true);
                    }
                }
                LRESULT(0)
            }
            WM_CAPTURECHANGED => {
                if let Some(state) = state(hwnd) {
                    state.done.set(true);
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, false);
    }
}

fn colorref(colour: Rgba) -> windows::Win32::Foundation::COLORREF {
    windows::Win32::Foundation::COLORREF(
        u32::from(colour.r) | (u32::from(colour.g) << 8) | (u32::from(colour.b) << 16),
    )
}

/// Draws into a memory bitmap and copies it once, so hover changes do not flicker.
unsafe fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        if let Some(state) = state(hwnd) {
            let mut client = RECT::default();
            let _ = GetClientRect(hwnd, &mut client);
            let memory = CreateCompatibleDC(Some(dc));
            let bitmap = CreateCompatibleBitmap(dc, client.right, client.bottom);
            let old = SelectObject(memory, bitmap.into());
            draw(memory, &client, dpi_of(hwnd), state);
            let _ = BitBlt(
                dc,
                0,
                0,
                client.right,
                client.bottom,
                Some(memory),
                0,
                0,
                SRCCOPY,
            );
            SelectObject(memory, old);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory);
        }
        let _ = EndPaint(hwnd, &ps);
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

unsafe fn draw(dc: HDC, client: &RECT, dpi: u32, state: &State) {
    unsafe {
        let palette = theme::palette(glass::appearance(), true);
        let s = |v: i32| scaled(v, dpi);
        SetBkMode(dc, TRANSPARENT);
        SelectObject(dc, state.body.into());

        let background = CreateSolidBrush(colorref(palette.window));
        FillRect(dc, client, background);
        let _ = DeleteObject(background.into());
        smooth::stroke_round_rect(
            dc,
            rect(0, 0, client.right - 1, client.bottom - 1),
            s(RADIUS),
            palette.hairline,
            1,
        );

        // Header: avatar with the profile's initial, the name and a muted caption.
        let header = state.layout.header;
        let avatar_top = (header.top + header.bottom - s(AVATAR)) / 2;
        let avatar = rect(
            header.left + s(PAD + 8),
            avatar_top,
            header.left + s(PAD + 8) + s(AVATAR),
            avatar_top + s(AVATAR),
        );
        smooth::fill_ellipse(dc, avatar, palette.progress_fill);
        let initial = state
            .menu
            .profile
            .chars()
            .next()
            .map_or_else(|| "B".to_owned(), |c| c.to_uppercase().to_string());
        SelectObject(dc, state.heading.into());
        draw_text(
            dc,
            avatar,
            &initial,
            palette.window,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        let text_left = avatar.right + s(12);
        draw_text(
            dc,
            rect(
                text_left,
                header.top + s(16),
                header.right - s(PAD),
                header.top + s(40),
            ),
            &state.menu.profile,
            palette.tab_text_active,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
        SelectObject(dc, state.body.into());
        draw_text(
            dc,
            rect(
                text_left,
                header.top + s(40),
                header.right - s(PAD),
                header.top + s(60),
            ),
            "Current profile",
            palette.omnibox_placeholder,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );

        let current = state.current.get();
        let pressed = state.pressed.get();
        for row in &state.layout.rows {
            match &state.menu.entries[row.entry] {
                Entry::Separator => {
                    let y = (row.rect.top + row.rect.bottom) / 2;
                    smooth::stroke_line(
                        dc,
                        POINT {
                            x: row.rect.left + s(PAD + 8),
                            y,
                        },
                        POINT {
                            x: row.rect.right - s(PAD + 8),
                            y,
                        },
                        palette.hairline,
                        1,
                    );
                }
                Entry::Item(item) => {
                    let highlighted = row.selectable && current == Some(row.entry);
                    let held = row.selectable && pressed == Some(row.entry);
                    draw_item(dc, row.rect, item, (highlighted, held), palette, dpi);
                }
            }
        }

        // The footer explains the highlighted row in full, and otherwise shows the version.
        let description = state
            .current
            .get()
            .and_then(|index| match &state.menu.entries[index] {
                Entry::Item(item) => item.description,
                Entry::Separator => None,
            });
        draw_text(
            dc,
            rect(
                state.layout.footer.left + s(PAD + 8),
                state.layout.footer.top,
                state.layout.footer.right - s(PAD),
                state.layout.footer.bottom,
            ),
            description.unwrap_or(&state.menu.footer),
            palette.omnibox_placeholder,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
    }
}

/// One row: optional hover or pressed fill, the icon or check colour, the label, and the shortcut
/// or chevron on the right.
unsafe fn draw_item(
    dc: HDC,
    row: RECT,
    item: &Item,
    (highlighted, held): (bool, bool),
    palette: &theme::Palette,
    dpi: u32,
) {
    unsafe {
        let s = |v: i32| scaled(v, dpi);
        let inner = rect(row.left + s(PAD), row.top, row.right - s(PAD), row.bottom);
        let fill = if !item.enabled {
            None
        } else if held {
            Some(palette.tab_active)
        } else if highlighted {
            Some(palette.tab_hover)
        } else {
            None
        };
        if let Some(fill) = fill {
            smooth::fill_round_rect(dc, inner, s(ROW_RADIUS), fill);
        }
        let (text, muted) = if item.enabled {
            (palette.tab_text_inactive, false)
        } else {
            (palette.omnibox_placeholder, true)
        };
        let text = if highlighted && item.enabled {
            palette.tab_text_active
        } else {
            text
        };
        let glyph_colour = if item.checked {
            palette.progress_fill
        } else {
            text
        };
        if item.icon != Icon::None {
            let centre = (inner.top + inner.bottom) / 2;
            let size = s(16);
            let glyph = rect(
                inner.left + s(10),
                centre - size / 2,
                inner.left + s(10) + size,
                centre - size / 2 + size,
            );
            draw_icon(dc, item.icon, glyph, glyph_colour);
        }
        let has_hint = item.submenu || item.shortcut.is_some();
        let label_right = if has_hint {
            inner.right - s(HINT_WIDTH + 8)
        } else {
            inner.right - s(10)
        };
        draw_text(
            dc,
            rect(inner.left + s(36), inner.top, label_right, inner.bottom),
            &item.label,
            text,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
        let hint_bounds = rect(
            inner.right - s(HINT_WIDTH),
            inner.top,
            inner.right - s(10),
            inner.bottom,
        );
        if item.submenu {
            draw_text(
                dc,
                hint_bounds,
                "›",
                text,
                DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        } else if let Some(shortcut) = item.shortcut {
            let colour = if muted {
                palette.omnibox_placeholder
            } else {
                palette.omnibox_completion
            };
            draw_text(
                dc,
                hint_bounds,
                shortcut,
                colour,
                DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        }
    }
}

/// Draws a 16-unit glyph scaled into `r`, antialiased so the curves and diagonals stay smooth.
unsafe fn draw_icon(dc: HDC, icon: Icon, r: RECT, colour: Rgba) {
    unsafe {
        let width = (r.right - r.left).max(1);
        let height = (r.bottom - r.top).max(1);
        let stroke = (width / 10).max(1);
        let x = |v: i32| r.left + v * width / 16;
        let y = |v: i32| r.top + v * height / 16;
        let point = |px: i32, py: i32| POINT { x: x(px), y: y(py) };
        let line = |x0: i32, y0: i32, x1: i32, y1: i32| {
            smooth::stroke_line(dc, point(x0, y0), point(x1, y1), colour, stroke);
        };
        let boxed = |left: i32, top: i32, right: i32, bottom: i32| {
            rect(x(left), y(top), x(right), y(bottom))
        };
        let corner = width / 4;
        match icon {
            Icon::None => {}
            Icon::Plus => {
                line(8, 3, 8, 13);
                line(3, 8, 13, 8);
            }
            Icon::Person => {
                smooth::stroke_ellipse(dc, boxed(5, 0, 11, 6), colour, stroke);
                smooth::stroke_round_rect(dc, boxed(2, 8, 14, 16), corner, colour, stroke);
            }
            Icon::Grid => {
                for (left, top) in [(2, 2), (9, 2), (2, 9), (9, 9)] {
                    smooth::stroke_rect(dc, boxed(left, top, left + 5, top + 5), colour, stroke);
                }
            }
            Icon::Key => {
                smooth::stroke_ellipse(dc, boxed(1, 5, 8, 12), colour, stroke);
                line(8, 8, 15, 8);
                line(12, 8, 12, 11);
                line(15, 8, 15, 10);
            }
            Icon::Shield => {
                let points = [(8, 1), (14, 3), (14, 8), (8, 15), (2, 8), (2, 3)]
                    .map(|(px, py)| point(px, py));
                smooth::stroke_polygon(dc, &points, colour, stroke);
            }
            Icon::Gear => {
                smooth::stroke_ellipse(dc, boxed(1, 1, 15, 15), colour, stroke);
                smooth::stroke_ellipse(dc, boxed(5, 5, 11, 11), colour, stroke);
            }
            Icon::Close => {
                line(3, 3, 13, 13);
                line(13, 3, 3, 13);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: usize, enabled: bool) -> Entry {
        let item = Item::new(id, format!("Item {id}"), Icon::None);
        Entry::Item(if enabled { item } else { item.disabled() })
    }

    fn menu(entries: Vec<Entry>) -> Menu {
        Menu {
            profile: "Default".to_owned(),
            entries,
            footer: "Bowser test".to_owned(),
        }
    }

    fn sample() -> Vec<Entry> {
        vec![
            item(1, true),
            Entry::Separator,
            item(2, false),
            item(3, true),
        ]
    }

    #[test]
    fn down_skips_separators_and_disabled_rows_and_wraps() {
        let entries = sample();
        assert_eq!(step(&entries, None, true), Some(0));
        assert_eq!(step(&entries, Some(0), true), Some(3));
        assert_eq!(step(&entries, Some(3), true), Some(0));
    }

    #[test]
    fn up_skips_disabled_rows_and_wraps() {
        let entries = sample();
        assert_eq!(step(&entries, None, false), Some(3));
        assert_eq!(step(&entries, Some(3), false), Some(0));
        assert_eq!(step(&entries, Some(0), false), Some(3));
    }

    #[test]
    fn nothing_is_selectable_when_every_row_is_disabled_or_empty() {
        assert_eq!(step(&[], None, true), None);
        let entries = vec![item(1, false), Entry::Separator];
        assert_eq!(step(&entries, None, true), None);
        assert_eq!(step(&entries, Some(0), false), None);
    }

    #[test]
    fn hit_testing_finds_only_enabled_rows() {
        let menu = menu(sample());
        let layout = Layout::new(&menu, 96);
        let first = layout.rows[0].rect;
        let separator = layout.rows[1].rect;
        let disabled = layout.rows[2].rect;
        assert_eq!(layout.entry_at(first.left + 5, first.top + 5), Some(0));
        assert_eq!(layout.entry_at(separator.left + 5, separator.top), None);
        assert_eq!(layout.entry_at(disabled.left + 5, disabled.top + 5), None);
        assert_eq!(layout.entry_at(-1, -1), None);
        assert_eq!(
            layout.entry_at(layout.header.left + 5, layout.header.top + 5),
            None
        );
    }

    #[test]
    fn layout_scales_rows_and_keeps_the_footer_below_the_list() {
        let menu = menu(sample());
        let at_96 = Layout::new(&menu, 96);
        let at_144 = Layout::new(&menu, 144);
        assert_eq!(at_96.width, WIDTH);
        assert_eq!(at_144.width, WIDTH * 3 / 2);
        assert_eq!(
            at_144.rows[0].rect.bottom - at_144.rows[0].rect.top,
            ROW_HEIGHT * 3 / 2
        );
        let last = at_96.rows.last().expect("rows").rect;
        assert!(at_96.footer.top > last.bottom);
        assert_eq!(at_96.height, at_96.footer.bottom);
    }

    #[test]
    fn placement_opens_below_the_anchor_and_clamps_to_the_work_area() {
        let work = rect(0, 0, 1000, 700);
        let inside = place(POINT { x: 900, y: 40 }, 280, 300, work);
        assert_eq!((inside.x, inside.y), (620, 40));
        let past_bottom = place(POINT { x: 900, y: 600 }, 280, 300, work);
        assert_eq!(past_bottom.y, 400);
        let past_left = place(POINT { x: 100, y: 40 }, 280, 300, work);
        assert_eq!(past_left.x, 0);
    }

    #[test]
    fn a_selection_moves_only_between_selectable_rows_when_the_menu_is_rebuilt() {
        let before = menu(sample());
        let after = menu(vec![item(9, true), Entry::Separator, item(8, true)]);
        assert_eq!(step(&before.entries, Some(3), true), Some(0));
        assert_eq!(step(&after.entries, Some(0), true), Some(2));
    }
}
