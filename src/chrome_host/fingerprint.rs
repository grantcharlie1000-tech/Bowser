//! Shared FNV-1a fingerprint for the embedded UI asset guard.
use std::{fs, io, path::{Path, PathBuf}};

const OFFSET: u64 = 0xcbf29ce484222325;
const PRIME: u64 = 0x100000001b3;

/// Hash each file's relative UTF-8 path, a NUL separator, then its bytes, in path order.
pub fn tree_fingerprint(root: &Path) -> io::Result<u64> {
    fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                collect(root, &path, files)?;
            } else if path.is_file() {
                let relative = path.strip_prefix(root).expect("descendant")
                    .to_str().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 UI path"))?
                    .replace('\\', "/");
                files.push((relative, path));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    collect(root, root, &mut files)?;
    files.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let mut hash = OFFSET;
    for (relative, path) in files {
        for byte in relative.bytes().chain(std::iter::once(0)).chain(fs::read(path)?.into_iter()) {
            hash = (hash ^ u64::from(byte)).wrapping_mul(PRIME);
        }
    }
    Ok(hash)
}

pub fn manifest_value(manifest: &str, key: &str) -> Option<u64> {
    let value = manifest.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix('='))?;
    (value.len() == 16 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| u64::from_str_radix(value, 16).ok()).flatten()
}
