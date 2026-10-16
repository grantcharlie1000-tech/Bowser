//! Tab list model (glass UI work in progress; see docs/superpowers/specs/2026-10-09-bowser-glass-ui-design.md)
//!
//! Ids come from a counter that only moves forward, so a stale id (for example one carried by an
//! engine event that raced a close) never matches a different tab. Lookups for unknown ids are
//! no-ops that report failure rather than panicking.

/// Label for tabs without a title, including a tab that has not navigated yet.
const NEW_TAB_TITLE: &str = "New tab";

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TabId(pub u32);

/// Identifies a decoded favicon in the favicon cache.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FaviconKey(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tab {
    pub id: TabId,
    /// Tab that opened this tab through a page new-window request, if any.
    pub opener: Option<TabId>,
    pub pinned: bool,
    pub title: String,
    /// Display form of the current address. `None` until the tab first has a page.
    pub url: Option<String>,
    pub loading: bool,
    pub favicon: Option<FaviconKey>,
}

impl Tab {
    pub fn new(id: TabId) -> Self {
        Self {
            id,
            opener: None,
            pinned: false,
            title: String::new(),
            url: None,
            loading: false,
            favicon: None,
        }
    }
}

/// Title shown in the tab island: the page title, or "New tab" when there is none.
pub fn display_title(tab: &Tab) -> &str {
    if tab.title.trim().is_empty() {
        NEW_TAB_TITLE
    } else {
        &tab.title
    }
}

/// Where a new tab is inserted (the "New tab position" option).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Placement {
    /// At the end of the strip.
    Last,
    /// Right of the active tab.
    AfterActive,
    /// Right of the tab that opened it and that tab's earlier children; with no opener, right of the
    /// active tab.
    AfterRelated,
}

/// Result of [`TabList::close`]. `new_active` is the active tab after the close.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CloseOutcome {
    pub new_active: TabId,
}

/// Ordered tabs with exactly one active tab. The list is never empty.
#[derive(Clone, Debug)]
pub struct TabList {
    tabs: Vec<Tab>,
    active: TabId,
    next_id: u32,
}

impl Default for TabList {
    fn default() -> Self {
        Self::new()
    }
}

impl TabList {
    /// Starts with one empty tab, which is active.
    pub fn new() -> Self {
        let mut list = Self {
            tabs: Vec::new(),
            active: TabId(0),
            next_id: 0,
        };
        let id = list.allocate_id();
        list.tabs.push(Tab::new(id));
        list.active = id;
        list
    }

    fn allocate_id(&mut self) -> TabId {
        let id = TabId(self.next_id);
        self.next_id = self.next_id.checked_add(1).expect("tab ids exhausted");
        id
    }

    /// Inserts an empty tab and returns its id. The active tab is unchanged; call [`Self::select`]
    /// to switch to the new tab. With `after_active` the tab goes right of the active tab,
    /// otherwise at the end.
    pub fn add(&mut self, after_active: bool) -> TabId {
        let id = self.allocate_id();
        let at = match self.index_of(self.active) {
            Some(active_index) if after_active => {
                if !self.tabs[active_index].pinned {
                    active_index + 1
                } else {
                    self.pinned_count()
                }
            }
            _ => self.tabs.len(),
        };
        self.tabs.insert(at, Tab::new(id));
        id
    }

    /// Inserts an empty tab where `placement` says and returns its id. `opener` is recorded on the new
    /// tab and is what `Placement::AfterRelated` keeps it next to. The active tab is unchanged.
    pub fn add_placed(&mut self, placement: Placement, opener: Option<TabId>) -> TabId {
        let id = self.allocate_id();
        let opener = opener.filter(|opener| self.index_of(*opener).is_some());
        let at = match placement {
            Placement::Last => self.tabs.len(),
            Placement::AfterRelated if opener.is_some() => {
                let opener = opener.expect("checked above");
                let index = self.index_of(opener).expect("the opener is in the list");
                let mut at = if self.tabs[index].pinned { self.pinned_count() } else { index + 1 };
                while at < self.tabs.len() && !self.tabs[at].pinned && self.tabs[at].opener == Some(opener) {
                    at += 1;
                }
                at
            }
            Placement::AfterActive | Placement::AfterRelated => match self.index_of(self.active) {
                Some(active_index) if !self.tabs[active_index].pinned => active_index + 1,
                Some(_) => self.pinned_count(),
                None => self.tabs.len(),
            },
        };
        let mut tab = Tab::new(id);
        tab.opener = opener;
        self.tabs.insert(at, tab);
        id
    }

    /// Removes a tab. Closing the active tab selects its right neighbour, or the left one when it
    /// was the rightmost. Closing the last remaining tab replaces it with a fresh empty tab.
    /// An unknown id changes nothing.
    pub fn close(&mut self, id: TabId) -> CloseOutcome {
        let Some(index) = self.index_of(id) else {
            return CloseOutcome {
                new_active: self.active,
            };
        };

        if self.tabs.len() == 1 {
            let fresh = self.allocate_id();
            self.tabs[0] = Tab::new(fresh);
            self.active = fresh;
            return CloseOutcome { new_active: fresh };
        }

        self.tabs.remove(index);
        for tab in &mut self.tabs {
            if tab.opener == Some(id) {
                tab.opener = None;
            }
        }
        if id == self.active {
            // After removal the right neighbour sits at `index`. Otherwise the tab closed was
            // the rightmost, so the new last tab is the left neighbour.
            let neighbour = index.min(self.tabs.len() - 1);
            self.active = self.tabs[neighbour].id;
        }
        CloseOutcome {
            new_active: self.active,
        }
    }

    /// Makes `id` the active tab. Returns `false` if no tab has that id.
    pub fn select(&mut self, id: TabId) -> bool {
        if self.index_of(id).is_some() {
            self.active = id;
            true
        } else {
            false
        }
    }

    /// Selects the tab to the right, wrapping from the last tab to the first.
    pub fn select_next(&mut self) {
        let next = (self.active_index() + 1) % self.tabs.len();
        self.active = self.tabs[next].id;
    }

    /// Selects the tab to the left, wrapping from the first tab to the last.
    pub fn select_prev(&mut self) {
        let len = self.tabs.len();
        let prev = (self.active_index() + len - 1) % len;
        self.active = self.tabs[prev].id;
    }

    /// Moves a tab so it ends up at `to_index` in the resulting list. Indices past the end go to
    /// the end. The active tab does not change. Returns `true` if the order changed, and `false`
    /// for an unknown id or a move that leaves the order as it was.
    pub fn move_tab(&mut self, id: TabId, to_index: usize) -> bool {
        let Some(from) = self.index_of(id) else {
            return false;
        };
        let pinned = self.tabs[from].pinned;
        let pinned_count = self.pinned_count();
        let (first, last) = if pinned {
            (0, pinned_count.saturating_sub(1))
        } else {
            (pinned_count, self.tabs.len() - 1)
        };
        let to = to_index.clamp(first, last);
        if from == to {
            return false;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        true
    }

    /// Changes whether `id` is pinned. Pinned tabs form a stable leading partition; pinning puts
    /// the tab at its end, while unpinning puts it at the start of the unpinned partition.
    /// Relative order in both partitions and the active tab identity are preserved. Returns false
    /// for an unknown tab or when its state already matches `pinned`.
    pub fn set_pinned(&mut self, id: TabId, pinned: bool) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        if self.tabs[index].pinned == pinned {
            return false;
        }
        let mut tab = self.tabs.remove(index);
        tab.pinned = pinned;
        let insert_at = self.pinned_count();
        self.tabs.insert(insert_at, tab);
        true
    }

    pub fn pinned_count(&self) -> usize {
        self.tabs.iter().take_while(|tab| tab.pinned).count()
    }

    pub fn active(&self) -> TabId {
        self.active
    }

    pub fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Always `false`: the list always holds at least one tab.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Tab> {
        self.tabs.iter()
    }

    pub fn get(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    /// Records the page tab that opened `id`. Unknown tabs and self-links are rejected.
    pub fn set_opener(&mut self, id: TabId, opener: TabId) -> bool {
        if id == opener || self.index_of(opener).is_none() {
            return false;
        }
        let Some(tab) = self.get_mut(id) else {
            return false;
        };
        tab.opener = Some(opener);
        true
    }

    /// Returns `true` if the title changed. Unchanged titles are not reallocated.
    pub fn set_title(&mut self, id: TabId, title: &str) -> bool {
        let Some(tab) = self.get_mut(id) else {
            return false;
        };
        if tab.title == title {
            return false;
        }
        title.clone_into(&mut tab.title);
        true
    }

    /// Returns `true` if the URL changed.
    pub fn set_url(&mut self, id: TabId, url: Option<&str>) -> bool {
        let Some(tab) = self.get_mut(id) else {
            return false;
        };
        if tab.url.as_deref() == url {
            return false;
        }
        tab.url = url.map(str::to_owned);
        true
    }

    /// Returns `true` if the loading state changed.
    pub fn set_loading(&mut self, id: TabId, loading: bool) -> bool {
        let Some(tab) = self.get_mut(id) else {
            return false;
        };
        if tab.loading == loading {
            return false;
        }
        tab.loading = loading;
        true
    }

    /// Returns `true` if the favicon changed.
    pub fn set_favicon(&mut self, id: TabId, favicon: Option<FaviconKey>) -> bool {
        let Some(tab) = self.get_mut(id) else {
            return false;
        };
        if tab.favicon == favicon {
            return false;
        }
        tab.favicon = favicon;
        true
    }

    fn active_index(&self) -> usize {
        self.index_of(self.active)
            .expect("the active tab is always in the list")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn ids(list: &TabList) -> Vec<TabId> {
        list.iter().map(|tab| tab.id).collect()
    }

    /// Builds a list of `n` tabs with ids 0..n, active index 0.
    fn list_of(n: usize) -> TabList {
        let mut list = TabList::new();
        for _ in 1..n {
            list.add(false);
        }
        list.select(TabId(0));
        list
    }

    #[test]
    fn new_list_has_one_active_empty_tab() {
        let list = TabList::new();

        assert_eq!(list.len(), 1);
        assert!(!list.is_empty());
        assert_eq!(list.active(), TabId(0));
        let tab = list.get(TabId(0)).expect("initial tab");
        assert_eq!(tab.title, "");
        assert_eq!(tab.url, None);
        assert!(!tab.loading);
        assert_eq!(tab.favicon, None);
    }

    #[test]
    fn default_matches_new() {
        let list = TabList::default();

        assert_eq!(ids(&list), vec![TabId(0)]);
        assert_eq!(list.active(), TabId(0));
    }

    #[test]
    fn display_title_falls_back_for_empty_and_blank_titles() {
        let mut tab = Tab::new(TabId(7));
        assert_eq!(display_title(&tab), "New tab");

        tab.title = "   \t".to_owned();
        assert_eq!(display_title(&tab), "New tab");

        tab.title = "Te Ara".to_owned();
        assert_eq!(display_title(&tab), "Te Ara");
    }

    #[test]
    fn add_at_end_keeps_active_and_appends() {
        let mut list = list_of(2);
        let new = list.add(false);

        assert_eq!(ids(&list), vec![TabId(0), TabId(1), new]);
        assert_eq!(list.active(), TabId(0));
    }

    #[test]
    fn add_after_active_inserts_right_of_active() {
        let mut list = list_of(3);
        list.select(TabId(1));
        let new = list.add(true);

        assert_eq!(ids(&list), vec![TabId(0), TabId(1), new, TabId(2)]);
        assert_eq!(list.active(), TabId(1));
    }

    #[test]
    fn add_after_active_when_active_is_last_appends() {
        let mut list = list_of(2);
        list.select(TabId(1));
        let new = list.add(true);

        assert_eq!(ids(&list), vec![TabId(0), TabId(1), new]);
    }

    #[test]
    fn select_known_and_unknown_ids() {
        let mut list = list_of(3);

        assert!(list.select(TabId(2)));
        assert_eq!(list.active(), TabId(2));

        assert!(!list.select(TabId(99)));
        assert_eq!(list.active(), TabId(2));
    }

    #[test]
    fn select_next_and_prev_wrap() {
        let mut list = list_of(3);

        list.select_next();
        assert_eq!(list.active(), TabId(1));
        list.select_next();
        list.select_next();
        assert_eq!(list.active(), TabId(0), "next wraps from last to first");

        list.select_prev();
        assert_eq!(list.active(), TabId(2), "prev wraps from first to last");
        list.select_prev();
        assert_eq!(list.active(), TabId(1));
    }

    #[test]
    fn select_next_and_prev_on_single_tab_stay_put() {
        let mut list = TabList::new();

        list.select_next();
        assert_eq!(list.active(), TabId(0));
        list.select_prev();
        assert_eq!(list.active(), TabId(0));
    }

    #[test]
    fn closing_active_selects_right_neighbour() {
        let mut list = list_of(3);
        list.select(TabId(1));

        let outcome = list.close(TabId(1));

        assert_eq!(outcome.new_active, TabId(2));
        assert_eq!(list.active(), TabId(2));
        assert_eq!(ids(&list), vec![TabId(0), TabId(2)]);
    }

    #[test]
    fn closing_opener_clears_children_opener_links() {
        let mut list = list_of(3);
        list.set_opener(TabId(1), TabId(0));
        list.set_opener(TabId(2), TabId(0));

        list.close(TabId(0));

        assert_eq!(list.get(TabId(1)).unwrap().opener, None);
        assert_eq!(list.get(TabId(2)).unwrap().opener, None);
    }

    #[test]
    fn closing_rightmost_active_selects_left_neighbour() {
        let mut list = list_of(3);
        list.select(TabId(2));

        let outcome = list.close(TabId(2));

        assert_eq!(outcome.new_active, TabId(1));
        assert_eq!(ids(&list), vec![TabId(0), TabId(1)]);
    }

    #[test]
    fn closing_first_active_selects_new_first() {
        let mut list = list_of(3);

        let outcome = list.close(TabId(0));

        assert_eq!(outcome.new_active, TabId(1));
        assert_eq!(ids(&list), vec![TabId(1), TabId(2)]);
    }

    #[test]
    fn closing_inactive_tab_keeps_active() {
        let mut list = list_of(3);
        list.select(TabId(1));

        let outcome = list.close(TabId(2));

        assert_eq!(outcome.new_active, TabId(1));
        assert_eq!(list.active(), TabId(1));
        assert_eq!(list.index_of(TabId(1)), Some(1));
        assert_eq!(ids(&list), vec![TabId(0), TabId(1)]);
    }

    #[test]
    fn closing_unknown_id_changes_nothing() {
        let mut list = list_of(2);
        list.select(TabId(1));

        let outcome = list.close(TabId(42));

        assert_eq!(outcome.new_active, TabId(1));
        assert_eq!(ids(&list), vec![TabId(0), TabId(1)]);
    }

    #[test]
    fn closing_last_tab_leaves_fresh_empty_tab() {
        let mut list = TabList::new();
        list.set_title(TabId(0), "Old");

        let outcome = list.close(TabId(0));

        assert_eq!(list.len(), 1);
        assert_ne!(outcome.new_active, TabId(0));
        assert_eq!(list.active(), outcome.new_active);
        let fresh = list.get(outcome.new_active).expect("fresh tab");
        assert_eq!(fresh.title, "");
        assert_eq!(display_title(fresh), "New tab");
    }

    #[test]
    fn closing_stale_id_on_last_tab_keeps_tab() {
        let mut list = TabList::new();

        let outcome = list.close(TabId(5));

        assert_eq!(outcome.new_active, TabId(0));
        assert_eq!(ids(&list), vec![TabId(0)]);
    }

    #[test]
    fn closed_ids_are_never_reused() {
        let mut list = TabList::new();
        let first = list.active();
        let second = list.add(false);
        list.close(first);
        let third = list.add(false);
        let outcome = list.close(list.active());
        assert_eq!(
            outcome.new_active, third,
            "closing active second leaves third"
        );
        let fresh = list.close(third).new_active;

        assert!(second > first);
        assert!(third > second);
        assert!(fresh > third, "last-tab replacement gets a new id");
        assert!(!list.select(first), "closed id must not select anything");
        assert!(list.get(first).is_none());
        assert!(list.get(third).is_none());
    }

    #[test]
    fn ids_stay_unique_across_many_adds_and_closes() {
        let mut list = TabList::new();
        let mut seen = HashSet::new();
        seen.insert(list.active());

        for round in 0..200 {
            let id = list.add(round % 3 == 0);
            assert!(seen.insert(id), "id {id:?} was issued twice");
            if round % 4 == 0 {
                list.close(id);
            }
            if round % 7 == 0 {
                list.close(list.active());
            }
        }

        assert!(!list.is_empty());
        let live: HashSet<TabId> = list.iter().map(|tab| tab.id).collect();
        assert_eq!(live.len(), list.len(), "live tab ids must be distinct");
    }

    #[test]
    fn move_tab_forward_and_backward() {
        let mut list = list_of(4);

        assert!(list.move_tab(TabId(0), 2));
        assert_eq!(ids(&list), vec![TabId(1), TabId(2), TabId(0), TabId(3)]);

        assert!(list.move_tab(TabId(3), 0));
        assert_eq!(ids(&list), vec![TabId(3), TabId(1), TabId(2), TabId(0)]);
    }

    #[test]
    fn move_tab_to_same_index_reports_no_change() {
        let mut list = list_of(3);

        assert!(!list.move_tab(TabId(1), 1));
        assert_eq!(ids(&list), vec![TabId(0), TabId(1), TabId(2)]);
    }

    #[test]
    fn move_tab_past_end_goes_last() {
        let mut list = list_of(3);

        assert!(list.move_tab(TabId(0), 99));
        assert_eq!(ids(&list), vec![TabId(1), TabId(2), TabId(0)]);
    }

    #[test]
    fn move_last_tab_past_end_is_no_change() {
        let mut list = list_of(3);

        assert!(!list.move_tab(TabId(2), 10));
        assert_eq!(ids(&list), vec![TabId(0), TabId(1), TabId(2)]);
    }

    #[test]
    fn move_tab_keeps_active_tab() {
        let mut list = list_of(3);
        list.select(TabId(2));

        list.move_tab(TabId(2), 0);

        assert_eq!(list.active(), TabId(2));
    }

    #[test]
    fn move_unknown_tab_is_ignored() {
        let mut list = list_of(2);

        assert!(!list.move_tab(TabId(9), 0));
        assert_eq!(ids(&list), vec![TabId(0), TabId(1)]);
    }

    #[test]
    fn move_single_tab_is_no_change() {
        let mut list = TabList::new();

        assert!(!list.move_tab(TabId(0), 0));
        assert!(!list.move_tab(TabId(0), 5));
        assert_eq!(ids(&list), vec![TabId(0)]);
    }

    #[test]
    fn pinning_moves_tab_to_stable_prefix_without_changing_active_identity() {
        let mut list = list_of(4);
        list.select(TabId(2));

        assert!(list.set_pinned(TabId(3), true));
        assert_eq!(ids(&list), vec![TabId(3), TabId(0), TabId(1), TabId(2)]);
        assert_eq!(list.active(), TabId(2));
        assert!(list.get(TabId(3)).unwrap().pinned);
    }

    #[test]
    fn unpinning_appends_after_remaining_pinned_tabs_and_preserves_active() {
        let mut list = list_of(4);
        list.set_pinned(TabId(0), true);
        list.set_pinned(TabId(2), true);
        list.select(TabId(2));

        assert!(list.set_pinned(TabId(0), false));
        assert_eq!(ids(&list), vec![TabId(2), TabId(0), TabId(1), TabId(3)]);
        assert_eq!(list.active(), TabId(2));
        assert!(!list.get(TabId(0)).unwrap().pinned);
    }

    #[test]
    fn pinning_unknown_or_unchanged_tabs_is_a_noop() {
        let mut list = list_of(2);

        assert!(!list.set_pinned(TabId(99), true));
        assert!(!list.set_pinned(TabId(0), false));
        assert_eq!(ids(&list), vec![TabId(0), TabId(1)]);
    }

    #[test]
    fn adding_after_active_never_splits_the_pinned_prefix() {
        let mut list = list_of(3);
        list.set_pinned(TabId(0), true);
        list.set_pinned(TabId(1), true);
        list.select(TabId(1));

        let added = list.add(true);

        assert_eq!(ids(&list), vec![TabId(0), TabId(1), added, TabId(2)]);
        assert!(!list.get(added).unwrap().pinned);
    }

    #[test]
    fn drag_reordering_cannot_move_tabs_across_the_pinned_partition() {
        let mut list = list_of(4);
        list.set_pinned(TabId(0), true);
        list.set_pinned(TabId(1), true);

        assert!(!list.move_tab(TabId(2), 0));
        assert_eq!(ids(&list), vec![TabId(0), TabId(1), TabId(2), TabId(3)]);
        assert!(list.move_tab(TabId(0), 3));
        assert_eq!(ids(&list), vec![TabId(1), TabId(0), TabId(2), TabId(3)]);
        assert_eq!(list.pinned_count(), 2);
    }

    #[test]
    fn placement_last_after_active_and_after_related_put_the_tab_where_asked() {
        let mut list = list_of(4);
        list.select(TabId(1));
        let last = list.add_placed(Placement::Last, None);
        assert_eq!(ids(&list), vec![TabId(0), TabId(1), TabId(2), TabId(3), last]);
        let beside = list.add_placed(Placement::AfterActive, Some(TabId(3)));
        assert_eq!(list.index_of(beside), Some(2), "right of the active tab, whatever the opener");
        assert_eq!(list.get(beside).unwrap().opener, Some(TabId(3)));
        let related = list.add_placed(Placement::AfterRelated, Some(TabId(3)));
        let at = list.index_of(related).unwrap();
        assert_eq!(list.index_of(TabId(3)).unwrap() + 1, at, "right of its opener");
    }

    #[test]
    fn related_tabs_stack_up_after_their_opener_in_opening_order() {
        let mut list = list_of(3);
        list.select(TabId(0));
        let first = list.add_placed(Placement::AfterRelated, Some(TabId(0)));
        let second = list.add_placed(Placement::AfterRelated, Some(TabId(0)));
        let third = list.add_placed(Placement::AfterRelated, Some(TabId(0)));
        assert_eq!(ids(&list), vec![TabId(0), first, second, third, TabId(1), TabId(2)]);
        let other = list.add_placed(Placement::AfterRelated, Some(TabId(1)));
        assert_eq!(list.index_of(other), Some(5), "another opener keeps its own place");
        let nested = list.add_placed(Placement::AfterRelated, Some(first));
        assert_eq!(list.index_of(nested), Some(2), "a child's child goes beside that child");
    }

    #[test]
    fn related_without_a_known_opener_falls_back_to_after_the_active_tab() {
        let mut list = list_of(3);
        list.select(TabId(1));
        let tab = list.add_placed(Placement::AfterRelated, None);
        assert_eq!(list.index_of(tab), Some(2));
        let stale = list.add_placed(Placement::AfterRelated, Some(TabId(99)));
        assert_eq!(list.index_of(stale), Some(2), "a closed opener is no opener");
        assert_eq!(list.get(stale).unwrap().opener, None);
    }

    #[test]
    fn placed_tabs_never_split_the_pinned_prefix() {
        let mut list = list_of(4);
        list.set_pinned(TabId(0), true);
        list.set_pinned(TabId(1), true);
        list.select(TabId(0));
        let after_active = list.add_placed(Placement::AfterActive, None);
        assert_eq!(list.index_of(after_active), Some(2));
        let related = list.add_placed(Placement::AfterRelated, Some(TabId(1)));
        assert_eq!(list.index_of(related), Some(2), "the first unpinned slot, ahead of earlier children");
        assert!(!list.get(related).unwrap().pinned);
        assert_eq!(list.pinned_count(), 2);
    }

    #[test]
    fn index_of_and_get_follow_order() {
        let mut list = list_of(3);
        list.move_tab(TabId(2), 0);

        assert_eq!(list.index_of(TabId(2)), Some(0));
        assert_eq!(list.index_of(TabId(0)), Some(1));
        assert_eq!(list.index_of(TabId(8)), None);
        assert!(list.get(TabId(8)).is_none());
        assert!(list.get_mut(TabId(8)).is_none());
    }

    #[test]
    fn set_title_reports_change_only_when_different() {
        let mut list = TabList::new();

        assert!(list.set_title(TabId(0), "Otago Daily Times"));
        assert!(!list.set_title(TabId(0), "Otago Daily Times"));
        assert!(list.set_title(TabId(0), ""));
        assert_eq!(list.get(TabId(0)).map(|t| t.title.as_str()), Some(""));
    }

    #[test]
    fn set_url_reports_change_and_allows_clearing() {
        let mut list = TabList::new();

        assert!(list.set_url(TabId(0), Some("https://example.nz/")));
        assert!(!list.set_url(TabId(0), Some("https://example.nz/")));
        assert!(list.set_url(TabId(0), None));
        assert!(!list.set_url(TabId(0), None));
        assert_eq!(list.get(TabId(0)).and_then(|t| t.url.clone()), None);
    }

    #[test]
    fn set_loading_and_favicon_report_change() {
        let mut list = TabList::new();

        assert!(list.set_loading(TabId(0), true));
        assert!(!list.set_loading(TabId(0), true));
        assert!(list.set_loading(TabId(0), false));

        assert!(list.set_favicon(TabId(0), Some(FaviconKey(3))));
        assert!(!list.set_favicon(TabId(0), Some(FaviconKey(3))));
        assert!(list.set_favicon(TabId(0), None));
        assert!(!list.set_favicon(TabId(0), None));
    }

    #[test]
    fn setters_ignore_unknown_ids() {
        let mut list = TabList::new();
        let stale = TabId(77);

        assert!(!list.set_title(stale, "Ghost"));
        assert!(!list.set_url(stale, Some("https://ghost.example/")));
        assert!(!list.set_loading(stale, true));
        assert!(!list.set_favicon(stale, Some(FaviconKey(1))));
        assert_eq!(list.get(TabId(0)), Some(&Tab::new(TabId(0))));
    }

    #[test]
    fn get_mut_allows_direct_field_updates() {
        let mut list = TabList::new();

        list.get_mut(TabId(0)).expect("tab").loading = true;

        assert!(list.get(TabId(0)).expect("tab").loading);
    }

    #[test]
    fn iter_yields_tabs_in_display_order() {
        let mut list = list_of(3);
        list.move_tab(TabId(0), 2);

        let order: Vec<TabId> = list.iter().map(|tab| tab.id).collect();

        assert_eq!(order, vec![TabId(1), TabId(2), TabId(0)]);
        assert_eq!(list.iter().len(), 3);
    }
}
