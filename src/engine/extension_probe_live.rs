//! Live extension probe. One ignored test, `live_probe_mv3_extension_executes`, runs in order:
//!
//! 1. `probe-mv3` in a fresh profile: `AddBrowserExtension`, the content script, the service worker,
//!    and the extension URL scheme from `chrome.runtime.getURL`.
//! 2. A real process restart: the test runs itself again (`live_probe_restart_child`) on the same
//!    profile and compares the extension id, the persisted `chrome.storage.local` value and the
//!    beacon.
//! 3. `probe-mv3-locales` and `probe-mv3-key` in fresh profiles: load HRESULT and id.
//! 4. `probe-mv3-metadata`: only Bowser's own install check (it is refused before WebView2).
//! 5. A tiny fake file ending `.crx` passed to `AddBrowserExtension`: HRESULT.
//!
//! Ignored by default because it needs the WebView2 runtime and starts browser processes. Run it by
//! hand, never on a profile with data you care about:
//!
//!   $env:BOWSER_EXTENSION_TRACE = "1"
//!   cargo test --lib live_probe_mv3_extension_executes -- --ignored --nocapture --test-threads=1
//!
//! Most fixture windows are hidden. The real-package link diagnostic deliberately shows one
//! disposable extension page for genuine Windows input/accessibility. Every WebView2
//! user data folder, extension copy and id file lives in one fresh temp directory that is passed to
//! the engine explicitly; the test never reads or writes `%LOCALAPPDATA%\Bowser*`.

use std::{
    cell::RefCell,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use url::Url;
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
            TranslateMessage, WINDOW_EX_STYLE, WS_POPUP,
        },
    },
    core::w,
};

use super::{EngineAdapter, EngineEvents, EngineExtras, WebView2Engine};
use crate::{extensions, tabs::TabId};

const BEACON_PREFIX: &str = "/__bowser_probe__/beacon";
const CHILD_ROOT_ENV: &str = "BOWSER_PROBE_CHILD_ROOT";
/// The id Chromium derives from the `key` in `probe-mv3-key` (see `tests/fixtures/extensions/README.md`).
const KEY_FIXTURE_ID: &str = "ciocoifocbggchcecjgibbaeojegkpen";

fn pump_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if done() {
            return true;
        }
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    done()
}

/// Serves one HTML page and records every beacon request path.
fn serve_loopback(listener: TcpListener, beacons: Arc<Mutex<Vec<String>>>, stop: Arc<AtomicBool>) {
    listener.set_nonblocking(true).unwrap();
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => handle(stream, &beacons),
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn handle(mut stream: TcpStream, beacons: &Mutex<Vec<String>>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buffer = [0u8; 8192];
    let Ok(read) = stream.read(&mut buffer) else {
        return;
    };
    let request = String::from_utf8_lossy(&buffer[..read]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .unwrap_or("")
        .to_owned();
    beacons.lock().unwrap().push(path.clone());
    if path == "/document-gate-slow" { std::thread::sleep(Duration::from_secs(2)); }
    let (status, content_type, body) = if path.starts_with(BEACON_PREFIX) {
        ("200 OK", "text/plain", String::new())
    } else if path == "/control-script.js" || path == "/ads/banner.js" || path == "/adserver/ad.js" || path == "/pagead/show_ads.js" {
        ("200 OK", "application/javascript", format!("document.documentElement.setAttribute('data-script-{}','loaded');", path.trim_start_matches('/').replace(['/', '.'], "-")))
    } else if path == "/dnr-external" {
        ("200 OK", "text/html; charset=utf-8", "<!doctype html><title>Disposable default-filter network fixture</title><p>No accounts or user data</p><script src='/control-script.js'></script><img alt='network control' src='https://pagead2.googlesyndication.com/pagead/gen_204?id=bowser_disposable_probe'>".to_owned())
    } else if path.starts_with("/dnr-fixture") {
        ("200 OK", "text/html; charset=utf-8", "<!doctype html><title>Disposable native DNR fixture</title><p>Controlled loopback requests</p><script src='/control-script.js'></script><script src='/ads/banner.js'></script><script src='/adserver/ad.js'></script><script src='/pagead/show_ads.js'></script>".to_owned())
    } else if path.starts_with('/') && !path.contains("favicon") {
        (
            "200 OK",
            "text/html; charset=utf-8",
            "<!doctype html><title>Bowser probe page</title><p>Bowser probe page</p>".to_owned(),
        )
    } else {
        ("404 Not Found", "text/plain", String::new())
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    );
}

struct Loopback {
    port: u16,
    beacons: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Loopback {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let beacons = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (beacons, stop) = (beacons.clone(), stop.clone());
            std::thread::spawn(move || serve_loopback(listener, beacons, stop))
        };
        Self {
            port,
            beacons,
            stop,
            thread: Some(thread),
        }
    }

    fn page(&self, name: &str) -> Url {
        Url::parse(&format!("http://127.0.0.1:{}/{name}", self.port)).unwrap()
    }

    /// The decoded `reply` of the first beacon for `stage`, if one arrived.
    fn reply(&self, stage: &str) -> Option<String> {
        let marker = format!("stage={stage}&");
        let beacons = self.beacons.lock().unwrap();
        let path = beacons.iter().find(|path| path.contains(&marker))?;
        let url = Url::parse(&format!("http://127.0.0.1{path}")).ok()?;
        url.query_pairs()
            .find(|(key, _)| key == "reply")
            .map(|(_, value)| value.into_owned())
    }

    fn print_beacons(&self) {
        for beacon in self.beacons.lock().unwrap().iter() {
            println!("LIVE_PROBE beacon={beacon}");
        }
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The text of a JSON string member, for logging only. Not a JSON parser.
fn json_string(text: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\":\"");
    let start = text.find(&marker)? + marker.len();
    let end = text[start..].find('"')?;
    Some(text[start..start + end].to_owned())
}

/// Parsed protocol identity field, never the logging-only substring helper above.
fn protocol_string(text: &str, container: Option<&str>, field: &str) -> Option<String> {
    use extensions::json::{self, Value};
    let Value::Object(root) = json::parse(text)? else { return None; };
    let fields = if let Some(container) = container {
        let Value::Object(fields) = json::get(&root, container)? else { return None; };
        fields
    } else { &root };
    let Value::String(value) = json::get(fields, field)? else { return None; };
    Some(value.clone())
}

/// `scheme://id/` of an extension URL, or `None` when it has no authority.
fn url_scheme(url: &str) -> Option<String> {
    url.split_once("://").map(|(scheme, _)| scheme.to_owned())
}

fn temp_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("bowser-ext-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn mapping_for(ids_file: &Path, bowser_id: &str) -> Option<String> {
    std::fs::read_to_string(ids_file)
        .ok()?
        .lines()
        .find_map(|line| {
            line.strip_prefix(bowser_id)?
                .strip_prefix('\t')
                .map(str::to_owned)
        })
}

fn fixture(variant: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("extensions")
        .join(variant)
}

/// One engine on a hidden window, in the profile folder `profile`.
struct Session {
    engine: WebView2Engine,
    host: HWND,
    failures: Rc<RefCell<Vec<String>>>,
}

impl Session {
    /// Creates the engine and starts its first tab on `first`, which is what lets the extension
    /// profile be captured. Retries engine creation a few times because a browser process from the
    /// previous session can still be shutting down on the same data folder.
    fn open(
        profile: &Path,
        startup: Vec<(String, PathBuf)>,
        known: Vec<(String, bool)>,
        first: &Url,
    ) -> Self {
        Self::open_with_private_first(profile, startup, known, first, false)
    }

    fn open_with_private_first(profile: &Path, startup: Vec<(String, PathBuf)>, known: Vec<(String, bool)>, first: &Url, private_first: bool) -> Self {
        Self::open_configured(profile, startup, known, first, private_first, false)
    }

    fn open_configured(profile: &Path, startup: Vec<(String, PathBuf)>, known: Vec<(String, bool)>, first: &Url, private_first: bool, chrome_ui_scheme: bool) -> Self {
        let host = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("bowser-extension-probe"),
                WS_POPUP,
                0,
                0,
                800,
                600,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap();
        let bounds = RECT {
            left: 0,
            top: 0,
            right: 800,
            bottom: 600,
        };
        let webview_dir = profile.join("WebView2");
        let ids_file = profile.join("extension_ids.txt");
        let mut engine = None;
        let failures = Rc::new(RefCell::new(Vec::new()));
        for attempt in 1..=4 {
            let created: Rc<RefCell<Option<windows::core::Result<WebView2Engine>>>> =
                Rc::new(RefCell::new(None));
            let slot = created.clone();
            let extras = EngineExtras {
                browser_extensions: true,
                chrome_ui_scheme,
                private_blocker: private_first.then(|| Rc::new(RefCell::new(crate::blocker::BlockerState::new(crate::blocker::Blocker::from_lists(None), &[])))),
                startup_extensions: startup.clone(),
                known_extensions: known.clone(),
                extension_ids_file: Some(ids_file.clone()),
                extension_records_file: Some(profile.join("extensions.txt")),
                ..EngineExtras::default()
            };
            let mut events = EngineEvents::default();
            let failed = failures.clone(); events.failed = Some(Rc::new(move |error| { let message = format!("controller_or_environment_hresult=0x{:08x} error={error}", error.code().0 as u32); println!("LIVE_NATIVE_FAILURE {message}"); failed.borrow_mut().push(message); }));
            events.navigation_failed = Some(Rc::new(|tab, failed| println!("LIVE_NATIVE_NAVIGATION tab={tab:?} failed={failed}")));
            if let (Ok(cut), Some(cut_root)) = (std::env::var("BOWSER_EXTENSION_CRASH_EVENT"), std::env::var_os("BOWSER_EXTENSION_CRASH_ROOT").map(PathBuf::from)) {
                assert!(["extension_add_preclaim_committed", "extension_add_native_complete", "extension_disable_native_complete", "extension_remove_native_complete"].contains(&cut.as_str()));
                assert!(cut_root.starts_with(std::env::temp_dir()) && cut_root.file_name().unwrap().to_string_lossy().starts_with("bowser-ext-live-"));
                events.policy_event = Some(Rc::new(move |event| {
                    if event == cut {
                        let mut file = std::fs::File::create(cut_root.join("crash-marker")).unwrap(); file.write_all(event.as_bytes()).unwrap(); file.sync_all().unwrap();
                        // Abrupt process exit: no Rust/COM cleanup and no lease destructor. This
                        // models process crash only, not filesystem/power-loss durability.
                        std::process::exit(73);
                    }
                }));
            }
            WebView2Engine::begin_create_with_extras(
                host,
                bounds,
                &webview_dir,
                None,
                events,
                None,
                extras,
                move |result| *slot.borrow_mut() = Some(result),
            )
            .unwrap();
            assert!(
                pump_until(Duration::from_secs(60), || created.borrow().is_some()),
                "no engine in 60 s"
            );
            match created.borrow_mut().take().unwrap() {
                Ok(created) => {
                    engine = Some(created);
                    break;
                }
                Err(error) => {
                    println!("LIVE_PROBE engine_create_attempt={attempt} error={error}");
                    pump_until(Duration::from_secs(3), || false);
                }
            }
        }
        let mut engine = engine.expect("engine created");
        let tab = TabId(1);
        if private_first { engine.set_tab_private(tab); }
        engine.select_tab(tab).unwrap();
        engine.navigate_tab(tab, first).unwrap();
        Self { engine, host, failures }
    }

    fn native_inventory(&self) -> Result<Vec<(String, bool)>, String> {
        let answer = Rc::new(RefCell::new(None)); let sink = answer.clone();
        self.engine.probe_native_extension_inventory(Box::new(move |result| *sink.borrow_mut() = Some(result)));
        pump_until(Duration::from_secs(15), || answer.borrow().is_some());
        answer.borrow_mut().take().unwrap_or_else(|| Err("Native inventory timed out.".into()))
    }

    fn tab(&self) -> TabId {
        TabId(1)
    }

    /// Waits for native Add completion, never the earlier durable ownership preclaim.
    fn wait_for_add(&self, profile: &Path, bowser_id: &str) -> (Option<String>, Option<String>) {
        let _ = profile; // completion belongs to this engine, not merely an on-disk map row.
        pump_until(Duration::from_secs(60), || {
            self.engine.probe_loaded_extension_id(bowser_id).is_some()
                || extensions::load_failure(bowser_id).is_some()
        });
        (
            self.engine.probe_loaded_extension_id(bowser_id),
            extensions::load_failure(bowser_id),
        )
    }

    /// Loads pages until the extension's worker answers a content-script message, or gives up. A
    /// content script only reaches documents that load after the extension is active.
    fn wait_for_worker_reply(&mut self, loopback: &Loopback) -> bool {
        for attempt in 0..8 {
            let page = loopback.page(&format!("page{attempt}"));
            self.engine.navigate_tab(self.tab(), &page).unwrap();
            if pump_until(Duration::from_secs(6), || {
                loopback.reply("worker-reply").is_some()
            }) {
                return true;
            }
        }
        false
    }

    fn read_marker(&mut self, attribute: &str) -> Option<String> {
        let slot = Rc::new(RefCell::new(None));
        let sink = slot.clone();
        self.engine.run_reader_script(
            self.tab(),
            &format!("document.documentElement.getAttribute('{attribute}')"),
            Box::new(move |answer| *sink.borrow_mut() = Some(answer)),
        );
        pump_until(Duration::from_secs(10), || slot.borrow().is_some());
        slot.borrow_mut().take().flatten()
    }

    /// Opens the extension's popup or options page through the same engine call the menu uses, in a
    /// window that is never shown.
    fn open_page(
        &mut self,
        id: &str,
        folder: &Path,
        kind: extensions::PageKind,
    ) -> Result<(), String> {
        let outcome = Rc::new(RefCell::new(None));
        let sink = outcome.clone();
        self.engine.open_extension_page_hidden(
            id,
            folder,
            kind,
            Box::new(move |result| *sink.borrow_mut() = Some(result)),
        );
        pump_until(Duration::from_secs(30), || outcome.borrow().is_some());
        outcome
            .borrow_mut()
            .take()
            .unwrap_or_else(|| Err("no answer in 30 s".to_owned()))
    }

    /// Runs a script in the hosted page and returns the JSON text of its result.
    fn page_script(&mut self, script: &str) -> Option<String> {
        let slot = Rc::new(RefCell::new(None));
        let sink = slot.clone();
        self.engine.extension_page_script(
            script,
            Box::new(move |answer| *sink.borrow_mut() = Some(answer)),
        );
        pump_until(Duration::from_secs(10), || slot.borrow().is_some());
        slot.borrow_mut().take().flatten()
    }

    /// Read/drive the exact disposable browsing WebView through its existing in-process test hook.
    fn browsing_script(&mut self, script: &str) -> Option<String> {
        let slot = Rc::new(RefCell::new(None)); let sink = slot.clone();
        self.engine.run_reader_script(self.tab(), script, Box::new(move |answer| *sink.borrow_mut() = Some(answer)));
        pump_until(Duration::from_secs(10), || slot.borrow().is_some()); slot.borrow_mut().take().flatten()
    }

    /// The page's own report (`data-bowser-popup`), once `done` says it is complete.
    fn page_report(&mut self, done: &str) -> Option<String> {
        let script =
            "JSON.parse(document.documentElement.getAttribute('data-bowser-popup') || 'null')";
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(20) {
            if let Some(report) = self.page_script(script)
                && report.contains(done)
            {
                return Some(report);
            }
            pump_until(Duration::from_millis(250), || false);
        }
        self.page_script(script)
    }

    fn replace(&self, id: &str, folder: &Path) -> Result<(), String> {
        let slot = Rc::new(RefCell::new(None)); let sink = slot.clone();
        self.engine.probe_replace_extension(id, folder, Box::new(move |result| *sink.borrow_mut() = Some(result)));
        pump_until(Duration::from_secs(60), || slot.borrow().is_some());
        slot.borrow_mut().take().unwrap_or_else(|| Err("replacement timed out".into()))
    }

    fn protocol(&self, tab: TabId, method: &str, params: &str) -> Option<String> {
        self.protocol_session(tab, None, method, params)
    }

    fn protocol_session(&self, tab: TabId, session: Option<&str>, method: &str, params: &str) -> Option<String> {
        let slot = Rc::new(RefCell::new(None)); let sink = slot.clone();
        self.engine.probe_protocol_session(tab, session, method, params, Box::new(move |result| *sink.borrow_mut() = Some(result)));
        pump_until(Duration::from_secs(15), || slot.borrow().is_some());
        slot.borrow_mut().take().flatten()
    }

    fn close(mut self) {
        let _ = self.engine.close();
        drop(self.engine);
        unsafe {
            let _ = DestroyWindow(self.host);
        }
    }
}

fn remove_with_retries(path: &Path) {
    // WebView2 can hold the folder for a moment after close, so retry for a few seconds.
    for _ in 0..20 {
        if std::fs::remove_dir_all(path).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Installs a fixture into `profile` the way Bowser does and returns (record id, folder).
fn install_fixture(profile: &Path, variant: &str) -> (String, PathBuf) {
    let extensions_dir = profile.join("Extensions");
    let records_file = profile.join("extensions.txt");
    let record = extensions::install(&fixture(variant), &extensions_dir, &records_file).unwrap();
    let folder = extensions::folder_for(&extensions_dir, &record.id).unwrap();
    (record.id, folder)
}

fn load_variant(root: &Path, variant: &str) {
    let profile = root.join(variant);
    let (id, folder) = install_fixture(&profile, variant);
    let loopback = Loopback::start();
    let mut session = Session::open(
        &profile,
        vec![(id.clone(), folder)],
        vec![(id.clone(), true)],
        &loopback.page("first"),
    );
    let (webview_id, failure) = session.wait_for_add(&profile, &id);
    println!("LIVE_PROBE variant={variant} webview_id={webview_id:?} load_failure={failure:?}");
    if variant == "probe-mv3-key" {
        println!(
            "LIVE_PROBE variant={variant} expected_key_id={KEY_FIXTURE_ID} id_matches_key={}",
            webview_id.as_deref() == Some(KEY_FIXTURE_ID)
        );
    }
    if webview_id.is_some() {
        let replied = session.wait_for_worker_reply(&loopback);
        let reply = loopback.reply("worker-reply").unwrap_or_default();
        println!(
            "LIVE_PROBE variant={variant} worker_reply={replied} runtime_id={:?} url={:?}",
            json_string(&reply, "id"),
            json_string(&reply, "url")
        );
    }
    session.close();
    drop(loopback);
}

/// Opens the fixture's popup and options pages the way the extensions menu does and records what
/// the hosted page could see. Nothing here asserts on `chrome.*`: the answer is the evidence.
fn hosted_pages(session: &mut Session, id: &str, folder: &Path, loopback: &Loopback) {
    use extensions::PageKind;
    for (label, kind) in [("popup", PageKind::Popup), ("options", PageKind::Options)] {
        let opened = session.open_page(id, folder, kind);
        println!("LIVE_PROBE {label} open={opened:?}");
        if opened.is_err() {
            continue;
        }
        println!(
            "LIVE_PROBE {label} report={:?}",
            session.page_report("\"ready\":true")
        );
        // Leak check: what the page did with `chrome.webview`, and what the host saw. Let the
        // asynchronous host-object calls settle first.
        pump_until(Duration::from_secs(2), || false);
        println!(
            "LIVE_PROBE {label} leak_page_view={:?}",
            session.page_script(
                "JSON.stringify((JSON.parse(document.documentElement.getAttribute('data-bowser-popup') || 'null') || {}).leak || null)"
            )
        );
        println!(
            "LIVE_PROBE {label} leak_host_received={:?}",
            super::extension_page::observe::take_received()
        );
        println!(
            "LIVE_PROBE {label} href={:?}",
            session.page_script("location.href")
        );
        println!(
            "LIVE_PROBE {label} click={:?}",
            session.page_script("document.getElementById('send').click(); 'clicked'")
        );
        println!(
            "LIVE_PROBE {label} after_send={:?}",
            session.page_report("\"sendDone\":true")
        );
        // A navigation away from the granted URL must be cancelled by the one-shot policy.
        let away = loopback.page("away");
        session.page_script(&format!("location.href = '{away}'; 'requested'"));
        pump_until(Duration::from_secs(3), || false);
        println!(
            "LIVE_PROBE {label} href_after_navigation_attempt={:?}",
            session.page_script("location.href")
        );
    }

    // Positive control for the leak check: with web messages deliberately left on, the same page
    // script must reach the observer. Without this, "nothing received" could just mean the observer
    // cannot see anything.
    super::extension_page::observe::positive_control(true);
    let opened = session.open_page(id, folder, PageKind::Popup);
    println!("LIVE_PROBE control open={opened:?}");
    if opened.is_ok() {
        session.page_report("\"ready\":true");
        pump_until(Duration::from_secs(2), || false);
        println!(
            "LIVE_PROBE control leak_host_received={:?}",
            super::extension_page::observe::take_received()
        );
    }
    super::extension_page::observe::positive_control(false);
}

fn crx_path(root: &Path) {
    let profile = root.join("crx");
    std::fs::create_dir_all(&profile).unwrap();
    let file = profile.join("fake.crx");
    // Not a real package: the CRX3 magic and a version word, nothing else.
    std::fs::write(&file, b"Cr24\x03\x00\x00\x00").unwrap();
    let id = "ext-0000000000000c01".to_owned();
    let loopback = Loopback::start();
    let session = Session::open(
        &profile,
        vec![(id.clone(), file)],
        vec![(id.clone(), true)],
        &loopback.page("first"),
    );
    let (webview_id, failure) = session.wait_for_add(&profile, &id);
    println!("LIVE_PROBE crx webview_id={webview_id:?} load_failure={failure:?}");
    session.close();
    drop(loopback);
}

fn metadata_variant() {
    let dir = std::env::temp_dir().join(format!("bowser-ext-live-meta-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let outcome = extensions::install(
        &fixture("probe-mv3-metadata"),
        &dir.join("Extensions"),
        &dir.join("extensions.txt"),
    );
    println!(
        "LIVE_PROBE variant=probe-mv3-metadata refused_by=bowser install={:?} reached_webview2=false",
        outcome.as_ref().map(|record| record.id.as_str())
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn run_child(root: &Path) -> Vec<String> {
    let exe = std::env::current_exe().unwrap();
    let output = std::process::Command::new(exe)
        .args([
            "live_probe_restart_child",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ROOT_ENV, root)
        .output()
        .expect("child test process starts");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    println!("LIVE_PROBE child_exit={:?}", output.status.code());
    for line in stderr
        .lines()
        .filter(|line| line.starts_with("extension call="))
    {
        println!("CHILD {line}");
    }
    let lines: Vec<String> = stdout
        .lines()
        .filter(|line| line.starts_with("LIVE_PROBE"))
        .map(str::to_owned)
        .collect();
    for line in &lines {
        println!("CHILD {line}");
    }
    lines
}

#[test]
#[ignore = "needs a live WebView2 runtime; run by hand, see docs/extensions.md (Verification status)"]
fn live_probe_mv3_extension_executes() {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let root = temp_root();
    println!("LIVE_PROBE temp_root={}", root.display());

    // 1. The baseline fixture in a fresh profile.
    let main_profile = root.join("main");
    let (id, folder) = install_fixture(&main_profile, "probe-mv3");
    let loopback = Loopback::start();
    let mut session = Session::open(
        &main_profile,
        vec![(id.clone(), folder.clone())],
        vec![(id.clone(), true)],
        &loopback.page("first"),
    );
    println!("LIVE_PROBE engine=created");
    let (webview_id, failure) = session.wait_for_add(&main_profile, &id);
    println!("LIVE_PROBE add webview_id={webview_id:?} load_failure={failure:?}");
    let saw_reply = session.wait_for_worker_reply(&loopback);
    let reply = loopback.reply("worker-reply").unwrap_or_default();
    let url = json_string(&reply, "url");
    println!(
        "LIVE_PROBE dom_marker={:?}",
        session.read_marker("data-bowser-probe")
    );
    println!("LIVE_PROBE worker_reply_beacon={saw_reply} reply={reply}");
    println!(
        "LIVE_PROBE runtime_id={:?} get_url={url:?} scheme={:?}",
        json_string(&reply, "id"),
        url.as_deref().and_then(url_scheme)
    );
    loopback.print_beacons();
    hosted_pages(&mut session, &id, &folder, &loopback);
    session.close();
    drop(loopback);

    // 2. A real process restart on the same profile.
    println!("LIVE_PROBE restart=begin first_id={:?}", webview_id);
    let child = run_child(&root);
    let child_id = child
        .iter()
        .find_map(|line| line.strip_prefix("LIVE_PROBE restart_runtime_id="))
        .map(str::to_owned);
    println!(
        "LIVE_PROBE restart=end first_id={webview_id:?} second_id={child_id:?} same_id={}",
        webview_id.is_some() && webview_id == child_id
    );

    // 3 to 5. The other variants.
    load_variant(&root, "probe-mv3-locales");
    load_variant(&root, "probe-mv3-key");
    metadata_variant();
    crx_path(&root);

    remove_with_retries(&root);
    assert!(
        saw_reply,
        "the content script and service worker did not both run"
    );
}

/// Second half of the restart check. Run only by the parent test, on the profile it left behind.
#[test]
#[ignore = "child of live_probe_mv3_extension_executes; needs BOWSER_PROBE_CHILD_ROOT"]
fn live_probe_restart_child() {
    let Some(root) = std::env::var_os(CHILD_ROOT_ENV).map(PathBuf::from) else {
        println!("LIVE_PROBE restart_child skipped: {CHILD_ROOT_ENV} is not set");
        return;
    };
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let profile = root.join("main");
    let records = extensions::read_records(&profile.join("extensions.txt")).unwrap();
    let record = records
        .first()
        .expect("the parent installed one extension")
        .clone();
    let folder = extensions::folder_for(&profile.join("Extensions"), &record.id).unwrap();
    println!(
        "LIVE_PROBE restart_child ids_file_before={:?}",
        mapping_for(&profile.join("extension_ids.txt"), &record.id)
    );
    let loopback = Loopback::start();
    let mut session = Session::open(
        &profile,
        vec![(record.id.clone(), folder)],
        vec![(record.id.clone(), true)],
        &loopback.page("first"),
    );
    let replied = session.wait_for_worker_reply(&loopback);
    let reply = loopback.reply("worker-reply").unwrap_or_default();
    println!("LIVE_PROBE restart_worker_reply={replied} reply={reply}");
    println!(
        "LIVE_PROBE restart_runtime_id={}",
        json_string(&reply, "id").unwrap_or_default()
    );
    println!(
        "LIVE_PROBE restart_stored_probe={:?}",
        json_string(&reply, "probe")
    );
    println!(
        "LIVE_PROBE restart_load_failure={:?}",
        extensions::load_failure(&record.id)
    );
    session.close();
}

/// Real uBO default rules against controlled loopback assets, paired with a zero-extension
/// control. This probes a concrete network effect, not tab targeting or arbitrary websites.
fn probe_real_blocker_network(root: &Path, session: &mut Session, loopback: &Loopback) {
    session.page_script("(()=>{chrome.declarativeNetRequest.getEnabledRulesets(r=>document.documentElement.setAttribute('data-enabled-rules',JSON.stringify({rules:r,error:chrome.runtime.lastError?.message||null})));return true})()");
    pump_until(Duration::from_secs(1), || false);
    println!("BLOCKER_PROBE enabled_rules={:?}", session.page_script("document.documentElement.getAttribute('data-enabled-rules')"));
    let baseline_loopback = Loopback::start();
    let baseline = Session::open(&root.join("zero-extension-control"), vec![], vec![], &baseline_loopback.page("dnr-fixture"));
    assert!(pump_until(Duration::from_secs(30), || baseline_loopback.beacons.lock().unwrap().iter().any(|path| path == "/control-script.js")), "zero-extension baseline did not acknowledge its actual page/script readiness");
    pump_until(Duration::from_millis(300), || false);
    let baseline_requests = baseline_loopback.beacons.lock().unwrap().clone();
    baseline.close();
    assert!(baseline_requests.iter().any(|p| p == "/control-script.js"), "control did not request its normal script");
    let ad_paths = ["/ads/banner.js", "/adserver/ad.js", "/pagead/show_ads.js"];
    assert!(ad_paths.iter().all(|path| baseline_requests.iter().any(|p| p == path)), "zero-extension ad fixtures must all reach server");
    loopback.beacons.lock().unwrap().clear();
    session.engine.navigate_tab(session.tab(), &loopback.page("dnr-fixture")).unwrap();
    pump_until(Duration::from_secs(4), || false);
    let extension_requests = loopback.beacons.lock().unwrap().clone();
    let control = extension_requests.iter().any(|p| p == "/control-script.js");
    let blocked: Vec<_> = ad_paths.into_iter().filter(|path| !extension_requests.iter().any(|p| p == path)).collect();
    println!("BLOCKER_PROBE zero_extension_requests={baseline_requests:?} with_extension_requests={extension_requests:?} normal_control_requested={control} blocked_candidates={blocked:?}");
    println!("BLOCKER_PROBE verdict={}", if control && !blocked.is_empty() { "concrete-native-default-rule-network-effect-partial-only" } else { "no-blocking-demonstrated-by-these-controlled-candidates" });
    // Separate native DNR API control, NOT a claim about uBO's default filtering. This uses the
    // genuine signed extension context in a disposable profile, without editing its package.
    let blocked_url = loopback.page("ads/banner.js").to_string();
    let rule_filter = format!("|{blocked_url}|");
    let script = format!("(()=>{{chrome.declarativeNetRequest.updateDynamicRules({{removeRuleIds:[987654],addRules:[{{id:987654,priority:100,action:{{type:'block'}},condition:{{urlFilter:{},resourceTypes:['script']}}}}]}},()=>document.documentElement.setAttribute('data-dnr-native-control',JSON.stringify({{error:chrome.runtime.lastError?.message||null}})));return true}})()", crate::chrome_host::protocol::json_string(&rule_filter));
    session.page_script(&script); pump_until(Duration::from_secs(2), || false);
    let installed = session.page_script("document.documentElement.getAttribute('data-dnr-native-control')");
    println!("DNR_NATIVE_CONTROL rule_filter={rule_filter:?} installed={installed:?} scope=native-api-only-not-default-ubo-functionality");
    if installed.as_deref().is_some_and(|answer| answer.contains("error") && answer.contains("null")) {
        loopback.beacons.lock().unwrap().clear();
        session.engine.navigate_tab(session.tab(), &loopback.page("dnr-fixture-dynamic")).unwrap();
        pump_until(Duration::from_secs(3), || false);
        let requests = loopback.beacons.lock().unwrap().clone();
        let control_requested = requests.iter().any(|path| path == "/control-script.js");
        let target_requested = requests.iter().any(|path| path == "/ads/banner.js");
        session.page_script("(()=>{chrome.declarativeNetRequest.updateDynamicRules({removeRuleIds:[987654]},()=>document.documentElement.setAttribute('data-dnr-native-removed',JSON.stringify({error:chrome.runtime.lastError?.message||null})));return true})()");
        pump_until(Duration::from_secs(2), || false);
        let removed = session.page_script("document.documentElement.getAttribute('data-dnr-native-removed')");
        loopback.beacons.lock().unwrap().clear();
        session.engine.navigate_tab(session.tab(), &loopback.page("dnr-fixture-restored")).unwrap();
        pump_until(Duration::from_secs(3), || false);
        let restored = loopback.beacons.lock().unwrap().iter().any(|path| path == "/ads/banner.js");
        println!("DNR_NATIVE_CONTROL requests={requests:?} control_requested={control_requested} target_requested={target_requested} removed={removed:?} target_requested_after_removal={restored}");
        println!("DNR_NATIVE_CONTROL verdict={}", if control_requested && !target_requested && restored { "exact-native-rule-network-effect-and-removal-proven" } else { "native-rule-network-effect-not-demonstrated" });
    }
}

fn observe_default_ubo_network(session: &mut Session, loopback: &Loopback) -> (bool, bool, bool, bool) {
    let events = session.engine.probe_network_events(session.tab()).expect("read-only in-process network receiver");
    let enabled = session.protocol(session.tab(), "Network.enable", "{}");
    println!("UBO_DEFAULT_NETWORK network_domain={enabled:?}");
    loopback.beacons.lock().unwrap().clear(); session.engine.navigate_tab(session.tab(), &loopback.page("dnr-external")).unwrap();
    pump_until(Duration::from_secs(8), || false);
    let events = events.borrow();
    let request_ids: Vec<_> = events.iter().filter(|(name, raw)| name == "Network.requestWillBeSent" && protocol_string(raw, Some("request"), "url").as_deref() == Some("https://pagead2.googlesyndication.com/pagead/gen_204?id=bowser_disposable_probe"))
        .filter_map(|(_, raw)| protocol_string(raw, None, "requestId")).collect();
    let mut blocked = false; let mut responded = false;
    for (name, raw) in events.iter() {
        if protocol_string(raw, None, "requestId").is_some_and(|id| request_ids.contains(&id)) {
            if name == "Network.loadingFailed" {
                let error = protocol_string(raw, None, "errorText"); let reason = protocol_string(raw, None, "blockedReason");
                println!("UBO_DEFAULT_NETWORK controlled_resource_failure error={error:?} reason={reason:?}");
                blocked |= error.as_deref() == Some("net::ERR_BLOCKED_BY_CLIENT");
            }
            responded |= name == "Network.responseReceived";
        }
    }
    let control = loopback.beacons.lock().unwrap().iter().any(|path| path == "/control-script.js");
    println!("UBO_DEFAULT_NETWORK requested={} blocked_by_client={blocked} received_response={responded} own_control_requested={control}", !request_ids.is_empty());
    (!request_ids.is_empty(), blocked, control, responded)
}

fn probe_real_ubo_default_external(root: &Path, session: &mut Session, loopback: &Loopback) {
    let baseline_loopback = Loopback::start();
    let mut baseline = Session::open(&root.join("zero-default-external-control"), vec![], vec![], &baseline_loopback.page("external-control-ready"));
    pump_until(Duration::from_secs(2), || false);
    let zero = observe_default_ubo_network(&mut baseline, &baseline_loopback); baseline.close(); drop(baseline_loopback);
    let actual = observe_default_ubo_network(session, loopback);
    println!("UBO_DEFAULT_NETWORK zero={zero:?} actual_ubo={actual:?} verdict={} scope=one-real-default-ad-domain-request-not-universal-filtering-or-correct-tab-action", if zero.0 && !zero.1 && zero.2 && zero.3 && actual.0 && actual.1 && actual.2 { "actual-default-filter-network-effect-demonstrated-partial" } else { "default-filter-effect-not-demonstrated" });
}

/// Genuine Stylus backup import through its real manager's ordinary DOM drop handler. No package
/// edits, injected CSS, internal extension API replacement, or fake Chrome globals are involved.
fn probe_real_stylus_appearance(profile: &Path, id: &str, folder: &Path, session: &mut Session, loopback: &Loopback) {
    let target = loopback.page("stylus-appearance");
    // Use the production visible popup and actual Windows accessibility/native context menu.
    // Keep pumping the owning STA while the separate automation process sends genuine input.
    let opened = Rc::new(RefCell::new(None)); let sink = opened.clone();
    session.engine.open_extension_page(id, folder, extensions::PageKind::Popup, "Bowser disposable Stylus navigation", Box::new(move |result| *sink.borrow_mut() = Some(result)));
    assert!(pump_until(Duration::from_secs(30), || opened.borrow().is_some())); opened.borrow_mut().take().unwrap().unwrap();
    pump_until(Duration::from_secs(2), || false);
    assert!(pump_until(Duration::from_secs(30), || session.page_script("!!document.querySelector('#popup-manage-button')").as_deref() == Some("true")), "actual signed popup did not expose its Manage link");
    let expected = session.page_script("document.querySelector('#popup-manage-button').href").unwrap();
    let Some(extensions::json::Value::String(expected_url)) = extensions::json::parse(&expected) else { panic!("actual Manage href was not a string"); };
    let expected_url = Url::parse(&expected_url).unwrap();
    assert!(extensions::is_page_url(&expected_url));
    assert_eq!(expected_url.host_str(), session.engine.probe_loaded_extension_id(id).as_deref());
    let actual_bounds = |session: &mut Session| {
        let bounds = session.page_script("(()=>{const r=document.querySelector('#popup-manage-button').getBoundingClientRect();return [r.left,r.top,r.width,r.height,devicePixelRatio,innerWidth,innerHeight]})()").unwrap();
    let Some(extensions::json::Value::Array(bounds)) = extensions::json::parse(&bounds) else { panic!("actual link bounds were not an array"); };
    let bounds: Vec<f64> = bounds.into_iter().map(|value| match value {
        extensions::json::Value::Number(value) => value.parse::<f64>().unwrap(),
        _ => panic!("actual link bound was not numeric"),
    }).collect();
    assert_eq!(bounds.len(), 7);
    assert!(bounds.iter().all(|value| value.is_finite() && *value >= 0.0 && *value <= 32767.0));
    assert!(bounds[2] > 0.0 && bounds[3] > 0.0 && bounds[4] > 0.0 && bounds[4] <= 4.0);
    let client_x = ((bounds[0] + bounds[2] / 2.0) * bounds[4]).round() as i32;
    let client_y = ((bounds[1] + bounds[3] / 2.0) * bounds[4]).round() as i32;
    let viewport_width = (bounds[5] * bounds[4]).round() as i32;
    let viewport_height = (bounds[6] * bounds[4]).round() as i32;
        (client_x, client_y, viewport_width, viewport_height)
    };
    let run_native = |mode: &str, output: &Path, bounds: (i32,i32,i32,i32)| {
        let (client_x,client_y,viewport_width,viewport_height) = bounds;
    let mut automation = std::process::Command::new("pwsh").args(["-NoProfile", "-File"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/scripts/probe-native-extension-link.ps1"))
        .arg("-OwnerPid").arg(std::process::id().to_string()).arg("-Out").arg(output)
        .arg("-ClientX").arg(client_x.to_string()).arg("-ClientY").arg(client_y.to_string())
        .arg("-ViewportWidth").arg(viewport_width.to_string()).arg("-ViewportHeight").arg(viewport_height.to_string())
        .arg("-Mode").arg(mode).arg("-ExpectedLinkLabel").arg(expected_url.path().rsplit('/').next().unwrap())
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    if !pump_until(Duration::from_secs(75), || automation.try_wait().is_ok_and(|status| status.is_some())) {
        let _ = automation.kill();
        let _ = automation.wait();
        panic!("bounded native link automation timed out; its owned helper was stopped");
    }
    let automation = automation.wait_with_output().unwrap();
    pump_until(Duration::from_secs(2), || false);
        automation
    };
    let output = std::env::var_os("RUNNER_TEMP").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("bowser-extension-link-native");
    crate::extension_window_probe::expected_destination(Some(expected_url.clone()));
    crate::extension_window_probe::interval("explicit-no-input-baseline");
    pump_until(Duration::from_secs(2), || false);
    assert_eq!(session.page_script("(()=>{const key='__bowser_native_manage_click_v1',href=document.querySelector('#popup-manage-button').href;sessionStorage.removeItem(key);document.addEventListener('click',e=>{const a=e.target instanceof Element&&e.target.closest('#popup-manage-button');if(e.isTrusted&&e.button===0&&a&&a.href===href)sessionStorage.setItem(key,'trusted-exact-link')},true);return true})()").as_deref(),Some("true"));
    crate::extension_window_probe::interval("real-owned-manage-left-click");
    let click = run_native("LinkClick", &output.join("actual-left-click"), actual_bounds(session));
    assert!(click.status.success(), "actual owned left-click helper failed before native observation");
    let trusted = session.page_script("sessionStorage.getItem('__bowser_native_manage_click_v1')");
    let reached = session.page_script("location.href").as_deref()==Some(expected.as_str()) && session.page_script("!!document.querySelector('#import')").as_deref()==Some("true");
    assert!(trusted.as_deref()==Some("\"trusted-exact-link\""), "actual trusted Manage click was not independently observed");
    println!("NATIVE_WINDOW_CLICK trusted_receipt={} exact_same_view_manager={} scope=observation-only-routing-unchanged",trusted.as_deref()==Some("\"trusted-exact-link\""),reached);
    crate::extension_window_probe::interval("reopen-independent-popup");
    // Reopen visibly: a preceding successful route cannot rescue the independent Page links test.
    let opened = Rc::new(RefCell::new(None)); let sink = opened.clone();
    session.engine.open_extension_page(id, folder, extensions::PageKind::Popup, "Bowser disposable Stylus navigation", Box::new(move |result| *sink.borrow_mut()=Some(result)));
    assert!(pump_until(Duration::from_secs(30),|| opened.borrow().is_some())); opened.borrow_mut().take().unwrap().unwrap();
    assert!(pump_until(Duration::from_secs(30),|| session.page_script("document.querySelector('#popup-manage-button')?.href").as_deref()==Some(expected.as_str())));
    crate::extension_window_probe::interval("independent-native-page-links");
    let automation = run_native("PageLinks", &output, actual_bounds(session));
    println!("STYLUS_NATIVE_LINK exit={:?} stdout={} stderr={} actual_page={:?}", automation.status.code(), String::from_utf8_lossy(&automation.stdout), String::from_utf8_lossy(&automation.stderr), session.page_script("({url:location.href,title:document.title,importButton:!!document.querySelector('#import')})"));
    super::extension_links::assert_no_modal_timeout();
    assert!(automation.status.success(), "real owned Page links helper failed; no scripted fallback is accepted as menu evidence");
    assert!(pump_until(Duration::from_secs(30), || {
        session.page_script("location.href").as_deref() == Some(expected.as_str())
            && session.page_script("!!document.querySelector('#import')").as_deref() == Some("true")
    }), "native Page links selection did not reach the exact real signed manager href and manager DOM");
    println!("STYLUS_NATIVE_LINK verdict=real-owned-page-links-command-exact-manager-destination-and-dom-proven expected={expected_url}");
    // Separate each comparison from the successful native-menu document. Neither scripted route
    // is permitted to rescue the mandatory real-input assertion above.
    session.open_page(id, folder, extensions::PageKind::Popup).unwrap();
    assert!(pump_until(Duration::from_secs(30), || session.page_script("!!document.querySelector('#popup-manage-button')").as_deref() == Some("true")));
    crate::extension_window_probe::interval("independent-scripted-click");
    let manager_link = session.page_script("(()=>{const a=document.querySelector('#popup-manage-button');if(!a)return null;const href=a.href;a.click();return href})()");
    pump_until(Duration::from_secs(3), || false);
    println!("STYLUS_APPEARANCE ordinary_manage_click={manager_link:?} resulting_page={:?}", session.page_script("({url:location.href,title:document.title,importButton:!!document.querySelector('#import')})"));
    session.open_page(id, folder, extensions::PageKind::Popup).unwrap();
    assert!(pump_until(Duration::from_secs(30), || session.page_script("!!document.querySelector('#popup-manage-button')").as_deref() == Some("true")));
    // Exercise the standard native new-window link route (as offered by a link context menu),
    // not a guessed page path, location assignment, tabs shim or claim about chrome.tabs.create.
    crate::extension_window_probe::interval("independent-scripted-window-open");
    let followed = session.page_script("(()=>{const a=document.querySelector('#popup-manage-button');if(!a)return false;const u=new URL(a.href);if(u.protocol!==location.protocol||u.host!==location.host)return false;window.open(a.href,'_blank');return true})()");
    pump_until(Duration::from_secs(4), || false);
    println!("STYLUS_APPEARANCE same_extension_native_window_link_requested={followed:?} resulting_page={:?} scope=standard-new-window-native-handler-not-chrome-tabs-create", session.page_script("({url:location.href,title:document.title,importButton:!!document.querySelector('#import')})"));
    crate::extension_window_probe::interval("independent-forbidden-destinations");
    let escape = loopback.page("extension-window-escape");
    let script = format!("(()=>{{window.open({},'_blank');window.open('bowser-ui://app/index.html?view=settings','_blank');window.open('chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/options.html','_blank');return location.href}})()", crate::chrome_host::protocol::json_string(escape.as_str()));
    let before_escape = session.page_script(&script); pump_until(Duration::from_secs(1), || false);
    let after_escape = session.page_script("location.href");
    let escaped = loopback.beacons.lock().unwrap().iter().any(|path| path == "/extension-window-escape");
    assert_eq!(before_escape, after_escape, "forbidden new-window request changed the signed extension page"); assert!(!escaped, "external new-window request escaped its isolated host");
    println!("STYLUS_APPEARANCE external_ui_other_extension_new_window_attempts=made same_page_retained=true own_loopback_escape_absent=true scope=observe-native-event-log-for-each-request-not-universal-window-coverage");
    crate::extension_window_probe::expected_destination(None);
    crate::extension_window_probe::interval("post-observation");
    let style = format!(r#"[{{"name":"Bowser disposable appearance fixture","enabled":true,"sections":[{{"code":"body {{ background-color: rgb(7, 11, 19) !important; }}","urls":[{}],"urlPrefixes":[],"domains":[],"regexps":[]}}]}}]"#, crate::chrome_host::protocol::json_string(target.as_str()));
    let script = format!(r#"(()=>{{if(typeof document.body.ondrop!=='function')return {{dropHandler:false,title:document.title}};const data=new DataTransfer();data.items.add(new File([{}],'bowser-disposable-style.json',{{type:'application/json'}}));document.body.dispatchEvent(new DragEvent('drop',{{dataTransfer:data,bubbles:true,cancelable:true}}));return {{dropHandler:true,title:document.title}}}})()"#, crate::chrome_host::protocol::json_string(&style));
    let imported = session.page_script(&script); pump_until(Duration::from_secs(5), || false);
    println!("STYLUS_APPEARANCE import_dom={imported:?} manager_report={:?}", session.page_script("({title:document.title,fixtureVisible:document.body?.textContent?.includes('Bowser disposable appearance fixture'),text:document.body?.textContent?.slice(0,1200)})"));
    session.engine.navigate_tab(session.tab(), &target).unwrap(); pump_until(Duration::from_secs(4), || false);
    // The production reader deliberately caps results at64 bytes. Read only the actual
    // computed colour, never widen the production cap to accommodate a diagnostic.
    let read = "getComputedStyle(document.body).backgroundColor";
    let styled = session.browsing_script(read);
    session.engine.navigate_tab(session.tab(), &loopback.page("stylus-unmatched-control")).unwrap(); pump_until(Duration::from_secs(2), || false);
    let unmatched = session.browsing_script(read);
    println!("STYLUS_APPEARANCE exact_url={target} styled={styled:?} unmatched={unmatched:?} package_unchanged=true");
    let effect = styled.as_deref().is_some_and(|value| value.contains("rgb(7, 11, 19)")) && unmatched.as_deref().is_some_and(|value| !value.contains("rgb(7, 11, 19)"));
    println!("STYLUS_APPEARANCE verdict={}", if effect { "actual-imported-style-effect-and-url-boundary-proven-partial" } else { "appearance-effect-not-demonstrated" });
    assert!(effect, "the real imported appearance style or unmatched-URL control failed");
    // A genuinely new executable process must retain the imported style, not merely a second
    // document in the same process. Keep the parent loopback server alive, close its engine first.
    session.engine.close().unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "engine::extension_probe_live::live_real_stylus_restart_child", "--ignored", "--nocapture", "--test-threads=1"])
        .env("BOWSER_STYLUS_RESTART_PROFILE", profile).env("BOWSER_STYLUS_RESTART_ID", id).env("BOWSER_STYLUS_RESTART_URL", target.as_str()).output().unwrap();
    println!("STYLUS_APPEARANCE restart_child_exit={:?} stdout={} stderr={}", child.status.code(), String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
    assert!(child.status.success(), "the imported real appearance style did not survive a new executable process");
    let restarted = Session::open(profile, vec![(id.to_owned(), folder.to_owned())], vec![(id.to_owned(), true)], &target);
    let old = std::mem::replace(session, restarted); old.close();
    assert!(session.wait_for_add(profile, id).1.is_none());
    // A cached native object can become visible before every queued startup callback releases
    // its production lease. Observe actual quiescence; never call a busy rejection a removal.
    assert!(pump_until(Duration::from_secs(30), || {
        extensions::RecordsLease::runtime(&profile.join("extensions.txt")).is_ok()
    }), "real appearance profile did not finish its queued startup operations");
    // Real native Remove, then the same under-lease durable commit the product uses.
    let result = Rc::new(RefCell::new(None)); let sink = result.clone(); let records = profile.join("extensions.txt"); let extensions_dir = profile.join("Extensions"); let removed_id = id.to_owned();
    session.engine.remove_extension(id, Box::new(move |native| {
        let answer = native.and_then(|()| extensions::uninstall_under_runtime(&extensions_dir, &records, &removed_id).map_err(|error| error.to_string()));
        *sink.borrow_mut() = Some(answer);
    }));
    pump_until(Duration::from_secs(15), || result.borrow().is_some());
    session.engine.navigate_tab(session.tab(), &target).unwrap(); pump_until(Duration::from_secs(3), || false);
    let removed = session.browsing_script(read);
    println!("STYLUS_APPEARANCE remove={:?} after_remove={removed:?} folder_exists={} scope=actual-import-style-url-boundary-new-process-restart-and-removal-not-all-features", result.borrow(), folder.exists());
    assert!(result.borrow().as_ref().is_some_and(|answer| answer.is_ok()), "real appearance native/durable removal did not complete");
    assert!(!folder.exists(), "removed appearance package still has its managed folder");
    assert!(removed.as_deref().is_some_and(|value| !value.contains("rgb(7, 11, 19)")), "removed real appearance style still affects a newly loaded page");
}

/// Real catalogue popup queries with two distinct Bowser browsing views on the same fixture URL.
/// This is observation only: no URL/title-based ID binding, guessed senderTabId or API shim.
fn probe_real_tab_topology(name: &str, session: &mut Session, loopback: &Loopback) {
    let duplicate = loopback.page("duplicate-url-control");
    session.engine.navigate_tab(TabId(1), &duplicate).unwrap();
    session.engine.select_tab(TabId(2)).unwrap(); session.engine.navigate_tab(TabId(2), &duplicate).unwrap();
    pump_until(Duration::from_secs(3), || false); session.engine.select_tab(TabId(1)).unwrap();
    let script = "(()=>{const result={};const queries={all:{},activeCurrent:{active:true,currentWindow:true},activeLastFocused:{active:true,lastFocusedWindow:true}};Promise.all(Object.entries(queries).map(([key,q])=>new Promise(resolve=>chrome.tabs.query(q,tabs=>{result[key]={error:chrome.runtime.lastError?.message||null,tabs:tabs?.map(t=>({id:t.id,windowId:t.windowId,active:t.active,incognito:t.incognito,url:t.url}))};resolve()})))).then(()=>document.documentElement.setAttribute('data-bowser-topology',JSON.stringify(result)));return true})()";
    session.page_script(script); pump_until(Duration::from_secs(2), || false);
    let report = session.page_script("document.documentElement.getAttribute('data-bowser-topology')");
    println!("CATALOGUE_TOPOLOGY name={name} host_browser_window_count=1 host_browsing_view_count=2 duplicate_url_control=true genuine_popup_queries={report:?} scope=observation-only-no-id-adapter-or-guessed-target");
}

/// Click the genuine TSM save control in its own DOM. The fixture has exactly one Bowser
/// window and two browsing views; preserve its real saved-window count rather than rewriting IDs.
/// Do not invoke restore-current: its documented behaviour removes other native windows.
fn probe_real_tsm_save(session: &mut Session) {
    let clicked = session.page_script("(()=>{const input=document.querySelector('#saveArea input');const save=document.querySelector('#saveArea .saveButton');if(!input||!save)return false;input.value='Bowser disposable two-tab session';save.click();return true})()");
    pump_until(Duration::from_secs(4), || false);
    let report = session.page_script("({sessions:[...document.querySelectorAll('.session')].map(e=>({text:e.textContent?.slice(0,900),buttons:[...e.querySelectorAll('button')].map(b=>({text:b.textContent,title:b.title,className:b.className}))})),body:document.body.textContent?.slice(0,1600)})");
    println!("TSM_REAL_SAVE genuine_save_click={clicked:?} actual_saved_session_dom={report:?} host_window_count=1 host_browsing_view_count=2 scope=real-save-only-not-restore-or-correct-host-window-grouping");
}

/// Real packages, fresh throwaway profiles, no accounts and no user data. This proves package
/// identity/load and genuine extension context only, not product functionality or action grants.
#[test]
#[ignore = "live store network and WebView2; explicit disposable Windows acceptance diagnostic"]
fn live_store_current_extension_loads() {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let runtime = WebView2Engine::availability().expect("installed runtime");
    let version = runtime.split_whitespace().next().unwrap();
    let root = temp_root();
    let cancelled = AtomicBool::new(false);
    let mut loaded = 0;
    // Optional handoff to the native executable measurement in this same disposable runner.
    // Never an artifact or user profile; source packages are deleted by the following script.
    let measurement = std::env::var_os("BOWSER_EXTENSION_MEASURE_ROOT").map(PathBuf::from);
    if let Some(path) = &measurement {
        let runner_temp = std::env::var_os("RUNNER_TEMP").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        assert!(path.starts_with(runner_temp) && !path.exists(), "measurement root must be fresh under this runner's temp directory");
        std::fs::create_dir_all(path.join("zero")).unwrap();
        std::fs::create_dir(path.join("several")).unwrap();
    }
    println!("REAL_STORE runtime={version}");
    for (name, id) in [
        ("ublock-origin-lite", "ddkjiahejlhfcafbddmgiahcphecmpfh"),
        ("bitwarden", "nngceckbapebfimnlniiiahkandclblb"),
        ("stylus", "clngdbkpkpeebahjckkjfobafhncgmne"),
        ("tab-session-manager", "iaiomicjabeggjcfkbimgmglanimpnae"),
    ] {
        let profile = root.join(name);
        let review = match crate::store::prepare(id, version, &profile.join("Extensions"), &profile.join("extensions.txt"), &cancelled) {
            Ok(review) => review,
            Err(reason) => { println!("REAL_STORE name={name} id={id} stage=prepare failed={reason}"); continue; }
        };
        let reviewed = review.prepared.inspection().manifest.clone();
        println!("REAL_STORE name={name} id={id} version={} stage=permission_review required={:?} hosts={:?} scripts={:?} optional={:?} optional_hosts={:?}", reviewed.version, reviewed.permissions, reviewed.host_permissions, reviewed.content_script_matches, reviewed.optional_permissions, reviewed.optional_host_permissions);
        if let Some(path) = &measurement {
            let several = path.join("several");
            let prepared = extensions::prepare_install(&review.prepared.folder().unwrap(), &several.join("Extensions"), &several.join("extensions.txt")).unwrap();
            assert_eq!(prepared.inspection(), review.prepared.inspection());
            // Explicit repeated consent to the same complete declarations printed immediately above.
            let copied = prepared.commit_named(&review.name).unwrap();
            println!("REAL_STORE measurement_copy id={} name={} version={} consent=explicit-disposable-profile", copied.id, copied.name, copied.version);
        }
        // Explicit test-harness consent to exactly the verified copy, never credentials/accounts.
        let record = review.commit().expect("test-only reviewed package consent");
        let folder = extensions::folder_for(&profile.join("Extensions"), &record.id).unwrap();
        let loopback = Loopback::start();
        let mut session = Session::open(&profile, vec![(record.id.clone(), folder.clone())], vec![(record.id.clone(), true)], &loopback.page("real-first"));
        let (webview_id, failure) = session.wait_for_add(&profile, &record.id);
        println!("REAL_STORE name={name} version={} signed_id={id} runtime_id={webview_id:?} load_failure={failure:?}", reviewed.version);
        if webview_id.as_deref() == Some(id) && failure.is_none() {
            loaded += 1;
            for (label, kind) in [("popup", extensions::PageKind::Popup), ("options", extensions::PageKind::Options)] {
                if !extensions::declares_page(&folder, kind) { println!("REAL_STORE name={name} page={label} declared=false"); continue; }
                let opened = session.open_page(&record.id, &folder, kind);
                println!("REAL_STORE name={name} page={label} open={opened:?}");
                if opened.is_ok() {
                    pump_until(Duration::from_secs(3), || false);
                    let context = session.page_script("({origin:location.origin,runtimeId:typeof chrome==='object'&&chrome.runtime?chrome.runtime.id:null,bodyTextLength:document.body?.textContent?.length||0,tabs:typeof chrome?.tabs,windows:typeof chrome?.windows,action:typeof chrome?.action,storage:typeof chrome?.storage,dnr:typeof chrome?.declarativeNetRequest})");
                    println!("REAL_STORE name={name} page={label} genuine_context={context:?}");
                    session.page_script("(()=>{chrome.tabs.query({},tabs=>document.documentElement.setAttribute('data-bowser-api-probe',JSON.stringify({lastError:chrome.runtime.lastError?.message||null,tabs:tabs?.map(t=>({id:t.id,windowId:t.windowId,active:t.active,incognito:t.incognito,url:t.url}))})));return true})()");
                    pump_until(Duration::from_secs(2), || false);
                    println!("REAL_STORE name={name} page={label} tabs_query={:?}", session.page_script("document.documentElement.getAttribute('data-bowser-api-probe')"));
                    if label == "popup" && matches!(name, "bitwarden" | "tab-session-manager") { probe_real_tab_topology(name, &mut session, &loopback); }
                    if label == "popup" && name == "tab-session-manager" { probe_real_tsm_save(&mut session); }
                }
            }
        }
        if name == "ublock-origin-lite" && failure.is_none() && webview_id.as_deref() == Some(id) {
            probe_real_blocker_network(&root, &mut session, &loopback);
            probe_real_ubo_default_external(&root, &mut session, &loopback);
        }
        if name == "stylus" && failure.is_none() && webview_id.as_deref() == Some(id) {
            probe_real_stylus_appearance(&profile, &record.id, &folder, &mut session, &loopback);
        }
        session.close();
        drop(loopback);
    }
    if let Some(path) = &measurement {
        assert_eq!(extensions::read_records(&path.join("several/extensions.txt")).unwrap().len(), 4, "four verified packages are required for the native several-extension comparison");
    }
    remove_with_retries(&root);
    assert!(loaded > 0, "no real signed current extension loaded; inspect diagnostic reasons");
}

/// Isolated runtime feasibility only: it deliberately does not enable production updates. Uses
/// two immutable local fixture copies sharing a key; signature/update-consent unit tests are separate.
#[test]
#[ignore = "live WebView2 update/rollback feasibility; disposable profiles only"]
fn live_probe_same_identity_update_and_rollback() {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let root = temp_root();
    let profile = root.join("update-profile");
    let original = std::fs::read_to_string(fixture("probe-mv3-key").join("manifest.json")).unwrap();
    let base = original.trim().strip_suffix('}').unwrap();
    let manifest = format!("{base},\"action\":{{\"default_popup\":\"popup.html\"}}}}");
    let first_source = root.join("first-source");
    let second_source = root.join("second-source");
    for (source, version) in [(&first_source, "0.0.1"), (&second_source, "0.0.2")] {
        std::fs::create_dir(source).unwrap();
        for name in ["worker.js", "content.js"] { std::fs::copy(fixture("probe-mv3-key").join(name), source.join(name)).unwrap(); }
        std::fs::write(source.join("manifest.json"), manifest.replace("0.0.1", version)).unwrap();
        std::fs::write(source.join("popup.html"), "<!doctype html><title>Disposable update probe</title><p>Update probe</p><script src=\"popup.js\"></script>").unwrap();
        std::fs::write(source.join("popup.js"), "document.documentElement.setAttribute('data-ready','yes');").unwrap();
    }
    let first_record = extensions::install(&first_source, &profile.join("Extensions"), &profile.join("extensions.txt")).unwrap();
    let id = first_record.id;
    let first = extensions::folder_for(&profile.join("Extensions"), &id).unwrap();
    let second_record = extensions::install(&second_source, &root.join("versions"), &root.join("version-records.txt")).unwrap();
    let second = extensions::folder_for(&root.join("versions"), &second_record.id).unwrap();
    let loopback = Loopback::start();
    let mut session = Session::open(&profile, vec![(id.clone(), first.clone())], vec![(id.clone(), true)], &loopback.page("before-update"));
    let (runtime_id, failure) = session.wait_for_add(&profile, &id);
    assert_eq!(runtime_id.as_deref(), Some(KEY_FIXTURE_ID)); assert!(failure.is_none());
    session.open_page(&id, &first, extensions::PageKind::Popup).unwrap();
    pump_until(Duration::from_secs(2), || false);
    session.page_script("(()=>{chrome.storage.local.set({bowserUpdateSentinel:'disposable-test-value'},()=>document.documentElement.setAttribute('data-update-written','yes'));return true})()");
    pump_until(Duration::from_secs(1), || false);
    let first_manifest_before = std::fs::read(first.join("manifest.json")).unwrap();
    let second_manifest_before = std::fs::read(second.join("manifest.json")).unwrap();
    for (label, folder, expect_version) in [("update", second.as_path(), "0.0.2"), ("rollback", first.as_path(), "0.0.1")] {
        let result = session.replace(&id, folder);
        println!("UPDATE_PROBE phase={label} add={result:?}");
        result.unwrap();
        session.open_page(&id, folder, extensions::PageKind::Popup).unwrap();
        pump_until(Duration::from_secs(2), || false);
        session.page_script("(()=>{chrome.storage.local.get('bowserUpdateSentinel',s=>document.documentElement.setAttribute('data-update-state',JSON.stringify({id:chrome.runtime.id,version:chrome.runtime.getManifest().version,sentinel:s.bowserUpdateSentinel||null})));return true})()");
        pump_until(Duration::from_secs(1), || false);
        let state = session.page_script("document.documentElement.getAttribute('data-update-state')").unwrap_or_default();
        println!("UPDATE_PROBE phase={label} state={state}");
        assert!(state.contains(KEY_FIXTURE_ID) && state.contains(expect_version) && state.contains("disposable-test-value"), "identity/version/storage not preserved: {state}");
        assert_eq!(std::fs::read(first.join("manifest.json")).unwrap(), first_manifest_before);
        assert_eq!(std::fs::read(second.join("manifest.json")).unwrap(), second_manifest_before);
    }
    // An invalid staged replacement must not remove the working previous version.
    let bad = root.join("bad-version"); std::fs::create_dir(&bad).unwrap();
    std::fs::write(bad.join("manifest.json"), "not json").unwrap();
    let refused = session.replace(&id, &bad);
    println!("UPDATE_PROBE phase=invalid-replacement result={refused:?}");
    assert!(refused.is_err());
    session.open_page(&id, &first, extensions::PageKind::Popup).unwrap();
    pump_until(Duration::from_secs(2), || false);
    let live_version = session.page_script("chrome.runtime.getManifest().version").unwrap_or_default();
    println!("UPDATE_PROBE phase=after-invalid-replacement version={live_version}");
    assert!(live_version.contains("0.0.1"));
    let targets = session.protocol(session.tab(), "Target.getTargets", r#"{"filter":[{}]}"#);
    println!("API_PROBE scope=disposable-profile target_inventory={targets:?}");
    let enabled = session.protocol(session.tab(), "ServiceWorker.enable", "{}");
    println!("API_PROBE worker_domain_enable={enabled:?}");
    let stopped = if enabled.is_some() { session.protocol(session.tab(), "ServiceWorker.stopAllWorkers", "{}") } else { None };
    println!("API_PROBE worker_stop={stopped:?}");
    session.close(); drop(loopback); remove_with_retries(&root);
}

/// Genuine action/activeTab feasibility, not a synthesized click or a host-side permission grant.
/// This fixture has no host permissions and cannot inject until a real action grant exists.
#[test]
#[ignore = "live in-process CDP action feasibility; disposable fixture only"]
fn live_probe_native_action_active_tab() {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let root = temp_root();
    let source = root.join("active-tab-source"); std::fs::create_dir(&source).unwrap();
    let raw = std::fs::read_to_string(fixture("probe-mv3-key").join("manifest.json")).unwrap();
    let key = protocol_string(&raw, None, "key").expect("fixture development public key");
    let manifest = format!(r#"{{"manifest_version":3,"name":"Disposable activeTab feasibility","version":"0.0.1","key":"{key}","permissions":["activeTab","scripting"],"action":{{"default_title":"Test action"}},"background":{{"service_worker":"worker.js"}}}}"#);
    std::fs::write(source.join("manifest.json"), manifest).unwrap();
    std::fs::write(source.join("worker.js"), r#"chrome.action.onClicked.addListener(tab=>{
      chrome.scripting.executeScript({target:{tabId:tab.id},func:()=>document.documentElement.setAttribute('data-action-active-tab','granted-by-real-action')});
    });"#).unwrap();
    let profile = root.join("action-profile");
    let record = extensions::install(&source, &profile.join("Extensions"), &profile.join("extensions.txt")).unwrap();
    let folder = extensions::folder_for(&profile.join("Extensions"), &record.id).unwrap();
    let loopback = Loopback::start();
    let mut session = Session::open(&profile, vec![(record.id.clone(), folder)], vec![(record.id.clone(), true)], &loopback.page("action-target"));
    let (runtime_id, failure) = session.wait_for_add(&profile, &record.id);
    assert_eq!(runtime_id.as_deref(), Some(KEY_FIXTURE_ID)); assert!(failure.is_none());
    pump_until(Duration::from_secs(2), || false);
    assert!(!session.read_marker("data-action-active-tab").unwrap_or_default().contains("granted-by-real-action"));
    let targets = session.protocol(session.tab(), "Target.getTargets", r#"{"filter":[{}]}"#);
    println!("ACTION_PROBE inventory={targets:?}");
    let info = session.protocol(session.tab(), "Target.getTargetInfo", "{}");
    println!("ACTION_PROBE owning_target={info:?}");
    // Runtime 154.0.4258.62 explicitly refused Target.attachToBrowserTarget with protocol
    // -32000 "Not allowed" (run39087008886). Do not retry that denied route or use flags/ports
    // to reach the same browser target. Discovery is not a grant or a compatibility pass.
    println!("ACTION_PROBE verdict=unsupported-browser-target-route-explicitly-denied-no-action-attempted");
    session.close(); drop(loopback); remove_with_retries(&root);
}


/// Delivers at most one actual Windows/COM message before testing the boundary again. This lets
/// the diagnostic observe a real pending native callback, not replace it with fake API success.
fn pump_one_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if done() { return true; }
        unsafe {
            let mut message = MSG::default();
            if PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message); DispatchMessageW(&message);
            } else { std::thread::sleep(Duration::from_millis(5)); }
        }
    }
    done()
}

fn wait_file_without_parent_com(path: &Path) {
    let started = Instant::now();
    while !path.is_file() && started.elapsed() < Duration::from_secs(30) { std::thread::sleep(Duration::from_millis(10)); }
    assert!(path.is_file(), "competing child did not acknowledge the real pending native phase: {}", path.display());
}

#[test]
#[ignore = "real delayed native inventory/Add and two-process competing mutation; disposable Windows profiles only"]
fn live_probe_pending_native_operations() {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let root = temp_root(); let profile = root.join("shared-profile");
    let record = extensions::install(&fixture("probe-mv3-key"), &profile.join("Extensions"), &profile.join("extensions.txt")).unwrap();
    let folder = extensions::folder_for(&profile.join("Extensions"), &record.id).unwrap();
    let loopback = Loopback::start();
    let mut session = Session::open(&profile, vec![], vec![(record.id.clone(), true)], &loopback.page("parent-ready"));
    assert!(pump_one_until(Duration::from_secs(30), || session.engine.probe_extension_profile_ready()));
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "engine::extension_probe_live::live_runtime_competing_child", "--ignored", "--nocapture", "--test-threads=1"])
        .env("BOWSER_PENDING_NATIVE_ROOT", &root).env("BOWSER_PENDING_NATIVE_ID", &record.id)
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    assert!(pump_one_until(Duration::from_secs(45), || root.join("child-ready").is_file()));
    let answer = Rc::new(RefCell::new(None)); let sink = answer.clone();
    session.engine.set_extension_enabled(&record.id, &folder, true, Box::new(move |result| *sink.borrow_mut() = Some(result)));
    assert!(answer.borrow().is_none(), "runtime delivered inventory/Add inline; this run did not establish a pending callback boundary");
    // Do not pump parent COM while the other real engine/process attempts its mutations.
    std::fs::write(root.join("go-inventory"), b"real-native-inventory-pending").unwrap();
    wait_file_without_parent_com(&root.join("ack-inventory"));
    println!("PENDING_NATIVE phase=GetBrowserExtensions competing_process=busy record_unchanged=true");
    let ids = profile.join("extension_ids.txt");
    assert!(pump_one_until(Duration::from_secs(30), || mapping_for(&ids, &record.id).is_some()));
    assert!(answer.borrow().is_none(), "native Add completed inline; this run did not establish the second pending callback boundary");
    std::fs::write(root.join("go-add"), b"real-native-Add-called-callback-pending").unwrap();
    wait_file_without_parent_com(&root.join("ack-add"));
    println!("PENDING_NATIVE phase=AddBrowserExtension competing_process=busy record_unchanged=true actual_completion_pending=true");
    assert!(pump_one_until(Duration::from_secs(30), || answer.borrow().is_some()));
    answer.borrow_mut().take().unwrap().unwrap();
    assert_eq!(session.engine.probe_loaded_extension_id(&record.id).as_deref(), Some(KEY_FIXTURE_ID));
    assert!(extensions::read_authority_records(&profile.join("extensions.txt")).unwrap()[0].enabled);
    let output = child.wait_with_output().unwrap();
    println!("PENDING_NATIVE child_stdout={} child_stderr={}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    // A real native off operation must commit only after its callback, then be removable.
    let disabled = Rc::new(RefCell::new(None)); let sink = disabled.clone();
    session.engine.set_extension_enabled(&record.id, &folder, false, Box::new(move |result| *sink.borrow_mut() = Some(result)));
    assert!(pump_one_until(Duration::from_secs(30), || disabled.borrow().is_some())); disabled.borrow_mut().take().unwrap().unwrap();
    assert!(!extensions::read_authority_records(&profile.join("extensions.txt")).unwrap()[0].enabled);
    let native = session.native_inventory().unwrap();
    assert!(native.iter().any(|(id, enabled)| id == KEY_FIXTURE_ID && !enabled), "actual native IsEnabled did not confirm off: {native:?}");
    println!("PENDING_NATIVE native_inventory_after_disable={native:?}");
    println!("PENDING_NATIVE final_native_disable_and_durable_commit=passed scope=real-two-process-pending-inventory-and-Add-not-crash-cut-points");
    session.close(); drop(loopback); remove_with_retries(&root);
}

#[test]
#[ignore = "child of the bounded real pending-native-operation diagnostic"]
fn live_runtime_competing_child() {
    let Some(root) = std::env::var_os("BOWSER_PENDING_NATIVE_ROOT").map(PathBuf::from) else { panic!("Only the owning diagnostic launches this child."); };
    let id = std::env::var("BOWSER_PENDING_NATIVE_ID").unwrap(); let profile = root.join("shared-profile");
    assert!(root.starts_with(std::env::temp_dir()) && root.file_name().unwrap().to_string_lossy().starts_with("bowser-ext-live-"));
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let loopback = Loopback::start(); let folder = extensions::folder_for(&profile.join("Extensions"), &id).unwrap();
    let mut session = Session::open(&profile, vec![], vec![(id.clone(), true)], &loopback.page("child-ready"));
    assert!(pump_one_until(Duration::from_secs(30), || session.engine.probe_extension_profile_ready()));
    std::fs::write(root.join("child-ready"), b"actual-second-engine-profile-ready").unwrap();
    for phase in ["inventory", "add"] {
        assert!(pump_one_until(Duration::from_secs(45), || root.join(format!("go-{phase}")).is_file()));
        let before = std::fs::read(profile.join("extensions.txt")).unwrap();
        let disabled = Rc::new(RefCell::new(None)); let sink = disabled.clone();
        session.engine.set_extension_enabled(&id, &folder, false, Box::new(move |result| *sink.borrow_mut() = Some(result)));
        assert!(pump_one_until(Duration::from_secs(10), || disabled.borrow().is_some()));
        let disable = disabled.borrow_mut().take().unwrap();
        let removed = Rc::new(RefCell::new(None)); let sink = removed.clone();
        session.engine.remove_extension(&id, Box::new(move |result| *sink.borrow_mut() = Some(result)));
        assert!(pump_one_until(Duration::from_secs(10), || removed.borrow().is_some()));
        let remove = removed.borrow_mut().take().unwrap();
        assert!(disable.as_ref().is_err_and(|reason| reason.contains("operation is finishing")), "disable did not honestly report busy: {disable:?}");
        assert!(remove.as_ref().is_err_and(|reason| reason.contains("operation is finishing")), "remove did not honestly report busy: {remove:?}");
        assert_eq!(std::fs::read(profile.join("extensions.txt")).unwrap(), before);
        assert!(folder.is_dir());
        println!("PENDING_NATIVE_CHILD phase={phase} disable={disable:?} remove={remove:?} record_and_folder_unchanged=true");
        std::fs::write(root.join(format!("ack-{phase}")), b"both-native-mutations-busy-record-unchanged").unwrap();
    }
    session.close(); drop(loopback);
}


#[test]
#[ignore = "precise disposable child process-crash cut points; not power-loss durability"]
fn live_probe_native_crash_recovery() {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    for cut in ["extension_add_preclaim_committed", "extension_add_native_complete", "extension_disable_native_complete", "extension_remove_native_complete"] {
        let root = temp_root(); let profile = root.join("crash-profile");
        let record = extensions::install(&fixture("probe-mv3-key"), &profile.join("Extensions"), &profile.join("extensions.txt")).unwrap();
        let folder = extensions::folder_for(&profile.join("Extensions"), &record.id).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "engine::extension_probe_live::live_runtime_crash_child", "--ignored", "--nocapture", "--test-threads=1"])
            .env("BOWSER_EXTENSION_CRASH_EVENT", cut).env("BOWSER_EXTENSION_CRASH_ROOT", &root).env("BOWSER_EXTENSION_CRASH_ID", &record.id).output().unwrap();
        println!("CRASH_RECOVERY cut={cut} child_exit={:?} stderr={}", output.status.code(), String::from_utf8_lossy(&output.stderr));
        assert_eq!(output.status.code(), Some(73), "exact native cut was not reached: {}", String::from_utf8_lossy(&output.stdout));
        assert_eq!(std::fs::read_to_string(root.join("crash-marker")).unwrap(), cut);
        let records = extensions::read_authority_records(&profile.join("extensions.txt")).unwrap();
        assert_eq!(records.len(), 1); assert!(records[0].enabled, "interrupted revocation must not pretend it durably committed");
        let loopback = Loopback::start();
        let mut recovered = Session::open(&profile, vec![(record.id.clone(), folder.clone())], vec![(record.id.clone(), true)], &loopback.page("crash-recovery"));
        let (id, failure) = recovered.wait_for_add(&profile, &record.id);
        assert_eq!(id.as_deref(), Some(KEY_FIXTURE_ID)); assert!(failure.is_none(), "recovery load failed: {failure:?}");
        let mut native = Vec::new();
        for _ in 0..8 { native = recovered.native_inventory().unwrap(); if native.iter().any(|(id, enabled)| id == KEY_FIXTURE_ID && *enabled) { break; } pump_until(Duration::from_millis(300), || false); }
        assert!(native.iter().any(|(id, enabled)| id == KEY_FIXTURE_ID && *enabled), "actual native enabled recovery was not established: {native:?}");
        assert!(recovered.wait_for_worker_reply(&loopback), "consented extension worker/content messaging did not recover");
        println!("CRASH_RECOVERY cut={cut} actual_native_inventory={native:?} worker_content_reply=true record_authority=prior-consented-enabled interrupted_revocation_not_accepted=true scope=signed-keyed-process-crash-not-power-loss");
        recovered.close(); drop(loopback); remove_with_retries(&root);
    }
}

#[test]
#[ignore = "child of the exact process-crash lifecycle diagnostic"]
fn live_runtime_crash_child() {
    let root = std::env::var_os("BOWSER_EXTENSION_CRASH_ROOT").map(PathBuf::from).expect("Only the owning diagnostic launches this child.");
    let id = std::env::var("BOWSER_EXTENSION_CRASH_ID").unwrap(); let cut = std::env::var("BOWSER_EXTENSION_CRASH_EVENT").unwrap();
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let profile = root.join("crash-profile"); let folder = extensions::folder_for(&profile.join("Extensions"), &id).unwrap(); let loopback = Loopback::start();
    let mut session = Session::open(&profile, vec![(id.clone(), folder.clone())], vec![(id.clone(), true)], &loopback.page("crash-owner"));
    let (loaded, failure) = session.wait_for_add(&profile, &id);
    assert_eq!(loaded.as_deref(), Some(KEY_FIXTURE_ID)); assert!(failure.is_none());
    let result = Rc::new(RefCell::new(None)); let sink = result.clone();
    match cut.as_str() {
        "extension_disable_native_complete" => session.engine.set_extension_enabled(&id, &folder, false, Box::new(move |answer| *sink.borrow_mut() = Some(answer))),
        "extension_remove_native_complete" => session.engine.remove_extension(&id, Box::new(move |answer| *sink.borrow_mut() = Some(answer))),
        _ => panic!("The exact Add cut should already have exited before its completion."),
    }
    pump_until(Duration::from_secs(30), || result.borrow().is_some());
    panic!("The exact crash cut was not delivered before completion: {:?}", result.borrow());
}


fn run_completed_state_child(root: &Path, expected: &str) {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "engine::extension_probe_live::live_runtime_completed_state_child", "--ignored", "--nocapture", "--test-threads=1"])
        .env("BOWSER_COMPLETED_STATE_ROOT", root).env("BOWSER_COMPLETED_STATE_EXPECTED", expected).output().unwrap();
    println!("COMPLETED_RESTART expected={expected} child_exit={:?} stdout={} stderr={}", output.status.code(), String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "the actual new-process completed-state control failed");
}

#[test]
#[ignore = "actual durable Disable and Remove persist across separate executable processes"]
fn live_probe_completed_revocations_restart() {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let root = temp_root(); let profile = root.join("state-profile");
    let record = extensions::install(&fixture("probe-mv3-key"), &profile.join("Extensions"), &profile.join("extensions.txt")).unwrap();
    let folder = extensions::folder_for(&profile.join("Extensions"), &record.id).unwrap(); let loopback = Loopback::start();
    let mut session = Session::open(&profile, vec![(record.id.clone(), folder.clone())], vec![(record.id.clone(), true)], &loopback.page("completed-enabled"));
    let (_, failure) = session.wait_for_add(&profile, &record.id); assert!(failure.is_none());
    assert!(session.native_inventory().unwrap().iter().any(|(id, on)| id == KEY_FIXTURE_ID && *on));
    let answer = Rc::new(RefCell::new(None)); let sink = answer.clone();
    session.engine.set_extension_enabled(&record.id, &folder, false, Box::new(move |result| *sink.borrow_mut() = Some(result)));
    assert!(pump_one_until(Duration::from_secs(30), || answer.borrow().is_some())); answer.borrow_mut().take().unwrap().unwrap();
    assert!(!extensions::read_authority_records(&profile.join("extensions.txt")).unwrap()[0].enabled);
    assert!(session.native_inventory().unwrap().iter().any(|(id, on)| id == KEY_FIXTURE_ID && !on));
    session.close(); drop(loopback); run_completed_state_child(&root, "off");
    let loopback = Loopback::start();
    let mut session = Session::open(&profile, vec![], vec![(record.id.clone(), false)], &loopback.page("completed-off"));
    assert!(pump_one_until(Duration::from_secs(30), || session.engine.probe_extension_profile_ready()));
    assert!(pump_one_until(Duration::from_secs(30), || extensions::RecordsLease::runtime(&profile.join("extensions.txt")).is_ok()));
    let answer = Rc::new(RefCell::new(None)); let sink = answer.clone(); let records = profile.join("extensions.txt"); let dir = profile.join("Extensions"); let id = record.id.clone();
    session.engine.remove_extension(&record.id, Box::new(move |native| {
        *sink.borrow_mut() = Some(native.and_then(|()| extensions::uninstall_under_runtime(&dir, &records, &id).map_err(|error| error.to_string())));
    }));
    assert!(pump_one_until(Duration::from_secs(30), || answer.borrow().is_some())); answer.borrow_mut().take().unwrap().unwrap();
    assert!(extensions::read_authority_records(&profile.join("extensions.txt")).unwrap().is_empty());
    assert!(!session.native_inventory().unwrap().iter().any(|(id, _)| id == KEY_FIXTURE_ID)); assert!(!folder.exists());
    session.close(); drop(loopback); run_completed_state_child(&root, "absent");
    println!("COMPLETED_RESTART verdict=actual-native-off-and-remove-stay-off-or-absent-in-new-processes scope=signed-keyed-fixture-not-all-extension-functionality");
    remove_with_retries(&root);
}

#[test]
#[ignore = "child of the completed durable revocation restart diagnostic"]
fn live_runtime_completed_state_child() {
    let root = std::env::var_os("BOWSER_COMPLETED_STATE_ROOT").map(PathBuf::from).expect("Only the owning diagnostic starts this child.");
    assert!(root.starts_with(std::env::temp_dir()) && root.file_name().unwrap().to_string_lossy().starts_with("bowser-ext-live-"));
    let expected = std::env::var("BOWSER_COMPLETED_STATE_EXPECTED").unwrap(); let profile = root.join("state-profile");
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let records = extensions::read_authority_records(&profile.join("extensions.txt")).unwrap();
    assert!(records.iter().all(|record| !record.enabled), "completed-off authority must not become enabled in a new process");
    let known = records.iter().map(|record| (record.id.clone(), record.enabled)).collect(); let loopback = Loopback::start();
    let mut session = Session::open(&profile, vec![], known, &loopback.page("completed-restart-control"));
    assert!(pump_one_until(Duration::from_secs(30), || session.engine.probe_extension_profile_ready()));
    let native = session.native_inventory().unwrap();
    match expected.as_str() {
        "off" => { assert_eq!(records.len(), 1); assert!(native.iter().any(|(id, on)| id == KEY_FIXTURE_ID && !on)); }
        "absent" => { assert!(records.is_empty()); assert!(!native.iter().any(|(id, _)| id == KEY_FIXTURE_ID)); }
        _ => panic!("Unknown owning test state"),
    }
    assert!(pump_until(Duration::from_secs(15), || loopback.beacons.lock().unwrap().iter().any(|path| path == "/completed-restart-control")), "completed-state negative did not load its actual HTTP page");
    pump_until(Duration::from_secs(2), || false);
    assert_eq!(session.read_marker("data-bowser-probe").as_deref(), Some("null"));
    assert!(!loopback.beacons.lock().unwrap().iter().any(|path| path.starts_with(BEACON_PREFIX)), "completed revoked extension executed in the new process");
    println!("COMPLETED_RESTART_CHILD expected={expected} actual_native_inventory={native:?} record_count={} actual_page_ready=true content_marker_absent=true extension_beacons_absent=true", records.len());
    session.close(); drop(loopback);
}

#[test]
#[ignore = "real two-process stale native owner versus replacement signing identity"]
fn live_probe_stale_owner_replacement() {
    assert!(std::env::var_os("BOWSER_EXTENSION_TRACE").is_none(), "this control must exercise production identity checks without tracing");
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let root = temp_root(); let profile = root.join("replacement-profile");
    let (old_id, old_folder) = install_fixture(&profile, "probe-mv3-key");
    let loopback = Loopback::start();
    let mut owner = Session::open(&profile, vec![(old_id.clone(), old_folder.clone())], vec![(old_id.clone(), true)], &loopback.page("replacement-owner"));
    assert!(owner.wait_for_add(&profile, &old_id).1.is_none());
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "engine::extension_probe_live::live_runtime_stale_owner_child", "--ignored", "--nocapture", "--test-threads=1"])
        .env("BOWSER_STALE_OWNER_ROOT", &root).env("BOWSER_STALE_OWNER_ID", &old_id)
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    assert!(pump_until(Duration::from_secs(45), || root.join("stale-ready").is_file()));
    let answer = Rc::new(RefCell::new(None)); let sink = answer.clone();
    let records = profile.join("extensions.txt"); let dir = profile.join("Extensions"); let id = old_id.clone();
    owner.engine.remove_extension(&old_id, Box::new(move |native| *sink.borrow_mut() = Some(native.and_then(|()| extensions::uninstall_under_runtime(&dir, &records, &id).map_err(|e| e.to_string())))));
    assert!(pump_until(Duration::from_secs(30), || answer.borrow().is_some())); answer.borrow_mut().take().unwrap().unwrap();
    // Same authentic signing key, deliberately distinct Bowser record/folder. Never edit loaded code.
    let (new_id, new_folder) = install_fixture(&profile, "probe-mv3-key"); assert_ne!(old_id, new_id);
    let answer = Rc::new(RefCell::new(None)); let sink = answer.clone();
    owner.engine.set_extension_enabled(&new_id, &new_folder, true, Box::new(move |result| *sink.borrow_mut() = Some(result)));
    assert!(pump_until(Duration::from_secs(30), || answer.borrow().is_some())); answer.borrow_mut().take().unwrap().unwrap();
    assert!(owner.native_inventory().unwrap().iter().any(|(id, on)| id == KEY_FIXTURE_ID && *on));
    let before_records = std::fs::read(profile.join("extensions.txt")).unwrap(); let before_map = std::fs::read(profile.join("extension_ids.txt")).unwrap();
    std::fs::write(root.join("replacement-ready"), b"new-consented-record-now-owns-same-native-ID").unwrap();
    assert!(pump_until(Duration::from_secs(45), || root.join("stale-done").is_file()));
    let output = child.wait_with_output().unwrap(); println!("STALE_OWNER child_stdout={} child_stderr={}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)); assert!(output.status.success());
    assert_eq!(std::fs::read(profile.join("extensions.txt")).unwrap(), before_records); assert_eq!(std::fs::read(profile.join("extension_ids.txt")).unwrap(), before_map);
    assert!(new_folder.is_dir()); assert!(!old_folder.exists());
    let native = owner.native_inventory().unwrap(); assert!(native.iter().any(|(id, on)| id == KEY_FIXTURE_ID && *on));
    assert!(owner.wait_for_worker_reply(&loopback));
    println!("STALE_OWNER verdict=old-process-cannot-mutate-replacement-owner native_inventory={native:?} new_record={new_id} worker_content_reply=true records_and_map_unchanged=true trace_absent=true");
    owner.close(); drop(loopback); remove_with_retries(&root);
}

#[test]
#[ignore = "child retaining a genuinely loaded stale native extension object"]
fn live_runtime_stale_owner_child() {
    let root = std::env::var_os("BOWSER_STALE_OWNER_ROOT").map(PathBuf::from).expect("Only the owning diagnostic launches this child.");
    assert!(root.starts_with(std::env::temp_dir()) && root.file_name().unwrap().to_string_lossy().starts_with("bowser-ext-live-"));
    let id = std::env::var("BOWSER_STALE_OWNER_ID").unwrap(); let profile = root.join("replacement-profile"); let folder = extensions::folder_for(&profile.join("Extensions"), &id).unwrap();
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let loopback = Loopback::start(); let mut stale = Session::open(&profile, vec![(id.clone(), folder.clone())], vec![(id.clone(), true)], &loopback.page("stale-cache"));
    assert!(stale.wait_for_add(&profile, &id).1.is_none()); std::fs::write(root.join("stale-ready"), b"actual-native-object-retained").unwrap();
    assert!(pump_until(Duration::from_secs(45), || root.join("replacement-ready").is_file()));
    for operation in ["disable", "enable", "remove"] {
        let answer = Rc::new(RefCell::new(None)); let sink = answer.clone();
        if operation == "remove" { stale.engine.remove_extension(&id, Box::new(move |result| *sink.borrow_mut() = Some(result))); }
        else { stale.engine.set_extension_enabled(&id, &folder, operation == "enable", Box::new(move |result| *sink.borrow_mut() = Some(result))); }
        assert!(pump_until(Duration::from_secs(15), || answer.borrow().is_some())); let result = answer.borrow_mut().take().unwrap();
        assert!(result.is_err(), "stale {operation} unexpectedly accepted: {result:?}"); println!("STALE_OWNER_CHILD operation={operation} result={result:?}");
    }
    std::fs::write(root.join("stale-done"), b"all-stale-operations-rejected").unwrap(); stale.close(); drop(loopback);
}

#[test]
#[ignore = "damaged authority must refuse extension-enabled real environments before native startup"]
fn live_probe_fail_closed_environment() {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    for damage in ["malformed-map", "duplicate-map", "initialized-missing-map", "malformed-records"] {
        let root = temp_root(); let profile = root.join("damaged-profile"); let (id, folder) = install_fixture(&profile, "probe-mv3-key");
        let loopback = Loopback::start(); let mut initial = Session::open(&profile, vec![(id.clone(), folder.clone())], vec![(id.clone(), true)], &loopback.page("initial-consent"));
        assert!(initial.wait_for_add(&profile, &id).1.is_none()); assert!(initial.wait_for_worker_reply(&loopback));
        // Microsoft requires browser-process exit before reopening a folder with a changed
        // AreBrowserExtensionsEnabled value. Object drop alone is not that native boundary.
        let (environment, token, exited) = initial.engine.probe_browser_exit_observer().unwrap();
        initial.close(); drop(loopback);
        assert!(pump_until(Duration::from_secs(30), || exited.get()), "consented profile browser did not actually exit before changing environment options");
        unsafe { environment.remove_BrowserProcessExited(token).unwrap(); } drop(environment);
        let map = profile.join("extension_ids.txt"); let records = profile.join("extensions.txt");
        match damage {
            "malformed-map" => std::fs::write(&map, b"truncated-authority\n").unwrap(),
            "duplicate-map" => { let row = std::fs::read_to_string(&map).unwrap(); std::fs::write(&map, format!("{row}{row}")).unwrap(); }
            "initialized-missing-map" => { assert!(map.with_extension("initialized").is_file()); std::fs::remove_file(&map).unwrap(); }
            "malformed-records" => std::fs::write(&records, b"truncated-record\n").unwrap(),
            _ => unreachable!(),
        }
        let map_before = std::fs::read(&map).ok(); let records_before = std::fs::read(&records).unwrap();
        let loopback = Loopback::start(); let target = loopback.page("damaged-real-page");
        let mut refused = Session::open(&profile, vec![(id.clone(), folder.clone())], vec![(id.clone(), true)], &target);
        let page_loaded = pump_until(Duration::from_secs(15), || loopback.beacons.lock().unwrap().iter().any(|path| path == "/damaged-real-page"));
        println!("FAIL_CLOSED_ENV damage={damage} before_exclusion_assertions startup_state={:?} page_loaded={page_loaded} native_failures={:?}", refused.engine.probe_extension_startup_state(), refused.failures.borrow());
        assert!(page_loaded, "negative control did not load its actual HTTP page; native failures={:?}", refused.failures.borrow());
        pump_until(Duration::from_secs(2), || false);
        let state = refused.engine.probe_extension_startup_state(); assert!(!state.0 && state.1.is_some(), "damaged state was not refused before environment configuration: {state:?}");
        assert!(!refused.engine.probe_extension_profile_ready()); assert!(refused.engine.probe_loaded_extension_id(&id).is_none());
        assert_eq!(refused.read_marker("data-bowser-probe").as_deref(), Some("null"));
        assert!(!loopback.beacons.lock().unwrap().iter().any(|path| path.starts_with(BEACON_PREFIX)), "persisted native extension executed despite refused environment");
        assert_eq!(std::fs::read(&map).ok(), map_before); assert_eq!(std::fs::read(&records).unwrap(), records_before); assert!(folder.is_dir());
        println!("FAIL_CLOSED_ENV damage={damage} startup_state={state:?} actual_http_page_loaded=true content_marker_absent=true extension_beacons_absent=true authority_and_package_preserved=true scope=real-environment-not-parser-only");
        refused.close(); drop(loopback); remove_with_retries(&root);
    }
}

#[test]
#[ignore = "real independent profile and private-first controller exclusion"]
fn live_probe_profile_private_isolation() {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let root = temp_root(); let profile = root.join("ordinary-owner"); let (id, folder) = install_fixture(&profile, "probe-mv3-key");
    let loopback = Loopback::start(); let mut owner = Session::open(&profile, vec![(id.clone(), folder.clone())], vec![(id.clone(), true)], &loopback.page("owner-positive"));
    assert!(owner.wait_for_add(&profile, &id).1.is_none()); assert!(owner.wait_for_worker_reply(&loopback));
    let other_loopback = Loopback::start(); let mut other = Session::open(&root.join("different-profile"), vec![], vec![], &other_loopback.page("other-profile-negative"));
    assert!(pump_until(Duration::from_secs(15), || other_loopback.beacons.lock().unwrap().iter().any(|p| p == "/other-profile-negative"))); pump_until(Duration::from_secs(2), || false);
    assert_eq!(other.read_marker("data-bowser-probe").as_deref(), Some("null"));
    assert!(!other_loopback.beacons.lock().unwrap().iter().any(|p| p.starts_with(BEACON_PREFIX)));
    assert!(!other.native_inventory().unwrap().iter().any(|(native, _)| native == KEY_FIXTURE_ID));
    println!("PROFILE_ISOLATION owning_profile_worker_content=true different_profile_actual_page_loaded=true different_profile_native_absent=true different_profile_content_beacons_absent=true");
    other.close(); drop(other_loopback); owner.close(); drop(loopback);
    // Existing persisted enabled native code is the important private-first negative, not a fresh
    // empty environment. The ordinary controller is created only after the negative is observed.
    let private_loopback = Loopback::start();
    let mut private = Session::open_with_private_first(&profile, vec![(id.clone(), folder.clone())], vec![(id.clone(), true)], &private_loopback.page("private-first-negative"), true);
    assert!(pump_until(Duration::from_secs(20), || private_loopback.beacons.lock().unwrap().iter().any(|p| p == "/private-first-negative")), "private negative did not load its real page under the production privacy checks");
    pump_until(Duration::from_secs(2), || false); assert_eq!(private.read_marker("data-bowser-probe").as_deref(), Some("null"));
    assert!(!private.engine.probe_extension_profile_ready(), "private-first controller captured the extension profile");
    assert!(!private_loopback.beacons.lock().unwrap().iter().any(|p| p.starts_with(BEACON_PREFIX)));
    private.engine.select_tab(TabId(2)).unwrap(); private.engine.navigate_tab(TabId(2), &private_loopback.page("ordinary-after-private")).unwrap();
    assert!(pump_until(Duration::from_secs(30), || private.engine.probe_loaded_extension_id(&id).is_some()));
    assert!(private.engine.probe_extension_profile_ready());
    private.engine.select_tab(TabId(1)).unwrap(); private.engine.navigate_tab(TabId(1), &private_loopback.page("private-after-normal-negative")).unwrap();
    assert!(pump_until(Duration::from_secs(15), || private_loopback.beacons.lock().unwrap().iter().any(|p| p == "/private-after-normal-negative")), "second private negative did not navigate its actual new document");
    pump_until(Duration::from_secs(2), || false); assert_eq!(private.read_marker("data-bowser-probe").as_deref(), Some("null"));
    println!("PRIVATE_ISOLATION persisted_enabled_fixture=true private_first_actual_page_loaded=true private_profile_not_captured=true private_content_absent=true popup_privacy_gate_not_exercised=true later_ordinary_profile_captured=true private_after_ordinary_content_absent=true scope=profile-and-private-not-ui-context");
    private.close(); drop(private_loopback); remove_with_retries(&root);
}


#[test]
#[ignore = "child of the actual signed Stylus appearance diagnostic"]
fn live_real_stylus_restart_child() {
    let profile = std::env::var_os("BOWSER_STYLUS_RESTART_PROFILE").map(PathBuf::from).expect("Only the owning diagnostic starts this child.");
    assert!(profile.starts_with(std::env::temp_dir()) && profile.file_name().unwrap() == "stylus");
    let id = std::env::var("BOWSER_STYLUS_RESTART_ID").unwrap(); let target = Url::parse(&std::env::var("BOWSER_STYLUS_RESTART_URL").unwrap()).unwrap();
    assert_eq!(target.scheme(), "http"); assert_eq!(target.host_str(), Some("127.0.0.1"));
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let folder = extensions::folder_for(&profile.join("Extensions"), &id).unwrap();
    let mut session = Session::open(&profile, vec![(id.clone(), folder)], vec![(id.clone(), true)], &target);
    assert!(session.wait_for_add(&profile, &id).1.is_none());
    let mut styled = false;
    for _ in 0..6 {
        session.engine.navigate_tab(session.tab(), &target).unwrap(); pump_until(Duration::from_secs(1), || false);
        styled = session.browsing_script("getComputedStyle(document.body).backgroundColor").as_deref() == Some("\"rgb(7, 11, 19)\"");
        if styled { break; }
    }
    assert!(styled, "actual signed Stylus imported style did not execute after process restart");
    println!("STYLUS_RESTART actual_computed_colour=rgb(7,11,19) retained_imported_style=true runtime_id={:?}", session.engine.probe_loaded_extension_id(&id));
    session.close();
}


#[test]
#[ignore = "real production Settings ChromeHost profile and fresh native extension inventory"]
fn live_probe_ui_profile_isolation() {
    use crate::chrome_host::host::{ChromeEnv, ChromeHost, ChromeHostOptions, View, CHROME_PROFILE};
    use windows::core::Interface;
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let root = temp_root(); let profile = root.join("ui-isolation-owner"); let (id, folder) = install_fixture(&profile, "probe-mv3-key");
    let loopback = Loopback::start(); let mut owner = Session::open_configured(&profile, vec![(id.clone(), folder)], vec![(id.clone(), true)], &loopback.page("ui-owner-positive"), false, true);
    assert!(owner.wait_for_add(&profile, &id).1.is_none()); assert!(owner.wait_for_worker_reply(&loopback));
    assert!(owner.native_inventory().unwrap().iter().any(|(native, on)| native == KEY_FIXTURE_ID && *on));
    let env = ChromeEnv::new(owner.engine.chrome_environment().expect("production UI scheme environment"));
    let ready = Rc::new(std::cell::Cell::new(false)); let ready_sink = ready.clone();
    let failed = Rc::new(std::cell::Cell::new(false)); let failed_sink = failed.clone();
    let created = Rc::new(RefCell::new(None)); let sink = created.clone();
    ChromeHost::create(&env, ChromeHostOptions {
        parent: owner.host, bounds: RECT { left:0, top:0, right:800, bottom:600 }, view: View::Settings,
        on_command: Rc::new(move |command| { if matches!(command, crate::chrome_host::protocol::Command::Ready) { ready_sink.set(true); } }),
        on_failed: Rc::new(move || failed_sink.set(true)), on_accelerator: None,
    }, move |result| *sink.borrow_mut() = Some(result)).unwrap();
    assert!(pump_until(Duration::from_secs(30), || created.borrow().is_some() || failed.get())); assert!(!failed.get(), "genuine Settings host failed before creation");
    let ui = created.borrow_mut().take().unwrap().unwrap();
    assert!(pump_until(Duration::from_secs(30), || ready.get() || failed.get()) && ready.get() && !failed.get(), "actual Settings UI did not post its genuine ready message");
    let webview = ui.probe_native_webview();
    let profile = unsafe { webview.cast::<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_13>().unwrap().Profile().unwrap() };
    let mut name = windows::core::PWSTR::null(); let mut private = windows::core::BOOL::default();
    unsafe { profile.ProfileName(&mut name).unwrap(); profile.IsInPrivateModeEnabled(&mut private).unwrap(); }
    let name = webview2_com::CoTaskMemPWSTR::from(name).to_string(); assert_eq!(name, CHROME_PROFILE); assert!(private.as_bool());
    let mut source = windows::core::PWSTR::null(); unsafe { webview.Source(&mut source).unwrap(); }
    let source = webview2_com::CoTaskMemPWSTR::from(source).to_string(); assert_eq!(source, View::Settings.start_url());
    let inventory = Rc::new(RefCell::new(None)); let sink = inventory.clone();
    WebView2Engine::probe_webview_inventory(&webview, Box::new(move |result| *sink.borrow_mut() = Some(result))).unwrap();
    assert!(pump_until(Duration::from_secs(15), || inventory.borrow().is_some())); let inventory = inventory.borrow_mut().take().unwrap().unwrap();
    assert!(!inventory.iter().any(|(native, _)| native == KEY_FIXTURE_ID), "ordinary extension was present in the privileged UI profile: {inventory:?}");
    println!("UI_PROFILE_ISOLATION actual_settings_ready=true source={source} native_profile={name} in_private=true ordinary_fixture_enabled_and_worker_content=true ui_fresh_native_inventory={inventory:?} fixture_absent=true scope=profile-inventory-not-loopback-only-content-marker");
    ui.close(); drop(ui); drop(webview); drop(env); owner.close(); drop(loopback); remove_with_retries(&root);
}

/// Independent local fixture gates, not a claim about any marketplace package's functionality.
#[test]
#[ignore = "needs real WebView2 and ordinary native Windows menu input"]
fn live_probe_native_page_links_guards() {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let root = temp_root(); let profile = root.join("page-links"); let loopback = Loopback::start();
    let (id, folder) = install_fixture(&profile, "probe-mv3");
    let mut session = Session::open(&profile, vec![(id.clone(), folder.clone())], vec![(id.clone(), true)], &loopback.page("page-links"));
    let (_, failure) = session.wait_for_add(&profile, &id); assert!(failure.is_none(), "native fixture failed: {failure:?}");
    let result = Rc::new(RefCell::new(None)); let sink = result.clone();
    session.engine.open_extension_page(&id, &folder, extensions::PageKind::Popup, "Bowser disposable page-link controls", Box::new(move |answer| *sink.borrow_mut() = Some(answer)));
    assert!(pump_until(Duration::from_secs(30), || result.borrow().is_some())); result.borrow_mut().take().unwrap().unwrap();
    assert!(pump_until(Duration::from_secs(30), || session.page_script("!!document.querySelector('#send')").as_deref() == Some("true")));
    pump_until(Duration::from_secs(1), || false);
    let run_helper = |mode: &str, delay: &str, label: &str| {
        let out = std::env::var_os("RUNNER_TEMP").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join(format!("bowser-native-page-links-{mode}"));
        let mut child = std::process::Command::new("powershell.exe").args(["-NoProfile", "-File"])
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/scripts/probe-native-extension-link.ps1"))
            .args(["-OwnerPid", &std::process::id().to_string(), "-ClientX", "0", "-ClientY", "0", "-ViewportWidth", "0", "-ViewportHeight", "0", "-Mode", mode, "-SelectionDelayMs", delay, "-ExpectedLinkLabel", label])
            .arg("-Out").arg(out).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
        if !pump_until(Duration::from_secs(75), || child.try_wait().is_ok_and(|status| status.is_some())) { let _ = child.kill(); let _ = child.wait(); panic!("native page-link helper timed out"); }
        let output = child.wait_with_output().unwrap();
        println!("PAGE_LINKS_CONTROL mode={mode} exit={:?} stdout={} stderr={}", output.status.code(), String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        super::extension_links::assert_no_modal_timeout();
        assert!(output.status.success(), "real owned native helper failed");
    };
    assert_eq!(session.page_script("document.querySelectorAll('a[href]').length").as_deref(), Some("0"));
    run_helper("PageLinksEmpty", "0", "unused");
    assert!(super::extension_links::probe_results().2, "actual deferred no-links notice was not shown");
    session.page_script("(()=>{const a=document.createElement('a');a.href=chrome.runtime.getURL('options.html');a.textContent='Disposable options';a.id='old-document-only-link';document.body.append(a);return true})()");
    let original = session.page_script("location.href").unwrap();
    super::extension_links::arm_probe_reload();
    run_helper("PageLinks", "2000", "options.html");
    assert!(pump_until(Duration::from_secs(10), || session.page_script("document.querySelector('#old-document-only-link')===null").as_deref() == Some("true")));
    assert_eq!(session.page_script("location.href"), Some(original), "stale menu selection navigated after a real same-URL reload");
    let (reloaded, refused, _) = super::extension_links::probe_results(); assert!(reloaded && refused, "real native Reload and stale native selection refusal were not both observed");
    println!("PAGE_LINKS_CONTROL verdict=actual-empty-notice-dismissed-and-real-same-url-reload-stale-selection-refused");
    session.close(); drop(loopback); remove_with_retries(&root);
}

thread_local! { static COMMAND_ACCELERATOR_FORWARDED: std::cell::Cell<Option<(TabId, bool)>> = const { std::cell::Cell::new(None) }; }
/// Read-only observation at the actual production accelerator decision. Never changes handling.
pub(super) fn observe_command_accelerator(tab: TabId, forwarded: bool) {
    COMMAND_ACCELERATOR_FORWARDED.with(|value| value.set(Some((tab, forwarded))));
    println!("NATIVE_COMMAND_ACCELERATOR actual_ctrl_shift_x=true owned_tab={tab:?} bowser_forwarded={forwarded}");
}
fn command_field<'a>(value: &'a extensions::json::Value, key: &str) -> Option<&'a extensions::json::Value> {
    match value { extensions::json::Value::Object(fields) => extensions::json::get(fields, key), _ => None }
}
fn command_receipt(session: &mut Session, kind: &str) -> Option<extensions::json::Value> {
    let script = format!("(()=>{{document.documentElement.removeAttribute('data-command-receipt');chrome.runtime.sendMessage({{type:'{kind}'}},answer=>document.documentElement.setAttribute('data-command-receipt',JSON.stringify(answer)));return true}})()");
    session.page_script(&script);
    let mut value = None;
    pump_until(Duration::from_secs(10), || {
        value = session.page_script("JSON.parse(document.documentElement.getAttribute('data-command-receipt')||'null')").and_then(|text| extensions::json::parse(&text));
        value.as_ref().is_some_and(|value| matches!(value, extensions::json::Value::Object(_)))
    }); value.filter(|value| matches!(value, extensions::json::Value::Object(_)))
}
fn command_view(session: &mut Session, tab: TabId, script: &str) -> Option<String> {
    let slot = Rc::new(RefCell::new(None)); let sink = slot.clone();
    session.engine.run_reader_script(tab, script, Box::new(move |answer| *sink.borrow_mut() = Some(answer)));
    pump_until(Duration::from_secs(10), || slot.borrow().is_some()); slot.borrow_mut().take().flatten()
}
fn command_input(session: &mut Session, tab: TabId, chord: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow, ShowWindow, SW_SHOW};
    session.engine.select_tab(tab).unwrap(); session.engine.set_visible(true).unwrap();
    unsafe { let _ = ShowWindow(session.host, SW_SHOW); let _ = SetForegroundWindow(session.host); }
    session.engine.focus_page().unwrap(); pump_until(Duration::from_millis(500), || false);
    assert_eq!(unsafe { GetForegroundWindow() }, session.host);
    assert_eq!(command_view(session, tab, "document.hasFocus()").as_deref(), Some("true"));
    let mut child = std::process::Command::new("powershell.exe").args(["-NoProfile", "-File"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/scripts/probe-native-extension-command.ps1"))
        .args(["-OwnerPid", &std::process::id().to_string(), "-OwnerHandle", &(session.host.0 as isize).to_string(), "-Chord", chord])
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    if !pump_until(Duration::from_secs(20), || child.try_wait().is_ok_and(|status| status.is_some())) { let _ = child.kill(); let _ = child.wait(); panic!("owned command input helper timed out"); }
    let output = child.wait_with_output().unwrap();
    println!("NATIVE_COMMAND_INPUT result={:?} stdout={} stderr={}", output.status.code(), String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
}

/// Feasibility only: a missing native event is not a generic extension-API absence verdict.
#[test]
#[ignore = "needs live WebView2 and genuine OS keyboard input to disposable focused views"]
fn live_probe_native_named_command() { run_native_keyboard_probe(false, false); }

#[test]
#[ignore = "needs genuine reserved action keyboard input in disposable no-popup extension profiles"]
fn live_probe_native_reserved_action() { run_native_keyboard_probe(true, false); }

/// Exact native worker query used by Bitwarden, before injection, without event-ID fallback.
#[test]
#[ignore = "needs genuine OS keyboard and native worker WINDOW_ID_CURRENT targeting"]
fn live_probe_native_worker_current_query() { run_native_keyboard_probe(false, true); }

fn run_native_keyboard_probe(reserved_action: bool, worker_query: bool) {
    use extensions::json::Value;
    let assignment = if reserved_action { "_execute_action" } else { "bowser-native-probe" };
    let event_kind = if reserved_action { "action.onClicked" } else { "commands.onCommand" };
    let fixture_name = if worker_query { "probe-native-worker-query" } else if reserved_action { "probe-native-action" } else { "probe-native-commands" };
    println!("NATIVE_COMMAND scope=real-native-keyboard event_kind={event_kind} manifest_assignment={assignment} no_popup={reserved_action} exact_worker_query_before_injection={worker_query} observer_extension_context_present=true worker_query_observer_popup_present={worker_query}");
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
    let root = temp_root();
    for active_tab in [true, false] {
        let profile = root.join(if active_tab { "with-active-tab" } else { "without-active-tab" });
        let source = root.join(if active_tab { "source-with" } else { "source-without" }); std::fs::create_dir(&source).unwrap();
        for name in ["manifest.json", "worker.js", "popup.html", "popup.js"] { std::fs::copy(fixture(fixture_name).join(name), source.join(name)).unwrap(); }
        if !active_tab {
            let manifest = std::fs::read_to_string(source.join("manifest.json")).unwrap().replace(",\n    \"activeTab\"", "");
            assert!(!manifest.contains("\"activeTab\"")); std::fs::write(source.join("manifest.json"), manifest).unwrap();
        }
        let record = extensions::install(&source, &profile.join("Extensions"), &profile.join("extensions.txt")).unwrap();
        let folder = extensions::folder_for(&profile.join("Extensions"), &record.id).unwrap(); let loopback = Loopback::start();
        let mut session = Session::open(&profile, vec![(record.id.clone(), folder.clone())], vec![(record.id.clone(), true)], &loopback.page("command-duplicate"));
        let (native_id, failure) = session.wait_for_add(&profile, &record.id); assert!(failure.is_none(), "native command fixture load failed: {failure:?}");
        assert_eq!(native_id.as_deref(), Some(KEY_FIXTURE_ID));
        session.engine.select_tab(TabId(2)).unwrap(); session.engine.navigate_tab(TabId(2), &loopback.page("command-duplicate")).unwrap();
        let ready = format!("location.href==={}&&document.title==='Bowser probe page'&&document.readyState==='complete'", crate::chrome_host::protocol::json_string(loopback.page("command-duplicate").as_str()));
        for tab in [TabId(1), TabId(2)] { assert!(pump_until(Duration::from_secs(20), || command_view(&mut session,tab,&ready).as_deref()==Some("true")), "duplicate-URL controlled view was not actually ready: {tab:?}"); }
        session.open_page(&record.id, &folder, if reserved_action { extensions::PageKind::Options } else { extensions::PageKind::Popup }).unwrap();
        assert!(pump_until(Duration::from_secs(10), || session.page_script("document.documentElement.getAttribute('data-ready')").as_deref() == Some("\"yes\"")));
        let baseline = command_receipt(&mut session, "native-command-preflight").expect("genuine worker preflight receipt missing");
        println!("NATIVE_COMMAND phase=preflight active_tab={active_tab} receipt={baseline:?}");
        let assigned = matches!(command_field(&baseline, "commands"), Some(Value::Array(commands)) if commands.iter().any(|command| matches!(command_field(command, "name"), Some(Value::String(name)) if name==assignment) && matches!(command_field(command, "shortcut"), Some(Value::String(chord)) if chord=="Ctrl+Shift+X")));
        if !assigned { println!("NATIVE_COMMAND verdict=unavailable-assigned-native-chord-not-reported active_tab={active_tab}"); session.close(); drop(loopback); break; }
        for tab in [TabId(1), TabId(2)] {
            for role in ["primary", "negative"] { assert_eq!(command_view(&mut session, tab, &format!("document.documentElement.getAttribute('data-bowser-native-command-{role}')")).as_deref(), Some("null"), "pregesture injection changed a real browsing view"); }
        }
        let mut first_native_id = None; let mut first_primary_token = None; let mut second_native_id = None;
        for tab in [TabId(1), TabId(2)] {
            command_view(&mut session, tab, "document.addEventListener('keydown',e=>{if(e.isTrusted&&e.key==='F8')document.documentElement.setAttribute('data-native-f8','actual-os-key')});true");
            command_input(&mut session, tab, "F8");
            assert_eq!(command_view(&mut session, tab, "document.documentElement.getAttribute('data-native-f8')").as_deref(), Some("\"actual-os-key\""));
            let before = command_receipt(&mut session, "native-command-state").unwrap();
            let previous_events = match command_field(&before,"events") { Some(Value::Array(events)) => events.len(), _ => 0 };
            COMMAND_ACCELERATOR_FORWARDED.with(|value| value.set(None)); command_input(&mut session, tab, "CtrlShiftX");
            assert_eq!(COMMAND_ACCELERATOR_FORWARDED.with(std::cell::Cell::get), Some((tab, false)), "actual host accelerator observation did not establish unhandled command");
            let mut receipt = None;
            let received = pump_until(Duration::from_secs(8), || {
                receipt = command_receipt(&mut session, "native-command-state");
                matches!(receipt.as_ref().and_then(|r| command_field(r, "events")), Some(Value::Array(events)) if events.len() > previous_events && events.last().is_some_and(|event| (worker_query || command_field(event,"injection").is_some()) && command_field(event,"complete")==Some(&Value::Bool(true))))
            });
            let receipt = receipt.expect("genuine worker state missing"); println!("NATIVE_COMMAND active_tab={active_tab} focused_owned_view={tab:?} receipt={receipt:?}");
            if !received {
                assert!(!matches!(command_field(&receipt,"events"),Some(Value::Array(events)) if events.len()>previous_events), "native event arrived but injection/query/negative completion timed out; not a delivery-absence verdict");
                println!("NATIVE_COMMAND verdict=no-new-native-{event_kind}-on-tested-runtime assignment_and_focus_and_os_delivery_and_unhandled_host_proven=true focused_owned_view={tab:?}"); session.close(); drop(loopback); remove_with_retries(&root); return;
            }
            let Some(Value::Array(events)) = command_field(&receipt, "events") else { panic!("worker event array missing"); };
            let Some(event) = events.last() else { println!("NATIVE_COMMAND verdict=no-native-{event_kind}-on-tested-runtime assignment_and_focus_and_os_delivery_and_unhandled_host_proven=true"); session.close(); drop(loopback); remove_with_retries(&root); return; };
            if reserved_action {
                assert_eq!(command_field(event,"eventKind"),Some(&Value::String("action.onClicked".to_owned())), "no genuine native action event receipt");
                // Manifest metadata, deliberately not represented as a native onClicked command name.
                assert_eq!(command_field(event,"manifestAssignment"),Some(&Value::String(assignment.to_owned())));
                assert!(matches!(command_field(&receipt,"unexpectedCommandEvents"),Some(Value::Array(events)) if events.is_empty()), "reserved action unexpectedly delivered commands.onCommand");
            } else {
                assert_eq!(command_field(event,"command"),Some(&Value::String(assignment.to_owned())), "native event did not name the declared command");
            }
            assert!(command_field(event,"error").is_none(), "native command boundary probes did not all settle successfully");
            assert!(matches!(command_field(event,"otherTabAttempts"),Some(Value::Array(_))), "other-view attempts did not finish");
            let native = command_field(event, "nativeTab");
            let Some(Value::Number(native_id)) = native.and_then(|tab| command_field(tab,"id")) else { println!("NATIVE_COMMAND verdict=native-event-has-no-authoritative-tab-target"); session.close(); drop(loopback); remove_with_retries(&root); return; };
            let event_id = Value::Number(native_id.clone());
            if worker_query {
                assert_eq!(command_field(event,"operationOrder"),Some(&Value::Array(vec![Value::String("query-started".to_owned()),Value::String("query-settled".to_owned()),Value::String("primary-attempt".to_owned())])),"query did not settle before the primary attempt");
                assert_eq!(command_field(event,"workerQueryComplete"),Some(&Value::Bool(true)),"worker query did not complete before primary injection");
                assert!(command_field(event,"workerQueryError").is_none(),"exact native query failed");
                let query = command_field(event,"workerQuery").expect("pre-injection exact worker query missing");
                assert_eq!(command_field(query,"active"),Some(&Value::Bool(true)));
                assert_eq!(command_field(query,"windowId"),Some(&Value::Number("-2".to_owned())), "fixture did not use native WINDOW_ID_CURRENT");
                let Some(Value::Array(queried)) = command_field(event,"workerQueryTabs") else { panic!("worker query result missing"); };
                assert_eq!(queried.len(),1,"exact pre-injection worker query did not return one native target");
                assert_eq!(command_field(event,"queryNativeId"),Some(&event_id),"retained target is not the query-selected native ID");
                assert_eq!(command_field(&queried[0],"id"),Some(&event_id),"exact worker query selected a different tab than the genuine focused-view command event; no fallback permitted");
            }
            if tab == TabId(1) { first_native_id = Some(event_id.clone()); }
            else { assert_ne!(first_native_id.as_ref(),Some(&event_id), "two different focused COM-owned views received the same native event tab identity"); second_native_id = Some(event_id.clone()); }
            assert_eq!(command_field(event,"currentNativeTab").and_then(|tab| command_field(tab,"id")),Some(&event_id), "native event target no longer exists");
            assert!(matches!(command_field(&baseline,"preflight"),Some(Value::Array(attempts)) if attempts.iter().any(|attempt| command_field(attempt,"id")==Some(&event_id) && command_field(attempt,"allowed")==Some(&Value::Bool(false)) && command_field(attempt,"permissionDenied")==Some(&Value::Bool(true)))), "later native event target lacks actual pre-gesture permission-denial evidence");
            if !active_tab {
                let injection = command_field(event,"injection").unwrap();
                assert_eq!(command_field(injection,"id"),Some(&event_id)); assert_eq!(command_field(injection,"allowed"),Some(&Value::Bool(false)));
                assert_eq!(command_field(injection,"permissionDenied"),Some(&Value::Bool(true)), "no-activeTab command failed for a reason other than native permission denial");
            }
            let marker = command_view(&mut session, tab, "document.documentElement.getAttribute('data-bowser-native-command-primary')");
            if active_tab {
                let injection = command_field(event,"injection").unwrap();
                assert_eq!(command_field(injection,"id"),Some(&event_id)); assert_eq!(command_field(injection,"allowed"),Some(&Value::Bool(true)), "event-target primary injection did not succeed");
                let Some(Value::String(token)) = command_field(event,"primaryToken") else { panic!("per-event primary token missing"); };
                if tab == TabId(1) { first_primary_token = Some(token.clone()); }
                else { assert_ne!(first_primary_token.as_deref(),Some(token.as_str()), "second real invocation reused its primary attribution token"); }
                let expected = crate::chrome_host::protocol::json_string(token);
                assert_eq!(marker.as_deref(),Some(expected.as_str()), "event-target injection did not affect the exact focused COM-owned view");
            }
            else { assert_eq!(marker.as_deref(), Some("null"), "command without activeTab unexpectedly injected"); }
            // The other real view is read by its COM owner, not identified by URL/native-ID guessing.
            let other = if tab == TabId(1) { TabId(2) } else { TabId(1) };
            if tab == TabId(1) {
                assert_eq!(command_view(&mut session, other, "document.documentElement.getAttribute('data-bowser-native-command-primary')").as_deref(), Some("null"));
                assert_eq!(command_view(&mut session, other, "document.documentElement.getAttribute('data-bowser-native-command-negative')").as_deref(), Some("null"), "late settled negative attempt affected the ungranted other view");
            }
        }
        if !active_tab {
            for tab in [TabId(1),TabId(2)] {
                for role in ["primary","negative"] { assert_eq!(command_view(&mut session,tab,&format!("document.documentElement.getAttribute('data-bowser-native-command-{role}')")).as_deref(),Some("null"), "settled no-activeTab attempts affected a controlled view"); }
            }
        }
        if active_tab {
            let mut cross = loopback.page("command-cross-origin"); cross.set_host(Some("localhost")).unwrap();
            session.engine.select_tab(TabId(2)).unwrap(); session.engine.navigate_tab(TabId(2), &cross).unwrap();
            assert!(pump_until(Duration::from_secs(15), || loopback.beacons.lock().unwrap().iter().any(|path| path=="/command-cross-origin")));
            let ready = format!("location.origin==={}&&document.title==='Bowser probe page'&&document.readyState==='complete'", crate::chrome_host::protocol::json_string(cross.origin().ascii_serialization().as_str()));
            assert!(pump_until(Duration::from_secs(10), || command_view(&mut session,TabId(2),&ready).as_deref()==Some("true")), "cross-origin revocation negative did not load its actual new document");
            let receipt = command_receipt(&mut session, "native-command-again").unwrap(); println!("NATIVE_COMMAND phase=cross-origin-no-new-gesture receipt={receipt:?}");
            let again = command_field(&receipt,"again").unwrap();
            assert_eq!(command_field(again,"id"),second_native_id.as_ref(), "cross-origin retry did not use the retained second native event target");
            assert_eq!(command_field(again,"currentNativeTab").and_then(|tab| command_field(tab,"id")),second_native_id.as_ref(), "retained native tab was closed/replaced rather than permission-revoked");
            assert_eq!(command_field(again,"allowed"),Some(&Value::Bool(false)));
            assert_eq!(command_field(again,"permissionDenied"),Some(&Value::Bool(true)), "cross-origin retry failed for a reason other than native permission denial");
            assert_eq!(command_view(&mut session,TabId(2),"document.documentElement.getAttribute('data-bowser-native-command-primary')").as_deref(),Some("null"));
            assert_eq!(command_view(&mut session,TabId(2),"document.documentElement.getAttribute('data-bowser-native-command-negative')").as_deref(),Some("null"), "retained-ID negative injection affected the new cross-origin document");
        }
        println!("NATIVE_COMMAND verdict=real-native-keyboard-boundary-controls-passed event_kind={event_kind} manifest_assignment={assignment} active_tab={active_tab}");
        session.close(); drop(loopback);
    }
    remove_with_retries(&root);
}


#[test]
#[ignore = "requires native WebView2 and a disposable profile"]
fn live_successful_document_gate() {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap(); }
    let root = temp_root().join("native-document-gate"); std::fs::create_dir_all(&root).unwrap();
    let loopback = Loopback::start(); let first = loopback.page("document-gate-first");
    let mut session = Session::open(&root, vec![], vec![], &first);
    let tab = session.tab();
    assert!(pump_until(Duration::from_secs(30), || session.engine.successful_document_url(tab).as_ref()==Some(&first)));
    let webview = session.engine.probe_document_webview(tab).unwrap();
    let starts = Rc::new(RefCell::new(Vec::<(u64,String)>::new()));
    let completions = Rc::new(RefCell::new(Vec::<(u64,bool)>::new()));
    let start_sink = starts.clone(); let complete_sink = completions.clone();
    let cancelled = Rc::new(std::cell::Cell::new(false)); let cancel_sink = cancelled.clone();
    let mut start_token=0;let mut complete_token=0;
    unsafe {
        webview.add_NavigationStarting(&webview2_com::NavigationStartingEventHandler::create(Box::new(move |_,args| {
            if let Some(args)=args {
                let mut id=0;args.NavigationId(&mut id)?;
                let mut raw=windows::core::PWSTR::null();args.Uri(&mut raw)?;
                let uri=webview2_com::CoTaskMemPWSTR::from(raw).to_string();
                start_sink.borrow_mut().push((id,uri.clone()));
                if uri.ends_with("/document-gate-cancel") {args.SetCancel(true)?;cancel_sink.set(true);}
            } Ok(())
        })), &mut start_token).unwrap();
        webview.add_NavigationCompleted(&webview2_com::NavigationCompletedEventHandler::create(Box::new(move |_,args| {
            if let Some(args)=args {let mut id=0;let mut success=windows::core::BOOL::default();args.NavigationId(&mut id)?;args.IsSuccess(&mut success)?;complete_sink.borrow_mut().push((id,success.as_bool()));}Ok(())
        })), &mut complete_token).unwrap();
    }
    let listener=TcpListener::bind("127.0.0.1:0").unwrap();let port=listener.local_addr().unwrap().port();drop(listener);
    let failing=Url::parse(&format!("http://127.0.0.1:{port}/document-gate-failure")).unwrap();
    let before=completions.borrow().len();session.engine.navigate_tab(tab,&failing).unwrap();
    assert!(session.engine.successful_document_url(tab).is_none(),"request must revoke before any completion");
    assert!(pump_until(Duration::from_secs(30),||completions.borrow().len()>before));
    assert!(!completions.borrow().last().unwrap().1,"actual failed navigation must be observed");
    assert!(session.engine.successful_document_url(tab).is_none());
    session.engine.navigate_tab(tab,&first).unwrap();
    assert!(pump_until(Duration::from_secs(30),||session.engine.successful_document_url(tab).as_ref()==Some(&first)));
    let before=completions.borrow().len();session.engine.navigate_tab(tab,&loopback.page("document-gate-cancel")).unwrap();
    assert!(session.engine.successful_document_url(tab).is_none());
    assert!(pump_until(Duration::from_secs(30),||cancelled.get()&&completions.borrow().len()>before));
    assert!(!completions.borrow().last().unwrap().1);
    assert!(session.engine.successful_document_url(tab).is_none());
    let slow=loopback.page("document-gate-slow");let latest=loopback.page("document-gate-latest");
    session.engine.navigate_tab(tab,&slow).unwrap();
    assert!(pump_until(Duration::from_secs(10),||starts.borrow().iter().any(|(_,uri)|uri==slow.as_str())));
    session.engine.navigate_tab(tab,&latest).unwrap();assert!(session.engine.successful_document_url(tab).is_none());
    assert!(pump_until(Duration::from_secs(30),||session.engine.successful_document_url(tab).as_ref()==Some(&latest)));
    let starts=starts.borrow();let a=starts.iter().find(|(_,uri)|uri==slow.as_str()).unwrap().0;let b=starts.iter().find(|(_,uri)|uri==latest.as_str()).unwrap().0;
    assert_ne!(a,b);assert!(completions.borrow().contains(&(b,true)));
    drop(starts);
    session.browsing_script("history.pushState({},'', '/document-gate-same-document');true").unwrap();
    assert!(pump_until(Duration::from_secs(10),||session.engine.successful_document_url(tab).as_ref()==Some(&loopback.page("document-gate-same-document"))));
    session.engine.stop().unwrap();assert!(session.engine.successful_document_url(tab).is_none());
    session.engine.close().unwrap();assert!(session.engine.successful_document_url(tab).is_none());
    println!("NATIVE_DOCUMENT_GATE actual_failed=true actual_cancelled=true overlapping_native_ids_differ=true latest_success=true same_document_source=true stop_closed_ineligible=true");
    session.close();
}
