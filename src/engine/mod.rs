mod webview2;

pub use webview2::{RuntimeUnavailable, WebView2Engine};

use windows::Win32::Foundation::RECT;

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
}

pub trait EngineAdapter {
    fn navigate(&mut self, url: &url::Url) -> windows::core::Result<()>;
    fn can_go_back(&self) -> bool;
    fn go_back(&mut self) -> windows::core::Result<()>;
    fn can_go_forward(&self) -> bool;
    fn go_forward(&mut self) -> windows::core::Result<()>;
    fn resize(&mut self, bounds: RECT) -> windows::core::Result<()>;
    fn set_memory_usage_target(
        &mut self,
        target: MemoryUsageTarget,
    ) -> windows::core::Result<MemoryTargetApply>;
    fn close(&mut self) -> windows::core::Result<()>;
}

#[derive(Clone, Default)]
pub struct EngineEvents {
    pub address_changed: Option<std::rc::Rc<dyn Fn(url::Url)>>,
    pub page_usable: Option<std::rc::Rc<dyn Fn()>>,
    pub navigation_started: Option<std::rc::Rc<dyn Fn()>>,
    pub failed: Option<std::rc::Rc<dyn Fn(windows::core::Error)>>,
    pub policy_event: Option<std::rc::Rc<dyn Fn(&'static str)>>,
}
