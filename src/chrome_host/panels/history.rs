use super::super::{json::{self, Json}, protocol::{self, PanelFields}};
use crate::{
    productivity::HistoryEntry,
    win32::{
        HistoryAction as Action, HistoryLine as Line, HistoryModel as Model,
        HistoryRange as Range, HistoryRow as Row,
    },
};

/// Builds a history snapshot from the same grouped model used by the native panel.
pub(crate) fn snapshot(entries: &[HistoryEntry], query: &str, now: u64, offset_secs: i64) -> String {
    let mut model = Model::default();
    model.set_query(query);
    model.rebuild(entries, now, offset_secs);
    protocol::snap_json("history", |out| {
        out.array("rows", |rows| {
            for row in &model.rows {
                match row {
                    Row::Header { label, count } => rows.object(|row| {
                        row.string("label", label);
                        row.uint("count", *count as u64);
                    }),
                    Row::Entry(index) => if let Some(line) = model.lines.get(*index) {
                        rows.object(|row| {
                            row.uint("id", u64::from(entry_id(line)));
                            row.string("title", json::capped(&line.title, 48));
                            row.string("host", json::capped(&line.host, 64));
                            row.string("clock", &line.clock);
                        });
                    },
                }
            }
        });
        out.string("query", query);
    }).unwrap_or_else(|| Json::object(|_| {}))
}

/// Stable 32-bit id for one stored visit. Commands still resolve it against the live model and
/// reject collisions, so a stale or ambiguous id cannot target another entry.
fn entry_id(line: &Line) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in line.timestamp.to_le_bytes().into_iter().chain(line.url.bytes()) {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    hash.max(1)
}

/// Converts one parsed bridge command using ids from the currently displayed Rust model.
pub(crate) fn map_command(model: &mut Model, fields: PanelFields) -> Option<Action> {
    match fields.a.as_str() {
        "search" if fields.id.is_none() && fields.parent.is_none() && fields.before.is_none()
            && fields.n.is_none() && fields.on.is_none() && fields.text.is_some() && fields.url.is_none() => {
            model.set_query(fields.text.as_deref()?);
            Some(Action::Redraw)
        }
        "dismiss" if fields.id.is_none() && fields.parent.is_none() && fields.before.is_none()
            && fields.n.is_none() && fields.on.is_none() && fields.text.is_none() && fields.url.is_none() => Some(Action::Dismiss),
        "clear" if fields.id.is_none() && fields.parent.is_none() && fields.before.is_none()
            && fields.on.is_none() && fields.text.is_none() && fields.url.is_none() => Some(Action::ClearRange(match fields.n? {
            0 => Range::LastHour,
            1 => Range::Today,
            2 => Range::All,
            _ => return None,
        })),
        action @ ("open" | "newtab" | "remove" | "removesite")
            if fields.parent.is_none() && fields.before.is_none() && fields.n.is_none()
                && fields.on.is_none() && fields.text.is_none() && fields.url.is_none() => {
                let id = u32::try_from(fields.id?).ok()?;
                let mut matches = model.lines.iter().filter(|line| entry_id(line) == id);
                let line = matches.next()?;
                if matches.next().is_some() { return None; }
                Some(match action {
                    "open" | "newtab" => Action::Open { url: line.url.clone(), new_tab: action == "newtab" },
                    "remove" => Action::RemoveEntry { url: line.url.clone() },
                    _ if !line.host.is_empty() => Action::RemoveSite { host: line.host.clone() },
                    _ => return None,
                })
            }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_uses_the_native_day_groups_and_one_based_entry_ids() {
        let entries = vec![(0, "https://example.com/path".to_owned(), "Page".to_owned())];
        assert_eq!(snapshot(&entries, "", 0, 0), r#"{"v":1,"t":"snap","p":"history","d":{"rows":[{"label":"Today","count":1},{"id":2439783619,"title":"Page","host":"example.com","clock":"00:00"}],"query":""}}"#);
    }

    #[test]
    fn commands_map_only_live_entry_ids_and_known_clear_ranges() {
        let mut model = Model {
            lines: vec![Line { timestamp: 1, url: "https://example.com/".into(), title: "Example".into(), host: "example.com".into(), clock: "00:00".into() }],
            rows: vec![Row::Entry(0)],
            ..Model::default()
        };
        assert_eq!(map_command(&mut model, command("open", Some(4_022_810_993), None)), Some(Action::Open { url: "https://example.com/".into(), new_tab: false }));
        assert_eq!(map_command(&mut model, command("open", Some(42), None)), None);
        assert_eq!(map_command(&mut model, command("remove", Some(4_022_810_993), Some(9))), None);
        assert_eq!(map_command(&mut model, command("removesite", Some(4_022_810_993), None)), Some(Action::RemoveSite { host: "example.com".into() }));
        let mut clear = command("clear", None, Some(1));
        assert_eq!(map_command(&mut model, clear.clone()), Some(Action::ClearRange(Range::Today)));
        clear.n = Some(9);
        assert_eq!(map_command(&mut model, clear), None);
    }
}

#[cfg(test)]
fn command(a: &str, id: Option<u64>, n: Option<i32>) -> PanelFields {
    PanelFields { a: a.into(), id, n, ..PanelFields::default() }
}
