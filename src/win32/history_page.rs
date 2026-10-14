//! The history page (`bowser://history`, docs/design/history-page.md): one trusted HTML host
//! (`View::HistoryPage`) per window, in a Bowser-owned child window over the page area of the
//! shown history tab. The tab has no content WebView2; this host is the page.
//!
//! Same shape and lifetime as the settings page (`settings_page.rs`): created on first use, hidden
//! and suspended while no history tab is shown, closed with the window. Commands are queued from
//! the WebView2 callback and run from the window procedure (`WM_HISTORY_PAGE_COMMAND`), and only
//! while a history tab is shown. The wire and the field checks are in `chrome_host::history_page`;
//! reading, searching and deleting are the rail panel's (`productivity`, `panel_history.rs`).
//!
//! Every function runs on the UI thread. No `RefCell` borrow of the runtime table is held while
//! calling into `App`, WebView2 or window functions, which may re-enter this module.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, IsWindow, RegisterClassW, SetWindowPos,
            ShowWindow, HWND_TOP, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WNDCLASSW, WS_CHILD,
            WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
        },
    },
    core::{w, PCWSTR},
};

use crate::{
    app::{App, AppRef},
    chrome_host::{
        self,
        history_page::{self as wire, Ack, Action, State},
        host::{Accel, AccelHandler, ChromeHost, View},
        protocol::{Command, PanelFields, PanelType},
    },
    internal_page::{InternalPage, Section},
    productivity::{self, HistoryEntry},
    split_view::Pane,
    tabs::TabId,
};

use super::{HistoryAction, HistoryModel, HISTORY_LIMIT};

const CLASS: PCWSTR = w!("BowserHistoryPage");

/// Where the page is shown: over history tab `tab`, at `rect` (client coordinates).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shown {
    tab: TabId,
    rect: RECT,
}

#[derive(Default)]
struct Runtime {
    container: Option<HWND>,
    host: Option<ChromeHost>,
    requested: bool,
    ready: bool,
    failed: bool,
    suspended: bool,
    shown: Option<Shown>,
    /// Last theme state the page accepted.
    state: Option<String>,
    /// Last snapshot sent, and whether a fresh one is due (history is read from disk only then).
    snapshot: Option<String>,
    dirty: bool,
    focus_pending: bool,
    /// The search the page last asked for (shared by every history tab in this window).
    query: String,
    ack: Ack,
    /// The last removed entry, which Undo puts back.
    removed: Option<HistoryEntry>,
}

thread_local! {
    static RUNTIMES: RefCell<HashMap<isize, Runtime>> = RefCell::new(HashMap::new());
    static QUEUE: RefCell<Vec<(isize, PanelFields)>> = const { RefCell::new(Vec::new()) };
}

fn with<T>(owner: HWND, f: impl FnOnce(&mut Runtime) -> T) -> Option<T> {
    RUNTIMES.with(|runtimes| {
        let mut runtimes = runtimes.try_borrow_mut().ok()?;
        Some(f(runtimes.entry(owner.0 as isize).or_default()))
    })
}

fn rect_size(rect: RECT) -> RECT {
    RECT { left: 0, top: 0, right: (rect.right - rect.left).max(1), bottom: (rect.bottom - rect.top).max(1) }
}

/// Whether this window can draw the history page: its HTML chrome is on, the chrome environment
/// exists and the page's host has not failed. Otherwise History is the side panel, as before (the
/// rail button and Ctrl+H then toggle it, as they always did).
pub(crate) fn available(owner: HWND) -> bool {
    chrome_host::html_ui_enabled()
        && super::window::html_environment(owner).is_some()
        && !with(owner, |rt| rt.failed).unwrap_or(false)
}

/// Opens the history page (rail button, Ctrl+H, main menu, omnibox): an existing history tab in
/// this window is selected, never duplicated. `reuse_blank`: a blank normal shown tab becomes the
/// page (the omnibox) instead of a new tab opening (the menu, the rail, Ctrl+H). Without the HTML
/// chrome it opens the History side panel instead.
pub(crate) fn open(owner: HWND, app: &AppRef, reuse_blank: bool) {
    if !available(owner) {
        super::window::open_history_panel(owner, app);
        return;
    }
    let _ = App::open_internal_page(app, InternalPage::History, reuse_blank);
    let _ = with(owner, |rt| {
        rt.focus_pending = true;
        rt.dirty = true;
    });
    super::window::split_ui_invalidate(owner);
}

/// F5 on a history tab: read history again.
pub(crate) fn refresh(owner: HWND) {
    let _ = with(owner, |rt| rt.dirty = true);
    super::window::split_ui_invalidate(owner);
}

/// A visit was just recorded (or history changed elsewhere in this window): a shown page refreshes
/// its list. Only a changed snapshot is posted, so focus, scroll and the search are left alone.
pub(crate) fn history_changed(owner: HWND) {
    let shown = with(owner, |rt| {
        rt.dirty = true;
        rt.shown.is_some()
    })
    .unwrap_or(false);
    if shown {
        super::window::split_ui_invalidate(owner);
    }
}

/// Gives the keyboard to the page (after the omnibox submits or escapes on a history tab).
pub(crate) fn focus(owner: HWND) {
    let host = with(owner, |rt| (rt.ready && !rt.failed && rt.shown.is_some()).then(|| rt.host.clone()).flatten()).flatten();
    match host {
        Some(host) => {
            let _ = host.focus();
        }
        None => {
            let _ = with(owner, |rt| rt.focus_pending = true);
        }
    }
}

/// Whether the page covers the viewport (or a pane) right now: the overlay then frames it like a
/// page instead of painting the blank-tab surface over it.
pub(crate) fn shown(owner: HWND) -> bool {
    with(owner, |rt| rt.ready && !rt.failed && rt.shown.is_some()).unwrap_or(false)
}

/// The split pane the page covers, if it is shown inside a split.
pub(crate) fn pane(owner: HWND, app: &AppRef) -> Option<Pane> {
    let tab = with(owner, |rt| rt.ready.then_some(rt.shown).flatten().map(|shown| shown.tab)).flatten()?;
    App::split_view(app)?.pane_for(tab)
}

// ----- the host and its container window -----

fn register_class() -> bool {
    thread_local! { static REGISTERED: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) }; }
    if let Some(done) = REGISTERED.with(|slot| slot.get()) {
        return done;
    }
    let instance = unsafe { GetModuleHandleW(None) }.map(|module| module.into()).unwrap_or_default();
    let class = WNDCLASSW { lpfnWndProc: Some(container_proc), hInstance: instance, lpszClassName: CLASS, ..Default::default() };
    let ok = unsafe { RegisterClassW(&class) } != 0;
    REGISTERED.with(|slot| slot.set(Some(ok)));
    ok
}

unsafe extern "system" fn container_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// The host could not be created or its page failed: the window falls back to the History side
/// panel, and an open history tab is left showing the blank-tab surface.
fn fail(owner: HWND) {
    let _ = with(owner, |rt| rt.failed = true);
    super::window::split_ui_invalidate(owner);
}

/// Creates the container (visible at one pixel, see host.rs) and starts the controller, once.
fn ensure_host(owner: HWND) {
    let start = with(owner, |rt| {
        if rt.requested || rt.failed {
            return false;
        }
        rt.requested = true;
        true
    })
    .unwrap_or(false);
    if !start {
        return;
    }
    let Some(env) = super::window::html_environment(owner) else {
        let _ = with(owner, |rt| rt.requested = false);
        return;
    };
    if !register_class() {
        fail(owner);
        return;
    }
    let instance = unsafe { GetModuleHandleW(None) }.map(|module| module.into()).ok();
    let container = unsafe {
        CreateWindowExW(Default::default(), CLASS, w!(""), WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS, 0, 0, 1, 1, Some(owner), None, instance, None)
    };
    let Ok(container) = container else {
        fail(owner);
        return;
    };
    unsafe {
        let _ = SetWindowPos(container, Some(HWND_TOP), 0, 0, 1, 1, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
    let _ = with(owner, |rt| rt.container = Some(container));
    let raw = owner.0 as isize;
    let on_command: Rc<dyn Fn(Command)> = Rc::new(move |command| {
        // Never inside WebView2's callback: commands open tabs and the privacy dashboard.
        let owner = HWND(raw as *mut _);
        match command {
            Command::Ready => {
                let _ = with(owner, |rt| {
                    rt.ready = true;
                    rt.state = None;
                    rt.snapshot = None;
                    rt.dirty = true;
                    // A reloaded page starts without a search: Bowser's list follows it.
                    rt.query.clear();
                });
                super::window::split_ui_invalidate(owner);
            }
            Command::Panel { kind: PanelType::HistoryPage, f } => queue(owner, f),
            _ => {}
        }
    });
    let on_failed: Rc<dyn Fn()> = Rc::new(move || fail(HWND(raw as *mut _)));
    let on_accelerator: AccelHandler = Rc::new(move |accel: Accel| super::window::html_history_page_accelerator(HWND(raw as *mut _), accel));
    let created = ChromeHost::create(&env, container, RECT { left: 0, top: 0, right: 1, bottom: 1 }, View::HistoryPage, on_command, on_failed, Some(on_accelerator), move |result| {
        let owner = HWND(raw as *mut _);
        match result {
            Ok(host) => {
                let keep = with(owner, |rt| {
                    if rt.failed {
                        return false;
                    }
                    rt.host = Some(host.clone());
                    true
                })
                .unwrap_or(false);
                if !keep {
                    host.close();
                }
                super::window::split_ui_invalidate(owner);
            }
            Err(_) => fail(owner),
        }
    });
    if created.is_err() {
        fail(owner);
    }
}

/// Queues a page command and posts `WM_HISTORY_PAGE_COMMAND` so it runs from the window procedure.
fn queue(owner: HWND, fields: PanelFields) {
    let full = QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        if queue.len() >= 16 {
            return true;
        }
        queue.push((owner.0 as isize, fields));
        false
    });
    if !full {
        crate::win32::post(owner, super::WM_HISTORY_PAGE_COMMAND, WPARAM(0), LPARAM(0));
    }
}

/// Closes the host (window closing or chrome failure).
pub(crate) fn close(owner: HWND) {
    let removed = RUNTIMES.with(|runtimes| runtimes.try_borrow_mut().ok().and_then(|mut all| all.remove(&(owner.0 as isize))));
    if let Some(runtime) = removed {
        if let Some(host) = runtime.host {
            host.close();
        }
        if let Some(container) = runtime.container {
            unsafe {
                if IsWindow(Some(container)).as_bool() {
                    let _ = DestroyWindow(container);
                }
            }
        }
    }
}

fn is_history_tab(app: &AppRef, tab: TabId) -> bool {
    App::internal_page(app, tab).is_some_and(InternalPage::is_history)
}

/// Where the page should be now: over the shown history tab, or over a history tab's pane in a
/// shown split (the focused one when both are).
fn wanted(owner: HWND, app: &AppRef) -> Option<Shown> {
    let active = App::active_tab_id(app);
    if let Some(split) = App::split_view(app) {
        let panes = [split.focused, split.focused.other()];
        let pane = panes.into_iter().find(|pane| is_history_tab(app, split.tab(*pane)))?;
        let tab = split.tab(pane);
        let rect = match App::split_bounds(app) {
            Some((left, right)) => if pane == Pane::Left { left } else { right },
            None if split.focused == pane => super::window::split_viewport(owner)?,
            None => return None,
        };
        return Some(Shown { tab, rect });
    }
    if !is_history_tab(app, active) {
        return None;
    }
    Some(Shown { tab: active, rect: super::window::split_viewport(owner)? })
}

/// Brings the host in line with the shown tab. Cheap when nothing changed; called from every
/// chrome paint.
pub(crate) fn sync(owner: HWND, app: &AppRef) {
    if !chrome_host::html_ui_enabled() {
        return;
    }
    let want = wanted(owner, app);
    if want.is_some() {
        ensure_host(owner);
    }
    let Some((host, container, ready, shown, suspended)) = with(owner, |rt| (rt.host.clone(), rt.container, rt.ready && !rt.failed, rt.shown, rt.suspended)) else {
        return;
    };
    let (Some(host), Some(container), true) = (host, container, ready) else {
        return;
    };
    let Some(want) = want else {
        if shown.is_some() {
            let _ = host.set_visible(false);
            unsafe {
                let _ = ShowWindow(container, SW_HIDE);
            }
            let suspend = with(owner, |rt| {
                rt.shown = None;
                !std::mem::replace(&mut rt.suspended, true)
            })
            .unwrap_or(false);
            if suspend && host.try_suspend().is_err() {
                let _ = with(owner, |rt| rt.suspended = false);
            }
        }
        return;
    };
    if suspended {
        if host.resume().is_err() {
            return;
        }
        let _ = with(owner, |rt| rt.suspended = false);
    }
    // Coming back to the page (or another history tab): read history again, it may have changed.
    if shown.map(|shown| shown.tab) != Some(want.tab) {
        let _ = with(owner, |rt| rt.dirty = true);
    }
    // Theme first, then content, then show: the page never appears unthemed or stale.
    let state = super::window::split_host_state(owner, App::settings_accent(app));
    if with(owner, |rt| rt.state.as_deref() != Some(state.as_str())).unwrap_or(false) && host.post_json(&state).is_ok() {
        let _ = with(owner, |rt| rt.state = Some(state));
    }
    if with(owner, |rt| std::mem::take(&mut rt.dirty)).unwrap_or(false) {
        let snapshot = snapshot(owner);
        // An answer is always delivered, even when the list did not change.
        let fresh = with(owner, |rt| rt.ack != Ack::None || rt.snapshot.as_deref() != Some(snapshot.as_str())).unwrap_or(false);
        if fresh && host.post_json(&snapshot).is_ok() {
            let _ = with(owner, |rt| {
                rt.snapshot = Some(snapshot);
                rt.ack = Ack::None;
            });
        }
    }
    if shown != Some(want) {
        let size = rect_size(want.rect);
        unsafe {
            let _ = SetWindowPos(container, Some(HWND_TOP), want.rect.left, want.rect.top, size.right, size.bottom, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        let _ = host.set_bounds(size);
        let _ = host.set_visible(true);
        let _ = with(owner, |rt| rt.shown = Some(want));
    }
    // The keyboard goes to the page when it was opened or a history tab was selected, unless the
    // user is typing in the omnibox.
    let asked = with(owner, |rt| std::mem::take(&mut rt.focus_pending)).unwrap_or(false);
    let appeared = shown.is_none() && !super::window::omnibox_focused(owner);
    if (asked || appeared) && want.tab == App::active_tab_id(app) {
        let _ = host.focus();
    }
}

/// What the profile records, and the entries for the page's search (when it records). Private
/// tabs never write history (window.rs `may_persist`), so there is nothing private to show.
fn read(query: &str) -> (State, Vec<HistoryEntry>) {
    let Ok(profile) = crate::profile::active() else { return (State::Error, Vec::new()) };
    if profile.is_guest() || !productivity::profile_allows_productivity(&profile) {
        return (State::Guest, Vec::new());
    }
    if !profile.history_enabled() {
        return (State::Off, Vec::new());
    }
    match productivity::read_history_strict(&profile.history_file()) {
        Ok(entries) => (State::On, productivity::search_entries(entries, query, HISTORY_LIMIT)),
        Err(_) => (State::Error, Vec::new()),
    }
}

/// The snapshot for `owner`'s page: its own search, the undo state and the last answer.
fn snapshot(owner: HWND) -> String {
    let (query, undo, ack) = with(owner, |rt| (rt.query.clone(), rt.removed.is_some(), rt.ack)).unwrap_or_default();
    let (state, entries) = read(&query);
    wire::snapshot_json(&wire::Snapshot {
        state,
        query: &query,
        entries: &entries,
        now: super::history_now_secs(),
        offset_secs: super::history_local_offset(),
        undo: undo && state == State::On,
        ack,
    })
}

/// The live entries the page shows for the current search, for resolving an id.
fn live_model(query: &str) -> HistoryModel {
    let (_, entries) = read(query);
    let mut model = HistoryModel::default();
    model.set_query(query);
    model.rebuild(&entries, super::history_now_secs(), super::history_local_offset());
    model
}

/// Runs the queued commands for `owner` (from `WM_HISTORY_PAGE_COMMAND`). A command acts only while
/// the page is shown over a history tab; anything else is dropped.
pub(crate) fn run_queued(owner: HWND, app: &AppRef) {
    let commands: Vec<PanelFields> = QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        let (mine, rest): (Vec<_>, Vec<_>) = queue.drain(..).partition(|(id, _)| *id == owner.0 as isize);
        *queue = rest;
        mine.into_iter().map(|(_, fields)| fields).collect()
    });
    for fields in commands {
        let Some(tab) = with(owner, |rt| rt.shown.map(|shown| shown.tab)).flatten() else { continue };
        if !is_history_tab(app, tab) {
            continue;
        }
        if let Some(action) = wire::map_command(&fields) {
            run(owner, app, tab, action);
        }
    }
    super::window::split_ui_invalidate(owner);
}

/// The page acted on an entry that is gone (removed in another window, cleared): it gets the
/// current list.
fn stale(owner: HWND) {
    let _ = with(owner, |rt| rt.dirty = true);
}

fn run(owner: HWND, app: &AppRef, tab: TabId, action: Action) {
    match action {
        Action::Search(text) => {
            let _ = with(owner, |rt| {
                rt.query = text;
                rt.dirty = true;
            });
        }
        Action::Open { id, new_tab } => {
            let query = with(owner, |rt| rt.query.clone()).unwrap_or_default();
            let model = live_model(&query);
            let Some(line) = wire::resolve(&model.lines, id) else { return stale(owner) };
            let url = line.url.clone();
            // In a split the page may be the other pane: it becomes the focused one first, so the
            // address opens in the history tab (or the new tab opens from it).
            if tab != App::active_tab_id(app) {
                let _ = App::focus_split_tab(app, tab);
            }
            let _ = super::execute_history_action(app, owner, HistoryAction::Open { url, new_tab });
        }
        Action::Remove(id) => {
            let query = with(owner, |rt| rt.query.clone()).unwrap_or_default();
            let model = live_model(&query);
            let Some(line) = wire::resolve(&model.lines, id) else { return stale(owner) };
            let entry = (line.timestamp, line.url.clone(), line.title.clone());
            let Some(path) = super::current_history_file() else { return };
            let result = productivity::delete_history_entry(&path, &entry.1);
            let _ = with(owner, |rt| {
                rt.dirty = true;
                match result {
                    Ok(true) => {
                        rt.ack = Ack::Removed;
                        rt.removed = Some(entry);
                    }
                    Ok(false) => {}
                    Err(_) => rt.ack = Ack::Failed,
                }
            });
            // The rail flyout, if open in this window, shows the change too.
            super::html_panel::history_changed(owner);
        }
        Action::Undo => {
            let Some(entry) = with(owner, |rt| rt.removed.take()).flatten() else { return };
            let Some(path) = super::current_history_file() else { return };
            // Only when the address is not back already (visited again since): a newer visit wins.
            let present = productivity::read_history_strict(&path).map(|all| all.iter().any(|(_, url, _)| *url == entry.1));
            let result = match present {
                Ok(true) => Ok(()),
                Ok(false) => productivity::add_history_entry_at(&path, entry.0, &entry.1, &entry.2),
                Err(error) => Err(error),
            };
            let _ = with(owner, |rt| {
                rt.dirty = true;
                rt.ack = if result.is_ok() { Ack::Restored } else { Ack::Failed };
            });
            super::html_panel::history_changed(owner);
        }
        Action::Privacy => super::privacy_panel::open(owner, app),
        Action::Settings => super::settings_page::open(owner, app, Some(Section::Privacy), false),
        Action::Refresh => {
            let _ = with(owner, |rt| rt.dirty = true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_rect_is_never_empty() {
        assert_eq!(rect_size(RECT { left: 10, top: 20, right: 10, bottom: 15 }), RECT { left: 0, top: 0, right: 1, bottom: 1 });
        assert_eq!(rect_size(RECT { left: 10, top: 20, right: 410, bottom: 320 }), RECT { left: 0, top: 0, right: 400, bottom: 300 });
    }
}
