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
fn a_page_save_or_reset_of_layout_efficiency_and_sleep_is_not_pushed_over_a_newer_file_value() {
    use crate::settings::{EfficiencyMode, TabLayout};
    for reset in [false, true] {
        let app = app_with_pending(rect(800, 600));
        let path = temp_settings(if reset { "page-reset" } else { "page-save" });
        crate::settings::save(&path, &Settings::default()).unwrap();
        // The page saves (or resets to) these four values and applies them, as `settings_page::run` does.
        let page = if reset {
            {
                let mut state = app.borrow_mut();
                state.settings.tab_layout = TabLayout::Vertical;
                state.settings.sidebar_width = 300;
                state.settings.efficiency_mode = EfficiencyMode::On;
                state.settings.sleep_enabled = false;
            }
            // Memory and the file agree on these before Reset.
            App::save_settings_to(&app, &path).unwrap();
            crate::settings::reset_defaults(&Settings::load(&path))
        } else {
            page_writes(&path, |s| {
                s.tab_layout = TabLayout::Vertical;
                s.sidebar_width = 300;
                s.efficiency_mode = EfficiencyMode::On;
                s.sleep_enabled = false;
            })
        };
        crate::settings::save(&path, &page).unwrap();
        App::apply_page_settings(&app, &page);
        // Another window then changes all four in the file, to values neither the page nor memory held.
        let other = page_writes(&path, |s| {
            s.tab_layout = TabLayout::TabsBottom;
            s.sidebar_width = 220;
            s.efficiency_mode = EfficiencyMode::Off;
            s.sleep_enabled = !page.sleep_enabled;
        });
        // This window saves an unrelated value from memory: the other window's values stay.
        app.borrow_mut().settings.translate_offer = false;
        App::save_settings_to(&app, &path).unwrap();
        let saved = Settings::load(&path);
        assert_eq!(
            (saved.tab_layout, saved.sidebar_width, saved.efficiency_mode, saved.sleep_enabled),
            (other.tab_layout, other.sidebar_width, other.efficiency_mode, other.sleep_enabled),
            "reset={reset}"
        );
        assert!(!saved.translate_offer);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}

#[test]
fn a_settings_row_reports_a_failed_save_instead_of_saved() {
    let _profile = crate::profile::TestProfileGuard::isolated();
    let app = app_with_pending(rect(800, 600));
    // A folder where settings.txt belongs: every write fails.
    let path = crate::profile::active().unwrap().settings_file();
    std::fs::create_dir_all(&path).unwrap();
    let rows: [(&str, std::result::Result<(), String>); 8] = [
        ("tab option", App::set_tab_option(&app, "tabs_new_tab_position=related")),
        ("theme option", App::set_theme_option(&app, "theme_background=#102030")),
        ("theme reset", App::reset_theme(&app)),
        ("bar option", App::set_bar_option(&app, "feeds_detect=false")),
        ("translate offer", App::set_translate_offer(&app, false)),
        ("translate auto", App::set_translate_auto(&app, true)),
        ("translate target", App::set_translate_target(&app, "fr")),
        ("ask where to save", App::set_ask_download_location(&app, true)),
    ];
    for (row, result) in rows {
        let error = result.expect_err(row);
        assert!(error.starts_with("Settings could not be saved"), "{row}: {error}");
    }
    assert!(path.is_dir(), "nothing replaced the folder");
    // An invalid value still says so, and a good save still succeeds once the file can be written.
    assert_eq!(App::set_translate_target(&app, "not a language!").unwrap_err(), "That value is not allowed. Nothing was saved.");
    std::fs::remove_dir(&path).unwrap();
    assert_eq!(App::set_tab_option(&app, "tabs_new_tab_position=stack"), Ok(()));
    assert!(path.is_file());
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