//! Workspaces across a restart (T1, T2, T3), and strip moves and workspace creation that the model
//! refuses (T4, T7).

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
        let (store, _) = crate::session::SessionStore::open_with(&dir, false).unwrap();
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
