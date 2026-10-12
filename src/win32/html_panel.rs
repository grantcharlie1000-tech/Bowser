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
    chrome_host::{
        self,
        host::{Accel, AccelHandler, ChromeEnv, ChromeHost, View},
        protocol::{self, Command, PanelType},
    },
    productivity,
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
}

thread_local! {
    static RUNTIMES: RefCell<HashMap<isize, Rc<RefCell<Runtime>>>> = RefCell::new(HashMap::new());
}

/// Registers the history panel factory after the chrome environment is ready.
pub(crate) fn register(app: &AppRef, owner: HWND, env: ChromeEnv) {
    let runtime = Rc::new(RefCell::new(Runtime {
        app: app.clone(), owner, env, parent: None, bounds: RECT::default(), host: None,
        started: false, ready: false, active: false, suspended: false, closed: false, failed: false,
        failed_posts: 0, query: String::new(),
    }));
    RUNTIMES.with(|runtimes| { runtimes.borrow_mut().insert(owner.0 as isize, runtime.clone()); });
    side_panel::register_content(PanelId::History, move || Box::new(HtmlPanel {
        id: PanelId::History,
        runtime: runtime.clone(),
    }));
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

    fn after_shown(&mut self) { show_panel(&self.runtime); }
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
    let (host, owner, active, query, path) = {
        let Ok(state) = runtime.try_borrow() else { return; };
        if state.closed || !state.ready { return; }
        (state.host.clone(), state.owner, state.active, state.query.clone(), current_history_file())
    };
    let Some(host) = host.filter(|_| active) else { return; };
    let entries = path.as_deref().map(|path| productivity::search_history(path, &query, LIMIT)).unwrap_or_default();
    if !post(runtime, &host, &super::window::html_panel_state(owner)) { return; }
    let Some(panelshow) = protocol::panelshow_json("history") else { return; };
    if !post(runtime, &host, &panelshow) { return; }
    let snapshot = chrome_host::panels::history::snapshot(&entries, &query, history_now_secs(), history_local_offset());
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
