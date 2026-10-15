//! The on-disk cache of language files and the download that fills it.
//!
//! Layout under the cache folder (see [`cache_root`]): `worker.html` and `worker.js` (written from
//! the embedded copies each time a worker page is needed), `engine/` and `models/<src>-<dst>/` as the
//! manifest names them. Files arrive only through [`ensure`], on a background thread, and only when
//! the user asked to translate: each is fetched to a `.part` file, checked against its pinned size
//! and SHA-256, and only then renamed into place, so a file at its final name is always a verified one.
//! A file already there (a download from before, or one placed by hand) is accepted after the same
//! check and never fetched again.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use super::manifest::File;

/// Folder name of this engine's cache, so a newer engine never reads an older one's files.
const CACHE_DIR: &str = "translate-v1";

/// Why files could not be made ready. Fixed words for the user's message; no paths or addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// The download failed or was cut short.
    Network,
    /// What arrived (or sat in the folder) is not the pinned file. It was thrown away.
    Hash,
    /// The folder could not be written.
    Io,
}

/// Where the language files live: shared by every profile under the app data folder, so a pack is
/// downloaded once. A guest session keeps its own copy inside the guest profile, which is deleted
/// when the session ends. `None` when there is no data folder.
pub fn cache_root() -> Option<PathBuf> {
    let profile = crate::profile::active().ok()?;
    let base = if profile.is_guest() { profile.root().to_path_buf() } else { crate::profile::app_data_dir()? };
    Some(base.join(CACHE_DIR))
}

/// The place of `file` under `root`.
pub fn path_of(root: &Path, file: &File) -> PathBuf {
    file.path.split('/').fold(root.to_path_buf(), |path, part| path.join(part))
}

/// Total bytes of `files`.
pub fn total_bytes(files: &[&File]) -> u64 {
    files.iter().map(|file| file.size).sum()
}

/// The files that must still be fetched or checked: missing, or of the wrong size. A cheap test (one
/// `stat` each, no hashing) that decides what to tell the user and whether a private tab must be
/// asked first; [`ensure`] does the real check.
pub fn missing<'a>(root: &Path, files: &[&'a File]) -> Vec<&'a File> {
    files
        .iter()
        .copied()
        .filter(|file| fs::metadata(path_of(root, file)).map_or(true, |meta| !meta.is_file() || meta.len() != file.size))
        .collect()
}

/// Writes the worker page and script into `root` if they are not already exactly the embedded
/// copies. They are small and rewritten whenever they differ, so the cache folder never decides
/// what the worker page runs.
pub fn write_worker_files(root: &Path) -> io::Result<()> {
    fs::create_dir_all(root)?;
    for (name, content) in [("worker.html", super::WORKER_HTML), ("worker.js", super::WORKER_JS)] {
        let path = root.join(name);
        if fs::read(&path).is_ok_and(|existing| existing == content.as_bytes()) {
            continue;
        }
        let part = root.join(format!("{name}.part"));
        fs::write(&part, content)?;
        let _ = fs::remove_file(&path);
        fs::rename(&part, &path)?;
    }
    Ok(())
}

/// Streams one file's bytes to the sink it is given. The real one is a WinHTTP GET; tests use fakes.
pub type Fetch<'a> = dyn FnMut(&File, &mut dyn FnMut(&[u8]) -> io::Result<()>) -> Result<(), StoreError> + 'a;
/// Lower-case hex SHA-256 of a file, or `None` if it cannot be read.
pub type HashFile<'a> = dyn Fn(&Path) -> Option<String> + 'a;

/// Makes every file in `files` present and verified under `root`, fetching only what is not already
/// there. Stops at the first failure. Safe to run again after any failure or interruption.
pub fn ensure_with(root: &Path, files: &[&File], fetch: &mut Fetch<'_>, hash_file: &HashFile<'_>) -> Result<(), StoreError> {
    for file in files {
        let dest = path_of(root, file);
        if dest.is_file() {
            if file_matches(&dest, file, hash_file) {
                continue;
            }
            // Wrong content at the final name: a hand-placed or damaged file. Remove it and fetch.
            fs::remove_file(&dest).map_err(|_| StoreError::Io)?;
        }
        if let Some(folder) = dest.parent() {
            fs::create_dir_all(folder).map_err(|_| StoreError::Io)?;
        }
        let part = part_path(&dest);
        let _ = fs::remove_file(&part);
        let fetched = fetch_to(&part, file, fetch);
        let verdict = match fetched {
            Ok(()) if file_matches(&part, file, hash_file) => Ok(()),
            Ok(()) => Err(StoreError::Hash),
            Err(error) => Err(error),
        };
        if let Err(error) = verdict {
            let _ = fs::remove_file(&part);
            return Err(error);
        }
        // The destination does not exist here, so the rename is the only step that publishes it.
        if fs::rename(&part, &dest).is_err() {
            let _ = fs::remove_file(&part);
            return Err(StoreError::Io);
        }
    }
    Ok(())
}

fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().map(|name| name.to_os_string()).unwrap_or_default();
    name.push(".part");
    dest.with_file_name(name)
}

fn file_matches(path: &Path, file: &File, hash_file: &HashFile<'_>) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.len() == file.size) && hash_file(path).is_some_and(|hash| hash == file.sha256)
}

fn fetch_to(part: &Path, file: &File, fetch: &mut Fetch<'_>) -> Result<(), StoreError> {
    let mut out = fs::File::create(part).map_err(|_| StoreError::Io)?;
    let mut written = 0u64;
    let mut io_failed = false;
    let result = fetch(file, &mut |chunk| {
        written += chunk.len() as u64;
        if written > file.size {
            return Err(io::Error::other("larger than pinned"));
        }
        out.write_all(chunk).inspect_err(|_| io_failed = true)
    });
    result?;
    if io_failed || out.flush().is_err() {
        return Err(StoreError::Io);
    }
    drop(out);
    if written == file.size { Ok(()) } else { Err(StoreError::Network) }
}

/// [`ensure_with`] with the real download (WinHTTP, HTTPS only, at most the pinned size) and the
/// real hash. Meant for a background thread: it blocks for the whole download.
pub fn ensure(root: &Path, files: &[&File]) -> Result<(), StoreError> {
    let mut fetch = |file: &File, sink: &mut dyn FnMut(&[u8]) -> io::Result<()>| {
        let url = url::Url::parse(file.url).map_err(|_| StoreError::Network)?;
        crate::updater::http::get(&url, file.size, &mut |chunk| sink(chunk).map_err(|error| crate::updater::UpdateError::Io(error.to_string())))
            .map(|_| ())
            .map_err(|_| StoreError::Network)
    };
    let hash = |path: &Path| crate::updater::sha256_file(path).ok().map(|(digest, _)| crate::updater::hex_encode(&digest));
    ensure_with(root, files, &mut fetch, &hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bowser-translate-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // A tiny stand-in for the hash: the file's bytes as hex-less text. The real digest is only the
    // comparison key here, so a fake keeps the tests independent of the network and of CNG.
    fn fake_hash(path: &Path) -> Option<String> {
        fs::read(path).ok().map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    fn file(path: &'static str, content: &'static str) -> File {
        File { path, url: "https://example.invalid/x", size: content.len() as u64, sha256: content }
    }

    #[test]
    fn a_good_download_is_verified_then_published_under_its_final_name() {
        let root = temp("good");
        let a = file("models/xx-en/model.bin", "model-bytes");
        let b = file("engine/e.js", "engine-bytes");
        let fetched = Cell::new(0);
        let mut fetch = |f: &File, sink: &mut dyn FnMut(&[u8]) -> io::Result<()>| {
            fetched.set(fetched.get() + 1);
            // Two chunks, like a real download.
            let bytes = f.sha256.as_bytes();
            sink(&bytes[..3]).map_err(|_| StoreError::Io)?;
            sink(&bytes[3..]).map_err(|_| StoreError::Io)
        };
        ensure_with(&root, &[&a, &b], &mut fetch, &fake_hash).unwrap();
        assert_eq!(fetched.get(), 2);
        assert_eq!(fs::read_to_string(path_of(&root, &a)).unwrap(), "model-bytes");
        assert_eq!(fs::read_to_string(path_of(&root, &b)).unwrap(), "engine-bytes");
        assert!(!path_of(&root, &a).with_file_name("model.bin.part").exists(), "no partial file is left");
        assert!(missing(&root, &[&a, &b]).is_empty());
        // A second run fetches nothing: the verified files are accepted as they are.
        ensure_with(&root, &[&a, &b], &mut fetch, &fake_hash).unwrap();
        assert_eq!(fetched.get(), 2);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_hash_mismatch_is_refused_and_leaves_nothing_behind() {
        let root = temp("mismatch");
        let a = file("models/xx-en/model.bin", "model-bytes");
        // Same length, different content.
        let mut fetch = |_: &File, sink: &mut dyn FnMut(&[u8]) -> io::Result<()>| sink(b"MODEL-BYTES").map_err(|_| StoreError::Io);
        assert_eq!(ensure_with(&root, &[&a], &mut fetch, &fake_hash), Err(StoreError::Hash));
        assert!(!path_of(&root, &a).exists(), "nothing is published under the final name");
        assert!(!part_path(&path_of(&root, &a)).exists(), "the partial file is removed");
        assert_eq!(missing(&root, &[&a]).len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_short_or_oversized_or_failed_download_is_refused() {
        let root = temp("short");
        let a = file("models/xx-en/model.bin", "model-bytes");
        let mut short = |_: &File, sink: &mut dyn FnMut(&[u8]) -> io::Result<()>| sink(b"model").map_err(|_| StoreError::Io);
        assert_eq!(ensure_with(&root, &[&a], &mut short, &fake_hash), Err(StoreError::Network), "cut short");
        let mut long = |_: &File, sink: &mut dyn FnMut(&[u8]) -> io::Result<()>| sink(b"model-bytes-and-more").map_err(|_| StoreError::Io);
        assert!(ensure_with(&root, &[&a], &mut long, &fake_hash).is_err(), "more than the pinned size");
        let mut failing = |_: &File, _: &mut dyn FnMut(&[u8]) -> io::Result<()>| Err(StoreError::Network);
        assert_eq!(ensure_with(&root, &[&a], &mut failing, &fake_hash), Err(StoreError::Network));
        assert!(!path_of(&root, &a).exists());
        assert!(!part_path(&path_of(&root, &a)).exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_stale_partial_file_never_counts_and_is_replaced() {
        let root = temp("partial");
        let a = file("models/xx-en/model.bin", "model-bytes");
        let dest = path_of(&root, &a);
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::write(part_path(&dest), "model-by").unwrap();
        assert_eq!(missing(&root, &[&a]).len(), 1, "a .part file is not the file");
        let mut fetch = |f: &File, sink: &mut dyn FnMut(&[u8]) -> io::Result<()>| sink(f.sha256.as_bytes()).map_err(|_| StoreError::Io);
        ensure_with(&root, &[&a], &mut fetch, &fake_hash).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "model-bytes");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_placed_by_hand_is_accepted_only_after_the_same_check() {
        let root = temp("manual");
        let a = file("models/xx-en/model.bin", "model-bytes");
        let dest = path_of(&root, &a);
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        // The right file, placed by hand: kept, and nothing is fetched.
        fs::write(&dest, "model-bytes").unwrap();
        let fetched = Cell::new(0);
        let mut fetch = |f: &File, sink: &mut dyn FnMut(&[u8]) -> io::Result<()>| {
            fetched.set(fetched.get() + 1);
            sink(f.sha256.as_bytes()).map_err(|_| StoreError::Io)
        };
        ensure_with(&root, &[&a], &mut fetch, &fake_hash).unwrap();
        assert_eq!(fetched.get(), 0);
        // The same size with other content: thrown away and fetched again.
        fs::write(&dest, "MODEL-BYTES").unwrap();
        ensure_with(&root, &[&a], &mut fetch, &fake_hash).unwrap();
        assert_eq!(fetched.get(), 1);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "model-bytes");
        // Another size is not even hashed.
        fs::write(&dest, "short").unwrap();
        assert_eq!(missing(&root, &[&a]).len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_first_failure_stops_the_run_but_earlier_files_stay() {
        let root = temp("stop");
        let a = file("engine/a.js", "aaa");
        let b = file("engine/b.js", "bbb");
        let c = file("engine/c.js", "ccc");
        let mut fetch = |f: &File, sink: &mut dyn FnMut(&[u8]) -> io::Result<()>| {
            if f.path.ends_with("b.js") { Err(StoreError::Network) } else { sink(f.sha256.as_bytes()).map_err(|_| StoreError::Io) }
        };
        assert_eq!(ensure_with(&root, &[&a, &b, &c], &mut fetch, &fake_hash), Err(StoreError::Network));
        assert!(path_of(&root, &a).exists() && !path_of(&root, &b).exists() && !path_of(&root, &c).exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn worker_files_are_the_embedded_copies_and_rewritten_when_they_differ() {
        let root = temp("worker");
        write_worker_files(&root).unwrap();
        assert_eq!(fs::read_to_string(root.join("worker.html")).unwrap(), crate::translate::WORKER_HTML);
        assert_eq!(fs::read_to_string(root.join("worker.js")).unwrap(), crate::translate::WORKER_JS);
        fs::write(root.join("worker.js"), "alert(1)").unwrap();
        write_worker_files(&root).unwrap();
        assert_eq!(fs::read_to_string(root.join("worker.js")).unwrap(), crate::translate::WORKER_JS, "a tampered script is replaced");
        assert!(!root.join("worker.js.part").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn paths_follow_the_manifest_under_the_cache_folder_only() {
        let root = PathBuf::from("C:/cache");
        let f = &crate::translate::MANIFEST.engine[0];
        assert!(path_of(&root, f).starts_with(&root));
        assert!(path_of(&root, f).ends_with("bergamot-translator-worker.js"));
        let route = crate::translate::pair_route("fr", "en").unwrap();
        let files = route.files();
        assert_eq!(total_bytes(&files), files.iter().map(|f| f.size).sum::<u64>());
        assert!(total_bytes(&files) > 20 * 1024 * 1024, "a pack is tens of megabytes");
    }
}
