//! Tab groups ("stacks" in the code), tiling and workspaces in the window (T10): the registry
//! commands (group, ungroup, the three tilings, untile, next and previous workspace) and the tab
//! menu's "Tab group" rows: the manual group rows (new group, add to a group, remove), what can be
//! done with the tab's group, and the automatic "by website" rows. The rules are in `crate::stacks` and `app/stacks.rs`; the group editor is the island menu
//! (`island_menu.rs`, its name, colour and member rows), opened through `window::open_stack_editor`.

use windows::Win32::Foundation::HWND;

use super::menu::{Entry, Icon, Item};
use crate::{
    app::{App, AppRef},
    commands::registry::Context,
    stacks::TileMode,
    tabs::TabId,
};

const TITLE: &str = "Tab groups";

fn toast(hwnd: HWND, message: &str) {
    super::window::show_toast(hwnd, message, TITLE);
}

// ----- registry commands -----

pub(crate) fn run_stack_site(cx: &Context) {
    stack_site(cx.hwnd, cx.app, App::active_tab_id(cx.app));
}

pub(crate) fn run_stack_all_by_site(cx: &Context) {
    match App::stack_all_by_site(cx.app) {
        0 => toast(cx.hwnd, "No site has two tabs to group"),
        1 => toast(cx.hwnd, "1 group made"),
        count => toast(cx.hwnd, &format!("{count} groups made")),
    }
}

pub(crate) fn run_unstack(cx: &Context) {
    if !App::unstack(cx.app, App::active_tab_id(cx.app)) {
        toast(cx.hwnd, "This tab is not in a group");
    }
}

pub(crate) fn run_tile_vertical(cx: &Context) {
    tile(cx.hwnd, cx.app, TileMode::Vertical);
}

pub(crate) fn run_tile_horizontal(cx: &Context) {
    tile(cx.hwnd, cx.app, TileMode::Horizontal);
}

pub(crate) fn run_tile_grid(cx: &Context) {
    tile(cx.hwnd, cx.app, TileMode::Grid);
}

pub(crate) fn run_untile(cx: &Context) {
    if App::split_pair(cx.app).is_none() {
        super::window::show_toast(cx.hwnd, "Nothing is tiled", "Tiling");
        return;
    }
    let _ = App::separate_split(cx.app);
}

pub(crate) fn run_next_workspace(cx: &Context) {
    if App::workspaces_enabled(cx.app) {
        let _ = App::step_workspace(cx.app, 1);
    }
}

pub(crate) fn run_previous_workspace(cx: &Context) {
    if App::workspaces_enabled(cx.app) {
        let _ = App::step_workspace(cx.app, -1);
    }
}

/// "Group tabs of this site" for `id`.
fn stack_site(hwnd: HWND, app: &AppRef, id: TabId) {
    if App::stack_site(app, id) == 0 {
        toast(hwnd, "There is no other tab of this site to group with");
    }
}

/// Tiles the selection, or the stack of the tab in front, in `mode`.
fn tile(hwnd: HWND, app: &AppRef, mode: TileMode) {
    let tabs = App::tile_targets(app);
    if tabs.len() < 2 {
        super::window::show_toast(hwnd, "Select two to four tabs, or use a group, to tile them", "Tiling");
        return;
    }
    tile_tabs(hwnd, app, &tabs, mode);
}

/// Tiles `tabs` in `mode` and lays the panes out at once.
pub(super) fn tile_tabs(hwnd: HWND, app: &AppRef, tabs: &[TabId], mode: TileMode) -> bool {
    let opened = App::tile_tabs(app, tabs, mode, |view| super::window::split_rects_for_view(hwnd, view));
    if opened {
        super::window::sync_split_ratio(hwnd, App::split_view(app));
        if let Some(panes) = super::window::split_rects(hwnd, None) {
            App::update_split_bounds(app, panes);
        }
    }
    opened
}

// ----- the tab menu's "Tab group" rows -----

const STACK_SITE: usize = 0x5601;
const STACK_ALL: usize = 0x5602;
const STACK_EDIT: usize = 0x5603;
const STACK_PIN: usize = 0x5604;
const STACK_HIBERNATE: usize = 0x5605;
const STACK_TILE: usize = 0x5606;
const STACK_SESSION: usize = 0x5607;
const STACK_BOOKMARK: usize = 0x5608;
const STACK_COPY: usize = 0x5609;
const STACK_WINDOW: usize = 0x560A;
const STACK_UNSTACK: usize = 0x560B;
const STACK_WORKSPACE: usize = 0x560C;
const STACK_NEW_GROUP: usize = 0x560D;
const STACK_REMOVE: usize = 0x560E;
const STACK_ADD_TO: usize = 0x560F;
/// "Add tab to group" rows: this plus the group's place in [`App::group_choices`].
const STACK_ADD_TO_BASE: usize = 0x5610;
const MAX_GROUP_ROWS: usize = 0x60;
/// "Move the group to workspace" rows: this plus the workspace's place in the rail.
const STACK_WORKSPACE_BASE: usize = 0x5680;
const MAX_WORKSPACE_ROWS: usize = 0x70;

/// What "Group all tabs by website" does, in one line: the submenu shows it under its heading and
/// the native menu in its footer while the row is highlighted.
pub(super) const BY_WEBSITE_EXPLAINED: &str = "Tabs from the same website, such as all your GitHub tabs, share one group";

/// The tab menu's "Tab group" rows for `id` (window.rs puts them in that submenu, then the site's
/// grouping choices right under the last "By website" row). First the groups made by hand: a new
/// group, an existing group to add the tab to, and taking it out of its group. Then what can be done
/// with its group, and last the automatic grouping by website. None for a private or pinned tab
/// (which never joins a group).
pub(super) fn tab_menu_entries(app: &AppRef, id: TabId) -> Vec<Entry> {
    if App::tab_private(app, id) || App::tab_pinned(app, id) {
        return Vec::new();
    }
    let members = App::stack_members(app, id);
    let grouped = !members.is_empty();
    let choices: Vec<Entry> = App::group_choices(app, id)
        .into_iter()
        .take(MAX_GROUP_ROWS)
        .enumerate()
        .map(|(index, (_, name))| Entry::Item(Item::new(STACK_ADD_TO_BASE + index, name, Icon::None).glyph("tab-group")))
        .collect();
    let mut rows = vec![
        Entry::Item(Item::new(STACK_NEW_GROUP, "Add tab to new group", Icon::None).glyph("plus")),
    ];
    if !choices.is_empty() {
        let label = if grouped { "Move tab to another group" } else { "Add tab to group" };
        rows.push(Entry::Item(Item::new(STACK_ADD_TO, label, Icon::None).glyph("tab-group").submenu_with(choices)));
    }
    if grouped {
        rows.push(Entry::Item(Item::new(STACK_REMOVE, "Remove from group", Icon::None).glyph("caption-minimize")));
    }
    rows.push(Entry::Separator);
    if grouped {
        rows.extend(group_rows(app, id, &members));
        rows.push(Entry::Separator);
    }
    // Automatic grouping, with a line that says what it means.
    let host = App::tab_committed_url(app, id).and_then(|url| crate::tab_groups::DomainKey::from_url(&url));
    let same_site = !grouped
        && host.as_ref().is_some_and(|key| {
            App::tab_ids(app).into_iter().any(|other| {
                other != id && !App::tab_pinned(app, other) && App::tab_committed_url(app, other).and_then(|url| crate::tab_groups::DomainKey::from_url(&url)).as_ref() == Some(key)
            })
        });
    rows.extend([
        Entry::SectionHeader("Group by website".to_owned()),
        Entry::SectionHeader(BY_WEBSITE_EXPLAINED.to_owned()),
        Entry::Item(Item::new(STACK_SITE, "Group this website's tabs", Icon::None).glyph("tab-group").disabled_if(!same_site)),
        Entry::Item(Item::new(STACK_ALL, "Group all tabs by website", Icon::None).glyph("tab-group").described(BY_WEBSITE_EXPLAINED)),
    ]);
    rows
}

/// What can be done with the group `id` is in (`members`, never empty).
fn group_rows(app: &AppRef, id: TabId, members: &[TabId]) -> Vec<Entry> {
    let workspaces: Vec<Entry> = App::workspace_entries(app)
        .into_iter()
        .enumerate()
        .take(MAX_WORKSPACE_ROWS)
        .filter(|(_, (_, _, _, _, _, active))| !active)
        .map(|(index, (_, name, _, _, _, _))| Entry::Item(Item::new(STACK_WORKSPACE_BASE + index, name, Icon::None).glyph("workspace")))
        .collect();
    let has_workspaces = !workspaces.is_empty() && App::workspaces_enabled(app);
    let web = !App::stack_links(app, id).is_empty();
    let tiles = members.len() >= 2;
    let mut rows = vec![
        Entry::Item(Item::new(STACK_EDIT, "Name and colour…", Icon::None).glyph("edit")),
        Entry::Item(Item::new(STACK_PIN, "Pin all tabs in the group", Icon::None).glyph("pin")),
        Entry::Item(Item::new(STACK_HIBERNATE, "Hibernate the group", Icon::None).glyph("clock").disabled_if(members.len() < 2)),
        Entry::Item(Item::new(STACK_TILE, "Tile the group", Icon::None).glyph("split").disabled_if(!tiles)),
        Entry::Separator,
        Entry::Item(Item::new(STACK_SESSION, "Save the group as a session…", Icon::None).glyph("save").disabled_if(!web)),
        Entry::Item(Item::new(STACK_BOOKMARK, "Bookmark all tabs in the group", Icon::None).glyph("bookmark").disabled_if(!web)),
        Entry::Item(Item::new(STACK_COPY, "Copy all links", Icon::None).glyph("link").disabled_if(!web)),
        Entry::Separator,
    ];
    if has_workspaces {
        rows.push(Entry::Item(Item::new(STACK_WORKSPACE, "Move the group to workspace", Icon::None).glyph("workspace").submenu_with(workspaces)));
    }
    rows.push(Entry::Item(Item::new(STACK_WINDOW, "Move the group to a new window", Icon::None).glyph("window").disabled_if(!web)));
    rows.push(Entry::Separator);
    rows.push(Entry::Item(Item::new(STACK_UNSTACK, "Ungroup", Icon::None).glyph("caption-minimize")));
    rows
}

/// "Add tab to new group": the new group's name and colour editor opens at once, as in other
/// browsers, so it can be named straight away (Enter keeps "Group N").
fn new_group(hwnd: HWND, app: &AppRef, id: TabId) {
    if App::add_tab_to_new_group(app, id).is_some() {
        super::window::open_stack_editor(hwnd, app, id);
    } else {
        toast(hwnd, "This window has as many groups as it can hold");
    }
}

/// Runs a row of [`tab_menu_entries`]. False for any other row.
pub(super) fn tab_menu_pick(hwnd: HWND, app: &AppRef, id: TabId, command: usize) -> bool {
    match command {
        STACK_SITE => stack_site(hwnd, app, id),
        STACK_ALL => {
            let _ = App::stack_all_by_site(app);
        }
        STACK_NEW_GROUP => new_group(hwnd, app, id),
        STACK_REMOVE => {
            let _ = App::remove_tab_from_group(app, id);
        }
        c if (STACK_ADD_TO_BASE..STACK_ADD_TO_BASE + MAX_GROUP_ROWS).contains(&c) => {
            // The list is read again: the same groups, in the same order, unless one closed meanwhile.
            if let Some((key, _)) = App::group_choices(app, id).get(c - STACK_ADD_TO_BASE) {
                let _ = App::add_tab_to_group(app, id, key);
            }
        }
        STACK_EDIT => super::window::open_stack_editor(hwnd, app, id),
        STACK_PIN => {
            let _ = App::pin_stack(app, id);
        }
        STACK_HIBERNATE => {
            let _ = App::hibernate_stack(app, id);
        }
        STACK_TILE => {
            let members = App::stack_members(app, id);
            let mode = if members.len() > 2 { TileMode::Grid } else { TileMode::Vertical };
            let _ = tile_tabs(hwnd, app, &members, mode);
        }
        STACK_SESSION => {
            let name = App::stack_name(app, id).unwrap_or_default();
            let Some(name) = super::profile_rename::prompt(hwnd, &name, "Save the group as a session", "Save") else {
                return true;
            };
            if let Err(message) = App::save_session(app, &name, &App::stack_members(app, id)) {
                super::show_error_message(&message);
            }
        }
        STACK_BOOKMARK => match App::bookmark_stack(app, id) {
            Ok(saved) => {
                toast(hwnd, &App::tabs_saved_message(&App::stack_name(app, id).unwrap_or_default(), saved));
                super::post(hwnd, super::WM_RAIL_CHANGED, windows::Win32::Foundation::WPARAM(0), windows::Win32::Foundation::LPARAM(0));
            }
            Err(message) => super::show_error_message(&message),
        },
        STACK_COPY => {
            let text = App::stack_links_text(app, id);
            if text.is_empty() || !super::window::copy_text_to_clipboard(hwnd, &text) {
                super::show_error_message("Bowser could not copy the group's links.");
            }
        }
        STACK_WINDOW => {
            let urls = App::stack_links(app, id);
            let launched = crate::profile::active().is_ok_and(|profile| crate::profile::spawn_window_with_urls(&profile, &urls));
            if launched {
                for member in App::stack_members(app, id) {
                    if App::tab_committed_url(app, member).is_some_and(|url| urls.contains(&url)) {
                        let _ = App::close_tab(app, member);
                    }
                }
            } else {
                super::show_error_message("Bowser could not open the group in a new window.");
            }
        }
        STACK_UNSTACK => {
            let _ = App::unstack(app, id);
        }
        c if (STACK_WORKSPACE_BASE..STACK_WORKSPACE_BASE + MAX_WORKSPACE_ROWS).contains(&c) => {
            let entries = App::workspace_entries(app);
            if let Some((target, ..)) = entries.get(c - STACK_WORKSPACE_BASE) {
                let members = App::stack_members(app, id);
                let _ = App::move_tabs_to_workspace(app, &members, *target, false);
            }
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stack_rows_never_take_an_id_another_tab_menu_row_uses() {
        // The tab menu's own ids (window.rs), the split rows (0x5100..), the tab options rows
        // (0x5200..) and the closed tabs list (0x52FF..0x5300+).
        for id in [STACK_SITE, STACK_UNSTACK, STACK_WORKSPACE, STACK_WORKSPACE_BASE, STACK_WORKSPACE_BASE + MAX_WORKSPACE_ROWS - 1] {
            assert!((0x5600..0x5700).contains(&id), "{id:#x}");
        }
        // The manual group rows sit between the fixed rows and the workspace rows.
        for id in [STACK_NEW_GROUP, STACK_REMOVE, STACK_ADD_TO] {
            assert!(id > STACK_WORKSPACE && id < STACK_ADD_TO_BASE, "{id:#x}");
        }
        assert!(STACK_ADD_TO_BASE + MAX_GROUP_ROWS <= STACK_WORKSPACE_BASE);
    }
}
