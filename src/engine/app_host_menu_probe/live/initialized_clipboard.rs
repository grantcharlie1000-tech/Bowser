//! Separate, held-for-review native Copy experiment on an explicitly initialized
//! empty clipboard in a dedicated new hosted Windows VM. Not a zero-lane rescue.
//! Environment values bind provenance; the reviewed job is the out-of-band
//! disposable-resource contract, not these strings or a claim of atomic custody.
use super::*;

const INITIALIZED_TEST: &str = "engine::app_host_menu_probe::live::initialized_clipboard::live_probe_initialized_empty_native_copy";
const CONTRACT: &str = "github-hosted-fresh-exclusive-empty-v1";
const JOB: &str = "initialized-empty-copy";
const KEYS: [&str; 14] = [
    "GITHUB_ACTIONS", "RUNNER_ENVIRONMENT", "RUNNER_OS", "GITHUB_REPOSITORY", "GITHUB_JOB",
    "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_SHA", "ImageVersion",
    "BOWSER_INITIALIZED_EMPTY_CLIPBOARD_CONTRACT", "BOWSER_INITIALIZED_EMPTY_CLIPBOARD_JOB",
    "BOWSER_INITIALIZED_EMPTY_CLIPBOARD_CHECKOUT", "BOWSER_INITIALIZED_EMPTY_CLIPBOARD_RUN",
    "BOWSER_INITIALIZED_EMPTY_CLIPBOARD_ATTEMPT",
];

#[derive(Clone, Debug)]
pub(super) struct FixtureContract { run: String, attempt: String, checkout: String, image: String }

fn positive_decimal(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && !value.starts_with('0')
        && value.bytes().all(|b| b.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|v| v > 0)
}
fn validate(values: &[String]) -> Option<FixtureContract> {
    if values.len() != KEYS.len() { return None }
    let at = |name| values[KEYS.iter().position(|key| *key == name).unwrap()].as_str();
    if at("GITHUB_ACTIONS") != "true" || at("RUNNER_ENVIRONMENT") != "github-hosted"
        || at("RUNNER_OS") != "Windows" || at("GITHUB_REPOSITORY") != "grantcharlie1000-tech/Bowser"
        || at("GITHUB_JOB") != JOB || at("BOWSER_INITIALIZED_EMPTY_CLIPBOARD_CONTRACT") != CONTRACT
        || at("BOWSER_INITIALIZED_EMPTY_CLIPBOARD_JOB") != JOB { return None }
    let (run, attempt, checkout, image) = (at("GITHUB_RUN_ID"), at("GITHUB_RUN_ATTEMPT"), at("GITHUB_SHA"), at("ImageVersion"));
    if !positive_decimal(run) || !positive_decimal(attempt) || attempt.len() > 10 || checkout.len() != 40
        || !checkout.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || image.is_empty() || image.len() > 80
        || !image.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || at("BOWSER_INITIALIZED_EMPTY_CLIPBOARD_RUN") != run
        || at("BOWSER_INITIALIZED_EMPTY_CLIPBOARD_ATTEMPT") != attempt
        || at("BOWSER_INITIALIZED_EMPTY_CLIPBOARD_CHECKOUT") != checkout { return None }
    Some(FixtureContract { run: run.into(), attempt: attempt.into(), checkout: checkout.into(), image: image.into() })
}
pub(super) fn contract() -> FixtureContract {
    let values = KEYS.iter().map(|key| std::env::var(key).unwrap_or_default()).collect::<Vec<_>>();
    validate(&values).expect("separate initialized-empty Copy requires the exact dedicated hosted-VM job contract")
}
impl FixtureContract {
    fn json(&self) -> String {
        format!(r#"{{"scope":"explicit-initialized-environment-precondition-not-sequence-authority","provisioning_contract":"reviewed-dedicated-new-hosted-Windows-VM-no-personal-or-RDP-clipboard-workload","job":{},"run":{},"attempt":{},"checkout":{},"image":{},"metadata_alone_is_not_exclusivity_proof":true,"continuous_handoff_custody_claimed":false,"uninitialized_zero_lane":"separate-unchanged-not-this-result"}}"#,
            quote(JOB), quote(&self.run), quote(&self.attempt), quote(&self.checkout), quote(&self.image))
    }
}

pub(super) fn assert_initialized_evidence(report: &Value) {
    let init = field(report, "initialized_empty_fixture").expect("explicit initializer evidence absent");
    assert_eq!(text(init, "scope"), "environment-precondition-only-not-native-copy-or-continuous-custody");
    for key in ["selected", "initialization_attempted", "initialization_succeeded", "after_format_empty", "close_attempted", "close_confirmed", "owner_close_attempted", "owner_destroy_confirmed", "before_owner_query_attempted", "before_owner_query_succeeded", "after_owner_query_attempted", "after_owner_query_succeeded"] {
        assert_bool(init, key, true);
    }
    for key in ["before_owner_query_error", "after_owner_query_error"] { assert_eq!(number(init, key), 0); }
    for key in ["initializer_cleanup_authority", "initializer_cleanup_after_handoff", "atomic_handoff", "continuous_custody"] {
        assert_bool(init, key, false);
    }
    assert_eq!(number(init, "empty_attempts"), 1); assert!(number(init, "after_sequence") > 0);
    assert_eq!(number(init, "after_enum_error"), 0); assert_eq!(text(init, "after_owner"), "exact-live-fixture");
    assert_eq!(text(init, "handoff"), "unchanged-guard-independently-admitted-not-atomic-custody");
    let cleanup = field(report, "initialized_clipboard_cleanup").expect("strict initialized cleanup evidence absent");
    assert_eq!(text(cleanup, "scope"), "latest-exact-owner-fences-not-hwnd-lifetime-lease");
    for key in ["native_copy_arm_succeeded", "armed_before_finish", "exact_owner_verified_twice", "helper_verified_before_open", "helper_verified_at_final_fence", "success", "post_owner_is_helper", "post_empty_verified", "close_confirmed"] {
        assert_bool(cleanup, key, true);
    }
    assert_eq!(number(cleanup, "attempts"), 1); assert!(number(cleanup, "final_sequence") > 0);
    assert_eq!(text(cleanup, "latest_epoch_kind"), "native-copy");
}

fn copy_sequence(app: &AppRef, root: &Path, server: &Loopback, out: &Path) {
    contract();
    let source_tab = App::active_tab_id(app);
    let mut source = None;
    assert!(pump_until(Duration::from_secs(60), || {
        source = handles(app, source_tab);
        source.as_ref().is_some_and(|h| (h.proof)().is_some())
    }), "initialized fixture source controller/document readiness unavailable");
    let source = source.unwrap(); let proof = (source.proof)().unwrap();
    current(&source, proof); ordinary_profile(&source.view); isolated_environment(app, &source);
    let before = snapshot(&source.view);
    assert_eq!(text(&before, "nonce"), server.nonce);
    assert_eq!(text(&before, "url"), server.url.as_str());
    assert_eq!(text(&before, "ready"), "complete");
    assert_eq!(text(&before, "copy"), format!("copy-{}", server.nonce));
    assert_eq!(text(&before, "paste"), ""); assert_eq!(text(&before, "note"), "");
    let negative_tab = App::new_tab(app);
    let negative_url = server.url.join(&format!("/negative?nonce={}", server.nonce)).unwrap();
    App::submit(app, negative_url.as_str()); let mut negative = None;
    assert!(pump_until(Duration::from_secs(30), || {
        negative = handles(app, negative_tab);
        negative.as_ref().is_some_and(|h| (h.proof)().is_some())
    }));
    let negative = negative.unwrap(); let negative_proof = (negative.proof)().unwrap();
    let negative_before = snapshot(&negative.view);
    assert_eq!(text(&negative_before, "url"), negative_url.as_str());
    assert_eq!(text(&negative_before, "ready"), "complete");
    assert_ne!(source.view.as_raw(), negative.view.as_raw());
    assert!(App::select_tab(app, source_tab)); current(&source, proof); current(&negative, negative_proof);
    let tabs = App::tab_ids(app); let persistent = storage(root);
    assert!(App::split_view(app).is_none()); effects(server, 0, 0, 0);
    // The sole initialized entry. No failed ordinary attempt, fallback or initialization retry.
    native_input_in_fixture(app, &source, source_tab, proof, "Copy", "copy", &server.nonce, &out.join("copy"), true);
    let copied = snapshot(&source.view); unchanged_document(&copied, &before);
    for key in ["paste", "note", "effects", "pasteAck", "noteAck"] {
        assert_eq!(field(&copied, key), field(&before, key));
    }
    assert_eq!(snapshot(&negative.view), negative_before);
    assert_eq!(App::tab_ids(app), tabs); assert!(App::split_view(app).is_none());
    assert_eq!(storage(root), persistent); effects(server, 0, 0, 0);
    current(&source, proof); current(&negative, negative_proof);
    std::fs::write(out.join("initialized-copy-summary.json"), r#"{"scope":"genuine-native-Copy-under-explicit-initialized-empty-disposable-CI-precondition","copy":"exact-production-completion-and-generated-native-browser-owned-text-with-owned-empty-cleanup","uninitialized_zero_lane":"separate-unchanged-not-remediated","continuous_HWND_lifetime_or_atomic_handoff":"NOT_CLAIMED","permission_grants":"NONE","shipping_feature":"DEFAULT_OFF"}"#).unwrap();
}

#[test]
#[ignore = "held experiment: requires reviewed dedicated new hosted Windows VM and one explicit empty-clipboard initialization; run alone"]
fn live_probe_initialized_empty_native_copy() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    assert_eq!(args.iter().map(String::as_str).collect::<Vec<_>>(),
        vec![INITIALIZED_TEST, "--exact", "--ignored", "--nocapture", "--test-threads=1"]);
    let contract = contract();
    assert!(crate::profile::data_root_override().is_none()); assert!(std::env::var_os("BOWSER_DATA_DIR").is_none());
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let token = format!("{}-{stamp}", std::process::id());
    let root = std::env::temp_dir().join(format!("bowser-initialized-empty-copy-{token}")); std::fs::create_dir(&root).unwrap();
    let evidence = std::env::current_dir().unwrap().join("store-initialized-empty-copy");
    std::fs::create_dir(&evidence).expect("never reuse initialized fixture evidence");
    std::fs::write(evidence.join("environment-contract.json"), contract.json()).unwrap();
    let server = Loopback::start(token.clone());
    crate::profile::set_data_root(root.clone()).unwrap(); crate::profile::set_dev_launch(true);
    extensions::write_records(&root.join("extensions.txt"), &[]).unwrap();
    std::fs::write(root.join("extension_ids.txt"), b"").unwrap();
    let options = LaunchOptions::parse_from(vec!["--dev".into(), "--data-dir".into(), root.to_string_lossy().into_owned(), "--open-url".into(), server.url.to_string()],
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/fixtures")).unwrap();
    assert!(options.dev && !options.guest && !options.agent && options.benchmark_log.is_none());
    assert_eq!(options.data_dir.as_deref(), Some(root.as_path())); assert!(options.user_data_dir.is_none());
    let outcome = Rc::new(RefCell::new(None)); let sink = outcome.clone(); let output = evidence.clone(); let destination = root.clone();
    let started = Instant::now(); let timed_out = Rc::new(Cell::new(false)); let expired = timed_out.clone();
    let trace = Rc::new(ScriptTrace::new()); let _trace_scope = TraceScope::enter(trace.clone());
    let ready_trace = trace.clone(); let watchdog_trace = trace.clone();
    let deregistrations = Rc::new(RefCell::new(Vec::<Option<i32>>::new())); let removal = deregistrations.clone();
    crate::win32::set_app_menu_probe_hooks(Box::new(move |app| {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| copy_sequence(&app, &destination, &server, &output)));
        save_transport_diagnostic(&server, &output); if result.is_err() { ready_trace.report("initialized-copy-sequence-failed"); }
        *sink.borrow_mut() = Some(result.map_err(|_| "initialized-empty native Copy failed; original zero lane is independent".to_owned()));
        set_observer(Some(Rc::new(move |event| if let HostProbeEvent::Lifecycle(MenuProbeEvent::Deregistered { native_error }) = event { removal.borrow_mut().push(native_error); })));
        let owner = app.borrow().hwnd; unsafe { let _ = PostMessageW(Some(owner), WM_CLOSE, WPARAM(0), LPARAM(0)); }
    }), Box::new(move |app| if started.elapsed() > Duration::from_secs(240) && !expired.replace(true) {
        watchdog_trace.report("initialized-copy-watchdog-before-close");
        let owner = app.borrow().hwnd; unsafe { let _ = PostMessageW(Some(owner), WM_CLOSE, WPARAM(0), LPARAM(0)); }
    }));
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let run = App::run(options, Rc::new(PerfLog::new(None).unwrap())); unsafe { CoUninitialize() }; set_observer(None);
    assert!(run.is_ok(), "App::run failed: {run:?}"); assert!(!timed_out.get());
    outcome.borrow_mut().take().expect("initialized fixture ready hook absent").unwrap();
    let removed = deregistrations.borrow(); assert!(removed.len() >= 2); assert!(removed.iter().all(Option::is_none));
    std::fs::write(evidence.join("native-handler-teardown.json"), format!(r#"{{"status":"observed-native-removal-success","native_handler_receipts":{}}}"#, removed.len())).unwrap();
    println!("INITIALIZED_EMPTY_COPY genuine_native_result=true original_zero_lane_remains_separate=true evidence={}", evidence.display());
}

#[cfg(test)]
mod synthetic_contract_tests {
    use super::*;
    fn baseline() -> Vec<String> {
        vec!["true", "github-hosted", "Windows", "grantcharlie1000-tech/Bowser", JOB, "123", "1",
            "0123456789abcdef0123456789abcdef01234567", "20261020.1.0", CONTRACT, JOB,
            "0123456789abcdef0123456789abcdef01234567", "123", "1"].into_iter().map(str::to_owned).collect()
    }
    #[test]
    fn synthetic_initialized_contract_requires_each_exact_binding() {
        assert!(validate(&baseline()).is_some()); assert!(validate(&[]).is_none());
        for index in 0..KEYS.len() { let mut values = baseline(); values[index].clear(); assert!(validate(&values).is_none(), "{}", KEYS[index]); }
        for (key, invalid) in [("RUNNER_ENVIRONMENT", "self-hosted"), ("GITHUB_REPOSITORY", "wrong/repository"), ("GITHUB_JOB", "native-safety"),
            ("GITHUB_RUN_ID", "0"), ("GITHUB_RUN_ATTEMPT", "01"), ("GITHUB_SHA", "ABCDEF"), ("ImageVersion", "not\na-fixed-image"),
            ("BOWSER_INITIALIZED_EMPTY_CLIPBOARD_RUN", "124"), ("BOWSER_INITIALIZED_EMPTY_CLIPBOARD_ATTEMPT", "2"),
            ("BOWSER_INITIALIZED_EMPTY_CLIPBOARD_CHECKOUT", "1123456789abcdef0123456789abcdef01234567")] {
            let mut values = baseline(); values[KEYS.iter().position(|value| *value == key).unwrap()] = invalid.into(); assert!(validate(&values).is_none(), "{key}");
        }
    }
    #[test]
    fn synthetic_initialized_contract_is_bounded_and_not_a_custody_claim() {
        for invalid in ["", "0", "01", "-1", "1.0", "18446744073709551616"] { assert!(!positive_decimal(invalid)); }
        assert!(positive_decimal("18446744073709551615"));
        let mut values = baseline(); values[8] = "x".repeat(81); assert!(validate(&values).is_none());
        let mut values = baseline(); values[6] = "12345678901".into(); values[13] = values[6].clone(); assert!(validate(&values).is_none());
        let contract = validate(&baseline()).unwrap(); let json = json::parse(&contract.json()).unwrap();
        assert_eq!(text(&json, "checkout"), baseline()[7]); assert_bool(&json, "continuous_handoff_custody_claimed", false);
        assert_bool(&json, "metadata_alone_is_not_exclusivity_proof", true);
        assert_eq!(text(&json, "uninitialized_zero_lane"), "separate-unchanged-not-this-result");
    }
}
