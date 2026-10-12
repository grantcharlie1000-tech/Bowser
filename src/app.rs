use std::{
    cell::RefCell,
    collections::HashMap,
    hash::Hash,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use crate::{
    chrome::{
        island_menu::{IslandModel, IslandRow},
        tab_card::{self, CardModel, CardRow, Marker, TabCardInput},
    },
    tab_groups::{self, TabGroupKey},
    tab_organisation::{self, InFlightDomains, Membership, Snapshot, UndoStack},
};

thread_local! {
    static TAB_CHANGE_POSTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static TAB_CHANGE_FLAGS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

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
    site_security::{self, LoadState, SiteSecurity},
    split_view::{Pane, SplitView},
    tabs::TabId,
};

use crate::{
    engine::{self, EngineAdapter, MemoryTargetApply, MemoryUsageTarget, TrackingPreventionLevel},
    win32,
};
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment;

/// What the HTML chrome can expect from the tab environment (see `App::request_chrome_environment`).
pub enum ChromeEnvironment {
    /// The environment exists and has the `bowser-ui` scheme registered.
    Ready(ICoreWebView2Environment),
    /// The engine is being created; `win32::WM_HTML_CHROME_ENV` is posted when it settles.
    Pending,
    /// No usable environment (creation failed, the scheme could not be registered, or closing).
    Unavailable,
}

/// Pure decision behind `App::request_chrome_environment`, given whether an environment is ready.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChromeEnvironmentStep {
    Ready,
    Wait,
    StartEngine,
    Unavailable,
}

pub fn chrome_environment_step(
    ready: bool,
    closing: bool,
    settled: bool,
    initializing: bool,
) -> ChromeEnvironmentStep {
    if closing {
        ChromeEnvironmentStep::Unavailable
    } else if ready {
        ChromeEnvironmentStep::Ready
    } else if settled {
        ChromeEnvironmentStep::Unavailable
    } else if initializing {
        ChromeEnvironmentStep::Wait
    } else {
        ChromeEnvironmentStep::StartEngine
    }
}

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

/// Marker a tab shows. A grouped tab uses only its group's entry, so every member agrees; a tab in
/// no group uses its own entry. A tab that leaves a group shows its own entry again.
fn resolve_marker(
    key: Option<&TabGroupKey>,
    id: TabId,
    groups: &HashMap<TabGroupKey, Marker>,
    tabs: &HashMap<TabId, Marker>,
) -> Option<Marker> {
    match key {
        Some(key) => groups.get(key).copied(),
        None => tabs.get(&id).copied(),
    }
}

/// Stores or clears one marker entry. Returns whether the stored value changed.
fn set_entry<K: Eq + Hash>(map: &mut HashMap<K, Marker>, key: K, marker: Option<Marker>) -> bool {
    match marker {
        Some(marker) => map.insert(key, marker) != Some(marker),
        None => map.remove(&key).is_some(),
    }
}

/// One tab as the hover card reads it.
struct CardTab<'a> {
    id: TabId,
    title: &'a str,
    opener: Option<TabId>,
    has_favicon: bool,
}

/// Rows of a hover card: every other tab in the hovered tab's group, in model order. Empty when the
/// hovered tab is in no group. A row is linked when one of the two tabs opened the other.
fn card_rows(
    tabs: &[CardTab],
    keys: &[Option<TabGroupKey>],
    hovered: usize,
    active: usize,
) -> Vec<CardRow> {
    let Some(key) = keys.get(hovered).and_then(Option::as_ref) else {
        return Vec::new();
    };
    let hovered_tab = &tabs[hovered];
    tabs.iter()
        .enumerate()
        .filter(|(index, _)| *index != hovered && keys[*index].as_ref() == Some(key))
        .map(|(index, tab)| CardRow {
            tab_index: index,
            title: if tab.title.trim().is_empty() {
                tab_card::NEW_TAB_TITLE.to_owned()
            } else {
                tab.title.to_owned()
            },
            favicon_present: tab.has_favicon,
            active: index == active,
            linked: tab.opener == Some(hovered_tab.id) || hovered_tab.opener == Some(tab.id),
        })
        .collect()
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
    pending_login_messages: Vec<(TabId, crate::passwords::LoginMessage)>,
    pub pending: Option<url::Url>,
    pending_tab: Option<TabId>,
    pub initializing: bool,
    prewarm_requested: bool,
    /// HTML chrome mode: the tab environment, once the engine has been created with the
    /// `bowser-ui` scheme. The chrome views are hosted in it, in their own private profile.
    chrome_environment: Option<ICoreWebView2Environment>,
    /// The engine's first creation attempt has finished, successfully or not. Until then the HTML
    /// chrome waits for the environment; afterwards a missing environment means native chrome.
    engine_creation_settled: bool,
    pub closing: bool,
    /// When the window title last changed for blocked-request counts.
    title_synced_at: Option<Instant>,
    /// A blocked-count title change is waiting for the throttle timer.
    title_dirty: bool,
    /// The throttle timer is set.
    title_timer_armed: bool,
    pub loop_shutdown_pending: bool,
    pub user_data_dir: PathBuf,
    pub benchmark_url: Option<url::Url>,
    initial_url: Option<url::Url>,
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
    /// Visible pane relationship. Session recovery intentionally restores its tabs as a normal view.
    split_view: Option<SplitView>,
    split_bounds: Option<(
        windows::Win32::Foundation::RECT,
        windows::Win32::Foundation::RECT,
    )>,
    group_preferences: crate::tab_groups::GroupPreferences,
    group_preferences_path: Option<PathBuf>,
    /// Marker per tab group, keyed by group identity. In memory only: markers are not persisted.
    group_markers: HashMap<TabGroupKey, Marker>,
    /// Marker per tab that is in no group. In memory only, and dropped when the tab closes.
    tab_markers: HashMap<crate::tabs::TabId, Marker>,
    /// Manual placement of tabs into or out of domain groups. In memory only: tab ids do not
    /// survive a restart, and the per-domain preferences above are what persist.
    memberships: HashMap<TabId, Membership>,
    /// Pre-navigation domain of tabs whose navigation is in flight; see `InFlightDomains`.
    in_flight_domains: InFlightDomains,
    /// Group preference and placement steps for Undo tab organisation. Collapse is not a step.
    organisation_undo: UndoStack<Snapshot>,
    workspaces: crate::workspaces::WorkspaceSet,
    workspaces_path: Option<PathBuf>,
    settings: crate::settings::Settings,
    session: Option<crate::session::SessionWriter>,
    session_save_pending: bool,
    last_session_snapshot: Option<crate::session::Session>,
    pending_restore: Option<crate::session::Session>,
    privacy_stats: Option<crate::privacy_stats::PrivacyStats>,
    privacy_stats_path: Option<PathBuf>,
    privacy_session_blocked: u64,
    /// Ad/tracker blocker shared with the engine; `None` for benchmark launches or when disabled.
    blocker: Option<engine::BlockerHandle>,
    /// Stored per-site permission decisions; `None` for benchmark launches, which deny every request.
    permissions: Option<engine::PermissionsHandle>,
}

struct AppTab {
    id: TabId,
    url: Option<url::Url>,
    title: String,
    loading: bool,
    progress: NavigationProgress,
    favicon: Option<Vec<u8>>,
    favicon_key: Option<crate::tabs::FaviconKey>,
    can_back: bool,
    can_forward: bool,
    page_usable: bool,
    controller_created: bool,
    muted: bool,
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
    /// Outcome of the last top-level navigation, for the connection state in Site information.
    load: LoadState,
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
            favicon_key: None,
            can_back: false,
            can_forward: false,
            page_usable: false,
            controller_created: false,
            muted: false,
            hidden_since: None,
            suspended: false,
            discarded: false,
            sleep_check_started: None,
            sleep_blocked_until: None,
            last_crash_reload: None,
            blocked: 0,
            load: LoadState::Loaded,
        }
    }
}

pub type AppRef = Rc<RefCell<App>>;

/// Window timer for the throttled blocked-count title. Timer ids 1 to 4 are taken in win32/mod.rs.
pub(crate) const BLOCKED_TITLE_TIMER: usize = 5;
/// Shortest gap between blocked-count title refreshes (about four a second).
const TITLE_SYNC_INTERVAL: Duration = Duration::from_millis(250);
/// Name of the file mapping Bowser extension ids to WebView2 ids, beside the records file.
const EXTENSION_IDS_FILE_NAME: &str = "extension_ids.txt";

/// How long until the next blocked-count title refresh, given the time since the last one.
/// None means refresh now.
fn title_wait(since_last: Option<Duration>) -> Option<Duration> {
    match since_last {
        Some(elapsed) if elapsed < TITLE_SYNC_INTERVAL => Some(TITLE_SYNC_INTERVAL - elapsed),
        _ => None,
    }
}

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

/// Every installed extension as (id, enabled), and the file mapping them to WebView2 ids. None
/// when this window does not use the profile's extensions, or the records cannot be read.
fn extension_records(
    app: &AppRef,
    user_data_dir: &std::path::Path,
) -> Option<(Vec<(String, bool)>, std::path::PathBuf)> {
    if app.borrow().benchmark_url.is_some() {
        return None;
    }
    let profile = crate::profile::active().ok()?;
    if profile.webview2_dir() != user_data_dir {
        return None;
    }
    let records = crate::extensions::read_records(&profile.extensions_file()).ok()?;
    let known = records
        .into_iter()
        .map(|record| (record.id, record.enabled))
        .collect();
    let ids_file = profile
        .extensions_file()
        .with_file_name(EXTENSION_IDS_FILE_NAME);
    Some((known, ids_file))
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

    /// Whether the blocker is acting on the active tab's site: `None` when the blocker is
    /// disabled or the tab has no site, else `Some(true)` while blocking and `Some(false)` once
    /// the site is exempted.
    pub fn blocker_active_for_site(app: &AppRef) -> Option<bool> {
        let state = app.borrow();
        let blocker = state.blocker.as_ref()?;
        let host = state
            .tabs
            .iter()
            .find(|tab| tab.id == state.tab_model.active())
            .and_then(|tab| tab.url.as_ref())
            .and_then(|url| url.host_str())?;
        Some(!blocker.borrow().site_allowed(host))
    }

    pub fn tab_layout(app: &AppRef) -> crate::settings::TabLayout {
        app.borrow().settings.tab_layout
    }

    pub fn sidebar_width(app: &AppRef) -> u16 {
        app.borrow().settings.sidebar_width
    }

    pub fn set_sidebar_width(app: &AppRef, width: u16) {
        let hwnd = {
            let mut state = app.borrow_mut();
            state.settings.sidebar_width = width.clamp(
                crate::settings::MIN_SIDEBAR_WIDTH,
                crate::settings::MAX_SIDEBAR_WIDTH,
            );
            state.hwnd
        };
        let bounds = app.borrow_mut().latest_viewport_bounds();
        Self::resize_current(app, bounds);
        Self::post_tab_changed(hwnd, Self::active_tab_id(app));
    }

    pub fn save_settings(app: &AppRef) {
        if let Ok(profile) = crate::profile::active() {
            let mut settings = app.borrow().settings.clone();
            settings.side_panel_width =
                crate::settings::Settings::load(&profile.settings_file()).side_panel_width;
            if let Err(error) = crate::settings::save(&profile.settings_file(), &settings) {
                win32::show_error_message(&format!("Settings could not be saved: {error}"));
            }
        }
    }

    pub fn workspaces(app: &AppRef) -> crate::workspaces::WorkspaceSet {
        app.borrow().workspaces.clone()
    }

    /// Ordered workspace metadata for the rail and workspace pickers.
    pub fn workspace_entries(
        app: &AppRef,
    ) -> Vec<(
        crate::workspaces::WorkspaceId,
        String,
        crate::workspaces::WorkspaceIcon,
        Option<crate::workspaces::WorkspaceColour>,
        usize,
        bool,
    )> {
        let state = app.borrow();
        let active = state.workspaces.active_id();
        state
            .workspaces
            .all()
            .iter()
            .map(|workspace| (
                workspace.id(),
                workspace.name().to_owned(),
                workspace.icon(),
                workspace.colour(),
                workspace.tabs().len(),
                workspace.id() == active,
            ))
            .collect()
    }

    pub fn workspace_set_icon(
        app: &AppRef,
        id: crate::workspaces::WorkspaceId,
        icon: crate::workspaces::WorkspaceIcon,
    ) -> std::result::Result<(), crate::workspaces::WorkspaceError> {
        app.borrow_mut().workspaces.set_icon(id, icon)?;
        Self::save_workspaces(app);
        Self::notify_rail_changed(app);
        Ok(())
    }

    pub fn workspace_set_colour(
        app: &AppRef,
        id: crate::workspaces::WorkspaceId,
        colour: Option<crate::workspaces::WorkspaceColour>,
    ) -> std::result::Result<(), crate::workspaces::WorkspaceError> {
        app.borrow_mut().workspaces.set_colour(id, colour)?;
        Self::save_workspaces(app);
        Self::notify_rail_changed(app);
        Ok(())
    }

    pub fn workspace_move(
        app: &AppRef,
        id: crate::workspaces::WorkspaceId,
        new_index: usize,
    ) -> std::result::Result<(), crate::workspaces::WorkspaceError> {
        app.borrow_mut().workspaces.reorder(id, new_index)?;
        Self::save_workspaces(app);
        Self::notify_rail_changed(app);
        Ok(())
    }

    pub fn workspace_create(
        app: &AppRef,
        name: &str,
    ) -> std::result::Result<crate::workspaces::WorkspaceId, crate::workspaces::WorkspaceError> {
        Self::create_workspace(app, name)
    }

    fn notify_rail_changed(app: &AppRef) {
        let hwnd = app.borrow().hwnd;
        win32::post(hwnd, win32::WM_RAIL_CHANGED, Default::default(), Default::default());
    }

    pub fn switch_workspace(app: &AppRef, id: crate::workspaces::WorkspaceId) -> bool {
        let selected = {
            let mut state = app.borrow_mut();
            match state.workspaces.switch_to(id) {
                Ok(transition) => transition.selected,
                Err(_) => return false,
            }
        };
        let switched = Self::select_tab(app, selected);
        if switched {
            Self::save_workspaces(app);
            Self::notify_rail_changed(app);
            Self::schedule_tab_idle_timer(app);
        }
        switched
    }

    pub fn create_workspace(
        app: &AppRef,
        name: &str,
    ) -> std::result::Result<crate::workspaces::WorkspaceId, crate::workspaces::WorkspaceError>
    {
        let tab = Self::new_tab(app);
        let id = {
            let mut state = app.borrow_mut();
            state.workspaces.remove_tab(tab)?;
            match state.workspaces.create(name, tab) {
                Ok(id) => id,
                Err(error) => {
                    let _ = state.workspaces.add_tab(tab);
                    let _ = state.workspaces.select_tab(tab);
                    return Err(error);
                }
            }
        };
        let _ = Self::switch_workspace(app, id);
        Self::save_workspaces(app);
        Self::notify_rail_changed(app);
        Ok(id)
    }

    pub fn rename_workspace(
        app: &AppRef,
        id: crate::workspaces::WorkspaceId,
        name: &str,
    ) -> std::result::Result<(), crate::workspaces::WorkspaceError> {
        app.borrow_mut().workspaces.rename(id, name)?;
        Self::save_workspaces(app);
        Self::notify_rail_changed(app);
        Self::post_tab_changed(app.borrow().hwnd, Self::active_tab_id(app));
        Ok(())
    }

    pub fn delete_workspace(
        app: &AppRef,
        id: crate::workspaces::WorkspaceId,
    ) -> std::result::Result<(), crate::workspaces::WorkspaceError> {
        let (removed, selected) = {
            let mut state = app.borrow_mut();
            let removed = state.workspaces.remove_workspace(id)?;
            (removed, state.workspaces.active().active_tab())
        };
        let _ = Self::select_tab(app, selected);
        for tab in removed {
            let _ = Self::close_tab(app, tab);
        }
        Self::save_workspaces(app);
        Self::notify_rail_changed(app);
        Ok(())
    }

    fn save_workspaces(app: &AppRef) {
        let (path, workspaces) = {
            let state = app.borrow();
            let order = state
                .tab_model
                .iter()
                .filter_map(|model| {
                    let url = state
                        .tabs
                        .iter()
                        .find(|tab| tab.id == model.id)?
                        .url
                        .as_ref()?;
                    crate::session::persistable_url(url).map(|_| model.id)
                })
                .take(crate::session::MAX_TABS)
                .collect::<Vec<_>>();
            let mut workspaces = state.workspaces.clone();
            workspaces.set_session_order(&order);
            (state.workspaces_path.clone(), workspaces)
        };
        if let Some(path) = path {
            if let Err(error) = workspaces.save(&path) {
                win32::show_error_message(&format!("Workspaces could not be saved: {error}"));
            }
        }
    }

    pub fn toggle_tab_layout(app: &AppRef) -> crate::settings::TabLayout {
        let layout = match Self::tab_layout(app) {
            crate::settings::TabLayout::Horizontal => crate::settings::TabLayout::Vertical,
            crate::settings::TabLayout::Vertical => crate::settings::TabLayout::Horizontal,
        };
        Self::apply_tab_layout_settings(app, layout, Self::sidebar_width(app));
        Self::save_settings(app);
        layout
    }

    pub fn apply_tab_layout_settings(app: &AppRef, layout: crate::settings::TabLayout, width: u16) {
        let hwnd = {
            let mut state = app.borrow_mut();
            state.settings.tab_layout = layout;
            state.settings.sidebar_width = width.clamp(
                crate::settings::MIN_SIDEBAR_WIDTH,
                crate::settings::MAX_SIDEBAR_WIDTH,
            );
            state.hwnd
        };
        let bounds = app.borrow_mut().latest_viewport_bounds();
        Self::resize_current(app, bounds);
        Self::post_tab_changed(hwnd, Self::active_tab_id(app));
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

    /// Refreshes the title for a blocked-count change. Bursts of blocked requests would otherwise
    /// rewrite the title for every request, so the refresh is at most one per
    /// `TITLE_SYNC_INTERVAL`: at once when the last one is old, else by a one-shot timer.
    fn schedule_blocked_title(app: &AppRef) {
        let (hwnd, wait) = {
            let mut state = app.borrow_mut();
            state.title_dirty = true;
            if state.title_timer_armed {
                return;
            }
            let wait = title_wait(state.title_synced_at.map(|at| at.elapsed()));
            if wait.is_some() {
                state.title_timer_armed = true;
            }
            (state.hwnd, wait)
        };
        match wait {
            None => Self::flush_blocked_title(app),
            Some(wait) => unsafe {
                windows::Win32::UI::WindowsAndMessaging::SetTimer(
                    Some(hwnd),
                    BLOCKED_TITLE_TIMER,
                    (wait.as_millis() as u32).max(1),
                    None,
                );
            },
        }
    }

    /// Runs when the throttle timer fires. It refreshes the title only if a change is waiting.
    pub(crate) fn blocked_title_timer(app: &AppRef) {
        let dirty = {
            let mut state = app.borrow_mut();
            state.title_timer_armed = false;
            state.title_dirty
        };
        if dirty {
            Self::flush_blocked_title(app);
        }
    }

    fn flush_blocked_title(app: &AppRef) {
        {
            let mut state = app.borrow_mut();
            state.title_dirty = false;
            state.title_synced_at = Some(Instant::now());
        }
        Self::sync_window_title(app);
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
        Self::post_tab_changed_kind(hwnd, id, true, true);
    }

    fn post_tab_changed_kind(hwnd: HWND, id: TabId, structural: bool, persisted: bool) -> bool {
        let flags = u32::from(structural) | (u32::from(persisted) << 1);
        let should_post = TAB_CHANGE_POSTED.with(|posted| {
            TAB_CHANGE_FLAGS.with(|pending| pending.set(pending.get() | flags));
            !posted.replace(true)
        });
        if should_post {
            win32::post(
                hwnd,
                win32::WM_TABS_CHANGED,
                windows::Win32::Foundation::WPARAM(id.0 as usize),
                windows::Win32::Foundation::LPARAM(0),
            );
        }
        should_post
    }

    pub fn take_tab_change_flags() -> (bool, bool) {
        TAB_CHANGE_POSTED.with(|posted| posted.set(false));
        let flags = TAB_CHANGE_FLAGS.with(|pending| pending.replace(0));
        (flags & 1 != 0, flags & 2 != 0)
    }

    pub fn new_tab(app: &AppRef) -> TabId {
        let (id, hwnd, old, old_has_controller, in_split) = {
            let mut state = app.borrow_mut();
            let old = state.tab_model.active();
            let in_split = state.split_view.is_some();
            let old_has_controller = state
                .tabs
                .iter()
                .find(|tab| tab.id == old)
                .is_some_and(|tab| tab.controller_created);
            let id = state.tab_model.add(false);
            state.tab_model.select(id);
            let _ = state.workspaces.add_tab(id);
            let _ = state.workspaces.select_tab(id);
            if let Some(mut split) = state.split_view {
                let _ = split.replace_focused(id);
                state.split_view = Some(split);
            }
            state.tabs.push(AppTab::empty(id));
            state.navigation_progress.reset();
            state.page_loaded = false;
            state.desired_memory_target = MemoryUsageTarget::Normal;
            (id, state.hwnd, old, old_has_controller, in_split)
        };
        if in_split {
            Self::apply_visible_panes(app);
            Self::post_tab_changed(hwnd, id);
            Self::save_workspaces(app);
            Self::sync_window_title(app);
            Self::schedule_tab_idle_timer(app);
            return id;
        }
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
        Self::save_workspaces(app);
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
            state.tab_markers.remove(&id);
            state.memberships.remove(&id);
            state.in_flight_domains.finish(id);
            if let Some(remaining) = state.split_view.and_then(|split| split.remove(id)) {
                state.split_view = None;
                state.split_bounds = None;
                state.tab_model.select(remaining);
            }
            if state.tabs.len() == 1 {
                let outcome = state.tab_model.close(id);
                let _ = state.workspaces.replace_tab(id, outcome.new_active);
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
                if let Some(workspace_id) = state.workspaces.workspace_for(id) {
                    let is_last_member = state
                        .workspaces
                        .get(workspace_id)
                        .is_some_and(|workspace| workspace.tabs().len() == 1);
                    if is_last_member && state.workspaces.workspace_count() > 1 {
                        let _ = state.workspaces.remove_workspace(workspace_id);
                    } else {
                        let _ = state.workspaces.remove_tab(id);
                    }
                }
                state.tabs.remove(index);
                let active = if was_active {
                    state.workspaces.active().active_tab()
                } else {
                    outcome.new_active
                };
                if was_active {
                    let _ = state.tab_model.select(active);
                }
                if was_active {
                    let (progress, page_usable) = state
                        .tabs
                        .iter()
                        .find(|tab| tab.id == active)
                        .map(|tab| (tab.progress, tab.page_usable))
                        .unwrap();
                    state.navigation_progress = progress;
                    state.page_loaded = page_usable;
                    state.desired_memory_target = MemoryUsageTarget::Normal;
                }
                (true, state.hwnd, active, was_active, false)
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
        Self::release_unused_site_rules(app);
        Self::save_workspaces(app);
        Self::sync_window_title(app);
        Self::schedule_tab_idle_timer(app);
        changed
    }

    pub fn select_tab(app: &AppRef, id: TabId) -> bool {
        let _ = Self::expand_group_for_tab(app, id);
        if app.borrow().split_view.is_some() {
            return Self::select_split_tab(app, id);
        }
        let (hwnd, previous, old_has_controller, new_has_controller) = {
            let mut state = app.borrow_mut();
            let previous = state.tab_model.active();
            if let Some(workspace_id) = state.workspaces.workspace_for(id) {
                let _ = state.workspaces.switch_to(workspace_id);
            }
            let was_active = state.tab_model.active() == id;
            if !state.tab_model.select(id) {
                return false;
            }
            let _ = state.workspaces.select_tab(id);
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
        Self::save_workspaces(app);
        Self::sync_window_title(app);
        Self::schedule_tab_idle_timer(app);
        true
    }

    fn select_split_tab(app: &AppRef, id: TabId) -> bool {
        let (active, hwnd) = {
            let mut state = app.borrow_mut();
            if state.tab_model.get(id).is_none() {
                return false;
            }
            let Some(mut split) = state.split_view else {
                return false;
            };
            if let Some(pane) = split.pane_for(id) {
                split.focus(pane);
            } else {
                let replaced = split.focused_tab();
                if !split.replace_focused(id) {
                    return false;
                }
                if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == replaced) {
                    tab.hidden_since = Some(Instant::now());
                    tab.suspended = false;
                }
            }
            state.split_view = Some(split);
            state.tab_model.select(id);
            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) {
                tab.hidden_since = None;
                tab.suspended = false;
            }
            if let Some((progress, page_usable)) = state
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| (tab.progress, tab.page_usable))
            {
                state.navigation_progress = progress;
                state.page_loaded = page_usable;
            }
            (id, state.hwnd)
        };
        Self::apply_visible_panes(app);
        Self::restore_discarded_tab(app, active);
        Self::post_tab_changed(hwnd, active);
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
        let ids = Self::tab_ids(app);
        let active = Self::active_tab_id(app);
        let index = ids.iter().position(|id| *id == active).unwrap_or(0);
        let next = ids[(index as isize + delta).rem_euclid(ids.len().max(1) as isize) as usize];
        if next != active {
            let _ = Self::select_tab(app, next);
        }
        next
    }

    pub fn move_tab(app: &AppRef, id: TabId, new_index: usize) -> bool {
        let hwnd = {
            let mut state = app.borrow_mut();
            let Some(index) = state.tabs.iter().position(|tab| tab.id == id) else {
                return false;
            };
            if state.workspaces.move_tab(id, new_index).is_err() {
                return false;
            }
            let ordered = state.workspaces.active().tabs();
            let target = ordered
                .iter()
                .position(|tab| *tab == id)
                .and_then(|position| {
                    ordered
                        .get(position + 1)
                        .and_then(|next| state.tab_model.index_of(*next))
                        .or_else(|| {
                            position
                                .checked_sub(1)
                                .and_then(|previous| state.tab_model.index_of(ordered[previous]))
                                .map(|previous| previous + 1)
                        })
                })
                .unwrap_or(0);
            if !state.tab_model.move_tab(id, target) {
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
        Self::save_workspaces(app);
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
            let ordered = state.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
            state.workspaces.sync_order(&ordered);
            state.hwnd
        };
        Self::post_tab_changed(hwnd, id);
        Self::save_workspaces(app);
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

    pub fn queue_login_message(app: &AppRef, tab: TabId, message: crate::passwords::LoginMessage) {
        let hwnd = {
            let mut state = app.borrow_mut();
            if state.closing || state.pending_login_messages.len() >= 32 {
                return;
            }
            state.pending_login_messages.push((tab, message));
            state.hwnd
        };
        win32::post(
            hwnd,
            win32::WM_LOGIN_MESSAGE,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(0),
        );
    }

    pub fn take_login_messages(app: &AppRef) -> Vec<(TabId, crate::passwords::LoginMessage)> {
        std::mem::take(&mut app.borrow_mut().pending_login_messages)
    }

    /// Sends an explicit user-triggered fill to the matching tab. The engine rechecks its live
    /// WebView2 source immediately before execution to reject navigations racing this call.
    pub fn execute_script_for_origin(
        app: &AppRef,
        id: TabId,
        origin: &str,
        script: &str,
    ) -> windows::core::Result<()> {
        let Some(url) = Self::tab_committed_url(app, id) else {
            return Err(windows::core::Error::from(
                windows::Win32::Foundation::E_ABORT,
            ));
        };
        if crate::passwords::secure_origin(url.as_str()).as_deref() != Some(origin) {
            return Err(windows::core::Error::from(
                windows::Win32::Foundation::E_ACCESSDENIED,
            ));
        }
        Self::with_engine(app, |engine| {
            engine.execute_script_for_origin(id, origin, script)
        })
        .unwrap_or_else(|| {
            Err(windows::core::Error::from(
                windows::Win32::Foundation::E_ABORT,
            ))
        })
    }

    pub fn tab_group_preferences(app: &AppRef) -> crate::tab_groups::GroupPreferences {
        app.borrow().group_preferences.clone()
    }

    /// Projection inputs for the tab strip, in model order, with manual placement applied.
    pub fn tab_group_inputs(app: &AppRef) -> Vec<tab_groups::TabGroupInput> {
        app.borrow().group_inputs()
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
            let before = state.organisation_snapshot();
            let Ok(changed) = state.group_preferences.set_mode(&key, mode) else {
                return false;
            };
            state.record_organisation_step(before, changed);
            (state.hwnd, state.tab_model.active(), changed)
        };
        if changed {
            Self::save_group_preferences(app);
            Self::post_tab_changed_kind(hwnd, active, true, false);
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
            Self::post_tab_changed_kind(hwnd, active, true, false);
        }
        changed
    }

    /// Moves a dragged tab to model index `to_index`, then places it: a tab dropped between two
    /// members of a domain group joins that group, and a grouped tab dropped elsewhere leaves it.
    /// Returns whether the tab moved.
    pub fn drop_tab(app: &AppRef, id: TabId, to_index: usize) -> bool {
        let current = app.borrow().group_key_of(id);
        if !Self::move_tab(app, id, to_index) {
            return false;
        }
        let placement = {
            let state = app.borrow();
            let Some(index) = state.tab_model.index_of(id) else {
                return true;
            };
            let keys = tab_groups::group_keys(&state.group_inputs(), &state.group_preferences);
            let left = index
                .checked_sub(1)
                .and_then(|left| keys.get(left))
                .and_then(Option::as_ref);
            let right = keys.get(index + 1).and_then(Option::as_ref);
            tab_organisation::drop_membership(current.as_ref(), left, right)
        };
        if let Some(placement) = placement {
            let (hwnd, active, changed) = {
                let mut state = app.borrow_mut();
                let changed = state.place_tab(id, placement);
                (state.hwnd, state.tab_model.active(), changed)
            };
            if changed {
                Self::post_tab_changed_kind(hwnd, active, true, false);
            }
        }
        true
    }

    /// Takes a tab out of its group so automatic grouping leaves it alone. Returns whether it changed.
    pub fn remove_tab_from_group(app: &AppRef, id: TabId) -> bool {
        let (hwnd, active, changed) = {
            let mut state = app.borrow_mut();
            if state.group_key_of(id).is_none() {
                return false;
            }
            let changed = state.place_tab(id, Membership::Ungrouped);
            (state.hwnd, state.tab_model.active(), changed)
        };
        if changed {
            Self::post_tab_changed_kind(hwnd, active, true, false);
        }
        changed
    }

    /// Whether the tab is in any group, so "Remove from group" has something to remove.
    pub fn tab_is_grouped(app: &AppRef, id: TabId) -> bool {
        app.borrow().group_key_of(id).is_some()
    }

    pub fn can_undo_tab_organisation(app: &AppRef) -> bool {
        !app.borrow().organisation_undo.is_empty()
    }

    /// Undoes the latest group or placement change. Collapse and expand are not steps, so the
    /// current collapsed state is kept. Returns whether there was a step to undo.
    pub fn undo_tab_organisation(app: &AppRef) -> bool {
        let (hwnd, active) = {
            let mut state = app.borrow_mut();
            let Some(snapshot) = state.organisation_undo.pop() else {
                return false;
            };
            state.group_preferences = snapshot
                .preferences
                .with_collapse_from(&state.group_preferences);
            state.memberships = snapshot.memberships;
            (state.hwnd, state.tab_model.active())
        };
        Self::save_group_preferences(app);
        Self::post_tab_changed_kind(hwnd, active, true, false);
        // The snapshot may predate the site's last tab closing, so release its rule again.
        Self::release_unused_site_rules(app);
        true
    }

    /// Island menu for the domain island that `id` belongs to: its name, colour and member rows in
    /// model order, with the member tab ids in the same order. `None` for a tab that is not in a
    /// domain island, so opener-only islands have no menu.
    pub fn island_menu(app: &AppRef, id: TabId) -> Option<(IslandModel, Vec<TabId>)> {
        let state = app.borrow();
        let key = state.domain_island_of(id)?;
        let inputs = state.group_inputs();
        let keys = tab_groups::group_keys(&inputs, &state.group_preferences);
        let active = state.tab_model.active();
        let mut rows = Vec::new();
        let mut ids = Vec::new();
        for index in tab_groups::group_members(&keys, &TabGroupKey::Domain(key.clone())) {
            let tab = inputs[index].id;
            let mirror = state.mirror(tab);
            let title = mirror.map_or("", |tab| tab.title.as_str());
            rows.push(IslandRow {
                title: if title.trim().is_empty() {
                    tab_card::NEW_TAB_TITLE.to_owned()
                } else {
                    title.to_owned()
                },
                favicon_present: mirror.is_some_and(|tab| tab.favicon_key.is_some()),
                active: tab == active,
            });
            ids.push(tab);
        }
        let model = IslandModel {
            name: state.group_preferences.name(&key).map(str::to_owned),
            suggested_name: Some(tab_organisation::suggested_name(&key)),
            colour: state.group_preferences.colour(&key),
            rows,
        };
        Some((model, ids))
    }

    /// Renames the domain island that `id` belongs to. Returns whether the stored name changed.
    pub fn set_tab_group_name(app: &AppRef, id: TabId, name: &str) -> bool {
        Self::change_island(app, id, |prefs, key| prefs.set_name(key, name))
    }

    /// Recolours the domain island that `id` belongs to; `None` restores the theme accent.
    pub fn set_tab_group_colour(app: &AppRef, id: TabId, colour: Option<u8>) -> bool {
        Self::change_island(app, id, |prefs, key| prefs.set_colour(key, colour))
    }

    /// Closes every tab in the domain island that `id` belongs to, and returns how many closed.
    pub fn close_tab_island(app: &AppRef, id: TabId) -> usize {
        let Some((_, members)) = Self::island_menu(app, id) else {
            return 0;
        };
        members
            .into_iter()
            .filter(|&tab| Self::close_tab(app, tab))
            .count()
    }

    fn change_island(
        app: &AppRef,
        id: TabId,
        change: impl FnOnce(
            &mut crate::tab_groups::GroupPreferences,
            &crate::tab_groups::DomainKey,
        ) -> std::result::Result<bool, crate::tab_groups::PreferencesError>,
    ) -> bool {
        let (hwnd, active, changed) = {
            let mut state = app.borrow_mut();
            let Some(key) = state.domain_island_of(id) else {
                return false;
            };
            let before = state.organisation_snapshot();
            let Ok(changed) = change(&mut state.group_preferences, &key) else {
                return false;
            };
            state.record_organisation_step(before, changed);
            (state.hwnd, state.tab_model.active(), changed)
        };
        if changed {
            Self::save_group_preferences(app);
            Self::post_tab_changed_kind(hwnd, active, true, false);
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
            Self::post_tab_changed_kind(hwnd, active, true, false);
        }
        changed
    }

    /// Drops `Always` rules for sites with no open unpinned tab, then saves and repaints when that
    /// changed anything. A navigation still waiting for the engine counts as open, since its site is
    /// about to be shown.
    pub fn release_unused_site_rules(app: &AppRef) {
        let (hwnd, active, released) = {
            let mut state = app.borrow_mut();
            let mut live = tab_groups::live_domains(&state.group_inputs());
            live.extend(
                state
                    .pending
                    .as_ref()
                    .and_then(tab_groups::DomainKey::from_url),
            );
            let released = state.group_preferences.release_unused_always(&live);
            (state.hwnd, state.tab_model.active(), released)
        };
        if released {
            Self::save_group_preferences(app);
            Self::post_tab_changed_kind(hwnd, active, true, false);
        }
    }

    fn save_group_preferences(app: &AppRef) {
        let state = app.borrow();
        if let Some(path) = &state.group_preferences_path
            && state.group_preferences.save(path).is_err()
        {
            let _ = state.log.record("tab_groups_preferences_save_failed");
        }
    }

    /// Inputs for the group projection, in model order, from the same state the strip reads.
    fn group_inputs(&self) -> Vec<tab_groups::TabGroupInput> {
        let mut inputs: Vec<_> = self
            .tab_model
            .iter()
            .map(|tab| {
                let committed = self
                    .mirror(tab.id)
                    .and_then(|mirror| mirror.url.as_ref())
                    .and_then(tab_groups::DomainKey::from_url);
                tab_groups::TabGroupInput {
                    id: tab.id,
                    domain: self.in_flight_domains.domain_for(tab.id, committed),
                    pinned: tab.pinned,
                    opener: tab.opener,
                    manual: false,
                }
            })
            .collect();
        tab_organisation::apply_membership(
            &mut inputs,
            &self.memberships,
            &self.group_preferences,
            self.settings.automatic_tab_grouping,
        );
        inputs
    }

    fn organisation_snapshot(&self) -> Snapshot {
        Snapshot {
            preferences: self.group_preferences.clone(),
            memberships: self.memberships.clone(),
        }
    }

    /// Records `before` as an undo step when the change it precedes actually changed something.
    fn record_organisation_step(&mut self, before: Snapshot, changed: bool) {
        if changed {
            self.organisation_undo.push(before);
        }
    }

    /// Stores a tab's placement, dropping the entry when it is back to automatic. Returns whether
    /// the placement changed.
    fn place_tab(&mut self, id: TabId, placement: Membership) -> bool {
        let before = self.organisation_snapshot();
        let changed = self.memberships.get(&id).cloned().unwrap_or_default() != placement;
        if changed {
            if placement == Membership::Automatic {
                self.memberships.remove(&id);
            } else {
                self.memberships.insert(id, placement);
            }
        }
        self.record_organisation_step(before, changed);
        changed
    }

    /// Keeps a tab grouped by the domain it had before a navigation that is about to start.
    fn begin_navigation_grouping(&mut self, id: TabId) {
        let before = self
            .mirror(id)
            .and_then(|tab| tab.url.as_ref())
            .and_then(tab_groups::DomainKey::from_url);
        self.in_flight_domains.begin(id, before);
    }

    fn mirror(&self, id: TabId) -> Option<&AppTab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    /// Group identity of one tab under the current projection, or `None` when it is in no group.
    /// Registrable domain of the domain island `id` belongs to. `None` when the tab is in no
    /// group or only in an opener group, since names, colours and collapse are per domain.
    fn domain_island_of(&self, id: TabId) -> Option<tab_groups::DomainKey> {
        match self.group_key_of(id)? {
            TabGroupKey::Domain(key) => Some(key),
            TabGroupKey::Opener(_) => None,
        }
    }

    fn group_key_of(&self, id: TabId) -> Option<TabGroupKey> {
        let inputs = self.group_inputs();
        let index = inputs.iter().position(|input| input.id == id)?;
        tab_groups::group_keys(&inputs, &self.group_preferences)
            .into_iter()
            .nth(index)
            .flatten()
    }

    /// Stores a marker change, then posts a non-structural tab change so the strip repaints.
    fn change_markers(app: &AppRef, change: impl FnOnce(&mut App) -> bool) -> bool {
        let (hwnd, active, changed) = {
            let mut state = app.borrow_mut();
            let changed = change(&mut state);
            (state.hwnd, state.tab_model.active(), changed)
        };
        if changed {
            Self::post_tab_changed_kind(hwnd, active, false, false);
        }
        changed
    }

    /// Marker shared by every tab of a group. Markers are in memory only.
    pub fn group_marker(app: &AppRef, key: &TabGroupKey) -> Option<Marker> {
        app.borrow().group_markers.get(key).copied()
    }

    pub fn set_group_marker(app: &AppRef, key: TabGroupKey, marker: Option<Marker>) -> bool {
        Self::change_markers(app, |state| {
            set_entry(&mut state.group_markers, key, marker)
        })
    }

    /// Marker a tab shows: its group's marker when it is in a group, otherwise its own. A click
    /// on any tab of a group therefore marks the whole group.
    pub fn tab_marker(app: &AppRef, id: TabId) -> Option<Marker> {
        let state = app.borrow();
        let keys = tab_groups::group_keys(&state.group_inputs(), &state.group_preferences);
        let index = state.tab_model.index_of(id)?;
        resolve_marker(
            keys.get(index).and_then(Option::as_ref),
            id,
            &state.group_markers,
            &state.tab_markers,
        )
    }

    /// Sets or clears the marker for a tab's group, or for the tab itself when it is in no group.
    pub fn set_tab_marker(app: &AppRef, id: TabId, marker: Option<Marker>) -> bool {
        Self::change_markers(app, |state| {
            if state.tab_model.get(id).is_none() {
                return false;
            }
            match state.group_key_of(id) {
                Some(key) => set_entry(&mut state.group_markers, key, marker),
                None => set_entry(&mut state.tab_markers, id, marker),
            }
        })
    }

    /// Effective marker of every tab that shows one, for the strip painter.
    pub fn tab_markers(app: &AppRef) -> HashMap<TabId, Marker> {
        let state = app.borrow();
        // The strip refreshes on every title and loading change; with no markers set there is
        // nothing to resolve, so skip projecting the groups.
        if state.group_markers.is_empty() && state.tab_markers.is_empty() {
            return HashMap::new();
        }
        let inputs = state.group_inputs();
        let keys = tab_groups::group_keys(&inputs, &state.group_preferences);
        inputs
            .iter()
            .zip(&keys)
            .filter_map(|(input, key)| {
                resolve_marker(
                    key.as_ref(),
                    input.id,
                    &state.group_markers,
                    &state.tab_markers,
                )
                .map(|marker| (input.id, marker))
            })
            .collect()
    }

    /// Hover card for a tab, with the tab ids of its rows in the same order as `model.rows`.
    /// `None` when the tab is not open.
    pub fn tab_card_model(app: &AppRef, id: TabId) -> Option<(CardModel, Vec<TabId>)> {
        let state = app.borrow();
        let inputs = state.group_inputs();
        let keys = tab_groups::group_keys(&inputs, &state.group_preferences);
        let hovered = inputs.iter().position(|input| input.id == id)?;
        let active = state
            .tab_model
            .index_of(state.tab_model.active())
            .unwrap_or(0);
        let card_tabs: Vec<CardTab> = inputs
            .iter()
            .map(|input| {
                let mirror = state.mirror(input.id);
                CardTab {
                    id: input.id,
                    title: mirror.map_or("", |tab| tab.title.as_str()),
                    opener: input.opener,
                    has_favicon: mirror.is_some_and(|tab| tab.favicon_key.is_some()),
                }
            })
            .collect();
        let rows = card_rows(&card_tabs, &keys, hovered, active);
        let hovered_mirror = state.mirror(id);
        let marker = resolve_marker(
            keys[hovered].as_ref(),
            id,
            &state.group_markers,
            &state.tab_markers,
        );
        let model = tab_card::build(
            &TabCardInput {
                title: hovered_mirror.map_or_else(String::new, |tab| tab.title.clone()),
                domain: hovered_mirror
                    .and_then(|tab| tab.url.as_ref())
                    .and_then(|url| url.host_str())
                    .map(str::to_owned),
                marker,
            },
            &rows,
        );
        let row_ids = rows.iter().map(|row| card_tabs[row.tab_index].id).collect();
        Some((model, row_ids))
    }

    pub fn tab_count(app: &AppRef) -> usize {
        app.borrow().tab_model.len()
    }

    pub fn split_view(app: &AppRef) -> Option<SplitView> {
        app.borrow().split_view
    }

    fn apply_visible_panes(app: &AppRef) {
        let (split, bounds, active, viewport) = {
            let state = app.borrow();
            (
                state.split_view,
                state.split_bounds,
                state.tab_model.active(),
                state.last_valid_viewport_bounds,
            )
        };
        let _ = Self::with_engine(app, |engine| {
            if let (Some(split), Some((left, right))) = (split, bounds) {
                engine.set_visible_tabs(
                    &[(split.left, left), (split.right, right)],
                    split.focused_tab(),
                )
            } else if let Some(viewport) = viewport {
                // `select_tab` preserves all currently visible tabs. After leaving split view
                // that would leave the other pane controller visible over the single page.
                engine.set_visible_tabs(&[(active, viewport)], active)
            } else {
                engine.select_tab(active)
            }
        });
    }

    /// Opens a non-persisted split and keeps both existing tab controllers alive and visible.
    pub fn open_split_view(
        app: &AppRef,
        left: TabId,
        right: TabId,
        focused: Pane,
        bounds: (
            windows::Win32::Foundation::RECT,
            windows::Win32::Foundation::RECT,
        ),
    ) -> bool {
        Self::open_split_view_with_ratio(app, left, right, focused, 50, bounds)
    }

    fn open_split_view_with_ratio(
        app: &AppRef,
        left: TabId,
        right: TabId,
        focused: Pane,
        ratio: u8,
        bounds: (
            windows::Win32::Foundation::RECT,
            windows::Win32::Foundation::RECT,
        ),
    ) -> bool {
        let (focused_tab, hwnd) = {
            let mut state = app.borrow_mut();
            if left == right
                || state.tab_model.get(left).is_none()
                || state.tab_model.get(right).is_none()
            {
                return false;
            }
            let Some(mut split) = SplitView::new(left, right, focused) else {
                return false;
            };
            split.set_ratio(ratio);
            state.split_view = Some(split);
            state.split_bounds = Some(bounds);
            let focused_tab = split.focused_tab();
            state.tab_model.select(focused_tab);
            for tab in &mut state.tabs {
                if tab.id == left || tab.id == right {
                    tab.hidden_since = None;
                    tab.suspended = false;
                } else {
                    tab.hidden_since.get_or_insert_with(Instant::now);
                }
            }
            if let Some((progress, page_usable)) = state
                .tabs
                .iter()
                .find(|tab| tab.id == focused_tab)
                .map(|tab| (tab.progress, tab.page_usable))
            {
                state.navigation_progress = progress;
                state.page_loaded = page_usable;
            }
            (focused_tab, state.hwnd)
        };
        Self::apply_visible_panes(app);
        Self::restore_discarded_tab(app, left);
        Self::restore_discarded_tab(app, right);
        Self::post_tab_changed(hwnd, focused_tab);
        Self::sync_window_title(app);
        Self::schedule_tab_idle_timer(app);
        true
    }

    pub fn update_split_bounds(
        app: &AppRef,
        bounds: (
            windows::Win32::Foundation::RECT,
            windows::Win32::Foundation::RECT,
        ),
    ) {
        if app.borrow().split_view.is_some() {
            app.borrow_mut().split_bounds = Some(bounds);
            Self::apply_visible_panes(app);
        }
    }

    pub fn clear_split_bounds(app: &AppRef) {
        if app.borrow().split_view.is_some() {
            app.borrow_mut().split_bounds = None;
            Self::apply_visible_panes(app);
        }
    }

    pub fn set_split_ratio(app: &AppRef, ratio: u8) -> bool {
        let changed = {
            let mut state = app.borrow_mut();
            let Some(split) = state.split_view.as_mut() else {
                return false;
            };
            let old = split.ratio;
            split.set_ratio(ratio);
            old != split.ratio
        };
        changed
    }

    pub fn split_state_changed(app: &AppRef) {
        let (split, hwnd) = {
            let state = app.borrow();
            (state.split_view, state.hwnd)
        };
        if let Some(split) = split {
            Self::post_tab_changed_kind(hwnd, split.focused_tab(), false, true);
        }
    }

    pub fn focus_split_pane(app: &AppRef, pane: Pane) -> bool {
        let (active, hwnd) = {
            let mut state = app.borrow_mut();
            let Some(mut split) = state.split_view else {
                return false;
            };
            split.focus(pane);
            let active = split.focused_tab();
            state.split_view = Some(split);
            state.tab_model.select(active);
            if let Some((progress, page_usable)) = state
                .tabs
                .iter()
                .find(|tab| tab.id == active)
                .map(|tab| (tab.progress, tab.page_usable))
            {
                state.navigation_progress = progress;
                state.page_loaded = page_usable;
            }
            (active, state.hwnd)
        };
        Self::apply_visible_panes(app);
        Self::post_tab_changed(hwnd, active);
        Self::sync_window_title(app);
        true
    }

    pub fn focus_split_tab(app: &AppRef, id: TabId) -> bool {
        let pane = app.borrow().split_view.and_then(|split| split.pane_for(id));
        pane.is_some_and(|pane| Self::focus_split_pane(app, pane))
    }

    pub fn swap_split_panes(app: &AppRef) -> bool {
        let Some(mut split) = app.borrow().split_view else {
            return false;
        };
        split.swap();
        let bounds = app.borrow().split_bounds;
        let Some(bounds) = bounds else { return false };
        Self::open_split_view_with_ratio(
            app,
            split.left,
            split.right,
            split.focused,
            split.ratio,
            bounds,
        )
    }

    pub fn replace_focused_split_pane(app: &AppRef, tab: TabId) -> bool {
        let Some(mut split) = app.borrow().split_view else {
            return false;
        };
        if !split.replace_focused(tab) {
            return false;
        }
        let Some(bounds) = app.borrow().split_bounds else {
            return false;
        };
        Self::open_split_view_with_ratio(
            app,
            split.left,
            split.right,
            split.focused,
            split.ratio,
            bounds,
        )
    }

    pub fn close_split_pane(app: &AppRef, pane: Pane) -> bool {
        let split = { app.borrow().split_view };
        let Some(split) = split else { return false };
        let closing = match pane {
            Pane::Left => split.left,
            Pane::Right => split.right,
        };
        Self::close_tab(app, closing)
    }

    pub fn reload(app: &AppRef) {
        if let Some(Err(error)) = Self::with_engine(app, |engine| engine.reload()) {
            win32::show_error_message(&format!("Reload failed: {error}"));
        }
    }

    /// Reloads the requested tab without selecting it first.
    pub fn reload_tab(app: &AppRef, id: TabId) {
        if id == Self::active_tab_id(app) {
            Self::reload(app);
        } else {
            if let Some(Err(error)) = Self::with_engine(app, |engine| engine.reload_tab(id)) {
                win32::show_error_message(&format!("Reload failed: {error}"));
            }
        }
    }

    pub fn stop(app: &AppRef) {
        if let Some(Err(error)) = Self::with_engine(app, |engine| engine.stop()) {
            win32::show_error_message(&format!("Stopping navigation failed: {error}"));
        }
    }

    /// Moves keyboard focus into the active page. Used after the HTML toolbar submits or escapes.
    pub fn focus_page(app: &AppRef) {
        let _ = Self::with_engine(app, |engine| engine.focus_page());
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

    pub fn active_tab_is_usable(app: &AppRef) -> bool {
        let state = app.borrow();
        let active = state.tab_model.active();
        state
            .tabs
            .iter()
            .find(|tab| tab.id == active)
            .is_some_and(|tab| tab.page_usable)
    }

    pub fn tab_has_controller(app: &AppRef, id: TabId) -> bool {
        app.borrow()
            .tabs
            .iter()
            .find(|tab| tab.id == id)
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

    pub fn tab_muted(app: &AppRef, id: TabId) -> bool {
        let state = app.borrow();
        state
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .is_some_and(|tab| tab.muted)
            || state
                .engine
                .as_ref()
                .is_some_and(|engine| engine.tab_muted(id))
    }

    pub fn tab_supports_muting(app: &AppRef, id: TabId) -> bool {
        app.borrow()
            .engine
            .as_ref()
            .is_some_and(|engine| engine.supports_tab_muting(id))
    }

    pub fn set_tab_muted(app: &AppRef, id: TabId, muted: bool) {
        if let Some(tab) = app.borrow_mut().tabs.iter_mut().find(|tab| tab.id == id) {
            tab.muted = muted;
        } else {
            return;
        }
        if let Some(Err(error)) = Self::with_engine(app, |engine| engine.set_tab_muted(id, muted)) {
            win32::show_error_message(&format!("Audio control failed: {error}"));
        }
    }

    pub fn tab_hibernate_eligible(app: &AppRef, id: TabId) -> bool {
        let state = app.borrow();
        let Some(tab) = state.tabs.iter().find(|tab| tab.id == id) else {
            return false;
        };
        state.tab_model.active() != id
            && !state
                .split_view
                .is_some_and(|split| split.pane_for(id).is_some())
            && !state.tab_model.get(id).is_some_and(|model| model.pinned)
            && !tab.loading
            && tab.controller_created
            && !tab.discarded
            && tab
                .url
                .as_ref()
                .is_some_and(|url| matches!(url.scheme(), "http" | "https"))
    }

    /// Suspends one inactive tab after the same unsaved-input/media check used by automatic
    /// sleeping. The callback rechecks eligibility because it may complete after a tab switch.
    pub fn hibernate_tab(app: &AppRef, id: TabId) -> bool {
        if !Self::tab_hibernate_eligible(app, id) {
            return false;
        }
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
                    Self::finish_hibernate_check(&app, id, blocked);
                }
            })
        };
        let asked = Self::with_engine(app, |engine| {
            engine.check_sleep_blocked(id, done);
            Ok(())
        });
        answered_inline.set(false);
        match (asked, verdict.take()) {
            (Some(Ok(())), Some(blocked)) => Self::finish_hibernate_check(app, id, blocked),
            (Some(Ok(())), None) => {}
            _ => return false,
        }
        true
    }

    fn finish_hibernate_check(app: &AppRef, id: TabId, blocked: bool) {
        let eligible = Self::tab_hibernate_eligible(app, id);
        let playing_audio = eligible && Self::tab_playing_audio(app, id);
        if !eligible || blocked || playing_audio {
            if let Some(tab) = app.borrow_mut().tabs.iter_mut().find(|tab| tab.id == id) {
                tab.sleep_check_started = None;
            }
            if blocked || playing_audio {
                win32::show_error_message("This tab has unsaved input or audio and was left open.");
            }
            return;
        }
        if let Some(Err(error)) = Self::with_engine(app, |engine| engine.suspend_tab(id)) {
            win32::show_error_message(&format!("Tab hibernation failed: {error}"));
            return;
        }
        let hwnd = {
            let mut state = app.borrow_mut();
            let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) else {
                return;
            };
            tab.sleep_check_started = None;
            tab.suspended = true;
            tab.hidden_since.get_or_insert_with(Instant::now);
            state.hwnd
        };
        Self::post_tab_changed(hwnd, id);
    }

    /// Connection state of the active tab's page, for the address pill and Site information.
    pub fn site_security(app: &AppRef) -> SiteSecurity {
        let state = app.borrow();
        let active = state.tab_model.active();
        state
            .tabs
            .iter()
            .find(|tab| tab.id == active)
            .map_or(SiteSecurity::NoSite, |tab| {
                site_security::classify(tab.url.as_ref(), tab.load)
            })
    }

    /// The active tab and its committed page address, when it has one.
    pub fn active_site(app: &AppRef) -> Option<(TabId, url::Url)> {
        let state = app.borrow();
        let active = state.tab_model.active();
        let url = state
            .tabs
            .iter()
            .find(|tab| tab.id == active)?
            .url
            .clone()?;
        Some((active, url))
    }

    /// Requests the blocker stopped on the active tab's current page.
    pub fn active_blocked_count(app: &AppRef) -> u32 {
        let state = app.borrow();
        let active = state.tab_model.active();
        state
            .tabs
            .iter()
            .find(|tab| tab.id == active)
            .map_or(0, |tab| tab.blocked)
    }

    pub fn privacy_stats_snapshot(app: &AppRef) -> (crate::privacy_stats::PrivacyStats, u64) {
        let state = app.borrow();
        (
            state.privacy_stats.clone().unwrap_or_default(),
            state.privacy_session_blocked,
        )
    }

    pub fn privacy_settings_snapshot(app: &AppRef) -> (bool, bool, u64) {
        let state = app.borrow();
        (
            state.settings.blocker_enabled,
            state.settings.sleep_enabled,
            state.settings.sleep_after_secs,
        )
    }

    /// Counts only a request already rejected by the blocker. Called from the existing request
    /// event, so there is no timer or polling path for collection.
    pub fn record_blocked_request(app: &AppRef, site_host: &str) {
        let (hwnd, should_write) = {
            let mut state = app.borrow_mut();
            state.privacy_session_blocked = state.privacy_session_blocked.saturating_add(1);
            if let Some(stats) = state.privacy_stats.as_mut() {
                stats.record_blocked(crate::privacy_stats::today_utc().as_str(), site_host);
            }
            (state.hwnd, state.privacy_stats_path.is_some())
        };
        if should_write {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetTimer(
                    Some(hwnd),
                    win32::WM_PRIVACY_STATS_TIMER,
                    1_500,
                    None,
                );
            }
        }
    }

    pub fn privacy_stats_save_now(app: &AppRef) -> std::io::Result<()> {
        let (path, stats) = {
            let state = app.borrow();
            (
                state.privacy_stats_path.clone(),
                state.privacy_stats.clone(),
            )
        };
        match (path, stats) {
            (Some(path), Some(stats)) => stats.save(&path),
            _ => Ok(()),
        }
    }

    pub fn clear_privacy_statistics(app: &AppRef) -> std::io::Result<()> {
        let mut state = app.borrow_mut();
        let Some(path) = state.privacy_stats_path.clone() else {
            return Ok(());
        };
        let Some(stats) = state.privacy_stats.as_mut() else {
            return Ok(());
        };
        let previous = stats.clone();
        stats.clear();
        if let Err(error) = stats.save(&path) {
            *stats = previous;
            return Err(error);
        }
        state.privacy_session_blocked = 0;
        Ok(())
    }

    pub fn permissions_handle(app: &AppRef) -> Option<engine::PermissionsHandle> {
        app.borrow().permissions.clone()
    }

    /// Runs `f` on the engine when one exists. The app is borrowed for the call, so `f` must not
    /// re-enter the UI; the WebView2 calls used here only queue work.
    pub fn call_engine<R>(app: &AppRef, f: impl FnOnce(&mut dyn EngineAdapter) -> R) -> Option<R> {
        let mut state = app.borrow_mut();
        state.engine.as_mut().map(|engine| f(engine.as_mut()))
    }

    /// `done` always runs with App unborrowed: callers open menus that read App. An engine that
    /// answers synchronously (a tab with no live page) is answered after `call_engine` returns.
    pub fn picture_in_picture_eligible(app: &AppRef, tab: TabId, done: Box<dyn FnOnce(bool)>) {
        let done = Rc::new(RefCell::new(Some(done)));
        let early = Rc::new(std::cell::Cell::new(None::<bool>));
        let in_call = Rc::new(std::cell::Cell::new(true));
        let answer = {
            let (done, early, in_call) = (done.clone(), early.clone(), in_call.clone());
            Box::new(move |available: bool| {
                if in_call.get() {
                    early.set(Some(available));
                } else if let Some(done) = done.borrow_mut().take() {
                    done(available);
                }
            }) as Box<dyn FnOnce(bool)>
        };
        let called = Self::call_engine(app, |engine| engine.picture_in_picture_eligible(tab, answer));
        in_call.set(false);
        let available = match (called, early.take()) {
            (None, _) => Some(false),
            (Some(()), early) => early,
        };
        if let Some(available) = available {
            let done = done.borrow_mut().take();
            if let Some(done) = done {
                done(available);
            }
        }
    }

    pub fn toggle_picture_in_picture(app: &AppRef, tab: TabId) {
        let _ = Self::call_engine(app, |engine| engine.toggle_picture_in_picture(tab));
    }

    /// Copies the engine's real back/forward availability for `id` into the tab, which the native
    /// and HTML toolbars read. Called whenever the tab's address changes, which includes history
    /// navigations. Keeps the previous values while the engine is out of `self` (during a call).
    fn sync_tab_history(&mut self, id: TabId) {
        let Some((back, forward)) = self.engine.as_ref().and_then(|engine| engine.tab_history(id))
        else {
            return;
        };
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.can_back = back;
            tab.can_forward = forward;
        }
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
        app.borrow().workspaces.active().tabs().to_vec()
    }

    pub fn tab_opener(app: &AppRef, id: TabId) -> Option<TabId> {
        app.borrow().tab_model.get(id).and_then(|tab| tab.opener)
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

    pub fn tab_favicon_key(app: &AppRef, id: TabId) -> Option<crate::tabs::FaviconKey> {
        app.borrow()
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .and_then(|tab| tab.favicon_key)
    }

    fn apply_tab_progress(app: &AppRef, id: TabId, event: NavigationProgressEvent) -> bool {
        let mut state = app.borrow_mut();
        let active = state.tab_model.active() == id;
        match event {
            NavigationProgressEvent::Started(_) => state.begin_navigation_grouping(id),
            NavigationProgressEvent::Completed(_) => state.in_flight_domains.finish(id),
            NavigationProgressEvent::Advanced(_, _) => {}
        }
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
        drop(state);
        if matches!(event, NavigationProgressEvent::Completed(_)) {
            // The new site is committed now, so a rule for the site the tab left may be unused.
            Self::release_unused_site_rules(app);
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
        if crate::profile::sweep_stale_guests(&app_data).is_err() {
            win32::show_error_message("Bowser could not safely inspect its guest data folder.");
            return Ok(());
        }
        // Keep updater, benchmark and performance-probe launches on the historical default.
        let interactive_launch = options.benchmark_log.is_none()
            && options.benchmark_url.is_none()
            && options.user_data_dir.is_none();
        let profile_result = if options.guest {
            crate::profile::Profile::guest_in(&app_data)
        } else {
            crate::profile::startup_profile(
                &app_data,
                options.profile.as_deref(),
                interactive_launch,
            )
        };
        let profile = match profile_result {
            Ok(profile) => profile,
            Err(error) => {
                win32::show_error_message(&error.to_string());
                return Ok(());
            }
        };
        // A plain launch while another live Bowser has this profile open would share its WebView2
        // folder and session. Refuse before any window, workspace or session state is loaded.
        let new_window = options.new_window;
        let tracked = options.benchmark_url.is_none() && options.user_data_dir.is_none();
        let lock = crate::session::lock_state(&profile.session_dir());
        if crate::session::decide_launch(lock, profile.is_guest(), tracked, new_window)
            == crate::session::LaunchDecision::RefuseAlreadyOpen
        {
            win32::show_error_message("Bowser is already open for this profile.");
            return Ok(());
        }
        let _guest_session = if profile.is_guest() {
            match crate::profile::GuestSession::start(&profile) {
                Ok(session) => Some(session),
                Err(error) => {
                    let _ = crate::profile::cleanup_guest(&profile);
                    win32::show_error_message(&error.to_string());
                    return Ok(());
                }
            }
        } else {
            None
        };
        if interactive_launch && !profile.is_guest() {
            // Remembering is a convenience; an unwritable preferences folder must not block launch.
            let _ = crate::profile::remember_last_profile(&app_data, profile.listed_name());
        }
        crate::profile::set_active(profile.clone());
        let settings = crate::settings::Settings::load(&profile.settings_file());
        let provider =
            SearchProvider::from_config(settings.search_engine, &profile.search_provider_file());
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
        let track_session = options.benchmark_url.is_none()
            && options.user_data_dir.is_none()
            && !profile.is_guest();
        let track_privacy = options.benchmark_url.is_none() && options.user_data_dir.is_none();
        let mut app = Self::from_launch_options(
            options,
            user_data_dir,
            provider,
            log,
            Some(profile.tab_groups_file()),
        );
        app.settings = settings;
        if track_privacy && app.settings.blocker_enabled {
            let mut state = crate::blocker::BlockerState::load(
                &profile.blocker_dir(),
                &app.settings.blocker_allow,
            );
            state.enabled = true;
            app.blocker = Some(Rc::new(RefCell::new(state)));
        }
        if track_privacy {
            app.permissions = Some(Rc::new(RefCell::new(
                crate::permissions::PermissionStore::load(&profile.permissions_file()),
            )));
            app.privacy_stats_path = Some(profile.privacy_stats_file());
            app.privacy_stats = Some(crate::privacy_stats::PrivacyStats::load(
                &profile.privacy_stats_file(),
            ));
        }
        if track_session
            && !app.init_session(&profile.session_dir())
            && !new_window
            && crate::session::lock_state(&profile.session_dir())
                == crate::session::LockState::HeldByLiveInstance
        {
            // Another instance took the marker after the early check above (a double-click).
            win32::show_error_message("Bowser is already open for this profile.");
            return Ok(());
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
        let initial_url = options.open_url;
        let allowed_benchmark_file = benchmark_url.clone();
        let tab_model = crate::tabs::TabList::new();
        let initial_tab = tab_model.active();
        let workspaces_path = crate::profile::active()
            .ok()
            .filter(|profile| !profile.is_guest())
            .map(|profile| profile.workspaces_file());
        Self {
            hwnd: HWND::default(),
            edit: HWND::default(),
            input_font: HFONT::default(),
            engine: None,
            pending_login_messages: Vec::new(),
            pending: None,
            pending_tab: None,
            initializing: false,
            prewarm_requested: false,
            chrome_environment: None,
            engine_creation_settled: false,
            closing: false,
            title_synced_at: None,
            title_dirty: false,
            title_timer_armed: false,
            loop_shutdown_pending: false,
            user_data_dir,
            allowed_benchmark_file,
            benchmark_url,
            initial_url,
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
            split_view: None,
            split_bounds: None,
            group_preferences: group_preferences_path
                .as_deref()
                .map(crate::tab_groups::GroupPreferences::load)
                .unwrap_or_default(),
            group_preferences_path,
            group_markers: HashMap::new(),
            tab_markers: HashMap::new(),
            memberships: HashMap::new(),
            in_flight_domains: InFlightDomains::default(),
            organisation_undo: UndoStack::default(),
            workspaces: crate::workspaces::WorkspaceSet::new("Default", initial_tab),
            workspaces_path,
            settings: crate::settings::Settings::default(),
            session: None,
            session_save_pending: false,
            last_session_snapshot: None,
            pending_restore: None,
            privacy_stats: None,
            privacy_stats_path: None,
            privacy_session_blocked: 0,
            blocker: None,
            permissions: None,
        }
    }

    /// Opens the crash-recovery state and, after a crash, asks whether to restore.
    /// Returns `false` when the session could not be opened because another Bowser holds it.
    fn init_session(&mut self, dir: &std::path::Path) -> bool {
        let workspace_path = self
            .workspaces_path
            .clone()
            .unwrap_or_else(|| dir.join(crate::workspaces::FILE_NAME));
        if let Ok(Some(workspaces)) = crate::workspaces::WorkspaceSet::load(&workspace_path) {
            self.workspaces = workspaces;
        }
        let Some((mut store, offer)) = crate::session::SessionStore::open(dir) else {
            return false; // another Bowser is running; leave its files alone
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
        if self.pending_restore.is_none() {
            let active = self.tab_model.active();
            self.workspaces.reconcile(&[active], active);
        }
        self.session = crate::session::SessionWriter::new(store).ok();
        true
    }

    /// Rebuilds the tab strip from an accepted crash snapshot. Only the active tab loads now;
    /// the others are created asleep and load when selected. A bad entry never aborts the rest.
    pub fn restore_pending_session(app: &AppRef) {
        let Some(session) = app.borrow_mut().pending_restore.take() else {
            return;
        };
        let mut active_url = None;
        let mut restored_ids = Vec::with_capacity(session.tabs.len());
        let restored_split = session.split;
        let hwnd = {
            let mut state = app.borrow_mut();
            let mut first = true;
            let mut active_id = None;
            for (index, saved) in session.tabs.iter().enumerate() {
                let Ok(url) = url::Url::parse(&saved.url) else {
                    restored_ids.push(None);
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
                restored_ids.push(Some(id));
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
            if let Some(saved_split) = restored_split
                && let (Some(Some(left)), Some(Some(right))) = (
                    restored_ids.get(saved_split.left),
                    restored_ids.get(saved_split.right),
                )
                && let Some(mut split) = SplitView::new(
                    *left,
                    *right,
                    if saved_split.focused_left {
                        Pane::Left
                    } else {
                        Pane::Right
                    },
                )
            {
                split.set_ratio(saved_split.ratio);
                state.split_view = Some(split);
                state.tab_model.select(split.focused_tab());
                if let Some(tab) = state
                    .tabs
                    .iter_mut()
                    .find(|tab| tab.id == split.focused_tab())
                {
                    tab.discarded = false;
                }
                active_url = state
                    .tabs
                    .iter()
                    .find(|tab| tab.id == split.focused_tab())
                    .and_then(|tab| tab.url.clone());
            }
            let ids = state.tab_model.iter().map(|tab| tab.id).collect::<Vec<_>>();
            let active = state.tab_model.active();
            state.workspaces.remap_session_order(&ids);
            state.workspaces.reconcile(&ids, active);
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
        let snapshot = Self::session_snapshot(app);
        let mut state = app.borrow_mut();
        if snapshot == state.last_session_snapshot {
            state.session_save_pending = false;
            return false;
        }
        if state.session.is_some()
            && snapshot != state.last_session_snapshot
            && !state.session_save_pending
            && !state.closing
        {
            state.session_save_pending = true;
            true
        } else {
            false
        }
    }

    fn session_snapshot(app: &AppRef) -> Option<crate::session::Session> {
        let state = app.borrow();
        let active = state.tab_model.index_of(state.tab_model.active())?;
        let mut saved_indexes = HashMap::new();
        let mut saved_count = 0;
        for model in state.tab_model.iter() {
            let url = state
                .tabs
                .iter()
                .find(|tab| tab.id == model.id)
                .and_then(|tab| tab.url.as_ref());
            if url.is_some_and(|url| crate::session::persistable_url(url).is_some()) {
                saved_indexes.insert(model.id, saved_count);
                saved_count += 1;
            }
        }
        let mut session = crate::session::Session::from_tabs(
            state.tab_model.iter().map(|model| {
                let tab = state.tabs.iter().find(|tab| tab.id == model.id);
                (
                    tab.and_then(|tab| tab.url.as_ref()),
                    tab.map_or("", |tab| tab.title.as_str()),
                    model.pinned,
                )
            }),
            active,
        )?;
        if let Some(split) = state.split_view {
            if let (Some(&left), Some(&right)) = (
                saved_indexes.get(&split.left),
                saved_indexes.get(&split.right),
            ) {
                session.split = Some(crate::session::SessionSplit {
                    left,
                    right,
                    focused_left: split.focused == Pane::Left,
                    ratio: split.ratio,
                });
            }
        }
        Some(session)
    }

    /// The debounce timer fired: write the snapshot if anything changed.
    pub fn session_save_now(app: &AppRef) {
        let snapshot = Self::session_snapshot(app);
        let mut state = app.borrow_mut();
        if state.session.is_none() {
            return;
        }
        state.session_save_pending = false;
        if let Some(snapshot) = snapshot {
            if state.last_session_snapshot.as_ref() != Some(&snapshot)
                && let Some(store) = state.session.as_ref()
            {
                store.save(snapshot.clone());
                state.last_session_snapshot = Some(snapshot);
            }
        }
        drop(state);
        Self::save_workspaces(app);
    }

    /// Clean exit: final snapshot and removal of the running marker.
    pub fn session_finish(app: &AppRef) {
        let snapshot = Self::session_snapshot(app);
        let store = app.borrow_mut().session.take();
        if let Some(store) = store {
            store.finish(snapshot);
        }
        Self::save_workspaces(app);
    }

    /// A tab's render process crashed. The active tab reloads; a hidden tab is marked so that it
    /// navigates afresh when selected. A repeat within [`CRASH_RELOAD_MIN_GAP`] is reported
    /// instead of reloaded again, so a page that crashes on load cannot loop.
    pub fn tab_render_failed(app: &AppRef, id: TabId) {
        let now = Instant::now();
        let (focused, visible, failed_pane) = {
            let mut state = app.borrow_mut();
            if state.closing {
                return;
            }
            let focused = state.tab_model.active() == id;
            let split = state.split_view;
            let visible = focused || split.is_some_and(|split| split.pane_for(id).is_some());
            let failed_pane = split.and_then(|split| split.pane_for(id));
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
            if !visible {
                tab.discarded = true;
            }
            (focused, visible, failed_pane)
        };
        let _ = app.borrow().log.record("tab_render_process_failed");
        if focused {
            Self::reload(app);
        } else if visible && let Some(pane) = failed_pane {
            let previous = app.borrow().split_view.map(|split| split.focused);
            let _ = Self::focus_split_pane(app, pane);
            Self::reload(app);
            if let Some(previous) = previous {
                let _ = Self::focus_split_pane(app, previous);
            }
        }
    }

    /// Re-reads the saved engine choice before each search, so a choice saved in Settings applies
    /// without a restart. The provider from startup stays in use when no profile is active.
    fn refresh_search_provider(&mut self) {
        let Ok(profile) = crate::profile::active() else {
            return;
        };
        let settings = crate::settings::Settings::load(&profile.settings_file());
        self.provider =
            SearchProvider::from_config(settings.search_engine, &profile.search_provider_file());
    }

    pub fn submit(app: &AppRef, input: &str) {
        let target = {
            let mut state = app.borrow_mut();
            state.refresh_search_provider();
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

    pub fn submit_initial_url(app: &AppRef) {
        let url = app.borrow_mut().initial_url.take();
        if let Some(url) = url {
            Self::submit_url(app, url, None);
        }
    }

    fn submit_url(app: &AppRef, url: url::Url, allowed_file: Option<url::Url>) {
        {
            let mut state = app.borrow_mut();
            let _ = state.log.record("submitted");
            if state.engine.is_some() {
                let active_tab = state.tab_model.active();
                state.begin_navigation_grouping(active_tab);
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
        let start_environment = {
            let mut state = app.borrow_mut();
            if state.closing || !text.chars().any(|character| !character.is_whitespace()) {
                return;
            }
            let active = state.tab_model.active();
            let has_controller = state
                .tabs
                .iter()
                .find(|tab| tab.id == active)
                .is_some_and(|tab| tab.controller_created);
            if has_controller {
                return;
            }
            if state.engine.is_some() {
                drop(state);
                Self::with_engine(app, |engine| engine.prewarm_tab(active));
                return;
            }
            if state.initializing && state.pending.is_none() {
                // The environment is already being created (by an earlier prewarm, or early for
                // the HTML chrome): prewarm the active tab once it is ready. For an earlier
                // prewarm this flag is already set.
                state.prewarm_requested = true;
                return;
            }
            if !should_prewarm(false, state.initializing, state.closing, text) {
                return;
            }
            state.prewarm_requested = true;
            state.initializing = true;
            let _ = state.log.record("engine_prewarm_started");
            true
        };
        if start_environment {
            Self::start_engine_creation(app);
        }
    }

    /// HTML chrome mode: the tab environment the chrome views are hosted in. When the engine has
    /// not been started yet (native mode starts it lazily, when the user begins typing), this starts
    /// it now, with no tab controller and no navigation, so the chrome can appear at startup. The
    /// first tab is not delayed: a navigation submitted meanwhile waits for, then reuses, this
    /// same environment. `Pending` is answered later with `win32::WM_HTML_CHROME_ENV`.
    pub fn request_chrome_environment(app: &AppRef) -> ChromeEnvironment {
        let step = {
            let mut state = app.borrow_mut();
            let step = chrome_environment_step(
                state.chrome_environment.is_some(),
                state.closing,
                state.engine_creation_settled,
                state.initializing,
            );
            match step {
                ChromeEnvironmentStep::Ready => {
                    if let Some(environment) = state.chrome_environment.clone() {
                        return ChromeEnvironment::Ready(environment);
                    }
                    return ChromeEnvironment::Unavailable;
                }
                ChromeEnvironmentStep::Wait => return ChromeEnvironment::Pending,
                ChromeEnvironmentStep::Unavailable => return ChromeEnvironment::Unavailable,
                ChromeEnvironmentStep::StartEngine => {
                    state.initializing = true;
                    let _ = state.log.record("engine_started_for_chrome");
                }
            }
            step
        };
        debug_assert_eq!(step, ChromeEnvironmentStep::StartEngine);
        Self::start_engine_creation(app);
        let mut state = app.borrow_mut();
        if let Some(environment) = state.chrome_environment.clone() {
            return ChromeEnvironment::Ready(environment);
        }
        if !state.initializing {
            // The request could not even be started (the error was already shown).
            state.engine_creation_settled = true;
            return ChromeEnvironment::Unavailable;
        }
        ChromeEnvironment::Pending
    }

    /// Records the outcome of engine creation for the HTML chrome and tells the window. Native
    /// mode never posts anything.
    fn settle_chrome_environment(app: &AppRef, environment: Option<ICoreWebView2Environment>) {
        if !crate::chrome_host::html_ui_enabled() {
            return;
        }
        let hwnd = {
            let mut state = app.borrow_mut();
            state.engine_creation_settled = true;
            if state.closing || state.engine.is_none() {
                state.chrome_environment = None;
            } else {
                state.chrome_environment = environment;
            }
            state.hwnd
        };
        win32::post(
            hwnd,
            win32::WM_HTML_CHROME_ENV,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(0),
        );
    }

    /// Drops the app's reference to the shared environment. Called when the window closes, after
    /// its chrome hosts are closed and before the engine closes.
    pub fn release_chrome_environment(app: &AppRef) {
        app.borrow_mut().chrome_environment = None;
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
        let login_events_app = Rc::downgrade(app);
        let login_messages_enabled = crate::profile::active().is_ok_and(|profile| {
            crate::engine::password_login_messages_enabled(
                crate::passwords::PasswordStore::open_default().is_ok(),
                profile.is_guest(),
            )
        });
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
                                true
                            } else {
                                false
                            };
                        if found {
                            state.sync_tab_history(tab_id);
                        }
                        if found {
                            state.tab_model.set_url(tab_id, Some(url.as_str()));
                        }
                        (state.edit, state.log.clone(), active, found)
                    };
                    if !found {
                        return;
                    }
                    if active {
                        let main = app.borrow().hwnd;
                        let event = if win32::sync_address_input(main, edit, tab_id, &url) {
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
                        Self::post_tab_changed_kind(hwnd, tab_id, false, true);
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
            controller_ready: {
                let events_app = Rc::downgrade(app);
                Some(Rc::new(move |tab_id| {
                    if let Some(app) = events_app.upgrade() {
                        let should_mute = {
                            let mut state = app.borrow_mut();
                            let active = state.tab_model.active() == tab_id;
                            let visible = active
                                || state
                                    .split_view
                                    .is_some_and(|split| split.pane_for(tab_id).is_some());
                            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                                tab.controller_created = true;
                                tab.hidden_since = (!visible).then(Instant::now);
                                tab.suspended = false;
                                tab.muted
                            } else {
                                false
                            }
                        };
                        if should_mute {
                            Self::set_tab_muted(&app, tab_id, true);
                        }
                    }
                }))
            },
            focus_changed: {
                let events_app = Rc::downgrade(app);
                Some(Rc::new(move |tab_id| {
                    if let Some(app) = events_app.upgrade() {
                        let hwnd = app.borrow().hwnd;
                        win32::post(
                            hwnd,
                            win32::WM_PANE_FOCUSED,
                            windows::Win32::Foundation::WPARAM(tab_id.0 as usize),
                            windows::Win32::Foundation::LPARAM(0),
                        );
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
                            Self::post_tab_changed_kind(app.borrow().hwnd, tab_id, false, false);
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
                            tab.load = LoadState::Loading;
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
                            Self::post_tab_changed_kind(app.borrow().hwnd, tab_id, false, false);
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
                        Self::post_tab_changed_kind(app.borrow().hwnd, tab_id, false, false);
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
                        tab.favicon_key = Some(crate::favicon::key_for(&bytes));
                        tab.favicon = Some(bytes);
                        true
                    } else {
                        false
                    };
                    if found {
                        Self::post_tab_changed_kind(app.borrow().hwnd, tab_id, false, false);
                    }
                }
            })),
            new_window_requested: {
                let popup_app = Rc::downgrade(app);
                Some(Rc::new(move |source_tab, url| {
                    if let Some(app) = popup_app.upgrade() {
                        if app.borrow().tabs.iter().any(|tab| tab.id == source_tab) {
                            let child = App::new_tab(&app);
                            let _ = app.borrow_mut().tab_model.set_opener(child, source_tab);
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
                Some(Rc::new(move |tab_id, count, site_host| {
                    if let Some(app) = blocked_app.upgrade() {
                        let active = {
                            let mut state = app.borrow_mut();
                            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                                tab.blocked = count;
                            }
                            state.tab_model.active() == tab_id
                        };
                        if active {
                            Self::schedule_blocked_title(&app);
                        }
                        if count != 0 {
                            Self::record_blocked_request(&app, &site_host);
                        }
                    }
                }))
            },
            navigation_failed: {
                let failed_app = Rc::downgrade(app);
                Some(Rc::new(move |tab_id, certificate| {
                    if let Some(app) = failed_app.upgrade() {
                        let found = {
                            let mut state = app.borrow_mut();
                            match state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                                Some(tab) => {
                                    tab.load = LoadState::Failed { certificate };
                                    true
                                }
                                None => false,
                            }
                        };
                        if found {
                            Self::post_tab_changed_kind(app.borrow().hwnd, tab_id, false, false);
                        }
                    }
                }))
            },
            failed: Some(Rc::new(|error| {
                win32::show_error_message(&format!("WebView2 error: {error}"))
            })),
            login_message: login_messages_enabled.then(|| {
                let callback: Rc<dyn Fn(TabId, crate::passwords::LoginMessage)> =
                    Rc::new(move |tab_id, message| {
                        if let Some(app) = login_events_app.upgrade() {
                            App::queue_login_message(&app, tab_id, message);
                        }
                    });
                callback
            }),
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
        let records = extension_records(app, &profile);
        // The id file exists once any extension has loaded into this profile. Its presence means
        // the profile may still hold extensions, so they are reconciled even when none is enabled.
        let profile_may_hold_extensions = records
            .as_ref()
            .is_some_and(|(_, ids_file)| ids_file.exists());
        let (known_extensions, extension_ids_file) = records.unzip();
        let extras = engine::EngineExtras {
            blocker,
            browser_extensions: plan.installed || profile_may_hold_extensions,
            startup_extensions: plan.enabled,
            known_extensions: known_extensions.unwrap_or_default(),
            extension_ids_file,
            permissions: app.borrow().permissions.clone(),
            chrome_ui_scheme: crate::chrome_host::html_ui_enabled(),
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
                            let chrome_environment = engine.chrome_environment();
                            App::complete_engine_creation(&app, Box::new(engine));
                            App::settle_chrome_environment(&app, chrome_environment);
                        }
                        Err(error) => {
                            drop(state);
                            App::settle_chrome_environment(&app, None);
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
        let (bounds, target, pending, pending_tab, active_tab, tab_ids, prewarm_requested) = {
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
                std::mem::take(&mut state.prewarm_requested),
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
        } else if prewarm_requested {
            let _ = Self::with_engine(app, |engine| engine.prewarm_tab(active_tab));
        }
        let split = { app.borrow().split_view };
        if let Some(split) = split {
            Self::restore_discarded_tab(app, split.left);
            Self::restore_discarded_tab(app, split.right);
            Self::apply_visible_panes(app);
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
            if state.engine.is_some() {
                state.begin_navigation_grouping(tab_id);
            }
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
            // A navigation that never started will not report completion, so release its domain.
            app.borrow_mut().in_flight_domains.finish(tab_id);
            win32::show_error_message(&format!("Navigation failed: {error}"));
        }
        Self::schedule_tab_idle_timer(app);
    }

    pub fn resize_current(app: &AppRef, bounds: windows::Win32::Foundation::RECT) {
        let (split, panes) = {
            let state = app.borrow();
            (state.split_view, state.split_bounds)
        };
        let Some(mut engine) = app.borrow_mut().engine.take() else {
            return;
        };
        let result = if let (Some(split), Some((left, right))) = (split, panes) {
            engine.set_visible_tabs(
                &[(split.left, left), (split.right, right)],
                split.focused_tab(),
            )
        } else {
            engine.resize(bounds)
        };
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
                tab.load = LoadState::Loaded;
            } else {
                return;
            }
            if active {
                state.page_loaded = true;
            }
            active
        };
        // The address pill's connection icon depends on the load outcome, so repaint it.
        Self::post_tab_changed_kind(app.borrow().hwnd, tab_id, false, false);
        if active {
            let decision = {
                let state = app.borrow();
                idle_memory_decision(state.minimized, true, 0, state.desired_memory_target)
            };
            Self::apply_idle_decision(app, decision);
            let action = Self::combine_idle_and_suspend_timers(app, decision.timer);
            win32::schedule_idle_timer(app.borrow().hwnd, action);
        } else {
            Self::post_tab_changed_kind(app.borrow().hwnd, tab_id, false, false);
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

    pub fn active_navigation_progress_identity(app: &AppRef) -> Option<(u64, u8)> {
        let state = app.borrow();
        let progress = state
            .tabs
            .iter()
            .find(|tab| tab.id == state.tab_model.active())?
            .progress;
        Some((progress.navigation_id?, progress.percent?))
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
            self.settings.tab_layout,
            self.settings.sidebar_width,
            crate::win32::side_panel::active_width_px(self.hwnd),
        );
        if valid_viewport_bounds(bounds) {
            self.last_valid_viewport_bounds = Some(bounds);
            bounds
        } else {
            if let Some(bounds) = self.last_valid_viewport_bounds {
                bounds
            } else {
                let fallback = chrome_viewport_bounds(
                    1000,
                    720,
                    dpi,
                    self.tabs.len(),
                    maximised,
                    self.settings.tab_layout,
                    self.settings.sidebar_width,
                    crate::win32::side_panel::active_width_px(self.hwnd),
                );
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
    _tab_count: usize,
    maximised: bool,
    tab_layout: crate::settings::TabLayout,
    sidebar_width: u16,
    side_panel_width: i32,
) -> windows::Win32::Foundation::RECT {
    let mut viewport = match tab_layout {
        crate::settings::TabLayout::Horizontal => {
            let inset = crate::chrome::layout::left_rail_inset(dpi);
            crate::chrome::layout::content_rect(width.max(0) - inset, height, dpi, maximised)
                .shifted_right(inset)
        }
        crate::settings::TabLayout::Vertical => {
            crate::vertical_tabs::SidebarLayout::compute_in_frame(
                width,
                height,
                dpi,
                i32::from(sidebar_width),
                maximised,
            )
            .viewport
        }
    };
    viewport.left = (viewport.left + side_panel_width.max(0)).min(viewport.right);
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
            split: Some(crate::session::SessionSplit {
                left: 0,
                right: 2,
                focused_left: false,
                ratio: 64,
            }),
        });
        App::restore_pending_session(&app);
        let state = app.borrow();
        assert_eq!(state.tab_model.len(), 3);
        let titles: Vec<_> = state.tab_model.iter().map(|t| t.title.clone()).collect();
        assert_eq!(titles, ["A", "B", "C"]);
        assert!(state.tab_model.iter().next().unwrap().pinned);
        let split = state.split_view.unwrap();
        assert_eq!(split.ratio, 64);
        assert_eq!(split.focused, crate::split_view::Pane::Right);
        assert_eq!(state.tab_model.active(), split.right);
        let active = state.tab_model.active();
        assert_eq!(state.tab_model.index_of(active), Some(1));
        let asleep: Vec<_> = state.tabs.iter().map(|t| t.discarded).collect();
        assert_eq!(asleep, [true, false, true]);
        assert_eq!(state.pending.as_ref().unwrap().as_str(), "https://b.test/");
    }

    #[test]
    fn closing_a_split_pane_closes_its_tab_and_keeps_the_other_as_active_tab() {
        let app = app_with_pending(rect(800, 600));
        let left = App::active_tab_id(&app);
        let right = App::new_tab(&app);
        assert!(App::open_split_view(
            &app,
            left,
            right,
            crate::split_view::Pane::Right,
            (rect(0, 0), rect(400, 0)),
        ));

        assert!(App::close_split_pane(&app, crate::split_view::Pane::Right));

        assert_eq!(App::tab_ids(&app), [left]);
        assert_eq!(App::active_tab_id(&app), left);
        assert!(App::split_view(&app).is_none());
    }

    #[test]
    fn swapping_split_panes_preserves_the_inverted_divider_ratio() {
        let app = app_with_pending(rect(800, 600));
        let left = App::active_tab_id(&app);
        let right = App::new_tab(&app);
        let bounds = (rect(0, 0), rect(400, 0));
        assert!(App::open_split_view(
            &app,
            left,
            right,
            crate::split_view::Pane::Right,
            bounds,
        ));
        assert!(App::set_split_ratio(&app, 63));

        assert!(App::swap_split_panes(&app));

        let split = App::split_view(&app).unwrap();
        assert_eq!((split.left, split.right), (right, left));
        assert_eq!(split.ratio, 37);
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

    /// Sets committed addresses directly, the way the engine's address events do.
    fn set_addresses(app: &AppRef, addresses: &[(TabId, &str)]) {
        let mut state = app.borrow_mut();
        for &(id, address) in addresses {
            let url = url::Url::parse(address).unwrap();
            state.tab_model.set_url(id, Some(address));
            state.tabs.iter_mut().find(|tab| tab.id == id).unwrap().url = Some(url);
        }
    }

    fn tab_domain(app: &AppRef, id: TabId) -> Option<String> {
        App::tab_group_inputs(app)
            .into_iter()
            .find(|input| input.id == id)
            .and_then(|input| input.domain)
            .map(|domain| domain.as_str().to_owned())
    }

    #[test]
    fn dropping_a_tab_between_two_members_of_a_group_joins_it_and_leaving_ungroups_it() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        let third = App::new_tab(&app);
        // Model order: first and third share a site; second is another site, between them.
        set_addresses(
            &app,
            &[
                (first, "https://a.example.com/"),
                (second, "https://other.example.net/"),
                (third, "https://b.example.com/"),
            ],
        );
        assert!(App::move_tab(&app, second, 2));
        assert!(!App::tab_is_grouped(&app, second));

        // Drop the middle tab between the two members of its neighbours' group.
        assert!(App::drop_tab(&app, second, 1));
        assert_eq!(tab_domain(&app, second).as_deref(), Some("example.com"));
        assert!(App::tab_is_grouped(&app, second));

        // Moving it to the end, away from the group, takes it out and it stays out.
        assert!(App::drop_tab(&app, second, 2));
        assert!(!App::tab_is_grouped(&app, second));
        assert_eq!(
            tab_domain(&app, second),
            None,
            "an ungrouped tab has no group domain"
        );
        // The first and third tabs still share a site, so their own group is untouched.
        assert!(App::tab_is_grouped(&app, first));
    }

    #[test]
    fn remove_from_group_is_undoable_and_undo_restores_the_group() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        set_addresses(
            &app,
            &[
                (first, "https://a.example.com/"),
                (second, "https://b.example.com/"),
            ],
        );
        assert!(App::tab_is_grouped(&app, first));
        assert!(!App::can_undo_tab_organisation(&app));

        assert!(App::remove_tab_from_group(&app, first));
        assert!(!App::tab_is_grouped(&app, first));
        assert!(!App::remove_tab_from_group(&app, first), "already out");
        assert!(App::can_undo_tab_organisation(&app));

        assert!(App::undo_tab_organisation(&app));
        assert!(App::tab_is_grouped(&app, first));
        assert!(!App::can_undo_tab_organisation(&app));
        assert!(!App::undo_tab_organisation(&app));
    }

    #[test]
    fn undo_restores_names_but_keeps_a_collapse_made_since() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        set_addresses(
            &app,
            &[
                (first, "https://a.example.com/"),
                (second, "https://b.example.com/"),
            ],
        );
        let example = crate::tab_groups::DomainKey::parse("example.com").unwrap();

        assert!(App::set_tab_group_name(&app, first, "Work"));
        assert!(App::set_tab_group_collapsed(&app, first, true));
        assert!(App::undo_tab_organisation(&app));

        let preferences = App::tab_group_preferences(&app);
        assert_eq!(preferences.name(&example), None);
        assert!(preferences.is_collapsed(&example));
    }

    #[test]
    fn automatic_grouping_off_forms_no_new_domain_groups() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        set_addresses(
            &app,
            &[
                (first, "https://a.example.com/"),
                (second, "https://b.example.com/"),
            ],
        );
        assert!(App::tab_is_grouped(&app, first));

        app.borrow_mut().settings.automatic_tab_grouping = false;

        assert_eq!(tab_domain(&app, first), None);
        assert!(!App::tab_is_grouped(&app, second));
    }

    #[test]
    fn a_redirect_while_loading_does_not_regroup_the_tab_until_the_navigation_completes() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        set_addresses(
            &app,
            &[
                (first, "https://a.example.com/"),
                (second, "https://b.example.com/"),
            ],
        );
        assert!(App::apply_tab_progress(
            &app,
            first,
            NavigationProgressEvent::Started(4)
        ));

        // A redirect to an unrelated site commits while the navigation is still loading.
        set_addresses(&app, &[(first, "https://login.other.example.net/")]);
        assert_eq!(tab_domain(&app, first).as_deref(), Some("example.com"));

        assert!(App::apply_tab_progress(
            &app,
            first,
            NavigationProgressEvent::Completed(4)
        ));
        assert_eq!(tab_domain(&app, first).as_deref(), Some("example.net"));
    }

    #[test]
    fn unnamed_islands_suggest_a_name_without_storing_it() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        set_addresses(
            &app,
            &[
                (first, "https://chatgpt.com/"),
                (second, "https://chatgpt.com/c/1"),
            ],
        );

        let (model, _) = App::island_menu(&app, first).expect("two tabs form an island");

        assert_eq!(model.name, None);
        assert_eq!(model.suggested_name.as_deref(), Some("ChatGPT"));
        assert_eq!(
            App::tab_group_preferences(&app)
                .name(&crate::tab_groups::DomainKey::parse("chatgpt.com").unwrap()),
            None
        );
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

    /// Engine whose per-tab session history is set by the test.
    struct HistoryEngine {
        history: Rc<RefCell<HashMap<TabId, (bool, bool)>>>,
    }

    impl EngineAdapter for HistoryEngine {
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
        fn tab_history(&self, tab: TabId) -> Option<(bool, bool)> {
            self.history.borrow().get(&tab).copied()
        }
        fn resize(&mut self, _bounds: windows::Win32::Foundation::RECT) -> windows::core::Result<()> {
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
    fn picture_in_picture_answer_runs_after_the_app_borrow_is_released() {
        // A tab with no live page answers synchronously (the default engine method does). The
        // tab context menu then reads App, which used to panic with "already mutably borrowed".
        let app = app_with_pending(rect(800, 600));
        let tab = App::new_tab(&app);
        app.borrow_mut().engine = Some(Box::new(HistoryEngine {
            history: Rc::new(RefCell::new(HashMap::new())),
        }));
        let answered = Rc::new(std::cell::Cell::new(None));
        let (menu_app, seen) = (app.clone(), answered.clone());
        App::picture_in_picture_eligible(
            &app,
            tab,
            Box::new(move |available| {
                let _pinned = App::tab_pinned(&menu_app, tab);
                seen.set(Some(available));
            }),
        );
        assert_eq!(answered.get(), Some(false));
    }

    #[test]
    fn toolbar_back_and_forward_follow_the_engines_tab_history() {
        let app = app_with_pending(rect(800, 600));
        let first = App::new_tab(&app);
        let second = App::new_tab(&app);
        let history = Rc::new(RefCell::new(HashMap::new()));
        app.borrow_mut().engine = Some(Box::new(HistoryEngine { history: history.clone() }));

        // First page of a tab: nothing to go back to.
        history.borrow_mut().insert(first, (false, false));
        app.borrow_mut().sync_tab_history(first);
        assert!(!App::tab_can_go_back(&app, first));
        assert!(!App::tab_can_go_forward(&app, first));

        // After a second page and then Back, Forward becomes available.
        history.borrow_mut().insert(first, (true, false));
        app.borrow_mut().sync_tab_history(first);
        assert!(App::tab_can_go_back(&app, first));
        history.borrow_mut().insert(first, (false, true));
        app.borrow_mut().sync_tab_history(first);
        assert!(!App::tab_can_go_back(&app, first));
        assert!(App::tab_can_go_forward(&app, first));

        // Each tab keeps its own state.
        assert!(!App::tab_can_go_forward(&app, second));

        // A tab the engine has no page for, or a moment when the engine is out of App, keeps
        // the last known values.
        app.borrow_mut().sync_tab_history(second);
        let engine = app.borrow_mut().engine.take();
        app.borrow_mut().sync_tab_history(first);
        assert!(App::tab_can_go_forward(&app, first));
        app.borrow_mut().engine = engine;
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
        fn prewarm_tab(&mut self, tab: TabId) -> windows::core::Result<()> {
            self.calls.borrow_mut().push(format!("prewarm:{}", tab.0));
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
            pending_login_messages: Vec::new(),
            pending: Some(url::Url::parse("https://example.test/first").unwrap()),
            pending_tab: Some(TabId(0)),
            initializing: true,
            prewarm_requested: false,
            chrome_environment: None,
            engine_creation_settled: false,
            closing: false,
            title_synced_at: None,
            title_dirty: false,
            title_timer_armed: false,
            loop_shutdown_pending: false,
            user_data_dir: PathBuf::new(),
            benchmark_url: None,
            initial_url: None,
            allowed_benchmark_file: None,
            benchmark_tracking_prevention: None,
            benchmark_memory_pressure: false,
            provider: SearchProvider::from_config(None, Path::new("missing-search-provider.txt")),
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
            split_view: None,
            split_bounds: None,
            group_preferences: crate::tab_groups::GroupPreferences::default(),
            group_preferences_path: None,
            group_markers: HashMap::new(),
            tab_markers: HashMap::new(),
            memberships: HashMap::new(),
            in_flight_domains: InFlightDomains::default(),
            organisation_undo: UndoStack::default(),
            workspaces: crate::workspaces::WorkspaceSet::new("Default", TabId(0)),
            workspaces_path: None,
            // Existing tests exercise the memory-pressure path; tab sleep has its own tests.
            settings: crate::settings::Settings {
                sleep_enabled: false,
                ..Default::default()
            },
            session: None,
            session_save_pending: false,
            last_session_snapshot: None,
            pending_restore: None,
            privacy_stats: None,
            privacy_stats_path: None,
            privacy_session_blocked: 0,
            blocker: None,
            permissions: None,
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
            SearchProvider::from_config(None, Path::new("missing-search-provider.txt")),
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
        let bounds = chrome_viewport_bounds(
            1200,
            800,
            144,
            3,
            true,
            crate::settings::TabLayout::Horizontal,
            crate::settings::DEFAULT_SIDEBAR_WIDTH,
            0,
        );
        let viewport = crate::chrome::layout::compute_in_frame(1200, 800, 144, 3, 0, true).viewport;
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
    fn vertical_webview_bounds_reserve_the_sidebar_at_supported_dpis() {
        for dpi in [96, 120, 144, 192] {
            let bounds = chrome_viewport_bounds(
                1600,
                1000,
                dpi,
                3,
                true,
                crate::settings::TabLayout::Vertical,
                260,
                0,
            );
            let sidebar =
                crate::vertical_tabs::SidebarLayout::compute_in_frame(1600, 1000, dpi, 260, true);
            assert_eq!(bounds.left, sidebar.viewport.left, "dpi={dpi}");
            assert_eq!(bounds.right, sidebar.viewport.right, "dpi={dpi}");
            assert!(valid_viewport_bounds(bounds), "dpi={dpi}");
        }
    }

    #[test]
    fn webview_bounds_reserve_rail_sidebar_and_panel_in_both_tab_layouts() {
        for dpi in [96, 120, 144, 192] {
            let inset = crate::chrome::layout::left_rail_inset(dpi);
            for tab_layout in [crate::settings::TabLayout::Horizontal, crate::settings::TabLayout::Vertical] {
                let base = chrome_viewport_bounds(1800, 1000, dpi, 4, true, tab_layout, 260, 0);
                let with_panel = chrome_viewport_bounds(1800, 1000, dpi, 4, true, tab_layout, 260, 320);
                assert!(base.left >= inset, "rail must be reserved, dpi={dpi}, layout={tab_layout:?}");
                assert_eq!(with_panel.left, base.left + 320, "panel must be reserved, dpi={dpi}, layout={tab_layout:?}");
                assert_eq!(with_panel.right, base.right);
                assert_eq!(with_panel.top, base.top);
                assert_eq!(with_panel.bottom, base.bottom);
                assert!(valid_viewport_bounds(with_panel));
            }
        }
    }

    #[test]
    fn workspace_rail_wrappers_preserve_order_metadata_and_active_state() {
        let app = app_with_pending(rect(800, 600));
        let id = App::workspace_create(&app, "Work").unwrap();
        App::workspace_set_icon(&app, id, crate::workspaces::WorkspaceIcon::Code).unwrap();
        App::workspace_set_colour(&app, id, Some(crate::workspaces::WorkspaceColour::Teal)).unwrap();
        App::workspace_move(&app, id, 0).unwrap();

        let entries = App::workspace_entries(&app);
        assert_eq!(entries[0].0, id);
        assert_eq!(entries[0].1, "Work");
        assert_eq!(entries[0].2, crate::workspaces::WorkspaceIcon::Code);
        assert_eq!(entries[0].3, Some(crate::workspaces::WorkspaceColour::Teal));
        assert_eq!(entries[0].4, 1);
        assert!(entries[0].5);
        assert_eq!(entries[1].1, "Default");
        assert!(!entries[1].5);
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
    fn chrome_environment_waits_for_or_starts_the_one_tab_engine() {
        use ChromeEnvironmentStep::*;
        // (ready, closing, settled, initializing)
        assert_eq!(chrome_environment_step(true, false, true, false), Ready);
        assert_eq!(chrome_environment_step(true, true, true, false), Unavailable, "closing");
        assert_eq!(chrome_environment_step(false, true, false, false), Unavailable, "closing");
        // Creation finished without a usable environment: native chrome, never a second engine.
        assert_eq!(chrome_environment_step(false, false, true, false), Unavailable);
        assert_eq!(chrome_environment_step(false, false, true, true), Unavailable);
        // Already being created (by a submit or prewarm): wait for that same environment.
        assert_eq!(chrome_environment_step(false, false, false, true), Wait);
        // Not started yet (native mode starts it lazily): start it now.
        assert_eq!(chrome_environment_step(false, false, false, false), StartEngine);
    }

    #[test]
    fn prewarm_during_early_environment_creation_is_kept_for_when_it_is_ready() {
        let app = app_with_pending(rect(800, 600));
        {
            let mut state = app.borrow_mut();
            state.pending = None;
            state.pending_tab = None;
            state.initializing = true;
            state.prewarm_requested = false;
        }
        App::prewarm(&app, "exa");
        assert!(app.borrow().prewarm_requested);
        assert!(app.borrow().initializing);
    }

    #[test]
    fn tab_change_posts_coalesce_and_merge_structural_persistence_flags() {
        assert!(App::post_tab_changed_kind(
            HWND::default(),
            TabId(1),
            false,
            false
        ));
        assert!(!App::post_tab_changed_kind(
            HWND::default(),
            TabId(2),
            true,
            true
        ));
        assert_eq!(App::take_tab_change_flags(), (true, true));
    }

    #[test]
    fn typing_prewarm_requests_only_the_active_controller_without_navigation() {
        let app = app_with_pending(windows::Win32::Foundation::RECT::default());
        let calls = Rc::new(RefCell::new(Vec::new()));
        {
            let mut state = app.borrow_mut();
            state.initializing = false;
            state.pending = None;
            state.pending_tab = None;
            state.engine = Some(Box::new(TabRecordingEngine {
                calls: calls.clone(),
                playing: Vec::new(),
            }));
        }
        App::prewarm(&app, "example");
        assert_eq!(*calls.borrow(), ["prewarm:0"]);
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

    #[test]
    fn blocked_title_refreshes_at_most_every_interval() {
        assert_eq!(title_wait(None), None);
        assert_eq!(title_wait(Some(Duration::from_millis(250))), None);
        assert_eq!(title_wait(Some(Duration::from_millis(900))), None);
        assert_eq!(
            title_wait(Some(Duration::from_millis(100))),
            Some(Duration::from_millis(150))
        );
        assert_eq!(title_wait(Some(Duration::ZERO)), Some(TITLE_SYNC_INTERVAL));
    }
}

#[cfg(test)]
mod tab_marker_tests {
    use super::*;
    use crate::chrome::tab_card::Marker;
    use url::Url;

    fn domain(text: &str) -> tab_groups::DomainKey {
        tab_groups::DomainKey::from_url(&Url::parse(&format!("https://{text}/")).unwrap()).unwrap()
    }

    fn group(text: &str) -> TabGroupKey {
        TabGroupKey::Domain(domain(text))
    }

    #[test]
    fn grouped_tab_shows_only_its_group_entry() {
        let key = group("alpha.example");
        let mut groups = HashMap::new();
        let mut tabs = HashMap::new();
        groups.insert(key.clone(), Marker::Heart);
        // A stale per-tab entry must not show through while the tab is in a group.
        tabs.insert(TabId(1), Marker::Eyes);

        assert_eq!(
            resolve_marker(Some(&key), TabId(1), &groups, &tabs),
            Some(Marker::Heart)
        );
        assert_eq!(
            resolve_marker(Some(&key), TabId(2), &groups, &tabs),
            Some(Marker::Heart)
        );
    }

    #[test]
    fn ungrouped_tab_shows_its_own_entry_and_never_a_group_entry() {
        let mut groups = HashMap::new();
        let mut tabs = HashMap::new();
        groups.insert(group("alpha.example"), Marker::Heart);
        tabs.insert(TabId(3), Marker::Bag);

        assert_eq!(
            resolve_marker(None, TabId(3), &groups, &tabs),
            Some(Marker::Bag)
        );
        assert_eq!(resolve_marker(None, TabId(4), &groups, &tabs), None);
    }

    #[test]
    fn group_without_entry_has_no_marker() {
        let groups = HashMap::new();
        let tabs = HashMap::new();
        let key = group("beta.example");
        assert_eq!(resolve_marker(Some(&key), TabId(1), &groups, &tabs), None);
    }

    #[test]
    fn set_entry_reports_only_real_changes() {
        let mut map = HashMap::new();
        assert!(set_entry(&mut map, TabId(1), Some(Marker::Flame)));
        assert!(!set_entry(&mut map, TabId(1), Some(Marker::Flame)));
        assert!(set_entry(&mut map, TabId(1), Some(Marker::Palm)));
        assert_eq!(map.get(&TabId(1)), Some(&Marker::Palm));
        assert!(set_entry(&mut map, TabId(1), None));
        assert!(!set_entry(&mut map, TabId(1), None));
        assert!(map.is_empty());
    }

    fn card_tab(id: u32, title: &str, opener: Option<u32>, favicon: bool) -> CardTab<'_> {
        CardTab {
            id: TabId(id),
            title,
            opener: opener.map(TabId),
            has_favicon: favicon,
        }
    }

    #[test]
    fn card_rows_list_only_other_members_of_the_hovered_group() {
        let tabs = [
            card_tab(1, "Alpha one", None, true),
            card_tab(2, "Beta", None, false),
            card_tab(3, "Alpha two", None, true),
            card_tab(4, "Alpha three", None, false),
        ];
        let keys = [
            Some(group("alpha.example")),
            None,
            Some(group("alpha.example")),
            Some(group("alpha.example")),
        ];

        let rows = card_rows(&tabs, &keys, 0, 1);

        assert_eq!(
            rows.iter().map(|row| row.tab_index).collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(rows[0].title, "Alpha two");
        assert!(rows[0].favicon_present);
        assert!(!rows[1].favicon_present);
        assert!(rows.iter().all(|row| !row.active && !row.linked));
    }

    #[test]
    fn card_rows_are_empty_for_a_tab_in_no_group() {
        let tabs = [
            card_tab(1, "Solo", None, true),
            card_tab(2, "Other", None, true),
        ];
        let keys = [None, None];
        assert!(card_rows(&tabs, &keys, 0, 0).is_empty());
    }

    #[test]
    fn card_rows_mark_the_active_tab_and_opener_links() {
        let tabs = [
            card_tab(10, "Parent", None, true),
            card_tab(11, "Child", Some(10), true),
            card_tab(12, "Unrelated sibling", None, true),
        ];
        let key = Some(TabGroupKey::Opener(TabId(10)));
        let keys = [key.clone(), key.clone(), key];

        let rows = card_rows(&tabs, &keys, 0, 2);

        assert_eq!(
            rows.iter().map(|row| row.tab_index).collect::<Vec<_>>(),
            [1, 2]
        );
        assert!(rows[0].linked, "child was opened by the hovered tab");
        assert!(rows[0].linked && !rows[0].active);
        assert!(rows[1].active);
        assert!(!rows[1].linked);
    }

    #[test]
    fn card_rows_name_blank_titles_as_new_tab() {
        let tabs = [
            card_tab(1, "Hovered", None, true),
            card_tab(2, "  ", None, false),
        ];
        let keys = [Some(group("gamma.example")), Some(group("gamma.example"))];
        let rows = card_rows(&tabs, &keys, 0, 0);
        assert_eq!(rows[0].title, tab_card::NEW_TAB_TITLE);
    }
}
