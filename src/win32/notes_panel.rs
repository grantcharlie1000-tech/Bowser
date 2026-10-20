//! Right-docked, profile-local notes and task sidebar.

use super::{glass, smooth};
use crate::{
    productivity as data,
    profile::{self, Profile},
    theme,
};
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    fs,
};
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
            WindowsAndMessaging::{
                CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, EN_CHANGE,
                ES_AUTOHSCROLL, ES_MULTILINE, GWLP_USERDATA, GetClientRect, GetWindowLongPtrW,
                GetWindowTextLengthW, GetWindowTextW, IDC_ARROW, IDC_SIZEWE, LoadCursorW,
                RegisterClassW, SetCursor, SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW,
                WINDOW_STYLE, WM_COMMAND, WM_CTLCOLOREDIT, WM_ERASEBKGND, WM_LBUTTONUP,
                WM_MOUSEMOVE, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WNDCLASSW, WS_CHILD,
                WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
            },
        },
    },
    core::{PCWSTR, w},
};

const CLASS: PCWSTR = w!("BowserNotesSidebar");
const TITLE_ID: usize = 1;
const BODY_ID: usize = 2;
const TASK_ID: usize = 3;
const DEFAULT_WIDTH_DIP: i32 = 340;
const MIN_WIDTH_DIP: i32 = 280;
const MAX_WIDTH_DIP: i32 = 520;
const EXT: i32 = 24;
pub(crate) const WM_SIDEBAR_RESIZED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x77;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Tab {
    #[default]
    Notes,
    Tasks,
}

struct Panel {
    owner: HWND,
    profile: Profile,
    notes: Vec<data::Note>,
    tasks: Vec<data::Task>,
    selected_note: Option<u64>,
    selected_task: Option<u64>,
    tab: Tab,
    title: HWND,
    body: HWND,
    task: HWND,
    width_dip: i32,
    width: i32,
    height: i32,
    top: i32,
    right_screen: i32,
    updating: bool,
    resizing: bool,
    edit_brush: windows::Win32::Graphics::Gdi::HBRUSH,
    /// The HTML page is the only content: no native EDIT controls, and the window paints only its
    /// backing fill and border (V7).
    html_only: bool,
}

thread_local! { static HTML_ONLY: Cell<bool> = const { Cell::new(false) }; static REGISTERED: Cell<bool> = const { Cell::new(false) }; static OPEN: Cell<isize> = const { Cell::new(0) }; static GUEST_WIDTH: Cell<i32> = const { Cell::new(DEFAULT_WIDTH_DIP) }; static GUEST_DATA: RefCell<(Vec<data::Note>, Vec<data::Task>)> = const { RefCell::new((Vec::new(), Vec::new())) }; }

pub(crate) fn width_px(width_dip: i32, dpi: u32, available: i32) -> i32 {
    let width_dip = width_dip.clamp(MIN_WIDTH_DIP, MAX_WIDTH_DIP);
    (width_dip.saturating_mul(dpi.max(96) as i32) / 96).clamp(0, available.max(0))
}

pub(crate) fn sidebar_rect(viewport: RECT, width_dip: i32, dpi: u32) -> RECT {
    let width = width_px(width_dip, dpi, viewport.right - viewport.left);
    RECT {
        left: viewport.right - width,
        top: viewport.top,
        right: viewport.right,
        bottom: viewport.bottom,
    }
}

pub(crate) fn adjust_viewport(viewport: RECT, owner: HWND, dpi: u32) -> RECT {
    let width = OPEN.with(Cell::get);
    if width == 0 {
        return viewport;
    }
    let width_dip =
        unsafe { panel(HWND(width as *mut c_void)).map_or(DEFAULT_WIDTH_DIP, |p| p.width_dip) };
    let rect = sidebar_rect(viewport, width_dip, dpi);
    let _ = owner;
    RECT {
        right: rect.left,
        ..viewport
    }
}

pub(crate) fn sync(owner: HWND, viewport: RECT, dpi: u32) {
    let raw = OPEN.with(Cell::get);
    if raw == 0 {
        return;
    }
    let hwnd = HWND(raw as *mut c_void);
    if let Some(state) = unsafe { panel(hwnd) } {
        let mut origin = windows::Win32::Foundation::POINT {
            x: 0,
            y: viewport.top,
        };
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(owner, &mut origin);
        }
        let width = width_px(state.width_dip, dpi, viewport.right - viewport.left);
        let height = viewport.bottom - viewport.top;
        let _ = unsafe {
            windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                hwnd,
                None,
                origin.x + viewport.right - width,
                origin.y,
                width,
                height,
                windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
            )
        };
        state.width = width;
        state.height = height;
        state.top = origin.y;
        state.right_screen = origin.x + viewport.right;
        position_controls(hwnd, state);
        let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
        super::html_dialog::resize(hwnd);
    }
}

/// --- wave-1 fixes --- shows the panel without toggling it: a note picked in Quick commands while the
/// panel is already open leaves it open (`open` closes an open panel, as its button does).
pub(crate) fn show(owner: HWND) {
    if OPEN.with(Cell::get) == 0 {
        open(owner);
    }
}

/// Shows the panel at note `id` (a note picked in Quick commands). The native panel selects it,
/// open or not; the HTML panel's page has no way to be told a note yet, so it opens as `show` does.
pub(crate) fn show_note(owner: HWND, id: u64) {
    show(owner);
    if super::html_dialog::html_only(owner) {
        return;
    }
    let Some(raw) = OPEN.with(Cell::get).checked_sub(0).filter(|raw| *raw != 0) else { return };
    let hwnd = HWND(raw as *mut c_void);
    if let Some(s) = unsafe { panel(hwnd) } {
        let Some(picked) = picked_note(&s.notes, id) else { return };
        s.tab = Tab::Notes;
        s.selected_note = Some(picked);
        populate_edits(hwnd);
        let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
    }
}

/// The note to select for a pick of `id`: that note while it still exists, else none (the panel
/// keeps its own choice).
fn picked_note(notes: &[data::Note], id: u64) -> Option<u64> {
    notes.iter().any(|note| note.id == id).then_some(id)
}

pub(crate) fn open(owner: HWND) {
    if let Some(raw) = OPEN.with(Cell::get).checked_sub(0).filter(|raw| *raw != 0) {
        unsafe {
            let _ = DestroyWindow(HWND(raw as *mut c_void));
        }
        return;
    }
    let Ok(profile) = profile::active() else {
        return;
    };
    if !REGISTERED.with(Cell::get) && register_class().is_none() {
        return;
    }
    let (notes, tasks) = if profile.is_guest() {
        GUEST_DATA.with(|data| data.borrow().clone())
    } else {
        (
            data::read_note_list(&profile.notes_file()),
            data::read_tasks(&profile.tasks_file()),
        )
    };
    let width_dip = load_width(&profile);
    let dpi = unsafe { GetDpiForWindow(owner).max(96) };
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(owner, &mut client);
    }
    let html_only = super::html_dialog::html_only(owner);
    let viewport_top = (client.bottom - 500).max(0); // replaced with the live viewport by sync immediately below
    let state = Box::new(Panel {
        owner,
        profile,
        notes,
        tasks,
        selected_note: None,
        selected_task: None,
        tab: Tab::Notes,
        title: HWND::default(),
        body: HWND::default(),
        task: HWND::default(),
        width_dip,
        width: width_px(width_dip, dpi, client.right),
        height: (client.bottom - viewport_top).max(1),
        top: 0,
        right_screen: 0,
        updating: false,
        resizing: false,
        edit_brush: unsafe {
            CreateSolidBrush(colorref(
                theme::palette(glass::appearance(), true).omnibox_fill,
            ))
        },
        html_only,
    });
    let width = state.width;
    let height = state.height;
    let mut owner_rect = RECT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(owner, &mut owner_rect);
    }
    let Some(hwnd) = create_panel_window(
        state,
        RECT {
            left: owner_rect.right - width,
            top: owner_rect.top + viewport_top,
            right: owner_rect.right,
            bottom: owner_rect.top + viewport_top + height,
        },
        Some(owner),
    ) else {
        return;
    };
    OPEN.with(|slot| slot.set(hwnd.0 as isize));
    super::window::attach_html_dialog(owner, hwnd, super::html_dialog::Kind::Notes);
    sync(
        owner,
        RECT {
            left: 0,
            top: viewport_top,
            right: client.right,
            bottom: client.bottom,
        },
        dpi,
    );
    unsafe {
        let _ = SetForegroundWindow(hwnd);
        if let Some(state) = panel(hwnd) {
            state.selected_note = state.notes.first().map(|note| note.id);
            state.selected_task = state.tasks.first().map(|task| task.id);
        }
        if !html_only {
            populate_edits(hwnd);
        }
    }
}

thread_local! {
    /// The state handed to a `CreateWindowExW` whose `WM_NCCREATE` has not run yet: until it runs,
    /// `create_panel_window` still owns it.
    static UNADOPTED: Cell<usize> = const { Cell::new(0) };
}

/// Creates the panel window at `bounds` (screen) and gives it `state`. The window owns the state
/// from `WM_NCCREATE` until `WM_NCDESTROY`, which frees it; the pointer in `GWLP_USERDATA` stays
/// valid for every message in between (W-1: the Box used to drop when `open` returned).
fn create_panel_window(state: Box<Panel>, bounds: RECT, owner: Option<HWND>) -> Option<HWND> {
    let html_only = state.html_only;
    let raw = Box::into_raw(state);
    let instance = unsafe { HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0) };
    HTML_ONLY.with(|flag| flag.set(html_only));
    UNADOPTED.with(|slot| slot.set(raw as usize));
    let created = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS,
            w!("Notes and Tasks"),
            WS_POPUP | WS_VISIBLE,
            bounds.left,
            bounds.top,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            owner,
            None,
            Some(instance),
            Some(raw.cast::<c_void>()),
        )
    };
    HTML_ONLY.with(|flag| flag.set(false));
    // Creation that failed before WM_NCCREATE never handed the state over (after it, WM_NCDESTROY
    // has freed it already).
    if UNADOPTED.with(|slot| slot.replace(0)) == raw as usize {
        drop(unsafe { Box::from_raw(raw) });
    }
    created.ok()
}

pub(crate) fn html_notes(profile: &Profile) -> Vec<data::Note> {
    if profile.is_guest() { GUEST_DATA.with(|data| data.borrow().0.clone()) }
    else { data::read_note_list(&profile.notes_file()) }
}

pub(crate) fn html_tasks(profile: &Profile) -> Vec<data::Task> {
    if profile.is_guest() { GUEST_DATA.with(|data| data.borrow().1.clone()) }
    else { data::read_tasks(&profile.tasks_file()) }
}

pub(crate) fn save_html_notes(profile: &Profile, notes: &[data::Note]) -> std::io::Result<()> {
    if profile.is_guest() { GUEST_DATA.with(|data| data.borrow_mut().0 = notes.to_vec()); Ok(()) }
    else { data::write_note_list(&profile.notes_file(), notes) }
}

// --- page tools (T7): the notes with their folders, sort order and origin (src/notes_book.rs) ---
thread_local! { static GUEST_META: RefCell<String> = const { RefCell::new(String::new()) }; }

fn meta_file(profile: &Profile) -> std::path::PathBuf { profile.notes_file().with_file_name("notes-meta.txt") }

/// The notes of the profile with what `notes-meta.txt` adds. A guest keeps both in memory.
pub(crate) fn html_book(profile: &Profile) -> crate::notes_book::Book {
    let text = if profile.is_guest() { GUEST_META.with(|meta| meta.borrow().clone()) }
        else { data::read_bounded(&meta_file(profile)).unwrap_or_default() };
    crate::notes_book::Book::from_parts(html_notes(profile), &text)
}

pub(crate) fn save_html_book(profile: &Profile, book: &crate::notes_book::Book) -> std::io::Result<()> {
    save_html_notes(profile, &book.notes)?;
    let text = book.meta_text();
    if profile.is_guest() { GUEST_META.with(|meta| *meta.borrow_mut() = text); Ok(()) }
    else { data::atomic_write(&meta_file(profile), text.as_bytes()) }
}
// --- end page tools ---

pub(crate) fn save_html_tasks(profile: &Profile, tasks: &[data::Task]) -> std::io::Result<()> {
    if profile.is_guest() { GUEST_DATA.with(|data| data.borrow_mut().1 = tasks.to_vec()); Ok(()) }
    else { data::write_tasks(&profile.tasks_file(), tasks) }
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

fn create_edits(hwnd: HWND, s: &mut Panel) {
    let instance = unsafe { HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0) };
    s.title = create_edit(hwnd, TITLE_ID, ES_AUTOHSCROLL, instance);
    s.body = create_edit(hwnd, BODY_ID, ES_MULTILINE, instance);
    s.task = create_edit(hwnd, TASK_ID, ES_AUTOHSCROLL, instance);
    position_controls(hwnd, s);
}

fn create_edit(hwnd: HWND, id: usize, style: i32, instance: HINSTANCE) -> HWND {
    unsafe {
        CreateWindowExW(
            Default::default(),
            w!("EDIT"),
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style as u32),
            0,
            0,
            1,
            1,
            Some(hwnd),
            Some(windows::Win32::UI::WindowsAndMessaging::HMENU(
                id as *mut c_void,
            )),
            Some(instance),
            None,
        )
        .unwrap_or_default()
    }
}

fn position_controls(hwnd: HWND, s: &Panel) {
    if s.html_only {
        return;
    }
    let dpi = unsafe { GetDpiForWindow(hwnd).max(96) };
    let scale = |v: i32| v * dpi as i32 / 96;
    let show = |window: HWND, visible: bool| unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
            window,
            if visible {
                windows::Win32::UI::WindowsAndMessaging::SW_SHOW
            } else {
                windows::Win32::UI::WindowsAndMessaging::SW_HIDE
            },
        );
    };
    match s.tab {
        Tab::Notes => {
            let list_bottom = s.height - scale(196);
            let _ = unsafe {
                windows::Win32::UI::WindowsAndMessaging::MoveWindow(
                    s.title,
                    scale(EXT),
                    list_bottom,
                    s.width - scale(EXT * 2 + 64),
                    scale(30),
                    true,
                )
            };
            let _ = unsafe {
                windows::Win32::UI::WindowsAndMessaging::MoveWindow(
                    s.body,
                    scale(EXT),
                    list_bottom + scale(64),
                    s.width - scale(EXT * 2),
                    (s.height - list_bottom - scale(76)).max(scale(48)),
                    true,
                )
            };
            show(s.title, true);
            show(s.body, true);
            show(s.task, false);
        }
        Tab::Tasks => {
            let _ = unsafe {
                windows::Win32::UI::WindowsAndMessaging::MoveWindow(
                    s.task,
                    scale(EXT),
                    s.height - scale(56),
                    s.width - scale(EXT * 2 + 64),
                    scale(32),
                    true,
                )
            };
            show(s.title, false);
            show(s.body, false);
            show(s.task, true);
        }
    }
}

fn populate_edits(hwnd: HWND) {
    if let Some(s) = unsafe { panel(hwnd) } {
        s.updating = true;
        let note = s
            .selected_note
            .and_then(|id| s.notes.iter().find(|note| note.id == id));
        set_text(s.title, note.map_or("", |note| &note.title));
        set_text(s.body, note.map_or("", |note| &note.body));
        let task = s
            .selected_task
            .and_then(|id| s.tasks.iter().find(|task| task.id == id));
        set_text(s.task, task.map_or("", |task| &task.text));
        s.updating = false;
        position_controls(hwnd, s);
        let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
    }
}

fn set_text(hwnd: HWND, value: &str) {
    let wide: Vec<u16> = value.encode_utf16().chain([0]).collect();
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
    }
}

fn edit_text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut buffer = vec![0u16; len + 1];
        let count = GetWindowTextW(hwnd, &mut buffer).max(0) as usize;
        String::from_utf16_lossy(&buffer[..count])
    }
}

fn save_note(hwnd: HWND, s: &mut Panel) {
    let Some(id) = s.selected_note else {
        return;
    };
    if let Some(note) = s.notes.iter_mut().find(|note| note.id == id) {
        note.title = edit_text(s.title).trim().to_owned();
        note.body = edit_text(s.body);
        if !s.profile.is_guest() {
            let _ = data::write_note_list(&s.profile.notes_file(), &s.notes);
        }
        cache_guest(s);
    }
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
}

fn save_task(hwnd: HWND, s: &mut Panel) {
    let Some(id) = s.selected_task else {
        return;
    };
    if let Some(task) = s.tasks.iter_mut().find(|task| task.id == id) {
        task.text = edit_text(s.task).trim().to_owned();
        if !s.profile.is_guest() {
            let _ = data::write_tasks(&s.profile.tasks_file(), &s.tasks);
        }
        cache_guest(s);
    }
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
}

fn add_note(hwnd: HWND) {
    if let Some(s) = unsafe { panel(hwnd) } {
        let note = data::Note {
            id: next_id(),
            title: "New note".into(),
            body: String::new(),
        };
        s.notes.push(note.clone());
        s.selected_note = Some(note.id);
        s.tab = Tab::Notes;
        if !s.profile.is_guest() {
            let _ = data::write_note_list(&s.profile.notes_file(), &s.notes);
        }
        cache_guest(s);
        populate_edits(hwnd);
        unsafe {
            let _ = SetFocus(Some(s.title));
        }
    }
}

fn add_task(hwnd: HWND) {
    if let Some(s) = unsafe { panel(hwnd) } {
        let task = data::Task {
            id: next_id(),
            complete: false,
            text: "New task".into(),
        };
        s.tasks.push(task.clone());
        s.selected_task = Some(task.id);
        s.tab = Tab::Tasks;
        if !s.profile.is_guest() {
            let _ = data::write_tasks(&s.profile.tasks_file(), &s.tasks);
        }
        cache_guest(s);
        populate_edits(hwnd);
        unsafe {
            let _ = SetFocus(Some(s.task));
        }
    }
}

fn delete_selected(hwnd: HWND) {
    if let Some(s) = unsafe { panel(hwnd) } {
        match s.tab {
            Tab::Notes => {
                if let Some(id) = s.selected_note {
                    s.notes.retain(|note| note.id != id);
                    if !s.profile.is_guest() {
                        let _ = data::write_note_list(&s.profile.notes_file(), &s.notes);
                    }
                    s.selected_note = s.notes.first().map(|note| note.id);
                }
            }
            Tab::Tasks => {
                if let Some(id) = s.selected_task {
                    s.tasks.retain(|task| task.id != id);
                    if !s.profile.is_guest() {
                        let _ = data::write_tasks(&s.profile.tasks_file(), &s.tasks);
                    }
                    s.selected_task = s.tasks.first().map(|task| task.id);
                }
            }
        }
        cache_guest(s);
        populate_edits(hwnd);
    }
}

fn click(hwnd: HWND, x: i32, y: i32) {
    let Some(s) = (unsafe { panel(hwnd) }) else {
        return;
    };
    if y < scale(40, hwnd) && x > s.width - scale(44, hwnd) {
        let _ = unsafe { DestroyWindow(hwnd) };
        return;
    }
    let layout = layout_of(hwnd, s);
    let (notes_tab, tasks_tab, add) = (layout.notes_tab(), layout.tasks_tab(), layout.add());
    if (notes_tab.top..notes_tab.bottom).contains(&y) {
        if x < tasks_tab.left {
            s.tab = Tab::Notes;
        } else if x < add.left {
            s.tab = Tab::Tasks;
        } else {
            if s.tab == Tab::Notes {
                add_note(hwnd);
            } else {
                add_task(hwnd);
            }
            return;
        }
        populate_edits(hwnd);
        return;
    }
    if contains(layout.delete(s.tab), x, y) {
        delete_selected(hwnd);
        return;
    }
    if s.tab == Tab::Notes {
        if let Some(note) = layout.row_at(Tab::Notes, y).and_then(|index| s.notes.get(index)) {
            s.selected_note = Some(note.id);
            populate_edits(hwnd);
        }
    } else if let Some(index) = layout.row_at(Tab::Tasks, y)
        && let Some(task) = s.tasks.get_mut(index)
    {
        if x < layout.row(index).left + layout.px(34) {
            task.complete = !task.complete;
            if !s.profile.is_guest() {
                let _ = data::write_tasks(&s.profile.tasks_file(), &s.tasks);
            }
            cache_guest(s);
            let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
        } else {
            s.selected_task = Some(task.id);
            populate_edits(hwnd);
        }
    }
}

fn resize_from_pointer(hwnd: HWND, x: i32) {
    let Some(s) = (unsafe { panel(hwnd) }) else {
        return;
    };
    let mut window = RECT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut window);
    }
    let pointer_screen_x = window.left + x;
    let width = (s.right_screen - pointer_screen_x)
        .clamp(scale(MIN_WIDTH_DIP, hwnd), scale(MAX_WIDTH_DIP, hwnd));
    let _ = unsafe {
        windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
            hwnd,
            None,
            s.right_screen - width,
            s.top,
            width,
            s.height,
            windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
        )
    };
    s.width = width;
    s.width_dip = width * 96 / unsafe { GetDpiForWindow(hwnd).max(96) } as i32;
    position_controls(hwnd, s);
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
    let _ = unsafe {
        windows::Win32::UI::WindowsAndMessaging::PostMessageW(
            Some(s.owner),
            WM_SIDEBAR_RESIZED,
            WPARAM(0),
            LPARAM(0),
        )
    };
}

fn cache_guest(s: &Panel) {
    if s.profile.is_guest() {
        GUEST_DATA.with(|data| *data.borrow_mut() = (s.notes.clone(), s.tasks.clone()));
    }
}

fn load_width(profile: &Profile) -> i32 {
    if profile.is_guest() {
        return GUEST_WIDTH
            .with(Cell::get)
            .clamp(MIN_WIDTH_DIP, MAX_WIDTH_DIP);
    }
    fs::read_to_string(profile.settings_file())
        .ok()
        .map_or(DEFAULT_WIDTH_DIP, |text| width_from_settings(&text))
}

fn width_from_settings(text: &str) -> i32 {
    text.lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("notes_panel_width=")?
                .parse::<i32>()
                .ok()
        })
        .unwrap_or(DEFAULT_WIDTH_DIP)
        .clamp(MIN_WIDTH_DIP, MAX_WIDTH_DIP)
}

fn settings_with_width(existing: &str, width: i32) -> String {
    let mut lines = existing
        .lines()
        .filter(|line| !line.trim().starts_with("notes_panel_width="))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    lines.push(format!(
        "notes_panel_width={}",
        width.clamp(MIN_WIDTH_DIP, MAX_WIDTH_DIP)
    ));
    lines.join("\n") + "\n"
}

fn save_width(profile: &Profile, width: i32) {
    if profile.is_guest() {
        GUEST_WIDTH.with(|saved| saved.set(width.clamp(MIN_WIDTH_DIP, MAX_WIDTH_DIP)));
        return;
    }
    // Through the settings saver: a file it cannot read is left alone, not replaced by this one line.
    let _ = crate::settings::save_edited(&profile.settings_file(), |existing| settings_with_width(existing, width));
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
        // The HTML page draws the title, tabs and close button; only the backing is native.
        if panel(hwnd).is_some_and(|s| s.html_only) {
            let _ = EndPaint(hwnd, &ps);
            return;
        }
        SetBkMode(dc, TRANSPARENT);
        text(
            dc,
            RECT {
                left: scale(EXT, hwnd),
                top: scale(8, hwnd),
                right: client.right - scale(52, hwnd),
                bottom: scale(38, hwnd),
            },
            "Notes & Tasks",
            p.tab_text_active,
            false,
        );
        text(
            dc,
            RECT {
                left: client.right - scale(44, hwnd),
                top: scale(4, hwnd),
                right: client.right - scale(12, hwnd),
                bottom: scale(36, hwnd),
            },
            "×",
            p.tab_text_inactive,
            false,
        );
        if let Some(s) = panel(hwnd) {
            let layout = layout_of(hwnd, s);
            let active = if s.tab == Tab::Notes {
                p.tab_active
            } else {
                p.tab_hover
            };
            let inactive = if s.tab == Tab::Tasks {
                p.tab_active
            } else {
                p.tab_hover
            };
            let radius = layout.px(6);
            smooth::fill_round_rect(dc, layout.notes_tab(), radius, active);
            smooth::fill_round_rect(dc, layout.tasks_tab(), radius, inactive);
            text(dc, layout.notes_tab(), "Notes", p.tab_text_active, false);
            text(dc, layout.tasks_tab(), "Tasks", p.tab_text_active, false);
            smooth::fill_round_rect(dc, layout.add(), radius, p.tab_hover);
            text(dc, layout.add(), "+ Add", p.tab_text_active, false);
            match s.tab {
                Tab::Notes => {
                    for (index, note) in s.notes.iter().enumerate() {
                        let bounds = layout.row(index);
                        if bounds.top > layout.rows_limit(Tab::Notes) {
                            break;
                        }
                        if s.selected_note == Some(note.id) {
                            smooth::fill_round_rect(dc, bounds, layout.px(5), p.tab_hover);
                        }
                        text(dc, bounds, &note.title, p.tab_text_active, true);
                    }
                    text(dc, layout.delete(Tab::Notes), "Delete", p.tab_text_inactive, false);
                    text(dc, layout.note_label(), "Note", p.tab_text_inactive, false);
                }
                Tab::Tasks => {
                    for (index, task) in s.tasks.iter().enumerate() {
                        let bounds = layout.row(index);
                        if bounds.top > layout.rows_limit(Tab::Tasks) {
                            break;
                        }
                        if s.selected_task == Some(task.id) {
                            smooth::fill_round_rect(dc, bounds, layout.px(5), p.tab_hover);
                        }
                        text(
                            dc,
                            RECT {
                                left: bounds.left + layout.px(4),
                                top: bounds.top,
                                right: bounds.left + layout.px(34),
                                bottom: bounds.bottom,
                            },
                            if task.complete { "☑" } else { "□" },
                            p.tab_text_active,
                            false,
                        );
                        text(
                            dc,
                            RECT {
                                left: bounds.left + layout.px(36),
                                top: bounds.top,
                                right: bounds.right,
                                bottom: bounds.bottom,
                            },
                            &task.text,
                            if task.complete {
                                p.tab_text_inactive
                            } else {
                                p.tab_text_active
                            },
                            true,
                        );
                    }
                    text(dc, layout.task_label(), "Edit task", p.tab_text_inactive, false);
                    text(dc, layout.delete(Tab::Tasks), "Delete", p.tab_text_inactive, false);
                }
            }
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

fn text(
    dc: windows::Win32::Graphics::Gdi::HDC,
    mut bounds: RECT,
    value: &str,
    color: theme::Rgba,
    ellipsis: bool,
) {
    let mut units = value.encode_utf16().collect::<Vec<_>>();
    if units.is_empty() {
        return;
    }
    unsafe {
        SetTextColor(dc, colorref(color));
        let _ = DrawTextW(dc, &mut units, &mut bounds, text_flags(ellipsis));
    }
}

/// Labels and note titles are drawn as written: `&` is a character, not a mnemonic prefix (W-10:
/// the header read "Notes _Tasks" and a title lost its `&`).
fn text_flags(ellipsis: bool) -> windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT {
    use windows::Win32::Graphics::Gdi::{DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER};
    let flags = DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX;
    if ellipsis { flags | DT_END_ELLIPSIS } else { flags }
}

/// Where the native panel draws its tabs, rows and buttons, in device pixels, for a panel of
/// `width` x `height` device pixels at `dpi`. Paint and `click` both read it, so what is drawn is
/// what is hit (W-4: paint scaled the already-scaled width, height and row tops a second time).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Layout {
    dpi: u32,
    width: i32,
    height: i32,
}

impl Layout {
    fn new(width: i32, height: i32, dpi: u32) -> Self {
        Self { dpi: dpi.max(96), width, height }
    }
    fn px(&self, dip: i32) -> i32 {
        dip * self.dpi as i32 / 96
    }
    /// A rect whose edges are each a DIP offset from the panel's start, end or middle.
    fn rect(&self, left: Edge, top: Edge, right: Edge, bottom: Edge) -> RECT {
        let x = |edge: Edge| match edge {
            Edge::Start(dip) => self.px(dip),
            Edge::End(dip) => self.width - self.px(dip),
            Edge::Middle(dip) => self.width / 2 + self.px(dip),
        };
        let y = |edge: Edge| match edge {
            Edge::Start(dip) => self.px(dip),
            Edge::End(dip) => self.height - self.px(dip),
            Edge::Middle(dip) => self.height / 2 + self.px(dip),
        };
        RECT { left: x(left), top: y(top), right: x(right), bottom: y(bottom) }
    }
    fn notes_tab(&self) -> RECT {
        self.rect(Edge::Start(EXT), Edge::Start(46), Edge::Middle(-4), Edge::Start(84))
    }
    fn tasks_tab(&self) -> RECT {
        self.rect(Edge::Middle(4), Edge::Start(46), Edge::End(88), Edge::Start(84))
    }
    fn add(&self) -> RECT {
        self.rect(Edge::End(80), Edge::Start(46), Edge::End(EXT), Edge::Start(84))
    }
    fn row_height(&self) -> i32 {
        self.px(30)
    }
    fn rows_top(&self) -> i32 {
        self.px(98)
    }
    /// Row `index` of the note or task list.
    fn row(&self, index: usize) -> RECT {
        let top = self.rows_top() + index as i32 * self.row_height();
        RECT {
            left: self.px(EXT),
            top,
            right: self.width - self.px(EXT),
            bottom: top + self.row_height() - self.px(2),
        }
    }
    /// The last y a row may start at on `tab`.
    fn rows_limit(&self, tab: Tab) -> i32 {
        self.height - self.px(if tab == Tab::Notes { 202 } else { 70 })
    }
    /// The index of the row drawn under `y` on `tab`, if one is drawn there.
    fn row_at(&self, tab: Tab, y: i32) -> Option<usize> {
        if y < self.rows_top() {
            return None;
        }
        let index = ((y - self.rows_top()) / self.row_height()) as usize;
        (self.row(index).top <= self.rows_limit(tab)).then_some(index)
    }
    fn delete(&self, tab: Tab) -> RECT {
        match tab {
            Tab::Notes => self.rect(Edge::End(76), Edge::End(194), Edge::End(EXT), Edge::End(166)),
            Tab::Tasks => self.rect(Edge::End(76), Edge::End(42), Edge::End(EXT), Edge::End(14)),
        }
    }
    fn note_label(&self) -> RECT {
        self.rect(Edge::Start(EXT), Edge::End(158), Edge::End(EXT), Edge::End(136))
    }
    fn task_label(&self) -> RECT {
        self.rect(Edge::Start(EXT), Edge::End(86), Edge::End(EXT), Edge::End(60))
    }
}

#[derive(Clone, Copy, Debug)]
enum Edge {
    Start(i32),
    End(i32),
    Middle(i32),
}

fn layout_of(hwnd: HWND, s: &Panel) -> Layout {
    Layout::new(s.width, s.height, unsafe { GetDpiForWindow(hwnd) })
}

fn contains(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn colorref(c: theme::Rgba) -> windows::Win32::Foundation::COLORREF {
    windows::Win32::Foundation::COLORREF(
        u32::from(c.r) | (u32::from(c.g) << 8) | (u32::from(c.b) << 16),
    )
}

fn scale(value: i32, hwnd: HWND) -> i32 {
    unsafe { value * GetDpiForWindow(hwnd).max(96) as i32 / 96 }
}

fn next_id() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! { static DROPS: Cell<usize> = const { Cell::new(0) }; }

    impl Drop for Panel {
        fn drop(&mut self) {
            DROPS.with(|drops| drops.set(drops.get() + 1));
        }
    }

    fn guest_panel(root: &std::path::Path, width_dip: i32) -> Panel {
        Panel {
            owner: HWND::default(),
            profile: Profile::guest_in(root).expect("guest profile"),
            notes: vec![data::Note { id: 7, title: "kept".into(), body: String::new() }],
            tasks: Vec::new(),
            selected_note: None,
            selected_task: None,
            tab: Tab::Notes,
            title: HWND::default(),
            body: HWND::default(),
            task: HWND::default(),
            width_dip,
            width: 340,
            height: 600,
            top: 0,
            right_screen: 0,
            updating: false,
            resizing: false,
            edit_brush: Default::default(),
            html_only: true,
        }
    }

    #[test]
    fn the_panel_state_lives_as_long_as_its_window_and_is_freed_once_with_it() {
        let root = std::env::temp_dir().join(format!("bowser-notes-w1-{}", std::process::id()));
        assert!(register_class().is_some());
        DROPS.with(|drops| drops.set(0));
        let bounds = RECT { left: -32000, top: -32000, right: -31660, bottom: -31400 };
        let hwnd = create_panel_window(Box::new(guest_panel(&root, 333)), bounds, None)
            .expect("the panel window is created");
        // `open` returns here: the state must still be the window's, not freed with a local Box.
        assert_eq!(DROPS.with(Cell::get), 0, "the state outlives the call that created the window");
        let state = unsafe { panel(hwnd) }.expect("the window still has its state");
        assert_eq!((state.width_dip, state.notes[0].title.as_str()), (333, "kept"));
        unsafe { DestroyWindow(hwnd).expect("destroy") };
        assert_eq!(DROPS.with(Cell::get), 1, "WM_NCDESTROY frees the state exactly once");
        assert!(unsafe { panel(hwnd) }.is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn what_the_native_panel_paints_is_inside_it_and_is_what_a_click_hits_at_every_dpi() {
        for dpi in [96, 120, 144, 192] {
            let width = width_px(340, dpi, 4000);
            let height = 700 * dpi as i32 / 96;
            let layout = Layout::new(width, height, dpi);
            let painted = [
                layout.notes_tab(),
                layout.tasks_tab(),
                layout.add(),
                layout.row(0),
                layout.delete(Tab::Notes),
                layout.delete(Tab::Tasks),
                layout.note_label(),
                layout.task_label(),
            ];
            for rect in painted {
                assert!(
                    rect.left >= 0 && rect.top >= 0 && rect.right <= width && rect.bottom <= height
                        && rect.left < rect.right && rect.top < rect.bottom,
                    "{rect:?} is drawn inside the {width}x{height} panel at dpi {dpi}"
                );
            }
            // The tabs and "+ Add" sit side by side, in that order, without overlapping.
            assert!(layout.notes_tab().right <= layout.tasks_tab().left);
            assert!(layout.tasks_tab().right <= layout.add().left);
            // A click in the middle of a drawn row selects that row.
            for index in 0..3 {
                let row = layout.row(index);
                assert_eq!(layout.row_at(Tab::Notes, (row.top + row.bottom) / 2), Some(index), "dpi {dpi}");
            }
            let delete = layout.delete(Tab::Notes);
            assert!(contains(delete, (delete.left + delete.right) / 2, (delete.top + delete.bottom) / 2));
        }
    }

    #[test]
    fn labels_and_titles_draw_an_ampersand_instead_of_underlining_the_next_letter() {
        use windows::Win32::Graphics::Gdi::{DT_END_ELLIPSIS, DT_NOPREFIX};
        assert!(text_flags(false).contains(DT_NOPREFIX));
        assert!(text_flags(true).contains(DT_NOPREFIX));
        assert!(text_flags(true).contains(DT_END_ELLIPSIS));
        assert!(!text_flags(false).contains(DT_END_ELLIPSIS));
    }

    #[test]
    fn a_picked_note_is_selected_only_while_it_exists() {
        let notes = vec![data::Note { id: 4, title: "A".into(), body: String::new() }, data::Note { id: 9, title: "B".into(), body: String::new() }];
        assert_eq!(picked_note(&notes, 9), Some(9));
        assert_eq!(picked_note(&notes, 5), None);
    }

    #[test]
    fn sidebar_width_and_right_dock_scale_and_clamp_at_supported_dpis() {
        for (dpi, expected) in [(96, 340), (120, 425), (144, 510), (192, 680)] {
            assert_eq!(width_px(340, dpi, 2000), expected);
            let viewport = RECT {
                left: 10,
                top: 70,
                right: 1000,
                bottom: 800,
            };
            let sidebar = sidebar_rect(viewport, 340, dpi);
            assert_eq!(sidebar.right, viewport.right);
            assert_eq!(sidebar.left, viewport.right - expected);
            assert_eq!(sidebar.top, viewport.top);
            assert_eq!(sidebar.bottom, viewport.bottom);
        }
        assert_eq!(width_px(900, 96, 300), 300);
        assert_eq!(width_px(120, 96, 900), 280);
        assert_eq!(width_px(900, 96, 900), 520);
    }

    #[test]
    fn sidebar_width_setting_round_trips_while_preserving_other_profile_settings() {
        let original =
            "# keep this comment\nsleep_enabled=false\nnotes_panel_width=360\nunknown=value\n";
        let saved = settings_with_width(original, 410);
        assert_eq!(width_from_settings(&saved), 410);
        assert!(saved.contains("# keep this comment"));
        assert!(saved.contains("sleep_enabled=false"));
        assert!(saved.contains("unknown=value"));
        assert_eq!(saved.matches("notes_panel_width=").count(), 1);
    }

    /// R-1: resizing the popup must not replace a `settings.txt` it cannot read with its one line.
    #[test]
    fn resizing_leaves_an_unreadable_settings_file_alone_and_keeps_every_other_setting() {
        let dir = std::env::temp_dir().join(format!("bowser-notes-width-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let profile = crate::profile::Profile::default_in(&dir);
        let path = profile.settings_file();
        let ansi = b"search_engine=duckduckgo\nhome_page=https://caf\xe9.example/\nsleep_enabled=false\n";
        fs::write(&path, ansi).unwrap();
        save_width(&profile, 400);
        assert_eq!(fs::read(&path).unwrap(), ansi, "a non-UTF-8 settings file is left as it was");

        fs::write(&path, "# mine\nsleep_enabled=false\nunknown=value\n").unwrap();
        let before = crate::settings::save_generation();
        save_width(&profile, 400);
        let saved = fs::read_to_string(&path).unwrap();
        assert_eq!(saved, "# mine\nsleep_enabled=false\nunknown=value\nnotes_panel_width=400\n");
        assert!(crate::settings::save_generation() > before, "the width save counts as a settings save");
        let _ = fs::remove_dir_all(&dir);
    }
}

unsafe fn panel(hwnd: HWND) -> Option<&'static mut Panel> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Panel;
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
                UNADOPTED.with(|slot| {
                    if slot.get() == create.lpCreateParams as usize {
                        slot.set(0);
                    }
                });
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_CREATE => {
                if !HTML_ONLY.with(Cell::get)
                    && let Some(s) = panel(hwnd)
                {
                    create_edits(hwnd, s);
                }
                LRESULT(0)
            }
            WM_CTLCOLOREDIT => {
                if let Some(s) = panel(hwnd) {
                    let dc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut c_void);
                    let p = theme::palette(glass::appearance(), true);
                    SetBkColor(dc, colorref(p.omnibox_fill));
                    SetTextColor(dc, colorref(p.omnibox_text));
                    LRESULT(s.edit_brush.0 as isize)
                } else {
                    LRESULT(0)
                }
            }
            WM_COMMAND if (wparam.0 >> 16) as u32 == EN_CHANGE => {
                if let Some(s) = panel(hwnd)
                    && !s.updating
                {
                    if (wparam.0 & 0xffff) == TITLE_ID
                        || (wparam.0 & 0xffff) == BODY_ID
                    {
                        save_note(hwnd, s);
                    } else if (wparam.0 & 0xffff) == TASK_ID {
                        save_task(hwnd, s);
                    }
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE if panel(hwnd).is_some_and(|s| s.resizing) => {
                let x = lparam.0 as u16 as i16 as i32;
                resize_from_pointer(hwnd, x);
                LRESULT(0)
            }
            WM_MOUSEMOVE if (lparam.0 as u16) < scale(9, hwnd) as u16 => {
                if let Ok(cursor) = LoadCursorW(None, IDC_SIZEWE) {
                    let _ = SetCursor(Some(cursor));
                }
                LRESULT(0)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDOWN
                if (lparam.0 as u16) < scale(9, hwnd) as u16 =>
            {
                if let Some(s) = panel(hwnd) {
                    s.resizing = true;
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd);
                }
                LRESULT(0)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP
                if panel(hwnd).is_some_and(|s| s.resizing) =>
            {
                if let Some(s) = panel(hwnd) {
                    s.resizing = false;
                    save_width(&s.profile, s.width_dip);
                }
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let x = lparam.0 as u16 as i32;
                let y = (lparam.0 >> 16) as u16 as i32;
                click(hwnd, x, y);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                super::html_dialog::close(hwnd);
                OPEN.with(|slot| {
                    if slot.get() == hwnd.0 as isize {
                        slot.set(0);
                    }
                });
                if let Some(s) = panel(hwnd) {
                    save_width(&s.profile, s.width_dip);
                    let _ = DeleteObject(s.edit_brush.into());
                    // Closing from the HTML page (its X or Esc) does not pass through the owner, so
                    // ask it to give the page its width back.
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        Some(s.owner),
                        WM_SIDEBAR_RESIZED,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
                // The window owns its state (`create_panel_window`): free it with the window.
                let owned = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut Panel;
                if !owned.is_null() {
                    drop(Box::from_raw(owned));
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}
