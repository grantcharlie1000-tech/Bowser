//! Page tools, the app's side (thread T7), driven end to end with a scripted engine: no window, no page.
//! The engine answers each DevTools call and page script from a queue and records what it was asked, so
//! these tests prove what a setting changes: which calls a capture makes, where the file goes and what
//! it is called, how the zoom is kept, what the reader is told, and what the engine is asked to apply.

use super::*;
use crate::{
    capture::{self, tests as pics},
    page_tools::{CaptureFormat, CaptureMode, CaptureTarget, ZoomScope},
};

#[derive(Default)]
struct Record {
    calls: Vec<(String, String)>,
    scripts: Vec<String>,
    zooms: Vec<(TabId, u16)>,
    applied: usize,
    origin_scripts: Vec<(String, String)>,
}

struct FakeEngine {
    record: Rc<RefCell<Record>>,
    calls: Rc<RefCell<VecDeque<Option<String>>>>,
    scripts: Rc<RefCell<VecDeque<Option<String>>>>,
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
    fn devtools_call(&mut self, _tab: TabId, method: &str, params: &str, done: Box<dyn FnOnce(Option<String>)>) {
        self.record.borrow_mut().calls.push((method.to_owned(), params.to_owned()));
        let answer = self.calls.borrow_mut().pop_front().flatten();
        done(answer);
    }
    fn run_page_script(&mut self, _tab: TabId, script: &str, _limit: usize, done: Box<dyn FnOnce(Option<String>)>) {
        self.record.borrow_mut().scripts.push(script.to_owned());
        let answer = self.scripts.borrow_mut().pop_front().flatten();
        done(answer);
    }
    fn apply_page_tools(&mut self) {
        self.record.borrow_mut().applied += 1;
    }
    fn zoom_percent(&self, _tab: TabId) -> Option<u16> {
        Some(100)
    }
    fn set_zoom_percent(&mut self, tab: TabId, percent: u16) -> windows::core::Result<bool> {
        self.record.borrow_mut().zooms.push((tab, percent));
        Ok(true)
    }
    fn execute_script_for_origin(&mut self, _tab: TabId, origin: &str, script: &str) -> windows::core::Result<()> {
        self.record.borrow_mut().origin_scripts.push((origin.to_owned(), script.to_owned()));
        Ok(())
    }
}

struct Fixture {
    app: AppRef,
    tab: TabId,
    record: Rc<RefCell<Record>>,
    calls: Rc<RefCell<VecDeque<Option<String>>>>,
    scripts: Rc<RefCell<VecDeque<Option<String>>>>,
    dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let app = app_with_pending(rect(800, 600));
        let record = Rc::new(RefCell::new(Record::default()));
        let calls = Rc::new(RefCell::new(VecDeque::new()));
        let scripts = Rc::new(RefCell::new(VecDeque::new()));
        app.borrow_mut().engine = Some(Box::new(FakeEngine { record: record.clone(), calls: calls.clone(), scripts: scripts.clone() }));
        let tab = App::active_tab_id(&app);
        let dir = std::env::temp_dir().join(format!("bowser-pt-flow-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        {
            let mut state = app.borrow_mut();
            state.settings.page_tools.capture_dir = dir.to_string_lossy().into_owned();
            let entry = state.tabs.iter_mut().find(|entry| entry.id == tab).unwrap();
            entry.url = Some(url::Url::parse("https://www.example.test/docs/page").unwrap());
            entry.title = "Docs: the page?".into();
        }
        Self { app, tab, record, calls, scripts, dir }
    }

    fn tools(&self, edit: impl FnOnce(&mut crate::page_tools::PageTools)) {
        edit(&mut self.app.borrow_mut().settings.page_tools);
    }

    fn call_answer(&self, bytes: &[u8]) {
        self.calls.borrow_mut().push_back(Some(pics::answer(bytes)));
    }

    fn capture(&self, mode: CaptureMode, target: CaptureTarget) -> CaptureResult {
        let sink = Rc::new(RefCell::new(None));
        let out = sink.clone();
        App::capture_page(&self.app, self.tab, mode, target, Rc::new(move |result| *out.borrow_mut() = Some(result)));
        sink.borrow_mut().take().expect("a capture with a scripted engine finishes at once")
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.dir).map(|dir| dir.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        names.sort();
        names
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn capturing_the_visible_area_asks_for_one_picture_and_saves_it_under_the_template() {
    let fx = Fixture::new("visible");
    let png = pics::picture(8, 6, CaptureFormat::Png, |_| 90);
    fx.call_answer(&png);
    let result = fx.capture(CaptureMode::Visible, CaptureTarget::File);
    let CaptureResult::Saved { path, cut } = result else { panic!("{result:?}") };
    assert!(!cut);
    assert_eq!(fx.record.borrow().calls, vec![("Page.captureScreenshot".to_owned(), "{\"format\":\"png\",\"fromSurface\":true}".to_owned())]);
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("www.example.test ") && name.ends_with(".png"), "{name}: the default template is {{host}} {{date}} {{time}}");
    assert_eq!(std::fs::read(&path).unwrap(), png, "the picture the browser made is what is saved");
    assert!(!crate::app::capture_busy(), "a finished capture releases the next one");
}

#[test]
fn the_folder_the_name_template_and_the_format_settings_decide_where_and_what_is_saved() {
    let fx = Fixture::new("settings");
    fx.tools(|t| {
        t.capture_name = "{title} - {mode}".into();
        t.capture_format = CaptureFormat::Jpeg;
    });
    let jpeg = pics::picture(8, 6, CaptureFormat::Jpeg, |_| 90);
    fx.call_answer(&jpeg);
    let CaptureResult::Saved { path, .. } = fx.capture(CaptureMode::Visible, CaptureTarget::File) else { panic!() };
    assert_eq!(path, fx.dir.join("Docs the page - visible.jpg"), "the page title is cleaned for a file name, and the format sets the extension");
    assert!(fx.record.borrow().calls[0].1.contains("\"format\":\"jpeg\",\"fromSurface\":true,\"quality\":90"));
    // The same name again is numbered, never overwritten.
    fx.call_answer(&jpeg);
    let CaptureResult::Saved { path: second, .. } = fx.capture(CaptureMode::Visible, CaptureTarget::File) else { panic!() };
    assert_eq!(second, fx.dir.join("Docs the page - visible (2).jpg"));
    assert_eq!(fx.files().len(), 2);
}

#[test]
fn a_full_page_is_captured_in_slices_and_stitched_into_one_picture() {
    let fx = Fixture::new("full");
    fx.calls.borrow_mut().push_back(Some(r#"{"cssContentSize":{"x":0,"y":0,"width":4,"height":13001.5}}"#.to_owned()));
    for (top, height) in crate::page_tools::plan_slices(13_002) {
        fx.call_answer(&pics::picture(4, height, CaptureFormat::Png, move |row| ((top + row) / 400) as u8));
    }
    let CaptureResult::Saved { path, cut } = fx.capture(CaptureMode::FullPage, CaptureTarget::File) else { panic!() };
    assert!(!cut);
    let calls = fx.record.borrow().calls.clone();
    assert_eq!(calls[0], ("Page.getLayoutMetrics".to_owned(), "{}".to_owned()));
    let clips: Vec<&str> = calls[1..].iter().map(|(_, params)| params.split("\"clip\":").nth(1).unwrap()).collect();
    assert_eq!(
        clips,
        [
            "{\"x\":0,\"y\":0,\"width\":4,\"height\":6000,\"scale\":1}}",
            "{\"x\":0,\"y\":6000,\"width\":4,\"height\":6000,\"scale\":1}}",
            "{\"x\":0,\"y\":12000,\"width\":4,\"height\":1002,\"scale\":1}}"
        ]
    );
    assert!(calls[1..].iter().all(|(m, p)| m == "Page.captureScreenshot" && p.contains("\"captureBeyondViewport\":true")));
    assert_eq!(capture::size_of(&std::fs::read(path).unwrap()), Some((4, 13_002)));
}

#[test]
fn a_page_longer_than_30000_pixels_is_cut_and_the_result_says_so() {
    let fx = Fixture::new("tall");
    fx.calls.borrow_mut().push_back(Some(r#"{"cssContentSize":{"width":4,"height":90000}}"#.to_owned()));
    for (top, height) in crate::page_tools::plan_slices(30_000) {
        fx.call_answer(&pics::picture(4, height, CaptureFormat::Png, move |row| ((top + row) / 700) as u8));
    }
    let CaptureResult::Saved { path, cut } = fx.capture(CaptureMode::FullPage, CaptureTarget::File) else { panic!() };
    assert!(cut, "the cap is reported");
    assert_eq!(fx.record.borrow().calls.len(), 1 + 5, "five slices, never more");
    assert_eq!(capture::size_of(&std::fs::read(path).unwrap()), Some((4, 30_000)));
}

#[test]
fn a_capture_is_joined_and_saved_off_the_ui_thread() {
    let fx = Fixture::new("worker");
    fx.calls.borrow_mut().push_back(Some(r#"{"cssContentSize":{"x":0,"y":0,"width":4,"height":10}}"#.to_owned()));
    fx.call_answer(&pics::picture(4, 10, CaptureFormat::Png, |_| 7));
    *crate::app::page_actions::FINISH_THREAD.lock().unwrap() = None;
    let CaptureResult::Saved { path, .. } = fx.capture(CaptureMode::FullPage, CaptureTarget::File) else { panic!() };
    assert!(path.exists());
    let worker = crate::app::page_actions::FINISH_THREAD.lock().unwrap().clone();
    assert_eq!(worker.as_deref(), Some("bowser-capture-finish"), "stitching and saving ran on the worker");
    assert_ne!(std::thread::current().name(), Some("bowser-capture-finish"));
    assert!(!crate::app::capture_busy(), "the result came back and ended the job");
}

#[test]
fn a_selection_is_captured_with_a_clip_around_its_box() {
    let fx = Fixture::new("selection");
    fx.scripts.borrow_mut().push_back(Some(format!("{:?}", r#"{"c":1,"t":"hello","x":120,"y":900,"w":300,"h":40,"a":"https://www.example.test/docs/page","n":"Docs"}"#)));
    fx.call_answer(&pics::picture(8, 6, CaptureFormat::Png, |_| 5));
    let CaptureResult::Saved { path, .. } = fx.capture(CaptureMode::Selection, CaptureTarget::File) else { panic!() };
    assert!(path.file_name().unwrap().to_string_lossy().ends_with(".png"));
    assert_eq!(fx.record.borrow().scripts, vec![capture::SELECTION_SCRIPT.to_owned()]);
    assert!(fx.record.borrow().calls[0].1.ends_with("\"clip\":{\"x\":120,\"y\":900,\"width\":300,\"height\":40,\"scale\":1}}"), "{:?}", fx.record.borrow().calls);
    // Nothing selected: no picture is asked for.
    fx.scripts.borrow_mut().push_back(Some(format!("{:?}", r#"{"c":0}"#)));
    assert_eq!(fx.capture(CaptureMode::Selection, CaptureTarget::File), CaptureResult::NothingSelected);
    assert_eq!(fx.record.borrow().calls.len(), 1);
}

#[test]
fn copying_makes_a_bitmap_and_a_note_capture_names_the_saved_file_and_the_page() {
    let fx = Fixture::new("targets");
    let png = pics::picture(5, 2, CaptureFormat::Png, |row| if row == 0 { 200 } else { 20 });
    fx.call_answer(&png);
    let CaptureResult::Clipboard { dib, .. } = fx.capture(CaptureMode::Visible, CaptureTarget::Clipboard) else { panic!() };
    assert_eq!(dib.len(), 40 + 16 * 2);
    assert!(fx.files().is_empty(), "a copy writes no file");
    fx.call_answer(&png);
    let CaptureResult::Note { path, page_title, addr, .. } = fx.capture(CaptureMode::Visible, CaptureTarget::Note) else { panic!() };
    assert_eq!((page_title.as_str(), addr.as_str()), ("Docs: the page?", "https://www.example.test/docs/page"));
    assert_eq!(fx.files().len(), 1);
    assert!(path.exists());
}

#[test]
fn a_capture_that_cannot_run_says_why_and_one_at_a_time() {
    let fx = Fixture::new("refuse");
    // Not a web page.
    fx.app.borrow_mut().tabs.iter_mut().find(|e| e.id == fx.tab).unwrap().url = Some(url::Url::parse("about:blank").unwrap());
    assert_eq!(fx.capture(CaptureMode::Visible, CaptureTarget::File), CaptureResult::Unavailable);
    fx.app.borrow_mut().tabs.iter_mut().find(|e| e.id == fx.tab).unwrap().url = Some(url::Url::parse("https://www.example.test/docs/page").unwrap());
    // The browser refuses or answers with something that is not a picture.
    assert!(matches!(fx.capture(CaptureMode::Visible, CaptureTarget::File), CaptureResult::Failed(_)), "no answer");
    fx.calls.borrow_mut().push_back(Some(pics::answer(b"<html>not a picture</html>")));
    assert!(matches!(fx.capture(CaptureMode::Visible, CaptureTarget::File), CaptureResult::Failed(_)), "not a picture");
    fx.calls.borrow_mut().push_back(Some("{\"cssContentSize\":{\"width\":0,\"height\":0}}".into()));
    assert!(matches!(fx.capture(CaptureMode::FullPage, CaptureTarget::File), CaptureResult::Failed(_)), "an empty page");
    assert!(fx.files().is_empty());
}

#[test]
fn a_second_capture_waits_for_the_first_and_a_navigation_ends_it_quietly() {
    let fx = Fixture::new("busy");
    // Hold the first capture open by answering its call later: swap in an engine that keeps `done`.
    type Held = Rc<RefCell<Option<Box<dyn FnOnce(Option<String>)>>>>;
    struct Holding(Held);
    impl EngineAdapter for Holding {
        fn navigate(&mut self, _u: &url::Url) -> windows::core::Result<()> { Ok(()) }
        fn can_go_back(&self) -> bool { false }
        fn go_back(&mut self) -> windows::core::Result<()> { Ok(()) }
        fn can_go_forward(&self) -> bool { false }
        fn go_forward(&mut self) -> windows::core::Result<()> { Ok(()) }
        fn resize(&mut self, _b: windows::Win32::Foundation::RECT) -> windows::core::Result<()> { Ok(()) }
        fn set_memory_usage_target(&mut self, _t: MemoryUsageTarget) -> windows::core::Result<MemoryTargetApply> { Ok(MemoryTargetApply::Applied) }
        fn close(&mut self) -> windows::core::Result<()> { Ok(()) }
        fn devtools_call(&mut self, _tab: TabId, _m: &str, _p: &str, done: Box<dyn FnOnce(Option<String>)>) { *self.0.borrow_mut() = Some(done); }
    }
    let held = Rc::new(RefCell::new(None));
    fx.app.borrow_mut().engine = Some(Box::new(Holding(held.clone())));
    let results = Rc::new(RefCell::new(Vec::new()));
    let sink = results.clone();
    App::capture_page(&fx.app, fx.tab, CaptureMode::Visible, CaptureTarget::File, Rc::new(move |r| sink.borrow_mut().push(r)));
    assert!(results.borrow().is_empty(), "waiting on the browser");
    assert_eq!(fx.capture(CaptureMode::Visible, CaptureTarget::File), CaptureResult::Busy);
    // The tab navigates before the picture arrives: the capture ends without saving anything.
    fx.app.borrow_mut().tabs.iter_mut().find(|e| e.id == fx.tab).unwrap().url = Some(url::Url::parse("https://other.example/").unwrap());
    let done = held.borrow_mut().take().unwrap();
    done(Some(pics::answer(&pics::picture(4, 4, CaptureFormat::Png, |_| 1))));
    assert!(matches!(results.borrow().as_slice(), [CaptureResult::Failed(_)]));
    assert!(fx.files().is_empty());
    assert!(!crate::app::capture_busy());
}

#[test]
fn the_selection_for_a_note_is_its_text_page_and_address() {
    let fx = Fixture::new("note");
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    fx.scripts.borrow_mut().push_back(Some(format!("{:?}", r#"{"c":1,"t":"A quote\nsecond line","x":1,"y":2,"w":3,"h":4,"a":"https://evil.example/claimed","n":"Claimed\n# title"}"#)));
    App::read_selection(&fx.app, fx.tab, Box::new(move |s| sink.borrow_mut().push(s)));
    fx.scripts.borrow_mut().push_back(Some(format!("{:?}", r#"{"c":0}"#)));
    let sink = seen.clone();
    App::read_selection(&fx.app, fx.tab, Box::new(move |s| sink.borrow_mut().push(s)));
    let sink = seen.clone();
    App::read_selection(&fx.app, fx.tab, Box::new(move |s| sink.borrow_mut().push(s))); // the engine has no answer
    assert_eq!(
        *seen.borrow(),
        vec![
            // The address and title are the tab's, not what the page script claimed.
            Selected::Text { text: "A quote\nsecond line".into(), title: "Docs: the page?".into(), addr: "https://www.example.test/docs/page".into() },
            Selected::Nothing,
            Selected::Unavailable
        ]
    );
}

#[test]
fn a_note_is_typed_into_the_focused_field_only_on_a_web_page_and_only_as_text() {
    let fx = Fixture::new("insert");
    assert!(App::insert_text(&fx.app, fx.tab, "line one\n\"quoted\" </script><b>x</b>"));
    let (origin, script) = fx.record.borrow().origin_scripts[0].clone();
    assert_eq!(origin, "https://www.example.test");
    assert!(script.contains("insertText") && script.contains("type==='password'"), "never into a password field");
    assert!(!script.contains("</script"), "the text cannot close the script: {script}");
    assert!(script.contains("\\u003c") || script.contains("\\u003C"), "{script}");
    fx.app.borrow_mut().tabs.iter_mut().find(|e| e.id == fx.tab).unwrap().url = Some(url::Url::parse("bowser://settings/").unwrap());
    assert!(!App::insert_text(&fx.app, fx.tab, "x"));
    assert_eq!(fx.record.borrow().origin_scripts.len(), 1);
}

#[test]
fn zoom_kept_per_site_is_stored_and_zoom_kept_per_tab_is_not() {
    let fx = Fixture::new("zoom");
    let store = Rc::new(RefCell::new(crate::site_prefs::SitePrefsStore::in_memory()));
    fx.app.borrow_mut().site_prefs = Some(store.clone());
    let site = url::Url::parse("https://www.example.test/docs/page").unwrap();
    // Site scope (the default): the zoom is the site's, kept for the next visit and for other tabs.
    assert!(App::set_tab_zoom(&fx.app, fx.tab, 150));
    assert_eq!(store.borrow().zoom_for_uri(site.as_str(), 100), 150);
    // Tab scope: the zoom stays with the tab; nothing is stored for the site.
    fx.tools(|t| t.zoom_scope = ZoomScope::Tab);
    store.borrow_mut().set_zoom("https://www.example.test", None).unwrap();
    assert!(App::set_tab_zoom(&fx.app, fx.tab, 200));
    assert_eq!(store.borrow().zoom_for_uri(site.as_str(), 100), 100, "the site's zoom was not touched");
    // The tab navigates to another site: it keeps its zoom and the engine is not asked to change it.
    fx.record.borrow_mut().zooms.clear();
    App::apply_stored_zoom(&fx.app, fx.tab, &url::Url::parse("https://other.example/").unwrap());
    assert!(fx.record.borrow().zooms.is_empty());
    assert_eq!(App::active_zoom_percent(&fx.app), 200);
    // Back to site scope, the next navigation lands on that site's own zoom.
    fx.tools(|t| t.zoom_scope = ZoomScope::Site);
    App::apply_stored_zoom(&fx.app, fx.tab, &url::Url::parse("https://other.example/").unwrap());
    assert_eq!(fx.record.borrow().zooms, vec![(fx.tab, 100)]);
}

#[test]
fn a_tab_the_user_has_not_zoomed_follows_the_default_zoom_in_tab_scope() {
    let fx = Fixture::new("zoom-default");
    let store = Rc::new(RefCell::new(crate::site_prefs::SitePrefsStore::in_memory()));
    store.borrow_mut().set_zoom("https://other.example", Some(300)).unwrap();
    fx.app.borrow_mut().site_prefs = Some(store);
    fx.app.borrow_mut().settings.default_zoom = 125;
    fx.tools(|t| t.zoom_scope = ZoomScope::Tab);
    App::apply_stored_zoom(&fx.app, fx.tab, &url::Url::parse("https://other.example/").unwrap());
    assert_eq!(fx.record.borrow().zooms, vec![(fx.tab, 125)], "the site's 300% is ignored in tab scope");
}

#[test]
fn applying_the_settings_reaches_the_engine_and_the_reader() {
    let fx = Fixture::new("apply");
    let mut settings = crate::settings::Settings::default();
    settings.page_tools.spatial_nav = true;
    settings.page_tools.wheel_zoom = false;
    settings.page_tools.reader_font = crate::page_tools::ReaderFont::Mono;
    settings.page_tools.reader_size = 24;
    settings.page_tools.reader_line = 20;
    settings.page_tools.reader_width = 50;
    settings.page_tools.reader_vertical = true;
    settings.page_tools.reader_icon = false;
    settings.page_tools.reader_time = true;
    settings.page_tools.reader_theme = crate::reader::Theme::Sepia;
    App::apply_page_tools_settings(&fx.app, &settings);
    assert_eq!(fx.record.borrow().applied, 1, "every open tab is told");
    assert!(crate::page_tools::spatial_on() && !crate::page_tools::wheel_zoom_on());
    let call = App::reader_invocation(&fx.app, crate::reader::Action::Toggle(crate::reader::Mode::Reader));
    for part in ["\"t\":\"sepia\",\"s\":24", "\"f\":\"mono\"", "\"l\":20", "\"w\":50", "\"v\":true", "\"g\":false", "\"r\":true"] {
        assert!(call.contains(part), "{part} in the reader call");
    }
    // Unchanged settings do not poke the engine again.
    App::apply_page_tools_settings(&fx.app, &settings);
    assert_eq!(fx.record.borrow().applied, 1);
    settings.page_tools.spatial_nav = false;
    settings.page_tools.wheel_zoom = true;
    App::apply_page_tools_settings(&fx.app, &settings);
    assert_eq!(fx.record.borrow().applied, 2);
    assert!(!crate::page_tools::spatial_on() && crate::page_tools::wheel_zoom_on());
}
