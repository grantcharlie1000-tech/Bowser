//! The downloads page (`bowser://downloads`, docs/design/downloads.md): one trusted HTML host
//! (`View::DownloadsPage`) per window, in a Bowser-owned child window over the page area of the
//! shown downloads tab. The tab has no content WebView2; this host is the page.
//!
//! Same shape and lifetime as the history page (`history_page.rs`): created on first use, hidden
//! and suspended while no downloads tab is shown, closed with the window. Commands are queued from
//! the WebView2 callback and run from the window procedure (`WM_DOWNLOADS_PAGE_COMMAND`), and only
//! while a downloads tab is shown. The list, its snapshot and every action's legality live in
//! `crate::downloads` (reached through `App`); the wire's command side is
//! `chrome_host::downloads_page`. The page names entries by id only: Rust acts on the recorded path
//! and URL, never on anything the page sends.
//!
//! Live progress: `downloads_ui::changed` (the list's observer) calls [`downloads_changed`], which
//! marks the page dirty; the next sync posts a snapshot, only when it differs.
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
    app::{App, AppRef, DownloadFollowup},
    chrome_host::{
        self,
        downloads_page::{self as wire, Action},
        host::{Accel, AccelHandler, ChromeHost, View},
        protocol::{Command, PanelFields, PanelType},
    },
    downloads::{Ack, Action as Row},
    internal_page::{InternalPage, Section},
    split_view::Pane,
    tabs::TabId,
};

const CLASS: PCWSTR = w!("BowserDownloadsPage");

/// Where the page is shown: over downloads tab `tab`, at `rect` (client coordinates).
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
    /// Last snapshot sent, and whether a fresh one is due.
    snapshot: Option<String>,
    dirty: bool,
    focus_pending: bool,
    /// The search the page last asked for (shared by every downloads tab in this window).
    query: String,
    /// The answer to the last change, sent once with the snapshot that follows it.
    ack: Ack,
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

/// Whether this window can draw the downloads page: its HTML chrome is on, the chrome environment
/// exists and the page's host has not failed. Otherwise Ctrl+J opens the download folder instead.
pub(crate) fn available(owner: HWND) -> bool {
    chrome_host::html_ui_enabled()
        && super::window::html_environment(owner).is_some()
        && !with(owner, |rt| rt.failed).unwrap_or(false)
}

/// Opens the downloads page (Ctrl+J, main menu, the bubble's footer, the omnibox): an existing
/// downloads tab in this window is selected, never duplicated. `reuse_blank`: a blank normal shown
/// tab becomes the page (the omnibox) instead of a new tab opening. A downloads tab is never
/// private: from a blank private tab the page opens in a new normal tab. Without the HTML chrome
/// there is no page; the download folder opens in Explorer instead.
pub(crate) fn open(owner: HWND, app: &AppRef, reuse_blank: bool) {
    if !available(owner) {
        let _ = App::download_act(app, 0, Row::OpenFolder);
        return;
    }
    let _ = App::open_internal_page(app, InternalPage::Downloads, reuse_blank);
    let _ = with(owner, |rt| {
        rt.focus_pending = true;
        rt.dirty = true;
    });
    super::window::split_ui_invalidate(owner);
}

/// F5 on a downloads tab: read the list again.
pub(crate) fn refresh(owner: HWND) {
    let _ = with(owner, |rt| rt.dirty = true);
    super::window::split_ui_invalidate(owner);
}

/// The list changed (progress, a state change, a removal): a shown page refreshes. Only a changed
/// snapshot is posted, so focus, scroll and the search are left alone.
pub(crate) fn downloads_changed(owner: HWND) {
    let shown = with(owner, |rt| {
        rt.dirty = true;
        rt.shown.is_some()
    })
    .unwrap_or(false);
    if shown {
        super::window::split_ui_invalidate(owner);
    }
}

/// Gives the keyboard to the page (after the omnibox submits or escapes on a downloads tab).
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

/// The host could not be created or its page failed: an open downloads tab is left showing the
/// blank-tab surface, and Ctrl+J opens the download folder instead.
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
        // Never inside WebView2's callback: commands open files, tabs and Explorer.
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
            Command::Panel { kind: PanelType::DownloadsPage, f } => queue(owner, f),
            _ => {}
        }
    });
    let on_failed: Rc<dyn Fn()> = Rc::new(move || fail(HWND(raw as *mut _)));
    let on_accelerator: AccelHandler = Rc::new(move |accel: Accel| super::window::html_downloads_page_accelerator(HWND(raw as *mut _), accel));
    let created = ChromeHost::create(&env, container, RECT { left: 0, top: 0, right: 1, bottom: 1 }, View::DownloadsPage, on_command, on_failed, Some(on_accelerator), move |result| {
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

/// Queues a page command and posts `WM_DOWNLOADS_PAGE_COMMAND` so it runs from the window procedure.
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
        crate::win32::post(owner, super::WM_DOWNLOADS_PAGE_COMMAND, WPARAM(0), LPARAM(0));
    }
}

/// Closes the host (window closing or idle).
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

fn is_downloads_tab(app: &AppRef, tab: TabId) -> bool {
    App::internal_page(app, tab).is_some_and(InternalPage::is_downloads)
}

/// Where the page should be now: over the shown downloads tab, or over a downloads tab's pane in a
/// shown split (the focused one when both are).
fn wanted(owner: HWND, app: &AppRef) -> Option<Shown> {
    let active = App::active_tab_id(app);
    if let Some(split) = App::split_view(app) {
        let panes = [split.focused, split.focused.other()];
        let pane = panes.into_iter().find(|pane| is_downloads_tab(app, split.tab(*pane)))?;
        let tab = split.tab(pane);
        let rect = match App::split_bounds(app) {
            Some((left, right)) => if pane == Pane::Left { left } else { right },
            None if split.focused == pane => super::window::split_viewport(owner)?,
            None => return None,
        };
        return Some(Shown { tab, rect });
    }
    if !is_downloads_tab(app, active) {
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
    // Coming back to the page (or another downloads tab): read the list again, files may have moved.
    if shown.map(|shown| shown.tab) != Some(want.tab) {
        let _ = with(owner, |rt| rt.dirty = true);
    }
    // Theme first, then content, then show: the page never appears unthemed or stale.
    let state = super::window::split_host_state(owner, App::settings_accent(app));
    if with(owner, |rt| rt.state.as_deref() != Some(state.as_str())).unwrap_or(false) && host.post_json(&state).is_ok() {
        let _ = with(owner, |rt| rt.state = Some(state));
    }
    if with(owner, |rt| std::mem::take(&mut rt.dirty)).unwrap_or(false) {
        let snapshot = snapshot(owner, app);
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
    // The keyboard goes to the page when it was opened or a downloads tab was selected, unless the
    // user is typing in the omnibox.
    let asked = with(owner, |rt| std::mem::take(&mut rt.focus_pending)).unwrap_or(false);
    let appeared = shown.is_none() && !super::window::omnibox_focused(owner);
    if (asked || appeared) && want.tab == App::active_tab_id(app) {
        let _ = host.focus();
    }
}

/// The snapshot for `owner`'s page: its own search and the last answer. The list re-checks that
/// finished files still exist (at most every two seconds), so a deleted file reads `missing`.
fn snapshot(owner: HWND, app: &AppRef) -> String {
    let (query, ack) = with(owner, |rt| (rt.query.clone(), rt.ack)).unwrap_or_default();
    App::downloads_page_json(app, &query, ack)
}

/// Runs the queued commands for `owner` (from `WM_DOWNLOADS_PAGE_COMMAND`). A command acts only
/// while the page is shown over a downloads tab; anything else is dropped.
pub(crate) fn run_queued(owner: HWND, app: &AppRef) {
    let commands: Vec<PanelFields> = QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        let (mine, rest): (Vec<_>, Vec<_>) = queue.drain(..).partition(|(id, _)| *id == owner.0 as isize);
        *queue = rest;
        mine.into_iter().map(|(_, fields)| fields).collect()
    });
    for fields in commands {
        let Some(tab) = with(owner, |rt| rt.shown.map(|shown| shown.tab)).flatten() else { continue };
        if !is_downloads_tab(app, tab) {
            continue;
        }
        if let Some(action) = wire::map_command(&fields) {
            run(owner, app, tab, action);
        }
    }
    super::window::split_ui_invalidate(owner);
}

fn answer(owner: HWND, ack: Ack) {
    let _ = with(owner, |rt| {
        rt.ack = ack;
        rt.dirty = true;
    });
}

fn run(owner: HWND, app: &AppRef, tab: TabId, action: Action) {
    match action {
        Action::Search(text) => {
            let _ = with(owner, |rt| {
                rt.query = text;
                rt.dirty = true;
            });
        }
        Action::Row { id, action } => {
            // In a split the page may be the other pane: it becomes the focused one first, so a
            // Retry opens its tab from the downloads tab.
            if action == Row::Retry && tab != App::active_tab_id(app) {
                let _ = App::focus_split_tab(app, tab);
            }
            let followup = App::download_act(app, id, action);
            match followup {
                // An entry that is gone (removed in another window, cleared) or an action that is
                // no longer legal: the page gets the current list.
                DownloadFollowup::Rejected => return answer(owner, Ack::None),
                DownloadFollowup::Failed => return answer(owner, Ack::Failed),
                other => {
                    if !super::downloads_ui::follow_up(owner, app, other) {
                        return answer(owner, Ack::Failed);
                    }
                }
            }
            answer(owner, if action == Row::Remove { Ack::Removed } else { Ack::None });
        }
        Action::List(action) => {
            let followup = App::download_act(app, 0, action);
            let ack = match followup {
                DownloadFollowup::Failed => Ack::Failed,
                DownloadFollowup::Rejected => Ack::None,
                _ if action == Row::ClearAll => Ack::Cleared,
                _ => Ack::None,
            };
            answer(owner, ack);
        }
        Action::Undo => {
            let restored = App::downloads_undo(app);
            answer(owner, if restored { Ack::Restored } else { Ack::Failed });
        }
        Action::Refresh => {
            let _ = with(owner, |rt| rt.dirty = true);
        }
        Action::Settings => super::settings_page::open(owner, app, Some(Section::Downloads), false),
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
