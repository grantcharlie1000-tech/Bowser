mod webview2;

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

pub trait EngineAdapter {
    fn navigate(&mut self, url: &url::Url) -> windows::core::Result<()>;
    fn navigate_tab(
        &mut self,
        _tab: crate::tabs::TabId,
        url: &url::Url,
    ) -> windows::core::Result<()> {
        self.navigate(url)
    }
    fn can_go_back(&self) -> bool;
    fn go_back(&mut self) -> windows::core::Result<()>;
    fn can_go_forward(&self) -> bool;
    fn go_forward(&mut self) -> windows::core::Result<()>;
    fn resize(&mut self, bounds: RECT) -> windows::core::Result<()>;
    fn reload(&mut self) -> windows::core::Result<()> {
        Ok(())
    }
    fn stop(&mut self) -> windows::core::Result<()> {
        Ok(())
    }
    fn set_visible(&mut self, _visible: bool) -> windows::core::Result<()> {
        Ok(())
    }
    /// Selects a logical tab, hiding the previous controller and showing/resuming the new one.
    fn select_tab(&mut self, _tab: crate::tabs::TabId) -> windows::core::Result<()> {
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
    fn suspend(&mut self) {}
    fn set_memory_usage_target(
        &mut self,
        target: MemoryUsageTarget,
    ) -> windows::core::Result<MemoryTargetApply>;
    fn close(&mut self) -> windows::core::Result<()>;
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
    pub navigation_started: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, u64)>>,
    pub navigation_progress:
        Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, NavigationProgressEvent)>>,
    pub favicon_changed: Option<FaviconChangedCallback>,
    pub new_window_requested: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId, url::Url)>>,
    /// A tab's render process exited or crashed; the browser itself and other tabs are fine.
    pub render_process_failed: Option<std::rc::Rc<dyn Fn(crate::tabs::TabId)>>,
    pub failed: Option<std::rc::Rc<dyn Fn(windows::core::Error)>>,
    pub policy_event: Option<std::rc::Rc<dyn Fn(&'static str)>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationProgressEvent {
    ContentLoading(u64),
    DomContentLoaded(u64),
    Completed(u64),
}
