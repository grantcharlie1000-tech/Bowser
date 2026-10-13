//! Split view chrome around the two panes (see docs/design/split-view.md):
//!
//! - one HTML host (`View::Split`) in a Bowser-owned child window that shows either the empty
//!   side's "choose a tab" surface (over the pane that has no page) or the drop-zone card while a
//!   tab from the HTML strip is dragged over the page;
//! - the split menus (toolbar button, divider grip, tab menu entries) and the entry points
//!   (toggle, open with an empty side, drop a dragged tab);
//! - divider click detection and the `pair` broadcast cache.
//!
//! Every function runs on the UI thread. No `RefCell` borrow of the runtime table is held while
//! calling into `App`, WebView2 or window functions, which may re-enter this module.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::ScreenToClient,
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetSystemMetrics, IsWindow,
            RegisterClassW, SetWindowPos, ShowWindow, HWND_TOP, SM_CXDRAG, SWP_NOACTIVATE,
            SWP_SHOWWINDOW, SW_HIDE, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
        },
    },
    core::{w, PCWSTR},
};

use super::menu::{self, Entry, Icon, Item, Menu};
use crate::{
    app::{App, AppRef},
    chrome_host::{
        self,
        host::{Accel, AccelHandler, ChromeHost, View},
        protocol::{Command, SplitCommand},
        split::{self as wire, PickClosed, PickTab},
    },
    split_view::{self, Pane},
    tabs::TabId,
};

const CLASS: PCWSTR = w!("BowserSplitPane");

/// What the split host shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Shown {
    Hidden,
    /// The empty side: tab `tab` (no page) in `pane`, at `rect` (client coordinates).
    Picker { tab: TabId, pane: Pane, rect: RECT },
    /// The drop-zone card for a dragged tab over `pane`'s half of the page.
    Drop { pane: Pane, rect: RECT },
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    tab: TabId,
    target: Option<Pane>,
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
    /// Last content and state messages the page accepted.
    content: Option<String>,
    state: Option<String>,
    /// Open tabs changed since the picker content was built.
    dirty: bool,
    /// The empty tab the picker last took the keyboard for (focus moves there once per side).
    focused_for: Option<TabId>,
    drag: Option<Drag>,
    /// Divider press: the client x of the press and whether the pointer moved past the drag
    /// threshold since.
    press: Option<(i32, bool)>,
    /// Last `pair` message sent to the toolbar and the tab strip, with the host generation.
    pair_sent: [Option<(u64, String)>; 2],
}

thread_local! {
    static RUNTIMES: RefCell<HashMap<isize, Runtime>> = RefCell::new(HashMap::new());
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

fn wire_id(tab: TabId) -> u32 {
    tab.0.saturating_add(1)
}

fn tab_of_wire(app: &AppRef, id: u32) -> Option<TabId> {
    let tab = TabId(id.checked_sub(1)?);
    App::tab_ids(app).contains(&tab).then_some(tab)
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

/// Creates the container (hidden) and starts the controller inside it, once per window.
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
        let _ = with(owner, |rt| rt.failed = true);
        return;
    }
    let instance = unsafe { GetModuleHandleW(None) }.map(|module| module.into()).ok();
    // Created visible at one pixel: a hidden WebView2 loads at background priority (see host.rs).
    let container = unsafe {
        CreateWindowExW(
            Default::default(),
            CLASS,
            w!(""),
            WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            0,
            0,
            1,
            1,
            Some(owner),
            None,
            instance,
            None,
        )
    };
    let Ok(container) = container else {
        let _ = with(owner, |rt| rt.failed = true);
        return;
    };
    unsafe {
        let _ = SetWindowPos(container, Some(HWND_TOP), 0, 0, 1, 1, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
    let _ = with(owner, |rt| rt.container = Some(container));
    let raw = owner.0 as isize;
    let on_command: Rc<dyn Fn(Command)> = Rc::new(move |command| {
        // Never inside WebView2's callback: commands may close tabs or open menus.
        let owner = HWND(raw as *mut _);
        if let Command::Ready = command {
            let _ = with(owner, |rt| {
                rt.ready = true;
                rt.content = None;
                rt.state = None;
            });
            super::window::split_ui_invalidate(owner);
            return;
        }
        if let Command::Split(split) = command {
            queue(owner, split);
        }
    });
    let on_failed: Rc<dyn Fn()> = Rc::new(move || {
        let owner = HWND(raw as *mut _);
        let _ = with(owner, |rt| rt.failed = true);
        super::window::split_ui_invalidate(owner);
    });
    let on_accelerator: AccelHandler = Rc::new(move |accel: Accel| super::window::html_split_accelerator(HWND(raw as *mut _), accel));
    let created = ChromeHost::create(&env, container, RECT { left: 0, top: 0, right: 1, bottom: 1 }, View::Split, on_command, on_failed, Some(on_accelerator), move |result| {
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
            Err(_) => {
                let _ = with(owner, |rt| rt.failed = true);
                super::window::split_ui_invalidate(owner);
            }
        }
    });
    if created.is_err() {
        let _ = with(owner, |rt| rt.failed = true);
    }
}

thread_local! {
    static QUEUE: RefCell<Vec<(isize, SplitCommand)>> = const { RefCell::new(Vec::new()) };
}

/// Queues a page command and posts `WM_SPLIT_COMMAND` so it runs from the window procedure.
fn queue(owner: HWND, command: SplitCommand) {
    let full = QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        if queue.len() >= 16 {
            return true;
        }
        queue.push((owner.0 as isize, command));
        false
    });
    if !full {
        crate::win32::post(owner, super::WM_SPLIT_COMMAND, WPARAM(0), LPARAM(0));
    }
}

/// Runs the queued commands for `owner` (from `WM_SPLIT_COMMAND`).
pub(crate) fn run_queued(owner: HWND, app: &AppRef) {
    let commands: Vec<SplitCommand> = QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        let (mine, rest): (Vec<_>, Vec<_>) = queue.drain(..).partition(|(id, _)| *id == owner.0 as isize);
        *queue = rest;
        mine.into_iter().map(|(_, command)| command).collect()
    });
    for command in commands {
        match command {
            SplitCommand::DragOver { .. } | SplitCommand::Drop { .. } | SplitCommand::DragEnd { .. } => drag(owner, app, command),
            command => picker_command(owner, app, command),
        }
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

/// Open tabs changed (titles, favicons, order, selection): rebuild the picker's list on the next
/// sync, and give a pair that was just shown again (selected from the strip) its pane bounds.
pub(crate) fn tabs_changed(owner: HWND, app: &AppRef) {
    let _ = with(owner, |rt| rt.dirty = true);
    App::refresh_split_panes(app);
    if App::split_view(app).is_some() {
        super::window::sync_split_ratio(owner, App::split_view(app));
        let panes = super::window::split_rects(owner, None);
        if panes != App::split_bounds(app) {
            match panes {
                Some(panes) => App::update_split_bounds(app, panes),
                None => App::clear_split_bounds(app),
            }
            super::window::split_ui_invalidate(owner);
        }
    }
}

/// A drag message from the tab strip: queued like the empty side's commands.
pub(crate) fn queue_from_tabs(owner: HWND, command: SplitCommand) {
    if matches!(command, SplitCommand::DragOver { .. } | SplitCommand::Drop { .. } | SplitCommand::DragEnd { .. }) {
        queue(owner, command);
    }
}

thread_local! {
    static LINKS: RefCell<Vec<(isize, url::Url)>> = const { RefCell::new(Vec::new()) };
}

/// A page asked to open `url` in split view (link context menu). Runs later from the window
/// procedure, never inside the WebView2 callback.
pub(crate) fn queue_link(owner: HWND, url: url::Url) {
    if !matches!(url.scheme(), "http" | "https") {
        return;
    }
    let queued = LINKS.with(|links| {
        let mut links = links.borrow_mut();
        if links.len() >= 4 {
            return false;
        }
        links.push((owner.0 as isize, url));
        true
    });
    if queued {
        crate::win32::post(owner, super::WM_SPLIT_LINK, WPARAM(0), LPARAM(0));
    }
}

pub(crate) fn run_links(owner: HWND, app: &AppRef) {
    let urls: Vec<url::Url> = LINKS.with(|links| {
        let mut links = links.borrow_mut();
        let (mine, rest): (Vec<_>, Vec<_>) = links.drain(..).partition(|(id, _)| *id == owner.0 as isize);
        *links = rest;
        mine.into_iter().map(|(_, url)| url).collect()
    });
    if let Some(url) = urls.last() {
        open_link(owner, app, url);
    }
}

/// Gives the keyboard to the empty side when it is shown and is the focused side. True when it did.
pub(crate) fn take_focus(owner: HWND) -> bool {
    let Some(app) = super::window::popup_app(owner) else { return false };
    let Some((host, pane)) = with(owner, |rt| match rt.shown {
        Some(Shown::Picker { pane, .. }) if rt.ready && !rt.failed => rt.host.clone().map(|host| (host, pane)),
        _ => None,
    })
    .flatten() else {
        return false;
    };
    if !App::split_view(&app).is_some_and(|split| split.focused == pane) {
        return false;
    }
    host.focus().is_ok()
}

/// The side the HTML "choose a tab" surface covers right now (the overlay then leaves it alone).
pub(crate) fn picker_pane(owner: HWND) -> Option<Pane> {
    with(owner, |rt| match rt.shown {
        Some(Shown::Picker { pane, .. }) if rt.ready && !rt.failed => Some(pane),
        _ => None,
    })
    .flatten()
}

/// The side a dragged tab would land on, when the HTML card cannot show it (overlay fallback).
pub(crate) fn drop_fallback(owner: HWND) -> Option<Pane> {
    with(owner, |rt| {
        let target = rt.drag.and_then(|drag| drag.target)?;
        let card = rt.ready && !rt.failed && matches!(rt.shown, Some(Shown::Drop { .. }));
        (!card).then_some(target)
    })
    .flatten()
}

/// What the host should show now.
fn wanted(owner: HWND, app: &AppRef) -> Shown {
    let drag = with(owner, |rt| rt.drag).flatten();
    if let Some(Drag { target: Some(pane), .. }) = drag
        && let Some(viewport) = super::window::split_viewport(owner)
    {
        let gap = super::window::split_px(owner, split_view::GAP_DIP);
        let mid = viewport.left + (viewport.right - viewport.left) / 2;
        let rect = match pane {
            Pane::Left => RECT { right: mid - gap / 2, ..viewport },
            Pane::Right => RECT { left: mid + gap - gap / 2, ..viewport },
        };
        return Shown::Drop { pane, rect };
    }
    let Some(split) = App::split_view(app) else {
        return Shown::Hidden;
    };
    for pane in [Pane::Left, Pane::Right] {
        let tab = split.tab(pane);
        if !App::tab_is_blank(app, tab) {
            continue;
        }
        let rect = match App::split_bounds(app) {
            Some((left, right)) => if pane == Pane::Left { left } else { right },
            // Too narrow for two panes: only the focused side shows, over the whole viewport.
            None if split.focused == pane => match super::window::split_viewport(owner) {
                Some(viewport) => viewport,
                None => continue,
            },
            None => continue,
        };
        return Shown::Picker { tab, pane, rect };
    }
    Shown::Hidden
}

/// Brings the host in line with the split state. Cheap when nothing changed; called from every
/// chrome paint and after tab changes.
pub(crate) fn sync(owner: HWND, app: &AppRef) {
    if !chrome_host::html_ui_enabled() {
        return;
    }
    let want = wanted(owner, app);
    if want != Shown::Hidden {
        ensure_host(owner);
    }
    let Some((host, container, ready, shown, dirty, focused_for, suspended)) = with(owner, |rt| {
        (rt.host.clone(), rt.container, rt.ready && !rt.failed, rt.shown, rt.dirty, rt.focused_for, rt.suspended)
    }) else {
        return;
    };
    let (Some(host), Some(container), true) = (host, container, ready) else {
        return;
    };
    if want == Shown::Hidden {
        if shown.is_some_and(|shown| shown != Shown::Hidden) {
            let _ = host.post_json(&wire::hide_json());
            let _ = host.set_visible(false);
            unsafe {
                let _ = ShowWindow(container, SW_HIDE);
            }
            let suspend = with(owner, |rt| {
                rt.shown = Some(Shown::Hidden);
                rt.content = None;
                let idle = rt.drag.is_none() && !rt.suspended;
                if idle {
                    rt.suspended = true;
                }
                idle
            })
            .unwrap_or(false);
            if suspend && host.try_suspend().is_err() {
                let _ = with(owner, |rt| rt.suspended = false);
            }
        }
        return;
    }
    if suspended {
        if host.resume().is_err() {
            return;
        }
        let _ = with(owner, |rt| rt.suspended = false);
    }
    let rect = match want {
        Shown::Picker { rect, .. } | Shown::Drop { rect, .. } => rect,
        Shown::Hidden => return,
    };
    // Theme first, then content, then show: the page never appears unthemed or stale.
    let state = super::window::split_host_state(owner);
    if with(owner, |rt| rt.state.as_deref() != Some(state.as_str())).unwrap_or(false) && host.post_json(&state).is_ok() {
        let _ = with(owner, |rt| rt.state = Some(state));
    }
    let content = match want {
        Shown::Picker { tab, pane, .. } => {
            let same_side = matches!(shown, Some(Shown::Picker { tab: old, pane: old_pane, .. }) if old == tab && old_pane == pane);
            (!same_side || dirty).then(|| pick_content(app, pane))
        }
        Shown::Drop { pane, .. } => {
            let same = matches!(shown, Some(Shown::Drop { pane: old, .. }) if old == pane);
            (!same).then(|| {
                let title = with(owner, |rt| rt.drag.map(|drag| drag.tab))
                    .flatten()
                    .and_then(|tab| App::tab_title(app, tab))
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| "New tab".to_owned());
                wire::drop_json(pane, &title)
            })
        }
        Shown::Hidden => None,
    };
    if let Some(content) = content
        && with(owner, |rt| rt.content.as_deref() != Some(content.as_str())).unwrap_or(false)
        && host.post_json(&content).is_ok()
    {
        let _ = with(owner, |rt| {
            rt.content = Some(content);
            rt.dirty = false;
        });
    }
    let moved = !matches!(shown, Some(old) if old == want);
    if moved {
        let size = rect_size(rect);
        unsafe {
            let _ = SetWindowPos(
                container,
                Some(HWND_TOP),
                rect.left,
                rect.top,
                size.right,
                size.bottom,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        let _ = host.set_bounds(size);
        let _ = host.set_visible(true);
        let _ = with(owner, |rt| rt.shown = Some(want));
    }
    // The empty side takes the keyboard once, when it appears as the focused side.
    if let Shown::Picker { tab, pane, .. } = want
        && focused_for != Some(tab)
        && App::split_view(app).is_some_and(|split| split.focused == pane)
    {
        let _ = with(owner, |rt| rt.focused_for = Some(tab));
        let _ = host.focus();
    }
}

fn pick_content(app: &AppRef, pane: Pane) -> String {
    let pair = App::split_view(app);
    let workspace = App::workspaces(app).active().tabs().to_vec();
    struct Row {
        id: u32,
        title: String,
        host: String,
        png: Option<Vec<u8>>,
        private: bool,
    }
    let rows: Vec<Row> = workspace
        .into_iter()
        .filter(|tab| !pair.is_some_and(|pair| pair.contains(*tab)) && !App::tab_is_blank(app, *tab))
        .map(|tab| {
            let url = App::tab_committed_url(app, tab).map(|url| url.to_string()).unwrap_or_default();
            Row {
                id: wire_id(tab),
                title: App::tab_title(app, tab).filter(|title| !title.trim().is_empty()).unwrap_or_else(|| "New tab".to_owned()),
                host: wire::display_host(&url),
                png: App::tab_favicon(app, tab),
                private: App::tab_private(app, tab),
            }
        })
        .collect();
    let tabs: Vec<PickTab<'_>> = rows
        .iter()
        .map(|row| PickTab { id: row.id, title: &row.title, host: &row.host, png: row.png.as_deref(), private: row.private })
        .collect();
    let closed_rows: Vec<(String, String)> = App::recently_closed(app)
        .into_iter()
        .map(|closed| {
            let title = if closed.title.trim().is_empty() { closed.url.clone() } else { closed.title };
            (title, wire::display_host(&closed.url))
        })
        .collect();
    let closed: Vec<PickClosed<'_>> = closed_rows.iter().map(|(title, host)| PickClosed { title, host }).collect();
    wire::pick_json(pane, &tabs, &closed)
}

/// A command from the empty side. Only acts while the picker is shown, on its own tab.
fn picker_command(owner: HWND, app: &AppRef, command: SplitCommand) {
    let Some(Some(Shown::Picker { tab: empty, .. })) = with(owner, |rt| rt.shown) else {
        return;
    };
    if !App::split_view(app).is_some_and(|split| split.contains(empty)) || !App::tab_is_blank(app, empty) {
        return;
    }
    match command {
        SplitCommand::Pick { id } => {
            let Some(picked) = tab_of_wire(app, id) else { return };
            if !App::workspaces(app).active().tabs().contains(&picked) {
                return;
            }
            if App::fill_split_empty(app, empty, picked) {
                App::focus_page(app);
            }
        }
        SplitCommand::Reopen { n } => {
            let Some(closed) = App::recently_closed(app).into_iter().nth(usize::from(n)) else { return };
            let _ = App::focus_split_tab(app, empty);
            App::submit(app, &closed.url);
            App::focus_page(app);
        }
        SplitCommand::Go(text) => {
            let _ = App::focus_split_tab(app, empty);
            App::submit(app, &text);
            App::focus_page(app);
        }
        SplitCommand::Close => {
            let _ = App::close_tab(app, empty);
        }
        SplitCommand::Focus => {
            let _ = App::focus_split_tab(app, empty);
        }
        SplitCommand::DragOver { .. } | SplitCommand::Drop { .. } | SplitCommand::DragEnd { .. } => {}
    }
    super::window::split_ui_invalidate(owner);
}

// ----- dragging a tab from the HTML strip onto the page -----

fn drag(owner: HWND, app: &AppRef, command: SplitCommand) {
    match command {
        SplitCommand::DragOver { id, x, y } => {
            let Some(tab) = tab_of_wire(app, id) else { return };
            let target = drop_target(owner, app, tab, x, y);
            let changed = with(owner, |rt| {
                let old = rt.drag.and_then(|drag| drag.target);
                rt.drag = Some(Drag { tab, target });
                old != target
            })
            .unwrap_or(false);
            if changed {
                super::window::split_ui_invalidate(owner);
            }
        }
        SplitCommand::Drop { id } => {
            let drag = with(owner, |rt| rt.drag.take()).flatten();
            super::window::split_ui_invalidate(owner);
            let (Some(Drag { tab, target: Some(pane) }), Some(dropped)) = (drag, tab_of_wire(app, id)) else { return };
            if tab != dropped {
                return;
            }
            drop_tab(owner, app, tab, pane);
        }
        SplitCommand::DragEnd { .. } => {
            if with(owner, |rt| rt.drag.take()).flatten().is_some() {
                super::window::split_ui_invalidate(owner);
            }
        }
        _ => {}
    }
}

/// The side of the page under the dragged tab, or `None` when the pointer is not over the page
/// (or the drop would not make sense: the tab already fills that side).
fn drop_target(owner: HWND, app: &AppRef, tab: TabId, x: u32, y: u32) -> Option<Pane> {
    let (screen, dpi) = super::window::tabs_host_screen(owner)?;
    let (sx, sy) = wire::css_to_screen((screen.left, screen.top), dpi as f32 / 96.0, x, y);
    let mut point = POINT { x: sx, y: sy };
    unsafe {
        let _ = ScreenToClient(owner, &mut point);
    }
    let viewport = super::window::split_viewport(owner)?;
    if point.y < viewport.top || point.y >= viewport.bottom {
        return None;
    }
    let pane = split_view::side_of(viewport.left, viewport.right, point.x)?;
    match App::split_view(app) {
        Some(split) if split.tab(pane) == tab => None,
        _ => Some(pane),
    }
}

fn drop_tab(owner: HWND, app: &AppRef, tab: TabId, pane: Pane) {
    if App::split_view(app).is_some() {
        let _ = App::set_split_pane(app, pane, tab);
    } else {
        let active = App::active_tab_id(app);
        let bounds = bounds_for_new_split(owner);
        if active != tab {
            let (left, right) = match pane {
                Pane::Left => (tab, active),
                Pane::Right => (active, tab),
            };
            let _ = App::open_split_pair(app, left, right, pane, bounds);
        } else {
            // The shown tab itself: it moves to that side and the other side offers a tab.
            let _ = App::open_split_with_empty(app, tab, pane.other(), bounds);
        }
    }
    refresh_bounds(owner, app);
    App::focus_page(app);
}

// ----- bounds and entry points -----

/// The pane rectangles a new 50/50 split would get.
fn bounds_for_new_split(owner: HWND) -> Option<app_bounds::Bounds> {
    super::window::split_rects_for_ratio(owner, 50)
}

mod app_bounds {
    pub type Bounds = crate::app::SplitBounds;
}

/// Re-applies the divider position and pane rectangles of the shown split (after it was opened,
/// un-parked or swapped).
pub(crate) fn refresh_bounds(owner: HWND, app: &AppRef) {
    super::window::sync_split_ratio(owner, App::split_view(app));
    match super::window::split_rects(owner, None) {
        Some(panes) => {
            if App::split_bounds(app) != Some(panes) {
                App::update_split_bounds(app, panes);
            }
        }
        None => App::clear_split_bounds(app),
    }
    super::window::split_ui_invalidate(owner);
}

/// Ctrl+Shift+\: separate a shown split, otherwise split the shown tab with an empty side.
pub(crate) fn toggle(owner: HWND, app: &AppRef) {
    if App::split_view(app).is_some() {
        let _ = App::separate_split(app);
    } else {
        let tab = App::active_tab_id(app);
        let _ = App::open_split_with_empty(app, tab, Pane::Right, bounds_for_new_split(owner));
    }
    refresh_bounds(owner, app);
}

/// "Add tab to new split view" (the shown tab) or "New split view with current tab" (another tab).
pub(crate) fn split_with_tab(owner: HWND, app: &AppRef, tab: TabId) {
    let active = App::active_tab_id(app);
    let bounds = bounds_for_new_split(owner);
    if tab == active {
        let _ = App::open_split_with_empty(app, tab, Pane::Right, bounds);
    } else {
        let _ = App::open_split_pair(app, active, tab, Pane::Right, bounds);
    }
    refresh_bounds(owner, app);
}

/// "Open link in split view": the link opens in a new tab beside the shown tab.
pub(crate) fn open_link(owner: HWND, app: &AppRef, url: &url::Url) {
    let tab = App::active_tab_id(app);
    if App::split_view(app).is_some() {
        let _ = App::separate_split(app);
    }
    let _ = App::open_url_in_split(app, tab, url, bounds_for_new_split(owner));
    refresh_bounds(owner, app);
}

// ----- menus -----

const SEPARATE: usize = 1;
const SWAP: usize = 2;
const CLOSE_LEFT: usize = 3;
const CLOSE_RIGHT: usize = 4;

fn arrange_entries() -> Vec<Entry> {
    vec![
        Entry::Item(Item::new(SEPARATE, "Separate views", Icon::None).glyph("unsplit").shortcut("Ctrl+Shift+\\")),
        Entry::Item(Item::new(SWAP, "Swap sides", Icon::None).glyph("swap")),
        Entry::Separator,
        Entry::Item(Item::new(CLOSE_LEFT, "Close left tab", Icon::Close).glyph("split-left")),
        Entry::Item(Item::new(CLOSE_RIGHT, "Close right tab", Icon::Close).glyph("split-right")),
    ]
}

/// The split menu at `anchor` (screen): Separate views, Swap sides, Close left tab, Close right tab.
/// Each pick re-checks that the same pair is still shown.
pub(crate) fn show_menu(owner: HWND, app: &AppRef, anchor: POINT, placement: menu::Placement, kind: &str) {
    let Some(split) = App::split_view(app) else { return };
    menu::show(
        owner,
        anchor,
        placement,
        Menu { header: None, entries: arrange_entries(), footer: "Split view".to_owned() },
        kind,
        move |app, chosen| {
            let Some(chosen) = chosen else { return };
            let Some(now) = App::split_view(app) else { return };
            if (now.left, now.right) != (split.left, split.right) {
                return;
            }
            match chosen.id {
                SEPARATE => {
                    let _ = App::separate_split(app);
                }
                SWAP => {
                    let _ = App::swap_split_panes(app);
                }
                CLOSE_LEFT => {
                    let _ = App::close_split_pane(app, Pane::Left);
                }
                CLOSE_RIGHT => {
                    let _ = App::close_split_pane(app, Pane::Right);
                }
                _ => return,
            }
            refresh_bounds(owner, app);
        },
    );
}

/// The toolbar split button: not split, split with an empty side; split, the split menu under the
/// pointer (the button is under it, since the click just happened).
pub(crate) fn toolbar_button(owner: HWND, app: &AppRef) {
    if App::split_view(app).is_none() {
        toggle(owner, app);
        return;
    }
    let mut cursor = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut cursor);
    }
    let half = super::window::split_px(owner, 12);
    let anchor = POINT { x: cursor.x + half, y: cursor.y + half + super::window::split_px(owner, 4) };
    show_menu(owner, app, anchor, menu::Placement::AboveLeft, "split");
}

/// The divider menu, beside the grip.
pub(crate) fn divider_menu(owner: HWND, app: &AppRef, client_y: i32) {
    let Some(gap) = super::window::split_gap(owner) else { return };
    let anchor = super::window::screen_point(owner, POINT { x: gap.right + super::window::split_px(owner, 4), y: client_y });
    show_menu(owner, app, anchor, menu::Placement::BelowLeft, "split-divider");
}

/// The divider was pressed at client `x`.
pub(crate) fn divider_pressed(owner: HWND, x: i32) {
    let _ = with(owner, |rt| rt.press = Some((x, false)));
}

/// The pointer moved to client `x` while the divider is held: true once it counts as a drag.
pub(crate) fn divider_dragged(owner: HWND, x: i32) -> bool {
    let threshold = unsafe { GetSystemMetrics(SM_CXDRAG) }.max(2);
    with(owner, |rt| match rt.press.as_mut() {
        Some((start, moved)) => {
            if !*moved && (x - *start).abs() >= threshold {
                *moved = true;
            }
            *moved
        }
        None => true,
    })
    .unwrap_or(true)
}

/// The divider was released: true when it was a click (no drag), which opens the divider menu.
pub(crate) fn divider_released(owner: HWND) -> bool {
    with(owner, |rt| rt.press.take().is_some_and(|(_, moved)| !moved)).unwrap_or(false)
}

/// Entries for the tab context menu, given whether `tab` is in the window's pair.
pub(crate) const TAB_SPLIT: usize = 0x5100;
pub(crate) const TAB_SEPARATE: usize = 0x5101;
pub(crate) const TAB_SWAP: usize = 0x5102;

pub(crate) fn tab_menu_entries(app: &AppRef, tab: TabId) -> Vec<Entry> {
    let in_pair = App::split_pair(app).is_some_and(|(split, _)| split.contains(tab));
    if in_pair {
        let shown = App::split_view(app).is_some_and(|split| split.contains(tab));
        return vec![
            Entry::Item(Item::new(TAB_SEPARATE, "Separate views", Icon::None).glyph("unsplit")),
            Entry::Item(Item::new(TAB_SWAP, "Swap sides", Icon::None).glyph("swap").disabled_if(!shown)),
        ];
    }
    let label = if tab == App::active_tab_id(app) { "Add tab to new split view" } else { "New split view with current tab" };
    vec![Entry::Item(Item::new(TAB_SPLIT, label, Icon::Grid).glyph("split"))]
}

/// Runs a tab-menu split entry. Returns false for other ids.
pub(crate) fn tab_menu_pick(owner: HWND, app: &AppRef, tab: TabId, id: usize) -> bool {
    match id {
        TAB_SPLIT => split_with_tab(owner, app, tab),
        TAB_SEPARATE => {
            if App::split_pair(app).is_some_and(|(split, _)| split.contains(tab)) {
                let _ = App::separate_split(app);
            }
        }
        TAB_SWAP => {
            if App::split_view(app).is_some_and(|split| split.contains(tab)) {
                let _ = App::swap_split_panes(app);
            }
        }
        _ => return false,
    }
    refresh_bounds(owner, app);
    true
}

// ----- the pair broadcast -----

/// The `pair` message for the window's current pair.
pub(crate) fn pair_message(app: &AppRef) -> String {
    wire::pair_json(App::split_pair(app).map(|(split, shown)| (wire_id(split.left), wire_id(split.right), split.focused, shown)))
}

/// Whether `json` still has to be sent to host `index` (0 toolbar, 1 tabs) at `generation`.
pub(crate) fn pair_needs_send(owner: HWND, index: usize, generation: u64, json: &str) -> bool {
    with(owner, |rt| rt.pair_sent.get(index).and_then(Clone::clone) != Some((generation, json.to_owned()))).unwrap_or(false)
}

pub(crate) fn pair_sent(owner: HWND, index: usize, generation: u64, json: &str) {
    let _ = with(owner, |rt| {
        if let Some(slot) = rt.pair_sent.get_mut(index) {
            *slot = Some((generation, json.to_owned()));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_rect_is_never_empty() {
        assert_eq!(rect_size(RECT { left: 10, top: 20, right: 10, bottom: 15 }), RECT { left: 0, top: 0, right: 1, bottom: 1 });
        assert_eq!(rect_size(RECT { left: 10, top: 20, right: 410, bottom: 320 }), RECT { left: 0, top: 0, right: 400, bottom: 300 });
    }

    #[test]
    fn tab_menu_ids_do_not_collide_with_the_tab_menu() {
        // show_tab_context_menu uses ids below 100.
        for id in [TAB_SPLIT, TAB_SEPARATE, TAB_SWAP] {
            assert!(id >= 0x100);
        }
    }
}
