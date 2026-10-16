//! Search model for the command palette (Ctrl+Shift+K). The index is assembled only when opened.
//!
//! With the HTML chrome the palette is a page in the popup host (`html_page`, `{t:"palette"}`):
//! Rust sends the item list, the page filters it, and a pick names an item by index, which Rust
//! maps back to the action it built. NATIVE FALLBACK: `show` below (a GDI window with an EDIT
//! control) runs only when the HTML chrome is off or the popup host is not ready yet.

use crate::commands::{
    PaletteCommand,
    palette::{self as engine, Item},
    registry::PaletteKind,
    store::{Options, ResultType},
};
use std::{cell::Cell, ffi::c_void};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
            InvalidateRect, PAINTSTRUCT, SetBkColor, SetBkMode, SetTextColor, TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{EM_GETSEL, EM_SETSEL},
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::SetFocus,
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::{
                CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                EN_CHANGE, ES_AUTOHSCROLL, GWLP_USERDATA, GetClientRect, GetMessageW, SendMessageW, SetWindowTextW,
                GetWindowTextLengthW, GetWindowTextW, IDC_ARROW, LoadCursorW, MSG, RegisterClassW,
                SetForegroundWindow, SetWindowLongPtrW, TranslateMessage, WINDOW_STYLE, WM_APP,
                WM_COMMAND, WM_CTLCOLOREDIT, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP, WM_NCCREATE,
                WM_NCDESTROY, WM_PAINT, WNDCLASSW, WS_CHILD, WS_EX_TOOLWINDOW, WS_POPUP,
                WS_VISIBLE,
            },
        },
    },
    core::{PCWSTR, w},
};

use super::{glass, smooth};
use crate::theme;

const CLASS: PCWSTR = w!("BowserCommandPalette");
const EDIT_ID: usize = 1;
const EDIT_SUBCLASS: usize = 0x7A31;
const WM_PALETTE_ACTION: u32 = WM_APP + 0x61;
const WM_FINISH: u32 = WM_APP + 0x62;

/// Everything the palette can search, gathered once when it opens. Each list comes from a store that
/// already exists (open tabs, recently closed tabs, workspaces, bookmarks, notes, history and the
/// command registry); the palette keeps no data of its own.
#[derive(Clone, Debug, Default)]
pub(crate) struct PaletteData {
    pub tabs: Vec<(u32, String, String)>,
    /// Recently closed tabs, newest first: title and address.
    pub closed: Vec<(String, String)>,
    /// Workspace id, name and tab count.
    pub workspaces: Vec<(u32, String, usize)>,
    /// Title, address and folder.
    pub bookmarks: Vec<(String, String, String)>,
    /// Note id, title and text.
    pub notes: Vec<(u64, String, String)>,
    /// Time, address and title, oldest first as the file stores them.
    pub history: Vec<(u64, String, String)>,
    pub commands: Vec<PaletteCommand>,
    /// Bookmark nicknames (T2's bookmark dialog) as (address, nickname); empty nicknames are left out.
    pub nicknames: Vec<(String, String)>,
}

impl PaletteData {
    /// The rows the native fallback searches, in the palette's order.
    fn items(&self) -> Vec<Item> {
        let mut items = Vec::new();
        for (id, title, url) in &self.tabs {
            items.push(Item { kind: ResultType::Tab, label: title.clone(), detail: url.clone(), action: format!("tab:{id}") });
        }
        for (title, url) in &self.closed {
            items.push(Item { kind: ResultType::Closed, label: title.clone(), detail: url.clone(), action: format!("reopen:{url}") });
        }
        for (id, name, tabs) in &self.workspaces {
            items.push(Item { kind: ResultType::Workspace, label: name.clone(), detail: tab_count(*tabs), action: format!("workspace:{id}") });
        }
        for command in &self.commands {
            let kind = if command.kind == PaletteKind::Action { ResultType::Action } else { ResultType::Command };
            items.push(Item { kind, label: command.label.clone(), detail: command.detail.clone(), action: command.id.clone() });
        }
        for (title, url, folder) in &self.bookmarks {
            // The native fallback finds a bookmark by its nickname too (it is part of the searched text).
            let nick = self.nickname_of(url).map_or_else(String::new, |nick| format!(" {nick}"));
            items.push(Item { kind: ResultType::Bookmark, label: title.clone(), detail: format!("{url} {folder}{nick}"), action: format!("url:{url}") });
        }
        for (id, title, body) in &self.notes {
            items.push(Item { kind: ResultType::Note, label: note_label(title, body), detail: note_detail(body), action: format!("note:{id}") });
        }
        for (_, url, title) in self.history.iter().rev() {
            items.push(Item { kind: ResultType::History, label: title.clone(), detail: url.clone(), action: format!("url:{url}") });
        }
        items
    }

    /// The nickname of the bookmark at `url`, if it has one.
    fn nickname_of(&self, url: &str) -> Option<&str> {
        self.nicknames.iter().find(|(address, nick)| address == url && !nick.is_empty()).map(|(_, nick)| nick.as_str())
    }
}

/// Longest nickname sent with a bookmark row (T2's dialog caps nicknames at 24 characters).
pub(crate) const HTML_MAX_NICK: usize = 24;

fn tab_count(count: usize) -> String {
    format!("{count} tab{}", if count == 1 { "" } else { "s" })
}

/// A note's row label: its title, else its first line.
fn note_label(title: &str, body: &str) -> String {
    if title.trim().is_empty() { body.lines().next().unwrap_or("").trim().to_owned() } else { title.to_owned() }
}

/// A note's row detail: its text on one line.
fn note_detail(body: &str) -> String {
    body.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct Index {
    items: Vec<Item>,
    options: Options,
}

struct Dialog {
    edit: HWND,
    state: PaletteState,
    index: Index,
    result: Option<ResultItem>,
    done: bool,
    brush: windows::Win32::Graphics::Gdi::HBRUSH,
}

thread_local! { static REGISTERED: Cell<bool> = const { Cell::new(false) }; }

/// Opens the live, themed search overlay. Its data index is built once per invocation, on demand.
pub(crate) fn show(owner: HWND, data: PaletteData, options: Options) -> Option<ResultItem> {
    register_class()?;
    let palette = theme::palette(glass::appearance(), true);
    let mut state = Box::new(Dialog {
        edit: HWND::default(),
        state: PaletteState::new(),
        index: Index { items: data.items(), options },
        result: None,
        done: false,
        brush: unsafe { CreateSolidBrush(colorref(palette.omnibox_fill)) },
    });
    let mut owner_rect = RECT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(owner, &mut owner_rect);
    }
    let width = scale(640, owner);
    let height = scale(510, owner);
    let x = (owner_rect.left + owner_rect.right - width) / 2;
    let y = (owner_rect.top + owner_rect.bottom - height) / 2;
    let instance = unsafe { HINSTANCE(GetModuleHandleW(None).ok()?.0) };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS,
            w!("Command palette"),
            WS_POPUP | WS_VISIBLE,
            x,
            y,
            width,
            height,
            Some(owner),
            None,
            Some(instance),
            Some((&mut *state as *mut Dialog).cast::<c_void>()),
        )
        .ok()?
    };
    unsafe {
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(state.edit));
    }
    while !state.done {
        let mut msg = MSG::default();
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0).0 };
        if got <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    unsafe {
        if windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)).as_bool() {
            let _ = DestroyWindow(hwnd);
        }
        let _ = DeleteObject(state.brush.into());
    }
    state.result.take()
}

fn register_class() -> Option<()> {
    REGISTERED.with(|registered| {
        if registered.get() {
            return Some(());
        }
        let instance = unsafe { HINSTANCE(GetModuleHandleW(None).ok()?.0) };
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance,
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW).ok()? },
            lpszClassName: CLASS,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            return None;
        }
        registered.set(true);
        Some(())
    })
}

fn dialog(hwnd: HWND) -> Option<&'static mut Dialog> {
    let pointer =
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA) }
            as *mut Dialog;
    unsafe { pointer.as_mut() }
}

unsafe extern "system" fn proc(
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
            windows::Win32::UI::WindowsAndMessaging::WM_CREATE => {
                if let Some(state) = dialog(hwnd) {
                    state.edit = match CreateWindowExW(
                        Default::default(),
                        w!("EDIT"),
                        PCWSTR::null(),
                        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                        scale(24, hwnd),
                        scale(55, hwnd),
                        scale(592, hwnd),
                        scale(38, hwnd),
                        Some(hwnd),
                        Some(windows::Win32::UI::WindowsAndMessaging::HMENU(
                            EDIT_ID as *mut c_void,
                        )),
                        Some(HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0)),
                        None,
                    ) {
                        Ok(edit) => edit,
                        Err(_) => {
                            state.done = true;
                            return LRESULT(0);
                        }
                    };
                    let _ = SetWindowSubclass(
                        state.edit,
                        Some(edit_proc),
                        EDIT_SUBCLASS,
                        hwnd.0 as usize,
                    );
                    state.state.update("", &state.index);
                }
                LRESULT(0)
            }
            WM_CTLCOLOREDIT => {
                if let Some(state) = dialog(hwnd) {
                    let dc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut c_void);
                    let palette = theme::palette(glass::appearance(), true);
                    SetBkColor(dc, colorref(palette.omnibox_fill));
                    SetTextColor(dc, colorref(palette.omnibox_text));
                    LRESULT(state.brush.0 as isize)
                } else {
                    LRESULT(0)
                }
            }
            WM_COMMAND
                if (wparam.0 & 0xffff) == EDIT_ID && (wparam.0 >> 16) as u32 == EN_CHANGE =>
            {
                if let Some(state) = dialog(hwnd) {
                    let query = read_edit(state.edit);
                    state.state.update(&query, &state.index);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_PALETTE_ACTION if wparam.0 == 3 => {
                if let Some(state) = dialog(hwnd)
                    && let Some(text) = state.state.completion()
                {
                    let _ = SetWindowTextW(state.edit, &windows::core::HSTRING::from(text.as_str()));
                    let end = text.encode_utf16().count();
                    SendMessageW(state.edit, EM_SETSEL, Some(WPARAM(end)), Some(LPARAM(end as isize)));
                }
                LRESULT(0)
            }
            WM_PALETTE_ACTION => {
                if let Some(state) = dialog(hwnd) {
                    let key = match wparam.0 as isize {
                        -1 => 0x26,
                        1 => 0x28,
                        2 => 0x0D,
                        _ => 0x1B,
                    };
                    match state.state.key_down(key) {
                        PaletteKeyAction::Move(_) | PaletteKeyAction::Ignore => {}
                        PaletteKeyAction::Accept(item) => {
                            state.result = item;
                            state.done = true;
                        }
                        PaletteKeyAction::Cancel => finish(state, false),
                    }
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    if state.done {
                        let _ = DestroyWindow(hwnd);
                    }
                }
                LRESULT(0)
            }
            WM_FINISH => {
                if let Some(state) = dialog(hwnd) {
                    finish(state, wparam.0 != 0);
                }
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let y = (lparam.0 >> 16) as u16 as i32;
                if let Some(state) = dialog(hwnd) {
                    if let Some(index) = result_at(y, hwnd, state.state.results.len()) {
                        state.state.selected = index;
                        finish(state, true);
                        let _ = DestroyWindow(hwnd);
                    }
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                if let Some(state) = dialog(hwnd) {
                    state.done = true;
                }
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    parent: usize,
) -> LRESULT {
    // Shift, or Right arrow at the end of the text, completes the box to the top result.
    if message == WM_KEYDOWN && (wparam.0 == 0x10 || wparam.0 == 0x27) {
        let mut end = 0u32;
        unsafe {
            SendMessageW(hwnd, EM_GETSEL, None, Some(LPARAM((&mut end as *mut u32) as isize)));
        }
        if wparam.0 == 0x10 || end as i32 >= unsafe { GetWindowTextLengthW(hwnd) } {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(HWND(parent as *mut c_void)),
                    WM_PALETTE_ACTION,
                    WPARAM(3),
                    LPARAM(0),
                );
            }
            return LRESULT(0);
        }
    }
    if message == WM_KEYDOWN && matches!(wparam.0, 0x26 | 0x28 | 0x0D | 0x1B) {
        let action = match wparam.0 {
            0x26 => -1,
            0x28 => 1,
            0x0D => 2,
            _ => 0,
        };
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                Some(HWND(parent as *mut c_void)),
                WM_PALETTE_ACTION,
                WPARAM(action as usize),
                LPARAM(0),
            );
        }
        return LRESULT(0);
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

fn finish(state: &mut Dialog, accept: bool) {
    state.result = accept
        .then(|| state.state.results.get(state.state.selected).cloned())
        .flatten();
    state.done = true;
}

fn read_edit(hwnd: HWND) -> String {
    unsafe {
        let count = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut buffer = vec![0u16; count + 1];
        let written = GetWindowTextW(hwnd, &mut buffer).max(0) as usize;
        String::from_utf16_lossy(&buffer[..written])
    }
}

fn result_at(y: i32, hwnd: HWND, count: usize) -> Option<usize> {
    let row = scale(54, hwnd);
    let top = scale(112, hwnd);
    (y >= top)
        .then_some(((y - top) / row) as usize)
        .filter(|index| *index < count)
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let p = theme::palette(glass::appearance(), true);
        let brush = CreateSolidBrush(colorref(p.window));
        FillRect(dc, &client, brush);
        let _ = DeleteObject(brush.into());
        smooth::fill_round_rect(
            dc,
            RECT {
                left: 0,
                top: 0,
                right: client.right - 1,
                bottom: client.bottom - 1,
            },
            scale(12, hwnd),
            p.window,
        );
        smooth::stroke_round_rect(
            dc,
            RECT {
                left: 0,
                top: 0,
                right: client.right - 1,
                bottom: client.bottom - 1,
            },
            scale(12, hwnd),
            p.hairline,
            1,
        );
        SetBkMode(dc, TRANSPARENT);
        text(
            dc,
            RECT {
                left: scale(24, hwnd),
                top: scale(16, hwnd),
                right: client.right - scale(24, hwnd),
                bottom: scale(46, hwnd),
            },
            "Search Bowser",
            p.tab_text_active,
            false,
        );
        smooth::fill_round_rect(
            dc,
            RECT {
                left: scale(20, hwnd),
                top: scale(52, hwnd),
                right: client.right - scale(20, hwnd),
                bottom: scale(98, hwnd),
            },
            scale(8, hwnd),
            p.omnibox_fill,
        );
        if let Some(state) = dialog(hwnd) {
            for (i, item) in state.state.results.iter().enumerate() {
                let top = scale(112, hwnd) + i as i32 * scale(54, hwnd);
                if top + scale(52, hwnd) > client.bottom - scale(12, hwnd) {
                    break;
                }
                let bounds = RECT {
                    left: scale(14, hwnd),
                    top,
                    right: client.right - scale(14, hwnd),
                    bottom: top + scale(50, hwnd),
                };
                if i == state.state.selected {
                    smooth::fill_round_rect(dc, bounds, scale(7, hwnd), p.tab_hover);
                }
                text(
                    dc,
                    RECT {
                        left: bounds.left + scale(12, hwnd),
                        top: bounds.top + scale(3, hwnd),
                        right: bounds.right - scale(10, hwnd),
                        bottom: bounds.top + scale(27, hwnd),
                    },
                    &item.label,
                    p.tab_text_active,
                    false,
                );
                text(
                    dc,
                    RECT {
                        left: bounds.left + scale(12, hwnd),
                        top: bounds.top + scale(26, hwnd),
                        right: bounds.right - scale(10, hwnd),
                        bottom: bounds.bottom - scale(2, hwnd),
                    },
                    &item.detail,
                    p.tab_text_inactive,
                    true,
                );
            }
            if state.state.results.is_empty() {
                text(
                    dc,
                    RECT {
                        left: scale(24, hwnd),
                        top: scale(118, hwnd),
                        right: client.right - scale(24, hwnd),
                        bottom: scale(160, hwnd),
                    },
                    "No matching results",
                    p.tab_text_inactive,
                    false,
                );
            }
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

fn text(
    dc: windows::Win32::Graphics::Gdi::HDC,
    mut bounds: RECT,
    value: &str,
    colour: theme::Rgba,
    ellipsis: bool,
) {
    let mut units = value.encode_utf16().collect::<Vec<_>>();
    if units.is_empty() {
        return;
    }
    unsafe {
        SetTextColor(dc, colorref(colour));
        let mut flags = windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE;
        if ellipsis {
            flags |= windows::Win32::Graphics::Gdi::DT_END_ELLIPSIS;
        }
        let _ = DrawTextW(dc, &mut units, &mut bounds, flags);
    }
}

fn colorref(colour: theme::Rgba) -> windows::Win32::Foundation::COLORREF {
    windows::Win32::Foundation::COLORREF(
        u32::from(colour.r) | (u32::from(colour.g) << 8) | (u32::from(colour.b) << 16),
    )
}

fn scale(value: i32, hwnd: HWND) -> i32 {
    unsafe { value * GetDpiForWindow(hwnd).max(96) as i32 / 96 }
}


#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResultItem {
    pub label: String,
    pub detail: String,
    pub action: String,
}

#[derive(Default)]
pub(crate) struct PaletteState {
    pub(crate) query: String,
    pub(crate) results: Vec<ResultItem>,
    pub(crate) selected: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PaletteKeyAction {
    Move(Option<usize>),
    Accept(Option<ResultItem>),
    Cancel,
    Ignore,
}

impl PaletteState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn update(&mut self, query: &str, index: &Index) {
        self.query.clear();
        self.query.push_str(query);
        self.results = search(query, &index.items, &index.options);
        self.selected = self.selected.min(self.results.len().saturating_sub(1));
    }

    /// The text Right arrow completes the box to: the first result's label when it extends what was
    /// typed after any filter prefix (`tab: goo` becomes `tab: google`).
    pub(crate) fn completion(&self) -> Option<String> {
        complete(&self.query, self.results.first().map(|item| item.label.as_str())?)
    }

    pub(crate) fn move_selection(&mut self, delta: isize) -> Option<usize> {
        if self.results.is_empty() {
            return None;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.results.len() - 1);
        Some(self.selected)
    }

    pub(crate) fn key_down(&mut self, key: u32) -> PaletteKeyAction {
        match key {
            0x26 => PaletteKeyAction::Move(self.move_selection(-1)),
            0x28 => PaletteKeyAction::Move(self.move_selection(1)),
            0x0D => PaletteKeyAction::Accept(self.results.get(self.selected).cloned()),
            0x1B => PaletteKeyAction::Cancel,
            _ => PaletteKeyAction::Ignore,
        }
    }
}

/// `typed` completed by `label` when `label` starts with the text after the filter prefix (ignoring
/// case) and is longer. The typed prefix and spacing are kept; only the rest of the label is added.
pub(crate) fn complete(typed: &str, label: &str) -> Option<String> {
    let (filter, rest) = engine::parse_prefix(typed);
    let rest_start = if filter.is_some() { typed.len() - rest.len() } else { 0 };
    let tail = &typed[rest_start..];
    if tail.is_empty() || label.chars().count() <= tail.chars().count() {
        return None;
    }
    let head: String = label.chars().take(tail.chars().count()).collect();
    head.eq_ignore_ascii_case(tail).then(|| format!("{}{label}", &typed[..rest_start]))
}

pub(crate) fn search(query: &str, items: &[Item], options: &Options) -> Vec<ResultItem> {
    engine::search(items, query, options)
        .into_iter()
        .map(|item| ResultItem { label: item.label, detail: item.detail, action: item.action })
        .collect()
}

// --- popup surfaces (lane PP) ---
/// Most open tabs, closed tabs, workspaces, bookmarks, notes and history entries sent to the HTML palette.
const HTML_MAX_TABS: usize = 100;
const HTML_MAX_CLOSED: usize = 16;
const HTML_MAX_WORKSPACES: usize = 32;
const HTML_MAX_BOOKMARKS: usize = 200;
const HTML_MAX_NOTES: usize = 100;
const HTML_MAX_HISTORY: usize = 200;
/// UTF-16 caps for the label and secondary text of one row (the page's own caps match).
pub(crate) const HTML_MAX_LABEL: usize = 128;
pub(crate) const HTML_MAX_DETAIL: usize = 256;
/// Longest shortcut text of a row: the palette shows one chord, the cheat sheet up to six.
pub(crate) const HTML_MAX_ACCEL: usize = 96;
/// Budget for the item list in bytes, well inside `html_popup::MAX_PAGE_BYTES`.
const HTML_ITEMS_BUDGET: usize = 200 * 1024;

/// One row of the HTML palette. `section` is a result type key: `tab`, `closed`, `workspace`,
/// `command`, `action`, `bookmark`, `note` or `history`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HtmlItem {
    pub section: &'static str,
    pub label: String,
    pub detail: String,
    pub accel: Option<String>,
    pub action: String,
    /// Optional `data:image/png;base64,...` favicon for tab, closed, bookmark and history rows.
    pub fav: Option<String>,
    /// The cheat sheet's group heading (a command category); unused by the palette.
    pub group: Option<&'static str>,
    /// A bookmark row's nickname: typing it matches the row first, and opens it when
    /// `palette_open_nickname` is on.
    pub nick: Option<String>,
}

fn cap_utf16(text: &str, max: usize) -> String {
    let mut units = 0;
    for (at, ch) in text.char_indices() {
        units += ch.len_utf16();
        if units > max {
            return text[..at].to_owned();
        }
    }
    text.to_owned()
}

/// The palette's item list, in the user's type order: open tabs, closed tabs, workspaces, commands,
/// page actions, bookmarks, notes, then recent history (newest first, one row per address). Types the
/// user hid are left out. Capped per type and to a byte budget (history goes first, then bookmarks,
/// then notes), so the page message always fits.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn html_items(data: &PaletteData, options: &Options) -> Vec<HtmlItem> {
    html_items_with_favicons(data, options, |_| None, |_| None)
}

/// `html_items` plus favicons: `tab_png` gives an open tab's own favicon by tab id, `host_png` a
/// cached favicon by host (used for closed tabs, bookmarks and history, and for tabs without their
/// own). Only PNGs passing the shared favicon gate are sent, and their bytes count against the
/// item budget.
pub(crate) fn html_items_with_favicons(
    data: &PaletteData,
    options: &Options,
    tab_png: impl Fn(u32) -> Option<Vec<u8>>,
    host_png: impl Fn(&str) -> Option<Vec<u8>>,
) -> Vec<HtmlItem> {
    let shown = |kind: ResultType| options.palette_types.contains(&kind);
    let row = |section, label: &str, detail: &str, accel: Option<String>, action: String| HtmlItem {
        section,
        label: cap_utf16(label, HTML_MAX_LABEL),
        detail: cap_utf16(detail, HTML_MAX_DETAIL),
        accel,
        action,
        fav: None,
        group: None,
        nick: None,
    };
    let mut fixed = Vec::new();
    for kind in &options.palette_types {
        match kind {
            ResultType::Tab => {
                for (id, title, url) in data.tabs.iter().take(HTML_MAX_TABS) {
                    let label = if title.trim().is_empty() { url.as_str() } else { title.as_str() };
                    fixed.push(row("tab", label, url, None, format!("tab:{id}")));
                }
            }
            ResultType::Closed => {
                for (title, url) in data.closed.iter().filter(|(_, url)| is_web_url(url)).take(HTML_MAX_CLOSED) {
                    fixed.push(row("closed", if title.trim().is_empty() { url } else { title }, url, None, format!("reopen:{url}")));
                }
            }
            ResultType::Workspace => {
                for (id, name, tabs) in data.workspaces.iter().take(HTML_MAX_WORKSPACES) {
                    fixed.push(row("workspace", name, &tab_count(*tabs), None, format!("workspace:{id}")));
                }
            }
            ResultType::Command | ResultType::Action => {
                let wanted = if *kind == ResultType::Action { PaletteKind::Action } else { PaletteKind::Command };
                for command in data.commands.iter().filter(|c| c.kind == wanted) {
                    let section = if *kind == ResultType::Action { "action" } else { "command" };
                    fixed.push(row(section, &command.label, &command.detail, command.accel.as_deref().map(|a| cap_utf16(a, HTML_MAX_ACCEL)), command.id.clone()));
                }
            }
            ResultType::Bookmark | ResultType::Note | ResultType::History | ResultType::Calc => {}
        }
    }
    let mut marks: Vec<HtmlItem> = if shown(ResultType::Bookmark) {
        data.bookmarks
            .iter()
            .filter(|(_, url, _)| is_web_url(url))
            .take(HTML_MAX_BOOKMARKS)
            .map(|(title, url, folder)| {
                let detail = if folder.is_empty() { url.clone() } else { format!("{url}  ·  {folder}") };
                let mut item = row("bookmark", if title.trim().is_empty() { url } else { title }, &detail, None, format!("url:{url}"));
                item.nick = data.nickname_of(url).map(|nick| nick.chars().take(HTML_MAX_NICK).collect());
                item
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut notes: Vec<HtmlItem> = if shown(ResultType::Note) {
        data.notes
            .iter()
            .take(HTML_MAX_NOTES)
            .map(|(id, title, body)| row("note", &note_label(title, body), &note_detail(body), None, format!("note:{id}")))
            .filter(|item| !item.label.trim().is_empty())
            .collect()
    } else {
        Vec::new()
    };
    let mut seen = std::collections::HashSet::new();
    let mut recent: Vec<HtmlItem> = if shown(ResultType::History) {
        data.history
            .iter()
            .rev()
            .filter(|(_, url, _)| is_web_url(url) && seen.insert(url.clone()))
            .take(HTML_MAX_HISTORY)
            .map(|(_, url, title)| row("history", if title.trim().is_empty() { url } else { title }, url, None, format!("url:{url}")))
            .collect()
    } else {
        Vec::new()
    };
    let cost = |items: &[HtmlItem]| -> usize {
        items
            .iter()
            .map(|item| {
                crate::chrome_host::protocol::json_string(&item.label).len()
                    + crate::chrome_host::protocol::json_string(&item.detail).len()
                    + item.fav.as_ref().map_or(0, |fav| fav.len() + 8)
                    + 64
            })
            .sum()
    };
    let fixed_cost = cost(&fixed);
    while fixed_cost + cost(&marks) + cost(&notes) + cost(&recent) > HTML_ITEMS_BUDGET && !(recent.is_empty() && marks.is_empty() && notes.is_empty()) {
        if !recent.is_empty() {
            recent.truncate(recent.len() / 2);
        } else if !marks.is_empty() {
            marks.truncate(marks.len() / 2);
        } else {
            notes.truncate(notes.len() / 2);
        }
    }
    fixed.extend(marks);
    fixed.extend(notes);
    fixed.extend(recent);
    // Rows of one type stay together and the types follow the user's order (a stable sort).
    fixed.sort_by_key(|item| options.palette_types.iter().position(|kind| kind.key() == item.section));
    // Favicons are added last, in row order, into whatever budget the text left over; a row that
    // does not fit simply has none.
    let mut remaining = HTML_ITEMS_BUDGET.saturating_sub(cost(&fixed));
    let mut by_host: std::collections::HashMap<String, Option<String>> = std::collections::HashMap::new();
    for item in &mut fixed {
        if !matches!(item.section, "tab" | "closed" | "bookmark" | "history") {
            continue;
        }
        let own = item.action.strip_prefix("tab:").and_then(|id| id.parse::<u32>().ok()).and_then(&tab_png);
        let mut uri = own.as_deref().and_then(crate::chrome_host::protocol::favicon_data_uri);
        if uri.is_none() {
            let address = item.action.strip_prefix("url:").or_else(|| item.action.strip_prefix("reopen:")).unwrap_or(&item.detail);
            let host = url::Url::parse(address)
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned));
            if let Some(host) = host {
                uri = by_host
                    .entry(host.clone())
                    .or_insert_with(|| host_png(&host).as_deref().and_then(crate::chrome_host::protocol::favicon_data_uri))
                    .clone();
            }
        }
        if let Some(uri) = uri.filter(|uri| uri.len() + 8 <= remaining) {
            remaining -= uri.len() + 8;
            item.fav = Some(uri);
        }
    }
    fixed
}

fn is_web_url(text: &str) -> bool {
    url::Url::parse(text).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

/// The `{t:"palette"}` page for `items`; every row is pickable by its index. `options` travels with it:
/// the result types in their order, whether the order is the default (best match first), the number of
/// rows to show for a typed search and whether the box keeps its last text.
pub(crate) fn html_page(items: &[HtmlItem], options: &Options) -> super::html_popup::Page {
    let rows: Vec<HtmlItem> = items.to_vec();
    let count = rows.len();
    let (types, ranked, limit, keep, autonick) = (
        options.palette_types.iter().map(|kind| kind.key()).collect::<Vec<_>>(),
        options.palette_ranked(),
        u64::from(options.palette_max_results),
        options.palette_keep_last,
        options.palette_open_nickname,
    );
    let mut page = super::html_popup::Page::new("palette", move |out| {
        out.string("placeholder", "Search tabs, bookmarks, notes, history and commands");
        out.string("mode", "palette");
        out.array("types", |list| for key in &types { list.string(key); });
        out.bool("ranked", ranked);
        out.uint("limit", limit);
        out.bool("keep", keep);
        out.bool("autonick", autonick);
        write_items(out, &rows);
    });
    for index in 0..count {
        page.pick(index as u32);
    }
    page
}

fn write_items(out: &mut crate::chrome_host::json::Object<'_>, rows: &[HtmlItem]) {
    out.array("items", |list| {
        for (index, item) in rows.iter().enumerate() {
            list.object(|row| {
                row.uint("id", index as u64);
                row.string("s", item.section);
                row.string("label", &item.label);
                row.string("detail", &item.detail);
                if let Some(accel) = &item.accel {
                    row.string("acc", accel);
                }
                if let Some(fav) = &item.fav {
                    row.string("fav", fav);
                }
                if let Some(group) = item.group {
                    row.string("g", group);
                }
                if let Some(nick) = &item.nick {
                    row.string("k", nick);
                }
            });
        }
    });
}

/// The cheat sheet (Ctrl+F1): every command that has a shortcut now, grouped by category, with all
/// of its shortcuts. Rows are not pickable: it is a reference, and Escape closes it.
pub(crate) fn shortcuts_items(live: &crate::commands::Live) -> Vec<HtmlItem> {
    use crate::commands::{chord::Chord, registry::Category};
    let entries = live.store.entries();
    let mut rows = Vec::new();
    for category in Category::ALL {
        for entry in entries.iter().filter(|entry| entry.category == category) {
            let chords: Vec<String> = live.store.effective(entry).iter().filter_map(Chord::format).collect();
            if chords.is_empty() {
                continue;
            }
            rows.push(HtmlItem {
                section: "command",
                label: cap_utf16(&entry.label, HTML_MAX_LABEL),
                detail: cap_utf16(&entry.detail, HTML_MAX_DETAIL),
                accel: Some(cap_utf16(&chords.join("   "), HTML_MAX_ACCEL)),
                action: entry.id.clone(),
                fav: None,
                group: Some(category.label()),
                nick: None,
            });
        }
    }
    rows
}

/// The `{t:"palette"}` page in `shortcuts` mode. Nothing in it can be picked.
pub(crate) fn shortcuts_page(items: &[HtmlItem], shortcuts_on: bool) -> super::html_popup::Page {
    let rows: Vec<HtmlItem> = items.to_vec();
    super::html_popup::Page::new("palette", move |out| {
        out.string("placeholder", if shortcuts_on { "Filter shortcuts" } else { "Shortcuts are switched off in Settings" });
        out.string("mode", "shortcuts");
        out.array("types", |_| {});
        out.bool("ranked", true);
        out.uint("limit", 0);
        out.bool("keep", false);
        out.bool("autonick", false);
        write_items(out, &rows);
    })
}
// --- end popup surfaces ---

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::commands::{Live, palette_commands, store::Store};

    fn data() -> PaletteData {
        let live = Live::new(Store::default());
        PaletteData {
            tabs: vec![(4, "Daily notes".into(), "https://notes.example/".into()), (5, " ".into(), "https://blank.example/".into())],
            closed: vec![("Old page".into(), "https://old.example/".into()), ("Local".into(), "file:///C:/x".into())],
            workspaces: vec![(1, "Work".into(), 3), (2, "Home".into(), 1)],
            bookmarks: vec![("Plan".into(), "https://plan.example/".into(), "Work".into()), ("Local".into(), "file:///C:/x".into(), String::new())],
            notes: vec![(7, "Shopping".into(), "milk\nand tea".into()), (8, String::new(), "Untitled body line\nsecond".into())],
            history: vec![
                (1, "https://a.example/".into(), "A".into()),
                (2, "https://b.example/".into(), "B".into()),
                (3, "https://a.example/".into(), "A again".into()),
                (4, "javascript:alert(1)".into(), "Bad".into()),
            ],
            commands: palette_commands(&live),
            nicknames: vec![("https://plan.example/".into(), "zq".into())],
        }
    }

    /// Integration: T2 raised the address drop-down to 20 rows; the wire carries that many, and Quick
    /// Commands' own limit (3 to 30, its page's MAX_LIMIT) still reaches past it.
    #[test]
    fn the_palette_and_drop_down_result_caps_agree() {
        use crate::{address_settings::MAX_DROPDOWN_RESULTS, chrome_host::protocol::MAX_SUGGEST_ITEMS, commands::store::MAX_RESULTS};
        assert_eq!(usize::from(MAX_DROPDOWN_RESULTS), MAX_SUGGEST_ITEMS, "every row the setting allows can be sent");
        const { assert!(MAX_RESULTS >= MAX_DROPDOWN_RESULTS) };
        let page = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/src/palette-logic.ts")).unwrap();
        assert!(page.contains(&format!("export const MAX_LIMIT = {MAX_RESULTS};")), "the page's limit matches the store's");
    }

    /// Integration (T2 bookmark nicknames, T4 Quick Commands): a bookmark row carries its nickname,
    /// the page is told whether a nickname match opens at once, and the native fallback finds it.
    #[test]
    fn bookmark_nicknames_travel_with_their_rows_and_the_option_with_the_page() {
        let mut options = Options::default();
        assert!(options.palette_open_nickname, "on by default, as in Vivaldi");
        let items = html_items(&data(), &options);
        let plan = items.iter().find(|item| item.action == "url:https://plan.example/").unwrap();
        assert_eq!(plan.nick.as_deref(), Some("zq"));
        assert!(items.iter().filter(|item| item.section != "bookmark").all(|item| item.nick.is_none()));
        let json = super::super::html_popup::Page::debug_json(html_page(&items, &options), 2);
        assert!(json.contains("\"k\":\"zq\"") && json.contains("\"autonick\":true"), "{json}");
        options.palette_open_nickname = false;
        let json = super::super::html_popup::Page::debug_json(html_page(&items, &options), 2);
        assert!(json.contains("\"autonick\":false"));
        let found = search("zq", &data().items(), &options);
        assert!(found.iter().any(|item| item.action == "url:https://plan.example/"), "the fallback searches nicknames too");
    }

    fn sections(items: &[HtmlItem]) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for item in items {
            if out.last() != Some(&item.section) {
                out.push(item.section);
            }
        }
        out
    }

    #[test]
    fn html_palette_lists_every_source_in_the_users_order_with_real_accelerators_and_index_ids() {
        let options = Options::default();
        let items = html_items(&data(), &options);
        assert_eq!(sections(&items), ["tab", "closed", "workspace", "command", "action", "bookmark", "note", "history"]);
        assert_eq!(items[1].label, "https://blank.example/", "a blank title shows the address");
        let by = |section: &str| -> Vec<&HtmlItem> { items.iter().filter(|i| i.section == section).collect() };
        assert_eq!(by("closed").len(), 1, "only web pages reopen");
        assert_eq!(by("closed")[0].action, "reopen:https://old.example/");
        assert_eq!(by("workspace")[0].detail, "3 tabs");
        assert_eq!(by("workspace")[1].detail, "1 tab");
        assert_eq!(by("workspace")[1].action, "workspace:2");
        assert_eq!(by("bookmark").len(), 1, "only web bookmarks");
        let notes: Vec<_> = by("note").iter().map(|i| (i.label.as_str(), i.detail.as_str(), i.action.as_str())).collect();
        assert_eq!(notes, [("Shopping", "milk and tea", "note:7"), ("Untitled body line", "Untitled body line second", "note:8")]);
        let history: Vec<_> = by("history").iter().map(|i| i.label.as_str()).collect();
        assert_eq!(history, ["A again", "B"], "newest first, one row per address, no script URLs");
        let reload = items.iter().find(|i| i.action == "reload").unwrap();
        assert_eq!((reload.section, reload.accel.as_deref()), ("action", Some("F5")), "page actions have their own type");
        let new_tab = items.iter().find(|i| i.action == "new-tab").unwrap();
        assert_eq!((new_tab.section, new_tab.accel.as_deref()), ("command", Some("Ctrl+T")));
        let json = super::super::html_popup::Page::debug_json(html_page(&items, &options), 3);
        assert!(json.starts_with("{\"v\":1,\"t\":\"palette\",\"tok\":3,\"kind\":\"palette\""));
        assert!(json.contains("\"acc\":\"Ctrl+T\""));
        assert!(json.contains("\"mode\":\"palette\"") && json.contains("\"ranked\":true") && json.contains("\"limit\":12") && json.contains("\"keep\":false"));
        assert!(json.contains("\"types\":[\"tab\",\"closed\",\"workspace\",\"command\",\"action\",\"bookmark\",\"note\",\"history\",\"calc\"]"));
        assert!(json.contains(&format!("\"id\":{}", items.len() - 1)));
    }

    #[test]
    fn hidden_types_are_not_sent_and_the_options_travel_with_the_page() {
        let mut options = Options::default();
        options.palette_types = vec![ResultType::Note, ResultType::Command];
        options.palette_max_results = 5;
        options.palette_keep_last = true;
        let items = html_items(&data(), &options);
        assert_eq!(sections(&items), ["note", "command"], "only the chosen types, in the chosen order");
        let json = super::super::html_popup::Page::debug_json(html_page(&items, &options), 1);
        assert!(json.contains("\"types\":[\"note\",\"command\"]") && json.contains("\"ranked\":false"), "{json}");
        assert!(json.contains("\"limit\":5") && json.contains("\"keep\":true"));
    }

    #[test]
    fn html_palette_stays_within_its_byte_budget() {
        let long = "é".repeat(400);
        let mut big = data();
        big.bookmarks = (0..500).map(|i| (long.clone(), format!("https://b{i}.example/{long}"), String::new())).collect();
        big.history = (0..500).map(|i| (i as u64, format!("https://h{i}.example/{long}"), long.clone())).collect();
        big.notes = (0..100).map(|i| (i as u64, long.clone(), long.repeat(5))).collect();
        let options = Options::default();
        let items = html_items(&big, &options);
        assert!(items.iter().all(|i| i.label.encode_utf16().count() <= HTML_MAX_LABEL && i.detail.encode_utf16().count() <= HTML_MAX_DETAIL));
        let json = super::super::html_popup::Page::debug_json(html_page(&items, &options), 1);
        assert!(json.len() <= super::super::html_popup::MAX_PAGE_BYTES, "{}", json.len());
        assert!(items.iter().any(|i| i.section == "bookmark"));
    }

    fn fake_png(len: usize) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 16, 0, 0, 0, 16];
        bytes.resize(len, 0);
        bytes
    }

    #[test]
    fn html_palette_attaches_favicons_to_page_rows_only() {
        let icon = fake_png(40);
        let mut small = PaletteData::default();
        small.tabs = vec![(4, "Mine".into(), "https://own.example/".into()), (5, "Cached".into(), "https://cached.example/x".into())];
        small.closed = vec![("Closed".into(), "https://old.example/".into())];
        small.bookmarks = vec![("Plan".into(), "https://plan.example/".into(), "Work".into())];
        small.notes = vec![(1, "Note".into(), "text".into())];
        small.history = vec![(1, "https://news.example/".into(), "News".into())];
        small.commands = palette_commands(&Live::new(Store::default()));
        let items = html_items_with_favicons(
            &small,
            &Options::default(),
            |id| (id == 4).then(|| icon.clone()),
            |host| (host != "news.example").then(|| icon.clone()),
        );
        let uri = crate::chrome_host::protocol::favicon_data_uri(&icon).unwrap();
        for item in &items {
            match item.section {
                "command" | "action" | "note" | "workspace" => assert_eq!(item.fav, None, "{}", item.label),
                "history" => assert_eq!(item.fav, None, "no cached icon for that host"),
                _ => assert_eq!(item.fav.as_deref(), Some(uri.as_str()), "{}", item.label),
            }
        }
        let json = super::super::html_popup::Page::debug_json(html_page(&items, &Options::default()), 1);
        assert!(json.contains(&format!("\"fav\":\"{uri}\"")));
        assert!(!html_items(&small, &Options::default()).iter().any(|i| i.fav.is_some()));
    }

    #[test]
    fn html_palette_favicon_bytes_stay_inside_the_budget() {
        let long = "é".repeat(400);
        let mut big = PaletteData::default();
        big.bookmarks = (0..500).map(|i| (long.clone(), format!("https://b{i}.example/{long}"), String::new())).collect();
        big.tabs = (0..100).map(|i| (i, "T".to_owned(), format!("https://t{i}.example/"))).collect();
        big.commands = palette_commands(&Live::new(Store::default()));
        let png = fake_png(crate::chrome_host::protocol::MAX_FAVICON_BYTES);
        let items = html_items_with_favicons(&big, &Options::default(), |_| Some(png.clone()), |_| Some(png.clone()));
        assert!(items.iter().any(|i| i.fav.is_some()));
        let json = super::super::html_popup::Page::debug_json(html_page(&items, &Options::default()), 1);
        assert!(json.len() <= super::super::html_popup::MAX_PAGE_BYTES, "{}", json.len());
    }

    #[test]
    fn the_cheat_sheet_groups_every_bound_command_with_all_its_shortcuts() {
        let mut store = Store::default();
        store.assign("reload", "Ctrl+Shift+F", false).unwrap();
        store.clear("new-private-tab");
        let live = Live::new(store);
        let items = shortcuts_items(&live);
        let find = |id: &str| items.iter().find(|i| i.action == id);
        assert_eq!(find("close-tab").unwrap().accel.as_deref(), Some("Ctrl+W   Ctrl+F4"));
        assert_eq!(find("reload").unwrap().accel.as_deref(), Some("F5   Ctrl+R   BrowserRefresh   Ctrl+Shift+F"));
        assert!(find("new-private-tab").is_none(), "a cleared command is not listed");
        assert!(find("save-session").is_none(), "nor one that never had a shortcut");
        assert_eq!(find("shortcuts-cheat-sheet").unwrap().accel.as_deref(), Some("Ctrl+F1"));
        let groups: Vec<_> = items.iter().filter_map(|i| i.group).fold(Vec::new(), |mut acc, g| {
            if acc.last() != Some(&g) {
                acc.push(g);
            }
            acc
        });
        assert_eq!(groups.first(), Some(&"Tabs"));
        assert!(groups.len() >= 5, "{groups:?}");
        let json = super::super::html_popup::Page::debug_json(shortcuts_page(&items, true), 2);
        assert!(json.contains("\"mode\":\"shortcuts\"") && json.contains("\"g\":\"Tabs\""));
        let page_json = super::super::html_popup::Page::debug_json(shortcuts_page(&items, true), 2);
        assert!(page_json.len() <= super::super::html_popup::MAX_PAGE_BYTES, "{}", page_json.len());
    }

    #[test]
    fn typing_completes_to_the_top_result_after_any_prefix() {
        assert_eq!(complete("goo", "Google"), Some("Google".into()));
        assert_eq!(complete("tab: goo", "Google"), Some("tab: Google".into()));
        assert_eq!(complete("TB:   goo", "Google"), Some("TB:   Google".into()));
        assert_eq!(complete("google", "Google"), None, "nothing left to add");
        assert_eq!(complete("gx", "Google"), None);
        assert_eq!(complete("", "Google"), None);
        assert_eq!(complete("tab:", "Google"), None);
    }

    #[test]
    fn live_palette_reranks_each_query_and_moves_selection_within_results() {
        let index = Index {
            items: vec![
                Item { kind: ResultType::Command, label: "Open settings".into(), detail: "Browser settings".into(), action: "settings".into() },
                Item { kind: ResultType::Command, label: "Save session".into(), detail: "Save current tabs".into(), action: "save-session".into() },
            ],
            options: Options::default(),
        };
        let mut state = PaletteState::new();
        state.update("set", &index);
        assert_eq!(state.results[0].action, "settings");
        assert_eq!(state.completion().as_deref(), None, "the label starts with Open, not set");
        state.update("sav", &index);
        assert_eq!(state.results[0].action, "save-session");
        assert_eq!(state.completion().as_deref(), Some("Save session"));
        assert_eq!(state.selected, 0);
        state.update("", &index);
        assert_eq!(state.key_down(0x28), PaletteKeyAction::Move(Some(1)));
        assert_eq!(state.key_down(0x26), PaletteKeyAction::Move(Some(0)));
        assert!(matches!(state.key_down(0x0D), PaletteKeyAction::Accept(Some(item)) if item.action == "settings"));
        assert_eq!(state.key_down(0x1B), PaletteKeyAction::Cancel);
    }

    #[test]
    fn the_native_fallback_searches_every_type_through_the_same_engine() {
        let items = data_items();
        let options = Options::default();
        let found = search("note: tea", &items, &options);
        assert_eq!(found.iter().map(|r| r.action.as_str()).collect::<Vec<_>>(), ["note:7"]);
        assert_eq!(search("tb: old", &items, &options)[0].action, "reopen:https://old.example/");
        assert_eq!(search("workspace: wor", &items, &options)[0].action, "workspace:1");
        assert_eq!(search("2+2", &items, &options)[0].action, "copy:4");
        assert!(search("reload", &items, &options).iter().any(|r| r.action == "reload"));
    }

    fn data_items() -> Vec<Item> {
        data().items()
    }
}
