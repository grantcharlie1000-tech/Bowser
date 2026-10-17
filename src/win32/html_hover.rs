//! No-activate HTML hover layer owned by one browser window: tooltips, the tab hover card and the
//! island menu, drawn by one `View::Hover` chrome host in one owned `WS_POPUP`.
//!
//! The popup is `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` and answers `WM_MOUSEACTIVATE` with
//! `MA_NOACTIVATE`, so hovering or clicking it never takes focus from the omnibox or the page.
//! Tooltips also set `WS_EX_TRANSPARENT` and are hidden as soon as the pointer reaches them; the
//! card and the island menu take clicks. Only the island menu's name field ever activates the
//! popup, and only when the page asks for the keyboard (`hact` `focus`) after a click on it.
//!
//! Flow: Rust posts `{t:"hover",tok,kind,...}`; the page renders, measures and replies
//! `{t:"size",tok,w,h}`; Rust places the popup (clamped to the anchor monitor's work area, scaled by
//! that monitor's DPI) and shows it with `SWP_NOACTIVATE`. The host is suspended a little after the
//! layer hides. The layer is created lazily (after the HTML toolbar activates) and only in HTML mode;
//! native mode keeps `card.rs`, `island_menu.rs` and the native tooltips.
//!
//! Everything runs on the UI thread. No `LAYERS` borrow is held across a Win32 or WebView2 call.

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CombineRgn, CreateRectRgn, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, EndPaint, FillRect,
            GetMonitorInfoW, HGDIOBJ, HRGN, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
            PAINTSTRUCT, RGN_OR, SetWindowRgn,
        },
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
        UI::{
            HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, GWL_EXSTYLE, GWLP_USERDATA,
                GetCursorPos, GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, HWND_TOP, IDC_ARROW, IsWindowVisible,
                KillTimer, LoadCursorW, RegisterClassW, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW,
                SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
                WA_INACTIVE, WM_ACTIVATE, WM_APP, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_PAINT, WM_TIMER, WNDCLASSW,
                WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, w},
};

use super::{
    glass, hover_square,
    // --- omnibox suggest ---
    html_suggest::{Row, Target},
    // --- end omnibox suggest ---
};
use crate::{
    app::{App, AppRef},
    chrome::{
        island_menu::{self as island_model, SWATCH_COUNT},
        layout::Rect,
        tab_card::{self, MARKERS},
    },
    chrome_host::{
        self,
        host::{ChromeEnv, ChromeHost, View},
        protocol::{
            self, Command, HoverAction, HoverCard, HoverIsland, HoverRect, HoverRow, Security,
            ViewState,
        },
    },
    tabs::TabId,
    theme::{self, ISLAND_COLOURS, MARKER_COLOURS},
};

const CLASS_NAME: PCWSTR = w!("BowserHtmlHover");
/// Posted to the layer window when its host fails; the layer shuts down.
const WM_HOVER_FAILED: u32 = WM_APP + 0x51;
/// Posted to the layer window once per queued action, so actions run outside WebView2's callback.
const WM_HOVER_RUN: u32 = WM_APP + 0x52;
/// Posted when the layer was activated without asking (WebView2 focuses its window on click): the
/// owner gets activation back.
const WM_HOVER_RETURN: u32 = WM_APP + 0x53;
// --- omnibox suggest ---
/// Posted once per queued dropdown click, so the pick runs outside WebView2's callback.
const WM_HOVER_PICK: u32 = WM_APP + 0x54;
// --- end omnibox suggest ---
/// Card or island delay, armed on the layer window.
const OPEN_TIMER: usize = 1;
/// Leave check while something is shown.
const WATCH_TIMER: usize = 2;
/// Suspends the host a while after the layer hides, so quick re-shows skip the resume.
const SUSPEND_TIMER: usize = 3;
const WATCH_MS: u32 = 100;
// --- omnibox suggest ---
/// Drives the dropdown's open and close motion (the window region grows from, or shrinks into, the pill).
const DROP_TIMER: usize = 4;
/// Requested frame interval; Windows timers tick at about 15.6 ms, which is close to one display frame.
const DROP_FRAME_MS: u32 = 10;
/// Open: the list unrolls from the pill (design token --bw-dur-fast). Close is faster.
const DROP_OPEN: Duration = Duration::from_millis(83);
const DROP_CLOSE: Duration = Duration::from_millis(67);
/// Physical pixels the dropdown tucks under the pill's bottom edge, so no rounding can leave a gap.
const DROP_OVERLAP: i32 = 1;
// --- end omnibox suggest ---
const SUSPEND_AFTER_MS: u32 = 2000;
/// Most queued actions; more are dropped.
const ACTION_LIMIT: usize = 8;
/// `WM_MOUSEACTIVATE` result that keeps the layer from activating on click.
const MA_NOACTIVATE: isize = 3;
const FALLBACK_DPI: u32 = 96;
/// Logical gap between a tooltip and its anchor.
const TIP_GAP: i32 = 6;
/// Logical radius of the tooltip and of the card and island menu (design tokens: 4 and 8).
const TIP_RADIUS: i32 = 4;
const OVERLAY_RADIUS: i32 = 8;
// --- omnibox suggest ---
/// Logical radius of the dropdown's bottom corners: the omnibox pill's own (half its height, token
/// --bw-r-omnibox), so the open surface is rounded alike at both ends.
const SUGGEST_RADIUS: i32 = crate::chrome::layout::Metrics::OMNIBOX_HEIGHT / 2;
// --- end omnibox suggest ---

/// What the layer shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Tooltip,
    Card,
    Island,
    // --- omnibox suggest ---
    /// The omnibox suggestion dropdown (`html_suggest.rs` builds its rows).
    Suggest,
    // --- end omnibox suggest ---
}

#[derive(Clone, Debug, PartialEq)]
enum Content {
    /// `beside`: placed to the right of the anchor (the rail) rather than under it.
    Tooltip { text: String, beside: bool },
    Card { tab: TabId, rows: Vec<TabId>, model: HoverCard },
    Island { tab: TabId, rows: Vec<TabId>, model: HoverIsland },
    // --- omnibox suggest ---
    /// `seq` is the omnibox edit the rows answer; `width` is the dropdown width in CSS pixels;
    /// `query` is the typed text, which the page uses to emphasise matches.
    Suggest { seq: u32, width: u32, sel: usize, query: String, rows: Vec<Row> },
    // --- end omnibox suggest ---
}

impl Content {
    fn kind(&self) -> Kind {
        match self {
            Self::Tooltip { .. } => Kind::Tooltip,
            Self::Card { .. } => Kind::Card,
            Self::Island { .. } => Kind::Island,
            Self::Suggest { .. } => Kind::Suggest,
        }
    }
}

/// A card or island waiting for its hover delay.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pending {
    kind: Kind,
    tab: TabId,
    anchor: RECT,
}

/// The island the name field belongs to. Kept after the menu closes, so a commit sent from the
/// field's blur still lands.
#[derive(Clone, Debug, PartialEq, Eq)]
struct IslandSession {
    tab: TabId,
    first_tok: u32,
    committed: Option<String>,
}

type HostSlot = Rc<RefCell<Option<windows::core::Result<ChromeHost>>>>;

struct Layer {
    owner: HWND,
    window: HWND,
    /// Creation result, filled by the host's ready callback.
    slot: HostSlot,
    /// The host, once the page said `ready`.
    host: Option<ChromeHost>,
    next_tok: u32,
    tok: u32,
    content: Option<Content>,
    /// The trigger, in screen pixels.
    anchor: RECT,
    /// Where the layer is shown, in screen pixels. Empty while hidden.
    bounds: RECT,
    visible: bool,
    /// The theme last posted, so it is not re-sent for every tooltip.
    state_sent: Option<String>,
    target: Option<Pending>,
    island: Option<IslandSession>,
    actions: VecDeque<(u32, HoverAction)>,
    /// The island's name field asked for the keyboard, so activation is allowed until the layer hides.
    keyboard: bool,
    // --- omnibox suggest ---
    /// Clicks on dropdown rows, as (edit seq, row index), run outside WebView2's callback.
    picks: VecDeque<(u32, u8)>,
    /// The edit seq of the dropdown last shown, kept after it hides so the toolbar can be told.
    suggest_seq: u32,
    /// What the window shows right now (set when placed, cleared when hidden).
    shown: Option<Kind>,
    /// The dropdown's open or close motion while it runs.
    drop: Option<DropMotion>,
    /// Height of the dropdown's visible region, in physical pixels.
    drop_h: i32,
    /// The toolbar was told the dropdown is on screen (`suggestOpen`), so its pill joined it.
    drop_told: bool,
    /// The omnibox has the keyboard, so the page stays awake for the dropdown it is about to ask for.
    omnibox_focused: bool,
    // --- end omnibox suggest ---
}

// --- omnibox suggest ---
/// The dropdown unrolling from the pill (`closing` false) or rolling back into it, from a region
/// height of `from` physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DropMotion {
    started: Instant,
    from: i32,
    closing: bool,
}

/// Ease-out of the given power: 5 (quint) is strong, most of the travel at once; 3 (cubic) is
/// gentler, so a short close keeps moving until its end instead of sitting nearly done. Pure.
fn ease_out(t: f32, power: i32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(power)
}

/// How long before a close ends the toolbar is told: about two frames, its message latency, so the
/// pill's corners round as the list's last rows roll in rather than after the list has gone.
const DROP_UNTELL_LEAD: Duration = Duration::from_millis(33);

/// Visible height `elapsed` into `motion` for a dropdown `full` pixels tall, and whether the motion
/// is over. Pure.
fn drop_height(motion: DropMotion, full: i32, elapsed: Duration) -> (i32, bool) {
    let (target, length, power) = if motion.closing { (0, DROP_CLOSE, 3) } else { (full, DROP_OPEN, 5) };
    let t = elapsed.as_secs_f32() / length.as_secs_f32();
    if t >= 1.0 {
        return (target, true);
    }
    let from = motion.from.clamp(0, full.max(0));
    let h = from as f32 + (target - from) as f32 * ease_out(t, power);
    (h.round() as i32, false)
}

/// Whether a close `elapsed` in is near enough its end to give the pill its corners back. Pure.
fn drop_untell_due(motion: DropMotion, elapsed: Duration) -> bool {
    motion.closing && elapsed + DROP_UNTELL_LEAD >= DROP_CLOSE
}
// --- end omnibox suggest ---

thread_local! {
    static LAYERS: RefCell<HashMap<isize, Layer>> = RefCell::new(HashMap::new());
    static CLASS_REGISTERED: RefCell<bool> = const { RefCell::new(false) };
}

fn key(owner: HWND) -> isize {
    owner.0 as isize
}

fn with_layer<T>(owner: HWND, f: impl FnOnce(&mut Layer) -> T) -> Option<T> {
    LAYERS.with(|layers| layers.try_borrow_mut().ok()?.get_mut(&key(owner)).map(f))
}

fn owner_of_window(window: HWND) -> Option<HWND> {
    LAYERS.with(|layers| {
        layers.try_borrow().ok()?.values().find(|layer| layer.window == window).map(|layer| layer.owner)
    })
}

fn app_of(owner: HWND) -> Option<AppRef> {
    unsafe { (GetWindowLongPtrW(owner, GWLP_USERDATA) as *const AppRef).as_ref().cloned() }
}

// ----- lifetime -----

/// Creates the hidden layer and its host for `owner`, once. HTML mode only.
pub(crate) fn prewarm(owner: HWND, env: ChromeEnv) {
    if !chrome_host::html_ui_enabled()
        || owner.is_invalid()
        || LAYERS.with(|layers| layers.borrow().contains_key(&key(owner)))
        || register_class().is_err()
    {
        return;
    }
    let instance = unsafe { GetModuleHandleW(None).unwrap_or_default() };
    let Ok(window) = (unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT,
            CLASS_NAME,
            w!(""),
            WS_POPUP,
            0,
            0,
            480,
            640,
            Some(owner),
            None,
            Some(instance.into()),
            None,
        )
    }) else {
        return;
    };
    let slot: HostSlot = Rc::new(RefCell::new(None));
    let ready_slot = slot.clone();
    let raw = owner.0 as isize;
    let on_command = Rc::new(move |command| layer_command(HWND(raw as *mut _), command));
    let failed_window = window.0 as isize;
    let on_failed = Rc::new(move || {
        crate::win32::post(HWND(failed_window as *mut _), WM_HOVER_FAILED, WPARAM(0), LPARAM(0));
    });
    let created = ChromeHost::create(
        &env,
        window,
        RECT { left: 0, top: 0, right: 480, bottom: 640 },
        View::Hover,
        on_command,
        on_failed,
        None,
        move |result| *ready_slot.borrow_mut() = Some(result),
    );
    if created.is_err() {
        unsafe {
            let _ = DestroyWindow(window);
        }
        return;
    }
    LAYERS.with(|layers| {
        layers.borrow_mut().insert(
            key(owner),
            Layer {
                owner,
                window,
                slot,
                host: None,
                next_tok: 1,
                tok: 0,
                content: None,
                anchor: RECT::default(),
                bounds: RECT::default(),
                visible: false,
                state_sent: None,
                target: None,
                island: None,
                actions: VecDeque::new(),
                keyboard: false,
                picks: VecDeque::new(),
                suggest_seq: 0,
                shown: None,
                drop: None,
                drop_h: 0,
                drop_told: false,
                omnibox_focused: false,
            },
        );
    });
}

/// Closes the host and destroys the layer window.
pub(crate) fn shutdown(owner: HWND) {
    let Some(layer) = LAYERS.with(|layers| layers.try_borrow_mut().ok()?.remove(&key(owner))) else {
        return;
    };
    if let Some(host) = layer.host.as_ref() {
        host.close();
    } else if let Some(Ok(host)) = layer.slot.borrow_mut().take() {
        host.close();
    }
    unsafe {
        if GetForegroundWindow() == layer.window {
            let _ = SetForegroundWindow(layer.owner);
        }
        let _ = DestroyWindow(layer.window);
    }
    super::corner_fringe::destroy(owner); // --- omnibox suggest ---
}

fn ready_host(owner: HWND) -> Option<ChromeHost> {
    with_layer(owner, |layer| layer.host.clone()).flatten()
}

fn layer_command(owner: HWND, command: Command) {
    match command {
        Command::Ready => {
            let adopted = with_layer(owner, |layer| {
                let host = match layer.slot.borrow_mut().take() {
                    Some(Ok(host)) => host,
                    _ => return None,
                };
                layer.host = Some(host.clone());
                Some((host, layer.omnibox_focused)) // --- omnibox suggest ---
            })
            .flatten();
            if let Some((host, focused)) = adopted {
                let _ = host.set_visible(false);
                if !focused {
                    let _ = host.try_suspend();
                }
            }
        }
        Command::PopupSize { tok, w, h, .. } => place(owner, tok, w, h),
        Command::HoverAct { tok, action } => queue_action(owner, tok, action),
        // --- omnibox suggest ---
        Command::SuggestPick { seq, i } => queue_pick(owner, seq, i),
        // --- end omnibox suggest ---
        _ => {}
    }
}

// ----- entry points from window.rs, card.rs and island_menu.rs -----

/// Converts a rectangle in CSS pixels of a view whose controller covers `view` (screen pixels) at
/// `scale` device pixels per CSS pixel. Pure.
pub(crate) fn anchor_to_screen(view: RECT, scale: f32, rect: HoverRect) -> RECT {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let px = |v: u32| (v as f32 * scale).round() as i32;
    let left = view.left + px(rect.x);
    let top = view.top + px(rect.y);
    RECT { left, top, right: left + px(rect.w).max(1), bottom: top + px(rect.h).max(1) }
}

/// A view asked for a tooltip over `anchor` (screen pixels), under it or, with `beside`, to its right.
/// Ignored while a card or island is up.
pub(crate) fn tip(owner: HWND, anchor: RECT, text: &str, beside: bool) {
    heal_foreground(owner);
    let busy = with_layer(owner, |layer| blocks_tooltip(layer.content.as_ref().map(Content::kind)));
    if busy != Some(false) || text.trim().is_empty() {
        return;
    }
    present(owner, Content::Tooltip { text: text.to_owned(), beside }, anchor);
}

/// Whether showing `kind` keeps tooltips away: a card, island or dropdown owns the layer. A stale
/// record of one is what made tooltips refuse to appear (V6), so every popup close clears it. Pure.
fn blocks_tooltip(kind: Option<Kind>) -> bool {
    matches!(kind, Some(Kind::Card | Kind::Island | Kind::Suggest))
}

/// A popup (the main menu or any menu) closed. Forgets what the layer was showing or waiting to show,
/// so a card, island or dropdown that the popup displaced cannot block tooltips afterwards, and
/// takes the foreground back from a popup that closed without handing it over (clicking the menu
/// button again closes the menu by deactivation, unlike Esc, which restores it).
pub(crate) fn popup_closed(owner: HWND) {
    let stale = with_layer(owner, |layer| {
        layer.target = None;
        blocks_tooltip(layer.content.as_ref().map(Content::kind)) && !layer.keyboard
    });
    if stale == Some(true) {
        hide(owner);
    }
    heal_foreground(owner);
}

/// Where the foreground window is relative to the hover layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Foreground {
    /// The browser window or the hover layer itself.
    Here,
    /// A visible window of this application (a dialog, a popup that is open).
    Own,
    /// A window of this application that is hidden: a popup that closed without handing the
    /// foreground back.
    StaleOwn,
    /// Another application.
    Other,
}

/// Whether the pointer leaving the trigger should close the layer because the user went away:
/// only another application counts. A hidden popup of our own holding the foreground is not
/// "away", it is a bug that `heal_foreground` repairs. Pure.
fn user_is_away(foreground: Foreground) -> bool {
    foreground == Foreground::Other
}

fn classify_foreground(owner: HWND, layer_window: HWND) -> Foreground {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground == owner || foreground == layer_window {
            return Foreground::Here;
        }
        if foreground.0.is_null() || GetWindowThreadProcessId(foreground, None) != GetCurrentThreadId() {
            return Foreground::Other;
        }
        if IsWindowVisible(foreground).as_bool() { Foreground::Own } else { Foreground::StaleOwn }
    }
}

/// Gives the foreground back to the browser window when a hidden popup of ours still holds it.
fn heal_foreground(owner: HWND) {
    let Some(window) = with_layer(owner, |layer| layer.window) else { return };
    if classify_foreground(owner, window) == Foreground::StaleOwn {
        unsafe { let _ = SetForegroundWindow(owner); }
    }
}

/// The view hid its tooltip.
pub(crate) fn tip_off(owner: HWND) {
    let showing = with_layer(owner, |layer| matches!(layer.content, Some(Content::Tooltip { .. })));
    if showing == Some(true) {
        hide(owner);
    }
}

/// The strip pointer is over a tab (`Kind::Card`) or an island ball (`Kind::Island`), with its
/// trigger rectangle in screen pixels, or over neither (`None`). Arms the hover delay for a new
/// target; an open card for another tab closes, an open island stays until the pointer leaves it.
pub(crate) fn hover(owner: HWND, target: Option<(Kind, TabId, RECT)>) {
    let Some(window) = with_layer(owner, |layer| layer.window) else {
        return;
    };
    let target = target.map(|(kind, tab, anchor)| Pending { kind, tab, anchor });
    let (changed, close_card) = with_layer(owner, |layer| {
        let same = |a: Option<Pending>, b: Option<Pending>| {
            a.map(|p| (p.kind, p.tab)) == b.map(|p| (p.kind, p.tab))
        };
        if same(layer.target, target) {
            if let (Some(current), Some(new)) = (layer.target.as_mut(), target) {
                current.anchor = new.anchor;
            }
            return (false, false);
        }
        layer.target = target;
        let shown = match &layer.content {
            Some(Content::Card { tab, .. }) => Some((Kind::Card, *tab)),
            Some(Content::Island { tab, .. }) => Some((Kind::Island, *tab)),
            _ => None,
        };
        let close_card = matches!(shown, Some((Kind::Card, _)))
            && target.is_some_and(|t| Some((t.kind, t.tab)) != shown);
        (true, close_card)
    })
    .unwrap_or((false, false));
    if !changed {
        return;
    }
    unsafe {
        let _ = KillTimer(Some(window), OPEN_TIMER);
    }
    if close_card {
        hide(owner);
    }
    let Some(target) = target else {
        return;
    };
    // Back on the trigger of what is already shown: keep it.
    let shown = with_layer(owner, |layer| match &layer.content {
        Some(Content::Card { tab, .. }) => target.kind == Kind::Card && *tab == target.tab,
        Some(Content::Island { tab, .. }) => target.kind == Kind::Island && *tab == target.tab,
        _ => false,
    });
    if shown == Some(true) {
        return;
    }
    if target.kind == Kind::Card
        && app_of(owner).is_some_and(|app| App::active_tab_id(&app) == target.tab)
    {
        return;
    }
    let delay = match target.kind {
        Kind::Island => super::island_menu::HOVER_DELAY_MS,
        _ => super::card::HOVER_DELAY_MS,
    };
    unsafe {
        let _ = SetTimer(Some(window), OPEN_TIMER, delay, None);
    }
}

/// Opens the island menu for `tab` at once (a right click on its ball). `anchor` is in the owner's
/// client coordinates. Returns false when the layer is not ready, so the caller shows the native menu.
pub(crate) fn open_island(owner: HWND, app: &AppRef, tab: TabId, anchor: Rect) -> bool {
    if !chrome_host::html_ui_enabled() || ready_host(owner).is_none() {
        return false;
    }
    let mut origin = POINT::default();
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(owner, &mut origin);
    }
    let screen = RECT {
        left: origin.x + anchor.left,
        top: origin.y + anchor.top,
        right: origin.x + anchor.right,
        bottom: origin.y + anchor.bottom,
    };
    show_island(owner, app, tab, screen)
}

/// Hides everything and forgets the hover target, unless the island's name field has the keyboard.
/// Called from `card::cancel` (clicks, drags, moves, resizes, closing).
pub(crate) fn cancel(owner: HWND) {
    let Some(window) = with_layer(owner, |layer| {
        layer.target = None;
        layer.window
    }) else {
        return;
    };
    unsafe {
        let _ = KillTimer(Some(window), OPEN_TIMER);
        if GetForegroundWindow() == window {
            return;
        }
    }
    hide(owner);
}

/// Closes the card and its pending delay. Tab-list changes and tab switches use this.
pub(crate) fn cancel_card(owner: HWND) {
    let (window, card) = match with_layer(owner, |layer| {
        let pending_card = layer.target.is_some_and(|t| t.kind == Kind::Card);
        if pending_card {
            layer.target = None;
        }
        (layer.window, pending_card, matches!(layer.content, Some(Content::Card { .. })))
    }) {
        Some((window, pending, card)) => {
            if pending {
                unsafe {
                    let _ = KillTimer(Some(window), OPEN_TIMER);
                }
            }
            (window, card)
        }
        None => return,
    };
    let _ = window;
    if card {
        hide(owner);
    }
}

/// Keeps the card and island in step with the tab list: refreshes them, or closes them when their
/// tab is gone (or, for the card, became active).
pub(crate) fn sync(owner: HWND, app: &AppRef) {
    let Some((content, anchor)) =
        with_layer(owner, |layer| (layer.content.clone(), layer.anchor))
    else {
        return;
    };
    match content {
        Some(Content::Card { tab, model, .. }) => match card_content(app, tab) {
            Some(fresh) if App::active_tab_id(app) != tab => {
                if !matches!(&fresh, Content::Card { model: m, .. } if *m == model) {
                    present(owner, fresh, anchor);
                }
            }
            _ => hide(owner),
        },
        Some(Content::Island { tab, model, .. }) => match island_content(app, tab) {
            Some(fresh) => {
                if !matches!(&fresh, Content::Island { model: m, .. } if *m == model) {
                    present(owner, fresh, anchor);
                }
            }
            None => hide(owner),
        },
        _ => {}
    }
}

// ----- content -----

fn png_of(app: &AppRef, tab: TabId) -> Option<Vec<u8>> {
    App::tab_favicon(app, tab).filter(|png| chrome_host::favicon_sendable(png))
}

fn card_content(app: &AppRef, tab: TabId) -> Option<Content> {
    let (model, rows) = App::tab_card_model(app, tab)?;
    let card = HoverCard {
        title: model.title.clone(),
        host: model.domain.clone().unwrap_or_default(),
        marker: model.marker.map(|marker| marker.id()),
        markers: MARKER_COLOURS.iter().map(|c| chrome_host::css_colour(*c)).collect(),
        sleeping: App::tab_sleeping(app, tab),
        png: png_of(app, tab),
        rows: model
            .rows
            .iter()
            .zip(&rows)
            .take(tab_card::MAX_ROWS)
            .map(|(row, id)| HoverRow {
                title: row.title.clone(),
                active: row.active,
                linked: row.linked,
                png: png_of(app, *id),
            })
            .collect(),
        more: model.rows.len().saturating_sub(tab_card::MAX_ROWS),
        // --- tab options ---
        thumb: App::tab_thumbnail(app, tab),
        memory_mb: App::tab_memory_mb(app, tab),
        // --- end tab options ---
    };
    let rows = rows.into_iter().take(tab_card::MAX_ROWS).collect();
    Some(Content::Card { tab, rows, model: card })
}

fn island_content(app: &AppRef, tab: TabId) -> Option<Content> {
    let (model, rows) = App::island_menu(app, tab)?;
    let accent = theme::palette(glass::appearance(), true).tab_group_accent;
    let island = HoverIsland {
        name: model.name.clone().unwrap_or_default(),
        hint: model.suggested_name.clone().unwrap_or_else(|| island_model::PLACEHOLDER.to_owned()),
        slot: island_model::swatch_slot(model.colour),
        swatches: std::iter::once(accent)
            .chain(ISLAND_COLOURS)
            .map(chrome_host::css_colour)
            .collect(),
        rows: model
            .rows
            .iter()
            .zip(&rows)
            .take(island_model::MAX_ROWS)
            .map(|(row, id)| HoverRow {
                title: row.title.clone(),
                active: row.active,
                linked: false,
                png: png_of(app, *id),
            })
            .collect(),
        more: model.rows.len().saturating_sub(island_model::MAX_ROWS),
        close: model.close_label(),
    };
    let rows = rows.into_iter().take(island_model::MAX_ROWS).collect();
    Some(Content::Island { tab, rows, model: island })
}

fn show_island(owner: HWND, app: &AppRef, tab: TabId, anchor: RECT) -> bool {
    let Some(content) = island_content(app, tab) else {
        return false;
    };
    // The native island menu and card never stay up beside the HTML one.
    super::card::close();
    present(owner, content, anchor)
}

fn show_card(owner: HWND, app: &AppRef, tab: TabId, anchor: RECT) {
    if App::active_tab_id(app) == tab {
        return;
    }
    // --- tab options --- the memory line fills in when the measurement arrives.
    App::refresh_tab_memory(app);
    // --- end tab options ---
    if let Some(content) = card_content(app, tab) {
        present(owner, content, anchor);
    }
}

/// Theme state for a popup. Hover cards and tooltips use the opaque palette; the omnibox suggestions
/// are the pill grown downward, so they take the toolbar's own palette (focus ring colour and fill)
/// and the ring and material run on from the pill without a colour change.
fn theme_json(like_toolbar: bool) -> String {
    let palette = if like_toolbar { glass::palette() } else { theme::palette(glass::appearance(), true) };
    ViewState {
        scale: 1.0,
        pillx: 0,
        pillw: 0,
        menux: 0,
        solid: true,
        dark: glass::appearance() == theme::Appearance::Dark,
        back: false,
        forward: false,
        loading: false,
        split: false,
        bookmarked: false,
        progress: 0,
        show_progress: true,
        security: Security::None,
        label: String::new(),
        permblocked: false,
        zoom: 100,
        show_zoom: false,
        private: false,
        reader: crate::reader::Pill::Hidden,
        translate: crate::translate::Pill::Hidden,
        font_scale: 100,
        ui_font: String::new(),
        density: crate::layout_prefs::UiDensity::Regular, // --- layout (thread T6) ---
        compact_menus: false,
        look: crate::window_prefs::Look::default(), // --- window and panels (thread T9) ---
        bar: Default::default(), // --- toolbars and feeds (thread T13) ---
        theme: chrome_host::theme_tokens(palette),
    }
    .to_json()
}

/// Posts `content` with a fresh token. The layer is placed and shown when the page replies `size`.
fn present(owner: HWND, content: Content, anchor: RECT) -> bool {
    let Some(host) = ready_host(owner) else {
        return false;
    };
    let state = theme_json(matches!(content, Content::Suggest { .. }));
    let Some((window, tok, json, post_state, closed_suggest)) = with_layer(owner, |layer| {
        let tok = layer.next_tok;
        layer.next_tok = layer.next_tok.wrapping_add(1).max(1);
        layer.tok = tok;
        layer.anchor = anchor;
        let json = match &content {
            Content::Tooltip { text, .. } => protocol::tooltip_json(tok, text),
            Content::Card { model, .. } => protocol::card_json(tok, model),
            // --- omnibox suggest ---
            Content::Suggest { seq, width, sel, query, rows } => {
                layer.suggest_seq = *seq;
                let items: Vec<_> = rows.iter().map(|row| row.item.clone()).collect();
                protocol::suggest_json(tok, *seq, *width, *sel, query, &items)
            }
            // --- end omnibox suggest ---
            Content::Island { tab, model, .. } => {
                let same_island = layer.island.as_ref().is_some_and(|s| s.tab == *tab);
                if !same_island || !matches!(layer.content, Some(Content::Island { .. })) {
                    layer.island = Some(IslandSession {
                        tab: *tab,
                        first_tok: tok,
                        committed: (!model.name.is_empty()).then(|| model.name.clone()),
                    });
                } else if let Some(session) = layer.island.as_mut() {
                    session.committed = (!model.name.is_empty()).then(|| model.name.clone());
                }
                protocol::island_json(tok, model)
            }
        };
        // --- omnibox suggest ---
        let was_suggest = matches!(layer.content, Some(Content::Suggest { .. }));
        let is_suggest = matches!(content, Content::Suggest { .. });
        let closed_suggest = (was_suggest && !is_suggest).then_some(layer.suggest_seq);
        // --- end omnibox suggest ---
        layer.content = Some(content);
        let post_state = layer.state_sent.as_deref() != Some(state.as_str());
        if post_state {
            layer.state_sent = Some(state.clone());
        }
        (layer.window, tok, json, post_state, closed_suggest)
    }) else {
        return false;
    };
    let _ = tok;
    unsafe {
        let _ = KillTimer(Some(window), SUSPEND_TIMER);
    }
    if host.resume().is_err() {
        hide(owner);
        return false;
    }
    let _ = host.set_visible(true);
    if post_state {
        let _ = host.post_json(&state);
    }
    if host.post_json(&json).is_err() {
        hide(owner);
        return false;
    }
    // --- omnibox suggest ---
    if let Some(seq) = closed_suggest {
        tell_toolbar_closed(owner, seq);
    }
    // --- end omnibox suggest ---
    true
}

/// The page measured content `tok` at `w`x`h` CSS pixels: place and show the layer.
fn place(owner: HWND, tok: u32, w: u16, h: u16) {
    let Some((window, host, kind, anchor, beside)) = with_layer(owner, |layer| {
        if layer.tok != tok {
            return None;
        }
        let content = layer.content.as_ref()?;
        let beside = matches!(content, Content::Tooltip { beside: true, .. });
        Some((layer.window, layer.host.clone()?, content.kind(), layer.anchor, beside))
    })
    .flatten() else {
        return;
    };
    let (dpi, work) = monitor_data(POINT {
        x: (anchor.left + anchor.right) / 2,
        y: (anchor.top + anchor.bottom) / 2,
    });
    let width = scale_px(i32::from(w), dpi);
    let height = scale_px(i32::from(h), dpi);
    let bounds = match kind {
        Kind::Tooltip if beside => tooltip_beside_bounds(anchor, width, height, scale_px(TIP_GAP, dpi), work),
        Kind::Tooltip => tooltip_bounds(anchor, width, height, scale_px(TIP_GAP, dpi), work),
        Kind::Card | Kind::Island => overlay_bounds(
            anchor,
            width,
            height,
            scale_px(tab_card::ANCHOR_GAP, dpi),
            scale_px(tab_card::EDGE_MARGIN, dpi),
            work,
        ),
        // --- omnibox suggest ---
        Kind::Suggest => suggest_bounds(anchor, height, work),
        // --- end omnibox suggest ---
    };
    let radius = scale_px(
        match kind {
            Kind::Tooltip => TIP_RADIUS,
            Kind::Suggest => SUGGEST_RADIUS,
            Kind::Card | Kind::Island => OVERLAY_RADIUS,
        },
        dpi,
    );
    let (bw, bh) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
    unsafe {
        // Tooltips are click-through; the card and island menu take clicks. Neither ever activates.
        let style = GetWindowLongPtrW(window, GWL_EXSTYLE);
        let wanted = if kind == Kind::Tooltip {
            style | WS_EX_TRANSPARENT.0 as isize
        } else {
            style & !(WS_EX_TRANSPARENT.0 as isize)
        };
        if wanted != style {
            let _ = SetWindowLongPtrW(window, GWL_EXSTYLE, wanted);
        }
        // --- omnibox suggest ---
        // The dropdown unrolls from the pill the first time it appears (and re-opens from wherever a
        // close left it); while it stays open, a new height applies at once, so typing never animates.
        let visible_h = if kind == Kind::Suggest {
            let now = Instant::now();
            let animate = super::menu::animations_enabled();
            with_layer(owner, |layer| {
                let continuing = layer.visible && layer.shown == Some(Kind::Suggest);
                let from = match layer.drop {
                    Some(_) => layer.drop_h,
                    None if continuing => bh,
                    None => 0,
                };
                layer.drop = (animate && from < bh).then_some(DropMotion { started: now, from, closing: false });
                layer.drop_h = if layer.drop.is_some() { from } else { bh };
                layer.drop_h
            })
            .unwrap_or(bh)
        } else {
            let _ = with_layer(owner, |layer| layer.drop = None);
            let _ = KillTimer(Some(window), DROP_TIMER);
            bh
        };
        let region = if kind == Kind::Suggest {
            suggest_region(bw, visible_h, radius)
        } else {
            CreateRoundRectRgn(0, 0, bw + 1, bh + 1, radius * 2, radius * 2)
        };
        // --- end omnibox suggest ---
        if !region.is_invalid() && SetWindowRgn(window, Some(region), false) == 0 {
            let _ = DeleteObject(HGDIOBJ(region.0));
        }
        let _ = host.set_bounds(RECT { left: 0, top: 0, right: bw, bottom: bh });
        let _ = SetWindowPos(
            window,
            Some(HWND_TOP),
            bounds.left,
            bounds.top,
            bw,
            bh,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let _ = SetTimer(Some(window), WATCH_TIMER, WATCH_MS, None);
        // --- omnibox suggest ---
        if kind == Kind::Suggest {
            place_fringe(owner, window, bounds, visible_h, dpi);
        } else {
            super::corner_fringe::hide(owner);
        }
        // --- end omnibox suggest ---
    }
    // --- omnibox suggest ---
    let (animating, tell) = with_layer(owner, |layer| {
        layer.bounds = bounds;
        layer.visible = true;
        layer.shown = Some(kind);
        let open = kind == Kind::Suggest;
        let tell = (layer.drop_told != open).then_some(open);
        layer.drop_told = open;
        (layer.drop.is_some(), tell)
    })
    .unwrap_or((false, None));
    if animating {
        unsafe {
            let _ = SetTimer(Some(window), DROP_TIMER, DROP_FRAME_MS, None);
        }
    }
    if let Some(open) = tell {
        super::window::html_toolbar_post(owner, &protocol::suggest_open_json(open));
    }
    // --- end omnibox suggest ---
}

/// Hides the layer, gives the keyboard back if it had it, and schedules suspension.
fn hide(owner: HWND) {
    let Some((window, host, was_shown, closed_suggest, untell, focused_omnibox)) = with_layer(owner, |layer| {
        // --- omnibox suggest ---
        let was_suggest = matches!(layer.content, Some(Content::Suggest { .. }));
        let closed_suggest = was_suggest.then_some(layer.suggest_seq);
        let untell = std::mem::take(&mut layer.drop_told);
        layer.drop = None;
        layer.shown = None;
        // --- end omnibox suggest ---
        let was_shown = layer.content.take().is_some() || layer.visible;
        layer.visible = false;
        layer.bounds = RECT::default();
        layer.tok = 0;
        layer.keyboard = false;
        (layer.window, layer.host.clone(), was_shown, closed_suggest, untell, layer.omnibox_focused)
    }) else {
        return;
    };
    // --- omnibox suggest ---
    unsafe {
        let _ = KillTimer(Some(window), DROP_TIMER);
    }
    if let Some(seq) = closed_suggest {
        tell_toolbar_closed(owner, seq);
    }
    // --- end omnibox suggest ---
    if !was_shown {
        if untell {
            super::window::html_toolbar_post(owner, &protocol::suggest_open_json(false)); // --- omnibox suggest ---
        }
        return;
    }
    unsafe {
        let _ = KillTimer(Some(window), WATCH_TIMER);
        let focused = GetForegroundWindow() == window;
        super::corner_fringe::hide(owner); // --- omnibox suggest ---
        let _ = ShowWindow(window, SW_HIDE);
        if focused {
            let _ = SetForegroundWindow(owner);
        }
        if suspend_timer_wanted(false, focused_omnibox) {
            let _ = SetTimer(Some(window), SUSPEND_TIMER, SUSPEND_AFTER_MS, None);
        }
    }
    // --- omnibox suggest ---
    // The pill rounds its corners again only once the window is gone, so they never show a notch.
    if untell {
        super::window::html_toolbar_post(owner, &protocol::suggest_open_json(false));
    }
    // --- end omnibox suggest ---
    if let Some(host) = host {
        let _ = host.post_json(&protocol::hover_hide_json());
        let _ = host.set_visible(false);
    }
}

// ----- timers -----

fn open_timer(owner: HWND) {
    let Some(target) = with_layer(owner, |layer| layer.target).flatten() else {
        return;
    };
    let Some(app) = app_of(owner) else {
        return;
    };
    // The pointer must still be on the trigger.
    let mut cursor = POINT::default();
    if unsafe { GetCursorPos(&mut cursor) }.is_err()
        || !hover_square::contains(grow(target.anchor, 2), cursor)
    {
        return;
    }
    // As natively, an open island menu keeps the card away.
    let island_open = with_layer(owner, |layer| matches!(layer.content, Some(Content::Island { .. })));
    // --- omnibox suggest ---
    if with_layer(owner, |layer| matches!(layer.content, Some(Content::Suggest { .. }))) == Some(true) {
        return;
    }
    // --- end omnibox suggest ---
    match target.kind {
        Kind::Card if island_open == Some(true) => {}
        Kind::Card => show_card(owner, &app, target.tab, target.anchor),
        Kind::Island => {
            let _ = show_island(owner, &app, target.tab, target.anchor);
        }
        Kind::Tooltip | Kind::Suggest => {}
    }
}

fn watch_timer(owner: HWND) {
    let Some((window, kind, anchor, bounds, visible)) = with_layer(owner, |layer| {
        Some((layer.window, layer.content.as_ref()?.kind(), layer.anchor, layer.bounds, layer.visible))
    })
    .flatten() else {
        if let Some(window) = with_layer(owner, |layer| layer.window) {
            unsafe {
                let _ = KillTimer(Some(window), WATCH_TIMER);
            }
        }
        return;
    };
    if !visible {
        return;
    }
    let mut cursor = POINT::default();
    if unsafe { GetCursorPos(&mut cursor) }.is_err() {
        return;
    }
    let foreground = unsafe { GetForegroundWindow() };
    let focused = foreground == window;
    let away = user_is_away(classify_foreground(owner, window));
    let dpi = unsafe { GetDpiForWindow(owner) }.max(FALLBACK_DPI);
    if leave_decision(kind, anchor, bounds, cursor, focused, away, scale_px(hover_square::TOLERANCE, dpi))
        == hover_square::Leave::Close
    {
        hide(owner);
    }
}

/// Whether the layer closes on this leave check. Pure.
fn leave_decision(
    kind: Kind,
    anchor: RECT,
    bounds: RECT,
    cursor: POINT,
    focused: bool,
    away: bool,
    tolerance: i32,
) -> hover_square::Leave {
    if away {
        return hover_square::Leave::Close;
    }
    match kind {
        // A tooltip follows its trigger, and gets out of the way when the pointer reaches it.
        Kind::Tooltip => {
            if hover_square::contains(grow(anchor, tolerance), cursor)
                && !hover_square::contains(bounds, cursor)
            {
                hover_square::Leave::Keep
            } else {
                hover_square::Leave::Close
            }
        }
        // The dropdown stays while the omnibox has the keyboard; focus loss closes it elsewhere.
        Kind::Suggest => hover_square::Leave::Keep,
        Kind::Card | Kind::Island => hover_square::on_leave(
            hover_square::contains(bounds, cursor),
            hover_square::contains(hover_square::square(anchor, bounds, tolerance), cursor),
            focused,
        ),
    }
}

// ----- actions -----

fn queue_action(owner: HWND, tok: u32, action: HoverAction) {
    let window = with_layer(owner, |layer| {
        if layer.actions.len() >= ACTION_LIMIT {
            return None;
        }
        layer.actions.push_back((tok, action));
        Some(layer.window)
    })
    .flatten();
    if let Some(window) = window {
        crate::win32::post(window, WM_HOVER_RUN, WPARAM(0), LPARAM(0));
    }
}

/// Whether `tok` names the content now shown.
fn current(layer: &Layer, tok: u32) -> bool {
    tok != 0 && layer.tok == tok && layer.content.is_some()
}

/// Whether `tok` was issued for the island menu now shown. A name commit refreshes the menu under a
/// new token, so the `dismiss` an Enter sends right after it still carries the previous one.
fn same_island_session(layer: &Layer, tok: u32) -> bool {
    match (&layer.content, &layer.island) {
        (Some(Content::Island { tab, .. }), Some(session)) => {
            session.tab == *tab && tok != 0 && tok >= session.first_tok && tok <= layer.tok
        }
        _ => false,
    }
}

fn run_action(owner: HWND) {
    let Some(Some((tok, action))) = with_layer(owner, |layer| layer.actions.pop_front()) else {
        return;
    };
    let Some(app) = app_of(owner) else {
        return;
    };
    // Name commits may arrive from the field's blur after the menu closed: they are checked against
    // the island session, not the shown content.
    if let HoverAction::Name(text) = &action {
        let session = with_layer(owner, |layer| {
            layer.island.clone().filter(|s| tok >= s.first_tok && tok != 0 && tok <= layer.next_tok)
        })
        .flatten();
        if let Some(session) = session
            && App::tab_ids(&app).contains(&session.tab)
            && crate::tab_groups::clean_name(text) != session.committed
        {
            let _ = App::set_tab_group_name(&app, session.tab, text);
            let _ = with_layer(owner, |layer| {
                if let Some(s) = layer.island.as_mut() {
                    s.committed = crate::tab_groups::clean_name(text);
                }
            });
        }
        return;
    }
    let Some((content, window)) = with_layer(owner, |layer| {
        (current(layer, tok) || same_island_session(layer, tok))
            .then(|| (layer.content.clone(), layer.window))
    })
    .flatten()
    else {
        return;
    };
    let anchor = with_layer(owner, |layer| layer.anchor).unwrap_or_default();
    match (content, action) {
        (Some(Content::Card { tab, .. }), HoverAction::Marker(index)) => {
            if let Some(&marker) = MARKERS.get(usize::from(index)) {
                let current = App::tab_marker(&app, tab);
                App::set_tab_marker(&app, tab, tab_card::toggle_marker(current, marker));
                refresh(owner, &app, Kind::Card, tab, anchor);
            }
        }
        (Some(Content::Card { tab, .. }), HoverAction::NextMarker) => {
            let current = App::tab_marker(&app, tab);
            App::set_tab_marker(&app, tab, tab_card::next_marker(current));
            refresh(owner, &app, Kind::Card, tab, anchor);
        }
        (
            Some(Content::Card { rows, .. } | Content::Island { rows, .. }),
            HoverAction::Row(index),
        ) => {
            if let Some(&row) = rows.get(usize::from(index))
                && App::tab_ids(&app).contains(&row)
            {
                hide(owner);
                let _ = App::select_tab(&app, row);
            }
        }
        (Some(Content::Island { tab, .. }), HoverAction::Swatch(slot)) => {
            if usize::from(slot) < SWATCH_COUNT {
                let _ = App::set_tab_group_colour(&app, tab, island_model::swatch_colour(slot.into()));
                refresh(owner, &app, Kind::Island, tab, anchor);
            }
        }
        (Some(Content::Island { tab, .. }), HoverAction::CloseIsland) => {
            hide(owner);
            let _ = App::close_tab_island(&app, tab);
        }
        (Some(Content::Island { .. }), HoverAction::Focus) => {
            let _ = with_layer(owner, |layer| layer.keyboard = true);
            if let Some(host) = ready_host(owner) {
                unsafe {
                    let _ = SetForegroundWindow(window);
                }
                let _ = host.focus();
            }
        }
        (Some(Content::Island { .. }), HoverAction::Dismiss) => hide(owner),
        _ => {}
    }
}

fn refresh(owner: HWND, app: &AppRef, kind: Kind, tab: TabId, anchor: RECT) {
    let content = match kind {
        Kind::Card => card_content(app, tab),
        Kind::Island => island_content(app, tab),
        Kind::Tooltip | Kind::Suggest => None,
    };
    match content {
        Some(content) => {
            let _ = present(owner, content, anchor);
        }
        None => hide(owner),
    }
}

// --- omnibox suggest ---
// ----- omnibox suggestion dropdown -----
//
// The dropdown reuses this layer (one no-activate popup, one WebView2 host, shared theme, DPI and
// work-area placement) rather than a second host: a second WebView2 would cost another renderer
// process for a surface that is only ever shown while one toolbar edit is in flight. The toolbar page
// keeps the keyboard; it gets its own copy of the rows (no favicons) so Up and Down can preview text.

/// Tells the toolbar page the dropdown for edit `seq` is gone.
fn tell_toolbar_closed(owner: HWND, seq: u32) {
    super::window::html_toolbar_post(owner, &protocol::suggest_json(0, seq, 0, 0, "", &[]));
}

fn suggest_shown(owner: HWND) -> bool {
    with_layer(owner, |layer| matches!(layer.content, Some(Content::Suggest { .. }))) == Some(true)
}

/// Shows `rows` for omnibox edit `seq` under `anchor` (the pill, in screen pixels), `width` CSS pixels
/// wide, with the first row selected; `query` is the typed text the rows answer. No rows hides the
/// dropdown. False when the layer is not ready.
pub(crate) fn suggest_show(owner: HWND, seq: u32, anchor: RECT, width: u32, query: &str, rows: Vec<Row>) -> bool {
    if rows.is_empty() {
        suggest_close(owner);
        return false;
    }
    let plain: Vec<_> = rows
        .iter()
        .map(|row| protocol::SuggestItem { png: None, ..row.item.clone() })
        .collect();
    let toolbar = protocol::suggest_json(0, seq, 0, 0, "", &plain);
    if !present(owner, Content::Suggest { seq, width, sel: 0, query: query.to_owned(), rows }, anchor) {
        return false;
    }
    with_layer(owner, |layer| layer.omnibox_focused = true);
    super::window::html_toolbar_post(owner, &toolbar);
    true
}

/// Whether a hidden layer should count down to suspending its page: only while it is not shown and
/// the omnibox does not have the keyboard. Pure.
fn suspend_timer_wanted(visible: bool, omnibox_focused: bool) -> bool {
    !visible && !omnibox_focused
}

/// The omnibox took the keyboard: keep the page awake (cancel a pending suspend, wake a suspended
/// one) so the first dropdown does not wait for it.
pub(crate) fn omnibox_focused(owner: HWND) {
    let Some(window) = with_layer(owner, |layer| {
        layer.omnibox_focused = true;
        layer.window
    }) else {
        return;
    };
    unsafe {
        let _ = KillTimer(Some(window), SUSPEND_TIMER);
    }
    if let Some(host) = ready_host(owner) {
        let _ = host.resume();
    }
}

/// Closes the dropdown at once if it is open; the toolbar page is told. For a pick or a submit:
/// the page is about to change, so nothing lingers over it.
pub(crate) fn suggest_hide(owner: HWND) {
    if suggest_shown(owner) {
        hide(owner);
    }
}

/// Closes the dropdown by rolling it back into the pill (Escape, focus loss, nothing left to
/// suggest). The rows stop answering at once: the toolbar drops its copy now, and a pick that lands
/// during the motion finds no list. With animations off in Windows it closes at once.
pub(crate) fn suggest_close(owner: HWND) {
    let now = Instant::now();
    let animate = super::menu::animations_enabled();
    let Some(started) = with_layer(owner, |layer| {
        if !matches!(layer.content, Some(Content::Suggest { .. })) {
            return None;
        }
        let on_screen = layer.visible && layer.shown == Some(Kind::Suggest) && layer.drop_h > 0;
        if !on_screen || !animate {
            return Some(None);
        }
        layer.content = None;
        layer.tok = 0;
        layer.drop = Some(DropMotion { started: now, from: layer.drop_h, closing: true });
        Some(Some((layer.window, layer.suggest_seq)))
    })
    .flatten() else {
        return;
    };
    match started {
        None => hide(owner),
        Some((window, seq)) => {
            tell_toolbar_closed(owner, seq);
            unsafe {
                let _ = SetTimer(Some(window), DROP_TIMER, DROP_FRAME_MS, None);
            }
        }
    }
}

/// One frame of the dropdown's open or close motion; the end of a close hides the layer.
fn drop_frame(owner: HWND) {
    let now = Instant::now();
    let step = with_layer(owner, |layer| {
        let motion = layer.drop?;
        let (bw, bh) = (layer.bounds.right - layer.bounds.left, layer.bounds.bottom - layer.bounds.top);
        let elapsed = now.saturating_duration_since(motion.started);
        let (h, done) = drop_height(motion, bh, elapsed);
        layer.drop_h = h;
        if done {
            layer.drop = None;
        }
        let untell = layer.drop_told && drop_untell_due(motion, elapsed);
        if untell {
            layer.drop_told = false;
        }
        Some((bw, h, done, motion.closing, untell))
    });
    let Some(window) = with_layer(owner, |layer| layer.window) else {
        return;
    };
    let Some(Some((bw, h, done, closing, untell))) = step else {
        unsafe {
            let _ = KillTimer(Some(window), DROP_TIMER);
        }
        return;
    };
    if untell {
        super::window::html_toolbar_post(owner, &protocol::suggest_open_json(false));
    }
    if done {
        unsafe {
            let _ = KillTimer(Some(window), DROP_TIMER);
        }
        if closing {
            hide(owner);
            return;
        }
    }
    let dpi = unsafe { GetDpiForWindow(window) }.max(FALLBACK_DPI);
    unsafe {
        let region = suggest_region(bw, h, scale_px(SUGGEST_RADIUS, dpi));
        if !region.is_invalid() && SetWindowRgn(window, Some(region), true) == 0 {
            let _ = DeleteObject(HGDIOBJ(region.0));
        }
    }
    if let Some(bounds) = with_layer(owner, |layer| layer.bounds) {
        place_fringe(owner, window, bounds, h, dpi);
    }
}

/// The dropdown's bounds: tucked `DROP_OVERLAP` under the pill's bottom edge, exactly as wide as the
/// pill and aligned with it, so its side borders continue the pill's. Kept inside the work area. Pure.
fn suggest_bounds(anchor: RECT, height: i32, work: RECT) -> RECT {
    let width = (anchor.right - anchor.left).max(1);
    let left = clamp_span(anchor.left, width, work.left, work.right);
    let top = clamp_span(anchor.bottom - DROP_OVERLAP, height, work.top, work.bottom);
    RECT { left, top, right: left + width, bottom: top + height }
}

/// Radius of the dropdown's bottom corners while `visible` rows of pixels show: the full radius once
/// the list is tall and wide enough, a smaller one while it unrolls. Pure.
fn suggest_corner_radius(radius: i32, visible: i32, width: i32) -> i32 {
    radius.min(visible).min(width / 2).max(0)
}

/// The dropdown's window region with `visible` rows of pixels showing: square where it meets the
/// pill, rounded at its current bottom edge. Empty when nothing shows. The corners are cut a hair
/// inside the curve (only pixels it fully covers); `corner_fringe` paints the partly covered pixels
/// over them, so the visible edge is anti-aliased (V12).
unsafe fn suggest_region(width: i32, visible: i32, radius: i32) -> HRGN {
    unsafe {
        if visible <= 0 || width <= 0 {
            return CreateRectRgn(0, 0, 0, 0);
        }
        let r = suggest_corner_radius(radius, visible, width);
        let top = visible - r;
        let region = CreateRectRgn(0, 0, width, top.max(1).min(visible));
        if r > 0 {
            let insets = super::corner_fringe::insets_for(r);
            let mut y = 0;
            while y < r {
                let inset = insets[y as usize];
                let mut end = y + 1;
                while end < r && insets[end as usize] == inset {
                    end += 1;
                }
                let band = CreateRectRgn(inset, top + y, width - inset, top + end);
                if !band.is_invalid() {
                    let _ = CombineRgn(Some(region), Some(region), Some(band), RGN_OR);
                    let _ = DeleteObject(HGDIOBJ(band.0));
                }
                y = end;
            }
        }
        region
    }
}

/// Lays the dropdown's backdrop (anti-aliased bottom corners and shadow) under the region `visible`
/// pixels tall of the list at `bounds`, or takes it away when nothing shows.
fn place_fringe(owner: HWND, window: HWND, bounds: RECT, visible: i32, dpi: u32) {
    let width = bounds.right - bounds.left;
    let r = suggest_corner_radius(scale_px(SUGGEST_RADIUS, dpi), visible, width);
    let surface = suggest_surface(glass::palette());
    let dark = glass::appearance() == theme::Appearance::Dark;
    // No outline: the partly covered corner pixels take the list's own colour, so the curve blends
    // into the page without a ring of edge colour, and the shadow lifts the list off the page.
    super::corner_fringe::show(
        owner,
        window,
        super::corner_fringe::Backdrop {
            left: bounds.left,
            top: bounds.top,
            width,
            visible,
            full: bounds.bottom - bounds.top,
            radius: r,
            surface: (surface.r, surface.g, surface.b),
            shadow: suggest_shadow(dpi, dark),
        },
    );
}

/// The list's opaque colour, `--bw-omni-material` in tokens.css: the toolbar palette's omnibox fill
/// over its window colour without the glass alpha. The fill's alpha is rounded to three places, as
/// the CSS the page receives says it (`chrome_host::css_colour`), so both sides composite alike. Pure.
fn suggest_surface(palette: &theme::Palette) -> theme::Rgba {
    let (fill, window) = (palette.omnibox_fill, palette.window);
    let a = (f32::from(fill.a) / 255.0 * 1000.0).round() / 1000.0;
    let over = |top: u8, under: u8| (f32::from(top) * a + f32::from(under) * (1.0 - a)).round() as u8;
    theme::Rgba { r: over(fill.r, window.r), g: over(fill.g, window.g), b: over(fill.b, window.b), a: 0xFF }
}

/// The list's shadow at `dpi`: an ambient edge (CSS `0 0 2px`) and a key shadow (`0 8px 16px`), the
/// sizes of `--bw-sh-flyout`, a little stronger in dark where a dark page would swallow it. Pure.
fn suggest_shadow(dpi: u32, dark: bool) -> super::corner_fringe::Shadow {
    let scale = dpi.max(1) as f32 / FALLBACK_DPI as f32;
    let (ambient, key) = if dark { (0.30, 0.32) } else { (0.16, 0.14) };
    super::corner_fringe::Shadow {
        ambient_sigma: 1.0 * scale,
        ambient_alpha: ambient,
        key_drop: (8.0 * scale).round(),
        key_sigma: 8.0 * scale,
        key_alpha: key,
    }
}

/// The toolbar lost the keyboard. A press on the dropdown itself also blurs the omnibox, so the
/// dropdown stays when the pointer is over it, and closes otherwise.
pub(crate) fn suggest_focus_lost(owner: HWND) {
    with_layer(owner, |layer| layer.omnibox_focused = false);
    close_unless_pointer_over(owner);
    // A hidden layer was holding off suspension for the focus; start the countdown now.
    if let Some((window, visible)) = with_layer(owner, |layer| (layer.window, layer.visible))
        && suspend_timer_wanted(visible, false)
    {
        unsafe {
            let _ = SetTimer(Some(window), SUSPEND_TIMER, SUSPEND_AFTER_MS, None);
        }
    }
}

fn close_unless_pointer_over(owner: HWND) {
    let Some(bounds) = with_layer(owner, |layer| {
        matches!(layer.content, Some(Content::Suggest { .. })).then_some(layer.bounds)
    })
    .flatten() else {
        return;
    };
    let mut cursor = POINT::default();
    let over = unsafe { GetCursorPos(&mut cursor) }.is_ok() && hover_square::contains(bounds, cursor);
    if !over {
        suggest_close(owner);
    }
}

/// The toolbar moved the keyboard selection to row `i` of the list for edit `seq`. Stale or
/// out-of-range requests are ignored.
pub(crate) fn suggest_preview(owner: HWND, seq: u32, i: u8) {
    let moved = with_layer(owner, |layer| match &mut layer.content {
        Some(Content::Suggest { seq: shown, sel, rows, .. }) if *shown == seq && usize::from(i) < rows.len() => {
            *sel = usize::from(i);
            true
        }
        _ => false,
    });
    if moved == Some(true)
        && let Some(host) = ready_host(owner)
    {
        let _ = host.post_json(&protocol::suggest_sel_json(seq, usize::from(i)));
    }
}

/// What picking row `i` of the list for edit `seq` does; the dropdown closes. `None` when the list
/// is gone, is for another edit, or has no such row.
pub(crate) fn suggest_take(owner: HWND, seq: u32, i: u8) -> Option<Target> {
    let target = with_layer(owner, |layer| match &layer.content {
        Some(Content::Suggest { seq: shown, rows, .. }) if *shown == seq => {
            rows.get(usize::from(i)).map(|row| row.target.clone())
        }
        _ => None,
    })
    .flatten()?;
    hide(owner);
    Some(target)
}

fn queue_pick(owner: HWND, seq: u32, i: u8) {
    let window = with_layer(owner, |layer| {
        if layer.picks.len() >= ACTION_LIMIT {
            return None;
        }
        layer.picks.push_back((seq, i));
        Some(layer.window)
    })
    .flatten();
    if let Some(window) = window {
        crate::win32::post(window, WM_HOVER_PICK, WPARAM(0), LPARAM(0));
    }
}

fn run_pick(owner: HWND) {
    let Some(Some((seq, i))) = with_layer(owner, |layer| layer.picks.pop_front()) else {
        return;
    };
    if let Some(target) = suggest_take(owner, seq, i) {
        super::window::html_suggest_run(owner, target);
    }
}
// --- end omnibox suggest ---

// ----- geometry -----

fn scale_px(logical: i32, dpi: u32) -> i32 {
    (logical * dpi.max(1) as i32 + 48) / 96
}

fn grow(rect: RECT, by: i32) -> RECT {
    RECT { left: rect.left - by, top: rect.top - by, right: rect.right + by, bottom: rect.bottom + by }
}

/// Keeps a span of `size` starting at `start` inside `min..max`, preferring the low edge when the
/// span is larger than the range.
fn clamp_span(start: i32, size: i32, min: i32, max: i32) -> i32 {
    start.min(max - size).max(min)
}

/// A tooltip of `width`x`height` device pixels, centred under `anchor` with `gap`; above it when
/// there is no room below. Clamped to `work`. Pure.
fn tooltip_bounds(anchor: RECT, width: i32, height: i32, gap: i32, work: RECT) -> RECT {
    let centre = (anchor.left + anchor.right) / 2;
    let left = clamp_span(centre - width / 2, width, work.left, work.right);
    let below = anchor.bottom + gap;
    let top = if below + height <= work.bottom || anchor.top - gap - height < work.top {
        below
    } else {
        anchor.top - gap - height
    };
    let top = clamp_span(top, height, work.top, work.bottom);
    RECT { left, top, right: left + width, bottom: top + height }
}

/// A tooltip vertically centred to the right of `anchor` with `gap`; to its left when there is no
/// room on the right. Clamped to `work`. Pure.
fn tooltip_beside_bounds(anchor: RECT, width: i32, height: i32, gap: i32, work: RECT) -> RECT {
    let right = anchor.right + gap;
    let left = if right + width <= work.right || anchor.left - gap - width < work.left {
        right
    } else {
        anchor.left - gap - width
    };
    let left = clamp_span(left, width, work.left, work.right);
    let top = clamp_span((anchor.top + anchor.bottom) / 2 - height / 2, height, work.top, work.bottom);
    RECT { left, top, right: left + width, bottom: top + height }
}

/// A card or island menu of `width`x`height`, left-aligned under `anchor` with `gap`, kept `margin`
/// inside `work`; above the anchor when there is no room below. Pure.
fn overlay_bounds(anchor: RECT, width: i32, height: i32, gap: i32, margin: i32, work: RECT) -> RECT {
    let left = clamp_span(anchor.left, width, work.left + margin, work.right - margin);
    let below = anchor.bottom + gap;
    let top = if below + height <= work.bottom - margin || anchor.top - gap - height < work.top + margin {
        below
    } else {
        anchor.top - gap - height
    };
    let top = clamp_span(top, height, work.top, work.bottom);
    RECT { left, top, right: left + width, bottom: top + height }
}

fn monitor_data(point: POINT) -> (u32, RECT) {
    unsafe {
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let work = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            info.rcWork
        } else {
            RECT { left: 0, top: 0, right: 1920, bottom: 1080 }
        };
        let (mut x, mut y) = (FALLBACK_DPI, FALLBACK_DPI);
        let dpi = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y)
            .ok()
            .map_or(FALLBACK_DPI, |_| x);
        (dpi, work)
    }
}

// ----- window -----

fn register_class() -> windows::core::Result<()> {
    CLASS_REGISTERED.with(|registered| {
        if *registered.borrow() {
            return Ok(());
        }
        let instance = unsafe { GetModuleHandleW(None)? };
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err(windows::core::Error::from_thread());
        }
        *registered.borrow_mut() = true;
        Ok(())
    })
}

/// Fills the client with the opaque window colour, so nothing shows before the page paints.
/// `--bw-overlay` from tokens.css (dark, light): the tint every hover surface paints over the window.
const OVERLAY_DARK: (u8, u8, u8, f32) = (44, 44, 44, 0.94);
const OVERLAY_LIGHT: (u8, u8, u8, f32) = (252, 252, 252, 0.94);

/// The opaque colour of a hover surface: the overlay tint composited over `window`. Pure.
fn surface_colour(window: theme::Rgba, dark: bool) -> theme::Rgba {
    let (r, g, b, a) = if dark { OVERLAY_DARK } else { OVERLAY_LIGHT };
    let over = |top: u8, under: u8| (f32::from(top) * a + f32::from(under) * (1.0 - a)).round() as u8;
    theme::Rgba { r: over(r, window.r), g: over(g, window.g), b: over(b, window.b), a: 0xFF }
}

unsafe fn paint_background(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        // The page's own surface colour (the overlay tint over the window colour; for the omnibox
        // dropdown, its omnibox material), so any area the WebView has not painted yet, such as rows
        // the dropdown unrolls before its first frame at the new size, already reads as the surface
        // rather than as a band of another colour.
        let appearance = glass::appearance();
        let suggest = owner_of_window(hwnd)
            .and_then(|owner| with_layer(owner, |layer| layer.shown == Some(Kind::Suggest) || matches!(layer.content, Some(Content::Suggest { .. }))))
            == Some(true);
        let colour = if suggest {
            suggest_surface(glass::palette())
        } else {
            surface_colour(theme::palette(appearance, true).window, appearance == theme::Appearance::Dark)
        };
        let brush = CreateSolidBrush(COLORREF(
            u32::from(colour.r) | (u32::from(colour.g) << 8) | (u32::from(colour.b) << 16),
        ));
        FillRect(dc, &ps.rcPaint, brush);
        let _ = DeleteObject(brush.into());
        let _ = EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE),
        WM_ACTIVATE => {
            // WebView2 focuses its window when clicked, which activates the layer despite
            // WS_EX_NOACTIVATE. Unless the name field asked for the keyboard, hand it back.
            if (wparam.0 & 0xFFFF) as u32 != WA_INACTIVE
                && let Some(owner) = owner_of_window(hwnd)
                && with_layer(owner, |layer| !layer.keyboard) == Some(true)
            {
                crate::win32::post(hwnd, WM_HOVER_RETURN, WPARAM(0), LPARAM(0));
            }
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        WM_HOVER_RETURN => {
            if let Some(owner) = owner_of_window(hwnd)
                && with_layer(owner, |layer| !layer.keyboard) == Some(true)
                && unsafe { GetForegroundWindow() } == hwnd
            {
                unsafe {
                    let _ = SetForegroundWindow(owner);
                }
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            unsafe { paint_background(hwnd) };
            LRESULT(0)
        }
        WM_TIMER => {
            let Some(owner) = owner_of_window(hwnd) else {
                unsafe {
                    let _ = KillTimer(Some(hwnd), wparam.0);
                }
                return LRESULT(0);
            };
            match wparam.0 {
                OPEN_TIMER => {
                    unsafe {
                        let _ = KillTimer(Some(hwnd), OPEN_TIMER);
                    }
                    open_timer(owner);
                }
                WATCH_TIMER => watch_timer(owner),
                DROP_TIMER => drop_frame(owner), // --- omnibox suggest ---
                SUSPEND_TIMER => {
                    unsafe {
                        let _ = KillTimer(Some(hwnd), SUSPEND_TIMER);
                    }
                    let idle = with_layer(owner, |layer| {
                        !layer.visible && layer.content.is_none() && !layer.omnibox_focused // --- omnibox suggest ---
                    });
                    if idle == Some(true)
                        && let Some(host) = ready_host(owner)
                    {
                        let _ = host.try_suspend();
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_HOVER_RUN => {
            if let Some(owner) = owner_of_window(hwnd) {
                run_action(owner);
            }
            LRESULT(0)
        }
        // --- omnibox suggest ---
        WM_HOVER_PICK => {
            if let Some(owner) = owner_of_window(hwnd) {
                run_pick(owner);
            }
            LRESULT(0)
        }
        // --- end omnibox suggest ---
        WM_HOVER_FAILED => {
            if let Some(owner) = owner_of_window(hwnd) {
                shutdown(owner);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }

    fn tuple(r: RECT) -> (i32, i32, i32, i32) {
        (r.left, r.top, r.right, r.bottom)
    }

    const WORK: RECT = RECT { left: 0, top: 0, right: 1000, bottom: 700 };

    #[test]
    fn suspend_timer_runs_only_while_hidden_and_the_omnibox_is_unfocused() {
        assert!(suspend_timer_wanted(false, false));
        assert!(!suspend_timer_wanted(false, true));
        assert!(!suspend_timer_wanted(true, false));
        assert!(!suspend_timer_wanted(true, true));
    }

    #[test]
    fn dropdown_corner_radius_shrinks_with_a_short_or_narrow_list() {
        assert_eq!(suggest_corner_radius(18, 400, 600), 18);
        assert_eq!(suggest_corner_radius(18, 10, 600), 10);
        assert_eq!(suggest_corner_radius(18, 400, 20), 10);
        assert_eq!(suggest_corner_radius(18, 0, 600), 0);
        assert_eq!(suggest_corner_radius(18, -5, 600), 0);
    }

    #[test]
    fn the_dropdown_surface_is_the_omnibox_fill_over_the_opaque_window() {
        let rgb = |c: theme::Rgba| (c.r, c.g, c.b, c.a);
        // Glass: the fill (white at 0x1C = 0.110 / 0x50 = 0.314) over the window RGB, as the CSS
        // `--bw-omni-material` composites it.
        assert_eq!(rgb(suggest_surface(theme::palette(theme::Appearance::Dark, false))), (57, 60, 67, 255));
        assert_eq!(rgb(suggest_surface(theme::palette(theme::Appearance::Light, false))), (247, 249, 250, 255));
        // Solid: the fill is opaque, so it is the surface.
        assert_eq!(rgb(suggest_surface(theme::palette(theme::Appearance::Dark, true))), (0x2A, 0x3A, 0x3E, 255));
        assert_eq!(rgb(suggest_surface(theme::palette(theme::Appearance::Light, true))), (255, 255, 255, 255));
        // Not the hover cards' overlay colour: the list is the pill's material, not a menu's.
        let dark = theme::palette(theme::Appearance::Dark, false);
        assert_ne!(rgb(suggest_surface(dark)), rgb(surface_colour(theme::palette(theme::Appearance::Dark, true).window, true)));
    }

    #[test]
    fn the_dropdown_shadow_scales_with_dpi_and_is_firmer_in_dark() {
        let at96 = suggest_shadow(96, true);
        let at144 = suggest_shadow(144, true);
        assert_eq!(at96.margins(), (24, 32));
        assert_eq!(at144.margins(), (36, 48));
        assert_eq!(at144.key_drop, 12.0);
        let light = suggest_shadow(96, false);
        assert!(light.key_alpha < at96.key_alpha && light.ambient_alpha < at96.ambient_alpha);
        assert!(light.ambient_alpha > 0.0, "light keeps an edge: no outline, so the shadow separates it from a white page");
    }

    #[test]
    fn only_a_card_island_or_dropdown_blocks_tooltips() {
        assert!(!blocks_tooltip(None));
        assert!(!blocks_tooltip(Some(Kind::Tooltip)));
        assert!(blocks_tooltip(Some(Kind::Card)));
        assert!(blocks_tooltip(Some(Kind::Island)));
        assert!(blocks_tooltip(Some(Kind::Suggest)));
    }

    #[test]
    fn a_hidden_popup_of_ours_holding_the_foreground_is_not_the_user_going_away() {
        assert!(!user_is_away(Foreground::Here));
        assert!(!user_is_away(Foreground::Own));
        assert!(!user_is_away(Foreground::StaleOwn));
        assert!(user_is_away(Foreground::Other));
    }

    #[test]
    fn view_rectangles_scale_and_offset_into_screen_pixels() {
        let view = rect(100, 40, 900, 74);
        let r = HoverRect { x: 10, y: 2, w: 28, h: 28 };
        assert_eq!(tuple(anchor_to_screen(view, 1.0, r)), (110, 42, 138, 70));
        assert_eq!(tuple(anchor_to_screen(view, 1.5, r)), (115, 43, 157, 85));
        // A degenerate scale falls back to 1, and an empty rect still has a pixel.
        assert_eq!(tuple(anchor_to_screen(view, f32::NAN, HoverRect::default())), (100, 40, 101, 41));
    }

    #[test]
    fn tooltip_centres_below_and_flips_above_at_the_bottom_edge() {
        let below = tooltip_bounds(rect(400, 100, 440, 130), 80, 24, 6, WORK);
        assert_eq!(tuple(below), (380, 136, 460, 160));
        let above = tooltip_bounds(rect(400, 670, 440, 700), 80, 24, 6, WORK);
        assert_eq!(tuple(above), (380, 640, 460, 664));
    }

    #[test]
    fn tooltip_stays_inside_the_work_area_at_both_side_edges() {
        let left = tooltip_bounds(rect(0, 100, 20, 120), 200, 24, 6, WORK);
        assert_eq!(left.left, 0);
        let right = tooltip_bounds(rect(980, 100, 1000, 120), 200, 24, 6, WORK);
        assert_eq!(right.right, 1000);
        // Wider than the work area: pinned to its left edge.
        assert_eq!(tooltip_bounds(rect(500, 100, 520, 120), 1200, 24, 6, WORK).left, 0);
    }

    #[test]
    fn beside_tooltips_sit_right_of_the_rail_and_flip_at_the_edge() {
        let rail = tooltip_beside_bounds(rect(4, 40, 36, 72), 80, 24, 6, WORK);
        assert_eq!(tuple(rail), (42, 44, 122, 68));
        let edge = tooltip_beside_bounds(rect(960, 40, 992, 72), 80, 24, 6, WORK);
        assert_eq!(edge.right, 954);
        let top = tooltip_beside_bounds(rect(4, 0, 36, 4), 80, 24, 6, WORK);
        assert_eq!(top.top, 0);
    }

    #[test]
    fn overlay_left_aligns_below_and_keeps_its_margin() {
        let card = overlay_bounds(rect(100, 40, 300, 74), 260, 180, 4, 8, WORK);
        assert_eq!(tuple(card), (100, 78, 360, 258));
        let edge = overlay_bounds(rect(900, 40, 1000, 74), 260, 180, 4, 8, WORK);
        assert_eq!(edge.right, 992);
        let low = overlay_bounds(rect(100, 600, 300, 634), 260, 180, 4, 8, WORK);
        assert_eq!(low.bottom, 596, "flips above the trigger when there is no room below");
        // On a monitor whose work area starts away from zero.
        let offset = RECT { left: -1920, top: 0, right: 0, bottom: 1040 };
        let card = overlay_bounds(rect(-50, 40, -10, 74), 260, 180, 4, 8, offset);
        assert_eq!(card.right, -8);
    }

    #[test]
    fn dropdown_attaches_under_the_pill_at_its_exact_width() {
        // Tucked one physical pixel under the pill, never a gap, at any width the pill has.
        let pill = rect(300, 45, 852, 78);
        assert_eq!(tuple(suggest_bounds(pill, 200, WORK)), (300, 77, 852, 277));
        // A pill partly off the work area keeps its width; the dropdown slides inside it.
        let off = suggest_bounds(rect(-40, 45, 160, 78), 120, WORK);
        assert_eq!((off.left, off.right - off.left), (0, 200));
        // No room below: it stays inside the work area rather than running off it.
        assert_eq!(suggest_bounds(rect(300, 640, 700, 670), 120, WORK).bottom, 700);
    }

    #[test]
    fn dropdown_motion_unrolls_eases_out_and_closes_faster() {
        let started = Instant::now();
        let open = DropMotion { started, from: 0, closing: false };
        let ms = Duration::from_millis;
        assert_eq!(drop_height(open, 300, ms(0)), (0, false));
        let (early, _) = drop_height(open, 300, ms(20));
        assert!(early > 150, "ease-out: most of the height shows in the first quarter ({early})");
        let (late, done) = drop_height(open, 300, ms(40));
        assert!(late < 300 && late > early && !done);
        assert_eq!(drop_height(open, 300, DROP_OPEN), (300, true));
        // Re-opening during a close starts from where the close left it.
        let reopen = DropMotion { started, from: 120, closing: false };
        assert_eq!(drop_height(reopen, 300, ms(0)).0, 120);
        // Closing rolls back into the pill, in less time than opening took.
        let close = DropMotion { started, from: 300, closing: true };
        assert!(DROP_CLOSE < DROP_OPEN);
        assert!(drop_height(close, 300, ms(30)).0 < 150);
        assert_eq!(drop_height(close, 300, DROP_CLOSE), (0, true));
        // The starting height never exceeds the list (rows can disappear mid-motion).
        assert_eq!(drop_height(DropMotion { started, from: 500, closing: false }, 300, ms(0)).0, 300);
        assert_eq!(ease_out(0.0, 5), 0.0);
        assert_eq!(ease_out(1.0, 3), 1.0);
        assert_eq!(ease_out(2.0, 5), 1.0);
        // The close keeps moving to its end (cubic), so it never sits nearly done with the pill open.
        assert!(drop_height(close, 300, ms(40)).0 > 15, "still visibly closing at 40ms");
        // The pill gets its corners back about two frames before the close ends, never while opening.
        assert!(!drop_untell_due(close, ms(20)));
        assert!(drop_untell_due(close, DROP_CLOSE - DROP_UNTELL_LEAD));
        assert!(!drop_untell_due(open, DROP_OPEN));
    }

    #[test]
    fn dropdown_bottom_uses_the_pills_own_radius() {
        assert_eq!(SUGGEST_RADIUS, 13, "half the 26px pill, as --bw-r-omnibox");
        assert_eq!(scale_px(SUGGEST_RADIUS, 144), 20);
        assert_eq!(scale_px(SUGGEST_RADIUS, 192), 26);
    }

    #[test]
    fn popup_background_is_the_overlay_surface_colour() {
        // The overlay tint over the solid window colours, as the page paints it (tokens.css).
        let dark = surface_colour(theme::Rgba { r: 0x20, g: 0x24, b: 0x2C, a: 0xFF }, true);
        assert_eq!((dark.r, dark.g, dark.b, dark.a), (43, 44, 44, 0xFF));
        let light = surface_colour(theme::Rgba { r: 0xF4, g: 0xF6, b: 0xF8, a: 0xFF }, false);
        assert_eq!((light.r, light.g, light.b), (252, 252, 252));
    }

    #[test]
    fn scale_px_rounds_logical_pixels_per_dpi() {
        assert_eq!(scale_px(6, 96), 6);
        assert_eq!(scale_px(6, 144), 9);
        assert_eq!(scale_px(4, 120), 5);
        assert_eq!(scale_px(260, 192), 520);
    }

    #[test]
    fn tooltip_closes_off_its_trigger_and_on_itself() {
        let anchor = rect(100, 100, 130, 130);
        let bounds = rect(80, 136, 150, 160);
        let at = |x, y| POINT { x, y };
        let decide = |p| leave_decision(Kind::Tooltip, anchor, bounds, p, false, false, 4);
        assert_eq!(decide(at(110, 110)), hover_square::Leave::Keep);
        assert_eq!(decide(at(132, 110)), hover_square::Leave::Keep, "inside the tolerance");
        assert_eq!(decide(at(200, 110)), hover_square::Leave::Close);
        assert_eq!(decide(at(110, 140)), hover_square::Leave::Close, "pointer reached the tooltip");
        assert_eq!(
            leave_decision(Kind::Tooltip, anchor, bounds, at(110, 110), false, true, 4),
            hover_square::Leave::Close,
            "another application came to the front"
        );
    }

    #[test]
    fn card_and_island_follow_the_hover_square() {
        let anchor = rect(100, 40, 300, 74);
        let bounds = rect(100, 78, 360, 258);
        let at = |x, y| POINT { x, y };
        for kind in [Kind::Card, Kind::Island] {
            let decide = |p, focused| leave_decision(kind, anchor, bounds, p, focused, false, 4);
            assert_eq!(decide(at(150, 100), false), hover_square::Leave::Keep);
            assert_eq!(decide(at(150, 76), false), hover_square::Leave::Watch, "in the gap");
            assert_eq!(decide(at(600, 400), false), hover_square::Leave::Close);
            assert_eq!(decide(at(600, 400), true), hover_square::Leave::Keep, "the name field has the keyboard");
        }
    }

    #[test]
    fn stale_tokens_never_match_the_shown_content() {
        let layer = |tok: u32, shown: bool| Layer {
            owner: HWND::default(),
            window: HWND::default(),
            slot: Rc::new(RefCell::new(None)),
            host: None,
            next_tok: tok.wrapping_add(1),
            tok,
            content: shown.then(|| Content::Tooltip { text: "x".into(), beside: false }),
            anchor: RECT::default(),
            bounds: RECT::default(),
            visible: shown,
            state_sent: None,
            target: None,
            island: None,
            actions: VecDeque::new(),
            keyboard: false,
            picks: VecDeque::new(),
            suggest_seq: 0,
            shown: None,
            drop: None,
            drop_h: 0,
            drop_told: false,
            omnibox_focused: false,
        };
        assert!(current(&layer(5, true), 5));
        assert!(!current(&layer(5, true), 4));
        assert!(!current(&layer(5, false), 5));
        assert!(!current(&layer(0, true), 0));

        // Within one island session an older token still counts; another island's does not.
        let mut island = layer(7, true);
        island.content = Some(Content::Island { tab: TabId(3), rows: Vec::new(), model: HoverIsland::default() });
        island.island = Some(IslandSession { tab: TabId(3), first_tok: 5, committed: None });
        assert!(same_island_session(&island, 5));
        assert!(same_island_session(&island, 7));
        assert!(!same_island_session(&island, 4));
        assert!(!same_island_session(&island, 8));
        island.island = Some(IslandSession { tab: TabId(4), first_tok: 5, committed: None });
        assert!(!same_island_session(&island, 6));
        assert!(!same_island_session(&layer(7, true), 7), "a tooltip has no island session");
    }
}
