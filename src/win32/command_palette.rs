//! Search model for the command palette (Ctrl+Shift+K). The index is assembled only when opened.
//!
//! With the HTML chrome the palette is a page in the popup host (`html_page`, `{t:"palette"}`):
//! Rust sends the item list, the page filters it, and a pick names an item by index, which Rust
//! maps back to the action it built. NATIVE FALLBACK: `show` below (a GDI window with an EDIT
//! control) runs only when the HTML chrome is off or the popup host is not ready yet.

use crate::productivity::{self, CommandRegistry, SearchItem};
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
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::SetFocus,
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::{
                CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                EN_CHANGE, ES_AUTOHSCROLL, GWLP_USERDATA, GetClientRect, GetMessageW,
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

struct Index {
    tabs: Vec<(u32, String, String)>,
    bookmarks: Vec<(String, String, String)>,
    history: Vec<(u64, String, String)>,
    commands: CommandRegistry,
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
pub(crate) fn show(
    owner: HWND,
    tabs: Vec<(u32, String, String)>,
    bookmarks: Vec<(String, String, String)>,
    history: Vec<(u64, String, String)>,
    commands: CommandRegistry,
) -> Option<ResultItem> {
    register_class()?;
    let palette = theme::palette(glass::appearance(), true);
    let mut state = Box::new(Dialog {
        edit: HWND::default(),
        state: PaletteState::new(),
        index: Index {
            tabs,
            bookmarks,
            history,
            commands,
        },
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
                    state.state.update(
                        "",
                        &state.index.tabs,
                        &state.index.bookmarks,
                        &state.index.history,
                        &state.index.commands,
                    );
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
                    state.state.update(
                        &query,
                        &state.index.tabs,
                        &state.index.bookmarks,
                        &state.index.history,
                        &state.index.commands,
                    );
                    let _ = InvalidateRect(Some(hwnd), None, false);
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

    pub(crate) fn update(
        &mut self,
        query: &str,
        tabs: &[(u32, String, String)],
        bookmarks: &[(String, String, String)],
        history: &[(u64, String, String)],
        commands: &CommandRegistry,
    ) {
        self.query.clear();
        self.query.push_str(query);
        self.results = search(query, tabs, bookmarks, history, commands);
        self.selected = self.selected.min(self.results.len().saturating_sub(1));
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

pub(crate) fn search(
    query: &str,
    tabs: &[(u32, String, String)],
    bookmarks: &[(String, String, String)],
    history: &[(u64, String, String)],
    commands: &CommandRegistry,
) -> Vec<ResultItem> {
    let mut items = Vec::new();
    for (id, title, url) in tabs {
        items.push(SearchItem {
            label: title.clone(),
            detail: url.clone(),
            action: format!("tab:{id}"),
        });
    }
    for (title, url, folder) in bookmarks {
        items.push(SearchItem {
            label: title.clone(),
            detail: format!("{url} {folder}"),
            action: format!("url:{url}"),
        });
    }
    for (_, url, title) in history.iter().rev() {
        items.push(SearchItem {
            label: title.clone(),
            detail: url.clone(),
            action: format!("url:{url}"),
        });
    }
    items.extend(commands.commands().iter().cloned());
    productivity::rank_items(&items, query, 7)
        .into_iter()
        .map(|(item, _)| ResultItem {
            label: item.label.clone(),
            detail: item.detail.clone(),
            action: item.action.clone(),
        })
        .collect()
}

// --- popup surfaces (lane PP) ---
/// Most open tabs, bookmarks and history entries sent to the HTML palette.
const HTML_MAX_TABS: usize = 100;
const HTML_MAX_BOOKMARKS: usize = 200;
const HTML_MAX_HISTORY: usize = 200;
/// UTF-16 caps for the label and secondary text of one row (the page's own caps match).
pub(crate) const HTML_MAX_LABEL: usize = 128;
pub(crate) const HTML_MAX_DETAIL: usize = 256;
/// Budget for the item list in bytes, well inside `html_popup::MAX_PAGE_BYTES`.
const HTML_ITEMS_BUDGET: usize = 200 * 1024;

/// Shortcut labels for palette commands. Each one is a real binding (`shortcuts::command_for`,
/// checked by a test), shown right-aligned in the row.
const ACCELERATORS: [(&str, &str); 11] = [
    ("new-tab", "Ctrl+T"),
    ("new-private-tab", "Ctrl+Shift+P"),
    ("close-tab", "Ctrl+W"),
    ("toggle-sidebar", "Ctrl+Shift+N"),
    ("passwords", "Ctrl+,"),
    ("profiles", "Ctrl+Shift+M"),
    ("add-bookmark", "Ctrl+D"),
    ("reload", "Ctrl+R"),
    ("back", "Alt+Left"),
    ("forward", "Alt+Right"),
    ("extensions", "Ctrl+Shift+E"),
];

/// One row of the HTML palette. `section` is `tab`, `command`, `bookmark` or `history`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HtmlItem {
    pub section: &'static str,
    pub label: String,
    pub detail: String,
    pub accel: Option<&'static str>,
    pub action: String,
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

/// The palette's item list: open tabs, commands, bookmarks, then recent history (newest first,
/// one row per address). Capped per section and to a byte budget (history goes first, then
/// bookmarks), so the page message always fits.
pub(crate) fn html_items(
    tabs: &[(u32, String, String)],
    bookmarks: &[(String, String, String)],
    history: &[(u64, String, String)],
    commands: &CommandRegistry,
) -> Vec<HtmlItem> {
    let row = |section, label: &str, detail: &str, accel, action: String| HtmlItem {
        section,
        label: cap_utf16(label, HTML_MAX_LABEL),
        detail: cap_utf16(detail, HTML_MAX_DETAIL),
        accel,
        action,
    };
    let mut fixed = Vec::new();
    for (id, title, url) in tabs.iter().take(HTML_MAX_TABS) {
        let label = if title.trim().is_empty() { url.as_str() } else { title.as_str() };
        fixed.push(row("tab", label, url, None, format!("tab:{id}")));
    }
    for item in commands.commands() {
        let accel = ACCELERATORS.iter().find(|(action, _)| *action == item.action).map(|(_, a)| *a);
        fixed.push(row("command", &item.label, &item.detail, accel, item.action.clone()));
    }
    let mut marks: Vec<HtmlItem> = bookmarks
        .iter()
        .filter(|(_, url, _)| is_web_url(url))
        .take(HTML_MAX_BOOKMARKS)
        .map(|(title, url, folder)| {
            let detail = if folder.is_empty() { url.clone() } else { format!("{url}  ·  {folder}") };
            row("bookmark", if title.trim().is_empty() { url } else { title }, &detail, None, format!("url:{url}"))
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut recent: Vec<HtmlItem> = history
        .iter()
        .rev()
        .filter(|(_, url, _)| is_web_url(url) && seen.insert(url.clone()))
        .take(HTML_MAX_HISTORY)
        .map(|(_, url, title)| row("history", if title.trim().is_empty() { url } else { title }, url, None, format!("url:{url}")))
        .collect();
    let cost = |items: &[HtmlItem]| -> usize {
        items
            .iter()
            .map(|item| {
                crate::chrome_host::protocol::json_string(&item.label).len()
                    + crate::chrome_host::protocol::json_string(&item.detail).len()
                    + 64
            })
            .sum()
    };
    let fixed_cost = cost(&fixed);
    while fixed_cost + cost(&marks) + cost(&recent) > HTML_ITEMS_BUDGET && (!recent.is_empty() || !marks.is_empty()) {
        if !recent.is_empty() {
            recent.truncate(recent.len() / 2);
        } else {
            marks.truncate(marks.len() / 2);
        }
    }
    fixed.extend(marks);
    fixed.extend(recent);
    fixed
}

fn is_web_url(text: &str) -> bool {
    url::Url::parse(text).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

/// The `{t:"palette"}` page for `items`; every row is pickable by its index.
pub(crate) fn html_page(items: &[HtmlItem]) -> super::html_popup::Page {
    let rows: Vec<HtmlItem> = items.to_vec();
    let count = rows.len();
    let mut page = super::html_popup::Page::new("palette", move |out| {
        out.string("placeholder", "Search tabs, bookmarks, history and commands");
        out.array("items", |list| {
            for (index, item) in rows.iter().enumerate() {
                list.object(|row| {
                    row.uint("id", index as u64);
                    row.string("s", item.section);
                    row.string("label", &item.label);
                    row.string("detail", &item.detail);
                    if let Some(accel) = item.accel {
                        row.string("acc", accel);
                    }
                });
            }
        });
    });
    for index in 0..count {
        page.pick(index as u32);
    }
    page
}
// --- end popup surfaces ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_palette_lists_every_source_with_real_accelerators_and_index_ids() {
        let commands = productivity::browser_commands();
        let items = html_items(
            &[(4, "Daily notes".into(), "https://notes.example/".into()), (5, " ".into(), "https://blank.example/".into())],
            &[("Plan".into(), "https://plan.example/".into(), "Work".into()), ("Local".into(), "file:///C:/x".into(), String::new())],
            &[
                (1, "https://a.example/".into(), "A".into()),
                (2, "https://b.example/".into(), "B".into()),
                (3, "https://a.example/".into(), "A again".into()),
                (4, "javascript:alert(1)".into(), "Bad".into()),
            ],
            &commands,
        );
        let sections: Vec<_> = items.iter().map(|item| item.section).collect();
        assert_eq!(&sections[..2], ["tab", "tab"]);
        assert_eq!(items[1].label, "https://blank.example/", "a blank title shows the address");
        assert_eq!(sections.iter().filter(|s| **s == "command").count(), commands.commands().len());
        assert_eq!(items.iter().filter(|i| i.section == "bookmark").count(), 1, "only web bookmarks");
        let history: Vec<_> = items.iter().filter(|i| i.section == "history").map(|i| i.label.as_str()).collect();
        assert_eq!(history, ["A again", "B"], "newest first, one row per address, no script URLs");
        for (action, accel) in ACCELERATORS {
            assert!(commands.commands().iter().any(|item| item.action == action), "{action} is a palette command");
            let (key, ctrl, shift, alt) = parse_accel(accel);
            assert!(crate::shortcuts::command_for(key, ctrl, shift, alt).is_some(), "{accel} is bound");
        }
        let json = super::super::html_popup::Page::debug_json(html_page(&items), 3);
        assert!(json.starts_with("{\"v\":1,\"t\":\"palette\",\"tok\":3,\"kind\":\"palette\""));
        assert!(json.contains("\"acc\":\"Ctrl+T\""));
        assert!(json.contains(&format!("\"id\":{}", items.len() - 1)));
    }

    #[test]
    fn html_palette_stays_within_its_byte_budget() {
        let long = "é".repeat(400);
        let bookmarks: Vec<_> = (0..500).map(|i| (long.clone(), format!("https://b{i}.example/{long}"), String::new())).collect();
        let history: Vec<_> = (0..500).map(|i| (i as u64, format!("https://h{i}.example/{long}"), long.clone())).collect();
        let items = html_items(&[], &bookmarks, &history, &productivity::browser_commands());
        assert!(items.iter().all(|i| i.label.encode_utf16().count() <= HTML_MAX_LABEL && i.detail.encode_utf16().count() <= HTML_MAX_DETAIL));
        let json = super::super::html_popup::Page::debug_json(html_page(&items), 1);
        assert!(json.len() <= super::super::html_popup::MAX_PAGE_BYTES, "{}", json.len());
        assert!(items.iter().any(|i| i.section == "bookmark"));
    }

    fn parse_accel(text: &str) -> (u32, bool, bool, bool) {
        let mut parts: Vec<&str> = text.split('+').collect();
        let key = parts.pop().unwrap();
        let key = match key {
            "Left" => 0x25,
            "Right" => 0x27,
            "" => 0xBC,
            k => u32::from(k.as_bytes()[0]),
        };
        // "Ctrl+," splits into ["Ctrl", "", ""]: the comma key.
        let key = if text.ends_with(",") { 0xBC } else { key };
        (key, parts.contains(&"Ctrl"), parts.contains(&"Shift"), parts.contains(&"Alt"))
    }

    #[test]
    fn palette_search_indexes_tabs_bookmarks_history_and_registered_commands_lazily() {
        let mut registry = CommandRegistry::default();
        registry.register("New tab", "Open a blank tab", "new-tab");
        let found = search(
            "daily",
            &[(11, "Daily notes".into(), "https://notes.example/".into())],
            &[(
                "Daily plan".into(),
                "https://plan.example/".into(),
                "Work".into(),
            )],
            &[(1, "https://archive.example/".into(), "Daily archive".into())],
            &registry,
        );
        assert_eq!(found.len(), 3);
        assert!(found.iter().any(|item| item.action == "tab:11"));
        assert!(
            found
                .iter()
                .any(|item| item.action == "url:https://plan.example/")
        );
        assert!(
            found
                .iter()
                .any(|item| item.action == "url:https://archive.example/")
        );
        assert!(
            search("new tab", &[], &[], &[], &registry)
                .iter()
                .any(|item| item.action == "new-tab")
        );
    }

    #[test]
    fn live_palette_reranks_each_query_and_moves_selection_within_results() {
        let mut registry = CommandRegistry::default();
        registry.register("Open settings", "Browser settings", "settings");
        registry.register("Save session", "Save current tabs", "save-session");
        let mut state = PaletteState::new();
        state.update("set", &[], &[], &[], &registry);
        assert_eq!(state.results[0].action, "settings");
        state.update("sav", &[], &[], &[], &registry);
        assert_eq!(state.results[0].action, "save-session");
        assert_eq!(state.selected, 0);
        state.update("", &[], &[], &[], &registry);
        assert_eq!(state.key_down(0x28), PaletteKeyAction::Move(Some(1)));
        assert_eq!(state.key_down(0x26), PaletteKeyAction::Move(Some(0)));
        assert!(
            matches!(state.key_down(0x0D), PaletteKeyAction::Accept(Some(item)) if item.action == "settings")
        );
        assert_eq!(state.key_down(0x1B), PaletteKeyAction::Cancel);
    }
}
