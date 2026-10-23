use super::*;
use std::sync::atomic::AtomicUsize;

const PNG: &[u8] = include_bytes!("../../../icons/history-favicon.png");
const OTHER: &[u8] = include_bytes!("../../../icons/settings-favicon.png");

fn dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("bowser-icons-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn collect(service: &Service, count: usize) -> Vec<Answer> {
    let until = Instant::now() + Duration::from_secs(5);
    let mut answers = Vec::new();
    while answers.len() < count && Instant::now() < until {
        answers.extend(service.drain());
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(answers.len(), count);
    answers
}

#[test]
fn all_cached_icons_arrive_before_network_and_stale_failure_keeps_the_icon() {
    let path = dir("cache-first");
    for origin in ["https://old.example", "https://fresh.example"] {
        let name = super::super::store_favicon_in(&path, &cache_host(origin), PNG).unwrap();
        if origin.contains("old") {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(path.join(name))
                .unwrap();
            file.set_times(std::fs::FileTimes::new().set_modified(SystemTime::UNIX_EPOCH))
                .unwrap();
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let (release, gate) = mpsc::channel();
    let gate = Mutex::new(gate);
    let service = Service::start_with(
        path.clone(),
        || {},
        move |_, _| {
            called.fetch_add(1, Ordering::SeqCst);
            gate.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            None
        },
    )
    .unwrap();
    service.restore([
        Url::parse("https://old.example/private?q=secret#token").unwrap(),
        Url::parse("https://fresh.example/a").unwrap(),
    ]);
    let answers = collect(&service, 2);
    assert!(
        answers
            .iter()
            .all(|answer| answer.png.as_deref() == Some(PNG))
    );
    // Only the old icon is refreshed; all cache results have arrived while that request is blocked.
    for _ in 0..1000 {
        if calls.load(Ordering::SeqCst) > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(service);
    release.send(()).unwrap();
    assert_eq!(
        super::super::load_favicon_bytes_in(&path, &cache_host("https://old.example"))
            .unwrap()
            .as_deref(),
        Some(PNG)
    );
}

#[test]
fn missing_corrupt_and_oversized_icons_refresh_once_per_origin_with_safe_candidates() {
    let path = dir("missing");
    super::super::store_favicon_in(&path, &cache_host("https://bad.example"), b"not an image")
        .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let called = calls.clone();
    let service = Service::start_with(
        path,
        || {},
        move |url, _| {
            called.lock().unwrap().push(url.as_str().to_owned());
            if url.path() == "/favicon.ico" {
                Some(vec![0; super::super::MAX_FAVICON_FILE_BYTES + 1])
            } else {
                Some(PNG.to_vec())
            }
        },
    )
    .unwrap();
    service.restore(
        [
            "https://bad.example/account?secret=1",
            "https://bad.example/second",
            "https://missing.example/",
        ]
        .map(|text| Url::parse(text).unwrap()),
    );
    let answers = collect(&service, 2);
    assert!(
        answers
            .iter()
            .all(|answer| answer.png.as_deref() == Some(PNG))
    );
    assert_eq!(
        *calls.lock().unwrap(),
        [
            "https://bad.example/favicon.ico",
            "https://bad.example/favicon.png",
            "https://missing.example/favicon.ico",
            "https://missing.example/favicon.png"
        ]
    );
}

#[test]
fn caches_are_separate_by_profile_scheme_and_port_and_refresh_obeys_total_budget() {
    let first = dir("profile-a");
    let second = dir("profile-b");
    super::super::store_favicon_in(&first, &cache_host("https://same.example"), PNG).unwrap();
    super::super::store_favicon_in(&second, &cache_host("https://same.example"), OTHER).unwrap();
    let a =
        Service::start_with(first, || {}, |_, _| panic!("fresh cache needs no request")).unwrap();
    let b =
        Service::start_with(second, || {}, |_, _| panic!("fresh cache needs no request")).unwrap();
    a.restore([Url::parse("https://same.example/").unwrap()]);
    b.restore([Url::parse("https://same.example/").unwrap()]);
    assert_eq!(collect(&a, 1)[0].png.as_deref(), Some(PNG));
    assert_eq!(collect(&b, 1)[0].png.as_deref(), Some(OTHER));
    assert_ne!(
        cache_host("https://same.example"),
        cache_host("http://same.example")
    );
    assert_ne!(
        cache_host("https://same.example"),
        cache_host("https://same.example:8443")
    );
    let stopped = AtomicBool::new(false);
    let calls = AtomicUsize::new(0);
    let fetch = |_: &Url, _: Deadline| {
        calls.fetch_add(1, Ordering::Relaxed);
        None
    };
    assert!(refresh_origin("https://same.example", Instant::now(), &stopped, &fetch).is_none());
    stopped.store(true, Ordering::Relaxed);
    assert!(
        refresh_origin(
            "https://same.example",
            Instant::now() + Duration::from_secs(10),
            &stopped,
            &fetch
        )
        .is_none()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn queues_coalesce_and_bound_bytes_origins_results_and_candidate_attempts() {
    // No worker: inspect exactly what is queued without a scheduling race.
    let (send, answers) = mpsc::sync_channel(MAX_RESULTS);
    let service = Service {
        shared: Arc::new(Shared::default()),
        answers,
    };
    for n in 0..1000 {
        service.save(format!("https://h{n}.example"), PNG.to_vec());
    }
    service.save("https://h999.example".into(), OTHER.to_vec());
    service.save(
        "https://oversized.example".into(),
        vec![0; super::super::MAX_FAVICON_FILE_BYTES + 1],
    );
    service
        .restore((0..1000).map(|n| Url::parse(&format!("https://h{n}.example/private")).unwrap()));
    let pending = service.shared.pending.lock().unwrap();
    assert_eq!(pending.saves.len(), MAX_SAVES);
    assert_eq!(pending.saves.back().unwrap().1, OTHER);
    assert_eq!(
        pending.restore.as_ref().unwrap().len(),
        crate::session::MAX_TABS
    );
    drop(pending);
    for _ in 0..MAX_RESULTS {
        send.try_send(Answer {
            origin: String::new(),
            png: None,
        })
        .unwrap();
    }
    assert!(
        send.try_send(Answer {
            origin: String::new(),
            png: None
        })
        .is_err()
    );
    let calls = AtomicUsize::new(0);
    let result = refresh_origin(
        "https://same.example",
        Instant::now() + Duration::from_secs(20),
        &AtomicBool::new(false),
        &|_, _| {
            calls.fetch_add(1, Ordering::Relaxed);
            None
        },
    );
    assert!(result.is_none());
    assert_eq!(
        calls.load(Ordering::Relaxed),
        3,
        "fixed same-origin paths, no retry loop"
    );
}

#[test]
fn normalization_turns_ico_into_a_small_sendable_png_and_refuses_invalid_data() {
    unsafe {
        windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        )
        .ok()
        .unwrap();
    }
    assert!(checked_png(b"<html>failure</html>").is_none());
    // A single-image ICO whose payload is PNG (valid ICO format).
    let mut ico = vec![0, 0, 1, 0, 1, 0, 16, 16, 0, 0, 1, 0, 32, 0];
    ico.extend_from_slice(&(PNG.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(PNG);
    let png = checked_png(&ico).expect("ICO decoded by WIC");
    assert!(crate::chrome_host::favicon_sendable(&png));
    assert_eq!(crate::capture::png_header_size(&png), Some((32, 32)));
}

#[test]
fn revalidation_of_identical_bytes_becomes_fresh_and_blocked_refresh_still_uses_cache() {
    let path = dir("revalidation");
    let origin = "https://same.example";
    let name = super::super::store_favicon_in(&path, &cache_host(origin), PNG).unwrap();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path.join(name))
        .unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(SystemTime::UNIX_EPOCH))
        .unwrap();
    let service = Service::start_with(path.clone(), || {}, |_, _| Some(PNG.to_vec())).unwrap();
    service.restore([Url::parse(origin).unwrap()]);
    assert_eq!(
        collect(&service, 2).len(),
        2,
        "stale cache followed by revalidated icon"
    );
    drop(service);
    let service = Service::start_with(
        path.clone(),
        || {},
        |_, _| panic!("unchanged icon was just revalidated"),
    )
    .unwrap();
    service.restore([Url::parse(origin).unwrap()]);
    assert_eq!(collect(&service, 1)[0].png.as_deref(), Some(PNG));
    drop(service);
    let service =
        Service::start_with(path, || {}, |_, _| panic!("refresh refused by blocker")).unwrap();
    service.restore_with(
        [
            Url::parse(origin).unwrap(),
            Url::parse("https://missing.example").unwrap(),
        ],
        |_| false,
    );
    assert_eq!(collect(&service, 1)[0].png.as_deref(), Some(PNG));
}

#[test]
fn idle_workers_release_their_context_after_shutdown() {
    struct Finished(mpsc::Sender<()>);
    impl Drop for Finished {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let path = dir("shutdown");
    for _ in 0..32 {
        let (send, ended) = mpsc::channel();
        let finished = Finished(send);
        let service = Service::start_with(
            path.clone(),
            || {},
            move |_, _| {
                let _ = &finished;
                None
            },
        )
        .unwrap();
        std::thread::yield_now();
        drop(service);
        ended
            .recv_timeout(Duration::from_secs(2))
            .expect("an idle worker did not stop");
    }
}
