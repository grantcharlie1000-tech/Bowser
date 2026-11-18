//! Workspaces across a restart (T1, T2, T3), strip moves and workspace creation that the model
//! refuses (T4, T7), reopening a closed tab (T6) and crossing workspaces by selecting a tab (T8).

use super::*;
use crate::workspaces::{WorkspaceId, WorkspaceSet};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bowser-ws-restore-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn page(app: &AppRef, id: TabId, address: &str) {
    let mut state = app.borrow_mut();
    state.tab_model.set_url(id, Some(address));
    let tab = state.tabs.iter_mut().find(|tab| tab.id == id).expect("tab");
    tab.url = Some(url::Url::parse(address).expect("url"));
}

fn model_order(app: &AppRef) -> Vec<TabId> {
    app.borrow().tab_model.iter().map(|tab| tab.id).collect()
}

/// Every workspace member is a tab of this run, and no tab is in two workspaces.
fn assert_membership_is_sound(app: &AppRef) {
    let state = app.borrow();
    let live: Vec<TabId> = state.tabs.iter().map(|tab| tab.id).collect();
    let mut seen = std::collections::HashSet::new();
    for workspace in state.workspaces.all() {
        for tab in workspace.tabs() {
            assert!(live.contains(tab), "{} holds {tab:?}, which is not a tab", workspace.name());
            assert!(seen.insert(*tab), "{tab:?} is in two workspaces");
        }
    }
    assert_eq!(seen.len(), live.len(), "every tab belongs to a workspace");
    assert!(WorkspaceSet::parse(&state.workspaces.serialize()).is_ok());
}

/// Starts the app on a profile whose `workspaces.txt` holds `saved`.
fn start_with(name: &str, saved: &WorkspaceSet) -> (AppRef, PathBuf) {
    let dir = temp_dir(name);
    let path = dir.join(crate::workspaces::FILE_NAME);
    saved.save(&path).unwrap();
    let app = app_with_pending(rect(800, 600));
    app.borrow_mut().workspaces_path = Some(path);
    assert!(app.borrow_mut().init_session(&dir));
    (app, dir)
}

fn names(app: &AppRef) -> Vec<String> {
    app.borrow().workspaces.all().iter().map(|w| w.name().to_owned()).collect()
}

/// T1: a start without restore keeps every workspace; none depends on an old tab id.
#[test]
fn workspaces_survive_a_restart_without_restore() {
    let mut saved = WorkspaceSet::new("Default", TabId(3));
    saved.add_tab(TabId(5)).unwrap();
    saved.create("Research", TabId(8)).unwrap();
    let (app, dir) = start_with("plain", &saved);
    assert_eq!(names(&app), ["Default", "Research"]);
    assert_eq!(app.borrow().workspaces.active_id(), WorkspaceId(0));
    assert_eq!(App::tab_ids(&app), [App::active_tab_id(&app)], "the strip shows the first tab alone");
    assert_membership_is_sound(&app);
    drop(app);
    let _ = std::fs::remove_dir_all(dir);
}

/// T1: the last run's first tab id being in "Research" no longer decides which workspace lives.
#[test]
fn workspace_survival_does_not_depend_on_an_old_tab_id() {
    let mut saved = WorkspaceSet::new("Default", TabId(4));
    saved.create("Research", TabId(0)).unwrap();
    saved.switch_to(WorkspaceId(0)).unwrap();
    let (app, dir) = start_with("old-id", &saved);
    assert_eq!(names(&app), ["Default", "Research"]);
    assert_eq!(app.borrow().workspaces.active().name(), "Default");
    assert_membership_is_sound(&app);
    drop(app);
    let _ = std::fs::remove_dir_all(dir);
}

fn session(urls: &[&str]) -> crate::session::Session {
    crate::session::Session {
        tabs: urls
            .iter()
            .map(|url| crate::session::SessionTab { url: (*url).into(), title: String::new(), pinned: false, used: 0 })
            .collect(),
        active: 0,
        split: None,
    }
}

/// T2: a New Tab page (not in the saved session) never leaves a stale id that collides with a
/// restored one, so no tab is in two workspaces and the next save still loads.
#[test]
fn restore_with_an_unsaved_tab_never_puts_a_tab_in_two_workspaces() {
    // Last run: Default [0 web, 1 New Tab], Work [2 web]; the session saved tabs 0 and 2.
    let mut saved = WorkspaceSet::new("Default", TabId(0));
    saved.add_tab(TabId(1)).unwrap();
    let work = saved.create("Work", TabId(2)).unwrap();
    saved.switch_to(WorkspaceId(0)).unwrap();
    saved.set_session_order(&[TabId(0), TabId(2)]);
    // What `init_session` leaves when the restore offer is accepted: the file as loaded, a pending session.
    let app = app_with_pending(rect(800, 600));
    app.borrow_mut().workspaces = WorkspaceSet::parse(&saved.serialize()).unwrap();
    app.borrow_mut().pending_restore = Some(session(&["https://a.example/", "https://w.example/"]));
    App::restore_pending_session(&app);
    assert_membership_is_sound(&app);
    let state = app.borrow();
    let work_tabs = state.workspaces.get(work).unwrap().tabs().to_vec();
    assert_eq!(work_tabs.len(), 1);
    let url = state.tabs.iter().find(|tab| tab.id == work_tabs[0]).and_then(|tab| tab.url.clone());
    assert_eq!(url.map(String::from).as_deref(), Some("https://w.example/"));
}

/// T3: a navigation that adds a session tab also rewrites `order=`, so a crash restore pairs each
/// session tab with its own workspace.
#[test]
fn a_session_save_keeps_the_workspace_order_in_step() {
    let dir = temp_dir("order");
    let path = dir.join(crate::workspaces::FILE_NAME);
    let app = app_with_pending(rect(800, 600));
    {
        let (store, _) = crate::session::SessionStore::open(&dir).unwrap();
        let mut state = app.borrow_mut();
        state.workspaces_path = Some(path.clone());
        state.session = crate::session::SessionWriter::new(store).ok();
    }
    let first = App::active_tab_id(&app);
    page(&app, first, "https://first.example/");
    let blank = App::new_tab(&app);
    App::create_workspace(&app, "Work").unwrap();
    let work_tab = App::active_tab_id(&app);
    page(&app, work_tab, "https://work.example/");
    App::session_save_now(&app);
    // The New Tab page in Default navigates; no workspace changes.
    page(&app, blank, "https://later.example/");
    App::session_save_now(&app);
    let text = std::fs::read_to_string(&path).unwrap();
    let order = text.lines().find_map(|line| line.strip_prefix("order=")).unwrap_or_default();
    let expected = model_order(&app).iter().map(|id| id.0.to_string()).collect::<Vec<_>>().join(",");
    assert_eq!(order, expected, "{text}");
    App::session_finish(&app);
    drop(app);
    let _ = std::fs::remove_dir_all(dir);
}

/// T4: dropping an unpinned tab in front of a pinned one leaves the strip and the model in step.
#[test]
fn drag_unpinned_before_pinned_keeps_strip_and_model_in_step() {
    let app = app_with_pending(rect(800, 600));
    let a = App::active_tab_id(&app);
    let b = App::new_tab(&app);
    let c = App::new_tab(&app);
    assert!(App::set_tab_pinned(&app, a, true));
    assert_eq!(App::tab_ids(&app), [a, b, c]);
    let _ = App::drop_tab(&app, b, 0);
    assert_eq!(App::tab_ids(&app), model_order(&app), "strip (workspace) order and model order diverged");
    assert_eq!(App::tab_ids(&app), [a, b, c], "pinned tabs stay first");
}

/// T7: a name already in use is refused without leaving a stray blank tab.
#[test]
fn failed_workspace_create_leaves_no_new_tab() {
    let app = app_with_pending(rect(800, 600));
    let before = App::tab_ids(&app).len();
    assert!(App::create_workspace(&app, "default").is_err());
    assert_eq!(App::tab_ids(&app).len(), before, "a stray new tab was left behind");
    assert_eq!(app.borrow().tabs.len(), before);
}

/// A move to the right (a drag, "Duplicate" beside a tab further right) lands exactly at the
/// asked-for place in the strip, and the model agrees. `move_tab` takes the index without the tab.
#[test]
fn a_tab_moved_right_lands_where_it_was_dropped() {
    let app = app_with_pending(rect(800, 600));
    let a = App::active_tab_id(&app);
    let b = App::new_tab(&app);
    let c = App::new_tab(&app);
    let d = App::new_tab(&app);
    assert_eq!(App::tab_ids(&app), [a, b, c, d]);
    // The HTML strip drops a before c: post-removal index 1.
    let index = crate::chrome_host::drop_index(&App::tab_ids(&app), a, c.0 + 1);
    assert!(App::drop_tab(&app, a, index.expect("a move")));
    assert_eq!(App::tab_ids(&app), [b, a, c, d]);
    assert_eq!(App::tab_ids(&app), model_order(&app));
    assert!(App::move_tab(&app, b, 3));
    assert_eq!(App::tab_ids(&app), [a, c, d, b], "to the end");
    assert_eq!(App::tab_ids(&app), model_order(&app));
    assert!(App::move_tab(&app, a, 1));
    assert_eq!(App::tab_ids(&app), [c, a, d, b], "one place right");
    assert_eq!(App::tab_ids(&app), model_order(&app));
}

/// T5: "Duplicate" on a background tab right of the active one opens the copy beside the active
/// tab, then moves it beside the original.
#[test]
fn a_duplicate_of_a_background_tab_lands_beside_it() {
    let app = app_with_pending(rect(800, 600));
    let a = App::active_tab_id(&app);
    let b = App::new_tab(&app);
    let c = App::new_tab(&app);
    let d = App::new_tab(&app);
    assert!(App::select_tab(&app, a));
    let copy = App::new_tab(&app);
    assert_eq!(App::tab_ids(&app), [a, copy, b, c, d], "a new tab opens beside the active one");
    // window.rs `duplicate_index` with "beside": the original's place without the copy, plus one.
    let without: Vec<TabId> = App::tab_ids(&app).into_iter().filter(|tab| *tab != copy).collect();
    let after = without.iter().position(|tab| *tab == b).unwrap() + 1;
    assert!(App::move_tab(&app, copy, after));
    assert_eq!(App::tab_ids(&app), [a, b, copy, c, d]);
    assert_eq!(App::tab_ids(&app), model_order(&app));
}

/// T6: with two workspaces, a tab closed in the second one reopens in its own place there, and a
/// tab closed in the first one reopens in the first even when the second is shown by then.
#[test]
fn a_reopened_tab_returns_to_its_place_in_its_own_workspace() {
    let app = app_with_pending(rect(800, 600));
    let a0 = App::active_tab_id(&app);
    page(&app, a0, "https://a0.example/");
    let a1 = App::new_tab(&app);
    page(&app, a1, "https://a1.example/");
    let a2 = App::new_tab(&app);
    page(&app, a2, "https://a2.example/");
    let home = app.borrow().workspaces.active().id();
    let work = App::create_workspace(&app, "Work").unwrap();
    let b0 = App::active_tab_id(&app);
    page(&app, b0, "https://b0.example/");
    let b1 = App::new_tab(&app);
    page(&app, b1, "https://b1.example/");
    let b2 = App::new_tab(&app);
    page(&app, b2, "https://b2.example/");
    assert_eq!(App::tab_ids(&app), [b0, b1, b2]);

    assert!(App::close_tab(&app, b1));
    let back = App::reopen_closed_tab(&app).expect("reopened");
    assert_eq!(App::tab_ids(&app), [b0, back, b2], "the reopened tab is not where it was");

    assert!(App::switch_workspace(&app, home));
    assert!(App::close_tab(&app, a1));
    assert!(App::switch_workspace(&app, work));
    let again = App::reopen_closed_tab(&app).expect("reopened");
    assert_eq!(app.borrow().workspaces.active().id(), home, "it reopens in the workspace it was closed from");
    assert_eq!(App::tab_ids(&app), [a0, again, a2]);
    assert_eq!(app.borrow().workspaces.get(work).unwrap().tabs(), [b0, back, b2], "Work is unchanged");
    assert_membership_is_sound(&app);
}

/// Whether a `WM_RAIL_CHANGED` was posted to this thread (the test app has no window, so its
/// posts land in the thread's queue). Takes every one of them off the queue.
fn rail_change_posted() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{MSG, PM_REMOVE, PeekMessageW};
    let mut message = MSG::default();
    let mut seen = false;
    while unsafe { PeekMessageW(&mut message, None, crate::win32::WM_RAIL_CHANGED, crate::win32::WM_RAIL_CHANGED, PM_REMOVE) }.as_bool() {
        seen = true;
    }
    seen
}

/// T8: selecting a tab of another workspace (Ctrl+Tab in MRU order, a search result) switches to
/// that workspace, so the rail is told to show it, as a switch from the rail would.
#[test]
fn selecting_a_tab_of_another_workspace_tells_the_rail() {
    let _ = rail_change_posted(); // creates the thread's queue and clears it
    let app = app_with_pending(rect(800, 600));
    let a0 = App::active_tab_id(&app);
    let home = app.borrow().workspaces.active().id();
    let _work = App::create_workspace(&app, "Work").unwrap();
    let b0 = App::active_tab_id(&app);
    let _b1 = App::new_tab(&app); // active, so b0 below is a switch within Work
    let _ = rail_change_posted();

    assert!(App::select_tab(&app, b0));
    assert!(!rail_change_posted(), "a tab of the same workspace leaves the rail alone");

    assert!(App::select_tab(&app, a0));
    assert_eq!(app.borrow().workspaces.active().id(), home);
    assert!(rail_change_posted(), "the rail still marks the old workspace");
}

/// A second window of the profile (Ctrl+N, "Move tab to new window") cannot claim the session while
/// the first holds it. It writes neither `session.txt` nor `workspaces.txt` while the first is open;
/// closed last, it saves its own tabs, which the next start restores, and keeps the workspaces.
#[test]
fn a_second_window_closed_last_saves_its_tabs_and_never_overwrites_the_first() {
    let dir = temp_dir("second-window");
    let path = dir.join(crate::workspaces::FILE_NAME);
    // The first window: its session and its workspaces, "Research" included.
    let mut saved = WorkspaceSet::new("Default", TabId(0));
    saved.create("Research", TabId(1)).unwrap();
    saved.set_session_order(&[TabId(0), TabId(1)]);
    saved.save(&path).unwrap();
    let (mut first, _) = crate::session::SessionStore::open(&dir).unwrap();
    first.begin_run(None);
    first.save(&session(&["https://first.example/", "https://research.example/"])).unwrap();
    let session_before = std::fs::read(dir.join(crate::session::FILE_NAME)).unwrap();
    let workspaces_before = std::fs::read(&path).unwrap();
    let second_window = |address: &str| {
        let app = app_with_pending(rect(800, 600));
        app.borrow_mut().workspaces_path = Some(path.clone());
        assert!(!app.borrow_mut().init_session(&dir), "the first window holds the session");
        let id = App::active_tab_id(&app);
        page(&app, id, address);
        let _ = App::create_workspace(&app, "Second only");
        App::session_save_now(&app);
        app
    };
    let early = second_window("https://early.example/");
    let last = second_window("https://last.example/");
    // Closed while the first window is open: nothing of the first window's is touched.
    App::session_finish(&early);
    assert_eq!(std::fs::read(dir.join(crate::session::FILE_NAME)).unwrap(), session_before);
    assert_eq!(std::fs::read(&path).unwrap(), workspaces_before);
    // The first window closes, then the remaining second window: its tabs are the saved session.
    first.finish(None);
    App::session_finish(&last);
    assert!(!dir.join(crate::session::LOCK_NAME).exists(), "a clean close leaves no crash marker");
    let (store, offer) = crate::session::SessionStore::open(&dir).unwrap();
    let offer = offer.expect("the next start restores the window closed last");
    assert!(!offer.safe);
    let urls: Vec<&str> = offer.session.tabs.iter().map(|tab| tab.url.as_str()).collect();
    assert_eq!(urls, ["https://last.example/"]);
    // The profile's workspaces stay; only the pairing with the first window's tabs is dropped.
    let workspaces = WorkspaceSet::load(&path).unwrap().unwrap();
    let kept: Vec<&str> = workspaces.all().iter().map(|w| w.name()).collect();
    assert_eq!(kept, ["Default", "Research"]);
    assert!(!std::fs::read_to_string(&path).unwrap().contains("order="));
    drop(store);
    drop((early, last));
    let _ = std::fs::remove_dir_all(dir);
}

/// A further window (`--new-window`) of a profile whose first window is open, showing `address`
/// and saved once.
fn further_window(dir: &std::path::Path, address: &str) -> AppRef {
    let app = app_with_pending(rect(800, 600));
    app.borrow_mut().launch_new_window = true;
    assert!(!app.borrow_mut().init_session(dir), "the first window holds the main session");
    assert!(app.borrow().window_slot.is_some(), "a further window gets a slot of its own");
    let id = App::active_tab_id(&app);
    page(&app, id, address);
    App::session_save_now(&app);
    app
}

fn pending_urls(app: &AppRef) -> Vec<String> {
    app.borrow().pending_restore.as_ref().map_or_else(Vec::new, |session| session.tabs.iter().map(|tab| tab.url.clone()).collect())
}

fn main_window(dir: &std::path::Path, address: &str) -> AppRef {
    let app = app_with_pending(rect(800, 600));
    assert!(app.borrow_mut().init_session(dir));
    let id = App::active_tab_id(&app);
    page(&app, id, address);
    App::session_save_now(&app);
    app
}

/// Windows ends the session with three windows open, one of which the person had closed first:
/// the two left open both come back, each as its own window, and the closed one does not.
#[test]
fn every_window_left_open_comes_back_and_a_window_closed_on_purpose_does_not() {
    let dir = temp_dir("windows");
    let main = main_window(&dir, "https://main.example/");
    let kept = further_window(&dir, "https://kept.example/");
    let closed = further_window(&dir, "https://closed.example/");
    let kept_slot = kept.borrow().window_slot.clone().unwrap();
    let closed_slot = closed.borrow().window_slot.clone().unwrap();
    App::session_finish(&closed);
    assert!(!crate::session::slot_dir(&dir, &closed_slot).exists(), "closed on purpose: forgotten");
    // Restart, shutdown or sign-out: the order the windows close in does not matter.
    App::session_finish_at_end_session(&main);
    App::session_finish_at_end_session(&kept);
    drop((main, kept, closed));

    let next = app_with_pending(rect(800, 600));
    assert!(next.borrow_mut().init_session(&dir));
    assert_eq!(pending_urls(&next), ["https://main.example/"]);
    assert_eq!(next.borrow().stale_window_slots, std::slice::from_ref(&kept_slot), "the other window is reopened");
    // What `reopen_other_windows` starts: `--new-window --restore-window <slot>`.
    let reopened = app_with_pending(rect(800, 600));
    reopened.borrow_mut().launch_new_window = true;
    reopened.borrow_mut().launch_restore_window = Some(kept_slot.clone());
    assert!(!reopened.borrow_mut().init_session(&dir));
    assert_eq!(reopened.borrow().window_slot.as_deref(), Some(kept_slot.as_str()));
    assert_eq!(pending_urls(&reopened), ["https://kept.example/"]);
    // A second process for the same slot gets nothing, so it opens no window.
    let twice = app_with_pending(rect(800, 600));
    twice.borrow_mut().launch_new_window = true;
    twice.borrow_mut().launch_restore_window = Some(kept_slot);
    assert!(!twice.borrow_mut().init_session(&dir));
    assert!(twice.borrow().window_slot.is_none());
    App::session_finish(&reopened);
    App::session_finish(&next);
    drop((next, reopened, twice));
    let _ = std::fs::remove_dir_all(dir);
}

/// The main window is closed while a further window stays open, and that window then stops
/// without being closed (a crash, a power cut): the next start shows the further window's tabs in
/// the first window, and the main window's tabs, closed on purpose, stay closed.
#[test]
fn a_further_window_left_open_is_restored_into_the_first_window_when_nothing_else_is() {
    let dir = temp_dir("adopt");
    let main = main_window(&dir, "https://main.example/");
    let other = further_window(&dir, "https://other.example/");
    let slot = other.borrow().window_slot.clone().unwrap();
    App::session_finish(&main);
    App::session_finish_at_end_session(&other);
    drop((main, other));

    let next = app_with_pending(rect(800, 600));
    assert!(next.borrow_mut().init_session(&dir));
    assert_eq!(pending_urls(&next), ["https://other.example/"]);
    assert!(next.borrow().stale_window_slots.is_empty());
    assert!(!crate::session::slot_dir(&dir, &slot).exists(), "the slot is now the main session");
    let saved = crate::session::Session::parse(&std::fs::read_to_string(dir.join(crate::session::FILE_NAME)).unwrap()).unwrap();
    assert_eq!(saved.tabs[0].url, "https://other.example/", "written before the slot was removed");
    App::session_finish(&next);
    drop(next);
    let _ = std::fs::remove_dir_all(dir);
}

/// A further window closed last (the main window was closed first) is the session that comes back,
/// and leaves no slot behind.
#[test]
fn a_further_window_closed_last_becomes_the_saved_session() {
    let dir = temp_dir("closed-last");
    let main = main_window(&dir, "https://main.example/");
    let other = further_window(&dir, "https://other.example/");
    let slot = other.borrow().window_slot.clone().unwrap();
    App::session_finish(&main);
    App::session_finish(&other);
    drop((main, other));
    assert!(!crate::session::slot_dir(&dir, &slot).exists());
    let (_, offer) = crate::session::SessionStore::open(&dir).unwrap();
    let urls: Vec<String> = offer.unwrap().session.tabs.into_iter().map(|tab| tab.url).collect();
    assert_eq!(urls, ["https://other.example/"]);
    let _ = std::fs::remove_dir_all(dir);
}
