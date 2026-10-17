//! Auto-hide of the browser UI (thread T11, docs/design/t11-auto-hide.md).
//!
//! Pure rules, no window code: the settings (`frameless` is the master switch, `auto_hide_bars` and
//! `auto_hide_fullscreen` refine it), which bars are out of the layout right now, the reveal and hide
//! state machine (on an explicit clock so it is testable), and where the pointer sensors go.
//! `win32/auto_hide.rs` wires these to the window; `chrome/layout.rs` does the geometry.

use crate::chrome::layout::{Metrics, Rect};
use crate::settings::TabLayout;

/// Saved bar list, e.g. `tabs,address,bookmarks`.
pub const KEY_BARS: &str = "auto_hide_bars";
/// Saved flag: always auto-hide in full screen.
pub const KEY_FULLSCREEN: &str = "auto_hide_fullscreen";
/// Every key [`AutoHideSettings`] owns, in the order [`AutoHideSettings::pairs`] writes them.
pub const SETTING_KEYS: [&str; 2] = [KEY_BARS, KEY_FULLSCREEN];

/// The pointer must rest on a sensor this long before the bars appear (a pass through to another
/// monitor or the taskbar opens nothing). Fixed: there is no setting for it.
pub const REVEAL_DELAY_MS: u64 = 150;
/// The bars stay this long after the pointer leaves them.
pub const HIDE_DELAY_MS: u64 = 500;
/// How far past a shown bar the pointer may stray (logical px) before the hide delay starts.
pub const HIDE_MARGIN_DIP: i32 = 8;
/// Sensor strip thickness (logical px) where another monitor lies past the edge, so a pointer that
/// crosses still lands on it. At a screen edge that stops the cursor it is one physical pixel.
pub const SENSOR_DIP: i32 = 4;
/// How often the pointer is checked while bars are shown or a reveal is pending (never otherwise).
pub const POLL_MS: u32 = 50;

/// A set of bars. Add a bar by giving it a bit and a row in [`BAR_TABLE`]; nothing else enumerates them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct Bars(u8);

impl Bars {
    pub const NONE: Self = Self(0);
    /// The tab strip or the vertical sidebar.
    pub const TABS: Self = Self(1);
    /// The toolbar with the address bar.
    pub const ADDRESS: Self = Self(2);
    pub const BOOKMARKS: Self = Self(4);
    /// The side panel: it opens over the page instead of taking room from it.
    pub const PANEL: Self = Self(8);
    // The status bar is added by thread T6 (layout). Give it `Self(16)` and a row in BAR_TABLE.

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Every bar that can hide, with its saved name, in page order.
pub const BAR_TABLE: [(&str, Bars); 4] = [
    ("tabs", Bars::TABS),
    ("address", Bars::ADDRESS),
    ("bookmarks", Bars::BOOKMARKS),
    ("panel", Bars::PANEL),
];

/// Every hideable bar.
pub const ALL_BARS: Bars = {
    let mut all = Bars::NONE;
    let mut i = 0;
    while i < BAR_TABLE.len() {
        all = all.union(BAR_TABLE[i].1);
        i += 1;
    }
    all
};

/// What hides by default: the three bars that sit above or beside the page.
pub const DEFAULT_BARS: Bars = Bars::TABS.union(Bars::ADDRESS).union(Bars::BOOKMARKS);

impl Bars {
    /// The bars named by a comma list; `None` for a name that is not in [`BAR_TABLE`] or a repeat.
    /// The empty text is no bars.
    pub fn parse(text: &str) -> Option<Self> {
        let mut out = Self::NONE;
        if text.trim().is_empty() {
            return Some(out);
        }
        for name in text.split(',') {
            let (_, bar) = BAR_TABLE.iter().find(|(key, _)| *key == name.trim())?;
            if out.contains(*bar) {
                return None;
            }
            out = out.union(*bar);
        }
        Some(out)
    }

    /// The saved form: names in [`BAR_TABLE`] order.
    pub fn text(self) -> String {
        BAR_TABLE.iter().filter(|(_, bar)| self.contains(*bar)).map(|(key, _)| *key).collect::<Vec<_>>().join(",")
    }
}

/// The two rows added to the master `frameless` switch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AutoHideSettings {
    /// `auto_hide_bars`: which bars hide while the master switch is on.
    pub bars: Bars,
    /// `auto_hide_fullscreen` (default true): in full screen every bar hides, whatever the master switch says.
    pub in_fullscreen: bool,
}

impl Default for AutoHideSettings {
    fn default() -> Self {
        Self { bars: DEFAULT_BARS, in_fullscreen: true }
    }
}

fn flag(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn flag_text(on: bool) -> &'static str {
    if on { "true" } else { "false" }
}

impl AutoHideSettings {
    /// Reads one `settings.txt` line; a value that does not parse keeps the current one. `false` for a key that is not ours.
    pub fn parse_key(&mut self, key: &str, value: &str) -> bool {
        match key {
            KEY_BARS => {
                if let Some(bars) = Bars::parse(value) {
                    self.bars = bars;
                }
            }
            KEY_FULLSCREEN => {
                if let Some(on) = flag(value) {
                    self.in_fullscreen = on;
                }
            }
            _ => return false,
        }
        true
    }

    /// Like [`parse_key`](Self::parse_key) but strict, for the settings page's save: the exact saved
    /// spelling or nothing. `None` for a key that is not ours or a value Bowser refuses.
    pub fn apply_strict(&mut self, key: &str, value: &str) -> Option<bool> {
        match key {
            KEY_BARS => {
                let bars = Bars::parse(value).filter(|bars| bars.text() == value)?;
                self.bars = bars;
            }
            KEY_FULLSCREEN => {
                self.in_fullscreen = match value {
                    "true" => true,
                    "false" => false,
                    _ => return None,
                }
            }
            _ => return None,
        }
        Some(true)
    }

    /// The `key=value` pairs `settings.txt` stores, in [`SETTING_KEYS`] order.
    pub fn pairs(&self) -> Vec<(&'static str, String)> {
        vec![(KEY_BARS, self.bars.text()), (KEY_FULLSCREEN, flag_text(self.in_fullscreen).to_owned())]
    }
}

/// Full screen, as far as the layout is concerned.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Fullscreen {
    #[default]
    No,
    /// The window fills the monitor (F11).
    Window,
    /// A page element is full screen (a video): the page owns the whole window.
    Page,
}

/// What is out of the layout right now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Effective {
    pub hidden: Bars,
    /// Whether the pointer or the keyboard may bring hidden bars back. False while a page element is full screen.
    pub reveal: bool,
}

/// The bars that auto-hide, from the master switch (`frameless`), the two rows and the window state.
pub fn effective(master: bool, settings: &AutoHideSettings, fullscreen: Fullscreen) -> Effective {
    match fullscreen {
        Fullscreen::Page => Effective { hidden: ALL_BARS, reveal: false },
        Fullscreen::Window if settings.in_fullscreen => Effective { hidden: ALL_BARS, reveal: true },
        _ if master => Effective { hidden: settings.bars, reveal: true },
        _ => Effective { hidden: Bars::NONE, reveal: true },
    }
}

/// What the hidden bars take out of the window, per layout.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Plan {
    /// The toolbar, and in the horizontal layouts the tab row, with it (they share the top edge).
    pub top: bool,
    /// The vertical layout's sidebar.
    pub sidebar: bool,
    /// An open side panel floats over the page instead of taking room from it.
    pub panel: bool,
    /// The bookmarks bar.
    pub bookmarks: bool,
}

/// Which parts of the window `hidden` takes out of `layout`. Hiding tabs in a horizontal layout hides
/// the whole top chrome, because the tab row and the toolbar share the top edge and cannot be laid
/// out one without the other.
pub fn plan(hidden: Bars, layout: TabLayout) -> Plan {
    let vertical = layout.is_vertical();
    Plan {
        top: hidden.contains(Bars::ADDRESS) || (!vertical && hidden.contains(Bars::TABS)),
        sidebar: vertical && hidden.contains(Bars::TABS),
        panel: hidden.contains(Bars::PANEL),
        bookmarks: hidden.contains(Bars::BOOKMARKS),
    }
}

impl Plan {
    /// The part of this plan `layout.rs` lays out.
    pub fn layout(self) -> crate::chrome::layout::AutoHide {
        crate::chrome::layout::AutoHide { top: self.top, sidebar: self.sidebar }
    }
}

/// A part of the window that comes back when the pointer rests at its edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Zone {
    /// Toolbar and tab row, from the top edge.
    Top,
    /// The vertical sidebar, from the rail's edge.
    Sidebar,
    /// The bookmarks bar, when the top chrome is not hiding with it.
    Bookmarks,
}

impl Zone {
    const fn bit(self) -> u8 {
        match self {
            Self::Top => 1,
            Self::Sidebar => 2,
            Self::Bookmarks => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Zones(u8);

impl Zones {
    pub const NONE: Self = Self(0);

    pub const fn contains(self, zone: Zone) -> bool {
        self.0 & zone.bit() != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn with(self, zone: Zone) -> Self {
        Self(self.0 | zone.bit())
    }

    pub fn without(self, zone: Zone) -> Self {
        Self(self.0 & !zone.bit())
    }

    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// The zones with something to reveal. The bookmarks bar reveals with the top chrome when that hides
/// too and the bar sits at the top; otherwise it has its own zone.
pub fn zones(plan: Plan, bookmarks_present: bool, bookmarks_at_bottom: bool) -> Zones {
    let mut out = Zones::NONE;
    if plan.top {
        out = out.with(Zone::Top);
    }
    if plan.sidebar {
        out = out.with(Zone::Sidebar);
    }
    if plan.bookmarks && bookmarks_present && (bookmarks_at_bottom || !plan.top) {
        out = out.with(Zone::Bookmarks);
    }
    out
}

/// Whether the bookmarks bar is part of the top chrome's reveal (it hides and shows with it).
pub fn bookmarks_ride_with_top(plan: Plan, bookmarks_at_bottom: bool) -> bool {
    plan.bookmarks && plan.top && !bookmarks_at_bottom
}

/// Sensor strip thickness in physical pixels: one pixel where the screen edge stops the cursor, else
/// [`SENSOR_DIP`] logical pixels.
pub fn sensor_thickness(dpi: u32, screen_edge_stops_cursor: bool) -> i32 {
    if screen_edge_stops_cursor { 1 } else { Metrics::new(dpi).px(SENSOR_DIP).max(1) }
}

/// One pointer sensor: a strip of the page at the edge a hidden bar comes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sensor {
    pub zone: Zone,
    pub rect: Rect,
}

/// Where the sensors go. `page` is the page rectangle as laid out with the bars hidden;
/// `bookmarks_edge` is the rectangle the bar occupies when shown (when it has its own zone). `thin`
/// says the strip lies at a screen edge that stops the cursor (maximised or full screen, no monitor
/// beyond); `thin_top` and `thin_left` carry that per edge.
pub fn sensors(zones: Zones, page: Rect, bookmarks_bar: Rect, bookmarks_at_bottom: bool, dpi: u32, thin_top: bool, thin_left: bool) -> Vec<Sensor> {
    let mut out = Vec::new();
    if zones.contains(Zone::Top) {
        let t = sensor_thickness(dpi, thin_top);
        out.push(Sensor { zone: Zone::Top, rect: Rect::new(page.left, page.top, page.right, (page.top + t).min(page.bottom)) });
    }
    if zones.contains(Zone::Sidebar) {
        let t = sensor_thickness(dpi, thin_left);
        out.push(Sensor { zone: Zone::Sidebar, rect: Rect::new(page.left, page.top, (page.left + t).min(page.right), page.bottom) });
    }
    if zones.contains(Zone::Bookmarks) && !bookmarks_bar.is_empty() {
        let t = sensor_thickness(dpi, false);
        let rect = if bookmarks_at_bottom {
            Rect::new(bookmarks_bar.left, (bookmarks_bar.bottom - t).max(bookmarks_bar.top), bookmarks_bar.right, bookmarks_bar.bottom)
        } else {
            Rect::new(bookmarks_bar.left, bookmarks_bar.top, bookmarks_bar.right, (bookmarks_bar.top + t).min(bookmarks_bar.bottom))
        };
        out.push(Sensor { zone: Zone::Bookmarks, rect });
    }
    out
}

/// What the state machine needs to know each tick.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Tick {
    /// The pointer is over a shown bar (plus [`HIDE_MARGIN_DIP`] around it).
    pub pointer_over_bars: bool,
    /// Something keeps the bars up: the address bar has focus, a menu or popup from them is open, a
    /// tab is being dragged, or the window is not in front.
    pub held: bool,
}

/// Reveal and hide timing on an explicit clock (`now` is milliseconds from any fixed start).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Machine {
    shown: Zones,
    dwell: Option<(Zone, u64)>,
    outside_since: Option<u64>,
}

impl Machine {
    pub fn shown(&self) -> Zones {
        self.shown
    }

    /// Whether a timer is needed: a reveal is pending or bars are up. When false nothing polls.
    pub fn active(&self) -> bool {
        self.dwell.is_some() || !self.shown.is_empty()
    }

    /// The pointer is on `zone`'s sensor. The reveal happens after [`REVEAL_DELAY_MS`] of staying.
    pub fn enter_sensor(&mut self, zone: Zone, now: u64) {
        if self.shown.contains(zone) {
            self.outside_since = None;
            return;
        }
        match self.dwell {
            Some((current, _)) if current == zone => {}
            _ => self.dwell = Some((zone, now)),
        }
    }

    /// The pointer left `zone`'s sensor before the delay ran out.
    pub fn leave_sensor(&mut self, zone: Zone) {
        if matches!(self.dwell, Some((current, _)) if current == zone) {
            self.dwell = None;
        }
    }

    /// Brings `zones` up now (the keyboard: no delay). Returns whether anything changed.
    pub fn reveal(&mut self, zones: Zones, now: u64) -> bool {
        let before = self.shown;
        self.shown = self.shown.union(zones);
        if self.shown != before {
            self.outside_since = Some(now);
        }
        self.shown != before
    }

    /// Hides everything and forgets a pending reveal. Returns whether anything was shown.
    pub fn reset(&mut self) -> bool {
        let was = !self.shown.is_empty();
        *self = Self::default();
        was
    }

    /// Advances the clock. Returns whether the set of shown zones changed.
    pub fn tick(&mut self, now: u64, tick: Tick) -> bool {
        let before = self.shown;
        if let Some((zone, since)) = self.dwell {
            if now.saturating_sub(since) >= REVEAL_DELAY_MS {
                self.shown = self.shown.with(zone);
                self.dwell = None;
                self.outside_since = Some(now);
            }
        }
        if !self.shown.is_empty() {
            if tick.pointer_over_bars || tick.held {
                self.outside_since = None;
            } else {
                let since = *self.outside_since.get_or_insert(now);
                if now.saturating_sub(since) >= HIDE_DELAY_MS {
                    self.shown = Zones::NONE;
                    self.outside_since = None;
                }
            }
        } else {
            self.outside_since = None;
        }
        self.shown != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(bars: &str, fs: bool) -> AutoHideSettings {
        AutoHideSettings { bars: Bars::parse(bars).unwrap(), in_fullscreen: fs }
    }

    #[test]
    fn the_bar_list_round_trips_in_page_order_and_refuses_anything_else() {
        assert_eq!(DEFAULT_BARS.text(), "tabs,address,bookmarks");
        assert_eq!(Bars::parse("panel,tabs").unwrap().text(), "tabs,panel");
        assert_eq!(Bars::parse("").unwrap(), Bars::NONE);
        assert_eq!(Bars::parse("tabs,tabs"), None, "a repeat");
        assert_eq!(Bars::parse("status"), None, "the status bar arrives with T6");
        assert_eq!(Bars::parse("tabs,,address"), None);
        assert_eq!(ALL_BARS.text(), "tabs,address,bookmarks,panel");
    }

    #[test]
    fn settings_read_lenient_from_the_file_and_strict_from_the_page() {
        let mut s = AutoHideSettings::default();
        assert_eq!(s, settings("tabs,address,bookmarks", true));
        assert!(s.parse_key(KEY_BARS, "panel"));
        assert!(s.parse_key(KEY_FULLSCREEN, "off"));
        assert_eq!(s, settings("panel", false));
        assert!(s.parse_key(KEY_BARS, "nonsense"));
        assert!(s.parse_key(KEY_FULLSCREEN, "maybe"));
        assert_eq!(s, settings("panel", false), "a bad value keeps the old one");
        assert!(!s.parse_key("frameless", "true"), "the master switch is not ours");
        let mut t = AutoHideSettings::default();
        assert_eq!(t.apply_strict(KEY_BARS, "address,tabs"), None, "not the saved spelling");
        assert_eq!(t.apply_strict(KEY_BARS, "tabs,address"), Some(true));
        assert_eq!(t.apply_strict(KEY_FULLSCREEN, "on"), None);
        assert_eq!(t.apply_strict(KEY_FULLSCREEN, "false"), Some(true));
        assert_eq!(t.apply_strict("other", "x"), None);
        assert_eq!(t.pairs(), vec![(KEY_BARS, "tabs,address".to_owned()), (KEY_FULLSCREEN, "false".to_owned())]);
        let keys: Vec<&str> = t.pairs().iter().map(|(key, _)| *key).collect();
        assert_eq!(keys, SETTING_KEYS);
    }

    #[test]
    fn the_master_switch_rows_and_full_screen_decide_what_hides() {
        let s = settings("tabs,address", true);
        assert_eq!(effective(false, &s, Fullscreen::No).hidden, Bars::NONE, "off: nothing hides, the layout is as before");
        assert_eq!(effective(true, &s, Fullscreen::No).hidden, s.bars);
        assert_eq!(effective(false, &s, Fullscreen::Window), Effective { hidden: ALL_BARS, reveal: true }, "always in full screen");
        assert_eq!(effective(false, &settings("tabs", false), Fullscreen::Window).hidden, Bars::NONE, "row off: full screen keeps the bars");
        assert_eq!(effective(true, &settings("tabs", false), Fullscreen::Window).hidden, Bars::TABS, "the master switch still applies");
        assert_eq!(effective(false, &s, Fullscreen::Page), Effective { hidden: ALL_BARS, reveal: false }, "a video fills the window and nothing reveals");
        assert_eq!(effective(true, &settings("", true), Fullscreen::No).hidden, Bars::NONE, "on with no bars chosen hides none");
    }

    #[test]
    fn the_plan_takes_each_bar_out_of_each_layout() {
        let all = ALL_BARS;
        for layout in [TabLayout::ToolbarTop, TabLayout::TabsTop, TabLayout::SingleRow] {
            assert_eq!(plan(Bars::TABS, layout), Plan { top: true, ..Plan::default() }, "{layout:?}: the tab row hides the whole top");
            assert_eq!(plan(Bars::ADDRESS, layout), Plan { top: true, ..Plan::default() }, "{layout:?}");
            assert_eq!(plan(Bars::NONE, layout), Plan::default(), "{layout:?}");
            assert!(!plan(all, layout).sidebar, "{layout:?}: no sidebar");
        }
        assert_eq!(plan(Bars::TABS, TabLayout::Vertical), Plan { sidebar: true, ..Plan::default() }, "vertical: tabs are the sidebar");
        assert_eq!(plan(Bars::ADDRESS, TabLayout::Vertical), Plan { top: true, ..Plan::default() }, "vertical: address is the toolbar");
        assert_eq!(plan(Bars::BOOKMARKS, TabLayout::ToolbarTop), Plan { bookmarks: true, ..Plan::default() });
        assert_eq!(plan(Bars::PANEL, TabLayout::ToolbarTop), Plan { panel: true, ..Plan::default() });
        assert_eq!(plan(all, TabLayout::Vertical), Plan { top: true, sidebar: true, panel: true, bookmarks: true });
        assert_eq!(plan(Bars::TABS, TabLayout::Vertical).layout(), crate::chrome::layout::AutoHide { top: false, sidebar: true });
        assert!(plan(Bars::NONE, TabLayout::Vertical).layout().is_off());
    }

    #[test]
    fn the_bookmarks_bar_reveals_with_the_top_chrome_or_on_its_own() {
        let both = plan(Bars::ADDRESS.union(Bars::BOOKMARKS), TabLayout::ToolbarTop);
        assert_eq!(zones(both, true, false), Zones::NONE.with(Zone::Top), "at the top it rides along");
        assert!(bookmarks_ride_with_top(both, false));
        assert_eq!(zones(both, true, true), Zones::NONE.with(Zone::Top).with(Zone::Bookmarks), "at the bottom it has its own edge");
        let alone = plan(Bars::BOOKMARKS, TabLayout::ToolbarTop);
        assert_eq!(zones(alone, true, false), Zones::NONE.with(Zone::Bookmarks));
        assert_eq!(zones(alone, false, false), Zones::NONE, "no bar shown, nothing to reveal");
        let side = plan(Bars::TABS, TabLayout::Vertical);
        assert_eq!(zones(side, false, false), Zones::NONE.with(Zone::Sidebar));
    }

    #[test]
    fn sensors_are_strips_of_the_page_at_the_hidden_bars_edges() {
        let page = Rect::new(30, 0, 1920, 1080);
        let z = Zones::NONE.with(Zone::Top).with(Zone::Sidebar);
        let at_100 = sensors(z, page, Rect::default(), false, 96, true, true);
        assert_eq!(at_100[0], Sensor { zone: Zone::Top, rect: Rect::new(30, 0, 1920, 1) }, "one pixel at a screen edge");
        assert_eq!(at_100[1], Sensor { zone: Zone::Sidebar, rect: Rect::new(30, 0, 31, 1080) });
        let at_150 = sensors(z, page, Rect::default(), false, 144, false, false);
        assert_eq!(at_150[0].rect, Rect::new(30, 0, 1920, 6), "4 DIP at 150% with a neighbouring monitor or a window edge");
        assert_eq!(at_150[1].rect, Rect::new(30, 0, 36, 1080));
        assert_eq!(sensor_thickness(96, false), 4);
        assert_eq!(sensor_thickness(192, false), 8);
        let own = sensors(Zones::NONE.with(Zone::Bookmarks), page, Rect::new(30, 40, 1920, 72), false, 96, false, false);
        assert_eq!(own, vec![Sensor { zone: Zone::Bookmarks, rect: Rect::new(30, 40, 1920, 44) }]);
        let bottom = sensors(Zones::NONE.with(Zone::Bookmarks), page, Rect::new(30, 1048, 1920, 1080), true, 96, false, false);
        assert_eq!(bottom[0].rect, Rect::new(30, 1076, 1920, 1080));
        assert!(sensors(Zones::NONE, page, Rect::default(), false, 96, true, true).is_empty());
    }

    #[test]
    fn a_reveal_needs_the_pointer_to_rest_and_a_pass_through_opens_nothing() {
        let mut m = Machine::default();
        assert!(!m.active(), "idle: no timer");
        m.enter_sensor(Zone::Top, 1000);
        assert!(m.active());
        assert!(!m.tick(1000 + REVEAL_DELAY_MS - 1, Tick::default()));
        m.leave_sensor(Zone::Top);
        assert!(!m.tick(1000 + REVEAL_DELAY_MS + 50, Tick::default()), "it left before the delay");
        assert!(!m.active());
        m.enter_sensor(Zone::Top, 2000);
        m.enter_sensor(Zone::Top, 2100); // moving on the sensor keeps the first time
        assert!(m.tick(2000 + REVEAL_DELAY_MS, Tick { pointer_over_bars: true, held: false }));
        assert!(m.shown().contains(Zone::Top));
    }

    #[test]
    fn shown_bars_hide_half_a_second_after_the_pointer_leaves_them() {
        let mut m = Machine::default();
        m.enter_sensor(Zone::Top, 0);
        assert!(m.tick(REVEAL_DELAY_MS, Tick { pointer_over_bars: true, held: false }));
        let over = Tick { pointer_over_bars: true, held: false };
        let away = Tick::default();
        assert!(!m.tick(5000, over), "over the bars they stay however long");
        assert!(!m.tick(6000, away), "the clock starts when the pointer leaves");
        assert!(!m.tick(6000 + HIDE_DELAY_MS - 1, away));
        assert!(!m.tick(6200, over), "back over them: the clock resets");
        assert!(!m.tick(6300, away));
        assert!(!m.tick(6300 + HIDE_DELAY_MS - 1, away));
        assert!(m.tick(6300 + HIDE_DELAY_MS, away), "gone after the delay");
        assert!(m.shown().is_empty());
        assert!(!m.active(), "and the timer can stop");
    }

    #[test]
    fn nothing_hides_while_something_holds_the_bars() {
        let mut m = Machine::default();
        assert!(m.reveal(Zones::NONE.with(Zone::Top), 0), "the keyboard reveals at once");
        assert!(!m.reveal(Zones::NONE.with(Zone::Top), 10), "again: no change");
        let held = Tick { pointer_over_bars: false, held: true };
        assert!(!m.tick(10_000, held), "the address bar has focus, a menu is open, a tab is dragged or the window is behind");
        assert!(!m.tick(20_000, held));
        assert!(!m.tick(20_100, Tick::default()), "released: the delay starts now");
        assert!(m.tick(20_100 + HIDE_DELAY_MS, Tick::default()));
        assert!(m.reveal(Zones::NONE.with(Zone::Sidebar), 30_000));
        assert!(m.reset());
        assert!(!m.reset());
        assert_eq!(m, Machine::default());
    }

    #[test]
    fn another_zone_waits_its_own_delay_and_a_shown_zone_cancels_nothing() {
        let mut m = Machine::default();
        m.enter_sensor(Zone::Top, 0);
        m.tick(REVEAL_DELAY_MS, Tick { pointer_over_bars: true, held: false });
        m.enter_sensor(Zone::Sidebar, 1000);
        assert!(!m.tick(1000 + REVEAL_DELAY_MS - 1, Tick { pointer_over_bars: true, held: false }));
        assert!(m.tick(1000 + REVEAL_DELAY_MS, Tick { pointer_over_bars: true, held: false }));
        assert_eq!(m.shown(), Zones::NONE.with(Zone::Top).with(Zone::Sidebar));
        m.enter_sensor(Zone::Top, 3000); // already shown: only keeps it up
        assert_eq!(m.shown(), Zones::NONE.with(Zone::Top).with(Zone::Sidebar));
    }

    /// The cost of the rules themselves: they run only while a reveal is pending or bars are up.
    /// `cargo test --release --lib -- --ignored auto_hide_cost --nocapture` prints it.
    #[test]
    #[ignore]
    fn auto_hide_cost() {
        let s = AutoHideSettings::default();
        let start = std::time::Instant::now();
        let mut sink = 0u64;
        for i in 0..1_000_000u64 {
            let e = effective(true, &s, Fullscreen::No);
            let p = plan(e.hidden, TabLayout::Vertical);
            let mut m = Machine::default();
            m.enter_sensor(Zone::Top, i);
            m.tick(i + 200, Tick::default());
            sink += u64::from(p.top) + m.shown().0 as u64;
        }
        println!("effective + plan + one reveal: {:?} per round ({sink})", start.elapsed() / 1_000_000);
        let start = std::time::Instant::now();
        for _ in 0..100_000 {
            let mut s = AutoHideSettings::default();
            s.parse_key(KEY_BARS, "tabs,address,bookmarks,panel");
            s.parse_key(KEY_FULLSCREEN, "true");
            sink += s.pairs().len() as u64;
        }
        println!("settings parse + write: {:?} ({sink})", start.elapsed() / 100_000);
    }
}
