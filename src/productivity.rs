//! Local productivity data and command-palette ranking.
//!
//! Files live beside the other profile data: `notes.txt` is plain text, `tasks.txt` is a
//! versioned tab-separated list, `sessions.txt` is a versioned list of named URL snapshots,
//! and `bookmarks.txt` / `history.txt` are tab-separated URL/title records. History is capped
//! and only records successful credential-free HTTP(S) navigations. Feature 9 migration should
//! import bookmarks/history through [`add_bookmark`] and [`add_history_entry`] so both features
//! share the same bounded on-disk format. No account or network state is involved.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub const MAX_NOTES_BYTES: usize = 64 * 1024;
pub const MAX_TASKS: usize = 500;
pub const MAX_SESSIONS: usize = 32;
/// Most visits kept. History retention (Settings) drops older ones first; this cap only matters for
/// a very busy profile that keeps history "forever".
pub const MAX_HISTORY: usize = 5_000;
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

/// The commands the palette lists, from the one command registry (`crate::commands`): built-ins and
/// the user's chains. The action of each is the command id.
pub fn browser_commands() -> CommandRegistry {
    let mut commands = CommandRegistry::default();
    for command in crate::commands::palette_commands(&crate::commands::live()) {
        commands.register(command.label, command.detail, command.id);
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
    // The caller only queues the visit: the read-merge-rewrite of the file runs on the history
    // writer thread, and every history read or edit flushes the queue first. A file that exists
    // but cannot be read is still refused here, cheaply, and again by the writer.
    match fs::metadata(path) {
        Ok(meta) if !meta.is_file() || meta.len() > MAX_HISTORY_FILE_BYTES => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "history file cannot be read",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    queue_history_visit(path, (timestamp, url, title.chars().take(256).collect::<String>()));
    Ok(())
}

/// How long the writer waits after a visit so a burst becomes one rewrite.
const HISTORY_FLUSH_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

/// Visits not yet in their file, per history file, oldest first.
static PENDING_HISTORY: Mutex<Vec<(PathBuf, Vec<HistoryEntry>)>> = Mutex::new(Vec::new());
/// Held across every read-modify-write of a history file so the writer thread and the UI thread
/// never interleave.
static HISTORY_IO: Mutex<()> = Mutex::new(());

/// The parsed history this process last read or wrote, per file, so a write does not reread and
/// reparse the file. Only trusted while the file still has the size and modification time recorded
/// with it, so an edit from outside is picked up. Take it inside [`HISTORY_IO`].
static HISTORY_CACHE: Mutex<Vec<HistoryCache>> = Mutex::new(Vec::new());
/// Files with a cached parse; a process has one history, tests have a few.
const HISTORY_CACHE_FILES: usize = 16;

/// Size and modification time of a file, `None` when it does not exist.
type FileStamp = Option<(u64, std::time::SystemTime)>;

struct HistoryCache {
    path: PathBuf,
    stamp: FileStamp,
    entries: Vec<HistoryEntry>,
}

/// `None` when the file's state cannot be told apart cheaply (never cached).
fn history_stamp(path: &Path) -> Option<FileStamp> {
    match fs::metadata(path) {
        Ok(meta) => meta.modified().ok().map(|modified| Some((meta.len(), modified))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(None),
        Err(_) => None,
    }
}

fn remember_history(path: &Path, entries: &[HistoryEntry]) {
    let mut cache = lock_history(&HISTORY_CACHE);
    cache.retain(|cached| cached.path != path);
    let Some(stamp) = history_stamp(path) else {
        return;
    };
    if cache.len() >= HISTORY_CACHE_FILES {
        cache.remove(0);
    }
    cache.push(HistoryCache {
        path: path.to_path_buf(),
        stamp,
        entries: entries.to_vec(),
    });
}

fn forget_history(path: &Path) {
    lock_history(&HISTORY_CACHE).retain(|cached| cached.path != path);
}

/// The parsed history of `path` and whether it came from memory. Hold [`HISTORY_IO`].
fn load_history(path: &Path) -> std::io::Result<(Vec<HistoryEntry>, bool)> {
    if let Some(stamp) = history_stamp(path) {
        let cache = lock_history(&HISTORY_CACHE);
        if let Some(cached) = cache.iter().find(|cached| cached.path == path && cached.stamp == stamp) {
            return Ok((cached.entries.clone(), true));
        }
    }
    let entries = read_history_file(path)?;
    remember_history(path, &entries);
    Ok((entries, false))
}

fn lock_history<T>(mutex: &'static Mutex<T>) -> std::sync::MutexGuard<'static, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn queue_history_visit(path: &Path, visit: HistoryEntry) {
    {
        let mut pending = lock_history(&PENDING_HISTORY);
        let queue = match pending.iter().position(|(queued, _)| queued == path) {
            Some(index) => &mut pending[index].1,
            None => {
                pending.push((path.to_path_buf(), Vec::new()));
                &mut pending.last_mut().expect("just pushed").1
            }
        };
        // A revisit moves to the end, same as the file merge. Removing it first means the cap
        // does not drop a unique visit to make room for a second copy of this one.
        if let Some(index) = queue.iter().position(|(_, url, _)| url == &visit.1) {
            queue.remove(index);
        }
        queue.push(visit);
        if queue.len() > MAX_HISTORY {
            queue.drain(..queue.len() - MAX_HISTORY);
        }
    }
    wake_history_writer();
}

fn wake_history_writer() {
    static WRITER: std::sync::OnceLock<Option<Mutex<std::sync::mpsc::Sender<()>>>> =
        std::sync::OnceLock::new();
    let writer = WRITER.get_or_init(|| {
        let (wake, woken) = std::sync::mpsc::channel::<()>();
        std::thread::Builder::new()
            .name("history-writer".into())
            .spawn(move || {
                while woken.recv().is_ok() {
                    std::thread::sleep(HISTORY_FLUSH_DELAY);
                    while woken.try_recv().is_ok() {}
                    flush_pending_history();
                }
            })
            .ok()
            .map(|_| Mutex::new(wake))
    });
    match writer {
        Some(wake) => {
            let _ = lock_history(wake).send(());
        }
        // No writer thread: fall back to the synchronous write.
        None => flush_pending_history(),
    }
}

/// Writes every queued visit now. The writer thread calls this after a short delay; the window can
/// call it on exit so a visit made just before closing is not dropped.
pub fn flush_pending_history() {
    let _io = lock_history(&HISTORY_IO);
    let paths: Vec<PathBuf> = lock_history(&PENDING_HISTORY).iter().map(|(path, _)| path.clone()).collect();
    for path in paths {
        let _ = flush_history_locked(&path);
    }
}

/// Writes the visits queued for `path`. On failure they stay queued for the next flush.
fn flush_history_locked(path: &Path) -> std::io::Result<()> {
    let visits = {
        let mut pending = lock_history(&PENDING_HISTORY);
        match pending.iter().position(|(queued, _)| queued == path) {
            Some(index) => pending.swap_remove(index).1,
            None => return Ok(()),
        }
    };
    if visits.is_empty() {
        return Ok(());
    }
    // A file that exists but cannot be read must never be replaced by the queued entries.
    let result = load_history(path).and_then(|(mut entries, _)| {
        for (timestamp, url, title) in &visits {
            entries.retain(|(_, existing, _)| existing != url);
            entries.push((*timestamp, url.clone(), title.clone()));
        }
        if entries.len() > MAX_HISTORY {
            entries.drain(..entries.len() - MAX_HISTORY);
        }
        write_history(path, &entries)
    });
    if result.is_err() {
        let mut pending = lock_history(&PENDING_HISTORY);
        let mut visits = visits;
        match pending.iter().position(|(queued, _)| queued == path) {
            Some(index) => {
                visits.append(&mut pending[index].1);
                pending[index].1 = visits;
            }
            None => pending.push((path.to_path_buf(), visits)),
        }
        if let Some((_, queue)) = pending.iter_mut().find(|(queued, _)| queued == path) {
            if queue.len() > MAX_HISTORY {
                queue.drain(..queue.len() - MAX_HISTORY);
            }
        }
    }
    result
}

/// Entries for a read-modify-write: queued visits are flushed first. Hold [`HISTORY_IO`].
fn read_history_for_update(path: &Path) -> std::io::Result<Vec<HistoryEntry>> {
    let _ = flush_history_locked(path);
    load_history(path).map(|(entries, _)| entries)
}

/// What a tab's page load should add to history, so a visit is recorded whichever way the page
/// was reached (typed, link, redirect, same-document change, reload) and not only when the address
/// was typed into the omnibox. Pure: the window feeds it each tab's committed address, title and
/// loading flag and persists whatever it returns.
#[derive(Debug, Default)]
pub struct VisitTracker {
    /// Per tab: the address last recorded and whether its title was only the address.
    last: std::collections::HashMap<u32, (String, bool)>,
}

impl VisitTracker {
    /// `just_finished`: the tab was loading at the previous observation and is not now (a load,
    /// including a reload of the same address, completed). Returns `(url, title)` to persist.
    pub fn observe(
        &mut self,
        tab: u32,
        url: Option<&str>,
        title: &str,
        loading: bool,
        just_finished: bool,
    ) -> Option<(String, String)> {
        if loading {
            return None;
        }
        // Only credential-free http(s) pages are history: no about:, data:, helium:, bowser: or file:.
        let Some(url) = url.and_then(valid_http_url) else {
            // The tab is on a page that is not history: coming back to the old address counts anew.
            self.last.remove(&tab);
            return None;
        };
        let title = title.trim();
        let titled = !title.is_empty();
        let record = match self.last.get(&tab) {
            None => true,
            Some((last, _)) if *last != url => true,
            Some((_, was_placeholder)) => just_finished || (*was_placeholder && titled),
        };
        if !record {
            return None;
        }
        if self.last.len() >= 1024 {
            // Closed tabs are never reported; a bounded reset only costs one repeated visit.
            self.last.clear();
        }
        self.last.insert(tab, (url.clone(), !titled));
        let title = if titled { title.to_owned() } else { url.clone() };
        Some((url, title))
    }

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

/// Largest history file that is read at all. The file is capped at `MAX_HISTORY` lines of at most
/// a couple of kilobytes each, so this is only a guard against a foreign file.
const MAX_HISTORY_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// History for display. An unreadable file shows as empty; mutators use [`read_history_strict`] so
/// that never turns into a rewrite.
pub fn read_history(path: &Path) -> Vec<(u64, String, String)> {
    read_history_strict(path).unwrap_or_default()
}

/// Reads history, telling a missing file (empty) apart from one that exists but cannot be read.
/// Malformed lines are skipped; the newest `MAX_HISTORY` entries are kept (the file is oldest
/// first). Tolerates CRLF line ends, a UTF-8 byte order mark and invalid UTF-8.
pub(crate) fn read_history_strict(path: &Path) -> std::io::Result<Vec<(u64, String, String)>> {
    // Queued visits reach the file first, so a reader never sees a stale history.
    let _io = lock_history(&HISTORY_IO);
    read_history_for_update(path)
}

fn read_history_file(path: &Path) -> std::io::Result<Vec<(u64, String, String)>> {
    let bytes = match fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut std::io::Read::take(file, MAX_HISTORY_FILE_BYTES + 1), &mut bytes)?;
            if bytes.len() as u64 > MAX_HISTORY_FILE_BYTES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "history file is too large to read",
                ));
            }
            bytes
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    Ok(parse_history(&String::from_utf8_lossy(&bytes)))
}

fn parse_history(text: &str) -> Vec<(u64, String, String)> {
    let mut entries: Vec<_> = text
        .trim_start_matches('\u{feff}')
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let timestamp = fields.next()?.trim().parse().ok()?;
            let url = valid_http_url(&unescape(fields.next()?))?;
            let title = unescape(fields.next().unwrap_or(""));
            Some((timestamp, url, title))
        })
        .collect();
    if entries.len() > MAX_HISTORY {
        entries.drain(..entries.len() - MAX_HISTORY);
    }
    entries
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
    let _io = lock_history(&HISTORY_IO);
    let mut entries = read_history_for_update(path)?;
    let n = entries.len();
    entries.retain(|(_, u, _)| u != &normalized);
    if entries.len() == n {
        return Ok(false);
    }
    write_history(path, &entries)?;
    Ok(true)
}
pub fn delete_history_range(path: &Path, start: u64, end: u64) -> std::io::Result<usize> {
    let _io = lock_history(&HISTORY_IO);
    let mut entries = read_history_for_update(path)?;
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
    let _io = lock_history(&HISTORY_IO);
    let mut entries = read_history_for_update(path)?;
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
    let _io = lock_history(&HISTORY_IO);
    // Visits still queued were made before the clear.
    lock_history(&PENDING_HISTORY).retain(|(queued, _)| queued != path);
    forget_history(path);
    atomic_write(path, b"")
}
pub fn search_history(path: &Path, query: &str, limit: usize) -> Vec<HistoryEntry> {
    search_entries(read_history(path), query, limit)
}

/// [`search_history`] over entries already read (the history page reads strictly, so it can tell
/// an unreadable file from an empty one): newest first, `query` matched case-insensitively
/// against the address and the title, at most `limit`.
pub fn search_entries(mut entries: Vec<HistoryEntry>, query: &str, limit: usize) -> Vec<HistoryEntry> {
    let q = query.trim().to_lowercase();
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
/// Replaces the file with `entries` (at most the newest `MAX_HISTORY` are kept in order) and flushes
/// once. The rename is not written through: a crash may lose this write but leaves the previous
/// file whole, so earlier entries survive.
fn write_history(path: &Path, entries: &[HistoryEntry]) -> std::io::Result<()> {
    let entries = &entries[..entries.len().min(MAX_HISTORY)];
    let text = entries
        .iter()
        .map(|(t, u, title)| format!("{t}\t{}\t{}\n", escape(u), escape(title)))
        .collect::<String>();
    // The file may be half-replaced if this fails, so the parse in memory is dropped.
    forget_history(path);
    atomic_write_with(path, text.as_bytes(), false)?;
    remember_history(path, entries);
    Ok(())
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

/// True when `path` already holds exactly `bytes`, so a rewrite would change nothing. The length
/// check keeps a differing file from being read at all.
pub(crate) fn file_holds(path: &Path, bytes: &[u8]) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.len() == bytes.len() as u64)
        && fs::read(path).is_ok_and(|current| current == bytes)
}

pub(crate) fn read_bounded(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() as usize > MAX_FILE_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    atomic_write_with(path, bytes, true)
}

/// [`atomic_write`]; `write_through` also flushes the rename itself, a second flush for files where
/// losing the latest write to a crash is acceptable but tearing is not.
fn atomic_write_with(path: &Path, bytes: &[u8], write_through: bool) -> std::io::Result<()> {
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
        let flags = if write_through {
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH
        } else {
            MOVEFILE_REPLACE_EXISTING
        };
        unsafe {
            MoveFileExW(
                windows::core::PCWSTR(from.as_ptr()),
                windows::core::PCWSTR(to.as_ptr()),
                flags,
            )?;
        }
    }
    #[cfg(not(windows))]
    {
        let _ = write_through;
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

pub(crate) fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
pub(crate) fn unescape(value: &str) -> String {
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
            "toggle-notes",
            "toggle-sidebar",
            "save-session",
            "sessions",
            "save-all-session",
            "open-session",
            "reopen-tab",
            "closed-tabs",
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
        flush_pending_history();
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

    fn history_temp(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("bowser-history-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn visit_tracker_records_a_finished_load_however_the_page_was_reached() {
        let mut visits = VisitTracker::default();
        // Still loading: nothing yet, even though the address is already known.
        assert_eq!(visits.observe(1, Some("https://example.com/"), "", true, false), None);
        // A link click, with no omnibox involved, is recorded when the load finishes.
        assert_eq!(
            visits.observe(1, Some("https://example.com/"), "Example", false, true),
            Some(("https://example.com/".into(), "Example".into()))
        );
        // The same state seen again records nothing.
        assert_eq!(visits.observe(1, Some("https://example.com/"), "Example", false, false), None);
        // A redirect or same-document change lands on a new address.
        assert_eq!(
            visits.observe(1, Some("https://example.com/next#a"), "Next", false, true),
            Some(("https://example.com/next#a".into(), "Next".into()))
        );
        // A reload of the same address is a visit again.
        assert!(visits.observe(1, Some("https://example.com/next#a"), "Next", false, true).is_some());
        // Another tab is independent.
        assert!(visits.observe(2, Some("https://example.com/next#a"), "Next", false, false).is_some());
    }

    #[test]
    fn visit_tracker_skips_pages_that_are_not_history() {
        let mut visits = VisitTracker::default();
        for url in [
            "about:blank",
            "data:text/html,hi",
            "helium://settings",
            "bowser://newtab",
            "bowser://settings",
            "bowser://settings/privacy",
            "file:///C:/a.txt",
            "https://user:pw@example.com/",
        ] {
            assert_eq!(visits.observe(1, Some(url), "T", false, true), None, "{url}");
        }
        assert_eq!(visits.observe(1, None, "T", false, true), None);
        // Leaving a page for an internal one forgets it, so returning to it counts as a new visit.
        assert!(visits.observe(3, Some("https://a.test/"), "A", false, true).is_some());
        assert_eq!(visits.observe(3, Some("helium://settings"), "S", false, true), None);
        assert!(visits.observe(3, Some("https://a.test/"), "A", false, false).is_some());
    }

    #[test]
    fn visit_tracker_uses_the_address_for_an_empty_title_then_upgrades_it() {
        let mut visits = VisitTracker::default();
        assert_eq!(
            visits.observe(1, Some("https://example.com/p"), "  ", false, true),
            Some(("https://example.com/p".into(), "https://example.com/p".into()))
        );
        // The real title arrives after the load finished: recorded once more, then left alone.
        assert_eq!(
            visits.observe(1, Some("https://example.com/p"), "Page", false, false),
            Some(("https://example.com/p".into(), "Page".into()))
        );
        assert_eq!(visits.observe(1, Some("https://example.com/p"), "Page (2)", false, false), None);
    }

    #[test]
    fn recorded_visits_survive_a_round_trip_through_the_file() {
        let root = history_temp("roundtrip");
        let path = root.join("history.txt");
        let title = "Tab\there\nand a \\ slash\r\nand more";
        add_history_entry_at(&path, 1_700_000_000, "https://example.com/a?b=c#d", title).unwrap();
        add_history_entry_at(&path, 1_700_000_100, "https://other.test/", "Other").unwrap();
        let entries = read_history(&path);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].2, title);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2, "escaped newlines must not add lines");
        assert!(!root.join("history.tmp").exists(), "the temp file is renamed away");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn history_writes_reuse_the_parse_and_survive_outside_edits() {
        let root = history_temp("cache");
        let path = root.join("history.txt");
        let _io = lock_history(&HISTORY_IO);
        fs::write(&path, "1\thttps://a.test/\tA\n").unwrap();
        assert!(!load_history(&path).unwrap().1, "first read parses the file");
        assert!(load_history(&path).unwrap().1, "an unchanged file is not reparsed");
        // A visit merged and written by this process leaves the memory copy valid.
        add_history_entry_at(&path, 2, "https://b.test/", "B").unwrap();
        drop(_io);
        flush_pending_history();
        let _io = lock_history(&HISTORY_IO);
        let (entries, cached) = load_history(&path).unwrap();
        assert!(cached);
        assert_eq!(entries.len(), 2);
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);
        // An edit from outside is picked up, never overwritten by a stale parse.
        fs::write(&path, "1\thttps://a.test/\tA\n2\thttps://b.test/\tB\n3\thttps://c.test/\tC\n").unwrap();
        let (entries, cached) = load_history(&path).unwrap();
        assert!(!cached);
        assert_eq!(entries.len(), 3);
        drop(_io);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn history_write_dedupes_caps_and_leaves_no_temp_file() {
        let root = history_temp("cap");
        let path = root.join("history.txt");
        for i in 0..MAX_HISTORY + 5 {
            add_history_entry_at(&path, 1_700_000_000 + i as u64, &format!("https://example.com/{i}"), "T").unwrap();
        }
        // A revisit moves the entry to the end instead of adding a line.
        add_history_entry_at(&path, 1_800_000_000, "https://example.com/10", "Again").unwrap();
        flush_pending_history();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), MAX_HISTORY);
        let last = text.lines().last().unwrap();
        assert!(last.starts_with("1800000000\thttps://example.com/10\tAgain"));
        assert!(!text.contains("https://example.com/0\t"), "the oldest entries are dropped at the cap");
        assert_eq!(text.matches("https://example.com/10\t").count(), 1);
        assert!(!root.join("history.tmp").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn history_reads_crlf_bom_missing_titles_and_skips_only_malformed_lines() {
        let root = history_temp("tolerant");
        let path = root.join("history.txt");
        fs::write(
            &path,
            "\u{feff}1\thttps://a.test/\tA\r\nnot a line\r\n2\tftp://b.test/\tB\r\n3\thttps://c.test/\r\n4\thttps://d.test/\tD\r\n",
        )
        .unwrap();
        let urls: Vec<_> = read_history(&path).into_iter().map(|e| e.1).collect();
        assert_eq!(urls, ["https://a.test/", "https://c.test/", "https://d.test/"]);
        // Adding a visit keeps every good entry.
        add_history_entry_at(&path, 5, "https://e.test/", "E").unwrap();
        assert_eq!(read_history(&path).len(), 4);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_large_history_file_is_read_and_extended_not_replaced() {
        let root = history_temp("large");
        let path = root.join("history.txt");
        let mut text = String::new();
        for i in 0..MAX_HISTORY {
            text.push_str(&format!("{}\thttps://example.com/{}{i}\t{}\n", 1_700_000_000 + i, "p".repeat(900), "t".repeat(200)));
        }
        assert!(text.len() > MAX_FILE_BYTES, "the fixture must exceed the generic 256 KiB read bound");
        fs::write(&path, &text).unwrap();
        assert_eq!(read_history(&path).len(), MAX_HISTORY);
        add_history_entry_at(&path, 1_800_000_000, "https://new.test/", "New").unwrap();
        let entries = read_history(&path);
        assert_eq!(entries.len(), MAX_HISTORY, "the oldest entry makes room");
        assert_eq!(entries.last().unwrap().1, "https://new.test/");
        assert_eq!(entries[0].0, 1_700_000_001);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn history_keeps_the_newest_entries_when_the_file_has_too_many_lines() {
        let root = history_temp("newest");
        let path = root.join("history.txt");
        let text: String = (0..MAX_HISTORY + 20).map(|i| format!("{i}\thttps://h.test/{i}\tT\n")).collect();
        fs::write(&path, text).unwrap();
        let entries = read_history(&path);
        assert_eq!(entries.len(), MAX_HISTORY);
        assert_eq!(entries[0].0, 20);
        assert_eq!(entries.last().unwrap().0 as usize, MAX_HISTORY + 19);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_utf8_does_not_empty_the_history() {
        let root = history_temp("utf8");
        let path = root.join("history.txt");
        let mut bytes = b"1\thttps://a.test/\tcaf".to_vec();
        bytes.extend_from_slice(&[0xE9, b'\n']);
        bytes.extend_from_slice(b"2\thttps://b.test/\tB\n");
        fs::write(&path, bytes).unwrap();
        assert_eq!(read_history(&path).len(), 2);
        add_history_entry_at(&path, 3, "https://c.test/", "C").unwrap();
        assert_eq!(read_history(&path).len(), 3);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_queued_visit_is_visible_to_every_read_and_edit_before_it_is_flushed() {
        let root = history_temp("queued");
        let path = root.join("history.txt");
        add_history_entry_at(&path, 1, "https://a.test/", "A").unwrap();
        add_history_entry_at(&path, 2, "https://b.test/", "B").unwrap();
        add_history_entry_at(&path, 3, "https://a.test/", "A again").unwrap();
        // Reads flush first: one entry per URL, the latest visit last.
        let entries = read_history_strict(&path).unwrap();
        assert_eq!(entries, [(2, "https://b.test/".into(), "B".into()), (3, "https://a.test/".into(), "A again".into())]);
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);
        // An edit also sees a visit that is still queued.
        add_history_entry_at(&path, 4, "https://c.test/", "C").unwrap();
        assert!(delete_history_entry(&path, "https://c.test/").unwrap());
        assert_eq!(read_history(&path).len(), 2);
        // Clearing drops visits that were made before it.
        add_history_entry_at(&path, 5, "https://d.test/", "D").unwrap();
        clear_history(&path).unwrap();
        assert!(read_history(&path).is_empty());
        // A visit queued for a different file is untouched by this one's flush.
        let other = root.join("other.txt");
        add_history_entry_at(&other, 6, "https://e.test/", "E").unwrap();
        assert!(read_history(&path).is_empty());
        flush_pending_history();
        assert_eq!(fs::read_to_string(&other).unwrap(), "6\thttps://e.test/\tE\n");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_unreadable_history_file_is_never_overwritten() {
        let root = history_temp("unreadable");
        let path = root.join("history.txt");
        // A directory where the file should be: opening it for reading fails.
        fs::create_dir_all(&path).unwrap();
        assert!(add_history_entry_at(&path, 1, "https://a.test/", "A").is_err());
        assert!(delete_history_entry(&path, "https://a.test/").is_err());
        assert!(path.is_dir(), "nothing replaced the unreadable path");
        // A missing file is simply empty and is created by the first visit.
        let missing = root.join("new.txt");
        assert!(read_history(&missing).is_empty());
        add_history_entry_at(&missing, 1, "https://a.test/", "A").unwrap();
        assert_eq!(read_history(&missing).len(), 1);
        let _ = fs::remove_dir_all(root);
    }
}
