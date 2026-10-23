//! Downloads core (docs/design/downloads.md): the list of downloads, their naming, persistence and
//! the JSON the chrome shows. Pure model: no WebView2, no windows, no clock (time is passed in), so
//! every rule here is unit-tested. The engine side is `engine::download`, the shell side
//! (Save/Folder dialogs, Open/Show in folder) is `download_shell`.
//!
//! # Core API (what the chrome bridge calls)
//!
//! Owner: `App` holds one [`DownloadManager`] (`App::downloads`); a guest window's manager is memory
//! only and a profile whose `downloads.txt` cannot be read is in [`ListState::Error`] (never
//! rewritten). Time is injected: `now_ms` is Unix milliseconds, [`Time`] is Unix seconds plus the
//! local offset (`win32::history_local_offset()`).
//!
//! - Engine -> manager: [`DownloadManager::on_event`] with a [`DownloadEvent`] (`Started`,
//!   `Progress`, `State`). Returns the [`Change`] (`None`, `Progress` = bytes moved only,
//!   `State` = anything the list shows differently). `App` does this for the bridge.
//! - Change signal: [`DownloadManager::revision`] grows on every mutation; `App` also calls the
//!   observer set with `App::set_downloads_observer` with the [`Change`] after each one.
//! - Toolbar button: [`DownloadManager::snapshot_for_toolbar`] and [`Toolbar::json`]
//!   (`{v:1,t:"dlbtn",..}`). [`DownloadManager::mark_seen`] when the bubble opens and again when it
//!   closes (an entry that finished while it was open is unseen until then).
//! - Bubble: [`DownloadManager::snapshot_for_bubble`] (`{v:1,t:"downloads",tok,auto,items,more,active}`).
//! - Page: [`DownloadManager::snapshot_for_page`] (`{v:1,t:"snap",p:"downloadspage",d:{..}}`), with the
//!   page's query and the answer to its last change ([`Ack`]). [`DownloadManager::refresh_missing`]
//!   (or `App::check_download_files`) first, so a deleted file reads `missing`.
//! - Actions: [`Action::from_code`] maps the wire number; [`DownloadManager::act`] checks the id and
//!   that the action is legal for the entry's state and returns an [`Outcome`]. `App::download_act`
//!   performs the Outcome (engine pause/resume/cancel, open file, show in folder, open folder) and
//!   leaves what only the bridge can do (open the page, open a tab, copy text) as a `Followup`.
//! - Removal: [`DownloadManager::remove`], [`DownloadManager::clear_finished`],
//!   [`DownloadManager::undo`] / [`DownloadManager::can_undo`].
//!
//! # Files
//!
//! `downloads.txt` (profile folder): a `#next<TAB>n` line (the id high-water mark, so ids stay
//! monotonic after entries are removed) then one tab-separated line per entry, oldest first. Only
//! the newest [`MAX_ENTRIES`] are kept. Written on every state change (start, pause, finish, remove),
//! never on progress, atomically. Entries active when the file is read become failed `Interrupted`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::chrome_host::json::{capped, Json, Object};

/// Entries kept in memory and on disk.
pub const MAX_ENTRIES: usize = 500;
/// Rows the bubble lists.
pub const BUBBLE_ROWS: usize = 5;
/// Largest `downloads.txt` that is read at all.
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// Largest page snapshot sent (the page refuses inbound messages over 256 KiB).
pub const MAX_SNAPSHOT_BYTES: usize = 240 * 1024;
/// Longest file name Bowser creates (UTF-16 units), before the folder's own length is considered.
pub const MAX_NAME_UNITS: usize = 150;
const MAX_NAME_JSON: usize = 200;
const MAX_HOST_JSON: usize = 120;
const MAX_URL_JSON: usize = 200;
const MAX_ERR_CHARS: usize = 60;
/// Longest source URL kept (longer ones are not recorded: they could not be fetched again anyway).
const MAX_URL_BYTES: usize = 2048;
/// Longest search the page sends.
pub const MAX_QUERY_CHARS: usize = 256;
/// Speed smoothing: the time constant of the moving average, and the shortest span measured.
const SPEED_TAU_MS: f64 = 3000.0;
const SPEED_MIN_SPAN_MS: u64 = 250;
/// How long a file check stays valid for `refresh_missing_if_stale`.
const FILE_CHECK_TTL_MS: u64 = 2000;

// ---------------------------------------------------------------------------------------------
// Engine <-> core types
// ---------------------------------------------------------------------------------------------

/// What the engine reports about one download. `key` is the engine's own handle for the operation
/// (unique per process); the manager maps it to an entry id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadEvent {
    /// A download began saving to `path`. `total` is -1 when the size is unknown.
    Started { key: u64, url: String, path: PathBuf, total: i64 },
    /// Bytes moved (throttled by the engine to about four a second).
    Progress { key: u64, got: u64, total: i64 },
    /// The operation changed state. `err` is a short reason for `Failed`, else empty.
    State { key: u64, state: OpState, got: u64, total: i64, err: String, can_resume: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpState {
    InProgress,
    Paused,
    Completed,
    Cancelled,
    Failed,
}

/// What the engine can do to a running download.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineCommand {
    Pause,
    Resume,
    Cancel,
}

/// A download the engine is about to start, for the destination decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadRequest {
    pub url: String,
    /// The name WebView2 would use (from Content-Disposition or the URL), unsanitised.
    pub suggested_name: String,
    pub mime: String,
    pub total: i64,
}

/// The deliberately dead host the Chrome Web Store's own "Add to Chrome" flow points its download
/// at. `.invalid` is reserved and never resolves, so nothing can ever be saved from it.
pub const STORE_PLACEHOLDER_HOST: &str = "permanently-removed.invalid";

/// True when `url` is on exactly [`STORE_PLACEHOLDER_HOST`] (case-insensitively, no trailing dot, no
/// look-alike suffix or prefix). Such a download can never succeed, so it is never recorded and a
/// recorded one is never retried or resumed.
pub fn is_store_placeholder_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| url.host_str() == Some(STORE_PLACEHOLDER_HOST))
}

/// The extension id of the Web Store's installer placeholder download, or `None` for anything else:
/// `https://permanently-removed.invalid/?...prod=chromiumcrx...&x=id%3D<32 a-p chars>%26installsource%3Dondemand...`.
/// Strict on purpose: exact host, `https`, no userinfo or port, one `prod=chromiumcrx` and one `x`
/// holding exactly one `id` of 32 `a`-`p` characters. The id is only a lead for Bowser's own,
/// separately validated install flow; it never installs anything by itself.
pub fn store_placeholder_id(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some(STORE_PLACEHOLDER_HOST)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let only = |wanted: &str| {
        let mut found = url.query_pairs().filter(|(key, _)| key == wanted);
        let first = found.next()?.1.into_owned();
        found.next().is_none().then_some(first)
    };
    if only("prod")? != "chromiumcrx" {
        return None;
    }
    let inner = only("x")?;
    let mut ids = url::form_urlencoded::parse(inner.as_bytes()).filter(|(key, _)| key == "id");
    let id = ids.next()?.1.into_owned();
    if ids.next().is_some() {
        return None;
    }
    crate::crx_fetch::parse_extension_id(&id).ok()
}

/// What a Retry of a recorded download should do with `url`: open it again, open the extension's Web
/// Store listing (where Bowser's own "Add to Bowser" button lives) when it was the Store's installer
/// placeholder, or nothing when it is a placeholder without a usable id.
fn retry_target(url: &str) -> Option<String> {
    match (is_store_placeholder_url(url), store_placeholder_id(url)) {
        (false, _) => Some(url.to_owned()),
        (true, Some(id)) => Some(format!("https://chromewebstore.google.com/detail/{id}")),
        (true, None) => None,
    }
}

/// Where a download goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadPlan {
    /// Save here, with no prompt.
    Save { path: PathBuf },
    /// Show the Save dialog first, in `dir`, with `name` preselected.
    Ask { dir: PathBuf, name: String },
    /// Do not download (no usable folder).
    Cancel,
}

// ---------------------------------------------------------------------------------------------
// Entries
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Active,
    Paused,
    Done,
    Failed,
    Cancelled,
}

impl State {
    fn key(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "active" => Self::Active,
            "paused" => Self::Paused,
            "done" => Self::Done,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }

    /// Still moving or waiting to move.
    pub fn in_progress(self) -> bool {
        matches!(self, Self::Active | Self::Paused)
    }
}

/// Moving average of the transfer rate. Samples closer than [`SPEED_MIN_SPAN_MS`] are folded into
/// the next one, so a burst of tiny progress events does not make the figure jump.
#[derive(Clone, Debug, Default, PartialEq)]
struct Speed {
    last_ms: u64,
    last_got: u64,
    bps: f64,
}

impl Speed {
    fn reset(&mut self, now_ms: u64, got: u64) {
        *self = Self { last_ms: now_ms, last_got: got, bps: 0.0 };
    }

    fn sample(&mut self, now_ms: u64, got: u64) {
        if self.last_ms == 0 || now_ms < self.last_ms || got < self.last_got {
            self.reset(now_ms, got);
            return;
        }
        let span = now_ms - self.last_ms;
        if span < SPEED_MIN_SPAN_MS {
            return;
        }
        let instant = (got - self.last_got) as f64 * 1000.0 / span as f64;
        self.bps = if self.bps <= 0.0 {
            instant
        } else {
            let alpha = 1.0 - (-(span as f64) / SPEED_TAU_MS).exp();
            self.bps + alpha * (instant - self.bps)
        };
        self.last_ms = now_ms;
        self.last_got = got;
    }
}

/// Seconds left at `bps`, or -1 when it cannot be said.
pub fn eta_secs(got: u64, total: i64, bps: u64) -> i64 {
    if total <= 0 || bps == 0 {
        return -1;
    }
    let left = (total as u64).saturating_sub(got);
    left.div_ceil(bps).min(i64::MAX as u64) as i64
}

#[derive(Clone, Debug, PartialEq)]
pub struct DownloadEntry {
    pub id: u32,
    pub name: String,
    /// The source URL, http(s) only (empty for anything else, e.g. a blob: download).
    pub url: String,
    pub path: PathBuf,
    pub state: State,
    /// A finished download whose file is gone (shown as state `missing`).
    pub missing: bool,
    pub got: u64,
    /// -1 when unknown.
    pub total: i64,
    pub started: u64,
    /// 0 until it ends.
    pub ended: u64,
    pub err: String,
    /// The engine can resume it (needs `op`).
    pub resume: bool,
    /// The engine's handle while the operation is alive. Not persisted.
    pub op: Option<u64>,
    /// Finished or failed since the bubble was last seen. Not persisted.
    pub unseen: bool,
    speed: Speed,
}

impl DownloadEntry {
    /// Bytes per second now (0 unless active).
    pub fn bps(&self) -> u64 {
        if self.state == State::Active { self.speed.bps.max(0.0) as u64 } else { 0 }
    }

    /// Seconds left (-1 when unknown or not active).
    pub fn eta(&self) -> i64 {
        if self.state == State::Active { eta_secs(self.got, self.total, self.bps()) } else { -1 }
    }

    /// The state the page shows: a finished download whose file is gone reads `missing`.
    pub fn state_key(&self) -> &'static str {
        if self.state == State::Done && self.missing { "missing" } else { self.state.key() }
    }

    /// The source host without a leading `www.`; empty when there is no http(s) source.
    pub fn host(&self) -> String {
        host_of(&self.url)
    }

    /// The moment shown as the row's clock: when it ended, else when it started.
    fn clock_secs(&self) -> u64 {
        if self.ended > 0 { self.ended } else { self.started }
    }
}

/// The text "Copy link" puts on the clipboard: the recorded source, only when it is an http(s) URL.
pub fn copy_link_text(url: &str) -> Option<String> {
    let clean = clean_source_url(url);
    (!clean.is_empty()).then_some(clean)
}

fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .map(|h| h.strip_prefix("www.").unwrap_or(&h).to_owned())
        .unwrap_or_default()
}

/// The recorded source: http(s) only, no credentials, bounded. Anything else is not recorded.
fn clean_source_url(url: &str) -> String {
    if url.len() > MAX_URL_BYTES {
        return String::new();
    }
    match url::Url::parse(url) {
        Ok(mut parsed) if matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some() => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.to_string()
        }
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------------------------
// Names and folders
// ---------------------------------------------------------------------------------------------

/// Whether Windows reads the file name `name` as a device. Only the part before the first dot
/// counts, with the spaces Windows ignores at its end ("con", "con.txt" and "CON .txt" are all the
/// console): CON, PRN, AUX, NUL, CONIN$, CONOUT$, CLOCK$, or COM / LPT followed by one digit 1-9 or
/// a superscript 1, 2 or 3 (`COM¹`). The download and capture name cleaners share it.
pub fn is_reserved_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end_matches([' ', '\u{a0}']).to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$") {
        return true;
    }
    let Some(number) = stem.strip_prefix("COM").or_else(|| stem.strip_prefix("LPT")) else { return false };
    let mut chars = number.chars();
    matches!((chars.next(), chars.next()), (Some('1'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None))
}

fn invalid_name_char(c: char) -> bool {
    c.is_control()
        || "<>:\"/\\|?*".contains(c)
        // Direction overrides and zero-width characters make "photo\u{202e}gpj.exe" read as a picture.
        || ('\u{200b}'..='\u{200f}').contains(&c)
        || ('\u{202a}'..='\u{202e}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
        || c == '\u{feff}'
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn truncate_utf16(text: &str, max_units: usize) -> &str {
    let mut units = 0;
    for (at, c) in text.char_indices() {
        units += c.len_utf16();
        if units > max_units {
            return &text[..at];
        }
    }
    text
}

/// Splits `name` into stem and extension (with its dot). A leading dot alone (`.gitignore`) is not an
/// extension, nor is a "dot part" that is long or holds spaces. `.tar.gz` and its kin are one extension.
pub fn split_extension(name: &str) -> (&str, &str) {
    let Some(dot) = name.rfind('.') else { return (name, "") };
    if dot == 0 {
        return (name, "");
    }
    let ext = &name[dot + 1..];
    if ext.is_empty() || ext.chars().count() > 16 || ext.chars().any(char::is_whitespace) {
        return (name, "");
    }
    let stem = &name[..dot];
    let compressed = ["gz", "bz2", "xz", "z", "zst", "lz", "lzma", "bz"];
    if compressed.contains(&ext.to_ascii_lowercase().as_str())
        && stem.len() > 4
        && stem.is_char_boundary(stem.len() - 4)
        && stem[stem.len() - 4..].eq_ignore_ascii_case(".tar")
    {
        return (&name[..dot - 4], &name[dot - 4..]);
    }
    (stem, &name[dot..])
}

/// A file name that is safe to create: one path component, no characters Windows rejects, no
/// reserved device name, no trailing dot or space, at most [`MAX_NAME_UNITS`] UTF-16 units with the
/// extension kept. Never empty (`download` stands in).
pub fn sanitize_file_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base.chars().map(|c| if invalid_name_char(c) { '_' } else { c }).collect();
    let trimmed = cleaned.trim_start().trim_end_matches(['.', ' ', '\u{a0}']);
    let mut name = if trimmed.is_empty() { "download".to_owned() } else { trimmed.to_owned() };
    if is_reserved_device_name(&name) {
        name.insert(0, '_');
    }
    fit_name(&name, MAX_NAME_UNITS)
}

/// `name` cut to `max_units` UTF-16 units, keeping the extension when it leaves room for a stem.
fn fit_name(name: &str, max_units: usize) -> String {
    if utf16_len(name) <= max_units {
        return name.to_owned();
    }
    let (stem, ext) = split_extension(name);
    let ext_units = utf16_len(ext);
    if ext_units + 8 > max_units {
        return truncate_utf16(name, max_units).trim_end_matches(['.', ' ']).to_owned();
    }
    let stem = truncate_utf16(stem, max_units - ext_units).trim_end_matches(['.', ' ']);
    format!("{stem}{ext}")
}

/// The longest name that still fits a path in `dir` (Windows' 259 characters), with room for the
/// ` (NNNN)` suffix a clash adds. Never below 24 or above [`MAX_NAME_UNITS`].
pub fn name_budget(dir: &Path) -> usize {
    let used = utf16_len(&dir.to_string_lossy()) + 1 + 7;
    259usize.saturating_sub(used).clamp(24, MAX_NAME_UNITS)
}

/// `name` if it is free, else `stem (1).ext`, `stem (2).ext`, ... (Chrome's rule: the number goes
/// before the extension, and `.tar.gz` counts as one). `taken` says whether a path is in use.
pub fn unique_path(dir: &Path, name: &str, taken: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let name = fit_name(name, name_budget(dir));
    let first = dir.join(&name);
    if !taken(&first) {
        return Some(first);
    }
    let (stem, ext) = split_extension(&name);
    for n in 1..=9999u32 {
        let suffix = format!(" ({n})");
        let room = name_budget(dir).saturating_sub(utf16_len(ext) + suffix.len());
        let stem = truncate_utf16(stem, room.max(1)).trim_end_matches(['.', ' ']);
        let candidate = dir.join(format!("{stem}{suffix}{ext}"));
        if !taken(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// The folder downloads go to: `setting` when it is an absolute path that exists or can be created,
/// else `default_dir` (created if need be). `Err` only when neither can be used.
pub fn resolve_dir(setting: &str, default_dir: Option<&Path>) -> std::io::Result<PathBuf> {
    let usable = |path: &Path| path.is_absolute() && std::fs::create_dir_all(path).is_ok() && path.is_dir();
    let custom = setting.trim();
    if !custom.is_empty() && usable(Path::new(custom)) {
        return Ok(PathBuf::from(custom));
    }
    match default_dir {
        Some(dir) if usable(dir) => Ok(dir.to_path_buf()),
        _ => Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no usable download folder")),
    }
}

/// Decides where a download goes: the Save dialog when `ask`, else the first free name in `dir`.
/// `taken` is whether a path exists on disk or belongs to a download still in progress.
pub fn plan_destination(dir: &Path, ask: bool, suggested: &str, taken: &dyn Fn(&Path) -> bool) -> DownloadPlan {
    let name = sanitize_file_name(suggested);
    if ask {
        return DownloadPlan::Ask { dir: dir.to_path_buf(), name: fit_name(&name, name_budget(dir)) };
    }
    match unique_path(dir, &name, taken) {
        Some(path) => DownloadPlan::Save { path },
        None => DownloadPlan::Cancel,
    }
}

// ---------------------------------------------------------------------------------------------
// Toolbar, actions, answers
// ---------------------------------------------------------------------------------------------

/// What the toolbar icon shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonState {
    Idle,
    Active,
    Paused,
    Done,
    Failed,
}

impl ButtonState {
    pub fn key(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}

/// `{v:1,t:"dlbtn",on,state,n,pct,burst}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Toolbar {
    pub on: bool,
    pub state: ButtonState,
    /// Downloads in progress (active and paused).
    pub n: u32,
    /// Aggregate progress 0-100, -1 when no size is known.
    pub pct: i32,
    /// Counts every start; the page plays its start animation when it changes.
    pub burst: u32,
}

impl Toolbar {
    pub fn json(&self) -> String {
        Json::object(|o| {
            o.int("v", 1);
            o.string("t", "dlbtn");
            o.bool("on", self.on);
            o.string("state", self.state.key());
            o.uint("n", u64::from(self.n));
            o.int("pct", i64::from(self.pct));
            o.uint("burst", u64::from(self.burst));
        })
    }
}

/// What changed, so the window can post a refresh at the right urgency.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Change {
    #[default]
    None,
    /// Bytes moved only: throttle the refresh (about four a second).
    Progress,
    /// Something the list or button shows differently: refresh at once.
    State,
}

/// The actions the bubble and page send, by wire number (`dlact.n`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Pause,
    Resume,
    Cancel,
    Open,
    Show,
    Remove,
    CopyLink,
    Retry,
    OpenPage,
    OpenFolder,
    ClearAll,
}

impl Action {
    pub fn from_code(n: u8) -> Option<Self> {
        Some(match n {
            0 => Self::Pause,
            1 => Self::Resume,
            2 => Self::Cancel,
            3 => Self::Open,
            4 => Self::Show,
            5 => Self::Remove,
            6 => Self::CopyLink,
            7 => Self::Retry,
            8 => Self::OpenPage,
            9 => Self::OpenFolder,
            10 => Self::ClearAll,
            _ => return None,
        })
    }

    /// Whether the action names no entry (id 0).
    pub fn is_list_level(self) -> bool {
        matches!(self, Self::OpenPage | Self::OpenFolder | Self::ClearAll)
    }
}

/// What [`DownloadManager::act`] decided. The caller performs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Unknown id or an action that is not legal for the entry's state.
    Rejected,
    /// The list changed (removed, cleared); nothing else to do.
    Changed,
    /// Tell the engine about operation `key`.
    Engine { key: u64, command: EngineCommand },
    /// Open the recorded file with the shell, after checking it is still a regular file.
    OpenFile(PathBuf),
    /// Show the recorded file in Explorer.
    ShowInFolder(PathBuf),
    /// Open the download folder.
    OpenFolder,
    /// Open `bowser://downloads`.
    OpenPage,
    /// Put this recorded http(s) URL on the clipboard.
    CopyLink(String),
    /// Open this recorded http(s) URL in a new tab.
    Retry(String),
}

/// The answer to the page's last change, sent once with the snapshot that follows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ack {
    #[default]
    None,
    Removed,
    Restored,
    Cleared,
    Failed,
}

impl Ack {
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Removed => "removed",
            Self::Restored => "restored",
            Self::Cleared => "cleared",
            Self::Failed => "failed",
        }
    }
}

/// How the page's list is backed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListState {
    /// Saved in the profile.
    On,
    /// `downloads.txt` exists but could not be read; the list is memory only and the file is left alone.
    Error,
    /// A guest window: memory only by design.
    Guest,
}

impl ListState {
    pub fn key(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Error => "error",
            Self::Guest => "guest",
        }
    }
}

/// The wall-clock context of a snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Time {
    /// Unix seconds.
    pub now: u64,
    /// Local time minus UTC, in seconds.
    pub offset_secs: i64,
}

enum Store {
    Memory,
    File(PathBuf),
    /// The file could not be read: never written.
    Unreadable,
}

// ---------------------------------------------------------------------------------------------
// The manager
// ---------------------------------------------------------------------------------------------

pub struct DownloadManager {
    /// Oldest first; ids ascend.
    entries: Vec<DownloadEntry>,
    next_id: u32,
    store: Store,
    guest: bool,
    /// The last removal (one entry, or everything a clear took), for undo.
    removed: Vec<DownloadEntry>,
    burst: u32,
    /// Operation handles the engine should let go of (an entry that held one was removed or trimmed).
    released: Vec<u64>,
    revision: u64,
    last_file_check_ms: Option<u64>,
    /// Other windows of the profile are other processes writing the same file, so a save merges
    /// with what is on disk. Ids this process is the authority for (it started them, or put them
    /// back with undo): its copy wins. Every other entry is the file's.
    authored: HashSet<u32>,
    /// Ids that were in the file when this process last read or wrote it: one missing since then
    /// was removed by another window.
    synced: HashSet<u32>,
    /// Ids this process removed: never written back from the file.
    dropped: HashSet<u32>,
}

impl DownloadManager {
    /// A list that is never saved (a guest window, or a profile without a usable folder).
    pub fn in_memory(guest: bool) -> Self {
        Self::blank(Store::Memory, guest)
    }

    fn blank(store: Store, guest: bool) -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            store,
            guest,
            removed: Vec::new(),
            burst: 0,
            released: Vec::new(),
            revision: 0,
            last_file_check_ms: None,
            authored: HashSet::new(),
            synced: HashSet::new(),
            dropped: HashSet::new(),
        }
    }

    /// Loads the profile's list. A missing file is an empty list; a file that exists but cannot be
    /// read gives [`ListState::Error`] and is never written. Entries that were active when the file
    /// was last written become failed `Interrupted`.
    pub fn open(path: &Path) -> Self {
        match read_file(path) {
            Ok((entries, next_id)) => {
                let mut manager = Self::blank(Store::File(path.to_path_buf()), false);
                manager.synced = entries.iter().map(|e| e.id).collect();
                manager.entries = entries;
                manager.next_id = next_id;
                manager
            }
            Err(_) => Self::blank(Store::Unreadable, false),
        }
    }

    pub fn list_state(&self) -> ListState {
        match (&self.store, self.guest) {
            (_, true) => ListState::Guest,
            (Store::Unreadable, _) => ListState::Error,
            _ => ListState::On,
        }
    }

    pub fn entries(&self) -> &[DownloadEntry] {
        &self.entries
    }

    pub fn get(&self, id: u32) -> Option<&DownloadEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Grows on every mutation: poll it, or use the observer `App` offers.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The next id that will be handed out (the persisted high-water mark).
    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    /// Downloads in progress (active and paused).
    pub fn in_progress_count(&self) -> usize {
        self.entries.iter().filter(|e| e.state.in_progress()).count()
    }

    /// Whether `path` is the destination of a download still in progress (so a new one must not take it).
    pub fn path_in_use(&self, path: &Path) -> bool {
        let wanted = path.to_string_lossy().to_lowercase();
        self.entries.iter().any(|e| e.state.in_progress() && e.path.to_string_lossy().to_lowercase() == wanted)
    }

    /// Operation handles the engine may drop now. Call after any mutation.
    pub fn take_released(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.released)
    }

    fn touch(&mut self) {
        self.revision += 1;
    }

    fn index_of_key(&self, key: u64) -> Option<usize> {
        self.entries.iter().position(|e| e.op == Some(key))
    }

    // --- engine events ---

    pub fn on_event(&mut self, event: DownloadEvent, now_ms: u64) -> Change {
        let change = match event {
            DownloadEvent::Started { key, url, path, total } => self.started(key, &url, path, total, now_ms),
            DownloadEvent::Progress { key, got, total } => self.progress(key, got, total, now_ms),
            DownloadEvent::State { key, state, got, total, err, can_resume } => {
                self.state_changed(key, state, got, total, &err, can_resume, now_ms)
            }
        };
        if change != Change::None {
            self.touch();
        }
        if change == Change::State {
            self.save();
        }
        change
    }

    fn started(&mut self, key: u64, url: &str, path: PathBuf, total: i64, now_ms: u64) -> Change {
        if self.index_of_key(key).is_some() {
            return Change::None;
        }
        // Another window may have handed out ids since this list was read.
        if let Store::File(file) = &self.store
            && let Ok((_, next)) = read_file_as(file, false)
        {
            self.next_id = self.next_id.max(next);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.authored.insert(id);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "download".to_owned());
        let mut speed = Speed::default();
        speed.reset(now_ms.max(1), 0);
        self.entries.push(DownloadEntry {
            id,
            name,
            url: clean_source_url(url),
            path,
            state: State::Active,
            missing: false,
            got: 0,
            total: total.max(-1),
            started: now_ms / 1000,
            ended: 0,
            err: String::new(),
            resume: false,
            op: Some(key),
            unseen: false,
            speed,
        });
        self.burst = self.burst.wrapping_add(1);
        self.trim();
        Change::State
    }

    fn progress(&mut self, key: u64, got: u64, total: i64, now_ms: u64) -> Change {
        let Some(at) = self.index_of_key(key) else { return Change::None };
        let entry = &mut self.entries[at];
        if entry.state != State::Active {
            return Change::None;
        }
        let was = (entry.got, entry.total);
        entry.got = got;
        entry.total = total.max(-1);
        entry.speed.sample(now_ms, got);
        if was == (entry.got, entry.total) { Change::None } else { Change::Progress }
    }

    #[allow(clippy::too_many_arguments)]
    fn state_changed(
        &mut self,
        key: u64,
        state: OpState,
        got: u64,
        total: i64,
        err: &str,
        can_resume: bool,
        now_ms: u64,
    ) -> Change {
        let Some(at) = self.index_of_key(key) else { return Change::None };
        let entry = &mut self.entries[at];
        entry.got = got;
        entry.total = total.max(-1);
        let now = now_ms / 1000;
        match state {
            OpState::InProgress => {
                entry.state = State::Active;
                entry.err.clear();
                entry.resume = false;
                entry.ended = 0;
                entry.speed.reset(now_ms.max(1), got);
            }
            OpState::Paused => {
                entry.state = State::Paused;
                entry.err.clear();
                entry.resume = true;
                entry.speed.reset(0, got);
            }
            OpState::Completed => {
                entry.state = State::Done;
                if entry.total > 0 {
                    entry.got = entry.total as u64;
                } else {
                    entry.total = got as i64;
                }
                entry.ended = now;
                entry.op = None;
                entry.resume = false;
                entry.unseen = true;
            }
            OpState::Cancelled => {
                entry.state = State::Cancelled;
                entry.ended = now;
                entry.op = None;
                entry.resume = false;
            }
            OpState::Failed => {
                let reason = if err.trim().is_empty() { "Failed" } else { err.trim() };
                entry.state = State::Failed;
                entry.err = capped(reason, MAX_ERR_CHARS).to_owned();
                entry.ended = now;
                entry.resume = can_resume;
                if !can_resume {
                    entry.op = None;
                }
                entry.unseen = true;
            }
        }
        Change::State
    }

    /// Keeps the newest [`MAX_ENTRIES`], never dropping one in progress.
    fn trim(&mut self) {
        while self.entries.len() > MAX_ENTRIES {
            let Some(at) = self.entries.iter().position(|e| !e.state.in_progress()) else { break };
            let gone = self.entries.remove(at);
            self.released.extend(gone.op);
            self.dropped.insert(gone.id);
        }
    }

    /// Marks every download in progress failed `Interrupted` and saves: the window is closing and
    /// WebView2 operations cannot outlive their controller. Returns the handles to cancel.
    pub fn shutdown(&mut self, now_ms: u64) -> Vec<u64> {
        let mut keys = Vec::new();
        for entry in &mut self.entries {
            keys.extend(entry.op.take());
            if entry.state.in_progress() {
                interrupt(entry, now_ms);
            }
        }
        if !keys.is_empty() {
            self.touch();
        }
        self.save();
        keys
    }

    /// Fails the downloads whose operations `keys` named (their webview closed under them).
    pub fn interrupt_keys(&mut self, keys: &[u64], now_ms: u64) -> Change {
        let mut change = Change::None;
        for key in keys {
            let Some(at) = self.index_of_key(*key) else { continue };
            let entry = &mut self.entries[at];
            entry.op = None;
            entry.resume = false;
            if entry.state.in_progress() {
                interrupt(entry, now_ms);
            }
            change = Change::State;
        }
        if change != Change::None {
            self.touch();
            self.save();
        }
        change
    }

    // --- seen / toolbar ---

    /// The bubble was opened (or closed): finished downloads stop asking for attention. True if any did.
    pub fn mark_seen(&mut self) -> bool {
        let mut changed = false;
        for entry in &mut self.entries {
            changed |= std::mem::replace(&mut entry.unseen, false);
        }
        if changed {
            self.touch();
        }
        changed
    }

    pub fn snapshot_for_toolbar(&self) -> Toolbar {
        let active = self.entries.iter().filter(|e| e.state == State::Active).count() as u32;
        let paused = self.entries.iter().filter(|e| e.state == State::Paused).count() as u32;
        let (mut got, mut total) = (0u64, 0u64);
        for entry in self.entries.iter().filter(|e| e.state.in_progress() && e.total > 0) {
            got += entry.got.min(entry.total as u64);
            total += entry.total as u64;
        }
        let pct = (got * 100).checked_div(total).map_or(-1, |pct| pct.min(100) as i32);
        let latest_unseen = self
            .entries
            .iter()
            .filter(|e| e.unseen && matches!(e.state, State::Done | State::Failed))
            .max_by_key(|e| (e.ended, e.id));
        let state = if active > 0 {
            ButtonState::Active
        } else if paused > 0 {
            ButtonState::Paused
        } else {
            match latest_unseen.map(|e| e.state) {
                Some(State::Failed) => ButtonState::Failed,
                Some(_) => ButtonState::Done,
                None => ButtonState::Idle,
            }
        };
        Toolbar { on: state != ButtonState::Idle, state, n: active + paused, pct, burst: self.burst }
    }

    // --- queries and snapshots ---

    /// Entries matching `query` (case-insensitive substring of name or URL), newest first.
    pub fn search(&self, query: &str) -> Vec<&DownloadEntry> {
        let q = query.trim().to_lowercase();
        let mut found: Vec<&DownloadEntry> = self
            .entries
            .iter()
            .filter(|e| q.is_empty() || e.name.to_lowercase().contains(&q) || e.url.to_lowercase().contains(&q))
            .collect();
        found.sort_by_key(|e| std::cmp::Reverse((e.started, e.id)));
        found
    }

    /// The rows the bubble lists: in progress first, then newest, at most `max`; and how many more exist.
    pub fn bubble_rows(&self, max: usize) -> (Vec<&DownloadEntry>, usize) {
        let mut rows: Vec<&DownloadEntry> = self.entries.iter().collect();
        rows.sort_by_key(|e| (!e.state.in_progress(), std::cmp::Reverse(e.id)));
        let more = rows.len().saturating_sub(max);
        rows.truncate(max);
        (rows, more)
    }

    /// `{v:1,t:"downloads",tok,auto,items,more,active}`.
    pub fn snapshot_for_bubble(&self, tok: u32, auto: bool, max: usize, time: Time) -> String {
        let (rows, more) = self.bubble_rows(max);
        let active = self.in_progress_count();
        Json::object(|o| {
            o.int("v", 1);
            o.string("t", "downloads");
            o.uint("tok", u64::from(tok));
            o.bool("auto", auto);
            o.array("items", |a| {
                for entry in &rows {
                    a.object(|o| write_entry(o, entry, time.offset_secs));
                }
            });
            o.uint("more", more as u64);
            o.uint("active", active as u64);
        })
    }

    /// `{v:1,t:"snap",p:"downloadspage",d:{state,query,rows,more,undo,ack,active}}`: day-grouped rows
    /// (a header `{label,date,count}` then its entries), `more` = older rows were left out to stay under
    /// [`MAX_SNAPSHOT_BYTES`] (a bool, like the history page's; the bubble's `more` is a count).
    pub fn snapshot_for_page(&self, query: &str, ack: Ack, time: Time) -> String {
        let found = if self.list_state() == ListState::Error { Vec::new() } else { self.search(query) };
        let query = capped(query, MAX_QUERY_CHARS);
        let mut shown = found.len();
        loop {
            let json = self.page_json(&found[..shown], found.len() - shown, query, ack, time);
            if json.len() + 1024 <= MAX_SNAPSHOT_BYTES || shown == 0 {
                return json;
            }
            shown = shown * 3 / 4;
        }
    }

    fn page_json(&self, rows: &[&DownloadEntry], more: usize, query: &str, ack: Ack, time: Time) -> String {
        let today = day_of(time.now, time.offset_secs);
        let mut days: Vec<(i64, Vec<&DownloadEntry>)> = Vec::new();
        for entry in rows {
            let day = day_of(entry.started, time.offset_secs);
            match days.last_mut() {
                Some((open, list)) if *open == day => list.push(entry),
                _ => days.push((day, vec![entry])),
            }
        }
        Json::object(|o| {
            o.int("v", 1);
            o.string("t", "snap");
            o.string("p", "downloadspage");
            o.object("d", |o| {
                o.string("state", self.list_state().key());
                o.string("query", query);
                o.array("rows", |a| {
                    for (day, list) in &days {
                        a.object(|o| {
                            o.string("label", &day_label(*day, today));
                            o.string("date", &crate::win32::history_civil_date(*day));
                            o.uint("count", list.len() as u64);
                        });
                        for entry in list {
                            a.object(|o| write_entry(o, entry, time.offset_secs));
                        }
                    }
                });
                o.bool("more", more > 0);
                o.bool("undo", self.can_undo());
                o.string("ack", ack.key());
                o.uint("active", self.in_progress_count() as u64);
            });
        })
    }

    /// One entry as the wire object (docs/design/downloads.md "Entry object").
    pub fn entry_json(&self, id: u32, time: Time) -> Option<String> {
        self.get(id).map(|entry| Json::object(|o| write_entry(o, entry, time.offset_secs)))
    }

    // --- files that went away ---

    /// Re-checks finished downloads against the disk (`is_file` says whether a path is a regular
    /// file): a gone file reads `missing`, one that came back reads `done`. True if any changed.
    pub fn refresh_missing(&mut self, is_file: &dyn Fn(&Path) -> bool) -> bool {
        let mut changed = false;
        for entry in self.entries.iter_mut().filter(|e| e.state == State::Done) {
            let missing = !is_file(&entry.path);
            changed |= entry.missing != missing;
            entry.missing = missing;
        }
        if changed {
            self.touch();
        }
        changed
    }

    /// [`refresh_missing`](Self::refresh_missing), at most every two seconds.
    pub fn refresh_missing_if_stale(&mut self, now_ms: u64, is_file: &dyn Fn(&Path) -> bool) -> bool {
        if self.last_file_check_ms.is_some_and(|last| now_ms.saturating_sub(last) < FILE_CHECK_TTL_MS) {
            return false;
        }
        self.last_file_check_ms = Some(now_ms);
        self.refresh_missing(is_file)
    }

    // --- actions ---

    /// Whether `action` is legal for entry `id` (or for the list, with id 0).
    pub fn allowed(&self, id: u32, action: Action) -> bool {
        if action.is_list_level() {
            return id == 0;
        }
        let Some(entry) = self.get(id) else { return false };
        let has_url = !entry.url.is_empty();
        match action {
            Action::Pause => entry.state == State::Active && entry.op.is_some(),
            // The Store's placeholder URL can never be fetched: resuming it only fails again.
            Action::Resume => {
                entry.op.is_some()
                    && (entry.state == State::Paused || (entry.state == State::Failed && entry.resume))
                    && !is_store_placeholder_url(&entry.url)
            }
            Action::Cancel => entry.state.in_progress() && entry.op.is_some(),
            Action::Open | Action::Show => entry.state == State::Done && !entry.missing,
            Action::Remove => !entry.state.in_progress(),
            Action::CopyLink => has_url,
            Action::Retry => {
                has_url && matches!(entry.state, State::Failed | State::Cancelled) && retry_target(&entry.url).is_some()
            }
            Action::OpenPage | Action::OpenFolder | Action::ClearAll => false,
        }
    }

    /// Checks `id` and that `action` is legal for it, applies what the list itself can (remove,
    /// clear) and says what else to do. Nothing from a page is ever a path or a command: only an id.
    pub fn act(&mut self, id: u32, action: Action) -> Outcome {
        if !self.allowed(id, action) {
            return Outcome::Rejected;
        }
        match action {
            Action::OpenPage => return Outcome::OpenPage,
            Action::OpenFolder => return Outcome::OpenFolder,
            Action::ClearAll => {
                self.clear_finished();
                return Outcome::Changed;
            }
            _ => {}
        }
        let Some(entry) = self.get(id).cloned() else { return Outcome::Rejected };
        let key = entry.op.unwrap_or(0);
        match action {
            Action::Pause => Outcome::Engine { key, command: EngineCommand::Pause },
            Action::Resume => Outcome::Engine { key, command: EngineCommand::Resume },
            Action::Cancel => Outcome::Engine { key, command: EngineCommand::Cancel },
            Action::Open => Outcome::OpenFile(entry.path),
            Action::Show => Outcome::ShowInFolder(entry.path),
            Action::Remove => {
                self.remove(id);
                Outcome::Changed
            }
            Action::CopyLink => Outcome::CopyLink(entry.url),
            Action::Retry => retry_target(&entry.url).map_or(Outcome::Rejected, Outcome::Retry),
            Action::OpenPage | Action::OpenFolder | Action::ClearAll => Outcome::Rejected,
        }
    }

    /// Removes one finished entry (not its file). `false` if it is unknown or still in progress.
    pub fn remove(&mut self, id: u32) -> bool {
        let Some(at) = self.entries.iter().position(|e| e.id == id && !e.state.in_progress()) else {
            return false;
        };
        let gone = self.entries.remove(at);
        self.released.extend(gone.op);
        self.dropped.insert(gone.id);
        self.removed = vec![gone];
        self.touch();
        self.save();
        true
    }

    /// Removes every entry that is not in progress. Returns how many.
    pub fn clear_finished(&mut self) -> usize {
        let (gone, kept): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.entries).into_iter().partition(|e| !e.state.in_progress());
        self.entries = kept;
        for entry in &gone {
            self.released.extend(entry.op);
            self.dropped.insert(entry.id);
        }
        let count = gone.len();
        if count > 0 {
            self.removed = gone;
            self.touch();
            self.save();
        }
        count
    }

    pub fn can_undo(&self) -> bool {
        !self.removed.is_empty()
    }

    /// Puts the last removal back (where its ids belong). Returns how many came back.
    pub fn undo(&mut self) -> usize {
        let back = std::mem::take(&mut self.removed);
        let count = back.len();
        if count == 0 {
            return 0;
        }
        for mut entry in back {
            // The engine handle was released with the removal, so a restored failure cannot resume.
            entry.op = None;
            entry.resume = false;
            self.dropped.remove(&entry.id);
            self.authored.insert(entry.id);
            if !self.entries.iter().any(|e| e.id == entry.id) {
                self.entries.push(entry);
            }
        }
        self.entries.sort_by_key(|e| e.id);
        self.trim();
        self.touch();
        self.save();
        count
    }

    // --- persistence ---

    /// Writes the list now (atomic), merged with what other windows of the profile wrote since
    /// it was read. Guest and unreadable-file lists never write.
    pub fn save(&mut self) {
        let Store::File(path) = &self.store else { return };
        let path = path.clone();
        // A file that cannot be read now is replaced by this list, as before.
        let (disk, disk_next) = read_file_as(&path, false).unwrap_or_default();
        let written = self.merge(disk, disk_next);
        let text = render(&written, self.next_id);
        let _ = crate::productivity::atomic_write(&path, text.as_bytes());
        self.synced = written.iter().map(|e| e.id).collect();
    }

    /// Folds the file's entries (as written, in progress or not) into this list and returns what
    /// to write: this process's own entries, plus every other entry exactly as the file has it.
    fn merge(&mut self, mut disk: Vec<DownloadEntry>, disk_next: u32) -> Vec<DownloadEntry> {
        let mut changed = false;
        self.next_id = self.next_id.max(disk_next);
        // An id another window handed out at the same moment: this process's entry moves.
        for at in 0..self.entries.len() {
            let entry = &self.entries[at];
            let clash = self.authored.contains(&entry.id)
                && disk.iter().any(|d| d.id == entry.id && (d.started, &d.path) != (entry.started, &entry.path));
            if clash {
                let high = disk.iter().chain(&self.entries).map(|e| e.id).max().unwrap_or(0);
                let id = self.next_id.max(high.saturating_add(1));
                self.next_id = id.saturating_add(1);
                self.authored.remove(&self.entries[at].id);
                self.authored.insert(id);
                self.entries[at].id = id;
                changed = true;
            }
        }
        disk.retain(|d| !self.dropped.contains(&d.id) && !self.authored.contains(&d.id));
        let on_disk: HashSet<u32> = disk.iter().map(|d| d.id).collect();
        let before = self.entries.len();
        let (authored, synced) = (&self.authored, &self.synced);
        self.entries.retain(|e| authored.contains(&e.id) || on_disk.contains(&e.id) || !synced.contains(&e.id));
        changed |= self.entries.len() != before;
        let mut written: Vec<DownloadEntry> =
            self.entries.iter().filter(|e| !on_disk.contains(&e.id)).cloned().collect();
        for record in disk {
            let mut shown = record.clone();
            if shown.state.in_progress() {
                // Running in another window: this one cannot control it.
                interrupt(&mut shown, 0);
                shown.ended = record.started.max(record.ended);
                shown.unseen = false;
            }
            match self.entries.iter_mut().find(|e| e.id == record.id) {
                Some(mine) => {
                    shown.unseen = mine.unseen;
                    shown.missing = mine.missing;
                    if *mine != shown {
                        *mine = shown;
                        changed = true;
                    }
                }
                None => {
                    self.entries.push(shown);
                    changed = true;
                }
            }
            written.push(record);
        }
        if changed {
            self.entries.sort_by_key(|e| e.id);
            self.trim();
            self.touch();
        }
        written.sort_by_key(|e| e.id);
        let high = written.iter().map(|e| e.id).max().unwrap_or(0);
        self.next_id = self.next_id.max(high.saturating_add(1));
        written
    }
}

fn interrupt(entry: &mut DownloadEntry, now_ms: u64) {
    entry.state = State::Failed;
    entry.err = "Interrupted".to_owned();
    entry.ended = now_ms / 1000;
    entry.resume = false;
    entry.unseen = true;
}

fn write_entry(o: &mut Object<'_>, entry: &DownloadEntry, offset_secs: i64) {
    o.uint("id", u64::from(entry.id));
    o.string("name", capped(&entry.name, MAX_NAME_JSON));
    o.string("host", capped(&entry.host(), MAX_HOST_JSON));
    o.string("url", capped(&entry.url, MAX_URL_JSON));
    o.string("state", entry.state_key());
    o.uint("got", entry.got);
    o.int("total", entry.total);
    o.int("eta", entry.eta());
    o.uint("bps", entry.bps());
    o.string("err", if entry.state == State::Failed { capped(&entry.err, MAX_ERR_CHARS) } else { "" });
    o.bool("resume", entry.resume);
    o.string("clock", &clock_label(entry.clock_secs(), offset_secs));
}

fn clock_label(secs: u64, offset_secs: i64) -> String {
    let local = (secs as i64 + offset_secs).max(0);
    let of_day = local.rem_euclid(86_400);
    format!("{:02}:{:02}", of_day / 3600, of_day % 3600 / 60)
}

fn day_of(secs: u64, offset_secs: i64) -> i64 {
    (secs as i64 + offset_secs).max(0) / 86_400
}

fn day_label(day: i64, today: i64) -> String {
    match today - day {
        0 => "Today".to_owned(),
        1 => "Yesterday".to_owned(),
        _ => crate::win32::history_civil_date(day),
    }
}

// ---------------------------------------------------------------------------------------------
// downloads.txt
// ---------------------------------------------------------------------------------------------

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n").replace('\r', "\\r")
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// `#next`, then `id started ended state got total name url path err` per entry.
fn render(entries: &[DownloadEntry], next_id: u32) -> String {
    let mut text = format!("#next\t{next_id}\n");
    let skip = entries.len().saturating_sub(MAX_ENTRIES);
    for e in &entries[skip..] {
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            e.id,
            e.started,
            e.ended,
            e.state.key(),
            e.got,
            e.total,
            escape(&e.name),
            escape(&e.url),
            escape(&e.path.to_string_lossy()),
            escape(&e.err),
        ));
    }
    text
}

fn read_file(path: &Path) -> std::io::Result<(Vec<DownloadEntry>, u32)> {
    read_file_as(path, true)
}

/// [`read_file`]; with `interrupt` false, entries in progress keep their state (another window's).
fn read_file_as(path: &Path, interrupt: bool) -> std::io::Result<(Vec<DownloadEntry>, u32)> {
    use std::io::Read;
    let bytes = match std::fs::File::open(path) {
        Ok(file) => {
            if !file.metadata()?.is_file() {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "downloads file is not a file"));
            }
            let mut bytes = Vec::new();
            file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_FILE_BYTES {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "downloads file is too large to read"));
            }
            bytes
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 1)),
        Err(error) => return Err(error),
    };
    Ok(parse_as(&String::from_utf8_lossy(&bytes), interrupt))
}

/// Parses the file text. Malformed lines are skipped; entries that were in progress become failed
/// `Interrupted`. Returns the entries (oldest first, at most [`MAX_ENTRIES`]) and the next id.
#[cfg(test)]
fn parse(text: &str) -> (Vec<DownloadEntry>, u32) {
    parse_as(text, true)
}

fn parse_as(text: &str, interrupt_running: bool) -> (Vec<DownloadEntry>, u32) {
    let mut next_id = 1u32;
    let mut entries: Vec<DownloadEntry> = Vec::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        if let Some(rest) = line.strip_prefix("#next\t") {
            if let Ok(n) = rest.trim().parse::<u32>() {
                next_id = next_id.max(n);
            }
            continue;
        }
        let fields: Vec<&str> = line.splitn(10, '\t').collect();
        if fields.len() < 9 {
            continue;
        }
        let (Ok(id), Ok(started), Ok(ended), Some(state), Ok(got), Ok(total)) = (
            fields[0].parse::<u32>(),
            fields[1].parse::<u64>(),
            fields[2].parse::<u64>(),
            State::parse(fields[3]),
            fields[4].parse::<u64>(),
            fields[5].parse::<i64>(),
        ) else {
            continue;
        };
        if id == 0 || entries.iter().any(|e| e.id == id) {
            continue;
        }
        let path = PathBuf::from(unescape(fields[8]));
        if !path.is_absolute() {
            continue;
        }
        let interrupted = interrupt_running && state.in_progress();
        entries.push(DownloadEntry {
            id,
            name: sanitize_file_name(&unescape(fields[6])),
            url: clean_source_url(&unescape(fields[7])),
            path,
            state: if interrupted { State::Failed } else { state },
            missing: false,
            got,
            total: total.max(-1),
            started,
            ended: if interrupted { started.max(ended) } else { ended },
            err: if interrupted {
                "Interrupted".to_owned()
            } else {
                capped(&unescape(fields.get(9).copied().unwrap_or("")), MAX_ERR_CHARS).to_owned()
            },
            resume: false,
            op: None,
            unseen: false,
            speed: Speed::default(),
        });
    }
    entries.sort_by_key(|e| e.id);
    if entries.len() > MAX_ENTRIES {
        entries.drain(..entries.len() - MAX_ENTRIES);
    }
    let high = entries.iter().map(|e| e.id).max().unwrap_or(0);
    (entries, next_id.max(high.saturating_add(1)).max(1))
}

#[cfg(test)]
mod tests;
