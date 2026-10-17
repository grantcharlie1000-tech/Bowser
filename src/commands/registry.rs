//! The command registry: the one table every command, shortcut, chain, palette row and (later)
//! mouse gesture and toolbar button reads. See `docs/commands.md`.
//!
//! To add a command, append one [`CommandDef`] to [`BUILTINS`]: an id, a label, a category, the default
//! chords (`""`-free text such as `"Ctrl+Shift+K"`), whether the palette lists it, and a handler.
//! Nothing else needs to know about it: the keyboard editor, the cheat sheet, the palette, chains and
//! `run_command` in `win32/command_actions.rs` all iterate this table.

use windows::Win32::Foundation::HWND;

use crate::{app::AppRef, shortcuts::Command};

/// Where a handler runs: the window that asked and its app.
pub struct Context<'a> {
    pub hwnd: HWND,
    pub app: &'a AppRef,
}

/// How a command runs.
#[derive(Clone, Copy)]
pub enum Handler {
    /// A command the window's key dispatcher already runs (`win32::window::dispatch`).
    Key(Command),
    /// An action string the palette's runner handles (`run_palette_command`).
    Action(&'static str),
    /// A self-contained function. New commands use this: no window code needs to change.
    Run(fn(&Context)),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Tabs,
    Navigation,
    Page,
    Windows,
    Panels,
    Privacy,
    Bookmarks,
    Tools,
    Keyboard,
    Chains,
}

impl Category {
    pub const ALL: [Self; 10] = [
        Self::Tabs,
        Self::Navigation,
        Self::Page,
        Self::Windows,
        Self::Panels,
        Self::Bookmarks,
        Self::Privacy,
        Self::Tools,
        Self::Keyboard,
        Self::Chains,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Tabs => "Tabs",
            Self::Navigation => "Navigation",
            Self::Page => "Page",
            Self::Windows => "Windows and split view",
            Self::Panels => "Panels and pages",
            Self::Privacy => "Privacy",
            Self::Bookmarks => "Bookmarks",
            Self::Tools => "Tools",
            Self::Keyboard => "Keyboard and commands",
            Self::Chains => "Command chains",
        }
    }
}

/// Whether and how the palette lists a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteKind {
    /// Not searchable in the palette (still assignable and chainable).
    Hidden,
    /// Listed under Commands.
    Command,
    /// Listed under Page actions: it acts on the page in front.
    Action,
}

/// A chord that is bound only while a live condition holds (a setting, a layout, a split).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// `ctrl_shift_c_copy_link`.
    CopyLink,
    /// `ctrl_s_vertical_tabs` and the vertical layout.
    ToggleSidebar,
    /// A split with panes above one another is shown.
    StackedSplit,
}

#[derive(Clone, Copy)]
pub struct CommandDef {
    pub id: &'static str,
    pub label: &'static str,
    pub detail: &'static str,
    pub category: Category,
    /// Default chords, in the text form `Chord::parse` reads.
    pub defaults: &'static [&'static str],
    pub palette: PaletteKind,
    pub gate: Option<Gate>,
    pub handler: Handler,
    /// Stays the browser's on a site that prioritises its own shortcuts.
    pub priority: bool,
}

const fn def(
    id: &'static str,
    label: &'static str,
    detail: &'static str,
    category: Category,
    defaults: &'static [&'static str],
    palette: PaletteKind,
    handler: Handler,
) -> CommandDef {
    CommandDef { id, label, detail, category, defaults, palette, gate: None, handler, priority: false }
}

const fn gated(mut command: CommandDef, gate: Gate) -> CommandDef {
    command.gate = Some(gate);
    command
}

/// Marks a command as one the browser keeps on sites that prioritise their own shortcuts.
const fn browser(mut command: CommandDef) -> CommandDef {
    command.priority = true;
    command
}

use Category as C;
use Handler::{Action, Key, Run};
use PaletteKind::{Action as Page, Command as Cmd, Hidden};

/// Every built-in command. The order is the palette's order for an empty search and the keyboard
/// editor's order inside a category. Append new commands; do not reuse or rename an id (a saved
/// binding, chain or gesture names it).
pub static BUILTINS: &[CommandDef] = &[
    browser(def("new-tab", "New tab", "Open a blank tab", C::Tabs, &["Ctrl+T"], Cmd, Key(Command::NewTab))),
    browser(def("new-private-tab", "New private tab", "Browse without saving history, cookies or site data", C::Tabs, &["Ctrl+Shift+P"], Cmd, Key(Command::NewPrivateTab))),
    browser(def("close-tab", "Close tab", "Close the current tab", C::Tabs, &["Ctrl+W", "Ctrl+F4"], Cmd, Key(Command::CloseTab))),
    def("close-private-tabs", "Close all private tabs", "Close every private tab in this window", C::Tabs, &[], Cmd, Action("close-private-tabs")),
    def("settings", "Settings", "Open browser settings", C::Panels, &[], Cmd, Action("settings")),
    def("toggle-notes", "Toggle notes and tasks", "Show or hide the productivity sidebar", C::Panels, &["Ctrl+Shift+N"], Cmd, Key(Command::ToggleNotes)),
    def("passwords", "Passwords and passkeys", "Manage saved credentials", C::Panels, &["Ctrl+,"], Cmd, Key(Command::OpenPasswords)),
    def("profiles", "Profiles", "Switch or manage profiles", C::Windows, &["Ctrl+Shift+M"], Cmd, Key(Command::ShowProfileMenu)),
    def("save-session", "Save session", "Save the open tabs as a named session", C::Tabs, &[], Cmd, Action("save-session")),
    def("sessions", "Saved sessions", "Restore or delete a saved session", C::Tabs, &[], Cmd, Action("sessions")),
    def("add-bookmark", "Add bookmark", "Save the current page", C::Bookmarks, &["Ctrl+D"], Page, Key(Command::AddBookmark)),
    def("reload", "Reload", "Reload the current page", C::Navigation, &["F5", "Ctrl+R", "BrowserRefresh"], Page, Key(Command::Reload)),
    def("back", "Back", "Go back", C::Navigation, &["Alt+Left", "BrowserBack"], Cmd, Key(Command::Back)),
    def("forward", "Forward", "Go forward", C::Navigation, &["Alt+Right", "BrowserForward"], Cmd, Key(Command::Forward)),
    def("extensions", "Extensions", "Manage installed extensions", C::Panels, &["Ctrl+Shift+E"], Cmd, Key(Command::ShowExtensionsMenu)),
    def("reader", "Reader view", "Read this page without the clutter", C::Page, &["Ctrl+Alt+R"], Page, Key(Command::ToggleReader)),
    def("calm", "Hide distractions", "Hide overlays, cookie banners and sticky bars", C::Page, &[], Page, Action("calm")),
    def("translate", "Translate page", "Translate this page with Google Translate, or show the original", C::Page, &[], Page, Action("translate")),
    // Everything below was only reachable by its chord before the registry.
    browser(def("next-tab", "Next tab", "Switch to the tab on the right", C::Tabs, &["Ctrl+Tab", "Ctrl+PageDown"], Cmd, Key(Command::NextTab))),
    browser(def("prev-tab", "Previous tab", "Switch to the tab on the left", C::Tabs, &["Ctrl+Shift+Tab", "Ctrl+PageUp"], Cmd, Key(Command::PrevTab))),
    def("select-tab-1", "Select tab 1", "Switch to the first tab", C::Tabs, &["Ctrl+1"], Hidden, Key(Command::SelectTab(0))),
    def("select-tab-2", "Select tab 2", "Switch to the second tab", C::Tabs, &["Ctrl+2"], Hidden, Key(Command::SelectTab(1))),
    def("select-tab-3", "Select tab 3", "Switch to the third tab", C::Tabs, &["Ctrl+3"], Hidden, Key(Command::SelectTab(2))),
    def("select-tab-4", "Select tab 4", "Switch to the fourth tab", C::Tabs, &["Ctrl+4"], Hidden, Key(Command::SelectTab(3))),
    def("select-tab-5", "Select tab 5", "Switch to the fifth tab", C::Tabs, &["Ctrl+5"], Hidden, Key(Command::SelectTab(4))),
    def("select-tab-6", "Select tab 6", "Switch to the sixth tab", C::Tabs, &["Ctrl+6"], Hidden, Key(Command::SelectTab(5))),
    def("select-tab-7", "Select tab 7", "Switch to the seventh tab", C::Tabs, &["Ctrl+7"], Hidden, Key(Command::SelectTab(6))),
    def("select-tab-8", "Select tab 8", "Switch to the eighth tab", C::Tabs, &["Ctrl+8"], Hidden, Key(Command::SelectTab(7))),
    def("select-last-tab", "Select last tab", "Switch to the last tab", C::Tabs, &["Ctrl+9"], Hidden, Key(Command::SelectLastTab)),
    browser(def("focus-address-bar", "Focus the address bar", "Move the keyboard to the address bar", C::Navigation, &["Ctrl+L", "Alt+D", "F6"], Cmd, Key(Command::FocusOmnibox))),
    def("hard-reload", "Reload without cache", "Stop and reload the current page", C::Navigation, &["Ctrl+F5", "Ctrl+Shift+R"], Page, Key(Command::HardReload)),
    def("stop", "Stop loading", "Stop the page that is loading", C::Navigation, &["Esc", "BrowserStop"], Cmd, Key(Command::Stop)),
    def("fill-password", "Fill saved password", "Offer saved credentials for this site", C::Page, &["Ctrl+Shift+L"], Page, Key(Command::FillPassword)),
    def("privacy-dashboard", "Privacy dashboard", "Clear browsing data and review permissions", C::Privacy, &["Ctrl+Shift+Delete"], Cmd, Key(Command::OpenPrivacyDashboard)),
    def("toggle-blocker", "Toggle blocker for this site", "Turn the ad and tracker blocker off or on here", C::Privacy, &["Ctrl+Shift+B"], Page, Key(Command::ToggleBlocker)),
    def("undo-tab-organisation", "Undo tab organisation", "Undo the last tab group or placement change", C::Tabs, &["Ctrl+Shift+Z"], Cmd, Key(Command::UndoTabOrganisation)),
    def("picture-in-picture", "Picture in picture", "Float the page's video above other windows", C::Page, &["Ctrl+Shift+Y"], Page, Key(Command::TogglePictureInPicture)),
    def("toggle-split", "Toggle split view", "Show two tabs side by side, or go back to one", C::Windows, &["Ctrl+Shift+\\"], Cmd, Key(Command::ToggleSplit)),
    def("focus-split-left", "Focus the left split pane", "Move to the pane on the left", C::Windows, &["Ctrl+Shift+Left"], Cmd, Key(Command::FocusSplitLeft)),
    def("focus-split-right", "Focus the right split pane", "Move to the pane on the right", C::Windows, &["Ctrl+Shift+Right"], Cmd, Key(Command::FocusSplitRight)),
    gated(def("focus-split-up", "Focus the pane above", "Move to the pane above in a stacked split", C::Windows, &["Ctrl+Shift+Up"], Cmd, Key(Command::FocusSplitUp)), Gate::StackedSplit),
    gated(def("focus-split-down", "Focus the pane below", "Move to the pane below in a stacked split", C::Windows, &["Ctrl+Shift+Down"], Cmd, Key(Command::FocusSplitDown)), Gate::StackedSplit),
    def("close-split-pane", "Close the split pane", "Close the focused pane of the split", C::Windows, &["Ctrl+Shift+W"], Cmd, Key(Command::CloseSplitPane)),
    def("command-palette", "Quick commands", "Search tabs, bookmarks, history, notes and commands", C::Tools, &["Ctrl+Shift+K"], Hidden, Key(Command::ShowCommandPalette)),
    def("history", "History", "Open the history page", C::Panels, &["Ctrl+H"], Cmd, Key(Command::ShowHistory)),
    def("downloads", "Downloads", "Open the downloads page", C::Panels, &["Ctrl+J"], Cmd, Key(Command::ShowDownloads)),
    def("zoom-in", "Zoom in", "Make the page larger", C::Page, &["Ctrl+=", "Ctrl+Shift+=", "Ctrl+Numpad+"], Page, Key(Command::ZoomIn)),
    def("zoom-out", "Zoom out", "Make the page smaller", C::Page, &["Ctrl+-", "Ctrl+Numpad-"], Page, Key(Command::ZoomOut)),
    def("zoom-reset", "Reset zoom", "Return the page to 100%", C::Page, &["Ctrl+0", "Ctrl+Numpad0"], Page, Key(Command::ZoomReset)),
    def("devtools", "Developer tools", "Open or close the developer tools for this tab", C::Tools, &["F12", "Ctrl+Shift+I"], Page, Key(Command::OpenDevTools)),
    browser(def("new-window", "New window", "Open another window for this profile", C::Windows, &["Ctrl+N"], Cmd, Key(Command::NewWindow))),
    // Ctrl+Shift+C stays Copy address, gated by its setting; T7's "note-from-selection" uses Ctrl+Shift+O instead.
    gated(def("copy-link", "Copy address", "Copy the current page's address", C::Page, &["Ctrl+Shift+C"], Page, Key(Command::CopyLink)), Gate::CopyLink),
    gated(def("toggle-sidebar", "Collapse or expand the tab sidebar", "Vertical layout only", C::Tabs, &["Ctrl+S"], Cmd, Key(Command::ToggleSidebar)), Gate::ToggleSidebar),
    // Keyboard and commands (new with the registry).
    def("shortcuts-cheat-sheet", "Keyboard shortcuts", "Show every shortcut", C::Keyboard, &["Ctrl+F1"], Cmd, Run(crate::win32::run_cheat_sheet)),
    def("toggle-site-shortcuts", "Prioritise website shortcuts on this site", "On this site, keys the page uses go to the page; the browser keeps its priority list", C::Keyboard, &[], Page, Run(crate::win32::run_toggle_site_shortcuts)),
    def("keyboard-settings", "Keyboard settings", "Assign shortcuts and edit command chains", C::Keyboard, &[], Cmd, Run(crate::win32::run_keyboard_settings)),
    // --- tab options (T1): closed tabs and sessions; run by `run_key_command` / `run_palette_command` ---
    browser(def("reopen-tab", "Reopen closed tab", "Bring back the tab you closed last", C::Tabs, &["Ctrl+Shift+T"], Cmd, Key(Command::ReopenClosedTab))),
    def("closed-tabs", "Closed tabs", "List the tabs closed in this session", C::Tabs, &[], Cmd, Action("closed-tabs")),
    def("save-all-session", "Save All Tabs as Session", "Save every open tab as a named session", C::Tabs, &[], Cmd, Action("save-all-session")),
    def("open-session", "Open Saved Session", "Open a saved session in this window", C::Tabs, &[], Cmd, Action("open-session")),
    // --- end tab options ---
    // --- wave-1 integration: T1 periodic reload, T3 privacy tools, T2 address bar (win32/command_actions.rs) ---
    def("periodic-reload", "Reload this tab every…", "Reload the page in front on a timer, or stop", C::Tabs, &[], Page, Run(crate::win32::run_periodic_reload)),
    def("stop-all-reloads", "Stop reloading all tabs", "Stop every periodic reload", C::Tabs, &[], Cmd, Run(crate::win32::run_stop_all_reloads)),
    def("delete-browsing-data", "Delete browsing data", "Delete history, cookies, cache and more for a period you choose", C::Privacy, &[], Cmd, Run(crate::win32::run_delete_browsing_data)),
    def("cycle-blocking-level", "Next blocking level for this site", "None, trackers, or trackers and ads for the site in front", C::Privacy, &[], Page, Run(crate::win32::run_cycle_blocking_level)),
    def("qr-code", "QR code for this page", "Show the page's address as a QR code", C::Page, &[], Page, Run(crate::win32::run_qr_code)),
    def("toggle-address-bar", "Show or hide the address bar", "Turn Settings > Address bar > Show the address bar on or off", C::Navigation, &[], Cmd, Run(crate::win32::run_toggle_address_bar)),
    // --- end wave-1 integration ---
    // --- page tools (T7): capture, and notes made from the page (win32/page_tools_ui.rs) ---
    def("capture-visible", "Capture visible area", "Save what is on screen as a picture", C::Tools, &["Ctrl+Shift+S"], Page, Run(crate::win32::run_capture_visible)),
    def("capture-full", "Capture full page", "Save the whole page, up to 30000 pixels tall, as one picture", C::Tools, &[], Page, Run(crate::win32::run_capture_full)),
    def("capture-selection", "Capture selection", "Save the selected part of the page as a picture", C::Tools, &[], Page, Run(crate::win32::run_capture_selection)),
    def("capture-visible-copy", "Copy capture of visible area", "Copy what is on screen to the clipboard as a picture", C::Tools, &[], Page, Run(crate::win32::run_capture_visible_copy)),
    def("capture-full-copy", "Copy capture of full page", "Copy the whole page to the clipboard as a picture", C::Tools, &[], Page, Run(crate::win32::run_capture_full_copy)),
    def("capture-selection-copy", "Copy capture of selection", "Copy the selected part of the page to the clipboard as a picture", C::Tools, &[], Page, Run(crate::win32::run_capture_selection_copy)),
    def("capture-visible-note", "Capture visible area to a note", "Save what is on screen and add a note that points at it", C::Tools, &[], Page, Run(crate::win32::run_capture_visible_note)),
    def("capture-full-note", "Capture full page to a note", "Save the whole page and add a note that points at it", C::Tools, &[], Page, Run(crate::win32::run_capture_full_note)),
    def("capture-selection-note", "Capture selection to a note", "Save the selected part and add a note that points at it", C::Tools, &[], Page, Run(crate::win32::run_capture_selection_note)),
    // Ctrl+Shift+C stays "Copy address" (its own setting row decides); this chord is free and can be moved to it in Keyboard settings.
    def("note-from-selection", "Add selection to a new note", "Make a note from the text selected on the page", C::Tools, &["Ctrl+Shift+O"], Page, Run(crate::win32::run_note_from_selection)),
    def("note-append-selection", "Add selection to an existing note", "Pick a note and add the selected text to its end", C::Tools, &[], Page, Run(crate::win32::run_note_append)),
    // --- end page tools ---
    // --- auto-hide (T11): src/auto_hide.rs, win32/window/auto_hide.rs ---
    def("toggle-auto-hide", "Toggle auto-hide UI", "Hide the toolbars and tabs until the pointer rests at the window edge, or show them again", C::Windows, &["Ctrl+F11"], Cmd, Run(crate::win32::run_toggle_auto_hide)),
    browser(def("toggle-fullscreen", "Full screen", "Fill the screen with the page; the bars come back when the pointer rests at the edge", C::Windows, &["F11"], Cmd, Run(crate::win32::run_toggle_fullscreen))),
    // --- end auto-hide ---
    // --- tab stacks, tiling and workspaces (T10; win32/stack_ui.rs) ---
    def("stack-site-tabs", "Stack tabs of this site", "Put every tab of this site in one stack", C::Tabs, &[], Cmd, Run(crate::win32::run_stack_site)),
    def("stack-tabs-by-site", "Stack all tabs by site", "One stack for each site with two tabs or more", C::Tabs, &[], Cmd, Run(crate::win32::run_stack_all_by_site)),
    def("unstack", "Unstack", "Take the tabs of this stack out of it", C::Tabs, &[], Cmd, Run(crate::win32::run_unstack)),
    browser(def("tile-vertical", "Tile vertically", "Show the selected tabs, or this stack, side by side", C::Windows, &["Ctrl+F9"], Cmd, Run(crate::win32::run_tile_vertical))),
    browser(def("tile-horizontal", "Tile horizontally", "Show the selected tabs, or this stack, one above another", C::Windows, &["Ctrl+F8"], Cmd, Run(crate::win32::run_tile_horizontal))),
    browser(def("tile-grid", "Tile in a grid", "Show the selected tabs, or this stack, two by two", C::Windows, &["Ctrl+F7"], Cmd, Run(crate::win32::run_tile_grid))),
    def("untile", "Untile", "Show the tiled tabs one at a time again", C::Windows, &[], Cmd, Run(crate::win32::run_untile)),
    def("next-workspace", "Next workspace", "Switch to the workspace below this one in the rail", C::Windows, &[], Cmd, Run(crate::win32::run_next_workspace)),
    def("previous-workspace", "Previous workspace", "Switch to the workspace above this one in the rail", C::Windows, &[], Cmd, Run(crate::win32::run_previous_workspace)),
    // --- end tab stacks, tiling and workspaces ---
    // --- toolbars and feeds (T13): the Feeds panel; also the panel bar's Feeds button and a command button's target ---
    def("feeds", "Feeds", "Read the feeds you subscribed to, and subscribe to this page's feed", C::Panels, &[], Cmd, Run(crate::win32::run_open_feeds)),
    def("subscribe-feed", "Subscribe to this page's feed", "Open Feeds with the feeds this page offers, to preview and subscribe", C::Page, &[], Page, Run(crate::win32::run_open_feeds)),
    // --- end toolbars and feeds ---
];

/// The command with this id, if it is a built-in.
pub fn builtin(id: &str) -> Option<&'static CommandDef> {
    BUILTINS.iter().find(|def| def.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::chord::Chord;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_lower_case_words_and_labels_are_not_empty() {
        let mut seen = HashSet::new();
        for def in BUILTINS {
            assert!(seen.insert(def.id), "duplicate id {}", def.id);
            assert!(
                def.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                    && !def.id.starts_with("chain-")
                    && def.id.len() <= 32,
                "{}",
                def.id
            );
            assert!(!def.label.is_empty() && !def.detail.is_empty(), "{}", def.id);
        }
    }

    #[test]
    fn every_default_chord_parses_prints_the_same_and_belongs_to_one_command() {
        let mut owner = std::collections::HashMap::new();
        for def in BUILTINS {
            for text in def.defaults {
                let chord = Chord::parse(text).unwrap_or_else(|| panic!("{} default {text}", def.id));
                assert_eq!(chord.format().as_deref(), Some(*text), "{} writes its chords canonically", def.id);
                assert_eq!(owner.insert(chord, def.id), None, "{text} is the default of two commands");
            }
        }
    }

    #[test]
    fn the_registry_lists_exactly_the_commands_the_old_palette_did_first_and_in_order() {
        let first: Vec<_> = BUILTINS.iter().take(18).map(|def| def.label).collect();
        assert_eq!(
            first,
            [
                "New tab", "New private tab", "Close tab", "Close all private tabs", "Settings",
                "Toggle notes and tasks", "Passwords and passkeys", "Profiles", "Save session", "Saved sessions",
                "Add bookmark", "Reload", "Back", "Forward", "Extensions", "Reader view", "Hide distractions",
                "Translate page"
            ]
        );
    }

    #[test]
    fn only_the_gated_commands_carry_a_gate() {
        let gated: Vec<_> = BUILTINS.iter().filter(|d| d.gate.is_some()).map(|d| d.id).collect();
        assert_eq!(gated, ["focus-split-up", "focus-split-down", "copy-link", "toggle-sidebar"]);
    }
}
