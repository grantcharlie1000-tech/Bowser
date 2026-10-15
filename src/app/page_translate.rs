//! Page translation, the app's side (see `crate::translate` for the pure logic and the design).
//!
//! Nothing here runs until the user asks, except one pure decision per toolbar refresh (is this page
//! offered?) and, for a site set to "always translate" with the automatic setting on and its language
//! files already on disk, a start after the reader probe. A translation is one job at a time, driven
//! entirely by completions, never by waiting:
//!
//! 1. the language files are made ready on a background thread (`store::ensure`), only if some are
//!    missing or not yet checked in this run; `WM_TRANSLATE_PACK` reports back;
//! 2. the hidden worker page is started (or reused) and asked to load the route's models;
//! 3. `collect` runs in the page and returns the first batch of text; the worker translates it;
//!    `apply` writes it back and returns the next batch; repeat until a batch comes back empty.
//!
//! Every step is checked against the tab's translate epoch, so a navigation, a closed tab or a
//! reloaded page drops the job silently. A watchdog timer fails a step that never answers, and an
//! idle timer closes the worker page after [`translate::WORKER_IDLE_SECS`].

use std::{collections::HashSet, path::PathBuf, rc::Rc, sync::Mutex};

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::SetTimer,
};

use super::{App, AppRef};
use crate::{
    site_prefs::SiteTranslate,
    tabs::TabId,
    translate::{self, Action, Offer, OfferInput, Pill, ReplyCode, Route, WorkerMessage, store},
    win32,
};

/// The longest one step (loading the models, translating a batch, applying it) may go unanswered.
const WATCH_MS: u32 = 90_000;
/// Only one language-file download runs at a time, across windows.
static DOWNLOAD_LOCK: Mutex<()> = Mutex::new(());

/// What the user should be told. The window turns these into toasts; automatic starts ignore them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// Language files are being fetched (`bytes` in all), from the user's own request.
    Downloading { from: String, to: String, bytes: u64 },
    /// A private tab needs files that are not on disk: ask before fetching anything.
    NeedsConfirm { from: String, to: String, bytes: u64 },
    /// The page is translated.
    Done,
    /// The page is back to its original text.
    Restored,
    /// Not a web page, no live page, or this tab is already busy.
    Unavailable,
    /// A translation is running in another tab.
    Busy,
    /// The page does not say what language it is in.
    NoLanguage,
    /// The page is already in the target language.
    SameLanguage,
    /// No pinned language files cover this pair.
    NoRoute { from: String, to: String },
    /// The page has no text worth translating.
    NothingToTranslate,
    /// The language files could not be downloaded or did not check out.
    DownloadFailed,
    /// The worker or the page failed; the page was put back where possible.
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Waiting for the language files.
    Pack,
    /// Waiting for the worker page to say it is ready.
    Start,
    /// Waiting for the worker to load the models.
    Load,
    /// Waiting for the page's first batch.
    Collect,
    /// Waiting for the worker to translate a batch.
    Translate,
    /// Waiting for the page to take a translated batch and give the next.
    Apply,
}

pub(in crate::app) struct Job {
    id: u32,
    tab: TabId,
    epoch: u32,
    route: Route,
    root: PathBuf,
    token: String,
    phase: Phase,
    /// The id of the worker request this job is waiting on.
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

/// Everything translation keeps in the app besides each tab's state. Plain data until a translate
/// command runs.
#[derive(Default)]
pub(super) struct Runtime {
    pub(in crate::app) job: Option<Job>,
    /// Tests point the cache at a temporary folder instead of the profile's.
    #[cfg(test)]
    pub(in crate::app) root_override: Option<PathBuf>,
    next_id: u32,
    /// The worker page has said it is ready (cleared when it closes).
    pub(in crate::app) worker_ready: bool,
    /// Manifest paths whose hash was checked in this run, so they are not hashed again.
    pub(in crate::app) verified: HashSet<&'static str>,
    pub(in crate::app) watch: Option<(u32, u32)>,
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

    /// The offer for `tab`'s page. Pure apart from one `stat` per file, and only for a site set to
    /// "always translate" with the automatic setting on. The display language is asked of Windows the
    /// first time a page that names its language gets here, never before.
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
        let mut input = OfferInput { page_lang, target: &target, offer_on, auto_on, site, private, pack_ready: false };
        let offer = translate::should_offer(&input);
        if offer == Offer::Offer && auto_on && !private && site == Some(SiteTranslate::Always) {
            input.pack_ready = Self::translate_pack_ready(&target, page_lang);
            return translate::should_offer(&input);
        }
        offer
    }

    fn translate_pack_ready(target: &str, page_lang: translate::Lang) -> bool {
        let (Some(route), Some(root)) = (translate::pair_route(page_lang.as_str(), target), store::cache_root()) else {
            return false;
        };
        store::missing(&root, &route.files()).is_empty()
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
        let saved = store.borrow_mut().set_translate(&origin, choice).is_ok();
        saved
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
            return notify(Notice::Unavailable);
        }
        if app.borrow().translate.job.is_some() {
            return notify(Notice::Busy);
        }
        if state.page_lang.is_none() {
            return notify(Notice::NoLanguage);
        }
        let target = translate::target_language(&setting);
        let source = state.page_lang.as_str().to_owned();
        if source == target {
            return notify(Notice::SameLanguage);
        }
        let (from, to) = (translate::language_name(&source).to_owned(), translate::language_name(&target).to_owned());
        let Some(route) = translate::pair_route(&source, &target) else {
            return notify(Notice::NoRoute { from, to });
        };
        #[cfg(test)]
        let root = app.borrow().translate.root_override.clone().or_else(store::cache_root);
        #[cfg(not(test))]
        let root = store::cache_root();
        let Some(root) = root else {
            return notify(Notice::Failed);
        };
        let files = route.files();
        let missing = store::missing(&root, &files);
        let bytes = store::total_bytes(&missing);
        // A private tab asks before anything is fetched. (Only file names on Bowser's pinned list go
        // over the network, never page text or addresses, but the user decides.)
        if private && !missing.is_empty() && !confirmed {
            return notify(Notice::NeedsConfirm { from, to, bytes });
        }
        let (job_id, unverified) = {
            let mut guard = app.borrow_mut();
            let runtime = &mut guard.translate;
            let id = runtime.id();
            let unverified: Vec<&'static translate::File> = files.iter().copied().filter(|file| !runtime.verified.contains(file.path)).collect();
            runtime.job = Some(Job {
                id,
                tab,
                epoch: state.epoch,
                route,
                root: root.clone(),
                token: String::new(),
                phase: Phase::Pack,
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
            (id, unverified)
        };
        if unverified.is_empty() {
            return Self::translate_pack_ready_next(app, job_id);
        }
        if !missing.is_empty() {
            notify(Notice::Downloading { from, to, bytes });
        }
        let window = app.borrow().hwnd.0 as isize;
        let spawned = std::thread::Builder::new().name("bowser-translate-pack".into()).spawn(move || {
            let _one_at_a_time = DOWNLOAD_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let code = match store::ensure(&root, &unverified) {
                Ok(()) => PACK_OK,
                Err(store::StoreError::Network) => PACK_NETWORK,
                Err(store::StoreError::Hash) => PACK_HASH,
                Err(store::StoreError::Io) => PACK_IO,
            };
            win32::post(HWND(window as *mut _), win32::WM_TRANSLATE_PACK, WPARAM(job_id as usize), LPARAM(code));
        });
        if spawned.is_err() {
            Self::translate_fail(app, Notice::Failed);
        }
    }

    /// The pack thread finished (`WM_TRANSLATE_PACK`). `code` is one of the `PACK_*` values.
    pub fn translate_pack_done(app: &AppRef, job_id: u32, code: isize) {
        let (files, matches) = {
            let state = app.borrow();
            match state.translate.job.as_ref() {
                Some(job) if job.id == job_id && job.phase == Phase::Pack => (job.route.files(), true),
                _ => (Vec::new(), false),
            }
        };
        if !matches {
            return;
        }
        if code != PACK_OK {
            return Self::translate_fail(app, Notice::DownloadFailed);
        }
        app.borrow_mut().translate.verified.extend(files.iter().map(|file| file.path));
        Self::translate_pack_ready_next(app, job_id);
    }

    /// Files are ready: start the worker page, or reuse it, and ask it to load the models.
    fn translate_pack_ready_next(app: &AppRef, job_id: u32) {
        let (tab, epoch, root) = match app.borrow().translate.job.as_ref() {
            Some(job) if job.id == job_id => (job.tab, job.epoch, job.root.clone()),
            _ => return,
        };
        if !Self::translate_current(app, tab, epoch) {
            return Self::translate_drop(app);
        }
        if store::write_worker_files(&root).is_err() {
            return Self::translate_fail(app, Notice::Failed);
        }
        let started = Self::call_engine(app, |engine| engine.translate_worker_open(&root)).unwrap_or(false);
        if !started {
            return Self::translate_fail(app, Notice::Failed);
        }
        let ready = app.borrow().translate.worker_ready;
        if ready {
            Self::translate_load(app);
        } else {
            Self::translate_step(app, Phase::Start);
        }
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

    fn translate_load(app: &AppRef) {
        let message = {
            let mut guard = app.borrow_mut();
            let runtime = &mut guard.translate;
            let id = runtime.id();
            let Some(job) = runtime.job.as_mut() else {
                return;
            };
            job.request = id;
            translate::load_message(id, &job.route)
        };
        Self::translate_step(app, Phase::Load);
        if !Self::call_engine(app, |engine| engine.translate_worker_post(&message)).unwrap_or(false) {
            Self::translate_fail(app, Notice::Failed);
        }
    }

    /// A message from the worker page (see `EngineEvents::translate_worker_message`).
    pub fn translate_worker_message(app: &AppRef, text: &str) {
        let Some(message) = translate::parse_worker_message(text) else {
            return;
        };
        Self::translate_touch(app);
        match message {
            WorkerMessage::Ready => {
                app.borrow_mut().translate.worker_ready = true;
                let waiting = app.borrow().translate.job.as_ref().is_some_and(|job| job.phase == Phase::Start);
                if waiting {
                    Self::translate_load(app);
                }
            }
            WorkerMessage::Loaded { id } => {
                if Self::translate_expecting(app, id, Phase::Load) {
                    Self::translate_collect(app);
                }
            }
            WorkerMessage::Out { id, strings } => {
                if Self::translate_expecting(app, id, Phase::Translate) {
                    Self::translate_apply(app, strings);
                }
            }
            WorkerMessage::Error { id, .. } => {
                let ours = app.borrow().translate.job.as_ref().is_some_and(|job| job.request == id && matches!(job.phase, Phase::Load | Phase::Translate));
                if ours {
                    Self::translate_fail(app, Notice::Failed);
                }
            }
        }
    }

    fn translate_expecting(app: &AppRef, id: u32, phase: Phase) -> bool {
        app.borrow().translate.job.as_ref().is_some_and(|job| job.request == id && job.phase == phase)
    }

    /// The worker page's process failed or it could not start; it is already closed.
    pub fn translate_worker_failed(app: &AppRef) {
        app.borrow_mut().translate.worker_ready = false;
        if app.borrow().translate.job.is_some() {
            Self::translate_fail(app, Notice::Failed);
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

    /// Hands one batch to the worker.
    fn translate_send(app: &AppRef, strings: Vec<String>) {
        let message = {
            let mut guard = app.borrow_mut();
            let runtime = &mut guard.translate;
            let id = runtime.id();
            let Some(job) = runtime.job.as_mut() else {
                return;
            };
            job.request = id;
            job.sent = strings.len();
            job.batches += 1;
            translate::translate_message(id, &job.route, &strings)
        };
        Self::translate_step(app, Phase::Translate);
        if !Self::call_engine(app, |engine| engine.translate_worker_post(&message)).unwrap_or(false) {
            Self::translate_fail(app, Notice::Failed);
        }
    }

    /// The worker answered a batch: write it into the page and take the next one.
    fn translate_apply(app: &AppRef, strings: Vec<String>) {
        let (tab, epoch, token, index, sent) = match app.borrow().translate.job.as_ref() {
            Some(job) => (job.tab, job.epoch, job.token.clone(), job.next, job.sent),
            None => return,
        };
        if !Self::translate_current(app, tab, epoch) {
            return Self::translate_drop(app);
        }
        let Some(strings) = translate::check_answer(sent, strings) else {
            return Self::translate_fail(app, Notice::Failed);
        };
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
        Self::translate_touch(app);
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
            Self::translate_touch(app);
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
        Self::translate_touch(app);
        (job.notify)(notice);
    }

    /// Shows the original text again.
    pub fn translate_restore(app: &AppRef, tab: TabId, notify: Rc<dyn Fn(Notice)>) {
        let Some((state, _, _)) = Self::translate_state(app, tab) else {
            return notify(Notice::Unavailable);
        };
        if state.busy {
            return notify(Notice::Unavailable);
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
                Self::translate_touch(&app);
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
    /// translate", when the automatic setting is on and the files are already on disk.
    pub(super) fn translate_after_probe(app: &AppRef, tab: TabId) {
        if Self::translate_offer(app, tab) == Offer::Auto {
            Self::translate_start(app, tab, false, Rc::new(|_| {}));
        }
    }

    // --- the worker page's lifetime ---

    /// Notes that translation is in use and (re)arms the idle timer that closes the worker page.
    fn translate_touch(app: &AppRef) {
        let hwnd = app.borrow().hwnd;
        if hwnd.is_invalid() {
            return;
        }
        let running = Self::call_engine(app, |engine| engine.translate_worker_running()).unwrap_or(false);
        if !running {
            return;
        }
        unsafe {
            let _ = SetTimer(Some(hwnd), win32::WM_TRANSLATE_IDLE_TIMER, (translate::WORKER_IDLE_SECS * 1000) as u32, None);
        }
    }

    /// The idle timer fired: close the worker page unless a job is using it.
    pub fn translate_idle_timer(app: &AppRef) {
        if app.borrow().translate.job.is_some() {
            return Self::translate_touch(app);
        }
        app.borrow_mut().translate.worker_ready = false;
        let _ = Self::call_engine(app, |engine| engine.translate_worker_close());
    }

    /// The watchdog fired: a step that has not moved since it was armed fails the job.
    pub fn translate_watch_timer(app: &AppRef) {
        let stalled = {
            let state = app.borrow();
            match (state.translate.watch, state.translate.job.as_ref()) {
                (Some((id, step)), Some(job)) => job.id == id && job.step == step && job.phase != Phase::Pack,
                _ => false,
            }
        };
        if stalled {
            // A worker that stopped answering is not trusted again: close it so the next try starts clean.
            app.borrow_mut().translate.worker_ready = false;
            let _ = Self::call_engine(app, |engine| engine.translate_worker_close());
            Self::translate_fail(app, Notice::Failed);
        }
    }
}

/// `WM_TRANSLATE_PACK` result codes (the message's `LPARAM`).
pub const PACK_OK: isize = 0;
pub const PACK_NETWORK: isize = 1;
pub const PACK_HASH: isize = 2;
pub const PACK_IO: isize = 3;
