//! Split view chrome around the two panes (see docs/design/split-view.md):
//!
//! - one HTML host (`View::Split`) in a Bowser-owned child window that shows either the empty
//!   side's "choose a tab" surface (over the pane that has no page), the drop-zone card while a
//!   tab from the HTML strip is dragged over the page, or the private landing over a blank private
//!   tab outside split view (docs/design/private-tabs.md; one host, so no extra WebView2);
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
            RegisterClassW, SetWindowPos, ShowWindow, HWND_TOP, SM_CXDRAG, SM_CYDRAG, SWP_NOACTIVATE,
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
        protocol::{Command, PanelType, SplitCommand},
        split::{self as wire, PickClosed, PickTab},
    },
    split_view::{self, Divider, DropZone, Orientation, Pane},
    tabs::TabId,
};

const CLASS: PCWSTR = w!("BowserSplitPane");

/// What the split host shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Shown {
    Hidden,
    /// The empty side: tab `tab` (no page) in `pane`, against page edge `side`, at `rect` (client
    /// coordinates).
    Picker { tab: TabId, pane: Pane, side: DropZone, rect: RECT },
    /// The drop-zone card for a dragged tab: `zone` names where it lands (an edge for a half, a
    /// corner for a quarter) and `rect` is exactly the pane it will take.
    Drop { zone: DropZone, rect: RECT },
    /// The private landing over blank private tab `tab` (no split), at the whole viewport `rect`.
    Private { tab: TabId, rect: RECT },
    // --- start page ---
    /// The start page over blank normal tab `tab` (no split), at the whole viewport `rect`
    /// (docs/design/start-page.md, `start_ui.rs`).
    Start { tab: TabId, rect: RECT },
    // --- end start page ---
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    tab: TabId,
    target: Option<DropZone>,
    /// Where this drag came onto the page and whether that edge is offered yet (`drop_intent`).
    intent: crate::drop_intent::DropIntent,
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
    /// Divider press: the client coordinate of the press along the split axis (x side by side, y
    /// stacked) and whether the pointer moved past the drag threshold since.
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
        // --- start page --- the start page's own `spage` family
        } else if let Command::Panel { kind: PanelType::StartPage, f } = command {
            super::start_ui::queue(owner, f);
        // --- end start page ---
        }
    });
    let on_failed: Rc<dyn Fn()> = Rc::new(move || {
        let owner = HWND(raw as *mut _);
        let _ = with(owner, |rt| rt.failed = true);
        super::window::split_ui_invalidate(owner);
    });
    let on_accelerator: AccelHandler = Rc::new(move |accel: Accel| super::window::html_split_accelerator(HWND(raw as *mut _), accel));
    let created = ChromeHost::create(&env, crate::chrome_host::host::ChromeHostOptions {
        parent: container,
        bounds: RECT { left: 0, top: 0, right: 1, bottom: 1 },
        view: View::Split,
        on_command,
        on_failed,
        on_accelerator: Some(on_accelerator),
    }, move |result| {
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
    let full = QUEUE.with(|queue| push_coalesced(&mut queue.borrow_mut(), owner.0 as isize, command));
    if !full {
        crate::win32::post(owner, super::WM_SPLIT_COMMAND, WPARAM(0), LPARAM(0));
    }
}

/// Adds `command` for `owner` to the queue; `true` when it was refused (the queue is full).
///
/// A drag reports `over` every frame. Only the latest one matters, so a newer `over` replaces a
/// queued one, and a `drop` or `end` supersedes them. A drag's `drop` and `end` are never refused:
/// a full queue used to swallow them while a busy UI thread fell 16 frames behind, so the drop did
/// nothing, or the preview stayed up after the drag. Pure apart from the queue it is given.
fn push_coalesced(queue: &mut Vec<(isize, SplitCommand)>, owner: isize, command: SplitCommand) -> bool {
    let is_over = |c: &SplitCommand| matches!(c, SplitCommand::DragOver { .. });
    match command {
        SplitCommand::DragOver { .. } => {
            if let Some(slot) = queue.iter_mut().find(|(id, c)| *id == owner && is_over(c)) {
                slot.1 = command;
                return false;
            }
        }
        SplitCommand::Drop { .. } | SplitCommand::DragEnd { .. } => {
            queue.retain(|(id, c)| !(*id == owner && is_over(c)));
            queue.push((owner, command));
            return false;
        }
        _ => {}
    }
    if queue.len() >= 16 {
        return true;
    }
    queue.push((owner, command));
    false
}

/// Whether a dragged tab's drop zone is showing in `owner` right now (a release there opens it).
/// The tab ghost asks when the page reports a release over the page: before the queued `drop` runs.
pub(crate) fn drop_zone_showing(owner: HWND) -> bool {
    let queued_end = QUEUE.with(|queue| queue.borrow().iter().any(|(id, c)| *id == owner.0 as isize && matches!(c, SplitCommand::DragEnd { .. })));
    !queued_end && with(owner, |rt| rt.drag.is_some_and(|drag| drag.target.is_some())).unwrap_or(false)
}

/// Runs the queued commands for `owner` (from `WM_SPLIT_COMMAND`).
pub(crate) fn run_queued(owner: HWND, app: &AppRef) {
    super::start_ui::run_queued(owner, app); // --- start page ---
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

/// Whether the private landing or the start page covers the viewport right now (the overlay then
/// frames it like a page instead of painting the opaque new-tab surface over it).
pub(crate) fn private_page_shown(owner: HWND) -> bool {
    with(owner, |rt| rt.ready && !rt.failed && matches!(rt.shown, Some(Shown::Private { .. } | Shown::Start { .. }))).unwrap_or(false)
}

/// The rectangle a dragged tab would land in, when the HTML card cannot show it (overlay fallback).
pub(crate) fn drop_fallback(owner: HWND) -> Option<RECT> {
    let (tab, zone) = with(owner, |rt| {
        let drag = rt.drag?;
        let target = drag.target?;
        let card = rt.ready && !rt.failed && matches!(rt.shown, Some(Shown::Drop { .. }));
        (!card).then_some((drag.tab, target))
    })
    .flatten()?;
    let app = super::window::popup_app(owner)?;
    drop_card(owner, &app, tab, zone).map(|(_, rect)| rect)
}

/// Where dragging `tab` onto the `zone` of the page would put it: the zone name of its new pane
/// (for the card's wording) and exactly that pane's rectangle in the planned layout.
fn drop_card(owner: HWND, app: &AppRef, tab: TabId, zone: DropZone) -> Option<(DropZone, RECT)> {
    let viewport = super::window::split_viewport(owner)?;
    let page = (viewport.left, viewport.top, viewport.right, viewport.bottom);
    let px = |dip| super::window::split_px(owner, dip);
    let (gap, min_w, min_h) = (px(crate::stacks::gap_dip()), px(split_view::MIN_PANE_DIP), px(split_view::MIN_PANE_HEIGHT_DIP));
    let (shown, (left, top, right, bottom)) = match drop_plan(app, tab, zone) {
        Some(plan) => (
            split_view::drop_target_zone(plan, tab, zone),
            split_view::drop_target_rect(plan, tab, zone, page, min_w, min_h, gap),
        ),
        None => (zone, split_view::drop_rect(page, zone, gap)),
    };
    Some((shown, RECT { left, top, right, bottom }))
}

/// What the host should show now.
fn wanted(owner: HWND, app: &AppRef) -> Shown {
    let drag = with(owner, |rt| rt.drag).flatten();
    if let Some(Drag { tab, target: Some(zone), .. }) = drag
        && let Some((zone, rect)) = drop_card(owner, app, tab, zone)
    {
        return Shown::Drop { zone, rect };
    }
    let Some(split) = App::split_view(app) else {
        // A blank private tab gets the private landing; a private tab with a page, or a blank tab
        // inside a split (the picker), never does.
        let tab = App::active_tab_id(app);
        if App::tab_private(app, tab)
            && App::tab_is_blank(app, tab)
            && let Some(rect) = super::window::split_viewport(owner)
        {
            return Shown::Private { tab, rect };
        }
        // --- start page --- a blank normal tab shows the start page (not before the window has settled)
        if !App::tab_private(app, tab)
            && App::tab_is_blank(app, tab)
            && super::start_ui::enabled(app)
            && super::start_ui::ready(owner)
            && let Some(rect) = super::window::split_viewport(owner)
        {
            return Shown::Start { tab, rect };
        }
        // --- end start page ---
        return Shown::Hidden;
    };
    // One host: the first empty pane (in tree order) gets the picker; any other empty pane keeps
    // the overlay's opaque fill until this one is filled.
    for (pane, tab) in split.panes() {
        if !App::tab_is_blank(app, tab) {
            continue;
        }
        let rect = match App::split_bounds(app) {
            // A pane hidden by the collapse rule has no rectangle.
            Some(bounds) => match bounds.get(pane) {
                Some(rect) => rect,
                None => continue,
            },
            // Too narrow for two panes: only the focused pane shows, over the whole viewport.
            None if split.focused == pane => match super::window::split_viewport(owner) {
                Some(viewport) => viewport,
                None => continue,
            },
            None => continue,
        };
        return Shown::Picker { tab, pane, side: split.zone_of(pane), rect };
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
        Shown::Picker { rect, .. } | Shown::Drop { rect, .. } | Shown::Private { rect, .. } | Shown::Start { rect, .. } => rect,
        Shown::Hidden => return,
    };
    // Theme first, then content, then show: the page never appears unthemed or stale.
    let state = super::window::split_host_state(owner, App::settings_accent(app));
    if with(owner, |rt| rt.state.as_deref() != Some(state.as_str())).unwrap_or(false) && host.post_json(&state).is_ok() {
        let _ = with(owner, |rt| rt.state = Some(state));
    }
    let content = match want {
        Shown::Picker { tab, side, .. } => {
            // A turned pair (side by side to stacked) renames the empty side, so `side` counts too.
            let same_side = matches!(shown, Some(Shown::Picker { tab: old, side: old_side, .. }) if old == tab && old_side == side);
            (!same_side || dirty).then(|| pick_content(app, side))
        }
        Shown::Drop { zone, .. } => {
            let same = matches!(shown, Some(Shown::Drop { zone: old, .. }) if old == zone);
            (!same).then(|| {
                let title = with(owner, |rt| rt.drag.map(|drag| drag.tab))
                    .flatten()
                    .and_then(|tab| App::tab_title(app, tab))
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| "New tab".to_owned());
                wire::drop_json(zone, &title)
            })
        }
        // Cheap enough for every paint: the post below is skipped while the text is unchanged.
        Shown::Private { tab, .. } => Some(wire::private_json(wire_id(tab), private_tab_count(app))),
        // --- start page --- a snapshot only when what the page shows changed (start_ui::due reads no file otherwise)
        Shown::Start { tab, .. } => {
            let appeared = !matches!(shown, Some(Shown::Start { tab: old, .. }) if old == tab);
            super::start_ui::due(owner, app, tab, appeared)
        }
        // --- end start page ---
        Shown::Hidden => None,
    };
    // --- start page --- an identical snapshot is not posted twice, and then the page is as delivered as it gets
    if matches!(want, Shown::Start { .. })
        && content.as_ref().is_some_and(|c| with(owner, |rt| rt.content.as_deref() == Some(c.as_str())).unwrap_or(false))
    {
        super::start_ui::settled(owner);
    }
    // --- end start page ---
    if let Some(content) = content
        && with(owner, |rt| rt.content.as_deref() != Some(content.as_str())).unwrap_or(false)
        && host.post_json(&content).is_ok()
    {
        let _ = with(owner, |rt| {
            rt.content = Some(content);
            rt.dirty = false;
        });
        // --- start page --- the chosen images follow the page, each in its own message
        if matches!(want, Shown::Start { .. }) {
            super::start_ui::posted(owner, &host);
        }
        // --- end start page ---
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

fn private_tab_count(app: &AppRef) -> usize {
    App::private_tab_ids(app).len()
}

fn pick_content(app: &AppRef, side: DropZone) -> String {
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
    wire::pick_json(side, &tabs, &closed)
}

/// A command from the split host. Picker commands act only while the picker is shown, on its own
/// tab. `ClosePrivate` acts only while the private landing is shown.
fn picker_command(owner: HWND, app: &AppRef, command: SplitCommand) {
    let shown = with(owner, |rt| rt.shown).flatten();
    if let Some(Shown::Private { tab, .. }) = shown {
        if command == SplitCommand::ClosePrivate && App::tab_private(app, tab) {
            App::close_private_tabs(app);
            super::window::split_ui_invalidate(owner);
        }
        return;
    }
    let Some(Shown::Picker { tab: empty, .. }) = shown else {
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
            // OB-8: a closed tab offered again, not typed.
            App::submit_with(app, &closed.url, crate::address_input::Options::picked(false));
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
        SplitCommand::DragOver { .. } | SplitCommand::Drop { .. } | SplitCommand::DragEnd { .. } | SplitCommand::ClosePrivate => {}
    }
    super::window::split_ui_invalidate(owner);
}

// ----- dragging a tab from the HTML strip onto the page -----

fn drag(owner: HWND, app: &AppRef, command: SplitCommand) {
    match command {
        SplitCommand::DragOver { id, x, y } => {
            let Some(tab) = tab_of_wire(app, id) else { return };
            let (previous, mut intent) = with(owner, |rt| rt.drag.filter(|drag| drag.tab == tab).map(|drag| (drag.target, drag.intent)))
                .flatten()
                .unwrap_or_default();
            let target = drop_target(owner, app, tab, x, y, previous, &mut intent);
            let changed = with(owner, |rt| {
                let old = rt.drag.and_then(|drag| drag.target);
                rt.drag = Some(Drag { tab, target, intent });
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
            let (Some(Drag { tab, target: Some(zone), .. }), Some(dropped)) = (drag, tab_of_wire(app, id)) else { return };
            if tab != dropped {
                return;
            }
            drop_tab(owner, app, tab, zone);
        }
        SplitCommand::DragEnd { .. } if with(owner, |rt| rt.drag.take()).flatten().is_some() => {
            super::window::split_ui_invalidate(owner);
        }
        _ => {}
    }
}

/// --- wave-1 fixes --- the `split_edge_drop` option, as the running app has it.
fn edge_drop_enabled(app: &AppRef) -> bool {
    App::settings(app).split_edge_drop
}

/// The edge zone of the page under the dragged tab (`split_view::drop_zone`, which keeps `previous`
/// near the diagonals and in the middle), or `None` when the pointer is not over the page or the
/// drop would change nothing (a half dropped where it already is).
fn drop_target(owner: HWND, app: &AppRef, tab: TabId, x: i32, y: i32, previous: Option<DropZone>, intent: &mut crate::drop_intent::DropIntent) -> Option<DropZone> {
    if !edge_drop_enabled(app) {
        return None;
    }
    let (screen, dpi) = super::window::tabs_host_screen(owner)?;
    let (sx, sy) = wire::css_to_screen((screen.left, screen.top), dpi as f32 / 96.0, x, y);
    let mut point = POINT { x: sx, y: sy };
    unsafe {
        let _ = ScreenToClient(owner, &mut point);
    }
    let viewport = super::window::split_viewport(owner)?;
    let page = (viewport.left, viewport.top, viewport.right, viewport.bottom);
    // The corners count only over a shown split (where a drop can add or take a pane).
    let corners = split_view::corners_allowed(App::split_view(app), tab);
    // Only near the page's edges, and not the edge the tab came in through until it has gone
    // deeper (`drop_intent`): pulling a tab off the strip no longer flashes a split preview.
    let zone = crate::drop_intent::intent_zone(page, point.x, point.y, previous, corners, intent)?;
    drop_plan(app, tab, zone).map(|_| zone)
}

/// What dropping `tab` on `zone` would do now (`split_view::plan_drop` with the window's state).
fn drop_plan(app: &AppRef, tab: TabId, zone: DropZone) -> Option<split_view::DropPlan> {
    let pair = App::split_view(app);
    let empty = pair.and_then(|pair| pair.tabs().find(|side| App::tab_is_blank(app, *side)));
    split_view::plan_drop(pair, empty, App::active_tab_id(app), tab, zone)
}

/// Drops `tab` on the `zone` of the page (`split_view::plan_drop`): an edge's half, or a corner's
/// quarter of the split.
fn drop_tab(owner: HWND, app: &AppRef, tab: TabId, zone: DropZone) {
    let Some(plan) = drop_plan(app, tab, zone) else { return };
    let bounds = match plan {
        split_view::DropPlan::Pair { view, .. } | split_view::DropPlan::Layout { view, .. } => super::window::split_rects_for_view(owner, view),
        split_view::DropPlan::WithEmpty { zone, .. } => bounds_for(owner, zone.orientation(), 50),
    };
    let _ = App::apply_split_drop(app, plan, bounds);
    refresh_bounds(owner, app);
    App::focus_page(app);
}

// ----- bounds and entry points -----

/// The pane rectangles a new 50/50 side-by-side split would get.
fn bounds_for_new_split(owner: HWND) -> Option<app_bounds::Bounds> {
    bounds_for(owner, Orientation::SideBySide, 50)
}

/// The pane rectangles a split in `orientation` at `ratio` would get.
fn bounds_for(owner: HWND, orientation: Orientation, ratio: u8) -> Option<app_bounds::Bounds> {
    super::window::split_rects_for_ratio(owner, orientation, ratio)
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
const TURN: usize = 5;
/// Swap the cells of half 0 or 1 (`SWAP_CROSS + half`).
const SWAP_CROSS: usize = 6;
/// Close the tab of a pane of a larger layout (`CLOSE_PANE + Pane::index`).
const CLOSE_PANE: usize = 0x10;

/// The menu labels and glyphs that name the two sides, per orientation.
struct SideWords {
    swap: &'static str,
    turn: &'static str,
    turn_glyph: &'static str,
    close_first: &'static str,
    close_second: &'static str,
    first_glyph: &'static str,
    second_glyph: &'static str,
}

fn side_words(orientation: Orientation) -> SideWords {
    match orientation {
        Orientation::SideBySide => SideWords {
            swap: "Swap sides",
            turn: "Stack top and bottom",
            turn_glyph: "split-stacked",
            close_first: "Close left tab",
            close_second: "Close right tab",
            first_glyph: "split-left",
            second_glyph: "split-right",
        },
        Orientation::Stacked => SideWords {
            swap: "Swap top and bottom",
            turn: "Show side by side",
            turn_glyph: "split",
            close_first: "Close top tab",
            close_second: "Close bottom tab",
            first_glyph: "split-top",
            second_glyph: "split-bottom",
        },
    }
}

fn arrange_entries(orientation: Orientation) -> Vec<Entry> {
    let words = side_words(orientation);
    vec![
        Entry::Item(Item::new(SEPARATE, "Separate views", Icon::None).glyph("unsplit").shortcut("Ctrl+Shift+\\")),
        Entry::Item(Item::new(SWAP, words.swap, Icon::None).glyph("swap")),
        Entry::Item(Item::new(TURN, words.turn, Icon::None).glyph(words.turn_glyph)),
        Entry::Separator,
        Entry::Item(Item::new(CLOSE_LEFT, words.close_first, Icon::Close).glyph(words.first_glyph)),
        Entry::Item(Item::new(CLOSE_RIGHT, words.close_second, Icon::Close).glyph(words.second_glyph)),
    ]
}

/// The words for where a pane sits ("left", "top-left", ...).
fn zone_word(zone: DropZone) -> &'static str {
    match zone {
        DropZone::Left => "left",
        DropZone::Right => "right",
        DropZone::Top => "top",
        DropZone::Bottom => "bottom",
        DropZone::TopLeft => "top-left",
        DropZone::TopRight => "top-right",
        DropZone::BottomLeft => "bottom-left",
        DropZone::BottomRight => "bottom-right",
    }
}

/// The glyph of a pane's place: the half glyphs, or a quarter of a 2 x 2.
pub(crate) fn zone_glyph(zone: DropZone) -> &'static str {
    match zone {
        DropZone::Left => "split-left",
        DropZone::Right => "split-right",
        DropZone::Top => "split-top",
        DropZone::Bottom => "split-bottom",
        DropZone::TopLeft => "split-top-left",
        DropZone::TopRight => "split-top-right",
        DropZone::BottomLeft => "split-bottom-left",
        DropZone::BottomRight => "split-bottom-right",
    }
}

/// The menu of a layout of three or four panes, for `divider` (`None`: the toolbar button):
/// Separate views, Swap the two sides of that divider, then Close for each pane beside it (every
/// pane for the primary divider, the crossing and the toolbar).
fn layout_entries(view: split_view::SplitView, divider: Option<Divider>) -> Vec<Entry> {
    let mut entries = vec![Entry::Item(Item::new(SEPARATE, "Separate views", Icon::None).glyph("unsplit").shortcut("Ctrl+Shift+\\"))];
    let primary_swap = if view.orientation.is_stacked() { "Swap top and bottom" } else { "Swap left and right" };
    let cross_swap = if view.orientation.is_stacked() { "Swap left and right" } else { "Swap top and bottom" };
    match divider {
        Some(Divider::Cross(half)) => entries.push(Entry::Item(Item::new(SWAP_CROSS + half, cross_swap, Icon::None).glyph("swap"))),
        _ => entries.push(Entry::Item(Item::new(SWAP, primary_swap, Icon::None).glyph("swap"))),
    }
    entries.push(Entry::Separator);
    for (pane, _) in view.panes() {
        let beside = match divider {
            Some(Divider::Cross(half)) => pane.half_index() == half,
            _ => true,
        };
        if beside {
            let zone = view.zone_of(pane);
            entries.push(Entry::Item(Item::new(CLOSE_PANE + pane.index(), format!("Close {} tab", zone_word(zone)), Icon::Close).glyph(zone_glyph(zone))));
        }
    }
    entries
}

/// The split menu at `anchor` (screen). A pair: Separate views, Swap sides, Stack top and bottom
/// (or Show side by side), Close left/top tab, Close right/bottom tab. A larger layout:
/// [`layout_entries`] for `divider`. Each pick re-checks that the same layout is still shown.
pub(crate) fn show_menu(owner: HWND, app: &AppRef, anchor: POINT, placement: menu::Placement, kind: &str, divider: Option<Divider>) {
    let Some(split) = App::split_view(app) else { return };
    let entries = if split.is_pair() { arrange_entries(split.orientation) } else { layout_entries(split, divider) };
    menu::show(
        owner,
        anchor,
        placement,
        Menu { header: None, entries, footer: "Split view".to_owned() },
        kind,
        move |app, chosen| {
            let Some(chosen) = chosen else { return };
            let Some(now) = App::split_view(app) else { return };
            if now.tabs().collect::<Vec<_>>() != split.tabs().collect::<Vec<_>>() {
                return;
            }
            match chosen.id {
                id if (SWAP_CROSS..SWAP_CROSS + 2).contains(&id) => {
                    let _ = App::swap_split_cross(app, id - SWAP_CROSS);
                }
                id if (CLOSE_PANE..CLOSE_PANE + 4).contains(&id) => {
                    if let Some(pane) = Pane::ALL.into_iter().find(|pane| pane.index() == id - CLOSE_PANE) {
                        let _ = App::close_split_pane(app, pane);
                    }
                }
                SEPARATE => {
                    let _ = App::separate_split(app);
                }
                SWAP => {
                    let _ = App::swap_split_panes(app);
                }
                TURN => {
                    let _ = App::turn_split(app, bounds_for(owner, now.orientation.other(), now.ratio));
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
    show_menu(owner, app, anchor, menu::Placement::AboveLeft, "split", None);
}

/// The menu of the divider at `client`, beside its grip: right of an upright gap at the pointer's
/// height, below a flat gap at the pointer's x.
pub(crate) fn divider_menu(owner: HWND, app: &AppRef, client: POINT) {
    let divider = super::window::split_divider_at_point(owner, client).unwrap_or(Divider::Primary);
    let Some(gap) = super::window::split_gap(owner, divider) else { return };
    let nudge = super::window::split_px(owner, 4);
    let flat = gap.right - gap.left > gap.bottom - gap.top;
    let point = if flat { POINT { x: client.x, y: gap.bottom + nudge } } else { POINT { x: gap.right + nudge, y: client.y } };
    let anchor = super::window::screen_point(owner, point);
    show_menu(owner, app, anchor, menu::Placement::BelowLeft, "split-divider", Some(divider));
}

/// The divider was pressed at client coordinate `at` along the split axis.
pub(crate) fn divider_pressed(owner: HWND, at: i32) {
    let _ = with(owner, |rt| rt.press = Some((at, false)));
}

/// The pointer moved to `at` (along the split axis) while the divider is held: true once it counts
/// as a drag.
pub(crate) fn divider_dragged(owner: HWND, at: i32) -> bool {
    let threshold = unsafe { GetSystemMetrics(SM_CXDRAG).max(GetSystemMetrics(SM_CYDRAG)) }.max(2);
    with(owner, |rt| match rt.press.as_mut() {
        Some((start, moved)) => {
            if !*moved && (at - *start).abs() >= threshold {
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
        let orientation = App::split_pair(app).map_or(Orientation::SideBySide, |(split, _)| split.orientation);
        return vec![
            Entry::Item(Item::new(TAB_SEPARATE, "Separate views", Icon::None).glyph("unsplit")),
            Entry::Item(Item::new(TAB_SWAP, side_words(orientation).swap, Icon::None).glyph("swap").disabled_if(!shown)),
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

/// The `pair` message for the window's current pair (with `panes` for three or four panes).
pub(crate) fn pair_message(app: &AppRef) -> String {
    let Some((split, shown)) = App::split_pair(app) else { return wire::pair_json(None) };
    let pair = (wire_id(split.left), wire_id(split.right), split.focused, shown, split.orientation);
    if split.is_pair() {
        return wire::pair_json(Some(pair));
    }
    let panes: Vec<(u32, DropZone)> = split.panes().map(|(pane, tab)| (wire_id(tab), split.zone_of(pane))).collect();
    wire::layout_json(pair, &panes, split.focused_zone())
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

    /// --- wave-1 fixes --- "Split by dropping a tab on the page's edge" off: no drop zone at all, before
    /// the pointer is even looked at; on, the gate lets the zone logic run.
    #[test]
    fn the_split_edge_drop_option_gates_every_drop_zone() {
        let app = crate::app::test_app();
        App::apply_live_settings(&app, &crate::settings::Settings { split_edge_drop: false, ..crate::settings::Settings::default() });
        assert!(!edge_drop_enabled(&app));
        let mut intent = crate::drop_intent::DropIntent::default();
        assert_eq!(drop_target(HWND::default(), &app, App::active_tab_id(&app), 5, 5, None, &mut intent), None);
        App::apply_live_settings(&app, &crate::settings::Settings::default());
        assert!(edge_drop_enabled(&app), "on by default");
    }

    #[test]
    fn container_rect_is_never_empty() {
        assert_eq!(rect_size(RECT { left: 10, top: 20, right: 10, bottom: 15 }), RECT { left: 0, top: 0, right: 1, bottom: 1 });
        assert_eq!(rect_size(RECT { left: 10, top: 20, right: 410, bottom: 320 }), RECT { left: 0, top: 0, right: 400, bottom: 300 });
    }

    #[test]
    fn split_menus_name_the_sides_of_either_orientation() {
        let labels = |orientation| {
            arrange_entries(orientation)
                .into_iter()
                .filter_map(|entry| match entry {
                    Entry::Item(item) => Some(item.label.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            labels(Orientation::SideBySide),
            ["Separate views", "Swap sides", "Stack top and bottom", "Close left tab", "Close right tab"]
        );
        assert_eq!(
            labels(Orientation::Stacked),
            ["Separate views", "Swap top and bottom", "Show side by side", "Close top tab", "Close bottom tab"]
        );
    }

    #[test]
    fn tab_menu_ids_do_not_collide_with_the_tab_menu() {
        // show_tab_context_menu uses ids below 100.
        for id in [TAB_SPLIT, TAB_SEPARATE, TAB_SWAP] {
            assert!(id >= 0x100);
        }
    }
}

#[cfg(test)]
mod queue_tests {
    use super::*;

    fn over(x: i32) -> SplitCommand {
        SplitCommand::DragOver { id: 3, x, y: 400 }
    }

    #[test]
    fn a_drop_after_a_backlog_of_moves_is_never_refused() {
        // Replay: the UI thread falls behind while a tab is dragged over the page (60 `over` a
        // second), then the tab is released. The old queue refused everything past 16, the drop
        // included.
        let mut queue = Vec::new();
        for x in 0..40 {
            assert!(!push_coalesced(&mut queue, 1, over(x)));
        }
        assert_eq!(queue.len(), 1, "moves coalesce to the latest");
        assert_eq!(queue[0].1, over(39));
        assert!(!push_coalesced(&mut queue, 1, SplitCommand::Drop { id: 3 }));
        assert_eq!(queue, vec![(1, SplitCommand::Drop { id: 3 })], "the drop supersedes the queued move");
    }

    #[test]
    fn end_is_kept_even_when_the_queue_is_full_and_other_windows_keep_their_moves() {
        let mut queue: Vec<(isize, SplitCommand)> = (0..16).map(|i| (100 + i, SplitCommand::Focus)).collect();
        assert!(push_coalesced(&mut queue, 1, SplitCommand::Close), "picker commands are still capped");
        assert!(!push_coalesced(&mut queue, 1, SplitCommand::DragEnd { id: 3 }));
        assert_eq!(queue.last(), Some(&(1, SplitCommand::DragEnd { id: 3 })));
        let mut two = vec![(2, over(5))];
        assert!(!push_coalesced(&mut two, 1, over(6)));
        assert!(!push_coalesced(&mut two, 1, SplitCommand::DragEnd { id: 3 }));
        assert_eq!(two, vec![(2, over(5)), (1, SplitCommand::DragEnd { id: 3 })]);
    }
}
