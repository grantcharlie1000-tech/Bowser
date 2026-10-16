//! What the Tabs options, closed-tab history, periodic reload, sessions and the strip's selection do
//! to the running app (src/app/tab_features.rs), driven with a recording engine and no window.

use super::*;
use crate::tab_options::{CloseActivation, CloseKind, DuplicatePosition, MuteMode, NewTabPage, PageUrl, PinnedClose};
use std::collections::HashSet;

#[derive(Default)]
struct Shared {
    calls: Vec<String>,
    playing: HashSet<TabId>,
    muted: HashSet<TabId>,
}

struct FakeEngine {
    shared: Rc<RefCell<Shared>>,
}

impl EngineAdapter for FakeEngine {
    fn navigate(&mut self, url: &url::Url) -> windows::core::Result<()> {
        self.shared.borrow_mut().calls.push(format!("navigate:{url}"));
        Ok(())
    }
    fn navigate_tab(&mut self, tab: TabId, url: &url::Url) -> windows::core::Result<()> {
        self.shared.borrow_mut().calls.push(format!("navigate:{}:{url}", tab.0));
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
    fn reload_tab(&mut self, tab: TabId) -> windows::core::Result<()> {
        self.shared.borrow_mut().calls.push(format!("reload:{}", tab.0));
        Ok(())
    }
    fn tab_playing_audio(&self, tab: TabId) -> bool {
        self.shared.borrow().playing.contains(&tab)
    }
    fn tab_muted(&self, tab: TabId) -> bool {
        self.shared.borrow().muted.contains(&tab)
    }
    fn set_tab_muted(&mut self, tab: TabId, muted: bool) -> windows::core::Result<()> {
        let mut shared = self.shared.borrow_mut();
        if muted {
            shared.muted.insert(tab);
        } else {
            shared.muted.remove(&tab);
        }
        shared.calls.push(format!("mute:{}:{muted}", tab.0));
        Ok(())
    }
    fn suspend_tab(&mut self, tab: TabId) -> windows::core::Result<()> {
        self.shared.borrow_mut().calls.push(format!("suspend:{}", tab.0));
        Ok(())
    }
}

fn fixture() -> (AppRef, Rc<RefCell<Shared>>) {
    let app = app_with_pending(rect(800, 600));
    let shared = Rc::new(RefCell::new(Shared::default()));
    app.borrow_mut().engine = Some(Box::new(FakeEngine { shared: shared.clone() }));
    (app, shared)
}

/// A web page in tab `id`, as if it had loaded.
fn page(app: &AppRef, id: TabId, address: &str) {
    let mut state = app.borrow_mut();
    state.tab_model.set_url(id, Some(address));
    let tab = state.tabs.iter_mut().find(|tab| tab.id == id).expect("tab");
    tab.url = Some(url::Url::parse(address).expect("url"));
    tab.controller_created = true;
}

fn set(app: &AppRef, change: impl FnOnce(&mut crate::tab_options::TabOptions)) {
    change(&mut app.borrow_mut().settings.tabs);
}

/// `n` tabs in a row, the first active.
fn tabs(app: &AppRef, n: usize) -> Vec<TabId> {
    let first = App::active_tab_id(app);
    let mut ids = vec![first];
    for _ in 1..n {
        ids.push(App::new_tab(app));
    }
    let _ = App::select_tab(app, first);
    ids
}

fn calls(shared: &Rc<RefCell<Shared>>) -> Vec<String> {
    shared.borrow().calls.clone()
}

#[test]
fn a_new_tab_goes_where_the_position_option_says_in_the_strip() {
    let app = app_with_pending(rect(800, 600));
    let a = App::active_tab_id(&app);
    let b = App::new_tab(&app);
    let c = App::new_tab(&app);
    assert_eq!(App::tab_ids(&app), [a, b, c], "after the active tab each time");
    let _ = App::select_tab(&app, a);
    let d = App::new_tab(&app);
    assert_eq!(App::tab_ids(&app), [a, d, b, c], "right of the active tab, in the strip and not only in the model");

    app.borrow_mut().settings.new_tab_beside_active = false;
    let e = App::new_tab(&app);
    assert_eq!(App::tab_ids(&app).last(), Some(&e), "as last");

    // After related: a tab a page opened sits beside its opener and that opener's earlier children.
    app.borrow_mut().settings.new_tab_beside_active = true;
    set(&app, |tabs| tabs.new_tab_related = true);
    let _ = App::select_tab(&app, b);
    let first = App::spawn_tab(&app, false, Some(b));
    let _ = App::select_tab(&app, d);
    let second = App::spawn_tab(&app, false, Some(b));
    let order = App::tab_ids(&app);
    let at = |id| order.iter().position(|tab| *tab == id).expect("in strip");
    assert_eq!(at(first), at(b) + 1, "the first child is right of its opener");
    assert_eq!(at(second), at(first) + 1, "the next child follows the first, not the active tab");
}

#[test]
fn a_link_in_a_new_tab_can_load_in_the_background() {
    let (app, shared) = fixture();
    let a = App::active_tab_id(&app);
    page(&app, a, "https://a.example/");
    set(&app, |tabs| tabs.link_background = true);
    shared.borrow_mut().calls.clear();

    App::open_link_tab(&app, a, false, url::Url::parse("https://b.example/").unwrap());

    assert_eq!(App::active_tab_id(&app), a, "the page being read stays in front");
    let order = App::tab_ids(&app);
    assert_eq!(order.len(), 2);
    let child = order[1];
    assert!(calls(&shared).contains(&format!("navigate:{}:https://b.example/", child.0)), "{:?}", calls(&shared));
    assert!(!calls(&shared).contains(&format!("select:{}", child.0)));
    assert_eq!(App::tab_opener(&app, child), Some(a));
    assert!(app.borrow().tabs.iter().find(|tab| tab.id == child).unwrap().hidden_since.is_some());

    set(&app, |tabs| tabs.link_background = false);
    App::open_link_tab(&app, a, false, url::Url::parse("https://c.example/").unwrap());
    assert_ne!(App::active_tab_id(&app), a, "off: the new tab takes the view as before");
}

#[test]
fn the_new_tab_page_option_decides_what_a_fresh_tab_opens() {
    let (app, shared) = fixture();
    let before = App::tab_count(&app);
    let id = App::open_new_tab(&app);
    assert_eq!(App::tab_count(&app), before + 1);
    assert!(App::tab_is_blank(&app, id), "the start page: the tab stays empty");

    set(&app, |tabs| {
        tabs.new_tab_page = NewTabPage::Url;
        tabs.new_tab_url = PageUrl("https://start.example/home".into());
    });
    shared.borrow_mut().calls.clear();
    let id = App::open_new_tab(&app);
    assert!(!App::tab_is_blank(&app, id));
    assert!(calls(&shared).iter().any(|call| call.ends_with("https://start.example/home")), "{:?}", calls(&shared));
    assert_eq!(App::tab_committed_url(&app, id).as_ref().map(url::Url::as_str), Some("https://start.example/home"));
}

/// Integration: a later save from memory keeps the keys the page wrote straight to the file
/// (grouping, tracking, YouTube ads, side panel, frameless, legacy engine) instead of reverting them.
#[test]
fn a_save_from_memory_keeps_the_keys_only_the_file_holds() {
    let defaults = crate::settings::Settings::default();
    let file = crate::settings::Settings {
        automatic_tab_grouping: !defaults.automatic_tab_grouping,
        block_youtube_video_ads: !defaults.block_youtube_video_ads,
        frameless: !defaults.frameless,
        tracking_prevention: crate::settings::TrackingLevel::Strict,
        ..crate::settings::Settings::default()
    };
    // What memory owns stays.
    let mut memory = crate::settings::Settings { sidebar_width: defaults.sidebar_width + 20, ..defaults };
    let width = memory.sidebar_width;
    crate::app::keep_file_owned_keys(&mut memory, &file);
    assert_eq!(memory.automatic_tab_grouping, file.automatic_tab_grouping);
    assert_eq!(memory.block_youtube_video_ads, file.block_youtube_video_ads);
    assert_eq!(memory.frameless, file.frameless);
    assert_eq!(memory.tracking_prevention, crate::settings::TrackingLevel::Strict);
    assert_eq!(memory.sidebar_width, width);
}

/// Integration: Reset settings (and any whole-file apply) puts the Tabs options into the running app.
#[test]
fn applying_the_file_settings_applies_the_tabs_options_live() {
    let (app, _shared) = fixture();
    set(&app, |tabs| tabs.keep_window = false);
    let defaults = crate::settings::Settings::default();
    App::apply_live_settings(&app, &defaults);
    assert!(App::tab_options(&app).keep_window, "the reset value is live");
}

/// Integration (T2 engine editor, T3 private window): a search typed in a private tab uses the
/// engine chosen "for private windows"; a normal tab keeps the default.
#[test]
fn a_private_tab_searches_with_the_private_window_engine() {
    let mut engines = crate::search_engines::EngineStore::default();
    engines.set_private("duckduckgo").unwrap();
    engines.set_default("bing").unwrap();
    crate::address_runtime::set_test_engines(Some(engines));
    let (app, shared) = fixture();
    App::new_private_tab(&app);
    assert!(App::active_tab_private(&app));
    App::submit(&app, "rust ownership");
    let searched = calls(&shared);
    assert!(searched.iter().any(|call| call.contains("duckduckgo.com") && call.contains("rust")), "{searched:?}");
    shared.borrow_mut().calls.clear();
    let _ = App::new_tab(&app);
    assert!(!App::active_tab_private(&app));
    App::submit(&app, "rust ownership");
    let searched = calls(&shared);
    assert!(searched.iter().any(|call| call.contains("bing.com")), "{searched:?}");
    assert!(!searched.iter().any(|call| call.contains("duckduckgo.com")));
    crate::address_runtime::set_test_engines(None);
}

/// Integration: Tabs' "new tab page" reads T5's `homepage` key (one key set) and gates T5's start page.
#[test]
fn the_new_tab_page_homepage_and_blank_choices_use_the_start_page_settings() {
    let (app, shared) = fixture();
    // Homepage with none set: the tab stays empty and the start page shows over it.
    set(&app, |tabs| tabs.new_tab_page = NewTabPage::Homepage);
    let id = App::open_new_tab(&app);
    assert!(App::tab_is_blank(&app, id));
    assert!(crate::win32::start_ui::enabled(&app));
    // Homepage set in Settings > Start page: a new tab opens it.
    app.borrow_mut().settings.start.homepage = "https://home.example/".to_owned();
    shared.borrow_mut().calls.clear();
    let id = App::open_new_tab(&app);
    assert!(!App::tab_is_blank(&app, id));
    assert_eq!(App::tab_committed_url(&app, id).as_ref().map(url::Url::as_str), Some("https://home.example/"));
    // Blank page: nothing loads and the start page is not drawn over the empty tab.
    set(&app, |tabs| tabs.new_tab_page = NewTabPage::Blank);
    let id = App::open_new_tab(&app);
    assert!(App::tab_is_blank(&app, id));
    assert!(!crate::win32::start_ui::enabled(&app), "blank turns the start page off");
    set(&app, |tabs| tabs.new_tab_page = NewTabPage::Start);
    assert!(crate::win32::start_ui::enabled(&app));
}

#[test]
fn closing_the_active_tab_activates_the_tab_the_option_names() {
    for (activation, expected_index) in [(CloseActivation::Right, 3), (CloseActivation::Left, 1)] {
        let app = app_with_pending(rect(800, 600));
        let ids = tabs(&app, 5);
        set(&app, |tabs| tabs.close_activation = activation);
        let _ = App::select_tab(&app, ids[2]);
        assert!(App::close_tab(&app, ids[2]));
        assert_eq!(App::active_tab_id(&app), ids[expected_index], "{activation:?}");
    }

    // Recently used: the tab before this one, whatever its place.
    let app = app_with_pending(rect(800, 600));
    let ids = tabs(&app, 5);
    set(&app, |tabs| tabs.close_activation = CloseActivation::Recent);
    let _ = App::select_tab(&app, ids[4]);
    let _ = App::select_tab(&app, ids[1]);
    assert!(App::close_tab(&app, ids[1]));
    assert_eq!(App::active_tab_id(&app), ids[4]);

    // Related: back to the tab that opened it, even when the other rules would pick a neighbour.
    let app = app_with_pending(rect(800, 600));
    let ids = tabs(&app, 4);
    set(&app, |tabs| {
        tabs.close_related = true;
        tabs.close_activation = CloseActivation::Right;
    });
    let _ = App::select_tab(&app, ids[3]);
    let child = App::spawn_tab(&app, false, Some(ids[0]));
    assert_ne!(App::tab_ids(&app).iter().position(|tab| *tab == child), Some(1), "setup: a child that is not next to its opener");
    assert!(App::close_tab(&app, child));
    assert_eq!(App::active_tab_id(&app), ids[0]);
    set(&app, |tabs| tabs.close_related = false);
    let child = App::spawn_tab(&app, false, Some(ids[0]));
    let neighbour = {
        let order = App::tab_ids(&app);
        let at = order.iter().position(|tab| *tab == child).unwrap();
        order[(at + 1) % order.len()]
    };
    assert!(App::close_tab(&app, child));
    assert_eq!(App::active_tab_id(&app), neighbour, "off: the right neighbour as the activation option says");
}

#[test]
fn a_user_close_follows_the_pinned_and_last_tab_options() {
    let app = app_with_pending(rect(800, 600));
    let ids = tabs(&app, 3);
    assert_eq!(App::user_close_tab(&app, ids[1]), CloseKind::Close);
    assert_eq!(App::tab_count(&app), 2);

    set(&app, |tabs| tabs.pinned_close = PinnedClose::Keep);
    let _ = App::set_tab_pinned(&app, ids[2], true);
    assert_eq!(App::user_close_tab(&app, ids[2]), CloseKind::Keep);
    assert_eq!(App::tab_count(&app), 2, "a pinned tab that is set not to close stays");
    set(&app, |tabs| tabs.pinned_close = PinnedClose::Close);
    assert_eq!(App::user_close_tab(&app, ids[2]), CloseKind::Close, "as other tabs again");

    // The last tab: the window stays (and gets a fresh tab) unless the option says it goes too.
    let only = App::active_tab_id(&app);
    set(&app, |tabs| tabs.keep_window = false);
    assert_eq!(App::tab_count(&app), 1);
    assert_eq!(App::user_close_tab(&app, only), CloseKind::CloseWindow);
    assert_eq!(App::tab_count(&app), 1, "the window closes the tab itself");
    set(&app, |tabs| tabs.keep_window = true);
    assert_eq!(App::user_close_tab(&app, only), CloseKind::Close);
    assert_ne!(App::active_tab_id(&app), only, "replaced by a fresh tab");
}

#[test]
fn minimising_a_pinned_tab_moves_on_and_puts_it_to_sleep() {
    let (app, shared) = fixture();
    let ids = tabs(&app, 3);
    for (n, id) in ids.iter().enumerate() {
        page(&app, *id, &format!("https://{n}.example/"));
    }
    let _ = App::set_tab_pinned(&app, ids[0], true);
    set(&app, |tabs| tabs.pinned_close = PinnedClose::Minimise);
    let _ = App::select_tab(&app, ids[1]);
    let _ = App::select_tab(&app, ids[0]);
    shared.borrow_mut().calls.clear();

    assert_eq!(App::user_close_tab(&app, ids[0]), CloseKind::Minimise);

    assert_eq!(App::active_tab_id(&app), ids[1], "back to the tab used before");
    assert_eq!(App::tab_count(&app), 3, "nothing closed");
    assert!(calls(&shared).contains(&format!("suspend:{}", ids[0].0)), "{:?}", calls(&shared));
    assert!(app.borrow().tabs.iter().find(|tab| tab.id == ids[0]).unwrap().extra.hibernated);
}

#[test]
fn closing_many_tabs_asks_only_from_the_threshold_up() {
    let app = app_with_pending(rect(800, 600));
    assert!(!App::close_needs_confirmation(&app, 50), "never by default");
    set(&app, |tabs| tabs.confirm_close = crate::tab_options::Bounded(3));
    assert!(!App::close_needs_confirmation(&app, 2));
    assert!(App::close_needs_confirmation(&app, 3));
}

#[test]
fn minimise_active_goes_back_to_the_last_used_tab() {
    let app = app_with_pending(rect(800, 600));
    let ids = tabs(&app, 3);
    let _ = App::select_tab(&app, ids[2]);
    let _ = App::select_tab(&app, ids[0]);
    assert_eq!(App::minimise_active_tab(&app), ids[2]);
    assert_eq!(App::active_tab_id(&app), ids[2]);
}

#[test]
fn the_audio_rule_mutes_background_tabs_without_touching_the_users_mute() {
    let (app, shared) = fixture();
    let ids = tabs(&app, 3);
    let (a, b, c) = (ids[0], ids[1], ids[2]);
    let muted = |id: TabId| shared.borrow().muted.contains(&id);

    set(&app, |tabs| tabs.mute_mode = MuteMode::Active);
    App::apply_mute_mode(&app);
    assert!(!muted(a) && muted(b) && muted(c), "only the active tab plays");
    assert!(!App::tab_muted(&app, b), "the rule's mute is not the user's: the menu still offers Mute");
    assert!(App::tab_auto_muted(&app, b));

    let _ = App::select_tab(&app, b);
    assert!(muted(a) && !muted(b) && muted(c), "the rule follows the active tab");

    // A tab the user muted stays muted when the rule lets everything play.
    App::set_tab_muted(&app, c, true);
    set(&app, |tabs| tabs.mute_mode = MuteMode::All);
    App::apply_mute_mode(&app);
    assert!(!muted(a) && !muted(b));
    assert!(muted(c), "the user's own mute survives");
    assert!(App::tab_muted(&app, c));
}

#[test]
fn prioritise_mutes_the_others_only_while_the_active_tab_plays() {
    let (app, shared) = fixture();
    let ids = tabs(&app, 3);
    set(&app, |tabs| tabs.mute_mode = MuteMode::Prioritise);
    assert!(app.borrow().tick_wanted(), "the rule needs the one-second tick to notice the active tab starting or stopping");
    let muted = |id: TabId| shared.borrow().muted.contains(&id);

    App::tab_tick_at(&app, Instant::now());
    assert!(!muted(ids[1]) && !muted(ids[2]), "the active tab is silent: the others may play");

    shared.borrow_mut().playing.insert(ids[0]);
    App::tab_tick_at(&app, Instant::now());
    assert!(!muted(ids[0]) && muted(ids[1]) && muted(ids[2]), "the active tab plays: the others wait");

    shared.borrow_mut().playing.clear();
    App::tab_tick_at(&app, Instant::now());
    assert!(!muted(ids[1]) && !muted(ids[2]));

    set(&app, |tabs| tabs.mute_mode = MuteMode::All);
    assert!(!app.borrow().tick_wanted(), "the tick stops when nothing needs it");
}

#[test]
fn a_periodic_reload_fires_on_its_interval_for_its_tab_only() {
    let (app, shared) = fixture();
    let ids = tabs(&app, 3);
    for (n, id) in ids.iter().enumerate() {
        page(&app, *id, &format!("https://{n}.example/"));
    }
    assert!(!app.borrow().tick_wanted());
    assert!(!App::set_tab_reload(&app, ids[1], Some(3)), "too short");
    assert!(!App::set_tab_reload(&app, TabId(999), Some(30)), "not a tab");
    assert!(App::set_tab_reload(&app, ids[1], Some(30)));
    assert_eq!(App::tab_reload_secs(&app, ids[1]), Some(30));
    assert_eq!(App::tab_reload_secs(&app, ids[0]), None);
    assert!(app.borrow().tick_wanted(), "a timer needs the tick");
    assert_eq!(App::reloading_tab_count(&app), 1);

    shared.borrow_mut().calls.clear();
    let start = Instant::now();
    App::tab_tick_at(&app, start + Duration::from_secs(10));
    assert!(calls(&shared).is_empty(), "not yet");
    App::tab_tick_at(&app, start + Duration::from_secs(31));
    assert_eq!(calls(&shared), [format!("reload:{}", ids[1].0)], "only the tab that asked, in the background too");
    App::tab_tick_at(&app, start + Duration::from_secs(40));
    assert_eq!(calls(&shared).len(), 1, "the next one is a full interval later");
    App::tab_tick_at(&app, start + Duration::from_secs(62));
    assert_eq!(calls(&shared).len(), 2);

    // A second tab, then "disable all".
    assert!(App::set_tab_reload(&app, ids[2], Some(60)));
    assert_eq!(App::reloading_tab_count(&app), 2);
    assert_eq!(App::disable_all_reloads(&app), 2);
    assert_eq!(App::reloading_tab_count(&app), 0);
    assert!(!app.borrow().tick_wanted(), "the tick stops with the last timer");
    shared.borrow_mut().calls.clear();
    App::tab_tick_at(&app, start + Duration::from_secs(600));
    assert!(calls(&shared).is_empty());

    // Disable one.
    assert!(App::set_tab_reload(&app, ids[1], Some(30)));
    assert!(App::set_tab_reload(&app, ids[1], None));
    assert_eq!(App::tab_reload_secs(&app, ids[1]), None);
}

#[test]
fn a_hibernated_tab_is_not_reloaded_into_memory_by_its_timer() {
    let (app, shared) = fixture();
    let ids = tabs(&app, 2);
    page(&app, ids[1], "https://b.example/");
    assert!(App::set_tab_reload(&app, ids[1], Some(5)));
    app.borrow_mut().tabs.iter_mut().find(|tab| tab.id == ids[1]).unwrap().discarded = true;
    shared.borrow_mut().calls.clear();
    App::tab_tick_at(&app, Instant::now() + Duration::from_secs(6));
    assert!(calls(&shared).is_empty(), "a discarded tab has no page to reload");
    assert_eq!(App::tab_reload_secs(&app, ids[1]), Some(5), "the timer keeps running");
}

#[test]
fn unread_marks_come_from_background_loads_and_growing_counts_and_clear_on_select() {
    let app = app_with_pending(rect(800, 600));
    let ids = tabs(&app, 2);
    {
        let mut state = app.borrow_mut();
        let tab = state.tabs.iter_mut().find(|tab| tab.id == ids[1]).unwrap();
        tab_features::note_title(tab, "(3) Inbox", true, false);
        assert_eq!(tab.extra.notify, 3);
        assert!(tab.extra.unread);
    }
    let extras = App::tab_strip_extras(&app, &ids);
    assert_eq!((extras[1].unread, extras[1].notify), (true, 3));
    assert_eq!((extras[0].unread, extras[0].notify), (false, 0));

    // The options turn the marks off in what the strip is told.
    set(&app, |tabs| {
        tabs.unread_indicator = false;
        tabs.title_notifications = false;
    });
    let extras = App::tab_strip_extras(&app, &ids);
    assert_eq!((extras[1].unread, extras[1].notify), (false, 0));
    set(&app, |tabs| {
        tabs.unread_indicator = true;
        tabs.title_notifications = true;
    });

    let _ = App::select_tab(&app, ids[1]);
    assert!(!App::tab_strip_extras(&app, &ids)[1].unread, "opening the tab reads it");
    assert_eq!(App::tab_strip_extras(&app, &ids)[1].notify, 3, "the count is the page's, so it stays");

    // With detection off a title carries no count.
    let mut state = app.borrow_mut();
    let tab = state.tabs.iter_mut().find(|tab| tab.id == ids[0]).unwrap();
    tab_features::note_title(tab, "(7) Chat", false, false);
    assert_eq!(tab.extra.notify, 0);
    assert!(!tab.extra.unread);
}

#[test]
fn the_strip_dims_hibernated_tabs_and_marks_idle_ones_only_when_asked() {
    let app = app_with_pending(rect(800, 600));
    let ids = tabs(&app, 3);
    {
        let mut state = app.borrow_mut();
        state.tabs.iter_mut().find(|tab| tab.id == ids[1]).unwrap().extra.hibernated = true;
        state.tabs.iter_mut().find(|tab| tab.id == ids[2]).unwrap().extra.last_used = crate::tab_state::unix_now() - 3 * 86_400;
    }
    let extras = App::tab_strip_extras(&app, &ids);
    assert!(extras[1].asleep, "dimmed by default");
    assert!(!extras[2].idle, "the threshold is off by default");
    set(&app, |tabs| {
        tabs.dim_hibernated = false;
        tabs.inactive_days = crate::tab_options::Bounded(2);
    });
    let extras = App::tab_strip_extras(&app, &ids);
    assert!(!extras[1].asleep);
    assert!(extras[2].idle, "unused for longer than the threshold");
    assert!(!extras[0].idle, "the active tab is never idle");
}

#[test]
fn ctrl_shift_t_brings_back_the_closed_tab_where_it_was() {
    let (app, shared) = fixture();
    let ids = tabs(&app, 4);
    for (n, id) in ids.iter().enumerate() {
        page(&app, *id, &format!("https://{n}.example/"));
    }
    let _ = App::select_tab(&app, ids[1]);
    assert!(App::close_tab(&app, ids[1]));
    assert!(App::close_tab(&app, ids[2]));
    let closed = App::closed_tabs(&app);
    assert_eq!(closed.iter().map(|tab| tab.url.as_str()).collect::<Vec<_>>(), ["https://2.example/", "https://1.example/"], "newest first");

    shared.borrow_mut().calls.clear();
    let first = App::reopen_closed_tab(&app).expect("a closed tab");
    assert_eq!(App::tab_committed_url(&app, first).as_ref().map(url::Url::as_str), Some("https://2.example/"));
    assert!(calls(&shared).iter().any(|call| call.ends_with("https://2.example/")));
    assert_eq!(App::closed_tabs(&app).len(), 1, "a reopened tab leaves the list");
    assert_eq!(App::active_tab_id(&app), first);

    // A second press brings back the one before; a third has nothing.
    let second = App::reopen_closed_tab(&app).expect("another");
    assert_eq!(App::tab_committed_url(&app, second).as_ref().map(url::Url::as_str), Some("https://1.example/"));
    assert!(App::reopen_closed_tab(&app).is_none());
    assert_eq!(App::tab_count(&app), 4);
}

#[test]
fn a_closed_pinned_tab_comes_back_pinned_and_a_picked_entry_leaves_the_rest() {
    let app = app_with_pending(rect(800, 600));
    let ids = tabs(&app, 3);
    for (n, id) in ids.iter().enumerate() {
        page(&app, *id, &format!("https://{n}.example/"));
    }
    let _ = App::set_tab_pinned(&app, ids[0], true);
    assert!(App::close_tab(&app, ids[0]));
    assert!(App::close_tab(&app, ids[2]));
    let again = App::reopen_closed_at(&app, 1).expect("the pinned one");
    assert!(App::tab_pinned(&app, again));
    assert_eq!(App::tab_ids(&app).first(), Some(&again), "pinned tabs lead the strip");
    assert_eq!(App::closed_tabs(&app).len(), 1);
    assert_eq!(App::closed_tabs(&app)[0].url, "https://2.example/");
    assert!(App::reopen_closed_at(&app, 5).is_none(), "an index past the list reopens nothing");
}

#[test]
fn private_tabs_are_never_offered_for_reopening() {
    let app = app_with_pending(rect(800, 600));
    let private = App::new_private_tab(&app);
    page(&app, private, "https://secret.example/");
    assert!(App::close_tab(&app, private));
    assert!(App::closed_tabs(&app).is_empty());
}

#[test]
fn the_closed_list_keeps_the_last_twenty_five() {
    let app = app_with_pending(rect(800, 600));
    for n in 0..30 {
        let id = App::new_tab(&app);
        page(&app, id, &format!("https://{n}.example/"));
        assert!(App::close_tab(&app, id));
    }
    let closed = App::closed_tabs(&app);
    assert_eq!(closed.len(), 25);
    assert_eq!(closed[0].url, "https://29.example/");
}

#[test]
fn a_session_holds_the_web_pages_of_the_tabs_asked_for() {
    let app = app_with_pending(rect(800, 600));
    let ids = tabs(&app, 4);
    page(&app, ids[0], "https://a.example/");
    page(&app, ids[1], "https://b.example/");
    page(&app, ids[3], "https://d.example/");
    let private = App::new_private_tab(&app);
    page(&app, private, "https://secret.example/");
    app.borrow_mut().tab_model.select(ids[1]);

    let all = App::session_of_all(&app, "  Work  ").expect("pages to save");
    assert_eq!(all.name, "Work");
    assert_eq!(all.urls, ["https://a.example/", "https://b.example/", "https://d.example/"], "no empty tab, no private tab");
    assert_eq!(all.active, 1);

    let some = App::session_of(&app, "Two", &[ids[3], ids[0]]).expect("pages");
    assert_eq!(some.urls, ["https://d.example/", "https://a.example/"], "in the order given");
    assert_eq!(some.active, 0, "the active tab is not among them: the first");

    assert!(App::session_of(&app, "Empty", &[ids[2]]).is_none(), "an empty tab has nothing to save");
    assert!(App::session_of(&app, "   ", &ids).is_none(), "a name is required");
    assert!(App::session_of(&app, "Private", &[private]).is_none());
    assert_eq!(App::session_of(&app, &"x".repeat(300), &ids).unwrap().name.chars().count(), crate::app::MAX_SESSION_NAME_CHARS);
}

#[test]
fn opening_a_session_adds_its_tabs_in_order_and_only_the_active_one_loads() {
    let (app, shared) = fixture();
    let first = App::active_tab_id(&app);
    let session = crate::productivity::SavedSession {
        name: "Work".into(),
        urls: vec!["https://a.example/".into(), "https://b.example/".into(), "https://c.example/".into()],
        active: 1,
    };
    shared.borrow_mut().calls.clear();
    assert_eq!(App::open_session(&app, &session, false), 3);

    let order = App::tab_ids(&app);
    assert_eq!(order.len(), 4);
    assert_eq!(order[0], first, "the open tab stays");
    let urls: Vec<_> = order[1..].iter().map(|id| App::tab_committed_url(&app, *id).unwrap().to_string()).collect();
    assert_eq!(urls, ["https://a.example/", "https://b.example/", "https://c.example/"]);
    assert_eq!(App::active_tab_id(&app), order[2], "the session's active page is shown");
    let asleep: Vec<bool> = order[1..].iter().map(|id| app.borrow().tabs.iter().find(|tab| tab.id == *id).unwrap().discarded).collect();
    assert_eq!(asleep, [true, false, true], "the others load when selected");
    let navigations: Vec<_> = calls(&shared).into_iter().filter(|call| call.starts_with("navigate")).collect();
    assert_eq!(navigations.len(), 1, "{navigations:?}");

    // Replace: the open tabs go once the session's are in.
    let before = App::tab_ids(&app);
    assert_eq!(App::open_session(&app, &session, true), 3);
    let after = App::tab_ids(&app);
    assert_eq!(after.len(), 3);
    assert!(after.iter().all(|id| !before.contains(id)), "every earlier tab was replaced");
}

#[test]
fn the_selection_is_what_a_tab_action_applies_to() {
    let app = app_with_pending(rect(800, 600));
    let ids = tabs(&app, 4);
    App::set_tab_selection(&app, &[ids[3], ids[1], TabId(999)]);
    assert_eq!(App::selection_targets(&app, ids[1]), [ids[1], ids[3], ids[0]], "strip order, then the active tab (included by default)");
    assert_eq!(App::selection_targets(&app, ids[2]), [ids[2]], "a tab outside the selection acts alone");
    assert_eq!(App::selected_tabs(&app), [ids[0], ids[1], ids[3]]);

    set(&app, |tabs| tabs.selection_active = false);
    assert_eq!(App::selection_targets(&app, ids[3]), [ids[1], ids[3]]);

    set(&app, |tabs| tabs.selection = false);
    App::set_tab_selection(&app, &[ids[1], ids[2]]);
    assert_eq!(App::selection_targets(&app, ids[1]), [ids[1]], "selection off: nothing is selected");
    assert!(App::selected_tabs(&app).is_empty());
}

#[test]
fn a_name_the_user_gave_a_tab_is_kept_trimmed_and_cleared_with_an_empty_name() {
    let app = app_with_pending(rect(800, 600));
    let id = App::active_tab_id(&app);
    assert!(App::set_tab_name(&app, id, "  Mail  "));
    assert_eq!(App::tab_name(&app, id).as_deref(), Some("Mail"));
    assert!(App::set_tab_name(&app, id, "   "));
    assert_eq!(App::tab_name(&app, id), None);
    assert!(!App::set_tab_name(&app, TabId(999), "Nope"));
    assert!(App::set_tab_name(&app, id, &"é".repeat(300)));
    assert_eq!(App::tab_name(&app, id).unwrap().chars().count(), crate::app::MAX_TAB_NAME_CHARS);
}

#[test]
fn restored_tabs_load_lazily_unless_the_options_say_otherwise() {
    fn restore(lazy: bool, pinned_first: bool) -> (Vec<bool>, Vec<TabId>) {
        let app = app_with_pending(rect(800, 600));
        set(&app, |tabs| {
            tabs.lazy_restore = lazy;
            tabs.load_pinned = pinned_first;
        });
        let session = crate::session::Session {
            tabs: vec![
                crate::session::SessionTab { url: "https://pinned.example/".into(), title: "P".into(), pinned: true, used: 0 },
                crate::session::SessionTab { url: "https://active.example/".into(), title: "A".into(), pinned: false, used: 0 },
                crate::session::SessionTab { url: "https://other.example/".into(), title: "O".into(), pinned: false, used: 0 },
            ],
            active: 1,
            split: None,
        };
        app.borrow_mut().pending_restore = Some(session);
        App::restore_pending_session(&app);
        let state = app.borrow();
        let asleep = state.tab_model.iter().map(|model| state.tabs.iter().find(|tab| tab.id == model.id).unwrap().discarded).collect();
        (asleep, state.tab_state.eager.clone())
    }
    let (asleep, eager) = restore(true, false);
    assert_eq!(asleep, [true, false, true], "lazy by default: only the active tab loads");
    assert!(eager.is_empty());
    let (_, eager) = restore(true, true);
    assert_eq!(eager.len(), 1, "always load pinned: the pinned tab loads at once");
    let (_, eager) = restore(false, false);
    assert_eq!(eager.len(), 2, "lazy loading off: every tab but the active one loads at once");
}

#[test]
fn eager_restored_tabs_load_once_the_engine_can() {
    let (app, shared) = fixture();
    let ids = tabs(&app, 2);
    page(&app, ids[1], "https://later.example/");
    {
        let mut state = app.borrow_mut();
        state.tabs.iter_mut().find(|tab| tab.id == ids[1]).unwrap().discarded = true;
        state.tab_state.eager.push(ids[1]);
    }
    shared.borrow_mut().calls.clear();
    App::load_eager_tabs(&app);
    assert!(calls(&shared).contains(&format!("navigate:{}:https://later.example/", ids[1].0)), "{:?}", calls(&shared));
    assert!(!app.borrow().tabs.iter().find(|tab| tab.id == ids[1]).unwrap().discarded);
    assert!(app.borrow().tab_state.eager.is_empty());
}

#[test]
fn a_changed_option_is_saved_in_the_app_and_refused_when_wrong() {
    let app = app_with_pending(rect(800, 600));
    assert!(App::tab_options(&app).keep_window);
    // `set_tab_option` also saves to the profile; here only the in-memory half is observable, and a
    // refused value changes nothing.
    assert!(!App::set_tab_option(&app, "tabs_confirm_close=500"));
    assert_eq!(App::tab_options(&app).confirm_close.0, 0);
    assert!(!App::set_tab_option(&app, "tabs_nonsense=true"));
    let _ = DuplicatePosition::Last;
}

#[test]
fn a_pinned_tab_sends_links_to_other_sites_to_a_new_tab_when_asked() {
    let (app, _shared) = fixture();
    let ids = tabs(&app, 2);
    let pinned = ids[0];
    page(&app, pinned, "https://www.example.com/start");
    let _ = App::set_tab_pinned(&app, pinned, true);
    let elsewhere = url::Url::parse("https://other.test/page").unwrap();
    let same_site = url::Url::parse("https://news.example.com/a").unwrap();

    // Off by default: nothing is refused.
    assert!(!App::pinned_link(&app, pinned, &elsewhere));
    set(&app, |tabs| tabs.pinned_restrict = true);
    assert!(!App::pinned_link(&app, pinned, &same_site), "the same site loads in place");
    assert!(!App::pinned_link(&app, ids[1], &elsewhere), "an unpinned tab is not restricted");
    assert!(App::pinned_link(&app, pinned, &elsewhere), "another site is refused");
    assert_eq!(app.borrow().tab_state.deferred_links.len(), 1);

    let before = App::tab_count(&app);
    App::run_deferred_links(&app);
    assert_eq!(App::tab_count(&app), before + 1, "and opens in a tab of its own");
    assert!(app.borrow().tab_state.deferred_links.is_empty());

    // Unpinned, the tab is free again.
    let _ = App::set_tab_pinned(&app, pinned, false);
    assert!(!App::pinned_link(&app, pinned, &elsewhere));
}
