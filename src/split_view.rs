//! The relationship between the two tabs of a split view (a tab pair, as in Chrome).
//!
//! A window holds at most one pair. The pair is *shown* while one of its tabs is the active tab and
//! *parked* while another tab is shown alone; the app keeps it in `split_view` or `split_parked`.
//! Everything here is pure and unit-tested; the app and window code apply the results.
//!
//! A pair is either side by side (left | right) or stacked (top over bottom), its [`Orientation`].
//! The two slots keep their historical names in both orientations: [`Pane::Left`] (and the `left`
//! field) is the *first* pane, which is the left pane side by side and the top pane stacked;
//! [`Pane::Right`] (`right`) is the *second* pane, right or bottom. `ratio` is always the first
//! pane's share of the split axis (width side by side, height stacked).
//!
//! **Up to four panes.** Each half of the pair (the *primary* split) may itself be divided once
//! across, perpendicular to the primary split: a side-by-side pair's columns into top and bottom,
//! a stacked pair's rows into left and right. The half's original slot (`left` / `right`) is its
//! *lead* cell (top of a column, left of a row) and `trail` holds the second cell, so 2, 3 ("T")
//! and 4 (2 x 2) pane layouts are one small tree of depth two. Both cross divisions share
//! `cross_ratio`, so a 2 x 2 is a true grid whose dividers cross in one point. With no `trail` the
//! view is exactly the old pair: every pair rule, test, saved line and wire message is unchanged.
//! [`SplitView::cells`] maps the layout onto the page's four quadrants, which is how drops, focus
//! moves and closing a pane are decided.

use crate::tabs::TabId;

/// Smallest and largest share of the split axis the first pane may take, in percent.
pub const MIN_RATIO: u8 = 20;
pub const MAX_RATIO: u8 = 80;
/// Narrowest pane side by side, in device-independent pixels. A viewport that cannot fit two of
/// these plus the gap shows only the focused pane.
pub const MIN_PANE_DIP: i32 = 240;
/// Shortest pane when stacked, in DIP (the same collapse rule along the height).
pub const MIN_PANE_HEIGHT_DIP: i32 = 160;
/// Width of the gap between the two pages, which is also the divider's hit target, in DIP.
pub const GAP_DIP: i32 = 8;

/// How the two pages of a pair share the viewport.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Orientation {
    /// Left | right, divided by a vertical gap (the default, as in Chrome).
    #[default]
    SideBySide,
    /// Top over bottom, divided by a horizontal gap.
    Stacked,
}

impl Orientation {
    pub fn is_stacked(self) -> bool {
        self == Self::Stacked
    }
    pub fn other(self) -> Self {
        match self {
            Self::SideBySide => Self::Stacked,
            Self::Stacked => Self::SideBySide,
        }
    }
    /// The smallest pane along the split axis, in DIP.
    pub fn min_pane_dip(self) -> i32 {
        match self {
            Self::SideBySide => MIN_PANE_DIP,
            Self::Stacked => MIN_PANE_HEIGHT_DIP,
        }
    }
    /// The page edge a pair's `pane` sits against (a trail counts as its half).
    pub fn zone(self, pane: Pane) -> DropZone {
        match (self, pane.half()) {
            (Self::SideBySide, Pane::Left) => DropZone::Left,
            (Self::SideBySide, _) => DropZone::Right,
            (Self::Stacked, Pane::Left) => DropZone::Top,
            (Self::Stacked, _) => DropZone::Bottom,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pane {
    /// The first pane: left side by side, top when stacked. In a divided half, its lead cell (the
    /// top of the left column, or the left of the top row).
    Left,
    /// The second pane: right side by side, bottom when stacked. In a divided half, its lead cell.
    Right,
    /// The trail cell of the first half, when it is divided (bottom of the left column, or right of
    /// the top row).
    LeftTrail,
    /// The trail cell of the second half (bottom of the right column, or right of the bottom row).
    RightTrail,
}

impl Pane {
    /// Every pane in tree order: the first half's lead and trail, then the second half's. The strip
    /// lists a layout's tabs in this order.
    pub const ALL: [Self; 4] = [Self::Left, Self::LeftTrail, Self::Right, Self::RightTrail];

    /// The pane across the primary split (left and right swap; so do the two trails).
    pub fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::LeftTrail => Self::RightTrail,
            Self::RightTrail => Self::LeftTrail,
        }
    }
    /// A stable index (`Left` 0, `Right` 1, `LeftTrail` 2, `RightTrail` 3) for per-pane arrays.
    pub fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::LeftTrail => 2,
            Self::RightTrail => 3,
        }
    }
    /// The half's lead pane (`Left` or `Right`).
    pub fn half(self) -> Self {
        match self {
            Self::Left | Self::LeftTrail => Self::Left,
            Self::Right | Self::RightTrail => Self::Right,
        }
    }
    /// 0 for the first half, 1 for the second.
    pub fn half_index(self) -> usize {
        usize::from(self.half() == Self::Right)
    }
    pub fn is_trail(self) -> bool {
        matches!(self, Self::LeftTrail | Self::RightTrail)
    }
    /// The other cell of the same half.
    pub fn sibling(self) -> Self {
        match self {
            Self::Left => Self::LeftTrail,
            Self::LeftTrail => Self::Left,
            Self::Right => Self::RightTrail,
            Self::RightTrail => Self::Right,
        }
    }
    fn of(half: usize, trail: bool) -> Self {
        match (half, trail) {
            (0, false) => Self::Left,
            (0, true) => Self::LeftTrail,
            (_, false) => Self::Right,
            (_, true) => Self::RightTrail,
        }
    }
}

/// One quarter of the page. A pane covers one quadrant, two (a half) or, alone, all four.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Quadrant {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Quadrant {
    pub const ALL: [Self; 4] = [Self::TopLeft, Self::TopRight, Self::BottomLeft, Self::BottomRight];
    /// Row-major index: top-left 0, top-right 1, bottom-left 2, bottom-right 3.
    pub fn index(self) -> usize {
        self.row() * 2 + self.col()
    }
    pub fn row(self) -> usize {
        usize::from(matches!(self, Self::BottomLeft | Self::BottomRight))
    }
    pub fn col(self) -> usize {
        usize::from(matches!(self, Self::TopRight | Self::BottomRight))
    }
    pub fn at(row: usize, col: usize) -> Self {
        match (row, col) {
            (0, 0) => Self::TopLeft,
            (0, _) => Self::TopRight,
            (_, 0) => Self::BottomLeft,
            _ => Self::BottomRight,
        }
    }
    /// The quadrant of cell `part` (0 lead, 1 trail) of primary half `half` in `orientation`.
    pub fn of_cell(orientation: Orientation, half: usize, part: usize) -> Self {
        match orientation {
            Orientation::SideBySide => Self::at(part, half),
            Orientation::Stacked => Self::at(half, part),
        }
    }
    /// The corner drop zone of this quadrant.
    pub fn zone(self) -> DropZone {
        match self {
            Self::TopLeft => DropZone::TopLeft,
            Self::TopRight => DropZone::TopRight,
            Self::BottomLeft => DropZone::BottomLeft,
            Self::BottomRight => DropZone::BottomRight,
        }
    }
}

/// A direction for moving the focus between panes (Ctrl+Shift+arrows).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitView {
    /// The first pane's tab (left, or top when stacked); the first half's lead cell when divided.
    pub left: TabId,
    /// The second pane's tab (right, or bottom when stacked); the second half's lead cell.
    pub right: TabId,
    pub focused: Pane,
    /// Percentage of the split axis assigned to the first pane (`MIN_RATIO` through `MAX_RATIO`).
    pub ratio: u8,
    pub orientation: Orientation,
    /// The trail cell of each half (`[first, second]`); `None` leaves that half undivided. Both
    /// `None` is a plain pair.
    pub trail: [Option<TabId>; 2],
    /// The lead cells' share of the cross axis (height of a column's top cell side by side, width
    /// of a row's left cell stacked), shared by both divided halves. Unused by a plain pair.
    pub cross_ratio: u8,
}

/// What is left when a tab leaves a layout ([`SplitView::without`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Remains {
    /// A pair lost a tab: the survivor becomes a normal tab, shown alone.
    Single(TabId),
    /// A layout of three or four panes lost one: the smaller layout (siblings expand).
    Layout(SplitView),
}

impl SplitView {
    /// A side-by-side pair at 50/50.
    pub fn new(left: TabId, right: TabId, focused: Pane) -> Option<Self> {
        (left != right).then_some(Self {
            left,
            right,
            focused: focused.half(),
            ratio: 50,
            orientation: Orientation::SideBySide,
            trail: [None, None],
            cross_ratio: 50,
        })
    }
    /// `tab` against page edge `zone` (focused) and `other` on the opposite side, at 50/50.
    pub fn at_edge(tab: TabId, other: TabId, zone: DropZone) -> Option<Self> {
        let (left, right) = match zone.pane() {
            Pane::Left => (tab, other),
            _ => (other, tab),
        };
        Self::new(left, right, zone.pane()).map(|view| view.with_orientation(zone.orientation()))
    }
    pub fn with_orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }
    /// A plain pair: no half is divided.
    pub fn is_pair(self) -> bool {
        self.trail == [None, None]
    }
    /// How many panes the layout shows (2 to 4).
    pub fn pane_count(self) -> usize {
        2 + self.trail.iter().flatten().count()
    }
    /// Whether any two panes sit one above the other (a stacked pair, or any divided layout), so
    /// Ctrl+Shift+Up / Down have somewhere to go.
    pub fn has_rows(self) -> bool {
        self.orientation.is_stacked() || !self.is_pair()
    }
    /// Whether half `half` (0 first, 1 second) is divided across.
    pub fn divided(self, half: usize) -> bool {
        self.trail.get(half).is_some_and(Option::is_some)
    }
    /// The page edge (a half) or corner (a quadrant) the focused pane covers.
    pub fn focused_zone(self) -> DropZone {
        self.zone_of(self.focused)
    }
    pub fn focused_tab(self) -> TabId {
        self.tab(self.focused)
    }
    /// The lead tab of the half across the primary split from the focused pane (a pair's other tab).
    pub fn other_tab(self) -> TabId {
        self.tab(self.focused.half().other())
    }
    /// The tab in `pane`, or `None` for the trail of an undivided half.
    pub fn tab_at(self, pane: Pane) -> Option<TabId> {
        match pane {
            Pane::Left => Some(self.left),
            Pane::Right => Some(self.right),
            Pane::LeftTrail => self.trail[0],
            Pane::RightTrail => self.trail[1],
        }
    }
    /// The tab in `pane`; the trail of an undivided half reads as its lead (the cell it covers).
    pub fn tab(self, pane: Pane) -> TabId {
        self.tab_at(pane).unwrap_or_else(|| self.tab(pane.half()))
    }
    fn set_tab(&mut self, pane: Pane, tab: TabId) {
        match pane {
            Pane::Left => self.left = tab,
            Pane::Right => self.right = tab,
            Pane::LeftTrail => self.trail[0] = Some(tab),
            Pane::RightTrail => self.trail[1] = Some(tab),
        }
    }
    /// The panes that exist, in tree order ([`Pane::ALL`]).
    pub fn panes(self) -> impl Iterator<Item = (Pane, TabId)> {
        Pane::ALL.into_iter().filter_map(move |pane| self.tab_at(pane).map(|tab| (pane, tab)))
    }
    /// The layout's tabs in tree order.
    pub fn tabs(self) -> impl Iterator<Item = TabId> {
        self.panes().map(|(_, tab)| tab)
    }
    /// The focused pane first, then the others in tree order.
    pub fn panes_focus_first(self) -> impl Iterator<Item = Pane> {
        std::iter::once(self.focused).chain(self.panes().map(|(pane, _)| pane).filter(move |pane| *pane != self.focused))
    }
    pub fn pane_for(self, tab: TabId) -> Option<Pane> {
        self.panes().find(|(_, t)| *t == tab).map(|(pane, _)| pane)
    }
    pub fn contains(self, tab: TabId) -> bool {
        self.pane_for(tab).is_some()
    }
    /// Focuses `pane`; a trail that does not exist is ignored.
    pub fn focus(&mut self, pane: Pane) {
        if self.tab_at(pane).is_some() {
            self.focused = pane;
        }
    }
    pub fn set_ratio(&mut self, ratio: u8) {
        self.ratio = ratio.clamp(MIN_RATIO, MAX_RATIO);
    }
    pub fn set_cross_ratio(&mut self, ratio: u8) {
        self.cross_ratio = ratio.clamp(MIN_RATIO, MAX_RATIO);
    }
    /// The ratios as page axes: `(width share of the left column, height share of the top row)`.
    pub fn col_row_ratios(self) -> (u8, u8) {
        match self.orientation {
            Orientation::SideBySide => (self.ratio, self.cross_ratio),
            Orientation::Stacked => (self.cross_ratio, self.ratio),
        }
    }
    /// Mirrors the layout across the primary divider: the halves trade places (with their trails),
    /// the focus follows its tab and the ratio is mirrored. A pair swaps its sides.
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.left, &mut self.right);
        self.trail.swap(0, 1);
        self.focused = self.focused.other();
        self.ratio = 100 - self.ratio;
    }
    /// Swaps the two cells of divided half `half` (the divider inside it). The cross ratio is
    /// shared with the other half, so it stays. False when that half is not divided.
    pub fn swap_cross(&mut self, half: usize) -> bool {
        let lead = Pane::of(half, false);
        let Some(trail) = self.tab_at(lead.sibling()) else {
            return false;
        };
        let lead_tab = self.tab(lead);
        self.set_tab(lead, trail);
        self.set_tab(lead.sibling(), lead_tab);
        if self.focused.half() == lead {
            self.focused = self.focused.sibling();
        }
        true
    }
    /// Side by side becomes stacked and back. The first tab stays first (left becomes top), the
    /// focus and the ratio stay. A divided layout is transposed the same way (columns become rows).
    pub fn turn(&mut self) {
        self.orientation = self.orientation.other();
    }
    /// The focused pane shows `tab` instead (a tab outside the layout selected while it is shown).
    /// False when `tab` is already another pane.
    pub fn replace_focused(&mut self, tab: TabId) -> bool {
        if self.contains(tab) && tab != self.focused_tab() {
            return false;
        }
        let pane = self.focused;
        self.set_tab(pane, tab);
        true
    }
    /// The tab that is shown when `tab` closes (a pair's survivor, or the smaller layout's focused
    /// tab), or `None` when `tab` is not in the layout.
    pub fn remove(self, tab: TabId) -> Option<TabId> {
        match self.without(tab)? {
            Remains::Single(survivor) => Some(survivor),
            Remains::Layout(view) => Some(view.focused_tab()),
        }
    }
    /// The layout without `tab`, or `None` when `tab` is not in it. The pane's sibling expands into
    /// its space: the other cell of its half, or, when it was a whole half, the other half (whose
    /// division becomes the new primary split). The focus stays on its tab, or moves to the pane
    /// that took the closed pane's space.
    pub fn without(self, tab: TabId) -> Option<Remains> {
        let pane = self.pane_for(tab)?;
        if self.is_pair() {
            return Some(Remains::Single(self.tab(pane.other())));
        }
        let focused_tab = self.focused_tab();
        let mut out = self;
        let half = pane.half_index();
        if self.divided(half) {
            // The half's other cell takes the whole half.
            let survivor = self.tab(pane.sibling());
            out.set_tab(pane.half(), survivor);
            out.trail[half] = None;
            out.focused = if focused_tab == tab { pane.half() } else { out.pane_for(focused_tab).unwrap_or(pane.half()) };
            return Some(Remains::Layout(out));
        }
        // A whole half closes: the other half, divided, becomes a pair across the old cross axis.
        let other = 1 - half;
        let lead = self.tab(Pane::of(other, false));
        let trail = self.tab_at(Pane::of(other, true))?;
        let mut pair = Self::new(lead, trail, Pane::Left)?.with_orientation(self.orientation.other());
        pair.ratio = self.cross_ratio;
        pair.focused = if focused_tab == trail { Pane::Right } else { Pane::Left };
        Some(Remains::Layout(pair))
    }
    /// Closing the tab in `pane`: `(closing, shown)`, where `shown` is the tab shown afterwards (a
    /// pair's survivor, which becomes a normal tab).
    pub fn close(self, pane: Pane) -> (TabId, TabId) {
        let closing = self.tab(pane);
        (closing, self.remove(closing).unwrap_or(self.tab(pane.other())))
    }
    /// "Separate views": every tab stays open as a normal tab; the focused one stays shown.
    pub fn separate(self) -> TabId {
        self.focused_tab()
    }
    /// The empty pane (`empty`, a tab with no page) takes `picked`. Fails when `empty` is not in the
    /// layout or `picked` already is.
    pub fn fill_empty(&mut self, empty: TabId, picked: TabId) -> bool {
        if self.contains(picked) {
            return false;
        }
        let Some(pane) = self.pane_for(empty) else {
            return false;
        };
        self.set_tab(pane, picked);
        self.focused = pane;
        true
    }

    // ----- the layout on the page's quadrants -----

    /// The tab covering each quadrant (row-major, [`Quadrant::index`]). A half covers two cells.
    pub fn cells(self) -> [TabId; 4] {
        let mut cells = [self.left; 4];
        for half in 0..2 {
            for part in 0..2 {
                cells[Quadrant::of_cell(self.orientation, half, part).index()] = self.tab(Pane::of(half, part == 1));
            }
        }
        cells
    }
    /// The quadrants `pane` covers (one or two; none for a missing trail).
    pub fn quadrants(self, pane: Pane) -> Vec<Quadrant> {
        let Some(tab) = self.tab_at(pane) else { return Vec::new() };
        let cells = self.cells();
        Quadrant::ALL.into_iter().filter(|q| cells[q.index()] == tab).collect()
    }
    /// The pane covering quadrant `q`.
    pub fn pane_at(self, q: Quadrant) -> Pane {
        let half = match self.orientation {
            Orientation::SideBySide => q.col(),
            Orientation::Stacked => q.row(),
        };
        let part = match self.orientation {
            Orientation::SideBySide => q.row(),
            Orientation::Stacked => q.col(),
        };
        let pane = Pane::of(half, part == 1);
        if self.tab_at(pane).is_some() { pane } else { pane.half() }
    }
    /// Where `pane` sits: an edge for a half (`Left`, `Right`, `Top`, `Bottom`) or a corner for a
    /// quadrant.
    pub fn zone_of(self, pane: Pane) -> DropZone {
        let quadrants = self.quadrants(pane);
        match quadrants.as_slice() {
            [q] => q.zone(),
            [a, b] if a.col() == b.col() => if a.col() == 0 { DropZone::Left } else { DropZone::Right },
            [a, _] => if a.row() == 0 { DropZone::Top } else { DropZone::Bottom },
            _ => self.orientation.zone(pane.half()),
        }
    }
    /// Builds a layout from a quadrant map: the primary split runs along `prefer` when the map
    /// allows it, otherwise along the other axis. `(col, row)` are the page-axis ratios to keep and
    /// `focused` the tab to focus. `None` when the map is not a 2, 3 or 4 pane layout (one tab
    /// everywhere, or a tab in two cells that do not share a side).
    pub fn from_cells(cells: [TabId; 4], prefer: Orientation, col: u8, row: u8, focused: TabId) -> Option<Self> {
        for orientation in [prefer, prefer.other()] {
            let half = |h: usize| (cells[Quadrant::of_cell(orientation, h, 0).index()], cells[Quadrant::of_cell(orientation, h, 1).index()]);
            let ((a0, a1), (b0, b1)) = (half(0), half(1));
            if a0 == b0 || a0 == b1 || a1 == b0 || a1 == b1 {
                continue;
            }
            let (ratio, cross) = match orientation {
                Orientation::SideBySide => (col, row),
                Orientation::Stacked => (row, col),
            };
            let mut view = Self::new(a0, b0, Pane::Left)?.with_orientation(orientation);
            view.trail = [(a1 != a0).then_some(a1), (b1 != b0).then_some(b1)];
            view.set_ratio(ratio);
            view.set_cross_ratio(cross);
            view.focused = view.pane_for(focused).unwrap_or(Pane::Left);
            return Some(view);
        }
        None
    }
    /// The pane next to the focused one in `direction`, or `None` at the page's edge.
    ///
    /// A pair keeps its historical keys: Left / Up focus the first pane and Right / Down the second,
    /// in either orientation. A larger layout moves geometrically through the quadrants; when two
    /// panes border the focused one on that side, the top (or left) one wins.
    pub fn neighbour(self, direction: Direction) -> Option<Pane> {
        if self.is_pair() {
            return Some(match direction {
                Direction::Left | Direction::Up => Pane::Left,
                Direction::Right | Direction::Down => Pane::Right,
            });
        }
        let cells = self.cells();
        let mine = self.focused_tab();
        Quadrant::ALL
            .into_iter()
            .filter(|q| cells[q.index()] == mine)
            .filter_map(|q| {
                let (row, col) = (q.row() as i32, q.col() as i32);
                let (row, col) = match direction {
                    Direction::Left => (row, col - 1),
                    Direction::Right => (row, col + 1),
                    Direction::Up => (row - 1, col),
                    Direction::Down => (row + 1, col),
                };
                ((0..2).contains(&row) && (0..2).contains(&col)).then(|| Quadrant::at(row as usize, col as usize))
            })
            .find(|q| cells[q.index()] != mine)
            .map(|q| self.pane_at(q))
    }
}

/// The divider ratio for a gap whose leading edge is at `divider_left` inside a viewport spanning
/// `left..right` along the split axis (physical pixels; pass top and bottom for a stacked pair),
/// measured at the gap's centre and clamped to the ratio range.
pub fn ratio_for_divider(left: i32, right: i32, divider_left: i32, gap: i32) -> u8 {
    let width = (right - left).max(1);
    let centre = divider_left + gap / 2 - left;
    let percent = (centre * 100 + width / 2) / width;
    percent.clamp(i32::from(MIN_RATIO), i32::from(MAX_RATIO)) as u8
}

/// The gap's leading edge for `ratio` in a viewport spanning `left..right` along the split axis
/// (physical pixels; top and bottom for a stacked pair).
pub fn divider_for_ratio(left: i32, right: i32, ratio: u8, gap: i32) -> i32 {
    left + (right - left) * i32::from(ratio) / 100 - gap / 2
}

/// The span of the split axis in a `(left, top, right, bottom)` viewport: x side by side, y stacked.
pub fn axis_span(viewport: (i32, i32, i32, i32), orientation: Orientation) -> (i32, i32) {
    let (left, top, right, bottom) = viewport;
    match orientation {
        Orientation::SideBySide => (left, right),
        Orientation::Stacked => (top, bottom),
    }
}

/// The coordinate of `(x, y)` along the split axis.
pub fn axis_coord(orientation: Orientation, x: i32, y: i32) -> i32 {
    match orientation {
        Orientation::SideBySide => x,
        Orientation::Stacked => y,
    }
}

/// Which side of the viewport `left..right` the point `x` falls on, for a drop target.
pub fn side_of(left: i32, right: i32, x: i32) -> Option<Pane> {
    if x < left || x >= right {
        return None;
    }
    Some(if x < left + (right - left) / 2 {
        Pane::Left
    } else {
        Pane::Right
    })
}

// ----- dropping a dragged tab on an edge of the page -----

/// The page edge a dragged tab is dropped against; it opens on that half of the page. The four
/// corners ([`DropZone::CORNERS`]) open it in that quarter of the page (a split of the pane there).
/// The same names describe where a pane sits ([`SplitView::zone_of`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DropZone {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl DropZone {
    /// The four edges (halves).
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];
    /// The four corners (quadrants).
    pub const CORNERS: [Self; 4] = [Self::TopLeft, Self::TopRight, Self::BottomLeft, Self::BottomRight];
    /// The slot the dropped tab takes: the first pane for left and top, the second for right and
    /// bottom. A corner counts as its column's edge (only edges make pairs).
    pub fn pane(self) -> Pane {
        match self {
            Self::Left | Self::Top | Self::TopLeft | Self::BottomLeft => Pane::Left,
            Self::Right | Self::Bottom | Self::TopRight | Self::BottomRight => Pane::Right,
        }
    }
    pub fn orientation(self) -> Orientation {
        match self {
            Self::Top | Self::Bottom => Orientation::Stacked,
            _ => Orientation::SideBySide,
        }
    }
    pub fn is_corner(self) -> bool {
        self.quadrant().is_some()
    }
    /// A corner's quadrant.
    pub fn quadrant(self) -> Option<Quadrant> {
        match self {
            Self::TopLeft => Some(Quadrant::TopLeft),
            Self::TopRight => Some(Quadrant::TopRight),
            Self::BottomLeft => Some(Quadrant::BottomLeft),
            Self::BottomRight => Some(Quadrant::BottomRight),
            _ => None,
        }
    }
    /// The quadrants the zone covers: two for an edge, one for a corner.
    pub fn quadrants(self) -> Vec<Quadrant> {
        match self {
            Self::Left => vec![Quadrant::TopLeft, Quadrant::BottomLeft],
            Self::Right => vec![Quadrant::TopRight, Quadrant::BottomRight],
            Self::Top => vec![Quadrant::TopLeft, Quadrant::TopRight],
            Self::Bottom => vec![Quadrant::BottomLeft, Quadrant::BottomRight],
            corner => corner.quadrant().into_iter().collect(),
        }
    }
}

/// Half-size of the page's central box, in per-mille of each axis, where a drag keeps the zone it
/// had (no flicker while crossing the middle).
pub const DROP_DEAD_ZONE_PERMILLE: i32 = 100;
/// How much nearer (per-mille) another edge must be before the zone leaves the previous one, so the
/// preview does not flicker along the diagonals that separate the zones.
pub const DROP_HYSTERESIS_PERMILLE: i32 = 40;

/// The edge zone of the page `(left, top, right, bottom)` under the point `(x, y)`, or `None` when
/// the point is outside the page.
///
/// The nearest edge wins, each distance measured as a fraction of the page's width (left, right) or
/// height (top, bottom), which cuts the page along its diagonals into four triangles. Ties go to
/// left/right. Near a diagonal the `previous` zone stays until another edge is clearly nearer
/// ([`DROP_HYSTERESIS_PERMILLE`]); in the central box ([`DROP_DEAD_ZONE_PERMILLE`]) the previous
/// zone stays, or with none the left/right half the point is on, so every point of the page has a
/// zone.
pub fn drop_zone(page: (i32, i32, i32, i32), x: i32, y: i32, previous: Option<DropZone>) -> Option<DropZone> {
    let (left, top, right, bottom) = page;
    if x < left || x >= right || y < top || y >= bottom {
        return None;
    }
    let previous = previous.filter(|zone| !zone.is_corner());
    let per_mille = |value: i32, start: i32, end: i32| -> i32 {
        let span = i64::from((end - start).max(1));
        (i64::from(value - start) * 1000 / span) as i32
    };
    let u = per_mille(x, left, right);
    let v = per_mille(y, top, bottom);
    let distance = |zone: DropZone| match zone {
        DropZone::Left => u,
        DropZone::Right => 1000 - u,
        DropZone::Top => v,
        DropZone::Bottom => 1000 - v,
        _ => 1000,
    };
    if (u - 500).abs() < DROP_DEAD_ZONE_PERMILLE && (v - 500).abs() < DROP_DEAD_ZONE_PERMILLE {
        return previous.or(Some(if u < 500 { DropZone::Left } else { DropZone::Right }));
    }
    let nearest = DropZone::ALL
        .into_iter()
        .min_by_key(|zone| distance(*zone))
        .unwrap_or(DropZone::Left);
    match previous {
        Some(previous) if distance(previous) - distance(nearest) < DROP_HYSTERESIS_PERMILLE => Some(previous),
        _ => Some(nearest),
    }
}

/// The half of the page `(left, top, right, bottom)` a tab dropped on `zone` opens in, with the page
/// gap taken out of the middle (the same split a new 50/50 pair gets). A corner gives that quarter.
pub fn drop_rect(page: (i32, i32, i32, i32), zone: DropZone, gap: i32) -> (i32, i32, i32, i32) {
    let (left, top, right, bottom) = page;
    if let Some(q) = zone.quadrant() {
        let column = drop_rect(page, if q.col() == 0 { DropZone::Left } else { DropZone::Right }, gap);
        let row = drop_rect(page, if q.row() == 0 { DropZone::Top } else { DropZone::Bottom }, gap);
        return (column.0, row.1, column.2, row.3);
    }
    let (start, end) = axis_span(page, zone.orientation());
    let mid = start + (end - start) / 2;
    let (near_end, far_start) = (mid - gap / 2, mid + gap - gap / 2);
    match zone {
        DropZone::Left => (left, top, near_end, bottom),
        DropZone::Right => (far_start, top, right, bottom),
        DropZone::Top => (left, top, right, near_end),
        _ => (left, far_start, right, bottom),
    }
}

/// How close to a corner (per-mille of both axes) the pointer must come for the corner zone.
pub const CORNER_PERMILLE: i32 = 250;
/// How far past [`CORNER_PERMILLE`] a corner zone that is already shown holds, so the preview does
/// not flicker on the corner's boundary.
pub const CORNER_HYSTERESIS_PERMILLE: i32 = 50;

/// [`drop_zone`] with the page's corners: within [`CORNER_PERMILLE`] of a corner along both axes
/// (once shown, until [`CORNER_PERMILLE`] + [`CORNER_HYSTERESIS_PERMILLE`]) the zone is that corner;
/// everywhere else the edge rule applies unchanged. `corners` is false when a corner could not add
/// or take a pane (no split, or a pair's own half being dragged: [`corners_allowed`]), which leaves
/// the old edge-only behaviour.
pub fn drop_zone_cornered(page: (i32, i32, i32, i32), x: i32, y: i32, previous: Option<DropZone>, corners: bool) -> Option<DropZone> {
    let (left, top, right, bottom) = page;
    if x < left || x >= right || y < top || y >= bottom {
        return None;
    }
    if corners {
        let per_mille = |value: i32, start: i32, end: i32| -> i32 {
            let span = i64::from((end - start).max(1));
            (i64::from(value - start) * 1000 / span) as i32
        };
        let (u, v) = (per_mille(x, left, right), per_mille(y, top, bottom));
        let corner = Quadrant::at(usize::from(v >= 500), usize::from(u >= 500)).zone();
        let near_x = u.min(1000 - u);
        let near_y = v.min(1000 - v);
        let limit = if previous == Some(corner) { CORNER_PERMILLE + CORNER_HYSTERESIS_PERMILLE } else { CORNER_PERMILLE };
        if near_x < limit && near_y < limit {
            return Some(corner);
        }
    }
    drop_zone(page, x, y, previous)
}

/// Whether the corner zones are offered for dragging `dragged` over the shown layout `view`: only
/// when a split is shown, and not for a half of a plain pair (whose drags keep re-arranging the
/// pair along the edges).
pub fn corners_allowed(view: Option<SplitView>, dragged: TabId) -> bool {
    view.is_some_and(|view| !view.is_pair() || !view.contains(dragged))
}

/// What dropping a dragged tab on a page edge does (see [`plan_drop`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DropPlan {
    /// Show `view` (the dragged tab focused, against the zone's edge). `leaving` is a tab of the
    /// window's old pair that is no longer in it: it stays open as a normal tab, or closes when it
    /// was the pair's empty side.
    Pair { view: SplitView, leaving: Option<TabId> },
    /// The dragged tab is the one shown alone: it moves to the zone's half and the caller opens a
    /// new empty side opposite `zone`.
    WithEmpty { tab: TabId, zone: DropZone },
    /// A corner drop, or any drop on a layout of three or four panes ([`plan_layout_drop`]): show
    /// `view`. `leaving` are tabs of the old layout no longer in it (they stay open as normal tabs,
    /// or close when they were an empty pane).
    Layout { view: SplitView, leaving: [Option<TabId>; 2] },
}

impl DropPlan {
    /// The layout the plan shows, when it is known up front (not for [`DropPlan::WithEmpty`]).
    pub fn view(self) -> Option<SplitView> {
        match self {
            Self::Pair { view, .. } | Self::Layout { view, .. } => Some(view),
            Self::WithEmpty { .. } => None,
        }
    }
}

/// Where `dragged`, dropped on `zone`, ends up. One rule for every case: **the dragged tab takes the
/// zone's half of the page, and the tab it is paired with takes the other half**, in the zone's
/// orientation.
///
/// - No pair (`pair` is `None`): the partner is the `active` tab; dragging the active tab itself
///   gives it an empty side instead ([`DropPlan::WithEmpty`]).
/// - A shown pair, dragged tab inside it: the partner is the other half, so a drag re-arranges the
///   pair (swap, or turn side by side into stacked). Dropping a half where it already is: `None`.
/// - A shown pair, dragged tab outside it: the partner is the focused half (the page being worked
///   on), unless that is the empty side, then the other half. The remaining half leaves the pair.
///
/// The ratio follows the partner: kept when the partner stays in its slot, mirrored when it moves
/// to the other slot, and reset to 50/50 when the orientation changes.
pub fn plan_drop(pair: Option<SplitView>, empty: Option<TabId>, active: TabId, dragged: TabId, zone: DropZone) -> Option<DropPlan> {
    if zone.is_corner() || pair.is_some_and(|pair| !pair.is_pair()) {
        return plan_layout_drop(pair?, empty, dragged, zone);
    }
    let Some(pair) = pair else {
        if dragged == active {
            return Some(DropPlan::WithEmpty { tab: dragged, zone });
        }
        let view = SplitView::at_edge(dragged, active, zone)?;
        return Some(DropPlan::Pair { view, leaving: None });
    };
    let (partner, leaving) = match pair.pane_for(dragged) {
        Some(pane) => {
            if pane == zone.pane() && pair.orientation == zone.orientation() {
                return None;
            }
            (pair.tab(pane.other()), None)
        }
        None => {
            let partner = if empty == Some(pair.focused_tab()) { pair.other_tab() } else { pair.focused_tab() };
            let leaving = if partner == pair.left { pair.right } else { pair.left };
            (partner, Some(leaving))
        }
    };
    let mut view = SplitView::at_edge(dragged, partner, zone)?;
    if pair.orientation == zone.orientation() {
        let partner_pane = zone.pane().other();
        view.set_ratio(if pair.pane_for(partner) == Some(partner_pane) { pair.ratio } else { 100 - pair.ratio });
    }
    Some(DropPlan::Pair { view, leaving })
}

/// A drop that builds or changes a layout of three or four panes, decided on the page's quadrants
/// ([`SplitView::cells`]):
///
/// - **Corner**: the dragged tab takes that quadrant. When the pane there is a whole half, the half
///   is divided and its tab keeps the other cell (one more pane, up to four); when it is already a
///   single quadrant (four panes, or a divided half), the dragged tab replaces it and that tab
///   leaves the layout. A corner over the empty pane fills that pane.
/// - **Edge** (only on three or four panes): the dragged tab takes that whole half; the tabs that
///   were only in that half leave the layout, the other half stays as it is.
/// - **A pane of the layout dragged**: onto another pane that is exactly the target quadrant, the
///   two tabs trade panes (nothing leaves); onto its own place, nothing (`None`); otherwise it
///   leaves its pane first (siblings expand, [`SplitView::without`]) and is dropped like an outside
///   tab.
///
/// The page-axis ratios stay (a newly divided half uses the shared cross ratio) and the dragged tab
/// is focused. `empty` is the layout's empty pane, if any.
pub fn plan_layout_drop(view: SplitView, empty: Option<TabId>, dragged: TabId, zone: DropZone) -> Option<DropPlan> {
    let (col, row) = view.col_row_ratios();
    let mut base = view;
    if let Some(pane) = view.pane_for(dragged) {
        let mine = view.quadrants(pane);
        if let Some(q) = zone.quadrant() {
            let target = view.pane_at(q);
            if target == pane && mine.len() == 1 {
                return None;
            }
            if target != pane && view.quadrants(target).len() == 1 {
                let mut out = view;
                out.set_tab(pane, view.tab(target));
                out.set_tab(target, dragged);
                out.focused = target;
                return Some(DropPlan::Layout { view: out, leaving: [None, None] });
            }
        } else if mine == zone.quadrants() {
            return None;
        }
        match view.without(dragged)? {
            Remains::Layout(rest) => base = rest,
            Remains::Single(_) => return None,
        }
    }
    let mut cells = base.cells();
    match zone.quadrant() {
        Some(q) => {
            let under = cells[q.index()];
            if Some(under) == empty {
                cells.iter_mut().filter(|cell| **cell == under).for_each(|cell| *cell = dragged);
            } else {
                cells[q.index()] = dragged;
            }
        }
        None => zone.quadrants().into_iter().for_each(|q| cells[q.index()] = dragged),
    }
    let prefer = if zone.is_corner() { base.orientation } else { zone.orientation() };
    let out = SplitView::from_cells(cells, prefer, col, row, dragged)?;
    let mut leaving = [None, None];
    for (slot, tab) in view.tabs().filter(|tab| *tab != dragged && !out.contains(*tab)).take(2).enumerate() {
        leaving[slot] = Some(tab);
    }
    Some(DropPlan::Layout { view: out, leaving })
}

/// A rectangle `(left, top, right, bottom)` in physical pixels.
pub type Bounds = (i32, i32, i32, i32);

/// One value per pane, indexed by [`Pane::index`]; `None` for a pane that does not exist or is not
/// shown.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PaneMap<T>(pub [Option<T>; 4]);

impl<T: Copy> PaneMap<T> {
    /// A pair's two panes.
    pub fn pair(first: T, second: T) -> Self {
        let mut map = Self([None; 4]);
        map.set(Pane::Left, Some(first));
        map.set(Pane::Right, Some(second));
        map
    }
    pub fn get(&self, pane: Pane) -> Option<T> {
        self.0[pane.index()]
    }
    pub fn set(&mut self, pane: Pane, value: Option<T>) {
        self.0[pane.index()] = value;
    }
    /// The panes that have a value, in tree order.
    pub fn iter(&self) -> impl Iterator<Item = (Pane, T)> + '_ {
        Pane::ALL.into_iter().filter_map(|pane| self.get(pane).map(|value| (pane, value)))
    }
    pub fn map<U: Copy>(&self, f: impl Fn(T) -> U) -> PaneMap<U> {
        PaneMap(self.0.map(|value| value.map(&f)))
    }
}

/// A divider of a layout: the full-length primary gap, the cross gap inside a divided half, or (a
/// hit at the crossing of a 2 x 2) both at once.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Divider {
    Primary,
    /// The cross gap of half 0 or 1. Both share one ratio, so dragging either moves both.
    Cross(usize),
    /// The crossing point of the primary gap and the cross gaps' line: dragging moves both ratios.
    Both,
}

/// Where a layout's panes and gaps fall on the page.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Geometry {
    /// Each shown pane's rectangle. A pane hidden by the collapse rule has none.
    pub panes: PaneMap<Bounds>,
    /// The full-length gap between the two halves.
    pub primary: Bounds,
    /// The gap inside each divided half that is shown divided.
    pub cross: [Option<Bounds>; 2],
}

impl Geometry {
    /// The divider under `(x, y)`. Inside the primary gap at the height (or, stacked, the x) of a
    /// cross gap is the crossing point, which counts as both.
    pub fn divider_at(&self, x: i32, y: i32, orientation: Orientation) -> Option<Divider> {
        let inside = |(l, t, r, b): Bounds| x >= l && x < r && y >= t && y < b;
        if inside(self.primary) {
            let crossing = self.cross.iter().flatten().any(|&(l, t, r, b)| match orientation {
                Orientation::SideBySide => y >= t && y < b,
                Orientation::Stacked => x >= l && x < r,
            });
            return Some(if crossing { Divider::Both } else { Divider::Primary });
        }
        (0..2).find(|half| self.cross[*half].is_some_and(inside)).map(Divider::Cross)
    }
    /// The pane at `(x, y)`, or the nearest shown pane (for a click in a gap or a margin).
    pub fn pane_near(&self, x: i32, y: i32) -> Option<Pane> {
        let distance = |(l, t, r, b): Bounds| {
            let dx = if x < l { l - x } else if x >= r { x - r + 1 } else { 0 };
            let dy = if y < t { t - y } else if y >= b { y - b + 1 } else { 0 };
            i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy)
        };
        self.panes.iter().min_by_key(|(_, rect)| distance(*rect)).map(|(pane, _)| pane)
    }
    /// The gap rectangle of `divider` (the primary gap for [`Divider::Both`]).
    pub fn divider_rect(&self, divider: Divider) -> Option<Bounds> {
        match divider {
            Divider::Primary | Divider::Both => Some(self.primary),
            Divider::Cross(half) => self.cross.get(half).copied().flatten(),
        }
    }
}

/// Cuts `start..end` at a gap whose leading edge is `at`, clamped so both sides keep `min`:
/// `(first side's end, second side's start)`, or `None` when the span cannot hold two.
fn cut(start: i32, end: i32, at: i32, min: i32, gap: i32) -> Option<(i32, i32)> {
    let (min, gap) = (min.max(1), gap.max(1));
    let low = start.saturating_add(min);
    let high = end.saturating_sub(min.saturating_add(gap));
    if high < low {
        return None;
    }
    let at = at.clamp(low, high);
    Some((at, at.saturating_add(gap)))
}

/// The layout of `view` on `page`: the primary gap's leading edge at `primary_at` and the cross
/// gaps' at `cross_at` (both clamped so every pane keeps `min_w` x `min_h`), `gap` wide.
///
/// Collapse rules (no tab is ever dropped; everything comes back when the page grows):
/// - the page cannot hold two panes along the primary axis: `None` (the window then shows only the
///   focused pane over the whole page, as a pair always did);
/// - a divided half cannot hold two cells across: that half shows one pane, the focused one when it
///   is there, else its lead, and the other gets no rectangle (it is hidden).
pub fn geometry(view: SplitView, page: Bounds, primary_at: i32, cross_at: i32, min_w: i32, min_h: i32, gap: i32) -> Option<Geometry> {
    let (left, top, right, bottom) = page;
    if right <= left || bottom <= top {
        return None;
    }
    let min_along = |orientation: Orientation| match orientation {
        Orientation::SideBySide => min_w,
        Orientation::Stacked => min_h,
    };
    let (start, end) = axis_span(page, view.orientation);
    let (near, far) = cut(start, end, primary_at, min_along(view.orientation), gap)?;
    let (halves, primary) = match view.orientation {
        Orientation::SideBySide => ([(left, top, near, bottom), (far, top, right, bottom)], (near, top, far, bottom)),
        Orientation::Stacked => ([(left, top, right, near), (left, far, right, bottom)], (left, near, right, far)),
    };
    let across = view.orientation.other();
    let (cross_start, cross_end) = axis_span(page, across);
    let cross_cut = cut(cross_start, cross_end, cross_at, min_along(across), gap);
    let mut out = Geometry { primary, ..Geometry::default() };
    for (index, rect) in halves.into_iter().enumerate() {
        let lead = Pane::of(index, false);
        if !view.divided(index) {
            out.panes.set(lead, Some(rect));
            continue;
        }
        let Some((near, far)) = cross_cut else {
            let shown = if view.focused.half() == lead { view.focused } else { lead };
            out.panes.set(shown, Some(rect));
            continue;
        };
        let (l, t, r, b) = rect;
        let (first, second, gap_rect) = match across {
            Orientation::Stacked => ((l, t, r, near), (l, far, r, b), (l, near, r, far)),
            Orientation::SideBySide => ((l, t, near, b), (far, t, r, b), (near, t, far, b)),
        };
        out.panes.set(lead, Some(first));
        out.panes.set(lead.sibling(), Some(second));
        out.cross[index] = Some(gap_rect);
    }
    Some(out)
}

/// The gaps' leading edges `(primary, cross)` for the view's ratios on `page`.
pub fn dividers_for_ratios(view: SplitView, page: Bounds, gap: i32) -> (i32, i32) {
    let (start, end) = axis_span(page, view.orientation);
    let (cross_start, cross_end) = axis_span(page, view.orientation.other());
    (
        divider_for_ratio(start, end, view.ratio, gap),
        divider_for_ratio(cross_start, cross_end, view.cross_ratio, gap),
    )
}

/// [`geometry`] with the gaps placed from the view's ratios.
pub fn geometry_for_ratios(view: SplitView, page: Bounds, min_w: i32, min_h: i32, gap: i32) -> Option<Geometry> {
    let (primary, cross) = dividers_for_ratios(view, page, gap);
    geometry(view, page, primary, cross, min_w, min_h, gap)
}

/// The rectangle the dragged tab would take if dropped now: its pane in the planned layout at the
/// planned ratios, or the zone's half / quarter of a 50/50 split when the plan has no layout yet or
/// the page is too small for it.
pub fn drop_target_rect(plan: DropPlan, dragged: TabId, zone: DropZone, page: Bounds, min_w: i32, min_h: i32, gap: i32) -> Bounds {
    plan.view()
        .and_then(|view| Some((view, geometry_for_ratios(view, page, min_w, min_h, gap)?)))
        .and_then(|(view, geometry)| geometry.panes.get(view.pane_for(dragged)?))
        .unwrap_or_else(|| drop_rect(page, zone, gap))
}

/// Where `dragged` lands under `plan` (its pane's edge or corner), for the card's wording; the
/// pointer's zone when the plan has no layout yet.
pub fn drop_target_zone(plan: DropPlan, dragged: TabId, zone: DropZone) -> DropZone {
    plan.view().and_then(|view| view.pane_for(dragged).map(|pane| view.zone_of(pane))).unwrap_or(zone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::TabId;

    #[test]
    fn pane_transitions_preserve_the_other_tab_and_track_focus_on_swap() {
        let mut view = SplitView::new(TabId(1), TabId(2), Pane::Right).unwrap();
        assert_eq!(view.focused_tab(), TabId(2));
        assert!(view.replace_focused(TabId(3)));
        assert_eq!((view.left, view.right), (TabId(1), TabId(3)));
        view.swap();
        assert_eq!((view.left, view.right), (TabId(3), TabId(1)));
        assert_eq!(
            view.focused_tab(),
            TabId(3),
            "the focused tab follows the swap"
        );
    }

    #[test]
    fn a_pair_needs_two_different_tabs() {
        assert!(SplitView::new(TabId(4), TabId(4), Pane::Left).is_none());
    }

    #[test]
    fn closing_either_split_tab_keeps_the_other_tab_as_single_view() {
        let view = SplitView::new(TabId(1), TabId(2), Pane::Left).unwrap();
        assert_eq!(view.remove(TabId(1)), Some(TabId(2)));
        assert_eq!(view.remove(TabId(2)), Some(TabId(1)));
        assert_eq!(view.remove(TabId(3)), None);
        assert_eq!(view.close(Pane::Left), (TabId(1), TabId(2)));
        assert_eq!(view.close(Pane::Right), (TabId(2), TabId(1)));
    }

    #[test]
    fn separating_keeps_the_focused_tab_shown() {
        let view = SplitView::new(TabId(1), TabId(2), Pane::Right).unwrap();
        assert_eq!(view.separate(), TabId(2));
        let view = SplitView::new(TabId(1), TabId(2), Pane::Left).unwrap();
        assert_eq!(view.separate(), TabId(1));
    }

    #[test]
    fn the_empty_side_takes_the_picked_tab_and_gains_focus() {
        let mut view = SplitView::new(TabId(1), TabId(9), Pane::Left).unwrap();
        assert!(view.fill_empty(TabId(9), TabId(5)));
        assert_eq!((view.left, view.right, view.focused), (TabId(1), TabId(5), Pane::Right));
        assert!(!view.fill_empty(TabId(5), TabId(1)), "a member cannot fill the other side");
        assert!(!view.fill_empty(TabId(7), TabId(3)), "the empty tab must be in the pair");
        assert_eq!((view.left, view.right), (TabId(1), TabId(5)));
    }

    #[test]
    fn divider_ratio_is_clamped_and_follows_swapped_panes() {
        let mut view = SplitView::new(TabId(1), TabId(2), Pane::Right).unwrap();
        view.set_ratio(5);
        assert_eq!(view.ratio, MIN_RATIO);
        view.set_ratio(63);
        assert_eq!(view.ratio, 63);
        view.swap();
        assert_eq!(view.ratio, 37);
        view.set_ratio(95);
        assert_eq!(view.ratio, MAX_RATIO);
    }

    #[test]
    fn divider_position_and_ratio_round_trip() {
        for ratio in [MIN_RATIO, 33, 50, 64, MAX_RATIO] {
            let divider = divider_for_ratio(100, 1100, ratio, 8);
            assert_eq!(ratio_for_divider(100, 1100, divider, 8), ratio);
        }
        assert_eq!(ratio_for_divider(0, 1000, -500, 8), MIN_RATIO);
        assert_eq!(ratio_for_divider(0, 1000, 5000, 8), MAX_RATIO);
        assert_eq!(ratio_for_divider(10, 10, 10, 8), MAX_RATIO, "an empty viewport never divides by zero");
    }

    #[test]
    fn stacked_pairs_keep_their_orientation_through_every_transition() {
        let stacked = |focused| SplitView::new(TabId(1), TabId(2), focused).unwrap().with_orientation(Orientation::Stacked);
        let mut view = stacked(Pane::Right);
        assert_eq!(view.focused_zone(), DropZone::Bottom);
        view.set_ratio(30);
        view.swap();
        assert_eq!((view.left, view.right, view.focused, view.ratio), (TabId(2), TabId(1), Pane::Left, 70));
        assert_eq!(view.orientation, Orientation::Stacked);
        assert_eq!(view.focused_zone(), DropZone::Top);
        assert!(view.replace_focused(TabId(3)));
        assert_eq!(view.orientation, Orientation::Stacked);
        assert_eq!(stacked(Pane::Left).separate(), TabId(1));
        assert_eq!(stacked(Pane::Left).close(Pane::Right), (TabId(2), TabId(1)), "closing the bottom keeps the top");
        assert_eq!(stacked(Pane::Left).remove(TabId(1)), Some(TabId(2)));
        let mut empty = stacked(Pane::Left);
        assert!(empty.fill_empty(TabId(2), TabId(5)));
        assert_eq!((empty.right, empty.focused, empty.orientation), (TabId(5), Pane::Right, Orientation::Stacked));
        let mut turned = stacked(Pane::Right);
        turned.set_ratio(64);
        turned.turn();
        assert_eq!((turned.left, turned.right, turned.focused, turned.ratio, turned.orientation), (TabId(1), TabId(2), Pane::Right, 64, Orientation::SideBySide));
        turned.turn();
        assert_eq!(turned.orientation, Orientation::Stacked);
        assert_eq!(SplitView::new(TabId(1), TabId(2), Pane::Left).unwrap().orientation, Orientation::SideBySide, "new pairs are side by side");
    }

    #[test]
    fn stacked_divider_math_runs_along_the_height() {
        let viewport = (40, 100, 1240, 900);
        let (top, bottom) = axis_span(viewport, Orientation::Stacked);
        assert_eq!((top, bottom), (100, 900));
        assert_eq!(axis_span(viewport, Orientation::SideBySide), (40, 1240));
        for ratio in [MIN_RATIO, 25, 50, 71, MAX_RATIO] {
            let divider = divider_for_ratio(top, bottom, ratio, 8);
            assert_eq!(ratio_for_divider(top, bottom, divider, 8), ratio);
        }
        assert_eq!(divider_for_ratio(top, bottom, 50, 8), 496, "the gap is centred on the middle row");
        assert_eq!(ratio_for_divider(top, bottom, 0, 8), MIN_RATIO);
        assert_eq!(ratio_for_divider(top, bottom, 4000, 8), MAX_RATIO);
        assert_eq!(axis_coord(Orientation::Stacked, 7, 9), 9);
        assert_eq!(axis_coord(Orientation::SideBySide, 7, 9), 7);
        assert_eq!(Orientation::Stacked.min_pane_dip(), MIN_PANE_HEIGHT_DIP);
        assert_eq!(Orientation::SideBySide.min_pane_dip(), MIN_PANE_DIP);
    }

    #[test]
    fn drop_zones_follow_the_nearest_edge_of_the_page() {
        let page = (100, 100, 1100, 700); // 1000 x 600
        assert_eq!(drop_zone(page, 150, 400, None), Some(DropZone::Left));
        assert_eq!(drop_zone(page, 1050, 400, None), Some(DropZone::Right));
        assert_eq!(drop_zone(page, 600, 130, None), Some(DropZone::Top));
        assert_eq!(drop_zone(page, 600, 680, None), Some(DropZone::Bottom));
        // Measured as a fraction of each axis: 20% from the left beats 30% from the top.
        assert_eq!(drop_zone(page, 300, 280, None), Some(DropZone::Left));
        assert_eq!(drop_zone(page, 400, 160, None), Some(DropZone::Top));
        // Corners: the nearer fraction wins; an exact tie goes to left/right.
        assert_eq!(drop_zone(page, 100, 100, None), Some(DropZone::Left));
        assert_eq!(drop_zone(page, 1099, 699, None), Some(DropZone::Right));
        // Outside the page there is no zone (right and bottom edges are exclusive).
        for (x, y) in [(99, 400), (1100, 400), (600, 99), (600, 700)] {
            assert_eq!(drop_zone(page, x, y, Some(DropZone::Left)), None, "{x},{y}");
        }
        // An empty page never divides by zero.
        assert_eq!(drop_zone((0, 0, 0, 0), 0, 0, None), None);
        assert_eq!(drop_zone((0, 0, 1, 1), 0, 0, None), Some(DropZone::Left));
    }

    #[test]
    fn the_middle_and_the_diagonals_keep_the_previous_zone() {
        let page = (0, 0, 1000, 1000);
        // Central box: the previous zone stays; with none, the side-by-side half.
        assert_eq!(drop_zone(page, 520, 480, Some(DropZone::Bottom)), Some(DropZone::Bottom));
        assert_eq!(drop_zone(page, 450, 500, None), Some(DropZone::Left));
        assert_eq!(drop_zone(page, 550, 500, None), Some(DropZone::Right));
        // Just across the top-left diagonal the previous zone holds ...
        assert_eq!(drop_zone(page, 220, 200, Some(DropZone::Left)), Some(DropZone::Left));
        assert_eq!(drop_zone(page, 220, 200, None), Some(DropZone::Top));
        // ... until another edge is clearly nearer.
        assert_eq!(drop_zone(page, 260, 200, Some(DropZone::Left)), Some(DropZone::Top));
        assert!(DROP_HYSTERESIS_PERMILLE < DROP_DEAD_ZONE_PERMILLE);
    }

    #[test]
    fn drop_rects_cover_the_target_half_without_the_gap() {
        let page = (100, 50, 1101, 851); // odd sizes: the gap still splits exactly
        let gap = 8;
        let left = drop_rect(page, DropZone::Left, gap);
        let right = drop_rect(page, DropZone::Right, gap);
        let top = drop_rect(page, DropZone::Top, gap);
        let bottom = drop_rect(page, DropZone::Bottom, gap);
        assert_eq!(left, (100, 50, 596, 851));
        assert_eq!(right, (604, 50, 1101, 851));
        assert_eq!(right.0 - left.2, gap);
        assert_eq!(top, (100, 50, 1101, 446));
        assert_eq!(bottom, (100, 454, 1101, 851));
        assert_eq!(bottom.1 - top.3, gap);
        for zone in DropZone::ALL {
            assert_eq!(Orientation::zone(zone.orientation(), zone.pane()), zone);
        }
    }

    #[test]
    fn a_drop_without_a_pair_pairs_the_dragged_tab_with_the_shown_tab() {
        let plan = plan_drop(None, None, TabId(1), TabId(2), DropZone::Bottom).unwrap();
        let DropPlan::Pair { view, leaving } = plan else { panic!("{plan:?}") };
        assert_eq!((view.left, view.right, view.focused, view.orientation, view.ratio), (TabId(1), TabId(2), Pane::Right, Orientation::Stacked, 50));
        assert_eq!(leaving, None);
        let DropPlan::Pair { view, .. } = plan_drop(None, None, TabId(1), TabId(2), DropZone::Left).unwrap() else { panic!() };
        assert_eq!((view.left, view.right, view.focused, view.orientation), (TabId(2), TabId(1), Pane::Left, Orientation::SideBySide));
        // The shown tab itself gets an empty side.
        assert_eq!(plan_drop(None, None, TabId(1), TabId(1), DropZone::Top), Some(DropPlan::WithEmpty { tab: TabId(1), zone: DropZone::Top }));
    }

    #[test]
    fn a_drop_onto_a_pair_keeps_the_focused_half_and_follows_the_partner_ratio() {
        let mut pair = SplitView::new(TabId(1), TabId(2), Pane::Left).unwrap();
        pair.set_ratio(30);
        // A tab from outside: the focused half (1) is the partner, the other half (2) leaves.
        let DropPlan::Pair { view, leaving } = plan_drop(Some(pair), None, TabId(1), TabId(9), DropZone::Top).unwrap() else { panic!() };
        assert_eq!((view.left, view.right, view.focused, view.orientation, view.ratio), (TabId(9), TabId(1), Pane::Left, Orientation::Stacked, 50));
        assert_eq!(leaving, Some(TabId(2)));
        // Same orientation, partner stays first: ratio kept. Partner moves to the other slot: mirrored.
        let DropPlan::Pair { view, .. } = plan_drop(Some(pair), None, TabId(1), TabId(9), DropZone::Right).unwrap() else { panic!() };
        assert_eq!((view.left, view.right, view.ratio), (TabId(1), TabId(9), 30));
        let DropPlan::Pair { view, .. } = plan_drop(Some(pair), None, TabId(1), TabId(9), DropZone::Left).unwrap() else { panic!() };
        assert_eq!((view.left, view.right, view.ratio), (TabId(9), TabId(1), 70));
        // The focused half is the empty side: the page half is the partner and the empty side leaves.
        let mut with_empty = pair;
        with_empty.focus(Pane::Right);
        let DropPlan::Pair { view, leaving } = plan_drop(Some(with_empty), Some(TabId(2)), TabId(2), TabId(9), DropZone::Bottom).unwrap() else { panic!() };
        assert_eq!((view.left, view.right), (TabId(1), TabId(9)));
        assert_eq!(leaving, Some(TabId(2)));
    }

    #[test]
    fn a_half_dragged_within_its_pair_rearranges_it() {
        let pair = SplitView::new(TabId(1), TabId(2), Pane::Left).unwrap();
        // Onto its own place: nothing to do (no preview either).
        assert_eq!(plan_drop(Some(pair), None, TabId(1), TabId(1), DropZone::Left), None);
        assert_eq!(plan_drop(Some(pair), None, TabId(1), TabId(2), DropZone::Right), None);
        // Onto the other side: a swap. Onto the bottom: the pair turns stacked.
        let DropPlan::Pair { view, leaving } = plan_drop(Some(pair), None, TabId(1), TabId(1), DropZone::Right).unwrap() else { panic!() };
        assert_eq!((view.left, view.right, view.focused, leaving), (TabId(2), TabId(1), Pane::Right, None));
        let DropPlan::Pair { view, .. } = plan_drop(Some(pair), None, TabId(1), TabId(1), DropZone::Bottom).unwrap() else { panic!() };
        assert_eq!((view.left, view.right, view.orientation), (TabId(2), TabId(1), Orientation::Stacked));
        // A stacked pair's top dropped on the top again is a no-op; on the left it turns side by side.
        let stacked = pair.with_orientation(Orientation::Stacked);
        assert_eq!(plan_drop(Some(stacked), None, TabId(1), TabId(1), DropZone::Top), None);
        let DropPlan::Pair { view, .. } = plan_drop(Some(stacked), None, TabId(1), TabId(1), DropZone::Left).unwrap() else { panic!() };
        assert_eq!((view.left, view.right, view.orientation), (TabId(1), TabId(2), Orientation::SideBySide));
    }

    #[test]
    fn drop_side_splits_the_viewport_in_half() {
        assert_eq!(side_of(100, 500, 100), Some(Pane::Left));
        assert_eq!(side_of(100, 500, 299), Some(Pane::Left));
        assert_eq!(side_of(100, 500, 300), Some(Pane::Right));
        assert_eq!(side_of(100, 500, 499), Some(Pane::Right));
        assert_eq!(side_of(100, 500, 500), None);
        assert_eq!(side_of(100, 500, 99), None);
    }

    // ----- up to four panes -----

    fn t(id: u32) -> TabId {
        TabId(id)
    }

    /// Plans a drop and returns the resulting layout's quadrant map, focused tab and leaving tabs.
    fn layout_drop(view: SplitView, empty: Option<u32>, dragged: u32, zone: DropZone) -> Option<([u32; 4], u32, Vec<u32>, SplitView)> {
        match plan_drop(Some(view), empty.map(t), view.focused_tab(), t(dragged), zone)? {
            DropPlan::Layout { view, leaving } => Some((
                view.cells().map(|tab| tab.0),
                view.focused_tab().0,
                leaving.into_iter().flatten().map(|tab| tab.0).collect(),
                view,
            )),
            other => panic!("expected a layout plan, got {other:?}"),
        }
    }

    fn side_by_side() -> SplitView {
        SplitView::new(t(1), t(2), Pane::Left).unwrap()
    }

    fn stacked_pair() -> SplitView {
        side_by_side().with_orientation(Orientation::Stacked)
    }

    /// [9 / 1 | 2]: the left column divided, 9 on top.
    fn tee() -> SplitView {
        layout_drop(side_by_side(), None, 9, DropZone::TopLeft).unwrap().3
    }

    /// [9 2 / 1 7]: a full 2 x 2 built by corner drops.
    fn quad() -> SplitView {
        layout_drop(tee(), None, 7, DropZone::BottomRight).unwrap().3
    }

    #[test]
    fn a_plain_pair_keeps_its_shape_and_maps_onto_the_quadrants() {
        let pair = side_by_side();
        assert!(pair.is_pair());
        assert_eq!(pair.pane_count(), 2);
        assert_eq!(pair.cells(), [t(1), t(2), t(1), t(2)]);
        assert_eq!(stacked_pair().cells(), [t(1), t(1), t(2), t(2)]);
        assert_eq!(pair.tab(Pane::LeftTrail), t(1), "a missing trail reads as its half");
        assert_eq!(pair.tab_at(Pane::LeftTrail), None);
        assert_eq!(pair.panes().map(|(pane, _)| pane).collect::<Vec<_>>(), [Pane::Left, Pane::Right]);
        assert_eq!(pair.zone_of(Pane::Left), DropZone::Left);
        assert_eq!(stacked_pair().zone_of(Pane::Right), DropZone::Bottom);
        let mut focus = pair;
        focus.focus(Pane::RightTrail);
        assert_eq!(focus.focused, Pane::Left, "focusing a missing trail is ignored");
    }

    #[test]
    fn a_corner_of_a_side_by_side_pair_divides_that_column() {
        // Every corner: the dragged tab lands in exactly that quadrant, the column's tab keeps the other cell.
        let cases = [
            (DropZone::TopLeft, [9, 2, 1, 2]),
            (DropZone::BottomLeft, [1, 2, 9, 2]),
            (DropZone::TopRight, [1, 9, 1, 2]),
            (DropZone::BottomRight, [1, 2, 1, 9]),
        ];
        for (zone, cells) in cases {
            let (got, focused, leaving, view) = layout_drop(side_by_side(), None, 9, zone).unwrap();
            assert_eq!(got, cells, "{zone:?}");
            assert_eq!((focused, leaving.len(), view.pane_count()), (9, 0, 3), "{zone:?}");
            assert_eq!(view.orientation, Orientation::SideBySide, "the full-height divider stays");
            assert_eq!(view.zone_of(view.focused), zone, "{zone:?}");
        }
        let view = tee();
        assert_eq!((view.left, view.trail, view.right), (t(9), [Some(t(1)), None], t(2)));
    }

    #[test]
    fn a_corner_of_a_stacked_pair_divides_that_row() {
        let cases = [
            (DropZone::TopLeft, [9, 1, 2, 2]),
            (DropZone::TopRight, [1, 9, 2, 2]),
            (DropZone::BottomLeft, [1, 1, 9, 2]),
            (DropZone::BottomRight, [1, 1, 2, 9]),
        ];
        for (zone, cells) in cases {
            let (got, focused, _, view) = layout_drop(stacked_pair(), None, 9, zone).unwrap();
            assert_eq!((got, focused), (cells, 9), "{zone:?}");
            assert_eq!(view.orientation, Orientation::Stacked, "{zone:?}");
        }
    }

    #[test]
    fn three_panes_become_four_and_four_replace_or_trade() {
        // 3 -> 4: the right column divides too.
        let (cells, focused, leaving, view) = layout_drop(tee(), None, 7, DropZone::BottomRight).unwrap();
        assert_eq!((cells, focused, leaving), ([9, 2, 1, 7], 7, vec![]));
        assert_eq!(view.pane_count(), 4);
        // A corner whose pane is already a quadrant: the dragged tab replaces it; that tab leaves.
        let (cells, _, leaving, _) = layout_drop(tee(), None, 5, DropZone::TopLeft).unwrap();
        assert_eq!((cells, leaving), ([5, 2, 1, 2], vec![9]));
        let (cells, focused, leaving, _) = layout_drop(quad(), None, 5, DropZone::BottomLeft).unwrap();
        assert_eq!((cells, focused, leaving), ([9, 2, 5, 7], 5, vec![1]));
        // A pane of the layout onto another quadrant: the two trade, nothing leaves.
        let (cells, focused, leaving, _) = layout_drop(quad(), None, 7, DropZone::TopLeft).unwrap();
        assert_eq!((cells, focused, leaving), ([7, 2, 1, 9], 7, vec![]));
        // Onto its own quadrant: nothing to do.
        assert_eq!(plan_drop(Some(quad()), None, t(9), t(9), DropZone::TopLeft), None);
        // A whole half onto a quadrant of the other half: they trade panes.
        let (cells, _, leaving, _) = layout_drop(tee(), None, 2, DropZone::TopLeft).unwrap();
        assert_eq!((cells, leaving), ([2, 9, 1, 9], vec![]));
        // A whole half onto its own corner: it leaves its half (the other column becomes a stacked
        // pair) and divides the top row.
        let (cells, focused, leaving, view) = layout_drop(tee(), None, 2, DropZone::TopRight).unwrap();
        assert_eq!((cells, focused, leaving), ([9, 2, 1, 1], 2, vec![]));
        assert_eq!(view.orientation, Orientation::Stacked);
    }

    #[test]
    fn edges_of_a_larger_layout_give_the_dragged_tab_that_whole_half() {
        let (cells, _, mut leaving, view) = layout_drop(quad(), None, 5, DropZone::Left).unwrap();
        leaving.sort();
        assert_eq!((cells, leaving), ([5, 2, 5, 7], vec![1, 9]));
        assert_eq!(view.pane_count(), 3);
        // Across the T's full divider: the layout turns rows-first.
        let (cells, _, leaving, view) = layout_drop(tee(), None, 5, DropZone::Top).unwrap();
        assert_eq!((cells, leaving), ([5, 5, 1, 2], vec![9]));
        assert_eq!(view.orientation, Orientation::Stacked);
        // Onto the half it already is: nothing.
        assert_eq!(plan_drop(Some(tee()), None, t(9), t(2), DropZone::Right), None);
        // A pane dragged to an edge leaves its place first.
        let (cells, _, leaving, _) = layout_drop(quad(), None, 7, DropZone::Left).unwrap();
        assert_eq!((cells, leaving), ([7, 2, 7, 2], vec![9, 1]));
    }

    #[test]
    fn corners_fill_an_empty_pane_and_need_a_split() {
        // The pair's empty side (2) under the corner: the dragged tab takes the whole pane; the empty tab leaves.
        let (cells, _, leaving, view) = layout_drop(side_by_side(), Some(2), 9, DropZone::TopRight).unwrap();
        assert_eq!((cells, leaving), ([1, 9, 1, 9], vec![2]));
        assert!(view.is_pair());
        assert_eq!(plan_drop(None, None, t(1), t(2), DropZone::TopLeft), None, "no split: corners are not offered");
        // Pairs and their edges keep the old plans exactly.
        assert!(matches!(plan_drop(Some(side_by_side()), None, t(1), t(9), DropZone::Left), Some(DropPlan::Pair { .. })));
    }

    #[test]
    fn closing_a_pane_lets_its_sibling_expand() {
        let quad = quad(); // [9 2 / 1 7], columns first
        let Some(Remains::Layout(view)) = quad.without(t(9)) else { panic!() };
        assert_eq!(view.cells(), [t(1), t(2), t(1), t(7)], "the column-mate grows (undoing the drop that divided it)");
        let Some(Remains::Layout(view)) = quad.without(t(7)) else { panic!() };
        assert_eq!(view.cells(), [t(9), t(2), t(1), t(2)]);
        assert_eq!(view.focused_tab(), t(2), "the closed pane's sibling takes the focus");
        let mut focused = quad;
        focused.focus(Pane::LeftTrail);
        let Some(Remains::Layout(view)) = focused.without(t(7)) else { panic!() };
        assert_eq!(view.focused_tab(), t(1), "focus elsewhere stays on its tab");
        // A whole half closes: the other half's division becomes the primary split.
        let mut tee = tee();
        tee.set_cross_ratio(30);
        let Some(Remains::Layout(view)) = tee.without(t(2)) else { panic!() };
        assert_eq!((view.left, view.right, view.orientation, view.ratio), (t(9), t(1), Orientation::Stacked, 30));
        assert!(view.is_pair());
        assert_eq!(side_by_side().without(t(1)), Some(Remains::Single(t(2))));
        assert_eq!(quad.without(t(4)), None);
        assert_eq!(quad.remove(t(9)), Some(t(7)), "the shown tab after a close");
        assert_eq!(quad.close(Pane::Left).0, t(9));
    }

    #[test]
    fn focus_moves_to_the_geometric_neighbour() {
        let mut quad = quad(); // [9 2 / 1 7]
        assert_eq!(quad.focused_tab(), t(7));
        assert_eq!(quad.neighbour(Direction::Left).map(|pane| quad.tab(pane)), Some(t(1)));
        assert_eq!(quad.neighbour(Direction::Up).map(|pane| quad.tab(pane)), Some(t(2)));
        assert_eq!(quad.neighbour(Direction::Right), None);
        assert_eq!(quad.neighbour(Direction::Down), None);
        quad.focus(Pane::Left);
        assert_eq!(quad.neighbour(Direction::Right).map(|pane| quad.tab(pane)), Some(t(2)));
        assert_eq!(quad.neighbour(Direction::Down).map(|pane| quad.tab(pane)), Some(t(1)));
        // From a whole half, the top neighbour wins.
        let mut tee = tee();
        tee.focus(Pane::Right);
        assert_eq!(tee.neighbour(Direction::Left).map(|pane| tee.tab(pane)), Some(t(9)));
        // A pair keeps the historical first / second keys in both orientations.
        for pair in [side_by_side(), stacked_pair()] {
            assert_eq!(pair.neighbour(Direction::Left), Some(Pane::Left));
            assert_eq!(pair.neighbour(Direction::Right), Some(Pane::Right));
            assert_eq!(pair.neighbour(Direction::Up), Some(Pane::Left));
            assert_eq!(pair.neighbour(Direction::Down), Some(Pane::Right));
        }
    }

    #[test]
    fn swaps_turns_and_cells_round_trip() {
        let mut quad = quad();
        quad.set_ratio(30);
        quad.swap();
        assert_eq!(quad.cells(), [t(2), t(9), t(7), t(1)]);
        assert_eq!((quad.ratio, quad.focused_tab()), (70, t(7)));
        assert!(quad.swap_cross(0));
        assert_eq!(quad.cells(), [t(7), t(9), t(2), t(1)]);
        assert!(!side_by_side().swap_cross(0));
        for view in [side_by_side(), stacked_pair(), tee(), quad] {
            let (col, row) = view.col_row_ratios();
            let back = SplitView::from_cells(view.cells(), view.orientation, col, row, view.focused_tab()).unwrap();
            assert_eq!(back.cells(), view.cells());
            assert_eq!(back.col_row_ratios(), (col, row));
        }
        assert_eq!(SplitView::from_cells([t(1); 4], Orientation::SideBySide, 50, 50, t(1)), None);
        assert_eq!(SplitView::from_cells([t(1), t(2), t(2), t(1)], Orientation::SideBySide, 50, 50, t(1)), None, "diagonal");
        assert_eq!(tee().zone_of(Pane::Left), DropZone::TopLeft);
        assert_eq!(tee().zone_of(Pane::Right), DropZone::Right);
    }

    #[test]
    fn corner_zones_with_hysteresis() {
        let page = (0, 0, 1000, 1000);
        assert_eq!(drop_zone_cornered(page, 100, 100, None, true), Some(DropZone::TopLeft));
        assert_eq!(drop_zone_cornered(page, 900, 100, None, true), Some(DropZone::TopRight));
        assert_eq!(drop_zone_cornered(page, 100, 900, None, true), Some(DropZone::BottomLeft));
        assert_eq!(drop_zone_cornered(page, 999, 999, None, true), Some(DropZone::BottomRight));
        // Without corners (no split, or a pair's own half): the old edge rule exactly.
        assert_eq!(drop_zone_cornered(page, 100, 100, None, false), Some(DropZone::Left));
        assert_eq!(drop_zone_cornered(page, 100, 100, Some(DropZone::TopLeft), false), Some(DropZone::Left));
        // Just outside the corner box an edge wins, unless the corner was already shown.
        assert_eq!(drop_zone_cornered(page, 260, 100, None, true), Some(DropZone::Top));
        assert_eq!(drop_zone_cornered(page, 260, 100, Some(DropZone::TopLeft), true), Some(DropZone::TopLeft));
        assert_eq!(drop_zone_cornered(page, 310, 100, Some(DropZone::TopLeft), true), Some(DropZone::Top));
        // Another corner's hysteresis does not leak.
        assert_eq!(drop_zone_cornered(page, 260, 100, Some(DropZone::BottomRight), true), Some(DropZone::Top));
        assert_eq!(drop_zone_cornered(page, 1000, 100, None, true), None, "off the page");
        assert!(!corners_allowed(None, t(1)));
        assert!(!corners_allowed(Some(side_by_side()), t(1)), "a pair's own half re-arranges the pair");
        assert!(corners_allowed(Some(side_by_side()), t(9)));
        assert!(corners_allowed(Some(tee()), t(9)));
        assert!(corners_allowed(Some(tee()), t(2)));
    }

    #[test]
    fn quadrant_geometry_at_96_and_192_dpi() {
        for scale in [1, 2] {
            let (w, h, gap) = (1200 * scale, 800 * scale, 8 * scale);
            let (min_w, min_h) = (MIN_PANE_DIP * scale, MIN_PANE_HEIGHT_DIP * scale);
            let page = (0, 0, w, h);
            let quad = quad();
            let geometry = geometry_for_ratios(quad, page, min_w, min_h, gap).unwrap();
            let s = |v: i32| v * scale;
            assert_eq!(geometry.panes.get(Pane::Left), Some((0, 0, s(596), s(396))));
            assert_eq!(geometry.panes.get(Pane::LeftTrail), Some((0, s(404), s(596), h)));
            assert_eq!(geometry.panes.get(Pane::Right), Some((s(604), 0, w, s(396))));
            assert_eq!(geometry.panes.get(Pane::RightTrail), Some((s(604), s(404), w, h)));
            assert_eq!(geometry.primary, (s(596), 0, s(604), h));
            assert_eq!(geometry.cross, [Some((0, s(396), s(596), s(404))), Some((s(604), s(396), w, s(404)))]);
            assert_eq!(geometry.divider_at(s(600), s(400), quad.orientation), Some(Divider::Both));
            assert_eq!(geometry.divider_at(s(600), s(100), quad.orientation), Some(Divider::Primary));
            assert_eq!(geometry.divider_at(s(100), s(400), quad.orientation), Some(Divider::Cross(0)));
            assert_eq!(geometry.divider_at(s(900), s(400), quad.orientation), Some(Divider::Cross(1)));
            assert_eq!(geometry.divider_at(s(100), s(100), quad.orientation), None);
            assert_eq!(geometry.pane_near(s(900), s(700)), Some(Pane::RightTrail));
            assert_eq!(geometry.pane_near(s(598), s(10)), Some(Pane::Left), "a gap click goes to the nearest pane");
            // A T: one cross gap; a pair: none, and the pair's rects are the old halves.
            let tee = geometry_for_ratios(tee(), page, min_w, min_h, gap).unwrap();
            assert_eq!(tee.panes.get(Pane::Right), Some((s(604), 0, w, h)));
            assert_eq!(tee.cross[1], None);
            let pair = geometry_for_ratios(side_by_side(), page, min_w, min_h, gap).unwrap();
            assert_eq!(pair.panes.iter().count(), 2);
            assert_eq!(pair.cross, [None, None]);
            // Ratios clamp so every pane keeps its minimum.
            let clamped = super::geometry(quad, page, -500, 99_999, min_w, min_h, gap).unwrap();
            assert_eq!(clamped.panes.get(Pane::Left).map(|r| (r.2, r.3)), Some((min_w, h - min_h - gap)));
        }
    }

    #[test]
    fn small_pages_collapse_without_dropping_tabs() {
        let mut quad = quad();
        quad.focus(Pane::LeftTrail);
        // Too short for two rows: each column shows one pane (the focused one, else its lead).
        let geometry = geometry_for_ratios(quad, (0, 0, 1200, 300), 240, 160, 8).unwrap();
        assert_eq!(geometry.panes.iter().map(|(pane, _)| pane).collect::<Vec<_>>(), [Pane::LeftTrail, Pane::Right]);
        assert_eq!(geometry.cross, [None, None]);
        assert!(geometry_for_ratios(quad, (0, 0, 1200, 328), 240, 160, 8).unwrap().cross[0].is_some());
        // Too narrow for two columns: no layout (the focused pane alone fills the page).
        assert_eq!(geometry_for_ratios(quad, (0, 0, 487, 800), 240, 160, 8), None);
        assert!(geometry_for_ratios(quad, (0, 0, 488, 800), 240, 160, 8).is_some());
        // The model is untouched: everything comes back when the page grows.
        assert_eq!(quad.pane_count(), 4);
    }

    #[test]
    fn the_preview_covers_exactly_the_new_pane() {
        let page = (0, 0, 1200, 800);
        let mut pair = side_by_side();
        pair.set_ratio(30);
        let plan = plan_drop(Some(pair), None, t(1), t(9), DropZone::BottomRight).unwrap();
        assert_eq!(drop_target_rect(plan, t(9), DropZone::BottomRight, page, 240, 160, 8), (364, 404, 1200, 800));
        assert_eq!(drop_target_zone(plan, t(9), DropZone::BottomRight), DropZone::BottomRight);
        // An edge of a layout: the whole half; a replaced quadrant: that quadrant.
        let plan = plan_drop(Some(quad()), None, t(7), t(5), DropZone::Right).unwrap();
        assert_eq!(drop_target_rect(plan, t(5), DropZone::Right, page, 240, 160, 8), (604, 0, 1200, 800));
        assert_eq!(drop_target_zone(plan, t(5), DropZone::Right), DropZone::Right);
        // No layout yet (an empty side opens): the zone's 50/50 half.
        let plan = plan_drop(None, None, t(1), t(1), DropZone::Top).unwrap();
        assert_eq!(drop_target_rect(plan, t(1), DropZone::Top, page, 240, 160, 8), (0, 0, 1200, 396));
        assert_eq!(drop_rect(page, DropZone::TopLeft, 8), (0, 0, 596, 396));
        assert_eq!(drop_rect(page, DropZone::BottomRight, 8), (604, 404, 1200, 800));
    }
}
