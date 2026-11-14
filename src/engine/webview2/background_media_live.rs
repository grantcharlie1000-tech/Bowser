//! Bounded native policy/engine probe. Hidden host + muted local WAV, not Spotify, audible
//! device output, or the App's settings UI/timer. No existing profile or foreground input.
use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, atomic::{AtomicBool, Ordering}},
    thread::JoinHandle,
};
use windows::Win32::{
    System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
    UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP},
};
use crate::engine::EngineExtras;

fn pump(seconds: u64, ready: impl FnMut() -> bool) -> bool {
    crate::engine::native_probe_pump::until(Duration::from_secs(seconds), ready)
}

/// Owns only this test's engine, hidden window, loopback server and fresh temp profile.
struct Probe {
    root: PathBuf,
    host: HWND,
    engine: Option<WebView2Engine>,
    stop: Arc<AtomicBool>,
    server: Option<JoinHandle<()>>,
    com: bool,
}
impl Probe {
    fn new() -> (Self, u16) {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("bowser-muted-media-{}-{nonce}", std::process::id()));
        assert!(!root.exists(), "fresh owned profile required");
        std::fs::create_dir_all(&root).unwrap();
        let mut probe = Self { root, host: HWND::default(), engine: None, stop: Arc::new(AtomicBool::new(false)), server: None, com: false };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = probe.stop.clone();
        probe.server = Some(std::thread::spawn(move || serve(listener, stop)));
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok().unwrap();
        probe.com = true;
        probe.host = unsafe {
            CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!("Bowser muted media probe"), WS_POPUP, 0, 0, 800, 600, None, None, None, None)
        }.unwrap();
        let created = Rc::new(RefCell::new(None));
        let sink = created.clone();
        WebView2Engine::begin_create_with_extras(
            probe.host, RECT { left: 0, top: 0, right: 800, bottom: 600 },
            &probe.root.join("WebView2"), None, EngineEvents::default(), None,
            EngineExtras::default(), move |result| *sink.borrow_mut() = Some(result),
        ).unwrap();
        assert!(pump(60, || created.borrow().is_some()), "engine creation deadline");
        probe.engine = Some(created.borrow_mut().take().unwrap().expect("native engine"));
        (probe, port)
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        if let Some(mut engine) = self.engine.take() { let _ = engine.close(); }
        if self.host != HWND::default() { unsafe { let _ = DestroyWindow(self.host); } }
        self.stop.store(true, Ordering::Relaxed);
        if let Some(server) = self.server.take() { let _ = server.join(); }
        if self.com { unsafe { CoUninitialize(); } }
        for _ in 0..50 {
            if std::fs::remove_dir_all(&self.root).is_ok() || !self.root.exists() { return; }
            std::thread::sleep(Duration::from_millis(100));
        }
        eprintln!("MEDIA_PROBE cleanup: owned temporary profile still locked; no user profile was used");
    }
}

fn wav() -> Vec<u8> {
    let samples: u32 = 8000 * 180;
    let bytes = samples * 2;
    let mut wave = Vec::new();
    wave.extend(b"RIFF"); wave.extend((36 + bytes).to_le_bytes()); wave.extend(b"WAVEfmt ");
    wave.extend(16u32.to_le_bytes()); wave.extend(1u16.to_le_bytes()); wave.extend(1u16.to_le_bytes());
    wave.extend(8000u32.to_le_bytes()); wave.extend(16000u32.to_le_bytes());
    wave.extend(2u16.to_le_bytes()); wave.extend(16u16.to_le_bytes());
    wave.extend(b"data"); wave.extend(bytes.to_le_bytes());
    for i in 0..samples { let sample: i16 = if i % 40 < 20 { 400 } else { -400 }; wave.extend(sample.to_le_bytes()); }
    wave
}
fn serve(listener: TcpListener, stop: Arc<AtomicBool>) {
    let wave = wav();
    while !stop.load(Ordering::Relaxed) {
        let Ok((mut stream, _)) = listener.accept() else { std::thread::sleep(Duration::from_millis(10)); continue; };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
        let mut request = [0u8; 8192];
        let count = stream.read(&mut request).unwrap_or(0);
        let request = String::from_utf8_lossy(&request[..count]);
        let page = if request.starts_with("GET /media ") {
            b"<!doctype html><title>media</title><audio id=media muted preload=auto src='/tone.wav'></audio>".as_slice()
        } else { b"<!doctype html><title>idle</title><p>Idle control</p>".as_slice() };
        let (kind, body) = if request.starts_with("GET /tone.wav ") { ("audio/wav", wave.as_slice()) } else { ("text/html", page) };
        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
        let _ = stream.write_all(body);
    }
}
fn script(engine: &mut WebView2Engine, id: TabId, code: &str) -> String {
    let slot = Rc::new(RefCell::new(None)); let sink = slot.clone();
    engine.run_page_script(id, code, 4096, Box::new(move |value| *sink.borrow_mut() = Some(value)));
    assert!(pump(15, || slot.borrow().is_some()), "script deadline");
    let result = slot.borrow_mut().take().flatten().expect("script response"); result
}
fn suspended(engine: &WebView2Engine, id: TabId) -> bool {
    let view = engine.core.borrow().tabs[&id].webview.clone().unwrap();
    let api = view.cast::<ICoreWebView2_3>().unwrap();
    let mut value = BOOL::default(); unsafe { api.IsSuspended(&mut value) }.unwrap(); value.as_bool()
}
fn load(engine: &mut WebView2Engine, id: TabId, url: &str) {
    engine.select_tab(id).unwrap();
    engine.navigate_tab(id, &Url::parse(url).unwrap()).unwrap();
    assert!(pump(30, || engine.core.borrow().tabs.get(&id).is_some_and(|tab| tab.document_gate.borrow().proof().is_some())), "document load deadline");
}
fn field<'a>(object: &'a str, key: &str) -> Option<&'a str> {
    let members = crate::blocker::json::whole_object(object)?;
    crate::blocker::json::field(&members, key)
}
fn media_state(engine: &mut WebView2Engine, id: TabId) -> String {
    script(engine, id, "(()=>{const m=document.querySelector('audio');return {time:m.currentTime,paused:m.paused,ended:m.ended,muted:m.muted,ready:m.readyState,hidden:document.hidden}})()")
}
fn start_muted_fixture(engine: &mut WebView2Engine, id: TabId) {
    // Test-only user gesture for our local fixture. The production capture-only CDP adapter
    // deliberately refuses Runtime.evaluate, so call the native test handle directly.
    let view = engine.core.borrow().tabs[&id].webview.clone().unwrap();
    let slot = Rc::new(RefCell::new(None)); let sink = slot.clone();
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |status, result| {
        *sink.borrow_mut() = Some((status.is_ok(), result)); Ok(())
    }));
    let params = HSTRING::from(r#"{"expression":"document.querySelector('audio').play().then(()=>true)","awaitPromise":true,"returnByValue":true,"userGesture":true}"#);
    unsafe { view.CallDevToolsProtocolMethod(w!("Runtime.evaluate"), PCWSTR(params.as_ptr()), &handler) }.unwrap();
    assert!(pump(20, || slot.borrow().is_some()), "muted fixture play deadline");
    let (ok, answer) = slot.borrow_mut().take().unwrap();
    assert!(ok && !answer.contains("exceptionDetails"), "muted fixture play failed: {answer}");
    let result = field(&answer, "result").unwrap();
    assert_eq!(field(result, "value"), Some("true"));
}

#[test]
#[ignore = "requires native WebView2; muted local fixture, not Spotify or foreground App UI"]
fn windows_media_hidden_sweep_sleep_off_and_on() {
    let runtime = WebView2Engine::availability().expect("WebView2 runtime required, never silently skip");
    let source = std::env::var("BOWSER_MEDIA_PROBE_SOURCE").unwrap_or_else(|_| "unreported-local-source".into());
    println!("MEDIA_PROBE source={source} runtime={runtime} scope=policy-plus-engine-not-App-UI muted=true hidden_host=true");
    let (mut probe, port) = Probe::new();
    let engine = probe.engine.as_mut().unwrap();
    for sleep_enabled in [false, true] {
        let policy = crate::efficiency::background_policy(true, sleep_enabled, 300);
        assert_eq!(policy.suspend_after, Duration::from_secs(10));
        assert_eq!(policy.sleep_after.is_some(), sleep_enabled);
        let media = TabId(if sleep_enabled { 11 } else { 1 });
        let idle = TabId(media.0 + 1); let front = TabId(media.0 + 2);
        load(engine, media, &format!("http://127.0.0.1:{port}/media"));
        assert_eq!(script(engine, media, "document.querySelector('audio').muted"), "true");
        start_muted_fixture(engine, media);
        pump(2, || false);
        let before: f64 = script(engine, media, "document.querySelector('audio').currentTime").parse().unwrap();
        assert!(before > 0.2, "fixture must actually start before hiding");
        println!("MEDIA_DIAGNOSTIC sleep_enabled={sleep_enabled} stage=before_hide state={}", media_state(engine, media));
        load(engine, idle, &format!("http://localhost:{port}/idle"));
        // A genuinely blank new tab keeps controller creation lazy. Neither control shares
        // a renderer with a visible document that could itself prevent TrySuspend.
        engine.select_tab(front).unwrap();
        assert!(!engine.core.borrow().tabs[&media].visible.get());
        let hidden = Instant::now(); pump(12, || false);
        let pre_sweep_native = suspended(engine, media);
        let pre_sweep_state = media_state(engine, media);
        let dom_blocks = script(engine, media, SLEEP_BLOCK_SCRIPT);
        let native_audio_blocks = {
            let view = engine.core.borrow().tabs[&media].webview.clone().unwrap();
            webview_audio_blocks_sleep(&view)
        };
        println!("MEDIA_DIAGNOSTIC sleep_enabled={sleep_enabled} stage=before_sweep native_suspended={pre_sweep_native} native_audio_blocks={native_audio_blocks} dom_blocks={dom_blocks} state={pre_sweep_state}");
        engine.suspend_hidden_tabs(policy.suspend_after.as_millis() as u64).unwrap();
        assert!(pump(15, || {
            let state = engine.core.borrow();
            state.suspended.contains(&idle) && state.suspend_checks.get(&media).is_none_or(|pending| !pending.get())
        }), "sweep must finish the media check and confirm the silent control suspended");
        // Check native state before any script can wake a view, then prove continued
        // progression after the completed sweep (not just playback before it ran).
        assert!(suspended(engine, idle), "positive control proves sweep is not a no-op");
        let first_media_suspended = suspended(engine, media);
        let engine_marked_media = engine.core.borrow().suspended.contains(&media);
        println!("MEDIA_DIAGNOSTIC sleep_enabled={sleep_enabled} stage=after_sweep native_suspended={first_media_suspended} engine_marked={engine_marked_media} state={}", media_state(engine, media));
        assert!(!first_media_suspended, "media cannot suspend during its check");
        let at_sweep: f64 = script(engine, media, "document.querySelector('audio').currentTime").parse().unwrap();
        pump(2, || false);
        let media_suspended = suspended(engine, media); let idle_suspended = suspended(engine, idle);
        assert!(idle_suspended, "idle control must remain suspended without observation scripts");
        assert!(!media_suspended, "playing muted media must remain unsuspended after late callbacks");
        let after: f64 = script(engine, media, "document.querySelector('audio').currentTime").parse().unwrap();
        let state = script(engine, media, "(()=>{const m=document.querySelector('audio');return {paused:m.paused,ended:m.ended,muted:m.muted}})()");
        println!("MEDIA_PROBE sleep_enabled={sleep_enabled} suspend_after_ms={} elapsed_hidden_ms={} before={before} at_sweep={at_sweep} after={after} media_suspended={media_suspended} idle_suspended={idle_suspended} media_state={state}", policy.suspend_after.as_millis(), hidden.elapsed().as_millis());
        assert_eq!(field(&state, "paused"), Some("false"));
        assert_eq!(field(&state, "ended"), Some("false"));
        assert_eq!(field(&state, "muted"), Some("true"));
        assert!(after - before > 10.0, "hidden playback must progress beyond ten seconds");
        assert!(after - at_sweep > 1.0, "playback must also progress after the completed sweep");
        engine.select_tab(media).unwrap(); assert!(!suspended(engine, media));
        engine.close_tab(media).unwrap(); engine.close_tab(idle).unwrap(); engine.close_tab(front).unwrap();
    }
    println!("MEDIA_PROBE passed_muted_policy_engine");
}
