//! Toolbar options (thread T13): which buttons the address bar, the tab bar and the panel bar show, and
//! in what order. Three lists in `settings.txt`, each a comma-separated list of item ids:
//!
//! ```text
//! toolbar_buttons=back,forward,reload,site-info,split,reader,extensions,downloads   (the address bar)
//! toolbar_tabs=closed-tabs                                                           (the tab bar)
//! toolbar_panel=                                                                     (the panel bar)
//! feeds_detect=true
//! feeds_refresh_minutes=60
//! ```
//!
//! An item is one of a bar's own buttons (a fixed id) or a command button: `cmd:<command id>` for any
//! built-in command and `chain:<n>` for a command chain. A command button runs through
//! `win32::window::run_registered`, the same entry point as Quick Commands and shortcuts, so a button
//! can never do something a command cannot. Menu (the three dots) is not an item: it is always there.
//!
//! This module is pure: it checks and orders ids. The toolbar page draws them (`ui/src/view.ts`), the
//! tab strip too (`tabs.ts`), the rail builds its extra buttons from them (`win32/window.rs`), and the
//! settings page edits the lists (`ui/src/toolbar-editor-logic.ts`). Docs: `docs/toolbars.md`.

use crate::settings::Settings;

/// The three bars the editor lists. (Bowser has no status bar, so there is nothing to edit there.)
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bar {
    Address,
    Tabs,
    Panel,
}

/// The ids the address bar's own buttons go by, in the order the editor offers them.
pub const ADDRESS_ITEMS: [&str; 10] =
    ["back", "forward", "reload", "site-info", "split", "reader", "bookmark", "newtab", "downloads", "extensions"];
/// What the address bar shows until the user changes it: today's look (bookmark and new tab are added by choice).
pub const DEFAULT_ADDRESS: [&str; 8] = ["back", "forward", "reload", "site-info", "split", "reader", "extensions", "downloads"];
/// The list an older Bowser saved when the toolbar editor did nothing. It is read as the default,
/// so nobody who never touched the row gets two new buttons.
pub const LEGACY_DEFAULT_ADDRESS: [&str; 8] = ["back", "forward", "reload", "site-info", "bookmark", "newtab", "split", "reader"];
pub const TAB_ITEMS: [&str; 1] = ["closed-tabs"];
pub const PANEL_ITEMS: [&str; 1] = ["feeds"];

/// Most items in one bar.
pub const MAX_ITEMS: usize = 14;
/// Most command buttons in one bar.
pub const MAX_COMMAND_ITEMS: usize = 4;
/// Most chains: the same cap as `commands::chains` (64) bounds the id.
const MAX_CHAIN_NUMBER: u32 = 9999;

impl Bar {
    pub const ALL: [Bar; 3] = [Self::Address, Self::Tabs, Self::Panel];

    /// The `settings.txt` key of the bar's list.
    pub fn key(self) -> &'static str {
        match self {
            Self::Address => "toolbar_buttons",
            Self::Tabs => "toolbar_tabs",
            Self::Panel => "toolbar_panel",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|bar| bar.key() == key)
    }

    /// The bar's own buttons, by id.
    pub fn fixed(self) -> &'static [&'static str] {
        match self {
            Self::Address => &ADDRESS_ITEMS,
            Self::Tabs => &TAB_ITEMS,
            Self::Panel => &PANEL_ITEMS,
        }
    }

    pub fn default_list(self) -> Vec<String> {
        match self {
            Self::Address => DEFAULT_ADDRESS.iter().map(|&id| id.to_owned()).collect(),
            Self::Tabs => TAB_ITEMS.iter().map(|&id| id.to_owned()).collect(),
            Self::Panel => Vec::new(),
        }
    }

    pub fn list(self, settings: &Settings) -> &[String] {
        match self {
            Self::Address => &settings.toolbar_buttons,
            Self::Tabs => &settings.toolbar_tabs,
            Self::Panel => &settings.toolbar_panel,
        }
    }

    fn set(self, settings: &mut Settings, list: Vec<String>) {
        match self {
            Self::Address => settings.toolbar_buttons = list,
            Self::Tabs => settings.toolbar_tabs = list,
            Self::Panel => settings.toolbar_panel = list,
        }
    }
}

/// `cmd:<built-in command id>` or `chain:<n>`.
pub fn is_command_item(id: &str) -> bool {
    if let Some(command) = id.strip_prefix("cmd:") {
        return crate::commands::registry::builtin(command).is_some();
    }
    id.strip_prefix("chain:").is_some_and(|number| {
        !number.starts_with('0') && number.len() <= 4 && number.parse::<u32>().is_ok_and(|n| (1..=MAX_CHAIN_NUMBER).contains(&n))
    })
}

/// The registry command id a command button runs (`chain:3` is itself the id), or `None` for a
/// button of the bar's own.
pub fn command_of(item: &str) -> Option<&str> {
    if item.starts_with("chain:") && is_command_item(item) {
        Some(item)
    } else {
        item.strip_prefix("cmd:").filter(|_| is_command_item(item))
    }
}

/// Parses a bar's list: each item one of the bar's fixed ids or a command button, no repeats, at most
/// [`MAX_ITEMS`] items and [`MAX_COMMAND_ITEMS`] command buttons, and (for the address bar) at most
/// [`MAX_RIGHT_STATIC`] buttons that share the end of the bar with the menu. `None` for anything else.
pub fn parse_list(bar: Bar, value: &str) -> Option<Vec<String>> {
    let mut seen = std::collections::HashSet::new();
    let mut list = Vec::new();
    for raw in value.split(',') {
        let id = raw.trim();
        if id.is_empty() {
            continue;
        }
        if !(bar.fixed().contains(&id) || is_command_item(id)) || !seen.insert(id) {
            return None;
        }
        list.push(id.to_owned());
    }
    if list.len() > MAX_ITEMS || list.iter().filter(|id| command_of(id).is_some()).count() > MAX_COMMAND_ITEMS {
        return None;
    }
    if bar == Bar::Address && right_static(&list) > MAX_RIGHT_STATIC {
        return None;
    }
    if bar == Bar::Address && list.as_slice() == LEGACY_DEFAULT_ADDRESS {
        return Some(Bar::Address.default_list());
    }
    Some(list)
}

/// [`parse_list`] for a value read back from settings.txt: a command button whose command no longer
/// exists (a hand-edited file, or a command retired since) is dropped on its own, so the rest of the
/// list is kept. What the settings page sends is still checked whole by [`parse_list`].
pub fn parse_stored_list(bar: Bar, value: &str) -> Option<Vec<String>> {
    let kept: Vec<&str> = value.split(',').map(str::trim).filter(|id| !(id.starts_with("cmd:") && !is_command_item(id))).collect();
    parse_list(bar, &kept.join(","))
}

/// Sets the list of the bar whose key is `key` from a settings.txt value with
/// [`parse_stored_list`]. `false` and nothing changed for another key or a bad list.
pub fn apply_stored_list(settings: &mut Settings, key: &str, value: &str) -> bool {
    let Some(list) = Bar::from_key(key).and_then(|bar| parse_stored_list(bar, value).map(|list| (bar, list))) else { return false };
    list.0.set(settings, list.1);
    true
}

// ---- the address bar's zones ----

/// Where on the address bar an item sits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Zone {
    /// Before the address field: back, forward, reload.
    Left,
    /// Inside the address field: the site button on its left, the others on its right.
    Pill,
    /// After the address field, before the menu button.
    Right,
}

pub fn zone_of(id: &str) -> Zone {
    match id {
        "back" | "forward" | "reload" => Zone::Left,
        "site-info" | "split" | "reader" => Zone::Pill,
        _ => Zone::Right,
    }
}

/// Most buttons that take room from the address field at the end of the bar (everything in the right
/// zone except the two that only appear when there is something to show). The field keeps its width
/// for the rest: five 32 px slots leave a field of a usable size at the narrowest window.
pub const MAX_RIGHT_STATIC: usize = 5;

/// Items in the right zone that take their place whether or not anything is going on: everything
/// there except Downloads and Extensions, which appear (and take room) only when they have something to show.
pub fn right_static(list: &[String]) -> usize {
    list.iter().filter(|id| zone_of(id) == Zone::Right && !matches!(id.as_str(), "downloads" | "extensions")).count()
}

/// Whether `id` is in the address bar's list (so the toolbar page shows it).
pub fn address_has(settings: &Settings, id: &str) -> bool {
    settings.toolbar_buttons.iter().any(|item| item == id)
}

/// CSS px the right zone's always-there buttons take from the address field: a 28 px button and a 4 px
/// gap each, the same as the Downloads and Extensions slots.
pub fn right_static_px(settings: &Settings) -> i32 {
    right_static(&settings.toolbar_buttons) as i32 * crate::chrome_host::EXT_BUTTON_SLOT
}

/// Applies one `key=value` change of the Toolbars and Feeds options (the settings page's `baropt`
/// command), checking the key and the value. `false` and nothing changed otherwise.
pub fn apply_assignment(settings: &mut Settings, text: &str) -> bool {
    let Some((key, value)) = text.split_once('=') else { return false };
    if let Some(bar) = Bar::from_key(key) {
        let Some(list) = parse_list(bar, value) else { return false };
        bar.set(settings, list);
        return true;
    }
    match key {
        "feeds_detect" => match value {
            "true" => settings.feeds_detect = true,
            "false" => settings.feeds_detect = false,
            _ => return false,
        },
        "feeds_refresh_minutes" => {
            let Some(minutes) = value.parse::<u32>().ok().filter(|minutes| crate::feeds::REFRESH_CHOICES.contains(minutes)) else { return false };
            settings.feeds_refresh_minutes = minutes;
        }
        _ => return false,
    }
    true
}

/// The keys [`apply_assignment`] accepts.
pub const OPTION_KEYS: [&str; 5] = ["toolbar_buttons", "toolbar_tabs", "toolbar_panel", "feeds_detect", "feeds_refresh_minutes"];

#[cfg(test)]
mod tests {
    use super::*;

    fn list(text: &str) -> Vec<String> {
        text.split(',').filter(|id| !id.is_empty()).map(str::to_owned).collect()
    }

    #[test]
    fn each_bar_accepts_its_own_buttons_in_any_order_and_nothing_else() {
        assert_eq!(parse_list(Bar::Address, "reload,back"), Some(list("reload,back")));
        assert_eq!(parse_list(Bar::Address, " back , reload "), Some(list("back,reload")));
        assert_eq!(parse_list(Bar::Address, ""), Some(Vec::new()), "an empty bar is allowed (the menu stays)");
        assert_eq!(parse_list(Bar::Tabs, "closed-tabs"), Some(list("closed-tabs")));
        assert_eq!(parse_list(Bar::Panel, "feeds"), Some(list("feeds")));
        for (bar, bad) in [
            (Bar::Address, "back,bogus"), (Bar::Address, "back,back"), (Bar::Address, "Back"), (Bar::Address, "menu"), (Bar::Address, "feeds"),
            (Bar::Address, "closed-tabs"), (Bar::Tabs, "back"), (Bar::Tabs, "feeds"), (Bar::Panel, "closed-tabs"), (Bar::Panel, "bookmarks"),
        ] {
            assert_eq!(parse_list(bar, bad), None, "{bar:?} {bad}");
        }
    }

    #[test]
    fn an_unknown_command_button_in_the_file_is_dropped_without_losing_the_rest_of_the_list() {
        assert_eq!(parse_stored_list(Bar::Address, "back,cmd:no-such-command,reload"), Some(list("back,reload")));
        assert_eq!(parse_stored_list(Bar::Tabs, "cmd:no-such-command,closed-tabs"), Some(list("closed-tabs")));
        assert_eq!(parse_stored_list(Bar::Tabs, "closed-tabs,back"), None, "another bar's own id still refuses the list");
        assert_eq!(parse_list(Bar::Address, "back,cmd:no-such-command,reload"), None, "the settings page's list is checked whole");
        let mut settings = Settings::default();
        assert!(apply_stored_list(&mut settings, "toolbar_panel", "feeds,cmd:nope"));
        assert_eq!(settings.toolbar_panel, list("feeds"));
        assert!(!apply_stored_list(&mut settings, "feeds_detect", "true"), "only a bar's key");
    }

    #[test]
    fn the_old_default_is_read_as_the_new_one_so_nobody_gets_two_new_buttons() {
        let legacy = LEGACY_DEFAULT_ADDRESS.join(",");
        assert_eq!(parse_list(Bar::Address, &legacy), Some(Bar::Address.default_list()));
        // A different choice, even a smaller one, is kept as it is.
        assert_eq!(parse_list(Bar::Address, "back,bookmark,newtab"), Some(list("back,bookmark,newtab")));
        assert!(!Bar::Address.default_list().contains(&"bookmark".to_owned()) && !Bar::Address.default_list().contains(&"newtab".to_owned()));
        // Every button the HTML toolbar has today is in the default.
        for id in ["back", "forward", "reload", "site-info", "split", "reader", "downloads", "extensions"] {
            assert!(Bar::Address.default_list().contains(&id.to_owned()), "{id}");
        }
    }

    #[test]
    fn command_buttons_are_built_in_commands_or_chains() {
        assert!(is_command_item("cmd:new-tab") && is_command_item("cmd:toggle-notes"));
        assert!(!is_command_item("cmd:no-such-command") && !is_command_item("cmd:") && !is_command_item("cmd:chain:1") && !is_command_item("new-tab"));
        assert!(is_command_item("chain:1") && is_command_item("chain:64"));
        assert!(!is_command_item("chain:0") && !is_command_item("chain:01") && !is_command_item("chain:") && !is_command_item("chain:x") && !is_command_item("chain:99999"));
        assert_eq!(command_of("cmd:new-tab"), Some("new-tab"));
        assert_eq!(command_of("chain:3"), Some("chain:3"));
        assert_eq!(command_of("back"), None);
        assert_eq!(command_of("cmd:bogus"), None);
        assert_eq!(parse_list(Bar::Tabs, "closed-tabs,cmd:new-private-tab,chain:2"), Some(list("closed-tabs,cmd:new-private-tab,chain:2")));
        assert_eq!(parse_list(Bar::Panel, "feeds,cmd:history"), Some(list("feeds,cmd:history")));
        assert_eq!(parse_list(Bar::Panel, "cmd:history,cmd:history"), None);
    }

    #[test]
    fn a_bar_cannot_hold_more_than_its_limits() {
        assert_eq!(parse_list(Bar::Tabs, "cmd:new-tab,cmd:new-window,cmd:history,cmd:downloads,cmd:settings"), None, "five command buttons");
        assert!(parse_list(Bar::Tabs, "cmd:new-tab,cmd:new-window,cmd:history,cmd:downloads").is_some());
        // The right zone: at most five buttons that always take room.
        assert!(parse_list(Bar::Address, "bookmark,newtab,cmd:new-tab,cmd:history,chain:1").is_some());
        assert_eq!(parse_list(Bar::Address, "bookmark,newtab,cmd:new-tab,cmd:history,chain:1,chain:2"), None);
        assert!(parse_list(Bar::Address, "bookmark,newtab,cmd:new-tab,cmd:history,downloads,extensions").is_some(), "downloads and extensions only take room when shown");
    }

    #[test]
    fn items_sit_in_the_zone_their_kind_belongs_to() {
        for id in ["back", "forward", "reload"] {
            assert_eq!(zone_of(id), Zone::Left);
        }
        for id in ["site-info", "split", "reader"] {
            assert_eq!(zone_of(id), Zone::Pill);
        }
        for id in ["bookmark", "newtab", "downloads", "extensions", "cmd:new-tab", "chain:1"] {
            assert_eq!(zone_of(id), Zone::Right);
        }
        assert_eq!(right_static(&list("back,bookmark,downloads,extensions,cmd:new-tab,newtab")), 3);
    }

    #[test]
    fn an_assignment_checks_its_key_and_value_and_changes_nothing_when_refused() {
        let mut settings = Settings::default();
        assert!(apply_assignment(&mut settings, "toolbar_tabs=cmd:new-private-tab,closed-tabs"));
        assert_eq!(settings.toolbar_tabs, list("cmd:new-private-tab,closed-tabs"));
        assert!(apply_assignment(&mut settings, "toolbar_panel=feeds"));
        assert_eq!(settings.toolbar_panel, list("feeds"));
        assert!(apply_assignment(&mut settings, "toolbar_buttons=reload,back"));
        assert_eq!(settings.toolbar_buttons, list("reload,back"));
        assert!(apply_assignment(&mut settings, "feeds_detect=false") && !settings.feeds_detect);
        assert!(apply_assignment(&mut settings, "feeds_refresh_minutes=180") && settings.feeds_refresh_minutes == 180);
        let before = settings.clone();
        for bad in [
            "toolbar_tabs=bogus", "toolbar_panel=back", "toolbar_buttons=back,back", "feeds_detect=yes", "feeds_detect=", "feeds_refresh_minutes=7",
            "feeds_refresh_minutes=-1", "feeds_refresh_minutes=", "unknown=1", "toolbar_tabs", "", "=",
        ] {
            assert!(!apply_assignment(&mut settings, bad), "{bad}");
            assert_eq!(settings, before, "{bad}");
        }
    }

    #[test]
    fn the_defaults_are_todays_chrome_and_every_key_is_listed() {
        let defaults = Settings::default();
        assert_eq!(defaults.toolbar_buttons, Bar::Address.default_list());
        assert_eq!(defaults.toolbar_tabs, Bar::Tabs.default_list());
        assert!(defaults.toolbar_panel.is_empty());
        assert!(defaults.feeds_detect);
        assert_eq!(defaults.feeds_refresh_minutes, crate::feeds::DEFAULT_REFRESH_MINUTES);
        assert_eq!(right_static_px(&defaults), 0, "today's address bar reserves nothing extra");
        let mut more = defaults.clone();
        more.toolbar_buttons = list("back,bookmark,newtab");
        assert_eq!(right_static_px(&more), 64);
        for bar in Bar::ALL {
            assert!(OPTION_KEYS.contains(&bar.key()));
        }
    }
}
