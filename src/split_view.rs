//! The relationship between the two tabs of a split view (a tab pair, as in Chrome).
//!
//! A window holds at most one pair. The pair is *shown* while one of its tabs is the active tab and
//! *parked* while another tab is shown alone; the app keeps it in `split_view` or `split_parked`.
//! Everything here is pure and unit-tested; the app and window code apply the results.

use crate::tabs::TabId;

/// Smallest and largest share of the viewport width the left pane may take, in percent.
pub const MIN_RATIO: u8 = 20;
pub const MAX_RATIO: u8 = 80;
/// Narrowest pane, in device-independent pixels. A viewport that cannot fit two of these plus the
/// gap shows only the focused pane.
pub const MIN_PANE_DIP: i32 = 240;
/// Width of the gap between the two pages, which is also the divider's hit target, in DIP.
pub const GAP_DIP: i32 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pane {
    Left,
    Right,
}

impl Pane {
    pub fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitView {
    pub left: TabId,
    pub right: TabId,
    pub focused: Pane,
    /// Percentage of viewport width assigned to the left pane (`MIN_RATIO` through `MAX_RATIO`).
    pub ratio: u8,
}

impl SplitView {
    pub fn new(left: TabId, right: TabId, focused: Pane) -> Option<Self> {
        (left != right).then_some(Self {
            left,
            right,
            focused,
            ratio: 50,
        })
    }
    pub fn focused_tab(self) -> TabId {
        self.tab(self.focused)
    }
    pub fn other_tab(self) -> TabId {
        self.tab(self.focused.other())
    }
    pub fn tab(self, pane: Pane) -> TabId {
        match pane {
            Pane::Left => self.left,
            Pane::Right => self.right,
        }
    }
    pub fn pane_for(self, tab: TabId) -> Option<Pane> {
        if self.left == tab {
            Some(Pane::Left)
        } else if self.right == tab {
            Some(Pane::Right)
        } else {
            None
        }
    }
    pub fn contains(self, tab: TabId) -> bool {
        self.pane_for(tab).is_some()
    }
    pub fn focus(&mut self, pane: Pane) {
        self.focused = pane;
    }
    pub fn set_ratio(&mut self, ratio: u8) {
        self.ratio = ratio.clamp(MIN_RATIO, MAX_RATIO);
    }
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.left, &mut self.right);
        self.focused = self.focused.other();
        self.ratio = 100 - self.ratio;
    }
    pub fn replace_focused(&mut self, tab: TabId) -> bool {
        if tab == self.other_tab() {
            return false;
        }
        match self.focused {
            Pane::Left => self.left = tab,
            Pane::Right => self.right = tab,
        }
        true
    }
    /// The tab that survives when `tab` closes, or `None` when `tab` is not in the pair.
    pub fn remove(self, tab: TabId) -> Option<TabId> {
        match (self.left == tab, self.right == tab) {
            (true, false) => Some(self.right),
            (false, true) => Some(self.left),
            _ => None,
        }
    }
    /// Closing the tab in `pane`: `(closing, survivor)`. The survivor becomes a normal tab.
    pub fn close(self, pane: Pane) -> (TabId, TabId) {
        (self.tab(pane), self.tab(pane.other()))
    }
    /// "Separate views": both tabs stay open as normal tabs; the focused one stays shown.
    pub fn separate(self) -> TabId {
        self.focused_tab()
    }
    /// The empty side (`empty`, a tab with no page) takes `picked`. Fails when `empty` is not in the
    /// pair or `picked` already is.
    pub fn fill_empty(&mut self, empty: TabId, picked: TabId) -> bool {
        if self.contains(picked) {
            return false;
        }
        let Some(pane) = self.pane_for(empty) else {
            return false;
        };
        match pane {
            Pane::Left => self.left = picked,
            Pane::Right => self.right = picked,
        }
        self.focused = pane;
        true
    }
}

/// The divider ratio for a gap whose left edge is at `divider_left` inside a viewport spanning
/// `left..right` (physical pixels), measured at the gap's centre and clamped to the ratio range.
pub fn ratio_for_divider(left: i32, right: i32, divider_left: i32, gap: i32) -> u8 {
    let width = (right - left).max(1);
    let centre = divider_left + gap / 2 - left;
    let percent = (centre * 100 + width / 2) / width;
    percent.clamp(i32::from(MIN_RATIO), i32::from(MAX_RATIO)) as u8
}

/// The gap's left edge for `ratio` in a viewport spanning `left..right` (physical pixels).
pub fn divider_for_ratio(left: i32, right: i32, ratio: u8, gap: i32) -> i32 {
    left + (right - left) * i32::from(ratio) / 100 - gap / 2
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
    fn drop_side_splits_the_viewport_in_half() {
        assert_eq!(side_of(100, 500, 100), Some(Pane::Left));
        assert_eq!(side_of(100, 500, 299), Some(Pane::Left));
        assert_eq!(side_of(100, 500, 300), Some(Pane::Right));
        assert_eq!(side_of(100, 500, 499), Some(Pane::Right));
        assert_eq!(side_of(100, 500, 500), None);
        assert_eq!(side_of(100, 500, 99), None);
    }
}
