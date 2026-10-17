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
    let mut state = Box::new(Panel {
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
    let instance = unsafe { HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0) };
    HTML_ONLY.with(|flag| flag.set(html_only));
    let hwnd = unsafe {
        match CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS,
            w!("Notes and Tasks"),
            WS_POPUP | WS_VISIBLE,
            owner_rect.right - width,
            owner_rect.top + viewport_top,
            width,
            height,
            Some(owner),
            None,
            Some(instance),
            Some((&mut *state as *mut Panel).cast::<c_void>()),
        ) {
            Ok(hwnd) => {
                HTML_ONLY.with(|flag| flag.set(false));
                hwnd
            }
            Err(_) => {
                HTML_ONLY.with(|flag| flag.set(false));
                return;
            }
        }
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
        if !state.notes.is_empty() {
            state.selected_note = Some(state.notes[0].id);
        }
        if !state.tasks.is_empty() {
            state.selected_task = Some(state.tasks[0].id);
        }
        if !html_only {
            populate_edits(hwnd);
        }
    }
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
    if (scale(46, hwnd)..scale(86, hwnd)).contains(&y) {
        if x < s.width / 2 {
            s.tab = Tab::Notes;
        } else if x < s.width - scale(84, hwnd) {
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
    if s.tab == Tab::Notes {
        let row = scale(30, hwnd);
        let first = scale(100, hwnd);
        if y >= first && y < s.height - scale(196, hwnd) {
            let index = ((y - first) / row) as usize;
            if let Some(note) = s.notes.get(index) {
                s.selected_note = Some(note.id);
                populate_edits(hwnd);
            }
        } else if y >= s.height - scale(194, hwnd) && x > s.width - scale(76, hwnd) {
            delete_selected(hwnd);
        }
    } else {
        let row = scale(30, hwnd);
        let first = scale(100, hwnd);
        if y >= first && y < s.height - scale(68, hwnd) {
            let index = ((y - first) / row) as usize;
            if let Some(task) = s.tasks.get_mut(index) {
                if x < scale(42, hwnd) {
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
        } else if y > s.height - scale(40, hwnd) && x > s.width - scale(76, hwnd) {
            delete_selected(hwnd);
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
    let path = profile.settings_file();
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let _ = fs::write(path, settings_with_width(&existing, width));
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
                left: scale(EXT as i32, hwnd),
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
            smooth::fill_round_rect(
                dc,
                rect(EXT as i32, 46, s.width / 2 - 4, 84, hwnd),
                scale(6, hwnd),
                active,
            );
            smooth::fill_round_rect(
                dc,
                rect(s.width / 2 + 4, 46, s.width - 88, 84, hwnd),
                scale(6, hwnd),
                inactive,
            );
            text(
                dc,
                rect(EXT as i32, 46, s.width / 2 - 4, 84, hwnd),
                "Notes",
                p.tab_text_active,
                false,
            );
            text(
                dc,
                rect(s.width / 2 + 4, 46, s.width - 88, 84, hwnd),
                "Tasks",
                p.tab_text_active,
                false,
            );
            smooth::fill_round_rect(
                dc,
                rect(s.width - 80, 46, s.width - EXT as i32, 84, hwnd),
                scale(6, hwnd),
                p.tab_hover,
            );
            text(
                dc,
                rect(s.width - 80, 46, s.width - EXT as i32, 84, hwnd),
                "+ Add",
                p.tab_text_active,
                false,
            );
            match s.tab {
                Tab::Notes => {
                    let row = scale(30, hwnd);
                    let top = scale(98, hwnd);
                    for (index, note) in s.notes.iter().enumerate() {
                        let y = top + index as i32 * row;
                        if y > s.height - scale(202, hwnd) {
                            break;
                        }
                        let bounds = rect(
                            EXT as i32,
                            y,
                            s.width - EXT as i32,
                            y + row - scale(2, hwnd),
                            hwnd,
                        );
                        if s.selected_note == Some(note.id) {
                            smooth::fill_round_rect(dc, bounds, scale(5, hwnd), p.tab_hover);
                        }
                        text(dc, bounds, &note.title, p.tab_text_active, true);
                    }
                    text(
                        dc,
                        rect(
                            s.width - 76,
                            s.height - 194,
                            s.width - EXT as i32,
                            s.height - 166,
                            hwnd,
                        ),
                        "Delete",
                        p.tab_text_inactive,
                        false,
                    );
                    text(
                        dc,
                        rect(
                            EXT as i32,
                            s.height - 158,
                            s.width - EXT as i32,
                            s.height - 136,
                            hwnd,
                        ),
                        "Note",
                        p.tab_text_inactive,
                        false,
                    );
                }
                Tab::Tasks => {
                    let row = scale(30, hwnd);
                    let top = scale(98, hwnd);
                    for (index, task) in s.tasks.iter().enumerate() {
                        let y = top + index as i32 * row;
                        if y > s.height - scale(70, hwnd) {
                            break;
                        }
                        let bounds = rect(
                            EXT as i32,
                            y,
                            s.width - EXT as i32,
                            y + row - scale(2, hwnd),
                            hwnd,
                        );
                        if s.selected_task == Some(task.id) {
                            smooth::fill_round_rect(dc, bounds, scale(5, hwnd), p.tab_hover);
                        }
                        text(
                            dc,
                            RECT {
                                left: bounds.left + scale(4, hwnd),
                                top: bounds.top,
                                right: bounds.left + scale(34, hwnd),
                                bottom: bounds.bottom,
                            },
                            if task.complete { "☑" } else { "□" },
                            p.tab_text_active,
                            false,
                        );
                        text(
                            dc,
                            RECT {
                                left: bounds.left + scale(36, hwnd),
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
                    text(
                        dc,
                        rect(
                            EXT as i32,
                            s.height - 86,
                            s.width - EXT as i32,
                            s.height - 60,
                            hwnd,
                        ),
                        "Edit task",
                        p.tab_text_inactive,
                        false,
                    );
                    text(
                        dc,
                        rect(
                            s.width - 76,
                            s.height - 42,
                            s.width - EXT as i32,
                            s.height - 14,
                            hwnd,
                        ),
                        "Delete",
                        p.tab_text_inactive,
                        false,
                    );
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
        let mut flags = windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE;
        if ellipsis {
            flags |= windows::Win32::Graphics::Gdi::DT_END_ELLIPSIS;
        }
        let _ = DrawTextW(dc, &mut units, &mut bounds, flags);
    }
}

fn rect(left: i32, top: i32, right: i32, bottom: i32, hwnd: HWND) -> RECT {
    RECT {
        left: scale(left, hwnd),
        top: scale(top, hwnd),
        right: scale(right, hwnd),
        bottom: scale(bottom, hwnd),
    }
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
                    if (wparam.0 & 0xffff) as usize == TITLE_ID
                        || (wparam.0 & 0xffff) as usize == BODY_ID
                    {
                        save_note(hwnd, s);
                    } else if (wparam.0 & 0xffff) as usize == TASK_ID {
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
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}
