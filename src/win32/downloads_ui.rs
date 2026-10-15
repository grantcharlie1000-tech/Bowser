//! The toolbar's download button and the downloads bubble (docs/design/downloads.md): the window's
//! side of `crate::downloads`, for the HTML chrome only (the GDI chrome shows no button; downloads
//! still save, and Ctrl+J opens the download folder).
//!
//! - The list tells this module about every change (`App::set_downloads_observer`); the observer only
//!   posts `WM_DOWNLOADS_CHANGED`, and [`changed`] runs from the window procedure. Byte progress is
//!   passed on at most four times a second (a trailing timer sends the last one); every other change
//!   goes at once.
//! - Each flush sends the toolbar `dlbtn` (and reserves the button's slot beside the pill while it
//!   shows, `window::html_set_download_button`), re-posts the open bubble under its own token, and
//!   tells a shown downloads page to refresh.
//! - The bubble is the popup page `downloads` (`html_popup::show_downloads`), hung under the button
//!   and right-aligned to it. A download starting opens it by itself when this window is the
//!   foreground window with the HTML chrome ready, without taking the keyboard (the popup is shown
//!   passively); the button toggles it otherwise. It closes like every popup (outside click, Esc,
//!   another tab, the button again) and marks finished downloads seen when it opens and when it closes.
//! - `dlact` is checked against the open bubble's token here, and the action itself, the id and its
//!   legality against the list (`App::download_act`); nothing the page sends is a path or a URL.
//!
//! Every function runs on the UI thread. No `RefCell` borrow of the runtime table is held while
//! calling into `App`, WebView2 or window functions, which may re-enter this module.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{GetForegroundWindow, IsIconic, KillTimer, SetTimer},
};

use crate::{
    app::{App, AppRef, DownloadFollowup},
    chrome_host::protocol::HoverRect,
    downloads::{Action, Change},
};

use super::html_popup::{self, Anchor, Placement, ShowRefused};

/// Least time between two flushes caused by byte progress (about four a second).
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// The open bubble: its popup token, whether a download start opened it, and the message last sent
/// (so an unchanged list is not posted again).
struct Bubble {
    tok: u32,
    auto: bool,
    sent: String,
}

#[derive(Default)]
struct Runtime {
    installed: bool,
    /// When the last flush ran, for the progress throttle.
    last_flush: Option<Instant>,
    timer_armed: bool,
    /// `Toolbar::burst` at the last flush: a change is a download start.
    burst: u32,
    bubble: Option<Bubble>,
    /// The button's rectangle as the toolbar page last reported it (toolbar CSS px).
    rect: Option<HoverRect>,
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

/// How long a progress flush must still wait: `None` when it may go now. Pure.
fn throttle_wait(last: Option<Instant>, now: Instant) -> Option<Duration> {
    let wait = PROGRESS_INTERVAL.saturating_sub(now.saturating_duration_since(last?));
    (!wait.is_zero()).then_some(wait)
}

/// Whether a bubble action closes the bubble after it ran: the ones that leave it for another
/// surface (the file, Explorer, the page, a new tab). A refused or failed one keeps it open so the
/// row can say what happened. Pure.
fn leaves_bubble(action: Action, followup: &DownloadFollowup) -> bool {
    if matches!(followup, DownloadFollowup::Rejected | DownloadFollowup::Failed) {
        return false;
    }
    matches!(action, Action::Open | Action::Show | Action::OpenPage | Action::OpenFolder | Action::Retry)
}

/// Starts listening to the downloads list (once) and sends the toolbar page the button's current
/// state. Called whenever the toolbar page becomes ready, so a reloaded page learns it again.
pub(crate) fn install(owner: HWND, app: &AppRef) {
    let first = with(owner, |rt| !std::mem::replace(&mut rt.installed, true)).unwrap_or(false);
    if first {
        let raw = owner.0 as isize;
        let observer: Rc<dyn Fn(Change)> = Rc::new(move |change| {
            let kind = usize::from(change != Change::Progress);
            super::post(HWND(raw as *mut _), super::WM_DOWNLOADS_CHANGED, WPARAM(kind), LPARAM(0));
        });
        App::set_downloads_observer(app, Some(observer));
        let burst = App::downloads_toolbar(app).burst;
        let _ = with(owner, |rt| rt.burst = burst);
    }
    push_button(owner, app);
}

/// Forgets this window (it is closing).
pub(crate) fn close(owner: HWND) {
    let armed = RUNTIMES
        .with(|runtimes| runtimes.try_borrow_mut().ok().and_then(|mut all| all.remove(&(owner.0 as isize))))
        .is_some_and(|runtime| runtime.timer_armed);
    if armed {
        unsafe {
            let _ = KillTimer(Some(owner), super::WM_DOWNLOADS_TIMER);
        }
    }
}

/// The page reported where the button is (every click carries it).
pub(crate) fn note_button_rect(owner: HWND, rect: HoverRect) {
    let _ = with(owner, |rt| rt.rect = Some(rect));
}

/// `WM_DOWNLOADS_CHANGED`: `kind` is 0 for byte progress (throttled) and 1 for anything else.
pub(crate) fn changed(owner: HWND, app: &AppRef, kind: usize) {
    if kind == 0 {
        let wait = with(owner, |rt| throttle_wait(rt.last_flush, Instant::now())).flatten();
        if let Some(wait) = wait {
            // The newest progress goes out when the interval is over.
            let arm = with(owner, |rt| !std::mem::replace(&mut rt.timer_armed, true)).unwrap_or(false);
            if arm {
                unsafe {
                    let _ = SetTimer(Some(owner), super::WM_DOWNLOADS_TIMER, wait.as_millis().max(1) as u32, None);
                }
            }
            return;
        }
    }
    flush(owner, app);
}

/// `WM_DOWNLOADS_TIMER`: the trailing flush of throttled progress.
pub(crate) fn timer(owner: HWND, app: &AppRef) {
    unsafe {
        let _ = KillTimer(Some(owner), super::WM_DOWNLOADS_TIMER);
    }
    let _ = with(owner, |rt| rt.timer_armed = false);
    flush(owner, app);
}

/// The button's state as the page should show it: while the bubble is open the button stays, even
/// when everything is seen and nothing is running (it closes when the bubble does).
fn button(owner: HWND, app: &AppRef) -> crate::downloads::Toolbar {
    let mut toolbar = App::downloads_toolbar(app);
    if with(owner, |rt| rt.bubble.is_some()).unwrap_or(false) {
        toolbar.on = true;
    }
    toolbar
}

fn push_button(owner: HWND, app: &AppRef) {
    let toolbar = button(owner, app);
    super::window::html_set_download_button(owner, toolbar.on, &toolbar.json());
}

/// Brings everything that shows downloads in line with the list.
fn flush(owner: HWND, app: &AppRef) {
    let _ = with(owner, |rt| rt.last_flush = Some(Instant::now()));
    super::downloads_page::downloads_changed(owner);
    let toolbar = App::downloads_toolbar(app);
    let started = with(owner, |rt| std::mem::replace(&mut rt.burst, toolbar.burst) != toolbar.burst).unwrap_or(false);
    push_button(owner, app);
    let bubble = with(owner, |rt| rt.bubble.as_ref().map(|bubble| (bubble.tok, bubble.auto))).flatten();
    match bubble {
        Some((tok, auto)) => repost(owner, app, tok, auto),
        None if started && may_auto_open(owner) => {
            open(owner, app, true);
        }
        None => {}
    }
}

/// A download started: the bubble opens by itself only while this window is the one in front, its
/// toolbar is up (the bubble hangs from the button), nothing else is popped up, and the downloads
/// page is not already showing the list.
fn may_auto_open(owner: HWND) -> bool {
    super::window::html_toolbar_active(owner)
        && !super::downloads_page::shown(owner)
        && unsafe { GetForegroundWindow() } == owner
        && !unsafe { IsIconic(owner) }.as_bool()
        && !html_popup::is_active(owner)
}

/// Posts the current rows to the open bubble under its token, when they changed.
fn repost(owner: HWND, app: &AppRef, tok: u32, auto: bool) {
    let json = App::downloads_bubble_json(app, tok, auto);
    let unchanged = with(owner, |rt| rt.bubble.as_ref().is_some_and(|bubble| bubble.sent == json)).unwrap_or(true);
    if unchanged {
        return;
    }
    if html_popup::repost(owner, tok, &json) {
        let _ = with(owner, |rt| {
            if let Some(bubble) = rt.bubble.as_mut().filter(|bubble| bubble.tok == tok) {
                bubble.sent = json;
            }
        });
    } else {
        // The popup is gone or refused the message: the next open starts clean.
        let _ = with(owner, |rt| rt.bubble = None);
    }
}

/// The bubble is open (it was posted by this module and its popup is showing).
fn is_open(owner: HWND) -> bool {
    with(owner, |rt| rt.bubble.is_some()).unwrap_or(false) && html_popup::is_active(owner)
}

/// Opens the bubble under the button. `auto`: a download started (shown without taking the
/// keyboard, and the page closes it after a few seconds unless the user is in it). False when it
/// could not be shown (no toolbar, popup host not ready, another popup the user is using).
fn open(owner: HWND, app: &AppRef, auto: bool) -> bool {
    let rect = with(owner, |rt| rt.rect).flatten();
    let Some(anchor) = super::window::downloads_anchor(owner, rect) else { return false };
    if !html_popup::page_ready(owner) {
        return false;
    }
    if html_popup::is_active(owner) {
        if auto {
            return false;
        }
        html_popup::cancel(owner, app);
    }
    let tok = Rc::new(Cell::new(0u32));
    let sent = Rc::new(RefCell::new(String::new()));
    let (build_tok, build_sent, build_app) = (tok.clone(), sent.clone(), app.clone());
    let shown = html_popup::show_downloads(
        owner,
        Anchor::Point(anchor, Placement::BelowRight),
        super::window::popup_context(owner),
        auto,
        move |token| {
            build_tok.set(token);
            let json = App::downloads_bubble_json(&build_app, token, auto);
            build_sent.replace(json.clone());
            json
        },
        Box::new(move |app, _| closed(owner, app)),
    );
    match shown {
        Ok(()) => {
            let _ = with(owner, |rt| rt.bubble = Some(Bubble { tok: tok.get(), auto, sent: sent.take() }));
            // Opening the bubble is looking at the downloads: finished ones stop asking for attention.
            let _ = App::downloads_mark_seen(app);
            true
        }
        Err(ShowRefused::Guarded(_)) | Err(ShowRefused::Unavailable(_)) => false,
    }
}

/// Every way the bubble closes ends here: finished downloads count as seen, and with nothing running
/// the button goes away.
fn closed(owner: HWND, app: &AppRef) {
    let _ = with(owner, |rt| rt.bubble = None);
    let _ = App::downloads_mark_seen(app);
    flush(owner, app);
}

/// The button was clicked: closes the bubble if it is up, opens it otherwise. A click that
/// deactivated the bubble (and so closed it) a moment ago reaches `open` as a guarded refusal, so
/// the click that closes does not reopen.
pub(crate) fn toggle(owner: HWND, app: &AppRef) {
    if is_open(owner) {
        html_popup::close(owner);
        return;
    }
    let _ = with(owner, |rt| rt.bubble = None);
    open(owner, app, false);
}

/// Whether `tok` names the open downloads bubble (checked before a `dlact` is queued and again when
/// it runs).
pub(crate) fn valid_act(owner: HWND, tok: u32) -> bool {
    with(owner, |rt| rt.bubble.as_ref().is_some_and(|bubble| bubble.tok == tok)).unwrap_or(false)
        && html_popup::valid_token(owner, tok)
}

/// Runs a `dlact` from the deferred queue.
pub(crate) fn run_act(owner: HWND, app: &AppRef, tok: u32, id: u32, n: u8) {
    if !valid_act(owner, tok) {
        return;
    }
    let Some(action) = Action::from_code(n) else { return };
    let followup = App::download_act(app, id, action);
    if leaves_bubble(action, &followup) {
        // The bubble closes first and hands the keyboard back, so the page or tab the action opens
        // is the one that keeps it.
        html_popup::cancel(owner, app);
    }
    let _ = follow_up(owner, app, followup);
}

/// What `App::download_act` leaves the window to do: open the downloads page, a new tab for Retry,
/// the clipboard for Copy link. False when it could not be done.
pub(super) fn follow_up(owner: HWND, app: &AppRef, followup: DownloadFollowup) -> bool {
    match followup {
        DownloadFollowup::None | DownloadFollowup::Rejected => true,
        DownloadFollowup::Failed => false,
        DownloadFollowup::OpenPage => {
            super::downloads_page::open(owner, app, false);
            true
        }
        DownloadFollowup::OpenTab(url) => match url::Url::parse(&url) {
            Ok(url) if matches!(url.scheme(), "http" | "https") => {
                App::new_tab(app);
                App::navigate_current(app, &url);
                true
            }
            _ => false,
        },
        DownloadFollowup::CopyText(text) => super::window::copy_text_to_clipboard(owner, &text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_goes_out_at_most_four_times_a_second() {
        let start = Instant::now();
        // Nothing sent yet, or the interval is over: now.
        assert_eq!(throttle_wait(None, start), None);
        assert_eq!(throttle_wait(Some(start), start + PROGRESS_INTERVAL), None);
        assert_eq!(throttle_wait(Some(start), start + PROGRESS_INTERVAL * 2), None);
        // Inside it: wait for the rest of the interval, never longer.
        assert_eq!(throttle_wait(Some(start), start), Some(PROGRESS_INTERVAL));
        assert_eq!(throttle_wait(Some(start), start + Duration::from_millis(100)), Some(Duration::from_millis(150)));
        assert_eq!(PROGRESS_INTERVAL * 4, Duration::from_secs(1));
    }

    /// The body of the top-level `fn <name>(` in `source`, up to its closing brace in column 0.
    fn fn_body<'a>(source: &'a str, name: &str) -> &'a str {
        let start = source.find(&format!("\nfn {name}(")).unwrap_or_else(|| panic!("fn {name} in window.rs"));
        let rest = &source[start + 1..];
        &rest[..rest.find("\n}").expect("end of fn")]
    }

    #[test]
    fn the_list_observer_is_installed_when_the_toolbar_page_is_ready() {
        // `install` is what makes the list tell this window about changes; without it no
        // WM_DOWNLOADS_CHANGED is ever posted, so the button never shows, the bubble never opens and
        // the downloads page never refreshes. It must run on every toolbar `ready` (html_refresh),
        // not only when the HTML chrome falls back to GDI. Not unit-testable without WebView2.
        let window = include_str!("window.rs");
        assert!(
            fn_body(window, "html_refresh").contains("super::downloads_ui::install(hwnd, &app);"),
            "html_refresh must install the downloads observer and send the button's state"
        );
    }

    #[test]
    fn only_actions_that_leave_for_another_surface_close_the_bubble() {
        let done = DownloadFollowup::None;
        for action in [Action::Open, Action::Show, Action::OpenPage, Action::OpenFolder] {
            assert!(leaves_bubble(action, &done), "{action:?}");
        }
        assert!(leaves_bubble(Action::Retry, &DownloadFollowup::OpenTab("https://example.test/f.zip".into())));
        for action in [Action::Pause, Action::Resume, Action::Cancel, Action::Remove, Action::CopyLink, Action::ClearAll] {
            assert!(!leaves_bubble(action, &done), "{action:?}");
        }
        assert!(!leaves_bubble(Action::CopyLink, &DownloadFollowup::CopyText("https://example.test/".into())));
        // A refused or failed action keeps the bubble, so the row can show the reason.
        for followup in [DownloadFollowup::Rejected, DownloadFollowup::Failed] {
            for action in [Action::Open, Action::Show, Action::Retry, Action::OpenFolder, Action::OpenPage] {
                assert!(!leaves_bubble(action, &followup), "{action:?} {followup:?}");
            }
        }
    }
}
