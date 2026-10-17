use std::{
    cell::RefCell,
    collections::HashMap,
    hash::Hash,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant, SystemTime},
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
    /// The form fields of a search with an engine that POSTs its query, for the one navigation to
    /// that address that follows (see `App::open_destination`).
    static POST_BODY: std::cell::RefCell<Option<(url::Url, String)>> = const { std::cell::RefCell::new(None) };
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
    efficiency::{self, EfficiencyState},
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

/// Whether a tab-state change may arm the session save timer at all. Checked before a snapshot is
/// built: with a save already queued, nothing is built per change.
fn session_arm_allowed(has_store: bool, save_pending: bool, closing: bool) -> bool {
    has_store && !save_pending && !closing
}

/// A change that touches the workspace file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkspaceChange {
    Create,
    Delete,
    Rename,
    Move,
    Icon,
    Colour,
    SwitchWorkspace,
    TabSelect,
    TabOpen,
    TabClose,
    TabMove,
}

/// How soon a workspace change must reach disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkspaceSave {
    /// Marks the file dirty; the session debounce timer, or the exit flush, writes it.
    Deferred,
    /// Writes at once.
    Now,
}

/// Structural edits to workspaces are rare and written at once. Tab traffic (selecting, opening,
/// closing, moving) is frequent and is written by the debounce timer instead.
fn workspace_save_mode(change: WorkspaceChange) -> WorkspaceSave {
    match change {
        WorkspaceChange::Create
        | WorkspaceChange::Delete
        | WorkspaceChange::Rename
        | WorkspaceChange::Move
        | WorkspaceChange::Icon
        | WorkspaceChange::Colour => WorkspaceSave::Now,
        WorkspaceChange::SwitchWorkspace
        | WorkspaceChange::TabSelect
        | WorkspaceChange::TabOpen
        | WorkspaceChange::TabClose
        | WorkspaceChange::TabMove => WorkspaceSave::Deferred,
    }
}

/// Modified time and length of a search-config file; a missing file is `(None, 0)`.
type ConfigStamp = (Option<SystemTime>, u64);

fn config_stamp(metadata: Option<&std::fs::Metadata>) -> ConfigStamp {
    metadata.map_or((None, 0), |metadata| (metadata.modified().ok(), metadata.len()))
}

/// Whether either config file differs from the stamps taken at the last read.
fn search_config_changed(previous: Option<[ConfigStamp; 2]>, now: [ConfigStamp; 2]) -> bool {
    previous != Some(now)
}

/// Whether a tab with this address needs a content WebView2. Internal pages (`bowser://`) never
/// do: Bowser draws them itself (docs/design/settings-page.md).
pub fn tab_needs_controller(url: Option<&url::Url>) -> bool {
    url.is_some_and(|url| !crate::internal_page::is_internal(url))
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

/// Whether a tab hidden for `hidden_for` is due to be suspended. `suspend_after` comes from the
/// background policy: 30 s normally, shorter while efficiency mode is active.
fn hidden_tab_suspend_eligible(hidden_for: Duration, suspend_after: Duration) -> bool {
    hidden_for >= suspend_after
}

/// How long after a page finishes loading the reader probe runs, so it stays off the first paint.
const READER_PROBE_DELAY_MS: u32 = 1_200;

/// Whether a scheduled reader probe should still run. `pending` is the tab and reader epoch stored
/// at schedule time; `page_usable`, `controller_created`, `discarded`, `probed` and `epoch_now`
/// describe that tab now. A tab that was left, navigated, discarded or already probed is skipped.
fn reader_probe_due(
    pending: Option<(TabId, u32)>,
    active: TabId,
    page_usable: bool,
    controller_created: bool,
    discarded: bool,
    probed: bool,
    epoch_now: u32,
) -> bool {
    let Some((tab, epoch)) = pending else {
        return false;
    };
    tab == active
        && epoch == epoch_now
        && page_usable
        && controller_created
        && !discarded
        && !probed
}

/// The tracking-prevention level to request when the engine is created. A benchmark's explicit
/// choice wins; other benchmark launches keep the engine default so recorded baselines stay
/// comparable; an ordinary launch states the user's setting (Balanced unless they chose Strict).
fn tracking_prevention_request(
    benchmark_choice: Option<TrackingPreventionLevel>,
    benchmark_launch: bool,
    setting: crate::settings::TrackingLevel,
) -> Option<TrackingPreventionLevel> {
    benchmark_choice.or_else(|| {
        (!benchmark_launch).then_some(match setting {
            crate::settings::TrackingLevel::Balanced => TrackingPreventionLevel::Balanced,
            crate::settings::TrackingLevel::Strict => TrackingPreventionLevel::Strict,
        })
    })
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
/// Tabs kept in `tab_mru` (`ctrl_tab_mru`), oldest dropped first.
const MAX_TAB_MRU: usize = 64;

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
    /// A private tab is never discarded: closing its controller could end the in-memory private
    /// profile, and the page could not be restored without its cookies.
    pub private: bool,
}

/// `audible` is queried last because it crosses into WebView2, and most tabs fail an earlier check.
pub fn tab_discard_eligible(tab: DiscardCandidate, audible: impl FnOnce() -> bool) -> bool {
    !tab.active
        && !tab.private
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
            timer: IdleTimerAction::Kill,
        };
    }
    let target = if idle_ms >= 3_000 {
        MemoryUsageTarget::Low
    } else {
        MemoryUsageTarget::Normal
    };
    let timer = if target == MemoryUsageTarget::Low {
        IdleTimerAction::Kill
    } else {
        IdleTimerAction::Arm(1_000)
    };
    IdleMemoryDecision { target, timer }
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

/// What a downloads action leaves for the window to do (see `App::download_act`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadFollowup {
    /// Done (or nothing more to do).
    None,
    /// Open `bowser://downloads`.
    OpenPage,
    /// Open this recorded http(s) URL in a new tab (Retry).
    OpenTab(String),
    /// Put this text on the clipboard (Copy link).
    CopyText(String),
    /// The id is unknown or the action is not legal for the entry: ignore the command.
    Rejected,
    /// The action could not be carried out (the file is gone, the shell or engine refused).
    Failed,
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
    /// --- tab stacks (T10) --- further `--open-url` addresses, opened after `initial_url`.
    more_initial_urls: Vec<url::Url>,
    pub allowed_benchmark_file: Option<url::Url>,
    pub benchmark_tracking_prevention: Option<TrackingPreventionLevel>,
    pub benchmark_memory_pressure: bool,
    pub provider: SearchProvider,
    /// Stamps of `settings.txt` and the search provider file when `provider` was last read.
    search_config_stamp: Option<[ConfigStamp; 2]>,
    pub log: Rc<PerfLog>,
    pub minimized: bool,
    /// Power source, efficiency decision and EcoQoS state. See `crate::efficiency`.
    pub efficiency: EfficiencyState,
    pub desired_memory_target: MemoryUsageTarget,
    pub effective_memory_target: Option<MemoryUsageTarget>,
    pub memory_target_unsupported_logged: bool,
    pub last_valid_viewport_bounds: Option<windows::Win32::Foundation::RECT>,
    pub has_navigated: bool,
    pub page_loaded: bool,
    navigation_progress: NavigationProgress,
    tabs: Vec<AppTab>,
    tab_model: crate::tabs::TabList,
    /// Tabs left most recently first, for `ctrl_tab_mru`. Updated in `select_tab`; split-pane
    /// selection does not go through it, so it only orders ordinary tab switches.
    tab_mru: Vec<TabId>,
    /// The reader probe waiting on `WM_READER_PROBE_TIMER`: the tab and its reader epoch when the
    /// probe was scheduled. A newer schedule replaces it.
    reader_probe_pending: Option<(TabId, u32)>,
    /// The feeds each tab's page offered, for the address bar's feed button (thread T13). In memory only.
    feed_pages: HashMap<TabId, feed_features::PageFeeds>,
    /// Visible pane relationship. Session recovery intentionally restores its tabs as a normal view.
    split_view: Option<SplitView>,
    // --- split view ---
    /// The window's split pair while another tab is shown alone (Chrome keeps the pair as one tab).
    /// Selecting either member shows the pair again. At most one of this and `split_view` is set.
    split_parked: Option<SplitView>,
    /// Recently closed tabs for the split view's empty side (newest first, http(s) only, never a
    /// private tab). In memory only.
    recently_closed: std::collections::VecDeque<ClosedTab>,
    // --- tab options (src/tab_state.rs) ---
    /// Strip selection, the one-second tick and hover thumbnails.
    tab_state: crate::tab_state::TabState,
    // --- end tab options ---
    /// A split side's controller became ready; the window re-applies the panes.
    split_refresh: bool,
    // --- end split view ---
    split_bounds: Option<SplitBounds>,
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
    /// A deferred workspace change waits for the debounce timer or the exit flush.
    workspaces_dirty: bool,
    settings: crate::settings::Settings,
    /// --- wave-1 fixes --- what `settings` last agreed with the file: a save from memory writes only the
    /// keys that differ from this (`settings::merge_changed`), so it never reverts a newer file value.
    settings_synced: crate::settings::Settings,
    session: Option<crate::session::SessionWriter>,
    session_save_pending: bool,
    last_session_snapshot: Option<crate::session::Session>,
    pending_restore: Option<crate::session::Session>,
    privacy_stats: Option<crate::privacy_stats::PrivacyStats>,
    privacy_stats_path: Option<PathBuf>,
    privacy_session_blocked: u64,
    /// Ad/tracker blocker shared with the engine; `None` for benchmark launches or when disabled.
    blocker: Option<engine::BlockerHandle>,
    /// Blocker for private tabs: always consulted, whatever the global switch and site exceptions
    /// say. `None` only for benchmark launches, which never open private tabs.
    private_blocker: Option<engine::BlockerHandle>,
    /// Stored per-site permission decisions; `None` for benchmark launches, which deny every request.
    permissions: Option<engine::PermissionsHandle>,
    /// Per-site preferences (page zoom). `None` for benchmark launches; guest sessions get an
    /// in-memory store.
    site_prefs: Option<crate::site_prefs::SitePrefsHandle>,
    /// The newest denied permission requests and blocked popups. In memory only, origins and
    /// kinds only; shown on the Privacy dashboard.
    denied: crate::permissions::DeniedLog,
    /// Reader theme and size as last reported by the page, passed back in on the next use. Session
    /// only.
    reader_prefs: crate::reader::Prefs,
    /// Page translation's job and worker bookkeeping; plain data until a translate command runs.
    translate: page_translate::Runtime,
    /// The downloads list (docs/design/downloads.md, `crate::downloads`): fed by the engine's
    /// download events, read by the chrome bridge through the `downloads_*` methods below.
    downloads: crate::downloads::DownloadManager,
    /// Told after every change to `downloads` (set by the window; see `set_downloads_observer`).
    downloads_observer: Option<Rc<dyn Fn(crate::downloads::Change)>>,
    /// --- tab stacks (T10) --- sites that open in a named workspace (`workspace-rules.txt`).
    workspace_rules: Vec<crate::workspace_rules::Rule>,
}

// --- split view ---
/// The pane rectangles of a shown split (two to four panes; a pane hidden by the collapse rule has
/// none), in the window's client coordinates.
pub type SplitBounds = crate::split_view::PaneMap<windows::Win32::Foundation::RECT>;

/// A closed tab offered again by the split view's empty side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosedTab {
    pub url: String,
    pub title: String,
    // --- tab options --- what "Reopen closed tab" needs to put the tab back as it was.
    pub pinned: bool,
    /// Its place in the strip when it closed (the strip may be shorter by now).
    pub index: usize,
    // --- end tab options ---
}
// --- end split view ---

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
    /// A permission request from the current page was denied (toolbar indicator).
    permission_blocked: bool,
    /// The page zoom in percent this tab last showed or was set to (100 for a new controller).
    zoom: u16,
    /// The user gave this tab its zoom (page tools, "tab zoom"): it then keeps it across pages.
    zoom_set: bool,
    /// Outcome of the last top-level navigation, for the connection state in Site information.
    load: LoadState,
    /// A private (InPrivate) tab: see `crate::private`. Fixed when the tab is created.
    private: bool,
    /// Reader view: whether the page looks like an article and whether reader view is on.
    reader: crate::reader::TabState,
    /// Page translation: the page's language, whether it is translated, and the epoch that makes
    /// work for an earlier document stale (`crate::translate`).
    translate: crate::translate::TabState,
    // --- tab options ---
    /// Unread mark, notification count, audio-rule mute, reload timer, custom name, last use.
    extra: crate::tab_state::TabExtra,
    // --- end tab options ---
}

impl AppTab {
    fn empty(id: TabId) -> Self {
        Self {
            id,
            private: false,
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
            permission_blocked: false,
            zoom: crate::site_prefs::ZOOM_DEFAULT,
            zoom_set: false,
            load: LoadState::Loaded,
            reader: crate::reader::TabState::default(),
            translate: crate::translate::TabState::default(),
            extra: crate::tab_state::TabExtra::fresh(), // --- tab options ---
        }
    }
}

mod page_translate;
mod page_actions; // page tools: capture, selection, applying the settings (thread T7)
pub use page_actions::{CaptureResult, Selected, capture_busy};
pub use page_translate::Notice as TranslateNotice;
mod feed_features; // toolbar and feed options at work (thread T13)
pub(crate) use feed_features::MAX_FEED_ANSWER_BYTES;
mod privacy; // privacy, history and website appearance (thread T3)
pub use privacy::DeleteReport;
// --- tab options --- tab behaviour that follows the settings page's Tabs section.
mod tab_features;
pub use tab_features::{TabStripExtra, MAX_SESSION_NAME_CHARS, MAX_TAB_NAME_CHARS};
// --- end tab options ---
// --- theme options --- colours, accent mode, schedule and the theme file (thread T8).
mod theme_features;
// --- end theme options ---
mod stacks; // --- tab stacks, tiling and workspaces (T10) ---

pub type AppRef = Rc<RefCell<App>>;

/// What a reader-view command did, so the caller can tell the user when nothing happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReaderResult {
    /// The page is now in this state (`Off` when the command left a mode).
    Applied(crate::reader::Active),
    /// The page has no article-like text, so reader view changed nothing.
    NothingToRead,
    /// Not an http(s) page, no live page, or a call is already running.
    Unavailable,
    /// The page did not answer or the script failed; the page was left as it was.
    Failed,
}

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

/// --- wave-1 integration --- `App::save_settings` writes the in-memory settings, but these keys are
/// saved by the settings page straight to the file and read at startup or from the file, so memory
/// holds their startup value. Without this, any later save (a Tabs option, a sidebar drag, the
/// translate or download settings) put the old value back in the file.
fn keep_file_owned_keys(settings: &mut crate::settings::Settings, file: &crate::settings::Settings) {
    settings.automatic_tab_grouping = file.automatic_tab_grouping;
    settings.tracking_prevention = file.tracking_prevention;
    settings.block_youtube_video_ads = file.block_youtube_video_ads;
    // --- layout (thread T6) --- `side_panel_position` is live now (`apply_live_settings`), so memory holds it.
    // --- auto-hide (T11) --- so is `frameless` (the auto-hide master switch).
    settings.search_engine = file.search_engine;
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
            win32::extensions_changed();
            return;
        }
        let report = id.clone();
        Self::with_engine(app, move |engine| {
            engine.set_extension_enabled(
                &id,
                &folder,
                enabled,
                Box::new(move |result| {
                    crate::extensions::record_load_result(&report, result);
                    // The toolbar button and the settings page show load results; they catch up later.
                    win32::extensions_changed();
                }),
            );
            Ok(())
        });
    }

    /// Opens a loaded extension's popup or options page in a Bowser-owned window. A failure is
    /// shown to the user; it is not recorded as a load failure, because the extension itself loaded.
    pub fn open_extension_page(
        app: &AppRef,
        id: String,
        folder: PathBuf,
        kind: crate::extensions::PageKind,
        title: String,
    ) {
        if app.borrow().engine.is_none() {
            win32::show_error_message("Bowser is still starting. Try again in a moment.");
            return;
        }
        // The engine is taken out of `app` while it runs, so a refusal that comes back at once is
        // shown after the call, not from inside it.
        let in_call = std::rc::Rc::new(std::cell::Cell::new(true));
        let early = std::rc::Rc::new(std::cell::RefCell::new(None::<String>));
        let (call_flag, call_early) = (in_call.clone(), early.clone());
        Self::with_engine(app, move |engine| {
            engine.open_extension_page(
                &id,
                &folder,
                kind,
                &title,
                Box::new(move |result| {
                    if let Err(reason) = result {
                        if call_flag.get() {
                            *call_early.borrow_mut() = Some(reason);
                        } else {
                            win32::show_error_message(&format!("The extension page was not opened. {reason}"));
                        }
                    }
                }),
            );
            Ok(())
        });
        in_call.set(false);
        if let Some(reason) = early.borrow_mut().take() {
            win32::show_error_message(&format!("The extension page was not opened. {reason}"));
        }
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
        let active = state.tab_model.active();
        if let Some(tab) = state.tabs.iter().find(|tab| tab.id == active && tab.private) {
            // The blocker is always on in a private tab; the site exceptions do not apply.
            return if tab.blocked > 0 {
                format!(" [Private, {} blocked]", tab.blocked)
            } else {
                " [Private]".to_owned()
            };
        }
        let Some(blocker) = state.blocker.as_ref() else {
            return String::new();
        };
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
        } else if !blocker.borrow().is_blocking(host, false) {
            // Blocking is off everywhere (level none): nothing was blocked, so nothing to say.
            String::new()
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
        if state
            .tabs
            .iter()
            .any(|tab| tab.id == state.tab_model.active() && tab.private)
        {
            return Some(true);
        }
        let blocker = state.blocker.as_ref()?;
        let host = state
            .tabs
            .iter()
            .find(|tab| tab.id == state.tab_model.active())
            .and_then(|tab| tab.url.as_ref())
            .and_then(|url| url.host_str())?;
        // At any level above none (thread T3); a site at none, or global none, reads as off.
        Some(blocker.borrow().is_blocking(host, false))
    }

    pub fn tab_layout(app: &AppRef) -> crate::settings::TabLayout {
        app.borrow().settings.tab_layout
    }

    // --- layout (thread T6) ---
    /// How the chrome is arranged, from the layout settings (the window folds in its Ctrl+L reveal).
    pub fn arrangement(app: &AppRef) -> crate::chrome::layout::Arrangement {
        app.borrow().arranged() // --- two-level stacks (T10b) ---
    }

    /// --- two-level stacks (T10b) --- the layout settings plus the second tab row two-level
    /// stacks want now: the one arrangement the window and the page bounds both lay out.
    fn arranged(&self) -> crate::chrome::layout::Arrangement {
        crate::chrome::layout::Arrangement::from_settings(&self.settings).with_stack_row(self.stack_row())
    }
    // --- end layout ---

    /// Layout width of the vertical sidebar: 48 when collapsed, else the saved width.
    pub fn sidebar_width(app: &AppRef) -> u16 {
        app.borrow().settings.effective_sidebar_width()
    }

    pub fn sidebar_collapsed(app: &AppRef) -> bool {
        app.borrow().settings.sidebar_collapsed
    }

    /// The setting-gated chords bound right now. Read from key handlers that may run while the app is
    /// borrowed, so a busy app binds none and the page keeps the key.
    pub fn gated_chords(app: &AppRef) -> crate::shortcuts::GatedChords {
        app.try_borrow()
            .map(|state| {
                crate::shortcuts::GatedChords::new(
                    state.settings.ctrl_shift_c_copy_link,
                    state.settings.ctrl_s_vertical_tabs,
                    state.settings.tab_layout.is_vertical(),
                )
                .with_stacked_split(state.split_view.is_some_and(|split| split.has_rows()))
            })
            .unwrap_or_default()
    }

    /// `shift_right_click_context`: Shift+Right Click shows the system menu, not Bowser's own.
    pub fn shift_right_click_context(app: &AppRef) -> bool {
        app.try_borrow()
            .is_ok_and(|state| state.settings.shift_right_click_context)
    }

    /// Collapses or expands the sidebar (the saved expanded width is untouched) and relays out.
    pub fn set_sidebar_collapsed(app: &AppRef, collapsed: bool) {
        let hwnd = {
            let mut state = app.borrow_mut();
            state.settings.sidebar_collapsed = collapsed;
            state.hwnd
        };
        let bounds = app.borrow_mut().latest_viewport_bounds();
        Self::resize_current(app, bounds);
        Self::post_tab_changed(hwnd, Self::active_tab_id(app));
    }

    pub fn set_sidebar_width(app: &AppRef, width: u16) {
        let hwnd = {
            let mut state = app.borrow_mut();
            // Dragging the resize handle of a collapsed sidebar expands it again.
            state.settings.sidebar_collapsed = false;
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
            if let Err(error) = Self::save_settings_to(app, &profile.settings_file()) {
                win32::show_error_message(&format!("Settings could not be saved: {error}"));
            }
        }
    }

    /// --- wave-1 fixes --- [`App::save_settings`] into `path`: only the keys the running app changed since
    /// memory last agreed with the file are written; every other key keeps the file's (newer) value.
    pub(crate) fn save_settings_to(app: &AppRef, path: &std::path::Path) -> std::io::Result<()> {
        let (memory, synced) = {
            let state = app.borrow();
            (state.settings.clone(), state.settings_synced.clone())
        };
        let file = crate::settings::Settings::load(path);
        let mut settings = crate::settings::merge_changed(&synced, &memory, &file);
        // The side panel writes its own width straight to the file.
        settings.side_panel_width = file.side_panel_width;
        // --- wave-1 integration --- keys the running app never changes itself keep the file's value.
        keep_file_owned_keys(&mut settings, &file);
        crate::settings::save(path, &settings)?;
        app.borrow_mut().settings_synced = memory;
        Ok(())
    }

    /// --- wave-1 fixes --- after memory took values from the file (`apply_live_settings`), those keys count
    /// as agreed with the file, so a later save from memory does not push them over a newer file value.
    fn mark_synced_from(app: &AppRef, before: &crate::settings::Settings) {
        let mut state = app.borrow_mut();
        let now = state.settings.clone();
        state.settings_synced = crate::settings::merge_changed(before, &now, &state.settings_synced);
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
        Self::save_workspaces(app, WorkspaceChange::Icon);
        Self::notify_rail_changed(app);
        Ok(())
    }

    pub fn workspace_set_colour(
        app: &AppRef,
        id: crate::workspaces::WorkspaceId,
        colour: Option<crate::workspaces::WorkspaceColour>,
    ) -> std::result::Result<(), crate::workspaces::WorkspaceError> {
        app.borrow_mut().workspaces.set_colour(id, colour)?;
        Self::save_workspaces(app, WorkspaceChange::Colour);
        Self::notify_rail_changed(app);
        Ok(())
    }

    pub fn workspace_move(
        app: &AppRef,
        id: crate::workspaces::WorkspaceId,
        new_index: usize,
    ) -> std::result::Result<(), crate::workspaces::WorkspaceError> {
        app.borrow_mut().workspaces.reorder(id, new_index)?;
        Self::save_workspaces(app, WorkspaceChange::Move);
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
            Self::save_workspaces(app, WorkspaceChange::SwitchWorkspace);
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
        Self::save_workspaces(app, WorkspaceChange::Create);
        Self::notify_rail_changed(app);
        Ok(id)
    }

    pub fn rename_workspace(
        app: &AppRef,
        id: crate::workspaces::WorkspaceId,
        name: &str,
    ) -> std::result::Result<(), crate::workspaces::WorkspaceError> {
        app.borrow_mut().workspaces.rename(id, name)?;
        Self::save_workspaces(app, WorkspaceChange::Rename);
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
        Self::save_workspaces(app, WorkspaceChange::Delete);
        Self::notify_rail_changed(app);
        Ok(())
    }

    /// Saves the workspace file as `workspace_save_mode` decides: at once, or by marking it dirty
    /// and arming the session debounce timer.
    fn save_workspaces(app: &AppRef, change: WorkspaceChange) {
        if workspace_save_mode(change) == WorkspaceSave::Now {
            Self::save_workspaces_now(app);
            return;
        }
        let hwnd = {
            let mut state = app.borrow_mut();
            state.workspaces_dirty = true;
            state.hwnd
        };
        if hwnd.is_invalid() {
            // No window yet to carry the timer, so nothing could flush the mark.
            Self::save_workspaces_now(app);
            return;
        }
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetTimer(
                Some(hwnd),
                win32::WM_SESSION_SAVE_TIMER,
                crate::session::SAVE_DEBOUNCE_MS,
                None,
            );
        }
    }

    fn save_workspaces_now(app: &AppRef) {
        app.borrow_mut().workspaces_dirty = false;
        let (path, workspaces) = {
            let state = app.borrow();
            let order = state
                .tab_model
                .iter()
                .filter_map(|model| {
                    let tab = state.tabs.iter().find(|tab| tab.id == model.id)?;
                    let url = crate::private::snapshot_url(tab.private, tab.url.as_ref())?;
                    crate::session::persistable_url(url).map(|_| model.id)
                })
                .take(crate::session::MAX_TABS)
                .collect::<Vec<_>>();
            let private: std::collections::HashSet<TabId> = state
                .tabs
                .iter()
                .filter(|tab| tab.private)
                .map(|tab| tab.id)
                .collect();
            let mut workspaces = state.workspaces.without_tabs(&private);
            workspaces.set_session_order(&order);
            (state.workspaces_path.clone(), workspaces)
        };
        if let Some(path) = path {
            if let Err(error) = workspaces.save(&path) {
                win32::show_error_message(&format!("Workspaces could not be saved: {error}"));
            }
        }
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
        if Self::active_tab_private(app) {
            // Site exceptions are saved to disk; a private tab never writes or honours them.
            win32::show_error_message("The blocker is always on in private tabs.");
            return;
        }
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
                format!(
                    "{title}{}{profile_suffix}{}",
                    Self::blocker_suffix(&state),
                    crate::profile::dev_title_suffix()
                ),
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
        Self::new_tab_in_mode(app, false)
    }

    /// A new tab that browses privately: its controller is created InPrivate in the shared private
    /// profile and nothing it does is persisted (see `crate::private`).
    pub fn new_private_tab(app: &AppRef) -> TabId {
        Self::new_tab_in_mode(app, true)
    }

    /// The private tabs of this window, in every workspace.
    pub fn private_tab_ids(app: &AppRef) -> Vec<TabId> {
        app.borrow().tabs.iter().filter(|tab| tab.private).map(|tab| tab.id).collect()
    }

    /// "Close all private tabs" (main menu, tab menu, command palette, private landing): closes
    /// every private tab of this window and leaves normal tabs alone. Closing the last tab still
    /// leaves a fresh normal tab, as `close_tab` always does. Returns how many closed.
    pub fn close_private_tabs(app: &AppRef) -> usize {
        Self::private_tab_ids(app)
            .into_iter()
            .filter(|id| Self::close_tab(app, *id))
            .count()
    }

    fn new_tab_in_mode(app: &AppRef, private: bool) -> TabId {
        Self::spawn_tab(app, private, None)
    }

    /// A new tab shown at once. `opener` is the tab whose page asked for it (a link or a script),
    /// which the "New tab position" option can keep it next to.
    pub(crate) fn spawn_tab(app: &AppRef, private: bool, opener: Option<TabId>) -> TabId {
        let (id, hwnd, old, old_has_controller, in_split) = {
            let mut state = app.borrow_mut();
            let old = state.tab_model.active();
            let in_split = state.split_view.is_some();
            let old_has_controller = state
                .tabs
                .iter()
                .find(|tab| tab.id == old)
                .is_some_and(|tab| tab.controller_created);
            let placement = tab_features::placement(&state.settings);
            let id = state.tab_model.add_placed(placement, opener);
            state.tab_model.select(id);
            let _ = state.workspaces.add_tab(id);
            tab_features::place_in_workspace(&mut state, id); // --- tab options ---
            let _ = state.workspaces.select_tab(id);
            // --- split view ---
            // A new tab is shown alone, as in Chrome; the pair stays together (parked) in the strip.
            state.park_split();
            // --- end split view ---
            let mut fresh = AppTab::empty(id);
            fresh.private = private;
            // --- tab options --- the strip's list follows the model's order, as `set_tab_pinned` expects.
            let at = state.tab_model.index_of(id).map_or(state.tabs.len(), |at| at.min(state.tabs.len()));
            state.tabs.insert(at, fresh);
            // --- end tab options ---
            state.navigation_progress.reset();
            state.page_loaded = false;
            state.desired_memory_target = MemoryUsageTarget::Normal;
            (id, state.hwnd, old, old_has_controller, in_split)
        };
        if private {
            // Before the engine first hears of the tab, so its controller is created InPrivate.
            Self::with_engine(app, |engine| {
                engine.set_tab_private(id);
                Ok(())
            });
        }
        if in_split {
            // Parking the split hides the pair. A blank tab has no page yet, so that pass
            // registers nothing. Select it afterwards: otherwise the next search is
            // E_POINTER (the engine has never heard of the tab) and the page stays blank.
            Self::apply_visible_panes(app);
            Self::with_engine(app, |engine| {
                engine.select_tab(id)?;
                let _ = engine.set_memory_usage_target(MemoryUsageTarget::Normal);
                Ok(())
            });
            Self::post_tab_changed(hwnd, id);
            Self::save_workspaces(app, WorkspaceChange::TabOpen);
            Self::sync_window_title(app);
            Self::schedule_tab_idle_timer(app);
            Self::on_tab_shown(app, old, id); // --- tab options ---
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
                    let _ = state.log.record("tab_hidden");
                }
            }
        }
        Self::with_engine(app, |engine| {
            engine.select_tab(id)?;
            let _ = engine.set_memory_usage_target(MemoryUsageTarget::Normal);
            Ok(())
        });
        Self::post_tab_changed(hwnd, id);
        Self::save_workspaces(app, WorkspaceChange::TabOpen);
        Self::sync_window_title(app);
        Self::schedule_tab_idle_timer(app);
        Self::on_tab_shown(app, old, id); // --- tab options ---
        id
    }

    pub fn close_tab(app: &AppRef, id: TabId) -> bool {
        let mut dissolved_split = false;
        let (changed, hwnd, active, was_active, last_tab) = {
            let mut state = app.borrow_mut();
            let Some(index) = state.tabs.iter().position(|tab| tab.id == id) else {
                return false;
            };
            state.tab_markers.remove(&id);
            state.memberships.remove(&id);
            state.in_flight_domains.finish(id);
            // --- split view ---
            state.remember_closed_tab(index);
            if state.split_parked.is_some_and(|split| split.contains(id)) {
                // A parked layout of three or four panes keeps its other panes; a pair dissolves.
                state.split_parked = match state.split_parked.and_then(|split| split.without(id)) {
                    Some(crate::split_view::Remains::Layout(view)) => Some(view),
                    _ => None,
                };
            }
            let remains = state.split_view.and_then(|split| split.without(id));
            let remaining = match remains {
                Some(crate::split_view::Remains::Single(remaining)) => {
                    state.split_view = None;
                    state.split_bounds = None;
                    Some(remaining)
                }
                // A pane of a larger layout closes: its sibling expands (the window re-lays the
                // panes when it hears of the change) and the layout's focused tab is shown.
                Some(crate::split_view::Remains::Layout(view)) => {
                    state.split_view = Some(view);
                    Some(view.focused_tab())
                }
                None => None,
            };
            if let Some(remaining) = remaining {
                state.tab_model.select(remaining);
                let _ = state.workspaces.select_tab(remaining);
                if let Some((progress, usable)) = state
                    .tabs
                    .iter()
                    .find(|tab| tab.id == remaining)
                    .map(|tab| (tab.progress, tab.page_usable))
                {
                    state.navigation_progress = progress;
                    state.page_loaded = usable;
                }
                dissolved_split = true;
            }
            // --- end split view ---
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
                // --- tab options --- what the close-activation options decide from.
                let order_before: Vec<TabId> = state.workspaces.active().tabs().to_vec();
                let opener_before = state.tab_model.get(id).and_then(|tab| tab.opener);
                state.tab_state.forget(id);
                // --- end tab options ---
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
                let mut active = if was_active {
                    state.workspaces.active().active_tab()
                } else {
                    outcome.new_active
                };
                // --- tab options --- "Close tab activation": a related, recent, left or right tab.
                if was_active
                    && let Some(target) =
                        tab_features::close_target_for(&state, &order_before, id, opener_before)
                {
                    let _ = state.workspaces.select_tab(target);
                    active = target;
                }
                // --- end tab options ---
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
        win32::dev_tools_dock_tab_closed(hwnd, id);
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
        // --- split view ---
        // The surviving side was sized as half a split: give it the whole viewport again.
        if dissolved_split {
            Self::apply_visible_panes(app);
        }
        // --- end split view ---
        if changed {
            Self::post_tab_changed(hwnd, active);
        }
        Self::release_unused_site_rules(app);
        Self::save_workspaces(app, WorkspaceChange::TabClose);
        Self::sync_window_title(app);
        Self::schedule_tab_idle_timer(app);
        Self::on_tab_closed(app, active, was_active); // --- tab options ---
        changed
    }

    pub fn select_tab(app: &AppRef, id: TabId) -> bool {
        let _ = Self::expand_group_for_tab(app, id);
        // --- split view ---
        let (shown, parked, known) = {
            let state = app.borrow();
            (state.split_view, state.split_parked, state.tab_model.get(id).is_some())
        };
        if !known {
            return false;
        }
        if shown.is_some_and(|split| split.contains(id)) {
            return Self::select_split_tab(app, id);
        }
        if parked.is_some_and(|split| split.contains(id)) {
            return Self::show_parked_split(app, id);
        }
        // Another tab while the pair is shown: that tab is shown alone and the pair is parked.
        let was_split_shown = shown.is_some();
        if was_split_shown {
            app.borrow_mut().park_split();
        }
        // --- end split view ---
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
            if !was_active {
                state.tab_mru.retain(|mru| *mru != previous);
                state.tab_mru.push(previous);
                if state.tab_mru.len() > MAX_TAB_MRU {
                    state.tab_mru.remove(0);
                }
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
                            let _ = state.log.record("tab_hidden");
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
        // --- split view ---
        // `select_tab` keeps every visible controller; hide the parked pair's other side too.
        if was_split_shown {
            Self::apply_visible_panes(app);
        }
        // --- end split view ---
        Self::restore_discarded_tab(app, id);
        Self::reader_probe_if_needed(app, id);
        Self::on_tab_shown(app, previous, id); // --- tab options ---
        Self::post_tab_changed(hwnd, id);
        Self::save_workspaces(app, WorkspaceChange::TabSelect);
        Self::sync_window_title(app);
        Self::schedule_tab_idle_timer(app);
        true
    }

    // --- split view ---
    /// Shows the parked pair again with `id`'s side focused (selecting either half in the strip).
    fn show_parked_split(app: &AppRef, id: TabId) -> bool {
        let split = {
            let mut state = app.borrow_mut();
            let Some(mut split) = state.split_parked.take() else {
                return false;
            };
            let Some(pane) = split.pane_for(id) else {
                state.split_parked = Some(split);
                return false;
            };
            split.focus(pane);
            if let Some(workspace_id) = state.workspaces.workspace_for(id) {
                let _ = state.workspaces.switch_to(workspace_id);
            }
            let _ = state.workspaces.select_tab(id);
            split
        };
        let bounds = app.borrow().split_bounds;
        Self::show_split(app, split, bounds)
    }
    // --- end split view ---

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
        Self::on_tab_shown(app, active, active); // --- tab options ---
        Self::post_tab_changed(hwnd, active);
        Self::sync_window_title(app);
        Self::schedule_tab_idle_timer(app);
        true
    }

    pub fn select_next(app: &AppRef) -> TabId {
        if app.borrow().settings.ctrl_tab_mru {
            return Self::select_mru_other(app, 1);
        }
        Self::select_relative(app, 1)
    }

    pub fn select_prev(app: &AppRef) -> TabId {
        if app.borrow().settings.ctrl_tab_mru {
            return Self::select_mru_other(app, -1);
        }
        Self::select_relative(app, -1)
    }

    /// `ctrl_tab_mru`: swaps to the tab last active before this one (the top of `tab_mru`, skipping
    /// any since closed), whichever of Ctrl+Tab/Ctrl+Shift+Tab was pressed: repeated presses simply
    /// flip between the last two tabs, rather than walking further back through history. With no
    /// recorded history yet (for example, right after new tabs were only ever appended) this falls
    /// back to `delta`'s ordinary positional cycling, so the first press after startup still does
    /// something sensible.
    fn select_mru_other(app: &AppRef, delta: isize) -> TabId {
        let active = Self::active_tab_id(app);
        let candidate = {
            let state = app.borrow();
            state
                .tab_mru
                .iter()
                .rev()
                .find(|id| **id != active && state.tab_model.get(**id).is_some())
                .copied()
        };
        match candidate {
            Some(id) => {
                let _ = Self::select_tab(app, id);
                id
            }
            None => Self::select_relative(app, delta),
        }
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
        Self::save_workspaces(app, WorkspaceChange::TabMove);
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
            App::note_pinned_site(&mut state, id, pinned, None); // --- tab options ---
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
        Self::save_workspaces(app, WorkspaceChange::TabMove);
        true
    }

    /// Whether `id` is a private tab.
    pub fn tab_private(app: &AppRef, id: TabId) -> bool {
        app.borrow()
            .tabs
            .iter()
            .any(|tab| tab.id == id && tab.private)
    }

    /// Whether the active tab is a private tab.
    pub fn active_tab_private(app: &AppRef) -> bool {
        let active = app.borrow().tab_model.active();
        Self::tab_private(app, active)
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

    // --- omnibox suggest ---
    /// The tabs of the active workspace that have a page, as (id, address, title), in one borrow.
    /// The omnibox dropdown offers them as "Switch to tab" rows.
    pub fn tab_suggest_sources(app: &AppRef) -> Vec<(TabId, String, String)> {
        let state = app.borrow();
        state
            .workspaces
            .active()
            .tabs()
            .iter()
            .filter_map(|id| state.tabs.iter().find(|tab| tab.id == *id))
            .filter_map(|tab| tab.url.as_ref().map(|url| (tab.id, url, tab.title.clone())))
            // An internal page is not a site: it is never offered as a "Switch to tab" row.
            .filter(|(_, url, _)| !crate::internal_page::is_internal(url))
            .map(|(id, url, title)| (id, url.as_str().to_owned(), title))
            .collect()
    }

    /// The address `input` would navigate to, or `None` when Enter would search for it (or it is
    /// empty or unusable). Uses the search provider as last loaded; reads nothing from disk.
    pub fn typed_address(app: &AppRef, input: &str) -> Option<url::Url> {
        match resolve_input(input, &app.borrow().provider) {
            Ok(NavigationTarget::Web(url)) => Some(url),
            _ => None,
        }
    }
    // --- end omnibox suggest ---

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

    /// The group preferences the strip, the sidebar and the bookmarks bar draw from: folded the way
    /// the stack style says (--- tab stacks (T10) ---, `stacks::collapse_rule`).
    pub fn tab_group_preferences(app: &AppRef) -> crate::tab_groups::GroupPreferences {
        app.borrow().styled_group_preferences()
    }

    /// Projection inputs for the tab strip, in model order, with manual placement applied.
    pub fn tab_group_inputs(app: &AppRef) -> Vec<tab_groups::TabGroupInput> {
        app.borrow().group_inputs()
    }

    pub fn set_tab_group_mode(app: &AppRef, id: TabId, mode: crate::tab_groups::GroupMode) -> bool {
        let (hwnd, active, changed) = {
            let mut state = app.borrow_mut();
            if state.mirror(id).is_some_and(|tab| tab.private) {
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
        // --- tab stacks (T10) --- Compact and Accordion fold stacks by themselves: a click on the
        // ball goes to a member instead of storing a collapse.
        if !matches!(Self::stack_style(app), crate::tab_options::StackStyle::Islands | crate::tab_options::StackStyle::Off) {
            return Self::stack_ball(app, id);
        }
        let (hwnd, active, changed) = {
            let mut state = app.borrow_mut();
            if state.mirror(id).is_some_and(|tab| tab.private) {
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
            let inputs = state.group_inputs();
            let Some(index) = inputs.iter().position(|input| input.id == id) else {
                return true;
            };
            let keys = tab_groups::group_keys(&inputs, &state.group_preferences);
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
            // --- tab stacks (T10) --- only Islands keeps a stored collapse to undo.
            if state.tab_model.active() == id || state.settings.tabs.stack_style != crate::tab_options::StackStyle::Islands {
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
            let mut live = tab_groups::live_domains(&state.all_group_inputs());
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

    /// Inputs for the group projection: the active workspace's tabs, in the order the strip shows
    /// them. A workspace is its own world, so a tab in another workspace never counts toward a
    /// group here, never joins one, and never lends an opener edge to one.
    fn group_inputs(&self) -> Vec<tab_groups::TabGroupInput> {
        let mut all: HashMap<TabId, tab_groups::TabGroupInput> = self
            .all_group_inputs()
            .into_iter()
            .map(|input| (input.id, input))
            .collect();
        let mut inputs: Vec<tab_groups::TabGroupInput> = self
            .workspaces
            .active()
            .tabs()
            .iter()
            .filter_map(|id| all.remove(id))
            .collect();
        // --- tab stacks (T10) --- "Tab stacks: Off": nothing forms a stack. Only this view of the
        // inputs: the site rules (which span workspaces) still see every tab's site.
        if !crate::stacks::stacks_on(self.settings.tabs.stack_style) {
            for input in &mut inputs {
                input.domain = None;
                input.opener = None;
                input.manual = false;
            }
        }
        inputs
    }

    /// Inputs for every tab of every workspace, in model order, from the same state the strip
    /// reads. Only for questions that span workspaces, such as which sites still have a tab open.
    fn all_group_inputs(&self) -> Vec<tab_groups::TabGroupInput> {
        let mut inputs: Vec<_> = self
            .tab_model
            .iter()
            .map(|tab| {
                let committed = self
                    .mirror(tab.id)
                    .and_then(|mirror| mirror.url.as_ref())
                    .filter(|url| !tab_groups::is_search_results(url))
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
        // A private tab is never in a group: group names, colours and site rules are stored per
        // site, and a private tab's site must not reach them.
        for input in &mut inputs {
            if self.mirror(input.id).is_some_and(|tab| tab.private) {
                input.domain = None;
                input.opener = None;
                input.manual = false;
            }
        }
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
            .filter(|url| !tab_groups::is_search_results(url))
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
        let inputs = state.group_inputs();
        let keys = tab_groups::group_keys(&inputs, &state.group_preferences);
        let index = inputs.iter().position(|input| input.id == id)?;
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
        let active = inputs
            .iter()
            .position(|input| input.id == state.tab_model.active())
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

    // --- split view ---
    /// Puts the shown tabs on screen: both sides of a shown pair at their pane bounds, or the active
    /// tab alone at the viewport. A side whose tab has no page yet (the empty side) gets no
    /// controller: the window shows the "choose a tab" surface there instead.
    fn apply_visible_panes(app: &AppRef) {
        let (split, bounds, active, viewport, has_page) = {
            let state = app.borrow();
            let has_page = |id: TabId| {
                state
                    .tabs
                    .iter()
                    .find(|tab| tab.id == id)
                    .is_some_and(|tab| tab.controller_created || tab.url.is_some())
            };
            let split = state.split_view;
            let pages: Vec<(Pane, TabId, bool)> = split
                .map(|split| split.panes().map(|(pane, tab)| (pane, tab, has_page(tab))).collect())
                .unwrap_or_default();
            (
                split.map(|split| (split, pages)),
                state.split_bounds,
                state.tab_model.active(),
                state.last_valid_viewport_bounds,
                has_page(state.tab_model.active()),
            )
        };
        let _ = Self::with_engine(app, |engine| {
            if let (Some((split, pages)), Some(bounds)) = (split, bounds) {
                // Every pane with a page at its rectangle; a pane hidden by the collapse rule, or
                // without a page yet (the empty side), gets none.
                let panes: Vec<_> = pages
                    .iter()
                    .filter(|(_, _, page)| *page)
                    .filter_map(|(pane, tab, _)| bounds.get(*pane).map(|rect| (*tab, rect)))
                    .collect();
                engine.set_visible_tabs(&panes, split.focused_tab())
            } else if let Some(viewport) = viewport {
                // `select_tab` preserves all currently visible tabs. After leaving split view
                // that would leave the other pane controller visible over the single page.
                let panes = if has_page { vec![(active, viewport)] } else { Vec::new() };
                engine.set_visible_tabs(&panes, active)
            } else {
                engine.select_tab(active)
            }
        });
    }

    /// Parks the shown pair (another tab is about to be shown alone). Its tabs count as hidden.
    fn park_split(&mut self) {
        let Some(split) = self.split_view.take() else {
            return;
        };
        self.split_parked = Some(split);
        let now = Instant::now();
        for tab in &mut self.tabs {
            if split.contains(tab.id) {
                tab.hidden_since.get_or_insert(now);
                tab.suspended = false;
            }
        }
    }

    /// Remembers a closing tab for the empty side's "Recently closed" list.
    fn remember_closed_tab(&mut self, index: usize) {
        // The split view's empty side shows the newest eight; "Closed tabs" lists all of them.
        const KEEP: usize = 25;
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        if tab.private {
            return;
        }
        let Some(url) = tab.url.as_ref().filter(|url| matches!(url.scheme(), "http" | "https")) else {
            return;
        };
        let entry = ClosedTab {
            url: url.as_str().to_owned(),
            title: tab.title.clone(),
            pinned: self.tab_model.get(tab.id).is_some_and(|model| model.pinned),
            index,
        };
        self.recently_closed.retain(|closed| closed.url != entry.url);
        self.recently_closed.push_front(entry);
        self.recently_closed.truncate(KEEP);
    }

    /// The window's pair and whether it is shown (`false`: parked while another tab is shown).
    pub fn split_pair(app: &AppRef) -> Option<(SplitView, bool)> {
        let state = app.borrow();
        state
            .split_view
            .map(|split| (split, true))
            .or_else(|| state.split_parked.map(|split| (split, false)))
    }

    /// A tab with no page: no address and no controller (a new tab). The split view's empty side.
    pub fn tab_is_blank(app: &AppRef, id: TabId) -> bool {
        app.borrow()
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .is_some_and(|tab| !tab.controller_created && tab.url.is_none())
    }

    // --- internal pages (docs/design/settings-page.md) ---
    /// The internal page tab `id` shows, if any.
    pub fn internal_page(app: &AppRef, id: TabId) -> Option<crate::internal_page::InternalPage> {
        app.borrow()
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .and_then(|tab| tab.url.as_ref())
            .and_then(crate::internal_page::parse)
    }

    /// Opens `page`: selects a tab already showing it, else turns the shown blank normal tab into
    /// it (when `reuse_blank`), else opens it in a new tab. A settings tab is never private.
    pub fn open_internal_page(app: &AppRef, page: crate::internal_page::InternalPage, reuse_blank: bool) -> TabId {
        use crate::internal_page::OpenPlan;
        let (tabs, current) = {
            let state = app.borrow();
            let active = state.tab_model.active();
            let tabs: Vec<(TabId, bool)> = state
                .tab_model
                .iter()
                .map(|model| {
                    // Only a tab on this same page is reused (a settings tab is not a history tab).
                    let same = state
                        .tabs
                        .iter()
                        .find(|tab| tab.id == model.id)
                        .and_then(|tab| tab.url.as_ref())
                        .and_then(crate::internal_page::parse)
                        .is_some_and(|shown| shown.same_page(page));
                    (model.id, same)
                })
                .collect();
            let tab = state.tabs.iter().find(|tab| tab.id == active);
            let blank = tab.is_some_and(|tab| !tab.controller_created && tab.url.is_none());
            let private = tab.is_some_and(|tab| tab.private);
            (tabs, (active, blank, private))
        };
        let id = match crate::internal_page::open_plan(&tabs, current, reuse_blank) {
            OpenPlan::Existing(id) => {
                let _ = Self::select_tab(app, id);
                id
            }
            OpenPlan::Current(id) => id,
            OpenPlan::NewTab => Self::new_tab(app),
        };
        Self::show_internal_page(app, id, page);
        id
    }

    /// Puts tab `id` on `page`: address, title and favicon, and no engine page. Only for a tab
    /// without a content controller (a new, blank, discarded or settings tab).
    fn show_internal_page(app: &AppRef, id: TabId, page: crate::internal_page::InternalPage) {
        let url = page.url();
        let (hwnd, edit, active) = {
            let mut state = app.borrow_mut();
            let active = state.tab_model.active() == id;
            let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) else {
                return;
            };
            if tab.private || tab.controller_created {
                return;
            }
            tab.url = Some(url.clone());
            tab.title = page.title().to_owned();
            tab.favicon = Some(page.favicon().to_vec());
            tab.favicon_key = Some(crate::favicon::key_for(page.favicon()));
            tab.loading = false;
            tab.page_usable = false;
            tab.discarded = false;
            tab.can_back = false;
            tab.can_forward = false;
            tab.load = LoadState::Loaded;
            tab.blocked = 0;
            tab.permission_blocked = false;
            state.tab_model.set_url(id, Some(url.as_str()));
            state.tab_model.set_title(id, page.title());
            if active {
                state.navigation_progress.reset();
                state.page_loaded = false;
            }
            (state.hwnd, state.edit, active)
        };
        if active {
            win32::sync_address_input(hwnd, edit, id, &url, Self::settings(app).minimal_url);
            Self::sync_window_title(app);
        }
        Self::post_tab_changed(hwnd, id);
    }

    /// The settings page moved to another section: the tab's address follows (no title change).
    pub fn set_internal_section(app: &AppRef, id: TabId, page: crate::internal_page::InternalPage) {
        let url = page.url();
        let (hwnd, edit, active) = {
            let mut state = app.borrow_mut();
            let active = state.tab_model.active() == id;
            let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) else {
                return;
            };
            if !tab.url.as_ref().and_then(crate::internal_page::parse).is_some_and(|old| old.same_page(page)) || tab.url.as_ref() == Some(&url) {
                return;
            }
            tab.url = Some(url.clone());
            state.tab_model.set_url(id, Some(url.as_str()));
            (state.hwnd, state.edit, active)
        };
        if active {
            win32::sync_address_input(hwnd, edit, id, &url, Self::settings(app).minimal_url);
        }
        Self::post_tab_changed_kind(hwnd, id, false, true);
    }

    /// Settings saved on the settings page: sleeping tabs follow without a restart.
    pub fn apply_sleep_settings(app: &AppRef, enabled: bool, after_secs: u64) {
        {
            let mut state = app.borrow_mut();
            state.settings.sleep_enabled = enabled;
            state.settings.sleep_after_secs = after_secs.clamp(
                crate::settings::MIN_SLEEP_AFTER_SECS,
                crate::settings::MAX_SLEEP_AFTER_SECS,
            );
        }
        Self::schedule_tab_idle_timer(app);
    }

    /// A tab leaving an internal page for a web address loses the page's title and favicon.
    fn leave_internal_page(tab: &mut AppTab, tab_model: &mut crate::tabs::TabList) {
        if tab.url.as_ref().is_some_and(crate::internal_page::is_internal) {
            tab.title.clear();
            tab.favicon = None;
            tab.favicon_key = None;
            tab_model.set_title(tab.id, "");
        }
    }
    // --- end internal pages ---

    /// Re-applies the shown split's panes once after a side's controller became ready.
    pub fn refresh_split_panes(app: &AppRef) {
        let refresh = std::mem::take(&mut app.borrow_mut().split_refresh);
        if refresh && app.borrow().split_view.is_some() {
            Self::apply_visible_panes(app);
        }
    }

    /// Recently closed tabs, newest first.
    pub fn recently_closed(app: &AppRef) -> Vec<ClosedTab> {
        app.borrow().recently_closed.iter().cloned().collect()
    }

    /// Moves `id` directly after `anchor` in the current workspace, so a pair sits together.
    fn place_after(app: &AppRef, id: TabId, anchor: TabId) {
        let target = {
            let state = app.borrow();
            let order = state.workspaces.active().tabs();
            let (Some(from), Some(at)) = (
                order.iter().position(|tab| *tab == id),
                order.iter().position(|tab| *tab == anchor),
            ) else {
                return;
            };
            if from == at + 1 {
                return;
            }
            if from < at { at } else { at + 1 }
        };
        let _ = Self::move_tab(app, id, target);
    }

    pub fn split_view(app: &AppRef) -> Option<SplitView> {
        app.borrow().split_view
    }

    /// Opens a split and keeps both existing tab controllers alive and visible.
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
        let Some(mut split) = SplitView::new(left, right, focused) else {
            return false;
        };
        split.set_ratio(ratio);
        Self::show_split(app, split, Some(SplitBounds::pair(bounds.0, bounds.1)))
    }

    /// Shows `split` (replacing any other pair of this window) with its focused side active.
    fn show_split(app: &AppRef, split: SplitView, bounds: Option<SplitBounds>) -> bool {
        let (focused_tab, hwnd) = {
            let mut state = app.borrow_mut();
            if split.tabs().any(|tab| state.tab_model.get(tab).is_none()) {
                return false;
            }
            state.split_view = Some(split);
            state.split_parked = None;
            if bounds.is_some() {
                state.split_bounds = bounds;
            }
            let focused_tab = split.focused_tab();
            state.tab_model.select(focused_tab);
            let _ = state.workspaces.select_tab(focused_tab);
            for tab in &mut state.tabs {
                if split.contains(tab.id) {
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
        for tab in split.tabs() {
            Self::restore_discarded_tab(app, tab);
        }
        Self::post_tab_changed(hwnd, focused_tab);
        Self::save_workspaces(app, WorkspaceChange::TabSelect);
        Self::sync_window_title(app);
        Self::schedule_tab_idle_timer(app);
        true
    }

    /// Opens a pair of `left` and `right` placed next to each other in the strip. `bounds` are the
    /// pane rectangles, or `None` when the window is too narrow for two panes (then only the focused
    /// side shows until it is wide enough).
    pub fn open_split_pair(
        app: &AppRef,
        left: TabId,
        right: TabId,
        focused: Pane,
        bounds: Option<SplitBounds>,
    ) -> bool {
        let Some(split) = SplitView::new(left, right, focused) else {
            return false;
        };
        Self::open_split_arranged(app, split, bounds)
    }

    /// Opens `split` as given (tabs, focus, ratio, orientation), placing its second tab directly
    /// after its first in the strip.
    pub fn open_split_arranged(app: &AppRef, split: SplitView, bounds: Option<SplitBounds>) -> bool {
        if split.tabs().any(|tab| app.borrow().tab_model.get(tab).is_none()) {
            return false;
        }
        Self::place_layout_together(app, split);
        Self::show_split(app, split, bounds)
    }

    /// Moves the layout's tabs next to each other in the strip, in tree order (a pair: the second
    /// tab directly after the first). Pinned tabs live in their own section; only the display joins
    /// a tab whose pin state differs from the first tab's.
    fn place_layout_together(app: &AppRef, split: SplitView) {
        let first = split.left;
        let pinned = Self::tab_pinned(app, first);
        let mut anchor = first;
        for tab in split.tabs().skip(1) {
            if Self::tab_pinned(app, tab) == pinned {
                Self::place_after(app, tab, anchor);
                anchor = tab;
            }
        }
    }

    /// Splits `tab` with a new empty side on `side` (focused) that offers the other tabs.
    pub fn open_split_with_empty(
        app: &AppRef,
        tab: TabId,
        side: Pane,
        bounds: Option<SplitBounds>,
    ) -> Option<TabId> {
        Self::open_split_with_empty_oriented(app, tab, side, crate::split_view::Orientation::SideBySide, bounds)
    }

    /// [`Self::open_split_with_empty`] side by side or stacked.
    pub fn open_split_with_empty_oriented(
        app: &AppRef,
        tab: TabId,
        side: Pane,
        orientation: crate::split_view::Orientation,
        bounds: Option<SplitBounds>,
    ) -> Option<TabId> {
        if app.borrow().tab_model.get(tab).is_none() {
            return None;
        }
        let empty = Self::new_tab(app);
        let (left, right) = match side {
            Pane::Left => (empty, tab),
            _ => (tab, empty),
        };
        let split = SplitView::new(left, right, side)?.with_orientation(orientation);
        Self::open_split_arranged(app, split, bounds).then_some(empty)
    }

    /// Carries out a tab drop on a page edge (`split_view::plan_drop`). A tab that leaves the old
    /// pair stays open, unless it was the pair's empty side (a tab with no page), which closes.
    pub fn apply_split_drop(app: &AppRef, plan: crate::split_view::DropPlan, bounds: Option<SplitBounds>) -> bool {
        match plan {
            crate::split_view::DropPlan::Pair { view, leaving } => {
                let leaving_blank = leaving.filter(|tab| Self::tab_is_blank(app, *tab));
                let shown = Self::open_split_arranged(app, view, bounds);
                if shown && let Some(tab) = leaving_blank {
                    let _ = Self::close_tab(app, tab);
                }
                shown
            }
            crate::split_view::DropPlan::WithEmpty { tab, zone } => {
                Self::open_split_with_empty_oriented(app, tab, zone.pane().other(), zone.orientation(), bounds).is_some()
            }
            crate::split_view::DropPlan::Layout { view, leaving } => {
                let leaving_blank: Vec<TabId> = leaving.into_iter().flatten().filter(|tab| Self::tab_is_blank(app, *tab)).collect();
                let shown = Self::open_split_arranged(app, view, bounds);
                if shown {
                    for tab in leaving_blank {
                        let _ = Self::close_tab(app, tab);
                    }
                }
                shown
            }
        }
    }

    /// Side by side becomes stacked and back ("Stack top and bottom" / "Show side by side").
    pub fn turn_split(app: &AppRef, bounds: Option<SplitBounds>) -> bool {
        let Some(mut split) = app.borrow().split_view else {
            return false;
        };
        split.turn();
        Self::show_split(app, split, bounds)
    }

    /// Opens `url` in a new tab beside `tab` ("Open link in split view").
    pub fn open_url_in_split(app: &AppRef, tab: TabId, url: &url::Url, bounds: Option<SplitBounds>) -> bool {
        let Some(side) = Self::open_split_with_empty(app, tab, Pane::Right, bounds) else {
            return false;
        };
        Self::navigate_tab(app, side, url);
        Self::apply_visible_panes(app);
        true
    }

    /// The shown pair's pane rectangles, when the window is wide enough for two panes.
    pub fn split_bounds(app: &AppRef) -> Option<SplitBounds> {
        let state = app.borrow();
        state.split_view.and(state.split_bounds)
    }

    /// The shown pair's empty side takes `picked`; the empty tab closes. `picked` must be an open
    /// tab outside the pair.
    pub fn fill_split_empty(app: &AppRef, empty: TabId, picked: TabId) -> bool {
        let split = {
            let state = app.borrow();
            let Some(mut split) = state.split_view else {
                return false;
            };
            let empty_is_blank = state
                .tabs
                .iter()
                .find(|tab| tab.id == empty)
                .is_some_and(|tab| !tab.controller_created && tab.url.is_none());
            if state.tab_model.get(picked).is_none()
                || !empty_is_blank
                || !split.fill_empty(empty, picked)
            {
                return false;
            }
            split
        };
        Self::place_layout_together(app, split);
        {
            // The empty tab is no longer in the pair, so closing it below keeps the split.
            let mut state = app.borrow_mut();
            state.split_view = Some(split);
            state.tab_model.select(picked);
        }
        let bounds = app.borrow().split_bounds;
        let shown = Self::show_split(app, split, bounds);
        let _ = Self::close_tab(app, empty);
        shown
    }

    /// "Separate views": both tabs stay open as normal tabs and the focused one stays shown.
    pub fn separate_split(app: &AppRef) -> bool {
        let (shown, hwnd) = {
            let mut state = app.borrow_mut();
            let Some(split) = state.split_view.take().or_else(|| state.split_parked.take()) else {
                return false;
            };
            state.split_bounds = None;
            let shown = state.tab_model.active();
            let now = Instant::now();
            for tab in state.tabs.iter_mut().filter(|tab| tab.id != shown && split.contains(tab.id)) {
                tab.hidden_since.get_or_insert(now);
            }
            (shown, state.hwnd)
        };
        Self::apply_visible_panes(app);
        Self::post_tab_changed(hwnd, shown);
        Self::sync_window_title(app);
        Self::schedule_tab_idle_timer(app);
        true
    }
    // --- end split view ---

    pub fn update_split_bounds(app: &AppRef, bounds: SplitBounds) {
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
        let mut state = app.borrow_mut();
        let Some(split) = state.split_view.as_mut() else {
            return false;
        };
        let old = split.ratio;
        split.set_ratio(ratio);
        old != split.ratio
    }

    /// The shared cross ratio of a divided layout (the dividers inside its halves).
    pub fn set_split_cross_ratio(app: &AppRef, ratio: u8) -> bool {
        let mut state = app.borrow_mut();
        let Some(split) = state.split_view.as_mut() else {
            return false;
        };
        let old = split.cross_ratio;
        split.set_cross_ratio(ratio);
        old != split.cross_ratio
    }

    /// Focuses the pane next to the focused one in `direction` (Ctrl+Shift+arrows).
    pub fn focus_split_direction(app: &AppRef, direction: crate::split_view::Direction) -> bool {
        let pane = app.borrow().split_view.and_then(|split| split.neighbour(direction));
        pane.is_some_and(|pane| Self::focus_split_pane(app, pane))
    }

    /// Swaps the two cells of divided half `half` (the divider inside it).
    pub fn swap_split_cross(app: &AppRef, half: usize) -> bool {
        let Some(mut split) = app.borrow().split_view else {
            return false;
        };
        if !split.swap_cross(half) {
            return false;
        }
        Self::place_layout_together(app, split);
        let bounds = app.borrow().split_bounds.map(|bounds| {
            let mut swapped = bounds;
            let lead = if half == 0 { Pane::Left } else { Pane::Right };
            swapped.set(lead, bounds.get(lead.sibling()));
            swapped.set(lead.sibling(), bounds.get(lead));
            swapped
        });
        Self::show_split(app, split, bounds)
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
        let (active, hwnd, changed) = {
            let mut state = app.borrow_mut();
            let Some(mut split) = state.split_view else {
                return false;
            };
            let changed = split.focused != pane;
            split.focus(pane);
            let active = split.focused_tab();
            state.split_view = Some(split);
            state.tab_model.select(active);
            let _ = state.workspaces.select_tab(active);
            if let Some((progress, page_usable)) = state
                .tabs
                .iter()
                .find(|tab| tab.id == active)
                .map(|tab| (tab.progress, tab.page_usable))
            {
                state.navigation_progress = progress;
                state.page_loaded = page_usable;
            }
            (active, state.hwnd, changed)
        };
        if changed {
            Self::apply_visible_panes(app);
            Self::post_tab_changed(hwnd, active);
            Self::sync_window_title(app);
        }
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
        // The strip shows the pair in model order, so the halves trade places there too.
        Self::place_layout_together(app, split);
        let bounds = app.borrow().split_bounds;
        Self::show_split(app, split, bounds)
    }

    pub fn replace_focused_split_pane(app: &AppRef, tab: TabId) -> bool {
        let Some(mut split) = app.borrow().split_view else {
            return false;
        };
        if !split.replace_focused(tab) {
            return false;
        }
        let bounds = app.borrow().split_bounds;
        Self::show_split(app, split, bounds)
    }

    pub fn close_split_pane(app: &AppRef, pane: Pane) -> bool {
        let split = { app.borrow().split_view };
        let Some(split) = split else { return false };
        let (closing, _survivor) = split.close(pane);
        Self::close_tab(app, closing)
    }

    pub fn reload(app: &AppRef) {
        // An internal page has no engine page: reloading it re-reads what it shows.
        if let Some(page) = Self::internal_page(app, Self::active_tab_id(app)) {
            let hwnd = app.borrow().hwnd;
            if page.is_history() {
                win32::history_page_refresh(hwnd);
            } else if page.is_downloads() {
                win32::downloads_page_refresh(hwnd);
            } else {
                win32::settings_page_refresh(hwnd);
            }
            return;
        }
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
        if Self::internal_page(app, Self::active_tab_id(app)).is_some() {
            return;
        }
        if let Some(Err(error)) = Self::with_engine(app, |engine| engine.stop()) {
            win32::show_error_message(&format!("Stopping navigation failed: {error}"));
        }
    }

    /// Moves keyboard focus into the active page. Used after the HTML toolbar submits or escapes.
    pub fn focus_page(app: &AppRef) {
        if let Some(page) = Self::internal_page(app, Self::active_tab_id(app)) {
            let hwnd = app.borrow().hwnd;
            if page.is_history() {
                win32::history_page_focus(hwnd);
            } else if page.is_downloads() {
                win32::downloads_page_focus(hwnd);
            } else {
                win32::settings_page_focus(hwnd);
            }
            return;
        }
        let _ = Self::with_engine(app, |engine| engine.focus_page());
    }

    /// Toggles the docked DevTools pane for the active tab's page. Internal pages (settings,
    /// history) have no engine page, so there is nothing to inspect and the request is ignored.
    /// F12 and Ctrl+Shift+I repeat while held, so this toggles rather than only ever opening —
    /// otherwise key-repeat would flap the dock open and shut (see `shortcuts::is_toggle`).
    pub fn open_dev_tools(app: &AppRef) {
        let tab = Self::active_tab_id(app);
        if Self::internal_page(app, tab).is_some() {
            return;
        }
        let hwnd = app.borrow().hwnd;
        if win32::dev_tools_dock_close_if_open(hwnd, tab) {
            return;
        }
        let before = win32::dev_tools_dock_snapshot();
        if let Some(Err(error)) = Self::with_engine(app, |engine| engine.open_dev_tools()) {
            win32::show_error_message(&format!("Developer tools could not be opened: {error}"));
            return;
        }
        win32::dev_tools_dock_discover(hwnd, tab, before);
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
        let tab = state.tabs.iter().find(|tab| tab.id == id);
        // The audio rule's own mute (Settings > Tabs > Mute tab audio) is not the user's: the engine
        // reports it as muted, the tab menu and the strip must not.
        tab.is_some_and(|tab| tab.muted)
            || (!tab.is_some_and(|tab| tab.extra.auto_muted)
                && state
                    .engine
                    .as_ref()
                    .is_some_and(|engine| engine.tab_muted(id)))
    }

    pub fn tab_supports_muting(app: &AppRef, id: TabId) -> bool {
        app.borrow()
            .engine
            .as_ref()
            .is_some_and(|engine| engine.supports_tab_muting(id))
    }

    pub fn set_tab_muted(app: &AppRef, id: TabId, muted: bool) {
        let effective = if let Some(tab) = app.borrow_mut().tabs.iter_mut().find(|tab| tab.id == id) {
            tab.muted = muted;
            muted || tab.extra.auto_muted // --- tab options ---
        } else {
            return;
        };
        if let Some(Err(error)) =
            Self::with_engine(app, |engine| engine.set_tab_muted(id, effective))
        {
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
            // --- tab options --- a pinned tab "minimised" instead of closed may sleep.
            && (tab.extra.allow_pinned_sleep
                || !state.tab_model.get(id).is_some_and(|model| model.pinned))
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
        if let Some(tab) = app.borrow_mut().tabs.iter_mut().find(|tab| tab.id == id) {
            tab.extra.allow_pinned_sleep = false; // --- tab options ---
        }
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
            tab.extra.hibernated = true; // --- tab options --- "dim icon when hibernated"
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

    /// A non-private tab with a live page, for profile-wide engine calls such as clearing browsing
    /// data: the active tab when it qualifies, else an awake one, else any. The settings page's own
    /// tab never has a page, so the Privacy dashboard drawn on it still reaches the profile this way.
    pub fn profile_page_tab(app: &AppRef) -> Option<TabId> {
        let state = app.borrow();
        let active = state.tab_model.active();
        state
            .tabs
            .iter()
            .filter(|tab| tab.controller_created && !tab.discarded && !tab.private)
            .min_by_key(|tab| (tab.id != active, tab.suspended))
            .map(|tab| tab.id)
    }

    /// Whether a permission request from the active tab's current page was denied.
    pub fn active_permission_blocked(app: &AppRef) -> bool {
        let state = app.borrow();
        let active = state.tab_model.active();
        state
            .tabs
            .iter()
            .find(|tab| tab.id == active)
            .is_some_and(|tab| tab.permission_blocked)
    }

    /// The newest denied permission requests and blocked popups, newest first (at most 50). Held in
    /// memory only; origin, kind and time, never page content.
    pub fn denied_requests(app: &AppRef) -> Vec<crate::permissions::DeniedAttempt> {
        app.borrow().denied.recent().cloned().collect()
    }

    pub fn clear_denied_requests(app: &AppRef) {
        app.borrow_mut().denied.clear();
    }

    /// The page zoom in percent of the active tab, 100 when it has none.
    pub fn active_zoom_percent(app: &AppRef) -> u16 {
        let state = app.borrow();
        let active = state.tab_model.active();
        state
            .tabs
            .iter()
            .find(|tab| tab.id == active)
            .map_or(crate::site_prefs::ZOOM_DEFAULT, |tab| tab.zoom)
    }

    /// Ctrl+Plus, Ctrl+Minus and Ctrl+0 on the active tab.
    pub fn zoom_active(app: &AppRef, step: crate::site_prefs::ZoomStep) {
        let tab = app.borrow().tab_model.active();
        let current = Self::call_engine(app, |engine| engine.zoom_percent(tab))
            .flatten()
            .unwrap_or_else(|| Self::active_zoom_percent(app));
        Self::set_tab_zoom(app, tab, step.apply(current));
    }

    /// Sets one tab's zoom and, when the tab shows a web page, stores it for that page's origin
    /// (guest sessions keep it in memory only). Returns whether the engine applied it.
    pub fn set_tab_zoom(app: &AppRef, tab: TabId, percent: u16) -> bool {
        let percent = percent.clamp(crate::site_prefs::ZOOM_MIN, crate::site_prefs::ZOOM_MAX);
        let applied = Self::call_engine(app, |engine| engine.set_zoom_percent(tab, percent))
            .and_then(Result::ok)
            .unwrap_or(false);
        if applied {
            Self::note_tab_zoom(app, tab, percent, true);
        }
        applied
    }

    /// Records a tab's zoom, optionally stores it for the page's origin, and refreshes the toolbar.
    fn note_tab_zoom(app: &AppRef, tab: TabId, percent: u16, persist: bool) {
        let (url, store, hwnd, scope) = {
            let mut state = app.borrow_mut();
            let store = state.site_prefs.clone();
            let hwnd = state.hwnd;
            let scope = state.settings.page_tools.zoom_scope;
            let Some(entry) = state.tabs.iter_mut().find(|entry| entry.id == tab) else {
                return;
            };
            entry.zoom = percent;
            if persist {
                entry.zoom_set = true; // the user's own change (page tools: tab zoom keeps it)
            }
            (entry.url.clone(), store, hwnd, scope)
        };
        // A private tab's zoom lives in the tab only: nothing is written to the site store. Neither
        // does a zoom kept per tab (Settings > Page tools > Page zoom).
        let persist = persist && !Self::tab_private(app, tab) && crate::page_tools::zoom_is_stored(scope);
        if persist
            && let (Some(store), Some(origin)) = (
                store,
                url.as_ref()
                    .and_then(|url| crate::permissions::origin_of(url.as_str())),
            )
        {
            // A full or unwritable store leaves the zoom applied for this tab only.
            let _ = store.borrow_mut().set_zoom(&origin, Some(percent));
        }
        Self::post_tab_changed_kind(hwnd, tab, false, false);
    }

    /// Navigation commit: shows the page at its site's stored zoom (100 for a site with none).
    /// Without a store (benchmark launches) the tab keeps whatever zoom it has.
    fn apply_stored_zoom(app: &AppRef, tab: TabId, url: &url::Url) {
        if Self::tab_private(app, tab) {
            // A private tab neither reads nor writes stored zoom; it keeps its own level.
            return;
        }
        let (store, current, default_zoom, scope, tab_set) = {
            let state = app.borrow();
            let Some(entry) = state.tabs.iter().find(|entry| entry.id == tab) else {
                return;
            };
            (state.site_prefs.clone(), entry.zoom, state.settings.default_zoom, state.settings.page_tools.zoom_scope, entry.zoom_set)
        };
        let Some(store) = store else {
            return;
        };
        let stored = store.borrow().zoom_for_uri(url.as_str(), default_zoom);
        let Some(target) = crate::page_tools::zoom_on_commit(scope, tab_set, current, stored, default_zoom) else {
            return;
        };
        let applied = Self::call_engine(app, |engine| engine.set_zoom_percent(tab, target))
            .and_then(Result::ok)
            .unwrap_or(false);
        if applied {
            Self::note_tab_zoom(app, tab, target, false);
        }
    }

    /// The engine reports a zoom change. The ones this app made are already recorded (equal to the
    /// tab's value) and ignored; what is left is the page's own zoom, such as Ctrl+wheel, which is
    /// stored for the site like a key press.
    fn page_zoom_changed(app: &AppRef, tab: TabId, percent: u16) {
        let differs = app.try_borrow().is_ok_and(|state| {
            state
                .tabs
                .iter()
                .find(|entry| entry.id == tab)
                .is_some_and(|entry| entry.zoom != percent)
        });
        if differs {
            Self::note_tab_zoom(app, tab, percent, true);
        }
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

    /// The tracking-prevention level chosen in Settings, which the next start will apply.
    pub fn tracking_setting(app: &AppRef) -> &'static str {
        app.borrow().settings.tracking_prevention.key()
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

    // --- downloads (docs/design/downloads.md; the model is `crate::downloads`) ---

    fn download_event_hook(app: &AppRef) -> Rc<dyn Fn(crate::downloads::DownloadEvent)> {
        let weak = Rc::downgrade(app);
        Rc::new(move |event| {
            if let Some(app) = weak.upgrade() {
                Self::on_download_event(&app, event);
            }
        })
    }

    #[allow(clippy::type_complexity)]
    fn download_plan_hook(
        app: &AppRef,
    ) -> Rc<dyn Fn(&crate::downloads::DownloadRequest) -> crate::downloads::DownloadPlan> {
        let weak = Rc::downgrade(app);
        Rc::new(move |request| match weak.upgrade() {
            Some(app) => Self::download_plan(&app, request),
            None => crate::downloads::DownloadPlan::Cancel,
        })
    }

    /// The event and plan hooks a web app panel's downloads report through (tabs get theirs in
    /// `start_engine_creation`).
    #[allow(clippy::type_complexity)]
    pub fn download_hooks(
        app: &AppRef,
    ) -> (
        Rc<dyn Fn(crate::downloads::DownloadEvent)>,
        Rc<dyn Fn(&crate::downloads::DownloadRequest) -> crate::downloads::DownloadPlan>,
    ) {
        (Self::download_event_hook(app), Self::download_plan_hook(app))
    }

    fn unix_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as u64)
    }

    fn downloads_time() -> crate::downloads::Time {
        crate::downloads::Time {
            now: Self::unix_ms() / 1000,
            offset_secs: win32::history_local_offset(),
        }
    }

    /// Where a download goes: the folder setting (or the Downloads folder when it is empty or
    /// unusable) with a name that clashes with nothing, or the Save dialog when the user asked to
    /// be asked. Runs inside WebView2's `DownloadStarting`, so it never waits on anything.
    fn download_plan(
        app: &AppRef,
        request: &crate::downloads::DownloadRequest,
    ) -> crate::downloads::DownloadPlan {
        use crate::downloads::{DownloadPlan, plan_destination, resolve_dir};
        let Ok(state) = app.try_borrow() else {
            return DownloadPlan::Cancel;
        };
        let default_dir = crate::download_shell::default_downloads_dir();
        let Ok(dir) = resolve_dir(&state.settings.download_dir, default_dir.as_deref()) else {
            return DownloadPlan::Cancel;
        };
        plan_destination(
            &dir,
            state.settings.ask_download_location,
            &request.suggested_name,
            // A file on disk, or the destination of a download still running (WebView2 may not
            // have created it yet).
            &|path| path.exists() || state.downloads.path_in_use(path),
        )
    }

    /// An engine download event reaches the list. The observer (set by the window) hears of the
    /// change afterwards, with the App unborrowed; operations the list let go of are released.
    pub fn on_download_event(app: &AppRef, event: crate::downloads::DownloadEvent) {
        let (change, released, observer) = {
            let Ok(mut state) = app.try_borrow_mut() else {
                return;
            };
            let change = state.downloads.on_event(event, Self::unix_ms());
            (change, state.downloads.take_released(), state.downloads_observer.clone())
        };
        if !released.is_empty() {
            engine::download::release(&released);
        }
        if change != crate::downloads::Change::None
            && let Some(observer) = observer
        {
            observer(change);
        }
    }

    /// Runs `f` on the list and, when it changed anything, tells the observer. `None` if the App
    /// is borrowed (a re-entrant call).
    fn downloads_apply<R>(
        app: &AppRef,
        f: impl FnOnce(&mut crate::downloads::DownloadManager) -> R,
    ) -> Option<R> {
        let (result, changed, released, observer) = {
            let mut state = app.try_borrow_mut().ok()?;
            let before = state.downloads.revision();
            let result = f(&mut state.downloads);
            (
                result,
                state.downloads.revision() != before,
                state.downloads.take_released(),
                state.downloads_observer.clone(),
            )
        };
        if !released.is_empty() {
            engine::download::release(&released);
        }
        if changed && let Some(observer) = observer {
            observer(crate::downloads::Change::State);
        }
        Some(result)
    }

    /// Registers (or clears) the function told after every change to the downloads list: `Progress`
    /// while bytes move (already about four a second), `State` for everything else. It runs with
    /// the App unborrowed; post a message to the window and read the snapshots there.
    pub fn set_downloads_observer(
        app: &AppRef,
        observer: Option<Rc<dyn Fn(crate::downloads::Change)>>,
    ) {
        app.borrow_mut().downloads_observer = observer;
    }

    /// Grows with every change to the list: a cheap "did anything change" poll.
    pub fn downloads_revision(app: &AppRef) -> u64 {
        app.borrow().downloads.revision()
    }

    pub fn downloads_list_state(app: &AppRef) -> crate::downloads::ListState {
        app.borrow().downloads.list_state()
    }

    /// The toolbar button's state (`{v:1,t:"dlbtn",..}` through `Toolbar::json`).
    pub fn downloads_toolbar(app: &AppRef) -> crate::downloads::Toolbar {
        app.borrow().downloads.snapshot_for_toolbar()
    }

    /// The bubble's message for popup token `tok` (`{v:1,t:"downloads",..}`): up to five rows.
    pub fn downloads_bubble_json(app: &AppRef, tok: u32, auto: bool) -> String {
        Self::downloads_check_files(app);
        app.borrow().downloads.snapshot_for_bubble(
            tok,
            auto,
            crate::downloads::BUBBLE_ROWS,
            Self::downloads_time(),
        )
    }

    /// The page's snapshot for `query` (`{v:1,t:"snap",p:"downloadspage",..}`); `ack` answers its
    /// last change.
    pub fn downloads_page_json(app: &AppRef, query: &str, ack: crate::downloads::Ack) -> String {
        Self::downloads_check_files(app);
        app.borrow().downloads.snapshot_for_page(query, ack, Self::downloads_time())
    }

    /// The bubble opened or closed: finished downloads stop asking for attention.
    pub fn downloads_mark_seen(app: &AppRef) -> bool {
        Self::downloads_apply(app, |downloads| downloads.mark_seen()).unwrap_or(false)
    }

    /// Re-checks finished downloads against the disk (at most every two seconds), so a deleted
    /// file reads `missing`. True when a row changed.
    pub fn downloads_check_files(app: &AppRef) -> bool {
        let now = Self::unix_ms();
        Self::downloads_apply(app, |downloads| {
            downloads.refresh_missing_if_stale(now, &crate::download_shell::regular_file)
        })
        .unwrap_or(false)
    }

    pub fn downloads_can_undo(app: &AppRef) -> bool {
        app.borrow().downloads.can_undo()
    }

    /// Puts the last removal back. True if anything came back.
    pub fn downloads_undo(app: &AppRef) -> bool {
        Self::downloads_apply(app, |downloads| downloads.undo()).is_some_and(|count| count > 0)
    }

    /// The folder downloads are saved to now, created if need be.
    fn download_dir(app: &AppRef) -> Option<PathBuf> {
        let setting = app.borrow().settings.download_dir.clone();
        crate::downloads::resolve_dir(&setting, crate::download_shell::default_downloads_dir().as_deref()).ok()
    }

    /// The download folder as the settings page shows it (the setting, else the Downloads folder).
    /// Not created.
    pub fn download_dir_display(app: &AppRef) -> String {
        let setting = app.borrow().settings.download_dir.clone();
        if !setting.is_empty() {
            return setting;
        }
        crate::download_shell::default_downloads_dir()
            .map(|dir| dir.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Settings > Downloads > Change: the native folder picker over `owner`, then the choice is
    /// saved. False when cancelled or the folder is not usable as a setting.
    pub fn pick_download_folder(app: &AppRef, owner: HWND) -> bool {
        let start = Self::download_dir(app);
        let Some(chosen) = crate::download_shell::pick_folder(owner, start.as_deref()) else {
            return false;
        };
        let Some(dir) = crate::settings::parse_download_dir(&chosen.to_string_lossy()) else {
            return false;
        };
        app.borrow_mut().settings.download_dir = dir;
        Self::save_settings(app);
        true
    }

    /// Settings > Translation: offer to translate pages in other languages.
    pub fn set_translate_offer(app: &AppRef, on: bool) {
        app.borrow_mut().settings.translate_offer = on;
        Self::save_settings(app);
    }

    /// Settings > Translation: translate sites set to "always translate" without asking.
    pub fn set_translate_auto(app: &AppRef, on: bool) {
        app.borrow_mut().settings.translate_auto = on;
        Self::save_settings(app);
    }

    /// Settings > Translation: the language pages are translated into (empty: the display language).
    pub fn set_translate_target(app: &AppRef, target: &str) {
        let Some(target) = crate::settings::parse_translate_target(target) else {
            return;
        };
        app.borrow_mut().settings.translate_target = target;
        Self::save_settings(app);
    }

    /// Settings > Downloads > "Ask where to save each file".
    pub fn set_ask_download_location(app: &AppRef, on: bool) {
        app.borrow_mut().settings.ask_download_location = on;
        Self::save_settings(app);
    }

    /// Performs a bubble or page action for entry `id` (0 for the list-level ones): the list
    /// checks the id and that the action is legal for the entry's state; engine pause, resume and
    /// cancel, opening the file, showing it in Explorer and opening the folder happen here, on the
    /// recorded path only. What needs the window (open the page, a new tab, the clipboard) comes back
    /// as the [`DownloadFollowup`].
    pub fn download_act(app: &AppRef, id: u32, action: crate::downloads::Action) -> DownloadFollowup {
        use crate::downloads::{EngineCommand, Outcome};
        let Some(outcome) = Self::downloads_apply(app, |downloads| downloads.act(id, action)) else {
            return DownloadFollowup::Failed;
        };
        let shell_result = |done: bool| {
            if done {
                DownloadFollowup::None
            } else {
                // The file is gone or the shell refused: let the row say so.
                let _ = Self::downloads_apply(app, |downloads| {
                    downloads.refresh_missing(&crate::download_shell::regular_file)
                });
                DownloadFollowup::Failed
            }
        };
        match outcome {
            Outcome::Rejected => DownloadFollowup::Rejected,
            Outcome::Changed => DownloadFollowup::None,
            Outcome::Engine { key, command } => {
                let done = match command {
                    EngineCommand::Pause => engine::pause_download(key),
                    EngineCommand::Resume => engine::resume_download(key),
                    EngineCommand::Cancel => engine::cancel_download(key),
                };
                if done { DownloadFollowup::None } else { DownloadFollowup::Failed }
            }
            Outcome::OpenFile(path) => shell_result(crate::download_shell::open_file(&path)),
            Outcome::ShowInFolder(path) => shell_result(crate::download_shell::show_in_folder(&path)),
            Outcome::OpenFolder => match Self::download_dir(app) {
                Some(dir) if crate::download_shell::open_folder(&dir) => DownloadFollowup::None,
                _ => DownloadFollowup::Failed,
            },
            Outcome::OpenPage => DownloadFollowup::OpenPage,
            Outcome::CopyLink(url) => crate::downloads::copy_link_text(&url)
                .map_or(DownloadFollowup::Rejected, DownloadFollowup::CopyText),
            Outcome::Retry(url) => DownloadFollowup::OpenTab(url),
        }
    }

    /// The window is closing: downloads still running are cancelled and kept as failed
    /// `Interrupted`, and the list is saved. Call after the engine has closed.
    pub fn downloads_shutdown(app: &AppRef) {
        let keys = {
            let Ok(mut state) = app.try_borrow_mut() else {
                return;
            };
            let keys = state.downloads.shutdown(Self::unix_ms());
            state.downloads_observer = None;
            keys
        };
        engine::download::release(&keys);
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

    // --- reader view ---
    /// Runs one reader script in `tab`. `done` always runs with the app unborrowed: an engine that
    /// answers at once (no live page) is answered after `call_engine` returns.
    fn run_reader_script(
        app: &AppRef,
        tab: TabId,
        script: String,
        done: Box<dyn FnOnce(Option<String>)>,
    ) {
        let done = Rc::new(RefCell::new(Some(done)));
        let early = Rc::new(RefCell::new(None::<Option<String>>));
        let in_call = Rc::new(std::cell::Cell::new(true));
        let answer = {
            let (done, early, in_call) = (done.clone(), early.clone(), in_call.clone());
            Box::new(move |text: Option<String>| {
                if in_call.get() {
                    *early.borrow_mut() = Some(text);
                } else if let Some(done) = done.borrow_mut().take() {
                    done(text);
                }
            }) as Box<dyn FnOnce(Option<String>)>
        };
        let called = Self::call_engine(app, |engine| engine.run_reader_script(tab, &script, answer));
        in_call.set(false);
        let text = match (called, early.borrow_mut().take()) {
            (None, _) => Some(None),
            (Some(()), early) => early,
        };
        if let Some(text) = text {
            let done = done.borrow_mut().take();
            if let Some(done) = done {
                done(text);
            }
        }
    }

    /// The script call for `action` with the session's reader settings and Bowser's current colours.
    fn reader_invocation(app: &AppRef, action: crate::reader::Action) -> String {
        let (prefs, typography) = {
            let state = app.borrow();
            (state.reader_prefs, state.settings.page_tools.reader())
        };
        let appearance = win32::glass::appearance();
        let palette = crate::theme::palette(appearance, true);
        crate::reader::invocation_with(
            action,
            prefs,
            typography,
            appearance == crate::theme::Appearance::Dark,
            &crate::reader::Colours::from_palette(palette),
        )
    }

    /// The tab's reader state, if the tab exists and shows a page reader view may script.
    fn reader_state(app: &AppRef, tab: TabId) -> Option<crate::reader::TabState> {
        let state = app.borrow();
        let tab = state.tabs.iter().find(|candidate| candidate.id == tab)?;
        tab.url
            .as_ref()
            .is_some_and(crate::reader::url_allowed)
            .then_some(tab.reader)
    }

    /// Marks a call as in flight. `false` when the tab is gone or already has one.
    fn begin_reader_call(app: &AppRef, tab: TabId) -> bool {
        let mut state = app.borrow_mut();
        match state.tabs.iter_mut().find(|candidate| candidate.id == tab) {
            Some(tab) if !tab.reader.busy => {
                tab.reader.busy = true;
                true
            }
            _ => false,
        }
    }

    /// Folds a script answer into the tab. An answer for an earlier document (`epoch` moved on) is
    /// dropped. Returns what the user should be told.
    fn finish_reader_call(
        app: &AppRef,
        tab: TabId,
        epoch: u32,
        outcome: Option<crate::reader::Outcome>,
        probe: bool,
    ) -> ReaderResult {
        use crate::reader::Code;
        let (hwnd, result, changed) = {
            let mut state = app.borrow_mut();
            let hwnd = state.hwnd;
            if let Some(outcome) = outcome.filter(|outcome| outcome.code != Code::Failed) {
                state.reader_prefs = outcome.prefs;
            }
            let Some(entry) = state.tabs.iter_mut().find(|candidate| candidate.id == tab) else {
                return ReaderResult::Unavailable;
            };
            if entry.reader.epoch != epoch {
                return ReaderResult::Unavailable;
            }
            let before = (entry.reader.pill(), entry.translate.page_lang);
            entry.reader.busy = false;
            let result = match outcome {
                None => ReaderResult::Failed,
                Some(outcome) => {
                    if outcome.code != Code::Failed {
                        // The page's language is only used to offer translation.
                        entry.translate.page_lang = outcome.lang;
                    }
                    if probe {
                        entry.reader.apply_probe(&outcome);
                    } else {
                        entry.reader.apply_toggle(&outcome);
                    }
                    match outcome.code {
                        Code::Ok => ReaderResult::Applied(outcome.state),
                        Code::None => ReaderResult::NothingToRead,
                        Code::Failed => ReaderResult::Failed,
                    }
                }
            };
            (hwnd, result, before != (entry.reader.pill(), entry.translate.page_lang))
        };
        if changed {
            Self::post_tab_changed_kind(hwnd, tab, false, false);
        }
        if probe && changed && result != ReaderResult::Failed {
            // A site set to "always translate" starts now, when the automatic setting is on.
            Self::translate_after_probe(app, tab);
        }
        result
    }

    /// Turns reader view or distraction hiding on or off for `tab`, on the user's request. `done`
    /// says what happened so the caller can tell the user when nothing could be done.
    pub fn reader_toggle(
        app: &AppRef,
        tab: TabId,
        mode: crate::reader::Mode,
        done: Box<dyn FnOnce(ReaderResult)>,
    ) {
        if Self::reader_state(app, tab).is_none() || !Self::begin_reader_call(app, tab) {
            done(ReaderResult::Unavailable);
            return;
        }
        let epoch = Self::reader_state(app, tab).map_or(0, |state| state.epoch);
        let script = Self::reader_invocation(app, crate::reader::Action::Toggle(mode));
        let weak = Rc::downgrade(app);
        Self::run_reader_script(
            app,
            tab,
            script,
            Box::new(move |text| {
                let Some(app) = weak.upgrade() else { return };
                let outcome = text.as_deref().and_then(crate::reader::parse_result);
                done(Self::finish_reader_call(&app, tab, epoch, outcome, false));
            }),
        );
    }

    /// Read-only: asks the page how article-like it is, so the toolbar can offer reader view. The
    /// only automatic reader work, run once a page has finished loading in the active tab.
    pub fn reader_probe(app: &AppRef, tab: TabId) {
        if Self::reader_state(app, tab).is_none() || !Self::begin_reader_call(app, tab) {
            return;
        }
        let epoch = Self::reader_state(app, tab).map_or(0, |state| state.epoch);
        let script = Self::reader_invocation(app, crate::reader::Action::Probe);
        let weak = Rc::downgrade(app);
        Self::run_reader_script(
            app,
            tab,
            script,
            Box::new(move |text| {
                let Some(app) = weak.upgrade() else { return };
                let outcome = text.as_deref().and_then(crate::reader::parse_result);
                Self::finish_reader_call(&app, tab, epoch, outcome, true);
            }),
        );
    }

    /// Probes a tab that finished loading while it was in the background, once it is shown.
    fn reader_probe_if_needed(app: &AppRef, tab: TabId) {
        let needed = {
            let state = app.borrow();
            state.tabs.iter().find(|candidate| candidate.id == tab).is_some_and(|tab| {
                tab.page_usable && tab.controller_created && !tab.discarded && !tab.reader.probed
            })
        };
        if needed {
            Self::schedule_reader_probe(app, tab);
        }
    }

    /// Defers the probe so it does not compete with the page's first paint. Remembers the tab and
    /// its reader epoch; scheduling again replaces an earlier pending probe.
    fn schedule_reader_probe(app: &AppRef, tab: TabId) {
        let hwnd = {
            let mut state = app.borrow_mut();
            let Some(epoch) = state
                .tabs
                .iter()
                .find(|candidate| candidate.id == tab)
                .map(|candidate| candidate.reader.epoch)
            else {
                return;
            };
            state.reader_probe_pending = Some((tab, epoch));
            state.hwnd
        };
        if hwnd.is_invalid() {
            return;
        }
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetTimer(
                Some(hwnd),
                win32::WM_READER_PROBE_TIMER,
                READER_PROBE_DELAY_MS,
                None,
            );
        }
    }

    /// The reader probe timer fired: probes the pending tab if it is still due.
    pub fn reader_probe_timer_fired(app: &AppRef) {
        let due_tab = {
            let mut state = app.borrow_mut();
            let pending = state.reader_probe_pending.take();
            pending.and_then(|(tab, _)| {
                let entry = state.tabs.iter().find(|candidate| candidate.id == tab)?;
                reader_probe_due(
                    pending,
                    state.tab_model.active(),
                    entry.page_usable,
                    entry.controller_created,
                    entry.discarded,
                    entry.reader.probed,
                    entry.reader.epoch,
                )
                .then_some(tab)
            })
        };
        if let Some(tab) = due_tab {
            Self::reader_probe(app, tab);
        }
    }

    /// Leaves reader view or distraction hiding in a tab. The state is cleared at once; the page is
    /// asked to restore itself (a no-op on a document that has already gone).
    fn reader_leave(app: &AppRef, tab: TabId) {
        let Some(state) = Self::reader_state(app, tab) else {
            return;
        };
        if state.active == crate::reader::Active::Off {
            return;
        }
        {
            let mut app = app.borrow_mut();
            let Some(entry) = app.tabs.iter_mut().find(|candidate| candidate.id == tab) else {
                return;
            };
            entry.reader.active = crate::reader::Active::Off;
        }
        Self::post_tab_changed_kind(app.borrow().hwnd, tab, false, false);
        Self::reader_run_exit(app, tab);
    }

    /// Asks a tab's page to put itself back as it was. Safe on a page that is not in reader view,
    /// and on a document that has replaced the one that was (it then does nothing).
    fn reader_run_exit(app: &AppRef, tab: TabId) {
        let epoch = {
            let state = app.borrow();
            match state.tabs.iter().find(|candidate| candidate.id == tab) {
                Some(entry) => entry.reader.epoch,
                None => return,
            }
        };
        let script = Self::reader_invocation(app, crate::reader::Action::Exit);
        let weak = Rc::downgrade(app);
        Self::run_reader_script(
            app,
            tab,
            script,
            Box::new(move |text| {
                let Some(app) = weak.upgrade() else { return };
                if let Some(outcome) = text.as_deref().and_then(crate::reader::parse_result)
                    && outcome.code != crate::reader::Code::Failed
                    && let Ok(mut state) = app.try_borrow_mut()
                    && state
                        .tabs
                        .iter()
                        .any(|entry| entry.id == tab && entry.reader.epoch == epoch)
                {
                    state.reader_prefs = outcome.prefs;
                }
            }),
        );
    }

    /// Escape went down in a tab's page: leave reader view if it is on.
    pub fn reader_escape(app: &AppRef, tab: TabId) {
        let reading = app.try_borrow().is_ok_and(|state| {
            state
                .tabs
                .iter()
                .find(|candidate| candidate.id == tab)
                .is_some_and(|tab| tab.reader.active == crate::reader::Active::Reader)
        });
        if reading {
            Self::reader_leave(app, tab);
        }
    }

    /// The toolbar button for the active tab.
    pub fn active_reader_pill(app: &AppRef) -> crate::reader::Pill {
        let state = app.borrow();
        let active = state.tab_model.active();
        state
            .tabs
            .iter()
            .find(|tab| tab.id == active)
            .filter(|tab| tab.url.as_ref().is_some_and(crate::reader::url_allowed))
            .map_or(crate::reader::Pill::Hidden, |tab| tab.reader.pill())
    }

    /// What reader view and distraction hiding show for `tab` right now (for menu checkmarks).
    pub fn reader_active(app: &AppRef, tab: TabId) -> crate::reader::Active {
        Self::reader_state(app, tab).map_or(crate::reader::Active::Off, |state| state.active)
    }

    /// Whether the command can run on `tab` at all (an http(s) page).
    pub fn reader_available(app: &AppRef, tab: TabId) -> bool {
        Self::reader_state(app, tab).is_some()
    }
    // --- end reader view ---

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

    /// Whether the tab's page is asleep to save memory: discarded or suspended. Shown on the hover card.
    pub fn tab_sleeping(app: &AppRef, id: TabId) -> bool {
        app.borrow()
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .is_some_and(|tab| tab.discarded || tab.suspended)
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
        // Address bar options read wherever an address is shown or completed.
        crate::omnibox::set_show_full_address(settings.address.show_full);
        crate::omnibox::set_autocomplete(settings.address.autocomplete);
        // --- tab stacks (T10) --- the split view's tile look, read by its geometry.
        crate::stacks::set_tile_look(settings.tabs.tile_titles, settings.tabs.tile_tight);
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
        let allow_private_tabs = options.benchmark_url.is_none();
        let mut app = Self::from_launch_options(
            options,
            user_data_dir,
            provider,
            log,
            Some(profile.tab_groups_file()),
        );
        app.settings_synced = settings.clone();
        app.settings = settings;
        // Retention and the website look are read by code that runs before any window setting is
        // applied (the history load, the first tab), so they are set here (thread T3).
        crate::history_retention::set_current(app.settings.history_retention);
        crate::page_tools::apply_runtime(&app.settings.page_tools); // --- page tools (T7) ---
        app.reader_prefs = app.settings.page_tools.reader_prefs();
        crate::website_look::set(crate::website_look::WebsiteLook::from_settings(&app.settings));
        crate::layout_prefs::set_status_overlay(app.settings.status_bar); // --- layout (thread T6) ---
        theme_features::install_launch_theme(&app.settings); // --- theme options ---
        crate::window_prefs::publish(&app.settings.window); // --- window and panels (thread T9) ---
        if track_privacy {
            // A guest's list lives in memory only; so does one the profile folder cannot hold.
            app.downloads = if profile.is_guest() {
                crate::downloads::DownloadManager::in_memory(true)
            } else {
                crate::downloads::DownloadManager::open(&profile.downloads_file())
            };
        }
        if track_privacy {
            // Always built, even with blocking off: cookie-notice hiding and per-site levels work
            // without the global switch (thread T3). A "none" level adds no request work.
            let mut state = crate::blocker::BlockerState::load(
                &profile.blocker_dir(),
                &app.settings.blocker_allow,
            );
            state.apply_settings(&app.settings);
            app.blocker = Some(Rc::new(RefCell::new(state)));
        }
        if allow_private_tabs {
            app.private_blocker = Some(app.blocker.clone().unwrap_or_else(|| {
                Rc::new(RefCell::new(crate::blocker::BlockerState::matcher_only(
                    &profile.blocker_dir(),
                )))
            }));
        }
        if track_privacy {
            app.permissions = Some(Rc::new(RefCell::new(
                crate::permissions::PermissionStore::load(&profile.permissions_file()),
            )));
            // Guests remember zoom for the session only; nothing of theirs reaches the disk.
            app.site_prefs = Some(Rc::new(RefCell::new(if profile.is_guest() {
                crate::site_prefs::SitePrefsStore::in_memory()
            } else {
                crate::site_prefs::SitePrefsStore::load(&profile.site_prefs_file())
            })));
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
        let more_initial_urls = options.more_urls; // --- tab stacks (T10) ---
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
            more_initial_urls,
            benchmark_tracking_prevention: options.benchmark_tracking_prevention,
            benchmark_memory_pressure: options.benchmark_memory_pressure,
            provider,
            search_config_stamp: None,
            log,
            minimized: false,
            efficiency: EfficiencyState::default(),
            desired_memory_target: MemoryUsageTarget::Normal,
            effective_memory_target: None,
            memory_target_unsupported_logged: false,
            last_valid_viewport_bounds: None,
            has_navigated: false,
            page_loaded: false,
            navigation_progress: NavigationProgress::default(),
            tabs: vec![AppTab::empty(initial_tab)],
            tab_model,
            tab_mru: Vec::new(),
            reader_probe_pending: None,
            feed_pages: HashMap::new(),
            split_view: None,
            split_parked: None,
            recently_closed: std::collections::VecDeque::new(),
            tab_state: crate::tab_state::TabState::default(), // --- tab options ---
            split_refresh: false,
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
            workspaces_dirty: false,
            settings: crate::settings::Settings::default(),
            settings_synced: crate::settings::Settings::default(),
            session: None,
            session_save_pending: false,
            last_session_snapshot: None,
            pending_restore: None,
            privacy_stats: None,
            privacy_stats_path: None,
            privacy_session_blocked: 0,
            blocker: None,
            private_blocker: None,
            permissions: None,
            site_prefs: None,
            denied: crate::permissions::DeniedLog::default(),
            reader_prefs: crate::reader::Prefs::default(),
            translate: page_translate::Runtime::default(),
            downloads: crate::downloads::DownloadManager::in_memory(false),
            downloads_observer: None,
            // --- tab stacks (T10) --- a guest window keeps no rules.
            workspace_rules: crate::profile::active()
                .ok()
                .filter(|profile| !profile.is_guest())
                .map(|profile| crate::workspace_rules::load(profile.root()))
                .unwrap_or_default(),
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
        // --- start page --- "continue where you left off": a clean exit's tabs come back too, unless the
        // launch carries an address of its own.
        let restore_clean = self.settings.start.startup == crate::start_page::Startup::LastSession && self.initial_url.is_none();
        // --- end start page ---
        let Some((mut store, offer)) = crate::session::SessionStore::open_with(dir, restore_clean) else {
            return false; // another Bowser is running; leave its files alone
        };
        let accepted = offer.filter(|offer| {
            !offer.ask
                || win32::ask_yes_no(&format!(
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
                if saved.used != 0 {
                    // --- tab options --- keeps "inactive days" across restarts.
                    if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) {
                        tab.extra.last_used = saved.used;
                    }
                }
                if saved.pinned {
                    state.tab_model.set_pinned(id, true);
                }
                if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.url = Some(url.clone());
                    tab.title.clone_from(&saved.title);
                    tab.discarded = true;
                    // A restored settings or history tab shows its icon before it is first selected.
                    if let Some(page) = crate::internal_page::parse(&url) {
                        tab.favicon = Some(page.favicon().to_vec());
                        tab.favicon_key = Some(crate::favicon::key_for(page.favicon()));
                    }
                }
                App::note_pinned_site(&mut state, id, saved.pinned, Some(&url)); // --- tab options ---
                restored_ids.push(Some(id));
                // --- tab options --- "Load restored tabs when first selected" / "Always load pinned tabs".
                if (!state.settings.tabs.lazy_restore || (state.settings.tabs.load_pinned && saved.pinned)) && index != session.active {
                    state.tab_state.eager.push(id);
                }
                // --- end tab options ---
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
                if saved_split.stacked {
                    split.orientation = crate::split_view::Orientation::Stacked;
                }
                // A layout of three or four panes: each trail whose tab restored (a trail that did
                // not is left out, so its half stays whole).
                let trail = saved_split.trail.map(|index| index.and_then(|index| restored_ids.get(index).copied().flatten()));
                if trail.iter().flatten().all(|tab| !split.contains(*tab)) && (trail[0].is_none() || trail[0] != trail[1]) {
                    split.trail = trail;
                    split.set_cross_ratio(saved_split.cross_ratio);
                    if saved_split.focused_trail {
                        split.focus(split.focused.sibling());
                    }
                }
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
        Self::load_eager_tabs(app); // --- tab options ---
    }

    /// Notes a tab-state change. Returns true when the caller should arm the one-shot save
    /// timer; further changes before it fires are coalesced.
    pub fn session_changed(app: &AppRef) -> bool {
        // A save already queued (or none possible) needs no snapshot: the timer builds the one that
        // is written, so a title that changes every few seconds costs nothing in between.
        {
            let state = app.borrow();
            if !session_arm_allowed(state.session.is_some(), state.session_save_pending, state.closing) {
                return false;
            }
        }
        let snapshot = Self::session_snapshot(app);
        let mut state = app.borrow_mut();
        if snapshot == state.last_session_snapshot {
            return false;
        }
        state.session_save_pending = true;
        true
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
                .and_then(|tab| crate::private::snapshot_url(tab.private, tab.url.as_ref()));
            if url.is_some_and(|url| crate::session::persistable_url(url).is_some()) {
                saved_indexes.insert(model.id, saved_count);
                saved_count += 1;
            }
        }
        let mut session = crate::session::Session::from_tabs(
            state.tab_model.iter().map(|model| {
                let tab = state.tabs.iter().find(|tab| tab.id == model.id);
                (
                    tab.and_then(|tab| crate::private::snapshot_url(tab.private, tab.url.as_ref())),
                    tab.map_or("", |tab| tab.title.as_str()),
                    model.pinned,
                )
            }),
            active,
        )?;
        if let Some(mut split) = state.split_view {
            // A pane whose tab is not saved (private, blank) leaves the saved layout; a pair that
            // loses one is not saved at all, as before.
            for tab in split.tabs().collect::<Vec<_>>() {
                if saved_indexes.contains_key(&tab) || !split.contains(tab) {
                    continue;
                }
                match split.without(tab) {
                    Some(crate::split_view::Remains::Layout(smaller)) => split = smaller,
                    _ => break,
                }
            }
            if let (Some(&left), Some(&right)) = (
                saved_indexes.get(&split.left),
                saved_indexes.get(&split.right),
            ) {
                let trail = split.trail.map(|tab| tab.and_then(|tab| saved_indexes.get(&tab).copied()));
                session.split = Some(crate::session::SessionSplit {
                    left,
                    right,
                    focused_left: split.focused.half() == Pane::Left,
                    ratio: split.ratio,
                    stacked: split.orientation.is_stacked(),
                    trail,
                    cross_ratio: split.cross_ratio,
                    focused_trail: split.focused.is_trail() && trail[split.focused.half_index()].is_some(),
                });
            }
        }
        // --- tab options --- when each saved tab was last used, for "inactive days".
        for model in state.tab_model.iter() {
            if let (Some(&saved), Some(tab)) = (saved_indexes.get(&model.id), state.tabs.iter().find(|tab| tab.id == model.id)) {
                session.tabs[saved].used = tab.extra.last_used;
            }
        }
        // --- end tab options ---
        Some(session)
    }

    /// The debounce timer fired: write the snapshot if anything changed.
    pub fn session_save_now(app: &AppRef) {
        let snapshot = Self::session_snapshot(app);
        let mut state = app.borrow_mut();
        if state.session.is_none() {
            let dirty = std::mem::take(&mut state.workspaces_dirty);
            drop(state);
            if dirty {
                Self::save_workspaces_now(app);
            }
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
        let dirty = std::mem::take(&mut state.workspaces_dirty);
        drop(state);
        if dirty {
            Self::save_workspaces_now(app);
        }
    }

    /// Clean exit: final snapshot and removal of the running marker.
    pub fn session_finish(app: &AppRef) {
        let snapshot = Self::session_snapshot(app);
        let store = app.borrow_mut().session.take();
        if let Some(store) = store {
            store.finish(snapshot);
        }
        Self::save_workspaces_now(app);
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
        let settings_file = profile.settings_file();
        let provider_file = profile.search_provider_file();
        let stamps = [
            config_stamp(std::fs::metadata(&settings_file).ok().as_ref()),
            config_stamp(std::fs::metadata(&provider_file).ok().as_ref()),
        ];
        if !search_config_changed(self.search_config_stamp, stamps) {
            return;
        }
        let settings = crate::settings::Settings::load(&settings_file);
        self.provider = SearchProvider::from_config(settings.search_engine, &provider_file);
        self.search_config_stamp = Some(stamps);
    }

    pub fn submit(app: &AppRef, input: &str) {
        Self::submit_with(app, input, crate::address_input::Options::default());
    }

    /// [`App::submit`] with how the text was submitted (new tab, domain expansion, the search field).
    pub fn submit_with(app: &AppRef, input: &str, options: crate::address_input::Options<'_>) {
        // `bowser://settings...` opens the settings page (a blank tab becomes it); other internal
        // hosts fall through and are reported as an unsupported address, as before.
        if let Some(page) = crate::internal_page::parse_input(input) {
            let hwnd = app.borrow().hwnd;
            if page.is_history() {
                win32::open_history_page(hwnd, app, true);
            } else if page.is_downloads() {
                win32::open_downloads_page(hwnd, app, true);
            } else {
                win32::open_settings_page(hwnd, app, page.section(), true);
            }
            return;
        }
        // Address bar, search and bookmark options (src/address_input.rs): scoped nicknames, engine
        // nicknames, the private-window engine, HTTPS upgrade, domain expansion, new-tab searches.
        let address = {
            let mut state = app.borrow_mut();
            state.refresh_search_provider();
            state.settings.address.clone()
        };
        let private = Self::active_tab_private(app);
        let engines = crate::address_runtime::engines();
        let options = crate::address_input::Options { private, ..options };
        match crate::address_input::resolve(input, &address, &engines, options) {
            crate::address_input::Outcome::Empty => {}
            crate::address_input::Outcome::Refused(message) => {
                win32::show_error_message(&format!("This address or search could not be used: {message}"));
            }
            crate::address_input::Outcome::Scoped { scope, rest } => {
                let hwnd = app.borrow().hwnd;
                win32::address_scoped_submit(hwnd, app, scope, &rest);
            }
            crate::address_input::Outcome::Open { destination, new_tab } => {
                Self::open_destination(app, input, destination, new_tab, private);
            }
        }
    }

    /// The search items a page's context menu gets for a selection or an image: the default engine
    /// (the private-window one in a private tab) searching for the selection, the image engine
    /// searching for the image. Nothing is offered when no engine can.
    fn context_search_labels(app: &AppRef, tab: TabId, info: &engine::ContextSearchInfo) -> engine::ContextSearchLabels {
        let Ok(state) = app.try_borrow() else { return Default::default() };
        let private = state.tabs.iter().any(|candidate| candidate.id == tab && candidate.private);
        let show_nickname = state.settings.address.show_nickname;
        drop(state);
        let engines = crate::address_runtime::engines();
        let selection = info.selection.as_ref().map(|text| {
            let engine = engines.effective(private);
            let shown: String = if text.chars().count() > 28 { format!("{}…", text.chars().take(28).collect::<String>()) } else { text.clone() };
            let _ = show_nickname;
            format!("Search {} for “{shown}”", engine.name)
        });
        let image = info.image.as_ref().and_then(|url| {
            let engine = engines.image_engine()?;
            engine.image_request(url.as_str())?;
            Some(format!("Search for this image with {}", engine.name))
        });
        engine::ContextSearchLabels { selection, image }
    }

    /// A search item of a page's context menu was picked.
    fn context_search_run(app: &AppRef, pick: engine::ContextSearchPick) {
        use crate::address_input::Destination;
        let (private, background) = {
            let state = app.borrow();
            (state.tabs.iter().any(|tab| tab.id == state.tab_model.active() && tab.private), state.settings.address.selection_background)
        };
        let engines = crate::address_runtime::engines();
        match pick {
            engine::ContextSearchPick::Selection(text) => {
                let before = app.borrow().tab_model.active();
                let options = crate::address_input::Options { new_tab: true, from_search_field: true, ..Default::default() };
                Self::submit_with(app, &text, options);
                // "Search for selected text in a background tab": the page you were reading stays shown.
                if background {
                    let _ = Self::select_tab(app, before);
                }
            }
            engine::ContextSearchPick::Image(url) => {
                let Some(engine) = engines.image_engine() else { return };
                let Some(request) = engine.image_request(url.as_str()) else { return };
                let destination = Destination::Search { request, query: url.to_string(), engine: engine.id.clone() };
                Self::open_destination(app, "", destination, true, private);
            }
        }
    }

    /// Opens what Enter resolved to, in the shown tab or a new one, and remembers the typed text.
    fn open_destination(app: &AppRef, typed: &str, destination: crate::address_input::Destination, new_tab: bool, private: bool) {
        use crate::address_input::Destination;
        crate::address_runtime::record_typed(typed, matches!(destination, Destination::Search { .. }), private);
        if new_tab {
            if private {
                Self::new_private_tab(app);
            } else {
                Self::new_tab(app);
            }
        }
        match destination {
            Destination::Navigate { url, upgraded_from } => {
                let tab = app.borrow().tab_model.active();
                match upgraded_from {
                    Some(original) => crate::address_runtime::note_upgrade(tab, original),
                    None => crate::address_runtime::forget_upgrade(tab),
                }
                Self::submit_url(app, url, None)
            }
            Destination::Search { request, .. } => {
                match request.post {
                    Some(body) => POST_BODY.with(|slot| *slot.borrow_mut() = Some((request.url.clone(), body))),
                    None => POST_BODY.with(|slot| *slot.borrow_mut() = None),
                }
                Self::submit_url(app, request.url, None);
                // Whatever did not reach the engine this time must not ride on a later navigation.
                POST_BODY.with(|slot| *slot.borrow_mut() = None);
            }
        }
    }

    pub fn submit_benchmark_url(app: &AppRef, url: url::Url) {
        Self::submit_url(app, url.clone(), Some(url));
    }

    pub fn submit_initial_url(app: &AppRef) {
        let url = app.borrow_mut().initial_url.take();
        if let Some(url) = url {
            Self::submit_url(app, url, None);
        }
        // --- tab stacks (T10) --- the rest of a stack moved to this new window: one tab each, asleep
        // until selected, as restored tabs are.
        let more = std::mem::take(&mut app.borrow_mut().more_initial_urls);
        let shown = Self::active_tab_id(app);
        for url in more {
            let id = Self::new_background_tab(app, false, None);
            Self::make_asleep(app, id, &url);
            let _ = Self::move_tab(app, id, usize::MAX);
        }
        let _ = Self::select_tab(app, shown);
    }

    // --- start page ---
    /// The start page settings alone (a window syncs this often; cloning every setting for it would not be cheap).
    pub fn start_settings(app: &AppRef) -> crate::start_page::StartSettings {
        app.borrow().settings.start.clone()
    }

    /// Whether the launch already decided what the first window shows: a session to restore, an address
    /// on the command line or a benchmark URL. Read before any of them is taken.
    pub fn launch_has_target(app: &AppRef) -> bool {
        let state = app.borrow();
        state.pending_restore.is_some() || state.initial_url.is_some() || state.benchmark_url.is_some()
    }

    /// The `startup` setting's plan for the first window (docs/design/start-page.md): the homepage, or
    /// the specific pages, one tab each. The start page itself needs nothing here (the first tab is
    /// blank and shows it) and "continue where you left off" is the session restore.
    pub fn apply_startup_plan(app: &AppRef, launch_decided: bool) {
        use crate::start_page::StartupPlan;
        let plan = crate::start_page::startup_plan(&app.borrow().settings.start, launch_decided);
        match plan {
            StartupPlan::ShowStartPage => {}
            StartupPlan::Homepage(url) => Self::navigate_current(app, &url),
            StartupPlan::Pages(urls) => {
                let mut first = None;
                for (index, url) in urls.iter().enumerate() {
                    if index == 0 {
                        Self::navigate_current(app, url);
                        first = Some(Self::active_tab_id(app));
                    } else {
                        Self::new_tab(app);
                        Self::navigate_current(app, url);
                    }
                }
                // The first page stays the one shown.
                if let Some(first) = first {
                    let _ = Self::select_tab(app, first);
                }
            }
        }
    }
    // --- end start page ---

    fn submit_url(app: &AppRef, url: url::Url, allowed_file: Option<url::Url>) {
        // An internal address (a restored settings tab; the engine never produces one) never
        // reaches the engine: the shown tab becomes the page, or the page opens beside it.
        if crate::internal_page::is_internal(&url) {
            let active = app.borrow().tab_model.active();
            Self::navigate_internal(app, active, &url);
            return;
        }
        // --- tab stacks (T10) --- a site with a workspace rule opens in its workspace, which the
        // window switches to.
        let active = app.borrow().tab_model.active();
        let _ = Self::apply_workspace_rule(app, active, &url, true);
        // The address bar shows the target now, not after the engine starts and the page commits.
        let (hwnd, edit, active) = {
            let state = app.borrow();
            (state.hwnd, state.edit, state.tab_model.active())
        };
        win32::show_pending_address(hwnd, edit, active, &url, Self::settings(app).minimal_url);
        {
            let mut state = app.borrow_mut();
            let _ = state.log.record("submitted");
            if state.engine.is_some() {
                let active_tab = state.tab_model.active();
                state.begin_navigation_grouping(active_tab);
                let App { tabs, tab_model, .. } = &mut *state;
                if let Some(tab) = tabs.iter_mut().find(|tab| tab.id == active_tab) {
                    Self::leave_internal_page(tab, tab_model);
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
        let (hwnd, profile, tracking_prevention, blocker, private_blocker) = {
            let s = app.borrow();
            (
                s.hwnd,
                s.user_data_dir.clone(),
                // Production launches state the user's level explicitly (Balanced by default, Strict
                // on request, read at startup); benchmark launches keep the engine default so
                // recorded baselines stay comparable.
                tracking_prevention_request(
                    s.benchmark_tracking_prevention,
                    s.benchmark_url.is_some(),
                    s.settings.tracking_prevention,
                ),
                s.blocker.clone(),
                s.private_blocker.clone(),
            )
        };
        let events_app = Rc::downgrade(app);
        let title_events_app = Rc::downgrade(app);
        let nav_progress_events_app = Rc::downgrade(app);
        let favicon_events_app = Rc::downgrade(app);
        let login_events_app = Rc::downgrade(app);
        // Metadata only: the vault is opened and decrypted when a login event asks for it, not here.
        let login_messages_enabled = crate::profile::active().is_ok_and(|profile| {
            crate::engine::password_login_messages_enabled(
                crate::passwords::PasswordStore::login_events_available(),
                profile.is_guest(),
            )
        });
        let progress_hwnd = hwnd;
        let events = engine::EngineEvents {
            address_changed: Some(Rc::new(move |tab_id, url| {
                if let Some(app) = events_app.upgrade() {
                    let mut left_reader = false;
                    let mut left_translate = false;
                    let (edit, log, active, found) = {
                        let mut state = app.borrow_mut();
                        let active = state.tab_model.active() == tab_id;
                        let found =
                            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                                // A different address (also a same-document one) ends reader view.
                                if tab.url.as_ref().is_some_and(|old| *old != url) {
                                    left_reader = tab.reader.active != crate::reader::Active::Off;
                                    tab.reader.reset();
                                    // A translated page that changes address in place (a single-page app)
                                    // keeps its translated text; ask it to put the original back.
                                    left_translate = tab.translate.active;
                                    tab.translate.reset();
                                }
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
                    if left_reader {
                        Self::reader_run_exit(&app, tab_id);
                    }
                    if left_translate {
                        Self::translate_run_restore(&app, tab_id);
                    }
                    if active {
                        let main = app.borrow().hwnd;
                        let event = if win32::sync_address_input(main, edit, tab_id, &url, Self::settings(&app).minimal_url) {
                            "address_updated"
                        } else {
                            "address_update_failed"
                        };
                        let _ = log.record(event);
                    }
                    // Navigation commit: the page shows at its site's stored zoom from here on.
                    Self::apply_stored_zoom(&app, tab_id, &url);
                    Self::post_tab_changed(app.borrow().hwnd, tab_id);
                }
            })),
            title_changed: Some(Rc::new(move |tab_id, title| {
                if let Some(app) = title_events_app.upgrade() {
                    let mut state = app.borrow_mut();
                    // --- tab options --- a count in the title ("(3) Inbox") is a notification.
                    let detect = state.settings.tabs.title_notifications;
                    let shown = state.tab_model.active() == tab_id;
                    // --- end tab options ---
                    let found =
                        if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                            tab.title.clone_from(&title);
                            tab_features::note_title(tab, &title, detect, shown); // --- tab options ---
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
                        App::sample_page_accent(&app, tab_id); // --- theme options ---
                        // One read-only probe decides whether the toolbar offers reader view.
                        if app.borrow().tab_model.active() == tab_id {
                            App::schedule_reader_probe(&app, tab_id);
                        }
                        // One small script reads the feeds the page offers (the address bar's feed button).
                        App::feed_probe(&app, tab_id);
                    }
                }))
            },
            escape_pressed: {
                let escape_app = Rc::downgrade(app);
                Some(Rc::new(move |tab_id| {
                    if let Some(app) = escape_app.upgrade() {
                        App::reader_escape(&app, tab_id);
                    }
                }))
            },
            // --- auto-hide (T11) --- a page element (a video) asks for full screen, or leaves it.
            fullscreen_changed: {
                let events_app = Rc::downgrade(app);
                Some(Rc::new(move |tab_id, on| {
                    if let Some(app) = events_app.upgrade() {
                        let (hwnd, active) = {
                            let state = app.borrow();
                            (state.hwnd, state.tab_model.active() == tab_id)
                        };
                        if active || !on {
                            win32::page_fullscreen(hwnd, &app, on);
                        }
                    }
                }))
            },
            // --- end auto-hide ---
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
                            win32::auto_hide_page_created(state.hwnd); // --- auto-hide (T11) --- a page created later starts above the bars
                            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                                tab.controller_created = true;
                                // A new controller starts at 100%; the next commit re-applies
                                // the site's stored zoom.
                                tab.zoom = crate::site_prefs::ZOOM_DEFAULT;
                                tab.hidden_since = (!visible).then(Instant::now);
                                tab.suspended = false;
                                tab.muted || tab.extra.auto_muted // --- tab options ---
                            } else {
                                false
                            }
                        };
                        if should_mute {
                            Self::push_effective_mute(&app, tab_id); // --- tab options ---
                        }
                        // --- split view ---
                        // A side's page now exists: put it at its pane (outside this callback).
                        let split_side = app.borrow().split_view.is_some_and(|split| split.contains(tab_id));
                        if split_side {
                            app.borrow_mut().split_refresh = true;
                            Self::post_tab_changed_kind(app.borrow().hwnd, tab_id, true, false);
                        }
                        // --- end split view ---
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
                                // --- tab options --- finished loading while out of sight: unread.
                                if !active {
                                    tab.extra.loaded_in_background();
                                }
                                // --- end tab options ---
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
                            App::schedule_thumbnail(&app); // --- tab options ---
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
                        let mut left_reader = false;
                        if let Some(tab) = app
                            .borrow_mut()
                            .tabs
                            .iter_mut()
                            .find(|tab| tab.id == tab_id)
                        {
                            tab.page_usable = false;
                            tab.loading = true;
                            tab.load = LoadState::Loading;
                            tab.permission_blocked = false;
                            // Any navigation, including a reload, ends reader view.
                            left_reader = tab.reader.active != crate::reader::Active::Off;
                            tab.reader.reset();
                            // The document is being replaced: forget the translation state, which also
                            // makes any translation still running for the old one stale.
                            tab.translate.reset();
                        }
                        if left_reader {
                            Self::reader_run_exit(&app, tab_id);
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
                        Self::note_favicon_colour(&app, tab_id); // --- theme options ---
                    }
                }
            })),
            new_window_requested: {
                let popup_app = Rc::downgrade(app);
                Some(Rc::new(move |source_tab, url| {
                    if let Some(app) = popup_app.upgrade() {
                        if app.borrow().tabs.iter().any(|tab| tab.id == source_tab) {
                            let private = crate::private::inherits_privacy(
                                App::tab_private(&app, source_tab),
                            );
                            // --- tab options --- position, related tabs and background loading.
                            App::open_link_tab(&app, source_tab, private, url);
                            // --- end tab options ---
                        }
                    }
                }))
            },
            // --- tab options --- pinned tabs stay on their site.
            link_navigation: {
                let link_app = Rc::downgrade(app);
                Some(Rc::new(move |tab, url: &url::Url| {
                    link_app.upgrade().is_some_and(|app| App::pinned_link(&app, tab, url))
                }))
            },
            // --- end tab options ---
            // --- split view ---
            open_in_split: {
                let split_app = Rc::downgrade(app);
                Some(Rc::new(move |source_tab, url| {
                    if let Some(app) = split_app.upgrade()
                        && app.borrow().tabs.iter().any(|tab| tab.id == source_tab)
                    {
                        win32::queue_split_link(app.borrow().hwnd, url);
                    }
                }))
            },
            // --- end split view ---
            // --- address bar, search and bookmarks ---
            context_search: {
                let search_app = Rc::downgrade(app);
                Some(Rc::new(move |tab, info: &engine::ContextSearchInfo| {
                    search_app.upgrade().map(|app| App::context_search_labels(&app, tab, info)).unwrap_or_default()
                }))
            },
            context_search_pick: {
                let search_app = Rc::downgrade(app);
                Some(Rc::new(move |_tab, pick| {
                    if let Some(app) = search_app.upgrade() {
                        App::context_search_run(&app, pick);
                    }
                }))
            },
            // --- end address bar, search and bookmarks ---
            // --- page tools (T7): "Insert note" in a text field's context menu ---
            context_notes: Some(Rc::new(move |_tab| win32::context_note_items())),
            context_note_pick: {
                let note_app = Rc::downgrade(app);
                Some(Rc::new(move |tab, note| {
                    if let Some(app) = note_app.upgrade() {
                        win32::insert_note_into_page(&app, tab, note);
                    }
                }))
            },
            // --- end page tools ---
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
                        if count != 0
                            && !app.borrow().tabs.iter().any(|tab| tab.id == tab_id && tab.private)
                        {
                            Self::record_blocked_request(&app, &site_host);
                        }
                    }
                }))
            },
            navigation_failed: {
                let failed_app = Rc::downgrade(app);
                Some(Rc::new(move |tab_id, certificate| {
                    if let Some(app) = failed_app.upgrade() {
                        // "Always use HTTPS": a site that cannot be reached securely is tried once over
                        // plain http (never after a certificate problem, which must not be bypassed).
                        if !certificate {
                            if let Some(original) = crate::address_runtime::take_upgrade(tab_id) {
                                // Only when the page that failed is the one that was upgraded.
                                let same_site = app.borrow().tabs.iter().find(|tab| tab.id == tab_id).and_then(|tab| tab.url.as_ref())
                                    .is_some_and(|shown| shown.scheme() == "https" && shown.host_str() == original.host_str());
                                if same_site {
                                    Self::navigate_tab(&app, tab_id, &original);
                                    return;
                                }
                            }
                        }
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
            permission_denied: {
                let denied_app = Rc::downgrade(app);
                Some(Rc::new(move |tab_id, kind, origin| {
                    if let Some(app) = denied_app.upgrade() {
                        let changed = app.try_borrow_mut().is_ok_and(|mut state| {
                            // The dashboard's recent list: origin, kind and time, never content.
                            // A private tab's requests are blocked but never listed (the list
                            // would name a private site and offer a stored Allow for it).
                            if !state.tabs.iter().any(|tab| tab.id == tab_id && tab.private) {
                                state
                                    .denied
                                    .record(&origin, kind, crate::permissions::now_secs());
                            }
                            state
                                .tabs
                                .iter_mut()
                                .find(|tab| tab.id == tab_id)
                                .is_some_and(|tab| !std::mem::replace(&mut tab.permission_blocked, true))
                        });
                        if changed {
                            Self::post_tab_changed_kind(app.borrow().hwnd, tab_id, false, false);
                        }
                    }
                }))
            },
            zoom_changed: {
                let zoom_app = Rc::downgrade(app);
                Some(Rc::new(move |tab_id, percent| {
                    if let Some(app) = zoom_app.upgrade() {
                        Self::page_zoom_changed(&app, tab_id, percent);
                    }
                }))
            },
            login_message: login_messages_enabled.then(|| {
                let callback: Rc<dyn Fn(TabId, crate::passwords::LoginMessage)> =
                    Rc::new(move |tab_id, message| {
                        if let Some(app) = login_events_app.upgrade() {
                            App::queue_login_message(&app, tab_id, message);
                        }
                    });
                callback
            }),
            download_event: Some(Self::download_event_hook(app)),
            download_plan: Some(Self::download_plan_hook(app)),
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
            private_blocker,
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
        // Private tabs are marked first, so their controllers are created InPrivate.
        let private_ids: Vec<TabId> = app
            .borrow()
            .tabs
            .iter()
            .filter(|tab| tab.private)
            .map(|tab| tab.id)
            .collect();
        for tab_id in private_ids {
            engine.set_tab_private(tab_id);
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
            for tab in split.tabs() {
                Self::restore_discarded_tab(app, tab);
            }
            Self::apply_visible_panes(app);
        }
        Self::load_eager_tabs(app); // --- tab options ---
    }

    pub fn navigate_current(app: &AppRef, url: &url::Url) {
        let active = app.borrow().tab_model.active();
        Self::navigate_tab(app, active, url);
    }

    fn navigate_tab(app: &AppRef, tab_id: TabId, url: &url::Url) {
        if !app.borrow().tabs.iter().any(|tab| tab.id == tab_id) {
            // The engine would otherwise create a controller for an id the window already closed.
            return;
        }
        if crate::internal_page::is_internal(url) {
            Self::navigate_internal(app, tab_id, url);
            return;
        }
        let (mut engine, closing) = {
            let mut state = app.borrow_mut();
            let is_background = state.tab_model.active() != tab_id;
            if state.engine.is_some() {
                state.begin_navigation_grouping(tab_id);
            }
            let App { tabs, tab_model, .. } = &mut *state;
            if let Some(tab) = tabs.iter_mut().find(|tab| tab.id == tab_id) {
                Self::leave_internal_page(tab, tab_model);
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
        // A search with an engine that posts its query goes out as that POST, once.
        let post = POST_BODY.with(|slot| {
            let mut slot = slot.borrow_mut();
            match slot.as_ref() {
                Some((target, _)) if target == url => slot.take().map(|(_, body)| body),
                _ => None,
            }
        });
        let result = match post {
            Some(body) => engine.navigate_tab_post(tab_id, url, &body),
            None => engine.navigate_tab(tab_id, url),
        };
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

    /// `url` is internal. A settings address shows the page in `tab_id` when that tab has no
    /// content page (restored, discarded, new), otherwise opens it like the menu does; anything
    /// else on the internal scheme is refused, as an unsupported address always was.
    fn navigate_internal(app: &AppRef, tab_id: TabId, url: &url::Url) {
        let Some(page) = crate::internal_page::parse(url) else {
            win32::show_error_message(&format!("Bowser has no page at {}.", url.as_str()));
            return;
        };
        let (has_page, private, active) = {
            let state = app.borrow();
            let tab = state.tabs.iter().find(|tab| tab.id == tab_id);
            (
                tab.is_some_and(|tab| tab.controller_created),
                tab.is_some_and(|tab| tab.private),
                state.tab_model.active() == tab_id,
            )
        };
        if has_page || private {
            if active {
                let hwnd = app.borrow().hwnd;
                if page.is_history() {
                    win32::open_history_page(hwnd, app, false);
                } else if page.is_downloads() {
                    win32::open_downloads_page(hwnd, app, false);
                } else {
                    win32::open_settings_page(hwnd, app, page.section(), false);
                }
            }
            return;
        }
        Self::show_internal_page(app, tab_id, page);
        if active {
            let hwnd = app.borrow().hwnd;
            if page.is_history() {
                win32::history_page_focus(hwnd);
            } else if page.is_downloads() {
                win32::downloads_page_focus(hwnd);
            } else {
                win32::settings_page_go(hwnd, page.section());
            }
        }
    }

    pub fn resize_current(app: &AppRef, bounds: windows::Win32::Foundation::RECT) {
        let (split, panes) = {
            let state = app.borrow();
            (state.split_view, state.split_bounds)
        };
        let Some(mut engine) = app.borrow_mut().engine.take() else {
            return;
        };
        let result = if let (Some(split), Some(panes)) = (split, panes) {
            let visible: Vec<_> = split.panes().filter_map(|(pane, tab)| panes.get(pane).map(|rect| (tab, rect))).collect();
            engine.set_visible_tabs(&visible, split.focused_tab())
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
        if transition.target == Some(MemoryUsageTarget::Normal) {
            let _ = Self::with_engine(app, |e| e.set_visible(true));
        }
        if transition.resize_bounds {
            if let Some(bounds) = resize_bounds {
                Self::resize_current(app, bounds);
            }
        }
        if transition.target == Some(MemoryUsageTarget::Low) {
            let _ = Self::with_engine(app, |e| e.set_visible(false));
        }
        // EcoQoS follows the minimised state; it is set only while nobody can see the window.
        Self::sync_eco_qos(app);
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
        let suspend_after = Self::background_policy(app).suspend_after;
        let _ = Self::with_engine(app, |engine| {
            engine.suspend_hidden_tabs(suspend_after.as_millis() as u64)
        });
        let newly_suspended = {
            let mut state = app.borrow_mut();
            let active = state.tab_model.active();
            let mut count = 0usize;
            for tab in &mut state.tabs {
                if tab.id != active
                    && tab.controller_created
                    && !tab.suspended
                    && tab.hidden_since.is_some_and(|hidden| {
                        hidden_tab_suspend_eligible(
                            now.saturating_duration_since(hidden),
                            suspend_after,
                        )
                    })
                {
                    tab.suspended = true;
                    count += 1;
                }
            }
            count
        };
        for _ in 0..newly_suspended {
            let _ = app.borrow().log.record("hidden_tab_suspended");
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

    fn policy_of(state: &App) -> efficiency::BackgroundPolicy {
        efficiency::background_policy(
            state.efficiency.active,
            state.settings.sleep_enabled,
            state.settings.sleep_after_secs,
        )
    }

    /// Delays that currently govern hidden tabs: the normal ones, or the shorter efficient ones.
    fn background_policy(app: &AppRef) -> efficiency::BackgroundPolicy {
        Self::policy_of(&app.borrow())
    }

    /// Reads the power source, decides whether efficiency mode is active and applies the result.
    /// Called at startup, on `WM_POWERBROADCAST` and from a slow fallback timer. Cheap when nothing
    /// changed: one `GetSystemPowerStatus` call and a comparison.
    pub fn refresh_efficiency(app: &AppRef) {
        let power = efficiency::current_power();
        let (changed, active, mode, hwnd) = {
            let mut state = app.borrow_mut();
            let mode = state.settings.efficiency_mode;
            let changed = state.efficiency.evaluate(mode, power);
            (changed, state.efficiency.active, mode, state.hwnd)
        };
        win32::schedule_power_timer(hwnd, mode == crate::settings::EfficiencyMode::Auto);
        if !changed {
            return;
        }
        let _ = app
            .borrow()
            .log
            .record(if active { "efficiency_active" } else { "efficiency_inactive" });
        if active {
            let forced = Self::with_engine(app, |engine| {
                engine.force_background_memory_low();
                Ok(())
            });
            if forced.is_some() {
                let _ = app.borrow().log.record("efficiency_background_memory_low");
            }
        }
        Self::sync_eco_qos(app);
        // Shorter (or longer) delays mean the next wake-up moves.
        Self::schedule_tab_idle_timer(app);
    }

    /// Whether efficiency mode is in force right now (for the Settings panel).
    pub fn efficiency_active(app: &AppRef) -> bool {
        app.borrow().efficiency.active
    }

    /// A clone of the profile's current settings, for callers outside this module that only need to
    /// read a value or two (win32 chrome glue). Settings are not large or changed often enough for
    /// this to matter.
    pub fn settings(app: &AppRef) -> crate::settings::Settings {
        app.borrow().settings.clone()
    }

    /// The accent a chrome host should theme with, from the `theme`/`theme_accent` settings and (for
    /// `theme=system`) the live Windows accent colour. `None` keeps the palette's own colours.
    pub fn settings_accent(app: &AppRef) -> Option<crate::theme::Rgba> {
        let state = app.borrow();
        crate::theme::resolved_accent(
            state.settings.theme,
            state.settings.theme_accent.as_deref(),
            crate::theme::system_accent_colour(),
        )
    }

    /// Settings saved on the settings page that take effect immediately and have no dedicated
    /// `apply_*` of their own (layout, efficiency and sleep already have one, each with its own
    /// resize or reschedule). Everything else here only changes `settings.txt`'s file-committed value
    /// and takes effect on the next start, matching the page's "Applies after restart" rows.
    pub fn apply_live_settings(app: &AppRef, settings: &crate::settings::Settings) {
        let before = app.borrow().settings.clone(); // --- wave-1 fixes ---
        // The blocker, retention and website-look settings take effect without a restart.
        Self::apply_privacy_settings(app, settings);
        Self::apply_page_tools_settings(app, settings); // --- page tools (T7) ---
        let (hwnd, edit) = {
            let mut state = app.borrow_mut();
            // --- wave-1 fixes --- the Downloads and Translation options the page saves through the app
            // (and Reset settings puts back), so memory never holds an older value than the file.
            state.settings.download_dir = settings.download_dir.clone();
            state.settings.ask_download_location = settings.ask_download_location;
            state.settings.translate_offer = settings.translate_offer;
            state.settings.translate_auto = settings.translate_auto;
            state.settings.translate_target = settings.translate_target.clone();
            // --- end wave-1 fixes ---
            state.settings.theme = settings.theme;
            state.settings.theme_accent = settings.theme_accent.clone();
            state.settings.themes = settings.themes.clone(); // --- theme options ---
            state.settings.centred_address_bar = settings.centred_address_bar;
            state.settings.minimal_url = settings.minimal_url;
            state.settings.show_load_progress = settings.show_load_progress;
            state.settings.rounded_content_frame = settings.rounded_content_frame;
            state.settings.show_home_button = settings.show_home_button;
            state.settings.toolbar_buttons = settings.toolbar_buttons.clone();
            // --- toolbars and feeds (thread T13) ---
            state.settings.toolbar_tabs = settings.toolbar_tabs.clone();
            state.settings.toolbar_panel = settings.toolbar_panel.clone();
            state.settings.feeds_detect = settings.feeds_detect;
            state.settings.feeds_refresh_minutes = settings.feeds_refresh_minutes;
            if !settings.feeds_detect {
                state.feed_pages.clear();
            }
            // --- end toolbars and feeds ---
            state.settings.font_size = settings.font_size;
            state.settings.ui_font = settings.ui_font.clone();
            state.settings.default_zoom = settings.default_zoom;
            state.settings.show_zoom_indicator = settings.show_zoom_indicator;
            state.settings.new_tab_beside_active = settings.new_tab_beside_active;
            state.settings.ctrl_tab_mru = settings.ctrl_tab_mru;
            state.settings.shift_right_click_context = settings.shift_right_click_context;
            state.settings.ctrl_shift_c_copy_link = settings.ctrl_shift_c_copy_link;
            state.settings.ctrl_s_vertical_tabs = settings.ctrl_s_vertical_tabs;
            state.settings.split_edge_drop = settings.split_edge_drop;
            state.settings.pin_new_tab_groups = settings.pin_new_tab_groups;
            state.settings.start = settings.start.clone(); // --- start page ---
            // --- auto-hide (T11) --- the master switch and its two rows take effect at once.
            state.settings.frameless = settings.frameless;
            state.settings.auto_hide = settings.auto_hide;
            // --- end auto-hide ---
            // Address bar, search and bookmark options (src/address_settings.rs).
            state.settings.address = settings.address.clone();
            crate::omnibox::set_show_full_address(settings.address.show_full);
            crate::omnibox::set_autocomplete(settings.address.autocomplete);
            state.settings.bookmarks_bar = settings.bookmarks_bar;
            state.settings.bookmarks_bar_tab_groups = settings.bookmarks_bar_tab_groups;
            // --- tab options --- the hover card's thumbnail and memory line.
            state.settings.tab_preview_images = settings.tab_preview_images;
            state.settings.tab_preview_memory = settings.tab_preview_memory;
            // --- end tab options ---
            // --- layout (thread T6) --- the Layout block: every row acts at once (the window lays
            // the chrome out again below from `App::arrangement`).
            state.settings.side_panel_position = settings.side_panel_position;
            state.settings.tab_bar_visible = settings.tab_bar_visible;
            state.settings.address_bar_position = settings.address_bar_position;
            state.settings.address_bar_visible = settings.address_bar_visible;
            state.settings.show_title_bar = settings.show_title_bar;
            state.settings.status_bar = settings.status_bar;
            state.settings.ui_density = settings.ui_density;
            state.settings.compact_menus = settings.compact_menus;
            state.settings.ui_zoom = settings.ui_zoom;
            state.settings.layout_saved = settings.layout_saved.clone();
            // --- end layout ---
            // --- window and panels (thread T9) --- the HTML chrome reads these from its next state push
            state.settings.window = settings.window;
            crate::window_prefs::publish(&settings.window);
            // --- end window and panels ---
            (state.hwnd, state.edit)
        };
        // --- layout (thread T6) --- the docked panel moves edge at once; the link-address overlay
        // follows the status bar choice in every open tab and the ones opened next.
        if !hwnd.is_invalid() {
            win32::side_panel::set_position(hwnd, settings.side_panel_position);
        }
        crate::layout_prefs::set_status_overlay(settings.status_bar);
        Self::call_engine(app, |engine| engine.apply_status_overlay());
        // --- end layout ---
        // --- wave-1 integration --- the Tabs options as the file has them (Reset settings, the GDI form):
        // the strip, the audio rule and the tick follow at once.
        let tabs_before = {
            let mut state = app.borrow_mut();
            (state.settings.tabs != settings.tabs).then(|| std::mem::replace(&mut state.settings.tabs, settings.tabs.clone()))
        };
        if let Some(before) = tabs_before {
            Self::tab_options_changed(app, &before);
        }
        Self::mark_synced_from(app, &before); // --- wave-1 fixes ---
        Self::apply_theme_options(app); // --- theme options ---
        // --- auto-hide (T11) --- lays the window out with the bars in or out and resizes the page.
        if !hwnd.is_invalid() {
            win32::auto_hide_sync(hwnd, app);
        }
        // --- end auto-hide ---
        let bounds = app.borrow_mut().latest_viewport_bounds();
        Self::resize_current(app, bounds);
        // The address field's own font follows font_size and ui_font (the painted chrome and the
        // HTML toolbar re-read them on the relayout below).
        if !hwnd.is_invalid() && !edit.is_invalid() {
            win32::refresh_edit_font(hwnd, app);
        }
        let active = Self::active_tab_id(app);
        if let Some(url) = Self::tab_committed_url(app, active) {
            win32::sync_address_input(hwnd, edit, active, &url, settings.minimal_url);
        }
        Self::notify_rail_changed(app);
        Self::post_tab_changed(hwnd, active);
        win32::start_ui::settings_changed(hwnd); // --- start page --- a shown start page redraws with the new settings
        win32::panel_settings_changed(hwnd); // --- look state (fix group B) --- an open flyout follows too
    }

    /// Applies a changed `efficiency_mode` setting without a restart.
    pub fn apply_efficiency_mode(app: &AppRef, mode: crate::settings::EfficiencyMode) {
        app.borrow_mut().settings.efficiency_mode = mode;
        Self::refresh_efficiency(app);
    }

    /// Sets or clears EcoQoS on this process only, to match `ecoqos_wanted`.
    fn sync_eco_qos(app: &AppRef) {
        let change = {
            let state = app.borrow();
            efficiency::eco_qos_change(
                state.efficiency.eco_qos,
                efficiency::ecoqos_wanted(state.efficiency.active, state.minimized),
            )
        };
        let Some(on) = change else {
            return;
        };
        let applied = efficiency::set_process_eco_qos(on);
        let mut state = app.borrow_mut();
        if applied {
            state.efficiency.eco_qos = on;
        }
        let _ = state.log.record(match (on, applied) {
            (true, true) => "ecoqos_applied",
            (false, true) => "ecoqos_cleared",
            (_, false) => "ecoqos_failed",
        });
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
        let after = Self::background_policy(app).sleep_after?;
        let state = app.borrow();
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
        let suspend_after = Self::background_policy(app).suspend_after;
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
                if hidden_tab_suspend_eligible(elapsed, suspend_after) {
                    1
                } else {
                    (suspend_after - elapsed).as_millis().max(1) as u32
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
            let sleep_after = Self::policy_of(&state).sleep_after;
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
            private: tab.private,
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
        // --- auto-hide (T11) --- maximised or full screen; and the bars out of the layout, which the
        // window lays out the same way (`chrome::layout::compute_arranged_auto_hide`).
        let maximised = crate::win32::zoomed_or_full(self.hwnd);
        let hidden = crate::win32::auto_hide_effective(self.hwnd, &self.settings).hidden;
        let plan = crate::auto_hide::plan(hidden, self.settings.tab_layout);
        // --- end auto-hide ---
        // --- layout (thread T6) --- the whole arrangement, with a hidden address bar shown while
        // Ctrl+L has it revealed, and the docked panel on the edge it is actually on (none of its
        // width when auto-hide floats it over the page).
        let arrangement = self
            .arranged() // --- two-level stacks (T10b) ---
            .with_address_bar_revealed(crate::win32::address_bar_revealed(self.hwnd));
        let side_panel = (
            if plan.panel { 0 } else { crate::win32::side_panel::active_width_px(self.hwnd) },
            crate::win32::side_panel::active_position(self.hwnd),
        );
        let bounds = chrome_viewport_bounds_arranged(
            rect.right - rect.left,
            rect.bottom - rect.top,
            dpi,
            self.tabs.len(),
            maximised,
            arrangement,
            self.settings.effective_sidebar_width(),
            side_panel,
            self.settings.ui_density,
            plan.layout(),
        );
        if valid_viewport_bounds(bounds) {
            self.last_valid_viewport_bounds = Some(bounds);
            bounds
        } else {
            if let Some(bounds) = self.last_valid_viewport_bounds {
                bounds
            } else {
                let fallback = chrome_viewport_bounds_arranged(
                    1000,
                    720,
                    dpi,
                    self.tabs.len(),
                    maximised,
                    arrangement,
                    self.settings.effective_sidebar_width(),
                    side_panel,
                    self.settings.ui_density,
                    plan.layout(),
                );
                self.last_valid_viewport_bounds = Some(fallback);
                fallback
            }
        }
    }
}

/// The page bounds for a plain tab layout (the tests' older entry point; the window uses
/// [`chrome_viewport_bounds_arranged`]).
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn chrome_viewport_bounds(
    width: i32,
    height: i32,
    dpi: u32,
    tab_count: usize,
    maximised: bool,
    tab_layout: crate::settings::TabLayout,
    sidebar_width: u16,
    side_panel_width: i32,
) -> windows::Win32::Foundation::RECT {
    chrome_viewport_bounds_hiding(
        width,
        height,
        dpi,
        tab_count,
        maximised,
        tab_layout,
        sidebar_width,
        side_panel_width,
        crate::chrome::layout::AutoHide::OFF,
    )
}

/// [`chrome_viewport_bounds`] with the chrome in `hide` out of the page's rectangle (auto-hide).
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn chrome_viewport_bounds_hiding(
    width: i32,
    height: i32,
    dpi: u32,
    tab_count: usize,
    maximised: bool,
    tab_layout: crate::settings::TabLayout,
    sidebar_width: u16,
    side_panel_width: i32,
    hide: crate::chrome::layout::AutoHide,
) -> windows::Win32::Foundation::RECT {
    chrome_viewport_bounds_arranged(
        width,
        height,
        dpi,
        tab_count,
        maximised,
        crate::chrome::layout::Arrangement { tabs: tab_layout, ..Default::default() },
        sidebar_width,
        (side_panel_width, crate::settings::SidePanelPosition::Left),
        crate::layout_prefs::UiDensity::Regular,
        hide,
    )
}

/// --- layout (thread T6) --- the page bounds for the whole arrangement (tab bar, address bar,
/// title row, sidebar side and density), from the same geometry the window lays its chrome out
/// with, less the docked side panel on its own edge. --- auto-hide (T11) --- `hide` takes the
/// hidden bars out of it the way the window does (`layout::compute_arranged_auto_hide`).
#[allow(clippy::too_many_arguments)]
fn chrome_viewport_bounds_arranged(
    width: i32,
    height: i32,
    dpi: u32,
    tab_count: usize,
    maximised: bool,
    arrangement: crate::chrome::layout::Arrangement,
    sidebar_width: u16,
    side_panel: (i32, crate::settings::SidePanelPosition),
    density: crate::layout_prefs::UiDensity,
    hide: crate::chrome::layout::AutoHide,
) -> windows::Win32::Foundation::RECT {
    let (layout, _) = crate::chrome::layout::compute_arranged_auto_hide(
        arrangement,
        width,
        height,
        dpi,
        tab_count.max(1),
        0,
        maximised,
        i32::from(sidebar_width),
        true,
        density.sidebar_row_height(),
        hide,
    );
    let mut viewport = layout.viewport;
    let (panel_width, position) = side_panel;
    match position {
        crate::settings::SidePanelPosition::Left => viewport.left = (viewport.left + panel_width.max(0)).min(viewport.right),
        crate::settings::SidePanelPosition::Right => viewport.right = (viewport.right - panel_width.max(0)).max(viewport.left),
    }
    windows::Win32::Foundation::RECT {
        left: viewport.left,
        top: viewport.top,
        right: viewport.right,
        bottom: viewport.bottom,
    }
}

/// --- wave-1 fixes --- a windowless app for tests outside this module (the win32 glue's rules).
#[cfg(test)]
pub(crate) fn test_app() -> AppRef {
    tests::app_with_pending(windows::Win32::Foundation::RECT { left: 0, top: 0, right: 800, bottom: 600 })
}

/// --- wave-1 fixes --- tab `id` shows `address`, as if it had loaded (for [`test_app`]).
#[cfg(test)]
pub(crate) fn test_show_page(app: &AppRef, id: TabId, address: &str) {
    let mut state = app.borrow_mut();
    state.tab_model.set_url(id, Some(address));
    if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) {
        tab.url = url::Url::parse(address).ok();
        tab.controller_created = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::MemoryUsageTarget;
    use std::{cell::RefCell, collections::VecDeque, path::Path, rc::Rc};

    mod translate_flow;
    mod start_page_flow; // the startup setting's first-window plan (docs/design/start-page.md)
    mod tab_flow; // --- tab options ---
    mod feed_flow; // --- toolbars and feeds (thread T13) ---
    mod wave1_fixes; // --- wave-1 fixes ---
    mod page_tools_flow; // --- page tools (T7) ---
    mod theme_flow; // --- theme options ---
    mod stack_flow; // --- tab stacks, tiling and workspaces (T10) ---

    #[test]
    fn reader_probe_due_table() {
        let tab = TabId(3);
        let other = TabId(4);
        // (pending, active, usable, controller, discarded, probed, epoch_now, due)
        let table = [
            (Some((tab, 2)), tab, true, true, false, false, 2, true),
            (None, tab, true, true, false, false, 2, false),
            (Some((tab, 2)), other, true, true, false, false, 2, false),
            (Some((tab, 2)), tab, false, true, false, false, 2, false),
            (Some((tab, 2)), tab, true, false, false, false, 2, false),
            (Some((tab, 2)), tab, true, true, true, false, 2, false),
            (Some((tab, 2)), tab, true, true, false, true, 2, false),
            (Some((tab, 2)), tab, true, true, false, false, 3, false),
        ];
        for (pending, active, usable, controller, discarded, probed, epoch, due) in table {
            assert_eq!(
                reader_probe_due(pending, active, usable, controller, discarded, probed, epoch),
                due,
                "{pending:?} active {active:?} usable {usable} controller {controller} \
                 discarded {discarded} probed {probed} epoch {epoch}"
            );
        }
    }

    #[test]
    fn workspace_save_mode_table() {
        use WorkspaceChange::*;
        use WorkspaceSave::*;
        let table = [
            (Create, Now),
            (Delete, Now),
            (Rename, Now),
            (Move, Now),
            (Icon, Now),
            (Colour, Now),
            (SwitchWorkspace, Deferred),
            (TabSelect, Deferred),
            (TabOpen, Deferred),
            (TabClose, Deferred),
            (TabMove, Deferred),
        ];
        for (change, mode) in table {
            assert_eq!(workspace_save_mode(change), mode, "{change:?}");
        }
    }

    #[test]
    fn config_stamp_of_a_missing_file_is_empty() {
        assert_eq!(config_stamp(None), (None, 0));
    }

    #[test]
    fn search_config_changed_compares_both_stamps() {
        let stamp = |len| (None, len);
        let base = [stamp(1), stamp(2)];
        assert!(search_config_changed(None, base));
        assert!(!search_config_changed(Some(base), base));
        assert!(search_config_changed(Some(base), [stamp(3), stamp(2)]));
        assert!(search_config_changed(Some(base), [stamp(1), stamp(3)]));
    }

    #[test]
    fn search_config_stamp_follows_a_rewritten_file() {
        let dir = std::env::temp_dir().join(format!("bowser-config-stamp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.txt");
        let provider = dir.join("search-provider.txt");
        let stamps = || {
            [
                config_stamp(std::fs::metadata(&settings).ok().as_ref()),
                config_stamp(std::fs::metadata(&provider).ok().as_ref()),
            ]
        };

        let missing = stamps();
        assert_eq!(missing, [(None, 0), (None, 0)]);

        std::fs::write(&settings, "search_engine=duckduckgo\n").unwrap();
        let written = stamps();
        assert!(search_config_changed(Some(missing), written));
        assert!(!search_config_changed(Some(written), stamps()));

        std::fs::write(&settings, "search_engine=duckduckgo\nsleep_enabled=false\n").unwrap();
        assert!(search_config_changed(Some(written), stamps()));

        let _ = std::fs::remove_dir_all(&dir);
    }

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
        let normal = efficiency::NORMAL_SUSPEND_AFTER;
        assert!(!hidden_tab_suspend_eligible(Duration::from_secs(29), normal));
        assert!(hidden_tab_suspend_eligible(Duration::from_secs(30), normal));
        assert!(hidden_tab_suspend_eligible(Duration::from_secs(31), normal));
    }

    #[test]
    fn efficient_suspension_starts_at_ten_seconds() {
        let efficient = efficiency::EFFICIENT_SUSPEND_AFTER;
        assert!(!hidden_tab_suspend_eligible(Duration::from_secs(9), efficient));
        assert!(hidden_tab_suspend_eligible(Duration::from_secs(10), efficient));
    }

    #[test]
    fn tracking_prevention_request_follows_the_setting_but_not_benchmarks() {
        use crate::settings::TrackingLevel::{Balanced, Strict};
        assert_eq!(
            tracking_prevention_request(None, false, Balanced),
            Some(TrackingPreventionLevel::Balanced),
            "ordinary launches state Balanced explicitly"
        );
        assert_eq!(
            tracking_prevention_request(None, false, Strict),
            Some(TrackingPreventionLevel::Strict)
        );
        assert_eq!(
            tracking_prevention_request(None, true, Strict),
            None,
            "benchmark launches keep the engine default"
        );
        assert_eq!(
            tracking_prevention_request(Some(TrackingPreventionLevel::Balanced), true, Strict),
            Some(TrackingPreventionLevel::Balanced),
            "an explicit benchmark level wins"
        );
        assert_eq!(
            crate::settings::Settings::default().tracking_prevention,
            Balanced,
            "Strict is never the default"
        );
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
            private: false,
        };
        assert!(tab_discard_eligible(eligible, || false));
        let excluded = [
            (
                "private",
                DiscardCandidate {
                    private: true,
                    ..eligible
                },
            ),
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
    fn efficiency_shortens_the_sleep_delay_but_keeps_the_eligibility_rules() {
        // Hidden for 150 s with a 300 s delay: not due normally, due once efficiency caps it at 120 s.
        let hidden_150 = |app: &AppRef, id: TabId| {
            hidden_web_tab(
                app,
                id,
                "https://hidden.example.test/",
                Instant::now() - Duration::from_secs(150),
            );
        };
        let (app, hidden, _active, calls) = sleeping_app(300);
        hidden_150(&app, hidden);
        App::discard_one_hidden_tab_if(&app, || false);
        assert!(closed_tabs(&calls).is_empty(), "normal delay not reached");

        app.borrow_mut().efficiency.active = true;
        App::discard_one_hidden_tab_if(&app, || false);
        assert_eq!(closed_tabs(&calls), [format!("close:{}", hidden.0)]);

        // Pinned, audible, loading and sleep-off tabs stay awake even when efficient.
        let (app, hidden, _active, calls) = sleeping_app(300);
        hidden_150(&app, hidden);
        app.borrow_mut().efficiency.active = true;
        app.borrow_mut().tab_model.set_pinned(hidden, true);
        App::discard_one_hidden_tab_if(&app, || false);
        assert!(closed_tabs(&calls).is_empty(), "pinned");

        let (app, hidden, _active, calls) = sleeping_app(300);
        hidden_150(&app, hidden);
        app.borrow_mut().efficiency.active = true;
        app.borrow_mut().tabs.iter_mut().find(|t| t.id == hidden).unwrap().loading = true;
        App::discard_one_hidden_tab_if(&app, || false);
        assert!(closed_tabs(&calls).is_empty(), "loading");

        let (app, hidden, _active, calls) = sleeping_app(300);
        hidden_150(&app, hidden);
        app.borrow_mut().efficiency.active = true;
        app.borrow_mut().engine = Some(Box::new(TabRecordingEngine {
            calls: calls.clone(),
            playing: vec![hidden],
        }));
        App::discard_one_hidden_tab_if(&app, || false);
        assert!(closed_tabs(&calls).is_empty(), "audible");

        let (app, hidden, _active, calls) = sleeping_app(300);
        hidden_150(&app, hidden);
        app.borrow_mut().efficiency.active = true;
        app.borrow_mut().settings.sleep_enabled = false;
        App::discard_one_hidden_tab_if(&app, || false);
        assert!(closed_tabs(&calls).is_empty(), "the user turned sleep off");
        assert_eq!(App::sleep_wait_ms(&app), None);
    }

    #[test]
    fn efficiency_moves_the_suspend_timer_from_thirty_to_ten_seconds() {
        let (app, hidden, _active, _calls) = sleeping_app(300);
        {
            let mut state = app.borrow_mut();
            let tab = state.tabs.iter_mut().find(|tab| tab.id == hidden).unwrap();
            tab.hidden_since = Some(Instant::now());
            tab.suspended = false;
        }
        let IdleTimerAction::Arm(normal) =
            App::idle_and_suspend_timer(&app, IdleTimerAction::Kill)
        else {
            panic!("a hidden tab that is not suspended arms the timer");
        };
        assert!((29_000..=30_000).contains(&normal), "{normal}");
        app.borrow_mut().efficiency.active = true;
        let IdleTimerAction::Arm(efficient) =
            App::idle_and_suspend_timer(&app, IdleTimerAction::Kill)
        else {
            panic!("a hidden tab that is not suspended arms the timer");
        };
        assert!((9_000..=10_000).contains(&efficient), "{efficient}");
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
            used: 0,
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
                stacked: true,
                ..Default::default()
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
        assert_eq!(split.orientation, crate::split_view::Orientation::Stacked);
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

    /// Mirrors the WebView2 engine's tab bookkeeping: `set_visible_tabs` registers only the panes
    /// that have a page, and navigating a tab that was never registered is `E_POINTER`.
    struct SplitRegistrationEngine {
        known: Rc<RefCell<std::collections::HashSet<TabId>>>,
        visible: Rc<RefCell<std::collections::HashSet<TabId>>>,
        navigated: Rc<RefCell<Vec<TabId>>>,
    }

    impl EngineAdapter for SplitRegistrationEngine {
        fn navigate(&mut self, _url: &url::Url) -> windows::core::Result<()> {
            Ok(())
        }
        fn navigate_tab(&mut self, tab: TabId, _url: &url::Url) -> windows::core::Result<()> {
            self.navigated.borrow_mut().push(tab);
            if self.known.borrow().contains(&tab) && self.visible.borrow().contains(&tab) {
                Ok(())
            } else {
                Err(windows::core::Error::from(
                    windows::Win32::Foundation::E_POINTER,
                ))
            }
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
            self.known.borrow_mut().insert(tab);
            self.visible.borrow_mut().clear();
            self.visible.borrow_mut().insert(tab);
            Ok(())
        }
        fn set_visible_tabs(
            &mut self,
            panes: &[(TabId, windows::Win32::Foundation::RECT)],
            _active: TabId,
        ) -> windows::core::Result<()> {
            let mut visible = self.visible.borrow_mut();
            visible.clear();
            let mut known = self.known.borrow_mut();
            for (tab, _) in panes {
                known.insert(*tab);
                visible.insert(*tab);
            }
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
    fn new_tab_opened_while_a_split_is_shown_is_registered_and_visible_before_search() {
        let app = app_with_pending(rect(800, 600));
        let known = Rc::new(RefCell::new(std::collections::HashSet::new()));
        let visible = Rc::new(RefCell::new(std::collections::HashSet::new()));
        let navigated = Rc::new(RefCell::new(Vec::new()));
        app.borrow_mut().engine = Some(Box::new(SplitRegistrationEngine {
            known: known.clone(),
            visible: visible.clone(),
            navigated: navigated.clone(),
        }));
        let left = App::active_tab_id(&app);
        let right = App::new_tab(&app);
        assert!(App::open_split_view(
            &app,
            left,
            right,
            crate::split_view::Pane::Left,
            (rect(0, 0), rect(400, 0)),
        ));

        let fresh = App::new_tab(&app);

        assert!(App::split_view(&app).is_none());
        assert_eq!(App::active_tab_id(&app), fresh);
        assert!(
            known.borrow().contains(&fresh),
            "a blank tab opened from a split must be registered before it is searched"
        );
        assert!(
            visible.borrow().contains(&fresh),
            "the active blank tab must be visible, or its page is created hidden"
        );

        let search = url::Url::parse("https://html.duckduckgo.com/html/?q=beans&k1=-1").unwrap();
        App::navigate_tab(&app, fresh, &search);
        assert_eq!(*navigated.borrow(), [fresh]);

        assert!(App::close_tab(&app, left));
        App::navigate_tab(&app, left, &search);
        assert_eq!(
            *navigated.borrow(),
            [fresh],
            "a closed tab is not navigated, so the engine does not recreate it"
        );
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
    fn ctrl_tab_mru_swaps_to_the_last_active_tab_and_can_be_turned_off() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        let third = App::new_tab(&app);
        assert!(app.borrow().settings.ctrl_tab_mru, "on by default");

        // Visit first, then second (recording second's predecessor, first, in the MRU history);
        // from third, Ctrl+Tab should swap back to second, the tab last left, not strip-order first.
        assert!(App::select_tab(&app, first));
        assert!(App::select_tab(&app, second));
        assert!(App::select_tab(&app, third));
        assert_eq!(App::select_next(&app), second);
        assert_eq!(App::active_tab_id(&app), second);
        // Pressing it again flips back to third: the swap alternates between the last two tabs.
        assert_eq!(App::select_prev(&app), third);

        app.borrow_mut().settings.ctrl_tab_mru = false;
        assert_eq!(App::active_tab_id(&app), third);
        assert_eq!(App::select_next(&app), first, "off: plain strip-order cycling");
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
    fn a_tab_in_another_workspace_never_joins_or_forms_a_group() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let home = app.borrow().workspaces.active_id();
        let work = App::create_workspace(&app, "Work").unwrap();
        let other = App::active_tab_id(&app);
        set_addresses(
            &app,
            &[
                (first, "https://a.example.com/"),
                (other, "https://b.example.com/"),
            ],
        );

        // The two tabs share a site but live in different workspaces, so neither is grouped.
        assert!(!App::tab_is_grouped(&app, other));
        assert_eq!(App::tab_group_inputs(&app).len(), 1);
        assert!(App::switch_workspace(&app, home));
        assert!(!App::tab_is_grouped(&app, first));

        // A second tab of the site in the same workspace does group, without the other workspace's.
        assert!(App::switch_workspace(&app, work));
        let third = App::new_tab(&app);
        set_addresses(&app, &[(third, "https://c.example.com/")]);
        assert!(App::tab_is_grouped(&app, other));
        assert!(App::tab_is_grouped(&app, third));
        assert!(App::switch_workspace(&app, home));
        assert!(!App::tab_is_grouped(&app, first));
    }

    #[test]
    fn search_result_tabs_are_not_grouped_by_their_search_engine() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        let third = App::new_tab(&app);
        set_addresses(
            &app,
            &[
                (first, "https://duckduckgo.com/?q=beans"),
                (second, "https://www.google.com/search?q=rust"),
                (third, "https://www.google.com/search?q=cats"),
            ],
        );
        for id in [first, second, third] {
            assert!(!App::tab_is_grouped(&app, id));
        }

        // Other pages of the same site still group.
        set_addresses(
            &app,
            &[
                (second, "https://mail.google.com/mail/u/0/"),
                (third, "https://docs.google.com/document/d/1/edit"),
            ],
        );
        assert!(App::tab_is_grouped(&app, second));
        assert!(App::tab_is_grouped(&app, third));
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
        fn set_tab_private(&mut self, tab: TabId) {
            self.calls.borrow_mut().push(format!("private:{}", tab.0));
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

    #[test]
    fn reader_view_works_in_a_private_tab_and_its_settings_stay_in_memory() {
        let app = app_with_pending(rect(800, 600));
        let private = App::new_private_tab(&app);
        assert!(App::tab_private(&app, private));
        {
            let mut state = app.borrow_mut();
            assert!(state.site_prefs.is_none(), "this test app has no on-disk store");
            let tab = state.tabs.iter_mut().find(|tab| tab.id == private).unwrap();
            tab.url = Some(url::Url::parse("https://example.test/article").unwrap());
        }
        let prefs = crate::reader::Prefs { theme: crate::reader::Theme::Sepia, size: 24 };
        let outcome = crate::reader::Outcome {
            code: crate::reader::Code::Ok,
            state: crate::reader::Active::Reader,
            words: 900,
            link_percent: 8,
            prefs,
            lang: crate::translate::Lang::NONE,
        };
        let epoch = App::reader_state(&app, private).expect("reader may script a private tab").epoch;
        let result = App::finish_reader_call(&app, private, epoch, Some(outcome), false);
        assert!(matches!(result, ReaderResult::Applied(crate::reader::Active::Reader)));
        assert_eq!(App::reader_active(&app, private), crate::reader::Active::Reader);
        // The reader theme and size live in the App for the session. Nothing hands them to a
        // store: no site-prefs store exists here, and the reader module has no file access.
        assert_eq!(app.borrow().reader_prefs, prefs);
        assert!(app.borrow().site_prefs.is_none());
    }

    #[test]
    fn a_private_tab_is_marked_private_for_the_engine_before_it_is_first_selected() {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        app.borrow_mut().engine = Some(Box::new(TabRecordingEngine {
            calls: calls.clone(),
            playing: Vec::new(),
        }));
        let normal = App::active_tab_id(&app);
        let private = App::new_private_tab(&app);
        let plain = App::new_tab(&app);
        assert!(!App::tab_private(&app, normal));
        assert!(App::tab_private(&app, private));
        assert!(!App::tab_private(&app, plain));
        assert_eq!(
            *calls.borrow(),
            [
                format!("private:{}", private.0),
                format!("select:{}", private.0),
                format!("select:{}", plain.0),
            ],
            "the mark precedes the first select, and normal tabs are never marked"
        );
        // Selecting the private tab makes it the active private tab.
        assert!(App::select_tab(&app, private));
        assert!(App::active_tab_private(&app));
        // Closing the last private tab leaves ordinary tabs, and a replacement tab is not private.
        assert!(App::close_tab(&app, private));
        assert!(!App::active_tab_private(&app));
    }

    #[test]
    fn closing_the_only_private_tab_in_a_window_replaces_it_with_a_normal_tab() {
        let app = app_with_pending(rect(800, 600));
        let only = App::active_tab_id(&app);
        app.borrow_mut()
            .tabs
            .iter_mut()
            .find(|tab| tab.id == only)
            .unwrap()
            .private = true;
        assert!(App::tab_private(&app, only));
        assert!(App::close_tab(&app, only));
        let fresh = App::active_tab_id(&app);
        assert_ne!(fresh, only);
        assert!(!App::tab_private(&app, fresh));
    }

    #[test]
    fn close_all_private_tabs_closes_only_the_private_ones() {
        let app = app_with_pending(rect(800, 600));
        let normal = App::active_tab_id(&app);
        let first = App::new_private_tab(&app);
        let other = App::new_tab(&app);
        let second = App::new_private_tab(&app);
        assert_eq!(App::private_tab_ids(&app), vec![first, second]);
        assert_eq!(App::close_private_tabs(&app), 2);
        assert!(App::private_tab_ids(&app).is_empty());
        let left = App::tab_ids(&app);
        assert!(left.contains(&normal) && left.contains(&other), "normal tabs stay: {left:?}");
        assert!(!left.contains(&first) && !left.contains(&second));
        // Nothing private left: a second run closes nothing.
        assert_eq!(App::close_private_tabs(&app), 0);
    }

    #[test]
    fn close_all_private_tabs_in_a_private_only_window_leaves_a_normal_tab() {
        let app = app_with_pending(rect(800, 600));
        let only = App::active_tab_id(&app);
        let private = App::new_private_tab(&app);
        assert!(App::close_tab(&app, only));
        assert_eq!(App::tab_ids(&app), vec![private]);
        assert_eq!(App::close_private_tabs(&app), 1);
        let fresh = App::active_tab_id(&app);
        assert_ne!(fresh, private);
        assert!(!App::tab_private(&app, fresh));
        assert!(App::private_tab_ids(&app).is_empty());
    }

    #[test]
    fn private_tabs_are_left_out_of_session_snapshots_and_never_join_groups() {
        let app = app_with_pending(rect(800, 600));
        let first = App::active_tab_id(&app);
        let second = App::new_tab(&app);
        let secret = App::new_private_tab(&app);
        let third = App::new_tab(&app);
        set_addresses(
            &app,
            &[
                (first, "https://example.com/a"),
                (second, "https://example.com/b"),
                (secret, "https://example.com/private-page"),
                (third, "https://example.org/c"),
            ],
        );
        App::select_tab(&app, secret);
        let snapshot = App::session_snapshot(&app).expect("normal tabs are worth saving");
        let urls: Vec<String> = snapshot.tabs.iter().map(|tab| tab.url.clone()).collect();
        assert_eq!(
            urls,
            [
                "https://example.com/a",
                "https://example.com/b",
                "https://example.org/c"
            ]
        );
        assert!(
            !snapshot.serialize().contains("private-page"),
            "{}",
            snapshot.serialize()
        );
        // The private tab shares a registrable domain with two normal tabs but is in no group.
        assert_eq!(tab_domain(&app, secret), None);
        assert!(tab_domain(&app, first).is_some());
        let inputs = App::tab_group_inputs(&app);
        let private_input = inputs.iter().find(|input| input.id == secret).unwrap();
        assert!(private_input.domain.is_none() && private_input.opener.is_none());
        // Site grouping choices are refused for a private tab.
        let rules_before = app.borrow().group_preferences.serialize();
        assert!(!App::set_tab_group_mode(
            &app,
            secret,
            crate::tab_groups::GroupMode::Always
        ));
        assert!(!App::set_tab_group_collapsed(&app, secret, true));
        assert_eq!(
            app.borrow().group_preferences.serialize(),
            rules_before,
            "no site rule was written for the private tab's site"
        );
    }

    #[test]
    fn a_window_of_only_private_tabs_has_no_session_to_save() {
        let app = app_with_pending(rect(800, 600));
        let only = App::active_tab_id(&app);
        {
            let mut state = app.borrow_mut();
            state.tabs[0].private = true;
        }
        set_addresses(&app, &[(only, "https://example.com/private-page")]);
        assert!(App::session_snapshot(&app).is_none());
    }

    #[test]
    fn a_page_opened_from_a_private_tab_stays_private() {
        let app = app_with_pending(rect(800, 600));
        let source = App::new_private_tab(&app);
        let inherit = crate::private::inherits_privacy(App::tab_private(&app, source));
        let child = App::new_tab_in_mode(&app, inherit);
        assert!(App::tab_private(&app, child));
        let normal_source = App::active_tab_id(&app);
        let _ = normal_source;
        let plain = App::new_tab_in_mode(&app, crate::private::inherits_privacy(false));
        assert!(!App::tab_private(&app, plain));
    }

    #[test]
    fn the_window_title_and_blocker_state_report_a_private_tab() {
        let app = app_with_pending(rect(800, 600));
        let private = App::new_private_tab(&app);
        app.borrow_mut()
            .tabs
            .iter_mut()
            .find(|tab| tab.id == private)
            .unwrap()
            .blocked = 4;
        assert_eq!(
            App::blocker_suffix(&app.borrow()),
            " [Private, 4 blocked]",
            "the suffix does not need the global blocker"
        );
        assert_eq!(App::blocker_active_for_site(&app), Some(true));
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

    pub(super) fn app_with_pending(bounds: windows::Win32::Foundation::RECT) -> AppRef {
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
            more_initial_urls: Vec::new(), // --- tab stacks (T10) ---
            allowed_benchmark_file: None,
            benchmark_tracking_prevention: None,
            benchmark_memory_pressure: false,
            provider: SearchProvider::from_config(None, Path::new("missing-search-provider.txt")),
            search_config_stamp: None,
            log: Rc::new(PerfLog::new(None).unwrap()),
            minimized: false,
            efficiency: EfficiencyState::default(),
            desired_memory_target: MemoryUsageTarget::Normal,
            effective_memory_target: None,
            memory_target_unsupported_logged: false,
            last_valid_viewport_bounds: Some(bounds),
            has_navigated: false,
            page_loaded: false,
            navigation_progress: NavigationProgress::default(),
            tabs: vec![AppTab::empty(TabId(0))],
            tab_model: crate::tabs::TabList::new(),
            tab_mru: Vec::new(),
            reader_probe_pending: None,
            feed_pages: HashMap::new(),
            split_view: None,
            split_parked: None,
            recently_closed: std::collections::VecDeque::new(),
            tab_state: crate::tab_state::TabState::default(), // --- tab options ---
            split_refresh: false,
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
            workspaces_dirty: false,
            // Existing tests exercise the memory-pressure path; tab sleep has its own tests.
            settings: crate::settings::Settings {
                sleep_enabled: false,
                ..Default::default()
            },
            settings_synced: crate::settings::Settings::default(),
            session: None,
            session_save_pending: false,
            last_session_snapshot: None,
            pending_restore: None,
            privacy_stats: None,
            privacy_stats_path: None,
            privacy_session_blocked: 0,
            blocker: None,
            private_blocker: None,
            permissions: None,
            site_prefs: None,
            denied: crate::permissions::DeniedLog::default(),
            reader_prefs: crate::reader::Prefs::default(),
            translate: page_translate::Runtime::default(),
            downloads: crate::downloads::DownloadManager::in_memory(false),
            downloads_observer: None,
            workspace_rules: Vec::new(), // --- tab stacks (T10) ---
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
        assert_eq!(decision.timer, IdleTimerAction::Kill);
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
    fn the_idle_timer_does_not_stay_repeating_after_memory_decision_is_stable() {
        let counting = idle_memory_decision(false, true, 2_000, MemoryUsageTarget::Normal);
        assert_eq!(counting.target, MemoryUsageTarget::Normal);
        assert_eq!(counting.timer, IdleTimerAction::Arm(1_000));

        let trimmed = idle_memory_decision(false, true, 3_000, MemoryUsageTarget::Normal);
        assert_eq!(trimmed.target, MemoryUsageTarget::Low);
        assert_eq!(trimmed.timer, IdleTimerAction::Kill);

        let stable_low = idle_memory_decision(false, true, 10_000, MemoryUsageTarget::Low);
        assert_eq!(stable_low.target, MemoryUsageTarget::Low);
        assert_eq!(stable_low.timer, IdleTimerAction::Kill);

        let active = idle_memory_decision(false, true, 0, MemoryUsageTarget::Low);
        assert_eq!(active.target, MemoryUsageTarget::Normal);
        assert_eq!(active.timer, IdleTimerAction::Arm(3_000));

        let minimized = idle_memory_decision(true, true, 0, MemoryUsageTarget::Normal);
        assert_eq!(minimized.target, MemoryUsageTarget::Low);
        assert_eq!(minimized.timer, IdleTimerAction::Kill);
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
            IdleTimerAction::Kill
        );
        assert_eq!(
            App::idle_timer_fired(&app, 2_000, true),
            IdleTimerAction::Kill
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

    // --- auto-hide (T11) ---
    /// The page rectangle the app sizes a tab to is the one the window lays out, for every layout, DPI,
    /// window state and set of hidden bars: switching auto-hide on or off resizes the page to it.
    #[test]
    fn the_apps_page_rectangle_is_the_windows_for_every_set_of_hidden_bars() {
        use crate::auto_hide::{plan, Bars, ALL_BARS, BAR_TABLE};
        use crate::chrome::layout::{self, AutoHide};
        let mut sets = vec![Bars::NONE, ALL_BARS];
        sets.extend(BAR_TABLE.iter().map(|(_, bar)| *bar));
        for tab_layout in crate::settings::TabLayout::ALL {
            for dpi in [96, 144, 192] {
                for maximised in [false, true] {
                    for hidden in &sets {
                        let hide = plan(*hidden, tab_layout).layout();
                        let context = format!("{tab_layout:?} dpi {dpi} max {maximised} hidden {}", hidden.text());
                        let app = chrome_viewport_bounds_hiding(1600, 1000, dpi, 3, maximised, tab_layout, 260, 0, hide);
                        // --- layout (thread T6) --- the window lays every layout out with the arranged geometry.
                        let arrangement = layout::Arrangement { tabs: tab_layout, ..Default::default() };
                        let row_h = crate::layout_prefs::UiDensity::Regular.sidebar_row_height();
                        let window = layout::compute_arranged_auto_hide(arrangement, 1600, 1000, dpi, 3, 0, maximised, 260, true, row_h, hide).0.viewport;
                        if matches!(tab_layout, crate::settings::TabLayout::ToolbarTop | crate::settings::TabLayout::TabsTop | crate::settings::TabLayout::SingleRow) {
                            let plain = layout::compute_in_frame_auto_hide(tab_layout, 1600, 1000, dpi, 3, 0, maximised, true, hide).viewport;
                            assert_eq!(window, plain, "{context}: the arranged geometry is the plain one");
                        } else if tab_layout == crate::settings::TabLayout::Vertical {
                            let plain = layout::compute_vertical_in_frame_auto_hide(1600, 1000, dpi, 3, 0, maximised, 260, true, hide).0.viewport;
                            assert_eq!(window, plain, "{context}: the arranged geometry is the plain one");
                        }
                        assert_eq!((app.left, app.top, app.right, app.bottom), (window.left, window.top, window.right, window.bottom), "{context}");
                        if hide == AutoHide::OFF {
                            let before = chrome_viewport_bounds(1600, 1000, dpi, 3, maximised, tab_layout, 260, 0);
                            assert_eq!((app.left, app.top, app.right, app.bottom), (before.left, before.top, before.right, before.bottom), "{context}: off is as it was");
                        }
                    }
                }
            }
        }
    }

    /// The saved settings decide what hides: off hides nothing, on hides the chosen bars, full screen all.
    #[test]
    fn the_saved_settings_decide_which_part_of_the_window_the_page_may_use() {
        use crate::auto_hide::{effective, plan, AutoHideSettings, Bars, Fullscreen};
        let layout_of = |frameless: bool, bars: &str, fullscreen: Fullscreen, in_fullscreen: bool| {
            let settings = AutoHideSettings { bars: Bars::parse(bars).unwrap(), in_fullscreen };
            plan(effective(frameless, &settings, fullscreen).hidden, crate::settings::TabLayout::Vertical).layout()
        };
        use crate::chrome::layout::AutoHide;
        assert_eq!(layout_of(false, "tabs,address", Fullscreen::No, true), AutoHide::OFF);
        assert_eq!(layout_of(true, "tabs", Fullscreen::No, true), AutoHide { top: false, sidebar: true });
        assert_eq!(layout_of(true, "address", Fullscreen::No, true), AutoHide { top: true, sidebar: false });
        assert_eq!(layout_of(true, "bookmarks,panel", Fullscreen::No, true), AutoHide::OFF, "those two are not part of the page rectangle");
        assert_eq!(layout_of(false, "", Fullscreen::Window, true), AutoHide { top: true, sidebar: true });
        assert_eq!(layout_of(false, "", Fullscreen::Window, false), AutoHide::OFF);
        assert_eq!(layout_of(false, "", Fullscreen::Page, false), AutoHide { top: true, sidebar: true });
    }
    // --- end auto-hide ---

    #[test]
    fn webview_bounds_match_chrome_viewport_in_physical_pixels() {
        let bounds = chrome_viewport_bounds(
            1200,
            800,
            144,
            3,
            true,
            crate::settings::TabLayout::ToolbarTop,
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
    fn webview_bounds_start_where_each_layouts_chrome_ends() {
        use crate::settings::TabLayout;
        for dpi in [96, 120, 144, 192] {
            for tab_layout in TabLayout::ALL {
                let bounds = chrome_viewport_bounds(1600, 1000, dpi, 3, true, tab_layout, 260, 0);
                assert_eq!(bounds.top, crate::chrome::layout::chrome_height(tab_layout, dpi), "dpi={dpi}, layout={tab_layout:?}");
                if !tab_layout.is_vertical() {
                    let viewport = crate::chrome::layout::compute_in_frame_for(tab_layout, 1600, 1000, dpi, 3, 0, true).viewport;
                    assert_eq!((bounds.left, bounds.top, bounds.right, bounds.bottom), (viewport.left, viewport.top, viewport.right, viewport.bottom), "dpi={dpi}, layout={tab_layout:?}");
                }
            }
            // Single row and the sidebar layout give the page the 30 px a tab row would have taken.
            let top = |layout| chrome_viewport_bounds(1600, 1000, dpi, 3, true, layout, 260, 0).top;
            assert!(top(TabLayout::SingleRow) < top(TabLayout::ToolbarTop), "dpi={dpi}");
            assert_eq!(top(TabLayout::Vertical), top(TabLayout::SingleRow), "dpi={dpi}");
            assert_eq!(top(TabLayout::TabsTop), top(TabLayout::ToolbarTop), "dpi={dpi}");
        }
    }

    #[test]
    fn webview_bounds_reserve_rail_sidebar_and_panel_in_every_tab_layout() {
        for dpi in [96, 120, 144, 192] {
            let inset = crate::chrome::layout::left_rail_inset(dpi);
            for tab_layout in crate::settings::TabLayout::ALL {
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
    fn session_timer_arms_only_without_a_queued_save() {
        assert!(session_arm_allowed(true, false, false));
        assert!(!session_arm_allowed(false, false, false));
        assert!(!session_arm_allowed(true, true, false), "a queued save builds no snapshot");
        assert!(!session_arm_allowed(true, false, true));
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
