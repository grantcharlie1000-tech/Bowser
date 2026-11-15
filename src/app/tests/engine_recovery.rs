use super::*;

fn blank_browser_loss() -> AppRef {
    let app = app_with_pending(rect(800, 600));
    {
        let mut state = app.borrow_mut();
        state.pending = None;
        state.pending_tab = None;
        state.initializing = false;
        state.settings.start.widgets = true;
        state.settings.start.columns = 4;
        state.settings.start.homepage = "https://example.test/configured-homepage".into();
        state.settings.start.startup = crate::start_page::Startup::Homepage;
        state.settings.theme = crate::settings::Theme::System;
        state.engine = Some(Box::new(TabRecordingEngine { calls: Rc::default(), playing: Vec::new() }));
        for tab in &mut state.tabs {
            tab.url = None;
            tab.loading = false;
            tab.controller_created = false;
        }
    }
    App::browser_loss_callback(&app, 0)(42);
    App::process_engine_notices(&app);
    app
}

// Only the external engine boundary is replaced; loss, creation ownership, pending navigation,
// publication, timeout and retirement all run the production App transitions.
struct PumpingCreationEngine {
    inner: TabRecordingEngine,
    pump: Option<Box<dyn FnOnce()>>,
    closing: Option<Box<dyn FnOnce()>>,
}

impl EngineAdapter for PumpingCreationEngine {
    fn navigate(&mut self, url: &url::Url) -> windows::core::Result<()> { self.inner.navigate(url) }
    fn navigate_tab(&mut self, tab: TabId, url: &url::Url) -> windows::core::Result<()> { self.inner.navigate_tab(tab, url) }
    fn navigate_tab_post(&mut self, tab: TabId, url: &url::Url, body: &str) -> windows::core::Result<()> { self.inner.navigate_tab_post(tab, url, body) }
    fn can_go_back(&self) -> bool { false }
    fn go_back(&mut self) -> windows::core::Result<()> { Ok(()) }
    fn can_go_forward(&self) -> bool { false }
    fn go_forward(&mut self) -> windows::core::Result<()> { Ok(()) }
    fn resize(&mut self, _: windows::Win32::Foundation::RECT) -> windows::core::Result<()> {
        if let Some(pump) = self.pump.take() { pump(); }
        Ok(())
    }
    fn select_tab(&mut self, tab: TabId) -> windows::core::Result<()> { self.inner.select_tab(tab) }
    fn set_tab_private(&mut self, tab: TabId) { self.inner.set_tab_private(tab); }
    fn set_memory_usage_target(&mut self, _: MemoryUsageTarget) -> windows::core::Result<MemoryTargetApply> { Ok(MemoryTargetApply::Applied) }
    fn close(&mut self) -> windows::core::Result<()> {
        self.inner.calls.borrow_mut().push("retired".into());
        if let Some(closing) = self.closing.take() { closing(); }
        Ok(())
    }
}

fn creation_engine(calls: Rc<RefCell<Vec<String>>>, pump: impl FnOnce() + 'static) -> Box<dyn EngineAdapter> {
    Box::new(PumpingCreationEngine {
        inner: TabRecordingEngine { calls, playing: Vec::new() }, pump: Some(Box::new(pump)), closing: None,
    })
}

#[test]
fn engine_recovery_only_blank_tabs_recreate_shared_engine_without_user_navigation() {
    for private in [false, true] {
        let app = blank_browser_loss();
        app.borrow_mut().tabs[0].private = private;
        let start = App::start_settings(&app);
        let theme = App::settings(&app).theme;
        let calls = Rc::new(RefCell::new(Vec::new()));
        let generation = App::begin_browser_recovery(&app, Instant::now()).expect("loss queues an automatic attempt");
        assert!(App::begin_browser_recovery(&app, Instant::now()).is_none(), "pending creation is shared");
        App::engine_creation_finished(&app, generation, Ok(creation_engine(calls.clone(), || {})), None);
        assert!(app.borrow().engine.is_some());
        assert!(!app.borrow().initializing);
        assert!(App::tab_is_blank(&app, App::active_tab_id(&app)));
        assert_eq!(App::tab_private(&app, App::active_tab_id(&app)), private);
        assert_eq!(App::start_settings(&app), start);
        assert_eq!(App::settings(&app).theme, theme);
        assert!(start.widgets);
        assert_eq!(start.columns, 4);
        assert_eq!(start.homepage, "https://example.test/configured-homepage");
        assert_eq!(theme, crate::settings::Theme::System);
        assert!(!calls.borrow().iter().any(|call| call.starts_with("navigate:") || call.starts_with("post:") || call.starts_with("prewarm:")));
    }
}

#[test]
fn engine_recovery_automatic_creation_never_revives_failed_or_eager_content_or_posts() {
    let (app, hidden, active, _) = sleeping_app(300);
    test_show_page(&app, active, "https://example.test/post");
    {
        let mut state = app.borrow_mut();
        state.pending_request_body = Some((active, url::Url::parse("https://example.test/post").unwrap(), "secret=old".into()));
        let dormant = state.tabs.iter_mut().find(|tab| tab.id == hidden).unwrap();
        dormant.controller_created = false;
        dormant.loading = false;
        dormant.discarded = true;
        state.tab_state.eager.push(hidden);
    }
    App::browser_loss_callback(&app, 0)(42);
    App::process_engine_notices(&app);
    let generation = App::begin_browser_recovery(&app, Instant::now()).unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    App::engine_creation_finished(&app, generation, Ok(creation_engine(calls.clone(), || {})), None);
    assert!(!calls.borrow().iter().any(|call| call.starts_with("navigate:") || call.starts_with("post:")));
    assert!(App::content_failure_message(&app, active).is_some());
    assert!(app.borrow().pending_request_body.is_none());
    assert!(app.borrow().tabs.iter().find(|tab| tab.id == hidden).unwrap().discarded);
}

#[test]
fn engine_recovery_creation_failure_and_repeated_loss_cannot_refill_automatic_budget() {
    for creation_fails in [false, true] {
        let app = blank_browser_loss();
        let generation = App::begin_browser_recovery(&app, Instant::now()).unwrap();
        if creation_fails {
            App::engine_creation_finished(&app, generation, Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL)), None);
        } else {
            App::engine_creation_finished(&app, generation, Ok(creation_engine(Rc::default(), || {})), None);
            App::browser_loss_callback(&app, generation)(43);
        }
        App::process_engine_notices(&app);
        for _ in 0..3 {
            assert!(App::begin_browser_recovery(&app, Instant::now()).is_none());
            let current = app.borrow().engine_generation;
            App::browser_loss_callback(&app, current)(44);
            App::process_engine_notices(&app);
        }
        assert!(app.borrow().engine.is_none());
        assert!(!app.borrow().initializing && !app.borrow().closing);
    }
}

#[test]
fn engine_recovery_automatic_creation_has_a_deadline_and_late_success_is_retired() {
    let app = blank_browser_loss();
    let started = Instant::now();
    let generation = App::begin_browser_recovery(&app, started).unwrap();
    assert!(App::engine_recovery_tick(&app, started + Duration::from_secs(29)));
    assert!(!App::engine_recovery_tick(&app, started + Duration::from_secs(31)));
    let calls = Rc::new(RefCell::new(Vec::new()));
    App::engine_creation_finished(&app, generation, Ok(creation_engine(calls.clone(), || panic!("obsolete creation must not be configured"))), None);
    assert_eq!(*calls.borrow(), ["retired"]);
    assert!(app.borrow().engine.is_none());
    assert!(App::begin_browser_recovery(&app, started + Duration::from_secs(32)).is_none());
}

#[test]
fn engine_recovery_close_cancels_queued_and_pumping_creation_without_app_borrows() {
    let queued = blank_browser_loss();
    queued.borrow_mut().closing = true;
    App::cancel_browser_recovery(&queued);
    assert!(App::begin_browser_recovery(&queued, Instant::now()).is_none());
    for close_during_configuration in [false, true] {
        let app = blank_browser_loss();
        let generation = App::begin_browser_recovery(&app, Instant::now()).unwrap();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let closing_app = app.clone();
        let on_close_app = app.clone();
        let engine = PumpingCreationEngine {
            inner: TabRecordingEngine { calls: calls.clone(), playing: Vec::new() },
            pump: Some(Box::new(move || {
                closing_app.borrow_mut().closing = true;
                App::cancel_browser_recovery(&closing_app);
            })),
            closing: Some(Box::new(move || { assert!(on_close_app.borrow_mut().closing); })),
        };
        if !close_during_configuration {
            app.borrow_mut().closing = true;
            App::cancel_browser_recovery(&app);
        }
        App::engine_creation_finished(&app, generation, Ok(Box::new(engine)), None);
        assert!(app.borrow().engine.is_none());
        assert!(!app.borrow().initializing, "failed-state close does not wait for a missing callback");
        assert!(calls.borrow().iter().any(|call| call == "retired"));
    }
}

#[test]
fn engine_recovery_old_completion_cannot_replace_a_new_user_engine() {
    let app = blank_browser_loss();
    let generation = App::begin_browser_recovery(&app, Instant::now()).unwrap();
    assert!(!App::engine_recovery_tick(&app, Instant::now() + Duration::from_secs(31)));
    let target = url::Url::parse("https://example.test/user").unwrap();
    app.borrow_mut().initializing = true;
    App::submit_url(&app, target, None);
    let replacement = App::begin_engine_creation(&app).unwrap();
    let user_calls = Rc::new(RefCell::new(Vec::new()));
    App::engine_creation_finished(&app, replacement, Ok(creation_engine(user_calls.clone(), || {})), None);
    let old_calls = Rc::new(RefCell::new(Vec::new()));
    App::engine_creation_finished(&app, generation, Ok(creation_engine(old_calls.clone(), || panic!("late engine"))), None);
    assert_eq!(*old_calls.borrow(), ["retired"]);
    assert!(app.borrow().engine.is_some());
    assert!(user_calls.borrow().iter().any(|call| call == "navigate:0:https://example.test/user"));
    assert_eq!(app.borrow().engine_generation, replacement);
}

#[test]
fn engine_recovery_user_navigation_during_creation_setup_wins_and_keeps_its_post_owner() {
    let app = blank_browser_loss();
    let generation = App::begin_browser_recovery(&app, Instant::now()).unwrap();
    App::submit_url(&app, url::Url::parse("https://example.test/older").unwrap(), None);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let input_app = app.clone();
    App::engine_creation_finished(&app, generation, Ok(creation_engine(calls.clone(), move || {
        assert!(input_app.borrow().initializing, "a pumping setup must keep user requests on the shared creation");
        let target = url::Url::parse("https://example.test/latest").unwrap();
        POST_BODY.with(|slot| *slot.borrow_mut() = Some((target.clone(), "q=new-user-request".into())));
        App::submit_url(&input_app, target, None);
        App::new_tab(&input_app);
    })), None);
    let requests: Vec<_> = calls.borrow().iter().filter(|call| call.starts_with("navigate:") || call.starts_with("post:")).cloned().collect();
    assert_eq!(requests, ["post:0:https://example.test/latest:q=new-user-request"]);
    assert!(app.borrow().pending.is_none() && app.borrow().pending_request_body.is_none());
    assert_eq!(app.borrow().engine_generation, generation);
}

#[test]
fn engine_recovery_loss_teardown_cannot_queue_over_a_nested_close_or_user_engine() {
    for close in [false, true] {
        let app = app_with_pending(rect(800, 600));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let nested_app = app.clone();
        let nested_calls = calls.clone();
        app.borrow_mut().engine = Some(Box::new(PumpingCreationEngine {
            inner: TabRecordingEngine { calls: Rc::default(), playing: Vec::new() },
            pump: None,
            closing: Some(Box::new(move || {
                if close {
                    nested_app.borrow_mut().closing = true;
                    App::cancel_browser_recovery(&nested_app);
                } else {
                    nested_app.borrow_mut().initializing = true;
                    App::submit_url(&nested_app, url::Url::parse("https://example.test/user-after-loss").unwrap(), None);
                    let user = App::begin_engine_creation(&nested_app).unwrap();
                    App::engine_creation_finished(&nested_app, user, Ok(creation_engine(nested_calls, || {})), None);
                }
            })),
        }));
        App::browser_loss_callback(&app, 0)(42);
        App::process_engine_notices(&app);
        assert!(App::begin_browser_recovery(&app, Instant::now()).is_none());
        if close {
            assert!(app.borrow().closing && app.borrow().engine.is_none());
        } else {
            assert!(app.borrow().engine.is_some());
            let requests: Vec<_> = calls.borrow().iter().filter(|call| call.starts_with("navigate:")).cloned().collect();
            assert_eq!(requests, ["navigate:0:https://example.test/user-after-loss"]);
        }
    }
}

#[test]
fn engine_recovery_pumping_configuration_cannot_publish_over_a_newer_user_generation() {
    let app = blank_browser_loss();
    let generation = App::begin_browser_recovery(&app, Instant::now()).unwrap();
    let old_calls = Rc::new(RefCell::new(Vec::new()));
    let user_calls = Rc::new(RefCell::new(Vec::new()));
    let nested_app = app.clone();
    let nested_calls = user_calls.clone();
    App::engine_creation_finished(&app, generation, Ok(creation_engine(old_calls.clone(), move || {
        App::submit_url(&nested_app, url::Url::parse("https://example.test/newer-generation").unwrap(), None);
        let user = App::begin_engine_creation(&nested_app).unwrap();
        App::engine_creation_finished(&nested_app, user, Ok(creation_engine(nested_calls, || {})), None);
    })), None);
    assert!(old_calls.borrow().iter().any(|call| call == "retired"));
    assert!(app.borrow().engine.is_some() && !app.borrow().initializing);
    let requests: Vec<_> = user_calls.borrow().iter().filter(|call| call.starts_with("navigate:")).cloned().collect();
    assert_eq!(requests, ["navigate:0:https://example.test/newer-generation"]);
}

#[test]
fn engine_recovery_environment_only_loss_retires_an_engine_with_only_new_tabs() {
    let app = app_with_pending(rect(800, 600));
    let active = App::active_tab_id(&app);
    App::new_tab(&app);
    let callback = App::browser_loss_callback(&app, 0);
    let (engine, emit) = engine::WebView2Engine::probe_environment_loss_engine(callback);
    {
        let mut state = app.borrow_mut();
        state.engine = Some(Box::new(engine));
        state.pending = None;
        state.pending_tab = None;
        for tab in &mut state.tabs {
            tab.url = None;
            tab.controller_created = false;
            tab.page_usable = false;
            tab.loading = false;
        }
        emit(42);
        emit(42);
        assert_eq!(state.engine_notices.borrow().0.len(), 1);
        assert_eq!(state.engine_generation, 0);
        assert!(state.engine.is_some(), "COM callback only enqueues while App is borrowed");
    }
    App::process_engine_notices(&app);
    let state = app.borrow();
    assert_eq!(state.engine_generation, 1);
    assert!(state.engine.is_none());
    assert!(state.engine_creation_settled && !state.initializing);
    assert!(state.tabs.iter().all(|tab| tab.url.is_none() && tab.failure.is_none()));
    assert!(state.tabs.iter().any(|tab| tab.id == active), "new tabs survive native recovery");
}

#[test]
fn engine_recovery_app_coalesces_both_loss_sources_and_rejects_old_or_shutdown_events() {
    for environment_first in [false, true] {
        let app = app_with_pending(rect(800, 600));
        let process = App::process_failure_callback(&app, 0);
        let environment = App::browser_loss_callback(&app, 0);
        let active = App::active_tab_id(&app);
        if environment_first { environment(42); }
        process(failure(active, engine::ProcessFailureKind::BrowserExited));
        environment(42);
        assert_eq!(app.borrow().engine_notices.borrow().0.len(), 1);
        App::process_engine_notices(&app);
        assert_eq!(app.borrow().engine_generation, 1);
        environment(42);
        App::process_engine_notices(&app);
        assert_eq!(app.borrow().engine_generation, 1);
        app.borrow_mut().closing = true;
        App::browser_loss_callback(&app, 1)(43);
        App::process_engine_notices(&app);
        assert_eq!(app.borrow().engine_generation, 1);
    }
}

#[test]
fn engine_recovery_chrome_attachment_hook_survives_engine_checkout_and_rejects_obsolete_generation() {
    let app = app_with_pending(rect(800, 600));
    let (engine, emit) = engine::WebView2Engine::probe_environment_loss_engine(App::browser_loss_callback(&app, 0));
    let observer = engine.browser_process_observer().unwrap();
    app.borrow_mut().browser_process_observer = Some(observer);
    assert!(!App::chrome_browser_process_attached(&app, 1, std::process::id()));
    assert!(!App::chrome_browser_process_attached(&app, 0, 0));
    assert!(App::chrome_browser_process_attached(&app, 0, std::process::id()), "sink remains usable while engine is outside App");
    emit(42); // A different browser PID must not retire the newly attached chrome.
    assert!(app.borrow().engine_notices.borrow().0.is_empty());
    app.borrow_mut().closing = true;
    assert!(!App::chrome_browser_process_attached(&app, 0, std::process::id()));
    drop(engine);
    app.borrow_mut().closing = false;
    assert!(!App::chrome_browser_process_attached(&app, 0, std::process::id()), "weak sink cannot retain an obsolete engine");
}

// Catches the repeated-crash branch returning through a modal before clearing live state.
#[test]
fn engine_recovery_repeated_renderer_failure_leaves_a_nonmodal_retry_state() {
    let (app, _, active, _) = sleeping_app(300);
    App::tab_render_failed(&app, active);
    {
        let mut state = app.borrow_mut();
        let tab = state.tabs.iter_mut().find(|tab| tab.id == active).unwrap();
        tab.url = Some(url::Url::parse("https://example.test/form-result").unwrap());
        tab.controller_created = true;
        tab.page_usable = true;
        tab.loading = true;
    }
    App::tab_render_failed(&app, active);
    let state = app.borrow();
    let tab = state.tabs.iter().find(|tab| tab.id == active).unwrap();
    assert!(!tab.page_usable);
    assert!(!tab.controller_created);
    assert!(!tab.loading);
    assert!(!state.closing, "native retry and close must remain available");
}

struct BorrowOnClose {
    app: std::rc::Weak<RefCell<App>>,
    closed: Rc<std::cell::Cell<bool>>,
}

impl EngineAdapter for BorrowOnClose {
    fn controller_generation(&self, _: TabId) -> Option<u64> { Some(7) }
    fn navigate(&mut self, _: &url::Url) -> windows::core::Result<()> { Ok(()) }
    fn can_go_back(&self) -> bool { false }
    fn go_back(&mut self) -> windows::core::Result<()> { Ok(()) }
    fn can_go_forward(&self) -> bool { false }
    fn go_forward(&mut self) -> windows::core::Result<()> { Ok(()) }
    fn resize(&mut self, _: windows::Win32::Foundation::RECT) -> windows::core::Result<()> { Ok(()) }
    fn set_memory_usage_target(&mut self, _: MemoryUsageTarget) -> windows::core::Result<MemoryTargetApply> {
        Ok(MemoryTargetApply::Applied)
    }
    fn close(&mut self) -> windows::core::Result<()> {
        // A pumping COM Close can synchronously reenter the app through a callback.
        let app = self.app.upgrade().unwrap();
        let mut state = app.borrow_mut();
        assert!(state.engine.is_none());
        state.page_loaded = false;
        self.closed.set(true);
        Ok(())
    }
}

fn failure(tab: TabId, kind: engine::ProcessFailureKind) -> engine::ProcessFailure {
    engine::ProcessFailure { tab, controller_generation: 7, kind, reason: Some(0), exit_code: Some(1) }
}

#[test]
fn engine_recovery_shared_loss_is_deferred_coalesced_and_never_replays_requests() {
    let (app, hidden, active, _) = sleeping_app(300);
    let closed = Rc::new(std::cell::Cell::new(false));
    {
        let mut state = app.borrow_mut();
        state.engine = Some(Box::new(BorrowOnClose { app: Rc::downgrade(&app), closed: closed.clone() }));
        state.tabs.iter_mut().find(|tab| tab.id == hidden).unwrap().private = true;
        let tab = state.tabs.iter_mut().find(|tab| tab.id == active).unwrap();
        tab.url = Some(url::Url::parse("https://example.test/post").unwrap());
        tab.controller_created = true;
        tab.page_usable = true;
        tab.loading = true;
        state.pending_request_body = Some((active, url::Url::parse("https://example.test/post").unwrap(), "q=private".into()));
    }
    let callback = App::process_failure_callback(&app, 0);
    {
        let state = app.borrow_mut();
        callback(failure(active, engine::ProcessFailureKind::BrowserExited));
        callback(failure(hidden, engine::ProcessFailureKind::BrowserExited));
        assert!(state.engine.is_some(), "callback only queues work, even while App is borrowed");
        assert_eq!(state.engine_notices.borrow().0.len(), 1, "one shared loss per engine");
        assert!(!closed.get());
    }
    App::process_engine_notices(&app);
    assert!(closed.get());
    let state = app.borrow();
    assert!(state.engine.is_none());
    assert!(state.pending.is_none() && state.pending_request_body.is_none());
    assert!(!state.initializing && !state.closing);
    for tab in &state.tabs {
        assert!(!tab.controller_created && !tab.page_usable && !tab.loading && !tab.discarded);
        assert_eq!(tab.failure, Some(ContentFailure::BrowserLost));
    }
    assert!(state.tabs.iter().find(|tab| tab.id == hidden).unwrap().private);
}

#[test]
fn engine_recovery_old_engine_and_controller_failures_cannot_invalidate_replacements() {
    let app = app_with_pending(rect(800, 600));
    let active = App::active_tab_id(&app);
    app.borrow_mut().engine = Some(Box::new(BorrowOnClose { app: Rc::downgrade(&app), closed: Rc::new(std::cell::Cell::new(false)) }));
    test_show_page(&app, active, "https://example.test/");
    let old_engine = App::process_failure_callback(&app, 0);
    app.borrow_mut().engine_generation = 1;
    old_engine(failure(active, engine::ProcessFailureKind::BrowserExited));
    let current = App::process_failure_callback(&app, 1);
    current(engine::ProcessFailure { controller_generation: 6, ..failure(active, engine::ProcessFailureKind::RendererExited) });
    App::process_engine_notices(&app);
    assert!(App::active_tab_has_controller(&app));
    assert!(App::content_failure_message(&app, active).is_none());
}

#[test]
fn engine_recovery_gpu_utility_and_subframe_exits_preserve_live_content() {
    let app = app_with_pending(rect(800, 600));
    let active = App::active_tab_id(&app);
    let closed = Rc::new(std::cell::Cell::new(false));
    app.borrow_mut().engine = Some(Box::new(BorrowOnClose { app: Rc::downgrade(&app), closed: closed.clone() }));
    test_show_page(&app, active, "https://example.test/");
    let callback = App::process_failure_callback(&app, 0);
    for kind in [engine::ProcessFailureKind::GpuExited, engine::ProcessFailureKind::UtilityExited, engine::ProcessFailureKind::FrameRendererExited, engine::ProcessFailureKind::Other(99)] {
        callback(failure(active, kind));
    }
    App::process_engine_notices(&app);
    assert!(App::active_tab_has_controller(&app));
    assert!(App::content_failure_message(&app, active).is_none());
    assert!(!closed.get());
}

#[test]
fn engine_recovery_unresponsive_renderer_preserves_the_live_document_and_history() {
    let app = app_with_pending(rect(800, 600));
    let active = App::active_tab_id(&app);
    let closed = Rc::new(std::cell::Cell::new(false));
    app.borrow_mut().engine = Some(Box::new(BorrowOnClose { app: Rc::downgrade(&app), closed: closed.clone() }));
    test_show_page(&app, active, "https://example.test/unsaved-form");
    {
        let mut state = app.borrow_mut();
        state.tabs[0].page_usable = true;
        state.tabs[0].can_back = true;
        state.tabs[0].can_forward = true;
    }
    let callback = App::process_failure_callback(&app, 0);
    callback(failure(active, engine::ProcessFailureKind::RendererUnresponsive));
    App::process_engine_notices(&app);
    assert!(!closed.get(), "temporary unresponsiveness is not permission to discard the document");
    let state = app.borrow();
    let tab = &state.tabs[0];
    assert!(tab.controller_created && tab.page_usable);
    assert!(tab.can_back && tab.can_forward);
    assert!(tab.failure.is_none());
}

#[test]
fn engine_recovery_renderer_exit_keeps_the_controller_for_explicit_retry() {
    let app = app_with_pending(rect(800, 600));
    let active = App::active_tab_id(&app);
    let closed = Rc::new(std::cell::Cell::new(false));
    app.borrow_mut().engine = Some(Box::new(BorrowOnClose { app: Rc::downgrade(&app), closed: closed.clone() }));
    test_show_page(&app, active, "https://example.test/form-result");
    App::tab_render_failed(&app, active);
    assert!(!closed.get(), "a renderer crash leaves a viable controller and its history");
    assert_eq!(App::content_failure_message(&app, active), Some(ContentFailure::RendererExited.message()));
    assert!(App::retry_failed_tab(&app, active));
    assert!(!closed.get(), "explicit GET retry can reuse the controller");
    assert_eq!(app.borrow().engine.as_ref().unwrap().controller_generation(active), Some(7));
}

#[test]
fn engine_recovery_a_loaded_retained_renderer_is_not_later_destroyed_by_a_creation_timeout() {
    let app = app_with_pending(rect(800, 600));
    let active = App::active_tab_id(&app);
    let closed = Rc::new(std::cell::Cell::new(false));
    app.borrow_mut().engine = Some(Box::new(BorrowOnClose { app: Rc::downgrade(&app), closed: closed.clone() }));
    test_show_page(&app, active, "https://example.test/form-result");
    App::tab_render_failed(&app, active);
    assert!(App::retry_failed_tab(&app, active));
    App::tab_page_loaded(&app, active);
    assert!(!App::engine_recovery_tick(&app, Instant::now() + Duration::from_secs(31)));
    let state = app.borrow();
    let tab = state.tabs.iter().find(|tab| tab.id == active).unwrap();
    assert!(tab.page_usable && tab.controller_created, "a recovered document remains usable");
    assert!(tab.failure.is_none());
    assert!(!closed.get());
    assert_eq!(state.engine.as_ref().unwrap().controller_generation(active), Some(7));
}

fn retained_renderer_navigation_survives_creation_timeout(submit: bool) {
    let app = app_with_pending(rect(800, 600));
    let active = App::active_tab_id(&app);
    let closed = Rc::new(std::cell::Cell::new(false));
    app.borrow_mut().engine = Some(Box::new(BorrowOnClose { app: Rc::downgrade(&app), closed: closed.clone() }));
    test_show_page(&app, active, "https://example.test/form-result");
    App::tab_render_failed(&app, active);
    let target = url::Url::parse("https://example.test/another-page").unwrap();
    if submit { App::submit_url(&app, target.clone(), None); }
    else { App::navigate_current(&app, &target); }
    App::tab_page_loaded(&app, active);
    assert!(!App::engine_recovery_tick(&app, Instant::now() + Duration::from_secs(31)));
    let state = app.borrow();
    let tab = state.tabs.iter().find(|tab| tab.id == active).unwrap();
    assert_eq!(tab.url.as_ref(), Some(&target));
    assert!(tab.page_usable && tab.controller_created);
    assert!(tab.failure.is_none());
    assert!(!closed.get());
}

#[test]
fn engine_recovery_typed_address_reuses_the_retained_renderer_without_a_creation_timeout() {
    retained_renderer_navigation_survives_creation_timeout(true);
}

#[test]
fn engine_recovery_direct_navigation_reuses_the_retained_renderer_without_a_creation_timeout() {
    retained_renderer_navigation_survives_creation_timeout(false);
}

#[test]
fn engine_recovery_unscoped_policy_errors_cannot_fail_whichever_tab_is_selected_later() {
    let (app, first, second, _) = sleeping_app(300);
    let queue = app.borrow().engine_notices.clone();
    App::enqueue_engine_notice(&queue, HWND::default(), 0, EngineNotice::OperationFailed(windows::Win32::Foundation::E_FAIL.0));
    App::select_tab(&app, second);
    App::process_engine_notices(&app);
    for id in [first, second] {
        assert!(App::content_failure_message(&app, id).is_none(), "an unscoped error is not evidence of this document failing");
    }
}

#[test]
fn engine_recovery_explicit_private_retry_preserves_profile_and_does_not_replay_post() {
    let (app, hidden, _, calls) = sleeping_app(300);
    app.borrow_mut().tabs.iter_mut().find(|tab| tab.id == hidden).unwrap().private = true;
    App::tab_render_failed(&app, hidden);
    calls.borrow_mut().clear();
    App::select_tab(&app, hidden);
    assert!(!calls.borrow().iter().any(|call| call.starts_with("navigate:")), "selection is not consent to resend");
    calls.borrow_mut().clear();
    assert!(App::retry_failed_tab(&app, hidden));
    let calls = calls.borrow();
    let private = calls.iter().position(|call| call == &format!("private:{}", hidden.0)).unwrap();
    let navigate = calls.iter().position(|call| call == &format!("navigate:{}:https://hidden.example.test/", hidden.0)).unwrap();
    assert!(private < navigate);
}

#[test]
fn engine_recovery_another_tab_cannot_consume_a_pending_post_body() {
    let (app, hidden, active, _) = sleeping_app(300);
    let target = url::Url::parse("https://example.test/search").unwrap();
    app.borrow_mut().pending_request_body = Some((active, target.clone(), "q=owned".into()));
    App::navigate_tab(&app, hidden, &target);
    assert_eq!(app.borrow().pending_request_body.as_ref().map(|(tab, _, body)| (*tab, body.as_str())), Some((active, "q=owned")));
}

#[test]
fn engine_recovery_a_user_post_waiting_for_the_environment_is_sent_once_to_its_owner() {
    let app = app_with_pending(rect(800, 600));
    let target = url::Url::parse("https://example.test/search").unwrap();
    POST_BODY.with(|slot| *slot.borrow_mut() = Some((target.clone(), "q=owned".into())));
    App::submit_url(&app, target, None);
    assert!(POST_BODY.with(|slot| slot.borrow().is_none()));
    App::new_tab(&app);
    let calls = Rc::new(RefCell::new(Vec::new()));
    App::complete_engine_creation(&app, 0, Box::new(TabRecordingEngine { calls: calls.clone(), playing: Vec::new() }));
    let requests: Vec<_> = calls.borrow().iter().filter(|call| call.starts_with("post:") || call.starts_with("navigate:")).cloned().collect();
    assert_eq!(requests, ["post:0:https://example.test/search:q=owned"]);
    assert!(app.borrow().pending_request_body.is_none());
}

#[test]
fn engine_recovery_notice_waits_until_the_borrowed_engine_is_returned() {
    let app = app_with_pending(rect(800, 600));
    let active = App::active_tab_id(&app);
    app.borrow_mut().engine = Some(Box::new(BorrowOnClose { app: Rc::downgrade(&app), closed: Rc::new(std::cell::Cell::new(false)) }));
    test_show_page(&app, active, "https://example.test/");
    let callback = App::process_failure_callback(&app, 0);
    App::with_engine(&app, |_| {
        callback(failure(active, engine::ProcessFailureKind::RendererExited));
        App::process_engine_notices(&app);
        assert!(App::active_tab_has_controller(&app), "defer until the operation unwinds");
        Ok(())
    });
    App::process_engine_notices(&app);
    assert!(!App::active_tab_has_controller(&app));
    assert_eq!(app.borrow().tabs[0].failure, Some(ContentFailure::RendererExited));
}

#[test]
fn engine_recovery_reentrant_navigation_cannot_release_an_outer_engine_lease_or_consume_its_post() {
    let app = app_with_pending(rect(800, 600));
    let active = App::active_tab_id(&app);
    let closed = Rc::new(std::cell::Cell::new(false));
    app.borrow_mut().engine = Some(Box::new(BorrowOnClose { app: Rc::downgrade(&app), closed }));
    test_show_page(&app, active, "https://example.test/");
    let target = url::Url::parse("https://example.test/submitted").unwrap();
    app.borrow_mut().pending_request_body = Some((active, target.clone(), "q=owned".into()));
    let callback = App::process_failure_callback(&app, 0);
    App::with_engine(&app, |_| {
        App::navigate_current(&app, &target);
        assert!(app.borrow().engine_in_use);
        assert!(app.borrow().pending_request_body.is_some());
        callback(failure(active, engine::ProcessFailureKind::BrowserExited));
        App::process_engine_notices(&app);
        assert_eq!(app.borrow().engine_generation, 0, "nested navigation cannot authorize teardown during the outer call");
        Ok(())
    });
    App::process_engine_notices(&app);
    assert!(app.borrow().engine.is_none());
    assert!(app.borrow().pending_request_body.is_none(), "retired submitted bodies cannot be replayed");
}

#[test]
fn engine_recovery_a_missing_creation_completion_returns_to_retry_without_resubmission() {
    let app = app_with_pending(rect(800, 600));
    let started = Instant::now();
    {
        let mut state = app.borrow_mut();
        state.tabs[0].url = Some(url::Url::parse("https://example.test/request").unwrap());
        state.tabs[0].loading = true;
        state.tabs[0].recovery_started = Some(started);
    }
    assert!(App::engine_recovery_tick(&app, started + Duration::from_secs(29)));
    assert!(app.borrow().initializing);
    assert!(!App::engine_recovery_tick(&app, started + Duration::from_secs(31)));
    let state = app.borrow();
    assert!(!state.initializing && !state.closing);
    assert!(state.pending.is_none());
    assert!(!state.tabs[0].controller_created && !state.tabs[0].loading);
    assert_eq!(state.tabs[0].failure, Some(ContentFailure::Initialization));
    assert_eq!(state.engine_generation, 1, "late completion belongs to the abandoned generation");
}

#[test]
fn engine_recovery_failures_delivered_during_shutdown_do_not_restart_content() {
    let app = app_with_pending(rect(800, 600));
    let callback = App::process_failure_callback(&app, 0);
    app.borrow_mut().closing = true;
    callback(failure(TabId(0), engine::ProcessFailureKind::BrowserExited));
    App::process_engine_notices(&app);
    assert!(app.borrow().closing);
    assert!(app.borrow().engine.is_none());
    assert!(app.borrow().tabs[0].failure.is_none());
}

#[test]
fn engine_recovery_close_reentry_does_not_hold_the_app_borrow() {
    let app = app_with_pending(rect(800, 600));
    let closed = Rc::new(std::cell::Cell::new(false));
    app.borrow_mut().engine = Some(Box::new(BorrowOnClose {
        app: Rc::downgrade(&app), closed: closed.clone(),
    }));
    let result = App::with_engine(&app, |_| {
        app.borrow_mut().closing = true;
        Ok(())
    });
    assert!(result.unwrap().is_ok());
    assert!(closed.get());
    assert!(app.borrow().engine.is_none());
}

struct PumpingFailureEngine {
    app: std::rc::Weak<RefCell<App>>,
    callback: Rc<dyn Fn(engine::ProcessFailure)>,
    active: TabId,
    deferred: Rc<std::cell::Cell<bool>>,
    closed: Rc<std::cell::Cell<bool>>,
    close_during_call: bool,
}

impl PumpingFailureEngine {
    fn pump(&self) {
        let app = self.app.upgrade().unwrap();
        if self.close_during_call {
            app.borrow_mut().closing = true;
        } else {
            (self.callback)(failure(self.active, engine::ProcessFailureKind::BrowserExited));
            App::process_engine_notices(&app);
            self.deferred.set(app.borrow().engine_generation == 0);
        }
    }
}

impl EngineAdapter for PumpingFailureEngine {
    fn navigate(&mut self, _: &url::Url) -> windows::core::Result<()> { Ok(()) }
    fn can_go_back(&self) -> bool { true }
    fn can_go_forward(&self) -> bool { true }
    fn go_back(&mut self) -> windows::core::Result<()> { self.pump(); Ok(()) }
    fn go_forward(&mut self) -> windows::core::Result<()> { self.pump(); Ok(()) }
    fn resize(&mut self, _: windows::Win32::Foundation::RECT) -> windows::core::Result<()> { self.pump(); Ok(()) }
    fn set_memory_usage_target(&mut self, _: MemoryUsageTarget) -> windows::core::Result<MemoryTargetApply> {
        self.pump(); Ok(MemoryTargetApply::Applied)
    }
    fn close(&mut self) -> windows::core::Result<()> {
        let app = self.app.upgrade().unwrap();
        let state = app.borrow_mut();
        assert!(state.engine.is_none(), "a retired engine is detached before pumping Close");
        self.closed.set(true);
        Ok(())
    }
}

fn pumping_operation_does_not_resurrect_an_engine(operation: u8, close_during_call: bool) {
    let app = app_with_pending(rect(800, 600));
    let active = App::active_tab_id(&app);
    let closed = Rc::new(std::cell::Cell::new(false));
    let deferred = Rc::new(std::cell::Cell::new(false));
    let callback = App::process_failure_callback(&app, 0);
    app.borrow_mut().engine = Some(Box::new(PumpingFailureEngine {
        app: Rc::downgrade(&app), callback, active,
        deferred: deferred.clone(), closed: closed.clone(), close_during_call,
    }));
    test_show_page(&app, active, "https://example.test/unsaved-form");
    match operation {
        0 => App::resize_current_then(&app, rect(640, 480), |_| panic!("resize succeeded")),
        1 => App::set_memory_target_current(&app, MemoryUsageTarget::Low),
        2 => App::history(&app, false),
        3 => App::history(&app, true),
        4 => assert!(matches!(App::call_engine(&app, |engine| engine.resize(rect(640, 480))), Some(Ok(())))),
        _ => unreachable!(),
    }
    if !close_during_call {
        assert!(deferred.get(), "browser loss must wait until the COM call returns");
        App::process_engine_notices(&app);
        assert_eq!(app.borrow().engine_generation, 1);
        assert_eq!(App::content_failure_message(&app, active), Some(ContentFailure::BrowserLost.message()));
        assert_eq!(app.borrow().effective_memory_target, None);
    }
    assert!(closed.get());
    assert!(app.borrow().engine.is_none(), "the invalidated engine must never be reinstalled");
}

#[test]
fn engine_recovery_resize_defers_pumped_browser_loss() {
    pumping_operation_does_not_resurrect_an_engine(0, false);
}

#[test]
fn engine_recovery_memory_target_defers_pumped_browser_loss() {
    pumping_operation_does_not_resurrect_an_engine(1, false);
}

#[test]
fn engine_recovery_back_defers_pumped_browser_loss() {
    pumping_operation_does_not_resurrect_an_engine(2, false);
}

#[test]
fn engine_recovery_forward_defers_pumped_browser_loss() {
    pumping_operation_does_not_resurrect_an_engine(3, false);
}

#[test]
fn engine_recovery_all_pumping_operations_close_without_holding_app_borrows() {
    for operation in 0..4 { pumping_operation_does_not_resurrect_an_engine(operation, true); }
}

#[test]
fn engine_recovery_public_call_defers_pumped_browser_loss() {
    pumping_operation_does_not_resurrect_an_engine(4, false);
}

#[test]
fn engine_recovery_public_call_can_close_during_pumped_call() {
    pumping_operation_does_not_resurrect_an_engine(4, true);
}

struct ScopedLossEngine {
    generations: Rc<RefCell<std::collections::HashMap<TabId, u64>>>,
    calls: Rc<RefCell<Vec<String>>>,
}

impl EngineAdapter for ScopedLossEngine {
    fn controller_generation(&self, tab: TabId) -> Option<u64> { self.generations.borrow().get(&tab).copied() }
    fn navigate(&mut self, url: &url::Url) -> windows::core::Result<()> {
        self.calls.borrow_mut().push(format!("navigate:{url}")); Ok(())
    }
    fn navigate_tab(&mut self, tab: TabId, url: &url::Url) -> windows::core::Result<()> {
        self.calls.borrow_mut().push(format!("navigate:{}:{url}", tab.0)); Ok(())
    }
    fn navigate_tab_post(&mut self, tab: TabId, _: &url::Url, _: &str) -> windows::core::Result<()> {
        self.calls.borrow_mut().push(format!("post:{}", tab.0)); Ok(())
    }
    fn can_go_back(&self) -> bool { false }
    fn go_back(&mut self) -> windows::core::Result<()> { Ok(()) }
    fn can_go_forward(&self) -> bool { false }
    fn go_forward(&mut self) -> windows::core::Result<()> { Ok(()) }
    fn resize(&mut self, _: windows::Win32::Foundation::RECT) -> windows::core::Result<()> { Ok(()) }
    fn set_memory_usage_target(&mut self, _: MemoryUsageTarget) -> windows::core::Result<MemoryTargetApply> { Ok(MemoryTargetApply::Applied) }
    fn close_tab(&mut self, tab: TabId) -> windows::core::Result<()> {
        self.generations.borrow_mut().remove(&tab);
        self.calls.borrow_mut().push(format!("close:{}", tab.0)); Ok(())
    }
    fn close(&mut self) -> windows::core::Result<()> {
        self.calls.borrow_mut().push("close-engine".into()); Ok(())
    }
}

fn scoped_loss_app(count: usize) -> (AppRef, Rc<RefCell<std::collections::HashMap<TabId, u64>>>, Rc<RefCell<Vec<String>>>) {
    let app = app_with_pending(rect(800, 600));
    for _ in 1..count { App::new_tab(&app); }
    let ids = App::tab_ids(&app);
    for id in &ids { test_show_page(&app, *id, &format!("https://example.test/tab/{}", id.0)); }
    let generations = Rc::new(RefCell::new(ids.into_iter().map(|id| (id, 7)).collect()));
    let calls = Rc::new(RefCell::new(Vec::new()));
    {
        let mut state = app.borrow_mut();
        state.engine = Some(Box::new(ScopedLossEngine { generations: generations.clone(), calls: calls.clone() }));
        state.initializing = false;
        state.engine_creation_settled = true;
        state.pending = None;
        state.pending_tab = None;
        state.pending_request_body = None;
        for tab in &mut state.tabs { tab.page_usable = true; tab.loading = false; }
    }
    (app, generations, calls)
}

#[test]
fn engine_recovery_scoped_browser_loss_retires_only_its_tab_and_never_replays_post() {
    let (app, generations, calls) = scoped_loss_app(2);
    let ids = App::tab_ids(&app);
    let (old, healthy) = (ids[0], ids[1]);
    let old_url = app.borrow().tabs.iter().find(|tab| tab.id == old).unwrap().url.clone().unwrap();
    {
        let mut state = app.borrow_mut();
        state.pending = Some(old_url.clone());
        state.pending_tab = Some(old);
        state.pending_request_body = Some((old, old_url.clone(), "private=query".into()));
    }
    App::process_failure_callback(&app, 0)(failure(old, engine::ProcessFailureKind::ControllerBrowserExited));
    App::process_engine_notices(&app);
    assert_eq!(app.borrow().engine_generation, 0);
    assert!(app.borrow().engine.is_some());
    assert_eq!(App::content_failure_message(&app, old), Some(ContentFailure::BrowserLost.message()));
    let state = app.borrow();
    let surviving = state.tabs.iter().find(|tab| tab.id == healthy).unwrap();
    assert!(surviving.page_usable && surviving.controller_created && surviving.failure.is_none());
    assert!(state.pending.is_none() && state.pending_request_body.is_none());
    drop(state);
    assert!(!generations.borrow().contains_key(&old));
    assert_eq!(generations.borrow().get(&healthy), Some(&7));
    assert_eq!(*calls.borrow(), [format!("close:{}", old.0)]);
    App::select_tab(&app, old);
    assert_eq!(*calls.borrow(), [format!("close:{}", old.0)], "selection cannot resend the failed request");
    assert!(App::retry_failed_tab(&app, old));
    assert!(calls.borrow().contains(&format!("navigate:{}:{old_url}", old.0)));
    assert!(!calls.borrow().iter().any(|call| call.starts_with("post:") || call == "close-engine"));
}

#[test]
fn engine_recovery_scoped_browser_notice_cannot_retire_same_tab_replacement() {
    let (app, generations, calls) = scoped_loss_app(2);
    let old = App::tab_ids(&app)[0];
    App::process_failure_callback(&app, 0)(failure(old, engine::ProcessFailureKind::ControllerBrowserExited));
    generations.borrow_mut().insert(old, 8);
    test_show_page(&app, old, "https://example.test/replacement");
    App::process_engine_notices(&app);
    assert!(App::content_failure_message(&app, old).is_none());
    assert_eq!(app.borrow().tabs.iter().find(|tab| tab.id == old).unwrap().url.as_ref().unwrap().as_str(), "https://example.test/replacement");
    assert!(calls.borrow().is_empty());
    assert_eq!(generations.borrow().get(&old), Some(&8));
}

#[test]
fn engine_recovery_scoped_browser_notices_deliver_all_affected_tabs_beyond_queue_cap() {
    let (app, generations, calls) = scoped_loss_app(132);
    let ids = App::tab_ids(&app);
    let healthy = *ids.last().unwrap();
    let notify = App::process_failure_callback(&app, 0);
    for id in &ids[..ids.len() - 1] { notify(failure(*id, engine::ProcessFailureKind::ControllerBrowserExited)); }
    assert_eq!(app.borrow().engine_notices.borrow().0.len(), 131);
    App::process_engine_notices(&app);
    for id in &ids[..ids.len() - 1] { assert_eq!(App::content_failure_message(&app, *id), Some(ContentFailure::BrowserLost.message())); }
    assert!(App::content_failure_message(&app, healthy).is_none());
    assert_eq!(generations.borrow().len(), 1);
    assert_eq!(generations.borrow().get(&healthy), Some(&7));
    assert_eq!(app.borrow().engine_generation, 0);
    assert!(!calls.borrow().iter().any(|call| call == "close-engine" || call.starts_with("navigate:") || call.starts_with("post:")));
}

#[test]
fn engine_recovery_scoped_browser_duplicates_coalesce_and_global_loss_still_dominates() {
    let mut notices = EngineNotices::default();
    let scoped = EngineNotice::Process(failure(TabId(1), engine::ProcessFailureKind::ControllerBrowserExited));
    assert!(notices.push(0, scoped));
    assert!(!notices.push(0, scoped));
    assert_eq!(notices.0.len(), 1);
    notices.push(0, EngineNotice::BrowserLost(42));
    assert_eq!(notices.0, [(0, EngineNotice::BrowserLost(42))]);
    notices.push(0, scoped);
    assert_eq!(notices.0, [(0, EngineNotice::BrowserLost(42))]);
}

#[test]
fn engine_recovery_scoped_browser_events_from_retired_engine_or_shutdown_are_inert() {
    let (app, _, calls) = scoped_loss_app(2);
    let active = App::active_tab_id(&app);
    let old = App::process_failure_callback(&app, 0);
    app.borrow_mut().engine_generation = 1;
    old(failure(active, engine::ProcessFailureKind::ControllerBrowserExited));
    App::process_engine_notices(&app);
    app.borrow_mut().closing = true;
    App::process_failure_callback(&app, 1)(failure(active, engine::ProcessFailureKind::ControllerBrowserExited));
    App::process_engine_notices(&app);
    assert!(App::content_failure_message(&app, active).is_none());
    assert!(calls.borrow().is_empty());
    assert_eq!(app.borrow().engine_generation, 1);
}
