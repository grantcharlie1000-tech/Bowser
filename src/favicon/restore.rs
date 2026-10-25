//! One bounded worker per window. Read every restored origin's cache before doing any network
//! work, then refresh missing/old icons. No WebView is created and no tab is activated.

use crate::blocker::fetch::Deadline;
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant, SystemTime},
};
use url::Url;

const MAX_SAVES: usize = 32;
const MAX_RESULTS: usize = 16;
const FRESH_FOR: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const REFRESH_SECONDS: u64 = 60;
const ORIGIN_SECONDS: u64 = 8;

/// Cache identities include scheme and port; no icon crosses an origin or profile boundary.
pub fn origin(url: &Url) -> Option<String> {
    (matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none())
    .then(|| url.origin().ascii_serialization())
}

fn cache_host(origin: &str) -> String {
    format!("tab-{}", super::hex(&super::sha256(origin.as_bytes())))
}

#[derive(Debug)]
pub struct Answer {
    pub origin: String,
    pub png: Option<Vec<u8>>,
}

#[derive(Default)]
struct Pending {
    saves: VecDeque<(String, Vec<u8>)>,
    restore: Option<Vec<(String, bool)>>,
}

#[derive(Default)]
struct Shared {
    pending: Mutex<Pending>,
    wake: Condvar,
    stop: AtomicBool,
}

pub struct Service {
    shared: Arc<Shared>,
    answers: Receiver<Answer>,
}

impl Service {
    pub fn start(dir: PathBuf, notify: impl Fn() + Send + 'static) -> Option<Self> {
        Self::start_with(dir, notify, super::fetch::fetch)
    }

    pub(crate) fn start_with(
        dir: PathBuf,
        notify: impl Fn() + Send + 'static,
        fetch: impl Fn(&Url, Deadline) -> Option<Vec<u8>> + Send + 'static,
    ) -> Option<Self> {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        let (send, answers) = mpsc::sync_channel(MAX_RESULTS);
        std::thread::Builder::new()
            .name("bowser-favicons".into())
            .spawn(move || {
                use windows::Win32::System::Com::{
                    COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize,
                };
                if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
                    return;
                }
                run(&dir, &worker, &send, &notify, fetch);
                super::wic::release_factory();
                unsafe {
                    CoUninitialize();
                }
            })
            .ok()?;
        Some(Self { shared, answers })
    }

    #[cfg(test)]
    pub fn restore(&self, urls: impl IntoIterator<Item = Url>) {
        self.restore_with(urls, |_| true);
    }

    pub fn restore_with(
        &self,
        urls: impl IntoIterator<Item = Url>,
        may_refresh: impl Fn(&Url) -> bool,
    ) {
        let mut seen = HashSet::new();
        let origins = urls
            .into_iter()
            .take(crate::session::MAX_TABS)
            .filter_map(|url| origin(&url).map(|origin| (origin, may_refresh(&url))))
            .filter(|(origin, _)| seen.insert(origin.clone()))
            .collect();
        self.shared
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .restore = Some(origins);
        self.shared.wake.notify_one();
    }

    /// Coalesce repeated callbacks; drop excess work rather than block the UI or grow without bound.
    pub fn save(&self, origin: String, png: Vec<u8>) {
        if !crate::chrome_host::favicon_sendable(&png) {
            return;
        }
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        pending.saves.retain(|(key, _)| key != &origin);
        if pending.saves.len() == MAX_SAVES {
            pending.saves.pop_front();
        }
        pending.saves.push_back((origin, png));
        self.shared.wake.notify_one();
    }

    pub fn drain(&self) -> Vec<Answer> {
        self.answers.try_iter().take(MAX_RESULTS).collect()
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        let _pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.wake.notify_one();
        // No join on the window thread. Dropping the receiver also releases a blocked result send.
    }
}

fn checked_png(bytes: &[u8]) -> Option<Vec<u8>> {
    if crate::chrome_host::favicon_sendable(bytes) && super::decode(bytes, 16).is_some() {
        Some(bytes.to_vec())
    } else {
        super::wic::png(bytes)
    }
}

fn cached(dir: &std::path::Path, origin: &str, now: SystemTime) -> (Option<Vec<u8>>, bool) {
    let host = cache_host(origin);
    let bytes = super::load_favicon_bytes_in(dir, &host)
        .ok()
        .flatten()
        .and_then(|bytes| checked_png(&bytes));
    let path = dir.join(format!(
        "{}.ico",
        super::hex(&super::sha256(host.as_bytes()))
    ));
    let fresh = bytes.is_some()
        && std::fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|at| now.duration_since(at).ok())
            .is_some_and(|age| age < FRESH_FOR);
    (bytes, fresh)
}

fn persist(dir: &std::path::Path, origin: &str, png: &[u8]) {
    if let Ok(name) = super::store_favicon_in(dir, &cache_host(origin), png) {
        let path = dir.join(name);
        // Identical bytes are not rewritten by the cache. Still record a successful revalidation,
        // at most once a week, or an unchanged icon would be fetched again on every restart.
        let recent = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|at| SystemTime::now().duration_since(at).ok())
            .is_some_and(|age| age < FRESH_FOR);
        if !recent && let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
            let _ = file.set_modified(SystemTime::now());
        }
    }
}

fn run(
    dir: &std::path::Path,
    shared: &Shared,
    send: &SyncSender<Answer>,
    notify: &dyn Fn(),
    fetch: impl Fn(&Url, Deadline) -> Option<Vec<u8>>,
) {
    let mut refresh = VecDeque::new();
    let mut until = Instant::now();
    loop {
        let (saves, restore) = {
            let mut pending = shared
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            while pending.saves.is_empty()
                && pending.restore.is_none()
                && refresh.is_empty()
                && !shared.stop.load(Ordering::Relaxed)
            {
                pending = shared
                    .wake
                    .wait(pending)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            if shared.stop.load(Ordering::Relaxed) {
                return;
            }
            (std::mem::take(&mut pending.saves), pending.restore.take())
        };
        for (origin, bytes) in saves {
            if shared.stop.load(Ordering::Relaxed) {
                return;
            }
            if let Some(png) = checked_png(&bytes) {
                persist(dir, &origin, &png);
                refresh.retain(|key| key != &origin);
            }
        }
        if let Some(origins) = restore {
            refresh.clear();
            until = Instant::now() + Duration::from_secs(REFRESH_SECONDS);
            // Cache delivery for ALL origins precedes the first (possibly slow) request.
            for (origin, allowed) in origins {
                if shared.stop.load(Ordering::Relaxed) {
                    return;
                }
                let (png, fresh) = cached(dir, &origin, SystemTime::now());
                if png.is_some() {
                    if send
                        .send(Answer {
                            origin: origin.clone(),
                            png,
                        })
                        .is_err()
                    {
                        return;
                    }
                    notify();
                }
                if !fresh && allowed {
                    refresh.push_back(origin);
                }
            }
        }
        if let Some(origin) = refresh.pop_front() {
            if Instant::now() >= until {
                continue;
            }
            let Some(png) = refresh_origin(&origin, until, &shared.stop, &fetch) else {
                continue;
            };
            if shared.stop.load(Ordering::Relaxed) {
                return;
            }
            persist(dir, &origin, &png);
            if send
                .send(Answer {
                    origin,
                    png: Some(png),
                })
                .is_err()
            {
                return;
            }
            notify();
        }
    }
}

fn refresh_origin(
    origin: &str,
    until: Instant,
    stop: &AtomicBool,
    fetch: &impl Fn(&Url, Deadline) -> Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    let page = Url::parse(origin).ok()?;
    if !super::fetch::allowed(&page) {
        return None;
    }
    let started = Instant::now();
    let seconds = until
        .saturating_duration_since(started)
        .as_secs()
        .min(ORIGIN_SECONDS);
    if seconds == 0 {
        return None;
    }
    let deadline = Deadline::after(started, seconds);
    for text in super::favicon_url_candidates(origin) {
        if stop.load(Ordering::Relaxed) || Instant::now() >= until || deadline.remaining().is_err()
        {
            break;
        }
        if let Some(bytes) = fetch(&Url::parse(&text).ok()?, deadline)
            && let Some(png) = checked_png(&bytes)
        {
            return Some(png);
        }
    }
    None
}

#[cfg(test)]
mod tests;
