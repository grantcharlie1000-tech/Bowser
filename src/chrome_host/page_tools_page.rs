//! The wire of the settings page's "Page tools" section (thread T7).
//!
//! - `{t:"snap",p:"pagetools",d:{s:{<key>:<value>},guest,ack,error}}` carries every option.
//! - `{t:"set",a:"ptset",text:"key=value"}` comes back; [`map_command`] accepts it with exactly those
//!   fields, and the key and value are checked again in [`crate::page_tools::PageTools::set`].

use super::protocol::{snap_json, PanelFields};
use crate::page_tools::{Kind, PageTools, KEYS};

const MAX_ERROR_CHARS: usize = 240;

/// How Bowser answered the page's last change. Sent once, with the snapshot that follows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ack {
    #[default]
    None,
    Saved,
    Failed,
}

impl Ack {
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Saved => "saved",
            Self::Failed => "failed",
        }
    }
}

/// The snapshot of every option.
pub fn snapshot_json(settings: &PageTools, guest: bool, ack: Ack, error: &str) -> String {
    snap_json("pagetools", |out| {
        out.object("s", |o| {
            for (key, kind) in KEYS {
                let Some(value) = settings.get(key) else { continue };
                match kind {
                    Kind::Bool => o.bool(key, value == "true"),
                    Kind::Int => o.uint(key, value.parse().unwrap_or(0)),
                    Kind::Text => o.string(key, &value),
                }
            }
        });
        out.bool("guest", guest);
        out.string("ack", ack.key());
        let clean: String = error.chars().take(MAX_ERROR_CHARS).map(|c| if c.is_control() { ' ' } else { c }).collect();
        out.string("error", clean.trim());
    })
    .expect("pagetools is a panel token")
}

/// A validated change: one option as `key` and its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub key: String,
    pub value: String,
}

/// Maps `{t:"set",a:"ptset",text}`; anything else is `None`.
pub fn map_command(f: &PanelFields) -> Option<Change> {
    if f.a != "ptset" || f.id.is_some() || f.parent.is_some() || f.before.is_some() || f.n.is_some() || f.url.is_some() || f.on.is_some() {
        return None;
    }
    let (key, value) = f.text.as_deref()?.split_once('=')?;
    PageTools::owns(key).then(|| Change { key: key.to_owned(), value: value.to_owned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(text: Option<&str>) -> PanelFields {
        PanelFields { a: "ptset".into(), text: text.map(str::to_owned), ..PanelFields::default() }
    }

    #[test]
    fn a_change_is_one_owned_key_and_value() {
        assert_eq!(map_command(&fields(Some("pt_spatial_nav=true"))), Some(Change { key: "pt_spatial_nav".into(), value: "true".into() }));
        assert_eq!(map_command(&fields(Some("pt_capture_name={host} {date}"))).map(|c| c.value), Some("{host} {date}".into()));
        for bad in [None, Some("pt_spatial_nav"), Some("dd_max_results=3"), Some("=x")] {
            assert_eq!(map_command(&fields(bad)), None, "{bad:?}");
        }
        let mut extra = fields(Some("pt_spatial_nav=true"));
        extra.id = Some(1);
        assert_eq!(map_command(&extra), None);
        let mut other = fields(Some("pt_spatial_nav=true"));
        other.a = "addrset".into();
        assert_eq!(map_command(&other), None);
    }

    /// The page's test (ui/test/page-tools.mjs) parses this very file, so the two sides cannot drift.
    #[test]
    fn the_snapshot_is_the_fixture_the_page_is_tested_with() {
        let fixture = include_str!("../../ui/test/fixtures/page-tools-snapshot.json");
        assert_eq!(snapshot_json(&PageTools::default(), false, Ack::None, ""), fixture.trim());
    }

    /// The Notes panel's new commands reach the host with exactly the fields the page builds
    /// (`ui/src/panels/notes-logic.ts`): a note id is beyond u32, `before` carries an anchor note.
    #[test]
    fn the_notes_panel_commands_survive_the_wire() {
        use super::super::protocol::{parse_command, Command, PanelType};
        let ok = |json: &str| match parse_command(json) {
            Ok(Command::Panel { kind: PanelType::Notes, f }) => f,
            other => panic!("{json}: {other:?}"),
        };
        let moved = ok(r#"{"v":1,"t":"notes","a":"move-note","id":1760000000000001,"before":1760000000000002}"#);
        assert_eq!((moved.a.as_str(), moved.id, moved.before), ("move-note", Some(1_760_000_000_000_001), Some(1_760_000_000_000_002)));
        let filed = ok(r#"{"v":1,"t":"notes","a":"note-folder","id":1760000000000001,"parent":0}"#);
        assert_eq!((filed.parent, filed.id.is_some()), (Some(0), true));
        assert_eq!(ok(r#"{"v":1,"t":"notes","a":"sort","text":"title"}"#).text.as_deref(), Some("title"));
        let export = ok(r#"{"v":1,"t":"notes","a":"export","n":1,"id":1760000000000003,"text":"md"}"#);
        assert_eq!((export.n, export.text.as_deref()), (Some(1), Some("md")));
        assert_eq!(ok(r#"{"v":1,"t":"notes","a":"import"}"#).a, "import");
    }

    #[test]
    fn the_snapshot_carries_every_key_with_its_type() {
        let json = snapshot_json(&PageTools::default(), false, Ack::Saved, "bad\nvalue");
        assert!(json.starts_with(r#"{"v":1,"t":"snap","p":"pagetools","d":{"s":{"pt_spatial_nav":false,"pt_capture_dir":"","pt_capture_name":"{host} {date} {time}","pt_capture_format":"png","pt_zoom_scope":"site","pt_wheel_zoom":true,"pt_pip_menu":true,"pt_reader_font":"serif","pt_reader_size":19,"#), "{json}");
        assert!(json.ends_with(r#""guest":false,"ack":"saved","error":"bad value"}}"#), "{json}");
    }
}
