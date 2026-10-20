//! Rail flyouts (History, Bookmarks, Customize) in the HTML chrome.
//!
//! The panel controller (`View::Panel`, `?view=panel`) lives in the flyout window (`flyout.rs`), a
//! popover anchored beside its rail button; nothing is docked and the page is never re-bounded
//! (docs/design/flyouts.md). Rust owns the stores: snapshots go to the page, the page's validated
//! `hist`/`bm`/`cust` commands come back and run here. Native mode, and a failed panel controller,
//! keep the docked native panels in `side_panel.rs`.

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::{Rc, Weak},
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
    Graphics::Gdi::ClientToScreen,
    UI::{
        Input::KeyboardAndMouse::{GetFocus, SetFocus},
        WindowsAndMessaging::{DestroyWindow, GetClientRect, GetForegroundWindow, IsWindow, SetForegroundWindow},
    },
};
use crate::{
    app::AppRef,
    bookmarks::BookmarkStore,
    chrome_host::{
        self,
        host::{Accel, AccelHandler, ChromeEnv, ChromeHost, View},
        protocol::{self, Command, PanelType},
    },
    productivity,
    profile,
    win32::{
        current_history_file, execute_history_action, flyout, history_local_offset, history_now_secs,
        HistoryAction as Action, HistoryModel as Model, HISTORY_LIMIT as LIMIT,
        side_panel::PanelId,
        show_error_message,
    },
};

/// A rail click on the flyout that a deactivation closed this recently is the toggle's second
/// click: the flyout stays closed. Longer than the popup host's 300 ms because rail clicks are
/// reported on mouse up.
const REOPEN_GUARD: Duration = Duration::from_millis(400);
/// A click on the button whose flyout a hover opened this recently confirms it instead of closing
/// it: the pointer reached the button, the flyout switched, and the click the user was already
/// making lands on it.
const HOVER_CLICK_GRACE: Duration = Duration::from_millis(700);

/// Active tab and workspace when a flyout opened. A change closes it (as for menus).
pub(crate) type Context = (u32, u32);

/// Where to put a flyout: the rail host's right edge and the button rectangle, screen pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Anchor {
    pub rail_right: i32,
    pub button: RECT,
}

struct Runtime {
    app: AppRef,
    owner: HWND,
    window: HWND,
    host: Option<ChromeHost>,
    ready: bool,
    suspended: bool,
    closed: bool,
    failed: bool,
    failed_posts: u8,
    query: String,
    bookmarks_query: String,
    panel: PanelId,
    /// A flyout is requested (it may still wait for the page to paint).
    open: bool,
    /// The window is on screen.
    shown: bool,
    placement: Option<flyout::Placement>,
    prior_focus: HWND,
    context: Option<Context>,
    closed_panel: Option<PanelId>,
    closed_at: Option<Instant>,
    /// Commands that may show a modal message box (confirmations) hold the flyout open: its own
    /// deactivation is ignored meanwhile, and it takes activation back afterwards.
    hold: u32,
    /// The flyout lost activation to a press on the rail; the rail command that follows decides.
    rail_press: bool,
    /// When a hover last switched the flyout (see `HOVER_CLICK_GRACE`).
    hovered_at: Option<Instant>,
}

thread_local! {
    static RUNTIMES: RefCell<HashMap<isize, Rc<RefCell<Runtime>>>> = RefCell::new(HashMap::new());
}

fn runtime(owner: HWND) -> Option<Rc<RefCell<Runtime>>> {
    RUNTIMES.with(|runtimes| runtimes.borrow().get(&(owner.0 as isize)).cloned())
}

/// Creates the hidden flyout window and its panel controller once the chrome environment is ready.
pub(crate) fn register(app: &AppRef, owner: HWND, env: ChromeEnv) {
    if runtime(owner).is_some() {
        return;
    }
    let Some(window) = flyout::create(owner) else {
        crate::win32::post(owner, super::WM_HTML_PANEL_FAILED, WPARAM(0), LPARAM(0));
        return;
    };
    let runtime = Rc::new(RefCell::new(Runtime {
        app: app.clone(), owner, window, host: None, ready: false, suspended: false, closed: false,
        failed: false, failed_posts: 0, query: String::new(), bookmarks_query: String::new(),
        panel: PanelId::History, open: false, shown: false, placement: None, prior_focus: HWND::default(),
        context: None, closed_panel: None, closed_at: None, hold: 0, rail_press: false, hovered_at: None,
    }));
    RUNTIMES.with(|runtimes| { runtimes.borrow_mut().insert(owner.0 as isize, runtime.clone()); });
    start(&runtime, &env);
}

fn start(runtime: &Rc<RefCell<Runtime>>, env: &ChromeEnv) {
    let (window, owner) = {
        let Ok(state) = runtime.try_borrow() else { return; };
        (state.window, state.owner)
    };
    let weak: Weak<RefCell<Runtime>> = Rc::downgrade(runtime);
    let commands: Rc<dyn Fn(Command)> = Rc::new(move |command| {
        if let Some(runtime) = weak.upgrade() { handle_command(&runtime, command); }
    });
    let failed: Rc<dyn Fn()> = Rc::new(move || crate::win32::post(owner, super::WM_HTML_PANEL_FAILED, WPARAM(0), LPARAM(0)));
    let accel: AccelHandler = Rc::new(move |accel: Accel| super::window::html_panel_accelerator(owner, accel));
    let ready_runtime = Rc::downgrade(runtime);
    let bounds = RECT { left: 0, top: 0, right: 360, bottom: 600 };
    if ChromeHost::create(env, crate::chrome_host::host::ChromeHostOptions { parent: window, bounds, view: View::Panel, on_command: commands, on_failed: failed, on_accelerator: Some(accel) }, move |result| match result {
        Ok(host) => {
            let Some(runtime) = ready_runtime.upgrade() else { host.close(); return; };
            let Ok(mut state) = runtime.try_borrow_mut() else { host.close(); return; };
            if state.closed { host.close(); return; }
            state.host = Some(host);
        }
        Err(_) => crate::win32::post(owner, super::WM_HTML_PANEL_FAILED, WPARAM(0), LPARAM(0)),
    }).is_err() {
        crate::win32::post(owner, super::WM_HTML_PANEL_FAILED, WPARAM(0), LPARAM(0));
    }
}

/// The panel controller failed: the flyout is gone for this window and the rail falls back to the
/// docked native panels.
pub(crate) fn fail(owner: HWND) {
    let runtime = RUNTIMES.with(|runtimes| runtimes.borrow_mut().remove(&(owner.0 as isize)));
    let Some(runtime) = runtime else { return; };
    let parts = runtime.try_borrow_mut().ok().map(|mut state| {
        state.closed = true;
        state.open = false;
        state.shown = false;
        (state.host.take(), state.window)
    });
    if let Some((host, window)) = parts {
        if let Some(host) = host { host.close(); }
        unsafe { let _ = DestroyWindow(window); }
    }
}

pub(crate) fn close(owner: HWND) { fail(owner); }

/// Whether this window shows rail panels as HTML flyouts (the runtime exists and has not failed).
pub(crate) fn available(owner: HWND) -> bool {
    runtime(owner).is_some_and(|runtime| runtime.try_borrow().is_ok_and(|state| !state.closed && !state.failed))
}

/// The flyout that is open (or opening), if any.
pub(crate) fn active_id(owner: HWND) -> Option<PanelId> {
    let runtime = runtime(owner)?;
    let state = runtime.try_borrow().ok()?;
    (state.open && !state.closed).then_some(state.panel)
}

/// Rail click or shortcut: opens `id`, closes it when it is the open one, or switches to it.
/// Returns false when flyouts are not available here (the caller uses the docked native panel).
pub(crate) fn toggle(owner: HWND, id: PanelId, anchor: Anchor, context: Context) -> bool {
    let Some(runtime) = runtime(owner) else { return false; };
    super::window::perf_event(owner, "html_flyout_toggle");
    let (open, panel, guarded, confirmed) = {
        let Ok(mut state) = runtime.try_borrow_mut() else { return true; };
        if state.closed || state.failed { return false; }
        let guarded = !state.open
            && state.closed_panel == Some(id)
            && state.closed_at.is_some_and(|at| at.elapsed() < REOPEN_GUARD);
        if guarded {
            state.closed_at = None;
        }
        state.rail_press = false;
        let confirmed = state.open && state.panel == id
            && std::mem::take(&mut state.hovered_at).is_some_and(|at| at.elapsed() < HOVER_CLICK_GRACE);
        (state.open, state.panel, guarded, confirmed)
    };
    if guarded {
        super::window::perf_event(owner, "html_flyout_guarded");
        return true;
    }
    if confirmed {
        // Same content and place: only take the activation back from the rail click.
        let parts = runtime.try_borrow().ok().map(|state| (state.window, state.host.clone()));
        if let Some((window, host)) = parts {
            unsafe { let _ = SetForegroundWindow(window); }
            if let Some(host) = host { let _ = host.focus(); }
        }
    } else if open && panel == id {
        dismiss_runtime(&runtime, Dismissal::RestoreFocus);
    } else {
        open_panel(&runtime, id, anchor, context);
    }
    true
}

/// The pointer rested on another flyout button while a flyout is open: switch to it.
pub(crate) fn hover(owner: HWND, id: PanelId, anchor: Anchor, context: Context) {
    let Some(runtime) = runtime(owner) else { return; };
    let switch = runtime.try_borrow().is_ok_and(|state| state.open && state.shown && state.panel != id);
    if switch {
        open_panel(&runtime, id, anchor, context);
        if let Ok(mut state) = runtime.try_borrow_mut() { state.hovered_at = Some(Instant::now()); }
    }
}

/// Closes the open flyout (Esc, close button, owner moved or resized). Focus goes back to where it
/// was when the flyout opened.
pub(crate) fn dismiss(owner: HWND) {
    if let Some(runtime) = runtime(owner) { dismiss_runtime(&runtime, Dismissal::RestoreFocus); }
}

/// Another rail button (home, a workspace, a web app) was used: an open flyout closes and leaves
/// the focus to what that button does.
pub(crate) fn close_for_rail(owner: HWND) {
    if let Some(runtime) = runtime(owner) { dismiss_runtime(&runtime, Dismissal::Outside); }
}

/// The active tab or workspace changed: an open flyout closes.
pub(crate) fn note_context(owner: HWND, now: Context) {
    let Some(runtime) = runtime(owner) else { return; };
    let stale = runtime.try_borrow().is_ok_and(|state| state.open && state.context.is_some_and(|opened| opened != now));
    if stale { dismiss_runtime(&runtime, Dismissal::FocusPage); }
}

/// The flyout window lost activation (a click outside, another app, a menu): it closes, and the
/// focus stays wherever the user went. Ignored while a confirmation it asked for is up.
///
/// A press on the rail is the exception: the rail click that follows decides (the same button
/// closes the flyout, another flyout button switches, any other button closes it), so the toggle
/// does not depend on how fast the click reaches Rust. If no rail command comes (a press that
/// became a drag), the flyout closes after `flyout::RAIL_SETTLE_MS`.
pub(crate) fn flyout_deactivated(owner: HWND) {
    // --- window and panels (thread T9) --- "Close floating panels when you click elsewhere" off: it stays
    // until Escape, its close button or its rail button closes it.
    if !crate::window_prefs::auto_close_panels() { return; }
    let Some(runtime) = runtime(owner) else { return; };
    let (close, window) = match runtime.try_borrow() {
        Ok(state) => (state.open && state.hold == 0 && unsafe { GetForegroundWindow() } != state.window, state.window),
        Err(_) => return,
    };
    if !close {
        return;
    }
    let on_rail = unsafe { GetForegroundWindow() } == owner && super::window::cursor_on_rail(owner);
    if on_rail {
        super::window::perf_event(owner, "html_flyout_rail_press");
        if let Ok(mut state) = runtime.try_borrow_mut() { state.rail_press = true; }
        flyout::arm_rail_settle(window);
        return;
    }
    super::window::perf_event(owner, "html_flyout_deactivated");
    dismiss_runtime(&runtime, Dismissal::Outside);
}

/// A rail press kept the flyout open but no rail command followed: close it now unless the flyout
/// took the activation back.
pub(crate) fn flyout_rail_settled(owner: HWND) {
    if !crate::window_prefs::auto_close_panels() { return; } // --- window and panels (thread T9) ---
    let Some(runtime) = runtime(owner) else { return; };
    let close = runtime.try_borrow_mut().is_ok_and(|mut state| {
        let pending = std::mem::take(&mut state.rail_press);
        pending && state.open && unsafe { GetForegroundWindow() } != state.window
    });
    if close { dismiss_runtime(&runtime, Dismissal::Outside); }
}

/// The page drew the flyout (or the show fallback fired): show and activate the window.
pub(crate) fn flyout_painted(owner: HWND) {
    if let Some(runtime) = runtime(owner) { reveal(&runtime); }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dismissal {
    /// Esc, close button, toggle: focus returns to where it was before the flyout opened.
    RestoreFocus,
    /// A page opened from the flyout, or the tab changed: focus goes to the page.
    FocusPage,
    /// The flyout lost activation to something else: leave the focus there.
    Outside,
}

fn dismiss_runtime(runtime: &Rc<RefCell<Runtime>>, how: Dismissal) {
    let (window, host, owner, prior, app) = {
        let Ok(mut state) = runtime.try_borrow_mut() else { return; };
        if !state.open { return; }
        super::window::perf_event(state.owner, match how {
            Dismissal::RestoreFocus => "html_flyout_close",
            Dismissal::FocusPage => "html_flyout_close_page",
            Dismissal::Outside => "html_flyout_close_outside",
        });
        state.open = false;
        state.shown = false;
        state.rail_press = false;
        state.closed_panel = Some(state.panel);
        state.closed_at = (how == Dismissal::Outside).then(Instant::now);
        state.context = None;
        let host = state.host.clone();
        if host.is_some() && state.ready && !state.suspended { state.suspended = true; }
        (state.window, host, state.owner, std::mem::take(&mut state.prior_focus), state.app.clone())
    };
    let was_foreground = unsafe { GetForegroundWindow() } == window;
    flyout::hide(window);
    if let Some(host) = host {
        let _ = host.set_visible(false);
        if host.try_suspend().is_err() {
            if let Ok(mut state) = runtime.try_borrow_mut() { state.suspended = false; }
        }
    }
    if how != Dismissal::Outside && (was_foreground || unsafe { GetForegroundWindow() } == owner) {
        unsafe { let _ = SetForegroundWindow(owner); }
        if how == Dismissal::RestoreFocus && unsafe { IsWindow(Some(prior)).as_bool() } {
            unsafe { let _ = SetFocus(Some(prior)); }
        } else {
            crate::app::App::focus_page(&app);
        }
    }
    crate::win32::post(owner, crate::win32::WM_RAIL_CHANGED, WPARAM(0), LPARAM(0));
}

fn open_panel(runtime: &Rc<RefCell<Runtime>>, id: PanelId, anchor: Anchor, context: Context) {
    let (window, owner) = {
        let Ok(state) = runtime.try_borrow() else { return; };
        (state.window, state.owner)
    };
    let centre = POINT { x: anchor.button.left, y: (anchor.button.top + anchor.button.bottom) / 2 };
    let (dpi, work) = super::html_popup::monitor_data(centre);
    let size = if id == PanelId::Customize { flyout::CUSTOMIZE_SIZE } else { flyout::LIST_SIZE };
    // --- layout (thread T6) --- the flyout follows `side_panel_position` like the docked panels.
    let side = runtime.try_borrow().ok().map_or(crate::settings::SidePanelPosition::Left, |state| crate::app::App::settings(&state.app).side_panel_position);
    let placement = flyout::place_at(anchor.rail_right, anchor.button, client_on_screen(owner), work, dpi, size, side);
    {
        let Ok(mut state) = runtime.try_borrow_mut() else { return; };
        if !state.open {
            state.prior_focus = unsafe { GetFocus() };
            state.shown = false;
        }
        state.open = true;
        state.rail_press = false;
        state.panel = id;
        state.placement = Some(placement);
        state.context = Some(context);
        state.closed_at = None;
    }
    super::window::perf_event(owner, "html_flyout_open");
    flyout::set_bounds(window, placement.window);
    present(runtime);
    crate::win32::post(owner, crate::win32::WM_RAIL_CHANGED, WPARAM(0), LPARAM(0));
}

fn client_on_screen(owner: HWND) -> RECT {
    let mut client = RECT::default();
    let mut origin = POINT::default();
    unsafe {
        let _ = GetClientRect(owner, &mut client);
        let _ = ClientToScreen(owner, &mut origin);
    }
    RECT { left: origin.x, top: origin.y, right: origin.x + client.right, bottom: origin.y + client.bottom }
}

/// Sends the open flyout to the page: theme, caret (restarts the enter motion), panel, snapshot.
/// The window is shown when the page answers that it painted (`flyout_painted`).
fn present(runtime: &Rc<RefCell<Runtime>>) {
    let (host, window, placement, shown, suspended) = {
        let Ok(state) = runtime.try_borrow() else { return; };
        if !state.open || !state.ready || state.closed { return; }
        let (Some(host), Some(placement)) = (state.host.clone(), state.placement) else { return; };
        (host, state.window, placement, state.shown, state.suspended)
    };
    if suspended && host.resume().is_err() {
        mark_failed(runtime);
        return;
    }
    if let Ok(mut state) = runtime.try_borrow_mut() { state.suspended = false; }
    let size = RECT { left: 0, top: 0, right: placement.window.right - placement.window.left, bottom: placement.window.bottom - placement.window.top };
    if host.set_bounds(size).is_err() || host.set_visible(true).is_err() { mark_failed(runtime); return; }
    if !post(runtime, &host, &protocol::flyout_json(placement.caret)) { return; }
    send_current(runtime);
    if shown {
        // A switch while open: the window moved; activation (a rail click may have taken it) and
        // focus follow into the new content.
        if unsafe { GetForegroundWindow() } != window {
            unsafe { let _ = SetForegroundWindow(window); }
        }
        let _ = host.focus();
    } else {
        flyout::arm_show_fallback(window);
    }
}

fn reveal(runtime: &Rc<RefCell<Runtime>>) {
    let (host, window) = {
        let Ok(mut state) = runtime.try_borrow_mut() else { return; };
        if !state.open || state.shown || state.closed { return; }
        let Some(host) = state.host.clone() else { return; };
        state.shown = true;
        super::window::perf_event(state.owner, "html_flyout_shown");
        (host, state.window)
    };
    flyout::show(window);
    let _ = host.focus();
}

/// A visit was just recorded: an open History flyout refreshes its list (only the snapshot is
/// posted, so focus, the search text and the flyout's placement are left alone).
pub(crate) fn history_changed(owner: HWND) {
    let Some(runtime) = runtime(owner) else { return; };
    let (host, query, path) = {
        let Ok(state) = runtime.try_borrow() else { return; };
        if state.closed || !state.ready || !state.open || state.panel != PanelId::History { return; }
        (state.host.clone(), state.query.clone(), current_history_file())
    };
    let Some(host) = host else { return; };
    let entries = path.as_deref().map(|path| productivity::search_history(path, &query, LIMIT)).unwrap_or_default();
    let _ = post(&runtime, &host, &chrome_host::panels::history::snapshot(&entries, &query, history_now_secs(), history_local_offset()));
}

// --- look state (fix group B) ---
/// Settings were saved: an open flyout takes the new text size, density and window look at once
/// (only the `state` is posted, so its list, search text and placement stay).
pub(crate) fn settings_changed(owner: HWND) {
    let Some(runtime) = runtime(owner) else { return; };
    let host = {
        let Ok(state) = runtime.try_borrow() else { return; };
        if state.closed || !state.ready || !state.open { return; }
        state.host.clone()
    };
    if let Some(host) = host {
        let _ = post(&runtime, &host, &super::window::html_panel_state(owner));
    }
}
// --- end look state ---

fn send_current(runtime: &Rc<RefCell<Runtime>>) {
    let (host, owner, open, panel, query, path, profile_dir, app) = {
        let Ok(state) = runtime.try_borrow() else { return; };
        if state.closed || !state.ready { return; }
        let query = if state.panel == PanelId::Bookmarks { state.bookmarks_query.clone() } else { state.query.clone() };
        (state.host.clone(), state.owner, state.open, state.panel, query, current_history_file(), profile::active().ok().map(|p| p.root().to_path_buf()), state.app.clone())
    };
    let Some(host) = host.filter(|_| open) else { return; };
    if !post(runtime, &host, &super::window::html_panel_state(owner)) { return; }
    let name = match panel { PanelId::History => "history", PanelId::Bookmarks => "bookmarks", PanelId::Customize => "customize", _ => "history" };
    let Some(panelshow) = protocol::panelshow_json(name) else { return; };
    if !post(runtime, &host, &panelshow) { return; }
    let snapshot = match panel {
        PanelId::History => {
            let entries = path.as_deref().map(|path| productivity::search_history(path, &query, LIMIT)).unwrap_or_default();
            chrome_host::panels::history::snapshot(&entries, &query, history_now_secs(), history_local_offset())
        }
        PanelId::Bookmarks => {
            let store = profile_dir.as_deref().and_then(|dir| BookmarkStore::load(dir).ok()).unwrap_or_default();
            chrome_host::panels::bookmarks::snapshot(&store, &query, crate::app::App::settings(&app).address.panel_single_click)
        }
        PanelId::Customize => {
            let Some(dir) = profile_dir.as_deref() else { return; };
            let sidebar = crate::sidebar::SidebarConfig::load(dir).unwrap_or_default();
            let entries = crate::app::App::workspace_entries(&app).into_iter().map(|(id, name, icon, colour, tabs, active)| (id.0, name, icon, colour, tabs, active)).collect::<Vec<_>>();
            chrome_host::panels::customize::snapshot(&sidebar, &entries)
        }
        _ => return,
    };
    let _ = post(runtime, &host, &snapshot);
}

fn post(runtime: &Rc<RefCell<Runtime>>, host: &ChromeHost, json: &str) -> bool {
    let success = host.post_json(json).is_ok();
    let failed = {
        let Ok(mut state) = runtime.try_borrow_mut() else { return success; };
        if success { state.failed_posts = 0; false }
        else { state.failed_posts = state.failed_posts.saturating_add(1); state.failed_posts >= 3 }
    };
    if failed { mark_failed(runtime); }
    success
}

fn mark_failed(runtime: &Rc<RefCell<Runtime>>) {
    let owner = {
        let Ok(mut state) = runtime.try_borrow_mut() else { return; };
        if state.failed || state.closed { return; }
        state.failed = true;
        state.owner
    };
    crate::win32::post(owner, super::WM_HTML_PANEL_FAILED, WPARAM(0), LPARAM(0));
}

/// Runs `work` with the flyout held open (a confirmation it shows deactivates it), then gives it
/// the activation back if it is still open.
fn holding(runtime: &Rc<RefCell<Runtime>>, work: impl FnOnce()) {
    if let Ok(mut state) = runtime.try_borrow_mut() { state.hold += 1; }
    work();
    let (window, host, reactivate) = {
        let Ok(mut state) = runtime.try_borrow_mut() else { return; };
        state.hold = state.hold.saturating_sub(1);
        (state.window, state.host.clone(), state.hold == 0 && state.open && state.shown)
    };
    if reactivate && unsafe { GetForegroundWindow() } != window {
        unsafe { let _ = SetForegroundWindow(window); }
        if let Some(host) = host { let _ = host.focus(); }
    }
}

/// A command from inside the flyout may switch the tab (open in a new tab) or the workspace
/// (delete the active one); that was the user's own doing, so the flyout stays open for it.
fn refresh_context(runtime: &Rc<RefCell<Runtime>>) {
    let owner = match runtime.try_borrow() { Ok(state) if state.open => state.owner, _ => return };
    if let Some(context) = super::window::popup_context(owner) {
        if let Ok(mut state) = runtime.try_borrow_mut() { state.context = Some(context); }
    }
}

fn handle_command(runtime: &Rc<RefCell<Runtime>>, command: Command) {
    let family = matches!(command, Command::Panel { kind: PanelType::History | PanelType::Bookmarks | PanelType::Customize, .. });
    run_command(runtime, command);
    if family { refresh_context(runtime); }
}

fn run_command(runtime: &Rc<RefCell<Runtime>>, command: Command) {
    match command {
        Command::Ready => {
            let open = runtime.try_borrow_mut().ok().map(|mut state| {
                state.ready = true;
                state.open
            }).unwrap_or(false);
            if open {
                present(runtime);
            } else {
                let host = runtime.try_borrow_mut().ok().and_then(|mut state| {
                    state.suspended = true;
                    state.host.clone()
                });
                if let Some(host) = host {
                    let _ = host.set_visible(false);
                    if host.try_suspend().is_err() {
                        if let Ok(mut state) = runtime.try_borrow_mut() { state.suspended = false; }
                    }
                }
            }
        }
        Command::Panel { kind: PanelType::Panel, f } if f.id.is_none() && f.parent.is_none() && f.before.is_none()
            && f.n.is_none() && f.on.is_none() && f.text.is_none() && f.url.is_none() => {
            match f.a.as_str() {
                "dismiss" => dismiss_runtime(runtime, Dismissal::RestoreFocus),
                "shown" => reveal(runtime),
                _ => {}
            }
        }
        Command::Panel { kind: PanelType::History, f } => {
            let (app, owner, query) = {
                let Ok(state) = runtime.try_borrow() else { return; };
                (state.app.clone(), state.owner, state.query.clone())
            };
            let path = current_history_file();
            let entries = path.as_deref().map(|path| productivity::search_history(path, &query, LIMIT)).unwrap_or_default();
            let mut model = Model::default();
            model.set_query(&query);
            model.rebuild(&entries, history_now_secs(), history_local_offset());
            let action = chrome_host::panels::history::map_command(&mut model, f);
            if model.query != query { if let Ok(mut state) = runtime.try_borrow_mut() { state.query = model.query.clone(); } }
            let Some(action) = action else { return; };
            match action {
                Action::Dismiss => dismiss_runtime(runtime, Dismissal::RestoreFocus),
                Action::Open { url, new_tab } => {
                    let _ = execute_history_action(&app, owner, Action::Open { url, new_tab });
                    if new_tab { send_current(runtime); } else { dismiss_runtime(runtime, Dismissal::FocusPage); }
                }
                action => {
                    holding(runtime, || {
                        if let Some(Err(error)) = execute_history_action(&app, owner, action) {
                            show_error_message(&format!("History could not be changed: {error}"));
                        }
                    });
                    send_current(runtime);
                }
            }
        }
        Command::Panel { kind: PanelType::Bookmarks, f } => {
            let (app, owner) = {
                let Ok(state) = runtime.try_borrow() else { return; };
                (state.app.clone(), state.owner)
            };
            let Ok(profile) = profile::active() else { return; };
            let profile_dir = profile.root();
            let Ok(mut store) = BookmarkStore::load(profile_dir) else { return; };
            let Some(action) = chrome_host::panels::bookmarks::map_command(&store, f) else { return; };
            let mutates = action.mutates();
            let mut deleted = false;
            match action {
                chrome_host::panels::bookmarks::Action::Search(query) => {
                    if let Ok(mut state) = runtime.try_borrow_mut() { state.bookmarks_query = query; }
                }
                chrome_host::panels::bookmarks::Action::Open { url, new_tab } => {
                    // "Open bookmarks in a new tab" (bm_open_new_tab).
                    let new_tab = new_tab || crate::app::App::settings(&app).address.bookmark_new_tab;
                    let _ = execute_history_action(&app, owner, Action::Open { url, new_tab });
                    if !new_tab {
                        dismiss_runtime(runtime, Dismissal::FocusPage);
                        return;
                    }
                }
                chrome_host::panels::bookmarks::Action::OpenAll(urls) => {
                    // Asks first from "confirm when opening N or more bookmarks" (bm_confirm_open).
                    holding(runtime, || super::address_bar::open_many(owner, &app, &urls));
                    return;
                }
                chrome_host::panels::bookmarks::Action::Delete(id) => { deleted = store.delete(id) > 0; }
                chrome_host::panels::bookmarks::Action::Rename { id, title } => { let _ = store.rename(id, &title); }
                chrome_host::panels::bookmarks::Action::Update { id, title, url } => { let _ = store.update_bookmark(id, &title, &url); }
                chrome_host::panels::bookmarks::Action::AddFolder { parent, title } => { let _ = store.add_folder(parent, &title); }
                chrome_host::panels::bookmarks::Action::AddLink { parent, title, url } => { let _ = store.add_bookmark(parent, &title, &url); }
                chrome_host::panels::bookmarks::Action::Move { id, parent, index } => { let _ = store.move_item(id, parent, index); }
            }
            // Searching and opening are read-only: no rewrite (and fsync) of bookmarks-v2.txt.
            if mutates {
                match store.save(profile_dir) {
                    Err(error) => holding(runtime, || show_error_message(&format!("Bookmarks could not be saved: {error}"))),
                    // The nickname and description of what was deleted (a folder takes its contents) go
                    // with it, as the bookmark bar and the bookmark dialog already do.
                    Ok(()) if deleted => {
                        if let Ok(mut extras) = crate::bookmark_extras::Extras::load_for_change(profile_dir) {
                            let before = extras.len();
                            extras.prune(&store);
                            if extras.len() != before {
                                let _ = extras.save(profile_dir);
                            }
                        }
                    }
                    Ok(()) => {}
                }
            }
            send_current(runtime);
        }
        Command::ComposerOpen { mode, id, name, url, rect } => {
            let (owner, host) = {
                let Ok(state) = runtime.try_borrow() else { return; };
                (state.owner, state.host.clone())
            };
            let Some(view) = host.and_then(|host| host.screen_bounds()) else { return; };
            super::window::request_composer(owner, view, mode, id, name, url, rect);
        }
        Command::Panel { kind: PanelType::Customize, f } => {
            let (app, owner) = {
                let Ok(state) = runtime.try_borrow() else { return; };
                (state.app.clone(), state.owner)
            };
            let Ok(profile) = profile::active() else { return; };
            let profile_dir = profile.root();
            let mut sidebar = crate::sidebar::SidebarConfig::load(profile_dir).unwrap_or_default();
            let Some(action) = chrome_host::panels::customize::map_command(f) else { return; };
            use chrome_host::panels::customize::Action as CustomizeAction;
            let mut changed_sidebar = false;
            match action {
                CustomizeAction::Visibility(kind, visible) => {
                    if sidebar.set_visible(kind, visible).is_ok() { changed_sidebar = true; }
                }
                CustomizeAction::AppRemove(id) => {
                    if sidebar.remove_app_remembered(id) { super::panel_webapp::forget_app(id); changed_sidebar = true; }
                }
                CustomizeAction::AppMove(id, before) => {
                    let Some(from) = sidebar.apps.iter().position(|item| item.id == id) else { send_current(runtime); return; };
                    let mut position = before.and_then(|before| sidebar.apps.iter().position(|item| item.id == before)).unwrap_or(sidebar.apps.len());
                    if position > from { position = position.saturating_sub(1); }
                    if sidebar.reorder_app(id, position).unwrap_or(false) { changed_sidebar = true; }
                }
                CustomizeAction::WorkspaceRename(id, name) => { let _ = crate::app::App::rename_workspace(&app, crate::workspaces::WorkspaceId(id), &name); }
                CustomizeAction::WorkspaceDelete(id) => { let _ = crate::app::App::delete_workspace(&app, crate::workspaces::WorkspaceId(id)); }
                CustomizeAction::WorkspaceMove(id, before) => {
                    let workspaces = crate::app::App::workspaces(&app);
                    if let Some(from) = workspaces.all().iter().position(|item| item.id().0 == id) {
                        let mut target = before.and_then(|before| workspaces.all().iter().position(|item| item.id().0 == before)).unwrap_or(workspaces.all().len());
                        if target > from { target = target.saturating_sub(1); }
                        let _ = crate::app::App::workspace_move(&app, crate::workspaces::WorkspaceId(id), target);
                    }
                }
                CustomizeAction::WorkspaceIcon(id, icon) => { let _ = crate::app::App::workspace_set_icon(&app, crate::workspaces::WorkspaceId(id), icon); }
                CustomizeAction::WorkspaceColour(id, colour) => { let _ = crate::app::App::workspace_set_colour(&app, crate::workspaces::WorkspaceId(id), colour); }
                CustomizeAction::SwitchProfile => {
                    let _ = super::window::html_panel_accelerator(owner, Accel { vk: 0x50, ctrl: true, shift: true, alt: false, key_down: true, is_system: false, repeat: false });
                }
                CustomizeAction::ClearAppData => {
                    holding(runtime, || {
                        if crate::win32::ask_yes_no_owned(owner, "Clear the cookies, storage and cache of every pinned web app? Each one will ask you to sign in again.") {
                            if let Err(error) = super::panel_webapp::clear_all_data() { show_error_message(&format!("Web app data could not be cleared: {error}")); }
                        }
                    });
                }
            }
            if changed_sidebar {
                if let Err(error) = sidebar.save(profile_dir) {
                    holding(runtime, || show_error_message(&format!("Sidebar changes could not be saved: {error}")));
                }
            }
            // Workspace changes refresh the rail too (workspace_create and friends post it already;
            // a visibility or app change needs it here).
            crate::win32::post(owner, crate::win32::WM_RAIL_CHANGED, WPARAM(0), LPARAM(0));
            send_current(runtime);
        }
        _ => {}
    }
}
