//! What the tab stack, tiling and workspace options and commands do to the running app
//! (src/app/stacks.rs), driven with a recording engine and no window. Each workspace and tiling
//! option has a test here that changes the option and watches the effect.

use super::*;
use crate::tab_groups::{DomainKey, TabGroupKey};

struct QuietEngine;

impl EngineAdapter for QuietEngine {
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
}

fn fixture() -> AppRef {
    let app = app_with_pending(rect(800, 600));
    app.borrow_mut().engine = Some(Box::new(QuietEngine));
    app
}

/// A web page in tab `id`, as if it had loaded.
fn page(app: &AppRef, id: TabId, address: &str) {
    let mut state = app.borrow_mut();
    state.tab_model.set_url(id, Some(address));
    let tab = state.tabs.iter_mut().find(|tab| tab.id == id).expect("tab");
    tab.url = Some(url::Url::parse(address).expect("url"));
    tab.controller_created = true;
}

/// Tabs showing `addresses`, in order, the first active.
fn tabs_on(app: &AppRef, addresses: &[&str]) -> Vec<TabId> {
    let first = App::active_tab_id(app);
    let mut ids = vec![first];
    for _ in 1..addresses.len() {
        ids.push(App::new_tab(app));
    }
    for (id, address) in ids.iter().zip(addresses) {
        page(app, *id, address);
    }
    let _ = App::select_tab(app, first);
    ids
}

fn set(app: &AppRef, change: impl FnOnce(&mut crate::tab_options::TabOptions)) {
    change(&mut app.borrow_mut().settings.tabs);
}

fn key(domain: &str) -> DomainKey {
    DomainKey::parse(domain).unwrap()
}

/// The stack each tab is in (`None` for none), in strip order.
fn stacks_of(app: &AppRef) -> Vec<Option<TabGroupKey>> {
    let inputs = App::tab_group_inputs(app);
    tab_groups::group_keys(&inputs, &App::tab_group_preferences(app))
}

fn in_stack(app: &AppRef, id: TabId, domain: &str) -> bool {
    App::island_menu(app, id).is_some_and(|_| app.borrow().domain_island_of(id) == Some(key(domain)))
}

#[test]
fn stacking_by_site_by_hand_works_without_automatic_grouping_and_unstack_undoes_it() {
    let app = fixture();
    app.borrow_mut().settings.automatic_tab_grouping = false;
    let ids = tabs_on(&app, &["https://a.com/1", "https://b.com/1", "https://a.com/2", "https://b.com/2", "https://c.com/"]);
    assert!(stacks_of(&app).iter().all(Option::is_none), "automatic grouping is off");
    assert_eq!(App::stack_site(&app, ids[0]), 2);
    assert!(in_stack(&app, ids[0], "a.com") && in_stack(&app, ids[2], "a.com"));
    assert!(!in_stack(&app, ids[1], "b.com"));
    assert_eq!(App::stack_site(&app, ids[4]), 0, "a lone site makes no stack");
    assert!(App::unstack(&app, ids[0]));
    assert!(App::stack_members(&app, ids[0]).is_empty());
    assert_eq!(App::stack_all_by_site(&app), 2, "a.com and b.com");
    assert!(in_stack(&app, ids[1], "b.com") && in_stack(&app, ids[3], "b.com") && in_stack(&app, ids[2], "a.com"));
    assert!(App::undo_tab_organisation(&app), "one undo step for the whole command");
    assert!(App::stack_members(&app, ids[1]).is_empty());
}

#[test]
fn dragging_a_tab_onto_another_stacks_them() {
    let app = fixture();
    app.borrow_mut().settings.automatic_tab_grouping = false;
    let ids = tabs_on(&app, &["https://a.com/", "https://x.org/", "https://b.com/"]);
    assert!(App::stack_members(&app, ids[0]).is_empty());
    assert!(App::stack_onto(&app, ids[2], ids[0]));
    assert!(in_stack(&app, ids[0], "a.com") && in_stack(&app, ids[2], "a.com"), "a stack of the target's site, whatever the dragged tab's");
    assert_eq!(App::tab_ids(&app), [ids[0], ids[2], ids[1]], "the dragged tab sits next to its stack");
    assert!(!App::stack_onto(&app, ids[0], ids[0]));
    App::set_tab_pinned(&app, ids[1], true);
    assert!(!App::stack_onto(&app, ids[1], ids[0]), "a pinned tab never stacks");
}

#[test]
fn new_tab_position_as_stack_with_related_stacks_a_link_with_its_page() {
    let app = fixture();
    app.borrow_mut().settings.automatic_tab_grouping = false;
    set(&app, |o| o.link_background = true);
    let ids = tabs_on(&app, &["https://a.com/article"]);
    assert!(crate::tab_options::apply_assignment(&mut app.borrow_mut().settings, "tabs_new_tab_position=related"));
    App::open_link_tab(&app, ids[0], false, url::Url::parse("https://elsewhere.net/").unwrap());
    assert!(App::stack_members(&app, ids[0]).is_empty(), "related only places the tab");
    assert!(crate::tab_options::apply_assignment(&mut app.borrow_mut().settings, "tabs_new_tab_position=stack"));
    App::open_link_tab(&app, ids[0], false, url::Url::parse("https://elsewhere.net/2").unwrap());
    let members = App::stack_members(&app, ids[0]);
    assert_eq!(members.len(), 2, "the page and its link make a stack: {members:?}");
    assert!(in_stack(&app, ids[0], "a.com"), "the stack is the opener's site");
    assert_eq!(App::tab_ids(&app).last(), members.last(), "placed after the related tabs, as related does");
}

#[test]
fn a_workspace_rule_opens_its_site_in_the_named_workspace() {
    let app = fixture();
    let ids = tabs_on(&app, &["https://start.example/"]);
    let home = app.borrow().workspaces.active_id();
    let work = App::create_workspace(&app, "Work").unwrap();
    let _ = App::switch_workspace(&app, home);
    let mut rules = Vec::new();
    crate::workspace_rules::add(&mut rules, "example.org", "work").unwrap();
    app.borrow_mut().workspace_rules = rules;
    set(&app, |o| o.link_background = true);
    App::open_link_tab(&app, ids[0], false, url::Url::parse("https://news.example.org/a").unwrap());
    let moved = *app.borrow().workspaces.get(work).unwrap().tabs().last().unwrap();
    assert_eq!(App::tab_committed_url(&app, moved).map(|url| url.to_string()).as_deref(), Some("https://news.example.org/a"));
    assert_eq!(app.borrow().workspaces.active_id(), home, "a background link moves, the window stays");
    App::submit_url(&app, url::Url::parse("https://example.org/").unwrap(), None);
    assert_eq!(app.borrow().workspaces.active_id(), work, "the page in front takes the window with it");
    assert_eq!(app.borrow().workspaces.workspace_for(App::active_tab_id(&app)), Some(work));
    assert!(!app.borrow().workspaces.get(home).unwrap().tabs().is_empty(), "the workspace it left keeps a tab");
    set(&app, |o| o.workspaces = false);
    let _ = App::switch_workspace(&app, home);
    let before = app.borrow().workspaces.get(work).unwrap().tabs().len();
    App::submit_url(&app, url::Url::parse("https://example.org/b").unwrap(), None);
    assert_eq!(app.borrow().workspaces.get(work).unwrap().tabs().len(), before, "workspaces off: rules rest");
}

#[test]
fn a_workspace_rule_also_moves_a_tab_that_follows_a_link_in_place() {
    let app = fixture();
    let ids = tabs_on(&app, &["https://start.example/", "https://keep.example/"]);
    let home = app.borrow().workspaces.active_id();
    let work = App::create_workspace(&app, "Work").unwrap();
    let _ = App::switch_workspace(&app, home);
    let mut rules = Vec::new();
    crate::workspace_rules::add(&mut rules, "example.org", "work").unwrap();
    app.borrow_mut().workspace_rules = rules;
    let link = url::Url::parse("https://news.example.org/a").unwrap();
    // Off: nothing is queued.
    set(&app, |o| o.workspaces = false);
    assert!(!App::defer_workspace_rule(&app, ids[0], &link));
    set(&app, |o| o.workspaces = true);
    let _ = App::select_tab(&app, ids[0]);
    assert!(!App::defer_workspace_rule(&app, ids[0], &url::Url::parse("https://other.test/").unwrap()), "no rule, no move");
    assert!(App::defer_workspace_rule(&app, ids[0], &link), "the move waits outside the engine's event");
    assert_eq!(app.borrow().workspaces.workspace_for(ids[0]), Some(home), "not moved inside the event");
    App::run_deferred_links(&app);
    assert_eq!(app.borrow().workspaces.workspace_for(ids[0]), Some(work), "moved to the rule's workspace");
    assert_eq!(app.borrow().workspaces.active_id(), work, "the tab in front takes the window with it");
    assert!(app.borrow().tab_state.deferred_workspace.is_empty());
}

#[test]
fn scrolling_switches_workspaces_only_with_the_option() {
    let app = fixture();
    let home = app.borrow().workspaces.active_id();
    let other = App::create_workspace(&app, "Other").unwrap();
    let _ = App::switch_workspace(&app, home);
    assert!(!App::scroll_workspaces(&app, 1), "off by default");
    set(&app, |o| o.workspace_scroll = true);
    assert!(App::scroll_workspaces(&app, 1));
    assert_eq!(app.borrow().workspaces.active_id(), other);
    assert!(!App::scroll_workspaces(&app, 1), "the last one: no wrap");
    assert!(App::step_workspace(&app, -1), "the commands work without the option");
    assert_eq!(app.borrow().workspaces.active_id(), home);
}

#[test]
fn a_stack_moves_to_another_workspace_and_stays_a_stack() {
    let app = fixture();
    let home = app.borrow().workspaces.active_id();
    let ids = tabs_on(&app, &["https://a.com/1", "https://a.com/2", "https://b.com/"]);
    let work = App::create_workspace(&app, "Work").unwrap();
    let _ = App::switch_workspace(&app, home);
    let _ = App::select_tab(&app, ids[0]);
    let members = App::stack_members(&app, ids[0]);
    assert_eq!(App::move_tabs_to_workspace(&app, &members, work, false), 2);
    assert_eq!(app.borrow().workspaces.active_id(), home, "the window stays");
    assert_eq!(App::active_tab_id(&app), ids[2], "the tab in front left, so another shows");
    let _ = App::switch_workspace(&app, work);
    assert_eq!(App::stack_members(&app, ids[0]).len(), 2, "still a stack over there");
}

#[test]
fn tiling_lays_the_tabs_out_in_the_split_view() {
    let app = fixture();
    let ids = tabs_on(&app, &["https://a.com/", "https://b.com/", "https://c.com/"]);
    assert!(App::tile_tabs(&app, &ids, crate::stacks::TileMode::Grid, |_| None));
    let split = App::split_view(&app).expect("tiled");
    assert_eq!(split.pane_count(), 3);
    assert_eq!(split.cells(), [ids[0], ids[1], ids[2], ids[2]]);
    assert!(App::separate_split(&app), "untile");
    assert!(App::split_view(&app).is_none());
    assert!(!App::tile_tabs(&app, &ids[..1], crate::stacks::TileMode::Vertical, |_| None), "one tab is no tiling");
}


#[test]
fn keep_window_off_counts_the_tabs_of_every_workspace() {
    let app = fixture();
    let home = app.borrow().workspaces.active_id();
    let _work = App::create_workspace(&app, "Work").unwrap();
    let _ = App::switch_workspace(&app, home);
    assert_eq!(app.borrow().workspaces.active().tabs().len(), 1, "one tab in front");
    assert!(App::tab_count(&app) > 1, "another workspace holds a tab");
    set(&app, |o| o.keep_window = false);
    let only = App::active_tab_id(&app);
    assert_eq!(App::user_close_tab(&app, only), crate::tab_options::CloseKind::Close, "the window has tabs elsewhere: it stays");
    assert!(App::tab_ids(&app).iter().all(|id| *id != only), "the tab itself closed");
    assert!(App::tab_count(&app) >= 1, "the window keeps the other workspace's tab");
}

// ----- manual tab groups (tab menu > Tab group) and new tabs in a group -----

fn manual_of(app: &AppRef, id: TabId) -> Option<crate::tab_groups::ManualGroupId> {
    match app.borrow().group_key_of(id)? {
        TabGroupKey::Manual(group) => Some(group),
        TabGroupKey::Domain(_) => None,
    }
}

#[test]
fn a_manual_group_holds_tabs_of_any_site_and_goes_when_its_last_tab_leaves() {
    let app = fixture();
    let ids = tabs_on(&app, &["https://a.com/", "https://b.com/", "https://c.com/"]);
    let group = App::add_tab_to_new_group(&app, ids[0]).expect("a new group");
    assert_eq!(manual_of(&app, ids[0]), Some(group), "a group of one tab is a group");
    assert_eq!(App::stack_name(&app, ids[0]).as_deref(), Some("Group 1"));
    // The other tabs are offered that group to join; the grouped tab is offered none (it is in the only one).
    let choices = App::group_choices(&app, ids[1]);
    assert_eq!(choices, vec![(TabGroupKey::Manual(group), "Group 1".to_owned())]);
    assert!(App::group_choices(&app, ids[0]).is_empty());
    assert!(App::add_tab_to_group(&app, ids[1], &TabGroupKey::Manual(group)));
    assert_eq!(App::stack_members(&app, ids[0]), [ids[0], ids[1]], "a.com and b.com share it");
    // It is named and coloured like any island.
    assert!(App::set_tab_group_name(&app, ids[1], "Holiday"));
    assert!(App::set_tab_group_colour(&app, ids[1], Some(2)));
    assert_eq!(App::stack_name(&app, ids[0]).as_deref(), Some("Holiday"));
    assert!(App::set_tab_group_collapsed(&app, ids[0], true));
    assert_eq!(App::tab_group_collapsed(&app, ids[1]), Some(true));
    // Leaving one by one: the group goes with its last tab.
    assert!(App::remove_tab_from_group(&app, ids[0]));
    assert_eq!(manual_of(&app, ids[1]), Some(group));
    assert!(App::remove_tab_from_group(&app, ids[1]));
    assert!(App::tab_group_preferences(&app).manual_group(group).is_none(), "an empty group is forgotten");
    // Undo brings it back, tab and all.
    assert!(App::undo_tab_organisation(&app));
    assert_eq!(manual_of(&app, ids[1]), Some(group));
    assert_eq!(App::stack_name(&app, ids[1]).as_deref(), Some("Holiday"));
}

#[test]
fn automatic_grouping_never_takes_a_tab_out_of_a_manual_group() {
    let app = fixture();
    let ids = tabs_on(&app, &["https://a.com/1", "https://a.com/2", "https://a.com/3", "https://b.com/1", "https://b.com/2"]);
    // Three a.com tabs: a site group.
    assert_eq!(stacks_of(&app)[0], Some(TabGroupKey::Domain(key("a.com"))));
    let group = App::add_tab_to_new_group(&app, ids[2]).expect("a new group");
    assert_eq!(manual_of(&app, ids[2]), Some(group));
    assert!(in_stack(&app, ids[0], "a.com"), "the other a.com tabs keep their site group");
    // "Group all tabs by website" stacks a.com's two free tabs and b.com, and leaves ids[2] where it is.
    assert_eq!(App::stack_all_by_site(&app), 2);
    assert_eq!(manual_of(&app, ids[2]), Some(group));
    assert!(in_stack(&app, ids[1], "a.com") && in_stack(&app, ids[3], "b.com"));
    // So do "Group this website's tabs" and the "Always" rule for its site.
    assert_eq!(App::stack_site(&app, ids[0]), 2);
    assert!(App::set_tab_group_mode(&app, ids[0], crate::tab_groups::GroupMode::Always));
    assert_eq!(manual_of(&app, ids[2]), Some(group));
    // Only the person takes it out: moving it to a site group is explicit.
    assert!(App::add_tab_to_group(&app, ids[2], &TabGroupKey::Domain(key("b.com"))));
    assert!(in_stack(&app, ids[2], "b.com"));
    assert!(App::tab_group_preferences(&app).manual_group(group).is_none());
}

#[test]
fn ctrl_t_on_a_grouped_tab_opens_the_new_tab_in_that_group() {
    let app = fixture();
    let ids = tabs_on(&app, &["https://a.com/", "https://b.com/", "https://c.com/"]);
    let group = App::add_tab_to_new_group(&app, ids[0]).expect("a new group");
    let _ = App::select_tab(&app, ids[0]);
    // "As last" would put it at the far end; it joins the group and sits right after its last tab.
    assert!(crate::tab_options::apply_assignment(&mut app.borrow_mut().settings, "tabs_new_tab_position=last"));
    let fresh = App::open_new_tab(&app);
    assert_eq!(manual_of(&app, fresh), Some(group));
    assert_eq!(App::tab_ids(&app), [ids[0], fresh, ids[1], ids[2]]);
    // A site group too: the new tab joins it whatever page it then opens.
    let _ = App::select_tab(&app, ids[1]);
    assert!(App::set_tab_group_mode(&app, ids[1], crate::tab_groups::GroupMode::Always));
    let site = App::open_new_tab(&app);
    assert!(in_stack(&app, site, "b.com"));
    page(&app, site, "https://elsewhere.net/");
    assert!(in_stack(&app, site, "b.com"), "it stays in the group it opened in");
    // A tab in no group opens a tab in no group.
    let _ = App::select_tab(&app, ids[2]);
    let loose = App::open_new_tab(&app);
    assert!(app.borrow().group_key_of(loose).is_none());
    // The option off: Ctrl+T on a grouped tab opens a tab in no group.
    set(&app, |o| o.new_tab_in_group = false);
    let _ = App::select_tab(&app, ids[0]);
    let off = App::open_new_tab(&app);
    assert!(app.borrow().group_key_of(off).is_none());
    // "New tab to the right" joins the right-clicked tab's group (window.rs calls join_group_of).
    set(&app, |o| o.new_tab_in_group = true);
    let right = App::new_tab(&app);
    assert!(App::join_group_of(&app, right, ids[0]));
    assert_eq!(manual_of(&app, right), Some(group));
    // A private tab never joins.
    let private = App::new_private_tab(&app);
    assert!(!App::join_group_of(&app, private, ids[0]));
}

#[test]
fn manual_groups_come_back_with_the_session() {
    let app = fixture();
    let ids = tabs_on(&app, &["https://a.com/", "https://b.com/", "https://c.com/"]);
    let group = App::add_tab_to_new_group(&app, ids[0]).expect("a new group");
    assert!(App::add_tab_to_group(&app, ids[2], &TabGroupKey::Manual(group)));
    assert!(App::set_tab_group_name(&app, ids[0], "Reading"));
    assert!(App::set_tab_group_colour(&app, ids[0], Some(5)));
    assert!(App::set_tab_group_collapsed(&app, ids[0], true));
    let snapshot = App::session_snapshot(&app).expect("a snapshot");
    assert_eq!(snapshot.tabs.iter().map(|tab| tab.group).collect::<Vec<_>>(), [Some(group.0), None, Some(group.0)]);
    assert_eq!(snapshot.groups.len(), 1);
    let text = snapshot.serialize();

    let restored = fixture();
    restored.borrow_mut().pending_restore = Some(crate::session::Session::parse(&text).expect("parses"));
    App::restore_pending_session(&restored);

    let tabs = App::tab_ids(&restored);
    assert_eq!(tabs.len(), 3);
    let back = manual_of(&restored, tabs[0]).expect("the group came back");
    assert_eq!(manual_of(&restored, tabs[2]), Some(back));
    assert_eq!(manual_of(&restored, tabs[1]), None);
    assert_eq!(App::stack_name(&restored, tabs[0]).as_deref(), Some("Reading"));
    let preferences = App::tab_group_preferences(&restored);
    assert_eq!(preferences.group_colour(&TabGroupKey::Manual(back)), Some(5));
    assert!(preferences.group_collapsed(&TabGroupKey::Manual(back)));
}
