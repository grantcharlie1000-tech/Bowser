//! HTML-backed side-panel content.

use std::{cell::RefCell, collections::HashMap, rc::{Rc, Weak}};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT, WPARAM},
        UI::HiDpi::GetDpiForWindow,
    },
};
use crate::{
    app::AppRef,
    bookmarks::BookmarkStore,
    chrome_host::{
        self,
        host::{Accel, AccelHandler, ChromeEnv, ChromeHost, View},
        protocol::{self, Command, PanelType},
    },
    productivity,
    profile,
    win32::{
        current_history_file, execute_history_action, history_local_offset, history_now_secs,
        HistoryAction as Action, HistoryModel as Model, HISTORY_LIMIT as LIMIT,
        side_panel::{self, PanelContent, PanelId},
        show_error_message,
    },
};

const RESIZE_STRIP_DIP: i32 = 6;

/// The WebView begins below the native header and leaves the native resize strip clear.
pub(crate) fn content_bounds(content: RECT, dpi: u32) -> RECT {
    let strip = RESIZE_STRIP_DIP.saturating_mul(dpi.max(96) as i32) / 96;
    RECT {
        right: content.right.saturating_sub(strip).max(content.left),
        bottom: content.bottom.max(content.top),
        ..content
    }
}

struct Runtime {
    app: AppRef,
    owner: HWND,
    env: ChromeEnv,
    parent: Option<HWND>,
    bounds: RECT,
    host: Option<ChromeHost>,
    started: bool,
    ready: bool,
    active: bool,
    suspended: bool,
    closed: bool,
    failed: bool,
    failed_posts: u8,
    query: String,
    bookmarks_query: String,
    panel: PanelId,
}

thread_local! {
    static RUNTIMES: RefCell<HashMap<isize, Rc<RefCell<Runtime>>>> = RefCell::new(HashMap::new());
}

/// Registers the history panel factory after the chrome environment is ready.
pub(crate) fn register(app: &AppRef, owner: HWND, env: ChromeEnv) {
    let runtime = Rc::new(RefCell::new(Runtime {
        app: app.clone(), owner, env, parent: None, bounds: RECT::default(), host: None,
        started: false, ready: false, active: false, suspended: false, closed: false, failed: false,
        failed_posts: 0, query: String::new(), bookmarks_query: String::new(), panel: PanelId::History,
    }));
    RUNTIMES.with(|runtimes| { runtimes.borrow_mut().insert(owner.0 as isize, runtime.clone()); });
    let history_runtime = runtime.clone();
    side_panel::register_content(PanelId::History, move || Box::new(HtmlPanel {
        id: PanelId::History,
        runtime: history_runtime.clone(),
    }));
    for id in [PanelId::Bookmarks, PanelId::Customize] {
        let runtime = runtime.clone();
        side_panel::register_content(id, move || Box::new(HtmlPanel { id, runtime: runtime.clone() }));
    }
}

pub(crate) fn fail(owner: HWND) {
    let runtime = RUNTIMES.with(|runtimes| runtimes.borrow_mut().remove(&(owner.0 as isize)));
    if let Some(runtime) = runtime {
        let host = runtime.try_borrow_mut().ok().map(|mut state| {
            state.closed = true;
            state.active = false;
            state.host.take()
        }).flatten();
        if let Some(host) = host { host.close(); }
    }
}

pub(crate) fn close(owner: HWND) { fail(owner); }

struct HtmlPanel {
    id: PanelId,
    runtime: Rc<RefCell<Runtime>>,
}

impl HtmlPanel {
    fn start(runtime: &Rc<RefCell<Runtime>>) {
        let (parent, bounds, env, owner) = {
            let Ok(mut state) = runtime.try_borrow_mut() else { return; };
            let Some(parent) = state.parent else { return; };
            if state.started || state.closed { return; }
            state.started = true;
            (parent, content_bounds(state.bounds, unsafe { GetDpiForWindow(parent) }), state.env.clone(), state.owner)
        };
        let weak: Weak<RefCell<Runtime>> = Rc::downgrade(runtime);
        let commands: Rc<dyn Fn(Command)> = Rc::new(move |command| {
            if let Some(runtime) = weak.upgrade() { handle_command(&runtime, command); }
        });
        let failed: Rc<dyn Fn()> = Rc::new(move || crate::win32::post(owner, super::WM_HTML_PANEL_FAILED, WPARAM(0), LPARAM(0)));
        let accel: AccelHandler = Rc::new(move |accel: Accel| super::window::html_panel_accelerator(owner, accel));
        let ready_runtime = Rc::downgrade(runtime);
        if ChromeHost::create(&env, parent, bounds, View::Panel, commands, failed, Some(accel), move |result| {
            match result {
                Ok(host) => {
                    if let Some(runtime) = ready_runtime.upgrade() {
                        let reveal = {
                            let Ok(mut state) = runtime.try_borrow_mut() else { host.close(); return; };
                            if state.closed { host.close(); return; }
                            state.host = Some(host.clone());
                            state.active
                        };
                        if reveal { show_panel(&runtime); }
                    } else { host.close(); }
                }
                Err(_) => crate::win32::post(owner, super::WM_HTML_PANEL_FAILED, WPARAM(0), LPARAM(0)),
            }
        }).is_err() {
            crate::win32::post(owner, super::WM_HTML_PANEL_FAILED, WPARAM(0), LPARAM(0));
        }
    }
}

impl PanelContent for HtmlPanel {
    fn on_host_created(&mut self, host: HWND) {
        if let Ok(mut runtime) = self.runtime.try_borrow_mut() { runtime.parent = Some(host); }
        Self::start(&self.runtime);
    }

    fn on_bounds_changed(&mut self, bounds: RECT) {
        let (host, applied) = {
            let Ok(mut state) = self.runtime.try_borrow_mut() else { return; };
            state.bounds = bounds;
            (state.host.clone(), state.parent.map(|parent| content_bounds(bounds, unsafe { GetDpiForWindow(parent) })))
        };
        if let (Some(host), Some(bounds)) = (host, applied) { if host.set_bounds(bounds).is_err() { mark_failed(&self.runtime); } }
    }

    fn on_focus(&mut self, focused: bool) {
        if focused { return; }
        suspend_panel(&self.runtime);
    }

    fn after_shown(&mut self) {
        if let Ok(mut state) = self.runtime.try_borrow_mut() { state.panel = self.id; }
        show_panel(&self.runtime);
    }
    fn title(&self) -> &str { if self.id == PanelId::History { "History" } else { "Panel" } }
}

fn show_panel(runtime: &Rc<RefCell<Runtime>>) {
    let (host, ready, suspended) = {
        let Ok(mut state) = runtime.try_borrow_mut() else { return; };
        state.active = true;
        (state.host.clone(), state.ready, state.suspended)
    };
    let Some(host) = host else { return; };
    if suspended && host.resume().is_err() {
        mark_failed(runtime);
        return;
    }
    if let Ok(mut state) = runtime.try_borrow_mut() { state.suspended = false; }
    if host.set_visible(true).is_err() || host.focus().is_err() { mark_failed(runtime); return; }
    if ready { send_current(runtime); }
}

fn suspend_panel(runtime: &Rc<RefCell<Runtime>>) {
    let (host, should_suspend) = {
        let Ok(mut state) = runtime.try_borrow_mut() else { return; };
        if state.closed { return; }
        let host = state.host.clone();
        let should_suspend = host.is_some() && state.ready && !state.suspended;
        if should_suspend { state.suspended = true; }
        (host, should_suspend)
    };
    let Some(host) = host else { return; };
    if host.set_visible(false).is_err() { mark_failed(runtime); return; }
    if should_suspend && host.try_suspend().is_err() {
        if let Ok(mut state) = runtime.try_borrow_mut() { state.suspended = false; }
    }
}

fn send_current(runtime: &Rc<RefCell<Runtime>>) {
    let (host, owner, active, panel, query, path, profile_dir, app) = {
        let Ok(state) = runtime.try_borrow() else { return; };
        if state.closed || !state.ready { return; }
        let query = if state.panel == PanelId::Bookmarks { state.bookmarks_query.clone() } else { state.query.clone() };
        (state.host.clone(), state.owner, state.active, state.panel, query, current_history_file(), profile::active().ok().map(|p| p.root().to_path_buf()), state.app.clone())
    };
    let Some(host) = host.filter(|_| active) else { return; };
    if !post(runtime, &host, &super::window::html_panel_state(owner)) { return; }
    let name = match panel { PanelId::History => "history", PanelId::Bookmarks => "bookmarks", PanelId::Customize => "customize", _ => "history" };
    let Some(panelshow) = protocol::panelshow_json(name) else { return; };
    if !post(runtime, &host, &panelshow) { return; }
    let snapshot = match panel {
        PanelId::History => {
            let entries = path.as_deref().map(|path| productivity::search_history(path, &query, LIMIT)).unwrap_or_default();
            chrome_host::panels::history::snapshot(&entries, &query, history_now_secs(), history_local_offset())
        }
        PanelId::Bookmarks => {
            let store = profile_dir.as_deref().and_then(|dir| BookmarkStore::load(dir).ok()).unwrap_or_default();
            chrome_host::panels::bookmarks::snapshot(&store, &query)
        }
        PanelId::Customize => {
            let Some(dir) = profile_dir.as_deref() else { return; };
            let sidebar = crate::sidebar::SidebarConfig::load(dir).unwrap_or_default();
            let entries = crate::app::App::workspace_entries(&app).into_iter().map(|(id, name, icon, colour, tabs, active)| (id.0, name, icon, colour, tabs, active)).collect::<Vec<_>>();
            chrome_host::panels::customize::snapshot(&sidebar, &entries)
        }
        _ => return,
    };
    let _ = post(runtime, &host, &snapshot);
}

fn post(runtime: &Rc<RefCell<Runtime>>, host: &ChromeHost, json: &str) -> bool {
    let success = host.post_json(json).is_ok();
    let failed = {
        let Ok(mut state) = runtime.try_borrow_mut() else { return success; };
        if success { state.failed_posts = 0; false }
        else { state.failed_posts = state.failed_posts.saturating_add(1); state.failed_posts >= 3 }
    };
    if failed { mark_failed(runtime); }
    success
}

fn mark_failed(runtime: &Rc<RefCell<Runtime>>) {
    let owner = {
        let Ok(mut state) = runtime.try_borrow_mut() else { return; };
        if state.failed || state.closed { return; }
        state.failed = true;
        state.owner
    };
    crate::win32::post(owner, super::WM_HTML_PANEL_FAILED, WPARAM(0), LPARAM(0));
}

fn handle_command(runtime: &Rc<RefCell<Runtime>>, command: Command) {
    match command {
        Command::Ready => {
            let active = runtime.try_borrow_mut().ok().map(|mut state| {
                state.ready = true;
                state.active
            }).unwrap_or(false);
            if active { show_panel(runtime); }
            else { suspend_panel(runtime); }
        }
        Command::Panel { kind: PanelType::Panel, f } if f.a == "dismiss" && f.id.is_none()
            && f.parent.is_none() && f.before.is_none() && f.n.is_none() && f.on.is_none()
            && f.text.is_none() && f.url.is_none() => {
            let owner = runtime.borrow().owner;
            side_panel::close(owner);
        }
        Command::Panel { kind: PanelType::History, f } => {
            let (app, owner, query) = {
                let Ok(state) = runtime.try_borrow() else { return; };
                (state.app.clone(), state.owner, state.query.clone())
            };
            let path = current_history_file();
            let entries = path.as_deref().map(|path| productivity::search_history(path, &query, LIMIT)).unwrap_or_default();
            let mut model = Model::default();
            model.set_query(&query);
            model.rebuild(&entries, history_now_secs(), history_local_offset());
            let action = chrome_host::panels::history::map_command(&mut model, f);
            if model.query != query { if let Ok(mut state) = runtime.try_borrow_mut() { state.query = model.query.clone(); } }
            let Some(action) = action else { return; };
            let dismissed = matches!(action, Action::Dismiss);
            if let Some(Err(error)) = execute_history_action(&app, owner, action) {
                show_error_message(&format!("History could not be changed: {error}"));
            }
            if !dismissed { send_current(runtime); }
        }
        Command::Panel { kind: PanelType::Bookmarks, f } => {
            let (app, owner) = {
                let Ok(state) = runtime.try_borrow() else { return; };
                (state.app.clone(), state.owner)
            };
            let Ok(profile) = profile::active() else { return; };
            let profile_dir = profile.root();
            let Ok(mut store) = BookmarkStore::load(profile_dir) else { return; };
            let Some(action) = chrome_host::panels::bookmarks::map_command(&store, f) else { return; };
            match action {
                chrome_host::panels::bookmarks::Action::Search(query) => {
                    if let Ok(mut state) = runtime.try_borrow_mut() { state.bookmarks_query = query; }
                }
                chrome_host::panels::bookmarks::Action::Open { url, new_tab } => {
                    if let Some(Err(error)) = execute_history_action(&app, owner, Action::Open { url, new_tab }) {
                        show_error_message(&format!("Bookmark could not be opened: {error}"));
                    }
                }
                chrome_host::panels::bookmarks::Action::Delete(id) => { store.delete(id); }
                chrome_host::panels::bookmarks::Action::Rename { id, title } => { let _ = store.rename(id, &title); }
                chrome_host::panels::bookmarks::Action::Update { id, title, url } => { let _ = store.update_bookmark(id, &title, &url); }
                chrome_host::panels::bookmarks::Action::AddFolder { parent, title } => { let _ = store.add_folder(parent, &title); }
                chrome_host::panels::bookmarks::Action::AddLink { parent, title, url } => { let _ = store.add_bookmark(parent, &title, &url); }
                chrome_host::panels::bookmarks::Action::Move { id, parent, index } => { let _ = store.move_item(id, parent, index); }
            }
            if let Err(error) = store.save(profile_dir) { show_error_message(&format!("Bookmarks could not be saved: {error}")); }
            send_current(runtime);
        }
        Command::Panel { kind: PanelType::Customize, f } => {
            let (app, owner) = {
                let Ok(state) = runtime.try_borrow() else { return; };
                (state.app.clone(), state.owner)
            };
            let Ok(profile) = profile::active() else { return; };
            let profile_dir = profile.root();
            let mut sidebar = crate::sidebar::SidebarConfig::load(profile_dir).unwrap_or_default();
            let Some(action) = chrome_host::panels::customize::map_command(f) else { return; };
            use chrome_host::panels::customize::Action as CustomizeAction;
            let mut changed_sidebar = false;
            match action {
                CustomizeAction::Visibility(kind, visible) => {
                    if sidebar.set_visible(kind, visible).is_ok() { changed_sidebar = true; }
                }
                CustomizeAction::AppAdd(name, url) => {
                    if sidebar.add_app(&name, &url).is_ok() {
                        if let Some(web_app) = sidebar.apps.last().cloned() { super::panel_webapp::register_app(&app, web_app); }
                        changed_sidebar = true;
                    }
                }
                CustomizeAction::AppUpdate(id, name, url) => {
                    if sidebar.apps.iter().any(|item| item.id == id)
                        && sidebar.rename_app(id, &name).is_ok() && sidebar.edit_url(id, &url).is_ok() {
                        super::panel_webapp::forget_app(id);
                        if let Some(web_app) = sidebar.apps.iter().find(|item| item.id == id).cloned() { super::panel_webapp::register_app(&app, web_app); }
                        changed_sidebar = true;
                    }
                }
                CustomizeAction::AppRemove(id) => {
                    if sidebar.remove_app(id) { super::panel_webapp::forget_app(id); changed_sidebar = true; }
                }
                CustomizeAction::AppMove(id, before) => {
                    let Some(from) = sidebar.apps.iter().position(|item| item.id == id) else { send_current(runtime); return; };
                    let mut position = before.and_then(|before| sidebar.apps.iter().position(|item| item.id == before)).unwrap_or(sidebar.apps.len());
                    if position > from { position = position.saturating_sub(1); }
                    if sidebar.reorder_app(id, position).unwrap_or(false) { changed_sidebar = true; }
                }
                CustomizeAction::WorkspaceCreate(name) => { let _ = crate::app::App::workspace_create(&app, &name); }
                CustomizeAction::WorkspaceRename(id, name) => { let _ = crate::app::App::rename_workspace(&app, crate::workspaces::WorkspaceId(id), &name); }
                CustomizeAction::WorkspaceDelete(id) => { let _ = crate::app::App::delete_workspace(&app, crate::workspaces::WorkspaceId(id)); }
                CustomizeAction::WorkspaceMove(id, before) => {
                    let workspaces = crate::app::App::workspaces(&app);
                    if let Some(from) = workspaces.all().iter().position(|item| item.id().0 == id) {
                        let mut target = before.and_then(|before| workspaces.all().iter().position(|item| item.id().0 == before)).unwrap_or(workspaces.all().len());
                        if target > from { target = target.saturating_sub(1); }
                        let _ = crate::app::App::workspace_move(&app, crate::workspaces::WorkspaceId(id), target);
                    }
                }
                CustomizeAction::WorkspaceIcon(id, icon) => { let _ = crate::app::App::workspace_set_icon(&app, crate::workspaces::WorkspaceId(id), icon); }
                CustomizeAction::WorkspaceColour(id, colour) => { let _ = crate::app::App::workspace_set_colour(&app, crate::workspaces::WorkspaceId(id), colour); }
                CustomizeAction::SwitchProfile => {
                    let _ = super::window::html_panel_accelerator(owner, Accel { vk: 0x50, ctrl: true, shift: true, alt: false, key_down: true, is_system: false, repeat: false });
                }
                CustomizeAction::ClearAppData => {
                    if crate::win32::ask_yes_no_owned(owner, "Clear the cookies, storage and cache of every pinned web app? Each one will ask you to sign in again.") {
                        if let Err(error) = super::panel_webapp::clear_all_data() { show_error_message(&format!("Web app data could not be cleared: {error}")); }
                    }
                }
            }
            if changed_sidebar {
                if let Err(error) = sidebar.save(profile_dir) { show_error_message(&format!("Sidebar changes could not be saved: {error}")); }
                crate::win32::post(owner, crate::win32::WM_RAIL_CHANGED, WPARAM(0), LPARAM(0));
            }
            send_current(runtime);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_bounds_preserve_header_offset_and_reserve_scaled_resize_strip() {
        let content = RECT { left: 0, top: 48, right: 340, bottom: 720 };
        assert_eq!(content_bounds(content, 96), RECT { right: 334, ..content });
        assert_eq!(content_bounds(content, 192), RECT { right: 328, ..content });
    }

    #[test]
    fn narrow_bounds_never_invert_the_child_rectangle() {
        let content = RECT { left: 4, top: 50, right: 7, bottom: 40 };
        assert_eq!(content_bounds(content, 96), RECT { left: 4, top: 50, right: 4, bottom: 50 });
    }
}
