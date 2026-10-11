//! Reader for Firefox's automatic `bookmarkbackups/*.jsonlz4` exports.

use std::{
    fs, io,
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::extensions::json::{self, Value};

use super::{Bookmark, validate_bookmark};

const MAGIC: &[u8; 8] = b"mozLz40\0";
const MAX_OUTPUT: usize = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 32;

pub fn decompress_mozlz4(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    if data.len() < 12 || data.get(..8) != Some(MAGIC) {
        return Err("Firefox bookmark backup has an invalid mozLz4 header");
    }
    let size =
        u32::from_le_bytes(data[8..12].try_into().map_err(|_| "Invalid Firefox size")?) as usize;
    if size > MAX_OUTPUT {
        return Err("Firefox bookmark backup exceeds the 16 MiB limit");
    }
    decompress_lz4_block(&data[12..], size)
}

/// Decodes one raw LZ4 block into exactly `output_size` bytes, rejecting malformed input.
pub fn decompress_lz4_block(input: &[u8], output_size: usize) -> Result<Vec<u8>, &'static str> {
    if output_size > MAX_OUTPUT {
        return Err("LZ4 output exceeds the 16 MiB limit");
    }
    let mut output = Vec::with_capacity(output_size);
    let mut cursor = 0usize;
    while cursor < input.len() {
        let token = *input.get(cursor).ok_or("Truncated LZ4 token")?;
        cursor += 1;
        let mut literals = (token >> 4) as usize;
        if literals == 15 {
            literals = extend_length(input, &mut cursor, literals)?;
        }
        let lit_end = cursor.checked_add(literals).ok_or("LZ4 length overflow")?;
        if lit_end > input.len()
            || output
                .len()
                .checked_add(literals)
                .is_none_or(|n| n > output_size)
        {
            return Err("Truncated or oversized LZ4 literals");
        }
        output.extend_from_slice(&input[cursor..lit_end]);
        cursor = lit_end;
        if cursor == input.len() {
            break;
        }
        let lo = *input.get(cursor).ok_or("Truncated LZ4 offset")? as usize;
        let hi = *input.get(cursor + 1).ok_or("Truncated LZ4 offset")? as usize;
        cursor += 2;
        let offset = lo | (hi << 8);
        if offset == 0 || offset > output.len() {
            return Err("Invalid LZ4 match offset");
        }
        let mut length = (token & 0x0f) as usize + 4;
        if token & 0x0f == 15 {
            length = extend_length(input, &mut cursor, length)?;
        }
        if output
            .len()
            .checked_add(length)
            .is_none_or(|n| n > output_size)
        {
            return Err("LZ4 output exceeds declared size");
        }
        for _ in 0..length {
            let byte = output[output.len() - offset];
            output.push(byte);
        }
    }
    if output.len() != output_size {
        return Err("LZ4 output does not match declared size");
    }
    Ok(output)
}

fn extend_length(
    input: &[u8],
    cursor: &mut usize,
    mut length: usize,
) -> Result<usize, &'static str> {
    loop {
        let byte = *input.get(*cursor).ok_or("Truncated LZ4 extended length")? as usize;
        *cursor += 1;
        length = length.checked_add(byte).ok_or("LZ4 length overflow")?;
        if byte != 255 {
            return Ok(length);
        }
    }
}

pub fn parse_bookmarks(json_text: &str) -> Result<(Vec<Bookmark>, usize), &'static str> {
    if json_text.len() > MAX_OUTPUT {
        return Err("Firefox bookmark JSON exceeds the 16 MiB limit");
    }
    let root = json::parse(json_text).ok_or("Firefox bookmark backup is not valid JSON")?;
    let mut candidates = Vec::new();
    visit(&root, "", 0, &mut candidates);
    let count = candidates.len();
    let safe = candidates
        .into_iter()
        .filter_map(validate_bookmark)
        .collect::<Vec<_>>();
    let skipped = count.saturating_sub(safe.len());
    Ok((safe, skipped))
}

fn visit(value: &Value, folder: &str, depth: usize, found: &mut Vec<Bookmark>) {
    if depth > MAX_DEPTH || found.len() >= MAX_ENTRIES {
        return;
    }
    let Value::Object(fields) = value else { return };
    let string = |key| match json::get(fields, key) {
        Some(Value::String(s)) => Some(s.as_str()),
        _ => None,
    };
    match string("type") {
        Some("text/x-moz-place") => {
            if let (Some(title), Some(uri)) = (string("title"), string("uri")) {
                found.push(Bookmark::new(title, uri, folder));
            }
        }
        Some("text/x-moz-place-container") => {
            let title = string("title").unwrap_or("");
            let next = if title.is_empty() {
                folder.to_owned()
            } else if folder.is_empty() {
                title.to_owned()
            } else {
                format!("{folder} / {title}")
            };
            if let Some(Value::Array(children)) = json::get(fields, "children") {
                for child in children {
                    visit(child, &next, depth + 1, found);
                    if found.len() >= MAX_ENTRIES {
                        break;
                    }
                }
            }
        }
        _ => {}
    }
}

/// Returns the most recently modified automatic Firefox bookmark backup, if present.
pub fn newest_backup(profile: &Path) -> Option<PathBuf> {
    let dir = profile.join("bookmarkbackups");
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let is_backup = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("jsonlz4"));
            if !is_backup || !path.is_file() {
                return None;
            }
            let modified = entry
                .metadata()
                .ok()?
                .modified()
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Some((modified, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

pub fn read_backup(path: &Path) -> Result<(Vec<Bookmark>, usize), String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => {
            let profile = crate::profile::active()
                .map_err(|_| "Firefox backup is locked and no private Bowser temp folder is available.")?;
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let temp = profile.root().join(format!(".firefox-bookmarks-{}-{nonce}.tmp", std::process::id()));
            let copied = (|| -> io::Result<Vec<u8>> {
                let mut source = fs::OpenOptions::new().read(true).share_mode(0x1 | 0x2 | 0x4).open(path)?;
                let mut target = fs::OpenOptions::new().write(true).create_new(true).share_mode(0).open(&temp)?;
                io::copy(&mut source, &mut target)?;
                target.sync_all()?;
                drop(target);
                fs::read(&temp)
            })();
            let _ = fs::remove_file(&temp);
            copied.map_err(|_| "Firefox bookmark backup is locked and could not be copied to the private Bowser temp folder.".to_owned())?
        }
        Err(_) => return Err("Firefox bookmark backup could not be read. Close Firefox if it is writing the backup, then retry.".to_owned()),
    };
    let decoded = decompress_mozlz4(&bytes).map_err(str::to_owned)?;
    let text = std::str::from_utf8(&decoded)
        .map_err(|_| "Firefox bookmark backup is not UTF-8.".to_owned())?;
    parse_bookmarks(text).map_err(str::to_owned)
}
