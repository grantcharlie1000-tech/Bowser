use super::*;

#[implement(ICoreWebView2BrowserProcessExitedEventArgs)]
struct ExitArgs {
    pid: u32,
    kind: COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND,
    unreadable: bool,
}

impl ICoreWebView2BrowserProcessExitedEventArgs_Impl for ExitArgs_Impl {
    fn BrowserProcessId(&self, value: *mut u32) -> Result<()> {
        if self.unreadable { return Err(Error::from(windows::Win32::Foundation::E_FAIL)); }
        unsafe { *value = self.pid; }
        Ok(())
    }
    fn BrowserProcessExitKind(&self, value: *mut COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND) -> Result<()> {
        unsafe { *value = self.kind; }
        Ok(())
    }
}

fn emit(handler: &ICoreWebView2BrowserProcessExitedEventHandler, pid: u32, kind: COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND) {
    let args: ICoreWebView2BrowserProcessExitedEventArgs = ExitArgs { pid, kind, unreadable: false }.into();
    unsafe { handler.Invoke(None::<&ICoreWebView2Environment>, &args).unwrap(); }
}

fn watch() -> (Rc<BrowserLossWatch>, Rc<RefCell<Vec<u32>>>) {
    let delivered = Rc::new(RefCell::new(Vec::new()));
    let sink = delivered.clone();
    let watch = Rc::new(BrowserLossWatch::new(Rc::new(Cell::new(false)), Some(Rc::new(move |pid| sink.borrow_mut().push(pid)))));
    (watch, delivered)
}

fn content_failure() -> ProcessFailure {
    ProcessFailure { tab: TabId(1), controller_generation: 7, kind: ProcessFailureKind::BrowserExited, reason: None, exit_code: None }
}

#[implement(ICoreWebView2ProcessFailedEventArgs)]
struct FailureArgs {
    kind: COREWEBVIEW2_PROCESS_FAILED_KIND,
    during_get: Option<Rc<dyn Fn()>>,
}

impl ICoreWebView2ProcessFailedEventArgs_Impl for FailureArgs_Impl {
    fn ProcessFailedKind(&self, value: *mut COREWEBVIEW2_PROCESS_FAILED_KIND) -> Result<()> {
        unsafe { *value = self.kind; }
        if let Some(during_get) = self.during_get.as_ref() { during_get(); }
        Ok(())
    }
}

fn emit_content(handler: &ICoreWebView2ProcessFailedEventHandler, kind: COREWEBVIEW2_PROCESS_FAILED_KIND, during_get: Option<Rc<dyn Fn()>>) {
    let args: ICoreWebView2ProcessFailedEventArgs = FailureArgs { kind, during_get }.into();
    unsafe { handler.Invoke(None::<&ICoreWebView2>, &args).unwrap(); }
}

#[test]
fn engine_recovery_current_old_controller_reports_loss_without_retiring_live_replacement() {
    for attached_before_exit in [false, true] {
        // Equal PIDs and backwards creation times must use identity equality, never ordering.
        for (old_pid, old_created, new_pid, new_created) in [(42, 10, 43, 20), (42, 20, 43, 10), (42, 20, 42, 10)] {
            let mut engine = super::tests::engine_without_controllers();
            let core = engine.core.clone();
            let watch = core.borrow().browser_watch.clone();
            let old = BrowserIdentity { pid: old_pid, created: old_created };
            let new = BrowserIdentity { pid: new_pid, created: new_created };
            assert!(watch.attach(old));
            let (generation, lifetime) = core.borrow_mut().begin_controller_lifetime(TabId(1));
            if attached_before_exit { assert!(watch.attach(new)); }
            // Model the validated live snapshot from GetProcessInfos + OS lifetime validation.
            // The old content controller has neither closed nor been superseded.
            watch.environment_exited(old.pid, Some(new));
            let delivered = Rc::new(RefCell::new(Vec::new()));
            let sink = delivered.clone();
            let callback_watch = watch.clone();
            let callback_lifetime = lifetime.clone();
            let handler = content_failed_handler(TabId(1), generation, old, watch.clone(), lifetime, Rc::new(move |failure| {
                assert!(!callback_watch.lost.get(), "the replacement browser must remain usable");
                assert!(!callback_lifetime.get(), "revoke before posting to App or reentering");
                sink.borrow_mut().push(failure);
                callback_watch.content_failed(old, &callback_lifetime, &|_| panic!("duplicate reentry"), failure);
            }));
            let replacement_generation = core.borrow_mut().begin_controller_lifetime(TabId(2)).0;
            {
                let state = core.borrow_mut();
                // The COM adapter must not borrow EngineCore, including when a getter pumps.
                emit_content(&handler, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED, None);
                emit_content(&handler, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED, None);
                assert!(!state.controller_request_current(TabId(1), generation));
                assert!(state.controller_request_current(TabId(2), replacement_generation));
            }
            assert_eq!(*delivered.borrow(), [ProcessFailure {
                tab: TabId(1), controller_generation: generation,
                kind: ProcessFailureKind::ControllerBrowserExited, reason: None, exit_code: None,
            }]);
            assert!(watch.attach(new), "a scoped loss must not poison the environment");
            engine.close_tab(TabId(1)).unwrap();
            assert!(core.borrow().controller_request_current(TabId(2), replacement_generation));
            engine.close().unwrap();
        }
    }
}

#[test]
fn engine_recovery_old_pending_controller_cannot_publish_or_post_after_scoped_browser_loss() {
    let mut engine = super::tests::engine_without_controllers();
    let core = engine.core.clone();
    let watch = core.borrow().browser_watch.clone();
    let old = BrowserIdentity { pid: 42, created: 10 };
    let new = BrowserIdentity { pid: 43, created: 20 };
    watch.attach(old);
    let (generation, lifetime) = core.borrow_mut().begin_controller_lifetime(TabId(1));
    let url = Url::parse("https://example.test/post").unwrap();
    {
        let mut state = core.borrow_mut();
        state.creating.insert(TabId(1));
        state.pending.insert(TabId(1), url.clone());
        state.pending_post.insert(TabId(1), (url, "private=query".into()));
    }
    // A chrome/panel controller attaches the replacement while the old content controller configures.
    watch.attach(new);
    let delivered = Rc::new(Cell::new(0));
    let sink = delivered.clone();
    let handler = content_failed_handler(TabId(1), generation, old, watch.clone(), lifetime, Rc::new(move |failure| {
        assert_eq!(failure.kind, ProcessFailureKind::ControllerBrowserExited);
        sink.set(sink.get() + 1);
    }));
    {
        let state = core.borrow_mut();
        assert!(state.controller_request_current(TabId(1), generation));
        emit_content(&handler, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED, None);
        assert!(!state.controller_request_current(TabId(1), generation), "production completion and pre-navigation guards reject the queued POST before App dispatch");
    }
    assert_eq!(delivered.get(), 1);
    assert!(!watch.lost.get());
    engine.close_tab(TabId(1)).unwrap();
    assert!(core.borrow().pending_post.is_empty());
    assert!(core.borrow().pending.is_empty());
    assert!(!core.borrow().creating.contains(&TabId(1)));
    engine.close().unwrap();
    emit_content(&handler, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED, None);
    assert_eq!(delivered.get(), 1);
}

#[test]
fn engine_recovery_closed_or_superseded_old_controller_cannot_fail_replacement() {
    for supersede in [false, true] {
        let engine = Rc::new(RefCell::new(super::tests::engine_without_controllers()));
        let core = engine.borrow().core.clone();
        let watch = core.borrow().browser_watch.clone();
        let old = BrowserIdentity { pid: 42, created: 10 };
        let new = BrowserIdentity { pid: 43, created: 20 };
        watch.attach(old);
        let (generation, lifetime) = core.borrow_mut().begin_controller_lifetime(TabId(1));
        let handler = content_failed_handler(TabId(1), generation, old, watch.clone(), lifetime, Rc::new(|_| panic!("revoked controller")));
        watch.attach(new);
        let closing_engine = engine.clone();
        let replacing_core = core.clone();
        emit_content(&handler, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED, Some(Rc::new(move || {
            if supersede {
                replacing_core.borrow_mut().begin_controller_lifetime(TabId(1));
            } else {
                closing_engine.borrow_mut().close_tab(TabId(1)).unwrap();
            }
        })));
        watch.environment_exited(old.pid, Some(new));
        emit_content(&handler, COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED, None);
        assert!(!watch.lost.get());
        assert!(watch.attach(new));
        engine.borrow_mut().close().unwrap();
    }
}

#[test]
fn engine_recovery_current_old_controller_keeps_renderer_notices_after_new_browser_attachment() {
    let (watch, environment_notices) = watch();
    let old = BrowserIdentity { pid: 42, created: 10 };
    let new = live_browser_identity(std::process::id()).unwrap();
    watch.attach(old);
    watch.attach(new);
    // Exercise the real environment COM adapter with a live OS process as the replacement stand-in.
    emit(&browser_exited_handler(&watch, None), old.pid, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED);
    let notices = Rc::new(RefCell::new(Vec::new()));
    let sink = notices.clone();
    let handler = content_failed_handler(TabId(1), 7, old, watch.clone(), Rc::new(Cell::new(true)), Rc::new(move |failure| sink.borrow_mut().push(failure)));
    emit_content(&handler, COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED, None);
    emit_content(&handler, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED, None);
    assert_eq!(*notices.borrow(), [
        ProcessFailure { kind: ProcessFailureKind::RendererExited, ..content_failure() },
        ProcessFailure { kind: ProcessFailureKind::ControllerBrowserExited, ..content_failure() },
    ]);
    assert!(environment_notices.borrow().is_empty());
    assert!(!watch.lost.get());
}

#[test]
fn engine_recovery_each_current_old_controller_reports_once_and_new_browser_can_still_fail() {
    let (watch, environment_notices) = watch();
    let old = BrowserIdentity { pid: 42, created: 10 };
    let new = BrowserIdentity { pid: 43, created: 20 };
    watch.attach(old);
    watch.environment_exited(old.pid, Some(new));
    let notices = Rc::new(RefCell::new(Vec::new()));
    for tab in [TabId(1), TabId(2)] {
        let sink = notices.clone();
        let handler = content_failed_handler(tab, 7, old, watch.clone(), Rc::new(Cell::new(true)), Rc::new(move |failure| sink.borrow_mut().push(failure)));
        emit_content(&handler, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED, None);
        emit_content(&handler, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED, None);
    }
    assert_eq!(*notices.borrow(), [
        ProcessFailure { tab: TabId(1), kind: ProcessFailureKind::ControllerBrowserExited, ..content_failure() },
        ProcessFailure { tab: TabId(2), kind: ProcessFailureKind::ControllerBrowserExited, ..content_failure() },
    ]);
    watch.environment_exited(new.pid, None);
    watch.environment_exited(new.pid, None);
    assert!(watch.lost.get());
    assert_eq!(*environment_notices.borrow(), [new.pid]);
}

#[test]
fn engine_recovery_environment_com_callback_detects_loss_without_any_content_controller() {
    let (watch, delivered) = watch();
    let handler = browser_exited_handler(&watch, None);
    emit(&handler, 42, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED);
    emit(&handler, 42, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED);
    assert!(watch.lost.get());
    assert_eq!(*delivered.borrow(), [42]);
}

#[test]
fn engine_recovery_environment_and_content_events_coalesce_in_both_orders() {
    for environment_first in [false, true] {
        let (watch, delivered) = watch();
        let handler = browser_exited_handler(&watch, None);
        let identity = BrowserIdentity { pid: 42, created: 10 };
        assert!(watch.attach(identity));
        let content = Cell::new(0);
        let callback = |_| { assert!(watch.lost.get()); content.set(content.get() + 1); };
        if environment_first { emit(&handler, 42, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED); }
        watch.content_failed(identity, &Cell::new(true), &callback, content_failure());
        emit(&handler, 42, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED);
        watch.content_failed(identity, &Cell::new(true), &callback, content_failure());
        assert_eq!(content.get() + delivered.borrow().len(), 1);
        assert!(watch.lost.get());
    }
}

#[test]
fn engine_recovery_environment_normal_unknown_and_unreadable_exits_do_not_claim_loss() {
    let (watch, delivered) = watch();
    let logs = Rc::new(RefCell::new(Vec::new()));
    let sink = logs.clone();
    let handler = browser_exited_handler(&watch, Some(Rc::new(move |tag| sink.borrow_mut().push(tag))));
    emit(&handler, 42, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_NORMAL);
    emit(&handler, 42, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND(99));
    emit(&handler, 0, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED);
    let args: ICoreWebView2BrowserProcessExitedEventArgs = ExitArgs { pid: 42, kind: COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED, unreadable: true }.into();
    unsafe { handler.Invoke(None::<&ICoreWebView2Environment>, &args).unwrap(); }
    unsafe { handler.Invoke(None::<&ICoreWebView2Environment>, None::<&ICoreWebView2BrowserProcessExitedEventArgs>).unwrap(); }
    assert!(!watch.lost.get());
    assert!(delivered.borrow().is_empty());
    assert_eq!(logs.borrow().len(), 4);
    // Normal shutdown does not poison the environment; a new browser can attach to it.
    assert!(watch.attach(BrowserIdentity { pid: 43, created: 20 }));
    emit(&handler, 43, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED);
    assert_eq!(*delivered.borrow(), [43]);
}

#[test]
fn engine_recovery_old_pid_and_controller_lifetimes_cannot_retire_new_attachment() {
    let (watch, delivered) = watch();
    let old = BrowserIdentity { pid: 42, created: 10 };
    let new = BrowserIdentity { pid: 43, created: 20 };
    assert!(watch.attach(old));
    assert!(watch.attach(new));
    let handler = browser_exited_handler(&watch, None);
    emit(&handler, old.pid, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED);
    watch.content_failed(old, &Cell::new(false), &|_| panic!("closed/superseded old controller"), content_failure());
    watch.content_failed(new, &Cell::new(false), &|_| panic!("closed controller"), content_failure());
    assert!(!watch.lost.get());
    emit(&handler, new.pid, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED);
    assert_eq!(*delivered.borrow(), [new.pid]);
}

#[test]
fn engine_recovery_reused_pid_and_a_late_environment_exit_preserve_live_replacement() {
    let (watch, delivered) = watch();
    let old = BrowserIdentity { pid: 42, created: 10 };
    let new = BrowserIdentity { pid: 42, created: 20 };
    watch.attach(old);
    watch.attach(new);
    watch.environment_exited(old.pid, Some(new));
    watch.content_failed(old, &Cell::new(false), &|_| panic!("closed/superseded old controller"), content_failure());
    assert!(!watch.lost.get());
    watch.environment_exited(new.pid, None);
    assert_eq!(*delivered.borrow(), [new.pid]);
}

#[test]
fn engine_recovery_browser_attachment_identity_does_not_assume_wall_clock_order() {
    let (watch, delivered) = watch();
    let old = BrowserIdentity { pid: 42, created: 20 };
    let new = BrowserIdentity { pid: 43, created: 10 };
    assert!(watch.attach(old));
    assert!(watch.attach(new));
    watch.environment_exited(old.pid, None);
    watch.content_failed(old, &Cell::new(false), &|_| panic!("closed/superseded old controller"), content_failure());
    assert!(!watch.lost.get());
    watch.environment_exited(new.pid, None);
    assert_eq!(*delivered.borrow(), [new.pid]);
}

#[test]
fn engine_recovery_browser_identity_checks_a_real_process_lifetime_without_launching_one() {
    assert!(live_browser_identity(0).is_none());
    let current = live_browser_identity(std::process::id()).unwrap();
    assert_eq!(current, live_browser_identity(std::process::id()).unwrap());
    let (watch, delivered) = watch();
    watch.attach(current);
    // Even without Environment8, a live recorded attachment makes this an old PID-only event.
    emit(&browser_exited_handler(&watch, None), current.pid, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED);
    assert!(!watch.lost.get() && delivered.borrow().is_empty());
}

#[test]
fn engine_recovery_environment_loss_stops_pending_controller_work_before_app_dispatch() {
    let mut engine = super::tests::engine_without_controllers();
    let core = engine.core.clone();
    let watch = core.borrow().browser_watch.clone();
    let handler = browser_exited_handler(&watch, None);
    let url = Url::parse("https://example.test/post").unwrap();
    {
        let mut state = core.borrow_mut();
        state.creating.insert(TabId(1));
        state.controller_generations.insert(TabId(1), 7);
        state.controller_lifetimes.insert(TabId(1), Rc::new(Cell::new(true)));
        assert!(state.controller_request_current(TabId(1), 7));
        assert!(!state.controller_request_current(TabId(1), 6), "obsolete completion generation");
        state.pending.insert(TabId(1), url.clone());
        state.pending_post.insert(TabId(1), (url, "private=query".into()));
        emit(&handler, 42, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED);
        assert!(state.browser_lost.get(), "callback must not need the EngineCore borrow");
        assert!(!state.controller_request_current(TabId(1), 7), "production completion guard must reject it before configuration or POST");
    }
    // This is the production controller-entry boundary; without the latch it needs a COM env.
    assert!(WebView2Engine::request_controller(&core, TabId(2), false).is_ok());
    assert!(!core.borrow().creating.contains(&TabId(2)));
    engine.close().unwrap();
    assert!(core.borrow().pending_post.is_empty());
    assert!(core.borrow().controller_generations.is_empty());
    assert!(!watch.attach(BrowserIdentity { pid: 43, created: 20 }));
}

#[test]
fn engine_recovery_unregister_reentry_and_drop_disable_observers_without_rc_cycles() {
    let mut engine = super::tests::engine_without_controllers();
    let core = engine.core.clone();
    let watch = core.borrow().browser_watch.clone();
    let weak = Rc::downgrade(&watch);
    let handler = browser_exited_handler(&watch, None);
    let retained_handler = handler.clone();
    let removed = Rc::new(Cell::new(0));
    let count = removed.clone();
    let reentry_core = Rc::downgrade(&core);
    engine.browser_exit_registration = Some(BrowserExitRegistration { watch: watch.clone(), unregister: Some(Box::new(move || {
        let core = reentry_core.upgrade().unwrap();
        let state = core.borrow_mut();
        assert_eq!(state.browser_watch.state.get(), BrowserWatchState::Shutdown);
        emit(&handler, 42, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED);
        count.set(count.get() + 1);
    })) });
    engine.close().unwrap();
    engine.close().unwrap();
    assert_eq!(removed.get(), 1);
    drop(engine);
    drop(core);
    drop(watch);
    assert!(weak.upgrade().is_none(), "retained COM handler cannot retain the engine/watch");
    emit(&retained_handler, 42, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED);
}

#[test]
fn engine_recovery_loss_notification_reentry_is_latched_before_the_consumer_runs() {
    let slot: Rc<RefCell<Option<std::rc::Weak<BrowserLossWatch>>>> = Rc::new(RefCell::new(None));
    let observer_slot = slot.clone();
    let calls = Rc::new(Cell::new(0));
    let sink = calls.clone();
    let watch = Rc::new(BrowserLossWatch::new(Rc::new(Cell::new(false)), Some(Rc::new(move |pid| {
        sink.set(sink.get() + 1);
        let watch = observer_slot.borrow().as_ref().unwrap().upgrade().unwrap();
        assert!(watch.lost.get());
        watch.environment_exited(pid, None);
    }))));
    *slot.borrow_mut() = Some(Rc::downgrade(&watch));
    emit(&browser_exited_handler(&watch, None), 42, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED);
    assert_eq!(calls.get(), 1);
}

impl WebView2Engine {
    pub(crate) fn probe_environment_loss_engine(callback: Rc<dyn Fn(u32)>) -> (Self, Rc<dyn Fn(u32)>) {
        let engine = super::tests::engine_without_controllers();
        let lost = engine.core.borrow().browser_lost.clone();
        let watch = Rc::new(BrowserLossWatch::new(lost, Some(callback)));
        engine.core.borrow_mut().browser_watch = watch.clone();
        let handler = browser_exited_handler(&watch, None);
        (engine, Rc::new(move |pid| emit(&handler, pid, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED)))
    }
}
