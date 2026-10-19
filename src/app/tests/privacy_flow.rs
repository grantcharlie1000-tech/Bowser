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
