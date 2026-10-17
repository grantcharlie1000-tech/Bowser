//! Page translation, the app's side (see `crate::translate` for the pure logic and the design).
//!
//! Nothing here runs until the user asks, except one pure decision per toolbar refresh and, for a site
//! set to "always translate" with the automatic setting on, a start after the reader probe. A
//! translation is one job at a time, driven entirely by completions, never by waiting:
//!
//! 1. `collect` runs in the page and returns a batch of text;
//! 2. the batch goes to Google Translate on a background thread (`translate::google`);
//!    `WM_TRANSLATE_NET` reports back;
//! 3. `apply` writes the translation into the page and returns the next batch; repeat until a batch
//!    comes back empty.
//!
//! **The page's text is sent to Google in step 2.** A private tab asks first.
//!
//! Every step is checked against the tab's translate epoch, so a navigation, a closed tab or a
//! reloaded page drops the job silently. A watchdog timer fails a step that never answers.

use std::{
    rc::Rc,
    sync::{
        Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::SetTimer,
};

use super::{App, AppRef};
use crate::{
    site_prefs::SiteTranslate,
    tabs::TabId,
    translate::{self, Action, Offer, OfferInput, Pill, ReplyCode, google::GoogleError},
    win32,
};

/// The longest one step (a page script, or a whole request to Google with its retry) may go unanswered.
const WATCH_MS: u32 = 120_000;

/// What the user should be told. The window turns these into toasts; automatic starts ignore them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// Translation has started: the page's text is going to Google Translate.
    Started,
    /// A private tab asks before any of its text is sent to Google.
    NeedsConfirm,
    /// The page is translated.
    Done,
    /// The page is back to its original text.
    Restored,
    /// Not a web page, or no live page.
    Unavailable,
    /// This tab is already being translated (or restored).
    InProgress,
    /// A translation is running in another tab.
    Busy,
    /// The page is already in the target language.
    SameLanguage,
    /// The page has no text worth translating.
    NothingToTranslate,
    /// Google Translate could not be reached.
    NetworkFailed,
    /// Google is refusing requests from this connection for now.
    Limited,
    /// The page or Google's answer was not usable; the page was put back where possible.
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Waiting for the page's first batch.
    Collect,
    /// Waiting for Google to translate a batch.
    Translate,
    /// Waiting for the page to take a translated batch and give the next.
    Apply,
}

pub(in crate::app) struct Job {
    id: u32,
    tab: TabId,
    epoch: u32,
    /// The page's language when it is one in the table, else `auto`.
    source: &'static str,
    target: String,
    token: String,
    phase: Phase,
    /// The id of the request to Google this job is waiting on.
    request: u32,
    /// How many strings the batch in flight holds.
    sent: usize,
    /// Index of the first string of the batch in flight.
    next: usize,
    batches: usize,
    /// Some of the page has been rewritten, so a failure must put it back.
    applied: bool,
    /// Bumped at every step so the watchdog can tell a stalled step from a moving job.
    step: u32,
    notify: Rc<dyn Fn(Notice)>,
}

type BatchResult = Result<Vec<String>, GoogleError>;

/// Answers from the request threads, waiting for the window thread to take them. Keyed by request
/// id, which is unique across the process (windows share this list).
static RESULTS: Mutex<Vec<(u32, BatchResult)>> = Mutex::new(Vec::new());
static NEXT_REQUEST: AtomicU32 = AtomicU32::new(1);

fn take_result(request: u32) -> Option<BatchResult> {
    let mut results = RESULTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let position = results.iter().position(|(id, _)| *id == request)?;
    Some(results.swap_remove(position).1)
}

/// Everything translation keeps in the app besides each tab's state. Plain data until a translate
/// command runs.
#[derive(Default)]
pub(super) struct Runtime {
    pub(in crate::app) job: Option<Job>,
    next_id: u32,
    pub(in crate::app) watch: Option<(u32, u32)>,
    /// Tests answer for Google instead of making a request.
    #[cfg(test)]
    pub(in crate::app) fetch: Option<Rc<dyn Fn(&str, &str, &[String]) -> BatchResult>>,
}

impl Runtime {
    fn id(&mut self) -> u32 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.next_id
    }
}

impl App {
    // --- what the toolbar and menus read ---

    /// Whether translation can run on `tab` at all (an http(s) page).
    pub fn translate_available(app: &AppRef, tab: TabId) -> bool {
        let state = app.borrow();
        state.tabs.iter().find(|entry| entry.id == tab).and_then(|entry| entry.url.as_ref()).is_some_and(translate::url_allowed)
    }

    pub fn translate_active(app: &AppRef, tab: TabId) -> bool {
        app.borrow().tabs.iter().find(|entry| entry.id == tab).is_some_and(|entry| entry.translate.active)
    }

    /// The stored per-site choice for `tab`'s page (`None` for a private tab, which never stores or
    /// reads one).
    pub fn translate_site_choice(app: &AppRef, tab: TabId) -> Option<SiteTranslate> {
        let state = app.borrow();
        let entry = state.tabs.iter().find(|entry| entry.id == tab)?;
        if entry.private {
            return None;
        }
        let origin = crate::permissions::origin_of(entry.url.as_ref()?.as_str())?;
        state.site_prefs.as_ref()?.borrow().translate(&origin)
    }

    /// The offer for `tab`'s page. Pure. The display language is asked of Windows the first time a page
    /// that names its language gets here, never before.
    pub fn translate_offer(app: &AppRef, tab: TabId) -> Offer {
        let (page_lang, private, offer_on, auto_on, setting) = {
            let state = app.borrow();
            let Some(entry) = state.tabs.iter().find(|entry| entry.id == tab) else {
                return Offer::No;
            };
            if !entry.url.as_ref().is_some_and(translate::url_allowed) || entry.translate.page_lang.is_none() {
                return Offer::No;
            }
            (entry.translate.page_lang, entry.private, state.settings.translate_offer, state.settings.translate_auto, state.settings.translate_target.clone())
        };
        let target = translate::target_language(&setting);
        let site = Self::translate_site_choice(app, tab);
        translate::should_offer(&OfferInput { page_lang, target: &target, offer_on, auto_on, site, private })
    }

    /// The toolbar button for `tab`.
    pub fn translate_pill(app: &AppRef, tab: TabId) -> Pill {
        if Self::translate_active(app, tab) {
            return Pill::On;
        }
        let offered = Self::translate_offer(app, tab) != Offer::No;
        app.borrow().tabs.iter().find(|entry| entry.id == tab).map_or(Pill::Hidden, |entry| entry.translate.pill(offered))
    }

    pub fn active_translate_pill(app: &AppRef) -> Pill {
        let active = app.borrow().tab_model.active();
        Self::translate_pill(app, active)
    }

    /// The menu's wording for the command on the active tab.
    pub fn translate_command_label(app: &AppRef, tab: TabId) -> String {
        let active = Self::translate_active(app, tab);
        let setting = app.borrow().settings.translate_target.clone();
        // The display language is only looked up for a menu that is being opened by a person.
        translate::command_label(&translate::target_language(&setting), active)
    }

    /// Stores "always" or "never" (or with `None` clears) for `tab`'s site. Refused (`false`) for a
    /// private tab, a page that is not a website, and an app with no site store.
    pub fn translate_set_site(app: &AppRef, tab: TabId, choice: Option<SiteTranslate>) -> bool {
        let (store, origin) = {
            let state = app.borrow();
            let Some(entry) = state.tabs.iter().find(|entry| entry.id == tab) else {
                return false;
            };
            if entry.private {
                return false;
            }
            let origin = entry.url.as_ref().filter(|url| translate::url_allowed(url)).and_then(|url| crate::permissions::origin_of(url.as_str()));
            (state.site_prefs.clone(), origin)
        };
        let (Some(store), Some(origin)) = (store, origin) else {
            return false;
        };
        store.borrow_mut().set_translate(&origin, choice).is_ok()
    }

    // --- running ---

    /// Starts translating `tab`, or shows the original again if it is already translated. `confirmed`
    /// is the user's answer to [`Notice::NeedsConfirm`]. Every outcome, including the final one, is
    /// reported to `notify` with the app unborrowed.
    pub fn translate_toggle(app: &AppRef, tab: TabId, confirmed: bool, notify: Rc<dyn Fn(Notice)>) {
        if Self::translate_active(app, tab) {
            Self::translate_restore(app, tab, notify);
        } else {
            Self::translate_start(app, tab, confirmed, notify);
        }
    }

    fn translate_state(app: &AppRef, tab: TabId) -> Option<(translate::TabState, bool, String)> {
        let state = app.borrow();
        let entry = state.tabs.iter().find(|entry| entry.id == tab)?;
        entry.url.as_ref().is_some_and(translate::url_allowed).then(|| (entry.translate, entry.private, state.settings.translate_target.clone()))
    }

    /// Whether work started for `tab` at `epoch` still belongs to its page.
    fn translate_current(app: &AppRef, tab: TabId, epoch: u32) -> bool {
        app.borrow().tabs.iter().find(|entry| entry.id == tab).is_some_and(|entry| entry.translate.is_current(epoch))
    }

    fn translate_set_busy(app: &AppRef, tab: TabId, busy: bool) {
        if let Some(entry) = app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == tab) {
            entry.translate.busy = busy;
        }
    }

    fn translate_changed(app: &AppRef, tab: TabId) {
        let hwnd = app.borrow().hwnd;
        Self::post_tab_changed_kind(hwnd, tab, false, false);
    }

    pub fn translate_start(app: &AppRef, tab: TabId, confirmed: bool, notify: Rc<dyn Fn(Notice)>) {
        let Some((state, private, setting)) = Self::translate_state(app, tab) else {
            return notify(Notice::Unavailable);
        };
        if state.busy {
            return notify(Notice::InProgress);
        }
        if app.borrow().translate.job.is_some() {
            return notify(Notice::Busy);
        }
        let target = translate::target_language(&setting);
        if !state.page_lang.is_none() && state.page_lang.as_str() == target {
            return notify(Notice::SameLanguage);
        }
        // A private tab asks before any of its text goes to Google.
        if private && !confirmed {
            return notify(Notice::NeedsConfirm);
        }
        {
            let mut guard = app.borrow_mut();
            let runtime = &mut guard.translate;
            let id = runtime.id();
            runtime.job = Some(Job {
                id,
                tab,
                epoch: state.epoch,
                source: translate::source_for(state.page_lang),
                target,
                token: String::new(),
                phase: Phase::Collect,
                request: 0,
                sent: 0,
                next: 0,
                batches: 0,
                applied: false,
                step: 0,
                notify: notify.clone(),
            });
            if let Some(entry) = guard.tabs.iter_mut().find(|entry| entry.id == tab) {
                entry.translate.busy = true;
            }
        }
        notify(Notice::Started);
        Self::translate_collect(app);
    }

    /// Records the phase the job is now waiting in and (re)arms the watchdog.
    fn translate_step(app: &AppRef, phase: Phase) {
        let hwnd = {
            let mut state = app.borrow_mut();
            let hwnd = state.hwnd;
            let runtime = &mut state.translate;
            let Some(job) = runtime.job.as_mut() else {
                return;
            };
            job.phase = phase;
            job.step = job.step.wrapping_add(1);
            runtime.watch = Some((job.id, job.step));
            hwnd
        };
        if !hwnd.is_invalid() {
            unsafe {
                let _ = SetTimer(Some(hwnd), win32::WM_TRANSLATE_WATCH_TIMER, WATCH_MS, None);
            }
        }
    }

    /// Runs one translation script in `tab`. `done` always runs with the app unborrowed, even when
    /// the engine answers at once (no live page).
    fn translate_script(app: &AppRef, tab: TabId, script: String, done: Box<dyn FnOnce(Option<String>)>) {
        let done = Rc::new(std::cell::RefCell::new(Some(done)));
        let early = Rc::new(std::cell::RefCell::new(None::<Option<String>>));
        let in_call = Rc::new(std::cell::Cell::new(true));
        let answer = {
            let (done, early, in_call) = (done.clone(), early.clone(), in_call.clone());
            Box::new(move |text: Option<String>| {
                if in_call.get() {
                    *early.borrow_mut() = Some(text);
                } else if let Some(done) = done.borrow_mut().take() {
                    done(text);
                }
            }) as Box<dyn FnOnce(Option<String>)>
        };
        let called = Self::call_engine(app, |engine| engine.run_translate_script(tab, &script, answer));
        in_call.set(false);
        let text = match (called, early.borrow_mut().take()) {
            (None, _) => Some(None),
            (Some(()), early) => early,
        };
        if let Some(text) = text
            && let Some(done) = done.borrow_mut().take()
        {
            done(text);
        }
    }

    fn translate_collect(app: &AppRef) {
        let (tab, epoch) = match app.borrow().translate.job.as_ref() {
            Some(job) => (job.tab, job.epoch),
            None => return,
        };
        if !Self::translate_current(app, tab, epoch) {
            return Self::translate_drop(app);
        }
        Self::translate_step(app, Phase::Collect);
        let weak = Rc::downgrade(app);
        Self::translate_script(
            app,
            tab,
            translate::invocation(&Action::Collect),
            Box::new(move |text| {
                let Some(app) = weak.upgrade() else { return };
                Self::translate_collected(&app, text);
            }),
        );
    }

    fn translate_collected(app: &AppRef, text: Option<String>) {
        let (tab, epoch) = match app.borrow().translate.job.as_ref() {
            Some(job) if job.phase == Phase::Collect => (job.tab, job.epoch),
            _ => return,
        };
        if !Self::translate_current(app, tab, epoch) {
            return Self::translate_drop(app);
        }
        let Some(reply) = text.as_deref().and_then(translate::parse_reply) else {
            return Self::translate_fail(app, Notice::Failed);
        };
        match reply.code {
            ReplyCode::None => Self::translate_fail(app, Notice::NothingToTranslate),
            ReplyCode::On => Self::translate_finish(app),
            ReplyCode::Ok if !reply.strings.is_empty() && reply.next == 0 => {
                if let Some(job) = app.borrow_mut().translate.job.as_mut() {
                    job.token = reply.token;
                    job.next = 0;
                }
                Self::translate_send(app, reply.strings);
            }
            _ => Self::translate_fail(app, Notice::Failed),
        }
    }

    /// Hands one batch to Google, on a background thread.
    fn translate_send(app: &AppRef, strings: Vec<String>) {
        let (job_id, request, source, target) = {
            let mut guard = app.borrow_mut();
            let Some(job) = guard.translate.job.as_mut() else {
                return;
            };
            let request = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
            job.request = request;
            job.sent = strings.len();
            job.batches += 1;
            (job.id, request, job.source, job.target.clone())
        };
        Self::translate_step(app, Phase::Translate);
        #[cfg(test)]
        {
            let fetch = app.borrow().translate.fetch.clone();
            if let Some(fetch) = fetch {
                RESULTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push((request, fetch(source, &target, &strings)));
                return Self::translate_net_done(app, job_id, request);
            }
        }
        let window = app.borrow().hwnd.0 as isize;
        let spawned = std::thread::Builder::new().name("bowser-translate-net".into()).spawn(move || {
            let result = translate::google::translate(source, &target, &strings);
            RESULTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push((request, result));
            win32::post(HWND(window as *mut _), win32::WM_TRANSLATE_NET, WPARAM(job_id as usize), LPARAM(request as isize));
        });
        if spawned.is_err() {
            Self::translate_fail(app, Notice::Failed);
        }
    }

    /// A request thread finished (`WM_TRANSLATE_NET`).
    pub fn translate_net_done(app: &AppRef, job_id: u32, request: u32) {
        // The result is always taken, so a job that moved on does not leave its answer behind.
        let result = take_result(request);
        let ours = app.borrow().translate.job.as_ref().is_some_and(|job| job.id == job_id && job.request == request && job.phase == Phase::Translate);
        if !ours {
            return;
        }
        match result {
            Some(Ok(strings)) => Self::translate_apply(app, strings),
            Some(Err(GoogleError::Network)) | None => Self::translate_fail(app, Notice::NetworkFailed),
            Some(Err(GoogleError::Limited)) => Self::translate_fail(app, Notice::Limited),
            Some(Err(GoogleError::Bad)) => Self::translate_fail(app, Notice::Failed),
        }
    }

    /// Google answered a batch: write it into the page and take the next one.
    fn translate_apply(app: &AppRef, strings: Vec<String>) {
        let (tab, epoch, token, index, sent) = match app.borrow().translate.job.as_ref() {
            Some(job) => (job.tab, job.epoch, job.token.clone(), job.next, job.sent),
            None => return,
        };
        if !Self::translate_current(app, tab, epoch) {
            return Self::translate_drop(app);
        }
        if strings.len() != sent {
            return Self::translate_fail(app, Notice::Failed);
        }
        Self::translate_step(app, Phase::Apply);
        let script = translate::invocation(&Action::Apply { token: &token, index, strings: &strings });
        let weak = Rc::downgrade(app);
        Self::translate_script(
            app,
            tab,
            script,
            Box::new(move |text| {
                let Some(app) = weak.upgrade() else { return };
                Self::translate_applied(&app, text);
            }),
        );
    }

    fn translate_applied(app: &AppRef, text: Option<String>) {
        let (tab, epoch, index, sent, batches) = match app.borrow().translate.job.as_ref() {
            Some(job) if job.phase == Phase::Apply => (job.tab, job.epoch, job.next, job.sent, job.batches),
            _ => return,
        };
        if !Self::translate_current(app, tab, epoch) {
            return Self::translate_drop(app);
        }
        let Some(reply) = text.as_deref().and_then(translate::parse_reply) else {
            return Self::translate_fail(app, Notice::Failed);
        };
        if reply.code != ReplyCode::Ok || reply.next != index + sent {
            return Self::translate_fail(app, Notice::Failed);
        }
        if let Some(job) = app.borrow_mut().translate.job.as_mut() {
            job.applied = true;
            job.next = reply.next;
        }
        if reply.strings.is_empty() || batches >= translate::MAX_BATCHES {
            return Self::translate_finish(app);
        }
        Self::translate_send(app, reply.strings);
    }

    /// The whole page is translated.
    fn translate_finish(app: &AppRef) {
        let Some(job) = app.borrow_mut().translate.job.take() else {
            return;
        };
        {
            let mut state = app.borrow_mut();
            state.translate.watch = None;
            if let Some(entry) = state.tabs.iter_mut().find(|entry| entry.id == job.tab && entry.translate.is_current(job.epoch)) {
                entry.translate.active = true;
                entry.translate.busy = false;
            }
        }
        Self::translate_changed(app, job.tab);
        (job.notify)(Notice::Done);
    }

    /// The job ends without a result for its tab (the page changed under it): nothing to put back,
    /// nothing to tell.
    fn translate_drop(app: &AppRef) {
        let job = {
            let mut state = app.borrow_mut();
            state.translate.watch = None;
            state.translate.job.take()
        };
        if let Some(job) = job {
            Self::translate_set_busy_if_current(app, job.tab, job.epoch);
        }
    }

    fn translate_set_busy_if_current(app: &AppRef, tab: TabId, epoch: u32) {
        if let Some(entry) = app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == tab && entry.translate.is_current(epoch)) {
            entry.translate.busy = false;
        }
    }

    /// The job failed or was refused: put the page back if any of it was rewritten, free the tab and
    /// tell the user.
    fn translate_fail(app: &AppRef, notice: Notice) {
        let job = {
            let mut state = app.borrow_mut();
            state.translate.watch = None;
            state.translate.job.take()
        };
        let Some(job) = job else {
            return;
        };
        let current = Self::translate_current(app, job.tab, job.epoch);
        if current {
            Self::translate_set_busy(app, job.tab, false);
            if job.applied {
                Self::translate_script(app, job.tab, translate::invocation(&Action::Restore), Box::new(|_| {}));
            }
            Self::translate_changed(app, job.tab);
        }
        (job.notify)(notice);
    }

    /// Shows the original text again.
    pub fn translate_restore(app: &AppRef, tab: TabId, notify: Rc<dyn Fn(Notice)>) {
        let Some((state, _, _)) = Self::translate_state(app, tab) else {
            return notify(Notice::Unavailable);
        };
        if state.busy {
            return notify(Notice::InProgress);
        }
        Self::translate_set_busy(app, tab, true);
        let epoch = state.epoch;
        let weak = Rc::downgrade(app);
        Self::translate_script(
            app,
            tab,
            translate::invocation(&Action::Restore),
            Box::new(move |_| {
                let Some(app) = weak.upgrade() else { return };
                if let Some(entry) = app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == tab && entry.translate.is_current(epoch)) {
                    entry.translate.active = false;
                    entry.translate.busy = false;
                }
                Self::translate_changed(&app, tab);
                notify(Notice::Restored);
            }),
        );
    }

    /// The page navigated while translated: ask the (new or same) document to put itself back. A
    /// document with nothing translated does nothing.
    pub(super) fn translate_run_restore(app: &AppRef, tab: TabId) {
        Self::translate_script(app, tab, translate::invocation(&Action::Restore), Box::new(|_| {}));
    }

    /// After the reader probe reports the page's language: start translating a site set to "always
    /// translate", when the automatic setting is on.
    pub(super) fn translate_after_probe(app: &AppRef, tab: TabId) {
        if Self::translate_offer(app, tab) == Offer::Auto {
            Self::translate_start(app, tab, false, Rc::new(|_| {}));
        }
    }

    /// The watchdog fired: a step that has not moved since it was armed fails the job.
    pub fn translate_watch_timer(app: &AppRef) {
        let stalled = {
            let state = app.borrow();
            match (state.translate.watch, state.translate.job.as_ref()) {
                (Some((id, step)), Some(job)) if job.id == id && job.step == step => Some(job.phase),
                _ => None,
            }
        };
        match stalled {
            // A request that never came back: the thread's late answer, if any, is dropped by id.
            Some(Phase::Translate) => Self::translate_fail(app, Notice::NetworkFailed),
            Some(_) => Self::translate_fail(app, Notice::Failed),
            None => {}
        }
    }
}
