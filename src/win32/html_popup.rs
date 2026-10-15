//! Reusable HTML menu popup owned by one browser window.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            CombineRgn, CreateRoundRectRgn, DeleteObject, GetMonitorInfoW, HGDIOBJ, HRGN,
            MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, RGN_OR, SetWindowRgn,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
            Input::KeyboardAndMouse::{GetFocus, SetFocus},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow, HWND_TOP,
                IsWindow, RegisterClassW, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, SetForegroundWindow, SetWindowPos,
                SWP_NOACTIVATE, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_ERASEBKGND, WM_NCCREATE, WM_NCDESTROY,
                WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, w},
};

use crate::{
    app::AppRef,
    chrome_host::{
        self,
        host::{Accel, ChromeEnv, ChromeHost, View},
        json::{Json, Object},
        protocol::{Command, MenuCards, MAX_ACT_CHOICE},
    },
    win32::menu::{self, Chosen, Entry, Icon, Menu},
};

const CLASS_NAME: PCWSTR = w!("BowserHtmlPopup");
const MAX_ITEMS: usize = 128;
const MAX_TOTAL_ITEMS: usize = 256;
const MAX_LABEL: usize = 128;
const MAX_SHORTCUT: usize = 48;
const MAX_FOOTER: usize = 256;
/// Distinct favicons and their total data: URI size in one menu.
const MAX_FAVICONS: usize = 48;
const MAX_FAVICON_TOTAL: usize = 512 * 1024;
const REOPEN_GUARD: Duration = Duration::from_millis(300);
const FALLBACK_DPI: u32 = 96;
/// Largest page message (palette, site info) posted to the popup page. The page's own inbound cap
/// is 256 KiB; a larger page is refused and the caller falls back to its native surface.
pub(crate) const MAX_PAGE_BYTES: usize = 240 * 1024;
/// The command palette's top edge sits this far down the owner window (reference spec: 12%).
const TOP_CENTER_PERCENT: i32 = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placement {
    AboveLeft,
    BelowLeft,
    /// Below the anchor with the popup's right edge at the anchor (the downloads bubble under its
    /// toolbar button), flipping above it when there is no room below.
    BelowRight,
}

pub(crate) type ResultCallback = Box<dyn FnOnce(&AppRef, Option<Chosen>)>;

// --- popup surfaces (lane PP) ---
/// Where a popup page is placed once the page reports its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Anchor {
    /// A menu or flyout at a screen point (`menu::place` / `menu::place_below`).
    Point(POINT, Placement),
    /// Centred on a window rectangle (screen coordinates), its top edge 12% down the window:
    /// the command palette.
    TopCenter(RECT),
}

impl Anchor {
    /// The screen point whose monitor supplies the DPI and work area.
    fn monitor_point(self) -> POINT {
        match self {
            Self::Point(point, _) => point,
            Self::TopCenter(window) => POINT {
                x: (window.left + window.right) / 2,
                y: window.top + (window.bottom - window.top) * TOP_CENTER_PERCENT / 100,
            },
        }
    }
}

/// A non-menu popup page (`{t:"palette"}`, `{t:"siteinfo"}`): its wire type, the fields Rust
/// writes after `v`, `t`, `tok` and `kind`, the ids a `pick` may name, and the ids an `act` may
/// name with the largest choice each accepts. Anything outside those sets is rejected.
pub(crate) struct Page {
    t: &'static str,
    write: Box<dyn FnOnce(&mut Object<'_>)>,
    picks: HashSet<u32>,
    acts: HashMap<u32, u8>,
    compose: Option<ComposeHandler>,
}

impl Page {
    pub(crate) fn new(t: &'static str, write: impl FnOnce(&mut Object<'_>) + 'static) -> Self {
        Self { t, write: Box::new(write), picks: HashSet::new(), acts: HashMap::new(), compose: None }
    }
    /// Lets the page submit text (`compose`: a name and, for web apps, an address). Without a
    /// handler the popup accepts no `compose` at all.
    pub(crate) fn compose(&mut self, handler: ComposeHandler) {
        self.compose = Some(handler);
    }
    /// Lets a `pick` name `id` (it closes the popup and runs the result callback).
    pub(crate) fn pick(&mut self, id: u32) {
        self.picks.insert(id);
    }
    /// Lets an `act` name `id` with a choice from 0 to `max` (the popup stays open).
    pub(crate) fn act(&mut self, id: u32, max: u8) {
        self.acts.insert(id, max.min(MAX_ACT_CHOICE));
    }
    /// The message this page posts, for tests and fixtures.
    #[cfg(test)]
    pub(crate) fn debug_json(page: Self, tok: u32) -> String {
        let kind = page.t;
        page_json(tok, if kind == "siteinfo" { "site-info" } else { kind }, page).map(|built| built.0).unwrap_or_default()
    }
}

/// What an in-place action did. `Refresh` re-posts the page under the same token.
pub(crate) enum ActOutcome {
    Refresh(Page),
    Close,
    Keep,
}

pub(crate) type ActHandler = Rc<dyn Fn(&AppRef, u32, u8) -> ActOutcome>;

/// Runs a composer's submit: the trimmed-by-the-page name and optional address. `Refresh` re-posts
/// the page (with Rust's error, or `done` once the change is made); the popup closes only when the
/// page then dismisses itself.
pub(crate) type ComposeHandler = Rc<dyn Fn(&AppRef, &str, Option<&str>) -> ActOutcome>;

type Built = (String, HashSet<u32>, HashMap<u32, u8>);

fn page_json(tok: u32, kind: &str, page: Page) -> Option<Built> {
    if kind.is_empty() || kind.encode_utf16().count() > 32 {
        return None;
    }
    let Page { t, write, picks, acts, .. } = page;
    let json = Json::object(|out| {
        out.int("v", 1);
        out.string("t", t);
        out.uint("tok", u64::from(tok));
        out.string("kind", kind);
        write(out);
    });
    (json.len() <= MAX_PAGE_BYTES).then_some((json, picks, acts))
}

fn act_allowed(active: bool, current: u32, requested: u32, id: u32, n: u8, acts: &HashMap<u32, u8>) -> bool {
    popup_token_matches(active, current, requested) && acts.get(&id).is_some_and(|max| n <= *max)
}
// --- end popup surfaces ---

/// Why `show_menu` did not take a menu. The callback is handed back either way.
pub(crate) enum ShowRefused {
    /// No ready popup host, HTML chrome off, or a menu the HTML view cannot carry: the caller runs
    /// the native menu instead.
    Unavailable(ResultCallback),
    /// The same kind of menu closed under 300 ms ago (the click that dismissed it is reopening it):
    /// the caller reports a dismissal and shows nothing.
    Guarded(ResultCallback),
}

/// Active tab and workspace when a menu opened. A change dismisses the menu.
pub(crate) type MenuContext = (u32, u32);

struct Popup {
    owner: HWND,
    window: HWND,
    host: ChromeHost,
    token: u32,
    next_token: u32,
    dismiss: DismissState,
    prior_focus: HWND,
    callback: Option<ResultCallback>,
    enabled_ids: HashSet<u32>,
    /// In-place actions the open page accepts (`act`), id -> largest choice.
    acts: HashMap<u32, u8>,
    on_act: Option<ActHandler>,
    /// A composer page's submit handler, and the text of a submit waiting in the deferred queue.
    on_compose: Option<ComposeHandler>,
    pending_compose: Option<(String, Option<String>)>,
    at: Anchor,
    /// The page has been placed and shown; a later `size` only re-bounds it, without refocusing.
    shown: bool,
    dpi: u32,
    work: RECT,
    bounds: RECT,
    /// Card rectangles from the last `size`, so a later frame can move the clip without moving the window.
    cards: Option<MenuCards>,
    context: Option<MenuContext>,
    /// Shown without taking the keyboard (the downloads bubble opened by a download starting): the
    /// window is shown with `SW_SHOWNOACTIVATE` and neither it nor its page is focused. Clicking into
    /// it activates it like any popup, and from then on it closes when it loses activation.
    passive: bool,
}

thread_local! { static POPUPS: RefCell<HashMap<isize, Popup>> = RefCell::new(HashMap::new()); }
// Windows whose popup page has said `ready`: before that a posted menu would be lost.
thread_local! { static PAGE_READY: RefCell<HashSet<isize>> = RefCell::new(HashSet::new()); }
thread_local! { static CLASS_REGISTERED: RefCell<bool> = const { RefCell::new(false) }; }

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DismissState {
    active: bool,
    kind: String,
    closed_kind: String,
    closed_at: Option<Instant>,
}

impl Default for DismissState {
    fn default() -> Self {
        Self {
            active: false,
            kind: String::new(),
            closed_kind: String::new(),
            closed_at: None,
        }
    }
}

impl DismissState {
    pub(crate) fn can_open(&self, kind: &str, now: Instant) -> bool {
        !self.active
            && !(self.closed_kind == kind
                && self
                    .closed_at
                    .is_some_and(|at| now.saturating_duration_since(at) < REOPEN_GUARD))
    }
    pub(crate) fn open(&mut self, kind: &str, now: Instant) -> bool {
        if !self.can_open(kind, now) {
            return false;
        }
        self.active = true;
        self.kind = kind.to_owned();
        self.closed_at = None;
        true
    }
    pub(crate) fn close(&mut self, now: Instant) -> bool {
        if !self.active {
            return false;
        }
        self.active = false;
        self.closed_kind.clone_from(&self.kind);
        self.closed_at = Some(now);
        true
    }
    pub(crate) fn is_active(&self) -> bool {
        self.active
    }
    /// Lifts the reopen guard after a pick: a command may chain straight into another menu of the
    /// same kind (the site info panel reopens after each choice). Only dismissals are guarded.
    pub(crate) fn forgive(&mut self) {
        self.closed_at = None;
    }
}

/// Whether a menu opened in `opened` should close now that the window shows `now`.
pub(crate) fn context_changed(opened: Option<MenuContext>, now: MenuContext) -> bool {
    opened.is_some_and(|opened| opened != now)
}

pub(crate) fn prewarm(owner: HWND, env: ChromeEnv) {
    if !chrome_host::html_ui_enabled()
        || owner.is_invalid()
        || POPUPS.with(|p| p.borrow().contains_key(&(owner.0 as isize)))
        || PENDING.with(|p| p.borrow().contains_key(&(owner.0 as isize)))
    {
        return;
    }
    if register_class().is_err() {
        return;
    }
    let instance = unsafe { GetModuleHandleW(None).unwrap_or_default() };
    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS_NAME,
            w!(""),
            WS_POPUP,
            0,
            0,
            800,
            800,
            Some(owner),
            None,
            Some(instance.into()),
            None,
        )
    };
    let Ok(window) = window else {
        return;
    };
    // The same sheet of glass as the rail flyout: pixels the page leaves transparent (the gap
    // between the two menu cards, and the room a card crosses while it slides) show the page
    // behind, instead of a black window fill.
    let margins = [-1_i32; 4];
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(window, margins.as_ptr());
    }
    super::window::perf_event(owner, "html_popup_prewarm");
    let raw = owner.0 as isize;
    let command = Rc::new(move |command| popup_command(HWND(raw as *mut _), command));
    let failed = Rc::new(move || {
        crate::win32::post(
            owner,
            super::window::WM_HTML_POPUP_FAILED,
            WPARAM(0),
            LPARAM(0),
        );
    });
    let host_result = Rc::new(RefCell::new(None));
    let result_slot = host_result.clone();
    let on_ready = move |result| {
        *result_slot.borrow_mut() = Some(result);
    };
    let created = ChromeHost::create(
        &env,
        window,
        RECT {
            left: 0,
            top: 0,
            right: 800,
            bottom: 800,
        },
        View::Popup,
        command,
        failed,
        Some(Rc::new(move |accel: Accel| popup_accelerator(owner, accel))),
        on_ready,
    );
    if created.is_err() {
        unsafe {
            let _ = DestroyWindow(window);
        }
        return;
    }
    // WebView2 creation is asynchronous. The ready callback records into this slot, then the
    // WebMessageReceived path takes ownership when it first receives the page's `ready` command.
    let pending = PendingHost {
        owner,
        window,
        slot: host_result,
    };
    PENDING.with(|p| {
        p.borrow_mut().insert(owner.0 as isize, pending);
    });
}

struct PendingHost {
    owner: HWND,
    window: HWND,
    slot: Rc<RefCell<Option<windows::core::Result<ChromeHost>>>>,
}
thread_local! { static PENDING: RefCell<HashMap<isize, PendingHost>> = RefCell::new(HashMap::new()); }

fn adopt_ready(owner: HWND) {
    let key = owner.0 as isize;
    let pending = PENDING.with(|p| p.borrow_mut().remove(&key));
    let Some(pending) = pending else {
        return;
    };
    if let Some(Ok(host)) = pending.slot.borrow_mut().take() {
        let (dpi, work) = monitor_data(POINT { x: 0, y: 0 });
        POPUPS.with(|p| {
            p.borrow_mut().insert(
                key,
                Popup {
                    owner: pending.owner,
                    window: pending.window,
                    host,
                    token: 0,
                    next_token: 1,
                    dismiss: DismissState::default(),
                    prior_focus: HWND::default(),
                    callback: None,
                    enabled_ids: HashSet::new(),
                    acts: HashMap::new(),
                    on_act: None,
                    on_compose: None,
                    pending_compose: None,
                    at: Anchor::Point(POINT::default(), Placement::BelowLeft),
                    shown: false,
                    dpi,
                    work,
                    bounds: RECT::default(),
                    cards: None,
                    context: None,
                    passive: false,
                },
            );
        });
        let _ = POPUPS.with(|p| p.borrow().get(&key).map(|p| p.host.set_visible(false)));
        let _ = POPUPS.with(|p| p.borrow().get(&key).map(|p| p.host.try_suspend()));
    } else {
        PENDING.with(|p| {
            p.borrow_mut().insert(key, pending);
        });
    }
}

/// Posts `menu` to the popup page. On success the page measures itself and replies `size`; the
/// popup then shows, and the callback later runs from `DeferredChromeCommand::MenuResult`. On
/// refusal the callback comes back so the caller can run the native menu or report a dismissal.
/// The caller closes any active menu first (`cancel`).
pub(crate) fn show_menu(
    owner: HWND,
    anchor: POINT,
    placement: Placement,
    menu: &Menu,
    kind: &str,
    context: Option<MenuContext>,
    on_result: ResultCallback,
) -> Result<(), ShowRefused> {
    open(
        owner,
        Anchor::Point(anchor, placement),
        kind,
        context,
        |token| {
            let mut ids = HashSet::new();
            menu_json(token, kind, menu, &mut ids).map(|json| (json, ids, HashMap::new()))
        },
        on_result,
        None,
        None,
        false,
    )
}

// --- popup surfaces (lane PP) ---
/// Posts a non-menu page (command palette, site info). Same contract as `show_menu`: on success
/// the page measures itself and the popup shows at `at`; a `pick` runs `on_result` from the
/// deferred queue, and an `act` runs `on_act` (popup stays open). On refusal the caller shows its
/// native surface (or nothing, when guarded).
pub(crate) fn show_page(
    owner: HWND,
    at: Anchor,
    kind: &str,
    context: Option<MenuContext>,
    mut page: Page,
    on_result: ResultCallback,
    on_act: Option<ActHandler>,
) -> Result<(), ShowRefused> {
    let on_compose = page.compose.take();
    open(owner, at, kind, context, |token| page_json(token, kind, page), on_result, on_act, on_compose, false)
}

// --- downloads (docs/design/downloads.md) ---
/// Posts the downloads bubble. `build` receives the popup token and returns the whole message
/// (`{v:1,t:"downloads",tok,...}`, which has no `kind` key, so it does not go through [`Page`]); it is
/// refused when it is over [`MAX_PAGE_BYTES`]. The bubble accepts no `pick` and no `act`: its row
/// actions are `dlact` messages that `downloads_ui` checks against the live list. `passive`: show
/// it without taking the keyboard (opened by a download starting, not by the user).
pub(crate) fn show_downloads(
    owner: HWND,
    at: Anchor,
    context: Option<MenuContext>,
    passive: bool,
    build: impl FnOnce(u32) -> String,
    on_result: ResultCallback,
) -> Result<(), ShowRefused> {
    open(
        owner,
        at,
        "downloads",
        context,
        |token| {
            let json = build(token);
            (json.len() <= MAX_PAGE_BYTES).then_some((json, HashSet::new(), HashMap::new()))
        },
        on_result,
        None,
        None,
        passive,
    )
}

/// Posts a fresh message to the open page under the same token (the bubble's live progress). False
/// when `token` is not the open page, or the message is too large or was refused.
pub(crate) fn repost(owner: HWND, token: u32, json: &str) -> bool {
    if json.len() > MAX_PAGE_BYTES {
        return false;
    }
    let host = POPUPS.with(|p| {
        p.borrow()
            .get(&(owner.0 as isize))
            .filter(|p| popup_token_matches(p.dismiss.is_active(), p.token, token))
            .map(|p| p.host.clone())
    });
    host.is_some_and(|host| host.post_json(json).is_ok())
}
// --- end downloads ---

/// Whether a popup page could be shown now (host adopted and page ready). Callers that build an
/// expensive page check this first; it also starts creating the host when it does not exist yet.
pub(crate) fn page_ready(owner: HWND) -> bool {
    let key = owner.0 as isize;
    let ready = PAGE_READY.with(|ready| ready.borrow().contains(&key))
        && POPUPS.with(|p| p.borrow().contains_key(&key));
    if !ready {
        super::window::request_popup_prewarm(owner);
    }
    ready
}
// --- end popup surfaces ---

#[allow(clippy::too_many_arguments)]
fn open(
    owner: HWND,
    at: Anchor,
    kind: &str,
    context: Option<MenuContext>,
    build: impl FnOnce(u32) -> Option<Built>,
    on_result: ResultCallback,
    on_act: Option<ActHandler>,
    on_compose: Option<ComposeHandler>,
    passive: bool,
) -> Result<(), ShowRefused> {
    if !chrome_host::html_ui_enabled() || kind.is_empty() || kind.len() > 32 {
        return Err(ShowRefused::Unavailable(on_result));
    }
    // The host is adopted (and suspended while hidden) only once its page has said `ready`:
    // adopting it earlier would suspend the page before it loads, and a menu posted then is lost.
    // --- hover layer ---
    super::html_hover::cancel(owner);
    // --- end hover layer ---
    let key = owner.0 as isize;
    if !PAGE_READY.with(|ready| ready.borrow().contains(&key)) {
        super::window::perf_event(owner, "html_popup_fallback");
        super::window::request_popup_prewarm(owner);
        return Err(ShowRefused::Unavailable(on_result));
    }
    let mut on_result = Some(on_result);
    let popup_data = POPUPS.with(|p| {
        let mut all = p.borrow_mut();
        let popup = all.get_mut(&key).ok_or(false)?;
        let now = Instant::now();
        if popup.dismiss.is_active() {
            return Err(false);
        }
        if !popup.dismiss.can_open(kind, now) {
            return Err(true);
        }
        let token = popup.next_token;
        let (json, ids, acts) = build(token).ok_or(false)?;
        if !popup.dismiss.open(kind, now) {
            return Err(true);
        }
        popup.token = token;
        popup.next_token = popup.next_token.wrapping_add(1).max(1);
        popup.enabled_ids = ids;
        popup.acts = acts;
        popup.on_act = on_act;
        popup.on_compose = on_compose;
        popup.pending_compose = None;
        popup.at = at;
        popup.shown = false;
        popup.passive = passive;
        popup.context = context;
        (popup.dpi, popup.work) = monitor_data(at.monitor_point());
        popup.callback = on_result.take();
        popup.prior_focus = unsafe { GetFocus() };
        Ok((popup.host.clone(), json))
    });
    let (host, json) = match popup_data {
        Ok(data) => data,
        Err(guarded) => {
            let callback = on_result.take().expect("callback kept on refusal");
            return Err(if guarded {
                ShowRefused::Guarded(callback)
            } else {
                ShowRefused::Unavailable(callback)
            });
        }
    };
    let posted = host.resume().is_ok() && {
        let _ = host.set_visible(true);
        if let Some(state) = crate::win32::window::popup_state_json(owner) {
            let _ = host.post_json(&state);
        }
        host.post_json(&json).is_ok()
    };
    if !posted {
        let callback = take_callback(owner);
        close_now(owner);
        forgive(owner);
        return Err(ShowRefused::Unavailable(
            callback.unwrap_or_else(|| Box::new(|_, _| {})),
        ));
    }
    Ok(())
}

fn take_callback(owner: HWND) -> Option<ResultCallback> {
    POPUPS.with(|p| {
        p.borrow_mut()
            .get_mut(&(owner.0 as isize))
            .and_then(|popup| popup.callback.take())
    })
}

fn forgive(owner: HWND) {
    POPUPS.with(|p| {
        if let Some(popup) = p.borrow_mut().get_mut(&(owner.0 as isize)) {
            popup.dismiss.forgive();
        }
    });
}

/// Whether an HTML menu is open (or waiting for its size) on this window.
pub(crate) fn is_active(owner: HWND) -> bool {
    POPUPS.with(|p| {
        p.borrow()
            .get(&(owner.0 as isize))
            .is_some_and(|popup| popup.dismiss.is_active())
    })
}

/// Closes the open menu now and runs its callback as a dismissal. For a new menu that replaces it;
/// runs on the UI thread outside WebView2's callbacks, like every menu entry point.
pub(crate) fn cancel(owner: HWND, app: &AppRef) {
    let token = POPUPS.with(|p| {
        p.borrow()
            .get(&(owner.0 as isize))
            .filter(|popup| popup.dismiss.is_active())
            .map(|popup| popup.token)
    });
    if let Some(token) = token {
        run_result(owner, token, None, app);
    }
}

/// Dismisses the open menu when the active tab or workspace has changed since it opened.
pub(crate) fn note_context(owner: HWND, now: MenuContext) {
    let stale = POPUPS.with(|p| {
        p.borrow()
            .get(&(owner.0 as isize))
            .is_some_and(|popup| popup.dismiss.is_active() && context_changed(popup.context, now))
    });
    if stale {
        close(owner);
    }
}

pub(crate) fn close(owner: HWND) {
    let token = POPUPS.with(|p| {
        p.borrow()
            .get(&(owner.0 as isize))
            .filter(|popup| popup.dismiss.is_active())
            .map(|popup| popup.token)
    });
    if let Some(token) = token {
        crate::win32::window::defer_popup_result(owner, token, None);
    }
}

fn close_now(owner: HWND) {
    let key = owner.0 as isize;
    let state = POPUPS.with(|p| {
        p.borrow_mut().get_mut(&key).map(|popup| {
            let was_active = popup.dismiss.close(Instant::now());
            popup.callback = None;
            popup.on_act = None;
            popup.on_compose = None;
            popup.pending_compose = None;
            popup.acts.clear();
            popup.shown = false;
            popup.passive = false;
            (
                popup.window,
                popup.host.clone(),
                popup.prior_focus,
                was_active,
            )
        })
    });
    let Some((window, host, focus, was_active)) = state else {
        return;
    };
    if !was_active {
        return;
    }
    unsafe {
        let _ = ShowWindow(window, SW_HIDE);
    }
    let _ = host.set_visible(false);
    let _ = host.try_suspend();
    let foreground = unsafe { GetForegroundWindow() };
    if (foreground == window || foreground == owner) && unsafe { IsWindow(Some(focus)).as_bool() } {
        unsafe {
            let _ = SetForegroundWindow(owner);
            let _ = SetFocus(Some(focus));
        }
    }
}

pub(crate) fn shutdown(owner: HWND) {
    close_now(owner);
    let key = owner.0 as isize;
    PAGE_READY.with(|ready| ready.borrow_mut().remove(&key));
    if let Some(popup) = POPUPS.with(|p| p.borrow_mut().remove(&key)) {
        popup.host.close();
        unsafe {
            let _ = DestroyWindow(popup.window);
        }
    }
    if let Some(pending) = PENDING.with(|p| p.borrow_mut().remove(&key)) {
        if let Some(Ok(host)) = pending.slot.borrow_mut().take() {
            host.close();
        }
        unsafe {
            let _ = DestroyWindow(pending.window);
        }
    }
}

fn popup_command(owner: HWND, command: Command) {
    match command {
        Command::Ready => {
            if !PAGE_READY.with(|ready| ready.borrow().contains(&(owner.0 as isize))) {
                super::window::perf_event(owner, "html_popup_ready");
            }
            adopt_ready(owner);
            PAGE_READY.with(|ready| ready.borrow_mut().insert(owner.0 as isize));
        }
        command @ (Command::PopupSize { .. }
        | Command::PopupPick { .. }
        | Command::PopupDismiss { .. }
        | Command::PopupAct { .. }
        | Command::DownloadsAct { .. }
        | Command::PopupCompose { .. }) => crate::win32::window::html_popup_command(owner, command),
        _ => {}
    }
}

pub(super) fn valid_token(owner: HWND, token: u32) -> bool {
    POPUPS.with(|p| {
        p.borrow()
            .get(&(owner.0 as isize))
            .is_some_and(|p| popup_token_matches(p.dismiss.is_active(), p.token, token))
    })
}
fn popup_token_matches(active: bool, current: u32, requested: u32) -> bool {
    active && current == requested
}
fn popup_pick_allowed(
    active: bool,
    current: u32,
    requested: u32,
    id: u32,
    enabled_ids: &HashSet<u32>,
) -> bool {
    popup_token_matches(active, current, requested) && enabled_ids.contains(&id)
}
fn popup_accelerator(owner: HWND, accel: Accel) -> bool {
    if !accel.key_down || accel.vk != 0x1B {
        return false;
    }
    // A composer plays its own 100 ms exit on Escape and then dismisses itself.
    let tok = POPUPS.with(|p| {
        p.borrow()
            .get(&(owner.0 as isize))
            .filter(|p| p.dismiss.is_active() && p.on_compose.is_none())
            .map(|p| p.token)
    });
    if let Some(tok) = tok {
        crate::win32::window::defer_popup_result(owner, tok, None);
        true
    } else {
        false
    }
}
pub(super) fn valid_pick(owner: HWND, token: u32, id: u32) -> bool {
    POPUPS.with(|p| {
        p.borrow().get(&(owner.0 as isize)).is_some_and(|p| {
            popup_pick_allowed(p.dismiss.is_active(), p.token, token, id, &p.enabled_ids)
        })
    })
}

// --- popup surfaces (lane PP) ---
pub(super) fn valid_act(owner: HWND, token: u32, id: u32, n: u8) -> bool {
    POPUPS.with(|p| {
        p.borrow().get(&(owner.0 as isize)).is_some_and(|p| {
            act_allowed(p.dismiss.is_active(), p.token, token, id, n, &p.acts)
        })
    })
}

/// Runs an in-place action from the deferred queue. The handler may pump messages (site info asks
/// the browser for cookies), so the token is checked again before the refreshed page is posted.
pub(crate) fn run_act(owner: HWND, token: u32, id: u32, n: u8, app: &AppRef) {
    if !valid_act(owner, token, id, n) {
        return;
    }
    let Some(handler) = POPUPS.with(|p| p.borrow().get(&(owner.0 as isize)).and_then(|p| p.on_act.clone())) else {
        return;
    };
    apply_outcome(owner, token, handler(app, id, n), app);
}

/// Whether `token` names the open composer page, which has no submit waiting yet.
pub(super) fn valid_compose(owner: HWND, token: u32) -> bool {
    POPUPS.with(|p| {
        p.borrow().get(&(owner.0 as isize)).is_some_and(|p| {
            popup_token_matches(p.dismiss.is_active(), p.token, token)
                && p.on_compose.is_some()
                && p.pending_compose.is_none()
        })
    })
}

/// Holds a composer's text until the deferred queue runs it (the deferred command is `Copy`).
/// False when `token` is not the open composer, or a submit is already waiting.
pub(super) fn stash_compose(owner: HWND, token: u32, name: String, url: Option<String>) -> bool {
    if !valid_compose(owner, token) {
        return false;
    }
    POPUPS.with(|p| {
        p.borrow_mut().get_mut(&(owner.0 as isize)).map(|popup| popup.pending_compose = Some((name, url)))
    }).is_some()
}

/// Runs a composer's stashed submit from the deferred queue. The handler validates the text again
/// and answers with the page to show: Rust's error, or `done` after the change is made.
pub(crate) fn run_compose(owner: HWND, token: u32, app: &AppRef) {
    let taken = POPUPS.with(|p| {
        p.borrow_mut().get_mut(&(owner.0 as isize)).and_then(|popup| {
            if !popup_token_matches(popup.dismiss.is_active(), popup.token, token) {
                return None;
            }
            Some((popup.on_compose.clone()?, popup.pending_compose.take()?))
        })
    });
    let Some((handler, (name, url))) = taken else {
        return;
    };
    apply_outcome(owner, token, handler(app, &name, url.as_deref()), app);
}

fn apply_outcome(owner: HWND, token: u32, outcome: ActOutcome, app: &AppRef) {
    match outcome {
        ActOutcome::Keep => {}
        ActOutcome::Close => run_result(owner, token, None, app),
        ActOutcome::Refresh(page) => {
            let key = owner.0 as isize;
            let Some(kind) = POPUPS.with(|p| {
                p.borrow()
                    .get(&key)
                    .filter(|p| popup_token_matches(p.dismiss.is_active(), p.token, token))
                    .map(|p| p.dismiss.kind.clone())
            }) else {
                return;
            };
            let Some((json, picks, acts)) = page_json(token, &kind, page) else {
                close(owner);
                return;
            };
            let host = POPUPS.with(|p| {
                p.borrow_mut().get_mut(&key).map(|popup| {
                    popup.enabled_ids = picks;
                    popup.acts = acts;
                    popup.host.clone()
                })
            });
            if let Some(host) = host {
                if host.post_json(&json).is_err() {
                    close(owner);
                }
            }
        }
    }
}
// --- end popup surfaces ---

pub(crate) fn run_result(owner: HWND, token: u32, id: Option<u32>, app: &AppRef) {
    if !valid_token(owner, token) || id.is_some_and(|id| !valid_pick(owner, token, id)) {
        return;
    }
    let Some((callback, row)) = POPUPS.with(|p| {
        p.borrow_mut()
            .get_mut(&(owner.0 as isize))
            .and_then(|popup| {
                if popup.token != token {
                    return None;
                }
                let callback = popup.callback.take()?;
                let chosen = id.map(|id| Chosen {
                    id: id as usize,
                    row: RECT::default(),
                });
                Some((callback, chosen))
            })
    }) else {
        return;
    };
    close_now(owner);
    if row.is_some() {
        forgive(owner);
    }
    callback(app, row);
}

pub(super) fn show_sized(owner: HWND, token: u32, w: u16, h: u16, cards: Option<MenuCards>) {
    let key = owner.0 as isize;
    let Some((window, host, at, dpi, work, shown, previous, previous_cards, passive)) = POPUPS.with(|p| {
        p.borrow().get(&key).and_then(|popup| {
            (popup.dismiss.is_active() && popup.token == token).then(|| {
                (
                    popup.window,
                    popup.host.clone(),
                    popup.at,
                    popup.dpi,
                    popup.work,
                    popup.shown,
                    popup.bounds,
                    popup.cards,
                    popup.passive,
                )
            })
        })
    }) else {
        return;
    };
    // A menu with submenus reports its cards: Rust anchors the parent card, and the page around it
    // (room for a child card on both sides) hangs off it, so a submenu only ever moves the clip.
    let (bounds, room) = match (at, cards) {
        (Anchor::Point(..), Some(cards)) => {
            let parent = page_bounds(at, i32::from(cards.parent_w), i32::from(cards.parent_h), dpi, work);
            (flyout_bounds(parent, cards, i32::from(w), dpi, work), Some(flyout_room(parent, work, dpi)))
        }
        _ => {
            let bounds = page_bounds(at, i32::from(w), i32::from(h), dpi, work);
            (if shown && matches!(at, Anchor::Point(..)) { keep_right_edge(previous, bounds, work) } else { bounds }, None)
        }
    };
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    // Matches the page's 8px overlay radius (--bw-r-overlay).
    let radius = ((8 * dpi + 48) / 96).max(1) as i32;
    if shown && bounds == previous {
        // The window stays put while a submenu opens, slides and closes. Only the clip follows the cards.
        if cards != previous_cards {
            POPUPS.with(|p| {
                if let Some(popup) = p.borrow_mut().get_mut(&key) {
                    popup.cards = cards;
                }
            });
            set_popup_region(window, width, height, radius, cards, dpi);
        }
        return;
    }
    POPUPS.with(|p| {
        if let Some(popup) = p.borrow_mut().get_mut(&key) {
            popup.bounds = bounds;
            popup.cards = cards;
            popup.shown = true;
        }
    });
    set_popup_region(window, width, height, radius, cards, dpi);
    // Where a child card has room: the page picks its side from this before any submenu can open.
    if let Some((left, right)) = room {
        let _ = host.post_json(&room_json(token, left, right));
    }
    unsafe {
        let _ = SetWindowPos(
            window,
            Some(HWND_TOP),
            bounds.left,
            bounds.top,
            width,
            height,
            if shown || passive { SWP_NOACTIVATE } else { Default::default() },
        );
        let _ = host.set_bounds(RECT { left: 0, top: 0, right: width, bottom: height });
        // A page that grows or shrinks while open (palette results, site info rows) is only
        // re-bounded: focus stays where the user is typing.
        if shown {
            return;
        }
        // A passive popup (the downloads bubble a download start opened) appears over the page
        // without taking the keyboard from it.
        if passive {
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
            return;
        }
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = host.focus();
    }
}

/// Clips the popup to one round rectangle, or to the union of the parent card and the child card.
/// Pixels outside that clip are not part of the window, so the gap between the two menus shows the page.
fn set_popup_region(window: HWND, width: i32, height: i32, radius: i32, cards: Option<MenuCards>, dpi: u32) {
    let region = popup_region(width, height, radius, cards, dpi);
    if region.is_invalid() {
        return;
    }
    unsafe {
        if SetWindowRgn(window, Some(region), true) == 0 {
            let _ = DeleteObject(HGDIOBJ(region.0));
        }
    }
}

fn popup_region(width: i32, height: i32, radius: i32, cards: Option<MenuCards>, dpi: u32) -> HRGN {
    let full = round_region(0, 0, width, height, radius);
    let Some(cards) = cards else {
        return full;
    };
    let parent = card_device_rect(cards.parent_x, cards.parent_y, cards.parent_w, cards.parent_h, dpi, width, height);
    let child = card_device_rect(cards.child_x, cards.child_y, cards.child_w, cards.child_h, dpi, width, height);
    let region = match (parent, child) {
        (Some(parent), Some(child)) => union_region(round_region(parent.left, parent.top, parent.right, parent.bottom, radius), round_region(child.left, child.top, child.right, child.bottom, radius)),
        (Some(parent), None) => round_region(parent.left, parent.top, parent.right, parent.bottom, radius),
        (None, Some(child)) => round_region(child.left, child.top, child.right, child.bottom, radius),
        (None, None) => full,
    };
    if !matches!((parent, child), (None, None)) && !full.is_invalid() {
        unsafe { let _ = DeleteObject(HGDIOBJ(full.0)); }
    }
    region
}

/// Exclusive device-pixel edges of one card, clipped to the window. `None` when the card misses the window.
fn card_device_rect(x: u16, y: u16, w: u16, h: u16, dpi: u32, window_w: i32, window_h: i32) -> Option<RECT> {
    let left = css_px(i32::from(x), dpi);
    let top = css_px(i32::from(y), dpi);
    let right = css_px(i32::from(x) + i32::from(w), dpi);
    let bottom = css_px(i32::from(y) + i32::from(h), dpi);
    let (left, right) = clip_span(left, right, window_w)?;
    let (top, bottom) = clip_span(top, bottom, window_h)?;
    Some(RECT { left, top, right, bottom })
}

fn css_px(value: i32, dpi: u32) -> i32 {
    (value * dpi.max(1) as i32 + 48) / 96
}

fn clip_span(start: i32, end: i32, limit: i32) -> Option<(i32, i32)> {
    let start = start.max(0);
    let end = end.min(limit);
    (end - start >= 1).then_some((start, end))
}

fn round_region(left: i32, top: i32, right: i32, bottom: i32, radius: i32) -> HRGN {
    unsafe { CreateRoundRectRgn(left, top, right + 1, bottom + 1, radius * 2, radius * 2) }
}

fn union_region(base: HRGN, extra: HRGN) -> HRGN {
    unsafe {
        if base.is_invalid() {
            return extra;
        }
        if !extra.is_invalid() {
            let _ = CombineRgn(Some(base), Some(base), Some(extra), RGN_OR);
            let _ = DeleteObject(HGDIOBJ(extra.0));
        }
        base
    }
}

fn menu_json(tok: u32, kind: &str, menu: &Menu, ids: &mut HashSet<u32>) -> Option<String> {
    let mut count = 0;
    let mut seen = HashSet::new();
    if kind.is_empty()
        || kind.encode_utf16().count() > 32
        || menu.entries.len() > MAX_ITEMS
        || !validate_entries(&menu.entries, 0, &mut count, &mut seen, ids)
    {
        return None;
    }
    let mut favs = FavTable::default();
    Some(Json::object(|out| {
        out.int("v", 1);
        out.string("t", "menu");
        out.uint("tok", u64::from(tok));
        out.string("kind", kind);
        if let Some(header) = &menu.header {
            out.object("header", |out| {
                out.string("name", cap16(&header.profile, MAX_LABEL));
                out.string("caption", cap16(&header.caption, MAX_LABEL));
                out.string("avatar", cap16(&header.avatar, 4));
                let accent = header.accent;
                out.string(
                    "accent",
                    &format!("#{:02x}{:02x}{:02x}", accent.r, accent.g, accent.b),
                );
            });
        }
        if !menu.footer.is_empty() {
            out.string("footer", cap16(&menu.footer, MAX_FOOTER));
        }
        out.array("entries", |rows| write_entries(rows, &menu.entries, &mut favs));
        if !favs.uris.is_empty() {
            out.array("favs", |rows| {
                for uri in &favs.uris {
                    rows.string(uri);
                }
            });
        }
    }))
}

/// Favicons of one menu, each distinct PNG once: rows refer to an entry by index (`fav`), so a
/// menu of twenty tabs from one site sends one image. Bounded, and every image passes the same gate
/// as the tab strip's favicons (a small PNG with sane dimensions), so a refused one is just omitted.
#[derive(Default)]
struct FavTable {
    uris: Vec<String>,
    bytes: usize,
}

impl FavTable {
    fn index_of(&mut self, png: &[u8]) -> Option<usize> {
        let uri = chrome_host::protocol::favicon_data_uri(png)?;
        if let Some(found) = self.uris.iter().position(|known| *known == uri) {
            return Some(found);
        }
        if self.uris.len() >= MAX_FAVICONS || self.bytes + uri.len() > MAX_FAVICON_TOTAL {
            return None;
        }
        self.bytes += uri.len();
        self.uris.push(uri);
        Some(self.uris.len() - 1)
    }
}
fn validate_entries(
    entries: &[Entry],
    depth: usize,
    count: &mut usize,
    seen: &mut HashSet<u32>,
    ids: &mut HashSet<u32>,
) -> bool {
    if depth > 8 {
        return false;
    }
    for entry in entries {
        *count += 1;
        if *count > MAX_TOTAL_ITEMS {
            return false;
        }
        match entry {
            Entry::Separator => {}
            Entry::SectionHeader(_) => {}
            Entry::Item(item) => {
                let Ok(id) = u32::try_from(item.id) else {
                    return false;
                };
                if !seen.insert(id)
                    || item
                        .shortcut
                        .is_some_and(|s| s.encode_utf16().count() > MAX_SHORTCUT)
                {
                    return false;
                }
                if item.enabled && item.submenu_entries.is_none() {
                    ids.insert(id);
                }
                if let Some(children) = &item.submenu_entries {
                    if children.len() > MAX_ITEMS
                        || !validate_entries(children, depth + 1, count, seen, ids)
                    {
                        return false;
                    }
                }
            }
        }
    }
    true
}
/// `text` cut to at most `max` UTF-16 units (the page's length unit) at a character boundary, so
/// long tab titles and names still fit the payload caps instead of refusing the whole menu.
fn cap16(text: &str, max: usize) -> &str {
    let mut units = 0;
    for (at, ch) in text.char_indices() {
        units += ch.len_utf16();
        if units > max {
            return &text[..at];
        }
    }
    text
}

fn write_entries(
    rows: &mut crate::chrome_host::json::Array<'_>,
    entries: &[Entry],
    favs: &mut FavTable,
) {
    for entry in entries {
        match entry {
            Entry::Separator => rows.object(|out| out.bool("separator", true)),
            Entry::SectionHeader(label) => {
                rows.object(|out| out.string("section", cap16(label, MAX_LABEL)))
            }
            Entry::Item(item) => rows.object(|out| {
                out.uint("id", item.id as u64);
                out.string("label", cap16(&item.label, MAX_LABEL));
                out.bool("enabled", item.enabled);
                out.bool("checked", item.checked);
                if let Some(shortcut) = item.shortcut {
                    out.string("shortcut", shortcut);
                }
                if let Some(icon) = item.glyph.or_else(|| icon_token(item.icon)) {
                    out.string("icon", icon);
                }
                if let Some(index) = item
                    .favicon_png
                    .as_deref()
                    .and_then(|png| favs.index_of(png))
                {
                    out.uint("fav", index as u64);
                }
                if let Some((avatar, accent)) = &item.badge {
                    out.object("badge", |out| {
                        out.string("avatar", cap16(avatar, 4));
                        out.string(
                            "accent",
                            &format!("#{:02x}{:02x}{:02x}", accent.r, accent.g, accent.b),
                        );
                    });
                }
                if let Some(children) = &item.submenu_entries {
                    out.array("submenu", |rows| write_entries(rows, children, favs));
                }
            }),
        }
    }
}
fn icon_token(icon: Icon) -> Option<&'static str> {
    Some(match icon {
        // Grid and Check were reused for unrelated commands (D3); the HTML menu draws only an
        // explicit `Item::glyph` for them, and a row with neither has no icon.
        Icon::None | Icon::Grid | Icon::Check => return None,
        Icon::Plus => "plus",
        Icon::Person => "person",
        Icon::Key => "key",
        Icon::Shield => "shield",
        Icon::Gear => "gear",
        Icon::Close => "close",
        Icon::Lock => "lock",
        Icon::Unlock => "unlock",
        Icon::Warning => "warning",
        Icon::Info => "info",
        Icon::Mic => "mic",
        Icon::Camera => "camera",
        Icon::Location => "location",
        Icon::Bell => "bell",
        Icon::Cookie => "cookie",
        Icon::Reset => "reset",
    })
}

fn popup_bounds(
    anchor: POINT,
    width_css: i32,
    height_css: i32,
    dpi: u32,
    work: RECT,
    placement: Placement,
) -> RECT {
    let dpi = dpi.max(1);
    // A menu taller or wider than the work area is capped to it; the page scrolls inside.
    let width = ((width_css * dpi as i32 + 48) / 96).clamp(1, (work.right - work.left).max(1));
    let height = ((height_css * dpi as i32 + 48) / 96).clamp(1, (work.bottom - work.top).max(1));
    let origin = match placement {
        Placement::AboveLeft => menu::place(anchor, width, height, work),
        Placement::BelowLeft => menu::place_below(anchor, width, height, work),
        Placement::BelowRight => menu::place_below(POINT { x: anchor.x - width, y: anchor.y }, width, height, work),
    };
    RECT {
        left: origin.x,
        top: origin.y,
        right: origin.x + width,
        bottom: origin.y + height,
    }
}

/// A shown anchored page without cards (`c`) that reports a new width keeps its top and right edge
/// (clamped into the work area) and grows or shrinks on the left and the bottom, like a menu hung
/// from a right-aligned button. An unchanged width leaves placement to `bounds` (a flip above the
/// anchor included). Menus with submenus never get here: `flyout_bounds` anchors their parent card.
fn keep_right_edge(previous: RECT, bounds: RECT, work: RECT) -> RECT {
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    if width == previous.right - previous.left {
        return bounds;
    }
    let right = previous.right.min(work.right).max(work.left + width);
    let top = previous.top.clamp(work.top, (work.bottom - 1).max(work.top));
    let bottom = (top + height).min(work.bottom).max(top + 1);
    RECT { left: right - width, top, right, bottom }
}

/// The window of a menu that has submenus. The page is the parent card with room for a child card on
/// both sides, and `cards` says where the parent card sits in it. `parent` is the parent card placed
/// as if it were the whole menu (anchor, flip, work-area clamp), so the window hangs off it: the
/// parent's screen rectangle depends only on the parent card, and a child opening, sliding or closing
/// (which changes only the child card) never moves it. The window runs from the parent's top to the
/// bottom of the work area, so a child dropped to a low row has room, and may run past the work area
/// on the sides: that room is clipped away, and the page opens a child only on a side `flyout_room`
/// says is on screen.
fn flyout_bounds(parent: RECT, cards: MenuCards, width_css: i32, dpi: u32, work: RECT) -> RECT {
    let left = parent.left - css_px(i32::from(cards.parent_x), dpi);
    let top = parent.top - css_px(i32::from(cards.parent_y), dpi);
    let width = css_px(width_css, dpi).max(css_px(i32::from(cards.parent_x) + i32::from(cards.parent_w), dpi)).max(1);
    let bottom = work.bottom.max(parent.bottom).max(top + 1);
    RECT { left, top, right: left + width, bottom }
}

/// CSS pixels of work area past the parent card's left and right edges (Rust -> page `room`).
fn flyout_room(parent: RECT, work: RECT, dpi: u32) -> (u32, u32) {
    let css = |device: i32| (i64::from(device.max(0)) * 96 / i64::from(dpi.max(1))).min(4000) as u32;
    (css(parent.left - work.left), css(work.right - parent.right))
}

fn room_json(tok: u32, left: u32, right: u32) -> String {
    Json::object(|out| {
        out.int("v", 1);
        out.string("t", "room");
        out.uint("tok", u64::from(tok));
        out.uint("l", u64::from(left));
        out.uint("r", u64::from(right));
    })
}

// --- popup surfaces (lane PP) ---
fn page_bounds(at: Anchor, width_css: i32, height_css: i32, dpi: u32, work: RECT) -> RECT {
    match at {
        Anchor::Point(anchor, placement) => popup_bounds(anchor, width_css, height_css, dpi, work, placement),
        Anchor::TopCenter(window) => top_center_bounds(window, width_css, height_css, dpi, work),
    }
}

/// Centred on `window`, top edge 12% down it, kept inside the work area.
fn top_center_bounds(window: RECT, width_css: i32, height_css: i32, dpi: u32, work: RECT) -> RECT {
    let dpi = dpi.max(1) as i32;
    let width = ((width_css * dpi + 48) / 96).clamp(1, (work.right - work.left).max(1));
    let height = ((height_css * dpi + 48) / 96).clamp(1, (work.bottom - work.top).max(1));
    let x = (window.left + window.right - width) / 2;
    let y = window.top + (window.bottom - window.top) * TOP_CENTER_PERCENT / 100;
    let x = x.clamp(work.left, (work.right - width).max(work.left));
    let y = y.clamp(work.top, (work.bottom - height).max(work.top));
    RECT { left: x, top: y, right: x + width, bottom: y + height }
}
// --- end popup surfaces ---

pub(crate) fn monitor_data(anchor: POINT) -> (u32, RECT) {
    unsafe {
        let monitor = MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            info.rcWork
        } else {
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            }
        };
        let mut x = FALLBACK_DPI;
        let mut y = FALLBACK_DPI;
        let dpi = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y)
            .ok()
            .map(|_| x)
            .unwrap_or(FALLBACK_DPI);
        (dpi, work)
    }
}

fn register_class() -> windows::core::Result<()> {
    CLASS_REGISTERED.with(|registered| {
        if *registered.borrow() {
            return Ok(());
        }
        let instance = unsafe { GetModuleHandleW(None)? };
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        let atom = unsafe { RegisterClassW(&class) };
        if atom == 0 {
            return Err(windows::core::Error::from_thread());
        }
        *registered.borrow_mut() = true;
        Ok(())
    })
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmExtendFrameIntoClientArea(hwnd: HWND, margins: *const i32) -> i32;
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    if message == WM_ACTIVATE && (wparam.0 & 0xffff) == WA_INACTIVE as usize {
        if let Some(owner) = POPUPS.with(|p| {
            p.borrow()
                .values()
                .find(|p| p.window == hwnd && p.dismiss.is_active())
                .map(|p| p.owner)
        }) {
            close(owner);
        }
    }
    if message == WM_ERASEBKGND {
        return windows::Win32::Foundation::LRESULT(1);
    }
    if message == WM_NCCREATE {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    if message == WM_NCDESTROY {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_converts_css_pixels_with_monitor_dpi_and_clamps() {
        let work = RECT {
            left: 100,
            top: 50,
            right: 1100,
            bottom: 750,
        };
        let bounds = popup_bounds(
            POINT { x: 1080, y: 680 },
            300,
            200,
            144,
            work,
            Placement::BelowLeft,
        );
        assert_eq!(
            (bounds.left, bounds.top, bounds.right, bounds.bottom),
            (630, 380, 1080, 680)
        );
        let anchored = popup_bounds(
            POINT { x: 100, y: 50 },
            300,
            200,
            96,
            work,
            Placement::AboveLeft,
        );
        assert_eq!(
            (anchored.left, anchored.top, anchored.right, anchored.bottom),
            (100, 50, 400, 250)
        );
    }

    #[test]
    fn the_downloads_bubble_hangs_right_aligned_under_its_button_and_stays_on_screen() {
        let work = RECT { left: 0, top: 0, right: 1000, bottom: 700 };
        let key = |r: RECT| (r.left, r.top, r.right, r.bottom);
        // 360 CSS px wide at 96 dpi, its right edge at the button's right edge, just under the button.
        let under = popup_bounds(POINT { x: 900, y: 40 }, 360, 200, 96, work, Placement::BelowRight);
        assert_eq!(key(under), (540, 40, 900, 240));
        // At 150% the same CSS size is 540 device pixels wide and the right edge still lines up.
        let scaled = popup_bounds(POINT { x: 900, y: 40 }, 360, 200, 144, work, Placement::BelowRight);
        assert_eq!((scaled.right, scaled.right - scaled.left, scaled.top), (900, 540, 40));
        // A button near the left edge (a narrow window): the bubble is clamped into the work area.
        let narrow = popup_bounds(POINT { x: 200, y: 40 }, 360, 200, 96, work, Placement::BelowRight);
        assert_eq!((narrow.left, narrow.right), (0, 360));
        // No room below the button: it flips above it, still right-aligned.
        let flipped = popup_bounds(POINT { x: 900, y: 600 }, 360, 200, 96, work, Placement::BelowRight);
        assert_eq!(key(flipped), (540, 400, 900, 600));
        // The left-aligned placement the menus use is unchanged by the new variant.
        let left = popup_bounds(POINT { x: 100, y: 40 }, 360, 200, 96, work, Placement::BelowLeft);
        assert_eq!((left.left, left.right), (100, 460));
    }

    #[test]
    fn a_shown_popup_that_changes_width_keeps_its_right_edge_inside_the_work_area() {
        let work = RECT { left: 0, top: 0, right: 1000, bottom: 700 };
        let rect = |left, top, right, bottom| RECT { left, top, right, bottom };
        let key = |r: RECT| (r.left, r.top, r.right, r.bottom);
        // A submenu opens: 200 px becomes 500 px, growing on the left; the height still follows the page.
        assert_eq!(key(keep_right_edge(rect(700, 100, 900, 400), rect(700, 100, 1200, 420), work)), (400, 100, 900, 420));
        // It closes: back to 200 px, the right edge unchanged.
        assert_eq!(key(keep_right_edge(rect(400, 100, 900, 420), rect(400, 100, 600, 400), work)), (700, 100, 900, 400));
        // No room on the left: the window is clamped to the work area instead.
        assert_eq!(key(keep_right_edge(rect(100, 100, 300, 400), rect(100, 100, 600, 400), work)), (0, 100, 500, 400));
        // An unchanged width leaves the placement alone (a flip above the anchor included).
        assert_eq!(key(keep_right_edge(rect(700, 100, 900, 400), rect(700, 50, 900, 350), work)), (700, 50, 900, 350));
        // A taller submenu would have been placed higher. The parent stays at its old top and the window grows down.
        assert_eq!(key(keep_right_edge(rect(700, 100, 900, 400), rect(660, 40, 1200, 520), work)), (360, 100, 900, 580));
    }

    /// The parent card's screen rectangle: the window origin plus its card in the clip.
    fn parent_on_screen(window: RECT, cards: MenuCards, dpi: u32) -> RECT {
        let card = card_device_rect(cards.parent_x, cards.parent_y, cards.parent_w, cards.parent_h, dpi, window.right - window.left, window.bottom - window.top).unwrap();
        RECT { left: window.left + card.left, top: window.top + card.top, right: window.left + card.right, bottom: window.top + card.bottom }
    }

    fn child_on_screen(window: RECT, cards: MenuCards, dpi: u32) -> Option<RECT> {
        let card = card_device_rect(cards.child_x, cards.child_y, cards.child_w, cards.child_h, dpi, window.right - window.left, window.bottom - window.top)?;
        Some(RECT { left: window.left + card.left, top: window.top + card.top, right: window.left + card.right, bottom: window.top + card.bottom })
    }

    #[test]
    fn a_submenu_opening_sliding_and_closing_never_moves_the_parent_card_on_either_side() {
        let key = |r: RECT| (r.left, r.top, r.right, r.bottom);
        let work = RECT { left: 0, top: 0, right: 1920, bottom: 1040 };
        // The page: the parent card (280 x 600 CSS px) with 354 px of room on both sides (718 + 280 wide).
        let parent_only = MenuCards { parent_x: 354, parent_y: 0, parent_w: 280, parent_h: 600, child_x: 0, child_y: 0, child_w: 0, child_h: 0 };
        let with_child = |x: u16, w: u16| MenuCards { child_x: x, child_y: 120, child_w: w, child_h: 200, ..parent_only };
        for dpi in [96, 120, 144, 192] {
            for (at, side) in [
                // The main menu hangs right-aligned from the menu button at the right of the screen: no
                // room on the right, so its submenus open to the left.
                (Anchor::Point(POINT { x: 1900, y: 80 }, Placement::BelowRight), "left"),
                // A tab's context menu at the pointer on the left of the screen: room on the right.
                (Anchor::Point(POINT { x: 300, y: 80 }, Placement::BelowLeft), "right"),
                // Near the bottom: the parent flips above the anchor, the child still never moves it.
                (Anchor::Point(POINT { x: 300, y: 1000 }, Placement::AboveLeft), "right"),
            ] {
                let place = |cards: MenuCards| {
                    let parent = page_bounds(at, i32::from(cards.parent_w), i32::from(cards.parent_h), dpi, work);
                    flyout_bounds(parent, cards, 988, dpi, work)
                };
                let shut = place(parent_only);
                let parent = parent_on_screen(shut, parent_only, dpi);
                // The parent card is where the anchor put it, as if it were the whole menu.
                let alone = page_bounds(at, 280, 600, dpi, work);
                assert_eq!((parent.left, parent.top), (alone.left, alone.top), "{side} at {dpi}");
                assert!(parent.bottom <= work.bottom && parent.left >= work.left && parent.right <= work.right);
                // The side the page will pick from `room` (a 280 px child needs 284 px).
                let (left, right) = flyout_room(alone, work, dpi);
                let picked = if right >= 284 { "right" } else if left >= 284 { "left" } else { "neither" };
                assert_eq!(picked, side, "room {left} {right} at {dpi}");
                // The child's column at rest, then part way out of the parent's edge, then gone again.
                let (rest, part) = if side == "right" { (with_child(638, 280), with_child(638, 120)) } else { (with_child(70, 280), with_child(230, 120)) };
                for cards in [part, rest, part, parent_only] {
                    let window = place(cards);
                    assert_eq!(key(window), key(shut), "the window never moves or resizes ({side} at {dpi})");
                    assert_eq!(key(parent_on_screen(window, cards, dpi)), key(parent), "the parent never moves ({side} at {dpi})");
                }
                // The child shows beside the parent on its side, with the gap between them, below its top.
                let child = child_on_screen(place(rest), rest, dpi).unwrap();
                if side == "right" { assert!(child.left > parent.right) } else { assert!(child.right < parent.left) }
                assert!(child.top > parent.top && child.right <= work.right && child.left >= work.left);
                // The window runs to the bottom of the work area so a low child has room.
                assert_eq!(shut.bottom, work.bottom);
            }
        }
    }

    #[test]
    fn room_beside_the_parent_card_is_reported_in_css_pixels_inside_the_work_area() {
        let work = RECT { left: -1920, top: 0, right: 0, bottom: 1040 };
        let parent = RECT { left: -600, top: 40, right: -180, bottom: 940 };
        assert_eq!(flyout_room(parent, work, 96), (1320, 180));
        // 150%: 180 device px is 120 CSS px; a parent touching the edge has none.
        assert_eq!(flyout_room(parent, work, 144), (880, 120));
        assert_eq!(flyout_room(RECT { left: -1920, ..parent }, work, 96).0, 0);
        let json = room_json(9, 880, 120);
        assert_eq!(json, r#"{"v":1,"t":"room","tok":9,"l":880,"r":120}"#);
    }

    #[test]
    fn a_submenu_card_is_clipped_to_the_window_and_keeps_its_own_size() {
        let parent = card_device_rect(0, 0, 200, 400, 96, 320, 400).unwrap();
        assert_eq!((parent.left, parent.top, parent.right, parent.bottom), (0, 0, 200, 400));
        // The child sits beside a lower row and is only as tall as its own buttons.
        let child = card_device_rect(204, 280, 110, 96, 96, 320, 400).unwrap();
        assert_eq!((child.left, child.top, child.right, child.bottom), (204, 280, 314, 376));
        assert!(child.bottom - child.top < parent.bottom - parent.top);
        // 150%: CSS pixels scale the same way the window does, and a card past the right edge is cut.
        let scaled = card_device_rect(204, 280, 110, 96, 144, 480, 600).unwrap();
        assert_eq!((scaled.left, scaled.top, scaled.right, scaled.bottom), (306, 420, 471, 564));
        assert!(card_device_rect(400, 0, 80, 40, 96, 320, 400).is_none());
    }

    #[test]
    fn palette_sits_centred_twelve_percent_down_the_window_inside_the_work_area() {
        let work = RECT { left: 0, top: 0, right: 1920, bottom: 1040 };
        let window = RECT { left: 100, top: 100, right: 1100, bottom: 900 };
        let bounds = top_center_bounds(window, 560, 300, 96, work);
        assert_eq!((bounds.left, bounds.top, bounds.right, bounds.bottom), (320, 196, 880, 496));
        // 150%: 840 x 450 device px, still centred.
        let scaled = top_center_bounds(window, 560, 300, 144, work);
        assert_eq!((scaled.left, scaled.top, scaled.right - scaled.left, scaled.bottom - scaled.top), (180, 196, 840, 450));
        // A window hanging off the left edge keeps the palette on screen.
        let off = top_center_bounds(RECT { left: -900, top: 0, right: 100, bottom: 800 }, 560, 300, 96, work);
        assert_eq!(off.left, 0);
        assert_eq!(page_bounds(Anchor::TopCenter(window), 560, 300, 96, work), bounds);
    }

    #[test]
    fn page_json_carries_token_kind_and_only_declared_ids() {
        let mut page = Page::new("siteinfo", |out| out.string("host", "a<b"));
        page.pick(3);
        page.act(1, 2);
        page.act(9, 200);
        let (json, picks, acts) = page_json(5, "site-info", page).expect("small page");
        assert_eq!(json, "{\"v\":1,\"t\":\"siteinfo\",\"tok\":5,\"kind\":\"site-info\",\"host\":\"a\\u003cb\"}");
        assert_eq!(picks, HashSet::from([3]));
        assert_eq!(acts.get(&9), Some(&MAX_ACT_CHOICE), "choices are capped to the wire limit");
        assert!(act_allowed(true, 5, 5, 1, 2, &acts));
        assert!(!act_allowed(true, 5, 5, 1, 3, &acts), "choice above the row's range");
        assert!(!act_allowed(true, 5, 5, 2, 0, &acts), "undeclared row");
        assert!(!act_allowed(true, 6, 5, 1, 0, &acts), "stale token");
        assert!(!act_allowed(false, 5, 5, 1, 0, &acts), "closed popup");
        let huge = Page::new("palette", |out| out.string("x", &"a".repeat(MAX_PAGE_BYTES)));
        assert!(page_json(1, "palette", huge).is_none(), "oversized pages are refused");
        assert!(page_json(1, "", Page::new("palette", |_| {})).is_none());
    }

    #[test]
    fn a_composer_page_declares_no_pick_or_act_and_its_submit_handler_never_reaches_the_wire() {
        let mut page = Page::new("composer", |out| out.string("mode", "app-add"));
        page.compose(Rc::new(|_, _, _| ActOutcome::Keep));
        assert!(page.compose.is_some());
        let (json, picks, acts) = page_json(9, "composer:app-add", page).expect("small page");
        assert_eq!(json, "{\"v\":1,\"t\":\"composer\",\"tok\":9,\"kind\":\"composer:app-add\",\"mode\":\"app-add\"}");
        assert!(picks.is_empty(), "a composer is closed by dismissing, never by picking");
        assert!(acts.is_empty());
        assert!(!json.contains("compose\""), "the handler is Rust state, not page data");
    }

    #[test]
    fn stale_tokens_and_disabled_picks_are_rejected() {
        let enabled = HashSet::from([1, 3]);
        assert!(popup_pick_allowed(true, 7, 7, 1, &enabled));
        assert!(!popup_pick_allowed(true, 8, 7, 1, &enabled));
        assert!(!popup_pick_allowed(true, 7, 7, 2, &enabled));
        assert!(!popup_token_matches(false, 7, 7));
    }

    #[test]
    fn picks_from_a_closed_or_replaced_menu_are_rejected() {
        let start = Instant::now();
        let mut state = DismissState::default();
        let enabled = HashSet::from([4]);
        assert!(state.open("tab:1", start));
        assert!(popup_pick_allowed(state.is_active(), 9, 9, 4, &enabled));
        // A pick queued before the menu closed arrives after it: rejected.
        state.close(start);
        assert!(!popup_pick_allowed(state.is_active(), 9, 9, 4, &enabled));
        // The next menu has a new token; the old menu's token is stale.
        assert!(state.open("tab:2", start));
        assert!(!popup_token_matches(state.is_active(), 10, 9));
        // Submenu parents and disabled rows are never in the enabled set.
        let mut ids = HashSet::new();
        let menu = Menu {
            header: None,
            entries: vec![
                Entry::Item(crate::win32::menu::Item::new(1, "Parent", Icon::None).submenu_with(
                    vec![Entry::Item(crate::win32::menu::Item::new(2, "Child", Icon::None))],
                )),
                Entry::Item(crate::win32::menu::Item::new(3, "Off", Icon::None).disabled()),
            ],
            footer: String::new(),
        };
        assert!(menu_json(1, "x", &menu, &mut ids).is_some());
        assert_eq!(ids, HashSet::from([2]));
    }

    #[test]
    fn duplicate_ids_refuse_the_html_menu() {
        let menu = Menu {
            header: None,
            entries: vec![Entry::Item(
                crate::win32::menu::Item::new(5, "Session", Icon::None)
                    .submenu_with(vec![Entry::Item(crate::win32::menu::Item::new(5, "Restore", Icon::None))]),
            )],
            footer: String::new(),
        };
        assert!(menu_json(1, "sessions", &menu, &mut HashSet::new()).is_none());
    }

    #[test]
    fn a_pick_lifts_the_reopen_guard_but_a_dismissal_keeps_it() {
        let start = Instant::now();
        let mut state = DismissState::default();
        assert!(state.open("site-info", start));
        state.close(start);
        state.forgive();
        assert!(state.can_open("site-info", start + Duration::from_millis(1)));
        assert!(state.open("site-info", start));
        state.close(start);
        assert!(!state.can_open("site-info", start + Duration::from_millis(1)));
    }

    #[test]
    fn tab_or_workspace_switch_dismisses() {
        assert!(!context_changed(None, (1, 1)));
        assert!(!context_changed(Some((1, 1)), (1, 1)));
        assert!(context_changed(Some((1, 1)), (2, 1)));
        assert!(context_changed(Some((1, 1)), (1, 2)));
    }

    #[test]
    fn header_footer_and_long_labels_are_written_within_caps() {
        let long = "é".repeat(200) + "😀";
        let menu = Menu {
            header: Some(crate::win32::menu::ProfileHeader {
                profile: "Work".into(),
                caption: "Current profile".into(),
                avatar: "W".into(),
                accent: crate::theme::Rgba { r: 0x33, g: 0x66, b: 0xff, a: 255 },
            }),
            entries: vec![
                Entry::SectionHeader(long.clone()),
                Entry::Item(crate::win32::menu::Item::new(1, long.clone(), Icon::None)),
            ],
            footer: "Bowser 1.0".into(),
        };
        let json = menu_json(2, "main", &menu, &mut HashSet::new()).expect("long labels are cut, not refused");
        assert!(json.contains("\"header\":{\"name\":\"Work\",\"caption\":\"Current profile\",\"avatar\":\"W\",\"accent\":\"#3366ff\"}"));
        assert!(json.contains("\"footer\":\"Bowser 1.0\""));
        assert!(!json.contains('😀'));
        assert_eq!(cap16(&long, MAX_LABEL).encode_utf16().count(), MAX_LABEL);
        assert_eq!(cap16("a😀", 2), "a", "a surrogate pair is never split");
    }

    #[test]
    fn reopen_guard_only_blocks_the_same_kind_for_300ms() {
        let start = Instant::now();
        let mut state = DismissState::default();
        assert!(state.open("main", start));
        assert!(state.is_active());
        assert!(state.close(start + Duration::from_millis(1)));
        assert!(!state.open("main", start + Duration::from_millis(100)));
        assert!(state.open("context", start + Duration::from_millis(100)));
        assert!(state.close(start + Duration::from_millis(110)));
        assert!(state.open("context", start + Duration::from_millis(411)));
    }

    #[test]
    fn builder_caps_entries_and_escapes_text() {
        let entries = vec![Entry::Item(
            crate::win32::menu::Item::new(7, "A < B", Icon::Plus)
                .shortcut("Ctrl+T")
                .checked(true)
                .disabled(),
        )];
        let mut ids = HashSet::new();
        let json = menu_json(
            3,
            "main",
            &Menu {
                header: None,
                entries,
                footer: String::new(),
            },
            &mut ids,
        )
        .unwrap();
        assert!(json.contains("\\u003c"));
        assert!(json.contains("\"enabled\":false"));
        assert!(json.contains("\"checked\":true"));
        let over_cap = Menu {
            header: None,
            entries: vec![Entry::Separator; MAX_ITEMS + 1],
            footer: String::new(),
        };
        assert!(menu_json(3, "main", &over_cap, &mut HashSet::new()).is_none());
    }

    #[test]
    fn popup_messages_parse_and_write_the_lane_fixture() {
        use crate::chrome_host::protocol::{Command, parse_command};
        assert!(matches!(
            parse_command(r#"{"v":1,"t":"size","tok":7,"w":240,"h":100}"#),
            Ok(Command::PopupSize {
                tok: 7,
                w: 240,
                h: 100,
                cards: None,
            })
        ));
        for invalid in [
            r#"{"v":1,"t":"size","tok":7,"w":0,"h":100}"#,
            r#"{"v":1,"t":"size","tok":7,"w":4001,"h":100}"#,
            r#"{"v":1,"t":"pick","tok":4294967296,"id":1}"#,
        ] {
            assert!(parse_command(invalid).is_err(), "{invalid}");
        }
        assert!(matches!(
            parse_command(r#"{"v":1,"t":"pick","tok":7,"id":1}"#),
            Ok(Command::PopupPick { tok: 7, id: 1 })
        ));
        assert!(matches!(
            parse_command(r#"{"v":1,"t":"dismiss","tok":7}"#),
            Ok(Command::PopupDismiss { tok: 7 })
        ));

        let menu = Menu {
            header: None,
            entries: vec![
                Entry::Item(crate::win32::menu::Item::new(1, "Open", Icon::Plus)),
                Entry::Item(
                    crate::win32::menu::Item::new(2, "More", Icon::Info).submenu_with(vec![
                        Entry::Item(crate::win32::menu::Item::new(3, "Nested", Icon::Close)),
                    ]),
                ),
                Entry::Item(crate::win32::menu::Item::new(4, "Disabled", Icon::Close).disabled()),
            ],
            footer: String::new(),
        };
        let mut ids = HashSet::new();
        let menu_json = menu_json(7, "sample", &menu, &mut ids).expect("bounded menu");
        assert!(menu_json.contains("\"submenu\":["));
        assert_eq!(ids, HashSet::from([1, 3]));
        let fixture = format!(
            "{{\"menu\":{menu_json},\"size\":{{\"v\":1,\"t\":\"size\",\"tok\":7,\"w\":240,\"h\":100}},\"pick\":{{\"v\":1,\"t\":\"pick\",\"tok\":7,\"id\":1}},\"dismiss\":{{\"v\":1,\"t\":\"dismiss\",\"tok\":7}}}}\n"
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("ui/test/fixtures/popup-messages.json");
        if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some() {
            std::fs::write(&path, &fixture).expect("write popup fixture");
        }
        assert_eq!(
            std::fs::read_to_string(&path).expect("popup fixture"),
            fixture,
            "set BOWSER_WRITE_FIXTURES=1 cargo test popup_messages_parse_and_write_the_lane_fixture --lib"
        );
    }
}
