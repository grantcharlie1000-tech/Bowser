//! Private tabs and history deletion, driven through the app with no window.

use super::*;

#[test]
fn a_link_opened_in_split_view_from_a_private_tab_stays_private() {
    let app = app_with_pending(rect(800, 600));
    let source = App::new_private_tab(&app);
    let before = App::tab_ids(&app);
    let url = url::Url::parse("https://example.com/secret?token=1").unwrap();
    assert!(App::open_url_in_split(&app, source, &url, None));
    let side = App::tab_ids(&app).into_iter().find(|id| !before.contains(id)).expect("split view made a new side tab");
    assert!(App::tab_private(&app, side), "the link from a private page opened in a normal tab");
}

#[test]
fn split_with_a_new_tab_from_a_private_tab_gives_a_private_side_and_a_normal_one_from_a_normal_tab() {
    let app = app_with_pending(rect(800, 600));
    let private = App::new_private_tab(&app);
    let side = App::open_split_with_empty(&app, private, Pane::Right, None).expect("split view opened");
    assert!(App::tab_private(&app, side), "the empty side of a private tab is private");
    let normal = App::new_tab(&app);
    let side = App::open_split_with_empty(&app, normal, Pane::Left, None).expect("split view opened");
    assert!(!App::tab_private(&app, side), "a normal tab's side stays normal");
}

#[test]
fn deleting_history_forgets_the_recently_closed_tabs_for_every_range() {
    let _profile = crate::profile::TestProfileGuard::isolated();
    for range in [crate::browsing_data::Range::Hour, crate::browsing_data::Range::Everything] {
        let app = app_with_pending(rect(800, 600));
        let tab = App::new_tab(&app);
        app.borrow_mut().tabs.iter_mut().find(|entry| entry.id == tab).unwrap().url =
            Some(url::Url::parse("https://closed.example/page").unwrap());
        assert!(App::close_tab(&app, tab));
        assert_eq!(App::recently_closed(&app).len(), 1, "{range:?}: the closed tab is remembered");
        let plan = crate::browsing_data::Plan {
            range,
            local_history: true,
            engine: Vec::new(),
            passwords: false,
            own_site_settings: false,
            ignored_range: Vec::new(),
        };
        let reported = Rc::new(RefCell::new(false));
        let seen = reported.clone();
        App::delete_browsing_data(&app, plan, Box::new(move |_| *seen.borrow_mut() = true));
        assert!(*reported.borrow());
        assert!(App::recently_closed(&app).is_empty(), "{range:?}: a deleted page can still be reopened");
    }
}

/// Records the tracking-prevention levels the app puts in force.
struct TrackingEngine(Rc<RefCell<Vec<TrackingPreventionLevel>>>);

impl EngineAdapter for TrackingEngine {
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
    fn set_tracking_prevention(&mut self, level: TrackingPreventionLevel) {
        self.0.borrow_mut().push(level);
    }
}

/// Settings > Privacy > Tracking acts without a restart: a saved change reaches the engine once, memory
/// holds it (so a later save from memory keeps it), and an unchanged save asks nothing of the engine.
#[test]
fn a_tracking_prevention_change_is_put_in_force_at_once() {
    use crate::settings::TrackingLevel;
    let app = app_with_pending(rect(800, 600));
    let seen = Rc::new(RefCell::new(Vec::new()));
    app.borrow_mut().engine = Some(Box::new(TrackingEngine(seen.clone())));
    app.borrow_mut().settings.tracking_prevention = TrackingLevel::Balanced;
    let mut settings = App::settings(&app);
    settings.tracking_prevention = TrackingLevel::Strict;
    App::apply_live_settings(&app, &settings);
    assert_eq!(*seen.borrow(), [TrackingPreventionLevel::Strict]);
    assert_eq!(App::settings(&app).tracking_prevention, TrackingLevel::Strict, "memory holds the level in force");
    App::apply_live_settings(&app, &settings);
    assert_eq!(seen.borrow().len(), 1, "the same level is not applied twice");
    settings.tracking_prevention = TrackingLevel::Balanced;
    App::apply_live_settings(&app, &settings);
    assert_eq!(*seen.borrow(), [TrackingPreventionLevel::Strict, TrackingPreventionLevel::Balanced]);
}

/// A benchmark launch keeps the engine's default level, as it did at start, whatever is saved.
#[test]
fn a_benchmark_launch_leaves_tracking_prevention_alone() {
    let app = app_with_pending(rect(800, 600));
    let seen = Rc::new(RefCell::new(Vec::new()));
    app.borrow_mut().engine = Some(Box::new(TrackingEngine(seen.clone())));
    app.borrow_mut().benchmark_url = Some(url::Url::parse("https://bench.example/").unwrap());
    app.borrow_mut().settings.tracking_prevention = crate::settings::TrackingLevel::Balanced;
    let mut settings = App::settings(&app);
    settings.tracking_prevention = crate::settings::TrackingLevel::Strict;
    App::apply_live_settings(&app, &settings);
    assert!(seen.borrow().is_empty());
}
