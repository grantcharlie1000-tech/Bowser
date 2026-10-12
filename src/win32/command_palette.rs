//! Search model for the themed command prompt. The index is assembled only when opened.

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

#[cfg(test)]
mod tests {
    use super::*;

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
