//! Atomic storage for imports using the shared simple UTF-8 line formats.

use std::{fs, io, path::Path};

use super::{Bookmark, HistoryEntry, dedupe_bookmarks, encode_history, parse_history_store};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub imported: usize,
    pub duplicates: usize,
    pub skipped: usize,
    pub failed: usize,
    pub reasons: Vec<String>,
}

pub fn read_bookmark_file(path: &Path) -> Result<(Vec<Bookmark>, usize), String> {
    let bytes = fs::read(path).map_err(|_| "The selected file could not be read. Close the source browser if it is holding the file open, then try again.".to_owned())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("The selected bookmarks file is larger than 32 MiB.".into());
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| "The selected bookmarks file is not UTF-8.".to_owned())?;
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" if bytes.len() > 16 * 1024 * 1024 => {
            Err("The selected Netscape HTML file is larger than 16 MiB.".into())
        }
        "html" | "htm" => Ok(super::bookmarks_html::parse_netscape_bookmarks_detailed(
            text,
        )),
        _ => super::chromium::parse_bookmarks_detailed(text).map_err(str::to_owned),
    }
}

/// The profile's bookmarks, in the import shape, for de-duplicating a preview. `path` is the
/// profile's `bookmarks.txt`; the bookmarks themselves live in the tree beside it.
pub fn existing_bookmarks(path: &Path) -> Vec<Bookmark> {
    crate::productivity::read_bookmarks(path)
        .into_iter()
        .map(|(title, url, folder)| Bookmark::new(title, url, folder))
        .collect()
}

/// Adds `incoming` to the profile's bookmark tree (`bookmarks-v2.txt` next to `path`, the
/// profile's `bookmarks.txt`), the one store the bar, panel, suggestions and palette read.
pub fn import_bookmarks(path: &Path, incoming: Vec<Bookmark>) -> Result<ImportSummary, String> {
    let profile_dir = path
        .parent()
        .ok_or_else(|| "Bowser could not find its profile folder.".to_owned())?;
    let mut tree = crate::bookmarks::BookmarkStore::load(profile_dir)
        .map_err(|_| "Bowser could not read its existing bookmarks.".to_owned())?;
    let existing = tree
        .flat_links()
        .into_iter()
        .map(|(title, url, folder)| Bookmark::new(title, url, folder))
        .collect::<Vec<_>>();
    let (unique, duplicates) = dedupe_bookmarks(incoming, &existing);
    // Import folders are " / "-joined; the tree splits on '/' and trims each name.
    let counts = tree.import_links(
        unique
            .iter()
            .map(|b| (b.folder.as_str(), b.title.as_str(), b.url.as_str())),
    );
    if counts.added > 0 {
        tree.save(profile_dir)
            .map_err(|_| "Bowser could not save imported bookmarks.".to_owned())?;
    }
    let mut reasons = Vec::new();
    if counts.limited > 0 {
        reasons.push(format!(
            "{} bookmark(s) were not imported because the bookmark list is full.",
            counts.limited
        ));
    }
    Ok(ImportSummary {
        imported: counts.added,
        duplicates: duplicates + counts.duplicates,
        skipped: counts.skipped + counts.limited,
        failed: 0,
        reasons,
    })
}

pub fn import_history(path: &Path, incoming: Vec<HistoryEntry>) -> Result<ImportSummary, String> {
    let existing = match fs::read_to_string(path) {
        Ok(text) => parse_history_store(&text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(_) => return Err("Bowser could not read its existing history.".into()),
    };
    let mut seen = existing
        .iter()
        .map(|h| (h.unix_ts, h.url.clone()))
        .collect::<std::collections::HashSet<_>>();
    let mut unique = Vec::new();
    let mut duplicates = 0;
    let mut skipped = 0;
    for entry in incoming {
        if !matches!(
            url::Url::parse(&entry.url)
                .ok()
                .map(|u| u.scheme().to_owned())
                .as_deref(),
            Some("http" | "https")
        ) {
            skipped += 1;
            continue;
        }
        if seen.insert((entry.unix_ts, entry.url.clone())) {
            unique.push(entry);
        } else {
            duplicates += 1;
        }
    }
    let mut all = existing;
    all.extend(unique.iter().cloned());
    super::atomic_write(path, &encode_history(&all))
        .map_err(|_| "Bowser could not save imported history.".to_owned())?;
    let reasons = if skipped > 0 {
        vec!["Some history entries had an unsupported URL scheme.".to_owned()]
    } else {
        Vec::new()
    };
    Ok(ImportSummary {
        imported: unique.len(),
        duplicates,
        skipped,
        failed: 0,
        reasons,
    })
}

pub fn read_store(path: &Path) -> io::Result<(Vec<Bookmark>, Vec<HistoryEntry>)> {
    let bookmarks = existing_bookmarks(&path.with_file_name("bookmarks.txt"));
    let history = match fs::read_to_string(path.with_file_name("history.txt")) {
        Ok(text) => parse_history_store(&text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    Ok((bookmarks, history))
}
