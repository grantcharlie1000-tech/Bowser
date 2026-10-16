//! The Sessions panel's wire (`ui/src/panels/sessions.ts`): the `sessions` snapshot and the
//! `{t:"sess",a,...}` commands, validated here so the dialog host only acts on a well-formed command.
//!
//! - `save` `{text:name, n:0|1}`: all the window's tabs (0) or the selected ones (1) as a session.
//! - `open` `{text:name, n:0|1}`: the saved session's pages added to this window (0) or replacing the
//!   open tabs (1).
//! - `delete` `{text:name}`.
//!
//! A command names a session by its name; the host finds it in the file again, so a list that went
//! stale in the page never opens or deletes the wrong one.

use super::super::{json::capped, protocol::{self, PanelFields}};
use crate::productivity::SavedSession;

/// Most characters a session name may have (the app's own cap).
pub const MAX_NAME_CHARS: usize = 100;
/// Sites listed under a session in the panel.
pub const MAX_HOSTS: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Save { name: String, selected: bool },
    Open { name: String, replace: bool },
    Delete { name: String },
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= MAX_NAME_CHARS && !name.chars().any(char::is_control)
}

/// Maps a `sess` command to an [`Action`]: each with exactly its own fields.
pub fn map_command(f: &PanelFields) -> Option<Action> {
    if f.id.is_some() || f.parent.is_some() || f.before.is_some() || f.on.is_some() || f.url.is_some() {
        return None;
    }
    let name = f.text.as_deref().filter(|name| valid_name(name))?;
    let flag = |n: Option<i32>| match n {
        Some(0) => Some(false),
        Some(1) => Some(true),
        _ => None,
    };
    match f.a.as_str() {
        "save" => Some(Action::Save { name: name.trim().to_owned(), selected: flag(f.n)? }),
        "open" => Some(Action::Open { name: name.to_owned(), replace: flag(f.n)? }),
        "delete" if f.n.is_none() => Some(Action::Delete { name: name.to_owned() }),
        _ => None,
    }
}

/// "Session 3": the first of "Session N" that is not taken.
pub fn next_name(sessions: &[SavedSession]) -> String {
    (1..).map(|n| format!("Session {n}")).find(|name| !sessions.iter().any(|session| &session.name == name)).unwrap_or_else(|| "Session".to_owned())
}

/// Up to [`MAX_HOSTS`] distinct sites of a session's pages, without `www.`.
pub fn hosts(session: &SavedSession) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for address in &session.urls {
        let Some(host) = url::Url::parse(address).ok().and_then(|url| url.host_str().map(|host| host.trim_start_matches("www.").to_owned())) else {
            continue;
        };
        if !seen.contains(&host) {
            seen.push(host);
        }
        if seen.len() == MAX_HOSTS {
            break;
        }
    }
    seen
}

/// `{"v":1,"t":"snap","p":"sessions","d":{allowed,tabs,selected,suggest,items}}`.
pub fn snapshot_json(allowed: bool, tabs: usize, selected: usize, sessions: &[SavedSession]) -> String {
    let suggest = next_name(sessions);
    protocol::snap_json("sessions", |out| {
        out.bool("allowed", allowed);
        out.uint("tabs", tabs as u64);
        out.uint("selected", selected as u64);
        out.string("suggest", &suggest);
        out.array("items", |items| {
            for session in sessions {
                items.object(|item| {
                    item.string("name", capped(&session.name, 128));
                    item.uint("count", session.urls.len() as u64);
                    item.array("hosts", |list| {
                        for host in hosts(session) {
                            list.string(capped(&host, 64));
                        }
                    });
                });
            }
        });
    })
    .expect("sessions is a panel token")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(a: &str, text: Option<&str>, n: Option<i32>) -> PanelFields {
        PanelFields { a: a.into(), text: text.map(str::to_owned), n, ..PanelFields::default() }
    }

    fn session(name: &str, urls: &[&str]) -> SavedSession {
        SavedSession { name: name.into(), urls: urls.iter().map(|url| (*url).to_owned()).collect(), active: 0 }
    }

    #[test]
    fn commands_carry_exactly_their_own_fields() {
        assert_eq!(map_command(&fields("save", Some("  Work  "), Some(0))), Some(Action::Save { name: "Work".into(), selected: false }));
        assert_eq!(map_command(&fields("save", Some("Work"), Some(1))), Some(Action::Save { name: "Work".into(), selected: true }));
        assert_eq!(map_command(&fields("open", Some("Work"), Some(1))), Some(Action::Open { name: "Work".into(), replace: true }));
        assert_eq!(map_command(&fields("open", Some("Work"), Some(0))), Some(Action::Open { name: "Work".into(), replace: false }));
        assert_eq!(map_command(&fields("delete", Some("Work"), None)), Some(Action::Delete { name: "Work".into() }));
        // Wrong or missing fields are refused.
        for bad in [
            fields("save", Some("Work"), None),
            fields("save", Some("Work"), Some(2)),
            fields("open", Some("Work"), Some(-1)),
            fields("delete", Some("Work"), Some(0)),
            fields("save", None, Some(0)),
            fields("save", Some("   "), Some(0)),
            fields("save", Some("a\nb"), Some(0)),
            fields("save", Some(&"x".repeat(MAX_NAME_CHARS + 1)), Some(0)),
            fields("rename", Some("Work"), Some(0)),
            fields("", Some("Work"), Some(0)),
        ] {
            assert_eq!(map_command(&bad), None, "{bad:?}");
        }
        let mut with_id = fields("delete", Some("Work"), None);
        with_id.id = Some(1);
        assert_eq!(map_command(&with_id), None);
        let mut with_url = fields("open", Some("Work"), Some(0));
        with_url.url = Some("https://example.com/".into());
        assert_eq!(map_command(&with_url), None);
        assert!(map_command(&fields("save", Some(&"x".repeat(MAX_NAME_CHARS)), Some(0))).is_some(), "the longest name is allowed");
    }

    #[test]
    fn the_suggested_name_is_the_first_free_session_number() {
        assert_eq!(next_name(&[]), "Session 1");
        assert_eq!(next_name(&[session("Session 1", &[]), session("Work", &[])]), "Session 2");
        assert_eq!(next_name(&[session("Session 2", &[])]), "Session 1");
    }

    #[test]
    fn a_session_lists_up_to_three_distinct_sites() {
        let s = session("S", &["https://www.a.example/1", "https://a.example/2", "https://b.example/", "not a url", "https://c.example/", "https://d.example/"]);
        assert_eq!(hosts(&s), ["a.example", "b.example", "c.example"]);
        assert_eq!(hosts(&session("E", &[])), Vec::<String>::new());
    }

    #[test]
    fn the_snapshot_is_what_the_page_parses() {
        let sessions = vec![session("Work \"<b>\"", &["https://www.example.com/a", "https://news.example/"]), session("Empty-ish", &["https://one.example/"])];
        let json = snapshot_json(true, 5, 2, &sessions);
        assert!(json.starts_with(r#"{"v":1,"t":"snap","p":"sessions","d":{"allowed":true,"tabs":5,"selected":2,"suggest":"Session 1","items":["#), "{json}");
        assert!(json.contains(r#""name":"Work \"\u003cb>\"","count":2,"hosts":["example.com","news.example"]"#), "{json}");
        let none = snapshot_json(false, 0, 0, &[]);
        assert!(none.contains(r#""allowed":false"#) && none.ends_with(r#""items":[]}}"#), "{none}");
    }

    #[test]
    fn sessions_messages_match_committed_fixture() {
        let sessions = vec![session("Work", &["https://www.example.com/a", "https://news.example/"]), session("Reading list", &["https://one.example/"])];
        let content = format!("{{\"snap\":{},\"guest\":{}}}\n", snapshot_json(true, 4, 3, &sessions), snapshot_json(false, 2, 0, &[]));
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/sessions-messages.json");
        if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some_and(|value| value == "1") {
            std::fs::create_dir_all(path.parent().expect("fixtures directory")).expect("create fixtures directory");
            std::fs::write(&path, content).expect("write sessions messages fixture");
        } else {
            assert_eq!(std::fs::read_to_string(path).expect("committed sessions messages fixture"), content);
        }
    }
}
