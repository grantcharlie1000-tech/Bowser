//! One explicit Windows-only correctness probe. Real WebApps controllers, not a performance arm.
//! Uses only a fresh disposable profile and the workflow's unique loopback fixture. No UI/focus
//! repair, product test commands, repeated attempt or resource/gain claim.
use super::*;
use std::path::PathBuf;
use windows::{
    core::w,
    Win32::{
        System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
        UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, DispatchMessageW, MSG,
            PeekMessageW, TranslateMessage, PM_REMOVE, WINDOW_EX_STYLE, WS_POPUP},
    },
};

type CompletionHook = Box<dyn FnOnce(super::super::web_app_profile::NativeRequest, bool)>;
thread_local! {
    static COMPLETION_HOOK: RefCell<Option<CompletionHook>> = const { RefCell::new(None) };
}
struct CompletionHookScope;
impl Drop for CompletionHookScope {
    fn drop(&mut self) { COMPLETION_HOOK.with(|slot| { let _ = slot.borrow_mut().take(); }); }
}
/// One-shot test seam at actual native completion entry; no queue, delay, fake result or replay.
/// Release the hook-storage borrow BEFORE shutdown can reenter a real native callback.
pub(super) fn environment_completion_entry(request: &super::super::web_app_profile::NativeRequest,
    native_success: bool) {
    let hook = COMPLETION_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook { hook(request.clone(), native_success); }
}

fn pump_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if done() { return true; }
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    done()
}
struct Cleanup { window: HWND, log: Rc<crate::performance::PerfLog> }
impl Drop for Cleanup {
    fn drop(&mut self) {
        super::shutdown_profiles();
        unsafe { let _ = DestroyWindow(self.window); }
        // On a panic, retain partial raw evidence without inventing a complete end marker.
        let _ = self.log.diagnostic_checkpoint();
        unsafe { CoUninitialize(); }
    }
}

#[test]
#[ignore = "Real WebView2 runtime, explicit loopback fixture and disposable output directory required"]
fn live_webapp_generation_remove_readd_clear_shutdown() {
    let base = std::env::var("BOWSER_WEBAPP_FIXTURE_BASE").expect("explicit fixture required");
    let parsed = Url::parse(&base).unwrap();
    assert_eq!(parsed.scheme(), "http");
    assert_eq!(parsed.host_str(), Some("127.0.0.1"));
    assert!(parsed.port().is_some());
    let out = PathBuf::from(std::env::var("BOWSER_WEBAPP_NATIVE_OUT").expect("explicit disposable output required"));
    std::fs::create_dir_all(&out).unwrap();
    let root = std::env::temp_dir().join(format!("bowser-webapp-native-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&root).unwrap();
    let log = Rc::new(crate::performance::PerfLog::new(Some(&out.join("events.csv"))).unwrap());
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok().unwrap();
    let window = unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"),
        w!("Bowser isolated WebApps lifecycle correctness"), WS_POPUP, 0, 0, 800, 600,
        None, None, None, None) }.unwrap();
    let _cleanup = Cleanup { window, log: log.clone() };
    let bounds = RECT { left: 0, top: 0, right: 800, bottom: 600 };
    let titles = Rc::new(RefCell::new(Vec::<(String, String)>::new()));
    let failed = Rc::new(RefCell::new(Vec::<String>::new()));
    let states = Rc::new(RefCell::new(HashMap::new()));
    let seen_states = states.clone();
    let seen_titles = titles.clone(); let seen_failures = failed.clone();
    let events = WebAppEvents {
        diagnostic_lifecycle: Some(Rc::downgrade(&log)),
        title_changed: Some(Rc::new(move |id, title| seen_titles.borrow_mut().push((id.to_owned(), title)))),
        navigation_state: Some(Rc::new(move |id, state| { seen_states.borrow_mut().insert(id.to_owned(), state); })),
        failed: Some(Rc::new(move |_, message| seen_failures.borrow_mut().push(message))),
        ..Default::default()
    };
    let slot = Rc::new(RefCell::new(None)); let completion = slot.clone();
    let profile = root.join("WebApps");
    WebAppHost::begin_create(window, bounds, &profile, events.clone(), move |result| *completion.borrow_mut() = Some(result)).unwrap();
    assert!(pump_until(Duration::from_secs(30), || slot.borrow().is_some()), "environment completion unavailable");
    let host = slot.borrow_mut().take().unwrap().unwrap();
    let a = format!("{base}/light?token=native_a");
    let old_b = format!("{base}/light?token=native_b_retired");
    let new_b = format!("{base}/light?token=native_b_current");
    host.open_app("1", &a).unwrap(); host.open_app("1", &a).unwrap();
    assert!(pump_until(Duration::from_secs(30), || host.has_app("1")
        && titles.borrow().iter().any(|(_, title)| title == "BOWSER_PERF_READY_native_a")
        && states.borrow().get("1") == Some(&AppNavigationState::Ready)), "A never ready");
    // A-ready -> first B-open, then remove while the REAL native request is still pending. A
    // synchronous completion would make this run unexercised, not a silently accepted race test.
    host.open_app("2", &old_b).unwrap();
    assert!(!host.has_app("2"), "pending-remove race was not exercised");
    let retired_at = titles.borrow().len();
    host.remove_app("2"); host.open_app("2", &new_b).unwrap(); host.open_app("1", &a).unwrap();
    let latest = RECT { left: 0, top: 0, right: 640, bottom: 480 };
    host.set_bounds(latest).unwrap(); host.hide_all().unwrap();
    // Hidden controllers may suppress the fixture's double-requestAnimationFrame title/ack.
    // First prove native navigation and latest hidden presentation, then separately show the
    // existing B controller to require the unchanged visual-readiness fixture signal.
    assert!(pump_until(Duration::from_secs(30), || host.has_app("2")
        && states.borrow().get("2") == Some(&AppNavigationState::Ready)), "re-added B native navigation never ready");
    {
        let state = host.state.borrow();
        assert_eq!(state.apps.len(), 2); assert!(state.current_app.is_none());
        for app in state.apps.values() {
            let mut actual = RECT::default(); let mut visible = BOOL::default();
            unsafe { app.controller.Bounds(&mut actual).unwrap(); app.controller.IsVisible(&mut visible).unwrap(); }
            assert_eq!(actual, latest); assert!(!visible.as_bool());
        }
    }
    host.show_app("2", latest).unwrap();
    assert!(pump_until(Duration::from_secs(30), || titles.borrow().iter()
        .any(|(_, title)| title == "BOWSER_PERF_READY_native_b_current")
        && states.borrow().get("2") == Some(&AppNavigationState::Ready)), "shown re-added B fixture never ready");
    {
        let state = host.state.borrow();
        assert_eq!(state.current_app.as_deref(), Some("2"));
        for (id, app) in &state.apps {
            let mut visible = BOOL::default();
            unsafe { app.controller.IsVisible(&mut visible).unwrap(); }
            assert_eq!(visible.as_bool(), id == "2");
        }
    }
    host.hide_all().unwrap();
    assert!(!titles.borrow()[retired_at..].iter().any(|(_, title)| title.contains("native_b_retired")), "retired creation navigated or delivered title after removal");
    assert!(failed.borrow().is_empty(), "unexpected UI failure");
    super::begin_clear_profile(&profile).unwrap();
    assert_eq!(super::finish_clear_profile(&profile).unwrap_err().kind(), std::io::ErrorKind::WouldBlock,
        "live controllers must not permit erase");
    host.close();
    assert!(host.open_app("1", &a).is_err(), "closed host reopened during clear");
    // Wait only for passive release evidence, then make ONE explicit clear retry. No erase is
    // scheduled by a production callback or performed by the polling predicate.
    assert!(pump_until(Duration::from_secs(30), || super::super::web_app_profile::clear_ready_for_test(&profile)),
        "native release never became affirmative; no erase success claimed");
    super::finish_clear_profile(&profile).unwrap(); assert!(!profile.exists());
    assert!(host.open_app("1", &a).is_err()); assert!(!profile.exists());

    // Deterministically shut down at a REAL native environment completion entry while its request
    // is outstanding, before registration/on_ready. This is NOT a naturally asynchronous scheduling
    // claim: the callback may run reentrantly before begin_create returns. No callback is delayed.
    let shutdown_root = root.join("shutdown"); std::fs::create_dir(&shutdown_root).unwrap();
    let shutdown_profile = shutdown_root.join("WebApps");
    let late = Rc::new(Cell::new(false)); let late_done = late.clone();
    let hook_count = Rc::new(Cell::new(0)); let observed_count = hook_count.clone();
    let observed = Rc::new(RefCell::new(None)); let hook_observed = observed.clone();
    let late_refused = Rc::new(Cell::new(false)); let callback_refused = late_refused.clone();
    let shutdown_url = format!("{base}/light?token=native_shutdown_refused");
    let _hook_scope = CompletionHookScope;
    assert!(COMPLETION_HOOK.with(|slot| slot.borrow().is_none()));
    COMPLETION_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(move |request, native_success| {
        observed_count.set(observed_count.get() + 1);
        *hook_observed.borrow_mut() = Some((native_success, request.snapshot_for_test(), request));
        super::shutdown_profiles();
    })));
    WebAppHost::begin_create(window, bounds, &shutdown_profile, events, move |result| {
        match result {
            Ok(host) => {
                callback_refused.set(host.open_app("3", &shutdown_url).is_err());
                host.close();
            }
            Err(_) => callback_refused.set(true),
        }
        late_done.set(true);
    }).unwrap();
    assert!(pump_until(Duration::from_secs(30), || late.get()), "shutdown callback never retired");
    assert_eq!(hook_count.get(), 1, "real native completion-entry shutdown was not exercised exactly once");
    let observed = observed.borrow();
    let (native_success, (was_pending, was_dispatched, observers_before), request) = observed.as_ref().unwrap();
    assert!(*native_success && *was_pending && *was_dispatched,
        "shutdown seam lacked a real successful dispatched outstanding native request");
    assert_eq!(*observers_before, 0);
    assert_eq!(request.snapshot_for_test(), (false, true, 0), "request did not retire or observer registered after shutdown");
    assert!(late_refused.get(), "late environment created a controller after shutdown");
    let refused = root.join("refused"); std::fs::create_dir(&refused).unwrap(); let refused = refused.join("WebApps");
    assert!(WebAppHost::begin_create(window, bounds, &refused, WebAppEvents::default(), |_| {}).is_err());
    assert!(!refused.exists(), "shutdown request touched profile storage");
    assert!(super::finish_clear_profile(&shutdown_profile).is_err(), "unknown shutdown profile release became erase success");
    assert!(log.failure().is_none());
    log.checkpoint_sidebar_if_observed().unwrap();
    assert!(log.failure().is_none());
    let csv = std::fs::read_to_string(out.join("events.csv")).unwrap();
    let count = |event: &str| csv.lines().filter(|line| line.starts_with(&format!("{event},"))).count();
    assert_eq!(count("webapp_controller_requested"), 3);
    assert_eq!(count("webapp_controller_installed"), 2);
    assert_eq!(count("webapp_controller_replaced"), 0);
    assert_eq!(count("webapp_controller_close_requested"), 3);
    assert_eq!(count("webapp_navigation_success"), 2);
    assert_eq!(count("webapp_observation_error"), 0);
    assert_eq!(count("webapp_observation_end"), 1);
    let rows = csv.lines().skip(1).map(|line| line.split(',').collect::<Vec<_>>()).collect::<Vec<_>>();
    let requested = rows.iter().filter(|row| row[0] == "webapp_controller_close_requested").map(|row| row[4]).collect::<Vec<_>>();
    let closed = rows.iter().filter(|row| row[0] == "webapp_controller_close_result").collect::<Vec<_>>();
    assert_eq!(closed.len(), 3);
    for id in requested {
        assert_eq!(closed.iter().filter(|row| row[4] == id && row[5] == "true").count(), 1);
    }
    assert!(rows.iter().filter(|row| row[0] == "webapp_observation_end").all(|row| row[5] == "true"));
    std::fs::write(out.join("result.json"), "{\"scope\":\"real isolated engine correctness only; no benchmark\",\"pending_remove_readd_exercised\":true,\"shutdown_at_real_environment_completion_entry_exercised\":true,\"naturally_async_environment_shutdown_exercised\":false,\"controller_requests\":3,\"installed\":2,\"replaced\":0,\"close_requests\":3,\"clear_release_confirmed\":true,\"unknown_shutdown_erase_refused\":true}\n").unwrap();
}
