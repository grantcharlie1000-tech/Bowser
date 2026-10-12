//! Local productivity data and command-palette ranking.
//!
//! Files live beside the other profile data: `notes.txt` is plain text, `tasks.txt` is a
//! versioned tab-separated list, `sessions.txt` is a versioned list of named URL snapshots,
//! and `bookmarks.txt` / `history.txt` are tab-separated URL/title records. History is capped
//! and only records successful credential-free HTTP(S) navigations. Feature 9 migration should
//! import bookmarks/history through [`add_bookmark`] and [`add_history_entry`] so both features
//! share the same bounded on-disk format. No account or network state is involved.

use std::{fs, io::Write, path::Path};

pub const MAX_NOTES_BYTES: usize = 64 * 1024;
pub const MAX_TASKS: usize = 500;
pub const MAX_SESSIONS: usize = 32;
pub const MAX_HISTORY: usize = 500;
pub const MAX_BOOKMARKS: usize = 500;
const MAX_FILE_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub id: u64,
    pub complete: bool,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub id: u64,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedSession {
    pub name: String,
    pub urls: Vec<String>,
    pub active: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchItem {
    pub label: String,
    pub detail: String,
    pub action: String,
}

/// A small command registry. Features can add commands without changing the ranking algorithm.
#[derive(Clone, Debug, Default)]
pub struct CommandRegistry {
    commands: Vec<SearchItem>,
}

impl CommandRegistry {
    pub fn register(
        &mut self,
        label: impl Into<String>,
        detail: impl Into<String>,
        action: impl Into<String>,
    ) {
        self.commands.push(SearchItem {
            label: label.into(),
            detail: detail.into(),
            action: action.into(),
        });
    }

    pub fn commands(&self) -> &[SearchItem] {
        &self.commands
    }

    pub fn search<'a>(&'a self, query: &str, limit: usize) -> Vec<(&'a SearchItem, i32)> {
        rank_items(&self.commands, query, limit)
    }
}

pub fn browser_commands() -> CommandRegistry {
    let mut commands = CommandRegistry::default();
    for (label, detail, action) in [
        ("New tab", "Open a blank tab", "new-tab"),
        ("Close tab", "Close the current tab", "close-tab"),
        ("Settings", "Open browser settings", "settings"),
        (
            "Toggle notes and tasks",
            "Show or hide the productivity sidebar",
            "toggle-sidebar",
        ),
        (
            "Passwords and passkeys",
            "Manage saved credentials",
            "passwords",
        ),
        ("Profiles", "Switch or manage profiles", "profiles"),
        (
            "Save session",
            "Save the open tabs as a named session",
            "save-session",
        ),
        (
            "Saved sessions",
            "Restore or delete a saved session",
            "sessions",
        ),
        ("Add bookmark", "Save the current page", "add-bookmark"),
        ("Reload", "Reload the current page", "reload"),
        ("Back", "Go back", "back"),
        ("Forward", "Go forward", "forward"),
        ("Extensions", "Manage installed extensions", "extensions"),
    ] {
        commands.register(label, detail, action);
    }
    commands
}

pub fn profile_allows_productivity(profile: &crate::profile::Profile) -> bool {
    !profile.is_guest()
}

/// Scores exact/prefix matches above ordered fuzzy subsequences, then sorts stably by label.
pub fn rank_items<'a>(
    items: &'a [SearchItem],
    query: &str,
    limit: usize,
) -> Vec<(&'a SearchItem, i32)> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return items.iter().take(limit).map(|item| (item, 0)).collect();
    }
    let mut matches: Vec<_> = items
        .iter()
        .filter_map(|item| {
            let label = item.label.to_lowercase();
            let detail = item.detail.to_lowercase();
            let score = if label == needle {
                3000
            } else if label.starts_with(&needle) {
                2000 - label.len().min(500) as i32
            } else if label
                .split_whitespace()
                .any(|word| word.starts_with(&needle))
            {
                1500 - label.len().min(500) as i32
            } else if detail.starts_with(&needle) {
                1000 - detail.len().min(500) as i32
            } else if let Some(gap) = subsequence_gap(&needle, &label) {
                500 - gap.min(400) as i32
            } else if let Some(gap) = subsequence_gap(&needle, &detail) {
                250 - gap.min(200) as i32
            } else {
                return None;
            };
            Some((item, score))
        })
        .collect();
    matches.sort_by(|(a, sa), (b, sb)| {
        sb.cmp(sa)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
    matches.truncate(limit);
    matches
}

fn subsequence_gap(needle: &str, haystack: &str) -> Option<usize> {
    let mut chars = haystack.char_indices();
    let mut first = None;
    let mut last = 0usize;
    for wanted in needle.chars() {
        let (at, _) = chars.find(|(_, found)| found.eq_ignore_ascii_case(&wanted))?;
        first.get_or_insert(at);
        last = at;
    }
    Some(last.saturating_sub(first?))
}

pub fn parse_tasks(text: &str) -> Vec<Task> {
    if text.len() > MAX_FILE_BYTES {
        return Vec::new();
    }
    let mut lines = text.lines();
    if lines.next() != Some("bowser-tasks-v1") {
        return Vec::new();
    }
    lines
        .take(MAX_TASKS)
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let id = fields.next()?.parse().ok()?;
            let complete = match fields.next()? {
                "0" => false,
                "1" => true,
                _ => return None,
            };
            let text = unescape(fields.next()?);
            (!text.is_empty()).then_some(Task { id, complete, text })
        })
        .collect()
}

pub fn parse_notes(text: &str) -> Vec<Note> {
    if text.len() > MAX_NOTES_BYTES {
        return Vec::new();
    }
    let mut lines = text.lines();
    if lines.next() != Some("bowser-notes-v1") {
        return Vec::new();
    }
    lines
        .take(100)
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let id = fields.next()?.parse().ok()?;
            let title = unescape(fields.next()?);
            let body = unescape(fields.next()?);
            (!title.is_empty()).then_some(Note { id, title, body })
        })
        .collect()
}

pub fn serialize_notes(notes: &[Note]) -> String {
    let mut out = String::from("bowser-notes-v1\n");
    for note in notes.iter().take(100) {
        out.push_str(&format!(
            "{}\t{}\t{}\n",
            note.id,
            escape(&note.title),
            escape(&note.body)
        ));
    }
    out
}

pub fn serialize_tasks(tasks: &[Task]) -> String {
    let mut out = String::from("bowser-tasks-v1\n");
    for task in tasks.iter().take(MAX_TASKS) {
        out.push_str(&format!(
            "{}\t{}\t{}\n",
            task.id,
            u8::from(task.complete),
            escape(&task.text)
        ));
    }
    out
}

pub fn parse_sessions(text: &str) -> Vec<SavedSession> {
    if text.len() > MAX_FILE_BYTES {
        return Vec::new();
    }
    let mut lines = text.lines();
    if lines.next() != Some("bowser-sessions-v1") {
        return Vec::new();
    }
    let mut out: Vec<SavedSession> = Vec::new();
    for line in lines {
        let mut fields = line.splitn(4, '\t');
        let (Some("session"), Some(name), Some(active), Some(urls)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let name = unescape(name);
        if name.is_empty() || name.len() > 128 {
            continue;
        }
        let urls: Vec<String> = urls
            .split('\t')
            .filter_map(|url| valid_http_url(&unescape(url)))
            .take(100)
            .collect();
        if urls.is_empty() {
            continue;
        }
        let active = active.parse::<usize>().unwrap_or(0).min(urls.len() - 1);
        out.push(SavedSession { name, urls, active });
        if out.len() == MAX_SESSIONS {
            break;
        }
    }
    out
}

pub fn serialize_sessions(sessions: &[SavedSession]) -> String {
    let mut out = String::from("bowser-sessions-v1\n");
    for session in sessions.iter().take(MAX_SESSIONS) {
        let urls: Vec<_> = session
            .urls
            .iter()
            .filter_map(|url| valid_http_url(url))
            .take(100)
            .collect();
        if urls.is_empty() {
            continue;
        }
        out.push_str("session\t");
        out.push_str(&escape(&session.name));
        out.push('\t');
        out.push_str(&session.active.min(urls.len() - 1).to_string());
        out.push('\t');
        out.push_str(
            &urls
                .iter()
                .map(|url| escape(url))
                .collect::<Vec<_>>()
                .join("\t"),
        );
        out.push('\n');
    }
    out
}

pub fn read_notes(path: &Path) -> String {
    fs::read(path)
        .ok()
        .filter(|bytes| bytes.len() <= MAX_NOTES_BYTES)
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default()
}

pub fn read_note_list(path: &Path) -> Vec<Note> {
    read_bounded(path).map_or_else(Vec::new, |text| parse_notes(&text))
}
pub fn write_note_list(path: &Path, notes: &[Note]) -> std::io::Result<()> {
    let text = serialize_notes(notes);
    if text.len() > MAX_NOTES_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "notes exceed size limit",
        ));
    }
    atomic_write(path, text.as_bytes())
}
pub fn add_note(path: &Path, note: Note) -> std::io::Result<()> {
    let mut notes = read_note_list(path);
    if notes.len() >= 100 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "too many notes",
        ));
    }
    notes.push(note);
    write_note_list(path, &notes)
}
pub fn edit_note(path: &Path, id: u64, title: String, body: String) -> std::io::Result<bool> {
    let mut notes = read_note_list(path);
    let Some(note) = notes.iter_mut().find(|note| note.id == id) else {
        return Ok(false);
    };
    note.title = title;
    note.body = body;
    write_note_list(path, &notes)?;
    Ok(true)
}
pub fn delete_note(path: &Path, id: u64) -> std::io::Result<bool> {
    let mut notes = read_note_list(path);
    let len = notes.len();
    notes.retain(|note| note.id != id);
    if len == notes.len() {
        return Ok(false);
    }
    write_note_list(path, &notes)?;
    Ok(true)
}

pub fn add_task(path: &Path, task: Task) -> std::io::Result<()> {
    let mut tasks = read_tasks(path);
    if tasks.len() >= MAX_TASKS {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "too many tasks",
        ));
    }
    tasks.push(task);
    write_tasks(path, &tasks)
}
pub fn edit_task(path: &Path, id: u64, text: String) -> std::io::Result<bool> {
    let mut tasks = read_tasks(path);
    let Some(task) = tasks.iter_mut().find(|task| task.id == id) else {
        return Ok(false);
    };
    task.text = text;
    write_tasks(path, &tasks)?;
    Ok(true)
}
pub fn complete_task(path: &Path, id: u64, complete: bool) -> std::io::Result<bool> {
    let mut tasks = read_tasks(path);
    let Some(task) = tasks.iter_mut().find(|task| task.id == id) else {
        return Ok(false);
    };
    task.complete = complete;
    write_tasks(path, &tasks)?;
    Ok(true)
}
pub fn delete_task(path: &Path, id: u64) -> std::io::Result<bool> {
    let mut tasks = read_tasks(path);
    let len = tasks.len();
    tasks.retain(|task| task.id != id);
    if len == tasks.len() {
        return Ok(false);
    }
    write_tasks(path, &tasks)?;
    Ok(true)
}
pub fn save_session(path: &Path, session: SavedSession) -> std::io::Result<()> {
    let mut sessions = read_sessions(path);
    sessions.retain(|existing| existing.name != session.name);
    if sessions.len() >= MAX_SESSIONS {
        sessions.remove(0);
    }
    sessions.push(session);
    write_sessions(path, &sessions)
}
pub fn delete_session(path: &Path, name: &str) -> std::io::Result<bool> {
    let mut sessions = read_sessions(path);
    let len = sessions.len();
    sessions.retain(|session| session.name != name);
    if len == sessions.len() {
        return Ok(false);
    }
    write_sessions(path, &sessions)?;
    Ok(true)
}

pub fn write_notes(path: &Path, notes: &str) -> std::io::Result<()> {
    if notes.len() > MAX_NOTES_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "notes exceed size limit",
        ));
    }
    atomic_write(path, notes.as_bytes())
}

pub fn read_tasks(path: &Path) -> Vec<Task> {
    read_bounded(path).map_or_else(Vec::new, |text| parse_tasks(&text))
}
pub fn write_tasks(path: &Path, tasks: &[Task]) -> std::io::Result<()> {
    atomic_write(path, serialize_tasks(tasks).as_bytes())
}
pub fn read_sessions(path: &Path) -> Vec<SavedSession> {
    read_bounded(path).map_or_else(Vec::new, |text| parse_sessions(&text))
}
pub fn write_sessions(path: &Path, sessions: &[SavedSession]) -> std::io::Result<()> {
    atomic_write(path, serialize_sessions(sessions).as_bytes())
}

pub fn add_history_entry(path: &Path, url: &str, title: &str) -> std::io::Result<()> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    add_history_entry_at(path, timestamp, url, title)
}

pub fn add_history_entry_at(
    path: &Path,
    timestamp: u64,
    url: &str,
    title: &str,
) -> std::io::Result<()> {
    let url = valid_http_url(url).ok_or_else(|| invalid_url_error())?;
    let mut entries = read_history(path);
    entries.retain(|(_, existing, _)| existing != &url);
    entries.push((timestamp, url, title.chars().take(256).collect::<String>()));
    if entries.len() > MAX_HISTORY {
        entries.drain(..entries.len() - MAX_HISTORY);
    }
    let mut out = String::new();
    for (timestamp, url, title) in entries {
        out.push_str(&format!(
            "{timestamp}\t{}\t{}\n",
            escape(&url),
            escape(&title)
        ));
    }
    atomic_write(path, out.as_bytes())
}

pub fn add_bookmark(path: &Path, url: &str, title: &str) -> std::io::Result<()> {
    add_bookmark_in_folder(path, url, title, "")
}

/// Removes a bookmark from the established flat file and the additive v2 tree.
pub fn remove_bookmark(path: &Path, url: &str) -> std::io::Result<bool> {
    let url = valid_http_url(url).ok_or_else(|| invalid_url_error())?;
    let mut entries = read_bookmarks(path);
    let old_len = entries.len();
    entries.retain(|(_, existing, _)| existing != &url);
    if entries.len() != old_len {
        let out = entries
            .iter()
            .map(|(title, url, folder)| {
                format!("{}\t{}\t{}\n", escape(title), escape(url), escape(folder))
            })
            .collect::<String>();
        atomic_write(path, out.as_bytes())?;
    }
    if let Some(profile_dir) = path.parent() {
        let mut tree = crate::bookmarks::BookmarkStore::load(profile_dir)?;
        if let Some(id) = tree
            .items()
            .iter()
            .find(|item| matches!(&item.kind, crate::bookmarks::BookmarkKind::Link { url: existing, .. } if existing == &url))
            .map(|item| item.id)
        {
            tree.delete(id);
            tree.save(profile_dir)?;
        }
    }
    Ok(entries.len() != old_len)
}

pub fn add_bookmark_in_folder(
    path: &Path,
    url: &str,
    title: &str,
    folder: &str,
) -> std::io::Result<()> {
    let url = valid_http_url(url).ok_or_else(|| invalid_url_error())?;
    let mut entries = read_bookmarks(path);
    entries.retain(|(_, existing, _)| existing != &url);
    entries.push((
        title.chars().take(256).collect(),
        url.clone(),
        folder.chars().take(128).collect(),
    ));
    if entries.len() > MAX_BOOKMARKS {
        entries.drain(..entries.len() - MAX_BOOKMARKS);
    }
    let out = entries
        .iter()
        .map(|(title, url, folder)| {
            format!("{}\t{}\t{}\n", escape(title), escape(url), escape(folder))
        })
        .collect::<String>();
    atomic_write(path, out.as_bytes())?;
    // Keep the established importer/command-palette file intact and mirror this mutation into
    // the additive tree format consumed by the sidebar.
    if let Some(profile_dir) = path.parent() {
        let mut tree = crate::bookmarks::BookmarkStore::load(profile_dir)?;
        let parent = folder_path(&mut tree, folder)?;
        if let Some(old) = tree.items().iter().find(|item| matches!(&item.kind, crate::bookmarks::BookmarkKind::Link { url: old, .. } if old == &url)).map(|item| item.id) { tree.delete(old); }
        tree.add_bookmark(parent, &title, &url)?;
        tree.save(profile_dir)?;
    }
    Ok(())
}

fn folder_path(
    tree: &mut crate::bookmarks::BookmarkStore,
    folder: &str,
) -> std::io::Result<Option<u64>> {
    let mut parent = None;
    for part in folder
        .split('/')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .take(16)
    {
        let existing = tree
            .children(parent)
            .into_iter()
            .find(|item| {
                matches!(item.kind, crate::bookmarks::BookmarkKind::Folder)
                    && item.title.eq_ignore_ascii_case(part)
            })
            .map(|item| item.id);
        parent = Some(match existing {
            Some(id) => id,
            None => tree.add_folder(parent, part)?,
        });
    }
    Ok(parent)
}

pub fn read_bookmarks(path: &Path) -> Vec<(String, String, String)> {
    read_bounded(path).map_or_else(Vec::new, |text| {
        text.lines()
            .take(MAX_BOOKMARKS)
            .filter_map(|line| {
                let mut fields = line.splitn(3, '\t');
                let title = unescape(fields.next()?);
                let url = valid_http_url(&unescape(fields.next()?))?;
                let folder = unescape(fields.next().unwrap_or(""));
                Some((title, url, folder))
            })
            .collect()
    })
}

pub fn read_history(path: &Path) -> Vec<(u64, String, String)> {
    read_bounded(path).map_or_else(Vec::new, |text| {
        text.lines()
            .take(MAX_HISTORY)
            .filter_map(|line| {
                let mut fields = line.splitn(3, '\t');
                let timestamp = fields.next()?.parse().ok()?;
                let url = valid_http_url(&unescape(fields.next()?))?;
                let title = unescape(fields.next()?);
                Some((timestamp, url, title))
            })
            .collect()
    })
}

pub type HistoryEntry = (u64, String, String);
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryDay {
    pub label: String,
    pub day: String,
    pub entries: Vec<HistoryEntry>,
}

pub fn delete_history_entry(path: &Path, url: &str) -> std::io::Result<bool> {
    let normalized = valid_http_url(url).ok_or_else(invalid_url_error)?;
    let mut entries = read_history(path);
    let n = entries.len();
    entries.retain(|(_, u, _)| u != &normalized);
    if entries.len() == n {
        return Ok(false);
    }
    write_history(path, &entries)?;
    Ok(true)
}
pub fn delete_history_range(path: &Path, start: u64, end: u64) -> std::io::Result<usize> {
    let mut entries = read_history(path);
    let n = entries.len();
    entries.retain(|(t, _, _)| *t < start || *t > end);
    let count = n - entries.len();
    if count > 0 {
        write_history(path, &entries)?
    }
    Ok(count)
}
pub fn delete_history_for_host(path: &Path, host: &str) -> std::io::Result<usize> {
    let host = host.trim().trim_start_matches("www.").to_ascii_lowercase();
    let mut entries = read_history(path);
    let n = entries.len();
    entries.retain(|(_, url, _)| {
        url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
            .is_none_or(|h| h.trim_start_matches("www.") != host)
    });
    let count = n - entries.len();
    if count > 0 {
        write_history(path, &entries)?
    }
    Ok(count)
}
pub fn clear_history(path: &Path) -> std::io::Result<()> {
    atomic_write(path, b"")
}
pub fn search_history(path: &Path, query: &str, limit: usize) -> Vec<HistoryEntry> {
    let q = query.trim().to_lowercase();
    let mut entries = read_history(path);
    entries.sort_by_key(|e| std::cmp::Reverse(e.0));
    entries
        .into_iter()
        .filter(|(_, u, t)| {
            q.is_empty() || u.to_lowercase().contains(&q) || t.to_lowercase().contains(&q)
        })
        .take(limit)
        .collect()
}
pub fn group_history_by_day(entries: &[HistoryEntry], now: u64) -> Vec<HistoryDay> {
    use std::collections::BTreeMap;
    let today = (now / 86400) as i64;
    let mut groups: BTreeMap<i64, Vec<HistoryEntry>> = BTreeMap::new();
    for e in entries {
        groups
            .entry((e.0 / 86400) as i64)
            .or_default()
            .push(e.clone())
    }
    groups
        .into_iter()
        .rev()
        .map(|(d, mut entries)| {
            entries.sort_by_key(|e| std::cmp::Reverse(e.0));
            let date = civil_from_days(d);
            HistoryDay {
                label: if d == today {
                    "Today".into()
                } else if d == today - 1 {
                    "Yesterday".into()
                } else {
                    date.clone()
                },
                day: date,
                entries,
            }
        })
        .collect()
}
fn write_history(path: &Path, entries: &[HistoryEntry]) -> std::io::Result<()> {
    let text = entries
        .iter()
        .take(MAX_HISTORY)
        .map(|(t, u, title)| format!("{t}\t{}\t{}\n", escape(u), escape(title)))
        .collect::<String>();
    atomic_write(path, text.as_bytes())
}
fn civil_from_days(days: i64) -> String {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    y += if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn read_url_titles(path: &Path, header: &str, limit: usize) -> Vec<(String, String)> {
    if header == "bowser-history-v1" {
        return read_history(path)
            .into_iter()
            .rev()
            .take(limit)
            .map(|(_, url, title)| (url, title))
            .collect();
    }
    read_bounded(path).map_or_else(Vec::new, |text| {
        let mut lines = text.lines();
        if lines.next() != Some(header) {
            return Vec::new();
        }
        lines
            .take(limit)
            .filter_map(|line| {
                let (url, title) = line.split_once('\t')?;
                Some((valid_http_url(&unescape(url))?, unescape(title)))
            })
            .collect()
    })
}

fn invalid_url_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "only credential-free HTTP(S) URLs can be stored",
    )
}

fn read_bounded(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() as usize > MAX_FILE_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temp = path.with_extension("tmp");
    let mut file = fs::File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            MoveFileExW(
                windows::core::PCWSTR(from.as_ptr()),
                windows::core::PCWSTR(to.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )?;
        }
    }
    #[cfg(not(windows))]
    {
        fs::rename(&temp, path)?;
    }
    Ok(())
}

fn valid_http_url(url: &str) -> Option<String> {
    if url.len() > 2048 {
        return None;
    }
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    Some(parsed.to_string())
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_round_trip_and_ignore_malformed_lines() {
        let tasks = vec![Task {
            id: 17,
            complete: true,
            text: "read\tthis\nsoon\\".into(),
        }];
        let mut text = serialize_tasks(&tasks);
        text.push_str("bad\tline\n18\t2\tignored\n");
        assert_eq!(parse_tasks(&text), tasks);
    }

    #[test]
    fn saved_sessions_round_trip_with_bounded_http_urls() {
        let sessions = vec![SavedSession {
            name: "Work\twindow".into(),
            urls: vec!["https://example.com/a".into(), "file:///private".into()],
            active: 8,
        }];
        let parsed = parse_sessions(&serialize_sessions(&sessions));
        assert_eq!(
            parsed,
            vec![SavedSession {
                name: "Work\twindow".into(),
                urls: vec!["https://example.com/a".into()],
                active: 0
            }]
        );
    }

    #[test]
    fn rank_prefers_exact_then_prefix_then_fuzzy() {
        let mut registry = CommandRegistry::default();
        registry.register("Settings", "Open browser settings", "settings");
        registry.register("Save session", "Save tabs as a session", "save");
        registry.register("Open settings", "Settings", "open-settings");
        let exact = registry.search("settings", 5);
        assert_eq!(exact[0].0.action, "settings");
        let fuzzy = registry.search("sve", 5);
        assert_eq!(fuzzy[0].0.action, "save");
    }

    #[test]
    fn browser_registry_exposes_core_productivity_and_browser_actions() {
        let registry = browser_commands();
        let actions: Vec<_> = registry
            .commands()
            .iter()
            .map(|item| item.action.as_str())
            .collect();
        for action in [
            "new-tab",
            "close-tab",
            "settings",
            "passwords",
            "profiles",
            "toggle-sidebar",
            "save-session",
            "sessions",
            "add-bookmark",
            "extensions",
        ] {
            assert!(actions.contains(&action), "missing command {action}");
        }
    }

    #[test]
    fn guest_profiles_are_gated_from_productivity_persistence() {
        let root = std::env::temp_dir().join(format!("bowser-guest-gate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let guest = crate::profile::Profile::guest_in(&root).unwrap();
        let regular = crate::profile::Profile::default_in(&root);
        assert!(!profile_allows_productivity(&guest));
        assert!(profile_allows_productivity(&regular));
        let _ = crate::profile::cleanup_guest(&guest);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn profile_files_round_trip_and_reject_credentials() {
        let root = std::env::temp_dir().join(format!("bowser-productivity-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let history = root.join("history.txt");
        add_history_entry(&history, "https://example.com/", "Example").unwrap();
        assert_eq!(
            read_url_titles(&history, "bowser-history-v1", 10),
            vec![("https://example.com/".into(), "Example".into())]
        );
        assert!(
            add_bookmark(
                &root.join("bookmarks.txt"),
                "https://u:p@example.com",
                "bad"
            )
            .is_err()
        );
        let notes = root.join("notes.txt");
        write_notes(&notes, "hello\n").unwrap();
        assert_eq!(read_notes(&notes), "hello\n");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn history_and_bookmarks_use_importer_formats_and_dedupe_by_url() {
        let root = std::env::temp_dir().join(format!(
            "bowser-productivity-formats-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let history = root.join("history.txt");
        add_history_entry_at(&history, 1_700_000_000, "https://example.com/", "Example").unwrap();
        assert_eq!(
            fs::read_to_string(&history).unwrap(),
            "1700000000\thttps://example.com/\tExample\n"
        );
        let bookmarks = root.join("bookmarks.txt");
        add_bookmark_in_folder(&bookmarks, "https://example.com/", "First", "Reading").unwrap();
        add_bookmark_in_folder(&bookmarks, "https://example.com/", "Latest", "Work").unwrap();
        assert_eq!(
            fs::read_to_string(&bookmarks).unwrap(),
            "Latest\thttps://example.com/\tWork\n"
        );
        assert_eq!(
            read_bookmarks(&bookmarks),
            vec![(
                "Latest".into(),
                "https://example.com/".into(),
                "Work".into()
            )]
        );
        assert!(remove_bookmark(&bookmarks, "https://example.com/").unwrap());
        assert!(read_bookmarks(&bookmarks).is_empty());
        let tree = crate::bookmarks::BookmarkStore::load(&root).unwrap();
        assert!(!tree.is_bookmarked("https://example.com/"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn note_and_task_mutations_persist_by_id() {
        let root = std::env::temp_dir().join(format!(
            "bowser-productivity-mutations-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let notes = root.join("notes.txt");
        add_note(
            &notes,
            Note {
                id: 1,
                title: "Plan".into(),
                body: "First".into(),
            },
        )
        .unwrap();
        assert!(edit_note(&notes, 1, "Plan".into(), "Updated".into()).unwrap());
        assert_eq!(read_note_list(&notes)[0].body, "Updated");
        assert!(delete_note(&notes, 1).unwrap());
        let tasks = root.join("tasks.txt");
        add_task(
            &tasks,
            Task {
                id: 2,
                complete: false,
                text: "Draft".into(),
            },
        )
        .unwrap();
        assert!(complete_task(&tasks, 2, true).unwrap());
        assert!(edit_task(&tasks, 2, "Review".into()).unwrap());
        assert!(delete_task(&tasks, 2).unwrap());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn history_delete_search_and_group_by_day() {
        let root = std::env::temp_dir().join(format!("bowser-history-edit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("history.txt");
        add_history_entry_at(&path, 1_700_000_000, "https://example.com/one", "One").unwrap();
        add_history_entry_at(&path, 1_700_086_400, "https://other.test/", "Two").unwrap();
        assert_eq!(search_history(&path, "TWO", 1)[0].2, "Two");
        assert_eq!(delete_history_for_host(&path, "example.com").unwrap(), 1);
        assert_eq!(
            delete_history_range(&path, 1_700_086_400, 1_700_086_400).unwrap(),
            1
        );
        add_history_entry_at(&path, 1_700_000_000, "https://example.com/", "One").unwrap();
        let grouped = group_history_by_day(&read_history(&path), 1_700_086_400);
        assert_eq!(grouped[0].label, "Yesterday");
        assert!(delete_history_entry(&path, "https://example.com/").unwrap());
        clear_history(&path).unwrap();
        assert!(read_history(&path).is_empty());
        let _ = fs::remove_dir_all(root);
    }
}
