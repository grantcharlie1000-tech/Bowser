//! The `startup` setting driving the first window (docs/design/start-page.md): the plan runs against a
//! real `App` with no engine, so the tabs and their addresses are what the user would see.

use super::*;
use crate::start_page::{StartSettings, Startup};

fn app_with(settings: StartSettings) -> AppRef {
    let app = app_with_pending(rect(800, 600));
    {
        let mut state = app.borrow_mut();
        state.pending = None;
        state.pending_tab = None;
        state.settings.start = settings;
    }
    app
}

fn urls(app: &AppRef) -> Vec<Option<String>> {
    let state = app.borrow();
    state.tab_model.iter().map(|tab| state.tabs.iter().find(|t| t.id == tab.id).and_then(|t| t.url.as_ref().map(url::Url::to_string))).collect()
}

fn active_url(app: &AppRef) -> Option<String> {
    let state = app.borrow();
    let active = state.tab_model.active();
    state.tabs.iter().find(|tab| tab.id == active).and_then(|tab| tab.url.as_ref().map(url::Url::to_string))
}

#[test]
fn the_start_page_mode_leaves_the_first_tab_blank_for_the_start_page() {
    let app = app_with(StartSettings::default());
    App::apply_startup_plan(&app, false);
    assert_eq!(urls(&app), [None]);
    assert!(App::tab_is_blank(&app, App::active_tab_id(&app)), "a blank tab shows the start page");
}

#[test]
fn the_homepage_mode_opens_the_homepage_in_the_first_tab() {
    let app = app_with(StartSettings { startup: Startup::Homepage, homepage: "https://home.example/".to_owned(), ..StartSettings::default() });
    App::apply_startup_plan(&app, false);
    assert_eq!(urls(&app), [Some("https://home.example/".to_owned())]);
}

#[test]
fn the_homepage_mode_without_a_homepage_shows_the_start_page() {
    let app = app_with(StartSettings { startup: Startup::Homepage, ..StartSettings::default() });
    App::apply_startup_plan(&app, false);
    assert_eq!(urls(&app), [None]);
}

#[test]
fn the_specific_pages_mode_opens_one_tab_each_and_keeps_the_first_shown() {
    let pages = vec!["https://a.example/".to_owned(), "https://b.example/".to_owned(), "https://c.example/".to_owned()];
    let app = app_with(StartSettings { startup: Startup::SpecificPages, startup_pages: pages.clone(), ..StartSettings::default() });
    App::apply_startup_plan(&app, false);
    let mut opened: Vec<String> = urls(&app).into_iter().flatten().collect();
    assert_eq!(opened.len(), 3, "{opened:?}");
    opened.sort();
    assert_eq!(opened, pages);
    assert_eq!(active_url(&app).as_deref(), Some("https://a.example/"), "the first page is the one shown");
}

#[test]
fn a_launch_that_already_has_a_target_is_left_alone() {
    for startup in [Startup::Homepage, Startup::SpecificPages] {
        let app = app_with(StartSettings {
            startup,
            homepage: "https://home.example/".to_owned(),
            startup_pages: vec!["https://a.example/".to_owned()],
            ..StartSettings::default()
        });
        App::apply_startup_plan(&app, true);
        assert_eq!(urls(&app), [None], "{startup:?}: a command-line address or a restored session decides");
    }
}

#[test]
fn the_launch_target_is_a_restored_session_a_command_line_address_or_a_benchmark_url() {
    let app = app_with(StartSettings::default());
    assert!(!App::launch_has_target(&app));
    app.borrow_mut().initial_url = Some(url::Url::parse("https://cli.example/").unwrap());
    assert!(App::launch_has_target(&app));
    app.borrow_mut().initial_url = None;
    app.borrow_mut().benchmark_url = Some(url::Url::parse("https://bench.example/").unwrap());
    assert!(App::launch_has_target(&app));
    app.borrow_mut().benchmark_url = None;
    app.borrow_mut().pending_restore = Some(crate::session::Session { tabs: Vec::new(), active: 0, split: None });
    assert!(App::launch_has_target(&app));
}
