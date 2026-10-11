//! The non-persisted relationship between two visible tabs.

use crate::tabs::TabId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pane { Left, Right }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitView {
    pub left: TabId,
    pub right: TabId,
    pub focused: Pane,
    /// Percentage of viewport width assigned to the left pane (20 through 80).
    pub ratio: u8,
}

impl SplitView {
    pub fn new(left: TabId, right: TabId, focused: Pane) -> Option<Self> {
        (left != right).then_some(Self { left, right, focused, ratio: 50 })
    }
    pub fn focused_tab(self) -> TabId {
        match self.focused { Pane::Left => self.left, Pane::Right => self.right }
    }
    pub fn other_tab(self) -> TabId {
        match self.focused { Pane::Left => self.right, Pane::Right => self.left }
    }
    pub fn pane_for(self, tab: TabId) -> Option<Pane> {
        if self.left == tab { Some(Pane::Left) } else if self.right == tab { Some(Pane::Right) } else { None }
    }
    pub fn focus(&mut self, pane: Pane) { self.focused = pane; }
    pub fn set_ratio(&mut self, ratio: u8) { self.ratio = ratio.clamp(20, 80); }
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.left, &mut self.right);
        self.focused = match self.focused { Pane::Left => Pane::Right, Pane::Right => Pane::Left };
        self.ratio = 100 - self.ratio;
    }
    pub fn replace_focused(&mut self, tab: TabId) -> bool {
        if tab == self.other_tab() { return false; }
        match self.focused { Pane::Left => self.left = tab, Pane::Right => self.right = tab }
        true
    }
    pub fn remove(self, tab: TabId) -> Option<TabId> {
        match (self.left == tab, self.right == tab) {
            (true, false) => Some(self.right),
            (false, true) => Some(self.left),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Pane, SplitView};
    use crate::tabs::TabId;

    #[test]
    fn pane_transitions_preserve_the_other_tab_and_track_focus_on_swap() {
        let mut view = SplitView::new(TabId(1), TabId(2), Pane::Right).unwrap();
        assert_eq!(view.focused_tab(), TabId(2));
        assert!(view.replace_focused(TabId(3)));
        assert_eq!((view.left, view.right), (TabId(1), TabId(3)));
        view.swap();
        assert_eq!((view.left, view.right), (TabId(3), TabId(1)));
        assert_eq!(view.focused_tab(), TabId(3), "the focused tab follows the swap");
    }

    #[test]
    fn closing_either_split_tab_keeps_the_other_tab_as_single_view() {
        let view = SplitView::new(TabId(1), TabId(2), Pane::Left).unwrap();
        assert_eq!(view.remove(TabId(1)), Some(TabId(2)));
        assert_eq!(view.remove(TabId(2)), Some(TabId(1)));
        assert_eq!(view.remove(TabId(3)), None);
    }

    #[test]
    fn divider_ratio_is_clamped_and_follows_swapped_panes() {
        let mut view = SplitView::new(TabId(1), TabId(2), Pane::Right).unwrap();
        view.set_ratio(5);
        assert_eq!(view.ratio, 20);
        view.set_ratio(63);
        assert_eq!(view.ratio, 63);
        view.swap();
        assert_eq!(view.ratio, 37);
        view.set_ratio(95);
        assert_eq!(view.ratio, 80);
    }
}
