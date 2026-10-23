//! WebView2 download tracking (docs/design/downloads.md).
//!
//! `handle_starting` replaces the old "cancel every download": a download from a normal tab or web
//! app panel gets a destination (`DownloadHooks::plan`, which may be the native Save dialog), is
//! marked handled so WebView2's own download UI stays away, and its `ICoreWebView2DownloadOperation`
//! is kept here and reported through `DownloadHooks::event` (`Started`, throttled `Progress`, every
//! `State` change). `pause`, `resume` and `cancel` act on the kept operation by the key the
//! `Started` event carried. Private tabs and a missing hook still cancel, exactly as before.
//!
//! Everything here runs on the UI thread, so the operations live in a thread-local registry shared
//! by tab webviews and web app panels (one `ICoreWebView2DownloadOperation` per key). The pure parts
//! (throttle, state and reason mapping) are unit-tested; the COM calls and the Save dialog cannot be
//! exercised without a live WebView2 and an interactive desktop.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use webview2_com::{
    BytesReceivedChangedEventHandler, CoTaskMemPWSTR, EstimatedEndTimeChangedEventHandler,
    Microsoft::Web::WebView2::Win32::*, StateChangedEventHandler,
};
use windows::{
    Win32::Foundation::HWND,
    core::{BOOL, HSTRING, PWSTR, Result},
};

use crate::downloads::{DownloadEvent, DownloadPlan, DownloadRequest, OpState};

/// Progress is reported at most this often per download; state changes are never held back.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

pub type EventHook = Rc<dyn Fn(DownloadEvent)>;
pub type PlanHook = Rc<dyn Fn(&DownloadRequest) -> DownloadPlan>;
pub type PolicyHook = Rc<dyn Fn(&'static str)>;

/// What the engine is wired to. Without `event` and `plan` every download is cancelled.
#[derive(Clone, Default)]
pub struct DownloadHooks {
    pub event: Option<EventHook>,
    pub plan: Option<PlanHook>,
    pub policy: Option<PolicyHook>,
}

/// Where a download came from.
pub struct Source {
    /// Groups the downloads of one webview (see [`tab_owner`], [`app_owner`]).
    pub owner: u64,
    /// The window the Save dialog is parented to.
    pub parent: HWND,
    /// A private tab: such a download is always cancelled.
    pub private: bool,
}

pub fn tab_owner(tab: u32) -> u64 {
    u64::from(tab)
}

/// A web app panel's owner key (FNV-1a of its id, in the half of the key space tabs never use).
pub fn app_owner(app_id: &str) -> u64 {
    let hash = app_id.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    hash | (1 << 63)
}

struct Tracked {
    op: ICoreWebView2DownloadOperation,
    owner: u64,
    event: EventHook,
}

thread_local! {
    static OPS: RefCell<HashMap<u64, Tracked>> = RefCell::new(HashMap::new());
    static NEXT_KEY: Cell<u64> = const { Cell::new(1) };
    /// A Save dialog is up: a second "ask" download saves without asking rather than stacking dialogs.
    static DIALOG_OPEN: Cell<bool> = const { Cell::new(false) };
}

fn log(hooks: &DownloadHooks, name: &'static str) {
    if let Some(policy) = hooks.policy.as_ref() {
        policy(name);
    }
}

fn cancel_start(args: &ICoreWebView2DownloadStartingEventArgs, hooks: &DownloadHooks) {
    let _ = unsafe { args.SetCancel(true) };
    log(hooks, "download_cancelled");
}

/// The `DownloadStarting` handler body for every non-chrome webview.
pub fn handle_starting(args: &ICoreWebView2DownloadStartingEventArgs, source: &Source, hooks: &DownloadHooks) {
    let (Some(event), Some(plan)) = (hooks.event.clone(), hooks.plan.clone()) else {
        return cancel_start(args, hooks);
    };
    if source.private {
        return cancel_start(args, hooks);
    }
    let Some((request, op)) = read_request(args) else {
        return cancel_start(args, hooks);
    };
    // The Web Store's own installer download points at a dead host. It is dropped before `plan` or
    // `begin` can give it a destination or a list row, and Bowser's own install flow takes over.
    if crate::downloads::is_store_placeholder_url(&request.url) {
        return redirect_store_placeholder(args, &request.url, source, hooks);
    }
    match plan(&request) {
        DownloadPlan::Cancel => cancel_start(args, hooks),
        DownloadPlan::Save { path } => begin(args, &op, &path, &request, source, event, hooks),
        DownloadPlan::Ask { dir, name } => {
            if DIALOG_OPEN.get() {
                // One dialog at a time: the second download takes the default folder.
                match crate::downloads::unique_path(&dir, &name, &|p| p.exists()) {
                    Some(path) => begin(args, &op, &path, &request, source, event, hooks),
                    None => cancel_start(args, hooks),
                }
                return;
            }
            // The deferral keeps the download pending while the dialog runs its own message loop.
            let Ok(deferral) = (unsafe { args.GetDeferral() }) else {
                return cancel_start(args, hooks);
            };
            DIALOG_OPEN.set(true);
            let chosen = crate::download_shell::save_dialog(source.parent, &dir, &name);
            DIALOG_OPEN.set(false);
            match chosen {
                Some(path) => begin(args, &op, &path, &request, source, event, hooks),
                None => {
                    // Cancelling the dialog cancels the download and records nothing.
                    let _ = unsafe { args.SetCancel(true) };
                    log(hooks, "download_save_dialog_cancelled");
                }
            }
            let _ = unsafe { deferral.Complete() };
        }
    }
}

/// Cancels the Store's installer placeholder download and, when its URL names an extension, asks the
/// window to start Bowser's explicit installation flow for that id. The window re-checks the id
/// against the page the user is on and the flow still stops at its permission review; nothing is
/// installed from here, and nothing is recorded in the downloads list.
fn redirect_store_placeholder(
    args: &ICoreWebView2DownloadStartingEventArgs,
    url: &str,
    source: &Source,
    hooks: &DownloadHooks,
) {
    let _ = unsafe { args.SetCancel(true) };
    log(hooks, "download_store_placeholder_cancelled");
    if let Some(id) = crate::downloads::store_placeholder_id(url) {
        crate::win32::request_store_install(source.parent, &id);
    }
}

fn read_request(args: &ICoreWebView2DownloadStartingEventArgs) -> Option<(DownloadRequest, ICoreWebView2DownloadOperation)> {
    unsafe {
        let op = args.DownloadOperation().ok()?;
        let read_text = |read: &dyn Fn(*mut PWSTR) -> Result<()>| -> String {
            let mut text = PWSTR::null();
            if read(&mut text).is_ok() && !text.is_null() { CoTaskMemPWSTR::from(text).to_string() } else { String::new() }
        };
        let url = read_text(&|out| op.Uri(out));
        let mime = read_text(&|out| op.MimeType(out));
        let suggested = read_text(&|out| args.ResultFilePath(out));
        let mut total = -1i64;
        let _ = op.TotalBytesToReceive(&mut total);
        let suggested_name = suggested.rsplit(['\\', '/']).next().unwrap_or("").to_owned();
        Some((DownloadRequest { url, suggested_name, mime, total: total.max(-1) }, op))
    }
}

fn read_bytes(op: &ICoreWebView2DownloadOperation) -> (u64, i64) {
    let (mut got, mut total) = (0i64, -1i64);
    unsafe {
        let _ = op.BytesReceived(&mut got);
        let _ = op.TotalBytesToReceive(&mut total);
    }
    (got.max(0) as u64, total.max(-1))
}

fn begin(
    args: &ICoreWebView2DownloadStartingEventArgs,
    op: &ICoreWebView2DownloadOperation,
    path: &std::path::Path,
    request: &DownloadRequest,
    source: &Source,
    event: EventHook,
    hooks: &DownloadHooks,
) {
    let wide = HSTRING::from(path.as_os_str());
    // Handled: WebView2 shows no download UI of its own; the file goes exactly where we said.
    let routed = unsafe { args.SetResultFilePath(&wide).and_then(|()| args.SetHandled(true)) };
    if routed.is_err() {
        return cancel_start(args, hooks);
    }
    let key = NEXT_KEY.get();
    NEXT_KEY.set(key + 1);
    let throttle = Rc::new(RefCell::new(Throttle::default()));
    let mut token = 0i64;
    let progress = |event: &EventHook, throttle: &Rc<RefCell<Throttle>>| {
        let (event, throttle) = (event.clone(), throttle.clone());
        Box::new(move |sender: Option<ICoreWebView2DownloadOperation>, _: Option<windows::core::IUnknown>| {
            if let Some(op) = sender
                && is_tracked(key)
                && throttle.borrow_mut().ready(Instant::now())
            {
                let (got, total) = read_bytes(&op);
                event(DownloadEvent::Progress { key, got, total });
            }
            Ok(())
        })
    };
    let registered = unsafe {
        op.add_BytesReceivedChanged(&BytesReceivedChangedEventHandler::create(progress(&event, &throttle)), &mut token)
            .and_then(|()| {
                op.add_EstimatedEndTimeChanged(&EstimatedEndTimeChangedEventHandler::create(progress(&event, &throttle)), &mut token)
            })
            .and_then(|()| {
                let event = event.clone();
                op.add_StateChanged(
                    &StateChangedEventHandler::create(Box::new(move |sender, _| {
                        if let Some(op) = sender {
                            on_state_changed(key, &op, &event);
                        }
                        Ok(())
                    })),
                    &mut token,
                )
            })
    };
    if registered.is_err() {
        return cancel_start(args, hooks);
    }
    OPS.with(|ops| ops.borrow_mut().insert(key, Tracked { op: op.clone(), owner: source.owner, event: event.clone() }));
    log(hooks, "download_started");
    event(DownloadEvent::Started { key, url: request.url.clone(), path: path.to_path_buf(), total: request.total });
}

fn is_tracked(key: u64) -> bool {
    OPS.with(|ops| ops.borrow().contains_key(&key))
}

fn on_state_changed(key: u64, op: &ICoreWebView2DownloadOperation, event: &EventHook) {
    if !is_tracked(key) {
        return;
    }
    let (mut state, mut reason, mut can_resume) =
        (COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS, COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NONE, BOOL::default());
    unsafe {
        let _ = op.State(&mut state);
        let _ = op.InterruptReason(&mut reason);
        let _ = op.CanResume(&mut can_resume);
    }
    let can_resume = can_resume.as_bool();
    let (op_state, err) = map_state(state, reason);
    let (got, total) = read_bytes(op);
    // A finished, cancelled or unrecoverable operation is let go; a resumable failure is kept.
    let terminal = match op_state {
        OpState::Completed | OpState::Cancelled => true,
        OpState::Failed => !can_resume,
        OpState::InProgress | OpState::Paused => false,
    };
    event(DownloadEvent::State { key, state: op_state, got, total, err: err.to_owned(), can_resume });
    if terminal {
        OPS.with(|ops| ops.borrow_mut().remove(&key));
    }
}

/// Engine state and interrupt reason to what the list shows, and a short human reason for failures.
pub fn map_state(
    state: COREWEBVIEW2_DOWNLOAD_STATE,
    reason: COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON,
) -> (OpState, &'static str) {
    if state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
        (OpState::Completed, "")
    } else if state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED {
        if reason == COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_PAUSED {
            (OpState::Paused, "")
        } else if reason == COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED {
            (OpState::Cancelled, "")
        } else {
            (OpState::Failed, reason_text(reason))
        }
    } else {
        (OpState::InProgress, "")
    }
}

/// Interrupt reasons and the short text the row shows (`Failed, <reason>`, at most 60 characters).
const REASONS: [(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON, &str); 27] = [
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_FAILED, "File error"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_TRANSIENT_ERROR, "File error"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_ACCESS_DENIED, "Access denied"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_NO_SPACE, "Disk full"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_NAME_TOO_LONG, "File name too long"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_TOO_LARGE, "File too large"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_BLOCKED_BY_POLICY, "Blocked by policy"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_MALICIOUS, "Blocked"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_SECURITY_CHECK_FAILED, "Blocked"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_TOO_SHORT, "Incomplete file"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_CONTENT_LENGTH_MISMATCH, "Incomplete file"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_HASH_MISMATCH, "Corrupted file"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_TIMEOUT, "Network timeout"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_DISCONNECTED, "Network disconnected"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_FAILED, "Network error"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_INVALID_REQUEST, "Invalid request"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_SERVER_DOWN, "Server unavailable"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_FAILED, "Server error"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_NO_RANGE, "Server error"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_UNEXPECTED_RESPONSE, "Server error"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_CROSS_ORIGIN_REDIRECT, "Server error"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_BAD_CONTENT, "File not found"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_UNAUTHORIZED, "Sign-in required"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_FORBIDDEN, "Forbidden"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_CERTIFICATE_PROBLEM, "Certificate problem"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_SHUTDOWN, "Interrupted"),
    (COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_DOWNLOAD_PROCESS_CRASHED, "Interrupted"),
];

/// A short reason for the row's `Failed, <reason>` line.
pub fn reason_text(reason: COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON) -> &'static str {
    REASONS.iter().find(|(known, _)| *known == reason).map_or("Failed", |(_, text)| text)
}

/// Lets at most one progress report through per [`PROGRESS_INTERVAL`].
#[derive(Default)]
struct Throttle {
    last: Option<Instant>,
}

impl Throttle {
    fn ready(&mut self, now: Instant) -> bool {
        if self.last.is_some_and(|last| now.saturating_duration_since(last) < PROGRESS_INTERVAL) {
            return false;
        }
        self.last = Some(now);
        true
    }
}

// --- commands on a kept operation ---

fn with_op(key: u64, act: impl FnOnce(&ICoreWebView2DownloadOperation) -> Result<()>) -> bool {
    let op = OPS.with(|ops| ops.borrow().get(&key).map(|t| t.op.clone()));
    op.is_some_and(|op| act(&op).is_ok())
}

/// Pauses operation `key` (the engine then reports `Paused`).
pub fn pause(key: u64) -> bool {
    with_op(key, |op| unsafe { op.Pause() })
}

/// Resumes a paused or resumable operation (the engine then reports `InProgress`).
pub fn resume(key: u64) -> bool {
    with_op(key, |op| unsafe { op.Resume() })
}

/// Cancels operation `key` (the engine then reports `Cancelled`).
pub fn cancel(key: u64) -> bool {
    with_op(key, |op| unsafe { op.Cancel() })
}

/// Cancels and forgets operations the list no longer shows (a removed resumable failure).
pub fn release(keys: &[u64]) {
    for key in keys {
        let tracked = OPS.with(|ops| ops.borrow_mut().remove(key));
        if let Some(tracked) = tracked {
            let _ = unsafe { tracked.op.Cancel() };
        }
    }
}

fn interrupt_where(pick: impl Fn(&Tracked) -> bool) -> Vec<u64> {
    let gone: Vec<(u64, Tracked)> = OPS.with(|ops| {
        let mut ops = ops.borrow_mut();
        let keys: Vec<u64> = ops.iter().filter(|(_, t)| pick(t)).map(|(k, _)| *k).collect();
        keys.into_iter().filter_map(|k| ops.remove(&k).map(|t| (k, t))).collect()
    });
    let mut keys = Vec::new();
    for (key, tracked) in gone {
        // Removed first, so the cancel's own StateChanged is not reported a second time.
        let (got, total) = read_bytes(&tracked.op);
        let _ = unsafe { tracked.op.Cancel() };
        (tracked.event)(DownloadEvent::State {
            key,
            state: OpState::Failed,
            got,
            total,
            err: "Interrupted".to_owned(),
            can_resume: false,
        });
        keys.push(key);
    }
    keys
}

/// A webview is closing: its downloads cannot outlive it, so each is cancelled and reported
/// `Failed` `Interrupted`. Returns the keys.
pub fn interrupt_owner(owner: u64) -> Vec<u64> {
    interrupt_where(|t| t.owner == owner)
}

/// The engine is closing: every download is interrupted.
pub fn interrupt_all() -> Vec<u64> {
    interrupt_where(|_| true)
}

/// Whether the webview `owner` has a download moving (a tab with one must not be put to sleep).
pub fn owner_downloading(owner: u64) -> bool {
    OPS.with(|ops| {
        ops.borrow().values().any(|t| {
            t.owner == owner && {
                let mut state = COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED;
                unsafe { t.op.State(&mut state).is_ok() && state == COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS }
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_limited_to_four_a_second() {
        let mut throttle = Throttle::default();
        let t0 = Instant::now();
        assert!(throttle.ready(t0), "the first report goes out at once");
        assert!(!throttle.ready(t0 + Duration::from_millis(100)));
        assert!(!throttle.ready(t0 + Duration::from_millis(249)));
        assert!(throttle.ready(t0 + Duration::from_millis(250)));
        assert!(!throttle.ready(t0 + Duration::from_millis(300)), "the window restarts at the last report");
        assert!(throttle.ready(t0 + Duration::from_millis(500)));
        let delivered = (0..1000u64).filter(|n| throttle.ready(t0 + Duration::from_millis(1000 + n))).count();
        assert!((3..=4).contains(&delivered), "{delivered} reports in one second");
        assert!(!throttle.ready(t0), "a clock that steps back does not panic or let a burst through");
    }

    #[test]
    fn engine_states_map_to_list_states() {
        let none = COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NONE;
        assert_eq!(map_state(COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS, none), (OpState::InProgress, ""));
        assert_eq!(map_state(COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED, none), (OpState::Completed, ""));
        assert_eq!(
            map_state(COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED, COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_PAUSED),
            (OpState::Paused, "")
        );
        assert_eq!(
            map_state(COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED, COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED),
            (OpState::Cancelled, "")
        );
        assert_eq!(
            map_state(COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED, COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_NO_SPACE),
            (OpState::Failed, "Disk full")
        );
    }

    #[test]
    fn every_interrupt_reason_has_a_short_text() {
        for (reason, text) in REASONS {
            assert!(!text.is_empty() && text.chars().count() <= 60, "{reason:?} -> {text:?}");
            assert_eq!(reason_text(reason), text);
        }
        let mut seen: Vec<i32> = REASONS.iter().map(|(reason, _)| reason.0).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), REASONS.len(), "no reason is listed twice");
        assert_eq!(reason_text(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NONE), "Failed");
        assert_eq!(reason_text(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_TOO_LARGE), "File too large");
        assert_eq!(reason_text(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_FAILED), "Network error");
        assert_eq!(
            reason_text(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_INVALID_REQUEST),
            "Invalid request",
            "a request WebView2 refused to send reads differently from a network that failed"
        );
        assert_eq!(reason_text(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_MALICIOUS), "Blocked");
    }

    #[test]
    fn owner_keys_keep_tabs_and_apps_apart() {
        assert_eq!(tab_owner(7), 7);
        assert_ne!(app_owner("notes"), app_owner("mail"));
        assert_eq!(app_owner("notes"), app_owner("notes"));
        assert!(app_owner("notes") >= 1 << 63 && tab_owner(u32::MAX) < 1 << 63);
    }

    #[test]
    fn commands_on_an_unknown_operation_do_nothing() {
        assert!(!pause(123_456) && !resume(123_456) && !cancel(123_456));
        assert!(interrupt_owner(123_456).is_empty());
        release(&[123_456]);
        assert!(!owner_downloading(123_456));
    }

    #[test]
    fn the_cancel_paths_stay_wired_for_private_tabs_and_missing_hooks() {
        // handle_starting is not unit-testable without WebView2; keep the guarantees from being
        // refactored away: private tabs and an unwired engine cancel and log `download_cancelled`.
        let source = include_str!("download.rs");
        let production = &source[..source.find("#[cfg(test)]").expect("test module")];
        for needle in [
            "let (Some(event), Some(plan)) = (hooks.event.clone(), hooks.plan.clone()) else {",
            "if source.private {",
            "if crate::downloads::is_store_placeholder_url(&request.url) {",
            "return redirect_store_placeholder(args, &request.url, source, hooks);",
            "log(hooks, \"download_cancelled\")",
            "args.SetCancel(true)",
            "args.SetHandled(true)",
        ] {
            assert!(production.contains(needle), "missing in engine/download.rs: {needle}");
        }
    }
}
