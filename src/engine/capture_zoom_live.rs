//! Live capture probe: a full-page capture of a page wider and taller than the window, at 150 %
//! page zoom, must hold the page's right and bottom edges.
//!
//! Ignored by default because it needs the WebView2 runtime and starts browser processes. Run it by
//! hand; the WebView2 profile lives in a fresh temp directory that is removed afterwards:
//!
//!   cargo test --lib live_full_page_capture_at_zoom -- --ignored --nocapture --test-threads=1

use std::{
    cell::RefCell,
    io::{Read, Write},
    net::TcpListener,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use url::Url;
use windows::{
    Win32::{
        Foundation::RECT,
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
            TranslateMessage, WINDOW_EX_STYLE, WS_POPUP,
        },
    },
    core::w,
};

use super::{EngineAdapter, EngineEvents, EngineExtras, WebView2Engine};
use crate::{capture, page_tools::CaptureFormat, tabs::TabId};

/// The page: 1200 x 2400 CSS px, white, a red band down the right edge and a blue band along the
/// bottom. The window is 800 x 600, so the page overflows both ways at any zoom.
const PAGE: &str = "<!doctype html><title>capture probe</title><style>html,body{margin:0;background:#fff}\
#p{position:relative;width:1200px;height:2400px}\
#r{position:absolute;left:1160px;top:0;width:40px;height:2400px;background:#f00}\
#b{position:absolute;left:0;top:2360px;width:1160px;height:40px;background:#00f}\
#s{position:absolute;left:600px;top:1200px;font:20px/40px monospace;color:#0a0;background:#0a0}\
#s::selection{color:#0a0;background:#0a0}</style>\
<div id=p><div id=r></div><div id=b></div><span id=s>selected text</span></div>";

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

fn serve(listener: TcpListener, stop: Arc<AtomicBool>) {
    listener.set_nonblocking(true).unwrap();
    while !stop.load(Ordering::Relaxed) {
        let Ok((mut stream, _)) = listener.accept() else {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut buffer = [0u8; 4096];
        let _ = stream.read(&mut buffer);
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{PAGE}",
            PAGE.len()
        );
    }
}

fn call(engine: &mut WebView2Engine, tab: TabId, method: &str, params: &str) -> Option<String> {
    let slot = Rc::new(RefCell::new(None));
    let sink = slot.clone();
    engine.devtools_call(tab, method, params, Box::new(move |answer| *sink.borrow_mut() = Some(answer)));
    pump_until(Duration::from_secs(30), || slot.borrow().is_some());
    slot.borrow_mut().take().flatten()
}

fn script(engine: &mut WebView2Engine, tab: TabId, text: &str) -> Option<String> {
    let slot = Rc::new(RefCell::new(None));
    let sink = slot.clone();
    engine.run_page_script(tab, text, 4096, Box::new(move |answer| *sink.borrow_mut() = Some(answer)));
    pump_until(Duration::from_secs(10), || slot.borrow().is_some());
    slot.borrow_mut().take().flatten()
}

/// The colour (r, g, b) at `x`, `y` (from the top) of a PNG.
fn pixel(png: &[u8], x: u32, y: u32) -> (u8, u8, u8) {
    let (width, height) = capture::size_of(png).unwrap();
    let dib = capture::dib_of(png).unwrap();
    let stride = (width * 3).div_ceil(4) * 4;
    let at = 40 + ((height - 1 - y) * stride + x * 3) as usize;
    (dib[at + 2], dib[at + 1], dib[at])
}

#[test]
#[ignore = "needs a live WebView2 runtime; run by hand"]
fn live_full_page_capture_at_zoom() {
    let root = std::env::temp_dir().join(format!("bowser-capture-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let server = {
        let stop = stop.clone();
        std::thread::spawn(move || serve(listener, stop))
    };
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok().unwrap();
    let host = unsafe {
        CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!("bowser-capture-probe"), WS_POPUP, 0, 0, 800, 600, None, None, None, None)
    }
    .unwrap();
    let bounds = RECT { left: 0, top: 0, right: 800, bottom: 600 };
    let created: Rc<RefCell<Option<windows::core::Result<WebView2Engine>>>> = Rc::new(RefCell::new(None));
    let slot = created.clone();
    WebView2Engine::begin_create_with_extras(
        host,
        bounds,
        &root.join("WebView2"),
        None,
        EngineEvents::default(),
        None,
        EngineExtras::default(),
        move |result| *slot.borrow_mut() = Some(result),
    )
    .unwrap();
    assert!(pump_until(Duration::from_secs(60), || created.borrow().is_some()), "no engine in 60 s");
    let mut engine = created.borrow_mut().take().unwrap().expect("engine created");
    let tab = TabId(1);
    engine.select_tab(tab).unwrap();
    let page = Url::parse(&format!("http://127.0.0.1:{port}/page")).unwrap();
    engine.navigate_tab(tab, &page).unwrap();
    assert!(
        pump_until(Duration::from_secs(30), || script(&mut engine, tab, "document.readyState").is_some_and(|state| state.contains("complete"))),
        "page never loaded"
    );

    let mut failures = Vec::new();
    for zoom in [100u16, 150] {
        assert!(engine.set_zoom_percent(tab, zoom).unwrap());
        pump_until(Duration::from_secs(1), || false);
        println!("LIVE_CAPTURE zoom={zoom} dpr={:?}", script(&mut engine, tab, "String(devicePixelRatio)"));
        let metrics = call(&mut engine, tab, capture::METHOD_METRICS, "{}").expect("metrics");
        println!("LIVE_CAPTURE zoom={zoom} metrics={metrics}");
        // The full-page path of the app, slice for slice.
        let capture::FullPagePlan { width, slices: plan, .. } = capture::full_page_plan(&metrics, zoom).expect("page size");
        let mut slices = Vec::new();
        for (top, height) in plan {
            let params = capture::shot_params(CaptureFormat::Png, Some(capture::Clip { x: 0, y: top, width, height }));
            let answer = call(&mut engine, tab, capture::METHOD_SHOT, &params).expect("screenshot");
            slices.push(capture::parse_shot(&answer).expect("a PNG"));
        }
        let png = capture::stitch(&slices, CaptureFormat::Png).unwrap();
        let (w, h) = capture::size_of(&png).unwrap();
        let right = pixel(&png, w - 3, h / 2);
        let bottom = pixel(&png, w / 2, h - 3);
        println!("LIVE_CAPTURE zoom={zoom} size={w}x{h} right_edge={right:?} bottom_edge={bottom:?}");
        let _ = std::fs::write(root.join(format!("zoom{zoom}.png")), &png);
        if right != (255, 0, 0) {
            failures.push(format!("zoom {zoom}: right edge is {right:?}, not the red band (cropped)"));
        }
        if bottom != (0, 0, 255) {
            failures.push(format!("zoom {zoom}: bottom edge is {bottom:?}, not the blue band (cropped)"));
        }

        // The selection path of the app: the box the selection script reports, as a clip.
        script(&mut engine, tab, "(function(){var r=document.createRange();r.selectNodeContents(document.getElementById('s'));var s=getSelection();s.removeAllRanges();s.addRange(r);return 'ok';})()");
        let answer = script(&mut engine, tab, capture::SELECTION_SCRIPT).expect("selection answer");
        let found = capture::parse_selection(&answer).ok().flatten().expect("a selection");
        let clip = capture::selection_clip(found.rect, zoom).expect("a clip");
        let params = capture::shot_params(CaptureFormat::Png, Some(clip));
        let shot = capture::parse_shot(&call(&mut engine, tab, capture::METHOD_SHOT, &params).expect("screenshot")).expect("a PNG");
        let (sw, sh) = capture::size_of(&shot).unwrap();
        let corners = [pixel(&shot, 1, 1), pixel(&shot, sw - 2, 1), pixel(&shot, 1, sh - 2), pixel(&shot, sw - 2, sh - 2)];
        println!("LIVE_CAPTURE zoom={zoom} selection rect={:?} clip={clip:?} size={sw}x{sh} corners={corners:?}", found.rect);
        if corners.iter().any(|&corner| corner != (0, 0xaa, 0)) {
            failures.push(format!("zoom {zoom}: selection capture corners are {corners:?}, not the selected text's box"));
        }
    }

    let _ = engine.close();
    drop(engine);
    unsafe {
        let _ = DestroyWindow(host);
    }
    stop.store(true, Ordering::Relaxed);
    let _ = server.join();
    for _ in 0..20 {
        if std::fs::remove_dir_all(&root).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
