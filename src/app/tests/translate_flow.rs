//! The app's side of page translation, driven end to end with a scripted engine: no window, no worker
//! page, no network and no real cache folder. The engine answers each page script from a queue and
//! records what it was asked; the test plays the worker by feeding its messages back.

use super::*;
use crate::{
    site_prefs::{SitePrefsStore, SiteTranslate},
    translate::{self, Lang},
};

#[derive(Default)]
struct Record {
    scripts: Vec<String>,
    posted: Vec<String>,
    opened: Vec<PathBuf>,
    closed: u32,
    refuse_open: bool,
}

struct FakeEngine {
    record: Rc<RefCell<Record>>,
    replies: Rc<RefCell<VecDeque<Option<String>>>>,
    running: bool,
}

impl EngineAdapter for FakeEngine {
    fn navigate(&mut self, _url: &url::Url) -> windows::core::Result<()> {
        Ok(())
    }
    fn can_go_back(&self) -> bool {
        false
    }
    fn go_back(&mut self) -> windows::core::Result<()> {
        Ok(())
    }
    fn can_go_forward(&self) -> bool {
        false
    }
    fn go_forward(&mut self) -> windows::core::Result<()> {
        Ok(())
    }
    fn resize(&mut self, _bounds: windows::Win32::Foundation::RECT) -> windows::core::Result<()> {
        Ok(())
    }
    fn set_memory_usage_target(&mut self, _target: MemoryUsageTarget) -> windows::core::Result<MemoryTargetApply> {
        Ok(MemoryTargetApply::Applied)
    }
    fn close(&mut self) -> windows::core::Result<()> {
        Ok(())
    }
    fn run_translate_script(&mut self, _tab: TabId, script: &str, done: Box<dyn FnOnce(Option<String>)>) {
        self.record.borrow_mut().scripts.push(script.to_owned());
        let reply = self.replies.borrow_mut().pop_front().flatten();
        done(reply);
    }
    fn translate_worker_open(&mut self, root: &Path) -> bool {
        if self.running {
            return true; // like the real engine: a running worker page is reused
        }
        let mut record = self.record.borrow_mut();
        record.opened.push(root.to_path_buf());
        if record.refuse_open {
            return false;
        }
        self.running = true;
        true
    }
    fn translate_worker_running(&self) -> bool {
        self.running
    }
    fn translate_worker_post(&mut self, json: &str) -> bool {
        self.record.borrow_mut().posted.push(json.to_owned());
        true
    }
    fn translate_worker_close(&mut self) {
        self.record.borrow_mut().closed += 1;
        self.running = false;
    }
}

struct Fixture {
    app: AppRef,
    tab: TabId,
    record: Rc<RefCell<Record>>,
    replies: Rc<RefCell<VecDeque<Option<String>>>>,
    notices: Rc<RefCell<Vec<TranslateNotice>>>,
    notify: Rc<dyn Fn(TranslateNotice)>,
    root: PathBuf,
}

impl Fixture {
    /// A French page in the active tab, translating into English, with every file of the route already
    /// checked (so nothing is downloaded) in a temporary cache folder.
    fn new(name: &str) -> Self {
        let app = app_with_pending(rect(800, 600));
        let record = Rc::new(RefCell::new(Record::default()));
        let replies = Rc::new(RefCell::new(VecDeque::new()));
        app.borrow_mut().engine = Some(Box::new(FakeEngine { record: record.clone(), replies: replies.clone(), running: false }));
        let tab = App::active_tab_id(&app);
        let root = std::env::temp_dir().join(format!("bowser-translate-flow-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        {
            let mut state = app.borrow_mut();
            state.settings.translate_target = "en".into();
            let entry = state.tabs.iter_mut().find(|entry| entry.id == tab).unwrap();
            entry.url = Some(url::Url::parse("https://example.test/page").unwrap());
            entry.translate.page_lang = Lang::new("fr").unwrap();
            state.translate.root_override = Some(root.clone());
            for file in translate::pair_route("fr", "en").unwrap().files() {
                state.translate.verified.insert(file.path);
            }
        }
        let notices = Rc::new(RefCell::new(Vec::new()));
        let sink = notices.clone();
        let notify: Rc<dyn Fn(TranslateNotice)> = Rc::new(move |notice| sink.borrow_mut().push(notice));
        Self { app, tab, record, replies, notices, notify, root }
    }

    fn reply(&self, text: &str) {
        self.replies.borrow_mut().push_back(Some(text.to_owned()));
    }

    fn start(&self) {
        App::translate_toggle(&self.app, self.tab, false, self.notify.clone());
    }

    fn worker(&self, text: &str) {
        App::translate_worker_message(&self.app, text);
    }

    /// The id the app put in the n-th message it posted to the worker.
    fn posted_id(&self, index: usize) -> u32 {
        let posted = self.record.borrow().posted[index].clone();
        translate::json::parse(&posted).unwrap().get("id").unwrap().as_int().unwrap() as u32
    }

    fn state(&self) -> translate::TabState {
        self.app.borrow().tabs.iter().find(|entry| entry.id == self.tab).unwrap().translate
    }

    fn job_running(&self) -> bool {
        self.app.borrow().translate.job.is_some()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn collect_reply(strings: &str, total: usize) -> String {
    format!("{{\"v\":1,\"c\":\"ok\",\"k\":\"tok1\",\"i\":0,\"n\":{total},\"s\":{strings}}}")
}

fn apply_reply(next: usize, total: usize, strings: &str) -> String {
    format!("{{\"v\":1,\"c\":\"ok\",\"k\":\"tok1\",\"i\":{next},\"n\":{total},\"s\":{strings}}}")
}

#[test]
fn a_translation_runs_load_collect_translate_apply_until_the_page_is_done() {
    let f = Fixture::new("flow");
    f.reply(&collect_reply(r#"["Bonjour","Le chat"]"#, 3));
    f.reply(&apply_reply(2, 3, r#"["Merci"]"#));
    f.reply(&apply_reply(3, 3, "[]"));
    f.start();
    // The worker page was started from the cache folder, with its page written there; nothing else yet.
    assert_eq!(f.record.borrow().opened, [f.root.clone()]);
    assert!(f.root.join("worker.html").is_file() && f.root.join("worker.js").is_file());
    assert!(f.record.borrow().posted.is_empty(), "waits for the page to say it is ready");
    assert!(f.job_running() && f.state().busy);
    assert_eq!(App::translate_pill(&f.app, f.tab), translate::Pill::Available);
    // Ready: the models are loaded.
    f.worker(r#"{"t":"ready"}"#);
    assert_eq!(f.record.borrow().posted.len(), 1);
    assert!(f.record.borrow().posted[0].starts_with(r#"{"t":"load","id":"#) && f.record.borrow().posted[0].ends_with(r#""pairs":["fr-en"]}"#));
    // A message with another id is not the answer.
    f.worker(&format!(r#"{{"t":"loaded","id":{}}}"#, f.posted_id(0) + 100));
    assert!(f.record.borrow().scripts.is_empty());
    f.worker(&format!(r#"{{"t":"loaded","id":{}}}"#, f.posted_id(0)));
    // Loaded: the page is asked for its first batch, which goes to the worker.
    assert_eq!(f.record.borrow().scripts.len(), 1);
    assert!(f.record.borrow().scripts[0].ends_with(r#"({"a":"collect"})"#));
    assert_eq!(f.record.borrow().posted.len(), 2);
    assert!(f.record.borrow().posted[1].contains(r#""route":["fr-en"],"s":["Bonjour","Le chat"]"#), "{}", f.record.borrow().posted[1]);
    f.worker(&format!(r#"{{"t":"out","id":{},"s":["Hello","The cat"]}}"#, f.posted_id(1)));
    // The translation is written back with the page's token and its first index, and the next batch follows.
    assert!(f.record.borrow().scripts[1].contains(r#"{"a":"apply","k":"tok1","i":0,"s":["Hello","The cat"]}"#));
    assert_eq!(f.record.borrow().posted.len(), 3);
    assert!(f.record.borrow().posted[2].contains(r#""s":["Merci"]"#));
    f.worker(&format!(r#"{{"t":"out","id":{},"s":["Thanks"]}}"#, f.posted_id(2)));
    assert!(f.record.borrow().scripts[2].contains(r#""i":2,"s":["Thanks"]"#));
    // The last (empty) batch ends it.
    assert!(!f.job_running());
    assert!(f.state().active && !f.state().busy);
    assert_eq!(App::translate_pill(&f.app, f.tab), translate::Pill::On);
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Done]);
    // The next command shows the original: a restore call, and the state follows.
    f.reply(r#"{"v":1,"c":"ok","k":"","i":0,"n":0,"s":[]}"#);
    f.start();
    assert!(f.record.borrow().scripts[3].ends_with(r#"({"a":"restore"})"#));
    assert!(!f.state().active && !f.state().busy);
    assert_eq!(f.notices.borrow().last(), Some(&TranslateNotice::Restored));
    // A second translation on the same page reuses the running worker page: no second start, load at once.
    f.reply(&collect_reply(r#"["Bonjour"]"#, 1));
    f.start();
    assert_eq!(f.record.borrow().opened.len(), 1, "the worker page is reused");
    assert!(f.record.borrow().posted[3].starts_with(r#"{"t":"load""#), "loads straight away");
}

#[test]
fn a_page_reply_that_is_not_the_documented_shape_fails_the_job_and_changes_nothing() {
    let f = Fixture::new("badreply");
    f.reply(r#"{"v":1,"c":"ok","k":"tok1","i":0,"n":1,"s":["a"],"extra":true}"#);
    f.start();
    f.worker(r#"{"t":"ready"}"#);
    f.worker(&format!(r#"{{"t":"loaded","id":{}}}"#, f.posted_id(0)));
    assert!(!f.job_running() && !f.state().busy && !f.state().active);
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Failed]);
    assert_eq!(f.record.borrow().posted.len(), 1, "nothing was sent to the worker");
    assert_eq!(f.record.borrow().scripts.len(), 1, "and nothing was applied or restored (nothing had changed)");
}

#[test]
fn an_answer_with_the_wrong_number_of_strings_is_not_applied() {
    let f = Fixture::new("count");
    f.reply(&collect_reply(r#"["Bonjour","Le chat"]"#, 2));
    f.start();
    f.worker(r#"{"t":"ready"}"#);
    f.worker(&format!(r#"{{"t":"loaded","id":{}}}"#, f.posted_id(0)));
    f.worker(&format!(r#"{{"t":"out","id":{},"s":["Hello"]}}"#, f.posted_id(1)));
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Failed]);
    assert_eq!(f.record.borrow().scripts.len(), 1, "only the collect call ran");
    assert!(!f.state().active && !f.state().busy);
}

#[test]
fn a_navigation_during_translation_drops_the_job_without_touching_the_new_page() {
    let f = Fixture::new("navigate");
    f.reply(&collect_reply(r#"["Bonjour"]"#, 1));
    f.start();
    f.worker(r#"{"t":"ready"}"#);
    f.worker(&format!(r#"{{"t":"loaded","id":{}}}"#, f.posted_id(0)));
    assert_eq!(f.record.borrow().posted.len(), 2);
    // The document is replaced while the worker is translating.
    {
        let mut state = f.app.borrow_mut();
        state.tabs.iter_mut().find(|entry| entry.id == f.tab).unwrap().translate.reset();
    }
    f.worker(&format!(r#"{{"t":"out","id":{},"s":["Hello"]}}"#, f.posted_id(1)));
    assert!(!f.job_running(), "the stale job is gone");
    assert_eq!(f.record.borrow().scripts.len(), 1, "nothing was written into the new document");
    assert!(f.notices.borrow().is_empty(), "and the user is told nothing");
    assert!(!f.state().busy && !f.state().active);
    // The tab can be translated again.
    assert!(f.app.borrow().translate.job.is_none());
}

#[test]
fn a_worker_error_after_some_text_was_written_puts_the_page_back() {
    let f = Fixture::new("error");
    f.reply(&collect_reply(r#"["Bonjour"]"#, 2));
    f.reply(&apply_reply(1, 2, r#"["Merci"]"#));
    f.reply(r#"{"v":1,"c":"ok","k":"","i":0,"n":0,"s":[]}"#);
    f.start();
    f.worker(r#"{"t":"ready"}"#);
    f.worker(&format!(r#"{{"t":"loaded","id":{}}}"#, f.posted_id(0)));
    f.worker(&format!(r#"{{"t":"out","id":{},"s":["Hello"]}}"#, f.posted_id(1)));
    assert_eq!(f.record.borrow().posted.len(), 3, "the second batch is with the worker");
    f.worker(&format!(r#"{{"t":"error","id":{},"m":"translate"}}"#, f.posted_id(2)));
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Failed]);
    assert!(f.record.borrow().scripts.last().unwrap().ends_with(r#"({"a":"restore"})"#), "the page was put back");
    assert!(!f.state().active && !f.state().busy && !f.job_running());
    // An error nobody is waiting for changes nothing.
    f.worker(r#"{"t":"error","id":999,"m":"model"}"#);
    assert_eq!(f.notices.borrow().len(), 1);
}

#[test]
fn a_step_that_never_answers_fails_on_the_watchdog_and_the_worker_is_closed() {
    let f = Fixture::new("watchdog");
    f.start();
    assert!(f.job_running(), "waiting for the page to say it is ready");
    App::translate_watch_timer(&f.app);
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Failed]);
    assert_eq!(f.record.borrow().closed, 1, "an unresponsive worker is not reused");
    assert!(!f.job_running() && !f.state().busy);
    assert!(!f.app.borrow().translate.worker_ready);
    // A watchdog that fires for a job that has moved on does nothing.
    let g = Fixture::new("watchdog-moved");
    g.reply(&collect_reply(r#"["Bonjour"]"#, 1));
    g.start();
    let stale = g.app.borrow().translate.watch;
    g.worker(r#"{"t":"ready"}"#);
    g.app.borrow_mut().translate.watch = stale;
    App::translate_watch_timer(&g.app);
    assert!(g.notices.borrow().is_empty(), "the step the watchdog was armed for is over");
    assert!(g.job_running());
}

#[test]
fn a_worker_that_cannot_start_or_fails_later_fails_the_job() {
    let f = Fixture::new("nostart");
    f.record.borrow_mut().refuse_open = true;
    f.start();
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Failed]);
    assert!(!f.job_running() && !f.state().busy);
    let g = Fixture::new("died");
    g.start();
    assert!(g.job_running());
    App::translate_worker_failed(&g.app);
    assert_eq!(*g.notices.borrow(), [TranslateNotice::Failed]);
    assert!(!g.job_running() && !g.app.borrow().translate.worker_ready);
}

#[test]
fn the_idle_timer_closes_the_worker_unless_a_translation_is_using_it() {
    let f = Fixture::new("idle");
    f.start();
    App::translate_idle_timer(&f.app);
    assert_eq!(f.record.borrow().closed, 0, "a job is using it");
    App::translate_watch_timer(&f.app);
    App::translate_idle_timer(&f.app);
    assert_eq!(f.record.borrow().closed, 2, "the stalled job closed it, and idle closes what is left (a second close is harmless)");
    assert!(!f.app.borrow().translate.worker_ready);
}

#[test]
fn commands_that_cannot_run_say_why_and_start_nothing() {
    // No language, same language, no route, not a web page, busy tab, another tab's job.
    let f = Fixture::new("refuse");
    let set_lang = |code: &str| {
        f.app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == f.tab).unwrap().translate.page_lang = if code.is_empty() { Lang::NONE } else { Lang::new(code).unwrap() };
    };
    set_lang("");
    f.start();
    set_lang("en");
    f.start();
    set_lang("ja");
    f.start();
    assert_eq!(
        *f.notices.borrow(),
        [
            TranslateNotice::NoLanguage,
            TranslateNotice::SameLanguage,
            TranslateNotice::NoRoute { from: "ja".into(), to: "English".into() },
        ]
    );
    set_lang("fr");
    f.notices.borrow_mut().clear();
    f.app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == f.tab).unwrap().url = Some(url::Url::parse("bowser://settings").unwrap());
    f.start();
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Unavailable], "only web pages");
    assert!(!App::translate_available(&f.app, f.tab));
    f.notices.borrow_mut().clear();
    f.app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == f.tab).unwrap().url = Some(url::Url::parse("https://example.test/").unwrap());
    f.app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == f.tab).unwrap().translate.busy = true;
    f.start();
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Unavailable], "this tab is mid-translation");
    f.app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == f.tab).unwrap().translate.busy = false;
    f.notices.borrow_mut().clear();
    f.start();
    f.start();
    assert_eq!(f.notices.borrow().last(), Some(&TranslateNotice::Unavailable), "a second start on the same tab");
    // Another tab while a job runs.
    let other = App::new_tab(&f.app);
    {
        let mut state = f.app.borrow_mut();
        let entry = state.tabs.iter_mut().find(|entry| entry.id == other).unwrap();
        entry.url = Some(url::Url::parse("https://other.test/").unwrap());
        entry.translate.page_lang = Lang::new("fr").unwrap();
    }
    App::translate_toggle(&f.app, other, false, f.notify.clone());
    assert_eq!(f.notices.borrow().last(), Some(&TranslateNotice::Busy));
    assert_eq!(f.record.borrow().opened.len(), 1, "only one worker start");
}

#[test]
fn a_private_tab_asks_before_any_download_and_never_stores_a_site_choice() {
    let f = Fixture::new("private");
    let private = App::new_private_tab(&f.app);
    {
        let mut state = f.app.borrow_mut();
        let entry = state.tabs.iter_mut().find(|entry| entry.id == private).unwrap();
        entry.url = Some(url::Url::parse("https://example.test/prive").unwrap());
        entry.translate.page_lang = Lang::new("de").unwrap();
        // The files for de to en are not on disk (an empty temporary cache): a private tab asks first.
        state.site_prefs = Some(Rc::new(RefCell::new(SitePrefsStore::in_memory())));
    }
    App::translate_toggle(&f.app, private, false, f.notify.clone());
    match f.notices.borrow().as_slice() {
        [TranslateNotice::NeedsConfirm { from, to, bytes }] => {
            assert_eq!((from.as_str(), to.as_str()), ("German", "English"));
            assert!(*bytes > 20 * 1024 * 1024, "the size of the pack is stated: {bytes}");
        }
        other => panic!("{other:?}"),
    }
    assert!(!f.job_running(), "nothing started, so nothing was downloaded");
    assert!(f.record.borrow().opened.is_empty() && !f.root.exists(), "and no worker or cache folder was made");
    // Private tabs never store a per-site choice, and never read one.
    assert!(!App::translate_set_site(&f.app, private, Some(SiteTranslate::Always)));
    assert_eq!(App::translate_site_choice(&f.app, private), None);
    assert_eq!(f.app.borrow().site_prefs.as_ref().unwrap().borrow().len(), 0);
    // A normal tab can.
    assert!(App::translate_set_site(&f.app, f.tab, Some(SiteTranslate::Always)));
    assert_eq!(App::translate_site_choice(&f.app, f.tab), Some(SiteTranslate::Always));
    assert!(App::translate_set_site(&f.app, f.tab, None));
    assert_eq!(f.app.borrow().site_prefs.as_ref().unwrap().borrow().len(), 0);
    // A private tab ignores what a normal tab stored for the same site.
    assert!(App::translate_set_site(&f.app, f.tab, Some(SiteTranslate::Never)));
    {
        let mut state = f.app.borrow_mut();
        state.tabs.iter_mut().find(|entry| entry.id == private).unwrap().url = Some(url::Url::parse("https://example.test/page").unwrap());
    }
    assert_eq!(App::translate_site_choice(&f.app, private), None);
    assert_eq!(App::translate_offer(&f.app, private), translate::Offer::Offer, "private: the stored never is not consulted");
    assert_eq!(App::translate_offer(&f.app, f.tab), translate::Offer::No, "a normal tab honours it");
}

#[test]
fn the_offer_follows_the_settings_and_never_asks_for_the_language_before_a_page_names_one() {
    let f = Fixture::new("offer");
    assert_eq!(App::translate_offer(&f.app, f.tab), translate::Offer::Offer);
    assert_eq!(App::translate_pill(&f.app, f.tab), translate::Pill::Available);
    f.app.borrow_mut().settings.translate_offer = false;
    assert_eq!(App::translate_pill(&f.app, f.tab), translate::Pill::Hidden, "the setting hides the button");
    f.app.borrow_mut().settings.translate_offer = true;
    // A page with no language, or in the target language, or a page that is not a website: no button.
    f.app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == f.tab).unwrap().translate.page_lang = Lang::NONE;
    assert_eq!(App::translate_pill(&f.app, f.tab), translate::Pill::Hidden);
    f.app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == f.tab).unwrap().translate.page_lang = Lang::new("en").unwrap();
    assert_eq!(App::translate_pill(&f.app, f.tab), translate::Pill::Hidden);
    // A setting that names a target is used as it is.
    f.app.borrow_mut().settings.translate_target = "es".into();
    assert_eq!(App::translate_pill(&f.app, f.tab), translate::Pill::Available, "English page, Spanish wanted");
    // "Always translate this site" with the automatic setting starts nothing while the files are missing.
    f.app.borrow_mut().settings.translate_target = "en".into();
    f.app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == f.tab).unwrap().translate.page_lang = Lang::new("fr").unwrap();
    f.app.borrow_mut().site_prefs = Some(Rc::new(RefCell::new(SitePrefsStore::in_memory())));
    assert!(App::translate_set_site(&f.app, f.tab, Some(SiteTranslate::Always)));
    f.app.borrow_mut().settings.translate_auto = true;
    assert_eq!(App::translate_offer(&f.app, f.tab), translate::Offer::Offer, "no files on disk: offered, not started");
    App::translate_after_probe(&f.app, f.tab);
    assert!(!f.job_running() && f.record.borrow().opened.is_empty(), "nothing is downloaded or started unasked");
}

#[test]
fn a_page_that_was_already_translated_is_recognised_instead_of_translated_twice() {
    let f = Fixture::new("already");
    f.reply(r#"{"v":1,"c":"on","k":"tok1","i":3,"n":3,"s":[]}"#);
    f.start();
    f.worker(r#"{"t":"ready"}"#);
    f.worker(&format!(r#"{{"t":"loaded","id":{}}}"#, f.posted_id(0)));
    assert!(f.state().active, "the page says it is translated");
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Done]);
    assert_eq!(f.record.borrow().posted.len(), 1, "nothing more went to the worker");
}

#[test]
fn a_page_with_nothing_to_translate_says_so() {
    let f = Fixture::new("none");
    f.reply(r#"{"v":1,"c":"none","k":"","i":0,"n":0,"s":[]}"#);
    f.start();
    f.worker(r#"{"t":"ready"}"#);
    f.worker(&format!(r#"{{"t":"loaded","id":{}}}"#, f.posted_id(0)));
    assert_eq!(*f.notices.borrow(), [TranslateNotice::NothingToTranslate]);
    assert!(!f.state().active && !f.state().busy);
}

#[test]
fn worker_messages_that_are_not_valid_are_ignored() {
    let f = Fixture::new("junk");
    f.start();
    for junk in ["", "ready", r#"{"t":"ready","x":1}"#, r#"{"t":"loaded"}"#, r#"{"t":"out","id":1,"s":[1]}"#, "[]", "{\"t\":\"loaded\",\"id\":-1}"] {
        f.worker(junk);
    }
    assert!(!f.app.borrow().translate.worker_ready);
    assert!(f.record.borrow().posted.is_empty());
    assert!(f.notices.borrow().is_empty());
}

#[test]
fn starting_a_translation_never_runs_while_navigation_state_is_reset() {
    // The tab's translate state is what a navigation resets: busy and active are cleared and the epoch moves on.
    let f = Fixture::new("reset");
    {
        let mut state = f.app.borrow_mut();
        let entry = state.tabs.iter_mut().find(|entry| entry.id == f.tab).unwrap();
        entry.translate.active = true;
        entry.translate.busy = true;
        entry.translate.reset();
        assert_eq!((entry.translate.active, entry.translate.busy, entry.translate.epoch), (false, false, 1));
        assert!(entry.translate.page_lang.is_none(), "the new document's language is unknown until its probe");
    }
    assert_eq!(App::translate_pill(&f.app, f.tab), translate::Pill::Hidden);
}
