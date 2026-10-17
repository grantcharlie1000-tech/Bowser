//! Tab stacks, tiling and workspace switching (T10): the pure rules behind the options of
//! Settings > Tabs and start page > Tab stacks, tiling and workspaces.
//!
//! A *stack* is one of Bowser's site islands (`tab_groups`): the same projection, preferences, names
//! and colours, so there is no second grouping system. The stack style only changes how islands are
//! folded for display ([`collapse_rule`]) or switches them off; stacking by hand places tabs with the
//! island machinery's own manual membership (`tab_organisation::Membership::Joined`). *Tiling* is the
//! split view (`split_view`, up to four panes): [`tile_layout`] turns a list of tabs into one of its
//! layouts. Nothing here touches windows, WebViews, timers or files.

use std::collections::BTreeMap;

use crate::split_view::{Bounds, Orientation, SplitView};
use crate::tab_groups::{CollapseRule, DomainKey};
use crate::tab_options::StackStyle;
use crate::tabs::TabId;
use crate::workspaces::WorkspaceId;

// ----- stack style -----

/// How the stack style folds islands, given the island that holds the tab in use.
pub fn collapse_rule(style: StackStyle, active_island: Option<DomainKey>) -> CollapseRule {
    match style {
        StackStyle::Islands | StackStyle::Off => CollapseRule::Stored,
        // --- two-level stacks (T10b) --- every stack is its ball in the tab row; the second row
        // shows the tabs of the one in use.
        StackStyle::Compact | StackStyle::TwoLevel => CollapseRule::All,
        StackStyle::Accordion => CollapseRule::AllBut(active_island),
    }
}

// --- two-level stacks (T10b) ---
/// The second tab row the window lays out: one only for two-level stacks while the tab in use is in
/// a stack (`active_in_stack`), compact when "Compact second row" is on. Every other style, and a
/// tab in no stack, reserve nothing. (`Arrangement::with_stack_row` drops it again for sidebars.)
pub fn stack_row(style: StackStyle, compact: bool, active_in_stack: bool) -> crate::chrome::layout::StackRow {
    use crate::chrome::layout::StackRow;
    match (style, active_in_stack) {
        (StackStyle::TwoLevel, true) if compact => StackRow::Compact,
        (StackStyle::TwoLevel, true) => StackRow::Regular,
        _ => StackRow::None,
    }
}
// --- end two-level stacks ---

/// Whether any stack forms at all.
pub fn stacks_on(style: StackStyle) -> bool {
    style != StackStyle::Off
}

/// What a click on a stack's ball does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BallClick {
    /// Islands: the ball folds and unfolds the island (the stored collapse flag).
    Toggle,
    /// Compact and Accordion fold by themselves: the click goes to this member instead.
    Select(TabId),
    /// Nothing to do (the stack in use under Accordion is already open).
    Nothing,
}

/// The ball of a stack with `members` (strip order) was clicked while `active` is in front.
/// `recent` lists tabs most recently used last, as the app keeps them (`tab_mru`).
///
/// - From outside the stack, Compact and Accordion bring back the member used last.
/// - Inside a Compact stack the click steps to the next member, so every member can be reached
///   from its ball alone; an open Accordion stack has nothing to unfold, and neither has the
///   two-level stack in use (its tabs are the second row).
pub fn ball_click(style: StackStyle, members: &[TabId], active: TabId, recent: &[TabId]) -> BallClick {
    if style == StackStyle::Islands || style == StackStyle::Off {
        return BallClick::Toggle;
    }
    if members.is_empty() {
        return BallClick::Nothing;
    }
    if let Some(at) = members.iter().position(|member| *member == active) {
        return match style {
            StackStyle::Compact if members.len() > 1 => BallClick::Select(members[(at + 1) % members.len()]),
            _ => BallClick::Nothing,
        };
    }
    let last_used = recent.iter().rev().copied().find(|tab| members.contains(tab));
    BallClick::Select(last_used.unwrap_or(members[0]))
}

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

/// Whether a dragged tab that has rested over another for `rested_ms` stacks with it on release.
pub fn drop_stacks(enabled: bool, rested_ms: u64, delay_ms: u32) -> bool {
    enabled && rested_ms >= u64::from(delay_ms)
}

/// The middle half of a tab: a dragged tab resting there means "stack", nearer the edges it means
/// "move between". `(left, right)` is the tab's span along the strip; `x` the pointer.
pub fn over_middle(left: i32, right: i32, x: i32) -> bool {
    let quarter = (right - left) / 4;
    right > left && x >= left + quarter && x < right - quarter
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
    fn each_style_folds_islands_its_own_way() {
        assert_eq!(collapse_rule(StackStyle::Islands, Some(key("a.com"))), CollapseRule::Stored, "islands keep the user's own collapse");
        assert_eq!(collapse_rule(StackStyle::Compact, None), CollapseRule::All);
        assert_eq!(collapse_rule(StackStyle::Accordion, Some(key("a.com"))), CollapseRule::AllBut(Some(key("a.com"))));
        assert!(stacks_on(StackStyle::Compact) && !stacks_on(StackStyle::Off));
    }

    #[test]
    fn a_ball_click_goes_to_a_member_when_the_style_folds_by_itself() {
        let members = ids(&[2, 3, 4]);
        assert_eq!(ball_click(StackStyle::Islands, &members, TabId(1), &[]), BallClick::Toggle);
        assert_eq!(ball_click(StackStyle::Compact, &members, TabId(1), &ids(&[3, 1])), BallClick::Select(TabId(3)), "the member used last");
        assert_eq!(ball_click(StackStyle::Accordion, &members, TabId(1), &[]), BallClick::Select(TabId(2)), "none used yet: the first");
        assert_eq!(ball_click(StackStyle::Compact, &members, TabId(4), &[]), BallClick::Select(TabId(2)), "inside compact: the next member, round");
        assert_eq!(ball_click(StackStyle::Accordion, &members, TabId(3), &[]), BallClick::Nothing, "the open accordion stack");
        assert_eq!(ball_click(StackStyle::Compact, &[], TabId(1), &[]), BallClick::Nothing);
    }

    // --- two-level stacks (T10b) ---
    #[test]
    fn two_level_folds_every_stack_to_its_ball_and_opens_the_one_in_use_in_a_second_row() {
        use crate::chrome::layout::StackRow;
        assert_eq!(collapse_rule(StackStyle::TwoLevel, Some(key("a.com"))), CollapseRule::All, "the tab row shows balls only");
        assert!(stacks_on(StackStyle::TwoLevel));
        let members = ids(&[2, 3, 4]);
        assert_eq!(ball_click(StackStyle::TwoLevel,&members, TabId(1), &ids(&[4, 1])), BallClick::Select(TabId(4)), "from outside: the member used last");
        assert_eq!(ball_click(StackStyle::TwoLevel, &members, TabId(3), &[]), BallClick::Nothing, "inside: its tabs are the second row already");
        assert_eq!(stack_row(StackStyle::TwoLevel, false, true), StackRow::Regular);
        assert_eq!(stack_row(StackStyle::TwoLevel, true, true), StackRow::Compact, "Compact second row");
        assert_eq!(stack_row(StackStyle::TwoLevel, true, false), StackRow::None, "a tab in no stack: no row");
        assert_eq!(stack_row(StackStyle::Compact, true, true), StackRow::None, "the option belongs to two-level only");
        assert_eq!(StackStyle::TwoLevel.key(), "two-level");
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
    fn a_drop_stacks_only_after_the_delay_and_only_when_allowed() {
        assert!(!drop_stacks(true, 399, 400));
        assert!(drop_stacks(true, 400, 400));
        assert!(drop_stacks(true, 0, 0), "a delay of 0 stacks at once");
        assert!(!drop_stacks(false, 5_000, 0), "off never stacks");
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
}
