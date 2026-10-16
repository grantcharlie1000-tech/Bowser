//! History retention: dropping visits that are older than the `history_retention` setting.
//!
//! The history list is `history.txt` (see `productivity`). This module only decides which visits are
//! too old and asks `productivity` to remove them, so the history writer's locks and cache stay in
//! one place. It runs when Bowser starts, when the setting changes, every few hours while Bowser is
//! open (on the next visit that is recorded), and, for "session only", once more when Bowser closes.
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

/// [`prune`] with the current setting and clock. For a worker thread: it takes the history lock.
pub fn prune_now(path: &Path) -> io::Result<usize> {
    LAST_PRUNE.store(now_secs(), Ordering::Relaxed);
    prune(path, current(), now_secs())
}

/// Whether a long-running Bowser is due for another check, and if so notes that it is being done.
pub fn due() -> bool {
    let now = now_secs();
    let last = LAST_PRUNE.load(Ordering::Relaxed);
    now.saturating_sub(last) >= CHECK_INTERVAL_SECS
        && LAST_PRUNE.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_ok()
}

/// After a visit is recorded: if a check is due, runs it on its own thread so the page load never
/// waits for it. "Forever" never checks.
pub fn after_visit(path: &Path) {
    if current() == HistoryRetention::Forever || !due() {
        return;
    }
    let path = path.to_path_buf();
    let _ = std::thread::Builder::new().name("history-retention".into()).spawn(move || {
        let _ = prune(&path, current(), now_secs());
    });
}

/// At exit: a session-only history ends here. Other settings leave the file alone.
pub fn finish_at_exit() {
    if current() != HistoryRetention::Session {
        return;
    }
    if let Ok(profile) = crate::profile::active() {
        if !profile.is_guest() {
            let _ = productivity::clear_history(&profile.history_file());
        }
    }
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
