//! Small files replaced off the UI thread (R-14, R-17): a site's zoom per Ctrl+wheel notch and the
//! privacy counters per ad-heavy page load each did a temp write, `sync_all` and rename on the UI
//! thread. [`queue`] hands the bytes to one writer thread instead; a newer queue for the same file
//! replaces an older one not yet written, so a burst costs one write.
//!
//! Ordering: every write, queued or [`write_now`], happens under one lock, and `write_now` first
//! drops what is queued for its file, so an older queued snapshot can never land after a newer one.
//! [`flush`] writes whatever is queued on the calling thread (a clean exit).

use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Condvar, Mutex, MutexGuard, OnceLock},
};

struct Shared {
    pending: Mutex<HashMap<PathBuf, Vec<u8>>>,
    wake: Condvar,
    /// Held for every write, so writes to one file never overlap or reorder.
    write: Mutex<()>,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| Shared { pending: Mutex::new(HashMap::new()), wake: Condvar::new(), write: Mutex::new(()) })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Starts the writer thread once. `false` when it cannot be started (then [`queue`] writes inline).
fn writer_running() -> bool {
    static STARTED: OnceLock<bool> = OnceLock::new();
    *STARTED.get_or_init(|| {
        std::thread::Builder::new()
            .name("bowser-file-writer".into())
            .spawn(|| {
                let shared = shared();
                loop {
                    {
                        let mut pending = lock(&shared.pending);
                        while pending.is_empty() {
                            pending = shared.wake.wait(pending).unwrap_or_else(std::sync::PoisonError::into_inner);
                        }
                    }
                    write_pending();
                }
            })
            .is_ok()
    })
}

/// Writes and empties the queue, under the write lock (taken before the queue, as `write_now` does).
fn write_pending() {
    let shared = shared();
    let _write = lock(&shared.write);
    let batch = std::mem::take(&mut *lock(&shared.pending));
    for (path, bytes) in batch {
        let _ = replace(&path, &bytes);
    }
}

/// Replaces `path` with `bytes` on the writer thread. Errors are not reported: callers queue only
/// what they can lose (the next change, or exit's [`flush`], writes it again).
pub fn queue(path: PathBuf, bytes: Vec<u8>) {
    if !writer_running() {
        let _write = lock(&shared().write);
        let _ = replace(&path, &bytes);
        return;
    }
    let shared = shared();
    lock(&shared.pending).insert(path, bytes);
    shared.wake.notify_one();
}

/// Replaces `path` with `bytes` now, on the calling thread, dropping an older snapshot of it still
/// queued. For a change the user is told about when it fails (clearing data, a translate choice).
pub fn write_now(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let shared = shared();
    let _write = lock(&shared.write);
    lock(&shared.pending).remove(path);
    replace(path, bytes)
}

/// Writes everything queued, on the calling thread.
pub fn flush() {
    write_pending();
}

/// Holds every write back until the guard is dropped, so a test can see that a change only queued.
#[cfg(test)]
pub(crate) fn hold_writes() -> MutexGuard<'static, ()> {
    lock(&shared().write)
}

/// A temp file, `sync_all`, then a rename over `path`; nothing at all when `path` already holds
/// `bytes`.
fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if crate::productivity::file_holds(path, bytes) {
        return Ok(());
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    let mut file = File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bowser-background-write-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("file.txt")
    }

    fn wait_for(path: &Path, text: &str) {
        for _ in 0..2000 {
            if fs::read_to_string(path).is_ok_and(|current| current == text) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("{} never held {text:?}", path.display());
    }

    #[test]
    fn a_burst_of_queued_snapshots_lands_as_the_newest_and_write_now_is_never_overtaken() {
        let path = temp_file("burst");
        {
            let _held = hold_writes();
            for n in 0..10 {
                queue(path.clone(), format!("snapshot {n}").into_bytes());
            }
            assert!(!path.exists(), "a queued snapshot was written on the calling thread");
        }
        wait_for(&path, "snapshot 9");

        // An older snapshot still queued never lands after a newer write_now.
        {
            let _held = hold_writes();
            queue(path.clone(), b"older".to_vec());
        }
        write_now(&path, b"newer").unwrap();
        flush();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(fs::read_to_string(&path).unwrap(), "newer");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
