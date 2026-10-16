//! The settings page's Keyboard and Quick Commands sections (`docs/commands.md`).
//!
//! - `{t:"snap",p:"commands",d:{..}}` carries every command with its shortcuts, the keyboard and
//!   Quick Commands options, the sites that prioritise their own shortcuts and the command chains.
//! - `{t:"set",a:"kb…"|"qc…"|"chain…",..}` commands come back through the settings page's `set`
//!   family; [`map_command`] accepts each with exactly its own fields and [`apply`] changes a copy of
//!   the store, which the caller saves and installs.
//!
//! Nothing here touches a window. The page never names a file or runs a command by itself: running
//! a chain for testing is the one command with an effect outside the store, and it is returned to
//! the caller as [`Outcome::Test`].

use super::protocol::{snap_json, PanelFields};
use crate::commands::{
    chains::{self, Chain},
    chord::Chord,
    registry,
    store::{self, AssignError, Store},
};

const MAX_ERROR_CHARS: usize = 240;
const MAX_ID_CHARS: usize = 40;
const MAX_TEXT_CHARS: usize = 4096;

/// The on/off options the page may set, by their file names.
pub const FLAGS: [&str; 7] = [
    "shortcuts_enabled",
    "single_key_shortcuts",
    "alt_opens_menu",
    "focus_cycling",
    "palette_keep_last",
    "palette_open_new_tab",
    "palette_notes_in_panel",
];

/// How Bowser answered the page's last command. Sent once, with the next snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ack {
    #[default]
    None,
    Saved,
    Failed,
    /// The chord is on another command; the snapshot names it and the page offers to take it.
    Conflict,
    /// A chain was started for testing; nothing was saved.
    Ran,
}

impl Ack {
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Saved => "saved",
            Self::Failed => "failed",
            Self::Conflict => "conflict",
            Self::Ran => "ran",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status<'a> {
    pub ack: Ack,
    pub error: &'a str,
    /// For [`Ack::Conflict`]: the chord, and the id and label of its owner.
    pub conflict: Option<(&'a str, &'a str, &'a str)>,
}

fn plain(text: &str, max: usize) -> String {
    text.chars().take(max).map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// `{v:1,t:"snap",p:"commands",d:{..}}`. Every key is always present.
pub fn snapshot_json(status: Status<'_>, store: &Store) -> String {
    let entries = store.entries();
    let options = &store.options;
    snap_json("commands", |out| {
        out.string("ack", status.ack.key());
        out.string("error", plain(status.error, MAX_ERROR_CHARS).trim());
        let (chord, owner, label) = status.conflict.unwrap_or(("", "", ""));
        out.string("conflictChord", chord);
        out.string("conflictId", owner);
        out.string("conflictLabel", &plain(label, 64));
        out.bool("shortcutsEnabled", options.shortcuts_enabled);
        out.bool("singleKey", options.single_key_shortcuts);
        out.bool("altMenu", options.alt_opens_menu);
        out.bool("focusCycling", options.focus_cycling);
        out.bool("paletteKeepLast", options.palette_keep_last);
        out.bool("paletteNewTab", options.palette_open_new_tab);
        out.bool("paletteNotesInPanel", options.palette_notes_in_panel);
        out.uint("paletteMax", u64::from(options.palette_max_results));
        out.array("paletteTypes", |list| for kind in &options.palette_types { list.string(kind.key()); });
        out.bool("paletteDefaultOrder", options.palette_types == store::ResultType::DEFAULT_ORDER);
        out.array("sites", |list| for site in &store.sites { list.string(site); });
        out.array("commands", |list| {
            for entry in &entries {
                let chords: Vec<String> = store.effective(entry).iter().filter_map(Chord::format).collect();
                let defaults: Vec<String> = entry.defaults.iter().filter_map(Chord::format).collect();
                list.object(|o| {
                    o.string("id", &entry.id);
                    o.string("label", &plain(&entry.label, 64));
                    o.string("category", entry.category.label());
                    o.array("chords", |l| for chord in &chords { l.string(chord); });
                    o.array("defaults", |l| for chord in &defaults { l.string(chord); });
                    o.bool("changed", chords != defaults);
                    o.bool("priority", store.priority_of(entry));
                    o.bool("chain", entry.category == registry::Category::Chains);
                });
            }
        });
        out.array("chains", |list| {
            for chain in &store.chains {
                list.object(|o| {
                    o.uint("id", u64::from(chain.id));
                    o.string("name", &plain(&chain.name, chains::MAX_NAME_CHARS));
                    // One string per step, `delay:command`: a chain of 32 steps stays small.
                    o.array("steps", |l| for step in &chain.steps { l.string(&format!("{}:{}", step.delay_ms, step.command)); });
                });
            }
        });
    })
    .expect("commands is a panel token")
}

/// A validated command from the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Assign { id: String, chord: String, take: bool },
    Remove { id: String, chord: String },
    Clear(String),
    Restore(String),
    RestoreAll,
    /// Show the cheat sheet popup (Ctrl+F1).
    CheatSheet,
    /// The editor starts or stops waiting for a key press: the page then gets every key.
    Capture(bool),
    Priority { id: String, on: bool },
    Flag { name: &'static str, on: bool },
    ResultsMax(u8),
    Types(String),
    TypesReset,
    SiteAdd(String),
    SiteRemove(String),
    ChainSave(String),
    ChainClone(u32),
    ChainDelete(u32),
    ChainTest(String),
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().count() <= MAX_ID_CHARS && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b':')
}

/// `id<TAB>chord` as the page sends it.
fn id_and_chord(text: &str) -> Option<(String, String)> {
    let (id, chord) = text.split_once('\t')?;
    (valid_id(id) && !chord.is_empty() && chord.len() <= 40 && !chord.contains(['\t', '\n'])).then(|| (id.to_owned(), chord.to_owned()))
}

/// Maps `{t:"set",a,...}` to an [`Action`]: each action with exactly its own fields, nothing else.
pub fn map_command(f: &PanelFields) -> Option<Action> {
    if f.parent.is_some() || f.before.is_some() || f.url.is_some() {
        return None;
    }
    let only = |id: bool, n: bool, on: bool, text: bool| f.id.is_some() == id && f.n.is_some() == n && f.on.is_some() == on && f.text.is_some() == text;
    let text = f.text.as_deref().unwrap_or("");
    if text.chars().count() > MAX_TEXT_CHARS {
        return None;
    }
    Some(match f.a.as_str() {
        "kbassign" if only(false, false, true, true) => {
            let (id, chord) = id_and_chord(text)?;
            Action::Assign { id, chord, take: f.on? }
        }
        "kbremove" if only(false, false, false, true) => {
            let (id, chord) = id_and_chord(text)?;
            Action::Remove { id, chord }
        }
        "kbclear" if only(false, false, false, true) => Action::Clear(valid_id(text).then(|| text.to_owned())?),
        "kbrestore" if only(false, false, false, true) => Action::Restore(valid_id(text).then(|| text.to_owned())?),
        "kbrestoreall" if only(false, false, false, false) => Action::RestoreAll,
        "kbsheet" if only(false, false, false, false) => Action::CheatSheet,
        "kbcapture" if only(false, false, true, false) => Action::Capture(f.on?),
        "kbpriority" if only(false, false, true, true) => Action::Priority { id: valid_id(text).then(|| text.to_owned())?, on: f.on? },
        "kboption" if only(false, false, true, true) => Action::Flag { name: FLAGS.into_iter().find(|name| *name == text)?, on: f.on? },
        "qcmax" if only(false, true, false, false) => {
            let n = f.n?;
            Action::ResultsMax(u8::try_from(n).ok().filter(|n| (store::MIN_RESULTS..=store::MAX_RESULTS).contains(n))?)
        }
        "qctypes" if only(false, false, false, true) => Action::Types(text.to_owned()),
        "qctypesreset" if only(false, false, false, false) => Action::TypesReset,
        "kbsiteadd" if only(false, false, false, true) => Action::SiteAdd(text.to_owned()),
        "kbsiteremove" if only(false, false, false, true) => Action::SiteRemove(text.to_owned()),
        "chainsave" if only(false, false, false, true) => Action::ChainSave(text.to_owned()),
        "chaintest" if only(false, false, false, true) => Action::ChainTest(text.to_owned()),
        "chainclone" if only(false, true, false, false) => Action::ChainClone(u32::try_from(f.n?).ok()?),
        "chaindelete" if only(false, true, false, false) => Action::ChainDelete(u32::try_from(f.n?).ok()?),
        _ => return None,
    })
}

/// What a command did to the store copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The copy changed: save and install it.
    Saved,
    /// Nothing changed: the chord is another command's. The page offers to take it.
    Conflict { chord: String, owner: String, label: String },
    /// Nothing changed; say why.
    Failed(String),
    /// Run this draft chain now (it is not saved by this).
    Test(Chain),
    /// Show the cheat sheet; nothing changed.
    CheatSheet,
    /// Capture mode changed; nothing is saved.
    Capture(bool),
}

/// Applies `action` to `store`.
pub fn apply(store: &mut Store, action: &Action) -> Outcome {
    let changed = |ok: bool, what: &str| if ok { Outcome::Saved } else { Outcome::Failed(format!("{what} no longer exists.")) };
    match action {
        Action::Assign { id, chord, take } => match store.assign(id, chord, *take) {
            Ok(_) => Outcome::Saved,
            Err(AssignError::Conflict { owner, label }) => Outcome::Conflict { chord: chord.clone(), owner, label },
            Err(error) => Outcome::Failed(error.message(chord)),
        },
        Action::Remove { id, chord } => changed(store.remove_chord(id, chord), "That shortcut"),
        Action::Clear(id) => changed(store.clear(id), "That command"),
        Action::Restore(id) => changed(store.restore(id), "That command"),
        Action::RestoreAll => {
            store.restore_all();
            Outcome::Saved
        }
        Action::CheatSheet => Outcome::CheatSheet,
        Action::Capture(on) => Outcome::Capture(*on),
        Action::Priority { id, on } => changed(store.set_priority(id, *on), "That command"),
        Action::Flag { name, on } => {
            let mut options = store.options.clone();
            set_flag(&mut options, name, *on);
            store.options = options;
            Outcome::Saved
        }
        Action::ResultsMax(n) => {
            store.options.palette_max_results = *n;
            Outcome::Saved
        }
        Action::Types(text) => {
            let types = store::parse_types(text);
            // Every name must have been understood: a partly bad list is a bug in the page.
            if text.split(',').filter(|part| !part.trim().is_empty()).count() != types.len() {
                return Outcome::Failed("That list of result types is not allowed.".into());
            }
            store.options.palette_types = types;
            Outcome::Saved
        }
        Action::TypesReset => {
            store.options.palette_types = store::ResultType::DEFAULT_ORDER.to_vec();
            Outcome::Saved
        }
        Action::SiteAdd(site) => match store.add_site(site) {
            Ok(()) => Outcome::Saved,
            Err(error) => Outcome::Failed(error),
        },
        Action::SiteRemove(site) => changed(store.remove_site(site), "That site"),
        Action::ChainSave(text) => match chains::parse_draft(text) {
            None => Outcome::Failed("That chain could not be read.".into()),
            Some(chain) => match store.save_chain(chain) {
                Ok(_) => Outcome::Saved,
                Err(error) => Outcome::Failed(error),
            },
        },
        Action::ChainClone(id) => match store.clone_chain(*id) {
            Ok(_) => Outcome::Saved,
            Err(error) => Outcome::Failed(error),
        },
        Action::ChainDelete(id) => changed(store.delete_chain(*id), "That chain"),
        Action::ChainTest(text) => match chains::parse_draft(text) {
            None => Outcome::Failed("That chain could not be read.".into()),
            Some(chain) => {
                let others: Vec<Chain> = store.chains.iter().filter(|c| c.id != chain.id).cloned().collect();
                match chains::validate(&chain, &others, |id| registry::builtin(id).is_some()) {
                    Ok(()) => Outcome::Test(chain),
                    Err(error) => Outcome::Failed(error),
                }
            }
        },
    }
}

fn set_flag(options: &mut store::Options, name: &str, on: bool) {
    match name {
        "shortcuts_enabled" => options.shortcuts_enabled = on,
        "single_key_shortcuts" => options.single_key_shortcuts = on,
        "alt_opens_menu" => options.alt_opens_menu = on,
        "focus_cycling" => options.focus_cycling = on,
        "palette_keep_last" => options.palette_keep_last = on,
        "palette_open_new_tab" => options.palette_open_new_tab = on,
        "palette_notes_in_panel" => options.palette_notes_in_panel = on,
        _ => {}
    }
}

/// The empty-snapshot text for tests and fixtures.
#[cfg(test)]
pub fn fixture_json() -> String {
    let mut store = Store::default();
    store.assign("close-tab", "Ctrl+Shift+X", false).expect("assign");
    store.clear("new-private-tab");
    store.set_priority("history", true);
    store.options.palette_types = vec![store::ResultType::Command, store::ResultType::Tab, store::ResultType::Calc];
    store.options.palette_max_results = 9;
    store.add_site("docs.example.com").expect("site");
    store
        .save_chain(Chain {
            id: 0,
            name: "Morning".into(),
            steps: vec![chains::Step { command: "new-tab".into(), delay_ms: 0 }, chains::Step { command: "history".into(), delay_ms: 250 }],
        })
        .expect("chain");
    snapshot_json(Status::default(), &store)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(a: &str) -> PanelFields {
        PanelFields { a: a.to_owned(), id: None, parent: None, before: None, n: None, on: None, text: None, url: None }
    }
    fn with_text(a: &str, text: &str) -> PanelFields {
        PanelFields { text: Some(text.into()), ..fields(a) }
    }

    #[test]
    fn commands_carry_exactly_their_own_fields() {
        let mut assign = with_text("kbassign", "new-tab\tCtrl+Shift+Q");
        assert_eq!(map_command(&assign), None, "assign needs its on flag");
        assign.on = Some(false);
        assert_eq!(map_command(&assign), Some(Action::Assign { id: "new-tab".into(), chord: "Ctrl+Shift+Q".into(), take: false }));
        assign.on = Some(true);
        assert!(matches!(map_command(&assign), Some(Action::Assign { take: true, .. })));
        let mut extra = assign.clone();
        extra.n = Some(1);
        assert_eq!(map_command(&extra), None);
        for bad in ["new-tab", "new-tab\t", "\tCtrl+Q", "Bad Id\tCtrl+Q", "new-tab\tCtrl\tQ"] {
            let mut f = with_text("kbassign", bad);
            f.on = Some(false);
            assert_eq!(map_command(&f), None, "{bad:?}");
        }
        assert_eq!(map_command(&with_text("kbclear", "reload")), Some(Action::Clear("reload".into())));
        assert_eq!(map_command(&with_text("kbclear", "chain:3")), Some(Action::Clear("chain:3".into())));
        assert_eq!(map_command(&with_text("kbclear", "Reload")), None);
        assert_eq!(map_command(&with_text("kbrestore", "reload")), Some(Action::Restore("reload".into())));
        assert_eq!(map_command(&fields("kbrestoreall")), Some(Action::RestoreAll));
        assert_eq!(map_command(&fields("kbsheet")), Some(Action::CheatSheet));
        assert_eq!(apply(&mut Store::default(), &Action::CheatSheet), Outcome::CheatSheet);
        let mut capture = fields("kbcapture");
        assert_eq!(map_command(&capture), None, "capture needs its on flag");
        capture.on = Some(true);
        assert_eq!(map_command(&capture), Some(Action::Capture(true)));
        capture.text = Some("x".into());
        assert_eq!(map_command(&capture), None);
        assert_eq!(apply(&mut Store::default(), &Action::Capture(false)), Outcome::Capture(false));
        assert_eq!(map_command(&with_text("kbrestoreall", "x")), None);
        let mut option = with_text("kboption", "single_key_shortcuts");
        option.on = Some(true);
        assert_eq!(map_command(&option), Some(Action::Flag { name: "single_key_shortcuts", on: true }));
        option.text = Some("palette_types".into());
        assert_eq!(map_command(&option), None, "only the listed flags");
        let mut max = fields("qcmax");
        max.n = Some(9);
        assert_eq!(map_command(&max), Some(Action::ResultsMax(9)));
        for out_of_range in [2, 31, -1, 1000] {
            max.n = Some(out_of_range);
            assert_eq!(map_command(&max), None, "{out_of_range}");
        }
        let mut clone = fields("chainclone");
        clone.n = Some(4);
        assert_eq!(map_command(&clone), Some(Action::ChainClone(4)));
        clone.n = Some(-4);
        assert_eq!(map_command(&clone), None);
        assert_eq!(map_command(&with_text("chainsave", "0\tX\n0\tnew-tab")), Some(Action::ChainSave("0\tX\n0\tnew-tab".into())));
        assert_eq!(map_command(&with_text("nonsense", "x")), None);
        let mut with_url = with_text("kbclear", "reload");
        with_url.url = Some("x".into());
        assert_eq!(map_command(&with_url), None);
        assert_eq!(map_command(&with_text("chainsave", &"x".repeat(MAX_TEXT_CHARS + 1))), None);
    }

    #[test]
    fn every_action_changes_the_store_copy_or_says_why_not() {
        let mut store = Store::default();
        let assign = |id: &str, chord: &str, take| Action::Assign { id: id.into(), chord: chord.into(), take };
        assert_eq!(apply(&mut store, &assign("history", "Ctrl+Shift+Q", false)), Outcome::Saved);
        assert_eq!(
            apply(&mut store, &assign("history", "Ctrl+J", false)),
            Outcome::Conflict { chord: "Ctrl+J".into(), owner: "downloads".into(), label: "Downloads".into() }
        );
        assert_eq!(apply(&mut store, &assign("history", "Ctrl+J", true)), Outcome::Saved);
        assert!(matches!(apply(&mut store, &assign("history", "Ctrl+C", false)), Outcome::Failed(m) if m.contains("editing")));
        assert_eq!(apply(&mut store, &Action::Remove { id: "history".into(), chord: "Ctrl+J".into() }), Outcome::Saved);
        assert!(matches!(apply(&mut store, &Action::Remove { id: "history".into(), chord: "Ctrl+J".into() }), Outcome::Failed(_)));
        assert_eq!(apply(&mut store, &Action::Clear("reload".into())), Outcome::Saved);
        assert_eq!(apply(&mut store, &Action::Restore("reload".into())), Outcome::Saved);
        assert!(matches!(apply(&mut store, &Action::Restore("nope".into())), Outcome::Failed(_)));
        assert_eq!(apply(&mut store, &Action::Priority { id: "history".into(), on: true }), Outcome::Saved);
        assert_eq!(apply(&mut store, &Action::Flag { name: "alt_opens_menu", on: true }), Outcome::Saved);
        assert!(store.options.alt_opens_menu);
        assert_eq!(apply(&mut store, &Action::Flag { name: "shortcuts_enabled", on: false }), Outcome::Saved);
        assert!(!store.options.shortcuts_enabled);
        assert_eq!(apply(&mut store, &Action::ResultsMax(20)), Outcome::Saved);
        assert_eq!(store.options.palette_max_results, 20);
        assert_eq!(apply(&mut store, &Action::Types("history,tab".into())), Outcome::Saved);
        assert_eq!(store.options.palette_types, [store::ResultType::History, store::ResultType::Tab]);
        assert!(matches!(apply(&mut store, &Action::Types("history,bogus".into())), Outcome::Failed(_)), "an unknown type is refused");
        assert!(matches!(apply(&mut store, &Action::Types(String::new())), Outcome::Failed(_)));
        assert_eq!(apply(&mut store, &Action::TypesReset), Outcome::Saved);
        assert_eq!(store.options.palette_types, store::ResultType::DEFAULT_ORDER.to_vec());
        assert_eq!(apply(&mut store, &Action::SiteAdd("https://Docs.Example.com/x".into())), Outcome::Saved);
        assert_eq!(store.sites, ["docs.example.com"]);
        assert!(matches!(apply(&mut store, &Action::SiteAdd("nope nope".into())), Outcome::Failed(_)));
        assert_eq!(apply(&mut store, &Action::SiteRemove("docs.example.com".into())), Outcome::Saved);
        assert!(matches!(apply(&mut store, &Action::SiteRemove("docs.example.com".into())), Outcome::Failed(_)));
        assert_eq!(apply(&mut store, &Action::RestoreAll), Outcome::Saved);
        assert!(store.binds.is_empty() && store.priority.is_empty());
    }

    #[test]
    fn chains_can_be_created_cloned_edited_deleted_and_tested() {
        let mut store = Store::default();
        assert_eq!(apply(&mut store, &Action::ChainSave("0\tMorning\n0\tnew-tab\n250\thistory".into())), Outcome::Saved);
        assert_eq!(store.chains.len(), 1);
        assert_eq!(store.chains[0].steps.len(), 2);
        assert_eq!(apply(&mut store, &Action::ChainSave("1\tMorning (edited)\n0\tnew-tab".into())), Outcome::Saved);
        assert_eq!((store.chains[0].name.as_str(), store.chains[0].steps.len()), ("Morning (edited)", 1));
        assert_eq!(apply(&mut store, &Action::ChainClone(1)), Outcome::Saved);
        assert_eq!(store.chains.len(), 2);
        assert!(matches!(apply(&mut store, &Action::ChainSave("0\tmorning (edited) copy\n0\tnew-tab".into())), Outcome::Failed(m) if m.contains("already")));
        assert!(matches!(apply(&mut store, &Action::ChainSave("0\tBad\n0\tnot-a-command".into())), Outcome::Failed(_)));
        assert!(matches!(apply(&mut store, &Action::ChainSave("0\tEmpty".into())), Outcome::Failed(_)));
        assert!(matches!(apply(&mut store, &Action::ChainSave("garbage".into())), Outcome::Failed(_)));
        match apply(&mut store, &Action::ChainTest("0\tDraft\n0\tnew-tab\n100\treload".into())) {
            Outcome::Test(chain) => assert_eq!(chain.steps.len(), 2),
            other => panic!("{other:?}"),
        }
        assert_eq!(store.chains.len(), 2, "testing a draft saves nothing");
        assert!(matches!(apply(&mut store, &Action::ChainTest("0\tDraft\n0\tnope".into())), Outcome::Failed(_)));
        assert_eq!(apply(&mut store, &Action::ChainDelete(1)), Outcome::Saved);
        assert!(matches!(apply(&mut store, &Action::ChainDelete(1)), Outcome::Failed(_)));
        assert_eq!(store.chains.len(), 1);
    }

    #[test]
    fn the_snapshot_lists_every_command_option_site_and_chain() {
        let json = fixture_json();
        assert!(json.starts_with("{\"v\":1,\"t\":\"snap\",\"p\":\"commands\",\"d\":{\"ack\":\"none\""));
        for needle in [
            "\"id\":\"close-tab\"",
            "\"chords\":[\"Ctrl+W\",\"Ctrl+F4\",\"Ctrl+Shift+X\"]",
            "\"id\":\"new-private-tab\",\"label\":\"New private tab\",\"category\":\"Tabs\",\"chords\":[],\"defaults\":[\"Ctrl+Shift+P\"],\"changed\":true",
            "\"paletteTypes\":[\"command\",\"tab\",\"calc\"]",
            "\"paletteDefaultOrder\":false",
            "\"paletteMax\":9",
            "\"sites\":[\"docs.example.com\"]",
            "\"steps\":[\"0:new-tab\",\"250:history\"]",
            "\"id\":\"chain:1\"",
            "\"shortcutsEnabled\":true",
        ] {
            assert!(json.contains(needle), "missing {needle}\n{json}");
        }
        assert!(json.len() < 64 * 1024, "{}", json.len());
        let conflict = snapshot_json(Status { ack: Ack::Conflict, error: "x", conflict: Some(("Ctrl+J", "downloads", "Downloads")) }, &Store::default());
        assert!(conflict.contains("\"ack\":\"conflict\"") && conflict.contains("\"conflictChord\":\"Ctrl+J\"") && conflict.contains("\"conflictLabel\":\"Downloads\""));
    }

    #[test]
    fn the_snapshot_fixture_matches_the_page_contract_file() {
        let fixture = format!("{}\n", fixture_json());
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/commands-snapshot.json");
        if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some() {
            std::fs::write(&path, &fixture).expect("write commands fixture");
        }
        assert_eq!(
            std::fs::read_to_string(&path).expect("commands fixture"),
            fixture,
            "set BOWSER_WRITE_FIXTURES=1 cargo test commands_page --lib"
        );
    }
}
