//! The history page's wire and the pure checks behind its commands (docs/design/history-page.md).
//!
//! - `snap` with `p:"historypage"` carries what the page shows: the recording state, the query it
//!   answers, the day-grouped rows (the rail panel's model, `win32::HistoryModel`), cached favicons
//!   and the answer to the last change. Every string is capped and the whole message is kept under
//!   what the page accepts.
//! - `{t:"hpage",a:...}` commands come back; [`map_command`] accepts each action only with exactly
//!   its own fields, and [`resolve`] maps an entry id to the live entry (stale or ambiguous ids
//!   resolve to nothing).

use super::json::{capped, Json};
use super::panels::history::entry_id;
use super::protocol::{favicon_data_uri, snap_json, PanelFields};
use crate::productivity::HistoryEntry;
use crate::win32::{history_civil_date as civil_date, HistoryLine as Line, HistoryModel as Model, HistoryRow as Row};

/// Longest search the page sends (its input's `maxLength`).
pub const MAX_QUERY_CHARS: usize = 256;
const MAX_TITLE_CHARS: usize = 120;
const MAX_HOST_CHARS: usize = 64;
const MAX_URL_CHARS: usize = 200;
/// Most hosts whose cached favicon rides along, and their total data-URI bytes.
const MAX_FAV_HOSTS: usize = 96;
const MAX_FAV_BYTES: usize = 128 * 1024;
/// Largest snapshot sent: the page refuses inbound messages over 256 KiB (`MAX_INBOUND`).
pub const MAX_SNAPSHOT_BYTES: usize = 240 * 1024;

/// Whether this profile records history, as the page shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Recording; rows follow.
    On,
    /// The profile's "Save browsing history" switch is off.
    Off,
    /// A guest window: nothing is ever saved.
    Guest,
    /// The history file exists but could not be read (or no profile is ready).
    Error,
}

impl State {
    pub fn key(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Guest => "guest",
            Self::Error => "error",
        }
    }
}

/// How Bowser answered the page's last change. Sent once, with the snapshot that follows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ack {
    #[default]
    None,
    Removed,
    Restored,
    Failed,
}

impl Ack {
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Removed => "removed",
            Self::Restored => "restored",
            Self::Failed => "failed",
        }
    }
}

/// Everything one snapshot reports. `entries` are what `productivity::search_entries` returned for
/// `query` (newest first); they are ignored unless `state` is [`State::On`].
#[derive(Clone, Copy, Debug)]
pub struct Snapshot<'a> {
    pub state: State,
    pub query: &'a str,
    pub entries: &'a [HistoryEntry],
    pub now: u64,
    pub offset_secs: i64,
    /// The last removal can still be undone.
    pub undo: bool,
    pub ack: Ack,
}

/// The snapshot, with cached favicons from the profile's favicon cache.
pub fn snapshot_json(s: &Snapshot<'_>) -> String {
    snapshot_json_with(s, |host| crate::favicon::load_favicon_bytes(host).ok().flatten())
}

/// [`snapshot_json`] with an injectable favicon lookup (host to cached PNG bytes).
pub fn snapshot_json_with(s: &Snapshot<'_>, favicon_of: impl Fn(&str) -> Option<Vec<u8>>) -> String {
    let entries: &[HistoryEntry] = if s.state == State::On { s.entries } else { &[] };
    let mut model = Model::default();
    model.set_query(s.query);
    // Drop the oldest rows until the message fits (only very long titles or addresses get here).
    let mut shown = entries.len();
    let mut json;
    loop {
        model.rebuild(&entries[..shown], s.now, s.offset_secs);
        json = build(s, &model, shown < entries.len(), &[]);
        if json.len() + 1024 <= MAX_SNAPSHOT_BYTES || shown == 0 {
            break;
        }
        shown = shown * 3 / 4;
    }
    // Favicons fill what is left of the budget, for the hosts of the rows shown, in row order.
    let budget = MAX_FAV_BYTES.min(MAX_SNAPSHOT_BYTES.saturating_sub(json.len() + 1024));
    let mut favs: Vec<(&str, String)> = Vec::new();
    let mut total = 0;
    for line in &model.lines {
        let host = capped(&line.host, MAX_HOST_CHARS);
        if favs.len() == MAX_FAV_HOSTS {
            break;
        }
        if host.is_empty() || favs.iter().any(|(seen, _)| *seen == host) {
            continue;
        }
        let Some(uri) = favicon_of(host).as_deref().and_then(favicon_data_uri) else { continue };
        // Key, quotes, colon and comma.
        let cost = uri.len() + host.len() + 6;
        if total + cost > budget {
            continue;
        }
        total += cost;
        favs.push((host, uri));
    }
    if favs.is_empty() { json } else { build(s, &model, shown < entries.len(), &favs) }
}

fn build(s: &Snapshot<'_>, model: &Model, more: bool, favs: &[(&str, String)]) -> String {
    snap_json("historypage", |out| {
        out.string("state", s.state.key());
        out.string("query", capped(s.query, MAX_QUERY_CHARS));
        out.array("rows", |rows| {
            for (index, row) in model.rows.iter().enumerate() {
                match row {
                    Row::Header { label, count } => {
                        // The local date of the group's first entry (also for Today and Yesterday).
                        let date = model.rows[index + 1..].iter().find_map(|row| match row {
                            Row::Entry(line) => model.lines.get(*line),
                            Row::Header { .. } => None,
                        });
                        let date = date.map(|line| civil_date((line.timestamp as i64 + s.offset_secs).max(0) / 86_400)).unwrap_or_default();
                        rows.object(|row| {
                            row.string("label", label);
                            row.string("date", &date);
                            row.uint("count", *count as u64);
                        });
                    }
                    Row::Entry(line) => {
                        let Some(line) = model.lines.get(*line) else { continue };
                        rows.object(|row| {
                            row.uint("id", u64::from(entry_id(line)));
                            row.string("title", capped(line.title.trim(), MAX_TITLE_CHARS));
                            row.string("host", capped(&line.host, MAX_HOST_CHARS));
                            row.string("url", capped(&line.url, MAX_URL_CHARS));
                            row.string("clock", &line.clock);
                        });
                    }
                }
            }
        });
        out.bool("more", more);
        out.bool("undo", s.undo);
        out.string("ack", s.ack.key());
        out.object("favs", |out| {
            for (host, uri) in favs {
                out.string(host, uri);
            }
        });
    })
    .unwrap_or_else(|| Json::object(|_| {}))
}

/// A validated command from the history page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// The search text changed (debounced by the page); `""` shows everything.
    Search(String),
    /// Open entry `id` here, or in a new tab (Ctrl or middle click, the row's button).
    Open { id: u32, new_tab: bool },
    /// Remove entry `id` (History keeps one line per address, so this is every visit to it).
    Remove(u32),
    /// Put the last removed entry back.
    Undo,
    /// "Clear browsing data": the privacy dashboard.
    Privacy,
    /// "History is off": Settings, at Privacy and security.
    Settings,
    /// The page wants a fresh snapshot (it regained focus).
    Refresh,
}

/// Maps `{t:"hpage",a,...}` to an [`Action`]: each action with exactly its own fields.
pub fn map_command(f: &PanelFields) -> Option<Action> {
    if f.parent.is_some() || f.before.is_some() || f.n.is_some() || f.on.is_some() || f.url.is_some() {
        return None;
    }
    let bare = f.id.is_none() && f.text.is_none();
    let id_only = f.id.is_some() && f.text.is_none();
    let text_only = f.id.is_none() && f.text.is_some();
    let id = || f.id.and_then(|id| u32::try_from(id).ok()).filter(|id| *id > 0);
    Some(match f.a.as_str() {
        "search" if text_only => {
            let text = f.text.as_deref()?;
            if text.chars().count() > MAX_QUERY_CHARS || text.chars().any(char::is_control) {
                return None;
            }
            Action::Search(text.to_owned())
        }
        "open" if id_only => Action::Open { id: id()?, new_tab: false },
        "newtab" if id_only => Action::Open { id: id()?, new_tab: true },
        "remove" if id_only => Action::Remove(id()?),
        "undo" if bare => Action::Undo,
        "privacy" if bare => Action::Privacy,
        "settings" if bare => Action::Settings,
        "refresh" if bare => Action::Refresh,
        _ => return None,
    })
}

/// The live entry an id names, only when exactly one line has it.
pub(crate) fn resolve(lines: &[Line], id: u32) -> Option<&Line> {
    let mut matches = lines.iter().filter(|line| entry_id(line) == id);
    let line = matches.next()?;
    matches.next().is_none().then_some(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;
    // 2026-10-14 12:00 UTC.
    const NOON: u64 = 20_740 * DAY + 12 * 3600;

    fn fields(a: &str) -> PanelFields {
        PanelFields { a: a.to_owned(), ..PanelFields::default() }
    }

    fn entries() -> Vec<HistoryEntry> {
        vec![
            (NOON - 600, "https://www.rust-lang.org/learn".into(), "Learn Rust".into()),
            (NOON - 3 * 3600, "https://docs.rs/serde".into(), "serde - Rust".into()),
            (NOON - DAY - 60, "https://news.example/a?b=<c>".into(), "News \"<b>\"".into()),
            (NOON - 5 * DAY, "https://old.example.org/".into(), "".into()),
        ]
    }

    fn sample<'a>(entries: &'a [HistoryEntry]) -> Snapshot<'a> {
        Snapshot { state: State::On, query: "", entries, now: NOON, offset_secs: 0, undo: false, ack: Ack::None }
    }

    fn png(len: usize) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 16, 0, 0, 0, 16];
        bytes.resize(len, 0);
        bytes
    }

    #[test]
    fn the_snapshot_groups_by_local_day_and_dates_every_group() {
        let list = entries();
        let json = snapshot_json_with(&sample(&list), |_| None);
        assert!(json.starts_with(r#"{"v":1,"t":"snap","p":"historypage","d":{"state":"on","query":"","rows":[{"label":"Today","date":"2026-10-14","count":2},{"id":"#), "{json}");
        assert!(json.contains(r#""title":"Learn Rust","host":"rust-lang.org","url":"https://www.rust-lang.org/learn","clock":"11:50"}"#), "{json}");
        assert!(json.contains(r#"{"label":"Yesterday","date":"2026-10-13","count":1}"#), "{json}");
        assert!(json.contains(r#"{"label":"2026-10-09","date":"2026-10-09","count":1}"#), "{json}");
        // Text stays text: quotes escaped, `<` written as a \u escape (built here from parts).
        let lt = ["\\", "u003c"].concat();
        assert!(json.contains(&format!(r#""title":"News \"{lt}b>\"""#)), "{json}");
        assert!(!json.contains('<'), "{json}");
        assert!(json.ends_with(r#""more":false,"undo":false,"ack":"none","favs":{}}}"#), "{json}");
        // A day group late in the evening at UTC+2 is the next local day.
        let late = vec![(NOON + 11 * 3600, "https://a.example/".to_owned(), "A".to_owned())];
        let shifted = snapshot_json_with(&Snapshot { now: NOON + 11 * 3600, offset_secs: 7200, ..sample(&late) }, |_| None);
        assert!(shifted.contains(r#"{"label":"Today","date":"2026-10-15","count":1}"#), "{shifted}");
    }

    #[test]
    fn off_guest_and_unreadable_profiles_show_no_rows() {
        let list = entries();
        for state in [State::Off, State::Guest, State::Error] {
            let json = snapshot_json_with(&Snapshot { state, ..sample(&list) }, |_| Some(png(40)));
            assert!(json.contains(&format!(r#""state":"{}","query":"","rows":[]"#, state.key())), "{json}");
            assert!(!json.contains("rust-lang"), "{json}");
            assert!(json.ends_with(r#""favs":{}}}"#), "{json}");
        }
    }

    #[test]
    fn the_snapshot_stays_under_the_page_limit_and_says_when_rows_were_left_out() {
        // 500 visits with the longest titles and addresses the wire carries.
        let long: Vec<HistoryEntry> = (0..500u64)
            .map(|i| (NOON - i * 60, format!("https://h{i}.example/{}", "p".repeat(400)), "\u{4e2d}".repeat(300)))
            .collect();
        let json = snapshot_json_with(&sample(&long), |_| Some(png(protocol_max())));
        assert!(json.len() <= MAX_SNAPSHOT_BYTES, "{}", json.len());
        assert!(json.contains(r#""more":true"#));
        assert!(json.matches(r#""id":"#).count() < 500);
        // Ordinary history fits whole, with favicons for the first hosts only.
        let plain: Vec<HistoryEntry> = (0..500u64).map(|i| (NOON - i * 60, format!("https://h{i}.example/"), format!("Page {i}"))).collect();
        let json = snapshot_json_with(&sample(&plain), |_| Some(png(40)));
        assert!(json.contains(r#""more":false"#));
        assert_eq!(json.matches(r#""id":"#).count(), 500);
        assert_eq!(json.matches("data:image/png").count(), MAX_FAV_HOSTS);
        assert!(json.len() <= MAX_SNAPSHOT_BYTES);
    }

    fn protocol_max() -> usize {
        super::super::protocol::MAX_FAVICON_BYTES
    }

    #[test]
    fn favicons_go_once_per_host_and_only_for_sendable_pngs() {
        let list = entries();
        let icon = png(40);
        let json = snapshot_json_with(&sample(&list), |host| match host {
            "rust-lang.org" => Some(icon.clone()),
            "docs.rs" => Some(vec![0, 0, 1, 0]),
            _ => None,
        });
        let uri = favicon_data_uri(&icon).unwrap();
        assert!(json.ends_with(&format!(r#""favs":{{"rust-lang.org":"{uri}"}}}}}}"#)), "{json}");
    }

    #[test]
    fn commands_carry_exactly_their_own_fields() {
        let mut search = fields("search");
        search.text = Some("rust".into());
        assert_eq!(map_command(&search), Some(Action::Search("rust".into())));
        search.text = Some(String::new());
        assert_eq!(map_command(&search), Some(Action::Search(String::new())));
        for bad in ["a\nb".to_owned(), "x".repeat(MAX_QUERY_CHARS + 1)] {
            search.text = Some(bad);
            assert_eq!(map_command(&search), None);
        }
        search.text = Some("ok".into());
        search.id = Some(1);
        assert_eq!(map_command(&search), None, "search with an id");
        assert_eq!(map_command(&fields("search")), None, "search needs its text");

        for (name, expect) in [("open", Action::Open { id: 9, new_tab: false }), ("newtab", Action::Open { id: 9, new_tab: true }), ("remove", Action::Remove(9))] {
            let mut command = fields(name);
            assert_eq!(map_command(&command), None, "{name} needs an id");
            command.id = Some(9);
            assert_eq!(map_command(&command), Some(expect), "{name}");
            for id in [0, u64::from(u32::MAX) + 1] {
                command.id = Some(id);
                assert_eq!(map_command(&command), None, "{name} {id}");
            }
            command.id = Some(9);
            command.text = Some("x".into());
            assert_eq!(map_command(&command), None, "{name} with text");
            command.text = None;
            command.url = Some("https://evil.example/".into());
            assert_eq!(map_command(&command), None, "{name} with a url");
        }
        for (name, expect) in [("undo", Action::Undo), ("privacy", Action::Privacy), ("settings", Action::Settings), ("refresh", Action::Refresh)] {
            assert_eq!(map_command(&fields(name)), Some(expect), "{name}");
            let mut with_id = fields(name);
            with_id.id = Some(1);
            assert_eq!(map_command(&with_id), None, "{name} with id");
            let mut with_on = fields(name);
            with_on.on = Some(true);
            assert_eq!(map_command(&with_on), None, "{name} with on");
            let mut with_n = fields(name);
            with_n.n = Some(2);
            assert_eq!(map_command(&with_n), None, "{name} with n");
        }
        for unknown in ["clear", "removesite", "dismiss", "navigate", "save", "history", "Open"] {
            let mut command = fields(unknown);
            assert_eq!(map_command(&command), None, "{unknown}");
            command.id = Some(1);
            assert_eq!(map_command(&command), None, "{unknown} with id");
        }
    }

    #[test]
    fn ids_resolve_only_to_one_live_entry() {
        let mut model = Model::default();
        model.rebuild(&entries(), NOON, 0);
        let first = &model.lines[0];
        let id = entry_id(first);
        assert_eq!(resolve(&model.lines, id).map(|line| line.url.as_str()), Some("https://www.rust-lang.org/learn"));
        assert_eq!(resolve(&model.lines, id ^ 1), None);
        // Two lines with the same id (a hash collision) resolve to neither.
        let twice = vec![first.clone(), first.clone()];
        assert_eq!(resolve(&twice, id), None);
        // The page's ids are the snapshot's ids.
        let json = snapshot_json_with(&sample(&entries()), |_| None);
        assert!(json.contains(&format!(r#""id":{id},"#)));
    }

    #[test]
    fn history_page_messages_match_committed_fixture() {
        let list = entries();
        let icon = png(40);
        let favs = |host: &str| (host == "docs.rs").then(|| icon.clone());
        let snap = snapshot_json_with(&sample(&list), favs);
        let searched: Vec<HistoryEntry> = list.iter().filter(|(_, url, _)| url.contains("rust")).cloned().collect();
        let search = snapshot_json_with(&Snapshot { query: "rust", ..sample(&searched) }, |_| None);
        let none = snapshot_json_with(&Snapshot { query: "zzz", ..sample(&[]) }, |_| None);
        let empty = snapshot_json_with(&sample(&[]), |_| None);
        let removed = snapshot_json_with(&Snapshot { undo: true, ack: Ack::Removed, ..sample(&list[1..]) }, |_| None);
        let off = snapshot_json_with(&Snapshot { state: State::Off, ..sample(&list) }, |_| None);
        let guest = snapshot_json_with(&Snapshot { state: State::Guest, ..sample(&list) }, |_| None);
        let error = snapshot_json_with(&Snapshot { state: State::Error, ack: Ack::Failed, ..sample(&list) }, |_| None);
        let content = format!(
            "{{\"snap\":{snap},\"search\":{search},\"none\":{none},\"empty\":{empty},\"removed\":{removed},\"off\":{off},\"guest\":{guest},\"error\":{error}}}\n"
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/history-page-messages.json");
        if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some_and(|value| value == "1") {
            std::fs::create_dir_all(path.parent().expect("fixtures directory")).expect("create fixtures directory");
            std::fs::write(&path, content).expect("write history page fixture");
        } else {
            assert_eq!(std::fs::read_to_string(path).expect("committed history page fixture"), content);
        }
    }
}
