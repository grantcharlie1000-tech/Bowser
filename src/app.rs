use std::{cell::RefCell, path::PathBuf, rc::Rc};

use windows::{
    Win32::{Foundation::HWND, Graphics::Gdi::HFONT},
    core::Result,
};

use crate::{
    navigation::{NavigationTarget, SearchProvider, resolve_input},
    performance::{LaunchOptions, PerfLog},
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
}

pub type AppRef = Rc<RefCell<App>>;

impl App {
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
        Self {
            hwnd: HWND::default(),
            edit: HWND::default(),
            input_font: HFONT::default(),
            engine: None,
            pending: None,
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
                if url.scheme() != "file" {
                    state.allowed_benchmark_file = None;
                }
                let repeat_boundary = state.has_navigated && state.log.enabled();
                drop(state);
                if repeat_boundary {
                    let _ = app.borrow().log.record("repeat_navigation_boundary");
                }
                Self::navigate_current(app, &url);
                return;
            }
            state.allowed_benchmark_file = allowed_file;
            state.pending = Some(url.clone());
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
        let events = engine::EngineEvents {
            address_changed: Some(Rc::new(move |url| {
                if let Some(app) = events_app.upgrade() {
                    let (edit, log) = {
                        let state = app.borrow();
                        (state.edit, state.log.clone())
                    };
                    let event = if win32::set_input_text(edit, url.as_str()) {
                        "address_updated"
                    } else {
                        "address_update_failed"
                    };
                    let _ = log.record(event);
                }
            })),
            page_usable: {
                let events_app = Rc::downgrade(app);
                Some(Rc::new(move || {
                    if let Some(app) = events_app.upgrade() {
                        let _ = app.borrow().log.record("page_usable");
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
        let (bounds, target, pending) = {
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
            )
        };
        if let Some(bounds) = bounds {
            if engine.resize(bounds).is_err() {
                let _ = app.borrow().log.record("engine_ready_resize_failed");
            }
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
            Ok(MemoryTargetApply::Unsupported) | Err(_) => None,
        };
        state.engine = Some(engine);
        drop(state);
        if let Some(pending) = pending {
            Self::navigate_current(app, &pending);
        }
    }

    pub fn navigate_current(app: &AppRef, url: &url::Url) {
        let Some(mut engine) = app.borrow_mut().engine.take() else {
            return;
        };
        let result = engine.navigate(url);
        let mut state = app.borrow_mut();
        state.has_navigated = true;
        if state.closing {
            let _ = engine.close();
        } else {
            state.engine = Some(engine);
        }
        if let Err(error) = result {
            win32::show_error_message(&format!("Navigation failed: {error}"));
        }
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
        }
        transition
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
            Ok(MemoryTargetApply::Unsupported) | Err(_) => None,
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
        let layout = layout_client(
            rect.right - rect.left,
            rect.bottom - rect.top,
            win32::dpi_scale(self.hwnd),
        );
        let bounds = windows::Win32::Foundation::RECT {
            left: layout.viewport_x,
            top: layout.viewport_y,
            right: layout.viewport_x + layout.viewport_width,
            bottom: layout.viewport_y + layout.viewport_height,
        };
        if valid_viewport_bounds(bounds) {
            self.last_valid_viewport_bounds = Some(bounds);
            bounds
        } else {
            if let Some(bounds) = self.last_valid_viewport_bounds {
                bounds
            } else {
                let fallback = layout_client(1000, 720, win32::dpi_scale(self.hwnd));
                let fallback = windows::Win32::Foundation::RECT {
                    left: fallback.viewport_x,
                    top: fallback.viewport_y,
                    right: fallback.viewport_x + fallback.viewport_width,
                    bottom: fallback.viewport_y + fallback.viewport_height,
                };
                self.last_valid_viewport_bounds = Some(fallback);
                fallback
            }
        }
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
