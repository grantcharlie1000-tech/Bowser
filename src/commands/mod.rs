//! Commands: one registry that every command, shortcut, Quick Commands row, chain and (later) mouse
//! gesture and toolbar button uses. Documented in `docs/commands.md`.
//!
//! - [`registry`]: the table of built-in commands (id, label, category, default chords, palette flag,
//!   handler). Add a command by appending a row.
//! - [`chord`]: a keyboard chord as text and as a key code.
//! - [`store`]: what the user changed: shortcut assignments, options, chains. [`chains`] runs them.
//! - [`calc`]: the palette's calculator.
//!
//! This file holds the live state (the keymap the key handlers consult) and [`resolve`], which maps a
//! pressed key to the command it runs.

pub mod behaviour;
pub mod calc;
pub mod chains;
pub mod chord;
pub mod palette;
pub mod registry;
pub mod store;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};

use crate::shortcuts::{Command, GatedChords};
use chord::Chord;
use registry::{Gate, Handler, PaletteKind};
use store::{Entry, EntryHandler, Store};

/// A chord's binding in the live keymap.
#[derive(Clone, Debug)]
pub struct Binding {
    pub id: String,
    pub command: Command,
    pub gate: Option<Gate>,
    pub priority: bool,
}

/// Chord to command, built from the registry defaults and the user's assignments.
#[derive(Clone, Debug, Default)]
pub struct KeyMap {
    bindings: HashMap<Chord, Binding>,
}

impl KeyMap {
    pub fn build(store: &Store) -> Self {
        let entries = store.entries();
        let mut bindings: HashMap<Chord, Binding> = HashMap::new();
        // Commands the user has not touched first, so an explicit assignment wins any clash.
        let ordered = entries
            .iter()
            .filter(|entry| !store.binds.contains_key(&entry.id))
            .chain(entries.iter().filter(|entry| store.binds.contains_key(&entry.id)));
        for entry in ordered {
            let command = match entry.handler {
                EntryHandler::Builtin(Handler::Key(command)) => command,
                _ => Command::Registered(intern(&entry.id)),
            };
            for chord in store.effective(entry) {
                bindings.insert(
                    chord,
                    Binding { id: entry.id.clone(), command, gate: entry.gate, priority: store.priority_of(entry) },
                );
            }
        }
        Self { bindings }
    }

    pub fn lookup(&self, chord: Chord) -> Option<&Binding> {
        self.bindings.get(&chord)
    }
}

/// Everything the key handlers consult: the user's store and the keymap built from it.
pub struct Live {
    pub store: Store,
    pub keymap: KeyMap,
}

impl Live {
    pub fn new(store: Store) -> Self {
        let keymap = KeyMap::build(&store);
        Self { store, keymap }
    }
}

static LIVE: RwLock<Option<Arc<Live>>> = RwLock::new(None);

/// Set while the keyboard editor waits for the user to press a new shortcut: the settings page then gets
/// every key, so pressing Ctrl+T assigns it instead of opening a tab.
static CAPTURING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_capturing(on: bool) {
    CAPTURING.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn capturing() -> bool {
    CAPTURING.load(std::sync::atomic::Ordering::Relaxed)
}

/// The live state. Loaded from the active profile the first time it is asked for; defaults when
/// there is no profile or no files.
pub fn live() -> Arc<Live> {
    if let Ok(guard) = LIVE.read()
        && let Some(live) = guard.as_ref()
    {
        return live.clone();
    }
    // Unit tests never read the developer's own profile: they see the defaults.
    #[cfg(test)]
    let store = Store::default();
    #[cfg(not(test))]
    let store = crate::profile::active()
        .ok()
        .map(|profile| Store::load(&profile.keybindings_file(), &profile.chains_file()))
        .unwrap_or_default();
    install(store)
}

/// Makes `store` the live state (after an edit, or when a profile loads) and returns it.
pub fn install(store: Store) -> Arc<Live> {
    let live = Arc::new(Live::new(store));
    if let Ok(mut guard) = LIVE.write() {
        *guard = Some(live.clone());
    }
    live
}

/// Writes `store` to the active profile and makes it live. A guest profile keeps it in memory only.
pub fn save_and_install(store: Store) -> Result<Arc<Live>, String> {
    if let Ok(profile) = crate::profile::active()
        && !profile.is_guest()
    {
        store
            .save(&profile.keybindings_file(), &profile.chains_file())
            .map_err(|error| format!("Your shortcuts could not be saved: {error}"))?;
    }
    Ok(install(store))
}

/// Ids handed out for commands without a `shortcuts::Command` of their own, so a chord can carry a
/// `Command::Registered(handle)`. Append-only for the life of the process.
static HANDLES: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn intern(id: &str) -> u32 {
    let mut handles = HANDLES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(at) = handles.iter().position(|known| known == id) {
        return at as u32;
    }
    handles.push(id.to_owned());
    (handles.len() - 1) as u32
}

pub fn id_of(handle: u32) -> Option<String> {
    HANDLES.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(handle as usize).cloned()
}

/// Where a key press came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A web page's WebView: keys the page uses for typing must reach it.
    Page,
    /// The browser's own chrome.
    Chrome,
}

/// What the caller knows about the key press.
#[derive(Clone, Copy, Debug)]
pub struct ResolveContext<'a> {
    pub gated: GatedChords,
    pub source: Source,
    /// A text field in the chrome has the keyboard (the address bar, a search box).
    pub typing: bool,
    /// The host of the page in front, for the per-site shortcut priority.
    pub host: Option<&'a str>,
}

impl Default for ResolveContext<'_> {
    fn default() -> Self {
        Self { gated: GatedChords::default(), source: Source::Chrome, typing: false, host: None }
    }
}

/// The ids that keep working while shortcuts are switched off, so the user is never locked out.
const ALWAYS_ON: [&str; 1] = ["command-palette"];

/// The command a key press runs under `live`, or `None` when it belongs to the page or the text
/// field. The rules, in order: the chord is bound; shortcuts are on; the chord's gate holds; a
/// single typing key is allowed only while the option is on, never from a page and never while a
/// text field has the keyboard; on a site that prioritises its own shortcuts only the browser
/// priority list is claimed.
pub fn resolve_in(live: &Live, key: u32, ctrl: bool, shift: bool, alt: bool, cx: &ResolveContext) -> Option<Command> {
    let chord = Chord::new(key, ctrl, shift, alt);
    let binding = live.keymap.lookup(chord)?;
    if !live.store.options.shortcuts_enabled && !ALWAYS_ON.contains(&binding.id.as_str()) {
        return None;
    }
    if let Some(gate) = binding.gate {
        let allowed = match gate {
            Gate::CopyLink => cx.gated.copy_link,
            Gate::ToggleSidebar => cx.gated.toggle_sidebar,
            Gate::StackedSplit => cx.gated.stacked_split,
        };
        if !allowed {
            return None;
        }
    }
    if chord.is_single_key() && (!live.store.options.single_key_shortcuts || cx.source == Source::Page || cx.typing) {
        return None;
    }
    if cx.source == Source::Page
        && !binding.priority
        && cx.host.is_some_and(|host| live.store.site_prioritised(host))
    {
        return None;
    }
    Some(binding.command)
}

/// [`resolve_in`] with the live state.
pub fn resolve(key: u32, ctrl: bool, shift: bool, alt: bool, cx: &ResolveContext) -> Option<Command> {
    resolve_in(&live(), key, ctrl, shift, alt, cx)
}

/// A command row for the palette.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteCommand {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub kind: PaletteKind,
    /// The first of the command's chords, when it has one that is bound right now.
    pub accel: Option<String>,
}

/// Every command the palette lists, with its current first shortcut.
pub fn palette_commands(live: &Live) -> Vec<PaletteCommand> {
    live.store
        .entries()
        .into_iter()
        .filter(|entry| entry.palette != PaletteKind::Hidden)
        .map(|entry| {
            let accel = if entry.gate.is_some() || !live.store.options.shortcuts_enabled {
                None
            } else {
                live.store.effective(&entry).first().and_then(Chord::format)
            };
            PaletteCommand { id: entry.id, label: entry.label, detail: entry.detail, kind: entry.palette, accel }
        })
        .collect()
}

/// The entry for `id`, built-in or chain.
pub fn entry(live: &Live, id: &str) -> Option<Entry> {
    live.store.entries().into_iter().find(|entry| entry.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts;

    fn cx() -> ResolveContext<'static> {
        ResolveContext::default()
    }

    /// The default keymap must be the old fixed table, key for key: nothing moved, nothing lost.
    #[test]
    fn the_default_keymap_is_exactly_the_old_chord_table() {
        let live = Live::new(Store::default());
        let all = GatedChords::new(true, true, true).with_stacked_split(true);
        let context = ResolveContext { gated: all, ..cx() };
        for key in 0..=0xFF_u32 {
            for modifiers in 0..8_u8 {
                let (ctrl, shift, alt) = (modifiers & 1 != 0, modifiers & 2 != 0, modifiers & 4 != 0);
                let resolved = resolve_in(&live, key, ctrl, shift, alt, &context);
                // Ctrl+F1, the cheat sheet, and --- tab stacks (T10) --- Ctrl+F7, F8 and F9, the three
                // tilings, are the chords the registry adds.
                if [0x70, 0x76, 0x77, 0x78].contains(&key) && (ctrl, shift, alt) == (true, false, false) {
                    assert!(matches!(resolved, Some(Command::Registered(_))), "{resolved:?}");
                    continue;
                }
                assert_eq!(
                    resolved,
                    shortcuts::command_for_gated(key, ctrl, shift, alt, all),
                    "key {key:#04X} ctrl={ctrl} shift={shift} alt={alt}"
                );
            }
        }
    }

    #[test]
    fn gated_chords_are_unbound_until_their_condition_holds() {
        let live = Live::new(Store::default());
        for (key, ctrl, shift, gate_on) in [
            (0x43, true, true, GatedChords::new(true, false, false)),
            (0x53, true, false, GatedChords::new(false, true, true)),
            (0x26, true, true, GatedChords::default().with_stacked_split(true)),
        ] {
            assert_eq!(resolve_in(&live, key, ctrl, shift, false, &cx()), None, "{key:#x} off");
            let on = ResolveContext { gated: gate_on, ..cx() };
            assert!(resolve_in(&live, key, ctrl, shift, false, &on).is_some(), "{key:#x} on");
        }
    }

    #[test]
    fn an_assignment_changes_what_the_chord_runs_and_frees_the_old_one() {
        let mut store = Store::default();
        store.assign("close-tab", "Ctrl+Shift+X", false).unwrap();
        store.remove_chord("close-tab", "Ctrl+W");
        let live = Live::new(store);
        assert_eq!(resolve_in(&live, 0x58, true, true, false, &cx()), Some(Command::CloseTab));
        assert_eq!(resolve_in(&live, 0x57, true, false, false, &cx()), None, "Ctrl+W is no longer bound");
        assert_eq!(resolve_in(&live, 0x73, true, false, false, &cx()), Some(Command::CloseTab), "the other default stays");
    }

    /// Copy address is a registry command like any other, so another thread (T7: "add selected text to a
    /// note") takes Ctrl+Shift+C with one assignment: the chord moves, nothing else changes.
    #[test]
    fn ctrl_shift_c_is_a_registry_binding_that_moves_with_one_assignment() {
        let copy_on = ResolveContext { gated: GatedChords::new(true, false, false), ..cx() };
        let live = Live::new(Store::default());
        assert_eq!(resolve_in(&live, 0x43, true, true, false, &copy_on), Some(Command::CopyLink));
        assert_eq!(resolve_in(&live, 0x43, true, true, false, &cx()), None, "the setting off: the page keeps it");
        assert_eq!(registry::builtin("copy-link").map(|d| d.defaults), Some(&["Ctrl+Shift+C"][..]));
        let mut store = Store::default();
        assert_eq!(store.assign("add-bookmark", "Ctrl+Shift+C", true), Ok(Some("copy-link".into())));
        let moved = Live::new(store);
        assert_eq!(resolve_in(&moved, 0x43, true, true, false, &copy_on), Some(Command::AddBookmark), "the chord now runs the other command");
        assert_eq!(resolve_in(&moved, 0x44, true, false, false, &cx()), Some(Command::AddBookmark), "its own Ctrl+D is untouched");
        let copy = entry(&moved, "copy-link").expect("copy-link");
        assert!(moved.store.effective(&copy).is_empty(), "Copy address lost the chord and still exists as a command");
    }

    #[test]
    fn chains_and_new_commands_resolve_to_a_registered_handle_for_their_id() {
        let mut store = Store::default();
        let id = store
            .save_chain(chains::Chain { id: 0, name: "Go".into(), steps: vec![chains::Step { command: "new-tab".into(), delay_ms: 0 }] })
            .unwrap();
        store.assign(&format!("chain:{id}"), "Ctrl+Shift+1", false).unwrap();
        store.assign("shortcuts-cheat-sheet", "Ctrl+Shift+2", false).unwrap();
        let live = Live::new(store);
        let Some(Command::Registered(handle)) = resolve_in(&live, 0x31, true, true, false, &cx()) else { panic!("not registered") };
        assert_eq!(id_of(handle).as_deref(), Some(format!("chain:{id}").as_str()));
        let Some(Command::Registered(sheet)) = resolve_in(&live, 0x32, true, true, false, &cx()) else { panic!("not registered") };
        assert_eq!(id_of(sheet).as_deref(), Some("shortcuts-cheat-sheet"));
        assert_eq!(intern("shortcuts-cheat-sheet"), sheet, "an id keeps one handle");
        let default = resolve_in(&live, 0x70, true, false, false, &cx());
        assert!(matches!(default, Some(Command::Registered(_))), "Ctrl+F1 is the cheat sheet: {default:?}");
    }

    #[test]
    fn switching_shortcuts_off_leaves_only_the_palette_chord() {
        let mut store = Store::default();
        store.options.shortcuts_enabled = false;
        let live = Live::new(store);
        let mut bound = 0;
        for key in 0..=0xFF_u32 {
            for modifiers in 0..8_u8 {
                let (ctrl, shift, alt) = (modifiers & 1 != 0, modifiers & 2 != 0, modifiers & 4 != 0);
                let all = ResolveContext { gated: GatedChords::new(true, true, true).with_stacked_split(true), ..cx() };
                if let Some(command) = resolve_in(&live, key, ctrl, shift, alt, &all) {
                    assert_eq!(command, Command::ShowCommandPalette, "key {key:#x}");
                    bound += 1;
                }
            }
        }
        assert_eq!(bound, 1);
    }

    #[test]
    fn single_key_shortcuts_need_the_option_a_chrome_source_and_no_typing() {
        let mut store = Store::default();
        store.options.single_key_shortcuts = true;
        store.assign("reload", "R", false).unwrap();
        let on = Live::new(store.clone());
        assert_eq!(resolve_in(&on, 0x52, false, false, false, &cx()), Some(Command::Reload));
        let page = ResolveContext { source: Source::Page, ..cx() };
        assert_eq!(resolve_in(&on, 0x52, false, false, false, &page), None, "a page keeps its typing keys");
        let typing = ResolveContext { typing: true, ..cx() };
        assert_eq!(resolve_in(&on, 0x52, false, false, false, &typing), None, "a text field keeps its typing keys");
        assert_eq!(resolve_in(&on, 0x74, false, false, false, &page), Some(Command::Reload), "F5 is not a typing key");
        store.options.single_key_shortcuts = false;
        let off = Live::new(store);
        assert_eq!(resolve_in(&off, 0x52, false, false, false, &cx()), None, "the option off unbinds them");
    }

    #[test]
    fn a_site_that_prioritises_its_shortcuts_keeps_only_the_browser_priority_list() {
        let mut store = Store::default();
        store.add_site("docs.example.com").unwrap();
        let live = Live::new(store);
        let on_site = ResolveContext { source: Source::Page, host: Some("docs.example.com"), ..cx() };
        let elsewhere = ResolveContext { source: Source::Page, host: Some("other.example"), ..cx() };
        // Ctrl+T (new tab) is on the browser's priority list; Ctrl+H (history) is not.
        assert_eq!(resolve_in(&live, 0x54, true, false, false, &on_site), Some(Command::NewTab));
        assert_eq!(resolve_in(&live, 0x48, true, false, false, &on_site), None);
        assert_eq!(resolve_in(&live, 0x48, true, false, false, &elsewhere), Some(Command::ShowHistory));
        let chrome = ResolveContext { host: Some("docs.example.com"), ..cx() };
        assert_eq!(resolve_in(&live, 0x48, true, false, false, &chrome), Some(Command::ShowHistory), "the chrome is never the site's");
        let mut store = live.store.clone();
        store.set_priority("history", true);
        store.set_priority("new-tab", false);
        let changed = Live::new(store);
        assert_eq!(resolve_in(&changed, 0x48, true, false, false, &on_site), Some(Command::ShowHistory));
        assert_eq!(resolve_in(&changed, 0x54, true, false, false, &on_site), None);
    }

    #[test]
    fn palette_commands_list_the_searchable_ones_with_live_shortcuts() {
        let mut store = Store::default();
        store.assign("history", "Ctrl+Shift+Q", true).unwrap();
        store.clear("new-tab");
        let live = Live::new(store);
        let commands = palette_commands(&live);
        let find = |id: &str| commands.iter().find(|c| c.id == id).cloned();
        assert_eq!(find("history").unwrap().accel.as_deref(), Some("Ctrl+H"));
        assert_eq!(find("new-tab").unwrap().accel, None, "a cleared command shows no shortcut");
        assert!(find("select-tab-1").is_none() && find("command-palette").is_none(), "hidden commands are not listed");
        assert_eq!(find("copy-link").unwrap().accel, None, "a gated chord is not advertised");
        assert_eq!(find("reader").unwrap().kind, PaletteKind::Action);
        assert_eq!(find("reload").unwrap().accel.as_deref(), Some("F5"));
        assert_eq!(commands[0].id, "new-tab");
        let mut off = live.store.clone();
        off.options.shortcuts_enabled = false;
        assert!(palette_commands(&Live::new(off)).iter().all(|c| c.accel.is_none()));
    }
}
