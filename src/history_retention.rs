//! History retention: dropping visits that are older than the `history_retention` setting.
//!
//! The history list is `history.txt` (see `productivity`). This module only decides which visits are
//! too old and asks `productivity` to remove them, so the history writer's locks and cache stay in
//! one place. It runs when Bowser starts, when the setting changes, every few hours while Bowser is
//! open (on the next visit that is recorded). "Session only" never clears while Bowser is open: the
//! history is cleared once, when Bowser closes.
//! Nothing here reads or changes bookmarks, passwords, downloads or any browser-engine data.

use std::{
    io,
    path::Path,
    sync::atomic::{AtomicU8, AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{productivity, settings::HistoryRetention};

/// How often a long-running Bowser checks for expired visits, in seconds.
const CHECK_INTERVAL_SECS: u64 = 6 * 60 * 60;

static CURRENT: AtomicU8 = AtomicU8::new(index_of(HistoryRetention::Months3));
static LAST_PRUNE: AtomicU64 = AtomicU64::new(0);

const fn index_of(retention: HistoryRetention) -> u8 {
    match retention {
        HistoryRetention::Session => 0,
        HistoryRetention::Day => 1,
        HistoryRetention::Week => 2,
        HistoryRetention::Month => 3,
        HistoryRetention::Months3 => 4,
        HistoryRetention::Months6 => 5,
        HistoryRetention::Year => 6,
        HistoryRetention::Forever => 7,
    }
}

/// The retention in force (the setting as of the last [`set_current`]).
pub fn current() -> HistoryRetention {
    HistoryRetention::ALL
        .get(CURRENT.load(Ordering::Relaxed) as usize)
        .copied()
        .unwrap_or_default()
}

pub fn set_current(retention: HistoryRetention) {
    CURRENT.store(index_of(retention), Ordering::Relaxed);
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

/// Removes from the history at `path` every visit older than `retention` allows, as of `now`
/// (Unix seconds). Returns how many were removed. "Forever" removes none; "session only" removes
/// all of them.
pub fn prune(path: &Path, retention: HistoryRetention, now: u64) -> io::Result<usize> {
    match retention.max_age_secs() {
        None => Ok(0),
        Some(0) => {
            let held = productivity::read_history_strict(path)?.len();
            if held > 0 {
                productivity::clear_history(path)?;
            }
            Ok(held)
        }
        // A visit exactly `age` old is still kept; the range removes everything before the cutoff.
        Some(age) => match now.checked_sub(age) {
            Some(cutoff) if cutoff > 0 => productivity::delete_history_range(path, 0, cutoff - 1),
            _ => Ok(0),
        },
    }
}

/// --- wave-1 fixes --- [`prune`] while Bowser is open: "session only" keeps this session's visits (they
/// are cleared by [`finish_at_exit`] when Bowser closes), so choosing it, a new window or the
/// periodic check never wipes the history in use.
pub fn prune_in_session(path: &Path, retention: HistoryRetention, now: u64) -> io::Result<usize> {
    if retention == HistoryRetention::Session {
        return Ok(0);
    }
    prune(path, retention, now)
}

/// [`prune_in_session`] with the current setting and clock. For a worker thread: it takes the
/// history lock.
pub fn prune_now(path: &Path) -> io::Result<usize> {
    LAST_PRUNE.store(now_secs(), Ordering::Relaxed);
    prune_in_session(path, current(), now_secs())
}

/// Whether a long-running Bowser is due for another check, and if so notes that it is being done.
pub fn due() -> bool {
    let now = now_secs();
    let last = LAST_PRUNE.load(Ordering::Relaxed);
    now.saturating_sub(last) >= CHECK_INTERVAL_SECS
        && LAST_PRUNE.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_ok()
}

/// After a visit is recorded: if a check is due, runs it on its own thread so the page load never
/// waits for it. "Forever" and "session only" never check.
pub fn after_visit(path: &Path) {
    if matches!(current(), HistoryRetention::Forever | HistoryRetention::Session) || !due() {
        return;
    }
    let path = path.to_path_buf();
    let _ = std::thread::Builder::new().name("history-retention".into()).spawn(move || {
        let _ = prune_in_session(&path, current(), now_secs());
    });
}

// --- latency (fix E) --- a session-only history is shared by every Bowser process of the profile
// (`profile::spawn_window` starts separate processes), so it ends with the last of them, and a
// session a crash cut short is cleared when the next one starts.

/// Where each running Bowser process of a profile keeps its marker, beside `history.txt`.
fn markers_dir(history: &Path) -> std::path::PathBuf {
    history.with_file_name("history-running")
}

/// This process's place in the profile's running set: a marker file it holds open without delete
/// sharing for its whole life. Another process that cannot delete it knows this one is alive; once
/// this process ends, however it ends, Windows closes the handle and the marker can be deleted.
pub struct SessionMarker {
    file: Option<std::fs::File>,
    path: std::path::PathBuf,
}

/// Deletes every other process's marker in `dir` that can be deleted (their process is gone) and
/// counts the ones that cannot (their process is alive).
fn live_others(dir: &Path, own: Option<&Path>) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| Some(path.as_path()) != own && path.extension().is_some_and(|ext| ext == "run"))
        .filter(|path| std::fs::remove_file(path).is_err() && path.exists())
        .count()
}

impl SessionMarker {
    /// Joins the running set for the history at `history`. `.1` is true when no other process of
    /// the profile is running (this one is the first).
    pub fn join(history: &Path) -> io::Result<(Self, bool)> {
        let dir = markers_dir(history);
        std::fs::create_dir_all(&dir)?;
        let first = live_others(&dir, None) == 0;
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let path = dir.join(format!("{}-{nanos}.run", std::process::id()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0x1); // FILE_SHARE_READ only: no delete while this process lives.
        }
        let file = options.open(&path)?;
        Ok((Self { file: Some(file), path }, first))
    }

    /// Leaves the running set. True when this was the last process of the profile.
    pub fn leave(mut self) -> bool {
        self.file = None;
        let _ = std::fs::remove_file(&self.path);
        self.path.parent().is_none_or(|dir| live_others(dir, Some(&self.path)) == 0)
    }
}

static MARKER: std::sync::Mutex<Option<SessionMarker>> = std::sync::Mutex::new(None);

/// At launch, once the retention setting is known: joins the profile's running set, and when this is
/// the first process and the history is session only, clears what a session that never reached
/// [`finish_at_exit`] (a crash, a kill, a power cut) left behind.
pub fn start_session() {
    let Ok(profile) = crate::profile::active() else { return };
    if profile.is_guest() {
        return;
    }
    if let Some((marker, _first)) = start_session_for(&profile.history_file(), current()) {
        *MARKER.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(marker);
    }
}

/// [`start_session`] for the history at `path`. `None` when the marker cannot be written (then
/// [`finish_at_exit`] behaves as before: it clears at this process's exit).
pub fn start_session_for(path: &Path, retention: HistoryRetention) -> Option<(SessionMarker, bool)> {
    let (marker, first) = SessionMarker::join(path).ok()?;
    if first && retention == HistoryRetention::Session {
        let _ = productivity::clear_history(path);
    }
    Some((marker, first))
}

/// At exit: a session-only history ends here, once the last Bowser process of the profile closes.
/// Other settings leave the file alone.
pub fn finish_at_exit() {
    let marker = MARKER.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
    if let Ok(profile) = crate::profile::active() {
        if !profile.is_guest() {
            let _ = finish_session_for(&profile.history_file(), current(), marker);
        }
    }
}

/// [`finish_at_exit`] for the history at `path` and this process's `marker`: clears a session-only
/// history only when no other process of the profile still runs. True when cleared.
pub fn finish_session_for(path: &Path, retention: HistoryRetention, marker: Option<SessionMarker>) -> bool {
    let last = marker.is_none_or(SessionMarker::leave);
    last && finish_at_exit_for(path, retention)
}
// --- end latency ---

/// [`finish_at_exit`] for the history at `path`: clears it only for "session only". True when cleared.
pub fn finish_at_exit_for(path: &Path, retention: HistoryRetention) -> bool {
    retention == HistoryRetention::Session && productivity::clear_history(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn history_with(name: &str, ages_days: &[u64], now: u64) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bowser-retention-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.txt");
        for (n, days) in ages_days.iter().enumerate() {
            productivity::add_history_entry_at(&path, now - days * 86_400, &format!("https://site{n}.test/"), "T").unwrap();
        }
        productivity::flush_pending_history();
        path
    }

    fn urls(path: &Path) -> Vec<String> {
        productivity::read_history(path).into_iter().map(|(_, url, _)| url).collect()
    }

    #[test]
    fn each_period_keeps_exactly_the_visits_inside_it() {
        let now = 1_900_000_000;
        // Ages in days: 0, 2, 20, 60, 120, 300, 800.
        let ages = [0, 2, 20, 60, 120, 300, 800];
        for (retention, kept) in [
            (HistoryRetention::Day, 1),
            (HistoryRetention::Week, 2),
            (HistoryRetention::Month, 3),
            (HistoryRetention::Months3, 4),
            (HistoryRetention::Months6, 5),
            (HistoryRetention::Year, 6),
            (HistoryRetention::Forever, 7),
        ] {
            let path = history_with(retention.key(), &ages, now);
            let removed = prune(&path, retention, now).unwrap();
            assert_eq!(urls(&path).len(), kept, "{retention:?}");
            assert_eq!(removed, 7 - kept, "{retention:?}");
            let newest: Vec<String> = (0..kept).map(|n| format!("https://site{n}.test/")).collect();
            assert_eq!(urls(&path), newest, "{retention:?}: the newest visits stay");
            let _ = fs::remove_dir_all(path.parent().unwrap());
        }
    }

    #[test]
    fn session_only_removes_every_visit() {
        let now = 1_900_000_000;
        let path = history_with("session", &[0, 1, 400], now);
        assert_eq!(prune(&path, HistoryRetention::Session, now).unwrap(), 3);
        assert!(urls(&path).is_empty());
        assert_eq!(prune(&path, HistoryRetention::Session, now).unwrap(), 0, "nothing left to remove");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    // --- wave-1 fixes ---
    #[test]
    fn session_only_keeps_the_history_while_open_and_clears_it_at_exit() {
        let now = 1_900_000_000;
        let path = history_with("session-open", &[0, 1, 400], now);
        assert_eq!(prune_in_session(&path, HistoryRetention::Session, now).unwrap(), 0, "choosing it or a check");
        assert_eq!(urls(&path).len(), 3, "nothing is wiped while Bowser is open");
        assert_eq!(prune_in_session(&path, HistoryRetention::Week, now).unwrap(), 1, "other periods still prune");
        assert!(!finish_at_exit_for(&path, HistoryRetention::Week), "other periods leave it at exit");
        assert_eq!(urls(&path).len(), 2);
        assert!(finish_at_exit_for(&path, HistoryRetention::Session));
        assert!(urls(&path).is_empty(), "closing Bowser ends a session-only history");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    /// Fix E: two Bowser processes on one profile (two markers): closing the first keeps the
    /// session-only history the second still uses; closing the last clears it.
    #[test]
    fn a_session_only_history_ends_with_the_last_process_of_the_profile() {
        let now = 1_900_000_000;
        let path = history_with("session-two", &[0, 1], now);
        let (first, is_first) = start_session_for(&path, HistoryRetention::Session).unwrap();
        assert!(is_first);
        assert!(urls(&path).is_empty(), "the first process starts a fresh session-only history");
        productivity::add_history_entry_at(&path, now, "https://a.test/", "A").unwrap();
        let (second, is_first) = start_session_for(&path, HistoryRetention::Session).unwrap();
        assert!(!is_first, "the second window process joins");
        productivity::add_history_entry_at(&path, now, "https://b.test/", "B").unwrap();
        productivity::flush_pending_history();
        assert_eq!(urls(&path).len(), 2, "joining never clears");
        assert!(!finish_session_for(&path, HistoryRetention::Session, Some(first)), "another process still runs");
        assert_eq!(urls(&path).len(), 2, "closing one window process keeps the history");
        assert!(finish_session_for(&path, HistoryRetention::Session, Some(second)), "the last one clears");
        assert!(urls(&path).is_empty());
        assert_eq!(fs::read_dir(markers_dir(&path)).unwrap().count(), 0, "no marker is left behind");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    /// Fix E: a crashed process leaves its marker (here: a file nobody holds open) and its
    /// session-only history; the next first process clears both. Other periods keep the history.
    #[test]
    fn a_session_cut_short_by_a_crash_is_cleared_when_the_next_one_starts() {
        let now = 1_900_000_000;
        let path = history_with("session-crash", &[0, 1, 2], now);
        fs::create_dir_all(markers_dir(&path)).unwrap();
        let stale = markers_dir(&path).join("4242-1.run");
        fs::write(&stale, b"").unwrap();
        let (marker, first) = start_session_for(&path, HistoryRetention::Week).unwrap();
        assert!(first, "a dead process's marker does not count");
        assert!(!stale.exists(), "and it is removed");
        assert_eq!(urls(&path).len(), 3, "a period other than session only keeps the history");
        assert!(!finish_session_for(&path, HistoryRetention::Week, Some(marker)));
        fs::write(&stale, b"").unwrap();
        let (marker, first) = start_session_for(&path, HistoryRetention::Session).unwrap();
        assert!(first);
        assert!(urls(&path).is_empty(), "the crashed session's history is cleared at start");
        assert!(finish_session_for(&path, HistoryRetention::Session, Some(marker)));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_visit_exactly_at_the_limit_is_kept_and_a_second_older_is_not() {
        let now = 1_900_000_000;
        let path = history_with("edge", &[], now);
        let day = 86_400;
        productivity::add_history_entry_at(&path, now - day, "https://edge.test/", "T").unwrap();
        productivity::add_history_entry_at(&path, now - day - 1, "https://old.test/", "T").unwrap();
        productivity::flush_pending_history();
        assert_eq!(prune(&path, HistoryRetention::Day, now).unwrap(), 1);
        assert_eq!(urls(&path), ["https://edge.test/"]);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_missing_file_or_a_young_clock_is_not_an_error() {
        let path = std::env::temp_dir().join(format!("bowser-retention-none-{}", std::process::id())).join("history.txt");
        assert_eq!(prune(&path, HistoryRetention::Week, 1_900_000_000).unwrap(), 0);
        assert_eq!(prune(&path, HistoryRetention::Year, 100).unwrap(), 0, "a clock earlier than the period");
    }

    #[test]
    fn the_setting_is_remembered_and_the_check_runs_at_most_every_few_hours() {
        for retention in HistoryRetention::ALL {
            set_current(retention);
            assert_eq!(current(), retention);
        }
        set_current(HistoryRetention::Months3);
        LAST_PRUNE.store(0, Ordering::Relaxed);
        assert!(due(), "never checked: due");
        assert!(!due(), "just checked: not due again");
        LAST_PRUNE.store(now_secs() - CHECK_INTERVAL_SECS - 1, Ordering::Relaxed);
        assert!(due());
    }
}
