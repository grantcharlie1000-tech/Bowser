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
            CreateRoundRectRgn, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO,
            MonitorFromPoint, SetWindowRgn,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
            Input::KeyboardAndMouse::{GetFocus, SetFocus},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow, HWND_TOP,
                IsWindow, RegisterClassW, SW_HIDE, SW_SHOW, SetForegroundWindow, SetWindowPos,
                ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_NCCREATE, WM_NCDESTROY, WNDCLASSW,
                WS_EX_TOOLWINDOW, WS_POPUP,
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
        json::Json,
        protocol::Command,
    },
    win32::menu::{self, Chosen, Entry, Icon, Menu},
};

const CLASS_NAME: PCWSTR = w!("BowserHtmlPopup");
const MAX_ITEMS: usize = 128;
const MAX_TOTAL_ITEMS: usize = 256;
const MAX_LABEL: usize = 128;
const MAX_SHORTCUT: usize = 48;
const MAX_FOOTER: usize = 256;
const REOPEN_GUARD: Duration = Duration::from_millis(300);
const FALLBACK_DPI: u32 = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placement {
    AboveLeft,
    BelowLeft,
}

pub(crate) type ResultCallback = Box<dyn FnOnce(&AppRef, Option<Chosen>)>;

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
    anchor: POINT,
    placement: Placement,
    dpi: u32,
    work: RECT,
    bounds: RECT,
    context: Option<MenuContext>,
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
                    anchor: POINT::default(),
                    placement: Placement::BelowLeft,
                    dpi,
                    work,
                    bounds: RECT::default(),
                    context: None,
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
    if !chrome_host::html_ui_enabled() || kind.is_empty() || kind.len() > 32 {
        return Err(ShowRefused::Unavailable(on_result));
    }
    // The host is adopted (and suspended while hidden) only once its page has said `ready`:
    // adopting it earlier would suspend the page before it loads, and a menu posted then is lost.
    let key = owner.0 as isize;
    if !PAGE_READY.with(|ready| ready.borrow().contains(&key)) {
        super::window::perf_event(owner, "html_popup_fallback");
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
        let mut ids = HashSet::new();
        let token = popup.next_token;
        let json = menu_json(token, kind, menu, &mut ids).ok_or(false)?;
        if !popup.dismiss.open(kind, now) {
            return Err(true);
        }
        popup.token = token;
        popup.next_token = popup.next_token.wrapping_add(1).max(1);
        popup.enabled_ids = ids;
        popup.anchor = anchor;
        popup.placement = placement;
        popup.context = context;
        (popup.dpi, popup.work) = monitor_data(anchor);
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
        | Command::PopupDismiss { .. }) => crate::win32::window::html_popup_command(owner, command),
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
    let tok = POPUPS.with(|p| {
        p.borrow()
            .get(&(owner.0 as isize))
            .filter(|p| p.dismiss.is_active())
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

pub(super) fn show_sized(owner: HWND, token: u32, w: u16, h: u16) {
    let key = owner.0 as isize;
    let Some((window, host, anchor, placement, dpi, work)) = POPUPS.with(|p| {
        p.borrow().get(&key).and_then(|popup| {
            (popup.dismiss.is_active() && popup.token == token).then(|| {
                (
                    popup.window,
                    popup.host.clone(),
                    popup.anchor,
                    popup.placement,
                    popup.dpi,
                    popup.work,
                )
            })
        })
    }) else {
        return;
    };
    let bounds = popup_bounds(anchor, i32::from(w), i32::from(h), dpi, work, placement);
    POPUPS.with(|p| {
        if let Some(popup) = p.borrow_mut().get_mut(&key) {
            popup.bounds = bounds;
        }
    });
    let radius = ((12 * dpi + 48) / 96).max(1) as i32;
    unsafe {
        let region = CreateRoundRectRgn(
            0,
            0,
            bounds.right - bounds.left + 1,
            bounds.bottom - bounds.top + 1,
            radius * 2,
            radius * 2,
        );
        if !region.is_invalid() {
            let _ = SetWindowRgn(window, Some(region), true);
        }
        let _ = SetWindowPos(
            window,
            Some(HWND_TOP),
            bounds.left,
            bounds.top,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            Default::default(),
        );
        let _ = host.set_bounds(RECT {
            left: 0,
            top: 0,
            right: bounds.right - bounds.left,
            bottom: bounds.bottom - bounds.top,
        });
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = host.focus();
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
        out.array("entries", |rows| write_entries(rows, &menu.entries));
    }))
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

fn write_entries(rows: &mut crate::chrome_host::json::Array<'_>, entries: &[Entry]) {
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
                if let Some(icon) = icon_token(item.icon) {
                    out.string("icon", icon);
                }
                if let Some(children) = &item.submenu_entries {
                    out.array("submenu", |rows| write_entries(rows, children));
                }
            }),
        }
    }
}
fn icon_token(icon: Icon) -> Option<&'static str> {
    Some(match icon {
        Icon::None => return None,
        Icon::Plus => "plus",
        Icon::Person => "person",
        Icon::Grid => "grid",
        Icon::Key => "key",
        Icon::Shield => "shield",
        Icon::Gear => "gear",
        Icon::Close => "close",
        Icon::Check => "check",
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
    };
    RECT {
        left: origin.x,
        top: origin.y,
        right: origin.x + width,
        bottom: origin.y + height,
    }
}

fn monitor_data(anchor: POINT) -> (u32, RECT) {
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
                h: 100
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
