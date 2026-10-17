pub mod download;
pub mod web_app;
mod extension_page;
mod webview2;
#[cfg(test)]
mod extension_probe_live;
pub(crate) use webview2::password_login_messages_enabled;

pub use webview2::{RuntimeUnavailable, WebView2Engine};

/// Pause, resume and cancel act on the operation the `Started` event named; they work for tab and
/// web app downloads alike (see `download`).
pub use download::{cancel as cancel_download, pause as pause_download, resume as resume_download};

use windows::Win32::Foundation::RECT;

pub type FaviconChangedCallback = std::rc::Rc<dyn Fn(crate::tabs::TabId, Vec<u8>)>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackingPreventionLevel {
    Balanced,
    Strict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryUsageTarget {
    Low,
    Normal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryTargetApply {
    Applied,
    Unsupported,
    Deferred,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowsingDataKind {
    SiteData,
    Cache,
    History,
    // --- Delete Browsing Data (thread T3) ---
    /// Cookies alone.
    Cookies,
    /// Local storage, IndexedDB and the other DOM storage, service workers and cache storage.
    Storage,
    /// Form autofill entries (names, addresses and the like) the browser engine saved.
    Autofill,
    /// The engine's per-site settings (its own saved permissions and content settings).
    SiteSettings,
    // --- end Delete Browsing Data ---
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowsingDataRange {
    LastHour,
    LastDay,
    LastSevenDays,
    // --- Delete Browsing Data (thread T3) ---
    LastFourWeeks,
    // --- end Delete Browsing Data ---
    AllTime,
}

impl BrowsingDataRange {
    /// How far back the range reaches, in seconds, or `None` for everything.
    pub fn seconds(self) -> Option<u64> {
        match self {
            Self::LastHour => Some(60 * 60),
            Self::LastDay => Some(24 * 60 * 60),
            Self::LastSevenDays => Some(7 * 24 * 60 * 60),
            Self::LastFourWeeks => Some(28 * 24 * 60 * 60),
            Self::AllTime => None,
        }
    }
}

pub trait EngineAdapter {
    fn navigate(&mut self, url: &url::Url) -> windows::core::Result<()>;
    fn navigate_tab(
        &mut self,
        _tab: crate::tabs::TabId,
        url: &url::Url,
    ) -> windows::core::Result<()> {
        self.navigate(url)
    }
    /// Navigates a tab by POSTing `body` (percent-encoded form fields) to an https address. Used for
    /// searches with an engine that takes its query that way. Engines that cannot refuse.
    fn navigate_tab_post(
        &mut self,
        _tab: crate::tabs::TabId,
        _url: &url::Url,
        _body: &str,
    ) -> windows::core::Result<()> {
        Err(windows::core::Error::from(windows::Win32::Foundation::E_NOTIMPL))
    }
    /// Executes a native-requested script only if the live top-level origin still matches.
    fn execute_script_for_origin(
        &mut self,
        _tab: crate::tabs::TabId,
        _origin: &str,
        _script: &str,
    ) -> windows::core::Result<()> {
        Err(windows::core::Error::from(
            windows::Win32::Foundation::E_NOTIMPL,
        ))
    }
    /// Runs one reader-view script (see `crate::reader`) in a tab and hands back its raw result: the
    /// JSON text `ExecuteScript` produced. Only an http(s) page is ever scripted, checked against
    /// the live address at the moment of the call; anything else, a missing page, a failed call
    /// or an oversized answer reports `None`. `done` may run before this returns.
    fn run_reader_script(
        &mut self,
        _tab: crate::tabs::TabId,
        _script: &str,
        done: Box<dyn FnOnce(Option<String>)>,
    ) {
        done(None);
    }
    /// Runs one page-translation script (see `crate::translate`) in a tab and hands back its raw
    /// result, under the same rules as `run_reader_script` (http(s) pages only, checked at the moment
    /// of the call) but with a larger answer limit, because the answer carries a batch of text.
    fn run_translate_script(
        &mut self,
        _tab: crate::tabs::TabId,
        _script: &str,
        done: Box<dyn FnOnce(Option<String>)>,
    ) {
        done(None);
    }
    /// Runs the feed-detection script (`app::feed_features`) in a tab and hands back its raw result,
    /// under the same rules as `run_reader_script` (http(s) pages only, checked at the moment of the
    /// call) with an answer limit of a few kilobytes: the answer is a short list of `<link>` tags.
    fn run_feed_script(
        &mut self,
        _tab: crate::tabs::TabId,
        _script: &str,
        done: Box<dyn FnOnce(Option<String>)>,
    ) {
        done(None);
    }
    /// Checks for a visible, currently playing video without installing page scripts or polling.
    fn picture_in_picture_eligible(
        &mut self,
        _tab: crate::tabs::TabId,
        done: Box<dyn FnOnce(bool)>,
    ) {
        done(false);
    }
    /// Toggles Chromium's native video PiP for the selected tab, if supported by its runtime.
    fn toggle_picture_in_picture(&mut self, _tab: crate::tabs::TabId) {}
    /// Creates the active tab's controller without starting a navigation.
    /// Marks a tab private before its controller exists: it is then created InPrivate in the
    /// shared private profile (see `crate::private`). Must be called before the tab is first
    /// selected or navigated; a tab that already has a controller is unaffected.
    fn set_tab_private(&mut self, _tab: crate::tabs::TabId) {}
    fn prewarm_tab(&mut self, _tab: crate::tabs::TabId) -> windows::core::Result<()> {
        Ok(())
    }
    fn can_go_back(&self) -> bool;
    fn go_back(&mut self) -> windows::core::Result<()>;
    fn can_go_forward(&self) -> bool;
    fn go_forward(&mut self) -> windows::core::Result<()>;
    /// `(can_go_back, can_go_forward)` from one tab's real session history, or `None` when the
    /// tab has no live page. The toolbar's back/forward buttons follow this.
    fn tab_history(&self, _tab: crate::tabs::TabId) -> Option<(bool, bool)> {
        None
    }
    fn resize(&mut self, bounds: RECT) -> windows::core::Result<()>;
    fn reload(&mut self) -> windows::core::Result<()> {
        Ok(())
    }
    /// Reloads a particular tab without changing the active tab.
    fn reload_tab(&mut self, _tab: crate::tabs::TabId) -> windows::core::Result<()> {
        self.reload()
    }
    fn stop(&mut self) -> windows::core::Result<()> {
        Ok(())
    }
    /// Moves keyboard focus into the active tab's page, out of whatever control had it.
    fn focus_page(&mut self) -> windows::core::Result<()> {
        Ok(())
    }
    /// Opens the page DevTools window for the active tab. Does nothing when the tab has no live
    /// page, so the user never sees a failure on a blank or internal tab.
    fn open_dev_tools(&mut self) -> windows::core::Result<()> {
        Ok(())
    }
    fn set_visible(&mut self, _visible: bool) -> windows::core::Result<()> {
        Ok(())
    }
    /// Selects a logical tab, hiding the previous controller and showing/resuming the new one.
    fn select_tab(&mut self, _tab: crate::tabs::TabId) -> windows::core::Result<()> {
        Ok(())
    }
    /// Reconciles the controllers that should currently be visible, including their client bounds.
    /// The active tab remains the command target. Engines that support only one visible controller
    /// retain the active pane as a normal view.
    fn set_visible_tabs(
        &mut self,
        panes: &[(crate::tabs::TabId, RECT)],
        active: crate::tabs::TabId,
    ) -> windows::core::Result<()> {
        self.select_tab(active)?;
        if let Some((_, bounds)) = panes.iter().find(|(tab, _)| *tab == active) {
            self.resize(*bounds)?;
        }
        Ok(())
    }
    /// Closes the controller owned by one logical tab.
    fn close_tab(&mut self, _tab: crate::tabs::TabId) -> windows::core::Result<()> {
        self.close()
    }
    /// Reports whether a tab's document is playing audio. Unknown counts as silent.
    fn tab_playing_audio(&self, _tab: crate::tabs::TabId) -> bool {
        false
    }
    /// Whether one tab's WebView is muted, when the engine exposes audio controls.
    fn tab_muted(&self, _tab: crate::tabs::TabId) -> bool {
        false
    }
    fn supports_tab_muting(&self, _tab: crate::tabs::TabId) -> bool {
        false
    }
    /// Mutes or unmutes one tab. Engines without per-tab audio controls ignore the request.
    fn set_tab_muted(
        &mut self,
        _tab: crate::tabs::TabId,
        _muted: bool,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    /// Captures the tab's page as a JPEG no larger than `max_w` x `max_h` (a thumbnail for the tab
    /// hover card). Only a tab that is on screen can be captured; any other, and any failure, reports
    /// `None`. `done` may run later. Nothing is stored by the engine.
    fn capture_tab_thumbnail(
        &mut self,
        _tab: crate::tabs::TabId,
        _max_w: u32,
        _max_h: u32,
        done: Box<dyn FnOnce(Option<Vec<u8>>)>,
    ) {
        done(None);
    }
    /// The working set in bytes of the page process of each tab that can be told apart (the tab hover
    /// card's memory line). Tabs the engine cannot match to a process are left out. `done` may run later.
    fn tab_memory_usage(&mut self, done: Box<dyn FnOnce(Vec<(crate::tabs::TabId, u64)>)>) {
        done(Vec::new());
    }
    /// Asks whether sleeping a hidden tab could lose the user's work or interrupt media: unsaved
    /// form input, playing video, picture-in-picture or a live camera/microphone stream.
    /// `done(true)` means "keep it awake". It may run later (after the page answers) but never
    /// reports page data, only the verdict. Engines that cannot ask report `false` at once.
    fn check_sleep_blocked(&mut self, _tab: crate::tabs::TabId, done: Box<dyn FnOnce(bool)>) {
        done(false);
    }
    /// Suspends controllers that have been hidden for at least the supplied duration.
    fn suspend_hidden_tabs(&mut self, _hidden_for_ms: u64) -> windows::core::Result<()> {
        Ok(())
    }
    /// Holds every hidden controller at the low memory target. Hiding a tab already does this, so
    /// the call only matters when efficiency mode switches on while tabs are already in the
    /// background. Returns how many tabs were set.
    fn force_background_memory_low(&mut self) -> usize {
        0
    }
    /// Suspends one already hidden controller in response to an explicit user request.
    fn suspend_tab(&mut self, _tab: crate::tabs::TabId) -> windows::core::Result<()> {
        Ok(())
    }
    fn suspend(&mut self) {}
    fn set_memory_usage_target(
        &mut self,
        target: MemoryUsageTarget,
    ) -> windows::core::Result<MemoryTargetApply>;
    fn close(&mut self) -> windows::core::Result<()>;
    /// Loads an installed unpacked extension (`enabled`) or disables a loaded one. `done` receives
    /// the outcome; it may run later. Engines without extension support report an error when
    /// enabling and succeed trivially when disabling.
    fn set_extension_enabled(
        &mut self,
        _id: &str,
        _folder: &std::path::Path,
        enabled: bool,
        done: Box<dyn FnOnce(Result<(), String>)>,
    ) {
        if enabled {
            done(Err(
                "This browser engine does not support extensions.".to_owned()
            ));
        } else {
            done(Ok(()));
        }
    }
    /// Unloads an extension from the running profile. `done` runs once the browser has removed it.
    fn remove_extension(&mut self, _id: &str, done: Box<dyn FnOnce(Result<(), String>)>) {
        done(Ok(()));
    }
    /// Opens a loaded extension's popup or options page (read from its manifest in `folder`) in a
    /// Bowser-owned window. `done` receives the outcome and may run later.
    fn open_extension_page(
        &mut self,
        _id: &str,
        _folder: &std::path::Path,
        _kind: crate::extensions::PageKind,
        _title: &str,
        done: Box<dyn FnOnce(Result<(), String>)>,
    ) {
        done(Err("This browser engine does not support extensions.".to_owned()));
    }
    /// Counts the cookies the browser would send to `url` from one tab. `done` may run later.
    fn site_cookie_count(
        &mut self,
        _tab: crate::tabs::TabId,
        _url: &url::Url,
        done: Box<dyn FnOnce(Result<usize, String>)>,
    ) {
        done(Err("This browser engine cannot read cookies.".to_owned()));
    }
    /// Deletes the cookies the browser would send to `url` from one tab and reports how many were
    /// removed. `done` may run later.
    fn clear_site_cookies(
        &mut self,
        _tab: crate::tabs::TabId,
        _url: &url::Url,
        done: Box<dyn FnOnce(Result<usize, String>)>,
    ) {
        done(Err("This browser engine cannot clear cookies.".to_owned()));
    }
    /// Clears one category from the active engine profile. Saved passwords are never included.
    fn clear_browsing_data(
        &mut self,
        _tab: crate::tabs::TabId,
        _kind: BrowsingDataKind,
        _range: BrowsingDataRange,
        done: Box<dyn FnOnce(Result<(), String>)>,
    ) {
        done(Err(
            "This browser engine cannot clear browsing data.".to_owned()
        ));
    }
    /// Clears several categories in one request (Delete Browsing Data). Saved passwords are never
    /// part of it: they live in Bowser's own vault.
    fn clear_browsing_data_set(
        &mut self,
        _tab: crate::tabs::TabId,
        _kinds: &[BrowsingDataKind],
        _range: BrowsingDataRange,
        done: Box<dyn FnOnce(Result<(), String>)>,
    ) {
        done(Err(
            "This browser engine cannot clear browsing data.".to_owned()
        ));
    }
    /// Applies the website look (`website_look::current`: the colour scheme sites are told and
    /// forced dark mode) to every open tab. Tabs created later start with it.
    fn apply_website_look(&mut self) {}
    /// --- layout (thread T6) --- applies the status-bar choice (`layout_prefs::status_overlay_shown`:
    /// whether a page may show the link-address overlay) to every open tab. Tabs created later
    /// start with it.
    fn apply_status_overlay(&mut self) {}
    /// The profile's tracking-prevention level as the browser reports it now, or `None` when the
    /// engine cannot say.
    fn tracking_prevention_level(&self, _tab: crate::tabs::TabId) -> Option<&'static str> {
        None
    }
    /// The tab's page zoom in percent, or `None` when it has no live page.
    fn zoom_percent(&self, _tab: crate::tabs::TabId) -> Option<u16> {
        None
    }
    /// Sets the tab's page zoom (25 to 500). `Ok(false)` when the tab has no live page to zoom.
    /// The engine reports the change through `EngineEvents::zoom_changed`.
    fn set_zoom_percent(
        &mut self,
        _tab: crate::tabs::TabId,
        _percent: u16,
    ) -> windows::core::Result<bool> {
        Ok(false)
    }
    /// Current WebView2 reputation-checking setting, or `None` when the runtime cannot report it.
    fn smart_screen_enabled(&self, _tab: crate::tabs::TabId) -> Option<bool> {
        None
    }
    // --- page tools (T7) ---
    /// Calls one DevTools method of a tab and hands back the raw answer, for page capture only: the
    /// engine refuses every method but `crate::capture::METHOD_METRICS` and `METHOD_SHOT`, never runs
    /// for a page that is not http(s) (checked against the live address at the moment of the call) and
    /// drops an answer over the capture limit. `None` is a refusal or a failure. `done` may run later.
    fn devtools_call(
        &mut self,
        _tab: crate::tabs::TabId,
        _method: &str,
        _params: &str,
        done: Box<dyn FnOnce(Option<String>)>,
    ) {
        done(None);
    }
    /// Runs one read-only page-tools script (the selection) in a tab, under the same rules as
    /// `run_reader_script` (http(s) pages only) but with the answer limit the caller names.
    fn run_page_script(
        &mut self,
        _tab: crate::tabs::TabId,
        _script: &str,
        _limit: usize,
        done: Box<dyn FnOnce(Option<String>)>,
    ) {
        done(None);
    }
    /// Applies Settings > Page tools to every open tab (spatial navigation, Ctrl+scroll zoom). Tabs
    /// created later start with them.
    fn apply_page_tools(&mut self) {}
    // --- end page tools ---
}

/// Operations against one logical tab. Implementations may share an environment across tabs.
pub trait TabEngine {
    fn navigate(&mut self, tab: crate::tabs::TabId, url: &url::Url) -> windows::core::Result<()>;
    fn reload(&mut self, tab: crate::tabs::TabId) -> windows::core::Result<()>;
    fn stop(&mut self, tab: crate::tabs::TabId) -> windows::core::Result<()>;
    fn go_back(&mut self, tab: crate::tabs::TabId) -> windows::core::Result<()>;
    fn go_forward(&mut self, tab: crate::tabs::TabId) -> windows::core::Result<()>;
    fn can_go_back(&self, tab: crate::tabs::TabId) -> bool;
    fn can_go_forward(&self, tab: crate::tabs::TabId) -> bool;
    fn set_visible(&mut self, tab: crate::tabs::TabId, visible: bool) -> windows::core::Result<()>;
    fn resize(&mut self, tab: crate::tabs::TabId, bounds: RECT) -> windows::core::Result<()>;
    fn close_tab(&mut self, tab: crate::tabs::TabId) -> windows::core::Result<()>;
}

#[derive(Clone, Default)]
pub struct EngineEvents {
    pub address_changed: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, url::Url)>>,
    pub title_changed: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, String)>>,
    pub page_loaded: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId)>>,
    pub page_usable: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId)>>,
    pub controller_ready: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId)>>,
    /// A page element went full screen or left it (a video's full-screen button). The window then
    /// fills its monitor and every bar hides (auto-hide, `src/auto_hide.rs`).
    pub fullscreen_changed: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, bool)>>,
    /// A WebView2 pane received focus from the user.
    pub focus_changed: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId)>>,
    pub navigation_started: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, u64)>>,
    pub navigation_progress:
        Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, NavigationProgressEvent)>>,
    pub favicon_changed: Option<FaviconChangedCallback>,
    /// Validated top-frame credential-submit data; never generated from a payload origin claim.
    pub login_message:
        Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, crate::passwords::LoginMessage)>>,
    pub new_window_requested: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, url::Url)>>,
    /// A link the user followed is about to load in this tab (a new document, not a redirect or a typed
    /// address). Returning true cancels it: the app opened the page in a new tab instead ("Keep pinned
    /// tabs on their site").
    pub link_navigation: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, &url::Url) -> bool>>,
    /// A tab's render process exited or crashed; the browser itself and other tabs are fine.
    pub render_process_failed: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId)>>,
    /// A blocker stopped a request; includes the current-page count and page host when known.
    pub blocked_changed: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, u32, String)>>,
    /// A top-level navigation in this tab finished with an error. The flag is true for certificate
    /// errors, which the Site information panel reports as a problem.
    pub navigation_failed: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, bool)>>,
    pub failed: Option<std::rc::Rc<dyn Fn(windows::core::Error)>>,
    /// Escape went down in this tab's page (unmodified). The key still reaches the page; reader
    /// view uses it to leave itself.
    pub escape_pressed: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId)>>,
    /// A camera, microphone, location or notification request, or a script popup, from this tab's
    /// page was denied (no stored Allow). The origin is the page's canonical origin only.
    /// Bowser never prompts; the toolbar shows a passive indicator.
    pub permission_denied:
        Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, crate::permissions::Permission, String)>>,
    /// The tab's zoom changed (our own set or the page's Ctrl+wheel). The value is in percent.
    pub zoom_changed: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, u16)>>,
    pub policy_event: Option<std::rc::Rc<dyn Fn(&'static str)>>,
    /// A download from a normal tab started, moved or changed state (docs/design/downloads.md).
    /// Without this and `download_plan`, every download is cancelled.
    pub download_event: Option<std::rc::Rc<dyn Fn(crate::downloads::DownloadEvent)>>,
    /// Where a download from a normal tab goes (a path, the Save dialog, or none).
    pub download_plan:
        Option<std::rc::Rc<dyn Fn(&crate::downloads::DownloadRequest) -> crate::downloads::DownloadPlan>>,
    // --- split view ---
    /// "Open link in split view" was picked on an http(s) link in this tab's page context menu.
    pub open_in_split: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, url::Url)>>,
    // --- end split view ---
    // --- address bar, search and bookmarks ---
    /// The page context menu is about to open on a selection or an image: the labels of the search
    /// items to add (none for nothing to add).
    pub context_search: Option<ContextSearchLabelsFn>,
    /// One of those items was picked.
    pub context_search_pick: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, ContextSearchPick)>>,
    // --- end address bar, search and bookmarks ---
    // --- page tools (T7) ---
    /// The page context menu opened on a text field: the notes it may insert, as (id, menu label).
    pub context_notes: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId) -> Vec<(u64, String)>>>,
    /// One of those notes was picked.
    pub context_note_pick: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, u64)>>,
    // --- end page tools ---
}

/// Asks for the labels of the search items a page context menu gets.
pub type ContextSearchLabelsFn = std::rc::Rc<dyn Fn(crate::tabs::TabId, &ContextSearchInfo) -> ContextSearchLabels>;

/// What the page context menu was opened on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextSearchInfo {
    /// The selected text, trimmed and cut, when there is a selection.
    pub selection: Option<String>,
    /// The address of the image under the pointer (http or https only).
    pub image: Option<url::Url>,
}

/// The labels of the search items to add to that menu.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextSearchLabels {
    pub selection: Option<String>,
    pub image: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContextSearchPick {
    Selection(String),
    Image(url::Url),
}

/// Per-site permission decisions, shared between the UI (Site information) and the WebView2 request
/// hook.
pub type PermissionsHandle = std::rc::Rc<std::cell::RefCell<crate::permissions::PermissionStore>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationProgressEvent {
    ContentLoading(u64),
    DomContentLoaded(u64),
    Completed(u64),
}

/// The ad/tracker blocker shared between the UI (site toggle) and the WebView2 request hook.
pub type BlockerHandle = std::rc::Rc<std::cell::RefCell<crate::blocker::BlockerState>>;

/// Optional engine features. The default (nothing) is what benchmark launches use.
#[derive(Clone, Default)]
pub struct EngineExtras {
    /// When set, subresource requests on every tab are checked against the blocker.
    pub blocker: Option<BlockerHandle>,
    /// The blocker private tabs use. It is always consulted for private tabs, whatever the global
    /// switch says (`BlockerState::should_block_private`). Without it, private tabs cannot be
    /// created: they fail closed rather than run unblocked.
    pub private_blocker: Option<BlockerHandle>,
    /// Creates the WebView2 environment with browser extensions enabled. Set only when the
    /// profile has an installed extension, so profiles without one pay nothing.
    pub browser_extensions: bool,
    /// Enabled unpacked extensions, as (id, folder), added once the first page is ready.
    pub startup_extensions: Vec<(String, std::path::PathBuf)>,
    /// Every installed extension as (id, enabled). Once the profile is ready, extensions the
    /// profile still holds that are not enabled here are removed or disabled.
    pub known_extensions: Vec<(String, bool)>,
    /// File that maps Bowser extension ids to WebView2's ids, which differ. Without it a restart
    /// cannot tell which profile extension is which.
    pub extension_ids_file: Option<std::path::PathBuf>,
    /// Stored per-site permission decisions, consulted for every permission request.
    pub permissions: Option<PermissionsHandle>,
    /// HTML chrome mode: registers the `bowser-ui` scheme on the environment so the chrome views
    /// can be hosted in it (in their own private WebView2 profile). Off in native mode, which
    /// leaves the environment options exactly as before.
    pub chrome_ui_scheme: bool,
}
