//! What each keyboard and Quick Commands option decides, as small pure functions the window code calls.
//! Keeping the decision here lets a test prove the option's effect without a window: the window only
//! carries the decision out.

use super::store::Options;

/// What pressing and releasing Alt on its own does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AltTap {
    /// Open Bowser's main menu (`alt_opens_menu` on).
    OpenMenu,
    /// Nothing: not Windows' hidden system-menu mode either.
    Nothing,
}

pub fn alt_tap(options: &Options) -> AltTap {
    if options.alt_opens_menu { AltTap::OpenMenu } else { AltTap::Nothing }
}

const VK_F6: u32 = 0x75;

/// Whether F6 should move the keyboard from the address bar to the page (`focus_cycling` on, F6 pressed
/// while the address bar has it). Otherwise F6 goes to the address bar, as it always did.
pub fn f6_moves_to_page(options: &Options, key: u32, address_bar_has_focus: bool) -> bool {
    options.focus_cycling && key == VK_F6 && address_bar_has_focus
}

/// Where a link picked in Quick Commands opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkOpen {
    CurrentTab,
    NewTab,
}

/// A bookmark or history address (`url:`) opens in a new tab when `palette_open_new_tab` is on; a closed
/// tab (`reopen:`) always comes back in a tab of its own. `None` for any other action.
pub fn link_open(options: &Options, action: &str) -> Option<LinkOpen> {
    if action.starts_with("reopen:") {
        Some(LinkOpen::NewTab)
    } else if action.starts_with("url:") {
        Some(if options.palette_open_new_tab { LinkOpen::NewTab } else { LinkOpen::CurrentTab })
    } else {
        None
    }
}

/// What picking a note does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotePick {
    /// Open the Notes panel (`palette_notes_in_panel` on).
    OpenPanel,
    /// Copy the note's text to the clipboard.
    CopyText,
}

pub fn note_pick(options: &Options) -> NotePick {
    if options.palette_notes_in_panel { NotePick::OpenPanel } else { NotePick::CopyText }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{
        chains::{Chain, Step},
        store::Store,
    };

    /// The options as the settings page would save them, read back the way the window reads them.
    fn saved(change: impl FnOnce(&mut Options)) -> Options {
        let mut store = Store::default();
        change(&mut store.options);
        let mut back = Store::default();
        back.parse_keys(&store.serialize_keys());
        back.options
    }

    #[test]
    fn alt_on_its_own_opens_the_menu_only_while_the_option_is_on() {
        assert_eq!(alt_tap(&saved(|_| {})), AltTap::Nothing, "off by default");
        assert_eq!(alt_tap(&saved(|o| o.alt_opens_menu = true)), AltTap::OpenMenu);
    }

    #[test]
    fn f6_moves_to_the_page_only_with_cycling_on_from_the_address_bar() {
        let off = saved(|_| {});
        let on = saved(|o| o.focus_cycling = true);
        assert!(!f6_moves_to_page(&off, 0x75, true), "off: F6 always goes to the address bar");
        assert!(f6_moves_to_page(&on, 0x75, true), "on, from the address bar: to the page");
        assert!(!f6_moves_to_page(&on, 0x75, false), "on, from the page: to the address bar");
        assert!(!f6_moves_to_page(&on, 0x4C, true), "Ctrl+L and Alt+D always go to the address bar");
    }

    #[test]
    fn links_open_in_a_new_tab_only_with_the_option_on_and_closed_tabs_always_do() {
        let off = saved(|_| {});
        let on = saved(|o| o.palette_open_new_tab = true);
        assert_eq!(link_open(&off, "url:https://a.example/"), Some(LinkOpen::CurrentTab));
        assert_eq!(link_open(&on, "url:https://a.example/"), Some(LinkOpen::NewTab));
        assert_eq!(link_open(&off, "reopen:https://a.example/"), Some(LinkOpen::NewTab));
        assert_eq!(link_open(&on, "reopen:https://a.example/"), Some(LinkOpen::NewTab));
        assert_eq!(link_open(&on, "tab:3"), None);
        assert_eq!(link_open(&on, "new-tab"), None);
    }

    #[test]
    fn a_picked_note_opens_the_panel_or_is_copied() {
        assert_eq!(note_pick(&saved(|_| {})), NotePick::OpenPanel, "on by default");
        assert_eq!(note_pick(&saved(|o| o.palette_notes_in_panel = false)), NotePick::CopyText);
    }

    #[test]
    fn a_saved_chain_is_what_the_registry_runs_for_its_command_id() {
        let mut store = Store::default();
        let id = store
            .save_chain(Chain { id: 0, name: "Go".into(), steps: vec![Step { command: "new-tab".into(), delay_ms: 0 }, Step { command: "reload".into(), delay_ms: 30 }] })
            .unwrap();
        let entry = store.entries().into_iter().find(|e| e.id == format!("chain:{id}")).unwrap();
        assert!(matches!(entry.handler, crate::commands::store::EntryHandler::Chain(n) if n == id));
        let planned = crate::commands::chains::plan(&store.chains[0]);
        assert_eq!(planned.iter().map(|p| (p.delay_ms, p.command.as_str())).collect::<Vec<_>>(), [(0, "new-tab"), (30, "reload")]);
    }
}
