//! Tab stacks, tiling and workspaces in the window (T10): the registry commands (stack, unstack,
//! the three tilings, untile, next and previous workspace) and the tab menu's "Tab stack" rows. The
//! rules are in `crate::stacks` and `app/stacks.rs`; the stack editor is the island menu
//! (`island_menu.rs`, its name, colour and member rows), opened through `window::open_stack_editor`.

use windows::Win32::Foundation::HWND;

use super::menu::{Entry, Icon, Item};
use crate::{
    app::{App, AppRef},
    commands::registry::Context,
    stacks::TileMode,
    tabs::TabId,
};

const TITLE: &str = "Tab stacks";

fn toast(hwnd: HWND, message: &str) {
    super::window::show_toast(hwnd, message, TITLE);
}

// ----- registry commands -----

pub(crate) fn run_stack_site(cx: &Context) {
    stack_site(cx.hwnd, cx.app, App::active_tab_id(cx.app));
}

pub(crate) fn run_stack_all_by_site(cx: &Context) {
    match App::stack_all_by_site(cx.app) {
        0 => toast(cx.hwnd, "No site has two tabs to stack"),
        1 => toast(cx.hwnd, "1 stack made"),
        count => toast(cx.hwnd, &format!("{count} stacks made")),
    }
}

pub(crate) fn run_unstack(cx: &Context) {
    if !App::unstack(cx.app, App::active_tab_id(cx.app)) {
        toast(cx.hwnd, "This tab is not in a stack");
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

/// "Stack tabs of this site" for `id`, then the editor when the option asks for it.
fn stack_site(hwnd: HWND, app: &AppRef, id: TabId) {
    if App::stack_site(app, id) == 0 {
        toast(hwnd, "There is no other tab of this site to stack with");
        return;
    }
    stack_made(hwnd, app, id);
}

/// A stack was made by hand: "Open the stack editor when a stack is made" opens its editor.
pub(super) fn stack_made(hwnd: HWND, app: &AppRef, id: TabId) {
    if App::tab_options(app).stack_editor {
        super::window::open_stack_editor(hwnd, app, id);
    }
}

/// Tiles the selection, or the stack of the tab in front, in `mode`.
fn tile(hwnd: HWND, app: &AppRef, mode: TileMode) {
    let tabs = App::tile_targets(app);
    if tabs.len() < 2 {
        super::window::show_toast(hwnd, "Select two to four tabs, or use a stack, to tile them", "Tiling");
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

// ----- the tab menu's "Tab stack" rows -----

const STACK_PARENT: usize = 0x5600;
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
/// "Move the stack to workspace" rows: this plus the workspace's place in the rail.
const STACK_WORKSPACE_BASE: usize = 0x5680;
const MAX_WORKSPACE_ROWS: usize = 0x70;

/// The tab menu's stack rows for `id`: what can be done with its stack, or how to make one. None
/// while stacks are off or for a private tab (which never stacks).
pub(super) fn tab_menu_entries(app: &AppRef, id: TabId) -> Vec<Entry> {
    if !crate::stacks::stacks_on(App::stack_style(app)) || App::tab_private(app, id) || App::tab_pinned(app, id) {
        return Vec::new();
    }
    let members = App::stack_members(app, id);
    if members.is_empty() {
        let host = App::tab_committed_url(app, id).and_then(|url| crate::tab_groups::DomainKey::from_url(&url));
        let same_site = host.as_ref().is_some_and(|key| {
            App::tab_ids(app).into_iter().filter(|other| {
                *other != id && !App::tab_pinned(app, *other) && App::tab_committed_url(app, *other).and_then(|url| crate::tab_groups::DomainKey::from_url(&url)).as_ref() == Some(key)
            }).count() > 0
        });
        return vec![Entry::Item(
            Item::new(STACK_PARENT, "Tab stack", Icon::Grid).glyph("tab-group").submenu_with(vec![
                Entry::Item(Item::new(STACK_SITE, "Stack tabs of this site", Icon::None).glyph("tab-group").disabled_if(!same_site)),
                Entry::Item(Item::new(STACK_ALL, "Stack all tabs by site", Icon::None).glyph("tab-group")),
            ]),
        )];
    }
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
        Entry::Item(Item::new(STACK_PIN, "Pin all tabs in the stack", Icon::None).glyph("pin")),
        Entry::Item(Item::new(STACK_HIBERNATE, "Hibernate the stack", Icon::None).glyph("clock").disabled_if(members.len() < 2)),
        Entry::Item(Item::new(STACK_TILE, "Tile the stack", Icon::None).glyph("split").disabled_if(!tiles)),
        Entry::Separator,
        Entry::Item(Item::new(STACK_SESSION, "Save the stack as a session…", Icon::None).glyph("save").disabled_if(!web)),
        Entry::Item(Item::new(STACK_BOOKMARK, "Bookmark all tabs in the stack", Icon::None).glyph("bookmark").disabled_if(!web)),
        Entry::Item(Item::new(STACK_COPY, "Copy all links", Icon::None).glyph("link").disabled_if(!web)),
        Entry::Separator,
    ];
    if has_workspaces {
        rows.push(Entry::Item(Item::new(STACK_WORKSPACE, "Move the stack to workspace", Icon::None).glyph("workspace").submenu_with(workspaces)));
    }
    rows.push(Entry::Item(Item::new(STACK_WINDOW, "Move the stack to a new window", Icon::None).glyph("window").disabled_if(!web)));
    rows.push(Entry::Separator);
    rows.push(Entry::Item(Item::new(STACK_UNSTACK, "Unstack", Icon::None).glyph("caption-minimize")));
    vec![Entry::Item(Item::new(STACK_PARENT, "Tab stack", Icon::Grid).glyph("tab-group").submenu_with(rows))]
}

/// Runs a row of [`tab_menu_entries`]. False for any other row.
pub(super) fn tab_menu_pick(hwnd: HWND, app: &AppRef, id: TabId, command: usize) -> bool {
    match command {
        STACK_SITE => stack_site(hwnd, app, id),
        STACK_ALL => {
            let _ = App::stack_all_by_site(app);
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
            let Some(name) = super::profile_rename::prompt(hwnd, &name, "Save the stack as a session", "Save") else {
                return true;
            };
            if let Err(message) = App::save_session(app, &name, &App::stack_members(app, id)) {
                super::show_error_message(&message);
            }
        }
        STACK_BOOKMARK => match App::bookmark_stack(app, id) {
            Ok(count) => toast(hwnd, &format!("{count} {} bookmarked", if count == 1 { "page" } else { "pages" })),
            Err(message) => super::show_error_message(&message),
        },
        STACK_COPY => {
            let text = App::stack_links_text(app, id);
            if text.is_empty() || !super::window::copy_text_to_clipboard(hwnd, &text) {
                super::show_error_message("Bowser could not copy the stack's links.");
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
                super::show_error_message("Bowser could not open the stack in a new window.");
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
        for id in [STACK_PARENT, STACK_SITE, STACK_UNSTACK, STACK_WORKSPACE, STACK_WORKSPACE_BASE, STACK_WORKSPACE_BASE + MAX_WORKSPACE_ROWS - 1] {
            assert!((0x5600..0x5700).contains(&id), "{id:#x}");
        }
    }
}
