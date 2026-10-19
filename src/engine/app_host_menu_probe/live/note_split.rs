//! Independent actual-App host-only lane. No clipboard precondition or clipboard access.
//! Reuses the same production coordinator and native input helper without changing either.
use super::*;

const NOTE_SPLIT_TEST_NAME: &str = "engine::app_host_menu_probe::live::note_split::live_probe_real_app_host_note_split_context_menu";
const NOTE_SPLIT_CONSENT: &str = "disposable-app-host-menu-note-split-no-clipboard";

#[derive(Clone, Copy)]
enum HostOnlyMode {
    Note,
    Split,
}
impl HostOnlyMode {
    fn name(self) -> &'static str {
        match self { Self::Note => "Note", Self::Split => "Split" }
    }
    fn control(self) -> &'static str {
        match self { Self::Note => "note", Self::Split => "split" }
    }
}

fn positive_identity(node: &Value, key: &str) -> usize {
    let identity = text(node, key).parse::<usize>().expect("canonical native identity");
    assert!(identity > 0, "native identity must not be guessed or absent");
    identity
}

fn host_only_input(
    app: &AppRef, source: &NativeContextProbeHandles, tab: TabId, proof: (u64, u64, u64),
    mode: HostOnlyMode, token: &str, out: &Path,
) {
    // This closed mode type can only enter the existing helper's Note/Split paths. Those paths
    // never construct ClipboardGuard, query a clipboard sequence, or read/write the clipboard.
    let selected = native_input(app, source, tab, proof, mode.name(), mode.control(), token, out);
    let report = json::parse(&std::fs::read_to_string(out.join("app-host-context-menu.json")).unwrap()).unwrap();
    assert_eq!(text(&report, "clipboard"), "not-used");
    assert!(array(&report, "owned_clipboard_sequences").is_empty());
    assert_bool(&report, "right_input_called", true);
    assert_bool(&report, "selection_called", true);
    assert_bool(&report, "separator_projection_contract", true);
    assert_bool(&report, "grayed_projection_contract", true);
    let request = field(&report, "request").expect("actual complete request witness");
    let path = array(request, "selected_path").iter().map(|v| {
        let Value::Number(index) = v else { panic!("native path index absent") };
        index.parse::<usize>().unwrap()
    }).collect::<Vec<_>>();
    let roots = array(request, "nodes");
    let leaf = match mode {
        HostOnlyMode::Note => {
            assert_eq!(path.len(), 2, "actual host-created note parent and child required");
            let parent = &roots[path[0]];
            assert_eq!(number(parent, "kind"), 4);
            assert_eq!(text(parent, "host_role"), "insert-note-parent");
            assert_bool(parent, "enabled", true);
            positive_identity(parent, "collection_identity");
            let parent_identity = positive_identity(parent, "identity");
            let child = &array(parent, "children")[path[1]];
            assert_eq!(text(child, "host_role"), "insert-note-child");
            assert_ne!(positive_identity(child, "identity"), parent_identity);
            child
        }
        HostOnlyMode::Split => {
            assert_eq!(path.len(), 1, "actual host-created root Split item required");
            let leaf = &roots[path[0]];
            assert_eq!(text(leaf, "host_role"), "split-link");
            leaf
        }
    };
    positive_identity(leaf, "identity");
    positive_identity(leaf, "collection_identity");
    assert_bool(leaf, "enabled", true);
    assert!((0..=2).contains(&number(leaf, "kind")));
    assert_eq!(number(leaf, "command_id"), i64::from(selected));
    assert!(array(leaf, "children").is_empty());
}

fn pristine_fixture(value: &Value, url: &url::Url, token: &str) {
    assert_eq!(text(value, "url"), url.as_str());
    assert_eq!(text(value, "nonce"), token);
    assert_eq!(text(value, "ready"), "complete");
    assert_eq!(text(value, "copy"), format!("copy-{token}"));
    for key in ["paste", "note"] {
        assert_eq!(text(value, key), "");
        assert!(array(field(value, "effects").unwrap(), key).is_empty());
    }
    for key in ["pasteAck", "noteAck"] { assert_eq!(field(value, key), Some(&Value::Null)); }
    for key in ["copy", "paste", "note", "split"] {
        assert_eq!(number(field(value, "trusted").unwrap(), key), 0);
    }
}

fn host_only_sequence(app: &AppRef, root: &Path, server: &Loopback, out: &Path) {
    let source_tab = App::active_tab_id(app);
    let mut source = None;
    assert!(pump_until(Duration::from_secs(60), || {
        source = handles(app, source_tab);
        source.as_ref().is_some_and(|h| (h.proof)().is_some())
    }), "fresh actual source controller/document did not settle");
    let source = source.unwrap();
    let proof = (source.proof)().unwrap();
    current(&source, proof);
    ordinary_profile(&source.view);
    isolated_environment(app, &source);
    let before = snapshot(&source.view);
    pristine_fixture(&before, &server.url, &server.nonce);

    // A distinct real AppTab loads the same synthetic document with the original lane nonce.
    let negative_tab = App::new_tab(app);
    let negative_url = server.url.join(&format!("/negative?nonce={}", server.nonce)).unwrap();
    App::submit(app, negative_url.as_str());
    let mut negative = None;
    assert!(pump_until(Duration::from_secs(30), || {
        negative = handles(app, negative_tab);
        negative.as_ref().is_some_and(|h| (h.proof)().is_some())
    }), "fresh duplicate negative controller/document did not settle");
    let negative = negative.unwrap();
    let negative_proof = (negative.proof)().unwrap();
    ordinary_profile(&negative.view);
    isolated_environment(app, &negative);
    let negative_before = snapshot(&negative.view);
    pristine_fixture(&negative_before, &negative_url, &server.nonce);
    assert_ne!(source.view.as_raw(), negative.view.as_raw());
    assert_ne!(source.controller.as_raw(), negative.controller.as_raw());
    assert_ne!(source_tab, negative_tab);
    assert!(App::select_tab(app, source_tab));
    current(&source, proof);
    current(&negative, negative_proof);
    let tabs_before = App::tab_ids(app);
    assert_eq!(tabs_before.len(), 2, "only fresh source and duplicate negative AppTabs");
    assert!(App::split_view(app).is_none());
    let persistent_before = storage(root);
    effects(server, 0, 0, 0);
    let token = &server.nonce;

    host_only_input(app, &source, source_tab, proof, HostOnlyMode::Note, token, &out.join("note"));
    let mut noted = None;
    assert!(pump_until(Duration::from_secs(10), || {
        let value = snapshot(&source.view);
        let ready = field(&value, "noteAck") == Some(&Value::String(token.clone()));
        noted = Some(value);
        ready
    }), "real Insert note source/body/loopback nonce acknowledgment absent");
    let noted = noted.unwrap();
    unchanged_document(&noted, &before);
    assert_eq!(text(&noted, "note"), format!("note-{token}"));
    assert_eq!(text(&noted, "paste"), "");
    assert_eq!(field(&noted, "pasteAck"), Some(&Value::Null));
    let note_events = array(field(&noted, "effects").unwrap(), "note");
    assert_eq!(note_events.len(), 1);
    assert_eq!(text(&note_events[0], "value"), format!("note-{token}"));
    assert!(array(field(&noted, "effects").unwrap(), "paste").is_empty());
    for (key, expected) in [("copy", 0), ("paste", 0), ("note", 1), ("split", 0)] {
        assert_eq!(number(field(&noted, "trusted").unwrap(), key), expected);
    }
    assert_eq!(snapshot(&negative.view), negative_before);
    assert_eq!(App::tab_ids(app), tabs_before);
    assert!(App::split_view(app).is_none());
    assert_eq!(storage(root), persistent_before);
    effects(server, 0, 1, 0);
    current(&source, proof);
    current(&negative, negative_proof);

    // Last action: only the genuine production Split route may create the destination tab.
    let owner = app.borrow().hwnd;
    let before_bounds = controller_bounds(&source, owner);
    host_only_input(app, &source, source_tab, proof, HostOnlyMode::Split, token, &out.join("split"));
    let mut destination = None;
    assert!(pump_until(Duration::from_secs(30), || {
        let ids = App::tab_ids(app);
        if ids.len() != tabs_before.len() + 1 { return false }
        let added = ids.into_iter().filter(|tab| !tabs_before.contains(tab)).collect::<Vec<_>>();
        if added.len() != 1 { return false }
        let tab = added[0];
        let Some(h) = handles(app, tab) else { return false };
        if (h.proof)().is_none() { return false }
        destination = Some((tab, h));
        true
    }), "actual Split destination AppTab/controller/successful DocumentGate absent");
    let (destination_tab, destination) = destination.unwrap();
    let destination_proof = (destination.proof)().unwrap();
    current(&destination, destination_proof);
    ordinary_profile(&destination.view);
    isolated_environment(app, &destination);
    assert_ne!(destination.view.as_raw(), source.view.as_raw());
    assert_ne!(destination.view.as_raw(), negative.view.as_raw());
    assert_ne!(destination.controller.as_raw(), source.controller.as_raw());
    assert_ne!(destination.controller.as_raw(), negative.controller.as_raw());
    let split = App::split_view(app).expect("actual App split_view absent");
    assert!(split.is_pair());
    assert_eq!(split.tabs().collect::<std::collections::HashSet<_>>(), [source_tab, destination_tab].into_iter().collect());
    let destination_url = server.url.join(&format!("/destination?nonce={token}")).unwrap();
    assert_eq!(App::tab_committed_url(app, destination_tab), Some(destination_url.clone()));
    let committed = {
        let state = app.borrow();
        state.engine.as_ref().unwrap().successful_document_query(destination_tab)
    }.expect("actual successful destination document query absent");
    assert_eq!(committed(), Some(destination_url.clone()));
    let receipt = script(&destination.view, "({url:location.href,ready:document.readyState,nonce:document.body.getAttribute('data-destination-nonce')})");
    assert_eq!(text(&receipt, "url"), destination_url.as_str());
    assert_eq!(text(&receipt, "ready"), "complete");
    assert_eq!(text(&receipt, "nonce"), token);
    let source_bounds = controller_bounds(&source, owner);
    let destination_bounds = controller_bounds(&destination, owner);
    let mut client = RECT::default();
    unsafe { GetClientRect(owner, &mut client).unwrap() };
    for bounds in [source_bounds, destination_bounds] {
        assert!(bounds.left >= 0 && bounds.top >= 0 && bounds.right <= client.right && bounds.bottom <= client.bottom);
    }
    assert!(source_bounds.right <= destination_bounds.left || destination_bounds.right <= source_bounds.left || source_bounds.bottom <= destination_bounds.top || destination_bounds.bottom <= source_bounds.top,
        "real visible split controllers must not overlap");
    assert_ne!((before_bounds.left, before_bounds.top, before_bounds.right, before_bounds.bottom), (source_bounds.left, source_bounds.top, source_bounds.right, source_bounds.bottom));
    let split_source = snapshot(&source.view);
    unchanged_document(&split_source, &before);
    for key in ["paste", "note", "effects", "pasteAck", "noteAck"] {
        assert_eq!(field(&split_source, key), field(&noted, key));
    }
    for (key, expected) in [("copy", 0), ("paste", 0), ("note", 1), ("split", 1)] {
        assert_eq!(number(field(&split_source, "trusted").unwrap(), key), expected);
    }
    assert_eq!(snapshot(&negative.view), negative_before);
    let ids = App::tab_ids(app);
    assert_eq!(ids.len(), tabs_before.len() + 1);
    assert_eq!(ids.into_iter().filter(|tab| !tabs_before.contains(tab)).collect::<Vec<_>>(), vec![destination_tab]);
    assert_eq!(storage(root), persistent_before);
    effects(server, 0, 1, 1);
    current(&source, proof);
    current(&negative, negative_proof);
    current(&destination, destination_proof);
    let committed_again = {
        let state = app.borrow();
        state.engine.as_ref().unwrap().successful_document_query(destination_tab)
    }.expect("fresh successful destination document query absent");
    assert_eq!(committed_again(), Some(destination_url));
    current(&destination, destination_proof);
    std::fs::write(out.join("app-host-note-split-summary.json"), r#"{"scope":"independent-fresh-actual-App-host-InsertNote-SplitLink-only","clipboard":"NOT USED: no ClipboardGuard construction or clipboard calls","note":"actual-parent-child-COM-identities-OS-hover-selection-source-only-exact-body-loopback-nonce-ack","split":"one-new-AppTab-live-pair-visible-nonoverlapping-controller-bounds-successful-current-committed-destination-nonce","source-and-duplicate-negative":"original-document-proofs-and-effects-retained","bounded-Bowser-storage":"notes-meta-records-map-unchanged","original-Copy-Paste-gate":"INDEPENDENT AND UNCHANGED; this lane cannot satisfy it","search-completed-transport":"NOT RUN","permission-race-matrix":"NOT RUN","signed-Bitwarden-workflow":"NOT RUN","shipping-feature":"DEFAULT OFF","store-compatibility":"NOT CLAIMED"}"#).unwrap();
    println!("APP_HOST_NOTE_SPLIT actual_App_run=true host_only_native_note_split=true clipboard_NOT_USED=true evidence={}", out.display());
}

#[test]
#[ignore = "requires explicit disposable host-only consent and an interactive unlocked Windows runner; run alone"]
fn live_probe_real_app_host_note_split_context_menu() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    assert_eq!(args.iter().map(String::as_str).collect::<Vec<_>>(), vec![NOTE_SPLIT_TEST_NAME, "--exact", "--ignored", "--nocapture", "--test-threads=1"],
        "only this independent exact single-test process may be retained by the failure guard");
    assert_eq!(std::env::var("BOWSER_APP_HOST_NOTE_SPLIT_PROBE_CONSENT").as_deref(), Ok(NOTE_SPLIT_CONSENT));
    assert!(crate::profile::data_root_override().is_none());
    assert!(std::env::var_os("BOWSER_DATA_DIR").is_none());
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let token = format!("{}-{stamp}", std::process::id());
    let root = std::env::temp_dir().join(format!("bowser-real-app-host-note-split-{token}"));
    std::fs::create_dir(&root).unwrap();
    let evidence = std::env::current_dir().unwrap().join("store-real-app-host-note-split-context-menu");
    std::fs::create_dir(&evidence).expect("never reuse another lane's evidence or failed profile");
    let server = Loopback::start(token.clone());
    let profile = crate::profile::Profile::default_in(&root);
    crate::profile::set_data_root(root.clone()).unwrap();
    crate::profile::set_dev_launch(true);
    extensions::write_records(&root.join("extensions.txt"), &[]).unwrap();
    std::fs::write(root.join("extension_ids.txt"), b"").unwrap();
    crate::productivity::write_note_list(&profile.notes_file(), &[crate::productivity::Note {
        id: 1, title: "Disposable independent host note".into(), body: format!("note-{token}"),
    }]).unwrap();
    let options = LaunchOptions::parse_from(vec!["--dev".into(), "--data-dir".into(), root.to_string_lossy().into_owned(), "--open-url".into(), server.url.to_string()],
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/fixtures")).unwrap();
    assert!(options.dev && !options.guest && !options.agent && options.benchmark_log.is_none());
    assert_eq!(options.data_dir.as_deref(), Some(root.as_path()));
    assert!(options.user_data_dir.is_none());
    let outcome = Rc::new(RefCell::new(None));
    let sink = outcome.clone();
    let destination = root.clone();
    let output = evidence.clone();
    let started = Instant::now();
    let timed_out = Rc::new(Cell::new(false));
    let expired = timed_out.clone();
    let deregistrations = Rc::new(RefCell::new(Vec::<Option<i32>>::new()));
    let removal = deregistrations.clone();
    crate::win32::set_app_menu_probe_hooks(Box::new(move |app| {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| host_only_sequence(&app, &destination, &server, &output)));
        *sink.borrow_mut() = Some(result.map_err(|_| "independent actual App note/Split regression failed; inspect host-only diagnostics".to_owned()));
        set_observer(Some(Rc::new(move |event| if let HostProbeEvent::Lifecycle(MenuProbeEvent::Deregistered { native_error }) = event {
            removal.borrow_mut().push(native_error);
        })));
        let owner = app.borrow().hwnd;
        unsafe { let _ = PostMessageW(Some(owner), WM_CLOSE, WPARAM(0), LPARAM(0)); }
    }), Box::new(move |app| if started.elapsed() > Duration::from_secs(240) && !expired.replace(true) {
        let owner = app.borrow().hwnd;
        unsafe { let _ = PostMessageW(Some(owner), WM_CLOSE, WPARAM(0), LPARAM(0)); }
    }));
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let run = App::run(options, Rc::new(PerfLog::new(None).unwrap()));
    unsafe { CoUninitialize() };
    set_observer(None);
    assert!(run.is_ok(), "actual App::run failed: {run:?}");
    assert!(!timed_out.get());
    outcome.borrow_mut().take().expect("real ready hook absent").unwrap();
    let removed = deregistrations.borrow();
    assert_eq!(removed.len(), 3, "exact source, negative and one Split destination native removals required");
    assert!(removed.iter().all(Option::is_none));
    std::fs::write(evidence.join("native-handler-teardown.json"), format!(r#"{{"status":"observed-native-removal-success","native_handler_receipts":{}}}"#, removed.len())).unwrap();
    println!("APP_HOST_NOTE_SPLIT actual_native_handler_teardown=true disposable_profile_retained_for_evidence={}", root.display());
}
