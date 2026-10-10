use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::HWND,
        Graphics::Gdi::HFONT,
        System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
    },
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

const MEMORY_PRESSURE_LOAD_PERCENT: u32 = 85;
const MEMORY_PRESSURE_FREE_BYTES: u64 = 512 * 1024 * 1024;
/// Cadence for discarding tabs while the window is minimised or in the background.
const DISCARD_TICK_MS: u32 = 10_000;
/// A sleep-eligible tab that was kept awake (media, unsaved input) is looked at again this often.
const SLEEP_RECHECK_MS: u32 = 30_000;
/// Shortest wait the sleep timer will arm for, so it never spins.
const SLEEP_MIN_WAIT_MS: u32 = 1_000;
/// A page that never answers the sleep check is retried after this long.
const SLEEP_CHECK_TIMEOUT: Duration = Duration::from_secs(60);
/// After a check keeps a tab awake, the page is not asked again for this long.
const SLEEP_BLOCKED_BACKOFF: Duration = Duration::from_secs(120);
/// A tab whose render process crashed again within this window is reported, not reloaded.
const CRASH_RELOAD_MIN_GAP: Duration = Duration::from_secs(10);

/// Either signal is enough: Windows' load figure lags behind a sudden drop in free memory.
pub fn memory_pressure(load_percent: u32, available_phys_bytes: u64) -> bool {
    load_percent >= MEMORY_PRESSURE_LOAD_PERCENT
        || available_phys_bytes < MEMORY_PRESSURE_FREE_BYTES
}

fn system_memory_pressure() -> bool {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_err() {
        return false;
    }
    memory_pressure(status.dwMemoryLoad, status.ullAvailPhys)
}

/// What the discard decision knows about a tab before asking WebView2 anything.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiscardCandidate {
    pub active: bool,
    pub pinned: bool,
    pub suspended: bool,
    /// Set from navigation start until completion, which also covers a navigation still waiting
    /// for its controller to be created.
    pub loading: bool,
    pub committed_web_url: bool,
}

/// `audible` is queried last because it crosses into WebView2, and most tabs fail an earlier check.
pub fn tab_discard_eligible(tab: DiscardCandidate, audible: impl FnOnce() -> bool) -> bool {
    !tab.active
        && !tab.pinned
        && tab.suspended
        && !tab.loading
        && tab.committed_web_url
        && !audible()
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

/// While another application is in the foreground, a loaded page is idle and needs no polling.
pub fn background_memory_decision(minimized: bool, page_loaded: bool) -> IdleMemoryDecision {
    let target = if minimized || page_loaded {
        MemoryUsageTarget::Low
    } else {
        MemoryUsageTarget::Normal
    };
    IdleMemoryDecision {
        target,
        timer: IdleTimerAction::Kill,
    }
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
    pub benchmark_memory_pressure: bool,
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
    group_preferences: crate::tab_groups::GroupPreferences,
    group_preferences_path: Option<PathBuf>,
    settings: crate::settings::Settings,
    session: Option<crate::session::SessionStore>,
    pending_restore: Option<crate::session::Session>,
    /// Ad/tracker blocker shared with the engine; `None` for benchmark launches or when disabled.
    blocker: Option<engine::BlockerHandle>,
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
    /// The WebView2 controller was closed under memory pressure. The URL, title and favicon stay,
    /// and the next selection navigates to the URL again.
    discarded: bool,
    /// A page-side "is it safe to sleep" check is running (answered or timed out after
    /// [`SLEEP_CHECK_TIMEOUT`]).
    sleep_check_started: Option<Instant>,
    /// The last check found unsaved input or media; do not ask again before this.
    sleep_blocked_until: Option<Instant>,
    /// Last time a crashed render process was recovered by reloading this tab.
    last_crash_reload: Option<Instant>,
    /// Requests the blocker stopped since this document started loading.
    blocked: u32,
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
            discarded: false,
            sleep_check_started: None,
            sleep_blocked_until: None,
            last_crash_reload: None,
            blocked: 0,
        }
    }
}

pub type AppRef = Rc<RefCell<App>>;

/// Extensions to load for a window. Benchmark launches and custom data folders load none, and a
/// profile without installed extensions gets a plan that costs nothing at engine creation.
fn extension_startup_plan(
    app: &AppRef,
    user_data_dir: &std::path::Path,
) -> crate::extensions::StartupPlan {
    if app.borrow().benchmark_url.is_some() {
        return crate::extensions::StartupPlan::default();
    }
    let Ok(profile) = crate::profile::active() else {
        return crate::extensions::StartupPlan::default();
    };
    if profile.webview2_dir() != user_data_dir {
        return crate::extensions::StartupPlan::default();
    }
    crate::extensions::startup_plan(&profile.extensions_dir(), &profile.extensions_file())
}

impl App {
    /// Loads an installed extension into the running profile, or unloads it when `enabled` is
    /// false. The outcome goes to the extension status list, which the extension menu shows.
    pub fn set_extension_enabled(app: &AppRef, id: String, folder: PathBuf, enabled: bool) {
        let present = app.borrow().engine.is_some();
        if !present {
            // No engine yet: the saved state takes effect when the window starts.
            let result = if enabled {
                Err("Bowser is still starting. Restart Bowser to load this extension.".to_owned())
            } else {
                Ok(())
            };
            crate::extensions::record_load_result(&id, result);
            return;
        }
        let report = id.clone();
        Self::with_engine(app, move |engine| {
            engine.set_extension_enabled(
                &id,
                &folder,
                enabled,
                Box::new(move |result| crate::extensions::record_load_result(&report, result)),
            );
            Ok(())
        });
    }

    /// Unloads an extension from the running profile, then calls `done` with the outcome. `done`
    /// runs immediately when no engine is running, because then nothing is loaded.
    pub fn remove_extension(
        app: &AppRef,
        id: String,
        done: Box<dyn FnOnce(std::result::Result<(), String>)>,
    ) {
        let present = app.borrow().engine.is_some();
        if !present {
            done(Ok(()));
            return;
        }
        Self::with_engine(app, move |engine| {
            engine.remove_extension(&id, done);
            Ok(())
        });
    }

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

    /// Title suffix describing the blocker for the active tab, e.g. ` [12 blocked]`.
    fn blocker_suffix(state: &App) -> String {
        let Some(blocker) = state.blocker.as_ref() else {
            return String::new();
        };
        let active = state.tab_model.active();
        let Some(tab) = state.tabs.iter().find(|tab| tab.id == active) else {
            return String::new();
        };
        let host = tab
            .url
            .as_ref()
            .and_then(|url| url.host_str())
            .unwrap_or("");
        if blocker.borrow().site_allowed(host) {
            " [blocker off for this site]".to_owned()
        } else if tab.blocked > 0 {
            format!(" [{} blocked]", tab.blocked)
        } else {
            String::new()
        }
    }

    /// Ctrl+Shift+B: switches blocking off or on for the active tab's site, then reloads it.
    pub fn toggle_blocker(app: &AppRef) {
        let (blocker, host) = {
            let state = app.borrow();
            let host = state
                .tabs
                .iter()
                .find(|tab| tab.id == state.tab_model.active())
                .and_then(|tab| tab.url.as_ref())
                .and_then(|url| url.host_str().map(str::to_owned));
            (state.blocker.clone(), host)
        };
        let Some(blocker) = blocker else {
            win32::show_error_message(
                "The blocker is disabled (blocker_enabled=false in settings.txt).",
            );
            return;
        };
        let Some(host) = host else { return };
        let result = blocker.borrow_mut().toggle_site(&host);
        match result {
            crate::blocker::SiteToggle::Allowed | crate::blocker::SiteToggle::Blocking => {
                Self::sync_window_title(app);
                Self::reload(app);
            }
            crate::blocker::SiteToggle::ConfiguredInSettings => win32::show_error_message(
                "This site is exempted by blocker_allow in settings.txt. Edit that file to change it.",
            ),
            crate::blocker::SiteToggle::NoSite => {}
        }
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
            let profile_suffix = crate::profile::active()
                .map(|profile| profile.title_suffix())
                .unwrap_or_default();
            (
                state.hwnd,
                format!("{title}{}{profile_suffix}", Self::blocker_suffix(&state)),
            )
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
        if was_active {
            Self::restore_discarded_tab(app, active);
        }
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
        let _ = Self::expand_group_for_tab(app, id);
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
        Self::restore_discarded_tab(app, id);
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
        Self::restore_discarded_tab(app, next);
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

    /// Pins or unpins one tab in the ordered model without touching its WebView controller or
    /// active selection. The ordered AppTab mirror is moved to the same model index.
    pub fn set_tab_pinned(app: &AppRef, id: TabId, pinned: bool) -> bool {
        let hwnd = {
            let mut state = app.borrow_mut();
            let Some(from) = state.tabs.iter().position(|tab| tab.id == id) else {
                return false;
            };
            if !state.tab_model.set_pinned(id, pinned) {
                return false;
            }
            let tab = state.tabs.remove(from);
            let target = state
                .tab_model
                .index_of(id)
                .unwrap_or(from)
                .min(state.tabs.len());
            state.tabs.insert(target, tab);
            state.hwnd
        };
        Self::post_tab_changed(hwnd, id);
        true
    }

    pub fn tab_pinned(app: &AppRef, id: TabId) -> bool {
        app.borrow().tab_model.get(id).is_some_and(|tab| tab.pinned)
    }

    pub fn tab_committed_url(app: &AppRef, id: TabId) -> Option<url::Url> {
        app.borrow()
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .and_then(|tab| tab.url.clone())
    }

    pub fn tab_group_preferences(app: &AppRef) -> crate::tab_groups::GroupPreferences {
        app.borrow().group_preferences.clone()
    }

    pub fn set_tab_group_mode(app: &AppRef, id: TabId, mode: crate::tab_groups::GroupMode) -> bool {
        let (hwnd, active, changed) = {
            let mut state = app.borrow_mut();
            let Some(url) = state.tab_model.get(id).and_then(|tab| tab.url.as_deref()) else {
                return false;
            };
            let Some(key) = url::Url::parse(url)
                .ok()
                .and_then(|url| crate::tab_groups::DomainKey::from_url(&url))
            else {
                return false;
            };
            let Ok(changed) = state.group_preferences.set_mode(&key, mode) else {
                return false;
            };
            (state.hwnd, state.tab_model.active(), changed)
        };
        if changed {
            Self::save_group_preferences(app);
            Self::post_tab_changed(hwnd, active);
        }
        changed
    }

    pub fn set_tab_group_collapsed(app: &AppRef, id: TabId, collapsed: bool) -> bool {
        let (hwnd, active, changed) = {
            let mut state = app.borrow_mut();
            let Some(url) = state.tab_model.get(id).and_then(|tab| tab.url.as_deref()) else {
                return false;
            };
            let Some(key) = url::Url::parse(url)
                .ok()
                .and_then(|url| crate::tab_groups::DomainKey::from_url(&url))
            else {
                return false;
            };
            let Ok(changed) = state.group_preferences.set_collapsed(&key, collapsed) else {
                return false;
            };
            (state.hwnd, state.tab_model.active(), changed)
        };
        if changed {
            Self::save_group_preferences(app);
            Self::post_tab_changed(hwnd, active);
        }
        changed
    }

    fn expand_group_for_tab(app: &AppRef, id: TabId) -> bool {
        let (hwnd, active, changed) = {
            let mut state = app.borrow_mut();
            if state.tab_model.active() == id {
                return false;
            }
            let Some(url) = state.tab_model.get(id).and_then(|tab| tab.url.as_deref()) else {
                return false;
            };
            let Some(key) = url::Url::parse(url)
                .ok()
                .and_then(|url| crate::tab_groups::DomainKey::from_url(&url))
            else {
                return false;
            };
            if !state.group_preferences.is_collapsed(&key) {
                return false;
            }
            let changed = state
                .group_preferences
                .set_collapsed(&key, false)
                .unwrap_or(false);
            (state.hwnd, state.tab_model.active(), changed)
        };
        if changed {
            Self::save_group_preferences(app);
            Self::post_tab_changed(hwnd, active);
        }
        changed
    }

    fn save_group_preferences(app: &AppRef) {
        let state = app.borrow();
        if let Some(path) = &state.group_preferences_path
            && state.group_preferences.save(path).is_err()
        {
            let _ = state.log.record("tab_groups_preferences_save_failed");
        }
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

    pub fn active_tab_has_controller(app: &AppRef) -> bool {
        let state = app.borrow();
        let active = state.tab_model.active();
        state
            .tabs
            .iter()
            .find(|tab| tab.id == active)
            .is_some_and(|tab| tab.controller_created)
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
        let Some(app_data) = crate::profile::app_data_dir() else {
            win32::show_error_message(
                "Bowser could not find this user's LocalAppData directory, which is required for its private WebView2 profile.",
            );
            return Ok(());
        };
        let profile = match options.profile.as_deref() {
            None => crate::profile::Profile::default_in(&app_data),
            Some(name) => match crate::profile::Profile::named(&app_data, name) {
                Ok(profile) => profile,
                Err(error) => {
                    win32::show_error_message(&error.to_string());
                    return Ok(());
                }
            },
        };
        crate::profile::set_active(profile.clone());
        let provider = SearchProvider::from_config(&profile.search_provider_file());
        let user_data_dir = options
            .user_data_dir
            .as_ref()
            .cloned()
            .unwrap_or_else(|| profile.webview2_dir());
        if let Err(error) = engine::WebView2Engine::availability() {
            win32::show_runtime_error(&error);
            return Ok(());
        }
        // Benchmarks and custom profiles never touch the user's session files.
        let track_session = options.benchmark_url.is_none() && options.user_data_dir.is_none();
        let mut app = Self::from_launch_options(
            options,
            user_data_dir,
            provider,
            log,
            Some(profile.tab_groups_file()),
        );
        app.settings = crate::settings::Settings::load(&profile.settings_file());
        if track_session && app.settings.blocker_enabled {
            let mut state = crate::blocker::BlockerState::load(
                &profile.blocker_dir(),
                &app.settings.blocker_allow,
            );
            state.enabled = true;
            app.blocker = Some(Rc::new(RefCell::new(state)));
        }
        if track_session {
            app.init_session(&profile.session_dir());
        }
        win32::run(app)
    }

    fn from_launch_options(
        options: LaunchOptions,
        user_data_dir: PathBuf,
        provider: SearchProvider,
        log: Rc<PerfLog>,
        group_preferences_path: Option<PathBuf>,
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
            benchmark_memory_pressure: options.benchmark_memory_pressure,
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
            group_preferences: group_preferences_path
                .as_deref()
                .map(crate::tab_groups::GroupPreferences::load)
                .unwrap_or_default(),
            group_preferences_path,
            settings: crate::settings::Settings::default(),
            session: None,
            pending_restore: None,
            blocker: None,
        }
    }

    /// Opens the crash-recovery state and, after a crash, asks whether to restore.
    fn init_session(&mut self, dir: &std::path::Path) {
        let Some((mut store, offer)) = crate::session::SessionStore::open(dir) else {
            return; // another Bowser is running; leave its files alone
        };
        let accepted = offer.filter(|offer| {
            win32::ask_yes_no(&format!(
                "Bowser did not close properly last time. Restore your {} open tab{}?",
                offer.session.tabs.len(),
                if offer.session.tabs.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ))
        });
        store.begin_run(accepted.as_ref());
        self.pending_restore = accepted.map(|offer| offer.session);
        self.session = Some(store);
    }

    /// Rebuilds the tab strip from an accepted crash snapshot. Only the active tab loads now;
    /// the others are created asleep and load when selected. A bad entry never aborts the rest.
    pub fn restore_pending_session(app: &AppRef) {
        let Some(session) = app.borrow_mut().pending_restore.take() else {
            return;
        };
        let mut active_url = None;
        let hwnd = {
            let mut state = app.borrow_mut();
            let mut first = true;
            let mut active_id = None;
            for (index, saved) in session.tabs.iter().enumerate() {
                let Ok(url) = url::Url::parse(&saved.url) else {
                    continue;
                };
                let id = if first {
                    first = false;
                    state.tab_model.active()
                } else {
                    let id = state.tab_model.add(false);
                    state.tabs.push(AppTab::empty(id));
                    id
                };
                state.tab_model.set_url(id, Some(url.as_str()));
                state.tab_model.set_title(id, &saved.title);
                if saved.pinned {
                    state.tab_model.set_pinned(id, true);
                }
                if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.url = Some(url.clone());
                    tab.title.clone_from(&saved.title);
                    tab.discarded = true;
                }
                // The saved active tab wins; otherwise the first tab that restored.
                if index == session.active || active_id.is_none() {
                    active_id = Some(id);
                    active_url = Some(url);
                }
            }
            if let Some(id) = active_id {
                state.tab_model.select(id);
                if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.discarded = false;
                }
            }
            state.hwnd
        };
        let Some(url) = active_url else {
            return;
        };
        let _ = app.borrow().log.record("session_restored");
        win32::set_input_text(app.borrow().edit, url.as_str());
        Self::post_tab_changed(hwnd, app.borrow().tab_model.active());
        Self::sync_window_title(app);
        Self::submit_url(app, url, None);
    }

    /// Notes a tab-state change. Returns true when the caller should arm the one-shot save
    /// timer; further changes before it fires are coalesced.
    pub fn session_changed(app: &AppRef) -> bool {
        let mut state = app.borrow_mut();
        let closing = state.closing;
        match state.session.as_mut() {
            Some(store) if !store.save_pending && !closing => {
                store.save_pending = true;
                true
            }
            _ => false,
        }
    }

    fn session_snapshot(app: &AppRef) -> Option<crate::session::Session> {
        let state = app.borrow();
        let active = state.tab_model.index_of(state.tab_model.active())?;
        crate::session::Session::from_tabs(
            state.tab_model.iter().map(|model| {
                let tab = state.tabs.iter().find(|tab| tab.id == model.id);
                (
                    tab.and_then(|tab| tab.url.as_ref()),
                    tab.map_or("", |tab| tab.title.as_str()),
                    model.pinned,
                )
            }),
            active,
        )
    }

    /// The debounce timer fired: write the snapshot if anything changed.
    pub fn session_save_now(app: &AppRef) {
        let snapshot = Self::session_snapshot(app);
        let mut state = app.borrow_mut();
        let Some(store) = state.session.as_mut() else {
            return;
        };
        store.save_pending = false;
        if let Some(snapshot) = snapshot {
            let _ = store.save(&snapshot);
        }
    }

    /// Clean exit: final snapshot and removal of the running marker.
    pub fn session_finish(app: &AppRef) {
        let snapshot = Self::session_snapshot(app);
        let store = app.borrow_mut().session.take();
        if let Some(mut store) = store {
            store.finish(snapshot.as_ref());
        }
    }

    /// A tab's render process crashed. The active tab reloads; a hidden tab is marked so that it
    /// navigates afresh when selected. A repeat within [`CRASH_RELOAD_MIN_GAP`] is reported
    /// instead of reloaded again, so a page that crashes on load cannot loop.
    pub fn tab_render_failed(app: &AppRef, id: TabId) {
        let now = Instant::now();
        let active = {
            let mut state = app.borrow_mut();
            if state.closing {
                return;
            }
            let active = state.tab_model.active() == id;
            let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) else {
                return;
            };
            if tab
                .last_crash_reload
                .is_some_and(|last| now.saturating_duration_since(last) < CRASH_RELOAD_MIN_GAP)
            {
                drop(state);
                win32::show_error_message("WebView2 error: the page process crashed again.");
                return;
            }
            tab.last_crash_reload = Some(now);
            tab.page_usable = false;
            if !active {
                tab.discarded = true;
            }
            active
        };
        let _ = app.borrow().log.record("tab_render_process_failed");
        if active {
            Self::reload(app);
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
        let (hwnd, profile, tracking_prevention, blocker) = {
            let s = app.borrow();
            (
                s.hwnd,
                s.user_data_dir.clone(),
                // Production launches state the default (Balanced) explicitly; benchmark launches
                // keep the engine default so recorded baselines stay comparable.
                s.benchmark_tracking_prevention.or(s
                    .benchmark_url
                    .is_none()
                    .then_some(TrackingPreventionLevel::Balanced)),
                s.blocker.clone(),
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
            render_process_failed: {
                let crash_app = Rc::downgrade(app);
                Some(Rc::new(move |tab_id| {
                    if let Some(app) = crash_app.upgrade() {
                        App::tab_render_failed(&app, tab_id);
                    }
                }))
            },
            blocked_changed: {
                let blocked_app = Rc::downgrade(app);
                Some(Rc::new(move |tab_id, count| {
                    if let Some(app) = blocked_app.upgrade() {
                        let active = {
                            let mut state = app.borrow_mut();
                            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                                tab.blocked = count;
                            }
                            state.tab_model.active() == tab_id
                        };
                        if active {
                            Self::sync_window_title(&app);
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
        let plan = extension_startup_plan(app, &profile);
        let extras = engine::EngineExtras {
            blocker,
            browser_extensions: plan.installed,
            startup_extensions: plan.enabled,
        };
        let create_result = engine::WebView2Engine::begin_create_with_extras(
            hwnd,
            bounds,
            &profile,
            allowed_file,
            events,
            tracking_prevention,
            extras,
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

    /// `foreground` is false while another application is active. System-wide input then says
    /// nothing about this window, so the page trims at once and input polling stops until the
    /// window is activated again.
    pub fn idle_timer_fired(app: &AppRef, idle_ms: u32, foreground: bool) -> IdleTimerAction {
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
        let forced = app.borrow().benchmark_memory_pressure;
        Self::discard_one_hidden_tab_if(app, || forced || system_memory_pressure());
        let decision = {
            let state = app.borrow();
            if foreground {
                idle_memory_decision(
                    state.minimized,
                    state.page_loaded,
                    idle_ms,
                    state.desired_memory_target,
                )
            } else {
                background_memory_decision(state.minimized, state.page_loaded)
            }
        };
        Self::apply_idle_decision(app, decision);
        Self::combine_idle_and_suspend_timers(app, decision.timer)
    }

    fn combine_idle_and_suspend_timers(app: &AppRef, idle: IdleTimerAction) -> IdleTimerAction {
        let forced = app.borrow().benchmark_memory_pressure;
        Self::combine_with_discard_tick(app, idle, || forced || system_memory_pressure())
    }

    /// Minimised and background windows stop the idle timer, which would leave a discard pending
    /// while other applications are using the memory. While a discard is possible the timer keeps
    /// ticking at the discard cadence.
    fn combine_with_discard_tick(
        app: &AppRef,
        idle: IdleTimerAction,
        under_pressure: impl FnOnce() -> bool,
    ) -> IdleTimerAction {
        let timer = Self::idle_and_suspend_timer(app, idle);
        // Tab sleep: wake at the moment the earliest hidden tab turns sleep-eligible.
        let timer = match Self::sleep_wait_ms(app) {
            Some(wait) => Self::merge_timer(timer, wait),
            None => timer,
        };
        if app.borrow().minimized {
            if !Self::has_minimized_discard_candidate(app) || !under_pressure() {
                return timer;
            }
            return Self::merge_timer(timer, DISCARD_TICK_MS);
        }
        if Self::next_discard_tab(app, under_pressure).is_none() {
            return timer;
        }
        Self::merge_timer(timer, DISCARD_TICK_MS)
    }

    fn merge_timer(timer: IdleTimerAction, wait: u32) -> IdleTimerAction {
        match timer {
            IdleTimerAction::Kill => IdleTimerAction::Arm(wait),
            IdleTimerAction::Arm(current) => IdleTimerAction::Arm(current.min(wait)),
        }
    }

    /// Milliseconds until the earliest hidden, unpinned web tab reaches the sleep delay, or
    /// [`SLEEP_RECHECK_MS`] if one is already past it (kept awake by media or unsaved input).
    /// `None` when tab sleep is off or no tab could ever sleep. This is the only timer tab sleep
    /// adds; there is no polling of its own.
    fn sleep_wait_ms(app: &AppRef) -> Option<u32> {
        let state = app.borrow();
        if !state.settings.sleep_enabled {
            return None;
        }
        let after = Duration::from_secs(state.settings.sleep_after_secs);
        let now = Instant::now();
        let active = state.tab_model.active();
        state
            .tabs
            .iter()
            .filter(|tab| {
                tab.id != active
                    && tab.controller_created
                    && !tab.discarded
                    && tab
                        .url
                        .as_ref()
                        .is_some_and(|url| matches!(url.scheme(), "http" | "https"))
                    && !state
                        .tab_model
                        .get(tab.id)
                        .is_some_and(|model| model.pinned)
            })
            .filter_map(|tab| tab.hidden_since)
            .map(|since| {
                let elapsed = now.saturating_duration_since(since);
                if elapsed >= after {
                    SLEEP_RECHECK_MS
                } else {
                    ((after - elapsed).as_millis().min(u32::MAX as u128) as u32)
                        .max(SLEEP_MIN_WAIT_MS)
                }
            })
            .min()
    }

    /// A minimised window needs periodic checks before its hidden tabs reach the suspension age.
    /// This lightweight check avoids sampling system memory when no live hidden controller could
    /// ever be discarded.
    fn has_minimized_discard_candidate(app: &AppRef) -> bool {
        let candidates: Vec<TabId> = {
            let state = app.borrow();
            let active = state.tab_model.active();
            state
                .tabs
                .iter()
                .filter(|tab| {
                    tab.id != active
                        && tab.controller_created
                        && !tab.discarded
                        && tab.hidden_since.is_some()
                        && !state
                            .tab_model
                            .get(tab.id)
                            .is_some_and(|model| model.pinned)
                })
                .map(|tab| tab.id)
                .collect()
        };
        candidates.into_iter().any(|id| {
            Self::discard_candidate(app, id).is_some_and(|candidate| {
                tab_discard_eligible(
                    DiscardCandidate {
                        suspended: true,
                        ..candidate
                    },
                    || Self::tab_playing_audio(app, id),
                )
            })
        })
    }

    fn idle_and_suspend_timer(app: &AppRef, idle: IdleTimerAction) -> IdleTimerAction {
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

    /// Under memory pressure, closes the controller of the longest-hidden suspended tab. At most
    /// one tab goes per tick.
    fn discard_one_hidden_tab_if(app: &AppRef, under_pressure: impl FnOnce() -> bool) {
        if let Some(id) = Self::next_discard_tab(app, under_pressure) {
            Self::sleep_tab_if_safe(app, id);
        }
    }

    /// Asks the page whether sleeping could lose unsaved input or stop media, then sleeps the
    /// tab if not. Engines that answer at once sleep it before this returns.
    fn sleep_tab_if_safe(app: &AppRef, id: TabId) {
        if let Some(tab) = app.borrow_mut().tabs.iter_mut().find(|tab| tab.id == id) {
            tab.sleep_check_started = Some(Instant::now());
        }
        let verdict = Rc::new(std::cell::Cell::new(None));
        let answered_inline = Rc::new(std::cell::Cell::new(true));
        let weak = Rc::downgrade(app);
        let done: Box<dyn FnOnce(bool)> = {
            let verdict = verdict.clone();
            let answered_inline = answered_inline.clone();
            Box::new(move |blocked| {
                if answered_inline.get() {
                    verdict.set(Some(blocked));
                } else if let Some(app) = weak.upgrade() {
                    Self::finish_sleep_check(&app, id, blocked);
                }
            })
        };
        let asked = Self::with_engine(app, move |engine| {
            engine.check_sleep_blocked(id, done);
            Ok(())
        });
        answered_inline.set(false);
        match (asked, verdict.take()) {
            (Some(_), Some(blocked)) => Self::finish_sleep_check(app, id, blocked),
            (Some(_), None) => {} // the page will answer later
            (None, _) => {
                if let Some(tab) = app.borrow_mut().tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.sleep_check_started = None;
                }
            }
        }
    }

    fn finish_sleep_check(app: &AppRef, id: TabId, blocked: bool) {
        {
            let mut state = app.borrow_mut();
            if state.closing {
                return;
            }
            let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) else {
                return;
            };
            tab.sleep_check_started = None;
            if blocked {
                tab.sleep_blocked_until = Some(Instant::now() + SLEEP_BLOCKED_BACKOFF);
            }
        }
        if blocked {
            let _ = app.borrow().log.record("tab_sleep_blocked");
            return;
        }
        // The answer may be late: only sleep a tab that is still hidden and still eligible.
        let still_eligible = Self::discard_candidate(app, id).is_some_and(|candidate| {
            tab_discard_eligible(candidate, || Self::tab_playing_audio(app, id))
        }) && app
            .borrow()
            .tabs
            .iter()
            .any(|tab| tab.id == id && tab.controller_created && !tab.discarded);
        if still_eligible {
            Self::discard_tab(app, id);
        }
    }

    /// The tab that the next idle tick would discard: a hidden suspended tab that is either past
    /// the user's sleep delay or hidden while memory is under pressure. `under_pressure` is
    /// evaluated only when a suspended hidden tab exists.
    fn next_discard_tab(app: &AppRef, under_pressure: impl FnOnce() -> bool) -> Option<TabId> {
        let now = Instant::now();
        let (mut hidden, sleep_after): (Vec<(Instant, TabId)>, Option<Duration>) = {
            let state = app.borrow();
            let active = state.tab_model.active();
            let sleep_after = state
                .settings
                .sleep_enabled
                .then(|| Duration::from_secs(state.settings.sleep_after_secs));
            let hidden = state
                .tabs
                .iter()
                .filter(|tab| {
                    tab.id != active && tab.controller_created && tab.suspended && !tab.discarded
                })
                .filter(|tab| {
                    tab.sleep_blocked_until.is_none_or(|until| now >= until)
                        && tab.sleep_check_started.is_none_or(|started| {
                            now.saturating_duration_since(started) >= SLEEP_CHECK_TIMEOUT
                        })
                })
                .filter_map(|tab| tab.hidden_since.map(|since| (since, tab.id)))
                .collect();
            (hidden, sleep_after)
        };
        if hidden.is_empty() {
            return None;
        }
        let pressure = under_pressure();
        hidden.sort_by_key(|(since, _)| *since);
        for (since, id) in hidden {
            let due =
                sleep_after.is_some_and(|after| now.saturating_duration_since(since) >= after);
            if !pressure && !due {
                continue;
            }
            let Some(candidate) = Self::discard_candidate(app, id) else {
                continue;
            };
            if tab_discard_eligible(candidate, || Self::tab_playing_audio(app, id)) {
                return Some(id);
            }
        }
        None
    }

    fn discard_candidate(app: &AppRef, id: TabId) -> Option<DiscardCandidate> {
        let state = app.borrow();
        let tab = state.tabs.iter().find(|tab| tab.id == id)?;
        Some(DiscardCandidate {
            active: state.tab_model.active() == id,
            pinned: state.tab_model.get(id).is_some_and(|model| model.pinned),
            suspended: tab.suspended,
            loading: tab.loading,
            committed_web_url: tab
                .url
                .as_ref()
                .is_some_and(|url| matches!(url.scheme(), "http" | "https")),
        })
    }

    fn tab_playing_audio(app: &AppRef, id: TabId) -> bool {
        let mut audible = false;
        Self::with_engine(app, |engine| {
            audible = engine.tab_playing_audio(id);
            Ok(())
        });
        audible
    }

    fn discard_tab(app: &AppRef, id: TabId) {
        let hwnd = {
            let mut state = app.borrow_mut();
            let hwnd = state.hwnd;
            let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) else {
                return;
            };
            tab.controller_created = false;
            tab.discarded = true;
            tab.page_usable = false;
            tab.progress = NavigationProgress::default();
            tab.can_back = false;
            tab.can_forward = false;
            tab.hidden_since = None;
            tab.suspended = false;
            hwnd
        };
        // The engine forgets the tab before closing its controller, so later selections cannot
        // reach a stale controller and a second close of this tab is a no-op.
        Self::with_engine(app, |engine| engine.close_tab(id));
        let _ = app.borrow().log.record("tab_discarded");
        Self::post_tab_changed(hwnd, id);
    }

    /// Recreates a discarded tab's controller through the normal navigation path, to its last
    /// committed URL. Back and forward history is not restored.
    fn restore_discarded_tab(app: &AppRef, id: TabId) {
        let url = {
            let mut state = app.borrow_mut();
            let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) else {
                return;
            };
            if !tab.discarded {
                return;
            }
            tab.discarded = false;
            tab.url.clone()
        };
        let Some(url) = url else {
            return;
        };
        let _ = app.borrow().log.record("tab_restored");
        Self::navigate_tab(app, id, &url);
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
    fn memory_pressure_needs_high_load_or_under_512_mib_free() {
        const MIB: u64 = 1024 * 1024;
        assert!(!memory_pressure(84, 600 * MIB));
        assert!(memory_pressure(85, 600 * MIB));
        assert!(memory_pressure(0, 511 * MIB));
        assert!(!memory_pressure(0, 512 * MIB));
        assert!(memory_pressure(100, 0));
    }

    #[test]
    fn discard_requires_a_suspended_inactive_unpinned_idle_web_tab_that_is_silent() {
        let eligible = DiscardCandidate {
            active: false,
            pinned: false,
            suspended: true,
            loading: false,
            committed_web_url: true,
        };
        assert!(tab_discard_eligible(eligible, || false));
        let excluded = [
            (
                "active",
                DiscardCandidate {
                    active: true,
                    ..eligible
                },
            ),
            (
                "pinned",
                DiscardCandidate {
                    pinned: true,
                    ..eligible
                },
            ),
            (
                "not suspended",
                DiscardCandidate {
                    suspended: false,
                    ..eligible
                },
            ),
            (
                "loading",
                DiscardCandidate {
                    loading: true,
                    ..eligible
                },
            ),
            (
                "no committed web URL",
                DiscardCandidate {
                    committed_web_url: false,
                    ..eligible
                },
            ),
        ];
        for (name, tab) in excluded {
            let mut audio_queried = false;
            assert!(
                !tab_discard_eligible(tab, || {
                    audio_queried = true;
                    false
                }),
                "{name} tabs must be kept"
            );
            assert!(!audio_queried, "{name} is decided before the audio query");
        }
        assert!(
            !tab_discard_eligible(eligible, || true),
            "audible tabs must be kept"
        );
    }

    fn hidden_web_tab(app: &AppRef, id: TabId, address: &str, hidden_since: Instant) {
        let mut state = app.borrow_mut();
        let tab = state.tabs.iter_mut().find(|tab| tab.id == id).unwrap();
        tab.url = Some(url::Url::parse(address).unwrap());
        tab.controller_created = true;
        tab.hidden_since = Some(hidden_since);
        tab.suspended = true;
    }

    fn closed_tabs(calls: &Rc<RefCell<Vec<String>>>) -> Vec<String> {
        calls
            .borrow()
            .iter()
            .filter(|call| call.starts_with("close:"))
            .cloned()
            .collect()
    }

    #[test]
    fn memory_pressure_discards_the_longest_hidden_tab_once_per_tick() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let older = App::active_tab_id(&app);
        let newer = App::new_tab(&app);
        let _active = App::new_tab(&app);
        let base = Instant::now();
        hidden_web_tab(&app, older, "https://older.example.test/", base);
        hidden_web_tab(
            &app,
            newer,
            "https://newer.example.test/",
            base + Duration::from_secs(5),
        );
        app.borrow_mut().engine = Some(Box::new(TabRecordingEngine {
            calls: calls.clone(),
            playing: Vec::new(),
        }));
        app.borrow_mut().benchmark_memory_pressure = true;

        App::idle_timer_fired(&app, 0, true);
        assert_eq!(closed_tabs(&calls), [format!("close:{}", older.0)]);
        {
            let state = app.borrow();
            let tab = state.tabs.iter().find(|tab| tab.id == older).unwrap();
            assert!(tab.discarded && !tab.controller_created && !tab.suspended);
            assert_eq!(
                tab.url.as_ref().map(url::Url::as_str),
                Some("https://older.example.test/")
            );
        }

        App::idle_timer_fired(&app, 0, true);
        assert_eq!(
            closed_tabs(&calls),
            [format!("close:{}", older.0), format!("close:{}", newer.0)]
        );
        App::idle_timer_fired(&app, 0, true);
        assert_eq!(
            closed_tabs(&calls).len(),
            2,
            "the active tab is never discarded"
        );
        assert_eq!(App::tab_count(&app), 3);
    }

    fn sleeping_app(sleep_after_secs: u64) -> (AppRef, TabId, TabId, Rc<RefCell<Vec<String>>>) {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let hidden = App::active_tab_id(&app);
        let active = App::new_tab(&app);
        hidden_web_tab(
            &app,
            hidden,
            "https://hidden.example.test/",
            Instant::now() - Duration::from_secs(400),
        );
        {
            let mut state = app.borrow_mut();
            state.settings = crate::settings::Settings {
                sleep_enabled: true,
                sleep_after_secs,
                ..Default::default()
            };
            state.engine = Some(Box::new(TabRecordingEngine {
                calls: calls.clone(),
                playing: Vec::new(),
            }));
        }
        (app, hidden, active, calls)
    }

    #[test]
    fn hidden_tab_sleeps_after_the_configured_delay_without_memory_pressure() {
        let (app, hidden, _active, calls) = sleeping_app(300);
        App::discard_one_hidden_tab_if(&app, || false);
        assert_eq!(closed_tabs(&calls), [format!("close:{}", hidden.0)]);
        assert!(
            app.borrow()
                .tabs
                .iter()
                .any(|t| t.id == hidden && t.discarded)
        );

        let (app, _hidden, _active, calls) = sleeping_app(3600);
        App::discard_one_hidden_tab_if(&app, || false);
        assert!(closed_tabs(&calls).is_empty(), "not hidden long enough");

        let (app, _hidden, _active, calls) = sleeping_app(300);
        app.borrow_mut().settings.sleep_enabled = false;
        App::discard_one_hidden_tab_if(&app, || false);
        assert!(closed_tabs(&calls).is_empty(), "sleep disabled");
    }

    #[test]
    fn sleep_timer_wakes_for_the_earliest_hidden_tab_only_when_enabled() {
        let (app, hidden, _active, _calls) = sleeping_app(500);
        let wait = App::sleep_wait_ms(&app).unwrap();
        assert!((90_000..=100_000).contains(&wait), "{wait}");
        app.borrow_mut().settings.sleep_enabled = false;
        assert_eq!(App::sleep_wait_ms(&app), None);
        app.borrow_mut().settings.sleep_enabled = true;
        app.borrow_mut().tab_model.set_pinned(hidden, true);
        assert_eq!(App::sleep_wait_ms(&app), None, "pinned tabs never sleep");
    }

    #[test]
    fn page_with_unsaved_input_or_media_keeps_the_tab_awake_and_is_not_asked_again_soon() {
        let (app, hidden, _active, calls) = sleeping_app(300);
        SLEEP_CHECK.with(|check| *check.borrow_mut() = SleepCheck::Inline(true));
        App::discard_one_hidden_tab_if(&app, || false);
        assert!(closed_tabs(&calls).is_empty());
        assert!(
            app.borrow()
                .tabs
                .iter()
                .any(|t| t.id == hidden && !t.discarded)
        );
        let asked = SLEEP_CHECKS_ASKED.with(|asked| asked.get());
        assert_eq!(asked, 1);
        App::discard_one_hidden_tab_if(&app, || false);
        assert_eq!(SLEEP_CHECKS_ASKED.with(|a| a.get()), 1, "backed off");
        assert!(App::sleep_wait_ms(&app).is_some(), "still rechecked later");
    }

    #[test]
    fn late_sleep_answer_sleeps_only_a_tab_that_is_still_hidden() {
        let (app, hidden, _active, calls) = sleeping_app(300);
        SLEEP_CHECK.with(|check| *check.borrow_mut() = SleepCheck::Deferred(None));
        App::discard_one_hidden_tab_if(&app, || false);
        App::discard_one_hidden_tab_if(&app, || false);
        assert_eq!(
            SLEEP_CHECKS_ASKED.with(|a| a.get()),
            1,
            "one check in flight"
        );
        let done = SLEEP_CHECK
            .with(|check| match &mut *check.borrow_mut() {
                SleepCheck::Deferred(slot) => slot.take(),
                SleepCheck::Inline(_) => None,
            })
            .unwrap();
        // The user switches to the tab before the page answers.
        assert!(App::select_tab(&app, hidden));
        done(false);
        assert!(closed_tabs(&calls).is_empty(), "now active, so it stays");

        let (app, hidden, _active, calls) = sleeping_app(300);
        SLEEP_CHECK.with(|check| *check.borrow_mut() = SleepCheck::Deferred(None));
        App::discard_one_hidden_tab_if(&app, || false);
        let done = SLEEP_CHECK
            .with(|check| match &mut *check.borrow_mut() {
                SleepCheck::Deferred(slot) => slot.take(),
                SleepCheck::Inline(_) => None,
            })
            .unwrap();
        done(false);
        assert_eq!(closed_tabs(&calls), [format!("close:{}", hidden.0)]);
    }

    #[test]
    fn render_crash_reloads_once_then_reports_and_marks_hidden_tabs() {
        let (app, hidden, active, _calls) = sleeping_app(300);
        app.borrow_mut().settings.sleep_enabled = false;
        App::tab_render_failed(&app, hidden);
        {
            let state = app.borrow();
            let tab = state.tabs.iter().find(|t| t.id == hidden).unwrap();
            assert!(tab.discarded, "hidden tab navigates afresh when selected");
            assert!(tab.last_crash_reload.is_some());
        }
        App::tab_render_failed(&app, active);
        assert!(
            app.borrow()
                .tabs
                .iter()
                .any(|t| t.id == active && t.last_crash_reload.is_some())
        );
    }

    #[test]
    fn restoring_a_session_builds_asleep_tabs_and_skips_bad_entries() {
        let app = app_with_pending(rect(800, 600));
        app.borrow_mut().pending = None;
        app.borrow_mut().pending_tab = None;
        app.borrow_mut().initializing = true; // no engine: the active load stays pending
        let tab = |url: &str, title: &str, pinned: bool| crate::session::SessionTab {
            url: url.to_owned(),
            title: title.to_owned(),
            pinned,
        };
        app.borrow_mut().pending_restore = Some(crate::session::Session {
            tabs: vec![
                tab("https://a.test/", "A", true),
                tab("not a url", "bad", false),
                tab("https://b.test/", "B", false),
                tab("https://c.test/", "C", false),
            ],
            active: 2,
        });
        App::restore_pending_session(&app);
        let state = app.borrow();
        assert_eq!(state.tab_model.len(), 3);
        let titles: Vec<_> = state.tab_model.iter().map(|t| t.title.clone()).collect();
        assert_eq!(titles, ["A", "B", "C"]);
        assert!(state.tab_model.iter().next().unwrap().pinned);
        let active = state.tab_model.active();
        assert_eq!(state.tab_model.index_of(active), Some(1));
        let asleep: Vec<_> = state.tabs.iter().map(|t| t.discarded).collect();
        assert_eq!(asleep, [true, false, true]);
        assert_eq!(state.pending.as_ref().unwrap().as_str(), "https://b.test/");
    }

    #[test]
    fn discard_skips_pinned_audible_and_loading_tabs_for_the_next_eligible_tab() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let pinned = App::active_tab_id(&app);
        let audible = App::new_tab(&app);
        let loading = App::new_tab(&app);
        let eligible = App::new_tab(&app);
        let _active = App::new_tab(&app);
        assert!(App::set_tab_pinned(&app, pinned, true));
        let base = Instant::now();
        hidden_web_tab(&app, pinned, "https://pinned.example.test/", base);
        hidden_web_tab(
            &app,
            audible,
            "https://audible.example.test/",
            base + Duration::from_secs(1),
        );
        hidden_web_tab(
            &app,
            loading,
            "https://loading.example.test/",
            base + Duration::from_secs(2),
        );
        hidden_web_tab(
            &app,
            eligible,
            "https://eligible.example.test/",
            base + Duration::from_secs(3),
        );
        app.borrow_mut()
            .tabs
            .iter_mut()
            .find(|tab| tab.id == loading)
            .unwrap()
            .loading = true;
        app.borrow_mut().engine = Some(Box::new(TabRecordingEngine {
            calls: calls.clone(),
            playing: vec![audible],
        }));
        app.borrow_mut().benchmark_memory_pressure = true;

        App::idle_timer_fired(&app, 0, true);

        assert_eq!(closed_tabs(&calls), [format!("close:{}", eligible.0)]);
    }

    #[test]
    fn without_memory_pressure_no_suspended_tab_is_discarded() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let hidden = App::active_tab_id(&app);
        let _active = App::new_tab(&app);
        hidden_web_tab(&app, hidden, "https://hidden.example.test/", Instant::now());
        app.borrow_mut().engine = Some(Box::new(TabRecordingEngine {
            calls: calls.clone(),
            playing: Vec::new(),
        }));

        App::discard_one_hidden_tab_if(&app, || false);

        assert!(closed_tabs(&calls).is_empty());
        assert!(
            app.borrow()
                .tabs
                .iter()
                .find(|tab| tab.id == hidden)
                .unwrap()
                .controller_created
        );
    }

    #[test]
    fn minimised_window_keeps_the_discard_tick_only_under_pressure_with_an_eligible_tab() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let hidden = App::active_tab_id(&app);
        let _active = App::new_tab(&app);
        hidden_web_tab(&app, hidden, "https://hidden.example.test/", Instant::now());
        app.borrow_mut()
            .tabs
            .iter_mut()
            .find(|tab| tab.id == hidden)
            .unwrap()
            .suspended = false;
        app.borrow_mut().engine = Some(Box::new(TabRecordingEngine {
            calls: calls.clone(),
            playing: Vec::new(),
        }));
        app.borrow_mut().minimized = true;

        assert_eq!(
            App::combine_with_discard_tick(&app, IdleTimerAction::Kill, || true),
            IdleTimerAction::Arm(DISCARD_TICK_MS)
        );
        assert!(
            closed_tabs(&calls).is_empty(),
            "arming never discards by itself"
        );
    }

    #[test]
    fn a_shorter_visible_arm_is_kept_under_pressure() {
        let app = app_with_pending(rect(800, 600));
        let hidden = App::active_tab_id(&app);
        let _active = App::new_tab(&app);
        hidden_web_tab(&app, hidden, "https://hidden.example.test/", Instant::now());

        assert_eq!(
            App::combine_with_discard_tick(&app, IdleTimerAction::Arm(3_000), || true),
            IdleTimerAction::Arm(3_000)
        );
    }

    #[test]
    fn minimised_window_without_pressure_stays_killed() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let hidden = App::active_tab_id(&app);
        let _active = App::new_tab(&app);
        hidden_web_tab(&app, hidden, "https://hidden.example.test/", Instant::now());
        app.borrow_mut().engine = Some(Box::new(TabRecordingEngine {
            calls: calls.clone(),
            playing: Vec::new(),
        }));
        app.borrow_mut().minimized = true;

        assert_eq!(
            App::combine_with_discard_tick(&app, IdleTimerAction::Kill, || false),
            IdleTimerAction::Kill
        );
    }

    #[test]
    fn pressure_without_an_eligible_tab_leaves_the_minimised_window_killed() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let audible = App::active_tab_id(&app);
        let _active = App::new_tab(&app);
        hidden_web_tab(
            &app,
            audible,
            "https://audible.example.test/",
            Instant::now(),
        );
        app.borrow_mut().engine = Some(Box::new(TabRecordingEngine {
            calls: calls.clone(),
            playing: vec![audible],
        }));
        app.borrow_mut().minimized = true;

        assert_eq!(
            App::combine_with_discard_tick(&app, IdleTimerAction::Kill, || true),
            IdleTimerAction::Kill
        );
        assert!(closed_tabs(&calls).is_empty());
    }

    #[test]
    fn minimised_window_with_only_the_active_tab_stays_killed_under_pressure() {
        let app = app_with_pending(rect(800, 600));
        app.borrow_mut().minimized = true;

        assert_eq!(
            App::combine_with_discard_tick(&app, IdleTimerAction::Kill, || panic!(
                "memory must not be sampled without a hidden candidate"
            )),
            IdleTimerAction::Kill
        );
    }

    #[test]
    fn minimised_hidden_tab_is_suspended_then_discarded_on_following_pressure_tick() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let hidden = App::active_tab_id(&app);
        let _active = App::new_tab(&app);
        let hidden_since = Instant::now() - Duration::from_secs(29);
        hidden_web_tab(&app, hidden, "https://hidden.example.test/", hidden_since);
        app.borrow_mut()
            .tabs
            .iter_mut()
            .find(|tab| tab.id == hidden)
            .unwrap()
            .suspended = false;
        app.borrow_mut().engine = Some(Box::new(TabRecordingEngine {
            calls: calls.clone(),
            playing: Vec::new(),
        }));
        app.borrow_mut().minimized = true;
        app.borrow_mut().benchmark_memory_pressure = true;

        assert_eq!(
            App::idle_timer_fired(&app, 0, false),
            IdleTimerAction::Arm(DISCARD_TICK_MS)
        );
        assert!(
            app.borrow()
                .tabs
                .iter()
                .find(|tab| tab.id == hidden)
                .unwrap()
                .controller_created
        );
        assert!(
            !app.borrow()
                .tabs
                .iter()
                .find(|tab| tab.id == hidden)
                .unwrap()
                .suspended
        );
        app.borrow_mut()
            .tabs
            .iter_mut()
            .find(|tab| tab.id == hidden)
            .unwrap()
            .hidden_since = Some(Instant::now() - Duration::from_secs(31));

        assert_eq!(App::idle_timer_fired(&app, 0, false), IdleTimerAction::Kill);
        assert_eq!(
            *calls.borrow(),
            [
                "suspend:30000".to_owned(),
                "suspend:30000".to_owned(),
                format!("close:{}", hidden.0),
            ]
        );
        let state = app.borrow();
        let tab = state.tabs.iter().find(|tab| tab.id == hidden).unwrap();
        assert!(tab.discarded);
        assert!(!tab.controller_created);
    }

    #[test]
    fn pressure_is_not_sampled_without_a_suspended_hidden_tab() {
        let app = app_with_pending(rect(800, 600));
        app.borrow_mut().minimized = true;

        assert_eq!(
            App::combine_with_discard_tick(&app, IdleTimerAction::Kill, || panic!(
                "memory must not be sampled"
            )),
            IdleTimerAction::Kill
        );
    }

    #[test]
    fn background_window_discards_under_pressure_and_stops_once_nothing_is_eligible() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let older = App::active_tab_id(&app);
        let newer = App::new_tab(&app);
        let _active = App::new_tab(&app);
        let base = Instant::now();
        hidden_web_tab(&app, older, "https://older.example.test/", base);
        hidden_web_tab(
            &app,
            newer,
            "https://newer.example.test/",
            base + Duration::from_secs(5),
        );
        app.borrow_mut().engine = Some(Box::new(TabRecordingEngine {
            calls: calls.clone(),
            playing: Vec::new(),
        }));
        app.borrow_mut().benchmark_memory_pressure = true;

        assert_eq!(
            App::idle_timer_fired(&app, 0, false),
            IdleTimerAction::Arm(DISCARD_TICK_MS)
        );
        assert_eq!(closed_tabs(&calls), [format!("close:{}", older.0)]);

        assert_eq!(App::idle_timer_fired(&app, 0, false), IdleTimerAction::Kill);
        assert_eq!(
            closed_tabs(&calls),
            [format!("close:{}", older.0), format!("close:{}", newer.0)]
        );
    }

    #[test]
    fn selecting_a_discarded_tab_restores_its_committed_url_and_logs_both_transitions() {
        let path =
            std::env::temp_dir().join(format!("bowser-discard-restore-{}.csv", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let log = Rc::new(PerfLog::new(Some(&path)).unwrap());
        let app = app_with_pending(rect(800, 600));
        app.borrow_mut().log = log.clone();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let second = App::new_tab(&app);
        let _active = App::new_tab(&app);
        hidden_web_tab(
            &app,
            second,
            "https://second.example.test/page",
            Instant::now(),
        );
        app.borrow_mut()
            .tabs
            .iter_mut()
            .find(|tab| tab.id == second)
            .unwrap()
            .title = "Second page".to_owned();
        app.borrow_mut().engine = Some(Box::new(TabRecordingEngine {
            calls: calls.clone(),
            playing: Vec::new(),
        }));
        app.borrow_mut().benchmark_memory_pressure = true;
        App::idle_timer_fired(&app, 0, true);
        assert_eq!(closed_tabs(&calls), [format!("close:{}", second.0)]);
        calls.borrow_mut().clear();

        assert!(App::select_tab(&app, second));

        assert_eq!(
            *calls.borrow(),
            [
                format!("select:{}", second.0),
                format!("navigate:{}:https://second.example.test/page", second.0),
            ]
        );
        assert_eq!(App::tab_title(&app, second).as_deref(), Some("Second page"));
        assert_eq!(
            App::tab_committed_url(&app, second).unwrap().as_str(),
            "https://second.example.test/page"
        );
        assert!(App::tab_loading(&app, second));
        assert!(!App::tab_can_go_back(&app, second));
        {
            let state = app.borrow();
            let tab = state.tabs.iter().find(|tab| tab.id == second).unwrap();
            assert!(tab.controller_created && !tab.discarded);
            assert!(tab.hidden_since.is_none());
        }
        drop(app);
        drop(log);

        let events = std::fs::read_to_string(&path).unwrap();
        assert!(events.contains("tab_discarded,"));
        assert!(events.contains("tab_restored,"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn closing_the_active_tab_restores_a_discarded_neighbour_it_activates() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        assert!(App::select_tab(&app, first));
        hidden_web_tab(&app, second, "https://second.example.test/", Instant::now());
        app.borrow_mut().engine = Some(Box::new(TabRecordingEngine {
            calls: calls.clone(),
            playing: Vec::new(),
        }));
        app.borrow_mut().benchmark_memory_pressure = true;
        App::idle_timer_fired(&app, 0, true);
        calls.borrow_mut().clear();

        assert!(App::close_tab(&app, first));

        assert_eq!(App::active_tab_id(&app), second);
        assert_eq!(
            *calls.borrow(),
            [
                format!("close:{}", first.0),
                format!("select:{}", second.0),
                format!("navigate:{}:https://second.example.test/", second.0),
            ]
        );
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
    fn pinning_moves_both_tab_models_without_touching_active_webview() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        let third = App::new_tab(&app);
        assert!(App::select_tab(&app, second));

        assert!(App::set_tab_pinned(&app, third, true));

        assert_eq!(App::active_tab_id(&app), second);
        assert_eq!(App::tab_ids(&app), vec![third, first, second]);
        assert!(App::tab_pinned(&app, third));
        let state = app.borrow();
        assert_eq!(
            state.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(),
            vec![third, first, second]
        );
        assert!(
            !state
                .tabs
                .iter()
                .find(|tab| tab.id == third)
                .unwrap()
                .controller_created
        );
    }

    #[test]
    fn selecting_a_hidden_group_member_expands_the_domain_and_preserves_lazy_controllers() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        let third = App::new_tab(&app);
        {
            let mut state = app.borrow_mut();
            for (id, address) in [
                (first, "https://a.example.com/"),
                (second, "https://b.example.com/"),
                (third, "https://other.test/"),
            ] {
                let url = url::Url::parse(address).unwrap();
                state.tab_model.set_url(id, Some(address));
                state.tabs.iter_mut().find(|tab| tab.id == id).unwrap().url = Some(url);
            }
            let key = crate::tab_groups::DomainKey::parse("example.com").unwrap();
            state.group_preferences.set_collapsed(&key, true).unwrap();
        }

        assert!(App::select_tab(&app, second));
        assert_eq!(App::active_tab_id(&app), second);
        assert!(
            !App::tab_group_preferences(&app)
                .is_collapsed(&crate::tab_groups::DomainKey::parse("example.com").unwrap())
        );
        assert!(app.borrow().tabs.iter().all(|tab| !tab.controller_created));
    }

    #[test]
    fn an_active_group_member_stays_selected_when_its_group_collapses() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        assert!(App::select_tab(&app, first));
        {
            let mut state = app.borrow_mut();
            for (id, address) in [
                (first, "https://a.example.com/"),
                (second, "https://b.example.com/"),
            ] {
                let url = url::Url::parse(address).unwrap();
                state.tab_model.set_url(id, Some(address));
                state.tabs.iter_mut().find(|tab| tab.id == id).unwrap().url = Some(url);
            }
            let key = crate::tab_groups::DomainKey::parse("example.com").unwrap();
            state.group_preferences.set_collapsed(&key, true).unwrap();
        }

        assert!(App::select_tab(&app, first));
        assert_eq!(App::active_tab_id(&app), first);
        assert!(
            App::tab_group_preferences(&app)
                .is_collapsed(&crate::tab_groups::DomainKey::parse("example.com").unwrap())
        );
    }

    #[test]
    fn manual_group_mode_is_scoped_to_the_committed_registrable_domain() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        let third = App::new_tab(&app);
        {
            let mut state = app.borrow_mut();
            for (id, address) in [
                (first, "https://a.example.com/"),
                (second, "http://b.example.com:8080/"),
                (third, "https://example.net/"),
            ] {
                let url = url::Url::parse(address).unwrap();
                state.tab_model.set_url(id, Some(address));
                state.tabs.iter_mut().find(|tab| tab.id == id).unwrap().url = Some(url);
            }
        }

        assert!(App::set_tab_group_mode(
            &app,
            first,
            crate::tab_groups::GroupMode::Always
        ));
        let preferences = App::tab_group_preferences(&app);
        let example = crate::tab_groups::DomainKey::parse("example.com").unwrap();
        assert_eq!(
            preferences.mode(&example),
            crate::tab_groups::GroupMode::Always
        );
        assert!(preferences.forms_group(&example, 1));
        assert_eq!(
            preferences.mode(&crate::tab_groups::DomainKey::parse("example.net").unwrap()),
            crate::tab_groups::GroupMode::Auto
        );
        assert!(!App::set_tab_group_mode(
            &app,
            second,
            crate::tab_groups::GroupMode::Always
        ));
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
        playing: Vec<TabId>,
    }

    enum SleepCheck {
        Inline(bool),
        Deferred(Option<Box<dyn FnOnce(bool)>>),
    }

    thread_local! {
        static SLEEP_CHECK: RefCell<SleepCheck> = const { RefCell::new(SleepCheck::Inline(false)) };
        static SLEEP_CHECKS_ASKED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    impl EngineAdapter for TabRecordingEngine {
        fn check_sleep_blocked(&mut self, _tab: TabId, done: Box<dyn FnOnce(bool)>) {
            SLEEP_CHECKS_ASKED.with(|asked| asked.set(asked.get() + 1));
            SLEEP_CHECK.with(|check| match &mut *check.borrow_mut() {
                SleepCheck::Inline(blocked) => done(*blocked),
                SleepCheck::Deferred(slot) => *slot = Some(done),
            });
        }
        fn tab_playing_audio(&self, tab: TabId) -> bool {
            self.playing.contains(&tab)
        }
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
            playing: Vec::new(),
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
        App::idle_timer_fired(&app, 0, true);

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
            benchmark_memory_pressure: false,
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
            group_preferences: crate::tab_groups::GroupPreferences::default(),
            group_preferences_path: None,
            // Existing tests exercise the memory-pressure path; tab sleep has its own tests.
            settings: crate::settings::Settings {
                sleep_enabled: false,
                ..Default::default()
            },
            session: None,
            pending_restore: None,
            blocker: None,
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
                playing: Vec::new(),
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
            None,
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
    fn background_policy_trims_loaded_pages_without_polling() {
        for (minimized, loaded, target) in [
            (false, true, MemoryUsageTarget::Low),
            (true, false, MemoryUsageTarget::Low),
            (false, false, MemoryUsageTarget::Normal),
        ] {
            assert_eq!(
                background_memory_decision(minimized, loaded),
                IdleMemoryDecision {
                    target,
                    timer: IdleTimerAction::Kill,
                }
            );
        }
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
            App::idle_timer_fired(&app, 3_000, true),
            IdleTimerAction::Arm(1_000)
        );
        assert_eq!(
            App::idle_timer_fired(&app, 2_000, true),
            IdleTimerAction::Arm(1_000)
        );
        assert_eq!(
            App::idle_timer_fired(&app, 1_000, true),
            IdleTimerAction::Arm(3_000)
        );
        assert_eq!(*calls.borrow(), ["target:Low", "target:Normal"]);
        // Recent system-wide input in another application neither restores nor keeps polling.
        assert_eq!(App::idle_timer_fired(&app, 0, false), IdleTimerAction::Kill);
        assert_eq!(
            calls.borrow().last().map(String::as_str),
            Some("target:Low")
        );
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
