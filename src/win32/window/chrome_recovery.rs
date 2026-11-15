//! Revocable leases belong to the window, not its reusable HWND. Posted failures carry the lease.
use super::*;
use std::{cell::Cell, rc::Weak, sync::atomic::{AtomicUsize, Ordering}};

pub(super) const EVENT: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x6D1;
pub(super) const INPUT_RELEASED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x6D2;
pub(super) const TIMER: usize = 0x5f3;
const READY_TIMEOUT: Duration = Duration::from_secs(15);
static NEXT_LEASE: AtomicUsize = AtomicUsize::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct NativeInput { text: String, start: usize, end: usize, focused: bool, composing: bool }
fn native_input(hwnd: HWND) -> Option<NativeInput> {
    let (edit, composing) = with_chrome(hwnd, |s| (s.edit, s.native_ime_composing))?;
    if edit.is_invalid() { return None; }
    let (start, end) = selection(edit);
    Some(NativeInput { text: read_edit(edit), start, end, focused: unsafe { GetFocus() == edit }, composing })
}

#[derive(Clone)]
pub(in crate::win32) struct Lease { owner: Weak<Cell<usize>>, pub(super) token: usize }
impl Lease {
    pub(in crate::win32) fn current(&self) -> bool {
        self.token != 0 && self.owner.upgrade().is_some_and(|owner| owner.get() == self.token)
    }
}

/// Shared by persistent chrome and the split/start-page host. Only leases escape the owner.
#[derive(Default)]
pub(in crate::win32) struct Owner(Rc<Cell<usize>>);
impl Owner {
    pub(in crate::win32) fn begin(&self) -> Lease {
        self.0.set(NEXT_LEASE.fetch_add(1, Ordering::Relaxed));
        self.lease()
    }
    pub(in crate::win32) fn lease(&self) -> Lease {
        Lease { owner: Rc::downgrade(&self.0), token: self.0.get() }
    }
    pub(in crate::win32) fn revoke(&self) { self.0.set(0); }
}

enum Notice { Command(ChromeCommand), Created(windows::core::Result<ChromeHost>) }

type ActionScopes = RefCell<Vec<(HWND, Rc<dyn Fn() -> bool>)>>;
thread_local! { static ACTION_SCOPES: ActionScopes = RefCell::new(Vec::new()); }
pub(super) struct ActionScope(bool);
impl Drop for ActionScope {
    fn drop(&mut self) { if self.0 { ACTION_SCOPES.with(|scopes| { scopes.borrow_mut().pop(); }); } }
}
/// Carry document authority through the existing second-stage modal-command queue. The closure
/// owns only a weak host, so dropping queued work while detaching window state cannot pump COM.
pub(super) fn action_scope(hwnd: HWND, index: usize, command: &ChromeCommand) -> ActionScope {
    if let ChromeCommand::ChromeAction { document, .. } = command {
        if let Some(host) = host(hwnd, index) {
            let current = host.document_lease(document.clone());
            ACTION_SCOPES.with(|scopes| scopes.borrow_mut().push((hwnd, current)));
            return ActionScope(true);
        }
    }
    ActionScope(false)
}
pub(super) fn action_lease(hwnd: HWND) -> Option<Rc<dyn Fn() -> bool>> {
    ACTION_SCOPES.with(|scopes| scopes.borrow().iter().rev().find(|(window, _)| *window == hwnd).map(|(_, current)| current.clone()))
}
pub(super) struct DeferredAction {
    pub(super) command: DeferredChromeCommand,
    pub(super) current: Option<Rc<dyn Fn() -> bool>>,
}

pub(super) struct Lifetimes {
    owners: [Owner; 3],
    deadlines: [Option<Instant>; 3],
    inboxes: [Rc<RefCell<Vec<Notice>>>; 3],
    pub(super) environments: Environments,
}
impl Lifetimes {
    pub(super) fn new() -> Self {
        Self { owners: std::array::from_fn(|_| Owner::default()), deadlines: [None; 3], inboxes: std::array::from_fn(|_| Rc::new(RefCell::new(Vec::new()))), environments: Environments::default() }
    }
    pub(super) fn begin(&mut self, now: Instant) {
        for index in 0..3 {
            self.owners[index].begin();
            self.deadlines[index] = Some(now + READY_TIMEOUT);
        }
    }
    pub(super) fn lease(&self, index: usize) -> Lease {
        self.owners[index].lease()
    }
    pub(super) fn accepts(&self, index: usize, token: usize) -> bool {
        token != 0 && self.owners[index].0.get() == token
    }
    pub(super) fn navigating(&mut self, index: usize, now: Instant) { if self.deadlines[index].is_none() { self.deadlines[index] = Some(now + READY_TIMEOUT); } }
    pub(super) fn ready(&mut self, index: usize) { self.deadlines[index] = None; }
    pub(super) fn revoke(&mut self, index: usize) {
        self.owners[index].revoke();
        self.deadlines[index] = None;
    }
    pub(super) fn revoke_all(&mut self) { for index in 0..3 { self.revoke(index); } }
    fn expired(&self, now: Instant) -> [bool; 3] { self.deadlines.map(|until| until.is_some_and(|until| now >= until)) }
    fn delay(&self, now: Instant) -> Option<u32> {
        self.deadlines.iter().flatten().min().map(|until| until.saturating_duration_since(now).as_millis().clamp(1, u32::MAX as u128) as u32)
    }
}

pub(super) fn schedule(hwnd: HWND) {
    unsafe { let _ = KillTimer(Some(hwnd), TIMER); }
    if window_closing(hwnd) { return; }
    if let Some(delay) = with_chrome(hwnd, |s| s.chrome_lifetimes.delay(Instant::now())).flatten()
        && unsafe { SetTimer(Some(hwnd), TIMER, delay, None) } == 0
    {
        // A timer resource failure must not leave the native controls waiting indefinitely.
        html_failed(hwnd); html_tabs_failed(hwnd); html_rail_failed(hwnd);
    }
}

pub(super) fn tick(hwnd: HWND, now: Instant) {
    unsafe { let _ = KillTimer(Some(hwnd), TIMER); }
    if window_closing(hwnd) { return; }
    let expired = with_chrome(hwnd, |s| s.chrome_lifetimes.expired(now)).unwrap_or([false; 3]);
    for (failed, fallback) in expired.into_iter().zip([html_failed as fn(HWND), html_tabs_failed, html_rail_failed]) {
        if failed { fallback(hwnd); }
    }
    let _ = with_chrome(hwnd, |s| {
        if s.html_failed.iter().all(|failed| *failed) { s.html_env_waiting = false; }
    });
    schedule(hwnd);
}

pub(super) fn accepts_failure(hwnd: HWND, index: usize, token: usize) -> bool {
    !window_closing(hwnd) && with_chrome(hwnd, |s| s.chrome_lifetimes.accepts(index, token)).unwrap_or(false)
}

#[allow(clippy::type_complexity)]
pub(super) fn callbacks(hwnd: HWND, index: usize, view: View, _command: fn(HWND, ChromeCommand), failed: u32)
    -> Option<(Lease, Rc<dyn Fn(ChromeCommand)>, AccelHandler, Rc<dyn Fn()>)>
{
    let lease = with_chrome(hwnd, |s| s.chrome_lifetimes.lease(index))?;
    if !lease.current() || window_closing(hwnd) { return None; }
    let inbox = with_chrome(hwnd, |s| s.chrome_lifetimes.inboxes[index].clone())?;
    let commands = lease.clone();
    let accelerators = lease.clone();
    let failures = lease.clone();
    Some((lease,
        Rc::new(move |value| {
            if !commands.current() { return; }
            let post = {
                let mut queue = inbox.borrow_mut();
                if queue.len() >= 512 {
                    unsafe { let _ = PostMessageW(Some(hwnd), failed, WPARAM(commands.token), LPARAM(0)); }
                    return;
                }
                let first = queue.is_empty(); queue.push(Notice::Command(value)); first
            };
            if post { unsafe { let _ = PostMessageW(Some(hwnd), EVENT, WPARAM(commands.token), LPARAM(index as isize)); } }
        }),
        Rc::new(move |value| accelerators.current() && app_of(hwnd).is_some_and(|app| app.try_borrow().is_ok_and(|a| !a.closing)) && WINDOWS.with(|all| all.try_borrow().is_ok()) && html_accelerator(hwnd, value, view)),
        Rc::new(move || {
            if failures.current() {
                unsafe { let _ = PostMessageW(Some(hwnd), failed, WPARAM(failures.token), LPARAM(0)); }
            }
        })))
}

/// A wake carries no environment authority. Only the App's current settled generation can
/// consume an attempt, once; no idle probing or engine startup lives here.
#[derive(Default)]
pub(super) struct Environments { last: Option<u64>, recovery: bool }
impl Environments {
    pub(super) fn lost(&mut self) { self.recovery = true; }
    pub(super) fn eligible(&self, waiting: bool, generation: u64) -> bool {
        (waiting || self.recovery) && self.last.is_none_or(|last| generation > last)
    }
    pub(super) fn begin(&mut self, generation: u64) { self.last = Some(generation); self.recovery = false; }
}

pub(super) fn dispatch(hwnd: HWND, index: usize, token: usize) {
    if index >= 3 || !accepts_failure(hwnd, index, token) { return; }
    let queue = with_chrome(hwnd, |s| std::mem::take(&mut *s.chrome_lifetimes.inboxes[index].borrow_mut())).unwrap_or_default();
    for notice in queue {
        if !accepts_failure(hwnd, index, token) { break; }
        match notice {
            Notice::Command(command) => [html_command as fn(HWND, ChromeCommand), html_tabs_command, html_rail_command][index](hwnd, command),
            Notice::Created(result) => {
                if let Some(lease) = with_chrome(hwnd, |s| s.chrome_lifetimes.lease(index)) {
                    [html_created, html_tabs_created, html_rail_created][index](hwnd, lease, result);
                }
            }
        }
    }
}

fn host(hwnd: HWND, index: usize) -> Option<ChromeHost> {
    with_chrome(hwnd, |s| match index { 0 => s.html.clone(), 1 => s.html_tabs.clone(), _ => s.html_rail.clone() }).flatten()
}

fn fallback(hwnd: HWND, index: usize, host: &ChromeHost, epoch: Option<u64>) {
    // COM failure may have pumped close/navigation/replacement. Never fail the replacement.
    if !host.owner_current() || host.document_epoch() != epoch { return; }
    [html_failed as fn(HWND), html_tabs_failed, html_rail_failed][index](hwnd);
    schedule(hwnd);
}

fn reset_state(hwnd: HWND, index: usize) {
    let _ = with_chrome(hwnd, |s| {
        s.html_state_generation[index] = s.html_state_generation[index].wrapping_add(1);
        match index {
            0 => { s.html_last_state = None; s.html_dl_last = None; s.html_ext_last = None; s.html_text_sent = None; s.html_synced_tab = None; }
            1 => { s.html_tabs_last = None; s.html_tabs_last_state = None; s.html_tabs_layout = None; s.html_tabs_opts = None; s.html_tabs_workspace = None; s.html_favicons_sent.clear(); }
            _ => { s.html_rail_last = None; s.html_rail_last_state = None; s.html_rail_favs_sent.clear(); }
        }
    });
}

pub(super) fn initial_state(hwnd: HWND, index: usize, host: &ChromeHost) {
    let epoch = host.document_epoch();
    if host.begin_initial_state().is_none() { return; }
    reset_state(hwnd, index);
    let _ = with_chrome(hwnd, |s| s.html_state_building[index] = true);
    if index == 0 {
        html_refresh_extensions_button(hwnd);
        if let Some(app) = app_of(hwnd) { super::super::downloads_ui::install(hwnd, &app); }
    }
    html_sync(hwnd);
    match index {
        0 => {
            let input = native_input(hwnd);
            if let Some(input) = &input {
                if input.focused {
                    if input.text.encode_utf16().count() > chrome_host::protocol::MAX_TEXT_CHARS { fallback(hwnd, index, host, epoch); return; }
                    let json = chrome_host::protocol::native_text_json(&input.text, input.start, input.end);
                    let _ = host.post_json(&json);
                }
            }
            let _ = with_chrome(hwnd, |s| s.html_initial_native_input = input);
        }
        1 => html_tabs_sync(hwnd),
        _ => html_rail_sync(hwnd),
    }
    let _ = with_chrome(hwnd, |s| s.html_state_building[index] = false);
    if host.finish_initial_state().is_err() { fallback(hwnd, index, host, epoch); }
}

pub(super) fn input_released(hwnd: HWND, token: usize) {
    if !accepts_failure(hwnd, 0, token) { return; }
    if let Some(id) = host(hwnd, 0).and_then(|host| host.applied_identity()) {
        let _ = document_command(hwnd, 0, ChromeCommand::ChromeApplied(id));
    }
}

/// Runs only on the deferred window dispatch, never inside a COM callback.
pub(super) fn document_command(hwnd: HWND, index: usize, command: ChromeCommand) -> Option<ChromeCommand> {
    let host = host(hwnd, index)?;
    let epoch = host.document_epoch();
    match command {
        ChromeCommand::ChromeAction { document, command, .. } => {
            (host.applied() && host.document_current(&document)).then_some(*command)
        }
        ChromeCommand::Ready | ChromeCommand::ChromeFailed(_) => None,
        ChromeCommand::ChromeNavigating(epoch) => {
            if host.document_epoch() != Some(epoch) { return None; }
            let edit = with_chrome(hwnd, |s| {
                s.chrome_lifetimes.navigating(index, Instant::now());
                match index { 0 => { s.html_page_ready = false; s.html_toolbar_active = false; s.html_omnibox_focused = false; }, 1 => s.html_tabs_page_ready = false, _ => s.html_rail_page_ready = false }
                s.sync_html_bounds(); s.sync_html_tabs_bounds(); s.sync_html_rail_bounds();
                s.edit
            });
            if index == 0 { if let Some(edit) = edit.filter(|edit| !edit.is_invalid()) { unsafe { let _ = ShowWindow(edit, SW_SHOW); } } }
            reset_state(hwnd, index);
            repaint(hwnd);
            schedule(hwnd);
            None
        }
        ChromeCommand::ChromeReady { document, .. } => {
            if host.document_current(&document) { initial_state(hwnd, index, &host); }
            None
        }
        ChromeCommand::ChromeApplied(id) => {
            if !host.document_current(&id.document) { return None; }
            let edit = with_chrome(hwnd, |s| s.edit).unwrap_or_default();
            let input = (index == 0).then(|| native_input(hwnd)).flatten();
            // A composing EDIT keeps the keyboard until WM_IME_ENDCOMPOSITION. The same
            // deadline remains armed; a stuck composition/document never removes native input.
            if input.as_ref().is_some_and(|input| input.composing) { return None; }
            let input_changed = index == 0 && with_chrome(hwnd, |s| s.html_initial_native_input != input).unwrap_or(false);
            if host.needs_initial_refresh(&id) || input_changed { initial_state(hwnd, index, &host); return None; }
            let bounds = with_chrome(hwnd, |s| {
                let (top, bottom) = s.ah.band_shift();
                match index {
                    0 => chrome_host::toolbar_bounds(&s.layout).filter(|_| !s.ah.visually_hidden(crate::auto_hide::Zone::Top)).map(|r| shift_rect(r, 0, if s.layout.toolbar_at_bottom() { bottom } else { top })),
                    1 => s.html_tabs_bounds().filter(|_| !s.ah.visually_hidden(if s.sidebar.is_some() { crate::auto_hide::Zone::Sidebar } else { crate::auto_hide::Zone::Top })).map(|r| if s.sidebar.is_some() { shift_rect(r, s.ah.sidebar_shift(), 0) } else { shift_rect(r, 0, if r.top >= s.layout.client.height() / 2 { bottom } else { top }) }),
                    _ => chrome_host::rail_bounds(&s.layout),
                }
            }).flatten();
            if !matches!(host.reveal_applied(&id, bounds.unwrap_or_default(), bounds.is_some()), Ok(true)) { fallback(hwnd, index, &host, epoch); return None; }
            if !host.applied() || !host.document_current(&id.document) { return None; }
            let _ = with_chrome(hwnd, |s| {
                s.chrome_lifetimes.ready(index);
                match index { 0 => {
                    s.html_page_ready = true; s.html_toolbar_active = true;
                    if let Some(input) = input.as_ref().filter(|input| input.focused) {
                        s.html_omnibox_focused = true;
                        s.edit_text.clone_from(&input.text);
                        s.omnibox_edited = true;
                    }
                }, 1 => s.html_tabs_page_ready = true, _ => s.html_rail_page_ready = true }
                s.sync_html_bounds(); s.sync_html_tabs_bounds(); s.sync_html_rail_bounds();
            });
            if index == 0 {
                if let Some(input) = input.filter(|input| input.focused) {
                    let _ = with_chrome(hwnd, |s| s.html_input_handoff = true);
                    let focused = host.post_json(&chrome_host::protocol::focus_native_json(input.start, input.end)).and_then(|()| host.focus_document(&id.document));
                    let _ = with_chrome(hwnd, |s| s.html_input_handoff = false);
                    if focused.is_err() { fallback(hwnd, index, &host, epoch); return None; }
                    if !host.applied() || !host.document_current(&id.document) { return None; }
                }
                if !edit.is_invalid() { unsafe { let _ = ShowWindow(edit, SW_HIDE); } }
                perf_event(hwnd, "html_toolbar_ready");
                if let Some(app) = app_of(hwnd) { let _ = app.borrow().log.record("html_chrome_shown"); }
                request_popup_prewarm(hwnd);
            }
            sync_html_host_bounds(hwnd);
            repaint(hwnd);
            schedule(hwnd);
            None
        }
        other => host.applied().then_some(other),
    }
}

/// Creation callbacks obey the same deferred dispatch boundary as document messages.
pub(super) fn creation(hwnd: HWND, index: usize, lease: Lease) -> Option<impl FnOnce(windows::core::Result<ChromeHost>)> {
    let queue = with_chrome(hwnd, |s| s.chrome_lifetimes.inboxes[index].clone())?;
    Some(move |result: windows::core::Result<ChromeHost>| {
        if !lease.current() { if let Ok(host) = result { host.close(); } return; }
        let first = { let mut q = queue.borrow_mut(); let first = q.is_empty(); q.push(Notice::Created(result)); first };
        if first { unsafe { let _ = PostMessageW(Some(hwnd), EVENT, WPARAM(lease.token), LPARAM(index as isize)); } }
    })
}

#[cfg(test)]
mod restoration_tests {
    use super::*;
    #[test]
    fn repeated_browser_losses_allow_one_attempt_per_new_live_generation() {
        let mut attempts = Environments::default();
        assert!(attempts.eligible(true, 1));
        attempts.begin(1);
        assert!(!attempts.eligible(true, 1));
        attempts.lost();
        assert!(!attempts.eligible(false, 1));
        assert!(!attempts.eligible(false, 0));
        assert!(attempts.eligible(false, 2));
        attempts.begin(2);
        assert!(!attempts.eligible(false, 2));
        attempts.lost();
        assert!(!attempts.eligible(false, 1));
        assert!(!attempts.eligible(false, 2));
        assert!(attempts.eligible(false, 3));
        attempts.begin(3);
        assert!(!attempts.eligible(false, 4), "a wake cannot recreate healthy hosts");
    }
}

/// Detach queued controller completions under the window borrow; drop/Close only after it ends.
pub(super) fn discard(hwnd: HWND, index: Option<usize>) {
    let detached = with_chrome(hwnd, |s| {
        let mut notices = Vec::new();
        for (i, inbox) in s.chrome_lifetimes.inboxes.iter().enumerate().take(3) { if index.is_none_or(|index| index == i) { notices.extend(std::mem::take(&mut *inbox.borrow_mut())); } }
        notices
    });
    drop(detached);
}

pub(super) fn host_created(hwnd: HWND, index: usize) {
    if let Some(host) = host(hwnd, index) { initial_state(hwnd, index, &host); }
}
