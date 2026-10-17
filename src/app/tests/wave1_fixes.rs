//! Wave-1 fixes: saves from memory never revert newer file values, Reset reaches memory, the tab
//! selection without the active tab, and bulk closes honouring the pinned option.

use super::*;
use crate::settings::{BlockerLevel, Settings, TrackingLevel};
use crate::tab_options::PinnedClose;

fn temp_settings(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bowser-wave1-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("settings.txt")
}

/// The page writes a key straight to the file; the app then saves another from memory.
fn page_writes(path: &std::path::Path, change: impl FnOnce(&mut Settings)) -> Settings {
    let mut file = Settings::load(path);
    change(&mut file);
    crate::settings::save(path, &file).unwrap();
    file
}

#[test]
fn a_save_from_memory_never_reverts_a_value_the_page_saved_since() {
    let app = app_with_pending(rect(800, 600));
    let path = temp_settings("revert");
    crate::settings::save(&path, &Settings::default()).unwrap();

    // Tracking prevention changed on the page, then "Ask where to save" flipped: tracking stays.
    page_writes(&path, |s| s.tracking_prevention = TrackingLevel::Strict);
    app.borrow_mut().settings.ask_download_location = true;
    App::save_settings_to(&app, &path).unwrap();
    let saved = Settings::load(&path);
    assert_eq!(saved.tracking_prevention, TrackingLevel::Strict);
    assert!(saved.ask_download_location);

    // The blocking level and the theme from the page, then a translate option and a sidebar collapse.
    page_writes(&path, |s| {
        s.blocker_level = BlockerLevel::Trackers;
        s.theme = crate::settings::Theme::System;
    });
    app.borrow_mut().settings.translate_offer = false;
    App::save_settings_to(&app, &path).unwrap();
    app.borrow_mut().settings.sidebar_collapsed = true;
    App::save_settings_to(&app, &path).unwrap();
    let saved = Settings::load(&path);
    assert_eq!(saved.blocker_level, BlockerLevel::Trackers);
    assert_eq!(saved.theme, crate::settings::Theme::System);
    assert_eq!(saved.tracking_prevention, TrackingLevel::Strict, "an earlier page change survives later saves");
    assert!(!saved.translate_offer && saved.sidebar_collapsed && saved.ask_download_location);

    // The page turns "Ask" off again and applies it: memory follows, and a later save keeps it off.
    let file = page_writes(&path, |s| s.ask_download_location = false);
    App::apply_live_settings(&app, &file);
    assert!(!App::settings(&app).ask_download_location);
    app.borrow_mut().settings.sidebar_collapsed = false;
    App::save_settings_to(&app, &path).unwrap();
    assert!(!Settings::load(&path).ask_download_location);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn reset_settings_reaches_the_values_memory_holds() {
    let app = app_with_pending(rect(800, 600));
    {
        let mut state = app.borrow_mut();
        state.settings.download_dir = "D:\\Elsewhere".into();
        state.settings.ask_download_location = true;
        state.settings.translate_offer = false;
        state.settings.translate_auto = true;
        state.settings.translate_target = "fr".into();
        state.settings.tabs.keep_window = false;
    }
    let reset = crate::settings::reset_defaults(&App::settings(&app));
    App::apply_live_settings(&app, &reset);
    let now = App::settings(&app);
    let defaults = Settings::default();
    assert_eq!(now.download_dir, defaults.download_dir);
    assert_eq!(now.ask_download_location, defaults.ask_download_location);
    assert_eq!((now.translate_offer, now.translate_auto, now.translate_target.as_str()), (true, false, ""));
    assert_eq!(now.tabs, defaults.tabs);
}

#[test]
fn with_include_active_off_the_active_tab_is_never_selected_or_acted_on() {
    let app = app_with_pending(rect(800, 600));
    let first = App::active_tab_id(&app);
    let ids = [first, App::new_tab(&app), App::new_tab(&app), App::new_tab(&app)];
    let _ = App::select_tab(&app, first);
    app.borrow_mut().settings.tabs.selection_active = false;

    App::set_tab_selection(&app, &[ids[0], ids[1], ids[2]]);
    assert_eq!(App::selected_tabs(&app), [ids[1], ids[2]], "the active tab is dropped from the selection");
    assert_eq!(App::selection_targets(&app, ids[1]), [ids[1], ids[2]]);
    assert_eq!(App::selection_targets(&app, ids[0]), [ids[0]], "the active tab acts alone");

    // Selected while the option was on, then turned off: still left out.
    app.borrow_mut().settings.tabs.selection_active = true;
    App::set_tab_selection(&app, &[ids[0], ids[3]]);
    app.borrow_mut().settings.tabs.selection_active = false;
    assert!(App::selected_tabs(&app).is_empty(), "one tab left is not a selection");
    assert_eq!(App::selection_targets(&app, ids[3]), [ids[3]]);
}

#[test]
fn a_bulk_close_keeps_or_minimises_pinned_tabs_as_the_option_says() {
    let app = app_with_pending(rect(800, 600));
    let first = App::active_tab_id(&app);
    let ids = [first, App::new_tab(&app), App::new_tab(&app)];
    let _ = App::set_tab_pinned(&app, ids[0], true);
    let others = [ids[0], ids[1]];

    app.borrow_mut().settings.tabs.pinned_close = PinnedClose::Close;
    assert_eq!(App::bulk_close_split(&app, &others), (vec![ids[0], ids[1]], vec![]));
    app.borrow_mut().settings.tabs.pinned_close = PinnedClose::Keep;
    assert_eq!(App::bulk_close_split(&app, &others), (vec![ids[1]], vec![]));
    app.borrow_mut().settings.tabs.pinned_close = PinnedClose::Minimise;
    assert_eq!(App::bulk_close_split(&app, &others), (vec![ids[1]], vec![ids[0]]), "the pinned tab sleeps, it is not closed");
}

/// "Search for selected text in a background tab": the results open in a new tab and, with the
/// option on, the page being read stays in front.
#[test]
fn a_selection_search_opens_in_the_background_only_with_the_option_on() {
    for background in [true, false] {
        let app = app_with_pending(rect(800, 600));
        app.borrow_mut().settings.address.selection_background = background;
        let before = App::active_tab_id(&app);
        let count = App::tab_count(&app);
        App::context_search_run(&app, crate::engine::ContextSearchPick::Selection("rust lifetimes".into()));
        assert_eq!(App::tab_count(&app), count + 1, "a new tab for the results");
        assert_eq!(App::active_tab_id(&app) == before, background, "background {background}");
    }
}