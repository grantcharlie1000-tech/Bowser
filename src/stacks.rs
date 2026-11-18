//! Tab stacks, tiling and workspace switching (T10): the pure rules behind the options of
//! Settings > Tabs and start page > Tab stacks, tiling and workspaces.
//!
//! A *stack* is one of Bowser's site islands (`tab_groups`): the same projection, preferences, names
//! and colours, so there is no second grouping system. Islands have one look; stacking by hand places
//! tabs with the island machinery's own manual membership (`tab_organisation::Membership::Joined`). *Tiling* is the
//! split view (`split_view`, up to four panes): [`tile_layout`] turns a list of tabs into one of its
//! layouts. Nothing here touches windows, WebViews, timers or files.

use std::collections::BTreeMap;

use crate::split_view::{Bounds, Orientation, SplitView};
use crate::tab_groups::DomainKey;
use crate::tabs::TabId;
use crate::workspaces::WorkspaceId;

// ----- stacking by site -----

/// One tab of the active workspace as the stacking commands see it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StackInput {
    pub id: TabId,
    /// The tab's site (registrable domain); `None` for pages with no site.
    pub domain: Option<DomainKey>,
    pub pinned: bool,
    pub private: bool,
}

fn stackable(tab: &StackInput) -> bool {
    !tab.pinned && !tab.private && tab.domain.is_some()
}

/// "Stack tabs of this site": the tabs that join `domain`'s stack, in strip order. Pinned and
/// private tabs never stack.
pub fn site_members(tabs: &[StackInput], domain: &DomainKey) -> Vec<TabId> {
    tabs.iter()
        .filter(|tab| stackable(tab) && tab.domain.as_ref() == Some(domain))
        .map(|tab| tab.id)
        .collect()
}

/// "Stack all tabs by site": one stack per site that has two tabs or more, sites in the order their
/// first tab appears.
pub fn sites_to_stack(tabs: &[StackInput]) -> Vec<(DomainKey, Vec<TabId>)> {
    let mut first_seen: Vec<DomainKey> = Vec::new();
    let mut members: BTreeMap<DomainKey, Vec<TabId>> = BTreeMap::new();
    for tab in tabs.iter().filter(|tab| stackable(tab)) {
        let domain = tab.domain.clone().expect("stackable tabs have a site");
        if !members.contains_key(&domain) {
            first_seen.push(domain.clone());
        }
        members.entry(domain).or_default().push(tab.id);
    }
    first_seen
        .into_iter()
        .filter_map(|domain| {
            let ids = members.remove(&domain)?;
            (ids.len() >= 2).then_some((domain, ids))
        })
        .collect()
}

/// How long (ms) a dragged tab must rest over the middle of another before a release stacks the two.
pub const STACK_DROP_DELAY_MS: u32 = 400;

/// Whether a dragged tab that has rested over another for `rested_ms` stacks with it on release.
pub fn drop_stacks(rested_ms: u64) -> bool {
    rested_ms >= u64::from(STACK_DROP_DELAY_MS)
}

/// The middle half of a tab: a dragged tab resting there means "stack", nearer the edges it means
/// "move between". `(left, right)` is the tab's span along the strip; `x` the pointer.
pub fn over_middle(left: i32, right: i32, x: i32) -> bool {
    let quarter = (right - left) / 4;
    right > left && x >= left + quarter && x < right - quarter
}

// ----- moving a whole island -----

/// One tab of the active workspace in the order the strip draws it (`tab_groups::strip_order`): an
/// island's members as one run at its first member's place.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StripSlot {
    pub id: TabId,
    /// The island the tab is drawn in; `None` for an ungrouped (or pinned) tab.
    pub island: Option<DomainKey>,
    pub pinned: bool,
}

/// Where a moved island goes: in front of a tab (which stays where it is), or to the end of the strip.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IslandAnchor {
    Before(TabId),
    End,
}

/// Where island `island` lands when its handle is dropped (or moved by the keyboard) before tab
/// `before` (`None`: the end of the strip), with every member moving as one block in its own order.
/// An island never lands inside another: a `before` in the middle of another island means in front of
/// that island. Pinned tabs stay first, so a `before` among them means in front of the first unpinned
/// tab. `None` when the island is not in `strip`, `before` is unknown or one of its own members, or the
/// block would end up exactly where it is.
pub fn island_move_anchor(strip: &[StripSlot], island: &DomainKey, before: Option<TabId>) -> Option<IslandAnchor> {
    let member = |slot: &StripSlot| !slot.pinned && slot.island.as_ref() == Some(island);
    if !strip.iter().any(member) {
        return None;
    }
    let others: Vec<&StripSlot> = strip.iter().filter(|slot| !member(slot)).collect();
    // Index among `others` the block goes in front of (`others.len()` = the end).
    let mut at = match before {
        None => others.len(),
        Some(id) => {
            let slot = strip.iter().find(|slot| slot.id == id)?;
            if member(slot) {
                return None;
            }
            others.iter().position(|other| other.id == id)?
        }
    };
    // Not among the pinned tabs: after the last of them.
    let first_unpinned = others.iter().position(|slot| !slot.pinned).unwrap_or(others.len());
    at = at.max(first_unpinned);
    // Not inside another island: back to that island's first member.
    if let Some(key) = others.get(at).and_then(|slot| slot.island.as_ref()).filter(|_| !others[at].pinned) {
        while at > 0 && others[at - 1].island.as_ref() == Some(key) && !others[at - 1].pinned {
            at -= 1;
        }
    }
    // Unchanged when the block already sits, in one piece, right in front of that place.
    let mut after: Vec<TabId> = others.iter().map(|slot| slot.id).collect();
    let block: Vec<TabId> = strip.iter().filter(|slot| member(slot)).map(|slot| slot.id).collect();
    after.splice(at..at, block);
    if after.iter().copied().eq(strip.iter().map(|slot| slot.id)) {
        return None;
    }
    Some(others.get(at).map_or(IslandAnchor::End, |slot| IslandAnchor::Before(slot.id)))
}

// ----- tiling -----

/// The three tiling commands (Ctrl+F9, Ctrl+F8, Ctrl+F7).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TileMode {
    /// Side by side: tabs fill columns.
    Vertical,
    /// One above another: tabs fill rows.
    Horizontal,
    /// Two by two.
    Grid,
}

/// Most tabs one tiling shows: the split view's four panes.
pub const MAX_TILES: usize = 4;

/// The tabs a tiling command tiles: the selected tabs when two or more are selected, else the stack
/// of the tab in front when it has two or more, else none. At most [`MAX_TILES`], in strip order.
pub fn tile_targets(selected: &[TabId], stack: &[TabId]) -> Vec<TabId> {
    let source = if selected.len() >= 2 { selected } else if stack.len() >= 2 { stack } else { &[] };
    source.iter().copied().take(MAX_TILES).collect()
}

/// The split view that tiles `tabs` (2 to 4, the first ones are used) in `mode`, focused on
/// `focused` (or the first tab). Two tabs make a pair (Grid lays two out side by side); three make a
/// "T" whose long pane is the first tab; four make a two by two grid, filled column by column for
/// Vertical and row by row otherwise. The split view has two halves, each divided at most once, so
/// three or four tabs never make three or four equal columns.
pub fn tile_layout(tabs: &[TabId], mode: TileMode, focused: TabId) -> Option<SplitView> {
    let focus = if tabs.contains(&focused) { focused } else { *tabs.first()? };
    let (cells, prefer) = match (tabs, mode) {
        (&[a, b], TileMode::Horizontal) => ([a, a, b, b], Orientation::Stacked),
        (&[a, b], _) => ([a, b, a, b], Orientation::SideBySide),
        (&[a, b, c], TileMode::Vertical) => ([a, b, a, c], Orientation::SideBySide),
        (&[a, b, c], TileMode::Horizontal) => ([a, a, b, c], Orientation::Stacked),
        (&[a, b, c], TileMode::Grid) => ([a, b, c, c], Orientation::Stacked),
        (&[a, b, c, d, ..], TileMode::Vertical) => ([a, c, b, d], Orientation::SideBySide),
        (&[a, b, c, d, ..], TileMode::Horizontal) => ([a, b, c, d], Orientation::Stacked),
        (&[a, b, c, d, ..], TileMode::Grid) => ([a, b, c, d], Orientation::SideBySide),
        _ => return None,
    };
    let mut unique = tabs.iter().take(MAX_TILES).collect::<Vec<_>>();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != tabs.len().min(MAX_TILES) {
        return None;
    }
    SplitView::from_cells(cells, prefer, 50, 50, focus)
}

/// Height of a tile's title bar, in DIP (one line of chrome text with its padding).
pub const TILE_TITLE_DIP: i32 = 24;
/// The gap between tiles with "Remove the space around tiles", in DIP: a hairline that is still the
/// divider's grip.
pub const TIGHT_GAP_DIP: i32 = 2;

/// A tile's page and title bar: the title bar takes the top `title_px` of the pane, unless the pane
/// is too short to give a page below it room (then it has none). `None` title for no title bars.
pub fn tile_parts(pane: Bounds, title_px: i32) -> (Bounds, Option<Bounds>) {
    let (left, top, right, bottom) = pane;
    if title_px <= 0 || bottom - top < title_px * 3 {
        return (pane, None);
    }
    ((left, top + title_px, right, bottom), Some((left, top, right, top + title_px)))
}

thread_local! {
    /// The window thread's tile look ("Show a title bar on each tile", "Remove the space around
    /// tiles"), set from the tab options whenever they load or change. The split geometry in
    /// `chrome::layout` and `win32::window` reads it instead of a constant.
    static TILE_LOOK: std::cell::Cell<(bool, bool)> = const { std::cell::Cell::new((false, false)) };
}

/// Sets the tile look for this thread: `(title bars, tight)`.
pub fn set_tile_look(titles: bool, tight: bool) {
    TILE_LOOK.with(|look| look.set((titles, tight)));
}

/// The gap between tiles, in DIP.
pub fn gap_dip() -> i32 {
    if TILE_LOOK.with(|look| look.get().1) { TIGHT_GAP_DIP } else { crate::split_view::GAP_DIP }
}

/// The title bar height, in DIP, or 0 without title bars.
pub fn title_dip() -> i32 {
    if TILE_LOOK.with(|look| look.get().0) { TILE_TITLE_DIP } else { 0 }
}

// ----- workspaces -----

/// The workspace a wheel turn over the rail's workspaces lands on: `steps` places along `order`
/// (negative is up), stopping at the ends. `None` when it does not move.
pub fn workspace_scroll_target(order: &[WorkspaceId], active: WorkspaceId, steps: i32) -> Option<WorkspaceId> {
    let at = order.iter().position(|id| *id == active)?;
    let last = order.len().checked_sub(1)?;
    let target = (at as i64 + i64::from(steps)).clamp(0, last as i64) as usize;
    (target != at).then(|| order[target])
}

/// The words of a workspace button's tab count: "3 tabs".
pub fn tab_count_label(count: usize) -> String {
    if count == 1 { "1 tab".to_owned() } else { format!("{count} tabs") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::split_view::Pane;

    fn ids(list: &[u32]) -> Vec<TabId> {
        list.iter().map(|id| TabId(*id)).collect()
    }

    fn key(domain: &str) -> DomainKey {
        DomainKey::parse(domain).unwrap()
    }

    fn tab(id: u32, domain: Option<&str>) -> StackInput {
        StackInput { id: TabId(id), domain: domain.map(key), pinned: false, private: false }
    }

    #[test]
    fn stacking_by_site_takes_unpinned_public_tabs_with_a_site() {
        let mut pinned = tab(5, Some("a.com"));
        pinned.pinned = true;
        let mut private = tab(6, Some("a.com"));
        private.private = true;
        let tabs = vec![tab(1, Some("a.com")), tab(2, Some("b.com")), tab(3, Some("a.com")), tab(4, None), pinned, private, tab(7, Some("b.com")), tab(8, Some("c.com"))];
        assert_eq!(site_members(&tabs, &key("a.com")), ids(&[1, 3]));
        assert_eq!(sites_to_stack(&tabs), vec![(key("a.com"), ids(&[1, 3])), (key("b.com"), ids(&[2, 7]))], "a lone c.com tab is no stack");
    }

    #[test]
    fn a_drop_stacks_only_after_the_delay() {
        assert!(!drop_stacks(399));
        assert!(drop_stacks(400));
        assert!(over_middle(0, 100, 50) && over_middle(0, 100, 25) && !over_middle(0, 100, 24) && !over_middle(0, 100, 75));
        assert!(!over_middle(10, 10, 10));
    }

    #[test]
    fn tiling_targets_the_selection_then_the_stack() {
        assert_eq!(tile_targets(&ids(&[1, 2, 3]), &ids(&[7, 8])), ids(&[1, 2, 3]));
        assert_eq!(tile_targets(&ids(&[1]), &ids(&[7, 8])), ids(&[7, 8]), "one selected tab is no selection");
        assert_eq!(tile_targets(&ids(&[1, 2, 3, 4, 5]), &[]), ids(&[1, 2, 3, 4]), "four tiles at most");
        assert!(tile_targets(&ids(&[1]), &ids(&[7])).is_empty());
    }

    #[test]
    fn two_tabs_tile_side_by_side_or_stacked() {
        let pair = tile_layout(&ids(&[1, 2]), TileMode::Vertical, TabId(2)).unwrap();
        assert_eq!((pair.left, pair.right, pair.orientation, pair.focused), (TabId(1), TabId(2), Orientation::SideBySide, Pane::Right));
        let stacked = tile_layout(&ids(&[1, 2]), TileMode::Horizontal, TabId(9)).unwrap();
        assert_eq!((stacked.orientation, stacked.focused), (Orientation::Stacked, Pane::Left), "an unknown focus falls back to the first tab");
        assert!(tile_layout(&ids(&[1, 2]), TileMode::Grid, TabId(1)).unwrap().is_pair());
        assert!(tile_layout(&ids(&[1]), TileMode::Grid, TabId(1)).is_none());
        assert!(tile_layout(&ids(&[1, 1]), TileMode::Grid, TabId(1)).is_none());
    }

    #[test]
    fn three_and_four_tabs_tile_as_the_split_views_larger_layouts() {
        let t = tile_layout(&ids(&[1, 2, 3]), TileMode::Vertical, TabId(1)).unwrap();
        assert_eq!(t.cells(), [TabId(1), TabId(2), TabId(1), TabId(3)], "a full-height first column");
        let rows = tile_layout(&ids(&[1, 2, 3]), TileMode::Horizontal, TabId(1)).unwrap();
        assert_eq!(rows.cells(), [TabId(1), TabId(1), TabId(2), TabId(3)], "a full-width first row");
        let grid3 = tile_layout(&ids(&[1, 2, 3]), TileMode::Grid, TabId(3)).unwrap();
        assert_eq!(grid3.cells(), [TabId(1), TabId(2), TabId(3), TabId(3)]);
        assert_eq!(grid3.focused_tab(), TabId(3));
        let columns = tile_layout(&ids(&[1, 2, 3, 4]), TileMode::Vertical, TabId(1)).unwrap();
        assert_eq!(columns.cells(), [TabId(1), TabId(3), TabId(2), TabId(4)], "column by column");
        let grid = tile_layout(&ids(&[1, 2, 3, 4]), TileMode::Grid, TabId(1)).unwrap();
        assert_eq!(grid.cells(), [TabId(1), TabId(2), TabId(3), TabId(4)]);
        assert_eq!(grid.pane_count(), 4);
        assert_eq!(tile_layout(&ids(&[1, 2, 3, 4, 5]), TileMode::Horizontal, TabId(5)).unwrap().pane_count(), 4, "a fifth tab is left out");
    }

    #[test]
    fn a_title_bar_takes_the_top_of_a_tile_that_has_room() {
        assert_eq!(tile_parts((0, 0, 100, 300), 24), ((0, 24, 100, 300), Some((0, 0, 100, 24))));
        assert_eq!(tile_parts((0, 0, 100, 60), 24), ((0, 0, 100, 60), None), "too short: the page keeps it all");
        assert_eq!(tile_parts((0, 0, 100, 300), 0), ((0, 0, 100, 300), None));
    }

    #[test]
    fn the_tile_look_sets_the_gap_and_the_title_bar() {
        set_tile_look(false, false);
        assert_eq!((gap_dip(), title_dip()), (crate::split_view::GAP_DIP, 0));
        set_tile_look(true, true);
        assert_eq!((gap_dip(), title_dip()), (TIGHT_GAP_DIP, TILE_TITLE_DIP));
        set_tile_look(false, false);
    }

    #[test]
    fn scrolling_steps_through_workspaces_and_stops_at_the_ends() {
        let order = [WorkspaceId(4), WorkspaceId(1), WorkspaceId(9)];
        assert_eq!(workspace_scroll_target(&order, WorkspaceId(1), 1), Some(WorkspaceId(9)));
        assert_eq!(workspace_scroll_target(&order, WorkspaceId(1), -1), Some(WorkspaceId(4)));
        assert_eq!(workspace_scroll_target(&order, WorkspaceId(9), 1), None, "no wrap past the last");
        assert_eq!(workspace_scroll_target(&order, WorkspaceId(4), -3), None);
        assert_eq!(workspace_scroll_target(&order, WorkspaceId(4), 5), Some(WorkspaceId(9)));
        assert_eq!(workspace_scroll_target(&order, WorkspaceId(7), 1), None);
        assert_eq!(tab_count_label(1), "1 tab");
        assert_eq!(tab_count_label(12), "12 tabs");
    }

    // ----- moving a whole island -----

    /// A strip from a compact spec: `p` pinned, `-` ungrouped, a letter an island (`a` = a.com).
    fn strip(spec: &[(u32, char)]) -> Vec<StripSlot> {
        spec.iter()
            .map(|&(id, c)| StripSlot {
                id: TabId(id),
                island: c.is_ascii_lowercase().then(|| key(&format!("{c}.com"))).filter(|_| c != 'p'),
                pinned: c == 'p',
            })
            .collect()
    }

    #[test]
    fn an_island_moves_as_one_block_in_front_of_a_tab_or_to_the_end() {
        // 1 pinned, 2 ungrouped, island a = 3 4 5, 6 ungrouped, island b = 7 8, 9 ungrouped.
        let s = strip(&[(1, 'p'), (2, '-'), (3, 'a'), (4, 'a'), (5, 'a'), (6, '-'), (7, 'b'), (8, 'b'), (9, '-')]);
        let a = key("a.com");
        assert_eq!(island_move_anchor(&s, &a, Some(TabId(2))), Some(IslandAnchor::Before(TabId(2))));
        assert_eq!(island_move_anchor(&s, &a, Some(TabId(9))), Some(IslandAnchor::Before(TabId(9))));
        assert_eq!(island_move_anchor(&s, &a, None), Some(IslandAnchor::End));
        // In front of another island's first member: before that island.
        assert_eq!(island_move_anchor(&s, &a, Some(TabId(7))), Some(IslandAnchor::Before(TabId(7))));
    }

    #[test]
    fn an_island_never_lands_inside_another_or_among_pinned_tabs() {
        let s = strip(&[(1, 'p'), (2, 'p'), (3, 'b'), (4, 'b'), (5, 'b'), (6, '-'), (7, 'a'), (8, 'a')]);
        let a = key("a.com");
        // Between b's members: in front of b.
        assert_eq!(island_move_anchor(&s, &a, Some(TabId(4))), Some(IslandAnchor::Before(TabId(3))));
        assert_eq!(island_move_anchor(&s, &a, Some(TabId(5))), Some(IslandAnchor::Before(TabId(3))));
        // Among the pinned tabs: in front of the first unpinned tab.
        assert_eq!(island_move_anchor(&s, &a, Some(TabId(1))), Some(IslandAnchor::Before(TabId(3))));
        assert_eq!(island_move_anchor(&s, &a, Some(TabId(2))), Some(IslandAnchor::Before(TabId(3))));
        // Only pinned tabs besides the island: after them is where it is already.
        let lone = strip(&[(1, 'p'), (7, 'a'), (8, 'a')]);
        assert_eq!(island_move_anchor(&lone, &a, Some(TabId(1))), None);
        assert_eq!(island_move_anchor(&lone, &a, None), None);
    }

    #[test]
    fn an_island_move_that_changes_nothing_or_names_no_known_tab_is_refused() {
        let s = strip(&[(2, '-'), (3, 'a'), (4, 'a'), (6, '-'), (7, 'b')]);
        let a = key("a.com");
        // Already in front of 6, and its own members are no anchor.
        assert_eq!(island_move_anchor(&s, &a, Some(TabId(6))), None);
        assert_eq!(island_move_anchor(&s, &a, Some(TabId(3))), None);
        assert_eq!(island_move_anchor(&s, &a, Some(TabId(4))), None);
        // An unknown tab, or an island that is not on the strip.
        assert_eq!(island_move_anchor(&s, &a, Some(TabId(42))), None);
        assert_eq!(island_move_anchor(&s, &key("c.com"), Some(TabId(2))), None);
        // At the end already; in front of the first tab is a move.
        let last = strip(&[(2, '-'), (3, 'a'), (4, 'a')]);
        assert_eq!(island_move_anchor(&last, &a, None), None);
        assert_eq!(island_move_anchor(&last, &a, Some(TabId(2))), Some(IslandAnchor::Before(TabId(2))));
    }
}
