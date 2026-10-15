//! The app's side of page translation, driven end to end with a scripted engine and a scripted
//! Google: no window and no network. The engine answers each page script from a queue (or holds it,
//! to play a step that never answers) and records what it was asked; Google is a closure that records
//! each request and answers it at once.

use super::*;
use crate::{
    site_prefs::{SitePrefsStore, SiteTranslate},
    translate::{self, Lang, google::GoogleError},
};

#[derive(Default)]
struct Record {
    scripts: Vec<String>,
    /// Answers of scripts that were held back instead of answered.
    held: Vec<Box<dyn FnOnce(Option<String>)>>,
    hold: bool,
}

struct FakeEngine {
    record: Rc<RefCell<Record>>,
    replies: Rc<RefCell<VecDeque<Option<String>>>>,
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
        let mut record = self.record.borrow_mut();
        record.scripts.push(script.to_owned());
        if record.hold {
            record.held.push(done);
            return;
        }
        drop(record);
        let reply = self.replies.borrow_mut().pop_front().flatten();
        done(reply);
    }
}

type Request = (String, String, Vec<String>);
type Answer = std::result::Result<Vec<String>, GoogleError>;

struct Fixture {
    app: AppRef,
    tab: TabId,
    record: Rc<RefCell<Record>>,
    replies: Rc<RefCell<VecDeque<Option<String>>>>,
    requests: Rc<RefCell<Vec<Request>>>,
    notices: Rc<RefCell<Vec<TranslateNotice>>>,
    notify: Rc<dyn Fn(TranslateNotice)>,
}

impl Fixture {
    /// A French page in the active tab, translating into English. Google answers every string with
    /// `T:` in front of it, and every request it gets is recorded.
    fn new() -> Self {
        let app = app_with_pending(rect(800, 600));
        let record = Rc::new(RefCell::new(Record::default()));
        let replies = Rc::new(RefCell::new(VecDeque::new()));
        app.borrow_mut().engine = Some(Box::new(FakeEngine { record: record.clone(), replies: replies.clone() }));
        let tab = App::active_tab_id(&app);
        let requests = Rc::new(RefCell::new(Vec::new()));
        {
            let mut state = app.borrow_mut();
            state.settings.translate_target = "en".into();
            let entry = state.tabs.iter_mut().find(|entry| entry.id == tab).unwrap();
            entry.url = Some(url::Url::parse("https://example.test/page").unwrap());
            entry.translate.page_lang = Lang::new("fr").unwrap();
            let seen = requests.clone();
            state.translate.fetch = Some(Rc::new(move |from, to, strings| {
                seen.borrow_mut().push((from.to_owned(), to.to_owned(), strings.to_vec()));
                Ok(strings.iter().map(|text| format!("T:{text}")).collect())
            }));
        }
        let notices = Rc::new(RefCell::new(Vec::new()));
        let sink = notices.clone();
        let notify: Rc<dyn Fn(TranslateNotice)> = Rc::new(move |notice| sink.borrow_mut().push(notice));
        Self { app, tab, record, replies, requests, notices, notify }
    }

    fn reply(&self, text: &str) {
        self.replies.borrow_mut().push_back(Some(text.to_owned()));
    }

    fn start(&self) {
        App::translate_toggle(&self.app, self.tab, false, self.notify.clone());
    }

    /// Google answers with `answer` from now on.
    fn google(&self, answer: impl Fn(&str, &str, &[String]) -> Answer + 'static) {
        self.app.borrow_mut().translate.fetch = Some(Rc::new(answer));
    }

    fn page_lang(&self, code: &str) {
        let lang = if code.is_empty() { Lang::NONE } else { Lang::new(code).unwrap() };
        self.app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == self.tab).unwrap().translate.page_lang = lang;
    }

    fn state(&self) -> translate::TabState {
        self.app.borrow().tabs.iter().find(|entry| entry.id == self.tab).unwrap().translate
    }

    fn job_running(&self) -> bool {
        self.app.borrow().translate.job.is_some()
    }

    fn scripts(&self) -> usize {
        self.record.borrow().scripts.len()
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.borrow().clone()
    }
}

fn collect_reply(strings: &str, total: usize) -> String {
    format!("{{\"v\":1,\"c\":\"ok\",\"k\":\"tok1\",\"i\":0,\"n\":{total},\"s\":{strings}}}")
}

fn apply_reply(next: usize, total: usize, strings: &str) -> String {
    format!("{{\"v\":1,\"c\":\"ok\",\"k\":\"tok1\",\"i\":{next},\"n\":{total},\"s\":{strings}}}")
}

const EMPTY_REPLY: &str = r#"{"v":1,"c":"ok","k":"","i":0,"n":0,"s":[]}"#;

fn request(from: &str, to: &str, strings: &[&str]) -> Request {
    (from.to_owned(), to.to_owned(), strings.iter().map(|text| (*text).to_owned()).collect())
}

#[test]
fn a_translation_runs_collect_translate_apply_until_the_page_is_done() {
    let f = Fixture::new();
    f.reply(&collect_reply(r#"["Bonjour","Le chat"]"#, 3));
    f.reply(&apply_reply(2, 3, r#"["Merci"]"#));
    f.reply(&apply_reply(3, 3, "[]"));
    f.start();
    // Each batch went to Google once, from the page's language into the target.
    assert_eq!(f.requests(), [request("fr", "en", &["Bonjour", "Le chat"]), request("fr", "en", &["Merci"])]);
    // The page's first batch was asked for, and each translation was written back with the page's
    // token and the index it starts at.
    let scripts = f.record.borrow().scripts.clone();
    assert!(scripts[0].ends_with(r#"({"a":"collect"})"#));
    assert!(scripts[1].contains(r#"{"a":"apply","k":"tok1","i":0,"s":["T:Bonjour","T:Le chat"]}"#), "{}", scripts[1]);
    assert!(scripts[2].contains(r#""i":2,"s":["T:Merci"]"#), "{}", scripts[2]);
    // The last (empty) batch ends it.
    assert!(!f.job_running());
    assert!(f.state().active && !f.state().busy);
    assert_eq!(App::translate_pill(&f.app, f.tab), translate::Pill::On);
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Started, TranslateNotice::Done]);
    // The next command shows the original: a restore call, and the state follows.
    f.reply(EMPTY_REPLY);
    f.start();
    assert!(f.record.borrow().scripts[3].ends_with(r#"({"a":"restore"})"#));
    assert!(!f.state().active && !f.state().busy);
    assert_eq!(f.notices.borrow().last(), Some(&TranslateNotice::Restored));
    assert_eq!(f.requests().len(), 2, "showing the original sends nothing to Google");
}

#[test]
fn the_source_is_the_pages_language_when_listed_and_detected_by_google_otherwise() {
    for (page, source) in [("fr", "fr"), ("ja", "ja"), ("", "auto"), ("xyz", "auto")] {
        let f = Fixture::new();
        f.page_lang(page);
        f.reply(&collect_reply(r#"["x"]"#, 1));
        f.reply(&apply_reply(1, 1, "[]"));
        f.start();
        assert_eq!(f.requests(), [request(source, "en", &["x"])], "page language {page:?}");
        assert!(f.state().active, "page language {page:?}");
    }
    // A target that is not a language Google has is replaced, so the request never names it.
    let f = Fixture::new();
    f.app.borrow_mut().settings.translate_target = "xyz".into();
    f.reply(&collect_reply(r#"["x"]"#, 1));
    f.reply(&apply_reply(1, 1, "[]"));
    f.start();
    assert!(translate::is_supported(&f.requests()[0].1), "{:?}", f.requests());
}

#[test]
fn a_page_reply_that_is_not_the_documented_shape_fails_the_job_and_changes_nothing() {
    let f = Fixture::new();
    f.reply(r#"{"v":1,"c":"ok","k":"tok1","i":0,"n":1,"s":["a"],"extra":true}"#);
    f.start();
    assert!(!f.job_running() && !f.state().busy && !f.state().active);
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Started, TranslateNotice::Failed]);
    assert!(f.requests().is_empty(), "nothing was sent to Google");
    assert_eq!(f.scripts(), 1, "and nothing was applied or restored (nothing had changed)");
}

#[test]
fn an_answer_with_the_wrong_number_of_strings_is_not_applied() {
    let f = Fixture::new();
    f.google(|_, _, _| Ok(vec!["Hello".to_owned()]));
    f.reply(&collect_reply(r#"["Bonjour","Le chat"]"#, 2));
    f.start();
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Started, TranslateNotice::Failed]);
    assert_eq!(f.scripts(), 1, "only the collect call ran");
    assert!(!f.state().active && !f.state().busy);
}

#[test]
fn a_navigation_during_translation_drops_the_job_without_touching_the_new_page() {
    let f = Fixture::new();
    f.reply(&collect_reply(r#"["Bonjour"]"#, 1));
    // The document is replaced while Google is translating.
    let weak = Rc::downgrade(&f.app);
    let tab = f.tab;
    f.google(move |_, _, strings| {
        let app = weak.upgrade().unwrap();
        app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == tab).unwrap().translate.reset();
        Ok(strings.to_vec())
    });
    f.start();
    assert!(!f.job_running(), "the stale job is gone");
    assert_eq!(f.scripts(), 1, "nothing was written into the new document");
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Started], "and the user is told nothing more");
    assert!(!f.state().busy && !f.state().active);
}

#[test]
fn a_failed_request_after_some_text_was_written_puts_the_page_back() {
    let f = Fixture::new();
    f.reply(&collect_reply(r#"["Bonjour"]"#, 2));
    f.reply(&apply_reply(1, 2, r#"["Merci"]"#));
    f.reply(EMPTY_REPLY);
    let calls = Rc::new(std::cell::Cell::new(0));
    let counter = calls.clone();
    f.google(move |_, _, strings| {
        counter.set(counter.get() + 1);
        if counter.get() == 1 { Ok(strings.iter().map(|text| format!("T:{text}")).collect()) } else { Err(GoogleError::Network) }
    });
    f.start();
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Started, TranslateNotice::NetworkFailed]);
    assert!(f.record.borrow().scripts.last().unwrap().ends_with(r#"({"a":"restore"})"#), "the page was put back");
    assert!(!f.state().active && !f.state().busy && !f.job_running());
}

#[test]
fn each_way_google_can_refuse_has_its_own_message() {
    for (error, notice) in [
        (GoogleError::Network, TranslateNotice::NetworkFailed),
        (GoogleError::Limited, TranslateNotice::Limited),
        (GoogleError::Bad, TranslateNotice::Failed),
    ] {
        let f = Fixture::new();
        f.reply(&collect_reply(r#"["Bonjour"]"#, 1));
        f.google(move |_, _, _| Err(error));
        f.start();
        assert_eq!(*f.notices.borrow(), [TranslateNotice::Started, notice], "{error:?}");
        assert!(!f.job_running() && !f.state().busy && !f.state().active);
        assert_eq!(f.scripts(), 1, "nothing had been written, so nothing is restored");
    }
}

#[test]
fn a_late_answer_from_google_is_ignored_and_does_not_pile_up() {
    let f = Fixture::new();
    // An answer for a job that is gone (a request id nobody waits for) changes nothing.
    App::translate_net_done(&f.app, 77, 987_654);
    assert!(f.notices.borrow().is_empty() && !f.job_running());
}

#[test]
fn a_step_that_never_answers_fails_on_the_watchdog() {
    let f = Fixture::new();
    f.record.borrow_mut().hold = true;
    f.start();
    assert!(f.job_running(), "waiting for the page's first batch");
    App::translate_watch_timer(&f.app);
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Started, TranslateNotice::Failed]);
    assert!(!f.job_running() && !f.state().busy);
    // The tab can be translated again.
    f.record.borrow_mut().hold = false;
    f.record.borrow_mut().held.clear();
    f.reply(&collect_reply(r#"["Bonjour"]"#, 1));
    f.reply(&apply_reply(1, 1, "[]"));
    f.start();
    assert!(f.state().active);
    // A watchdog that fires for a job that has moved on does nothing.
    let g = Fixture::new();
    g.record.borrow_mut().hold = true;
    g.start();
    let stale = g.app.borrow().translate.watch;
    let release = g.record.borrow_mut().held.pop().unwrap();
    release(Some(collect_reply(r#"["Bonjour"]"#, 1)));
    g.app.borrow_mut().translate.watch = stale;
    App::translate_watch_timer(&g.app);
    assert_eq!(*g.notices.borrow(), [TranslateNotice::Started], "the step the watchdog was armed for is over");
    assert!(g.job_running(), "waiting for the page to take the translation");
}

#[test]
fn commands_that_cannot_run_say_why_and_start_nothing() {
    // Same language, not a web page, busy tab, another tab's job.
    let f = Fixture::new();
    f.page_lang("en");
    f.start();
    assert_eq!(*f.notices.borrow(), [TranslateNotice::SameLanguage]);
    f.page_lang("fr");
    f.notices.borrow_mut().clear();
    f.app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == f.tab).unwrap().url = Some(url::Url::parse("bowser://settings").unwrap());
    f.start();
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Unavailable], "only web pages");
    assert!(!App::translate_available(&f.app, f.tab));
    f.notices.borrow_mut().clear();
    f.app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == f.tab).unwrap().url = Some(url::Url::parse("https://example.test/").unwrap());
    f.app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == f.tab).unwrap().translate.busy = true;
    f.start();
    assert_eq!(*f.notices.borrow(), [TranslateNotice::InProgress], "this tab is mid-translation");
    f.app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == f.tab).unwrap().translate.busy = false;
    f.notices.borrow_mut().clear();
    // A second start on the same tab while the first waits on the page.
    f.record.borrow_mut().hold = true;
    f.start();
    f.start();
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Started, TranslateNotice::InProgress]);
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
    assert_eq!(f.scripts(), 1, "only the first tab was scripted");
}

#[test]
fn a_private_tab_asks_before_any_text_is_sent_and_never_stores_a_site_choice() {
    let f = Fixture::new();
    let private = App::new_private_tab(&f.app);
    {
        let mut state = f.app.borrow_mut();
        let entry = state.tabs.iter_mut().find(|entry| entry.id == private).unwrap();
        entry.url = Some(url::Url::parse("https://example.test/prive").unwrap());
        entry.translate.page_lang = Lang::new("de").unwrap();
        state.site_prefs = Some(Rc::new(RefCell::new(SitePrefsStore::in_memory())));
    }
    App::translate_toggle(&f.app, private, false, f.notify.clone());
    assert_eq!(*f.notices.borrow(), [TranslateNotice::NeedsConfirm]);
    assert!(!f.job_running(), "nothing started");
    assert_eq!(f.scripts(), 0, "the private page was not even read");
    assert!(f.requests().is_empty(), "and nothing went to Google");
    // Confirmed, it runs.
    f.reply(&collect_reply(r#"["Hallo"]"#, 1));
    f.reply(&apply_reply(1, 1, "[]"));
    App::translate_toggle(&f.app, private, true, f.notify.clone());
    assert_eq!(f.requests(), [request("de", "en", &["Hallo"])]);
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
    let f = Fixture::new();
    assert_eq!(App::translate_offer(&f.app, f.tab), translate::Offer::Offer);
    assert_eq!(App::translate_pill(&f.app, f.tab), translate::Pill::Available);
    f.app.borrow_mut().settings.translate_offer = false;
    assert_eq!(App::translate_pill(&f.app, f.tab), translate::Pill::Hidden, "the setting hides the button");
    f.app.borrow_mut().settings.translate_offer = true;
    // A page with no language, or in the target language: no button (the menu command still works).
    f.page_lang("");
    assert_eq!(App::translate_pill(&f.app, f.tab), translate::Pill::Hidden);
    f.page_lang("en");
    assert_eq!(App::translate_pill(&f.app, f.tab), translate::Pill::Hidden);
    // A setting that names a target is used as it is.
    f.app.borrow_mut().settings.translate_target = "es".into();
    assert_eq!(App::translate_pill(&f.app, f.tab), translate::Pill::Available, "English page, Spanish wanted");
    f.page_lang("ja");
    f.app.borrow_mut().settings.translate_target = "en".into();
    assert_eq!(App::translate_pill(&f.app, f.tab), translate::Pill::Available, "any language Google reads is offered");
}

#[test]
fn always_translate_with_the_automatic_setting_starts_after_the_probe_and_not_otherwise() {
    let f = Fixture::new();
    f.app.borrow_mut().site_prefs = Some(Rc::new(RefCell::new(SitePrefsStore::in_memory())));
    assert!(App::translate_set_site(&f.app, f.tab, Some(SiteTranslate::Always)));
    // The site choice alone only offers.
    assert_eq!(App::translate_offer(&f.app, f.tab), translate::Offer::Offer);
    App::translate_after_probe(&f.app, f.tab);
    assert!(!f.job_running() && f.requests().is_empty() && f.scripts() == 0, "nothing runs unasked");
    // With the automatic setting it starts by itself.
    f.app.borrow_mut().settings.translate_auto = true;
    assert_eq!(App::translate_offer(&f.app, f.tab), translate::Offer::Auto);
    f.reply(&collect_reply(r#"["Bonjour"]"#, 1));
    f.reply(&apply_reply(1, 1, "[]"));
    App::translate_after_probe(&f.app, f.tab);
    assert_eq!(f.requests(), [request("fr", "en", &["Bonjour"])]);
    assert!(f.state().active);
}

#[test]
fn a_page_that_was_already_translated_is_recognised_instead_of_translated_twice() {
    let f = Fixture::new();
    f.reply(r#"{"v":1,"c":"on","k":"tok1","i":3,"n":3,"s":[]}"#);
    f.start();
    assert!(f.state().active, "the page says it is translated");
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Started, TranslateNotice::Done]);
    assert!(f.requests().is_empty(), "nothing more went to Google");
}

#[test]
fn a_page_with_nothing_to_translate_says_so() {
    let f = Fixture::new();
    f.reply(r#"{"v":1,"c":"none","k":"","i":0,"n":0,"s":[]}"#);
    f.start();
    assert_eq!(*f.notices.borrow(), [TranslateNotice::Started, TranslateNotice::NothingToTranslate]);
    assert!(!f.state().active && !f.state().busy);
    assert!(f.requests().is_empty());
}

#[test]
fn starting_a_translation_never_runs_while_navigation_state_is_reset() {
    // The tab's translate state is what a navigation resets: busy and active are cleared and the epoch moves on.
    let f = Fixture::new();
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
