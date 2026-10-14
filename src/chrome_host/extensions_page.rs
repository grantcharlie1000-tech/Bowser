//! The Extensions section of the settings page and the toolbar's extensions button (docs/extensions.md
//! "Managing extensions").
//!
//! - `{t:"snap",p:"extensions",d:{..}}` lists the profile's installed extensions with the
//!   declarations read from Bowser's own copy of each manifest. The page shows them; nothing here
//!   grants, narrows or enforces anything.
//! - `{t:"set",a:"extension"|"extremove"|"extpopup"|"extinstall"|"extinstallcrx",..}` commands come back through the
//!   settings page's `set` family; [`map_command`] accepts each with exactly its own fields and a
//!   well-formed id. The id is looked up in `extensions.txt` again before anything acts on it.
//! - `{t:"extbtn",on,n,name}` tells the toolbar whether to show its extensions button.

use super::json::Json;
use super::protocol::{snap_json, PanelFields};
use crate::extensions::{self, Manifest};

/// Items of one declaration list sent to the page; the full count is sent beside them.
pub const MAX_LISTED: usize = 8;
const MAX_ITEM_CHARS: usize = 64;
const MAX_NAME_CHARS: usize = 128;
const MAX_VERSION_CHARS: usize = 24;
const MAX_FAILURE_CHARS: usize = 200;
const MAX_ERROR_CHARS: usize = 240;

/// One installed extension as the page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    /// The manifest names a popup page Bowser can open.
    pub popup: bool,
    /// Why it failed to load or change this session, or empty.
    pub failure: String,
    /// The manifest in Bowser's copy, or `None` when it could not be read.
    pub manifest: Option<Manifest>,
}

/// How Bowser answered the page's last extension command. Sent once, with the next snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ack {
    #[default]
    None,
    Enabled,
    Disabled,
    Removed,
    Installed,
    Failed,
}

impl Ack {
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Removed => "removed",
            Self::Installed => "installed",
            Self::Failed => "failed",
        }
    }
}

/// What the snapshot reports besides the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status<'a> {
    /// `extensions.txt` could be read. When false the list is empty and `error` says why.
    pub readable: bool,
    /// The extension being removed (removal finishes later, in the engine), or empty.
    pub busy: &'a str,
    pub ack: Ack,
    pub error: &'a str,
}

/// `{v:1,t:"snap",p:"extensions",d:{state,busy,ack,error,items}}`. Every item key is always present.
pub fn snapshot_json(status: Status<'_>, entries: &[Entry]) -> String {
    snap_json("extensions", |out| {
        out.string("state", if status.readable { "on" } else { "error" });
        out.string("busy", if extensions::is_valid_id(status.busy) { status.busy } else { "" });
        out.string("ack", status.ack.key());
        out.string("error", plain(status.error, MAX_ERROR_CHARS).trim());
        out.array("items", |items| {
            for entry in entries.iter().take(extensions::MAX_INSTALLED) {
                items.object(|item| write_entry(item, entry));
            }
        });
    })
    .expect("extensions is a panel token")
}

/// `text` cut to `max` characters with control characters (an engine message's line breaks) as
/// spaces: the page draws it as one line of text and refuses a snapshot that carries any.
fn plain(text: &str, max: usize) -> String {
    text.chars().take(max).map(|c| if c.is_control() { ' ' } else { c }).collect()
}

fn write_entry(item: &mut super::json::Object<'_>, entry: &Entry) {
    let empty = Vec::new();
    let manifest = entry.manifest.as_ref();
    let list = |pick: fn(&Manifest) -> &Vec<String>| manifest.map_or(&empty, pick);
    item.string("id", &entry.id);
    item.string("name", &plain(&entry.name, MAX_NAME_CHARS));
    item.string("version", &plain(&entry.version, MAX_VERSION_CHARS));
    item.bool("enabled", entry.enabled);
    item.bool("popup", entry.popup);
    item.string("failure", plain(&entry.failure, MAX_FAILURE_CHARS).trim());
    item.bool("readable", manifest.is_some());
    item.bool("all", manifest.is_some_and(extensions::reaches_every_site));
    write_list(item, "sites", list(|m| &m.host_permissions).iter());
    write_list(item, "scripts", list(|m| &m.content_script_matches).iter());
    write_list(item, "perms", list(|m| &m.permissions).iter());
    let optional = list(|m| &m.optional_host_permissions).iter().chain(list(|m| &m.optional_permissions).iter());
    write_list(item, "optional", optional);
}

/// `key` with at most [`MAX_LISTED`] items, each cut to a fixed length, and `keyN` with the full
/// count, so a padded manifest cannot make the snapshot large or hide how much it declares.
fn write_list<'a>(item: &mut super::json::Object<'_>, key: &str, values: impl Iterator<Item = &'a String> + Clone) {
    let total = values.clone().count();
    item.array(key, |out| {
        for value in values.take(MAX_LISTED) {
            out.string(&plain(value, MAX_ITEM_CHARS));
        }
    });
    item.uint(&format!("{key}N"), total as u64);
}

/// A validated extension command from the settings page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Turn an installed extension on or off. Its copied files and declarations do not change.
    SetEnabled { id: String, on: bool },
    /// Remove it (the page has already asked). Unloads it, then deletes Bowser's copy.
    Remove(String),
    /// Open its popup in Bowser's extension page window.
    Popup(String),
    /// Install an unpacked folder: Bowser's folder picker and its native permission confirmation.
    Install,
    /// Install a `.crx` file on disk: Bowser's file picker, verification, then the same confirmation.
    InstallCrx,
}

/// Maps the extension actions of the `set` family; anything else (including every settings action)
/// is `None`.
pub fn map_command(f: &PanelFields) -> Option<Action> {
    if f.id.is_some() || f.parent.is_some() || f.before.is_some() || f.n.is_some() || f.url.is_some() {
        return None;
    }
    let id = || f.text.as_deref().filter(|id| extensions::is_valid_id(id)).map(str::to_owned);
    Some(match (f.a.as_str(), f.on) {
        ("extension", Some(on)) => Action::SetEnabled { id: id()?, on },
        ("extremove", None) => Action::Remove(id()?),
        ("extpopup", None) => Action::Popup(id()?),
        ("extinstall", None) if f.text.is_none() => Action::Install,
        ("extinstallcrx", None) if f.text.is_none() => Action::InstallCrx,
        _ => return None,
    })
}

/// Why the page's popup command is refused, in the page's own words, or `None` when it may open.
/// A popup opens only for an extension that is on and loaded this session (`failure` is empty).
pub fn popup_refusal(enabled: bool, failure: &str) -> Option<&'static str> {
    if !enabled {
        Some("Turn the extension on to open its popup.")
    } else if !failure.is_empty() {
        Some("This extension is not running. Its popup cannot open.")
    } else {
        None
    }
}

/// `{v:1,t:"extbtn",on,n,name}`: show the toolbar's extensions button while `popups` (enabled
/// extensions with a popup) is not empty; `name` is the only one's name, for its tooltip.
pub fn button_json(popups: &[&str]) -> String {
    Json::object(|o| {
        o.int("v", 1);
        o.string("t", "extbtn");
        o.bool("on", !popups.is_empty());
        o.uint("n", popups.len().min(extensions::MAX_INSTALLED) as u64);
        o.string("name", &if popups.len() == 1 { plain(popups[0], MAX_NAME_CHARS) } else { String::new() });
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(a: &str) -> PanelFields {
        PanelFields { a: a.to_owned(), ..PanelFields::default() }
    }

    const ID: &str = "ext-0123456789abcdef";

    fn manifest() -> Manifest {
        Manifest {
            name: "Reader \"<b>\"".into(),
            version: "1.2.3".into(),
            manifest_version: 3,
            permissions: vec!["storage".into(), "tabs".into()],
            host_permissions: vec!["https://example.com/*".into()],
            content_script_matches: vec!["<all_urls>".into()],
            optional_permissions: vec!["downloads".into()],
            optional_host_permissions: vec!["https://news.example/*".into()],
        }
    }

    fn entry() -> Entry {
        Entry {
            id: ID.into(),
            name: "Reader \"<b>\"".into(),
            version: "1.2.3".into(),
            enabled: true,
            popup: true,
            failure: String::new(),
            manifest: Some(manifest()),
        }
    }

    #[test]
    fn commands_carry_exactly_their_own_fields_and_a_well_formed_id() {
        let mut toggle = fields("extension");
        toggle.text = Some(ID.into());
        toggle.on = Some(false);
        assert_eq!(map_command(&toggle), Some(Action::SetEnabled { id: ID.into(), on: false }));
        toggle.on = None;
        assert_eq!(map_command(&toggle), None, "extension needs on");
        for (name, expect) in [("extremove", Action::Remove(ID.into())), ("extpopup", Action::Popup(ID.into()))] {
            let mut command = fields(name);
            assert_eq!(map_command(&command), None, "{name} needs an id");
            command.text = Some(ID.into());
            assert_eq!(map_command(&command), Some(expect), "{name}");
            command.on = Some(true);
            assert_eq!(map_command(&command), None, "{name} with on");
        }
        assert_eq!(map_command(&fields("extinstall")), Some(Action::Install));
        assert_eq!(map_command(&fields("extinstallcrx")), Some(Action::InstallCrx));
        let mut crx_path = fields("extinstallcrx");
        crx_path.text = Some("C:\\Users\\me\\a.crx".into());
        assert_eq!(map_command(&crx_path), None, "the page never names the file to install");
        let mut crx_on = fields("extinstallcrx");
        crx_on.on = Some(true);
        assert_eq!(map_command(&crx_on), None);
        for bad in ["", "ext-0123", "ext-0123456789ABCDEF", "..\\ext-0123456789abcdef", "ext-0123456789abcdeg", "C:\\Extensions"] {
            let mut command = fields("extremove");
            command.text = Some(bad.into());
            assert_eq!(map_command(&command), None, "{bad:?}");
        }
        let mut install = fields("extinstall");
        install.text = Some("C:\\folder".into());
        assert_eq!(map_command(&install), None, "install never names a path");
        let mut with_id = fields("extremove");
        with_id.text = Some(ID.into());
        with_id.id = Some(1);
        assert_eq!(map_command(&with_id), None);
        let mut with_url = fields("extpopup");
        with_url.text = Some(ID.into());
        with_url.url = Some("chrome-extension://abc/popup.html".into());
        assert_eq!(map_command(&with_url), None, "the page never names a page address");
        for settings in ["save", "refresh", "section", "extensions", "extgrant", "extenable"] {
            assert_eq!(map_command(&fields(settings)), None, "{settings}");
        }
    }

    #[test]
    fn the_snapshot_lists_capped_declarations_with_their_full_counts() {
        let json = snapshot_json(Status { readable: true, busy: "", ack: Ack::Removed, error: "" }, &[entry()]);
        assert!(json.starts_with(r#"{"v":1,"t":"snap","p":"extensions","d":{"state":"on","busy":"","ack":"removed","error":"","items":[{"id":"ext-0123456789abcdef","#), "{json}");
        assert!(json.contains(r#""all":true,"sites":["https://example.com/*"],"sitesN":1,"scripts":["\u003call_urls>"],"scriptsN":1,"perms":["storage","tabs"],"permsN":2,"optional":["https://news.example/*","downloads"],"optionalN":2}"#), "{json}");

        let mut padded = entry();
        let manifest = padded.manifest.as_mut().expect("manifest");
        manifest.permissions = (0..40).map(|i| format!("{}{i}", "p".repeat(100))).collect();
        manifest.content_script_matches.clear();
        let json = snapshot_json(Status { readable: true, busy: ID, ack: Ack::None, error: "" }, &[padded]);
        assert!(json.contains(r#""permsN":40"#), "{json}");
        assert!(json.contains(&format!("\"{}\"", "p".repeat(MAX_ITEM_CHARS))), "items are cut");
        assert!(!json.contains(&"p".repeat(MAX_ITEM_CHARS + 1)), "items are cut");
        assert_eq!(json.matches(&"p".repeat(MAX_ITEM_CHARS)).count(), MAX_LISTED);
        assert!(json.contains(r#""busy":"ext-0123456789abcdef""#));
        assert!(json.contains(r#""all":false"#), "no broad pattern left");

        let unreadable = Entry { manifest: None, failure: "Load\tfailed\r\n".into(), ..entry() };
        let json = snapshot_json(Status { readable: true, busy: "..\\x", ack: Ack::None, error: "" }, &[unreadable]);
        assert!(json.contains(r#""busy":"""#), "a malformed busy id is never echoed");
        assert!(json.contains(r#""failure":"Load failed","readable":false,"all":false,"sites":[],"sitesN":0"#), "control characters become spaces: {json}");

        let error = snapshot_json(Status { readable: false, busy: "", ack: Ack::Failed, error: "The installed extension list is damaged." }, &[]);
        assert_eq!(error, r#"{"v":1,"t":"snap","p":"extensions","d":{"state":"error","busy":"","ack":"failed","error":"The installed extension list is damaged.","items":[]}}"#);
    }

    #[test]
    fn a_popup_opens_only_for_an_enabled_extension_that_loaded() {
        assert_eq!(popup_refusal(true, ""), None);
        assert_eq!(popup_refusal(false, ""), Some("Turn the extension on to open its popup."));
        assert_eq!(popup_refusal(false, "Load failed"), Some("Turn the extension on to open its popup."));
        assert_eq!(popup_refusal(true, "Load failed"), Some("This extension is not running. Its popup cannot open."));
    }

    #[test]
    fn the_button_shows_only_while_an_enabled_extension_has_a_popup() {
        assert_eq!(button_json(&[]), r#"{"v":1,"t":"extbtn","on":false,"n":0,"name":""}"#);
        assert_eq!(button_json(&["Reader"]), r#"{"v":1,"t":"extbtn","on":true,"n":1,"name":"Reader"}"#);
        assert_eq!(button_json(&["Reader", "Notes"]), r#"{"v":1,"t":"extbtn","on":true,"n":2,"name":""}"#);
    }

    #[test]
    fn extension_messages_match_committed_fixture() {
        let other = Entry {
            id: "ext-fedcba9876543210".into(),
            name: "Tab Notes".into(),
            version: "0.4".into(),
            enabled: false,
            popup: false,
            failure: "Load failed: 0x80070002".into(),
            manifest: Some(Manifest {
                name: "Tab Notes".into(),
                version: "0.4".into(),
                manifest_version: 3,
                permissions: vec!["storage".into()],
                host_permissions: Vec::new(),
                content_script_matches: Vec::new(),
                optional_permissions: Vec::new(),
                optional_host_permissions: Vec::new(),
            }),
        };
        let snap = snapshot_json(Status { readable: true, busy: "", ack: Ack::None, error: "" }, &[entry(), other]);
        let busy = snapshot_json(Status { readable: true, busy: ID, ack: Ack::None, error: "" }, &[entry()]);
        let empty = snapshot_json(Status { readable: true, busy: "", ack: Ack::Removed, error: "" }, &[]);
        let error = snapshot_json(Status { readable: false, busy: "", ack: Ack::None, error: "The installed extension list is damaged." }, &[]);
        let content = format!(
            "{{\"snap\":{snap},\"busy\":{busy},\"empty\":{empty},\"error\":{error},\"buttonOff\":{},\"buttonOne\":{},\"buttonMany\":{}}}\n",
            button_json(&[]),
            button_json(&["Reader \"<b>\""]),
            button_json(&["Reader", "Tab Notes"]),
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/extensions-messages.json");
        if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some_and(|value| value == "1") {
            std::fs::write(&path, &content).expect("write extensions messages fixture");
        }
        assert_eq!(
            std::fs::read_to_string(&path).expect("committed extensions messages fixture"),
            content,
            "set BOWSER_WRITE_FIXTURES=1 cargo test extension_messages_match_committed_fixture --lib"
        );
    }
}
