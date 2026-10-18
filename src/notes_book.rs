//! Notes (thread T7): the pure model behind the Notes panel's folders, sort orders, appending,
//! notes made from a selection or a capture, and export and import as Markdown or plain text.
//!
//! `notes.txt` keeps its older format byte for byte (`bowser-notes-v1`: id, title, body), so an
//! older Bowser reads the same file. What the older format has no place for (where a note came from,
//! when it was made, which folder it is in, the folders themselves and the sort order) is kept in
//! `notes-meta.txt` beside it. A note with no entry there is simply unfiled and undated. Nothing here
//! touches the network, and an imported file is read as text only.

use std::collections::HashMap;

use crate::productivity::{Note, escape, unescape};

/// Most notes (the same limit the Notes panel has always had).
pub const MAX_NOTES: usize = 100;
/// Longest note text, in characters. A wire command carries at most 4096.
pub const MAX_BODY_CHARS: usize = 4000;
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_FOLDERS: usize = 50;
pub const MAX_FOLDER_NAME: usize = 60;
/// Largest file an import reads.
pub const MAX_IMPORT_BYTES: u64 = 256 * 1024;
const MAX_META_BYTES: usize = 64 * 1024;
const META_HEADER: &str = "bowser-notes-meta-v1";

/// How the panel lists notes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Sort {
    /// The order they are kept in, which the user changes by moving a note.
    #[default]
    Manual,
    Title,
    Address,
    Content,
    /// Newest first.
    Date,
}

impl Sort {
    pub const ALL: [Self; 5] = [Self::Manual, Self::Title, Self::Address, Self::Content, Self::Date];
    pub fn key(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Title => "title",
            Self::Address => "address",
            Self::Content => "content",
            Self::Date => "date",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|sort| sort.key() == value)
    }
}

/// What the older note format has no room for.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Meta {
    /// Seconds since 1970; 0 when unknown.
    pub created: u64,
    /// The folder's id, 0 for none.
    pub folder: u64,
    /// The page the note came from, or empty.
    pub addr: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Folder {
    pub id: u64,
    pub name: String,
}

/// Why a change was refused. The text is what the panel shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refused {
    Full,
    TooLong,
    NoNote,
    NoFolder,
    BadName,
    FoldersFull,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Full => "There is room for 100 notes. Delete one first.",
            Self::TooLong => "That note would be longer than 4000 characters.",
            Self::NoNote => "That note is no longer there.",
            Self::NoFolder => "That folder is no longer there.",
            Self::BadName => "A folder needs a name of up to 60 characters.",
            Self::FoldersFull => "There is room for 50 folders.",
        })
    }
}

/// The notes, their folders and the sort order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Book {
    pub notes: Vec<Note>,
    pub folders: Vec<Folder>,
    pub meta: HashMap<u64, Meta>,
    pub sort: Sort,
}

/// `text` cut to at most `max` characters (on a character boundary).
pub fn clip_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// `text` as one line (runs of whitespace, newlines included, become one space), cut to `max` characters.
pub fn plain_line(text: &str, max: usize) -> String {
    clip_chars(&text.split_whitespace().collect::<Vec<_>>().join(" "), max)
}

/// A folder name: one line, no tabs, 1 to 60 characters.
pub fn folder_name(text: &str) -> Option<String> {
    let name = plain_line(text, MAX_FOLDER_NAME);
    (!name.is_empty()).then_some(name)
}

/// A title and a body for a note made from `selection` on a page called `page_title`: the first line
/// of the selection is the title (the page's title when the selection has no text), and the text is
/// kept whole up to the note limit.
pub fn from_selection(selection: &str, page_title: &str) -> (String, String) {
    let body = selection.replace("\r\n", "\n").replace('\r', "\n");
    let body = body.trim();
    let first = body.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    let title = if first.is_empty() { plain_line(page_title, MAX_TITLE_CHARS) } else { plain_line(first, 60) };
    (if title.is_empty() { "Note".to_owned() } else { title }, clip_chars(body, MAX_BODY_CHARS))
}

/// A title and a body for a note that holds a capture saved at `file` (a `file:` address).
pub fn from_capture(page_title: &str, file_url: &str) -> (String, String) {
    let page = plain_line(page_title, 120);
    let title = if page.is_empty() { "Capture".to_owned() } else { format!("Capture: {page}") };
    (title, clip_chars(&format!("![Capture]({file_url})"), MAX_BODY_CHARS))
}

impl Book {
    /// The book for the notes read from `notes.txt` and the text of `notes-meta.txt`. Entries for notes
    /// that are gone, and folders no note names, are kept only while the folder exists.
    pub fn from_parts(notes: Vec<Note>, meta_text: &str) -> Self {
        let mut book = Self { notes, ..Self::default() };
        book.read_meta(meta_text);
        book
    }

    fn read_meta(&mut self, text: &str) {
        if text.len() > MAX_META_BYTES {
            return;
        }
        let mut lines = text.lines();
        if lines.next() != Some(META_HEADER) {
            return;
        }
        let live: std::collections::HashSet<u64> = self.notes.iter().map(|note| note.id).collect();
        for line in lines {
            let mut fields = line.split('\t');
            match fields.next() {
                Some("sort") => {
                    if let Some(sort) = fields.next().and_then(Sort::parse) {
                        self.sort = sort;
                    }
                }
                Some("folder") => {
                    let (Some(id), Some(name)) = (fields.next().and_then(|id| id.parse::<u64>().ok()), fields.next()) else { continue };
                    if id != 0 && self.folders.len() < MAX_FOLDERS && !self.folders.iter().any(|f| f.id == id) {
                        if let Some(name) = folder_name(&unescape(name)) {
                            self.folders.push(Folder { id, name });
                        }
                    }
                }
                Some("note") => {
                    let (Some(id), Some(created), Some(folder), Some(addr)) = (
                        fields.next().and_then(|v| v.parse::<u64>().ok()),
                        fields.next().and_then(|v| v.parse::<u64>().ok()),
                        fields.next().and_then(|v| v.parse::<u64>().ok()),
                        fields.next(),
                    ) else {
                        continue;
                    };
                    if live.contains(&id) {
                        self.meta.insert(id, Meta { created, folder, addr: clip_chars(&unescape(addr), 2048) });
                    }
                }
                _ => {}
            }
        }
        // A note never points at a folder that is not there.
        let folders: std::collections::HashSet<u64> = self.folders.iter().map(|f| f.id).collect();
        for meta in self.meta.values_mut() {
            if !folders.contains(&meta.folder) {
                meta.folder = 0;
            }
        }
    }

    /// The text of `notes-meta.txt`. Entries are written for notes that have something to say.
    pub fn meta_text(&self) -> String {
        let mut out = format!("{META_HEADER}\nsort\t{}\n", self.sort.key());
        for folder in self.folders.iter().take(MAX_FOLDERS) {
            out.push_str(&format!("folder\t{}\t{}\n", folder.id, escape(&folder.name)));
        }
        for note in self.notes.iter().take(MAX_NOTES) {
            if let Some(meta) = self.meta.get(&note.id)
                && (meta.created != 0 || meta.folder != 0 || !meta.addr.is_empty())
            {
                out.push_str(&format!("note\t{}\t{}\t{}\t{}\n", note.id, meta.created, meta.folder, escape(&meta.addr)));
            }
        }
        out
    }

    pub fn meta_of(&self, id: u64) -> Meta {
        self.meta.get(&id).cloned().unwrap_or_default()
    }

    fn next_id(&self, now_micros: u64) -> u64 {
        self.notes.iter().map(|n| n.id).chain(self.folders.iter().map(|f| f.id)).max().unwrap_or(0).saturating_add(1).max(now_micros)
    }

    fn note_index(&self, id: u64) -> Result<usize, Refused> {
        self.notes.iter().position(|note| note.id == id).ok_or(Refused::NoNote)
    }

    /// Adds a note at the end and returns its id.
    pub fn add(&mut self, title: &str, body: &str, addr: &str, folder: u64, now_secs: u64, now_micros: u64) -> Result<u64, Refused> {
        if self.notes.len() >= MAX_NOTES {
            return Err(Refused::Full);
        }
        let folder = if self.folders.iter().any(|f| f.id == folder) { folder } else { 0 };
        let id = self.next_id(now_micros);
        let title = plain_line(title, MAX_TITLE_CHARS);
        self.notes.push(Note { id, title: if title.is_empty() { "Untitled note".to_owned() } else { title }, body: clip_chars(body, MAX_BODY_CHARS) });
        self.meta.insert(id, Meta { created: now_secs, folder, addr: clip_chars(addr, 2048) });
        Ok(id)
    }

    /// Adds `text` to the end of the note `id`, after a blank line.
    pub fn append(&mut self, id: u64, text: &str) -> Result<(), Refused> {
        let at = self.note_index(id)?;
        let text = text.replace("\r\n", "\n");
        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }
        let note = &mut self.notes[at];
        let joined = if note.body.trim().is_empty() { text.to_owned() } else { format!("{}\n\n{}", note.body.trim_end(), text) };
        if joined.chars().count() > MAX_BODY_CHARS {
            return Err(Refused::TooLong);
        }
        note.body = joined;
        Ok(())
    }

    pub fn set_folder(&mut self, id: u64, folder: u64) -> Result<(), Refused> {
        self.note_index(id)?;
        if folder != 0 && !self.folders.iter().any(|f| f.id == folder) {
            return Err(Refused::NoFolder);
        }
        self.meta.entry(id).or_default().folder = folder;
        Ok(())
    }

    pub fn add_folder(&mut self, name: &str, now_micros: u64) -> Result<u64, Refused> {
        let name = folder_name(name).ok_or(Refused::BadName)?;
        if self.folders.len() >= MAX_FOLDERS {
            return Err(Refused::FoldersFull);
        }
        if let Some(found) = self.folders.iter().find(|f| f.name.eq_ignore_ascii_case(&name)) {
            return Ok(found.id);
        }
        let id = self.next_id(now_micros);
        self.folders.push(Folder { id, name });
        Ok(id)
    }

    pub fn rename_folder(&mut self, id: u64, name: &str) -> Result<(), Refused> {
        let name = folder_name(name).ok_or(Refused::BadName)?;
        self.folders.iter_mut().find(|f| f.id == id).ok_or(Refused::NoFolder)?.name = name;
        Ok(())
    }

    /// Removes a folder. Its notes are kept, unfiled.
    pub fn delete_folder(&mut self, id: u64) -> Result<(), Refused> {
        let before = self.folders.len();
        self.folders.retain(|f| f.id != id);
        if self.folders.len() == before {
            return Err(Refused::NoFolder);
        }
        for meta in self.meta.values_mut() {
            if meta.folder == id {
                meta.folder = 0;
            }
        }
        Ok(())
    }

    pub fn delete_note(&mut self, id: u64) -> bool {
        let before = self.notes.len();
        self.notes.retain(|note| note.id != id);
        self.meta.remove(&id);
        before != self.notes.len()
    }

    /// Moves a note to just before `before` (the end when `None`) in the manual order.
    pub fn move_note(&mut self, id: u64, before: Option<u64>) -> Result<(), Refused> {
        let from = self.note_index(id)?;
        let note = self.notes.remove(from);
        let at = before.and_then(|b| self.notes.iter().position(|n| n.id == b)).unwrap_or(self.notes.len());
        self.notes.insert(at, note);
        Ok(())
    }

    /// The notes in the order `sort` gives: manual is the stored order; title and content compare
    /// without regard to case; address groups by page (notes with none last); date is newest first.
    pub fn ordered(&self, sort: Sort) -> Vec<&Note> {
        let mut list: Vec<&Note> = self.notes.iter().collect();
        let lower = |text: &str| text.to_lowercase();
        match sort {
            Sort::Manual => {}
            Sort::Title => list.sort_by_cached_key(|n| (lower(&n.title), n.id)),
            Sort::Content => list.sort_by_cached_key(|n| (lower(n.body.trim()), n.id)),
            Sort::Address => list.sort_by_cached_key(|n| {
                let addr = self.meta.get(&n.id).map(|m| m.addr.to_lowercase()).unwrap_or_default();
                (addr.is_empty(), addr, lower(&n.title), n.id)
            }),
            Sort::Date => list.sort_by_cached_key(|n| (std::cmp::Reverse(self.meta.get(&n.id).map_or(0, |m| m.created)), std::cmp::Reverse(n.id))),
        }
        list
    }

    /// The notes of one folder (0 for the unfiled ones) in the book's sort order.
    pub fn in_folder(&self, folder: u64) -> Vec<&Note> {
        self.ordered(self.sort).into_iter().filter(|n| self.meta_of(n.id).folder == folder).collect()
    }
}

// ---------------------------------------------------------------------------------------------
// Export and import
// ---------------------------------------------------------------------------------------------

/// The file kinds notes are exported to and imported from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileKind {
    Markdown,
    Text,
}

impl FileKind {
    pub fn of_path(path: &std::path::Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "md" | "markdown" => Some(Self::Markdown),
            "txt" => Some(Self::Text),
            _ => None,
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Text => "txt",
        }
    }
}

/// What to export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scope {
    All,
    Folder(u64),
    One(u64),
}

impl Book {
    /// The notes of `scope` as the text of a file, in the book's order.
    ///
    /// Markdown: a folder is a `# heading`, a note a `## heading` followed by its address (as an
    /// `<address>` line) and its text; one note on its own is a `# heading`. Text: each note is its
    /// title, a line of `=`, then its text, separated by a line of `-`.
    pub fn export(&self, scope: Scope, kind: FileKind) -> String {
        let picked: Vec<&Note> = self
            .ordered(self.sort)
            .into_iter()
            .filter(|n| match scope {
                Scope::All => true,
                Scope::Folder(id) => self.meta_of(n.id).folder == id,
                Scope::One(id) => n.id == id,
            })
            .collect();
        let mut out = String::new();
        match kind {
            FileKind::Text => {
                for (index, note) in picked.iter().enumerate() {
                    if index > 0 {
                        out.push_str("\n---\n\n");
                    }
                    out.push_str(&format!("{}\n{}\n\n{}\n", note.title, "=".repeat(note.title.chars().count().clamp(3, 80)), note.body));
                }
            }
            FileKind::Markdown => {
                let single = matches!(scope, Scope::One(_));
                let mut groups: Vec<(Option<&Folder>, Vec<&Note>)> = Vec::new();
                if !single {
                    for folder in &self.folders {
                        let notes: Vec<&Note> = picked.iter().copied().filter(|n| self.meta_of(n.id).folder == folder.id).collect();
                        if !notes.is_empty() {
                            groups.push((Some(folder), notes));
                        }
                    }
                }
                let unfiled: Vec<&Note> = picked.iter().copied().filter(|n| single || self.meta_of(n.id).folder == 0 || !self.folders.iter().any(|f| f.id == self.meta_of(n.id).folder)).collect();
                if !unfiled.is_empty() {
                    groups.insert(0, (None, unfiled));
                }
                for (folder, notes) in groups {
                    if let Some(folder) = folder {
                        out.push_str(&format!("# {}\n\n", folder.name));
                    }
                    for note in notes {
                        out.push_str(&format!("{} {}\n\n", if single { "#" } else { "##" }, note.title));
                        let addr = self.meta_of(note.id).addr;
                        if !addr.is_empty() {
                            out.push_str(&format!("<{addr}>\n\n"));
                        }
                        out.push_str(note.body.trim_end());
                        out.push_str("\n\n");
                    }
                }
            }
        }
        out
    }
}

/// A note read from a file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Imported {
    pub folder: Option<String>,
    pub title: String,
    pub body: String,
    pub addr: String,
}

/// Reads the notes in a file's text. `name` is the file's name without its extension, the title of a
/// file that has no headings. Markdown: with `## ` headings, each is a note and a `# ` heading is the
/// folder of the notes after it; without, each `# ` heading is a note; without any heading the whole
/// file is one note. Text: one note, titled by its first line.
pub fn parse_import(text: &str, kind: FileKind, name: &str) -> Vec<Imported> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let one = |title: &str, body: &str| Imported { folder: None, title: plain_line(title, MAX_TITLE_CHARS), body: clip_chars(body.trim(), MAX_BODY_CHARS), addr: String::new() };
    let titled = |body: &str| -> Imported {
        let (title, body) = from_selection(body, name);
        Imported { folder: None, title, body, addr: String::new() }
    };
    if kind == FileKind::Text {
        // A file made by the text export holds several notes: a title, a line of =, then the text.
        let parts: Vec<&str> = text.split("\n---\n").collect();
        let notes: Vec<Imported> = parts
            .iter()
            .filter_map(|part| {
                let mut lines = part.trim().lines();
                let title = lines.next()?.trim();
                let underline = lines.next()?;
                (!title.is_empty() && underline.len() >= 3 && underline.chars().all(|c| c == '=')).then(|| one(title, &lines.collect::<Vec<_>>().join("\n")))
            })
            .collect();
        return if notes.len() == parts.len() && !notes.is_empty() { notes } else if text.trim().is_empty() { Vec::new() } else { vec![titled(&text)] };
    }
    let has_notes = text.lines().any(|line| line.starts_with("## "));
    let (note_marker, folder_marker) = if has_notes { ("## ", Some("# ")) } else { ("# ", None) };
    let mut out: Vec<Imported> = Vec::new();
    let mut folder: Option<String> = None;
    let mut current: Option<(String, Vec<&str>)> = None;
    let flush = |current: &mut Option<(String, Vec<&str>)>, folder: &Option<String>, out: &mut Vec<Imported>| {
        if let Some((title, lines)) = current.take() {
            let mut body = lines.join("\n");
            let mut addr = String::new();
            // An address line written by the export: `<https://...>` before the text.
            let trimmed = body.trim_start();
            if let Some(rest) = trimmed.strip_prefix('<')
                && let Some((link, after)) = rest.split_once(">\n")
                && (link.starts_with("http://") || link.starts_with("https://"))
                && !link.contains(char::is_whitespace)
            {
                addr = clip_chars(link, 2048);
                body = after.to_owned();
            }
            out.push(Imported { folder: folder.clone(), title: plain_line(&title, MAX_TITLE_CHARS), body: clip_chars(body.trim(), MAX_BODY_CHARS), addr });
        }
    };
    let mut preface: Vec<&str> = Vec::new();
    for line in text.lines() {
        if let Some(title) = line.strip_prefix(note_marker) {
            flush(&mut current, &folder, &mut out);
            current = Some((title.to_owned(), Vec::new()));
        } else if let Some(name) = folder_marker.and_then(|marker| line.strip_prefix(marker)) {
            flush(&mut current, &folder, &mut out);
            folder = folder_name(name);
        } else if let Some((_, lines)) = current.as_mut() {
            lines.push(line);
        } else {
            preface.push(line);
        }
    }
    flush(&mut current, &folder, &mut out);
    if out.is_empty() && !text.trim().is_empty() {
        return vec![titled(&text)];
    }
    // Text before the first heading is a note of its own when it says something.
    if !preface.join("\n").trim().is_empty() {
        out.insert(0, titled(&preface.join("\n")));
    }
    out.retain(|note| !note.title.is_empty() || !note.body.is_empty());
    for note in &mut out {
        if note.title.is_empty() {
            note.title = plain_line(name, MAX_TITLE_CHARS);
        }
    }
    out
}

impl Book {
    /// Adds the notes of an import (and their folders) as far as there is room. Returns how many were
    /// added and how many were left out.
    pub fn import(&mut self, items: Vec<Imported>, now_secs: u64, now_micros: u64) -> (usize, usize) {
        let (mut added, mut left_out) = (0, 0);
        for (index, item) in items.into_iter().enumerate() {
            let folder = item.folder.as_deref().and_then(|name| self.add_folder(name, now_micros + index as u64 + 1).ok()).unwrap_or(0);
            match self.add(&item.title, &item.body, &item.addr, folder, now_secs, now_micros + 1000 + index as u64) {
                Ok(_) => added += 1,
                Err(_) => left_out += 1,
            }
        }
        (added, left_out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: u64, title: &str, body: &str) -> Note {
        Note { id, title: title.into(), body: body.into() }
    }

    fn book() -> Book {
        let mut book = Book::from_parts(vec![note(1, "banana", "yellow fruit"), note(2, "Apple", "red fruit"), note(3, "cherry", "a stone fruit")], "");
        book.meta.insert(1, Meta { created: 300, folder: 0, addr: "https://b.example/page".into() });
        book.meta.insert(2, Meta { created: 100, folder: 0, addr: "https://a.example/".into() });
        book
    }

    fn ids(list: Vec<&Note>) -> Vec<u64> {
        list.into_iter().map(|n| n.id).collect()
    }

    #[test]
    fn each_sort_order_orders_as_it_says() {
        let book = book();
        assert_eq!(ids(book.ordered(Sort::Manual)), [1, 2, 3]);
        assert_eq!(ids(book.ordered(Sort::Title)), [2, 1, 3], "case does not matter");
        assert_eq!(ids(book.ordered(Sort::Address)), [2, 1, 3], "a.example, b.example, then the note with no address");
        assert_eq!(ids(book.ordered(Sort::Content)), [3, 2, 1], "a stone, red, yellow");
        assert_eq!(ids(book.ordered(Sort::Date)), [1, 2, 3], "newest first; an undated note is oldest");
        let mut dated = book.clone();
        dated.meta.insert(3, Meta { created: 900, ..Meta::default() });
        assert_eq!(ids(dated.ordered(Sort::Date)), [3, 1, 2]);
    }

    #[test]
    fn manual_order_changes_when_a_note_moves() {
        let mut book = book();
        book.move_note(3, Some(1)).unwrap();
        assert_eq!(ids(book.ordered(Sort::Manual)), [3, 1, 2]);
        book.move_note(3, None).unwrap();
        assert_eq!(ids(book.ordered(Sort::Manual)), [1, 2, 3]);
        book.move_note(1, Some(99)).unwrap();
        assert_eq!(ids(book.ordered(Sort::Manual)), [2, 3, 1], "an unknown anchor means the end");
        assert_eq!(book.move_note(77, None), Err(Refused::NoNote));
    }

    #[test]
    fn a_note_from_a_selection_is_titled_by_its_first_line() {
        assert_eq!(from_selection("  Hello world\r\nsecond line  ", "Page"), ("Hello world".to_owned(), "Hello world\nsecond line".to_owned()));
        assert_eq!(from_selection("   \n  ", "A page").0, "A page");
        assert_eq!(from_selection("", "").0, "Note");
        let long = from_selection(&"word ".repeat(5000), "p");
        assert_eq!(long.1.chars().count(), MAX_BODY_CHARS);
        assert!(long.0.chars().count() <= 60);
        assert_eq!(from_capture("Docs", "file:///C:/Users/x/cap.png"), ("Capture: Docs".to_owned(), "![Capture](file:///C:/Users/x/cap.png)".to_owned()));
        assert_eq!(from_capture("", "file:///a.png").0, "Capture");
    }

    #[test]
    fn adding_stamps_the_address_and_date_and_respects_the_limits() {
        let mut book = Book::default();
        let id = book.add("  A   title ", "text", "https://x.example/", 0, 1000, 5).unwrap();
        assert_eq!(book.notes[0].title, "A title");
        assert_eq!(book.meta_of(id), Meta { created: 1000, folder: 0, addr: "https://x.example/".into() });
        assert_eq!(book.add("", "", "", 0, 1, 6).map(|id| book.notes.iter().find(|n| n.id == id).unwrap().title.clone()), Ok("Untitled note".to_owned()));
        for n in 0..MAX_NOTES - 2 {
            book.add("n", "", "", 0, 1, 10 + n as u64).unwrap();
        }
        assert_eq!(book.add("one more", "", "", 0, 1, 999), Err(Refused::Full));
        assert!(book.notes.iter().map(|n| n.id).collect::<std::collections::HashSet<_>>().len() == MAX_NOTES, "ids are unique");
    }

    #[test]
    fn appending_joins_with_a_blank_line_and_stops_at_the_limit() {
        let mut book = book();
        book.append(1, "  more  ").unwrap();
        assert_eq!(book.notes[0].body, "yellow fruit\n\nmore");
        book.append(1, "   ").unwrap();
        assert_eq!(book.notes[0].body, "yellow fruit\n\nmore", "nothing to add changes nothing");
        book.notes[2].body.clear();
        book.append(3, "first").unwrap();
        assert_eq!(book.notes[2].body, "first", "no blank line before the first text");
        assert_eq!(book.append(1, &"x".repeat(MAX_BODY_CHARS)), Err(Refused::TooLong));
        assert_eq!(book.notes[0].body, "yellow fruit\n\nmore", "a refused append leaves the note alone");
        assert_eq!(book.append(42, "x"), Err(Refused::NoNote));
    }

    #[test]
    fn folders_hold_notes_and_deleting_one_keeps_its_notes() {
        let mut book = book();
        let work = book.add_folder("  Work  ", 50).unwrap();
        assert_eq!(book.add_folder("work", 51), Ok(work), "the same name is the same folder");
        assert_eq!(book.add_folder("   ", 52), Err(Refused::BadName));
        book.set_folder(1, work).unwrap();
        book.set_folder(2, work).unwrap();
        assert_eq!(ids(book.in_folder(work)), [1, 2]);
        assert_eq!(ids(book.in_folder(0)), [3]);
        assert_eq!(book.set_folder(1, 12345), Err(Refused::NoFolder));
        book.rename_folder(work, "Projects").unwrap();
        assert_eq!(book.folders[0].name, "Projects");
        book.delete_folder(work).unwrap();
        assert_eq!(ids(book.in_folder(0)), [1, 2, 3], "the notes are kept, unfiled");
        assert_eq!(book.delete_folder(work), Err(Refused::NoFolder));
    }

    #[test]
    fn the_meta_file_round_trips_and_the_notes_file_is_untouched() {
        let mut book = book();
        let work = book.add_folder("Work\tstuff", 50).unwrap();
        book.set_folder(2, work).unwrap();
        book.sort = Sort::Date;
        let text = book.meta_text();
        assert!(text.starts_with("bowser-notes-meta-v1\nsort\tdate\n"));
        let again = Book::from_parts(book.notes.clone(), &text);
        assert_eq!(again.sort, Sort::Date);
        assert_eq!(again.folders, vec![Folder { id: work, name: "Work stuff".into() }], "a tab in a name is one space");
        assert_eq!(again.meta_of(2), Meta { created: 100, folder: work, addr: "https://a.example/".into() });
        // The notes file itself stays the version-1 format, readable by an older Bowser.
        let notes = crate::productivity::serialize_notes(&book.notes);
        assert!(notes.starts_with("bowser-notes-v1\n"));
        assert_eq!(crate::productivity::parse_notes(&notes), book.notes);
    }

    #[test]
    fn a_damaged_meta_file_is_ignored_and_stale_entries_dropped() {
        let notes = vec![note(1, "a", "b")];
        assert_eq!(Book::from_parts(notes.clone(), "garbage").sort, Sort::Manual);
        assert_eq!(Book::from_parts(notes.clone(), &"x".repeat(MAX_META_BYTES + 1)).meta.len(), 0);
        let text = "bowser-notes-meta-v1\nsort\tbogus\nfolder\t5\tF\nnote\t1\t10\t5\thttps://x/\nnote\t99\t1\t0\t\nnote\t1\tbad\n";
        let book = Book::from_parts(notes.clone(), text);
        assert_eq!(book.sort, Sort::Manual);
        assert_eq!(book.meta.len(), 1, "a note that is gone has no entry");
        let orphan = Book::from_parts(notes, "bowser-notes-meta-v1\nnote\t1\t10\t5\t\n");
        assert_eq!(orphan.meta_of(1).folder, 0, "a note never points at a missing folder");
    }

    #[test]
    fn markdown_export_and_import_round_trip_folders_notes_and_addresses() {
        let mut book = book();
        let work = book.add_folder("Work", 50).unwrap();
        book.set_folder(2, work).unwrap();
        let md = book.export(Scope::All, FileKind::Markdown);
        assert!(md.contains("## banana\n\n<https://b.example/page>\n\nyellow fruit\n"), "{md}");
        assert!(md.contains("# Work\n\n## Apple\n\n<https://a.example/>\n\nred fruit\n"), "{md}");
        let items = parse_import(&md, FileKind::Markdown, "export");
        assert_eq!(items.len(), 3);
        let apple = items.iter().find(|i| i.title == "Apple").unwrap();
        assert_eq!((apple.folder.as_deref(), apple.body.as_str(), apple.addr.as_str()), (Some("Work"), "red fruit", "https://a.example/"));
        let mut fresh = Book::default();
        assert_eq!(fresh.import(items, 7, 1000), (3, 0));
        assert_eq!(fresh.folders.len(), 1);
        assert_eq!(fresh.notes.len(), 3);
        assert_eq!(fresh.export(Scope::All, FileKind::Markdown).matches("## ").count(), 3);
    }

    #[test]
    fn export_scopes_and_text_files() {
        let mut book = book();
        let work = book.add_folder("Work", 50).unwrap();
        book.set_folder(3, work).unwrap();
        let one = book.export(Scope::One(2), FileKind::Markdown);
        assert_eq!(one, "# Apple\n\n<https://a.example/>\n\nred fruit\n\n");
        let folder = book.export(Scope::Folder(work), FileKind::Markdown);
        assert!(folder.contains("cherry") && !folder.contains("banana"));
        let txt = book.export(Scope::All, FileKind::Text);
        assert!(txt.contains("banana\n======\n\nyellow fruit\n") && txt.contains("\n---\n\n"), "{txt}");
        let items = parse_import(&txt, FileKind::Text, "x");
        assert_eq!(items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["banana", "Apple", "cherry"]);
        assert_eq!(items[1].body, "red fruit");
    }

    #[test]
    fn a_plain_file_imports_as_one_note_titled_by_its_first_line() {
        let items = parse_import("\u{feff}Shopping list\r\n- milk\r\n- eggs\r\n", FileKind::Text, "list");
        assert_eq!(items.len(), 1);
        assert_eq!((items[0].title.as_str(), items[0].body.as_str()), ("Shopping list", "Shopping list\n- milk\n- eggs"));
        let md = parse_import("Just words, no headings.\n", FileKind::Markdown, "words");
        assert_eq!(md[0].title, "Just words, no headings.");
        assert!(parse_import("   \n", FileKind::Text, "x").is_empty());
        let headed = parse_import("intro line\n\n# One\ntext one\n# Two\ntext two\n", FileKind::Markdown, "f");
        assert_eq!(headed.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["intro line", "One", "Two"]);
        assert_eq!(headed[2].body, "text two");
    }

    #[test]
    fn an_import_stops_when_the_book_is_full_and_says_how_many_it_left_out() {
        let mut book = Book::default();
        let items: Vec<Imported> = (0..MAX_NOTES + 5).map(|n| Imported { folder: None, title: format!("n{n}"), body: String::new(), addr: String::new() }).collect();
        assert_eq!(book.import(items, 1, 100), (MAX_NOTES, 5));
        assert_eq!(book.notes.len(), MAX_NOTES);
    }

    #[test]
    fn file_kinds_come_from_the_extension() {
        use std::path::Path;
        assert_eq!(FileKind::of_path(Path::new("a.MD")), Some(FileKind::Markdown));
        assert_eq!(FileKind::of_path(Path::new("a.txt")), Some(FileKind::Text));
        assert_eq!(FileKind::of_path(Path::new("a.exe")), None);
        assert_eq!(FileKind::of_path(Path::new("a")), None);
    }
}
