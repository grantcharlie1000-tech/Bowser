//! Native OCR adapter for the Spoiler Firewall (docs/spoiler-firewall.md, DESIGN.md section 8).
//!
//! Bowser observes image responses of high-risk sites, decodes them on a worker thread and runs
//! `Windows.Media.Ocr` over them. The page-side bundle asks for the text of an image through one
//! non-enumerable `window` hook; native pulls those requests with `ExecuteScript`, answers from an
//! in-memory cache and pushes the answers back the same way. There is no web message channel, and
//! nothing is ever fetched on a page's behalf.
//!
//! This file holds everything that does not need a WebView: the site groups and the serve policy,
//! URL and drain parsing (the drain result is hostile input), the delivery script, the LRU cache,
//! the scale/tile plan, the worker queue and the WinRT recognizer. The per-tab glue is
//! `engine/webview2/ocr.rs`. OCR text lives in memory only: it is never written to disk or logged.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::spoiler::{OcrCredentials, json_string};


/// Largest image body taken (Content-Length, or bytes read when the length is unknown).
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
/// Smaller bodies cannot be a 96x64 image (a flat-colour text PNG can still be only ~1 KB).
pub const MIN_IMAGE_BYTES: u64 = 256;
/// Images below this size in either direction are never scanned.
pub const MIN_WIDTH: u32 = 96;
pub const MIN_HEIGHT: u32 = 64;
/// Requests taken from one drain, and the longest URL accepted.
pub const MAX_DRAIN_ITEMS: usize = 64;
pub const MAX_URL_LEN: usize = 2048;
pub const MAX_ID_LEN: usize = 64;
/// A drain result longer than this is not from the bundle and is dropped unread.
pub const MAX_DRAIN_CHARS: usize = 64 * 1024;
/// Longest text delivered per image, in UTF-16 units (the bundle accepts 4096).
pub const MAX_TEXT_UNITS: usize = 4096;
/// One `ExecuteScript` delivery stays under this many bytes.
pub const MAX_DELIVERY_BYTES: usize = 192 * 1024;
pub const LRU_CAPACITY: usize = 2000;
/// Image bodies kept in memory, unscanned, until the page asks about them (see `Parked`).
pub const PARKED_BYTES: usize = 64 * 1024 * 1024;
pub const PARKED_CAPACITY: usize = 600;
/// Jobs waiting for the worker, and the bytes they hold between them.
pub const QUEUE_CAPACITY: usize = 64;
pub const QUEUE_BYTES: usize = 48 * 1024 * 1024;
/// Images scanned below this short edge are scaled up 2x first.
const UPSCALE_BELOW: u32 = 300;
const TILE_HEIGHT: u32 = 4096;
const TILE_OVERLAP: u32 = 64;
const MAX_TILES: usize = 8;
/// Extra OCR languages are tried only for small, text-poor images (see `Recognizer::recognize`).
const MAX_EXTRA_ENGINES: usize = 2;
const EXTRA_ENGINE_MAX_PIXELS: u64 = 2_500_000;
const EXTRA_ENGINE_BELOW_ALNUM: usize = 12;

// ---- sites ----

/// A platform whose pages and image CDNs may be read for text. A page may only read images of its
/// own group, so Bowser is never a cross-origin image reader elsewhere.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Group {
    X,
    Reddit,
    Meta,
    TikTok,
    Bluesky,
    YouTube,
    Twitch,
    Discord,
    Imgur,
    FourChan,
    GtaForums,
    ResetEra,
    Telegram,
    Kick,
    Tumblr,
    Steam,
    Rockstar,
    /// Games press and guides (IGN, GameSpot, Kotaku, ...): one group, because their image CDNs are shared.
    Press,
    Fandom,
}

const SITES: &[(Group, &[&str])] = &[
    (Group::X, &["x.com", "twitter.com", "twimg.com"]),
    (Group::Reddit, &["reddit.com", "redditmedia.com", "redd.it", "redditstatic.com"]),
    (
        Group::Meta,
        &["instagram.com", "cdninstagram.com", "fbcdn.net", "facebook.com", "threads.net", "threads.com"],
    ),
    (Group::TikTok, &["tiktok.com", "tiktokcdn.com", "tiktokcdn-us.com"]),
    (Group::Bluesky, &["bsky.app"]),
    (Group::YouTube, &["youtube.com", "ytimg.com", "youtu.be"]),
    (Group::Twitch, &["twitch.tv", "jtvnw.net"]),
    (Group::Discord, &["discord.com", "discordapp.com", "discordapp.net"]),
    (Group::Imgur, &["imgur.com"]),
    (Group::FourChan, &["4chan.org", "4channel.org", "4cdn.org"]),
    (Group::GtaForums, &["gtaforums.com"]),
    (Group::ResetEra, &["resetera.com"]),
    (Group::Telegram, &["telegram.org", "t.me", "telesco.pe", "cdn-telegram.org"]),
    (Group::Kick, &["kick.com"]),
    (Group::Tumblr, &["tumblr.com"]),
    (Group::Steam, &["steamcommunity.com", "steamuserimages-a.akamaihd.net", "steamstatic.com"]),
    (Group::Rockstar, &["rockstargames.com"]),
    (
        Group::Press,
        &[
            "ign.com", "ignimgs.com", "gamespot.com", "kotaku.com", "kinja-img.com", "polygon.com", "vox-cdn.com", "eurogamer.net",
            "gamesradar.com", "pcgamer.com", "futurecdn.net", "vg247.com", "pushsquare.com", "purexbox.com", "gamefaqs.com",
            "gamefaqs.gamespot.com", "nintendolife.com", "gamerant.com", "thegamer.com", "dexerto.com", "screenrant.com", "destructoid.com",
            "giantbomb.com",
        ],
    ),
    (Group::Fandom, &["fandom.com", "wikia.nocookie.net"]),
];

/// `WebResourceRequested` filter URIs (image context) for every listed domain and its subdomains.
/// Only a fallback: `WebResourceResponseReceived` needs no filter, and each filtered request waits on
/// the UI thread, so a tab adds these only when it never sees a response event without them
/// (`TabOcr::filter_fallback`). Narrow on purpose: never a `*` image filter.
pub fn filter_uris() -> Vec<String> {
    let mut uris = Vec::with_capacity(SITES.iter().map(|(_, domains)| domains.len() * 2).sum::<usize>() + 1);
    for (_, domains) in SITES {
        for domain in *domains {
            uris.push(format!("*://{domain}/*"));
            uris.push(format!("*://*.{domain}/*"));
        }
    }
    #[cfg(debug_assertions)]
    if let Some(host) = test_host() {
        uris.push(format!("*://{host}*")); // the test fixtures run on a port
    }
    uris
}

/// The group of a host: the host is a listed domain or a subdomain of one (`cdn.bsky.app`,
/// `pbs.twimg.com`). `notx.com` is not `x.com`.
pub fn group_for_host(host: &str) -> Option<Group> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host.len() > 253 {
        return None;
    }
    #[cfg(debug_assertions)]
    if test_host().is_some_and(|test| test == host) {
        return Some(Group::X);
    }
    SITES.iter().find_map(|(group, domains)| {
        domains
            .iter()
            .any(|domain| {
                host == *domain
                    || host
                        .strip_suffix(domain)
                        .is_some_and(|rest| rest.ends_with('.'))
            })
            .then_some(*group)
    })
}

/// Debug builds only (never release): `BOWSER_SPOILER_OCR_TEST_HOST=<host>` makes that one host count as
/// a listed site, so the live fixtures on 127.0.0.1 can exercise OCR without a hosts-file entry.
#[cfg(debug_assertions)]
fn test_host() -> Option<&'static str> {
    static HOST: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    HOST.get_or_init(|| {
        std::env::var("BOWSER_SPOILER_OCR_TEST_HOST")
            .ok()
            .map(|host| host.trim().to_ascii_lowercase())
            .filter(|host| !host.is_empty() && host.len() < 254)
    })
    .as_deref()
}

pub fn group_for_url(url: &str) -> Option<Group> {
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    group_for_host(parsed.host_str()?)
}

/// OCR text is served only when the asking frame and the image are both on the allowlist and in
/// the same group.
pub fn may_serve(frame: Option<Group>, image: Option<Group>) -> bool {
    matches!((frame, image), (Some(a), Some(b)) if a == b)
}

pub fn visual_group(group: Group) -> bool {
    matches!(group, Group::X | Group::YouTube)
}

pub fn is_youtube_thumbnail(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else { return false };
    let Some(host) = parsed.host_str() else { return false };
    (host == "ytimg.com" || host.ends_with(".ytimg.com"))
        && (parsed.path().starts_with("/vi/") || parsed.path().starts_with("/vi_webp/"))
}

/// The URL a page asked about, in the form the cache uses: absolute http(s), at most 2048
/// characters, no credentials, no fragment, no whitespace or control characters.
pub fn normalize_url(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.len() > MAX_URL_LEN || raw.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let mut parsed = url::Url::parse(raw).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    parsed.set_fragment(None);
    let text = parsed.to_string();
    (text.len() <= MAX_URL_LEN).then_some(text)
}

/// Image URLs that are below the scan size by construction (X avatars in their small variants, emoji
/// and hashflags): their bodies are never read, and a request for one is answered `Skipped` at once,
/// the same answer a decode would give.
pub fn known_small(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else { return false };
    let host = parsed.host_str().unwrap_or("");
    let path = parsed.path();
    if host == "pbs.twimg.com" || host == "abs.twimg.com" {
        if path.starts_with("/emoji/") || path.starts_with("/hashflags/") {
            return true;
        }
        if path.starts_with("/profile_images/") {
            let stem = path.rsplit('/').next().unwrap_or("");
            let stem = stem.split('.').next().unwrap_or(stem);
            return stem.ends_with("_normal") || stem.ends_with("_mini") || stem.ends_with("_bigger");
        }
    }
    false
}

// ---- results and cache ----

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Ok,
    Unavailable,
    Error,
    Skipped,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Unavailable => "unavailable",
            Self::Error => "error",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Entry {
    /// Seen and being fetched or scanned.
    Pending,
    /// Read and kept in memory (`Parked`), not scanned: nothing has asked about it yet.
    Parked,
    Done { status: Status, text: String },
}

impl Entry {
    pub fn done(status: Status, text: impl Into<String>) -> Self {
        Self::Done { status, text: text.into() }
    }
}

/// (top-level site group, image URL).
pub type Key = (Group, String);

/// Image bodies that were read from responses but not scanned. OCR is the expensive part (decode,
/// scale, up to three recognition passes); most images a feed loads are never asked about, because
/// the page only asks for media near the viewport whose post is not already decided by its words.
/// So bodies wait here and are scanned only when a request names them. Oldest first out, bounded in
/// count and bytes.
pub struct Parked {
    tick: u64,
    bytes: usize,
    map: HashMap<Key, (u64, Vec<u8>)>,
    order: BTreeMap<u64, Key>,
}

impl Default for Parked {
    fn default() -> Self {
        Self::new()
    }
}

impl Parked {
    pub fn new() -> Self {
        Self { tick: 0, bytes: 0, map: HashMap::new(), order: BTreeMap::new() }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Keeps a body. Returns the keys dropped (oldest first) to stay within the caps.
    pub fn put(&mut self, key: Key, body: Vec<u8>) -> Vec<Key> {
        let _ = self.take(&key);
        if body.len() > PARKED_BYTES {
            return vec![key];
        }
        self.tick += 1;
        self.bytes += body.len();
        self.order.insert(self.tick, key.clone());
        self.map.insert(key, (self.tick, body));
        let mut dropped = Vec::new();
        while self.map.len() > PARKED_CAPACITY || self.bytes > PARKED_BYTES {
            let Some((_, old)) = self.order.pop_first() else { break };
            if let Some((_, body)) = self.map.remove(&old) {
                self.bytes -= body.len();
            }
            dropped.push(old);
        }
        dropped
    }

    /// Removes and returns a body.
    pub fn take(&mut self, key: &Key) -> Option<Vec<u8>> {
        let (tick, body) = self.map.remove(key)?;
        self.order.remove(&tick);
        self.bytes -= body.len();
        Some(body)
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
        self.bytes = 0;
    }
}

/// Least-recently-used map. Lookups and inserts refresh an entry.
pub struct Lru {
    capacity: usize,
    tick: u64,
    map: HashMap<Key, (Entry, u64)>,
    order: BTreeMap<u64, Key>,
}

impl Lru {
    pub fn new(capacity: usize) -> Self {
        Self { capacity: capacity.max(1), tick: 0, map: HashMap::new(), order: BTreeMap::new() }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get(&mut self, key: &Key) -> Option<Entry> {
        let (entry, old) = self.map.get_mut(key)?;
        self.tick += 1;
        self.order.remove(old);
        *old = self.tick;
        self.order.insert(self.tick, key.clone());
        Some(entry.clone())
    }

    pub fn insert(&mut self, key: Key, entry: Entry) {
        self.tick += 1;
        if let Some((_, old)) = self.map.insert(key.clone(), (entry, self.tick)) {
            self.order.remove(&old);
        }
        self.order.insert(self.tick, key);
        while self.map.len() > self.capacity {
            let Some((_, oldest)) = self.order.pop_first() else { break };
            self.map.remove(&oldest);
        }
    }

    pub fn remove(&mut self, key: &Key) {
        if let Some((_, tick)) = self.map.remove(key) {
            self.order.remove(&tick);
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

// ---- the drain (hostile input) and the delivery ----

/// One request the page's bundle handed out. `url` is `None` when it was not an acceptable
/// absolute http(s) address; the page is still answered ("unavailable") so it does not wait.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DrainItem {
    pub id: String,
    pub url: Option<String>,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':'))
}

/// Parses an `ExecuteScript` drain result. The script returns a JSON string holding a JSON array,
/// so the result is a JSON string literal; a bare array and `null` are also accepted. Everything
/// else, an oversized result, a wrong type, a bad id: dropped. At most 64 items are taken.
pub fn parse_drain(raw: &str) -> Vec<DrainItem> {
    use crate::extensions::json::{Value, get, parse};
    if raw.len() > MAX_DRAIN_CHARS {
        return Vec::new();
    }
    let Some(mut value) = parse(raw.trim()) else { return Vec::new() };
    if let Value::String(inner) = &value {
        if inner.len() > MAX_DRAIN_CHARS {
            return Vec::new();
        }
        let Some(parsed) = parse(inner.trim()) else { return Vec::new() };
        value = parsed;
    }
    let Value::Array(items) = value else { return Vec::new() };
    items
        .iter()
        .take(MAX_DRAIN_ITEMS)
        .filter_map(|item| {
            let Value::Object(fields) = item else { return None };
            let Some(Value::String(id)) = get(fields, "id") else { return None };
            if !valid_id(id) {
                return None;
            }
            let url = match get(fields, "url") {
                Some(Value::String(url)) => normalize_url(url),
                _ => None,
            };
            Some(DrainItem { id: id.clone(), url })
        })
        .collect()
}

/// One answer for the page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Delivery {
    pub id: String,
    pub status: Status,
    pub text: String,
}

/// `text` cut to at most `MAX_TEXT_UNITS` UTF-16 units, on a character boundary.
pub fn clip_text(text: &str) -> String {
    let mut units = 0usize;
    let mut out = String::new();
    for c in text.chars() {
        units += c.len_utf16();
        if units > MAX_TEXT_UNITS {
            break;
        }
        out.push(c);
    }
    out
}

fn delivery_json(item: &Delivery) -> String {
    let mut json = format!("{{\"id\":{},\"status\":\"{}\"", json_string(&item.id), item.status.as_str());
    if item.status == Status::Ok {
        json.push_str(",\"text\":");
        json.push_str(&json_string(&clip_text(&item.text)));
    }
    json.push('}');
    json
}

/// Scripts that deliver `items` to the hook, each under `MAX_DELIVERY_BYTES`. The payload is
/// JSON inside a JavaScript string literal (both escape U+2028/2029), and `</` is spelled
/// `<\u002f`. `None` when the credentials are not the shape native generates.
pub fn delivery_scripts(creds: &OcrCredentials, items: &[Delivery]) -> Option<Vec<String>> {
    if !creds.is_well_formed() {
        return None;
    }
    let mut scripts = Vec::new();
    let mut batch: Vec<String> = Vec::new();
    let mut size = 2usize;
    let flush = |batch: &mut Vec<String>, scripts: &mut Vec<String>| {
        if batch.is_empty() {
            return;
        }
        let payload = format!("[{}]", batch.join(","));
        let script = format!(
            "(function(){{try{{var h=window[{}];if(h&&typeof h.deliver===\"function\")h.deliver({},{})}}catch(e){{}}}})()",
            json_string(&creds.hook),
            json_string(&creds.key),
            json_string(&payload),
        );
        scripts.push(script.replace("</", "<\\u002f"));
        batch.clear();
    };
    for item in items {
        let json = delivery_json(item);
        if size + json.len() + 1 > MAX_DELIVERY_BYTES && !batch.is_empty() {
            flush(&mut batch, &mut scripts);
            size = 2;
        }
        size += json.len() + 1;
        batch.push(json);
    }
    flush(&mut batch, &mut scripts);
    Some(scripts)
}

/// The script that hands a verified term update (the feed's lexicon object as JSON text) to the page's
/// engine through the per-tab hook (`feed(key, json)`). `None` for malformed credentials or a lexicon
/// that is not an embeddable object of at most 256 KB.
pub fn feed_script(creds: &OcrCredentials, lexicon_json: &str) -> Option<String> {
    (creds.is_well_formed() && crate::spoiler::is_embeddable_object(lexicon_json)).then(|| {
        format!(
            "(function(){{try{{var h=window[{}];if(h&&typeof h.feed===\"function\")h.feed({},{})}}catch(e){{}}}})()",
            json_string(&creds.hook),
            json_string(&creds.key),
            json_string(lexicon_json),
        )
    })
}

/// The script that takes the page's pending requests.
pub fn drain_script(creds: &OcrCredentials) -> Option<String> {
    creds.is_well_formed().then(|| {
        format!(
            "(function(){{try{{var h=window[{}];return h&&typeof h.drain===\"function\"?h.drain({}):null}}catch(e){{return null}}}})()",
            json_string(&creds.hook),
            json_string(&creds.key),
        )
    })
}

// ---- scale and tile plan ----

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    pub scaled_width: u32,
    pub scaled_height: u32,
    /// `(y, height)` bands in scaled pixels, overlapping by `TILE_OVERLAP`.
    pub tiles: Vec<(u32, u32)>,
}

/// How to present a `width` x `height` image to the engine. `None` for an image too small to
/// hold readable text. A short edge under 300 px is scaled up 2x; a width over the engine limit is
/// scaled down to it; an image taller than one tile is cut into overlapping bands (at most 8).
pub fn plan(width: u32, height: u32, max_dimension: u32) -> Option<Plan> {
    if width < MIN_WIDTH || height < MIN_HEIGHT || max_dimension < 64 {
        return None;
    }
    let mut scale = if width.min(height) < UPSCALE_BELOW { 2.0 } else { 1.0 };
    let longest = f64::from(width.max(height)) * scale;
    if scale > 1.0 && longest > f64::from(max_dimension) {
        scale = f64::from(max_dimension) / f64::from(width.max(height));
    }
    if f64::from(width) * scale > f64::from(max_dimension) {
        scale = f64::from(max_dimension) / f64::from(width);
    }
    let scaled_width = ((f64::from(width) * scale).round() as u32).clamp(1, max_dimension);
    let scaled_height = (f64::from(height) * scale).round() as u32;
    let scaled_height = scaled_height.max(1);
    let tile = TILE_HEIGHT.min(max_dimension);
    let mut tiles = Vec::new();
    let mut y = 0u32;
    while y < scaled_height && tiles.len() < MAX_TILES {
        let band = tile.min(scaled_height - y);
        tiles.push((y, band));
        if y + band >= scaled_height {
            break;
        }
        y += tile - TILE_OVERLAP;
    }
    Some(Plan { scaled_width, scaled_height: scaled_height.min(max_dimension.saturating_mul(MAX_TILES as u32)), tiles })
}

// ---- the worker ----

pub struct Job {
    pub ticket: u64,
    pub bytes: Vec<u8>,
    pub group: Group,
    /// The tab's navigation epoch, and its value when the job was queued. A job whose epoch moved
    /// on is dropped unrun.
    pub epoch: Arc<AtomicU64>,
    pub queued_at: u64,
}

pub struct JobResult {
    pub ticket: u64,
    /// `None` when the job was dropped (navigation, queue overflow, shutdown).
    pub outcome: Option<(Status, String)>,
}

#[derive(Default)]
struct QueueState {
    jobs: VecDeque<Job>,
    bytes: usize,
    closed: bool,
}

struct Shared {
    queue: Mutex<QueueState>,
    wake: Condvar,
    results: Mutex<Vec<JobResult>>,
}

/// Set when no OCR engine could be created: every request is then answered "unavailable".
static ENGINE_UNAVAILABLE: AtomicBool = AtomicBool::new(false);

pub fn engine_unavailable() -> bool {
    ENGINE_UNAVAILABLE.load(Ordering::Relaxed)
}

pub struct Worker {
    shared: Arc<Shared>,
}

/// How many OCR threads share the queue: one, or two on a machine with at least 8 hardware threads.
/// One thread left thumbnails of a busy page (a YouTube results page holds dozens) covered for
/// seconds; the threads run below normal priority, so the page and the browser come first.
pub fn worker_count(parallelism: usize) -> usize {
    if parallelism >= 8 { 2 } else { 1 }
}

impl Worker {
    pub fn spawn() -> Option<Self> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(QueueState::default()),
            wake: Condvar::new(),
            results: Mutex::new(Vec::new()),
        });
        let threads = worker_count(std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));
        let mut started = 0;
        for index in 0..threads {
            let thread_shared = shared.clone();
            if std::thread::Builder::new()
                .name(format!("bowser-spoiler-ocr-{index}"))
                .spawn(move || worker_main(thread_shared))
                .is_ok()
            {
                started += 1;
            }
        }
        (started > 0).then_some(Self { shared })
    }

    /// Queues a job. Older jobs are dropped (and reported as dropped) to stay within the count and
    /// byte caps: the newest images are the ones on screen.
    pub fn push(&self, job: Job) {
        let mut dropped = Vec::new();
        {
            let Ok(mut queue) = self.shared.queue.lock() else { return };
            queue.bytes += job.bytes.len();
            queue.jobs.push_back(job);
            while queue.jobs.len() > QUEUE_CAPACITY || (queue.bytes > QUEUE_BYTES && queue.jobs.len() > 1) {
                if let Some(old) = queue.jobs.pop_front() {
                    queue.bytes -= old.bytes.len();
                    dropped.push(JobResult { ticket: old.ticket, outcome: None });
                }
            }
        }
        if !dropped.is_empty()
            && let Ok(mut results) = self.shared.results.lock()
        {
            results.extend(dropped);
        }
        self.shared.wake.notify_one();
    }

    /// Everything the worker finished or dropped since the last call.
    pub fn take_results(&self) -> Vec<JobResult> {
        self.shared
            .results
            .lock()
            .map(|mut results| std::mem::take(&mut *results))
            .unwrap_or_default()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Ok(mut queue) = self.shared.queue.lock() {
            queue.closed = true;
            queue.jobs.clear();
        }
        self.shared.wake.notify_all();
    }
}

fn worker_main(shared: Arc<Shared>) {
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
    let mut recognizer = Recognizer::default();
    loop {
        let job = {
            let Ok(mut queue) = shared.queue.lock() else { return };
            loop {
                if queue.closed {
                    return;
                }
                if let Some(job) = queue.jobs.pop_front() {
                    queue.bytes -= job.bytes.len();
                    break job;
                }
                queue = match shared.wake.wait(queue) {
                    Ok(queue) => queue,
                    Err(_) => return,
                };
            }
        };
        let outcome = if job.epoch.load(Ordering::Relaxed) != job.queued_at {
            None
        } else {
            let (status, text) = recognizer.recognize_for(&job.bytes, Some(job.group));
            if status == Status::Unavailable && recognizer.engine_missing() {
                ENGINE_UNAVAILABLE.store(true, Ordering::Relaxed);
            }
            Some((status, text))
        };
        if let Ok(mut results) = shared.results.lock() {
            results.push(JobResult { ticket: job.ticket, outcome });
        }
    }
}

// ---- the recognizer (WinRT) ----

use windows::Globalization::Language;
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapBounds, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform,
    ColorManagementMode, ExifOrientationMode, SoftwareBitmap,
};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
use windows::core::HSTRING;

/// OCR languages that are worth a second pass for non-Latin screenshots, in priority order.
const EXTRA_LANGUAGE_FAMILIES: [&str; 4] = ["ja", "zh", "ko", "ru"];

#[derive(Default)]
pub struct Recognizer {
    initialised: bool,
    primary: Option<OcrEngine>,
    primary_family: String,
    extras: Vec<OcrEngine>,
}

fn language_family(tag: &str) -> String {
    tag.split('-').next().unwrap_or("").to_ascii_lowercase()
}

/// Letters and digits in `text`: the measure of "this engine found real text".
pub fn alnum_count(text: &str) -> usize {
    text.chars().filter(|c| c.is_alphanumeric()).count()
}

impl Recognizer {
    pub fn engine_missing(&self) -> bool {
        self.initialised && self.primary.is_none()
    }

    fn init(&mut self) {
        if self.initialised {
            return;
        }
        self.initialised = true;
        self.primary = OcrEngine::TryCreateFromUserProfileLanguages().ok().or_else(|| {
            let language = Language::CreateLanguage(&HSTRING::from("en-US")).ok()?;
            OcrEngine::TryCreateFromLanguage(&language).ok()
        });
        let Some(primary) = self.primary.as_ref() else { return };
        self.primary_family = primary
            .RecognizerLanguage()
            .and_then(|language| language.LanguageTag())
            .map(|tag| language_family(&tag.to_string()))
            .unwrap_or_default();
        let Ok(available) = OcrEngine::AvailableRecognizerLanguages() else { return };
        let mut tags = Vec::new();
        for language in available {
            if let Ok(tag) = language.LanguageTag() {
                tags.push((language_family(&tag.to_string()), language));
            }
        }
        for family in EXTRA_LANGUAGE_FAMILIES {
            if self.extras.len() >= MAX_EXTRA_ENGINES || family == self.primary_family {
                continue;
            }
            if let Some((_, language)) = tags.iter().find(|(f, _)| f == family)
                && let Ok(engine) = OcrEngine::TryCreateFromLanguage(language)
            {
                self.extras.push(engine);
            }
        }
    }

    /// Decodes and scans one image. `Unavailable` when no engine exists or the image cannot be
    /// decoded (codec missing: WebP/AVIF without the Windows extension), `Skipped` for animated or
    /// too-small images, `Error` for an engine failure.
    fn recognize_for(&mut self, bytes: &[u8], group: Option<Group>) -> (Status, String) {
        let Ok(decoder) = decode(bytes) else { return (Status::Unavailable, String::new()) };
        if decoder.FrameCount().unwrap_or(1) > 1 {
            return (Status::Skipped, String::new());
        }
        let (Ok(width), Ok(height)) = (decoder.OrientedPixelWidth(), decoder.OrientedPixelHeight()) else {
            return (Status::Error, String::new());
        };
        // Retired: there is no perceptual screenshot matching or other visual recognition.
        // The native image adapter is disabled in Bowser; this code is legacy-only.
        if group == Some(Group::YouTube) {
            return (Status::Ok, String::new());
        }
        self.init();
        let Some(primary) = self.primary.clone() else { return (Status::Unavailable, String::new()) };
        let max_dimension = OcrEngine::MaxImageDimension().unwrap_or(10_000);
        let Some(plan) = plan(width, height, max_dimension) else { return (Status::Skipped, String::new()) };
        let try_extras = plan.tiles.len() <= 2
            && u64::from(width) * u64::from(height) <= EXTRA_ENGINE_MAX_PIXELS
            && !self.extras.is_empty();
        let mut text = String::new();
        for (y, band) in &plan.tiles {
            let Ok(bitmap) = tile_bitmap(&decoder, &plan, *y, *band) else {
                return (Status::Error, String::new());
            };
            let Some(mut found) = scan(&primary, &bitmap) else { return (Status::Error, String::new()) };
            if try_extras && alnum_count(&found) < EXTRA_ENGINE_BELOW_ALNUM {
                for engine in &self.extras {
                    if let Some(more) = scan(engine, &bitmap)
                        && alnum_count(&more) > alnum_count(&found)
                    {
                        found = more;
                    }
                }
            }
            if !found.trim().is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(found.trim());
            }
            if text.encode_utf16().count() >= MAX_TEXT_UNITS {
                break;
            }
        }
        (Status::Ok, clip_text(&text))
    }

    #[cfg(test)]
    fn recognize(&mut self, bytes: &[u8]) -> (Status, String) {
        self.recognize_for(bytes, None)
    }
}

fn decode(bytes: &[u8]) -> windows::core::Result<BitmapDecoder> {
    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(bytes)?;
    writer.StoreAsync()?.join()?;
    writer.FlushAsync()?.join()?;
    writer.DetachStream()?;
    stream.Seek(0)?;
    BitmapDecoder::CreateAsync(&stream)?.join()
}

fn tile_bitmap(decoder: &BitmapDecoder, plan: &Plan, y: u32, height: u32) -> windows::core::Result<SoftwareBitmap> {
    let transform = BitmapTransform::new()?;
    transform.SetScaledWidth(plan.scaled_width)?;
    transform.SetScaledHeight(plan.scaled_height)?;
    transform.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
    transform.SetBounds(BitmapBounds { X: 0, Y: y, Width: plan.scaled_width, Height: height })?;
    decoder
        .GetSoftwareBitmapTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Premultiplied,
            &transform,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .join()
}

fn scan(engine: &OcrEngine, bitmap: &SoftwareBitmap) -> Option<String> {
    let result = engine.RecognizeAsync(bitmap).ok()?.join().ok()?;
    result.Text().ok().map(|text| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds() -> OcrCredentials {
        OcrCredentials { hook: format!("__sfw_{}", "0a".repeat(16)), key: "f1".repeat(16) }
    }

    // ---- sites ----

    #[test]
    fn ocr_threads_two_only_on_big_machines() {
        assert_eq!(worker_count(0), 1);
        assert_eq!(worker_count(1), 1);
        assert_eq!(worker_count(4), 1);
        assert_eq!(worker_count(7), 1);
        assert_eq!(worker_count(8), 2);
        assert_eq!(worker_count(64), 2);
    }

    #[test]
    fn image_filters_cover_listed_hosts_and_never_everything() {
        let uris = filter_uris();
        assert!(uris.iter().any(|uri| uri == "*://pbs.twimg.com/*" || uri == "*://*.twimg.com/*"));
        assert!(uris.iter().any(|uri| uri == "*://*.ytimg.com/*") && uris.iter().any(|uri| uri == "*://bsky.app/*"));
        assert!(!uris.iter().any(|uri| uri == "*"), "a `*` image filter would put every image request on the UI thread");
        assert!(uris.iter().all(|uri| uri.starts_with("*://")));
    }

    #[test]
    fn hosts_group_by_registered_domain_and_subdomain() {
        assert_eq!(group_for_host("x.com"), Some(Group::X));
        assert_eq!(group_for_host("PBS.TWIMG.COM"), Some(Group::X));
        assert_eq!(group_for_host("old.reddit.com"), Some(Group::Reddit));
        assert_eq!(group_for_host("i.redd.it"), Some(Group::Reddit));
        assert_eq!(group_for_host("preview.redditmedia.com"), Some(Group::Reddit));
        assert_eq!(group_for_host("scontent.cdninstagram.com"), Some(Group::Meta));
        assert_eq!(group_for_host("scontent-lax3-1.xx.fbcdn.net"), Some(Group::Meta));
        assert_eq!(group_for_host("cdn.bsky.app"), Some(Group::Bluesky));
        assert_eq!(group_for_host("i.ytimg.com"), Some(Group::YouTube));
        assert_eq!(group_for_host("static-cdn.jtvnw.net"), Some(Group::Twitch));
        assert_eq!(group_for_host("media.discordapp.net"), Some(Group::Discord));
        assert_eq!(group_for_host("www.tiktok.com."), Some(Group::TikTok), "trailing dot");
        for (host, group) in [
            ("imgur.com", Group::Imgur),
            ("i.imgur.com", Group::Imgur),
            ("boards.4chan.org", Group::FourChan),
            ("i.4cdn.org", Group::FourChan),
            ("www.gtaforums.com", Group::GtaForums),
            ("www.resetera.com", Group::ResetEra),
            ("web.telegram.org", Group::Telegram),
            ("kick.com", Group::Kick),
            ("files.kick.com", Group::Kick),
            ("www.tumblr.com", Group::Tumblr),
            ("64.media.tumblr.com", Group::Tumblr),
            ("steamcommunity.com", Group::Steam),
            ("steamuserimages-a.akamaihd.net", Group::Steam),
            ("www.rockstargames.com", Group::Rockstar),
            ("www.ign.com", Group::Press),
            ("assets-prd.ignimgs.com", Group::Press),
            ("www.gamespot.com", Group::Press),
            ("kotaku.com", Group::Press),
            ("www.polygon.com", Group::Press),
            ("www.eurogamer.net", Group::Press),
            ("www.gamesradar.com", Group::Press),
            ("www.pcgamer.com", Group::Press),
            ("www.vg247.com", Group::Press),
            ("www.pushsquare.com", Group::Press),
            ("www.purexbox.com", Group::Press),
            ("gamefaqs.gamespot.com", Group::Press),
            ("gta.fandom.com", Group::Fandom),
            ("static.wikia.nocookie.net", Group::Fandom),
        ] {
            assert_eq!(group_for_host(host), Some(group), "{host}");
        }
        assert!(may_serve(group_for_host("i.imgur.com"), group_for_host("imgur.com")));
        assert!(!may_serve(group_for_host("kick.com"), group_for_host("i.imgur.com")));
    }

    #[test]
    fn lookalike_hosts_are_not_grouped() {
        for host in ["notx.com", "x.com.evil.example", "evilreddit.com", "twimg.com.example", "x.co", "", "com", "redd.it.attacker.net", "fbcdn.net.cn", "notimgur.com", "imgur.com.evil.example", "fakegamespot.com", "kick.com.evil.example", "ign.com.attacker.net"] {
            assert_eq!(group_for_host(host), None, "{host}");
        }
    }

    #[test]
    fn urls_group_only_over_http_and_https() {
        assert_eq!(group_for_url("https://pbs.twimg.com/media/a.jpg"), Some(Group::X));
        assert_eq!(group_for_url("http://i.redd.it/a.png"), Some(Group::Reddit));
        assert_eq!(group_for_url("ftp://x.com/a.png"), None);
        assert_eq!(group_for_url("data:image/png;base64,AAAA"), None);
        assert_eq!(group_for_url("blob:https://x.com/abc"), None);
        assert_eq!(group_for_url("not a url"), None);
    }

    #[test]
    fn serving_needs_both_sides_on_the_same_platform() {
        assert!(may_serve(Some(Group::X), Some(Group::X)));
        assert!(!may_serve(Some(Group::X), Some(Group::Reddit)));
        assert!(!may_serve(None, Some(Group::X)));
        assert!(!may_serve(Some(Group::X), None));
        assert!(!may_serve(None, None), "an unlisted page never reads an unlisted image");
        // an unlisted page cannot use Bowser as an image text reader even for listed images
        assert!(!may_serve(group_for_url("https://evil.example/"), group_for_url("https://pbs.twimg.com/a.jpg")));
    }

    #[test]
    fn urls_normalise_to_the_cache_form() {
        assert_eq!(normalize_url("https://pbs.twimg.com/a.jpg#frag").as_deref(), Some("https://pbs.twimg.com/a.jpg"));
        assert_eq!(normalize_url("HTTPS://PBS.TWIMG.COM/a.jpg?x=1").as_deref(), Some("https://pbs.twimg.com/a.jpg?x=1"));
        for bad in [
            "",
            "data:image/png;base64,AAAA",
            "blob:https://x.com/1",
            "javascript:alert(1)",
            "file:///C:/a.png",
            "//x.com/a.png",
            "/a.png",
            "https://user:pw@x.com/a.png",
            "https://x.com/a b.png",
            "https://x.com/a\n.png",
        ] {
            assert_eq!(normalize_url(bad), None, "{bad:?}");
        }
        let long = format!("https://x.com/{}", "a".repeat(MAX_URL_LEN));
        assert_eq!(normalize_url(&long), None);
        let ok = format!("https://x.com/{}", "a".repeat(MAX_URL_LEN - 14));
        assert_eq!(normalize_url(&ok).map(|u| u.len()), Some(MAX_URL_LEN));
    }

    // ---- LRU ----

    fn key(n: usize) -> Key {
        (Group::X, format!("https://pbs.twimg.com/{n}.jpg"))
    }

    #[test]
    fn small_avatar_and_emoji_urls_are_known_small() {
        assert!(known_small("https://pbs.twimg.com/profile_images/123/abc_normal.jpg"));
        assert!(known_small("https://pbs.twimg.com/profile_images/123/abc_bigger.png"));
        assert!(known_small("https://abs.twimg.com/emoji/v2/svg/1f602.svg"));
        assert!(known_small("https://abs.twimg.com/hashflags/x/y.png"));
        assert!(!known_small("https://pbs.twimg.com/profile_images/123/abc_400x400.jpg"));
        assert!(!known_small("https://pbs.twimg.com/profile_images/123/abc.jpg"));
        assert!(!known_small("https://pbs.twimg.com/media/abc_normal?format=jpg"));
        assert!(!known_small("https://example.com/profile_images/abc_normal.jpg"));
    }

    #[test]
    fn parked_bodies_are_taken_once_and_stay_within_the_caps() {
        let mut parked = Parked::new();
        assert!(parked.put(key(1), vec![0; 10]).is_empty());
        assert_eq!(parked.bytes(), 10);
        assert_eq!(parked.take(&key(1)), Some(vec![0; 10]));
        assert_eq!(parked.take(&key(1)), None);
        assert_eq!(parked.bytes(), 0);
        for n in 0..PARKED_CAPACITY + 3 {
            parked.put(key(n), vec![1]);
        }
        assert_eq!(parked.len(), PARKED_CAPACITY);
        assert!(parked.take(&key(0)).is_none(), "the oldest bodies leave first");
        assert!(parked.take(&key(PARKED_CAPACITY + 2)).is_some());
        let dropped = parked.put(key(9999), vec![0; PARKED_BYTES]);
        assert!(!dropped.is_empty());
        assert!(parked.bytes() <= PARKED_BYTES);
        assert_eq!(parked.put(key(7777), vec![0; PARKED_BYTES + 1]), vec![key(7777)], "a body over the cap is not kept");
    }

    #[test]
    fn lru_evicts_the_least_recently_used() {
        let mut lru = Lru::new(3);
        for n in 0..3 {
            lru.insert(key(n), Entry::done(Status::Ok, format!("t{n}")));
        }
        assert!(lru.get(&key(0)).is_some(), "touch 0: now 1 is the oldest");
        lru.insert(key(3), Entry::Pending);
        assert_eq!(lru.len(), 3);
        assert!(lru.get(&key(1)).is_none());
        assert!(lru.get(&key(0)).is_some() && lru.get(&key(2)).is_some() && lru.get(&key(3)).is_some());
    }

    #[test]
    fn lru_insert_replaces_and_stays_bounded() {
        let mut lru = Lru::new(2);
        lru.insert(key(1), Entry::Pending);
        lru.insert(key(1), Entry::done(Status::Ok, "hello"));
        assert_eq!(lru.len(), 1);
        assert_eq!(lru.get(&key(1)), Some(Entry::done(Status::Ok, "hello")));
        for n in 10..1000 {
            lru.insert(key(n), Entry::Pending);
        }
        assert_eq!(lru.len(), 2);
        lru.remove(&key(999));
        assert_eq!(lru.len(), 1);
        lru.clear();
        assert!(lru.is_empty());
    }

    #[test]
    fn lru_keys_separate_sites() {
        let mut lru = Lru::new(8);
        let a = (Group::X, "https://pbs.twimg.com/a.jpg".to_owned());
        let b = (Group::Reddit, "https://pbs.twimg.com/a.jpg".to_owned());
        lru.insert(a.clone(), Entry::done(Status::Ok, "x"));
        assert!(lru.get(&b).is_none());
    }

    // ---- drain parsing ----

    #[test]
    fn a_drain_result_is_a_json_string_holding_an_array() {
        let inner = r#"[{"id":"n1","url":"https://pbs.twimg.com/a.jpg"},{"id":"n2","url":"data:image/png;base64,AA"}]"#;
        let outer = json_string(inner);
        let items = parse_drain(&outer);
        assert_eq!(
            items,
            vec![
                DrainItem { id: "n1".into(), url: Some("https://pbs.twimg.com/a.jpg".into()) },
                DrainItem { id: "n2".into(), url: None },
            ]
        );
        assert_eq!(parse_drain(inner), items, "a bare array is accepted too");
    }

    #[test]
    fn hostile_drain_results_yield_nothing_or_only_the_valid_part() {
        for raw in [
            "", "null", "undefined", "true", "42", "\"\"", "\"not json\"", "{}", "{\"id\":\"a\"}", "[", "[[[[", "]", "\"[\"",
            "[1,2,3]", "[null]", "[\"a\"]", "[{}]", "[{\"id\":1}]", "[{\"id\":\"\"}]", "[{\"id\":\"a b\"}]", "[{\"id\":\"<script>\"}]",
            "[{\"id\":null,\"url\":\"https://x.com/a.png\"}]",
        ] {
            assert!(parse_drain(raw).is_empty(), "{raw:?}");
        }
        let long_id = format!("[{{\"id\":\"{}\"}}]", "a".repeat(MAX_ID_LEN + 1));
        assert!(parse_drain(&long_id).is_empty());
        // a valid item survives next to junk
        let mixed = r#"[3,{"id":"ok1","url":"https://x.com/a.png"},{"id":"bad id"},null]"#;
        assert_eq!(parse_drain(mixed).len(), 1);
        // wrong url types become "no usable url", not a panic
        let odd = r#"[{"id":"u1","url":42},{"id":"u2","url":["https://x.com/a.png"]},{"id":"u3"}]"#;
        let items = parse_drain(odd);
        assert_eq!(items.len(), 3);
        assert!(items.iter().all(|i| i.url.is_none()));
    }

    #[test]
    fn drain_is_capped_in_count_size_and_depth() {
        let many = format!(
            "[{}]",
            (0..200).map(|n| format!("{{\"id\":\"n{n}\",\"url\":\"https://x.com/{n}.png\"}}")).collect::<Vec<_>>().join(",")
        );
        assert_eq!(parse_drain(&many).len(), MAX_DRAIN_ITEMS);
        let huge = format!("[{{\"id\":\"a\",\"url\":\"{}\"}}]", "x".repeat(MAX_DRAIN_CHARS));
        assert!(parse_drain(&huge).is_empty());
        let deep = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
        assert!(parse_drain(&deep).is_empty());
        let deep_object = format!("[{{\"id\":\"a\",\"url\":{}}}]", format!("{}{}", "[".repeat(100), "]".repeat(100)));
        assert_eq!(parse_drain(&deep_object).len(), 0, "depth over the limit is rejected whole");
        let nested_string = json_string(&json_string("[{\"id\":\"a\"}]"));
        assert!(parse_drain(&nested_string).is_empty(), "only one level of string wrapping");
    }

    // ---- delivery ----

    #[test]
    fn delivery_scripts_escape_everything_a_page_could_smuggle() {
        let hostile = "\"; alert(1); //\u{2028}\u{2029}</script><!-- \\ \u{0}\u{1f}\n";
        let scripts = delivery_scripts(
            &creds(),
            &[Delivery { id: "n1".into(), status: Status::Ok, text: hostile.into() }],
        )
        .unwrap();
        assert_eq!(scripts.len(), 1);
        let script = &scripts[0];
        assert!(!script.contains('\u{2028}') && !script.contains('\u{2029}'));
        assert!(!script.contains("</"));
        assert!(!script.contains('\n') && !script.contains('\u{0}'));
        assert!(script.starts_with("(function(){try{var h=window[\"__sfw_"));
        assert!(script.contains(&format!("h.deliver(\"{}\",", creds().key)));
        // The payload is one JavaScript string literal: it closes exactly once, at the end.
        let marker = format!("h.deliver(\"{}\",", creds().key);
        let rest = &script[script.find(&marker).unwrap() + marker.len()..];
        assert!(rest.starts_with('"'));
        let mut escaped = false;
        let mut closed_at = None;
        for (i, c) in rest.char_indices().skip(1) {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                closed_at = Some(i);
                break;
            }
        }
        let closed_at = closed_at.expect("the literal is closed");
        assert_eq!(&rest[closed_at + 1..], ")}catch(e){}})()", "nothing follows the literal but the call's end");
    }

    #[test]
    fn delivery_only_carries_text_for_ok_and_clips_it() {
        let items = [
            Delivery { id: "a".into(), status: Status::Unavailable, text: "leak".into() },
            Delivery { id: "b".into(), status: Status::Ok, text: "Q".repeat(10_000) },
        ];
        let scripts = delivery_scripts(&creds(), &items).unwrap();
        let all = scripts.join("");
        assert!(!all.contains("leak"), "text is sent only with ok");
        let longest = all.split('Q').count() - 1;
        assert_eq!(longest, MAX_TEXT_UNITS);
        assert!(all.contains(r#"\"status\":\"unavailable\""#));
    }

    #[test]
    fn clipping_counts_utf16_units_and_keeps_characters_whole() {
        assert_eq!(clip_text("abc"), "abc");
        let astral = "\u{1F600}".repeat(MAX_TEXT_UNITS);
        let clipped = clip_text(&astral);
        assert_eq!(clipped.encode_utf16().count(), MAX_TEXT_UNITS);
        assert_eq!(clipped.chars().count(), MAX_TEXT_UNITS / 2);
    }

    #[test]
    fn large_deliveries_are_split_under_the_size_cap() {
        let items: Vec<Delivery> = (0..64)
            .map(|n| Delivery { id: format!("n{n}"), status: Status::Ok, text: "w".repeat(MAX_TEXT_UNITS) })
            .collect();
        let scripts = delivery_scripts(&creds(), &items).unwrap();
        assert!(scripts.len() > 1);
        for script in &scripts {
            assert!(script.len() <= MAX_DELIVERY_BYTES + 1024, "{}", script.len());
        }
        let total: usize = scripts.iter().map(|s| s.matches("status").count()).sum();
        assert_eq!(total, 64, "nothing is lost in the split");
    }

    #[test]
    fn malformed_credentials_produce_no_script() {
        let bad = OcrCredentials { hook: "x\"];alert(1);//".into(), key: "k".into() };
        assert!(delivery_scripts(&bad, &[]).is_none());
        assert!(drain_script(&bad).is_none());
        assert!(feed_script(&bad, "{}").is_none());
        assert!(feed_script(&creds(), "[1]").is_none());
        let feed = feed_script(&creds(), "{\"terms\":[\"a\\u2028\"]}").unwrap();
        assert!(feed.contains("h.feed(") && feed.contains(&creds().key) && !feed.contains('\u{2028}'));
        let drain = drain_script(&creds()).unwrap();
        assert!(drain.contains("h.drain(") && drain.contains(&creds().hook));
        assert_eq!(delivery_scripts(&creds(), &[]).unwrap().len(), 0);
    }

    // ---- plan ----

    #[test]
    fn tiny_images_are_skipped() {
        assert_eq!(plan(95, 200, 10_000), None);
        assert_eq!(plan(200, 63, 10_000), None);
        assert!(plan(96, 64, 10_000).is_some());
    }

    #[test]
    fn small_images_are_upscaled_and_big_ones_are_left_alone() {
        let small = plan(200, 100, 10_000).unwrap();
        assert_eq!((small.scaled_width, small.scaled_height), (400, 200));
        assert_eq!(small.tiles, vec![(0, 200)]);
        let normal = plan(1200, 800, 10_000).unwrap();
        assert_eq!((normal.scaled_width, normal.scaled_height), (1200, 800));
        let wide = plan(20_000, 600, 10_000).unwrap();
        assert_eq!(wide.scaled_width, 10_000, "scaled down to the engine limit");
        assert!(wide.scaled_height <= 10_000);
    }

    #[test]
    fn tall_images_are_cut_into_overlapping_bands() {
        let tall = plan(1000, 9000, 10_000).unwrap();
        assert_eq!(tall.scaled_height, 9000);
        assert!(tall.tiles.len() >= 3);
        for pair in tall.tiles.windows(2) {
            let (y0, h0) = pair[0];
            let (y1, _) = pair[1];
            assert!(y1 < y0 + h0, "bands overlap");
        }
        let (y, h) = *tall.tiles.last().unwrap();
        assert_eq!(y + h, 9000, "the last band reaches the bottom");
        let endless = plan(400, 400_000, 10_000).unwrap();
        assert_eq!(endless.tiles.len(), MAX_TILES);
    }

    #[test]
    fn narrow_tall_images_scale_up_without_exceeding_the_limit() {
        let p = plan(120, 4000, 10_000).unwrap();
        assert_eq!(p.scaled_width, 240);
        assert_eq!(p.scaled_height, 8000);
        let q = plan(120, 6000, 10_000).unwrap();
        assert_eq!(q.scaled_height, 10_000, "long edge capped at the engine limit");
        assert!(q.scaled_width < 240);
    }

    // ---- worker queue ----

    #[test]
    fn the_queue_drops_the_oldest_jobs_beyond_its_caps() {
        let shared = Arc::new(Shared {
            queue: Mutex::new(QueueState::default()),
            wake: Condvar::new(),
            results: Mutex::new(Vec::new()),
        });
        let worker = Worker { shared: shared.clone() };
        let epoch = Arc::new(AtomicU64::new(0));
        for ticket in 0..(QUEUE_CAPACITY as u64 + 5) {
            worker.push(Job { ticket, bytes: vec![0; 10], group: Group::X, epoch: epoch.clone(), queued_at: 0 });
        }
        let dropped: Vec<u64> = worker.take_results().into_iter().map(|r| r.ticket).collect();
        assert_eq!(dropped, vec![0, 1, 2, 3, 4]);
        assert_eq!(shared.queue.lock().unwrap().jobs.len(), QUEUE_CAPACITY);
        // byte cap: three 20 MB jobs keep only the newest two
        shared.queue.lock().unwrap().jobs.clear();
        shared.queue.lock().unwrap().bytes = 0;
        for ticket in 100..103u64 {
            worker.push(Job { ticket, bytes: vec![0; 20 * 1024 * 1024], group: Group::X, epoch: epoch.clone(), queued_at: 0 });
        }
        let dropped: Vec<u64> = worker.take_results().into_iter().map(|r| r.ticket).collect();
        assert_eq!(dropped, vec![100]);
        // dropping the Worker would close the queue; the fake has no thread to stop
        shared.queue.lock().unwrap().closed = true;
    }

    // ---- real engine (opens no window) ----

    /// A 1-bit-per-pixel BMP rendered with a minimal built-in 5x7 font would be long; instead the
    /// fixture is generated with GDI (an offscreen DC, no window) so the text is real glyphs.
    fn render_text_bmp(text: &str) -> Vec<u8> {
        use windows::Win32::Graphics::Gdi::*;
        use windows::core::PCWSTR;
        unsafe {
            let (w, h) = (900i32, 160i32);
            let dc = CreateCompatibleDC(None);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let dib = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
            let old = SelectObject(dc, dib.into());
            let pixels = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (w * h) as usize);
            pixels.fill(0x00FF_FFFF);
            let face: Vec<u16> = "Arial\0".encode_utf16().collect();
            let font = CreateFontW(72, 0, 0, 0, 700, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY, 0, PCWSTR(face.as_ptr()));
            let old_font = SelectObject(dc, font.into());
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, windows::Win32::Foundation::COLORREF(0));
            let wide: Vec<u16> = text.encode_utf16().collect();
            let _ = TextOutW(dc, 30, 40, &wide);
            let _ = GdiFlush();
            // BMP file: 54-byte header + top-down 32-bit pixels (negative height)
            let mut out = Vec::new();
            let size = 54 + (w * h * 4) as u32;
            out.extend_from_slice(b"BM");
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&[0, 0, 0, 0]);
            out.extend_from_slice(&54u32.to_le_bytes());
            out.extend_from_slice(&40u32.to_le_bytes());
            out.extend_from_slice(&w.to_le_bytes());
            out.extend_from_slice(&(-h).to_le_bytes());
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&32u16.to_le_bytes());
            out.extend_from_slice(&[0; 24]);
            for px in pixels.iter() {
                out.extend_from_slice(&(px | 0xFF00_0000).to_le_bytes());
            }
            SelectObject(dc, old_font);
            SelectObject(dc, old);
            let _ = DeleteObject(font.into());
            let _ = DeleteObject(dib.into());
            let _ = DeleteDC(dc);
            out
        }
    }

    #[test]
    #[ignore = "needs a Windows OCR language pack; opens no window. cargo test spoiler_ocr -- --ignored --nocapture"]
    fn the_real_engine_reads_the_png_fixture() {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("extensions/spoiler-firewall/test/bowser/fixtures/ocr-text.png");
        let png = std::fs::read(&path).expect("run node extensions/spoiler-firewall/test/bowser/make-ocr-fixture.mjs");
        assert!(png.len() as u64 >= MIN_IMAGE_BYTES, "the fixture passes the size gate");
        let mut recognizer = Recognizer::default();
        let started = std::time::Instant::now();
        let (status, text) = recognizer.recognize(&png);
        eprintln!("OCR png fixture (\"SPOILER TEST 123\") -> {status:?} {text:?} in {:?}", started.elapsed());
        assert_eq!(status, Status::Ok);
        let folded = text.to_ascii_lowercase();
        assert!(folded.contains("spoiler") && folded.contains("test"), "recognised {text:?}");
        let leak = std::fs::read(path.with_file_name("ocr-spoiler.png")).expect("spoiler-shaped fixture");
        let (status, text) = recognizer.recognize(&leak);
        eprintln!("OCR png fixture (\"GTA 6 ENDING LEAK\") -> {status:?} {text:?}");
        assert_eq!(status, Status::Ok);
        assert!(text.to_ascii_lowercase().contains("ending"), "recognised {text:?}");
    }

    #[test]
    #[ignore = "needs a Windows OCR language pack; opens no window. cargo test spoiler_ocr -- --ignored --nocapture"]
    fn the_real_engine_reads_a_generated_image() {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let fixture = "Hello Spoiler Test 123";
        let bmp = render_text_bmp(fixture);
        let mut recognizer = Recognizer::default();
        let started = std::time::Instant::now();
        let (status, text) = recognizer.recognize(&bmp);
        let first = started.elapsed();
        let started = std::time::Instant::now();
        let (status2, text2) = recognizer.recognize(&bmp);
        let second = started.elapsed();
        eprintln!("OCR fixture {fixture:?} -> {status:?} {text:?} (first {first:?}, warm {second:?}); again {status2:?} {text2:?}");
        assert_eq!(status, Status::Ok, "engine available");
        let folded = text.to_ascii_lowercase();
        assert!(folded.contains("hello") && folded.contains("spoiler"), "recognised {text:?}");
        // an all-white image has no text
        let blank = {
            let mut b = bmp.clone();
            for px in b[54..].chunks_exact_mut(4) {
                px.copy_from_slice(&[255, 255, 255, 255]);
            }
            b
        };
        let (blank_status, blank_text) = recognizer.recognize(&blank);
        assert_eq!(blank_status, Status::Ok);
        assert!(blank_text.trim().is_empty(), "{blank_text:?}");
        // garbage is not an image: unavailable, never a panic
        assert_eq!(recognizer.recognize(b"definitely not an image").0, Status::Unavailable);
    }
}
