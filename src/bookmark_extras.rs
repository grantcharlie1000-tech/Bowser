//! A bookmark's nickname and description, kept beside the bookmark tree rather than inside it.
//!
//! `bookmarks-v2.txt` keeps its format (an older Bowser still reads it); this file, keyed by bookmark
//! id, adds the two fields the address bar's "bookmark nicknames" and the bookmark dialog use.

use std::{collections::BTreeMap, fs, io, path::Path};

use crate::bookmarks::BookmarkStore;

pub const FILE_NAME: &str = "bookmark-extras.txt";
const HEADER: &str = "bowser-bookmark-extras-v1";
/// Room for every bookmark the tree can hold (5000) with a full nickname and the longest
/// description in a 4-byte script, about 10.5 MB; `save` refuses more, so nothing written is
/// unreadable.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_NICKNAME_CHARS: usize = 24;
pub const MAX_DESCRIPTION_CHARS: usize = 512;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Extra {
    /// The word typed in the address field to open the bookmark; lower case, may be empty.
    pub nickname: String,
    pub description: String,
}

impl Extra {
    fn is_empty(&self) -> bool {
        self.nickname.is_empty() && self.description.is_empty()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Extras {
    map: BTreeMap<u64, Extra>,
}

/// A nickname as stored, or `None` when it is not allowed: empty, or up to 24 letters, digits, dots,
/// dashes and underscores (folded to lower case).
pub fn clean_nickname(text: &str) -> Option<String> {
    let text = text.trim().to_ascii_lowercase();
    (text.chars().count() <= MAX_NICKNAME_CHARS
        && text.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_')))
    .then_some(text)
}

/// A description as stored: trimmed, control characters turned into spaces, cut to the limit.
pub fn clean_description(text: &str) -> String {
    text.trim().chars().map(|c| if c.is_control() { ' ' } else { c }).take(MAX_DESCRIPTION_CHARS).collect::<String>().trim().to_owned()
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n").replace('\r', "\\r")
}

fn unescape(text: &str) -> String {
    let mut out = String::new();
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

impl Extras {
    pub fn get(&self, id: u64) -> Option<&Extra> {
        self.map.get(&id)
    }

    pub fn nickname(&self, id: u64) -> &str {
        self.map.get(&id).map_or("", |extra| extra.nickname.as_str())
    }

    /// The bookmark that owns `nickname`, other than `except`.
    pub fn owner_of(&self, nickname: &str, except: Option<u64>) -> Option<u64> {
        if nickname.is_empty() {
            return None;
        }
        self.map.iter().find(|(id, extra)| Some(**id) != except && extra.nickname == nickname).map(|(id, _)| *id)
    }

    /// Sets bookmark `id`'s fields. A nickname another bookmark has, or one that is not allowed, is
    /// refused (`Err` says why). Empty fields remove the entry.
    pub fn set(&mut self, id: u64, nickname: &str, description: &str) -> Result<(), String> {
        let nickname = clean_nickname(nickname).ok_or_else(|| format!("A nickname is up to {MAX_NICKNAME_CHARS} letters, digits, dots, dashes or underscores."))?;
        if self.owner_of(&nickname, Some(id)).is_some() {
            return Err("Another bookmark already uses that nickname.".to_owned());
        }
        let extra = Extra { nickname, description: clean_description(description) };
        if extra.is_empty() {
            self.map.remove(&id);
        } else {
            self.map.insert(id, extra);
        }
        Ok(())
    }

    pub fn remove(&mut self, id: u64) {
        self.map.remove(&id);
    }

    /// Drops entries of bookmarks that no longer exist.
    pub fn prune(&mut self, store: &BookmarkStore) {
        self.map.retain(|id, _| store.items().iter().any(|item| item.id == *id));
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn serialize(&self) -> String {
        let mut out = format!("{HEADER}\n");
        for (id, extra) in &self.map {
            out.push_str(&format!("{id}\t{}\t{}\n", escape(&extra.nickname), escape(&extra.description)));
        }
        out
    }

    pub fn parse(text: &str) -> Self {
        let mut lines = text.trim_start_matches('\u{feff}').lines();
        let mut extras = Self::default();
        if lines.next() != Some(HEADER) {
            return extras;
        }
        for line in lines {
            let mut fields = line.splitn(3, '\t');
            let (Some(id), Some(nickname), Some(description)) = (fields.next(), fields.next(), fields.next()) else { continue };
            let Ok(id) = id.parse::<u64>() else { continue };
            let Some(nickname) = clean_nickname(&unescape(nickname)) else { continue };
            if extras.owner_of(&nickname, Some(id)).is_some() {
                continue;
            }
            let extra = Extra { nickname, description: clean_description(&unescape(description)) };
            if !extra.is_empty() {
                extras.map.insert(id, extra);
            }
        }
        extras
    }

    /// The details to show: none when the file is missing or cannot be read.
    pub fn load(root: &Path) -> Self {
        Self::load_for_change(root).unwrap_or_default()
    }

    /// The details an edit rewrites: an error, never an empty map, when the file is there but cannot
    /// be read (too large, not UTF-8, not headed as this file; a BOM is allowed), since saving what
    /// was loaded without it would delete every nickname and description.
    pub fn load_for_change(root: &Path) -> io::Result<Self> {
        Ok(read_for_change(&root.join(FILE_NAME))?.map_or_else(Self::default, |text| Self::parse(&text)))
    }

    /// Replaces the file (temp file, fsync, rename). Refused when the text would be too large to read
    /// back, or when the file there cannot be read (see [`Self::load_for_change`]).
    pub fn save(&self, root: &Path) -> io::Result<()> {
        let text = self.serialize();
        if text.len() as u64 > MAX_FILE_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "bookmark details are too large to save"));
        }
        let path = root.join(FILE_NAME);
        read_for_change(&path)?;
        crate::productivity::atomic_write(&path, text.as_bytes())
    }
}

/// The file's text, `None` when there is none (missing or blank); an error when it cannot be read.
fn read_for_change(path: &Path) -> io::Result<Option<String>> {
    let invalid = |why: &str| io::Error::new(io::ErrorKind::InvalidData, format!("{} {why}", path.display()));
    let bytes = match fs::metadata(path) {
        Ok(meta) if meta.len() > MAX_FILE_BYTES => return Err(invalid("is too large to read")),
        Ok(_) => fs::read(path)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(invalid("is too large to read"));
    }
    let text = String::from_utf8(bytes).map_err(|_| invalid("is not UTF-8 text"))?;
    let body = text.trim_start_matches('\u{feff}');
    if body.trim().is_empty() {
        return Ok(None);
    }
    if body.lines().next() != Some(HEADER) {
        return Err(invalid("is not a file Bowser can read"));
    }
    Ok(Some(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nicknames_are_folded_validated_and_unique() {
        let mut extras = Extras::default();
        extras.set(1, "Rust", "The language").unwrap();
        assert_eq!(extras.nickname(1), "rust");
        assert_eq!(extras.owner_of("rust", None), Some(1));
        assert_eq!(extras.owner_of("rust", Some(1)), None);
        assert!(extras.set(2, "RUST", "").is_err(), "a nickname is used once");
        assert!(extras.set(2, "a b", "").is_err());
        assert!(extras.set(2, &"x".repeat(25), "").is_err());
        extras.set(2, "", "just a description").unwrap();
        assert_eq!(extras.get(2).unwrap().description, "just a description");
        extras.set(2, "", "").unwrap();
        assert!(extras.get(2).is_none(), "empty fields remove the entry");
        extras.set(1, "rust", "again").unwrap();
        assert_eq!(extras.get(1).unwrap().description, "again", "a bookmark may keep its own nickname");
    }

    #[test]
    fn the_file_round_trips_with_tabs_and_newlines_in_descriptions() {
        let mut extras = Extras::default();
        extras.set(7, "w", "line\nbreak\tand a \\ slash").unwrap();
        extras.set(9, "", "only text").unwrap();
        let back = Extras::parse(&extras.serialize());
        assert_eq!(back.get(7).unwrap().description, "line break and a \\ slash");
        assert_eq!(back.nickname(7), "w");
        assert_eq!(Extras::parse("nonsense\n7\tw\td\n"), Extras::default(), "no header, nothing read");
        let hostile = "bowser-bookmark-extras-v1\n1\tOK\td\n2\tok\td2\nx\tz\td\n3\tb d\td\n";
        let parsed = Extras::parse(hostile);
        assert_eq!(parsed.len(), 1, "a repeated nickname, a bad id and a bad nickname are dropped");
        assert_eq!(parsed.nickname(1), "ok");
    }

    #[test]
    fn pruning_forgets_deleted_bookmarks_and_the_file_survives_disk() {
        let mut store = BookmarkStore::new();
        let kept = store.add_bookmark(None, "Rust", "https://www.rust-lang.org/").unwrap();
        let mut extras = Extras::default();
        extras.set(kept, "rl", "").unwrap();
        extras.set(kept + 50, "gone", "").unwrap();
        extras.prune(&store);
        assert_eq!(extras.len(), 1);
        let dir = std::env::temp_dir().join(format!("bowser-extras-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        extras.save(&dir).unwrap();
        assert_eq!(Extras::load(&dir), extras);
        assert_eq!(Extras::load(&dir.join("missing")), Extras::default());
        let _ = fs::remove_dir_all(&dir);
    }

    fn test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bowser-extras-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// R-4: 400 descriptions in a non-Latin script pass 512 KiB. They must all read back, and the
    /// next nickname edit must keep them.
    #[test]
    fn details_past_512_kib_read_back_and_survive_the_next_edit() {
        let dir = test_dir("large");
        let mut extras = Extras::default();
        for id in 1..=400u64 {
            extras.set(id, &format!("n{id}"), &"\u{4e2d}".repeat(MAX_DESCRIPTION_CHARS)).unwrap();
        }
        extras.save(&dir).unwrap();
        assert!(fs::metadata(dir.join(FILE_NAME)).unwrap().len() > 512 * 1024, "the fixture passes the old cap");
        let mut loaded = Extras::load(&dir);
        assert_eq!(loaded.len(), 400);
        loaded.set(9999, "new", "").unwrap();
        loaded.save(&dir).unwrap();
        assert_eq!(Extras::load(&dir).len(), 401);
        let _ = fs::remove_dir_all(&dir);
    }

    /// The most one bookmark tree can hold (5000 bookmarks, each with a full nickname and the
    /// longest description) still fits under the read cap, so nothing saved is unreadable.
    #[test]
    fn a_full_tree_of_details_fits_under_the_read_cap() {
        let mut extras = Extras::default();
        for id in 1..=5000u64 {
            let nickname = format!("{id:0>24}");
            extras.set(u64::MAX - id, &nickname, &"\u{1f600}".repeat(MAX_DESCRIPTION_CHARS)).unwrap();
        }
        assert!(extras.serialize().len() as u64 <= MAX_FILE_BYTES);
    }

    /// R-4: a file with a BOM is read; one that is not UTF-8 is never replaced by what was loaded
    /// without it.
    #[test]
    fn an_unreadable_details_file_is_never_saved_over() {
        let dir = test_dir("unreadable");
        let path = dir.join(FILE_NAME);
        fs::write(&path, "\u{feff}bowser-bookmark-extras-v1\n1\tmail\twork inbox\n").unwrap();
        assert_eq!(Extras::load(&dir).nickname(1), "mail", "a BOM is allowed");
        let ansi = b"bowser-bookmark-extras-v1\n1\tmail\twork inbox\n2\tcafe\tCaf\xe9\n".to_vec();
        fs::write(&path, &ansi).unwrap();
        let mut extras = Extras::load(&dir);
        extras.set(3, "new", "").unwrap();
        assert!(extras.save(&dir).is_err(), "the save is refused");
        assert_eq!(fs::read(&path).unwrap(), ansi, "the file is left as it was");
        let _ = fs::remove_dir_all(&dir);
    }
}
