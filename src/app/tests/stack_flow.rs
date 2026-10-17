//! What the tab stack, tiling and workspace options and commands do to the running app
//! (src/app/stacks.rs), driven with a recording engine and no window. Each option of Settings > Tab
//! stacks, tiling and workspaces has a test here that changes the option and watches the effect.

use super::*;
use crate::tab_groups::{DomainKey, TabGroupKey};
use crate::tab_options::StackStyle;

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
fn the_stack_style_folds_islands_for_every_surface_that_draws_them() {
    let app = fixture();
    let ids = tabs_on(&app, &["https://a.com/1", "https://a.com/2", "https://b.com/1", "https://b.com/2"]);
    let folded = |app: &AppRef| {
        let preferences = App::tab_group_preferences(app);
        (preferences.is_collapsed(&key("a.com")), preferences.is_collapsed(&key("b.com")))
    };
    assert_eq!(folded(&app), (false, false), "Islands: nothing folded until the user folds it");
    set(&app, |o| o.stack_style = StackStyle::Compact);
    assert_eq!(folded(&app), (true, true), "Compact: every stack folded");
    let slots = tab_groups::project_strip(&App::tab_group_inputs(&app), 0, &App::tab_group_preferences(&app));
    assert_eq!(slots.len(), 2, "the strip draws one slot per stack: {slots:?}");
    assert!(slots[0].collapsed && !slots[0].marker_only, "the stack in use shows its tab");
    assert!(slots[1].marker_only, "another stack is its ball alone");
    set(&app, |o| o.stack_style = StackStyle::Accordion);
    assert_eq!(folded(&app), (false, true), "Accordion: only the stack in use is open");
    let _ = App::select_tab(&app, ids[2]);
    assert_eq!(folded(&app), (true, false), "and it follows the tab in front");
    assert!(!app.borrow().group_preferences.is_collapsed(&key("a.com")), "the stored flags stay the user's own");
}

#[test]
fn stacks_off_forms_no_stack_but_keeps_the_site_rules() {
    let app = fixture();
    let _ = tabs_on(&app, &["https://a.com/1", "https://a.com/2"]);
    assert!(stacks_of(&app).iter().all(Option::is_some));
    set(&app, |o| o.stack_style = StackStyle::Off);
    assert!(stacks_of(&app).iter().all(Option::is_none), "no stack at all");
    assert!(tab_groups::live_domains(&app.borrow().all_group_inputs()).contains(&key("a.com")), "a standing site rule still sees the site open");
}

#[test]
fn a_ball_click_under_compact_goes_to_the_member_used_last() {
    let app = fixture();
    let ids = tabs_on(&app, &["https://a.com/1", "https://a.com/2", "https://c.com/"]);
    set(&app, |o| o.stack_style = StackStyle::Compact);
    let _ = App::select_tab(&app, ids[1]);
    let _ = App::select_tab(&app, ids[2]);
    assert!(App::set_tab_group_collapsed(&app, ids[0], false), "the ball's click selects instead of unfolding");
    assert_eq!(App::active_tab_id(&app), ids[1], "the member used last");
    assert!(App::set_tab_group_collapsed(&app, ids[0], false));
    assert_eq!(App::active_tab_id(&app), ids[0], "inside the stack the ball steps to the next member");
    assert!(!app.borrow().group_preferences.is_collapsed(&key("a.com")), "nothing stored");
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
    set(&app, |o| o.stack_style = StackStyle::Off);
    assert_eq!(App::stack_all_by_site(&app), 0, "stacks off: the command does nothing");
}

#[test]
fn dragging_a_tab_onto_another_stacks_them_only_while_the_option_is_on() {
    let app = fixture();
    app.borrow_mut().settings.automatic_tab_grouping = false;
    let ids = tabs_on(&app, &["https://a.com/", "https://x.org/", "https://b.com/"]);
    set(&app, |o| o.stack_drag = false);
    assert!(!App::stack_onto(&app, ids[2], ids[0]), "the option is off");
    assert!(App::stack_members(&app, ids[0]).is_empty());
    set(&app, |o| o.stack_drag = true);
    assert!(App::stack_onto(&app, ids[2], ids[0]));
    assert!(in_stack(&app, ids[0], "a.com") && in_stack(&app, ids[2], "a.com"), "a stack of the target's site, whatever the dragged tab's");
    assert_eq!(App::tab_ids(&app), [ids[0], ids[2], ids[1]], "the dragged tab sits next to its stack");
    assert!(!App::stack_onto(&app, ids[0], ids[0]));
    App::set_tab_pinned(&app, ids[1], true);
    assert!(!App::stack_onto(&app, ids[1], ids[0]), "a pinned tab never stacks");
}

#[test]
fn open_new_tabs_in_the_current_stack() {
    let app = fixture();
    app.borrow_mut().settings.automatic_tab_grouping = false;
    let ids = tabs_on(&app, &["https://a.com/1", "https://a.com/2"]);
    assert_eq!(App::stack_site(&app, ids[0]), 2);
    let outside = App::open_new_tab(&app);
    assert!(App::stack_members(&app, outside).is_empty(), "off: a new tab is a tab of its own");
    let _ = App::select_tab(&app, ids[0]);
    set(&app, |o| o.stack_open_in_current = true);
    let inside = App::open_new_tab(&app);
    assert!(in_stack(&app, inside, "a.com"), "on: it joins the stack in front");
    let _ = App::select_tab(&app, outside);
    let lone = App::open_new_tab(&app);
    assert!(App::stack_members(&app, lone).is_empty(), "no stack in front: nothing to join");
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
