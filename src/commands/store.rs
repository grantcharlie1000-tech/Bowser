//! What the user has changed about commands: shortcut assignments, the keyboard and Quick Commands
//! options, and the chains. Plain text beside the profile's other files (`keybindings.txt`,
//! `chains.txt`); nothing in them is a secret and nothing leaves the machine.
//!
//! `keybindings.txt`: `bowser-keys-v1`, then lines of tab-separated fields:
//! - `opt<TAB>name<TAB>value` for [`Options`];
//! - `bind<TAB>command id<TAB>chord;chord` the command's whole list, which may be empty (cleared);
//! - `priority<TAB>command id<TAB>true|false` whether the browser keeps the command's chords on a
//!   site that prioritises its own shortcuts, when that differs from the registry's default;
//! - `site<TAB>host` a site that prioritises its own shortcuts.
//!
//! A command with no `bind` line keeps its registry defaults. Unknown lines are ignored, so an older
//! or newer build reads the file without losing the rest.

use std::{
    collections::BTreeMap,
    fs, io,
    path::Path,
};

use super::{
    chains::{self, Chain},
    chord::{Chord, Refusal},
    registry::{Category, CommandDef, Gate, Handler, PaletteKind},
};

const MAX_FILE_BYTES: usize = 64 * 1024;
const HEADER: &str = "bowser-keys-v1";
pub const MAX_CHORDS_PER_COMMAND: usize = 6;
pub const MAX_SITES: usize = 200;
pub const MIN_RESULTS: u8 = 3;
pub const MAX_RESULTS: u8 = 30;
pub const DEFAULT_RESULTS: u8 = 12;

/// What the palette can show, in its default order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResultType {
    Tab,
    Closed,
    Workspace,
    Command,
    Action,
    Bookmark,
    Note,
    History,
    Calc,
}

impl ResultType {
    pub const DEFAULT_ORDER: [Self; 9] = [
        Self::Tab,
        Self::Closed,
        Self::Workspace,
        Self::Command,
        Self::Action,
        Self::Bookmark,
        Self::Note,
        Self::History,
        Self::Calc,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Tab => "tab",
            Self::Closed => "closed",
            Self::Workspace => "workspace",
            Self::Command => "command",
            Self::Action => "action",
            Self::Bookmark => "bookmark",
            Self::Note => "note",
            Self::History => "history",
            Self::Calc => "calc",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::DEFAULT_ORDER.into_iter().find(|kind| kind.key() == key)
    }
}

/// An ordered list of result types, each once; never empty.
pub fn parse_types(text: &str) -> Vec<ResultType> {
    let mut out: Vec<ResultType> = Vec::new();
    for part in text.split(',') {
        if let Some(kind) = ResultType::parse(part.trim())
            && !out.contains(&kind)
        {
            out.push(kind);
        }
    }
    if out.is_empty() {
        ResultType::DEFAULT_ORDER.to_vec()
    } else {
        out
    }
}

pub fn types_text(types: &[ResultType]) -> String {
    types.iter().map(|kind| kind.key()).collect::<Vec<_>>().join(",")
}

/// The keyboard and Quick Commands options (the settings page's switches).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub shortcuts_enabled: bool,
    pub single_key_shortcuts: bool,
    pub alt_opens_menu: bool,
    pub focus_cycling: bool,
    pub palette_types: Vec<ResultType>,
    pub palette_keep_last: bool,
    pub palette_max_results: u8,
    pub palette_open_new_tab: bool,
    pub palette_notes_in_panel: bool,
    /// Typing a bookmark's nickname (set in T2's bookmark dialog) opens that bookmark at once.
    pub palette_open_nickname: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            shortcuts_enabled: true,
            single_key_shortcuts: false,
            alt_opens_menu: false,
            focus_cycling: false,
            palette_types: ResultType::DEFAULT_ORDER.to_vec(),
            palette_keep_last: false,
            palette_max_results: DEFAULT_RESULTS,
            palette_open_new_tab: false,
            palette_notes_in_panel: true,
            palette_open_nickname: true,
        }
    }
}

impl Options {
    /// Whether the enabled types keep the default relative order: the palette then ranks by match
    /// quality and the best match leads; any other order groups results by it.
    pub fn palette_ranked(&self) -> bool {
        let mut last = None;
        for kind in &self.palette_types {
            let at = ResultType::DEFAULT_ORDER.iter().position(|k| k == kind);
            if at < last {
                return false;
            }
            last = at;
        }
        true
    }

    fn set(&mut self, name: &str, value: &str) {
        let flag = || match value {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        };
        match name {
            "shortcuts_enabled" => self.shortcuts_enabled = flag().unwrap_or(self.shortcuts_enabled),
            "single_key_shortcuts" => self.single_key_shortcuts = flag().unwrap_or(self.single_key_shortcuts),
            "alt_opens_menu" => self.alt_opens_menu = flag().unwrap_or(self.alt_opens_menu),
            "focus_cycling" => self.focus_cycling = flag().unwrap_or(self.focus_cycling),
            "palette_types" => self.palette_types = parse_types(value),
            "palette_keep_last" => self.palette_keep_last = flag().unwrap_or(self.palette_keep_last),
            "palette_max_results" => {
                if let Ok(n) = value.parse::<u8>() {
                    self.palette_max_results = n.clamp(MIN_RESULTS, MAX_RESULTS);
                }
            }
            "palette_open_new_tab" => self.palette_open_new_tab = flag().unwrap_or(self.palette_open_new_tab),
            "palette_notes_in_panel" => self.palette_notes_in_panel = flag().unwrap_or(self.palette_notes_in_panel),
            "palette_open_nickname" => self.palette_open_nickname = flag().unwrap_or(self.palette_open_nickname),
            _ => {}
        }
    }

    fn lines(&self) -> Vec<(&'static str, String)> {
        vec![
            ("shortcuts_enabled", self.shortcuts_enabled.to_string()),
            ("single_key_shortcuts", self.single_key_shortcuts.to_string()),
            ("alt_opens_menu", self.alt_opens_menu.to_string()),
            ("focus_cycling", self.focus_cycling.to_string()),
            ("palette_types", types_text(&self.palette_types)),
            ("palette_keep_last", self.palette_keep_last.to_string()),
            ("palette_max_results", self.palette_max_results.to_string()),
            ("palette_open_new_tab", self.palette_open_new_tab.to_string()),
            ("palette_notes_in_panel", self.palette_notes_in_panel.to_string()),
            ("palette_open_nickname", self.palette_open_nickname.to_string()),
        ]
    }
}

/// How a command runs, with owned data (a chain's number is not `'static`).
#[derive(Clone, Copy, Debug)]
pub enum EntryHandler {
    Builtin(Handler),
    Chain(u32),
}

impl std::fmt::Debug for Handler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Key(command) => write!(f, "Key({command:?})"),
            Self::Action(action) => write!(f, "Action({action})"),
            Self::Run(_) => write!(f, "Run(..)"),
        }
    }
}

/// A command as the keyboard editor, the cheat sheet and the palette see it: a built-in or a chain.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub category: Category,
    pub defaults: Vec<Chord>,
    pub palette: PaletteKind,
    pub gate: Option<Gate>,
    pub handler: EntryHandler,
    pub priority: bool,
}

impl Entry {
    pub fn from_def(def: &CommandDef) -> Self {
        Self {
            id: def.id.to_owned(),
            label: def.label.to_owned(),
            detail: def.detail.to_owned(),
            category: def.category,
            defaults: def.defaults.iter().filter_map(|text| Chord::parse(text)).collect(),
            palette: def.palette,
            gate: def.gate,
            handler: EntryHandler::Builtin(def.handler),
            priority: def.priority,
        }
    }

    pub fn from_chain(chain: &Chain) -> Self {
        Self {
            id: chain.command_id(),
            label: chain.name.clone(),
            detail: format!("Run a chain of {} command{}", chain.steps.len(), if chain.steps.len() == 1 { "" } else { "s" }),
            category: Category::Chains,
            defaults: Vec::new(),
            palette: PaletteKind::Command,
            gate: None,
            handler: EntryHandler::Chain(chain.id),
            priority: false,
        }
    }
}

/// Why an assignment was refused or needs a decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssignError {
    UnknownCommand,
    BadChord,
    Refused(Refusal),
    TooMany,
    /// Another command has the chord: the id and label of its owner.
    Conflict { owner: String, label: String },
}

impl AssignError {
    pub fn message(&self, chord: &str) -> String {
        match self {
            Self::UnknownCommand => "That command is not known.".into(),
            Self::BadChord => "That is not a shortcut Bowser can use.".into(),
            Self::Refused(refusal) => refusal.message().into(),
            Self::TooMany => format!("A command holds at most {MAX_CHORDS_PER_COMMAND} shortcuts."),
            Self::Conflict { label, .. } => format!("{chord} is already used by {label}."),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Store {
    pub options: Options,
    /// Per command: its whole shortcut list when it is not the registry default.
    pub binds: BTreeMap<String, Vec<Chord>>,
    pub priority: BTreeMap<String, bool>,
    pub sites: Vec<String>,
    pub chains: Vec<Chain>,
    /// The files that were there but could not be read when this was loaded. What they hold was
    /// never loaded, so [`Store::save`] refuses to write over them.
    pub unread: Unread,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unread {
    pub keys: bool,
    pub chains: bool,
}

/// The text of `path`, `None` when there is none (missing or blank). A file that cannot be read,
/// is larger than `max`, is not UTF-8 or is not headed `header` (a BOM is allowed) is an error:
/// treating it as empty would let the next save delete it.
fn read_for_change(path: &Path, header: &str, max: usize) -> io::Result<Option<String>> {
    let invalid = |why: &str| io::Error::new(io::ErrorKind::InvalidData, format!("{} {why}", path.display()));
    let bytes = match fs::metadata(path) {
        Ok(meta) if meta.len() > max as u64 => return Err(invalid("is too large to read")),
        Ok(_) => fs::read(path)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let text = String::from_utf8(bytes).map_err(|_| invalid("is not UTF-8 text"))?;
    let body = text.trim_start_matches('\u{feff}');
    if body.len() > max {
        return Err(invalid("is too large to read"));
    }
    if body.trim().is_empty() {
        return Ok(None);
    }
    if body.lines().next().map(str::trim) != Some(header) {
        return Err(invalid("is not a file Bowser can read"));
    }
    Ok(Some(body.to_owned()))
}

impl Store {
    pub fn parse_keys(&mut self, text: &str) {
        if text.len() > MAX_FILE_BYTES {
            return;
        }
        let mut lines = text.trim_start_matches('\u{feff}').lines();
        if lines.next().map(str::trim) != Some(HEADER) {
            return;
        }
        for line in lines {
            let parts: Vec<&str> = line.split('\t').collect();
            match parts.as_slice() {
                ["opt", name, value] => self.options.set(name, value),
                ["bind", id, list] if valid_id(id) => {
                    let mut chords: Vec<Chord> = Vec::new();
                    for text in list.split(';').filter(|t| !t.is_empty()) {
                        if let Some(chord) = Chord::parse(text)
                            && !chords.contains(&chord)
                            && chords.len() < MAX_CHORDS_PER_COMMAND
                        {
                            chords.push(chord);
                        }
                    }
                    self.binds.insert((*id).to_owned(), chords);
                }
                ["priority", id, value] if valid_id(id) && matches!(*value, "true" | "false") => {
                    self.priority.insert((*id).to_owned(), *value == "true");
                }
                ["site", host] => {
                    if let Some(site) = normalise_site(host)
                        && self.sites.len() < MAX_SITES
                        && !self.sites.contains(&site)
                    {
                        self.sites.push(site);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn serialize_keys(&self) -> String {
        let mut out = format!("{HEADER}\n");
        for (name, value) in self.options.lines() {
            out.push_str(&format!("opt\t{name}\t{value}\n"));
        }
        for (id, chords) in &self.binds {
            let list: Vec<String> = chords.iter().filter_map(Chord::format).collect();
            out.push_str(&format!("bind\t{id}\t{}\n", list.join(";")));
        }
        for (id, on) in &self.priority {
            out.push_str(&format!("priority\t{id}\t{on}\n"));
        }
        for site in &self.sites {
            out.push_str(&format!("site\t{site}\n"));
        }
        out
    }

    pub fn load(keys: &Path, chains_file: &Path) -> Self {
        let mut store = Self::default();
        match read_for_change(keys, HEADER, MAX_FILE_BYTES) {
            Ok(Some(text)) => store.parse_keys(&text),
            Ok(None) => {}
            Err(_) => store.unread.keys = true,
        }
        match read_for_change(chains_file, chains::HEADER, chains::MAX_FILE_BYTES) {
            Ok(Some(text)) => store.chains = chains::parse(&text),
            Ok(None) => {}
            Err(_) => store.unread.chains = true,
        }
        store.prune();
        store
    }

    /// Writes both files (each temp file, fsync, rename; the chains first, so a binding is never
    /// saved before the chain it names). Refused, writing nothing, when either file could not be
    /// read at load or cannot be read now: what it holds would be lost.
    pub fn save(&self, keys: &Path, chains_file: &Path) -> io::Result<()> {
        let refused = |path: &Path| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} could not be read, so it was left unchanged. Restart Bowser after fixing or restoring it.", path.display()),
            )
        };
        if self.unread.keys {
            return Err(refused(keys));
        }
        if self.unread.chains {
            return Err(refused(chains_file));
        }
        let (keys_text, chains_text) = (self.serialize_keys(), chains::serialize(&self.chains));
        if keys_text.len() > MAX_FILE_BYTES || chains_text.len() > chains::MAX_FILE_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "the shortcuts are too large to save"));
        }
        read_for_change(keys, HEADER, MAX_FILE_BYTES)?;
        read_for_change(chains_file, chains::HEADER, chains::MAX_FILE_BYTES)?;
        crate::productivity::atomic_write(chains_file, chains_text.as_bytes())?;
        crate::productivity::atomic_write(keys, keys_text.as_bytes())
    }

    /// Drops bindings and priorities that name a command that no longer exists (a deleted chain, a
    /// removed built-in), so they cannot come back to life under a reused id.
    pub fn prune(&mut self) {
        let chains = &self.chains;
        let known = |id: &str| super::registry::builtin(id).is_some() || chains::chain_number(id).is_some_and(|n| chains.iter().any(|c| c.id == n));
        self.binds.retain(|id, _| known(id));
        self.priority.retain(|id, _| known(id));
    }

    /// Every command: the built-ins in registry order, then the chains.
    pub fn entries(&self) -> Vec<Entry> {
        super::registry::BUILTINS
            .iter()
            .map(Entry::from_def)
            .chain(self.chains.iter().map(Entry::from_chain))
            .collect()
    }

    pub fn effective(&self, entry: &Entry) -> Vec<Chord> {
        self.binds.get(&entry.id).cloned().unwrap_or_else(|| entry.defaults.clone())
    }

    pub fn priority_of(&self, entry: &Entry) -> bool {
        self.priority.get(&entry.id).copied().unwrap_or(entry.priority)
    }

    /// Stores `chords` as `id`'s list, or forgets the override when it equals the default.
    fn put(&mut self, entry: &Entry, chords: Vec<Chord>) {
        if chords == entry.defaults {
            self.binds.remove(&entry.id);
        } else {
            self.binds.insert(entry.id.clone(), chords);
        }
    }

    fn owner_of(&self, entries: &[Entry], chord: Chord, except: &str) -> Option<Entry> {
        entries.iter().find(|entry| entry.id != except && self.effective(entry).contains(&chord)).cloned()
    }

    /// Adds `chord_text` to `id`'s shortcuts. A chord another command holds is a conflict unless
    /// `take` is set, which moves it. Returns the id of the command it was taken from, if any.
    pub fn assign(&mut self, id: &str, chord_text: &str, take: bool) -> Result<Option<String>, AssignError> {
        let entries = self.entries();
        let entry = entries.iter().find(|entry| entry.id == id).ok_or(AssignError::UnknownCommand)?;
        let chord = Chord::parse(chord_text).ok_or(AssignError::BadChord)?;
        if chord.format().is_none() {
            return Err(AssignError::BadChord);
        }
        // A command's own default is always allowed (Ctrl+Alt+R), anything else must pass the checks.
        if !entries.iter().any(|e| e.defaults.contains(&chord)) {
            chord.check_assignable(self.options.single_key_shortcuts).map_err(AssignError::Refused)?;
        }
        let mine = self.effective(entry);
        if mine.contains(&chord) {
            return Ok(None);
        }
        if mine.len() >= MAX_CHORDS_PER_COMMAND {
            return Err(AssignError::TooMany);
        }
        let taken = self.owner_of(&entries, chord, id);
        if let Some(owner) = &taken {
            if !take {
                return Err(AssignError::Conflict { owner: owner.id.clone(), label: owner.label.clone() });
            }
            let rest: Vec<Chord> = self.effective(owner).into_iter().filter(|c| *c != chord).collect();
            self.put(owner, rest);
        }
        let mut list = mine;
        list.push(chord);
        self.put(entry, list);
        Ok(taken.map(|owner| owner.id))
    }

    /// Removes one shortcut from a command.
    pub fn remove_chord(&mut self, id: &str, chord_text: &str) -> bool {
        let entries = self.entries();
        let (Some(entry), Some(chord)) = (entries.iter().find(|e| e.id == id), Chord::parse(chord_text)) else { return false };
        let mine = self.effective(entry);
        if !mine.contains(&chord) {
            return false;
        }
        self.put(entry, mine.into_iter().filter(|c| *c != chord).collect());
        true
    }

    /// Removes every shortcut from a command.
    pub fn clear(&mut self, id: &str) -> bool {
        let entries = self.entries();
        let Some(entry) = entries.iter().find(|e| e.id == id) else { return false };
        self.put(entry, Vec::new());
        true
    }

    /// Gives a command its default shortcuts back. A default another command has since taken moves
    /// back; the command being restored wins.
    pub fn restore(&mut self, id: &str) -> bool {
        let entries = self.entries();
        let Some(entry) = entries.iter().find(|e| e.id == id) else { return false };
        self.binds.remove(&entry.id);
        self.priority.remove(&entry.id);
        for chord in &entry.defaults {
            if let Some(owner) = self.owner_of(&entries, *chord, id) {
                let rest: Vec<Chord> = self.effective(&owner).into_iter().filter(|c| c != chord).collect();
                self.put(&owner, rest);
            }
        }
        true
    }

    pub fn restore_all(&mut self) {
        self.binds.clear();
        self.priority.clear();
    }

    pub fn set_priority(&mut self, id: &str, on: bool) -> bool {
        let entries = self.entries();
        let Some(entry) = entries.iter().find(|e| e.id == id) else { return false };
        if on == entry.priority {
            self.priority.remove(id);
        } else {
            self.priority.insert(id.to_owned(), on);
        }
        true
    }

    /// Whether `host` (a page's host) is on the list of sites that prioritise their own shortcuts.
    pub fn site_prioritised(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        let host = host.strip_prefix("www.").unwrap_or(&host);
        self.sites.iter().any(|site| host == site || host.ends_with(&format!(".{site}")))
    }

    pub fn add_site(&mut self, input: &str) -> Result<(), String> {
        let site = normalise_site(input).ok_or("Enter a site such as example.com.")?;
        if self.sites.contains(&site) {
            return Err(format!("{site} is already on the list."));
        }
        if self.sites.len() >= MAX_SITES {
            return Err(format!("The list holds at most {MAX_SITES} sites."));
        }
        self.sites.push(site);
        Ok(())
    }

    pub fn remove_site(&mut self, site: &str) -> bool {
        let before = self.sites.len();
        self.sites.retain(|s| s != site);
        self.sites.len() != before
    }

    /// Adds (id 0) or replaces a chain. The id of a new chain is the next free number.
    pub fn save_chain(&mut self, mut chain: Chain) -> Result<u32, String> {
        let known = |id: &str| super::registry::builtin(id).is_some();
        if chain.id == 0 {
            if self.chains.len() >= chains::MAX_CHAINS {
                return Err(format!("You can keep at most {} chains.", chains::MAX_CHAINS));
            }
            // A hand-edited file can hold an id of u32::MAX: take the lowest free id then.
            let max = self.chains.iter().map(|c| c.id).max().unwrap_or(0);
            chain.id = match max.checked_add(1) {
                Some(id) => id,
                None => (1..=u32::MAX).find(|id| !self.chains.iter().any(|c| c.id == *id)).ok_or("No chain id is free.")?,
            };
        } else if !self.chains.iter().any(|c| c.id == chain.id) {
            return Err("That chain no longer exists.".into());
        }
        chains::validate(&chain, &self.chains, known)?;
        chain.name = chain.name.trim().to_owned();
        let id = chain.id;
        match self.chains.iter_mut().find(|c| c.id == id) {
            Some(slot) => *slot = chain,
            None => self.chains.push(chain),
        }
        Ok(id)
    }

    pub fn clone_chain(&mut self, id: u32) -> Result<u32, String> {
        let source = self.chains.iter().find(|c| c.id == id).ok_or("That chain no longer exists.")?.clone();
        let mut name = format!("{} copy", source.name);
        let mut n = 2;
        while self.chains.iter().any(|c| c.name.eq_ignore_ascii_case(&name)) {
            name = format!("{} copy {n}", source.name);
            n += 1;
        }
        let name: String = name.chars().take(chains::MAX_NAME_CHARS).collect();
        self.save_chain(Chain { id: 0, name, steps: source.steps })
    }

    pub fn delete_chain(&mut self, id: u32) -> bool {
        let before = self.chains.len();
        self.chains.retain(|c| c.id != id);
        let removed = self.chains.len() != before;
        if removed {
            self.prune();
        }
        removed
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b':')
}

/// A typed site as a bare host: no scheme, path, port or `www.`; lower case.
pub fn normalise_site(input: &str) -> Option<String> {
    let mut text = input.trim().to_ascii_lowercase();
    if let Some(at) = text.find("://") {
        text = text[at + 3..].to_owned();
    }
    let text = text.split(['/', '?', '#']).next().unwrap_or("");
    let text = text.rsplit('@').next().unwrap_or("");
    let text = text.split(':').next().unwrap_or("");
    let text = text.trim_end_matches('.');
    let text = text.strip_prefix("www.").unwrap_or(text);
    let valid = !text.is_empty()
        && text.len() <= 253
        && text.split('.').all(|label| {
            !label.is_empty() && label.len() <= 63 && !label.starts_with('-') && !label.ends_with('-') && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    valid.then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effective(store: &Store, id: &str) -> Vec<String> {
        let entries = store.entries();
        let entry = entries.iter().find(|e| e.id == id).unwrap();
        store.effective(entry).iter().filter_map(Chord::format).collect()
    }

    #[test]
    fn defaults_stand_until_changed_and_a_file_round_trips_everything() {
        let mut store = Store::default();
        assert_eq!(effective(&store, "close-tab"), ["Ctrl+W", "Ctrl+F4"]);
        store.assign("close-tab", "Ctrl+Shift+X", false).unwrap();
        store.clear("new-private-tab");
        store.options.shortcuts_enabled = false;
        store.options.palette_types = vec![ResultType::Command, ResultType::Tab];
        store.options.palette_max_results = 7;
        store.set_priority("history", true);
        store.add_site("https://www.Example.com/path").unwrap();
        let text = store.serialize_keys();
        let mut back = Store::default();
        back.parse_keys(&text);
        assert_eq!(back, store);
        assert!(text.contains("bind\tnew-private-tab\t\n"), "a cleared command keeps an empty list: {text}");
        assert_eq!(effective(&back, "new-private-tab"), Vec::<String>::new());
        assert_eq!(back.sites, ["example.com"]);
    }

    #[test]
    fn damaged_or_foreign_files_change_nothing_and_values_are_clamped() {
        let mut store = Store::default();
        store.parse_keys("not our header\nopt\tshortcuts_enabled\tfalse\n");
        assert_eq!(store, Store::default());
        store.parse_keys("bowser-keys-v1\nopt\tpalette_max_results\t200\nopt\tshortcuts_enabled\tmaybe\nopt\tunknown\t1\nbind\tBad Id\tCtrl+T\nbind\tnew-tab\tCtrl+Q;nonsense;Ctrl+Q\nsite\tbad host\nsite\texample.org\nfuture\tline\n");
        assert_eq!(store.options.palette_max_results, MAX_RESULTS);
        assert!(store.options.shortcuts_enabled, "a bad value keeps the old one");
        assert_eq!(effective(&store, "new-tab"), ["Ctrl+Q"], "unparseable and repeated chords are dropped");
        assert_eq!(store.sites, ["example.org"]);
        assert!(!store.binds.contains_key("Bad Id"));
        let mut huge = Store::default();
        huge.parse_keys(&format!("bowser-keys-v1\n{}", "site\texample.com\n".repeat(10_000)));
        assert_eq!(huge, Store::default(), "an oversized file is ignored");
    }

    #[test]
    fn several_shortcuts_per_command_clear_and_remove_one() {
        let mut store = Store::default();
        assert_eq!(store.assign("reload", "Ctrl+Shift+F", false), Ok(None));
        assert_eq!(effective(&store, "reload"), ["F5", "Ctrl+R", "BrowserRefresh", "Ctrl+Shift+F"]);
        assert!(store.remove_chord("reload", "Ctrl+R"));
        assert_eq!(effective(&store, "reload"), ["F5", "BrowserRefresh", "Ctrl+Shift+F"]);
        assert!(!store.remove_chord("reload", "Ctrl+R"), "already gone");
        assert!(store.clear("reload"));
        assert_eq!(effective(&store, "reload"), Vec::<String>::new());
        assert_eq!(store.assign("reload", "Ctrl+Shift+F", false), Ok(None), "assigning after a clear starts a new list");
        assert_eq!(effective(&store, "reload"), ["Ctrl+Shift+F"]);
        let mut many = Store::default();
        for chord in ["Ctrl+Shift+1", "Ctrl+Shift+2", "Ctrl+Shift+3", "Ctrl+Shift+4", "Ctrl+Shift+5", "Ctrl+Shift+6"] {
            let result = many.assign("history", chord, false);
            assert!(result.is_ok() || chord == "Ctrl+Shift+6", "{chord}: {result:?}");
        }
        assert_eq!(many.assign("history", "Ctrl+Shift+7", false), Err(AssignError::TooMany));
    }

    #[test]
    fn a_taken_chord_is_a_conflict_until_the_user_takes_it() {
        let mut store = Store::default();
        let error = store.assign("history", "Ctrl+J", false).unwrap_err();
        assert_eq!(error, AssignError::Conflict { owner: "downloads".into(), label: "Downloads".into() });
        assert_eq!(error.message("Ctrl+J"), "Ctrl+J is already used by Downloads.");
        assert_eq!(store.assign("history", "Ctrl+J", true).unwrap(), Some("downloads".into()));
        assert_eq!(effective(&store, "history"), ["Ctrl+H", "Ctrl+J"]);
        assert_eq!(effective(&store, "downloads"), Vec::<String>::new(), "the owner lost it");
        assert_eq!(store.binds.get("downloads"), Some(&Vec::new()));
    }

    #[test]
    fn refused_chords_unknown_commands_and_bad_text() {
        let mut store = Store::default();
        assert_eq!(store.assign("history", "Ctrl+C", false), Err(AssignError::Refused(Refusal::Reserved)));
        assert_eq!(store.assign("history", "Ctrl+Alt+K", false), Err(AssignError::Refused(Refusal::AltGr)));
        assert_eq!(store.assign("history", "K", false), Err(AssignError::Refused(Refusal::SingleKey)));
        assert_eq!(store.assign("nope", "Ctrl+K", false), Err(AssignError::UnknownCommand));
        assert_eq!(store.assign("history", "Ctrl+Banana", false), Err(AssignError::BadChord));
        store.options.single_key_shortcuts = true;
        assert_eq!(store.assign("history", "K", false), Ok(None), "single keys work once the option is on");
        assert_eq!(store.assign("reader", "Ctrl+Alt+R", false), Ok(None), "a default of a command is never refused");
    }

    #[test]
    fn restore_one_and_restore_all_bring_defaults_back_even_from_a_taker() {
        let mut store = Store::default();
        store.assign("history", "Ctrl+J", true).unwrap();
        store.assign("history", "Ctrl+Shift+Q", false).unwrap();
        store.set_priority("history", true);
        assert!(store.restore("downloads"));
        assert_eq!(effective(&store, "downloads"), ["Ctrl+J"], "restoring wins the chord back");
        assert_eq!(effective(&store, "history"), ["Ctrl+H", "Ctrl+Shift+Q"], "the taker keeps its other shortcuts");
        assert!(store.restore("history"));
        assert_eq!(effective(&store, "history"), ["Ctrl+H"]);
        assert!(store.binds.is_empty() && store.priority.is_empty(), "{store:?}");
        store.clear("reload");
        store.clear("back");
        store.set_priority("close-tab", false);
        store.options.alt_opens_menu = true;
        store.restore_all();
        assert!(store.binds.is_empty() && store.priority.is_empty());
        assert!(store.options.alt_opens_menu, "restore all is about shortcuts, not options");
    }

    #[test]
    fn no_two_commands_share_an_effective_chord_after_any_edit_sequence() {
        let mut store = Store::default();
        for (id, chord) in [("history", "Ctrl+J"), ("downloads", "Ctrl+H"), ("reload", "Ctrl+T"), ("new-tab", "F5"), ("stop", "Ctrl+W")] {
            let _ = store.assign(id, chord, true);
        }
        store.restore("history");
        let mut seen = std::collections::HashMap::new();
        for entry in store.entries() {
            for chord in store.effective(&entry) {
                assert_eq!(seen.insert(chord, entry.id.clone()), None, "{chord:?} is on two commands");
            }
        }
    }

    #[test]
    fn site_priority_matches_the_host_and_its_subdomains_only() {
        let mut store = Store::default();
        store.add_site("docs.example.com").unwrap();
        assert!(store.site_prioritised("docs.example.com"));
        assert!(store.site_prioritised("a.docs.example.com"));
        assert!(store.site_prioritised("WWW.docs.example.com"));
        assert!(!store.site_prioritised("example.com"));
        assert!(!store.site_prioritised("notdocs.example.com"));
        assert_eq!(store.add_site("docs.example.com"), Err("docs.example.com is already on the list.".into()));
        assert!(store.add_site("not a host").is_err());
        assert!(store.remove_site("docs.example.com"));
        assert!(!store.site_prioritised("docs.example.com"));
        for bad in ["", "a b", "-x.com", "x..com", "a_b.com", "http://"] {
            assert_eq!(normalise_site(bad), None, "{bad:?}");
        }
        assert_eq!(normalise_site("https://user@WWW.Example.com:8080/a?b#c").as_deref(), Some("example.com"));
    }

    #[test]
    fn chains_are_commands_with_their_own_bindings_that_vanish_with_them() {
        let mut store = Store::default();
        let id = store.save_chain(Chain { id: 0, name: "Morning".into(), steps: vec![chains::Step { command: "new-tab".into(), delay_ms: 0 }] }).unwrap();
        assert_eq!(id, 1);
        assert!(store.entries().iter().any(|e| e.id == "chain:1" && e.label == "Morning" && e.category == Category::Chains));
        store.assign("chain:1", "Ctrl+Shift+1", false).unwrap();
        assert_eq!(effective(&store, "chain:1"), ["Ctrl+Shift+1"]);
        let copy = store.clone_chain(1).unwrap();
        assert_eq!(store.chains.iter().find(|c| c.id == copy).unwrap().name, "Morning copy");
        assert_eq!(store.clone_chain(1).map(|n| store.chains.iter().find(|c| c.id == n).unwrap().name.clone()), Ok("Morning copy 2".into()));
        assert!(store.delete_chain(1));
        assert!(!store.binds.contains_key("chain:1"), "its shortcut went with it");
        let reused = store.save_chain(Chain { id: 0, name: "Fresh".into(), steps: vec![chains::Step { command: "reload".into(), delay_ms: 5 }] }).unwrap();
        assert!(reused > copy, "ids are never reused while a later chain exists");
        assert!(store.save_chain(Chain { id: 99, name: "Ghost".into(), steps: vec![] }).is_err());
        assert!(store.save_chain(Chain { id: 0, name: "Fresh".into(), steps: vec![chains::Step { command: "reload".into(), delay_ms: 0 }] }).is_err(), "names are unique");
    }

    #[test]
    fn a_hand_edited_max_chain_id_does_not_overflow_the_next_id() {
        let mut store = Store::default();
        store.chains = chains::parse(&format!("bowser-chains-v1\nchain\t{}\tEdge\nstep\t0\tnew-tab\n", u32::MAX));
        assert_eq!(store.chains.len(), 1);
        let id = store.save_chain(Chain { id: 0, name: "Next".into(), steps: vec![chains::Step { command: "reload".into(), delay_ms: 0 }] }).unwrap();
        assert_eq!(id, 1, "the lowest free id is taken instead of wrapping");
    }

    #[test]
    fn the_palette_is_ranked_only_while_the_enabled_types_keep_the_default_order() {
        let mut options = Options::default();
        assert!(options.palette_ranked());
        options.palette_types = vec![ResultType::Tab, ResultType::Command, ResultType::History];
        assert!(options.palette_ranked(), "hiding types keeps the default order");
        options.palette_types = vec![ResultType::Command, ResultType::Tab];
        assert!(!options.palette_ranked());
        assert_eq!(parse_types("history, bogus, tab, history"), [ResultType::History, ResultType::Tab]);
        assert_eq!(parse_types("bogus"), ResultType::DEFAULT_ORDER.to_vec(), "never empty");
        assert_eq!(types_text(&ResultType::DEFAULT_ORDER), "tab,closed,workspace,command,action,bookmark,note,history,calc");
    }

    #[test]
    fn files_are_written_and_read_back_from_disk() {
        let dir = std::env::temp_dir().join(format!("bowser-commands-store-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (keys, chains_file) = (dir.join("keybindings.txt"), dir.join("chains.txt"));
        assert_eq!(Store::load(&keys, &chains_file), Store::default(), "no files: defaults");
        let mut store = Store::default();
        store.assign("history", "Ctrl+Shift+Q", false).unwrap();
        store.save_chain(Chain { id: 0, name: "A".into(), steps: vec![chains::Step { command: "reload".into(), delay_ms: 10 }] }).unwrap();
        store.save(&keys, &chains_file).unwrap();
        assert_eq!(Store::load(&keys, &chains_file), store);
        let _ = fs::remove_dir_all(&dir);
    }

    fn files_in(name: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("bowser-commands-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (keys, chains_file) = (dir.join("keybindings.txt"), dir.join("chains.txt"));
        (dir, keys, chains_file)
    }

    fn morning() -> String {
        chains::serialize(&[Chain { id: 1, name: "Morning".into(), steps: vec![chains::Step { command: "reload".into(), delay_ms: 0 }] }])
    }

    /// R-7: Notepad's "UTF-8 with BOM" must not make every chain (and every shortcut bound to one)
    /// vanish on load and then be saved back empty by the next shortcut edit.
    #[test]
    fn files_saved_with_a_bom_load_and_survive_the_next_shortcut_edit() {
        let (dir, keys, chains_file) = files_in("bom");
        fs::write(&chains_file, format!("\u{feff}{}", morning())).unwrap();
        fs::write(&keys, format!("\u{feff}{HEADER}\nbind\tchain:1\tCtrl+Shift+M\n")).unwrap();
        let mut store = Store::load(&keys, &chains_file);
        assert_eq!(store.chains.len(), 1, "the chain is read");
        assert!(store.binds.contains_key("chain:1"), "its shortcut is kept");
        store.assign("history", "Ctrl+Shift+Q", false).unwrap();
        store.save(&keys, &chains_file).unwrap();
        let after = Store::load(&keys, &chains_file);
        assert_eq!(after.chains.len(), 1, "{:?}", fs::read_to_string(&chains_file));
        assert!(after.binds.contains_key("chain:1") && after.binds.contains_key("history"));
        let _ = fs::remove_dir_all(&dir);
    }

    /// R-7: a file that cannot be read (here one ANSI byte) is never replaced by what was loaded
    /// without it, including when it has become readable again by the time of the save.
    #[test]
    fn an_unreadable_chains_or_keys_file_is_never_saved_over() {
        let (dir, keys, chains_file) = files_in("unreadable");
        let ansi = [morning().as_bytes(), b"chain\t2\tCaf\xe9\n"].concat();
        fs::write(&chains_file, &ansi).unwrap();
        let store = Store::load(&keys, &chains_file);
        assert!(store.save(&keys, &chains_file).is_err(), "the save is refused");
        assert_eq!(fs::read(&chains_file).unwrap(), ansi, "chains.txt is left as it was");
        // Readable again (a lock released, the user fixed the byte): the loaded list is still empty.
        fs::write(&chains_file, morning()).unwrap();
        assert!(store.save(&keys, &chains_file).is_err());
        assert_eq!(fs::read_to_string(&chains_file).unwrap(), morning());

        let ansi_keys = [format!("{HEADER}\nbind\thistory\tCtrl+H\n").as_bytes(), b"site\tcaf\xe9.example\n"].concat();
        fs::write(&keys, &ansi_keys).unwrap();
        let store = Store::load(&keys, &chains_file);
        assert!(store.save(&keys, &chains_file).is_err());
        assert_eq!(fs::read(&keys).unwrap(), ansi_keys, "keybindings.txt is left as it was");
        let _ = fs::remove_dir_all(&dir);
    }
}
