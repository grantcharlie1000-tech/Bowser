//! Crash recovery: a small, versioned snapshot of the open tabs plus a "running" marker.
//!
//! Files in `%LOCALAPPDATA%\Bowser`:
//! * `session.txt`     - latest snapshot (atomic temp + rename, size capped).
//! * `session.prev`    - the snapshot that existed when this run first saved (one-step backup).
//! * `session.crashed` - a snapshot set aside by the crash-loop guard; never deleted by Bowser.
//! * `running.lock`    - created at start, removed on clean exit. Its presence at start means
//!   the previous run ended unexpectedly. It holds the number of consecutive crashed runs and is
//!   kept open with no sharing, so a second live instance can tell it apart from a stale marker
//!   (and then leaves the session files alone).
//!
//! Only committed `http`/`https` URLs without embedded credentials are stored, together with the
//! title, pinned flag and active tab. Per-tab back/forward history is not stored. Bowser has no
//! private/incognito mode, so there is nothing to exclude on that account. Page contents, form
//! data and cookies are never written. A corrupt or partial file is ignored (and left in place
//! until the next successful save, which first backs it up to `session.prev`).

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use url::Url;

pub const FILE_NAME: &str = "session.txt";
pub const PREV_NAME: &str = "session.prev";
pub const CRASHED_NAME: &str = "session.crashed";
pub const LOCK_NAME: &str = "running.lock";
pub const MAX_FILE_BYTES: usize = 512 * 1024;
pub const MAX_TABS: usize = 200;
pub const MAX_URL_BYTES: usize = 4096;
pub const MAX_TITLE_CHARS: usize = 200;
/// Minimum time between two writes, enforced by the one-shot timer in the window procedure.
pub const SAVE_DEBOUNCE_MS: u32 = 5_000;
/// This many crashed runs in a row (the original and a run that restored its session) make the
/// next start fresh, with the snapshot set aside as `session.crashed`.
pub const CRASH_LOOP_LIMIT: u32 = 2;

const HEADER: &str = "bowser-session 1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionTab {
    pub url: String,
    pub title: String,
    pub pinned: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    pub tabs: Vec<SessionTab>,
    pub active: usize,
}

/// The URL text to persist, or `None` for anything that must not be stored: non-web schemes,
/// `about:blank`, URLs with userinfo, host-less or oversized URLs.
pub fn persistable_url(url: &Url) -> Option<String> {
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.as_str().len() > MAX_URL_BYTES
    {
        return None;
    }
    Some(url.as_str().to_owned())
}

impl Session {
    /// Builds a snapshot from `(url, title, pinned)` in strip order. Tabs without a persistable
    /// URL are dropped. `active` indexes the input; if the active tab was dropped the first
    /// remaining tab is active. `None` when nothing is worth saving.
    pub fn from_tabs<'a>(
        tabs: impl IntoIterator<Item = (Option<&'a Url>, &'a str, bool)>,
        active: usize,
    ) -> Option<Self> {
        let mut kept = Vec::new();
        let mut kept_active = 0;
        for (index, (url, title, pinned)) in tabs.into_iter().enumerate() {
            let Some(url) = url.and_then(persistable_url) else {
                continue;
            };
            if kept.len() >= MAX_TABS {
                break;
            }
            if index == active {
                kept_active = kept.len();
            }
            kept.push(SessionTab {
                url,
                title: title.chars().take(MAX_TITLE_CHARS).collect(),
                pinned,
            });
        }
        (!kept.is_empty()).then_some(Self {
            tabs: kept,
            active: kept_active,
        })
    }

    pub fn serialize(&self) -> String {
        let mut text = format!("{HEADER}\nactive {}\n", self.active);
        for tab in &self.tabs {
            text.push_str("tab\t");
            text.push(if tab.pinned { '1' } else { '0' });
            text.push('\t');
            text.push_str(&tab.url);
            text.push('\t');
            for c in tab.title.chars() {
                match c {
                    '\\' => text.push_str("\\\\"),
                    '\t' => text.push_str("\\t"),
                    '\n' => text.push_str("\\n"),
                    '\r' => text.push_str("\\r"),
                    c => text.push(c),
                }
            }
            text.push('\n');
        }
        text
    }

    /// Tolerant parser: a wrong header or oversized text gives `None`; individual bad lines are
    /// skipped so a partially damaged file still restores what it can.
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() > MAX_FILE_BYTES {
            return None;
        }
        let mut lines = text.lines();
        if lines.next()? != HEADER {
            return None;
        }
        let mut active = 0usize;
        let mut tabs = Vec::new();
        for line in lines {
            if let Some(value) = line.strip_prefix("active ") {
                active = value.trim().parse().unwrap_or(0);
                continue;
            }
            let mut fields = line.splitn(4, '\t');
            if fields.next() != Some("tab") {
                continue;
            }
            let (Some(pinned), Some(url), Some(title)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let pinned = match pinned {
                "1" => true,
                "0" => false,
                _ => continue,
            };
            let Some(url) = Url::parse(url).ok().and_then(|url| persistable_url(&url)) else {
                continue;
            };
            if tabs.len() >= MAX_TABS {
                break;
            }
            tabs.push(SessionTab {
                url,
                title: unescape(title).chars().take(MAX_TITLE_CHARS).collect(),
                pinned,
            });
        }
        if tabs.is_empty() {
            return None;
        }
        if active >= tabs.len() {
            active = 0;
        }
        Some(Self { tabs, active })
    }
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartDecision {
    /// Previous run exited cleanly (or this is the first run).
    Clean,
    /// Previous run crashed; `consecutive` crashed runs in a row including it.
    OfferRestore { consecutive: u32 },
    /// Too many crashed runs in a row: start fresh and set the snapshot aside.
    CrashLoop,
}

/// `lock` is the count stored in a leftover `running.lock`, or `None` if there was no lock.
pub fn decide_start(lock: Option<u32>) -> StartDecision {
    match lock {
        None => StartDecision::Clean,
        Some(before) => {
            let consecutive = before.saturating_add(1);
            if consecutive >= CRASH_LOOP_LIMIT {
                StartDecision::CrashLoop
            } else {
                StartDecision::OfferRestore { consecutive }
            }
        }
    }
}

fn parse_lock(text: &str) -> u32 {
    text.trim()
        .strip_prefix("bowser-lock ")
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0)
}

#[derive(Debug)]
pub struct Offer {
    pub session: Session,
    pub consecutive: u32,
}

pub struct SessionStore {
    dir: PathBuf,
    lock: Option<File>,
    rotated: bool,
    last_text: Option<String>,
    /// A one-shot save timer is armed; further changes do not re-arm it.
    pub save_pending: bool,
}

impl SessionStore {
    /// Inspects the previous run. `None` when another live Bowser holds the lock: this process
    /// then neither restores nor writes session files.
    pub fn open(dir: &Path) -> Option<(Self, Option<Offer>)> {
        fs::create_dir_all(dir).ok()?;
        let lock_path = dir.join(LOCK_NAME);
        let prior = if lock_path.exists() {
            let mut text = String::new();
            match File::open(&lock_path).and_then(|file| file.take(256).read_to_string(&mut text)) {
                Ok(_) => Some(parse_lock(&text)),
                Err(_) => return None, // held open by a live instance
            }
        } else {
            None
        };
        let store = Self {
            dir: dir.to_owned(),
            lock: None,
            rotated: false,
            last_text: None,
            save_pending: false,
        };
        let offer = match decide_start(prior) {
            StartDecision::Clean => None,
            StartDecision::OfferRestore { consecutive } => {
                store.read_session().map(|session| Offer {
                    session,
                    consecutive,
                })
            }
            StartDecision::CrashLoop => {
                // Set aside, never delete; a bad or missing file is simply left alone.
                let path = dir.join(FILE_NAME);
                if path.exists() {
                    let _ = fs::rename(&path, dir.join(CRASHED_NAME));
                }
                None
            }
        };
        Some((store, offer))
    }

    fn read_session(&self) -> Option<Session> {
        let mut bytes = Vec::new();
        File::open(self.dir.join(FILE_NAME))
            .ok()?
            .take(MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        Session::parse(&String::from_utf8(bytes).ok()?)
    }

    /// Creates the running marker. `restored` carries the crash count when the user accepted a
    /// restore; otherwise the count restarts at zero.
    pub fn begin_run(&mut self, restored: Option<&Offer>) {
        let count = restored.map_or(0, |offer| offer.consecutive);
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .share_mode(0)
            .open(self.dir.join(LOCK_NAME));
        if let Ok(mut file) = file {
            let _ = write!(file, "bowser-lock {count}");
            let _ = file.flush();
            self.lock = Some(file);
        }
    }

    /// Writes the snapshot unless it is unchanged. The first write of a run first copies any
    /// existing `session.txt` to `session.prev`.
    pub fn save(&mut self, session: &Session) -> io::Result<()> {
        let text = session.serialize();
        if text.len() > MAX_FILE_BYTES || self.last_text.as_deref() == Some(text.as_str()) {
            return Ok(());
        }
        let path = self.dir.join(FILE_NAME);
        if !self.rotated {
            if path.exists() {
                let _ = fs::copy(&path, self.dir.join(PREV_NAME));
            }
            self.rotated = true;
        }
        let mut temp = path.as_os_str().to_owned();
        temp.push(".tmp");
        let temp = PathBuf::from(temp);
        let written = (|| {
            let mut file = File::create(&temp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp, &path)
        })();
        if written.is_err() {
            let _ = fs::remove_file(&temp);
        } else {
            self.last_text = Some(text);
        }
        written
    }

    /// Clean exit: final save (if there is anything to save), then remove the running marker.
    pub fn finish(&mut self, session: Option<&Session>) {
        if let Some(session) = session {
            let _ = self.save(session);
        }
        if self.lock.take().is_some() {
            let _ = fs::remove_file(self.dir.join(LOCK_NAME));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bowser-session-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn sample() -> Session {
        let a = url("https://example.com/a?x=1#frag");
        let b = url("http://example.org/");
        Session::from_tabs(
            [
                (Some(&a), "Tab\twith\\odd\nchars", true),
                (None, "blank", false),
                (Some(&b), "B", false),
            ],
            2,
        )
        .unwrap()
    }

    #[test]
    fn only_plain_web_urls_are_persisted() {
        assert!(persistable_url(&url("https://example.com/p?q=1#f")).is_some());
        assert!(persistable_url(&url("about:blank")).is_none());
        assert!(persistable_url(&url("file:///C:/secret.txt")).is_none());
        assert!(persistable_url(&url("javascript:alert(1)")).is_none());
        assert!(persistable_url(&url("https://user:pw@example.com/")).is_none());
        assert!(persistable_url(&url("https://user@example.com/")).is_none());
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL_BYTES));
        assert!(persistable_url(&url(&long)).is_none());
    }

    #[test]
    fn snapshot_round_trips_and_remaps_active() {
        let session = sample();
        assert_eq!(session.tabs.len(), 2);
        assert_eq!(session.active, 1);
        assert!(session.tabs[0].pinned);
        let parsed = Session::parse(&session.serialize()).unwrap();
        assert_eq!(parsed, session);
        assert_eq!(parsed.tabs[0].title, "Tab\twith\\odd\nchars");
    }

    #[test]
    fn dropped_active_tab_falls_back_to_first_and_empty_gives_none() {
        let a = url("https://a.test/");
        let s = Session::from_tabs([(None, "x", false), (Some(&a), "a", false)], 0).unwrap();
        assert_eq!(s.active, 0);
        assert!(Session::from_tabs([(None, "x", false)], 0).is_none());
        assert!(Session::from_tabs(std::iter::empty(), 0).is_none());
    }

    #[test]
    fn corrupt_or_partial_files_are_tolerated() {
        assert!(Session::parse("").is_none());
        assert!(Session::parse("garbage\ntab\t0\thttps://a.test/\tA\n").is_none());
        assert!(Session::parse("bowser-session 2\n").is_none());
        assert!(Session::parse("bowser-session 1\nactive 0\n").is_none());
        let text = "bowser-session 1\nactive 9\ntab\t0\thttps://a.test/\tA\nbroken line\n\
                    tab\t7\thttps://bad.test/\tX\ntab\t0\tjavascript:x\tY\n\
                    tab\t1\thttps://u:p@c.test/\tZ\ntab\t1\thttps://b.test/\tB\ntab\t0\thttps://c.te";
        let s = Session::parse(text).unwrap();
        assert_eq!(s.tabs.len(), 2);
        assert_eq!(s.active, 0, "out-of-range active resets");
        assert_eq!(s.tabs[1].url, "https://b.test/");
        let oversized = format!("bowser-session 1\n{}", "x".repeat(MAX_FILE_BYTES));
        assert!(Session::parse(&oversized).is_none());
    }

    #[test]
    fn tab_count_and_title_are_capped() {
        let u = url("https://a.test/");
        let title = "t".repeat(MAX_TITLE_CHARS * 2);
        let tabs: Vec<_> = (0..MAX_TABS + 50)
            .map(|_| (Some(&u), title.as_str(), false))
            .collect();
        let s = Session::from_tabs(tabs, 0).unwrap();
        assert_eq!(s.tabs.len(), MAX_TABS);
        assert_eq!(s.tabs[0].title.chars().count(), MAX_TITLE_CHARS);
        assert!(s.serialize().len() < MAX_FILE_BYTES);
    }

    #[test]
    fn start_decision_offers_once_then_stops_a_crash_loop() {
        assert_eq!(decide_start(None), StartDecision::Clean);
        assert_eq!(
            decide_start(Some(0)),
            StartDecision::OfferRestore { consecutive: 1 }
        );
        assert_eq!(decide_start(Some(1)), StartDecision::CrashLoop);
        assert_eq!(decide_start(Some(u32::MAX)), StartDecision::CrashLoop);
        assert_eq!(parse_lock("bowser-lock 1"), 1);
        assert_eq!(parse_lock("junk"), 0);
        assert_eq!(parse_lock(""), 0);
    }

    #[test]
    fn clean_exit_removes_lock_and_saves() {
        let dir = temp_dir("clean");
        let (mut store, offer) = SessionStore::open(&dir).unwrap();
        assert!(offer.is_none());
        store.begin_run(None);
        assert!(dir.join(LOCK_NAME).exists());
        store.finish(Some(&sample()));
        assert!(!dir.join(LOCK_NAME).exists());
        let (_, offer) = SessionStore::open(&dir).unwrap();
        assert!(offer.is_none(), "a clean exit is not offered for restore");
        assert_eq!(
            Session::parse(&fs::read_to_string(dir.join(FILE_NAME)).unwrap()),
            Some(sample())
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn crash_is_offered_once_and_a_second_crash_sets_the_file_aside() {
        let dir = temp_dir("crash");
        {
            let (mut store, _) = SessionStore::open(&dir).unwrap();
            store.begin_run(None);
            store.save(&sample()).unwrap();
            // Crash: no finish; the handle is closed but the lock file remains.
        }
        let (mut store, offer) = SessionStore::open(&dir).unwrap();
        let offer = offer.expect("crash offers a restore");
        assert_eq!(offer.consecutive, 1);
        assert_eq!(offer.session, sample());
        store.begin_run(Some(&offer));
        drop(store); // crashes again

        let (mut store, offer) = SessionStore::open(&dir).unwrap();
        assert!(offer.is_none(), "crash loop starts fresh");
        assert!(dir.join(CRASHED_NAME).exists());
        assert!(!dir.join(FILE_NAME).exists());
        store.begin_run(None);
        store.finish(None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn declining_a_restore_resets_the_crash_count() {
        let dir = temp_dir("decline");
        {
            let (mut store, _) = SessionStore::open(&dir).unwrap();
            store.begin_run(None);
            store.save(&sample()).unwrap();
        }
        {
            let (mut store, offer) = SessionStore::open(&dir).unwrap();
            assert!(offer.is_some());
            store.begin_run(None); // user said no
        }
        let (_, offer) = SessionStore::open(&dir).unwrap();
        assert!(
            offer.is_some(),
            "still offered: only one crashed run in a row"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn live_instance_lock_disables_a_second_store() {
        let dir = temp_dir("live");
        let (mut first, _) = SessionStore::open(&dir).unwrap();
        first.begin_run(None);
        assert!(SessionStore::open(&dir).is_none());
        first.finish(None);
        assert!(SessionStore::open(&dir).is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_save_backs_up_previous_file_and_unchanged_saves_are_skipped() {
        let dir = temp_dir("prev");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(FILE_NAME), "old").unwrap();
        let (mut store, _) = SessionStore::open(&dir).unwrap();
        store.begin_run(None);
        store.save(&sample()).unwrap();
        assert_eq!(fs::read_to_string(dir.join(PREV_NAME)).unwrap(), "old");
        let first = fs::read_to_string(dir.join(FILE_NAME)).unwrap();
        store.save(&sample()).unwrap();
        let other = Session::from_tabs([(Some(&url("https://n.test/")), "n", false)], 0).unwrap();
        store.save(&other).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join(PREV_NAME)).unwrap(),
            "old",
            "backup is once per run"
        );
        assert_ne!(fs::read_to_string(dir.join(FILE_NAME)).unwrap(), first);
        assert!(!dir.join("session.txt.tmp").exists());
        store.finish(None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_session_with_a_crash_is_not_offered_and_is_left_in_place() {
        let dir = temp_dir("corrupt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(FILE_NAME), "\u{0}\u{1}not a session").unwrap();
        fs::write(dir.join(LOCK_NAME), "bowser-lock 0").unwrap();
        let (_, offer) = SessionStore::open(&dir).unwrap();
        assert!(offer.is_none());
        assert!(dir.join(FILE_NAME).exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
