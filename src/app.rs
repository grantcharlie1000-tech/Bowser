use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use windows::{
    Win32::{Foundation::HWND, Graphics::Gdi::HFONT},
    core::{PCWSTR, Result},
};

use crate::{
    navigation::{NavigationTarget, SearchProvider, resolve_input},
    performance::{LaunchOptions, PerfLog},
    tabs::TabId,
};

use crate::{
    engine::{self, EngineAdapter, MemoryTargetApply, MemoryUsageTarget, TrackingPreventionLevel},
    win32,
};

const EDIT_HEIGHT: i32 = 28;
const INPUT_PADDING: i32 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowLayout {
    pub input_x: i32,
    pub input_y: i32,
    pub input_width: i32,
    pub input_height: i32,
    pub viewport_x: i32,
    pub viewport_y: i32,
    pub viewport_width: i32,
    pub viewport_height: i32,
}

pub fn layout_client(width: i32, height: i32, scale: u32) -> WindowLayout {
    let width = width.max(0);
    let height = height.max(0);
    let scale = scale.max(1) as i32;
    let padding = INPUT_PADDING.saturating_mul(scale);
    let input_x = padding.min(width);
    let input_y = padding.min(height);
    let input_height = EDIT_HEIGHT
        .saturating_mul(scale)
        .min(height.saturating_sub(input_y));
    let viewport_y = input_y
        .saturating_add(input_height)
        .saturating_add(padding)
        .min(height);
    WindowLayout {
        input_x,
        input_y,
        input_width: width.saturating_sub(input_x.saturating_mul(2)).max(0),
        input_height,
        viewport_x: 0,
        viewport_y,
        viewport_width: width,
        viewport_height: height.saturating_sub(viewport_y),
    }
}

pub fn should_prewarm(has_engine: bool, initializing: bool, closing: bool, text: &str) -> bool {
    !has_engine
        && !initializing
        && !closing
        && text.chars().any(|character| !character.is_whitespace())
}

pub fn tab_needs_controller(url: Option<&url::Url>) -> bool {
    url.is_some()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControllerVisibilityAction {
    Hide(TabId),
    Show(TabId),
    Close(TabId),
}

fn controller_visibility_plan(
    old: (TabId, bool),
    new: (TabId, bool),
) -> Vec<ControllerVisibilityAction> {
    if old.0 == new.0 {
        return Vec::new();
    }
    let mut actions = Vec::with_capacity(2);
    if old.1 {
        actions.push(ControllerVisibilityAction::Hide(old.0));
    }
    if new.1 {
        actions.push(ControllerVisibilityAction::Show(new.0));
    }
    actions
}

fn controller_close_plan(
    closing: TabId,
    was_active: bool,
    next: TabId,
    next_has_controller: bool,
) -> Vec<ControllerVisibilityAction> {
    let mut actions = vec![ControllerVisibilityAction::Close(closing)];
    if was_active && next_has_controller {
        actions.push(ControllerVisibilityAction::Show(next));
    }
    actions
}

fn hidden_tab_suspend_eligible(hidden_for: Duration) -> bool {
    hidden_for >= Duration::from_secs(30)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowMemoryTransition {
    pub minimized: bool,
    pub resize_bounds: bool,
    pub target: Option<MemoryUsageTarget>,
}

pub fn window_memory_transition(was_minimized: bool, is_minimized: bool) -> WindowMemoryTransition {
    match (was_minimized, is_minimized) {
        (false, true) => WindowMemoryTransition {
            minimized: true,
            resize_bounds: false,
            target: Some(MemoryUsageTarget::Low),
        },
        (true, false) => WindowMemoryTransition {
            minimized: false,
            resize_bounds: true,
            target: Some(MemoryUsageTarget::Normal),
        },
        (false, false) => WindowMemoryTransition {
            minimized: false,
            resize_bounds: true,
            target: None,
        },
        (true, true) => WindowMemoryTransition {
            minimized: true,
            resize_bounds: false,
            target: None,
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdleTimerAction {
    Kill,
    Arm(u32),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct NavigationProgress {
    navigation_id: Option<u64>,
    percent: Option<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NavigationStage {
    ContentLoading,
    DomContentLoaded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NavigationProgressEvent {
    Started(u64),
    Advanced(u64, NavigationStage),
    Completed(u64),
}

impl NavigationStage {
    fn percent(self) -> u8 {
        match self {
            Self::ContentLoading => 45,
            Self::DomContentLoaded => 75,
        }
    }
}

impl NavigationProgress {
    fn apply(&mut self, event: NavigationProgressEvent) -> bool {
        let (navigation_id, percent) = match event {
            NavigationProgressEvent::Started(id) => (id, 15),
            NavigationProgressEvent::Advanced(id, stage) => (id, stage.percent()),
            NavigationProgressEvent::Completed(id) => (id, 100),
        };
        if matches!(event, NavigationProgressEvent::Started(_)) {
            if self.navigation_id == Some(navigation_id) {
                return false;
            }
            let next = Self {
                navigation_id: Some(navigation_id),
                percent: Some(percent),
            };
            let changed = *self != next;
            *self = next;
            return changed;
        }
        if self.navigation_id != Some(navigation_id)
            || self.percent.is_none_or(|current| percent <= current)
        {
            return false;
        }
        self.percent = Some(percent);
        true
    }

    fn clear_after_completion_paint(&mut self) -> bool {
        if self.percent == Some(100) {
            *self = Self::default();
            true
        } else {
            false
        }
    }

    fn reset(&mut self) -> bool {
        let changed = *self != Self::default();
        *self = Self::default();
        changed
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdleMemoryDecision {
    pub target: MemoryUsageTarget,
    pub timer: IdleTimerAction,
}

pub fn idle_memory_decision(
    minimized: bool,
    page_loaded: bool,
    idle_ms: u32,
    current_target: MemoryUsageTarget,
) -> IdleMemoryDecision {
    if minimized {
        return IdleMemoryDecision {
            target: MemoryUsageTarget::Low,
            timer: IdleTimerAction::Kill,
        };
    }
    if !page_loaded {
        return IdleMemoryDecision {
            target: MemoryUsageTarget::Normal,
            timer: IdleTimerAction::Kill,
        };
    }
    if idle_ms == 0 && current_target == MemoryUsageTarget::Normal {
        return IdleMemoryDecision {
            target: MemoryUsageTarget::Normal,
            timer: IdleTimerAction::Arm(3_000),
        };
    }
    if current_target == MemoryUsageTarget::Low {
        if idle_ms <= 1_000 {
            return IdleMemoryDecision {
                target: MemoryUsageTarget::Normal,
                timer: IdleTimerAction::Arm(3_000),
            };
        }
        return IdleMemoryDecision {
            target: MemoryUsageTarget::Low,
            timer: IdleTimerAction::Arm(1_000),
        };
    }
    IdleMemoryDecision {
        target: if idle_ms >= 3_000 {
            MemoryUsageTarget::Low
        } else {
            MemoryUsageTarget::Normal
        },
        timer: IdleTimerAction::Arm(1_000),
    }
}

fn valid_viewport_bounds(bounds: windows::Win32::Foundation::RECT) -> bool {
    bounds.right > bounds.left && bounds.bottom > bounds.top
}

#[derive(Debug)]
pub enum AppError {
    Windows(windows::core::Error),
    Navigation(String),
    Io(std::io::Error),
}

impl From<windows::core::Error> for AppError {
    fn from(value: windows::core::Error) -> Self {
        Self::Windows(value)
    }
}
impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub struct App {
    pub hwnd: HWND,
    pub edit: HWND,
    pub input_font: HFONT,
    pub engine: Option<Box<dyn engine::EngineAdapter>>,
    pub pending: Option<url::Url>,
    pending_tab: Option<TabId>,
    pub initializing: bool,
    pub closing: bool,
    pub loop_shutdown_pending: bool,
    pub user_data_dir: PathBuf,
    pub benchmark_url: Option<url::Url>,
    pub allowed_benchmark_file: Option<url::Url>,
    pub benchmark_tracking_prevention: Option<TrackingPreventionLevel>,
    pub provider: SearchProvider,
    pub log: Rc<PerfLog>,
    pub minimized: bool,
    pub desired_memory_target: MemoryUsageTarget,
    pub effective_memory_target: Option<MemoryUsageTarget>,
    pub memory_target_unsupported_logged: bool,
    pub last_valid_viewport_bounds: Option<windows::Win32::Foundation::RECT>,
    pub has_navigated: bool,
    pub page_loaded: bool,
    navigation_progress: NavigationProgress,
    tabs: Vec<AppTab>,
    tab_model: crate::tabs::TabList,
}

struct AppTab {
    id: TabId,
    url: Option<url::Url>,
    title: String,
    loading: bool,
    progress: NavigationProgress,
    favicon: Option<Vec<u8>>,
    can_back: bool,
    can_forward: bool,
    page_usable: bool,
    controller_created: bool,
    hidden_since: Option<Instant>,
    suspended: bool,
}

impl AppTab {
    fn empty(id: TabId) -> Self {
        Self {
            id,
            url: None,
            title: String::new(),
            loading: false,
            progress: NavigationProgress::default(),
            favicon: None,
            can_back: false,
            can_forward: false,
            page_usable: false,
            controller_created: false,
            hidden_since: None,
            suspended: false,
        }
    }
}

pub type AppRef = Rc<RefCell<App>>;

impl App {
    fn with_engine(
        app: &AppRef,
        operation: impl FnOnce(&mut dyn engine::EngineAdapter) -> windows::core::Result<()>,
    ) -> Option<windows::core::Result<()>> {
        let mut current = app.borrow_mut().engine.take()?;
        let result = operation(current.as_mut());
        let mut state = app.borrow_mut();
        if state.closing {
            let _ = current.close();
        } else {
            state.engine = Some(current);
        }
        Some(result)
    }

    fn sync_window_title(app: &AppRef) {
        let (hwnd, title) = {
            let state = app.borrow();
            let active = state.tab_model.active();
            let title = state
                .tabs
                .iter()
                .find(|tab| tab.id == active)
                .map(|tab| tab.title.trim())
                .filter(|title| !title.is_empty())
                .unwrap_or("Bowser");
            (state.hwnd, title.to_owned())
        };
        let mut title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(
                hwnd,
                PCWSTR(title.as_mut_ptr()),
            );
        }
    }

    fn post_tab_changed(hwnd: HWND, id: TabId) {
        win32::post(
            hwnd,
            win32::WM_TABS_CHANGED,
            windows::Win32::Foundation::WPARAM(id.0 as usize),
            windows::Win32::Foundation::LPARAM(0),
        );
    }

    pub fn new_tab(app: &AppRef) -> TabId {
        let (id, hwnd, old, old_has_controller) = {
            let mut state = app.borrow_mut();
            let old = state.tab_model.active();
            let old_has_controller = state
                .tabs
                .iter()
                .find(|tab| tab.id == old)
                .is_some_and(|tab| tab.controller_created);
            let id = state.tab_model.add(false);
            state.tab_model.select(id);
            state.tabs.push(AppTab::empty(id));
            state.navigation_progress.reset();
            state.page_loaded = false;
            state.desired_memory_target = MemoryUsageTarget::Normal;
            (id, state.hwnd, old, old_has_controller)
        };
        let now = Instant::now();
        let actions = controller_visibility_plan((old, old_has_controller), (id, false));
        {
            let mut state = app.borrow_mut();
            for action in actions {
                if let ControllerVisibilityAction::Hide(hidden) = action
                    && let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == hidden)
                {
                    tab.hidden_since = Some(now);
                    tab.suspended = false;
                }
            }
        }
        Self::with_engine(app, |engine| {
            engine.select_tab(id)?;
            let _ = engine.set_memory_usage_target(MemoryUsageTarget::Normal);
            Ok(())
        });
        Self::post_tab_changed(hwnd, id);
        Self::sync_window_title(app);
        Self::schedule_tab_idle_timer(app);
        id
    }

    pub fn close_tab(app: &AppRef, id: TabId) -> bool {
        let (changed, hwnd, active, was_active, last_tab) = {
            let mut state = app.borrow_mut();
            let Some(index) = state.tabs.iter().position(|tab| tab.id == id) else {
                return false;
            };
            if state.tabs.len() == 1 {
                let outcome = state.tab_model.close(id);
                state.tabs[0] = AppTab::empty(outcome.new_active);
                state.navigation_progress.reset();
                state.pending = None;
                state.pending_tab = None;
                state.has_navigated = false;
                state.page_loaded = false;
                (true, state.hwnd, outcome.new_active, true, true)
            } else {
                let was_active = state.tab_model.active() == id;
                let outcome = state.tab_model.close(id);
                state.tabs.remove(index);
                if was_active {
                    let (progress, page_usable) = state
                        .tabs
                        .iter()
                        .find(|tab| tab.id == outcome.new_active)
                        .map(|tab| (tab.progress, tab.page_usable))
                        .unwrap();
                    state.navigation_progress = progress;
                    state.page_loaded = page_usable;
                    state.desired_memory_target = MemoryUsageTarget::Normal;
                }
                (true, state.hwnd, outcome.new_active, was_active, false)
            }
        };
        let next_has_controller = app
            .borrow()
            .tabs
            .iter()
            .find(|tab| tab.id == active)
            .is_some_and(|tab| tab.controller_created);
        let close_plan = controller_close_plan(id, was_active, active, next_has_controller);
        {
            let mut state = app.borrow_mut();
            for action in close_plan {
                if let ControllerVisibilityAction::Show(shown) = action
                    && let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == shown)
                {
                    tab.hidden_since = None;
                    tab.suspended = false;
                }
            }
            if state.pending_tab == Some(id) {
                state.pending = None;
                state.pending_tab = None;
            }
        }
        Self::with_engine(app, |engine| {
            let closed = engine.close_tab(id);
            if was_active {
                let _ = engine.select_tab(active);
                let _ = engine.set_memory_usage_target(MemoryUsageTarget::Normal);
            }
            closed
        });
        if last_tab {
            app.borrow_mut().page_loaded = false;
        }
        if changed {
            Self::post_tab_changed(hwnd, active);
        }
        Self::sync_window_title(app);
        Self::schedule_tab_idle_timer(app);
        changed
    }

    pub fn select_tab(app: &AppRef, id: TabId) -> bool {
        let (hwnd, previous, old_has_controller, new_has_controller) = {
            let mut state = app.borrow_mut();
            let previous = state.tab_model.active();
            let was_active = state.tab_model.active() == id;
            if !state.tab_model.select(id) {
                return false;
            }
            if was_active {
                return true;
            }
            let (progress, page_usable, new_has_controller) = state
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| (tab.progress, tab.page_usable, tab.controller_created))
                .unwrap();
            state.navigation_progress = progress;
            state.page_loaded = page_usable;
            let old_has_controller = state
                .tabs
                .iter()
                .find(|tab| tab.id == previous)
                .is_some_and(|tab| tab.controller_created);
            state.desired_memory_target = MemoryUsageTarget::Normal;
            (state.hwnd, previous, old_has_controller, new_has_controller)
        };
        let actions =
            controller_visibility_plan((previous, old_has_controller), (id, new_has_controller));
        let now = Instant::now();
        {
            let mut state = app.borrow_mut();
            for action in actions {
                match action {
                    ControllerVisibilityAction::Hide(hidden) => {
                        if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == hidden) {
                            tab.hidden_since = Some(now);
                            tab.suspended = false;
                        }
                    }
                    ControllerVisibilityAction::Show(shown) => {
                        if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == shown) {
                            tab.hidden_since = None;
                            tab.suspended = false;
                        }
                    }
                    ControllerVisibilityAction::Close(_) => {}
                }
            }
        }
        Self::with_engine(app, |engine| {
            engine.select_tab(id)?;
            let _ = engine.set_memory_usage_target(MemoryUsageTarget::Normal);
            Ok(())
        });
        Self::post_tab_changed(hwnd, id);
        Self::sync_window_title(app);
        Self::schedule_tab_idle_timer(app);
        true
    }

    pub fn select_next(app: &AppRef) -> TabId {
        Self::select_relative(app, 1)
    }

    pub fn select_prev(app: &AppRef) -> TabId {
        Self::select_relative(app, -1)
    }

    fn select_relative(app: &AppRef, delta: isize) -> TabId {
        let (next, hwnd, previous, old_has_controller, new_has_controller) = {
            let mut state = app.borrow_mut();
            let previous = state.tab_model.active();
            if delta > 0 {
                state.tab_model.select_next();
            } else {
                state.tab_model.select_prev();
            }
            let id = state.tab_model.active();
            state.navigation_progress =
                state.tabs.iter().find(|tab| tab.id == id).unwrap().progress;
            let (page_usable, new_has_controller) = state
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| (tab.page_usable, tab.controller_created))
                .unwrap();
            state.page_loaded = page_usable;
            state.desired_memory_target = MemoryUsageTarget::Normal;
            let old_has_controller = state
                .tabs
                .iter()
                .find(|tab| tab.id == previous)
                .is_some_and(|tab| tab.controller_created);
            (
                id,
                state.hwnd,
                previous,
                old_has_controller,
                new_has_controller,
            )
        };
        let actions =
            controller_visibility_plan((previous, old_has_controller), (next, new_has_controller));
        let now = Instant::now();
        {
            let mut state = app.borrow_mut();
            for action in actions {
                match action {
                    ControllerVisibilityAction::Hide(hidden) => {
                        if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == hidden) {
                            tab.hidden_since = Some(now);
                            tab.suspended = false;
                        }
                    }
                    ControllerVisibilityAction::Show(shown) => {
                        if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == shown) {
                            tab.hidden_since = None;
                            tab.suspended = false;
                        }
                    }
                    ControllerVisibilityAction::Close(_) => {}
                }
            }
        }
        Self::with_engine(app, |engine| {
            engine.select_tab(next)?;
            let _ = engine.set_memory_usage_target(MemoryUsageTarget::Normal);
            Ok(())
        });
        Self::post_tab_changed(hwnd, next);
        Self::sync_window_title(app);
        Self::schedule_tab_idle_timer(app);
        next
    }

    pub fn move_tab(app: &AppRef, id: TabId, new_index: usize) -> bool {
        let hwnd = {
            let mut state = app.borrow_mut();
            let Some(index) = state.tabs.iter().position(|tab| tab.id == id) else {
                return false;
            };
            if !state.tab_model.move_tab(id, new_index) {
                return false;
            }
            let tab = state.tabs.remove(index);
            let target = state
                .tab_model
                .index_of(id)
                .unwrap_or(new_index)
                .min(state.tabs.len());
            state.tabs.insert(target, tab);
            state.hwnd
        };
        Self::post_tab_changed(hwnd, id);
        true
    }

    pub fn tab_count(app: &AppRef) -> usize {
        app.borrow().tab_model.len()
    }

    pub fn reload(app: &AppRef) {
        if let Some(Err(error)) = Self::with_engine(app, |engine| engine.reload()) {
            win32::show_error_message(&format!("Reload failed: {error}"));
        }
    }

    pub fn stop(app: &AppRef) {
        if let Some(Err(error)) = Self::with_engine(app, |engine| engine.stop()) {
            win32::show_error_message(&format!("Stopping navigation failed: {error}"));
        }
    }

    pub fn can_go_back(app: &AppRef) -> bool {
        app.borrow()
            .engine
            .as_ref()
            .is_some_and(|engine| engine.can_go_back())
    }

    pub fn can_go_forward(app: &AppRef) -> bool {
        app.borrow()
            .engine
            .as_ref()
            .is_some_and(|engine| engine.can_go_forward())
    }

    pub fn active_tab_id(app: &AppRef) -> TabId {
        app.borrow().tab_model.active()
    }

    pub fn tab_title(app: &AppRef, id: TabId) -> Option<String> {
        app.borrow()
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| tab.title.clone())
    }

    pub fn tab_loading(app: &AppRef, id: TabId) -> bool {
        app.borrow()
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .is_some_and(|tab| tab.loading)
    }

    pub fn tab_can_go_back(app: &AppRef, id: TabId) -> bool {
        app.borrow()
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .is_some_and(|tab| tab.can_back)
    }

    pub fn tab_can_go_forward(app: &AppRef, id: TabId) -> bool {
        app.borrow()
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .is_some_and(|tab| tab.can_forward)
    }

    pub fn tab_ids(app: &AppRef) -> Vec<TabId> {
        app.borrow().tab_model.iter().map(|tab| tab.id).collect()
    }

    pub fn tab_progress_percent(app: &AppRef, id: TabId) -> Option<u8> {
        app.borrow()
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .and_then(|tab| tab.progress.percent)
    }

    pub fn tab_favicon(app: &AppRef, id: TabId) -> Option<Vec<u8>> {
        app.borrow()
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .and_then(|tab| tab.favicon.clone())
    }

    fn apply_tab_progress(app: &AppRef, id: TabId, event: NavigationProgressEvent) -> bool {
        let mut state = app.borrow_mut();
        let active = state.tab_model.active() == id;
        let (changed, progress, loading) = {
            let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) else {
                return false;
            };
            let changed = tab.progress.apply(event);
            match event {
                NavigationProgressEvent::Started(_) => tab.loading = true,
                NavigationProgressEvent::Completed(_) => tab.loading = false,
                NavigationProgressEvent::Advanced(_, _) => {}
            }
            (changed, tab.progress, tab.loading)
        };
        state.tab_model.set_loading(id, loading);
        if active {
            state.navigation_progress = progress;
        }
        changed
    }

    pub fn active_progress_percent(app: &AppRef) -> Option<u8> {
        let state = app.borrow();
        state
            .tabs
            .iter()
            .find(|tab| tab.id == state.tab_model.active())
            .and_then(|tab| tab.progress.percent)
    }

    pub fn run(options: LaunchOptions, log: Rc<PerfLog>) -> Result<()> {
        for variable in [
            "WEBVIEW2_USER_DATA_FOLDER",
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
            "WEBVIEW2_RELEASE_CHANNEL_PREFERENCE",
        ] {
            if std::env::var_os(variable).is_some() {
                win32::show_error_message(&format!(
                    "Bowser cannot start while {variable} is set. This variable can override Bowser's isolated WebView2 profile or browser security settings. Remove it and try again."
                ));
                return Ok(());
            }
        }
        let Some(app_data) = local_app_data_dir() else {
            win32::show_error_message(
                "Bowser could not find this user's LocalAppData directory, which is required for its private WebView2 profile.",
            );
            return Ok(());
        };
        let provider = SearchProvider::from_config(&app_data.join("search-provider.txt"));
        let user_data_dir = options
            .user_data_dir
            .as_ref()
            .cloned()
            .unwrap_or_else(|| app_data.join("WebView2"));
        if let Err(error) = engine::WebView2Engine::availability() {
            win32::show_runtime_error(&error);
            return Ok(());
        }
        win32::run(Self::from_launch_options(
            options,
            user_data_dir,
            provider,
            log,
        ))
    }

    fn from_launch_options(
        options: LaunchOptions,
        user_data_dir: PathBuf,
        provider: SearchProvider,
        log: Rc<PerfLog>,
    ) -> Self {
        let benchmark_url = options.benchmark_url;
        let allowed_benchmark_file = benchmark_url.clone();
        let tab_model = crate::tabs::TabList::new();
        let initial_tab = tab_model.active();
        Self {
            hwnd: HWND::default(),
            edit: HWND::default(),
            input_font: HFONT::default(),
            engine: None,
            pending: None,
            pending_tab: None,
            initializing: false,
            closing: false,
            loop_shutdown_pending: false,
            user_data_dir,
            allowed_benchmark_file,
            benchmark_url,
            benchmark_tracking_prevention: options.benchmark_tracking_prevention,
            provider,
            log,
            minimized: false,
            desired_memory_target: MemoryUsageTarget::Normal,
            effective_memory_target: None,
            memory_target_unsupported_logged: false,
            last_valid_viewport_bounds: None,
            has_navigated: false,
            page_loaded: false,
            navigation_progress: NavigationProgress::default(),
            tabs: vec![AppTab::empty(initial_tab)],
            tab_model,
        }
    }

    pub fn submit(app: &AppRef, input: &str) {
        let target = {
            let state = app.borrow();
            match resolve_input(input, &state.provider) {
                Ok(target) => target,
                Err(error) => {
                    drop(state);
                    win32::show_error_message(&format!(
                        "This address or search could not be used: {error}"
                    ));
                    return;
                }
            }
        };
        let url = match target {
            NavigationTarget::Empty => return,
            NavigationTarget::Web(url) | NavigationTarget::Search { url, .. } => url,
        };
        Self::submit_url(app, url, None);
    }

    pub fn submit_benchmark_url(app: &AppRef, url: url::Url) {
        Self::submit_url(app, url.clone(), Some(url));
    }

    fn submit_url(app: &AppRef, url: url::Url, allowed_file: Option<url::Url>) {
        {
            let mut state = app.borrow_mut();
            let _ = state.log.record("submitted");
            if state.engine.is_some() {
                let active_tab = state.tab_model.active();
                if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == active_tab) {
                    tab.url = Some(url.clone());
                    tab.controller_created = tab_needs_controller(Some(&url));
                    tab.page_usable = false;
                    tab.hidden_since = None;
                    tab.suspended = false;
                }
                state.tab_model.set_url(active_tab, Some(url.as_str()));
                if url.scheme() != "file" {
                    state.allowed_benchmark_file = None;
                }
                let repeat_boundary = state.has_navigated && state.log.enabled();
                drop(state);
                Self::navigation_started(app);
                if repeat_boundary {
                    let _ = app.borrow().log.record("repeat_navigation_boundary");
                }
                Self::navigate_current(app, &url);
                return;
            }
            state.allowed_benchmark_file = allowed_file;
            state.pending = Some(url.clone());
            state.pending_tab = Some(state.tab_model.active());
            if state.initializing {
                return;
            }
            state.initializing = true;
        }
        Self::start_engine_creation(app);
    }

    pub fn prewarm(app: &AppRef, text: &str) {
        {
            let mut state = app.borrow_mut();
            if !should_prewarm(
                state.engine.is_some(),
                state.initializing,
                state.closing,
                text,
            ) {
                return;
            }
            state.initializing = true;
            let _ = state.log.record("engine_prewarm_started");
        }
        Self::start_engine_creation(app);
    }

    fn start_engine_creation(app: &AppRef) {
        let weak = Rc::downgrade(app);
        let bounds = app.borrow_mut().latest_viewport_bounds();
        let (hwnd, profile, tracking_prevention) = {
            let s = app.borrow();
            (
                s.hwnd,
                s.user_data_dir.clone(),
                s.benchmark_tracking_prevention,
            )
        };
        let events_app = Rc::downgrade(app);
        let title_events_app = Rc::downgrade(app);
        let nav_progress_events_app = Rc::downgrade(app);
        let favicon_events_app = Rc::downgrade(app);
        let progress_hwnd = hwnd;
        let events = engine::EngineEvents {
            address_changed: Some(Rc::new(move |tab_id, url| {
                if let Some(app) = events_app.upgrade() {
                    let (edit, log, active, found) = {
                        let mut state = app.borrow_mut();
                        let active = state.tab_model.active() == tab_id;
                        let found =
                            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                                tab.url = Some(url.clone());
                                tab.can_back = true;
                                true
                            } else {
                                false
                            };
                        if found {
                            state.tab_model.set_url(tab_id, Some(url.as_str()));
                        }
                        (state.edit, state.log.clone(), active, found)
                    };
                    if !found {
                        return;
                    }
                    if active {
                        let event = if win32::set_input_text(edit, url.as_str()) {
                            "address_updated"
                        } else {
                            "address_update_failed"
                        };
                        let _ = log.record(event);
                    }
                    Self::post_tab_changed(app.borrow().hwnd, tab_id);
                }
            })),
            title_changed: Some(Rc::new(move |tab_id, title| {
                if let Some(app) = title_events_app.upgrade() {
                    let mut state = app.borrow_mut();
                    let found =
                        if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                            tab.title.clone_from(&title);
                            true
                        } else {
                            false
                        };
                    if found {
                        state.tab_model.set_title(tab_id, &title);
                        let active = state.tab_model.active() == tab_id;
                        let hwnd = state.hwnd;
                        drop(state);
                        if active {
                            Self::sync_window_title(&app);
                        }
                        Self::post_tab_changed(hwnd, tab_id);
                    }
                }
            })),
            page_loaded: {
                let loaded_events_app = Rc::downgrade(app);
                Some(Rc::new(move |tab_id| {
                    if let Some(app) = loaded_events_app.upgrade() {
                        App::tab_page_loaded(&app, tab_id);
                    }
                }))
            },
            page_usable: {
                let events_app = Rc::downgrade(app);
                Some(Rc::new(move |tab_id| {
                    if let Some(app) = events_app.upgrade() {
                        let active = {
                            let mut state = app.borrow_mut();
                            let active = state.tab_model.active() == tab_id;
                            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                                tab.page_usable = true;
                                tab.loading = false;
                            } else {
                                return;
                            }
                            if active {
                                state.page_loaded = true;
                            }
                            active
                        };
                        if active {
                            let action = App::page_usable(&app);
                            let action = App::combine_idle_and_suspend_timers(&app, action);
                            win32::schedule_idle_timer(app.borrow().hwnd, action);
                        } else {
                            Self::post_tab_changed(app.borrow().hwnd, tab_id);
                        }
                    }
                }))
            },
            navigation_started: {
                let events_app = Rc::downgrade(app);
                let progress_hwnd = hwnd;
                Some(Rc::new(move |tab_id, navigation_id| {
                    if let Some(app) = events_app.upgrade() {
                        if let Some(tab) = app
                            .borrow_mut()
                            .tabs
                            .iter_mut()
                            .find(|tab| tab.id == tab_id)
                        {
                            tab.page_usable = false;
                            tab.loading = true;
                        }
                        let found = Self::apply_tab_progress(
                            &app,
                            tab_id,
                            NavigationProgressEvent::Started(navigation_id),
                        );
                        if found && app.borrow().tab_model.active() == tab_id {
                            win32::post(
                                progress_hwnd,
                                win32::WM_NAVIGATION_PROGRESS,
                                windows::Win32::Foundation::WPARAM(navigation_id as usize),
                                windows::Win32::Foundation::LPARAM(0),
                            );
                            let action = App::navigation_started(&app);
                            let action = App::combine_idle_and_suspend_timers(&app, action);
                            win32::schedule_idle_timer(app.borrow().hwnd, action);
                        }
                        if found {
                            Self::post_tab_changed(app.borrow().hwnd, tab_id);
                        }
                    }
                }))
            },
            navigation_progress: Some(Rc::new(move |tab_id, event| {
                let (navigation_id, stage) = match event {
                    engine::NavigationProgressEvent::ContentLoading(id) => (id, 1),
                    engine::NavigationProgressEvent::DomContentLoaded(id) => (id, 2),
                    engine::NavigationProgressEvent::Completed(id) => (id, 3),
                };
                if let Some(app) = nav_progress_events_app.upgrade() {
                    let active = app.borrow().tab_model.active() == tab_id;
                    let progress_event = match stage {
                        1 => NavigationProgressEvent::Advanced(
                            navigation_id,
                            NavigationStage::ContentLoading,
                        ),
                        2 => NavigationProgressEvent::Advanced(
                            navigation_id,
                            NavigationStage::DomContentLoaded,
                        ),
                        _ => NavigationProgressEvent::Completed(navigation_id),
                    };
                    let found = Self::apply_tab_progress(&app, tab_id, progress_event);
                    if found && active {
                        win32::post(
                            progress_hwnd,
                            win32::WM_NAVIGATION_PROGRESS,
                            windows::Win32::Foundation::WPARAM(navigation_id as usize),
                            windows::Win32::Foundation::LPARAM(stage),
                        );
                    }
                    if found {
                        Self::post_tab_changed(app.borrow().hwnd, tab_id);
                    }
                }
            })),
            favicon_changed: Some(Rc::new(move |tab_id, bytes| {
                if let Some(app) = favicon_events_app.upgrade() {
                    let found = if let Some(tab) = app
                        .borrow_mut()
                        .tabs
                        .iter_mut()
                        .find(|tab| tab.id == tab_id)
                    {
                        tab.favicon = Some(bytes);
                        true
                    } else {
                        false
                    };
                    if found {
                        Self::post_tab_changed(app.borrow().hwnd, tab_id);
                    }
                }
            })),
            new_window_requested: {
                let popup_app = Rc::downgrade(app);
                Some(Rc::new(move |source_tab, url| {
                    if let Some(app) = popup_app.upgrade() {
                        if app.borrow().tabs.iter().any(|tab| tab.id == source_tab) {
                            App::new_tab(&app);
                            App::submit_url(&app, url, None);
                        }
                    }
                }))
            },
            failed: Some(Rc::new(|error| {
                win32::show_error_message(&format!("WebView2 error: {error}"))
            })),
            policy_event: {
                let log = app.borrow().log.clone();
                Some(Rc::new(move |event| {
                    if log.enabled() {
                        let _ = log.record(event);
                    }
                }))
            },
        };
        let allowed_file = app.borrow().allowed_benchmark_file.clone();
        let create_result = engine::WebView2Engine::begin_create_with_tracking_policy(
            hwnd,
            bounds,
            &profile,
            allowed_file,
            events,
            tracking_prevention,
            move |result| {
                if let Some(app) = weak.upgrade() {
                    let mut state = app.borrow_mut();
                    state.initializing = false;
                    if state.closing {
                        let post_quit = state.loop_shutdown_pending;
                        drop(state);
                        if let Ok(mut engine) = result {
                            let _ = engine.close();
                        }
                        if post_quit {
                            unsafe {
                                windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
                            }
                        }
                        return;
                    }
                    match result {
                        Ok(engine) => {
                            drop(state);
                            App::complete_engine_creation(&app, Box::new(engine));
                        }
                        Err(error) => {
                            drop(state);
                            win32::show_error_message(&format!(
                                "WebView2 could not create its isolated profile or browser controller: {error}"
                            ));
                        }
                    }
                } else if let Ok(mut engine) = result {
                    let _ = engine.close();
                }
            },
        );
        if let Err(error) = create_result {
            app.borrow_mut().initializing = false;
            win32::show_error_message(&format!(
                "WebView2 initialization could not be started: {error}"
            ));
        }
    }

    fn complete_engine_creation(app: &AppRef, mut engine: Box<dyn EngineAdapter>) {
        let (bounds, target, pending, pending_tab, active_tab, tab_ids) = {
            let mut state = app.borrow_mut();
            state.initializing = false;
            if state.closing {
                let post_quit = state.loop_shutdown_pending;
                drop(state);
                let _ = engine.close();
                if post_quit {
                    unsafe {
                        windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
                    }
                }
                return;
            }
            let _ = state.log.record("engine_ready");
            (
                state.last_valid_viewport_bounds,
                state.desired_memory_target,
                state.pending.take(),
                state.pending_tab.take(),
                state.tab_model.active(),
                state.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(),
            )
        };
        if let Some(bounds) = bounds {
            if engine.resize(bounds).is_err() {
                let _ = app.borrow().log.record("engine_ready_resize_failed");
            }
        }
        // The engine learns about logical tabs through select_tab. Register tabs that
        // existed while the shared environment was initializing before replaying a
        // pending navigation, which may belong to a now-background tab.
        for tab_id in tab_ids.into_iter().filter(|tab_id| *tab_id != active_tab) {
            if engine.select_tab(tab_id).is_err() {
                let _ = app.borrow().log.record("engine_ready_select_tab_failed");
            }
        }
        if engine.select_tab(active_tab).is_err() {
            let _ = app.borrow().log.record("engine_ready_select_tab_failed");
        }
        let apply = engine.set_memory_usage_target(target);
        Self::record_memory_target_result(app, target, &apply);
        let mut state = app.borrow_mut();
        if state.closing {
            let post_quit = state.loop_shutdown_pending;
            drop(state);
            let _ = engine.close();
            if post_quit {
                unsafe {
                    windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
                }
            }
            return;
        }
        state.effective_memory_target = match apply {
            Ok(MemoryTargetApply::Applied) => Some(target),
            Ok(MemoryTargetApply::Unsupported | MemoryTargetApply::Deferred) | Err(_) => None,
        };
        state.engine = Some(engine);
        drop(state);
        if let Some(pending) = pending {
            Self::navigate_tab(app, pending_tab.unwrap_or(active_tab), &pending);
        }
    }

    pub fn navigate_current(app: &AppRef, url: &url::Url) {
        let active = app.borrow().tab_model.active();
        Self::navigate_tab(app, active, url);
    }

    fn navigate_tab(app: &AppRef, tab_id: TabId, url: &url::Url) {
        let (mut engine, closing) = {
            let mut state = app.borrow_mut();
            let is_background = state.tab_model.active() != tab_id;
            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                tab.url = Some(url.clone());
                tab.controller_created = tab_needs_controller(Some(url));
                tab.page_usable = false;
                tab.loading = true;
                tab.hidden_since = is_background.then(Instant::now);
                tab.suspended = false;
            }
            state.tab_model.set_url(tab_id, Some(url.as_str()));
            state.has_navigated = true;
            (state.engine.take(), state.closing)
        };
        if closing {
            if let Some(engine) = engine.as_mut() {
                let _ = engine.close();
            }
            return;
        }
        let Some(mut engine) = engine.take() else {
            return;
        };
        let result = engine.navigate_tab(tab_id, url);
        let mut state = app.borrow_mut();
        if state.tab_model.active() == tab_id {
            state.page_loaded = false;
        }
        if state.closing {
            let _ = engine.close();
        } else {
            state.engine = Some(engine);
        }
        drop(state);
        if let Err(error) = result {
            win32::show_error_message(&format!("Navigation failed: {error}"));
        }
        Self::schedule_tab_idle_timer(app);
    }

    pub fn resize_current(app: &AppRef, bounds: windows::Win32::Foundation::RECT) {
        let Some(mut engine) = app.borrow_mut().engine.take() else {
            return;
        };
        let result = engine.resize(bounds);
        let mut state = app.borrow_mut();
        if state.closing {
            let _ = engine.close();
        } else {
            state.engine = Some(engine);
        }
        if let Err(error) = result {
            win32::show_error_message(&format!("WebView2 resize failed: {error}"));
        }
    }

    pub fn window_size_changed(
        app: &AppRef,
        is_minimized: bool,
        bounds: windows::Win32::Foundation::RECT,
    ) -> WindowMemoryTransition {
        let (transition, resize_bounds) = {
            let mut state = app.borrow_mut();
            let transition = window_memory_transition(state.minimized, is_minimized);
            state.minimized = transition.minimized;
            if let Some(target) = transition.target {
                state.desired_memory_target = target;
            }
            if !is_minimized && valid_viewport_bounds(bounds) {
                state.last_valid_viewport_bounds = Some(bounds);
            }
            (transition, state.last_valid_viewport_bounds)
        };
        let lifecycle_event = match transition.target {
            Some(MemoryUsageTarget::Low) => Some("window_minimized"),
            Some(MemoryUsageTarget::Normal) => Some("window_restored"),
            None => None,
        };
        if let Some(event) = lifecycle_event {
            let log = app.borrow().log.clone();
            if log.enabled() {
                let _ = log.record(event);
            }
        }
        if transition.resize_bounds {
            if let Some(bounds) = resize_bounds {
                Self::resize_current(app, bounds);
            }
        }
        if let Some(target) = transition.target {
            Self::set_memory_target_current(app, target);
            if target == MemoryUsageTarget::Normal {
                Self::schedule_tab_idle_timer(app);
            } else {
                win32::schedule_idle_timer(app.borrow().hwnd, IdleTimerAction::Kill);
            }
        }
        transition
    }

    pub fn page_usable(app: &AppRef) -> IdleTimerAction {
        let active = app.borrow().tab_model.active();
        Self::tab_page_loaded(app, active);
        let decision = {
            let state = app.borrow();
            let _ = state.log.record("page_usable");
            idle_memory_decision(state.minimized, true, 0, state.desired_memory_target)
        };
        Self::apply_idle_decision(app, decision);
        decision.timer
    }

    fn tab_page_loaded(app: &AppRef, tab_id: TabId) {
        let active = {
            let mut state = app.borrow_mut();
            let active = state.tab_model.active() == tab_id;
            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                tab.page_usable = true;
                tab.loading = false;
            } else {
                return;
            }
            if active {
                state.page_loaded = true;
            }
            active
        };
        if active {
            let decision = {
                let state = app.borrow();
                idle_memory_decision(state.minimized, true, 0, state.desired_memory_target)
            };
            Self::apply_idle_decision(app, decision);
            let action = Self::combine_idle_and_suspend_timers(app, decision.timer);
            win32::schedule_idle_timer(app.borrow().hwnd, action);
        } else {
            Self::post_tab_changed(app.borrow().hwnd, tab_id);
        }
    }

    pub fn navigation_started(app: &AppRef) -> IdleTimerAction {
        {
            let mut state = app.borrow_mut();
            state.page_loaded = false;
            let active = state.tab_model.active();
            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == active) {
                tab.page_usable = false;
                tab.loading = true;
            }
        }
        let decision = {
            let state = app.borrow();
            idle_memory_decision(state.minimized, false, 0, state.desired_memory_target)
        };
        Self::apply_idle_decision(app, decision);
        decision.timer
    }

    pub fn navigation_progress_message(app: &AppRef, navigation_id: u64, stage: isize) -> bool {
        if !(0..=3).contains(&stage) {
            return false;
        }
        let state = app.borrow();
        state
            .tabs
            .iter()
            .find(|tab| tab.id == state.tab_model.active())
            .is_some_and(|tab| tab.progress.navigation_id == Some(navigation_id))
    }

    pub fn navigation_progress_percent(app: &AppRef) -> Option<u8> {
        app.borrow().navigation_progress.percent
    }

    pub fn navigation_progress_painted(app: &AppRef) -> bool {
        let mut state = app.borrow_mut();
        let cleared = state.navigation_progress.clear_after_completion_paint();
        if cleared {
            let active = state.tab_model.active();
            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == active) {
                tab.progress.clear_after_completion_paint();
            }
        }
        cleared
    }

    pub fn reset_navigation_progress(app: &AppRef) -> bool {
        app.borrow_mut().navigation_progress.reset()
    }

    pub fn idle_timer_fired(app: &AppRef, idle_ms: u32) -> IdleTimerAction {
        let now = Instant::now();
        let _ = Self::with_engine(app, |engine| engine.suspend_hidden_tabs(30_000));
        {
            let mut state = app.borrow_mut();
            let active = state.tab_model.active();
            for tab in &mut state.tabs {
                if tab.id != active
                    && tab.controller_created
                    && !tab.suspended
                    && tab.hidden_since.is_some_and(|hidden| {
                        hidden_tab_suspend_eligible(now.saturating_duration_since(hidden))
                    })
                {
                    tab.suspended = true;
                }
            }
        }
        let decision = {
            let state = app.borrow();
            idle_memory_decision(
                state.minimized,
                state.page_loaded,
                idle_ms,
                state.desired_memory_target,
            )
        };
        Self::apply_idle_decision(app, decision);
        Self::combine_idle_and_suspend_timers(app, decision.timer)
    }

    fn combine_idle_and_suspend_timers(app: &AppRef, idle: IdleTimerAction) -> IdleTimerAction {
        let state = app.borrow();
        if state.minimized {
            return IdleTimerAction::Kill;
        }
        let now = Instant::now();
        let suspend_in = state
            .tabs
            .iter()
            .filter(|tab| {
                tab.id != state.tab_model.active() && tab.controller_created && !tab.suspended
            })
            .filter_map(|tab| tab.hidden_since)
            .map(|hidden| {
                let elapsed = now.saturating_duration_since(hidden);
                if hidden_tab_suspend_eligible(elapsed) {
                    1
                } else {
                    (Duration::from_secs(30) - elapsed).as_millis().max(1) as u32
                }
            })
            .min();
        match (idle, suspend_in) {
            (IdleTimerAction::Kill, Some(wait)) => IdleTimerAction::Arm(wait),
            (IdleTimerAction::Arm(idle), Some(wait)) => IdleTimerAction::Arm(idle.min(wait)),
            (idle, None) => idle,
        }
    }

    fn schedule_tab_idle_timer(app: &AppRef) {
        let (hwnd, idle) = {
            let state = app.borrow();
            (
                state.hwnd,
                if state.minimized {
                    IdleTimerAction::Kill
                } else if state.page_loaded {
                    IdleTimerAction::Arm(3_000)
                } else {
                    IdleTimerAction::Kill
                },
            )
        };
        let action = Self::combine_idle_and_suspend_timers(app, idle);
        win32::schedule_idle_timer(hwnd, action);
    }

    fn apply_idle_decision(app: &AppRef, decision: IdleMemoryDecision) {
        let changed = {
            let mut state = app.borrow_mut();
            if state.desired_memory_target == decision.target {
                false
            } else {
                state.desired_memory_target = decision.target;
                true
            }
        };
        if changed {
            let event = match decision.target {
                MemoryUsageTarget::Low => "idle_trim_low",
                MemoryUsageTarget::Normal => "idle_trim_normal",
            };
            let _ = app.borrow().log.record(event);
            Self::set_memory_target_current(app, decision.target);
        }
    }

    fn set_memory_target_current(app: &AppRef, target: MemoryUsageTarget) {
        let Some(mut engine) = app.borrow_mut().engine.take() else {
            return;
        };
        let result = engine.set_memory_usage_target(target);
        Self::record_memory_target_result(app, target, &result);
        let mut state = app.borrow_mut();
        state.effective_memory_target = match result {
            Ok(MemoryTargetApply::Applied) => Some(target),
            Ok(MemoryTargetApply::Unsupported | MemoryTargetApply::Deferred) | Err(_) => None,
        };
        if state.closing {
            let _ = engine.close();
        } else {
            state.engine = Some(engine);
        }
    }

    fn record_memory_target_result(
        app: &AppRef,
        target: MemoryUsageTarget,
        result: &windows::core::Result<MemoryTargetApply>,
    ) {
        let event = match result {
            Ok(MemoryTargetApply::Applied) => Some(match target {
                MemoryUsageTarget::Low => "memory_target_applied_low",
                MemoryUsageTarget::Normal => "memory_target_applied_normal",
            }),
            Ok(MemoryTargetApply::Unsupported) => {
                let mut state = app.borrow_mut();
                if state.memory_target_unsupported_logged {
                    None
                } else {
                    state.memory_target_unsupported_logged = true;
                    Some("memory_target_unsupported")
                }
            }
            Ok(MemoryTargetApply::Deferred) => None,
            Err(_) => Some(match target {
                MemoryUsageTarget::Low => "memory_target_failed_low",
                MemoryUsageTarget::Normal => "memory_target_failed_normal",
            }),
        };
        if let Some(event) = event {
            let _ = app.borrow().log.record(event);
        }
    }

    pub fn history(app: &AppRef, forward: bool) {
        let Some(mut engine) = app.borrow_mut().engine.take() else {
            return;
        };
        let can_move = if forward {
            engine.can_go_forward()
        } else {
            engine.can_go_back()
        };
        let result = if can_move {
            if forward {
                engine.go_forward()
            } else {
                engine.go_back()
            }
        } else {
            Ok(())
        };
        let mut state = app.borrow_mut();
        if state.closing {
            let _ = engine.close();
        } else {
            state.engine = Some(engine);
        }
        if let Err(error) = result {
            win32::show_error_message(&format!("History navigation failed: {error}"));
        }
    }

    pub fn latest_viewport_bounds(&mut self) -> windows::Win32::Foundation::RECT {
        let mut rect = windows::Win32::Foundation::RECT::default();
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(self.hwnd, &mut rect);
        }
        // The chrome layout takes raw DPI; a scale factor here would clamp to 96 at 144 DPI.
        let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(self.hwnd) }.max(96);
        let maximised =
            unsafe { windows::Win32::UI::WindowsAndMessaging::IsZoomed(self.hwnd) }.as_bool();
        let bounds = chrome_viewport_bounds(
            rect.right - rect.left,
            rect.bottom - rect.top,
            dpi,
            self.tabs.len(),
            maximised,
        );
        if valid_viewport_bounds(bounds) {
            self.last_valid_viewport_bounds = Some(bounds);
            bounds
        } else {
            if let Some(bounds) = self.last_valid_viewport_bounds {
                bounds
            } else {
                let fallback = chrome_viewport_bounds(1000, 720, dpi, self.tabs.len(), maximised);
                self.last_valid_viewport_bounds = Some(fallback);
                fallback
            }
        }
    }
}

fn chrome_viewport_bounds(
    width: i32,
    height: i32,
    dpi: u32,
    tab_count: usize,
    maximised: bool,
) -> windows::Win32::Foundation::RECT {
    let viewport =
        crate::chrome::layout::compute(width, height, dpi, tab_count, maximised).viewport;
    windows::Win32::Foundation::RECT {
        left: viewport.left,
        top: viewport.top,
        right: viewport.right,
        bottom: viewport.bottom,
    }
}

fn local_app_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("Bowser"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::MemoryUsageTarget;
    use std::{cell::RefCell, collections::VecDeque, path::Path, rc::Rc};

    #[test]
    fn navigation_progress_stages_are_monotonic_and_ignore_stale_ids() {
        let mut progress = NavigationProgress::default();
        assert!(progress.apply(NavigationProgressEvent::Started(7)));
        assert_eq!(progress.percent, Some(15));
        assert!(progress.apply(NavigationProgressEvent::Advanced(
            7,
            NavigationStage::ContentLoading
        )));
        assert_eq!(progress.percent, Some(45));
        assert!(!progress.apply(NavigationProgressEvent::Advanced(
            7,
            NavigationStage::ContentLoading
        )));
        assert_eq!(progress.percent, Some(45));
        assert!(!progress.apply(NavigationProgressEvent::Advanced(
            6,
            NavigationStage::DomContentLoaded
        )));
        assert_eq!(progress.percent, Some(45));
        assert!(progress.apply(NavigationProgressEvent::Advanced(
            7,
            NavigationStage::DomContentLoaded
        )));
        assert_eq!(progress.percent, Some(75));
        assert!(!progress.apply(NavigationProgressEvent::Started(7)));
        assert_eq!(progress.percent, Some(75));
        assert!(progress.apply(NavigationProgressEvent::Completed(7)));
        assert_eq!(progress.percent, Some(100));
        assert!(progress.clear_after_completion_paint());
        assert_eq!(progress.percent, None);
    }

    #[test]
    fn tabs_start_with_one_empty_tab_and_close_last_keeps_one_empty_tab() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        assert_eq!(App::tab_count(&app), 1);
        assert!(App::tab_title(&app, first).unwrap().is_empty());

        assert!(App::close_tab(&app, first));
        assert_eq!(App::tab_count(&app), 1);
        let empty = App::active_tab_id(&app);
        assert_ne!(empty, first);
        assert_eq!(App::tab_progress_percent(&app, empty), None);
        assert!(!App::tab_loading(&app, empty));
    }

    #[test]
    fn only_tabs_with_navigation_targets_need_controllers() {
        let target = url::Url::parse("https://example.test/").unwrap();
        assert!(!tab_needs_controller(None));
        assert!(tab_needs_controller(Some(&target)));
    }

    #[test]
    fn switching_tabs_hides_only_existing_old_controller_and_shows_existing_new_one() {
        let first = TabId(1);
        let second = TabId(2);
        assert_eq!(
            controller_visibility_plan((first, true), (second, true)),
            [
                ControllerVisibilityAction::Hide(first),
                ControllerVisibilityAction::Show(second)
            ]
        );
        assert_eq!(
            controller_visibility_plan((first, true), (second, false)),
            [ControllerVisibilityAction::Hide(first)]
        );
        assert_eq!(
            controller_visibility_plan((first, false), (second, false)),
            []
        );
        assert_eq!(controller_visibility_plan((first, true), (first, true)), []);
    }

    #[test]
    fn closing_a_tab_closes_it_and_shows_only_an_existing_active_neighbour() {
        let first = TabId(4);
        let second = TabId(5);
        assert_eq!(
            controller_close_plan(first, true, second, true),
            [
                ControllerVisibilityAction::Close(first),
                ControllerVisibilityAction::Show(second)
            ]
        );
        assert_eq!(
            controller_close_plan(second, false, first, true),
            [ControllerVisibilityAction::Close(second)]
        );
        assert_eq!(
            controller_close_plan(first, true, second, false),
            [ControllerVisibilityAction::Close(first)]
        );
    }

    #[test]
    fn hidden_controller_suspension_starts_at_thirty_seconds() {
        assert!(!hidden_tab_suspend_eligible(Duration::from_secs(29)));
        assert!(hidden_tab_suspend_eligible(Duration::from_secs(30)));
        assert!(hidden_tab_suspend_eligible(Duration::from_secs(31)));
    }

    #[test]
    fn tab_lifecycle_selects_wraps_and_reorders_without_losing_identity() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        let third = App::new_tab(&app);

        assert_eq!(App::tab_count(&app), 3);
        assert_eq!(App::active_tab_id(&app), third);
        assert_eq!(App::select_next(&app), first);
        assert_eq!(App::select_prev(&app), third);
        assert!(App::move_tab(&app, first, 2));
        assert!(App::close_tab(&app, second));
        assert_eq!(App::tab_count(&app), 2);
        assert_eq!(App::active_tab_id(&app), third);
    }

    #[test]
    fn progress_is_per_tab_and_callbacks_for_closed_tabs_are_ignored() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        assert!(App::apply_tab_progress(
            &app,
            first,
            NavigationProgressEvent::Started(4)
        ));
        assert!(App::apply_tab_progress(
            &app,
            second,
            NavigationProgressEvent::Started(8)
        ));
        assert!(App::apply_tab_progress(
            &app,
            first,
            NavigationProgressEvent::Advanced(4, NavigationStage::ContentLoading)
        ));
        assert_eq!(App::tab_progress_percent(&app, first), Some(45));
        assert_eq!(App::tab_progress_percent(&app, second), Some(15));

        assert!(App::close_tab(&app, first));
        assert!(!App::apply_tab_progress(
            &app,
            first,
            NavigationProgressEvent::Completed(4)
        ));
        assert_eq!(App::active_progress_percent(&app), Some(15));
    }

    #[test]
    fn background_page_load_updates_only_its_tab_until_selected() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        App::tab_page_loaded(&app, first);

        assert!(!app.borrow().page_loaded);
        assert!(
            app.borrow()
                .tabs
                .iter()
                .find(|tab| tab.id == first)
                .unwrap()
                .page_usable
        );
        assert!(
            !app.borrow()
                .tabs
                .iter()
                .find(|tab| tab.id == second)
                .unwrap()
                .page_usable
        );

        assert!(App::select_tab(&app, first));
        assert!(app.borrow().page_loaded);
    }

    #[test]
    fn queued_progress_messages_never_reapply_data_to_another_active_tab() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        App::apply_tab_progress(&app, second, NavigationProgressEvent::Started(1));
        App::apply_tab_progress(
            &app,
            second,
            NavigationProgressEvent::Advanced(1, NavigationStage::ContentLoading),
        );

        assert!(App::navigation_progress_message(&app, 1, 3));
        assert!(!App::navigation_progress_message(&app, 999, 3));
        assert_eq!(App::active_progress_percent(&app), Some(45));
        assert_eq!(App::tab_progress_percent(&app, first), None);
    }

    #[test]
    fn navigation_progress_supersedes_and_resets_after_failure_or_close() {
        let mut progress = NavigationProgress::default();
        progress.apply(NavigationProgressEvent::Started(1));
        assert!(progress.apply(NavigationProgressEvent::Started(2)));
        assert_eq!(progress.navigation_id, Some(2));
        assert!(!progress.apply(NavigationProgressEvent::Completed(1)));
        assert_eq!(progress.percent, Some(15));
        assert!(progress.apply(NavigationProgressEvent::Completed(2)));
        assert!(progress.clear_after_completion_paint());
        progress.apply(NavigationProgressEvent::Started(3));
        assert!(progress.reset());
        assert_eq!(progress, NavigationProgress::default());
        assert!(!progress.reset());
    }

    struct TabRecordingEngine {
        calls: Rc<RefCell<Vec<String>>>,
    }

    impl EngineAdapter for TabRecordingEngine {
        fn navigate(&mut self, url: &url::Url) -> windows::core::Result<()> {
            self.calls.borrow_mut().push(format!("navigate:{url}"));
            Ok(())
        }
        fn navigate_tab(&mut self, tab: TabId, url: &url::Url) -> windows::core::Result<()> {
            self.calls
                .borrow_mut()
                .push(format!("navigate:{}:{url}", tab.0));
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
        fn resize(
            &mut self,
            _bounds: windows::Win32::Foundation::RECT,
        ) -> windows::core::Result<()> {
            Ok(())
        }
        fn select_tab(&mut self, tab: TabId) -> windows::core::Result<()> {
            self.calls.borrow_mut().push(format!("select:{}", tab.0));
            Ok(())
        }
        fn close_tab(&mut self, tab: TabId) -> windows::core::Result<()> {
            self.calls.borrow_mut().push(format!("close:{}", tab.0));
            Ok(())
        }
        fn suspend_hidden_tabs(&mut self, hidden_for_ms: u64) -> windows::core::Result<()> {
            self.calls
                .borrow_mut()
                .push(format!("suspend:{hidden_for_ms}"));
            Ok(())
        }
        fn set_memory_usage_target(
            &mut self,
            _target: MemoryUsageTarget,
        ) -> windows::core::Result<MemoryTargetApply> {
            Ok(MemoryTargetApply::Applied)
        }
        fn close(&mut self) -> windows::core::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn tab_actions_reach_the_engine_per_tab_without_navigating_background_tabs() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        app.borrow_mut().engine = Some(Box::new(TabRecordingEngine {
            calls: calls.clone(),
        }));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        let third = App::new_tab(&app);
        assert_eq!(App::active_tab_id(&app), third);

        assert!(App::select_tab(&app, first));
        let target = url::Url::parse("https://example.test/second").unwrap();
        App::navigate_tab(&app, second, &target);
        assert_eq!(App::active_tab_id(&app), first);

        assert!(App::close_tab(&app, second));
        assert!(App::close_tab(&app, first));
        assert_eq!(App::active_tab_id(&app), third);
        App::idle_timer_fired(&app, 0);

        assert_eq!(
            *calls.borrow(),
            [
                format!("select:{}", second.0),
                format!("select:{}", third.0),
                format!("select:{}", first.0),
                format!("navigate:{}:{target}", second.0),
                format!("close:{}", second.0),
                format!("close:{}", first.0),
                format!("select:{}", third.0),
                "suspend:30000".to_owned(),
            ]
        );
    }

    struct RecordingEngine {
        calls: Rc<RefCell<Vec<String>>>,
        target_results: VecDeque<windows::core::Result<MemoryTargetApply>>,
        log: Option<Rc<PerfLog>>,
    }

    impl EngineAdapter for RecordingEngine {
        fn navigate(&mut self, url: &url::Url) -> windows::core::Result<()> {
            if let Some(log) = &self.log {
                let _ = log.record("navigation_started");
            }
            self.calls.borrow_mut().push(format!("navigate:{url}"));
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
        fn resize(
            &mut self,
            bounds: windows::Win32::Foundation::RECT,
        ) -> windows::core::Result<()> {
            self.calls
                .borrow_mut()
                .push(format!("resize:{}x{}", bounds.right, bounds.bottom));
            Ok(())
        }
        fn set_memory_usage_target(
            &mut self,
            target: MemoryUsageTarget,
        ) -> windows::core::Result<MemoryTargetApply> {
            self.calls.borrow_mut().push(format!("target:{target:?}"));
            self.target_results
                .pop_front()
                .unwrap_or(Ok(MemoryTargetApply::Applied))
        }
        fn close(&mut self) -> windows::core::Result<()> {
            Ok(())
        }
    }

    fn app_with_pending(bounds: windows::Win32::Foundation::RECT) -> AppRef {
        Rc::new(RefCell::new(App {
            hwnd: HWND::default(),
            edit: HWND::default(),
            input_font: HFONT::default(),
            engine: None,
            pending: Some(url::Url::parse("https://example.test/first").unwrap()),
            pending_tab: Some(TabId(0)),
            initializing: true,
            closing: false,
            loop_shutdown_pending: false,
            user_data_dir: PathBuf::new(),
            benchmark_url: None,
            allowed_benchmark_file: None,
            benchmark_tracking_prevention: None,
            provider: SearchProvider::from_config(Path::new("missing-search-provider.txt")),
            log: Rc::new(PerfLog::new(None).unwrap()),
            minimized: false,
            desired_memory_target: MemoryUsageTarget::Normal,
            effective_memory_target: None,
            memory_target_unsupported_logged: false,
            last_valid_viewport_bounds: Some(bounds),
            has_navigated: false,
            page_loaded: false,
            navigation_progress: NavigationProgress::default(),
            tabs: vec![AppTab::empty(TabId(0))],
            tab_model: crate::tabs::TabList::new(),
        }))
    }

    fn rect(width: i32, height: i32) -> windows::Win32::Foundation::RECT {
        windows::Win32::Foundation::RECT {
            left: 0,
            top: 36,
            right: width,
            bottom: height,
        }
    }

    fn recording_engine(
        calls: Rc<RefCell<Vec<String>>>,
        target_results: impl IntoIterator<Item = windows::core::Result<MemoryTargetApply>>,
    ) -> Box<dyn EngineAdapter> {
        Box::new(RecordingEngine {
            calls,
            target_results: target_results.into_iter().collect(),
            log: None,
        })
    }

    #[test]
    fn engine_ready_uses_latest_resize_before_normal_target_and_pending_navigation() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        App::window_size_changed(&app, false, rect(1024, 700));

        App::complete_engine_creation(&app, recording_engine(calls.clone(), []));

        assert_eq!(
            *calls.borrow(),
            [
                "resize:1024x700",
                "target:Normal",
                "navigate:https://example.test/first",
            ]
        );
        assert_eq!(
            app.borrow().effective_memory_target,
            Some(MemoryUsageTarget::Normal)
        );
    }

    #[test]
    fn engine_ready_registers_tabs_created_during_initialization_before_pending_navigation() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);

        App::complete_engine_creation(
            &app,
            Box::new(TabRecordingEngine {
                calls: calls.clone(),
            }),
        );

        let first_url = "https://example.test/first";
        assert_eq!(
            *calls.borrow(),
            [
                format!("select:{}", first.0),
                format!("select:{}", second.0),
                format!("navigate:{}:{first_url}", first.0),
            ],
            "the pre-initialization tab must be known before its pending navigation is replayed"
        );
        assert_eq!(App::active_tab_id(&app), second);
        assert_eq!(App::tab_count(&app), 2);
    }

    #[test]
    fn pending_creation_uses_restored_bounds_and_current_normal_target() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        App::window_size_changed(&app, true, windows::Win32::Foundation::RECT::default());
        App::window_size_changed(&app, false, rect(1200, 760));

        App::complete_engine_creation(&app, recording_engine(calls.clone(), []));

        assert_eq!(
            *calls.borrow(),
            [
                "resize:1200x760",
                "target:Normal",
                "navigate:https://example.test/first",
            ]
        );
        assert_eq!(
            app.borrow().last_valid_viewport_bounds,
            Some(rect(1200, 760))
        );
    }

    #[test]
    fn pending_creation_while_still_minimized_uses_low_after_cached_bounds() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        App::window_size_changed(&app, true, windows::Win32::Foundation::RECT::default());

        App::complete_engine_creation(&app, recording_engine(calls.clone(), []));

        assert_eq!(
            *calls.borrow(),
            [
                "resize:800x600",
                "target:Low",
                "navigate:https://example.test/first",
            ]
        );
        assert_eq!(app.borrow().desired_memory_target, MemoryUsageTarget::Low);
    }

    #[test]
    fn failed_target_is_unknown_and_duplicate_size_does_not_retry() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let engine = recording_engine(
            calls.clone(),
            [
                Ok(MemoryTargetApply::Applied),
                Err(windows::core::Error::from(
                    windows::Win32::Foundation::E_FAIL,
                )),
                Ok(MemoryTargetApply::Applied),
            ],
        );
        App::complete_engine_creation(&app, engine);
        App::window_size_changed(&app, true, windows::Win32::Foundation::RECT::default());
        assert_eq!(app.borrow().effective_memory_target, None);
        App::window_size_changed(&app, true, windows::Win32::Foundation::RECT::default());
        App::window_size_changed(&app, false, rect(900, 650));

        assert_eq!(
            *calls.borrow(),
            [
                "resize:800x600",
                "target:Normal",
                "navigate:https://example.test/first",
                "target:Low",
                "resize:900x650",
                "target:Normal",
            ]
        );
        assert_eq!(
            app.borrow().effective_memory_target,
            Some(MemoryUsageTarget::Normal)
        );
    }

    #[test]
    fn ordinary_resizes_only_update_bounds_without_retrying_memory_target() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        app.borrow_mut().engine = Some(recording_engine(calls.clone(), []));
        app.borrow_mut().effective_memory_target = Some(MemoryUsageTarget::Normal);

        App::window_size_changed(&app, false, rect(900, 650));
        App::window_size_changed(&app, false, rect(1100, 720));

        assert_eq!(*calls.borrow(), ["resize:900x650", "resize:1100x720"]);
        assert_eq!(
            app.borrow().effective_memory_target,
            Some(MemoryUsageTarget::Normal)
        );
    }

    #[test]
    fn unsupported_memory_target_is_logged_once_and_does_not_block_navigation() {
        let path =
            std::env::temp_dir().join(format!("bowser-unsupported-{}.csv", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let log = Rc::new(PerfLog::new(Some(&path)).unwrap());
        let app = app_with_pending(rect(800, 600));
        app.borrow_mut().log = log.clone();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let engine = recording_engine(
            calls.clone(),
            [
                Ok(MemoryTargetApply::Unsupported),
                Ok(MemoryTargetApply::Unsupported),
            ],
        );
        App::complete_engine_creation(&app, engine);
        assert!(app.borrow().engine.is_some());
        assert!(app.borrow().has_navigated);
        assert_eq!(app.borrow().effective_memory_target, None);
        App::window_size_changed(&app, true, windows::Win32::Foundation::RECT::default());
        drop(app);
        drop(log);

        let events = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            events
                .lines()
                .filter(|line| line.starts_with("memory_target_unsupported,"))
                .count(),
            1
        );
        assert!(
            calls
                .borrow()
                .iter()
                .any(|call| call.starts_with("navigate:"))
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn benchmark_logs_minimize_and_restore_before_ready_target_and_pending_navigation() {
        let path = std::env::temp_dir().join(format!(
            "bowser-lifecycle-boundaries-{}.csv",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let log = Rc::new(PerfLog::new(Some(&path)).unwrap());
        let app = app_with_pending(rect(800, 600));
        app.borrow_mut().log = log.clone();
        let calls = Rc::new(RefCell::new(Vec::new()));

        App::window_size_changed(&app, false, rect(850, 620));
        App::window_size_changed(&app, false, rect(900, 650));
        App::window_size_changed(&app, true, windows::Win32::Foundation::RECT::default());
        App::window_size_changed(&app, true, windows::Win32::Foundation::RECT::default());
        App::window_size_changed(&app, false, rect(1100, 720));
        App::window_size_changed(&app, false, rect(1120, 730));
        App::complete_engine_creation(
            &app,
            Box::new(RecordingEngine {
                calls,
                target_results: VecDeque::new(),
                log: Some(log.clone()),
            }),
        );
        drop(app);
        drop(log);

        let events = std::fs::read_to_string(&path).unwrap();
        let names: Vec<_> = events
            .lines()
            .skip(1)
            .map(|line| line.split(',').next().unwrap())
            .collect();
        let minimized = names
            .iter()
            .position(|event| *event == "window_minimized")
            .unwrap();
        let restored = names
            .iter()
            .position(|event| *event == "window_restored")
            .unwrap();
        let normal_target = names
            .iter()
            .position(|event| *event == "memory_target_applied_normal")
            .unwrap();
        let navigation = names
            .iter()
            .position(|event| *event == "navigation_started")
            .unwrap();
        assert!(minimized < restored && restored < normal_target && normal_target < navigation);
        assert_eq!(
            names
                .iter()
                .filter(|event| **event == "window_minimized")
                .count(),
            1
        );
        assert_eq!(
            names
                .iter()
                .filter(|event| **event == "window_restored")
                .count(),
            1
        );
        assert!(!names.contains(&"memory_target_applied_low"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn ordinary_launch_state_has_no_engine_or_webview_phase_event_before_submit() {
        let path = std::env::temp_dir().join(format!("bowser-startup-{}.csv", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let log = Rc::new(PerfLog::new(Some(&path)).unwrap());
        let app = App::from_launch_options(
            LaunchOptions::default(),
            PathBuf::from("production-profile"),
            SearchProvider::from_config(Path::new("missing-search-provider.txt")),
            log.clone(),
        );
        assert!(app.engine.is_none());
        assert!(app.pending.is_none());
        assert_eq!(app.benchmark_tracking_prevention, None);
        drop(app);
        drop(log);
        let events = std::fs::read_to_string(&path).unwrap();
        assert_eq!(events.lines().count(), 1);
        assert!(!events.contains("webview_"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn benchmark_repeat_boundary_is_written_before_the_second_navigation_event() {
        let path = std::env::temp_dir().join(format!("bowser-repeat-{}.csv", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let log = Rc::new(PerfLog::new(Some(&path)).unwrap());
        let app = app_with_pending(rect(800, 600));
        app.borrow_mut().log = log.clone();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let engine = Box::new(RecordingEngine {
            calls: calls.clone(),
            target_results: VecDeque::new(),
            log: Some(log.clone()),
        });
        App::complete_engine_creation(&app, engine);
        App::submit_url(
            &app,
            url::Url::parse("https://example.test/second").unwrap(),
            None,
        );
        drop(app);
        drop(log);

        let events = std::fs::read_to_string(&path).unwrap();
        let names: Vec<_> = events
            .lines()
            .skip(1)
            .map(|line| line.split(',').next().unwrap())
            .collect();
        let boundary = names
            .iter()
            .position(|event| *event == "repeat_navigation_boundary")
            .unwrap();
        let second_start = names
            .iter()
            .rposition(|event| *event == "navigation_started")
            .unwrap();
        assert!(boundary < second_start);
        assert_eq!(
            calls
                .borrow()
                .iter()
                .filter(|call| call.starts_with("navigate:"))
                .count(),
            2
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn window_memory_transition_retries_only_on_minimize_restore_boundaries() {
        let initial = window_memory_transition(false, false);
        assert_eq!(
            initial,
            WindowMemoryTransition {
                minimized: false,
                resize_bounds: true,
                target: None
            }
        );

        let minimize = window_memory_transition(false, true);
        assert_eq!(
            minimize,
            WindowMemoryTransition {
                minimized: true,
                resize_bounds: false,
                target: Some(MemoryUsageTarget::Low)
            }
        );
        assert_eq!(
            window_memory_transition(true, true),
            WindowMemoryTransition {
                minimized: true,
                resize_bounds: false,
                target: None
            }
        );

        let restore = window_memory_transition(true, false);
        assert_eq!(
            restore,
            WindowMemoryTransition {
                minimized: false,
                resize_bounds: true,
                target: Some(MemoryUsageTarget::Normal)
            }
        );
        assert_eq!(window_memory_transition(false, false), initial);

        let maximize_after_minimize = window_memory_transition(true, false);
        assert_eq!(
            maximize_after_minimize.target,
            Some(MemoryUsageTarget::Normal)
        );
        let rapid = [
            window_memory_transition(false, true).target,
            window_memory_transition(true, false).target,
            window_memory_transition(false, true).target,
        ];
        assert_eq!(
            rapid,
            [
                Some(MemoryUsageTarget::Low),
                Some(MemoryUsageTarget::Normal),
                Some(MemoryUsageTarget::Low)
            ]
        );
    }

    #[test]
    fn idle_policy_trims_only_loaded_visible_pages_after_three_seconds() {
        let decision = idle_memory_decision(false, true, 3_000, MemoryUsageTarget::Normal);
        assert_eq!(decision.target, MemoryUsageTarget::Low);
        assert_eq!(decision.timer, IdleTimerAction::Arm(1_000));
        assert_eq!(
            idle_memory_decision(false, true, 2_999, MemoryUsageTarget::Normal),
            IdleMemoryDecision {
                target: MemoryUsageTarget::Normal,
                timer: IdleTimerAction::Arm(1_000),
            }
        );
        assert_eq!(
            idle_memory_decision(false, false, 30_000, MemoryUsageTarget::Normal).timer,
            IdleTimerAction::Kill
        );
    }

    #[test]
    fn idle_policy_restores_after_activity_and_preserves_minimize_priority() {
        assert_eq!(
            idle_memory_decision(false, true, 999, MemoryUsageTarget::Low),
            IdleMemoryDecision {
                target: MemoryUsageTarget::Normal,
                timer: IdleTimerAction::Arm(3_000),
            }
        );
        assert_eq!(
            idle_memory_decision(true, true, 0, MemoryUsageTarget::Normal),
            IdleMemoryDecision {
                target: MemoryUsageTarget::Low,
                timer: IdleTimerAction::Kill,
            }
        );
    }

    #[test]
    fn idle_timer_applies_targets_and_logs_benchmark_transitions() {
        let path =
            std::env::temp_dir().join(format!("bowser-idle-trim-{}.csv", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let log = Rc::new(PerfLog::new(Some(&path)).unwrap());
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        {
            let mut state = app.borrow_mut();
            state.log = log.clone();
            state.engine = Some(recording_engine(calls.clone(), []));
            state.page_loaded = true;
            state.effective_memory_target = Some(MemoryUsageTarget::Normal);
        }

        assert_eq!(
            App::idle_timer_fired(&app, 3_000),
            IdleTimerAction::Arm(1_000)
        );
        assert_eq!(
            App::idle_timer_fired(&app, 2_000),
            IdleTimerAction::Arm(1_000)
        );
        assert_eq!(
            App::idle_timer_fired(&app, 1_000),
            IdleTimerAction::Arm(3_000)
        );
        assert_eq!(*calls.borrow(), ["target:Low", "target:Normal"]);
        drop(app);
        drop(log);

        let events = std::fs::read_to_string(&path).unwrap();
        assert!(events.contains("idle_trim_low,"));
        assert!(events.contains("idle_trim_normal,"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn minimized_or_zero_sized_viewports_never_become_engine_bounds() {
        let bounds = windows::Win32::Foundation::RECT {
            left: 0,
            top: 36,
            right: 800,
            bottom: 600,
        };
        assert!(valid_viewport_bounds(bounds));
        assert!(!valid_viewport_bounds(
            windows::Win32::Foundation::RECT::default()
        ));
        assert!(!valid_viewport_bounds(windows::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: 800,
            bottom: 0
        }));
    }

    #[test]
    fn engine_creation_caches_a_nonzero_startup_bounds_fallback() {
        let app = app_with_pending(windows::Win32::Foundation::RECT::default());
        app.borrow_mut().last_valid_viewport_bounds = None;
        let bounds = app.borrow_mut().latest_viewport_bounds();
        assert!(valid_viewport_bounds(bounds));
        assert_eq!(app.borrow().last_valid_viewport_bounds, Some(bounds));
    }

    #[test]
    fn webview_bounds_match_chrome_viewport_in_physical_pixels() {
        let bounds = chrome_viewport_bounds(1200, 800, 144, 3, true);
        let viewport = crate::chrome::layout::compute(1200, 800, 144, 3, true).viewport;
        assert_eq!(
            bounds,
            windows::Win32::Foundation::RECT {
                left: viewport.left,
                top: viewport.top,
                right: viewport.right,
                bottom: viewport.bottom,
            }
        );
        assert!(bounds.top > crate::win32::frame::content_top_inset(144));
    }

    #[test]
    fn layout_has_fixed_input_and_remaining_viewport() {
        let l = layout_client(800, 600, 1);
        assert_eq!(
            (l.input_x, l.input_y, l.input_width, l.input_height),
            (8, 8, 784, 28)
        );
        assert_eq!(
            (l.viewport_y, l.viewport_width, l.viewport_height),
            (44, 800, 556)
        );
    }

    #[test]
    fn prewarm_requires_meaningful_text_and_an_idle_open_app() {
        assert!(!should_prewarm(false, false, false, ""));
        assert!(!should_prewarm(false, false, false, " \t\n"));
        assert!(should_prewarm(false, false, false, " h"));
        assert!(!should_prewarm(true, false, false, "hello"));
        assert!(!should_prewarm(false, true, false, "hello"));
        assert!(!should_prewarm(false, false, true, "hello"));
    }

    #[test]
    fn tiny_layout_never_has_negative_dimensions() {
        let l = layout_client(2, 1, 1);
        assert_eq!((l.input_height, l.viewport_height), (0, 0));
        assert!(l.input_width >= 0 && l.viewport_width >= 0);
        assert!(l.viewport_y <= 1);
    }

    #[test]
    fn scale_doubles_input_inset_and_height() {
        let one = layout_client(500, 1000, 1);
        let two = layout_client(500, 1000, 2);
        assert_eq!(one.input_x * 2, two.input_x);
        assert_eq!(one.input_y * 2, two.input_y);
        assert_eq!(one.input_height * 2, two.input_height);
        assert_eq!(one.input_width + one.input_x * 2, 500);
        assert_eq!(two.input_width + two.input_x * 2, 500);
        assert_eq!(one.viewport_width, two.viewport_width);
        assert_eq!(one.viewport_y * 2, two.viewport_y);
    }
}
