//! What the feed and toolbar options do to the running app (src/app/feed_features.rs), driven with a
//! recording engine and no window: the page's feed links are read after a load and shown only for that
//! page, `feeds_detect` switches the whole thing off, and a bar option is checked, kept and saved.

use super::*;

struct FeedEngine {
    /// What the page's script answers; `None` is a page that cannot be scripted.
    answer: Rc<RefCell<Option<String>>>,
    scripts: Rc<RefCell<u32>>,
}

impl EngineAdapter for FeedEngine {
    fn navigate(&mut self, _url: &url::Url) -> windows::core::Result<()> {
        Ok(())
    }
    fn navigate_tab(&mut self, _tab: TabId, _url: &url::Url) -> windows::core::Result<()> {
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
    fn run_feed_script(&mut self, _tab: TabId, _script: &str, done: Box<dyn FnOnce(Option<String>)>) {
        *self.scripts.borrow_mut() += 1;
        done(self.answer.borrow().clone());
    }
}

const ONE_FEED: &str = r#"[["alternate","application/rss+xml","https://blog.example/feed.xml","Posts"],["stylesheet","text/css","/a.css",""]]"#;

type Fixture = (AppRef, Rc<RefCell<Option<String>>>, Rc<RefCell<u32>>);

fn fixture(answer: Option<&str>) -> Fixture {
    let app = app_with_pending(rect(800, 600));
    let answer = Rc::new(RefCell::new(answer.map(str::to_owned)));
    let scripts = Rc::new(RefCell::new(0));
    app.borrow_mut().engine = Some(Box::new(FeedEngine { answer: answer.clone(), scripts: scripts.clone() }));
    (app, answer, scripts)
}

/// A web page in tab `id`, as if it had loaded.
fn page(app: &AppRef, id: TabId, address: &str) {
    let mut state = app.borrow_mut();
    state.tab_model.set_url(id, Some(address));
    let tab = state.tabs.iter_mut().find(|tab| tab.id == id).expect("tab");
    tab.url = Some(url::Url::parse(address).expect("url"));
    tab.controller_created = true;
    tab.page_usable = true;
}

#[test]
fn a_loaded_page_that_offers_a_feed_shows_its_links_for_that_page_only() {
    let (app, _, scripts) = fixture(Some(ONE_FEED));
    let tab = App::active_tab_id(&app);
    page(&app, tab, "https://blog.example/post");
    assert!(App::active_feed_links(&app).is_empty(), "nothing is known before the page is read");
    App::feed_probe(&app, tab);
    assert_eq!(*scripts.borrow(), 1);
    let links = App::active_feed_links(&app);
    assert_eq!(links.len(), 1, "the stylesheet link is not a feed");
    assert_eq!((links[0].url.as_str(), links[0].title.as_str()), ("https://blog.example/feed.xml", "Posts"));
    // The tab goes to another page: the old page's feeds are not shown for it.
    page(&app, tab, "https://other.example/");
    assert!(App::active_feed_links(&app).is_empty());
}

#[test]
fn a_page_with_no_feed_or_no_script_shows_none_and_forgets_an_older_answer() {
    let (app, answer, _) = fixture(Some(ONE_FEED));
    let tab = App::active_tab_id(&app);
    page(&app, tab, "https://blog.example/post");
    App::feed_probe(&app, tab);
    assert_eq!(App::active_feed_links(&app).len(), 1);
    *answer.borrow_mut() = Some("[]".to_owned());
    App::feed_probe(&app, tab);
    assert!(App::active_feed_links(&app).is_empty());
    *answer.borrow_mut() = Some(ONE_FEED.to_owned());
    App::feed_probe(&app, tab);
    *answer.borrow_mut() = None;
    App::feed_probe(&app, tab);
    assert!(App::active_feed_links(&app).is_empty(), "a page that cannot be scripted offers nothing");
    *answer.borrow_mut() = Some("not json at all".to_owned());
    App::feed_probe(&app, tab);
    assert!(App::active_feed_links(&app).is_empty());
}

#[test]
fn feeds_detect_off_reads_nothing_and_hides_what_was_read() {
    let (app, _, scripts) = fixture(Some(ONE_FEED));
    let tab = App::active_tab_id(&app);
    page(&app, tab, "https://blog.example/post");
    App::feed_probe(&app, tab);
    assert_eq!(App::active_feed_links(&app).len(), 1);
    assert!(App::apply_bar_option(&app, "feeds_detect=false"));
    assert!(App::active_feed_links(&app).is_empty(), "off: the button goes");
    let before = *scripts.borrow();
    App::feed_probe(&app, tab);
    assert_eq!(*scripts.borrow(), before, "off: the page is not even asked");
    assert!(App::apply_bar_option(&app, "feeds_detect=true"));
    App::feed_probe(&app, tab);
    assert_eq!(App::active_feed_links(&app).len(), 1);
}

#[test]
fn only_a_web_page_is_read() {
    let (app, _, scripts) = fixture(Some(ONE_FEED));
    let tab = App::active_tab_id(&app);
    page(&app, tab, "https://blog.example/post");
    app.borrow_mut().tabs.iter_mut().find(|t| t.id == tab).unwrap().url = Some(url::Url::parse("bowser://settings").unwrap());
    App::feed_probe(&app, tab);
    assert_eq!(*scripts.borrow(), 0);
}

#[test]
fn a_bar_option_is_checked_kept_and_refused_without_changing_anything() {
    let (app, _, _) = fixture(None);
    assert!(App::apply_bar_option(&app, "toolbar_tabs=cmd:new-private-tab,closed-tabs"));
    assert_eq!(app.borrow().settings.toolbar_tabs, ["cmd:new-private-tab", "closed-tabs"]);
    assert!(App::apply_bar_option(&app, "toolbar_panel=feeds"));
    assert_eq!(app.borrow().settings.toolbar_panel, ["feeds"]);
    assert!(App::apply_bar_option(&app, "feeds_refresh_minutes=0"));
    assert_eq!(app.borrow().settings.feeds_refresh_minutes, 0);
    let before = app.borrow().settings.clone();
    for bad in ["toolbar_tabs=feeds", "toolbar_panel=back", "feeds_refresh_minutes=7", "feeds_detect=maybe", "tab_layout=vertical", "toolbar_buttons=back"] {
        assert!(!App::apply_bar_option(&app, bad) || bad.starts_with("toolbar_buttons="), "{bad}");
    }
    let after = app.borrow().settings.clone();
    assert_eq!(after.toolbar_tabs, before.toolbar_tabs);
    assert_eq!(after.toolbar_panel, before.toolbar_panel);
    assert_eq!(after.tab_layout, before.tab_layout);
    assert_eq!(after.feeds_refresh_minutes, before.feeds_refresh_minutes);
}
