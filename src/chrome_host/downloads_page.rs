//! The downloads page's commands (docs/design/downloads.md "Page").
//!
//! The snapshot itself (`{t:"snap",p:"downloadspage",d:{...}}`) is written by the downloads list
//! (`DownloadManager::snapshot_for_page`, reached through `App::downloads_page_json`). This module
//! holds the other direction: `{t:"dpage",a:...}` commands come back, and [`map_command`] accepts
//! each action only with exactly its own fields, as the history page does for `hpage`. Entry ids
//! are checked against the live list by `App::download_act`, never trusted here.

use super::protocol::PanelFields;
use crate::downloads::{Action as Row, MAX_QUERY_CHARS};

/// A validated command from the downloads page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// The search text changed (debounced by the page); `""` shows everything.
    Search(String),
    /// A row action on entry `id` (open, show in folder, remove, pause, resume, cancel, retry, copy).
    Row { id: u32, action: Row },
    /// A list action: open the downloads folder, or clear every finished entry (the page asks first).
    List(Row),
    /// Put the last removal (or clear) back.
    Undo,
    /// The page wants a fresh snapshot (it regained focus, or "Try again").
    Refresh,
    /// Settings, at the Downloads section.
    Settings,
}

/// Maps `{t:"dpage",a,...}` to an [`Action`]: each action with exactly its own fields.
pub fn map_command(f: &PanelFields) -> Option<Action> {
    if f.parent.is_some() || f.before.is_some() || f.n.is_some() || f.on.is_some() || f.url.is_some() {
        return None;
    }
    let bare = f.id.is_none() && f.text.is_none();
    let id_only = f.id.is_some() && f.text.is_none();
    let text_only = f.id.is_none() && f.text.is_some();
    let id = || f.id.and_then(|id| u32::try_from(id).ok()).filter(|id| *id > 0);
    let row = |action: Row| Some(Action::Row { id: id()?, action });
    Some(match f.a.as_str() {
        "search" if text_only => {
            let text = f.text.as_deref()?;
            if text.chars().count() > MAX_QUERY_CHARS || text.chars().any(char::is_control) {
                return None;
            }
            Action::Search(text.to_owned())
        }
        "open" if id_only => row(Row::Open)?,
        "show" if id_only => row(Row::Show)?,
        "remove" if id_only => row(Row::Remove)?,
        "pause" if id_only => row(Row::Pause)?,
        "resume" if id_only => row(Row::Resume)?,
        "cancel" if id_only => row(Row::Cancel)?,
        "retry" if id_only => row(Row::Retry)?,
        "copy" if id_only => row(Row::CopyLink)?,
        "folder" if bare => Action::List(Row::OpenFolder),
        "clear" if bare => Action::List(Row::ClearAll),
        "undo" if bare => Action::Undo,
        "refresh" if bare => Action::Refresh,
        "settings" if bare => Action::Settings,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(a: &str) -> PanelFields {
        PanelFields { a: a.to_owned(), ..PanelFields::default() }
    }

    #[test]
    fn commands_carry_exactly_their_own_fields() {
        let mut search = fields("search");
        search.text = Some("zip".into());
        assert_eq!(map_command(&search), Some(Action::Search("zip".into())));
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

        for (name, action) in [
            ("open", Row::Open),
            ("show", Row::Show),
            ("remove", Row::Remove),
            ("pause", Row::Pause),
            ("resume", Row::Resume),
            ("cancel", Row::Cancel),
            ("retry", Row::Retry),
            ("copy", Row::CopyLink),
        ] {
            let mut command = fields(name);
            assert_eq!(map_command(&command), None, "{name} needs an id");
            command.id = Some(9);
            assert_eq!(map_command(&command), Some(Action::Row { id: 9, action }), "{name}");
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
        for (name, expect) in [
            ("folder", Action::List(Row::OpenFolder)),
            ("clear", Action::List(Row::ClearAll)),
            ("undo", Action::Undo),
            ("refresh", Action::Refresh),
            ("settings", Action::Settings),
        ] {
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
        // Page commands never name a path or a URL, and list actions have no wire spelling that
        // reaches a row: `OpenPage` is the bubble's, not the page's.
        for unknown in ["openpage", "dismiss", "navigate", "save", "download", "Open", "newtab", "privacy"] {
            let mut command = fields(unknown);
            assert_eq!(map_command(&command), None, "{unknown}");
            command.id = Some(1);
            assert_eq!(map_command(&command), None, "{unknown} with id");
        }
    }
}
