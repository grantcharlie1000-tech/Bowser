//! Lazy HTML hosts layered over owned browser chrome windows.

use std::{cell::{Cell, RefCell}, collections::HashMap, rc::{Rc, Weak}, time::{SystemTime, UNIX_EPOCH}};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect, GetMonitorInfoW, HDC, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, IDC_ARROW, IsWindow, LoadCursorW,
                RegisterClassW, SW_SHOW, SetForegroundWindow, ShowWindow, WM_CLOSE, WM_ERASEBKGND, WM_NCDESTROY,
                WM_SIZE, WNDCLASSW, WS_CLIPCHILDREN, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, w},
};
use crate::{
    app::{App, AppRef},
    chrome_host::{host::{Accel, AccelHandler, ChromeEnv, ChromeHost, View}, panels, protocol::{self, Command, PanelFields, PanelType}},
    profile,
    win32::{notes_panel, show_error_message},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind { Notes, Privacy, Rename }
impl Kind {
    fn view(self) -> View { match self { Self::Notes => View::DialogNotes, Self::Privacy => View::DialogPrivacy, Self::Rename => View::DialogRename } }
    fn token(self) -> &'static str { match self { Self::Notes => "notes", Self::Privacy => "privacy", Self::Rename => "rename" } }
    fn title(self) -> &'static str { match self { Self::Notes => "Notes and Tasks", Self::Privacy => "Privacy dashboard", Self::Rename => "Name prompt" } }
}

struct Runtime { app: AppRef, owner: HWND, parent: HWND, kind: Kind, host: Option<ChromeHost>, ready: bool, closed: bool }
thread_local! {
    static RUNTIMES: RefCell<HashMap<isize, Rc<RefCell<Runtime>>>> = RefCell::new(HashMap::new());
    static CLASS_READY: Cell<bool> = const { Cell::new(false) };
    /// The open Privacy dashboard window, so a second request focuses it instead of stacking one.
    static PRIVACY: Cell<isize> = const { Cell::new(0) };
    /// An HTML dialog host could not be created: later dialogs use their native windows.
    static FAILED: Cell<bool> = const { Cell::new(false) };
}

const DIALOG_CLASS: PCWSTR = w!("BowserHtmlDialog");
/// Logical (96 DPI) size of the Privacy dashboard window.
const PRIVACY_WIDTH: i32 = 640;
const PRIVACY_HEIGHT: i32 = 760;
/// The Notes window keeps its native resize strip on the left edge, so the page starts after it.
const NOTES_RESIZE_STRIP_DIP: i32 = 9;

/// Whether Notes and Privacy show the HTML page as their only content: the HTML chrome is
/// on, its environment exists and no dialog host has failed yet. Otherwise the native window runs.
pub(crate) fn html_only(owner: HWND) -> bool {
    crate::chrome_host::html_ui_enabled() && !FAILED.with(Cell::get) && super::window::html_environment(owner).is_some()
}

/// Where a `width` x `height` dialog goes: centred over `owner`, then kept inside the monitor work
/// area (never above its top edge). With `fit` it also shrinks to fit a short work area. Pure.
pub(crate) fn dialog_rect(owner: RECT, work: RECT, width: i32, height: i32, fit: bool) -> RECT {
    let (width, height) = if fit { (width.min(work.right - work.left), height.min(work.bottom - work.top)) } else { (width, height) };
    let left = (owner.left + (owner.right - owner.left - width) / 2).min(work.right - width).max(work.left);
    let top = (owner.top + (owner.bottom - owner.top - height) / 2).min(work.bottom - height).max(work.top);
    RECT { left, top, right: left + width, bottom: top + height }
}

/// The work area (screen minus taskbar) of the monitor `hwnd` is on.
pub(crate) fn work_area_of(hwnd: HWND) -> RECT {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(monitor, &mut info).as_bool() { info.rcWork } else { RECT { left: 0, top: 0, right: 1920, bottom: 1040 } }
    }
}

/// The rectangle the HTML page covers inside its window. Pure.
fn host_bounds(kind: Kind, client: RECT, dpi: u32) -> RECT {
    let strip = if kind == Kind::Notes { NOTES_RESIZE_STRIP_DIP * dpi.max(96) as i32 / 96 } else { 0 };
    RECT { left: (client.left + strip).min(client.right), ..client }
}

fn bounds_of(kind: Kind, parent: HWND) -> RECT {
    let mut client = RECT::default();
    unsafe { let _ = GetClientRect(parent, &mut client); }
    host_bounds(kind, client, unsafe { GetDpiForWindow(parent) })
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(hwnd: HWND, attribute: u32, value: *const std::ffi::c_void, size: u32) -> i32;
}

unsafe extern "system" fn dialog_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match message {
            WM_ERASEBKGND => {
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                let palette = crate::theme::palette(super::glass::appearance(), true);
                let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(u32::from(palette.window.r) | (u32::from(palette.window.g) << 8) | (u32::from(palette.window.b) << 16)));
                FillRect(HDC(wparam.0 as *mut std::ffi::c_void), &client, brush);
                let _ = DeleteObject(brush.into());
                LRESULT(1)
            }
            WM_SIZE => { resize(hwnd); LRESULT(0) }
            WM_CLOSE => { let _ = DestroyWindow(hwnd); LRESULT(0) }
            WM_NCDESTROY => {
                close(hwnd);
                PRIVACY.with(|slot| if slot.get() == hwnd.0 as isize { slot.set(0); });
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

/// An owned top-level window for a dialog that has no native window of its own, centred over
/// `owner` inside its monitor work area. The HTML host becomes its only child.
fn create_window(owner: HWND, title: &str, width_dip: i32, height_dip: i32) -> Option<HWND> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None).ok()?.0);
        if !CLASS_READY.with(Cell::get) {
            let class = WNDCLASSW { lpfnWndProc: Some(dialog_proc), hInstance: instance, hCursor: LoadCursorW(None, IDC_ARROW).ok()?, lpszClassName: DIALOG_CLASS, ..Default::default() };
            if RegisterClassW(&class) == 0 { return None; }
            CLASS_READY.with(|ready| ready.set(true));
        }
        let dpi = GetDpiForWindow(owner).max(96) as i32;
        let mut owner_rect = RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(owner, &mut owner_rect);
        let placed = dialog_rect(owner_rect, work_area_of(owner), width_dip * dpi / 96, height_dip * dpi / 96, true);
        let title: Vec<u16> = title.encode_utf16().chain([0]).collect();
        let hwnd = CreateWindowExW(Default::default(), DIALOG_CLASS, PCWSTR(title.as_ptr()), WS_POPUP | WS_CLIPCHILDREN, placed.left, placed.top, placed.right - placed.left, placed.bottom - placed.top, Some(owner), None, Some(instance), None).ok()?;
        // Rounded corners where the system supports them (Windows 11).
        let round: i32 = 2;
        let _ = DwmSetWindowAttribute(hwnd, 33, (&round as *const i32).cast(), 4);
        Some(hwnd)
    }
}

/// Gives up on a dialog whose HTML host could not be created: its window would stay blank, so it is
/// destroyed and later dialogs use their native windows.
fn abandon(runtime: &Rc<RefCell<Runtime>>) {
    let (kind, parent) = runtime.try_borrow().map(|state| (state.kind, state.parent)).unwrap_or((Kind::Rename, HWND::default()));
    close_runtime(runtime);
    if kind != Kind::Rename && !parent.0.is_null() {
        FAILED.with(|failed| failed.set(true));
        unsafe { let _ = DestroyWindow(parent); }
    }
}

pub(crate) fn attach(app: &AppRef, owner: HWND, parent: HWND, env: ChromeEnv, kind: Kind) {
    let runtime = Rc::new(RefCell::new(Runtime { app: app.clone(), owner, parent, kind, host: None, ready: false, closed: false }));
    RUNTIMES.with(|map| { map.borrow_mut().insert(parent.0 as isize, runtime.clone()); });
    let bounds = bounds_of(kind, parent);
    let weak: Weak<RefCell<Runtime>> = Rc::downgrade(&runtime);
    let commands: Rc<dyn Fn(Command)> = Rc::new(move |command| if let Some(state) = weak.upgrade() { handle(&state, command); });
    let failed_weak = Rc::downgrade(&runtime);
    let failed: Rc<dyn Fn()> = Rc::new(move || if let Some(state) = failed_weak.upgrade() {
        // --- popup surfaces (lane PP) ---
        let rename = state.try_borrow().ok().filter(|s| s.kind == Kind::Rename).map(|s| s.parent);
        // --- end popup surfaces ---
        abandon(&state);
        if let Some(parent) = rename { super::profile_rename::html_failed(parent); }
    });
    let accel_owner = owner;
    let accel: AccelHandler = Rc::new(move |key: Accel| super::window::html_panel_accelerator(accel_owner, key));
    let ready_weak = Rc::downgrade(&runtime);
    let view = kind.view();
    if let Err(error) = ChromeHost::create(&env, parent, bounds, view, commands, failed, Some(accel), move |result| match result {
        Ok(host) => if let Some(state) = ready_weak.upgrade() {
            if let Ok(mut runtime) = state.try_borrow_mut() {
                if runtime.closed { host.close(); return; }
                runtime.host = Some(host.clone());
                let _ = host.set_visible(true);
                let _ = host.focus();
            } else { host.close(); }
        } else { host.close(); },
        Err(_) => if let Some(state) = ready_weak.upgrade() {
            let rename = state.try_borrow().ok().filter(|s| s.kind == Kind::Rename).map(|s| s.parent);
            abandon(&state);
            if let Some(parent) = rename { super::profile_rename::html_failed(parent); }
        },
    }) {
        abandon(&runtime);
        // --- popup surfaces (lane PP) ---
        if kind == Kind::Rename { super::profile_rename::html_failed(parent); return; }
        // --- end popup surfaces ---
        show_error_message(&format!("{} could not be opened in HTML: {error}", kind.title()));
    }
}

/// Opens the Privacy dashboard in its own owned window, or brings the open one to the front. The host
/// used to be a child of the main window, created under the tab and chrome controllers (V2).
pub(crate) fn open_privacy(owner: HWND, app: &AppRef, env: ChromeEnv) {
    let existing = PRIVACY.with(Cell::get);
    if existing != 0 {
        let window = HWND(existing as *mut std::ffi::c_void);
        if unsafe { IsWindow(Some(window)) }.as_bool() {
            unsafe { let _ = ShowWindow(window, SW_SHOW); let _ = SetForegroundWindow(window); }
            if let Some(host) = RUNTIMES.with(|map| map.borrow().get(&existing).cloned()).and_then(|runtime| runtime.try_borrow().ok().and_then(|state| state.host.clone())) {
                let _ = host.focus();
            }
            return;
        }
        PRIVACY.with(|slot| slot.set(0));
    }
    let Some(window) = create_window(owner, Kind::Privacy.title(), PRIVACY_WIDTH, PRIVACY_HEIGHT) else {
        show_error_message("The Privacy dashboard could not be opened.");
        return;
    };
    PRIVACY.with(|slot| slot.set(window.0 as isize));
    attach(app, owner, window, env, Kind::Privacy);
    unsafe { let _ = ShowWindow(window, SW_SHOW); let _ = SetForegroundWindow(window); }
}

pub(crate) fn resize(parent: HWND) {
    let runtime = RUNTIMES.with(|map| map.borrow().get(&(parent.0 as isize)).cloned());
    let Some(runtime) = runtime else { return; };
    let (host, closed) = runtime.try_borrow().map(|state| (state.host.clone(), state.closed)).unwrap_or((None, true));
    if closed { return; }
    if let Some(host) = host {
        let kind = runtime.try_borrow().map(|state| state.kind).unwrap_or(Kind::Rename);
        if host.set_bounds(bounds_of(kind, parent)).is_err() { close_runtime(&runtime); }
    }
}

pub(crate) fn close(parent: HWND) {
    if let Some(runtime) = RUNTIMES.with(|map| map.borrow_mut().remove(&(parent.0 as isize))) { close_runtime(&runtime); }
}

fn close_runtime(runtime: &Rc<RefCell<Runtime>>) {
    let (parent, host) = runtime.try_borrow_mut().ok().map(|mut state| { state.closed = true; (state.parent, state.host.take()) }).unwrap_or((HWND::default(), None));
    RUNTIMES.with(|map| { map.borrow_mut().remove(&(parent.0 as isize)); });
    if let Some(host) = host { host.close(); }
}

fn handle(runtime: &Rc<RefCell<Runtime>>, command: Command) {
    match command {
        Command::Ready => {
            if let Ok(mut state) = runtime.try_borrow_mut() { state.ready = true; }
            send_current(runtime);
            // --- popup surfaces (lane PP) ---
            let (kind, parent, host) = { let state = runtime.borrow(); (state.kind, state.parent, state.host.clone()) };
            if kind == Kind::Rename {
                if super::profile_rename::html_ready(parent) {
                    if let Some(host) = host { let _ = host.focus(); }
                } else {
                    close_runtime(runtime);
                }
            }
            // --- end popup surfaces ---
        }
        // --- popup surfaces (lane PP) ---
        Command::Panel { kind: PanelType::Panel, f } if empty_fields(&f, false) && f.a == "dismiss" && runtime.borrow().kind == Kind::Rename => {
            super::profile_rename::html_cancel(runtime.borrow().parent);
        }
        Command::Panel { kind: PanelType::Rename, f } if runtime.borrow().kind == Kind::Rename => handle_rename(runtime, f),
        // --- end popup surfaces ---
        Command::Panel { kind: PanelType::Panel, f } if empty_fields(&f, false) && f.a == "dismiss" => {
            // Every HTML dialog owns its window, so the close button and Esc destroy it; the
            // window's destruction then releases the host.
            let parent = runtime.borrow().parent;
            unsafe { let _ = DestroyWindow(parent); }
        }
        Command::Panel { kind: PanelType::Notes, f } if runtime.borrow().kind == Kind::Notes => handle_notes(runtime, f),
        Command::Panel { kind: PanelType::Privacy, f } if runtime.borrow().kind == Kind::Privacy => handle_privacy(runtime, f),
        _ => {}
    }
}

fn send_current(runtime: &Rc<RefCell<Runtime>>) {
    let (host, ready, owner, kind) = match runtime.try_borrow() { Ok(state) => (state.host.clone(), state.ready, state.owner, state.kind), Err(_) => return };
    let Some(host) = host.filter(|_| ready) else { return; };
    let _ = host.post_json(&super::window::html_panel_state(owner));
    if let Some(message) = protocol::panelshow_json(kind.token()) { let _ = host.post_json(&message); }
    let snapshot = match kind { Kind::Notes => notes_snapshot(), Kind::Privacy => privacy_snapshot(runtime), Kind::Rename => rename_snapshot(runtime) };
    if let Some(snapshot) = snapshot { let _ = host.post_json(&snapshot); }
    if kind == Kind::Privacy {
        let app = runtime.try_borrow().ok().map(|state| state.app.clone());
        if let Some(snapshot) = app.and_then(|app| protection_snapshot_json(&app)) { let _ = host.post_json(&snapshot); }
    }
}

fn privacy_snapshot(runtime: &Rc<RefCell<Runtime>>) -> Option<String> {
    let app = runtime.try_borrow().ok()?.app.clone();
    privacy_snapshot_json(&app)
}

/// The Privacy dashboard's `privacy` snapshot, for its own window and for the settings page.
pub(crate) fn privacy_snapshot_json(app: &AppRef) -> Option<String> {
    let profile = profile::active().ok();
    let snapshot = super::privacy_panel::gather(&app, profile.as_ref(), None);
    protocol::snap_json("privacy", |out| {
        out.string("profile", &snapshot.profile); out.bool("guest", snapshot.guest);
        out.object("stats", |stats| {
            stats.uint("allTime", snapshot.stats.all_time_total()); stats.uint("session", snapshot.session_total);
            stats.uint("currentSite", snapshot.site_total);
            stats.array("topSites", |items| for (host, count) in &snapshot.top_sites { items.object(|item| { item.string("host", host); item.uint("count", *count); }); });
            stats.array("days", |items| for (day, count) in &snapshot.days { items.object(|item| { item.string("day", day); item.uint("count", *count); }); });
        });
        if let Some(site) = &snapshot.host { out.string("site", site); } else { out.null("site"); }
        out.string("security", match snapshot.security { crate::site_security::SiteSecurity::Secure => "secure", crate::site_security::SiteSecurity::NotSecure => "notsecure", crate::site_security::SiteSecurity::CertificateProblem => "certproblem", crate::site_security::SiteSecurity::Internal => "internal", crate::site_security::SiteSecurity::Unknown => "unknown", crate::site_security::SiteSecurity::NoSite => "none" });
        if let Some(blocking) = snapshot.blocking { out.bool("blocking", blocking); } else { out.null("blocking"); }
        if let Some(tracking) = snapshot.tracking { out.string("tracking", tracking); } else { out.null("tracking"); }
        out.string("trackingSetting", snapshot.tracking_setting); out.bool("efficiencyActive", snapshot.efficiency_active);
        if let Some(enabled) = snapshot.smart_screen { out.bool("smartScreen", enabled); } else { out.null("smartScreen"); }
        out.bool("blockerSetting", snapshot.blocker_setting); out.bool("sleeping", snapshot.sleeping); out.uint("sleepAfter", snapshot.sleep_after);
        let denied = App::denied_requests(&app);
        let sites = {
            // A private tab's site is never offered as "current": a stored decision made here would
            // be written to the shared permission store.
            let current = App::active_site(&app)
                .filter(|(tab, _)| !App::tab_private(&app, *tab))
                .and_then(|(_, url)| crate::permissions::origin_of(url.as_str()));
            let store = App::permissions_handle(&app);
            let guard = store.as_ref().map(|store| store.borrow());
            let empty = crate::permissions::PermissionStore::default();
            let also: Vec<&str> = denied.iter().map(|attempt| attempt.origin.as_str()).collect();
            panels::privacy::site_rows(guard.as_deref().unwrap_or(&empty), current.as_deref(), &also)
        };
        panels::privacy::write_permissions(out, &sites, &denied, crate::permissions::now_secs());
        if let Some(status) = &snapshot.status { out.string("status", status); } else { out.null("status"); }
    })
}

/// The Privacy section's `protection` snapshot (blocking, cookie notices, history retention,
/// website appearance, Delete Browsing Data; thread T3), for the settings page and the dashboard
/// window. It has its own snapshot, so its size never counts against the permissions table's.
pub(crate) fn protection_snapshot_json(app: &AppRef) -> Option<String> {
    let view = super::protection::gather(app);
    protocol::snap_json("protection", |out| panels::protection::write(out, &view))
}

fn handle_privacy(runtime: &Rc<RefCell<Runtime>>, f: PanelFields) {
    let (app, owner) = match runtime.try_borrow() { Ok(state) => (state.app.clone(), state.owner), Err(_) => return };
    if run_privacy_command(&app, owner, &f) { send_current(runtime); }
}

/// Runs one `priv` command from the dashboard (its window or the settings page). Returns whether
/// the page should be sent a fresh snapshot, i.e. the command was a dashboard command.
pub(crate) fn run_privacy_command(app: &AppRef, owner: HWND, f: &PanelFields) -> bool {
    // Permission dashboard commands are validated in `panels::privacy::map_command`; only a stored
    // Allow ever grants, and nothing here asks the page or the user for a decision.
    if let Some(action) = panels::privacy::map_command(f) {
        apply_permission_action(app, action);
        return true;
    }
    // Blocking, cookie notices, history retention, website appearance and Delete Browsing Data
    // (thread T3) are validated in `panels::protection::map_command`. The answer to a command
    // arrives in the next `protection` snapshot, which the page is always sent after one.
    if let Some(action) = panels::protection::map_command(f) {
        super::protection::run(app, owner, action);
        return true;
    }
    let id = match f.a.as_str() {
        "clear-stats" if empty_fields(f, false) => Some(8_800),
        _ => None,
    };
    let Some(id) = id else { return false; };
    let profile = profile::active().ok();
    let snapshot = super::privacy_panel::gather(app, profile.as_ref(), None);
    let _ = super::privacy_panel::apply(owner, app, &snapshot, id);
    true
}

fn apply_permission_action(app: &crate::app::AppRef, action: panels::privacy::Action) {
    use panels::privacy::Action;
    if matches!(action, Action::ClearDenied) {
        App::clear_denied_requests(app);
        return;
    }
    let Some(store) = App::permissions_handle(app) else { return; };
    let result = match action {
        Action::Set { origin, kind, decision } => store.borrow_mut().set(&origin, kind, decision),
        Action::ResetSite(origin) => store.borrow_mut().reset_site(&origin).map(|_| ()),
        Action::ResetAll => store.borrow_mut().reset_all().map(|_| ()),
        Action::ClearDenied => Ok(()),
    };
    if let Err(error) = result { show_error_message(&format!("Bowser could not save the permission: {error}")); }
}

// --- popup surfaces (lane PP) ---
fn rename_snapshot(runtime: &Rc<RefCell<Runtime>>) -> Option<String> {
    let parent = runtime.try_borrow().ok()?.parent;
    let request = super::profile_rename::html_request(parent)?;
    protocol::snap_json("rename", |out| {
        out.string("title", &request.title);
        out.string("save", &request.save);
        out.string("value", crate::chrome_host::json::capped(&request.value, 2048));
        out.string("hint", &request.hint);
        out.string("error", &request.error);
        out.uint("max", request.max as u64);
    })
}

/// `{t:"ren",a:"save",text}` or `{t:"ren",a:"cancel"}`. Rust validates again; an invalid name is
/// answered with the error in a fresh snapshot.
fn handle_rename(runtime: &Rc<RefCell<Runtime>>, f: PanelFields) {
    let parent = runtime.borrow().parent;
    match f.a.as_str() {
        "save" if empty_fields(&f, true) && f.text.is_some() => {
            if super::profile_rename::html_submit(parent, f.text.as_deref().unwrap_or("")).is_err() {
                send_current(runtime);
            }
        }
        "cancel" if empty_fields(&f, false) => super::profile_rename::html_cancel(parent),
        _ => {}
    }
}
// --- end popup surfaces ---

fn notes_snapshot() -> Option<String> {
    let profile = profile::active().ok()?;
    let notes = notes_panel::html_notes(&profile);
    let tasks = notes_panel::html_tasks(&profile);
    protocol::snap_json("notes", |out| {
        out.array("notes", |items| for note in &notes { items.object(|item| { item.uint("id", note.id); item.string("title", &note.title); item.string("body", &note.body); }); });
        out.array("tasks", |items| for task in &tasks { items.object(|item| { item.uint("id", task.id); item.string("text", &task.text); item.bool("complete", task.complete); }); });
    })
}

fn handle_notes(runtime: &Rc<RefCell<Runtime>>, f: PanelFields) {
    let Ok(profile) = profile::active() else { return; };
    let mut notes = notes_panel::html_notes(&profile);
    let mut tasks = notes_panel::html_tasks(&profile);
    let result = match f.a.as_str() {
        "add-note" if empty_fields(&f, false) => { let id = next_id(notes.iter().map(|n| n.id)); notes.push(crate::productivity::Note { id, title: "New note".into(), body: String::new() }); notes_panel::save_html_notes(&profile, &notes) }
        "save-title" if f.parent.is_none() && f.before.is_none() && f.n.is_none() && f.on.is_none() && f.url.is_none() && f.id.is_some() && f.text.as_ref().is_some_and(|s| s.chars().count() <= 2048) => {
            let Some(note) = notes.iter_mut().find(|item| item.id == f.id.unwrap_or_default()) else { return; }; note.title = f.text.unwrap_or_default(); notes_panel::save_html_notes(&profile, &notes)
        }
        "save-body" if f.parent.is_none() && f.before.is_none() && f.n.is_none() && f.on.is_none() && f.url.is_none() && f.id.is_some() && f.text.as_ref().is_some_and(|s| s.chars().count() <= 2048) => {
            let Some(note) = notes.iter_mut().find(|item| item.id == f.id.unwrap_or_default()) else { return; }; note.body = f.text.unwrap_or_default(); notes_panel::save_html_notes(&profile, &notes)
        }
        "delete-note" if id_only(&f) => { notes.retain(|item| item.id != f.id.unwrap_or_default()); notes_panel::save_html_notes(&profile, &notes) }
        "add-task" if empty_fields(&f, false) => { let id = next_id(tasks.iter().map(|task| task.id)); tasks.push(crate::productivity::Task { id, complete: false, text: "New task".into() }); notes_panel::save_html_tasks(&profile, &tasks) }
        "save-task" if f.parent.is_none() && f.before.is_none() && f.n.is_none() && f.on.is_none() && f.url.is_none() && f.id.is_some() && f.text.as_ref().is_some_and(|s| s.chars().count() <= 2048) => {
            let Some(task) = tasks.iter_mut().find(|item| item.id == f.id.unwrap_or_default()) else { return; }; task.text = f.text.unwrap_or_default(); notes_panel::save_html_tasks(&profile, &tasks)
        }
        "delete-task" if id_only(&f) => { tasks.retain(|item| item.id != f.id.unwrap_or_default()); notes_panel::save_html_tasks(&profile, &tasks) }
        "toggle-task" if f.parent.is_none() && f.before.is_none() && f.n.is_none() && f.text.is_none() && f.url.is_none() && f.id.is_some() && f.on.is_some() => {
            let Some(task) = tasks.iter_mut().find(|item| item.id == f.id.unwrap_or_default()) else { return; }; task.complete = f.on.unwrap_or(false); notes_panel::save_html_tasks(&profile, &tasks)
        }
        _ => return,
    };
    if let Err(error) = result { show_error_message(&format!("Notes could not be saved: {error}")); }
    send_current(runtime);
}

fn empty_fields(f: &PanelFields, allow_text: bool) -> bool { f.id.is_none() && f.parent.is_none() && f.before.is_none() && f.n.is_none() && f.on.is_none() && f.url.is_none() && (allow_text || f.text.is_none()) }
fn id_only(f: &PanelFields) -> bool { f.id.is_some() && f.parent.is_none() && f.before.is_none() && f.n.is_none() && f.on.is_none() && f.text.is_none() && f.url.is_none() }
fn next_id(existing: impl Iterator<Item = u64>) -> u64 { existing.max().unwrap_or(0).saturating_add(1).max(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_micros() as u64) }

#[cfg(test)]
mod tests {
    use super::*;
    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT { RECT { left, top, right, bottom } }

    #[test]
    fn dialog_is_centred_over_the_owner_and_never_leaves_the_work_area() {
        let work = rect(0, 0, 1920, 1040);
        // Centred over a normal window.
        let placed = dialog_rect(rect(40, 30, 1320, 830), work, 640, 720, true);
        assert_eq!(placed, rect(360, 70, 1000, 790));
        // A native window taller than the owner used to start 16px above the screen top (V3).
        let tall = dialog_rect(rect(40, 30, 1320, 830), work, 600, 892, false);
        assert_eq!((tall.top, tall.bottom - tall.top), (0, 892));
        // Shrinks to a short work area instead of overflowing it.
        let short = dialog_rect(rect(0, 0, 1280, 720), rect(0, 0, 1280, 680), 640, 720, true);
        assert_eq!((short.top, short.bottom), (0, 680));
        // An owner dragged mostly off the right edge still leaves the dialog on screen.
        let edge = dialog_rect(rect(1700, 100, 2500, 800), work, 640, 600, true);
        assert_eq!(edge.right, 1920);
        assert!(edge.left >= 0);
        // A second monitor to the left keeps its own origin.
        let left_monitor = dialog_rect(rect(-1900, 50, -700, 750), rect(-1920, 0, 0, 1040), 640, 600, true);
        assert!(left_monitor.left >= -1920 && left_monitor.right <= 0);
    }

    #[test]
    fn notes_page_leaves_the_native_resize_strip_and_other_dialogs_fill_the_window() {
        let client = rect(0, 0, 340, 700);
        assert_eq!(host_bounds(Kind::Notes, client, 96), rect(9, 0, 340, 700));
        assert_eq!(host_bounds(Kind::Notes, client, 192), rect(18, 0, 340, 700));
        assert_eq!(host_bounds(Kind::Rename, client, 192), client);
        assert_eq!(host_bounds(Kind::Privacy, client, 96), client);
        // A window narrower than the strip never inverts the rectangle.
        assert_eq!(host_bounds(Kind::Notes, rect(0, 0, 4, 10), 96).left, 4);
    }
}
