//! Runtime state that belongs to tabs but not to the tab model: unread marks, title notification
//! counts, the audio rule's mute, periodic reload timers, the selection made in the strip and the
//! time each tab was last used. All of it is in memory; nothing here is a setting.
//!
//! The pieces are plain data with pure transitions so each rule can be tested without a window or
//! an engine. `App` owns one [`TabState`] and one [`TabExtra`] per tab and calls these.

use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::tabs::TabId;

/// Intervals the tab menu offers for "Reload every", in seconds.
pub const RELOAD_PRESETS: [u32; 8] = [5, 15, 30, 60, 300, 900, 1800, 3600];
/// Shortest and longest interval a reload timer may have, in seconds.
pub const RELOAD_MIN_SECS: u32 = 5;
pub const RELOAD_MAX_SECS: u32 = 24 * 3600;

/// The words of an interval: "30 seconds", "5 minutes", "1 hour".
pub fn interval_label(secs: u32) -> String {
    let plural = |n: u32, unit: &str| format!("{n} {unit}{}", if n == 1 { "" } else { "s" });
    if secs % 3600 == 0 {
        plural(secs / 3600, "hour")
    } else if secs % 60 == 0 {
        plural(secs / 60, "minute")
    } else {
        plural(secs, "second")
    }
}

/// A tab's periodic reload: the interval and when it next fires.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeriodicReload {
    pub interval: Duration,
    pub next: Instant,
}

impl PeriodicReload {
    pub fn new(secs: u32, now: Instant) -> Option<Self> {
        if !(RELOAD_MIN_SECS..=RELOAD_MAX_SECS).contains(&secs) {
            return None;
        }
        let interval = Duration::from_secs(u64::from(secs));
        Some(Self { interval, next: now + interval })
    }

    pub fn secs(&self) -> u32 {
        self.interval.as_secs() as u32
    }

    /// Seconds until the next reload, rounded up, never negative.
    pub fn remaining_secs(&self, now: Instant) -> u32 {
        let left = self.next.saturating_duration_since(now);
        (left.as_millis().div_ceil(1000)).min(u128::from(u32::MAX)) as u32
    }

    /// Whether the reload is due at `now`.
    pub fn due(&self, now: Instant) -> bool {
        now >= self.next
    }

    /// The timer after a reload at `now`: the next one is a full interval away (a slow tick never
    /// makes up a missed reload with several in a row).
    pub fn fired(self, now: Instant) -> Self {
        Self { next: now + self.interval, ..self }
    }
}

/// Per-tab state outside the tab model.
#[derive(Clone, Debug, Default)]
pub struct TabExtra {
    /// Changed while it was in the background and not looked at since.
    pub unread: bool,
    /// The count a page title announces ("(3) Inbox"), 0 for none.
    pub notify: u32,
    /// The audio rule has muted this tab (the user's own mute is `AppTab::muted`).
    pub auto_muted: bool,
    pub reload: Option<PeriodicReload>,
    /// Put to sleep by the user (Hibernate tab); cleared when the tab is shown again.
    pub hibernated: bool,
    /// A pinned tab may be hibernated this once ("Minimise instead of close").
    pub allow_pinned_sleep: bool,
    /// A name the user gave the tab; shown instead of the page title.
    pub custom_title: Option<String>,
    /// The site (registrable host) the tab was pinned on, for "restrict pinned tabs to the site".
    pub pinned_site: Option<String>,
    /// When the tab was last the active tab, seconds since the Unix epoch. 0 when unknown.
    pub last_used: u64,
}

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl TabExtra {
    pub fn fresh() -> Self {
        Self { last_used: unix_now(), ..Self::default() }
    }

    /// The title the strip shows: the user's name for the tab, else the page's.
    pub fn display_title<'a>(&'a self, page_title: &'a str) -> &'a str {
        match self.custom_title.as_deref() {
            Some(name) if !name.trim().is_empty() => name,
            _ => page_title,
        }
    }

    /// Records that a title arrived. Returns true when the unread mark or the count changed.
    /// A background tab whose count grew is unread; the count itself is always kept current.
    pub fn title_arrived(&mut self, notify: u32, in_background: bool) -> bool {
        let grew = notify > self.notify;
        let changed_count = notify != self.notify;
        self.notify = notify;
        let mut changed = changed_count;
        if in_background && grew && !self.unread {
            self.unread = true;
            changed = true;
        }
        changed
    }

    /// A background tab finished loading something new.
    pub fn loaded_in_background(&mut self) -> bool {
        !std::mem::replace(&mut self.unread, true)
    }

    /// The tab was shown: nothing is unread any more. Returns true when that changed anything.
    pub fn seen(&mut self) -> bool {
        std::mem::take(&mut self.unread)
    }
}

/// App-level tab state.
#[derive(Clone, Debug, Default)]
pub struct TabState {
    /// The tabs the strip has selected with Ctrl/Shift click, in strip order. The active tab is
    /// not in here unless the strip put it there.
    pub selected: Vec<TabId>,
    /// The one-second timer is running (periodic reload countdowns or the audio rule).
    pub tick_armed: bool,
    /// Tab thumbnails the hover card shows, newest capture per tab.
    pub thumbs: HashMap<TabId, Vec<u8>>,
    /// When each tab's thumbnail was last captured (a capture is not repeated within a few seconds).
    pub thumb_at: HashMap<TabId, std::time::Instant>,
    /// The working set of each tab's page process in bytes, as last measured, and when.
    pub memory: HashMap<TabId, u64>,
    pub memory_at: Option<std::time::Instant>,
    /// Links a pinned tab refused to follow, waiting to open in a tab of their own: (the pinned tab, address).
    pub deferred_links: Vec<(TabId, url::Url)>,
    /// Same-tab link navigations a workspace rule matched, waiting (outside the engine's event) to
    /// move their tab to the rule's workspace: (the tab, address).
    pub deferred_workspace: Vec<(TabId, url::Url)>,
    /// Restored tabs that load as soon as the engine is ready ("Load restored tabs when first
    /// selected" off, or pinned tabs kept loading).
    pub eager: Vec<TabId>,
}

impl TabState {
    /// Keeps only ids that are still tabs, in `order`'s order, without repeats.
    pub fn set_selection(&mut self, ids: &[TabId], order: &[TabId]) {
        self.selected = order.iter().copied().filter(|tab| ids.contains(tab)).collect();
    }

    pub fn forget(&mut self, id: TabId) {
        self.selected.retain(|tab| *tab != id);
        self.thumbs.remove(&id);
        self.thumb_at.remove(&id);
        self.memory.remove(&id);
    }

    /// The tabs a tab action applies to: the selection when `id` is part of it, else just `id`.
    pub fn targets(&self, id: TabId, include_active: Option<TabId>) -> Vec<TabId> {
        let mut ids = self.selected.clone();
        if let Some(active) = include_active
            && !ids.is_empty()
            && !ids.contains(&active)
        {
            ids.push(active);
        }
        if ids.len() > 1 && ids.contains(&id) { ids } else { vec![id] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reload_timer_counts_down_and_fires_a_full_interval_apart() {
        let start = Instant::now();
        let timer = PeriodicReload::new(30, start).expect("30 seconds is allowed");
        assert_eq!(timer.secs(), 30);
        assert_eq!(timer.remaining_secs(start), 30);
        assert_eq!(timer.remaining_secs(start + Duration::from_millis(29_100)), 1, "rounded up");
        assert!(!timer.due(start + Duration::from_secs(29)));
        assert!(timer.due(start + Duration::from_secs(30)));
        let late = start + Duration::from_secs(95);
        let next = timer.fired(late);
        assert_eq!(next.next, late + Duration::from_secs(30), "one reload for a long gap, not three");
        assert_eq!(next.remaining_secs(late + Duration::from_secs(40)), 0);
    }

    #[test]
    fn reload_intervals_are_bounded_and_every_preset_is_allowed() {
        let now = Instant::now();
        assert!(PeriodicReload::new(4, now).is_none());
        assert!(PeriodicReload::new(RELOAD_MAX_SECS + 1, now).is_none());
        for preset in RELOAD_PRESETS {
            assert!(PeriodicReload::new(preset, now).is_some(), "{preset}");
        }
        assert_eq!(interval_label(5), "5 seconds");
        assert_eq!(interval_label(60), "1 minute");
        assert_eq!(interval_label(900), "15 minutes");
        assert_eq!(interval_label(3600), "1 hour");
        assert_eq!(interval_label(7200), "2 hours");
    }

    #[test]
    fn a_background_tab_is_unread_when_its_count_grows_or_it_loads() {
        let mut tab = TabExtra::default();
        assert!(!tab.title_arrived(0, true), "nothing to report");
        assert!(tab.title_arrived(2, true));
        assert!(tab.unread);
        assert_eq!(tab.notify, 2);
        assert!(tab.seen());
        assert!(!tab.unread);
        assert!(tab.title_arrived(1, true), "the count fell: it changed, but nothing new to read");
        assert!(!tab.unread);
        assert!(tab.title_arrived(4, false), "the active tab keeps its count but is never unread");
        assert!(!tab.unread);
        assert!(tab.loaded_in_background());
        assert!(tab.unread);
        assert!(!tab.loaded_in_background(), "already marked");
        assert!(!TabExtra::default().seen());
    }

    #[test]
    fn a_name_the_user_gave_wins_over_the_page_title() {
        let mut tab = TabExtra::default();
        assert_eq!(tab.display_title("Page"), "Page");
        tab.custom_title = Some("  ".into());
        assert_eq!(tab.display_title("Page"), "Page", "a blank name is no name");
        tab.custom_title = Some("Work mail".into());
        assert_eq!(tab.display_title("Page"), "Work mail");
    }

    #[test]
    fn the_selection_keeps_strip_order_and_drops_closed_tabs() {
        let mut state = TabState::default();
        let order = [TabId(1), TabId(2), TabId(3), TabId(4)];
        state.set_selection(&[TabId(4), TabId(2), TabId(9)], &order);
        assert_eq!(state.selected, vec![TabId(2), TabId(4)]);
        state.forget(TabId(2));
        assert_eq!(state.selected, vec![TabId(4)]);
    }

    #[test]
    fn an_action_on_a_selected_tab_applies_to_the_whole_selection() {
        let mut state = TabState::default();
        let order = [TabId(1), TabId(2), TabId(3), TabId(4)];
        state.set_selection(&[TabId(2), TabId(3)], &order);
        assert_eq!(state.targets(TabId(3), None), vec![TabId(2), TabId(3)]);
        assert_eq!(state.targets(TabId(4), None), vec![TabId(4)], "a tab outside the selection acts alone");
        assert_eq!(state.targets(TabId(2), Some(TabId(1))), vec![TabId(2), TabId(3), TabId(1)], "the active tab joins when asked");
        state.set_selection(&[TabId(2)], &order);
        assert_eq!(state.targets(TabId(2), None), vec![TabId(2)], "one tab is not a selection");
        assert_eq!(TabState::default().targets(TabId(1), Some(TabId(1))), vec![TabId(1)]);
    }
}
