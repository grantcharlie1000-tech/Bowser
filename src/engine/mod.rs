pub mod web_app;
mod webview2;
pub(crate) use webview2::password_login_messages_enabled;

pub use webview2::{RuntimeUnavailable, WebView2Engine};

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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowsingDataRange {
    LastHour,
    LastDay,
    LastSevenDays,
    AllTime,
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
    /// The profile's tracking-prevention level as the browser reports it now, or `None` when the
    /// engine cannot say.
    fn tracking_prevention_level(&self, _tab: crate::tabs::TabId) -> Option<&'static str> {
        None
    }
    /// Current WebView2 reputation-checking setting, or `None` when the runtime cannot report it.
    fn smart_screen_enabled(&self, _tab: crate::tabs::TabId) -> Option<bool> {
        None
    }
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
    /// A camera, microphone, location or notification request from this tab's page was denied
    /// (no stored Allow). Bowser never prompts; the toolbar shows a passive indicator.
    pub permission_denied: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId)>>,
    pub policy_event: Option<std::rc::Rc<dyn Fn(&'static str)>>,
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
