//! The address bar's two small dialogs: the QR code of the shown page, and the bookmark dialog (name,
//! folder, nickname, description). One owned top-level window each, centred over the browser window,
//! holding one HTML host (`View::DialogAddress`) that draws whichever page the snapshot names.
//!
//! Every function runs on the UI thread.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect, HDC},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowRect, IDC_ARROW, IsWindow, LoadCursorW, RegisterClassW,
                SW_SHOW, SetForegroundWindow, ShowWindow, WM_CLOSE, WM_ERASEBKGND, WM_NCDESTROY, WM_SIZE, WNDCLASSW, WS_CLIPCHILDREN, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, w},
};

use crate::{
    app::{App, AppRef},
    bookmark_extras::Extras,
    bookmarks::{BookmarkKind, BookmarkStore},
    chrome_host::{
        address_page::{self, DialogAction, Folder},
        host::{Accel, AccelHandler, ChromeEnv, ChromeHost, View},
        protocol::{self, Command, PanelType},
    },
    tabs::TabId,
};

const CLASS: PCWSTR = w!("BowserAddressDialog");
const QR_SIZE: (i32, i32) = (360, 470);
const BOOKMARK_SIZE: (i32, i32) = (400, 520);

/// Takes the page's declared title and description (when it could be read).
pub(crate) type MetadataDone = Box<dyn FnOnce(Option<(String, String)>)>;

/// What the open dialog is about.
#[derive(Clone, Debug)]
enum Page {
    Qr { url: String },
    Bookmark { url: String, title: String, description: String },
}

struct Runtime {
    owner: HWND,
    window: HWND,
    app: AppRef,
    page: Page,
    host: Option<ChromeHost>,
    ready: bool,
    closed: bool,
    error: String,
}

thread_local! {
    static RUNTIMES: RefCell<HashMap<isize, Rc<RefCell<Runtime>>>> = RefCell::new(HashMap::new());
    static CLASS_READY: Cell<bool> = const { Cell::new(false) };
}

unsafe extern "system" fn dialog_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match message {
            WM_ERASEBKGND => {
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                let palette = crate::theme::palette(super::glass::appearance(), true);
                let colour = windows::Win32::Foundation::COLORREF(u32::from(palette.window.r) | (u32::from(palette.window.g) << 8) | (u32::from(palette.window.b) << 16));
                let brush = CreateSolidBrush(colour);
                FillRect(HDC(wparam.0 as *mut std::ffi::c_void), &client, brush);
                let _ = DeleteObject(brush.into());
                LRESULT(1)
            }
            WM_SIZE => {
                resize(hwnd);
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                close(hwnd);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

fn bounds_of(window: HWND) -> RECT {
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(window, &mut client);
    }
    client
}

fn create_window(owner: HWND, title: &str, size: (i32, i32)) -> Option<HWND> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None).ok()?.0);
        if !CLASS_READY.with(Cell::get) {
            let class = WNDCLASSW { lpfnWndProc: Some(dialog_proc), hInstance: instance, hCursor: LoadCursorW(None, IDC_ARROW).ok()?, lpszClassName: CLASS, ..Default::default() };
            if RegisterClassW(&class) == 0 {
                return None;
            }
            CLASS_READY.with(|ready| ready.set(true));
        }
        let dpi = GetDpiForWindow(owner).max(96) as i32;
        let mut owner_rect = RECT::default();
        let _ = GetWindowRect(owner, &mut owner_rect);
        let placed = super::html_dialog::dialog_rect(owner_rect, super::html_dialog::work_area_of(owner), size.0 * dpi / 96, size.1 * dpi / 96, true);
        let title: Vec<u16> = title.encode_utf16().chain([0]).collect();
        let hwnd = CreateWindowExW(Default::default(), CLASS, PCWSTR(title.as_ptr()), WS_POPUP | WS_CLIPCHILDREN, placed.left, placed.top, placed.right - placed.left, placed.bottom - placed.top, Some(owner), None, Some(instance), None).ok()?;
        let round: i32 = 2;
        let _ = DwmSetWindowAttribute(hwnd, 33, (&round as *const i32).cast(), 4);
        Some(hwnd)
    }
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(hwnd: HWND, attribute: u32, value: *const std::ffi::c_void, size: u32) -> i32;
}

fn open(owner: HWND, app: &AppRef, page: Page) {
    let Some(env) = super::window::html_environment(owner) else {
        super::show_error_message("This dialog needs the HTML chrome, which is not available.");
        return;
    };
    let (title, size) = match &page {
        Page::Qr { .. } => ("QR code", QR_SIZE),
        Page::Bookmark { .. } => ("Bookmark", BOOKMARK_SIZE),
    };
    // One at a time: a second request replaces the first.
    let existing: Vec<HWND> = RUNTIMES.with(|map| map.borrow().values().filter_map(|runtime| runtime.try_borrow().ok().map(|state| state.window)).collect());
    for window in existing {
        unsafe {
            if IsWindow(Some(window)).as_bool() {
                let _ = DestroyWindow(window);
            }
        }
    }
    let Some(window) = create_window(owner, title, size) else {
        super::show_error_message("The dialog could not be opened.");
        return;
    };
    let runtime = Rc::new(RefCell::new(Runtime { owner, window, app: app.clone(), page, host: None, ready: false, closed: false, error: String::new() }));
    RUNTIMES.with(|map| map.borrow_mut().insert(window.0 as isize, runtime.clone()));
    attach(&runtime, env);
    unsafe {
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
    }
}

fn attach(runtime: &Rc<RefCell<Runtime>>, env: ChromeEnv) {
    let (owner, window) = {
        let state = runtime.borrow();
        (state.owner, state.window)
    };
    let weak: Weak<RefCell<Runtime>> = Rc::downgrade(runtime);
    let commands: Rc<dyn Fn(Command)> = Rc::new(move |command| {
        if let Some(state) = weak.upgrade() {
            handle(&state, command);
        }
    });
    let failed_weak = Rc::downgrade(runtime);
    let failed: Rc<dyn Fn()> = Rc::new(move || {
        if let Some(state) = failed_weak.upgrade() {
            let window = state.borrow().window;
            unsafe {
                let _ = DestroyWindow(window);
            }
        }
    });
    let accel: AccelHandler = Rc::new(move |key: Accel| super::window::html_panel_accelerator(owner, key));
    let ready_weak = Rc::downgrade(runtime);
    let created = ChromeHost::create(&env, crate::chrome_host::host::ChromeHostOptions { parent: window, bounds: bounds_of(window), view: View::DialogAddress, on_command: commands, on_failed: failed, on_accelerator: Some(accel) }, move |result| match result {
        Ok(host) => {
            if let Some(state) = ready_weak.upgrade() {
                if let Ok(mut runtime) = state.try_borrow_mut() {
                    if runtime.closed {
                        host.close();
                        return;
                    }
                    runtime.host = Some(host.clone());
                    let _ = host.set_visible(true);
                    let _ = host.focus();
                } else {
                    host.close();
                }
            } else {
                host.close();
            }
        }
        Err(_) => {
            if let Some(state) = ready_weak.upgrade() {
                let window = state.borrow().window;
                unsafe {
                    let _ = DestroyWindow(window);
                }
            }
        }
    });
    if created.is_err() {
        unsafe {
            let _ = DestroyWindow(window);
        }
        super::show_error_message("The dialog could not be opened.");
    }
}

fn resize(window: HWND) {
    let runtime = RUNTIMES.with(|map| map.borrow().get(&(window.0 as isize)).cloned());
    let Some(runtime) = runtime else { return };
    let host = runtime.try_borrow().ok().and_then(|state| state.host.clone());
    if let Some(host) = host {
        let _ = host.set_bounds(bounds_of(window));
    }
}

fn close(window: HWND) {
    if let Some(runtime) = RUNTIMES.with(|map| map.borrow_mut().remove(&(window.0 as isize))) {
        let host = runtime.try_borrow_mut().ok().and_then(|mut state| {
            state.closed = true;
            state.host.take()
        });
        if let Some(host) = host {
            host.close();
        }
    }
}

fn handle(runtime: &Rc<RefCell<Runtime>>, command: Command) {
    match command {
        Command::Ready => {
            if let Ok(mut state) = runtime.try_borrow_mut() {
                state.ready = true;
            }
            send_current(runtime);
        }
        Command::Panel { kind: PanelType::Panel, f } if f.a == "dismiss" => {
            let window = runtime.borrow().window;
            unsafe {
                let _ = DestroyWindow(window);
            }
        }
        Command::Panel { kind: PanelType::Address, f } => {
            if let Some(action) = address_page::dialog_command(&f) {
                run(runtime, action);
            }
        }
        _ => {}
    }
}

fn send_current(runtime: &Rc<RefCell<Runtime>>) {
    let (host, ready, owner, page, error) = match runtime.try_borrow() {
        Ok(state) => (state.host.clone(), state.ready, state.owner, state.page.clone(), state.error.clone()),
        Err(_) => return,
    };
    let Some(host) = host.filter(|_| ready) else { return };
    let _ = host.post_json(&super::window::html_panel_state(owner));
    if let Some(message) = protocol::panelshow_json("addressdlg") {
        let _ = host.post_json(&message);
    }
    let snapshot = match &page {
        Page::Qr { url } => Some(address_page::dialog_snapshot_json(&address_page::DialogPage::Qr { text: url })),
        Page::Bookmark { url, title, description, .. } => {
            let profile = crate::profile::active().ok();
            let root = profile.as_ref().map(|profile| profile.root().to_path_buf());
            let store = root.as_deref().and_then(|root| BookmarkStore::load(root).ok()).unwrap_or_default();
            let extras = root.as_deref().map(Extras::load).unwrap_or_default();
            let existing = store.items().iter().find(|item| matches!(&item.kind, BookmarkKind::Link { url: stored, .. } if stored == url));
            let (nickname, saved_description) = existing.map_or(("", description.as_str()), |item| (extras.nickname(item.id), extras.get(item.id).map_or("", |extra| extra.description.as_str())));
            let folders = folders_of(&store);
            Some(address_page::dialog_snapshot_json(&address_page::DialogPage::Bookmark(address_page::BookmarkDialog {
                url,
                existing: existing.is_some(),
                name: existing.map_or(title.as_str(), |item| item.title.as_str()),
                folder: existing.and_then(|item| item.parent).unwrap_or(0),
                nickname,
                description: saved_description,
                folders: &folders,
                error: &error,
            })))
        }
    };
    if let Some(snapshot) = snapshot {
        let _ = host.post_json(&snapshot);
    }
}

/// Every folder of `store`, in tree order with its depth.
pub(crate) fn folders_of(store: &BookmarkStore) -> Vec<Folder> {
    fn walk(store: &BookmarkStore, parent: Option<u64>, depth: u32, out: &mut Vec<Folder>) {
        for item in store.children(parent) {
            if matches!(item.kind, BookmarkKind::Folder) && out.len() < address_page::MAX_FOLDERS {
                out.push(Folder { id: item.id, title: item.title.clone(), depth });
                walk(store, Some(item.id), depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(store, None, 0, &mut out);
    out
}

fn run(runtime: &Rc<RefCell<Runtime>>, action: DialogAction) {
    let (app, owner, window, page) = {
        let state = runtime.borrow();
        (state.app.clone(), state.owner, state.window, state.page.clone())
    };
    match (action, page) {
        (DialogAction::QrCopy, Page::Qr { url }) => {
            let _ = super::window::copy_text_to_clipboard(owner, &url);
        }
        (DialogAction::BookmarkSave { name, nickname, description, folder }, Page::Bookmark { url, .. }) => {
            match save_bookmark(&app, owner, &url, &name, folder, &nickname, &description) {
                Ok(()) => unsafe {
                    let _ = DestroyWindow(window);
                },
                Err(message) => {
                    if let Ok(mut state) = runtime.try_borrow_mut() {
                        state.error = message;
                    }
                    send_current(runtime);
                }
            }
        }
        (DialogAction::BookmarkRemove, Page::Bookmark { url, .. }) => {
            remove_bookmark(owner, &url);
            unsafe {
                let _ = DestroyWindow(window);
            }
        }
        _ => {}
    }
}

/// The folder id a dialog sends (`0` is the top level) as a parent.
fn parent_of(folder: u64) -> Option<u64> {
    (folder != 0).then_some(folder)
}

/// Saves (adds or updates) the bookmark for `url` with the dialog's fields.
fn save_bookmark(app: &AppRef, owner: HWND, url: &str, name: &str, folder: u64, nickname: &str, description: &str) -> Result<(), String> {
    let profile = crate::profile::active().map_err(|_| "The profile is not ready.".to_owned())?;
    if !crate::productivity::profile_allows_productivity(&profile) {
        return Err("Bookmarks are not saved in a guest session.".to_owned());
    }
    let mut store = BookmarkStore::load(profile.root()).map_err(|error| format!("Bookmarks could not be loaded: {error}"))?;
    let mut extras = Extras::load(profile.root());
    extras.prune(&store);
    let parent = parent_of(folder);
    let existing = store.items().iter().find(|item| matches!(&item.kind, BookmarkKind::Link { url: stored, .. } if stored == url)).map(|item| item.id);
    // The nickname is checked before anything is written.
    let id = match existing {
        Some(id) => {
            extras.set(id, nickname, description)?;
            store.update_bookmark(id, name, url).map_err(|error| error.to_string())?;
            let at = store.children(parent).len();
            store.move_item(id, parent, at).map_err(|error| error.to_string())?;
            id
        }
        None => {
            let id = store.add_bookmark(parent, name, url).map_err(|error| error.to_string())?;
            extras.set(id, nickname, description)?;
            id
        }
    };
    store.save(profile.root()).map_err(|error| format!("Bookmark could not be saved: {error}"))?;
    extras.save(profile.root()).map_err(|error| format!("Bookmark details could not be saved: {error}"))?;
    if existing.is_none() {
        let _ = crate::productivity::add_bookmark(&profile.bookmarks_file(), url, name);
    }
    let _ = (app, id);
    super::post(owner, super::WM_RAIL_CHANGED, WPARAM(0), LPARAM(0));
    Ok(())
}

fn remove_bookmark(owner: HWND, url: &str) {
    let Ok(profile) = crate::profile::active() else { return };
    let Ok(mut store) = BookmarkStore::load(profile.root()) else { return };
    let Some(id) = store.items().iter().find(|item| matches!(&item.kind, BookmarkKind::Link { url: stored, .. } if stored == url)).map(|item| item.id) else { return };
    store.delete(id);
    let mut extras = Extras::load(profile.root());
    extras.remove(id);
    if store.save(profile.root()).is_ok() {
        let _ = extras.save(profile.root());
        let _ = crate::productivity::remove_bookmark(&profile.bookmarks_file(), url);
        super::post(owner, super::WM_RAIL_CHANGED, WPARAM(0), LPARAM(0));
    }
}

/// The QR code of the page at `url`.
pub(super) fn show_qr(owner: HWND, app: &AppRef, url: &str) {
    open(owner, app, Page::Qr { url: url.to_owned() });
}

/// The bookmark dialog for the shown tab, with the title and description the page declares when
/// `bm_fetch_metadata` is on and the page is not bookmarked yet.
pub(super) fn show_bookmark(owner: HWND, app: &AppRef) {
    let tab = App::active_tab_id(app);
    let Some(url) = App::tab_committed_url(app, tab) else { return };
    let title = App::tab_title(app, tab).unwrap_or_else(|| url.host_str().unwrap_or("Bookmark").to_owned());
    let user_name = App::tab_name(app, tab); // --- wave-1 fixes --- a name the user gave the tab is kept
    let fetch = App::settings(app).address.fetch_metadata && !super::html_suggest::is_bookmarked(url.as_str());
    let raw = owner.0 as isize;
    let app_handle = app.clone();
    let url_text = url.as_str().to_owned();
    if !fetch {
        open(owner, app, Page::Bookmark { url: url_text, title, description: String::new() });
        return;
    }
    fetch_metadata(app, tab, Box::new(move |meta| {
        let owner = HWND(raw as *mut _);
        if !unsafe { IsWindow(Some(owner)) }.as_bool() {
            return;
        }
        let (title, description) = match meta {
            Some((meta_title, description)) => (bookmark_title(user_name.as_deref(), &title, &meta_title, &url_text), description),
            None => (title, String::new()),
        };
        open(owner, &app_handle, Page::Bookmark { url: url_text, title, description });
    }));
}

thread_local! {
    /// What the page declared, set between the metadata answer and the save that uses it.
    static READY_META: RefCell<Option<Option<(String, String)>>> = const { RefCell::new(None) };
}

/// --- wave-1 fixes --- what Ctrl+D (the bookmark button) does under the bookmark options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuickAdd {
    /// "Always open the bookmark dialog": the dialog instead of a quick save.
    Dialog,
    /// "Fetch bookmark details from the page": read the page, then save with what it declares.
    ReadPageFirst,
    /// Save at once with the tab's title.
    SaveNow,
}

fn quick_add_plan(address: &crate::address_settings::AddressSettings, has_page: bool, bookmarked: bool) -> QuickAdd {
    if address.always_dialog {
        QuickAdd::Dialog
    } else if has_page && address.fetch_metadata && !bookmarked {
        QuickAdd::ReadPageFirst
    } else {
        QuickAdd::SaveNow
    }
}
/// Ctrl+D (the bookmark button) before it saves: with "always open the bookmark dialog" the dialog
/// opens instead, and with "fetch bookmark details from the page" a page that is not bookmarked yet is
/// read first and the save runs again with what it declared. `true` when the save is not to run now.
pub(super) fn defer_quick_add(owner: HWND, app: &AppRef) -> bool {
    if READY_META.with(|slot| slot.borrow().is_some()) {
        return false; // the second run, with the page's details in hand
    }
    let settings = App::settings(app);
    let tab = App::active_tab_id(app);
    let url = App::tab_committed_url(app, tab);
    let bookmarked = url.as_ref().is_some_and(|url| super::html_suggest::is_bookmarked(url.as_str()));
    match quick_add_plan(&settings.address, url.is_some(), bookmarked) {
        QuickAdd::Dialog => {
            show_bookmark(owner, app);
            return true;
        }
        QuickAdd::SaveNow => return false,
        QuickAdd::ReadPageFirst => {}
    }
    let raw = owner.0 as isize;
    let app_handle = app.clone();
    fetch_metadata(app, tab, Box::new(move |meta| {
        let owner = HWND(raw as *mut _);
        if !unsafe { IsWindow(Some(owner)) }.as_bool() {
            return;
        }
        READY_META.with(|slot| *slot.borrow_mut() = Some(meta));
        super::window::add_bookmark_now(owner, &app_handle);
        READY_META.with(|slot| *slot.borrow_mut() = None);
    }));
    true
}

/// The title to save and the description that goes beside it: the page's own details when
/// [`defer_quick_add`] fetched them, else the tab's title and nothing.
pub(super) fn apply_metadata(tab_title: String, user_name: Option<&str>, url: &str) -> (String, String) {
    match READY_META.with(|slot| slot.borrow().clone()) {
        Some(Some((meta_title, description))) => (bookmark_title(user_name, &tab_title, &meta_title, url), description),
        _ => (tab_title, String::new()),
    }
}

/// Keeps `description` with the bookmark just saved for `url`.
pub(super) fn save_description(profile: &crate::profile::Profile, url: &str, description: &str) {
    let Ok(store) = BookmarkStore::load(profile.root()) else { return };
    let Some(id) = store.items().iter().find(|item| matches!(&item.kind, BookmarkKind::Link { url: stored, .. } if stored == url)).map(|item| item.id) else { return };
    let mut extras = Extras::load(profile.root());
    extras.prune(&store);
    let nickname = extras.nickname(id).to_owned();
    if extras.set(id, &nickname, description).is_ok() {
        let _ = extras.save(profile.root());
    }
}

/// --- wave-1 fixes --- whether a tab title says nothing of its own: empty, too short, the address
/// (with or without its scheme, `www.` or a trailing slash), the host, or a placeholder such as
/// "New tab" or "Loading…" that a page without a title of its own shows.
fn placeholder_title(title: &str, url: &str) -> bool {
    let title = title.trim();
    if title.chars().count() < 3 || title.starts_with("http") {
        return true;
    }
    let lower = title.to_lowercase();
    if ["new tab", "untitled", "loading", "loading…", "loading...", "about:blank"].contains(&lower.as_str()) {
        return true;
    }
    let bare = |text: &str| {
        let text = text.trim().trim_end_matches('/').to_lowercase();
        let text = text.strip_prefix("https://").or_else(|| text.strip_prefix("http://")).map(str::to_owned).unwrap_or(text);
        text.strip_prefix("www.").map(str::to_owned).unwrap_or(text)
    };
    let (title, address) = (bare(title), bare(url));
    let host = address.split(['/', '?', '#']).next().unwrap_or_default().to_owned();
    title == address || title == host
}

/// The title a bookmark of `url` is saved with: the name the user gave the tab, when they gave one;
/// else the tab's own title unless it is only the address or a placeholder; else the title the page
/// declares (`og:title`); else the tab's title as it is.
pub(crate) fn bookmark_title(user_name: Option<&str>, tab_title: &str, meta_title: &str, url: &str) -> String {
    if let Some(name) = user_name.map(str::trim).filter(|name| !name.is_empty()) {
        return name.to_owned();
    }
    let (tab, meta) = (tab_title.trim(), meta_title.trim());
    if meta.is_empty() || !placeholder_title(tab, url) {
        return tab.to_owned();
    }
    meta.to_owned()
}

/// Reads the title and description the page declares (`og:title`, `description`), once, with the
/// page's own script world. `done` always runs, with `None` when the page cannot be read.
pub(crate) fn fetch_metadata(app: &AppRef, tab: TabId, done: MetadataDone) {
    const SCRIPT: &str = "(()=>{const m=n=>{const e=document.querySelector(n);return e?String(e.getAttribute('content')||'').trim():''};return JSON.stringify({t:(m('meta[property=\"og:title\"]')||m('meta[name=\"twitter:title\"]')||'').slice(0,200),d:(m('meta[name=\"description\"]')||m('meta[property=\"og:description\"]')).slice(0,400)})})()";
    let done = Rc::new(RefCell::new(Some(done)));
    let early = Rc::new(RefCell::new(None::<Option<(String, String)>>));
    let in_call = Rc::new(Cell::new(true));
    let answer = {
        let (done, early, in_call) = (done.clone(), early.clone(), in_call.clone());
        Box::new(move |text: Option<String>| {
            let meta = text.and_then(|raw| parse_metadata(&raw));
            if in_call.get() {
                *early.borrow_mut() = Some(meta);
            } else if let Some(done) = done.borrow_mut().take() {
                done(meta);
            }
        }) as Box<dyn FnOnce(Option<String>)>
    };
    let called = App::call_engine(app, |engine| engine.run_translate_script(tab, SCRIPT, answer));
    in_call.set(false);
    let result = match (called, early.borrow_mut().take()) {
        (None, _) => Some(None),
        (Some(()), early) => early,
    };
    if let Some(result) = result {
        if let Some(done) = done.borrow_mut().take() {
            done(result);
        }
    }
}

/// `ExecuteScript` answers a JSON string that holds JSON: the two-field object, read strictly.
pub(crate) fn parse_metadata(raw: &str) -> Option<(String, String)> {
    let outer = crate::translate::json::parse(raw)?;
    let inner = crate::translate::json::parse(outer.as_str()?)?;
    let clean = |text: &str| text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_owned();
    let title = clean(inner.get("t")?.as_str()?);
    let description = clean(inner.get("d")?.as_str()?);
    (!(title.is_empty() && description.is_empty())).then_some((title, description))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_metadata_is_read_from_the_double_encoded_answer() {
        let raw = r#""{\"t\":\"Rust\",\"d\":\"A language\\nempowering everyone\"}""#;
        assert_eq!(parse_metadata(raw), Some(("Rust".to_owned(), "A language empowering everyone".to_owned())));
        assert_eq!(parse_metadata(r#""{\"t\":\"\",\"d\":\"\"}""#), None, "nothing declared");
        for bad in ["null", "{}", "\"x\"", r#""{\"t\":1,\"d\":2}""#, ""] {
            assert_eq!(parse_metadata(bad), None, "{bad}");
        }
    }

    #[test]
    fn ctrl_d_opens_the_dialog_reads_the_page_or_saves_as_the_options_say() {
        let mut address = crate::address_settings::AddressSettings { always_dialog: true, fetch_metadata: true, ..Default::default() };
        assert_eq!(quick_add_plan(&address, true, false), QuickAdd::Dialog, "always the dialog wins");
        address.always_dialog = false;
        assert_eq!(quick_add_plan(&address, true, false), QuickAdd::ReadPageFirst);
        assert_eq!(quick_add_plan(&address, true, true), QuickAdd::SaveNow, "already bookmarked: nothing to fetch");
        assert_eq!(quick_add_plan(&address, false, false), QuickAdd::SaveNow, "no page to read");
        address.fetch_metadata = false;
        assert_eq!(quick_add_plan(&address, true, false), QuickAdd::SaveNow);
    }

    #[test]    fn the_page_s_own_title_replaces_a_missing_or_address_like_tab_title() {
        // --- wave-1 fixes --- the address without its scheme, the host and placeholders count as no title;
        // a name the user typed for the tab always wins.
        let url = "https://www.example.com/docs/";
        assert_eq!(bookmark_title(None, "example.com/docs", "Docs", url), "Docs");
        assert_eq!(bookmark_title(None, "www.example.com", "Docs", url), "Docs");
        assert_eq!(bookmark_title(None, "New tab", "Docs", url), "Docs");
        assert_eq!(bookmark_title(None, "Example Docs", "Docs", url), "Example Docs", "a real tab title stays");
        assert_eq!(bookmark_title(Some("My docs"), "example.com/docs", "Docs", url), "My docs");
        assert_eq!(bookmark_title(Some("  "), "example.com/docs", "Docs", url), "Docs", "an empty name is no name");
        assert_eq!(bookmark_title(None, "example.com/docs", "", url), "example.com/docs", "nothing better: as it is");
        assert_eq!(bookmark_title(None, "https://x.example/a", "Real title", ""), "Real title");
        assert_eq!(bookmark_title(None, "", "Real title", ""), "Real title");
        assert_eq!(bookmark_title(None, "Tab title", "Other", ""), "Tab title");
        assert_eq!(bookmark_title(None, "Tab title", "", ""), "Tab title");
        assert_eq!(bookmark_title(None, "", "", ""), "");
    }

    #[test]
    fn folders_list_in_tree_order_with_depth() {
        let mut store = BookmarkStore::new();
        let reading = store.add_folder(None, "Reading").unwrap();
        let rust = store.add_folder(Some(reading), "Rust").unwrap();
        store.add_folder(None, "Work").unwrap();
        store.add_bookmark(Some(rust), "Book", "https://doc.rust-lang.org/book/").unwrap();
        let folders = folders_of(&store);
        assert_eq!(folders.iter().map(|f| (f.title.as_str(), f.depth)).collect::<Vec<_>>(), [("Reading", 0), ("Rust", 1), ("Work", 0)]);
        assert_eq!(parent_of(0), None);
        assert_eq!(parent_of(5), Some(5));
    }
}
