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
//! The windows are popups that are never shown, so nothing appears on the desktop. Every WebView2
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
    let (status, content_type, body) = if path.starts_with(BEACON_PREFIX) {
        beacons.lock().unwrap().push(path);
        ("200 OK", "text/plain", String::new())
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
        for attempt in 1..=4 {
            let created: Rc<RefCell<Option<windows::core::Result<WebView2Engine>>>> =
                Rc::new(RefCell::new(None));
            let slot = created.clone();
            let extras = EngineExtras {
                browser_extensions: true,
                startup_extensions: startup.clone(),
                known_extensions: known.clone(),
                extension_ids_file: Some(ids_file.clone()),
                ..EngineExtras::default()
            };
            WebView2Engine::begin_create_with_extras(
                host,
                bounds,
                &webview_dir,
                None,
                EngineEvents::default(),
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
        engine.select_tab(tab).unwrap();
        engine.navigate_tab(tab, first).unwrap();
        Self { engine, host }
    }

    fn tab(&self) -> TabId {
        TabId(1)
    }

    /// Waits for the add to finish (a mapping or a recorded failure) and returns what it left.
    fn wait_for_add(&self, profile: &Path, bowser_id: &str) -> (Option<String>, Option<String>) {
        let ids_file = profile.join("extension_ids.txt");
        pump_until(Duration::from_secs(60), || {
            mapping_for(&ids_file, bowser_id).is_some()
                || extensions::load_failure(bowser_id).is_some()
        });
        (
            mapping_for(&ids_file, bowser_id),
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
