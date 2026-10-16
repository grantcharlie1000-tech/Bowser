//! The wire of the address bar, search and bookmark options (the settings page's "Address bar",
//! "Search engine" and "Bookmarks" sections, and the toolbar's address field).
//!
//! - `{t:"snap",p:"address",d:{..}}` carries every option, the engine list and the bookmark folders to
//!   the settings page.
//! - `{t:"set",a:"addrset"|"engine",..}` come back through the settings page's `set` family;
//!   [`map_command`] accepts each with exactly its own fields and checks the key and the value again
//!   in [`crate::address_settings::AddressSettings::set`] and [`crate::search_engines::EngineStore`].
//! - `{t:"addr",..}` and `{t:"sfield",..}` tell the toolbar how the address field and the search field
//!   behave and look. [`toolbar_command`] maps what the toolbar sends back (`{t:"addr",a:..}`).

use super::json::Json;
use super::protocol::{snap_json, PanelFields};
use crate::address_settings::{AddressSettings, Kind, KEYS};
use crate::search_engines::{Engine, EngineStore};

const MAX_ERROR_CHARS: usize = 240;
/// Folders listed for the bookmarks bar's folder choice.
pub const MAX_FOLDERS: usize = 200;

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

/// A bookmark folder as the folder choice lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub id: u64,
    pub title: String,
    pub depth: u32,
}

/// Everything the snapshot reports.
pub struct Snapshot<'a> {
    pub settings: &'a AddressSettings,
    pub engines: &'a EngineStore,
    pub folders: &'a [Folder],
    /// Guest profiles keep nothing: the page says so and disables what would be saved.
    pub guest: bool,
    pub ack: Ack,
    pub error: &'a str,
}

fn plain(text: &str, max: usize) -> String {
    text.chars().take(max).map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// `{v:1,t:"snap",p:"address",d:{s:{<key>:<value>},engines:[..],def,priv,img,folders:[..],guest,ack,error}}`.
pub fn snapshot_json(s: &Snapshot<'_>) -> String {
    snap_json("address", |out| {
        out.object("s", |o| {
            for (key, kind) in KEYS {
                let Some(value) = s.settings.get(key) else { continue };
                match kind {
                    Kind::Bool => o.bool(key, value == "true"),
                    Kind::Int => o.uint(key, value.parse().unwrap_or(0)),
                    Kind::Text => o.string(key, &value),
                }
            }
        });
        out.array("engines", |list| {
            for engine in s.engines.engines() {
                list.object(|e| {
                    e.string("id", &engine.id);
                    e.string("name", &engine.name);
                    e.string("nick", &engine.nickname);
                    e.string("url", &engine.url);
                    e.string("post", &engine.post);
                    e.string("suggest", &engine.suggest);
                    e.string("image", &engine.image);
                    e.string("imagePost", &engine.image_post);
                    e.bool("builtin", crate::search_engines::builtin_engines().iter().any(|b| b.id == engine.id));
                });
            }
        });
        out.string("def", s.engines.default_id());
        out.string("priv", s.engines.private_id());
        out.string("img", s.engines.image_id());
        out.array("folders", |list| {
            for folder in s.folders.iter().take(MAX_FOLDERS) {
                list.object(|f| {
                    f.uint("id", folder.id);
                    f.string("title", &plain(&folder.title, 80));
                    f.uint("depth", u64::from(folder.depth.min(16)));
                });
            }
        });
        out.bool("guest", s.guest);
        out.string("ack", s.ack.key());
        out.string("error", plain(s.error, MAX_ERROR_CHARS).trim());
    })
    .expect("address is a panel token")
}

/// A validated change to the engine list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineOp {
    Add(Engine),
    Update(String, Engine),
    Remove(String),
    Move(String, usize),
    Default(String),
    /// An empty id makes private windows use the default engine.
    Private(String),
    Image(String),
    Restore,
}

/// A validated command from the settings page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// One option, as `key` and its value. The key must be one of [`KEYS`].
    Set { key: String, value: String },
    Engine(EngineOp),
}

fn engine_from(fields: &[&str]) -> Option<Engine> {
    let [name, nickname, url, post, suggest, image, image_post] = fields else { return None };
    Some(Engine {
        id: String::new(),
        name: (*name).to_owned(),
        nickname: (*nickname).to_owned(),
        url: (*url).to_owned(),
        post: (*post).to_owned(),
        suggest: (*suggest).to_owned(),
        image: (*image).to_owned(),
        image_post: (*image_post).to_owned(),
    })
}

fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Parses an engine command's text: the operation, then tab-separated fields.
pub fn parse_engine_op(text: &str) -> Option<EngineOp> {
    let parts: Vec<&str> = text.split('\t').collect();
    Some(match parts.as_slice() {
        ["add", rest @ ..] if rest.len() == 7 => EngineOp::Add(engine_from(rest)?),
        ["update", id, rest @ ..] if rest.len() == 7 && id_ok(id) => EngineOp::Update((*id).to_owned(), engine_from(rest)?),
        ["remove", id] if id_ok(id) => EngineOp::Remove((*id).to_owned()),
        ["move", id, index] if id_ok(id) => EngineOp::Move((*id).to_owned(), index.parse::<usize>().ok().filter(|index| *index < 100)?),
        ["default", id] if id_ok(id) => EngineOp::Default((*id).to_owned()),
        ["private", id] if id.is_empty() || id_ok(id) => EngineOp::Private((*id).to_owned()),
        ["image", id] if id_ok(id) => EngineOp::Image((*id).to_owned()),
        ["restore"] => EngineOp::Restore,
        _ => return None,
    })
}

/// Maps `{t:"set",a:"addrset"|"engine",text}`; anything else is `None`.
pub fn map_command(f: &PanelFields) -> Option<Action> {
    if f.id.is_some() || f.parent.is_some() || f.before.is_some() || f.n.is_some() || f.url.is_some() || f.on.is_some() {
        return None;
    }
    let text = f.text.as_deref()?;
    match f.a.as_str() {
        "addrset" => {
            let (key, value) = text.split_once('=')?;
            AddressSettings::owns(key).then(|| Action::Set { key: key.to_owned(), value: value.to_owned() })
        }
        "engine" => parse_engine_op(text).map(Action::Engine),
        _ => None,
    }
}

/// `{v:1,t:"addr",..}`: the address field's options (`base` is the shown page's registrable domain).
pub fn addr_json(settings: &AddressSettings, base: &str) -> String {
    Json::object(|o| {
        o.int("v", 1);
        o.string("t", "addr");
        o.bool("show", settings.bar_show);
        o.bool("full", settings.show_full);
        o.bool("select", settings.select_on_activate);
        o.bool("encoded", settings.copy_encoded);
        o.bool("qr", settings.qr_button);
        o.bool("hbtn", settings.history_button);
        o.bool("typed", settings.typed_history);
        o.bool("typing", settings.dropdown_while_typing);
        o.bool("badge", settings.always_badge);
        o.bool("strip", settings.strip_javascript);
        o.bool("highlight", settings.highlight_base);
        o.string("base", &base.chars().take(253).collect::<String>());
    })
}

/// Most engines the toolbar's search field lists.
pub const MAX_FIELD_ENGINES: usize = 40;

/// `{v:1,t:"sfield",mode,icon,nick,suggest,keep,engine,engines:[{n,k}]}`: the search field's mode and
/// engine list. `engine` is the index of the chosen engine in `engines`.
pub fn sfield_json(settings: &AddressSettings, engines: &EngineStore, chosen: usize) -> String {
    Json::object(|o| {
        o.int("v", 1);
        o.string("t", "sfield");
        o.string("mode", settings.search_field.key());
        o.bool("icon", settings.show_engine_icon);
        o.bool("nick", settings.show_nickname);
        o.bool("suggest", settings.suggest_in_field);
        o.bool("keep", settings.keep_text);
        let list = engines.engines();
        o.uint("engine", chosen.min(list.len().saturating_sub(1)) as u64);
        o.array("engines", |items| {
            for engine in list.iter().take(MAX_FIELD_ENGINES) {
                items.object(|e| {
                    e.string("n", &plain(&engine.name, 40));
                    e.string("k", &engine.nickname);
                });
            }
        });
    })
}

/// What the toolbar asks of Bowser through `{t:"addr",a:..}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolbarAction {
    /// Enter in the address field. `how`: 0 plain, 1 new tab (Alt/Shift), 2 domain expansion (Ctrl).
    Submit { text: String, how: u8 },
    /// Show the typed-history list for edit `seq`.
    Typed { seq: u32 },
    /// Show the drop-down for `text` although typing does not open it (Down with the list closed).
    Suggest { text: String, seq: u32 },
    /// Show the QR code of the shown page.
    Qr,
    /// Enter in the search field: search `text` with engine `engine` (an index into the `sfield`
    /// list), in a new tab when `new_tab`.
    Search { text: String, engine: usize, new_tab: bool },
    /// The search field's text changed: engine suggestions for `text` with engine `engine`.
    SearchEdit { text: String, engine: usize, seq: u32 },
    /// Open the engine list under the search field button at `rect` (toolbar CSS pixels).
    Engines { rect: super::protocol::HoverRect },
}

/// Maps the `addr` family of the toolbar; exactly each action's fields, nothing else.
pub fn toolbar_command(f: &PanelFields) -> Option<ToolbarAction> {
    if f.parent.is_some() || f.before.is_some() || f.on.is_some() || f.url.is_some() {
        return None;
    }
    let seq = || f.n.and_then(|n| u32::try_from(n).ok());
    let index = |n: Option<u64>| n.and_then(|n| usize::try_from(n).ok()).filter(|n| *n < MAX_FIELD_ENGINES);
    match f.a.as_str() {
        "submit" if f.id.is_none() => Some(ToolbarAction::Submit { text: f.text.clone()?, how: u8::try_from(f.n?).ok().filter(|how| *how <= 2)? }),
        "typed" if f.id.is_none() && f.text.is_none() => Some(ToolbarAction::Typed { seq: seq()? }),
        "suggest" if f.id.is_none() => Some(ToolbarAction::Suggest { text: f.text.clone()?, seq: seq()? }),
        "qr" if f.id.is_none() && f.n.is_none() && f.text.is_none() => Some(ToolbarAction::Qr),
        "search" => Some(ToolbarAction::Search { text: f.text.clone()?, engine: index(f.id)?, new_tab: match f.n? { 0 => false, 1 => true, _ => return None } }),
        "sedit" => Some(ToolbarAction::SearchEdit { text: f.text.clone()?, engine: index(f.id)?, seq: seq()? }),
        "engines" if f.id.is_none() && f.n.is_none() => super::protocol::HoverRect::parse(f.text.as_deref()?).map(|rect| ToolbarAction::Engines { rect }),
        _ => None,
    }
}

/// What the address dialog shows.
pub enum DialogPage<'a> {
    /// The QR code of `text` (a page address).
    Qr { text: &'a str },
    Bookmark(BookmarkDialog<'a>),
}

/// The bookmark dialog's fields.
pub struct BookmarkDialog<'a> {
    pub url: &'a str,
    /// The page is bookmarked already: the dialog edits it and offers Remove.
    pub existing: bool,
    pub name: &'a str,
    /// Id of the folder, `0` for the top level.
    pub folder: u64,
    pub nickname: &'a str,
    pub description: &'a str,
    pub folders: &'a [Folder],
    pub error: &'a str,
}

/// `{v:1,t:"snap",p:"addressdlg",d:{kind:"qr",text}}` or the bookmark dialog's fields.
pub fn dialog_snapshot_json(page: &DialogPage<'_>) -> String {
    snap_json("addressdlg", |out| match page {
        DialogPage::Qr { text } => {
            out.string("kind", "qr");
            out.string("text", &text.chars().take(2048).collect::<String>());
        }
        DialogPage::Bookmark(b) => {
            out.string("kind", "bookmark");
            out.string("url", &b.url.chars().take(2048).collect::<String>());
            out.bool("existing", b.existing);
            out.string("name", &plain(b.name, 200));
            out.uint("folder", b.folder);
            out.string("nickname", &plain(b.nickname, 24));
            out.string("description", &plain(b.description, 512));
            out.array("folders", |list| {
                for folder in b.folders.iter().take(MAX_FOLDERS) {
                    list.object(|f| {
                        f.uint("id", folder.id);
                        f.string("title", &plain(&folder.title, 80));
                        f.uint("depth", u64::from(folder.depth.min(16)));
                    });
                }
            });
            out.string("error", plain(b.error, MAX_ERROR_CHARS).trim());
        }
    })
    .expect("addressdlg is a panel token")
}

/// What the QR code and bookmark dialogs ask of Bowser through `{t:"addr",a:..}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogAction {
    /// Copy the address the QR code stands for.
    QrCopy,
    /// Save the bookmark with these fields (`folder` 0 is the top level).
    BookmarkSave { name: String, nickname: String, description: String, folder: u64 },
    BookmarkRemove,
}

/// Maps the `addr` family of the address dialogs; exactly each action's fields, nothing else.
pub fn dialog_command(f: &PanelFields) -> Option<DialogAction> {
    if f.id.is_some() || f.before.is_some() || f.n.is_some() || f.on.is_some() || f.url.is_some() {
        return None;
    }
    match f.a.as_str() {
        "qrcopy" if f.text.is_none() && f.parent.is_none() => Some(DialogAction::QrCopy),
        "bmremove" if f.text.is_none() && f.parent.is_none() => Some(DialogAction::BookmarkRemove),
        "bmsave" => {
            let text = f.text.as_deref()?;
            let mut parts = text.split('\t');
            let (name, nickname, description) = (parts.next()?, parts.next()?, parts.next()?);
            if parts.next().is_some() || name.trim().is_empty() {
                return None;
            }
            Some(DialogAction::BookmarkSave {
                name: name.trim().to_owned(),
                nickname: nickname.to_owned(),
                description: description.to_owned(),
                folder: f.parent.unwrap_or(0),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes the cross-language fixture read by `ui/test/address.mjs`.
    #[test]
    fn address_messages_write_the_ui_fixture() {
        let mut settings = AddressSettings::default();
        settings.set("dd_sources", "bookmarks,history");
        settings.set("search_field", "field");
        let mut engines = EngineStore::default();
        engines.set_private("duckduckgo").unwrap();
        let folders = vec![Folder { id: 4, title: "Reading".into(), depth: 0 }, Folder { id: 9, title: "Rust".into(), depth: 1 }];
        let snapshot = snapshot_json(&Snapshot { settings: &settings, engines: &engines, folders: &folders, guest: false, ack: Ack::None, error: "" });
        let bookmark = dialog_snapshot_json(&DialogPage::Bookmark(BookmarkDialog {
            url: "https://www.rust-lang.org/",
            existing: true,
            name: "Rust",
            folder: 9,
            nickname: "rl",
            description: "A language",
            folders: &folders,
            error: "",
        }));
        let fixture = format!(
            "{{\"addr\":{},\"sfield\":{},\"snap\":{},\"qr\":{},\"bookmark\":{}}}\n",
            addr_json(&settings, "example.co.uk"),
            sfield_json(&settings, &engines, 1),
            snapshot,
            dialog_snapshot_json(&DialogPage::Qr { text: "https://example.com/a b" }),
            bookmark,
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/address-messages.json");
        if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some() {
            std::fs::write(&path, &fixture).expect("write address fixture");
        }
        assert_eq!(
            std::fs::read_to_string(&path).expect("address fixture").replace("\r\n", "\n"),
            fixture,
            "set BOWSER_WRITE_FIXTURES=1 cargo test address_messages_write_the_ui_fixture --lib"
        );
    }

    #[test]
    fn dialog_commands_carry_exactly_their_own_fields() {
        let plain = |a: &str| PanelFields { a: a.to_owned(), ..PanelFields::default() };
        assert_eq!(dialog_command(&plain("qrcopy")), Some(DialogAction::QrCopy));
        assert_eq!(dialog_command(&plain("bmremove")), Some(DialogAction::BookmarkRemove));
        let save = PanelFields { text: Some("Rust\trl\tThe language".into()), parent: Some(4), ..plain("bmsave") };
        assert_eq!(
            dialog_command(&save),
            Some(DialogAction::BookmarkSave { name: "Rust".into(), nickname: "rl".into(), description: "The language".into(), folder: 4 })
        );
        let top = PanelFields { parent: None, ..save.clone() };
        assert!(matches!(dialog_command(&top), Some(DialogAction::BookmarkSave { folder: 0, .. })));
        for bad in ["Rust\trl", "Rust\trl\td\textra", "\trl\td", "  \t\t"] {
            assert_eq!(dialog_command(&PanelFields { text: Some(bad.into()), ..save.clone() }), None, "{bad:?}");
        }
        assert_eq!(dialog_command(&PanelFields { text: Some("x".into()), ..plain("qrcopy") }), None);
        assert_eq!(dialog_command(&PanelFields { n: Some(1), ..save.clone() }), None);
        assert_eq!(dialog_command(&plain("explode")), None);
    }

    fn fields(a: &str) -> PanelFields {
        PanelFields { a: a.to_owned(), ..PanelFields::default() }
    }

    fn text(a: &str, text: &str) -> PanelFields {
        PanelFields { text: Some(text.to_owned()), ..fields(a) }
    }

    #[test]
    fn the_snapshot_carries_every_option_every_engine_and_the_folders() {
        let settings = AddressSettings::default();
        let engines = EngineStore::default();
        let folders = vec![Folder { id: 4, title: "Read\u{7}ing".into(), depth: 1 }];
        let json = snapshot_json(&Snapshot { settings: &settings, engines: &engines, folders: &folders, guest: false, ack: Ack::Saved, error: "a\nb" });
        assert!(json.starts_with(r#"{"v":1,"t":"snap","p":"address","d":{"s":{"address_bar_show":true,"#), "{json}");
        for (key, _) in KEYS {
            assert!(json.contains(&format!("\"{key}\":")), "{key}");
        }
        assert!(json.contains(r#""dd_max_results":8"#));
        assert!(json.contains(r#""dd_sources":"direct,bookmarks,nicknames,search,search-history,history,calculator""#));
        assert!(json.contains(r#""id":"google","name":"Google","nick":"g""#));
        assert!(json.contains(r#""builtin":true"#));
        assert!(json.contains(r#""def":"optimised","priv":"","img":"google""#));
        assert!(json.contains(r#""folders":[{"id":4,"title":"Read ing","depth":1}]"#), "{json}");
        assert!(json.contains(r#""ack":"saved","error":"a b""#));
    }

    #[test]
    fn address_settings_commands_name_a_known_key() {
        assert_eq!(map_command(&text("addrset", "dd_max_results=12")), Some(Action::Set { key: "dd_max_results".into(), value: "12".into() }));
        assert_eq!(map_command(&text("addrset", "tab_layout=vertical")), None, "only this module's keys");
        assert_eq!(map_command(&text("addrset", "no equals")), None);
        assert_eq!(map_command(&fields("addrset")), None);
        let mut with_on = text("addrset", "dd_max_results=12");
        with_on.on = Some(true);
        assert_eq!(map_command(&with_on), None);
        let mut with_id = text("addrset", "dd_max_results=12");
        with_id.id = Some(1);
        assert_eq!(map_command(&with_id), None);
        assert_eq!(map_command(&text("engine", "restore")), Some(Action::Engine(EngineOp::Restore)));
    }

    #[test]
    fn engine_commands_parse_with_exactly_their_fields() {
        assert_eq!(
            parse_engine_op("add\tMine\tmn\thttps://m.example/?q=%s\t\thttps://m.example/s?q=%s\t\t"),
            Some(EngineOp::Add(Engine {
                name: "Mine".into(),
                nickname: "mn".into(),
                url: "https://m.example/?q=%s".into(),
                suggest: "https://m.example/s?q=%s".into(),
                ..Engine::default()
            }))
        );
        assert_eq!(parse_engine_op("add\tonly\tthree"), None);
        assert!(matches!(parse_engine_op("update\tgoogle\tG\tg\thttps://g.example/?q=%s\t\t\t\t"), Some(EngineOp::Update(id, _)) if id == "google"));
        assert_eq!(parse_engine_op("update\tBad Id\tG\tg\thttps://g.example/?q=%s\t\t\t\t"), None);
        assert_eq!(parse_engine_op("remove\tbing"), Some(EngineOp::Remove("bing".into())));
        assert_eq!(parse_engine_op("remove\t"), None);
        assert_eq!(parse_engine_op("move\tbing\t2"), Some(EngineOp::Move("bing".into(), 2)));
        assert_eq!(parse_engine_op("move\tbing\t-1"), None);
        assert_eq!(parse_engine_op("move\tbing\t100"), None);
        assert_eq!(parse_engine_op("default\tbing"), Some(EngineOp::Default("bing".into())));
        assert_eq!(parse_engine_op("private\t"), Some(EngineOp::Private(String::new())));
        assert_eq!(parse_engine_op("private\tbing"), Some(EngineOp::Private("bing".into())));
        assert_eq!(parse_engine_op("image\t"), None);
        assert_eq!(parse_engine_op("restore\textra"), None);
        assert_eq!(parse_engine_op("explode"), None);
    }

    #[test]
    fn toolbar_flags_and_search_field_are_flat_objects() {
        let mut settings = AddressSettings::default();
        settings.set("address_show_full", "true");
        let json = addr_json(&settings, "example.co.uk");
        assert_eq!(
            json,
            r#"{"v":1,"t":"addr","show":true,"full":true,"select":true,"encoded":true,"qr":false,"hbtn":false,"typed":false,"typing":true,"badge":false,"strip":true,"highlight":false,"base":"example.co.uk"}"#
        );
        settings.set("search_field", "button");
        let json = sfield_json(&settings, &EngineStore::default(), 99);
        assert!(json.starts_with(r#"{"v":1,"t":"sfield","mode":"button","icon":true,"nick":true,"suggest":true,"keep":false,"engine":6,"engines":[{"n":"Optimised","k":"opt"},"#), "{json}");
    }

    #[test]
    fn toolbar_commands_carry_exactly_their_own_fields() {
        let with = |a: &str, text: Option<&str>, n: Option<i32>, id: Option<u64>| PanelFields { a: a.into(), text: text.map(str::to_owned), n, id, ..PanelFields::default() };
        assert_eq!(toolbar_command(&with("submit", Some("rust"), Some(1), None)), Some(ToolbarAction::Submit { text: "rust".into(), how: 1 }));
        assert_eq!(toolbar_command(&with("submit", Some("rust"), Some(3), None)), None);
        assert_eq!(toolbar_command(&with("submit", Some("rust"), None, None)), None);
        assert_eq!(toolbar_command(&with("typed", None, Some(5), None)), Some(ToolbarAction::Typed { seq: 5 }));
        assert_eq!(toolbar_command(&with("typed", None, Some(-1), None)), None);
        assert_eq!(toolbar_command(&with("suggest", Some("a"), Some(2), None)), Some(ToolbarAction::Suggest { text: "a".into(), seq: 2 }));
        assert_eq!(toolbar_command(&with("qr", None, None, None)), Some(ToolbarAction::Qr));
        assert_eq!(toolbar_command(&with("qr", Some("x"), None, None)), None);
        assert_eq!(toolbar_command(&with("search", Some("q"), Some(1), Some(2))), Some(ToolbarAction::Search { text: "q".into(), engine: 2, new_tab: true }));
        assert_eq!(toolbar_command(&with("search", Some("q"), Some(2), Some(2))), None);
        assert_eq!(toolbar_command(&with("search", Some("q"), Some(0), Some(40))), None, "an engine index past the list");
        assert_eq!(toolbar_command(&with("sedit", Some("q"), Some(9), Some(0))), Some(ToolbarAction::SearchEdit { text: "q".into(), engine: 0, seq: 9 }));
        assert!(matches!(toolbar_command(&with("engines", Some("1,2,3,4"), None, None)), Some(ToolbarAction::Engines { .. })));
        assert_eq!(toolbar_command(&with("engines", Some("nope"), None, None)), None);
        assert_eq!(toolbar_command(&with("explode", None, None, None)), None);
        let mut with_on = with("qr", None, None, None);
        with_on.on = Some(true);
        assert_eq!(toolbar_command(&with_on), None);
    }
}
