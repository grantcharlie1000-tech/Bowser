//! The left rail: a narrow, full-height column of navigation buttons. This module holds the view
//! model the app fills in, and the pure geometry that places, clips, scrolls and hit-tests the
//! buttons. Painting is in `paint.rs` and the glyph outlines are in `rail_icons.rs`.
//!
//! Sections, top to bottom:
//!
//! ```text
//! +--------+
//! | top    |  Home, Customize                      (fixed)
//! |--------|
//! | spaces |  workspaces, then "add workspace"     (scrolls)
//! | apps   |  pinned apps, then "add app"          (scrolls)
//! |--------|
//! | bottom |  Bookmarks, History, Menu             (fixed, anchored to the bottom edge)
//! +--------+
//! ```
//!
//! The top group is clipped above the bottom group, so a short window never draws over it. The
//! scrolling middle is clipped between the two fixed groups.

use std::time::{Duration, Instant};

use crate::{
    chrome::layout::{LEFT_RAIL_WIDTH, Metrics, Rect},
    favicon::FaviconBitmap,
    theme::Rgba,
};

/// Logical padding between the rail's ends and its first and last button.
const PAD: i32 = 4;
/// Logical side of a button: the rail width less a 2-pixel margin either side.
const BUTTON: i32 = LEFT_RAIL_WIDTH - 4;
/// Logical gap between two buttons in the same section.
const BUTTON_GAP: i32 = 3;
/// Logical height reserved for a boundary between two sections. The hairline sits in its middle.
const SECTION_GAP: i32 = 9;
/// Logical inset of a section hairline from the rail's edges.
const RULE_INSET: i32 = 5;

/// Logical corner radius of a button.
pub const BUTTON_RADIUS: i32 = 7;
/// Logical side of the square the outlined icons are drawn in, centred in the button.
pub const ICON: i32 = 16;
/// Logical side of the coloured tile that holds a letter.
pub const TILE: i32 = 16;
/// Logical corner radius of the letter tile.
pub const TILE_RADIUS: i32 = 5;
/// Logical pen width of an outlined icon, which the painter scales by dpi.
pub const STROKE: f32 = 1.5;
/// Logical diameter of the dot shown on a button that needs attention.
pub const DOT: i32 = 5;
/// Logical width and height of the accent bar on the active workspace.
pub const ACTIVE_BAR_WIDTH: i32 = 2;
pub const ACTIVE_BAR_HEIGHT: i32 = 12;

/// Identifies one button. Workspaces and apps are keyed by stable ids, never by their position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RailKey {
    Home,
    Customize,
    Workspace(u64),
    AddWorkspace,
    App(u32),
    AddApp,
    Bookmarks,
    History,
    Menu,
}

/// Workspace glyphs the user can pick from. Each is drawn as an outline in the workspace colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkspaceGlyph {
    Briefcase,
    Home,
    Code,
    Book,
    Cart,
    Game,
    Music,
    Heart,
    Star,
    Bolt,
    Globe,
    Leaf,
}

impl WorkspaceGlyph {
    pub const ALL: [WorkspaceGlyph; 12] = [
        WorkspaceGlyph::Briefcase,
        WorkspaceGlyph::Home,
        WorkspaceGlyph::Code,
        WorkspaceGlyph::Book,
        WorkspaceGlyph::Cart,
        WorkspaceGlyph::Game,
        WorkspaceGlyph::Music,
        WorkspaceGlyph::Heart,
        WorkspaceGlyph::Star,
        WorkspaceGlyph::Bolt,
        WorkspaceGlyph::Globe,
        WorkspaceGlyph::Leaf,
    ];
}

/// What a button shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RailIcon {
    Home,
    Customize,
    Plus,
    /// Bookmarks.
    Heart,
    /// History.
    Clock,
    /// The menu.
    Dots,
    /// The generic tile for an app with no favicon and no usable label.
    AppTile,
    /// The first letter of the label, on a coloured tile.
    Letter,
    Glyph(WorkspaceGlyph),
}

/// One button of the view model. The app builds these; the rail never mutates them.
#[derive(Clone, Debug)]
pub struct RailEntry<'a> {
    pub key: RailKey,
    pub icon: RailIcon,
    /// The workspace or app name. Also the source of the letter tile and the tooltip.
    pub label: String,
    /// The current workspace, or the open panel of its kind.
    pub active: bool,
    /// Shows a small dot in the corner, for example while a tab in the workspace loads.
    pub dot: bool,
    /// The workspace colour. `None` uses the theme accent.
    pub accent: Option<Rgba>,
    /// The app's favicon. When `None`, an app shows its letter tile.
    pub favicon: Option<&'a FaviconBitmap>,
    /// --- tab stacks and workspaces (T10) --- the workspace's tab count, shown as a badge
    /// ("Show tab counts on workspaces"); 0 shows none.
    pub count: u32,
    /// The emoji a workspace shows instead of its glyph ("icon or emoji").
    pub emoji: Option<String>,
}

/// The hover amount of one button that is still fading, in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RailFade {
    pub key: RailKey,
    pub amount: f32,
}

/// Everything the painter needs beyond the chrome view. Pointer hover and press come from
/// `ChromeView::hover` and `ChromeView::pressed`, as `Hit::Rail(key)`.
#[derive(Clone, Copy)]
pub struct RailView<'a> {
    pub entries: &'a [RailEntry<'a>],
    /// Hover amounts that are still animating. A button that is hovered but not listed here is
    /// drawn fully hovered, so a snapped hover needs no entry.
    pub fades: &'a [RailFade],
    /// The button whose panel is open, if any.
    pub open_panel: Option<RailKey>,
    /// Requested scroll of the workspaces and apps, in physical pixels. Geometry clamps it.
    pub scroll: i32,
    /// Insertion line shown while a reorderable item is being dragged.
    pub drop_indicator: Option<RailDropIndicator>,
    /// Item with keyboard focus, if the rail owns keyboard navigation.
    pub focused: Option<RailKey>,
    /// Item whose delayed tooltip is currently visible.
    pub tooltip: Option<RailKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RailDropIndicator {
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Top,
    Workspaces,
    Apps,
    Bottom,
}

fn section(key: RailKey) -> Section {
    match key {
        RailKey::Home | RailKey::Customize => Section::Top,
        RailKey::Workspace(_) | RailKey::AddWorkspace => Section::Workspaces,
        RailKey::App(_) | RailKey::AddApp => Section::Apps,
        RailKey::Bookmarks | RailKey::History | RailKey::Menu => Section::Bottom,
    }
}

/// A placed button. `rect` is where it would be, and `clip` is the part of the rail it may show in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RailItem {
    pub key: RailKey,
    /// Index of the entry this button came from, in the slice given to [`geometry`].
    pub index: usize,
    pub rect: Rect,
    pub clip: Rect,
}

impl RailItem {
    /// The part of the button on screen. Only this part paints and takes clicks.
    pub fn visible(&self) -> Rect {
        self.rect.intersect(self.clip)
    }
}

/// A section hairline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RailRule {
    pub rect: Rect,
    pub clip: Rect,
}

impl RailRule {
    pub fn visible(&self) -> Rect {
        self.rect.intersect(self.clip)
    }
}

/// Placed buttons and hairlines for one rail size, entry list and scroll offset.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RailGeometry {
    pub items: Vec<RailItem>,
    pub rules: Vec<RailRule>,
    /// The clamped scroll offset, in physical pixels.
    pub scroll: i32,
    /// The largest useful scroll offset. Zero when the workspaces and apps fit.
    pub max_scroll: i32,
    /// The scrolling region between the top and bottom groups.
    pub middle: Rect,
}

impl RailGeometry {
    /// The button under a client-relative point, if one is visible there.
    pub fn hit(&self, x: i32, y: i32) -> Option<RailKey> {
        self.items
            .iter()
            .find(|item| item.visible().contains(x, y))
            .map(|item| item.key)
    }

    pub fn item(&self, key: RailKey) -> Option<&RailItem> {
        self.items.iter().find(|item| item.key == key)
    }

    /// The smallest rect covering the visible part of every button in `keys`. Empty when none of
    /// them is on screen. Use it to invalidate a hover change.
    pub fn dirty_for(&self, keys: &[RailKey]) -> Rect {
        self.items
            .iter()
            .filter(|item| keys.contains(&item.key))
            .fold(Rect::default(), |acc, item| acc.union(item.visible()))
    }
}

/// Extra scale of an icon at full hover.
pub const HOVER_LIFT: f32 = 0.06;
/// Scale an icon gives up while its button is pressed, so a press settles at 0.96.
pub const PRESS_SINK: f32 = 0.04;
/// Extra pen weight of an icon at full hover, as a fraction of [`STROKE`].
pub const HOVER_WEIGHT: f32 = 0.13;
/// Height the active bar grows from, as a fraction of its full height.
pub const BAR_FROM: f32 = 0.3;

/// A press lands at once and eases back out, so the click always feels answered.
const PRESS_IN: Duration = Duration::ZERO;
const PRESS_OUT: Duration = Duration::from_millis(140);
/// A button becoming active draws itself on a little slower than it undraws.
const ACTIVE_IN: Duration = Duration::from_millis(180);
const ACTIVE_OUT: Duration = Duration::from_millis(120);

/// How far one button's hover, press and active motion has run, each in `0.0..=1.0`. Hover comes
/// from the chrome's own rail fade, so the pointer has one source of truth; press and active come
/// from [`RailMotion`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RailAmounts {
    pub hover: f32,
    pub press: f32,
    pub active: f32,
}

impl RailAmounts {
    /// Scale of the icon: it lifts on hover and sinks to 0.96 under a press. A press takes over
    /// from the hover lift rather than adding to it, because the pointer is always over a button
    /// it is pressing, and a click must read as smaller than the hovered icon.
    pub fn icon_scale(self) -> f32 {
        (1.0 + HOVER_LIFT * self.hover * (1.0 - self.press)) * (1.0 - PRESS_SINK * self.press)
    }

    /// Multiplier on the icon's pen width, a little heavier while hovered and not pressed.
    pub fn pen_scale(self) -> f32 {
        1.0 + HOVER_WEIGHT * self.hover * (1.0 - self.press)
    }

    /// Multiplier on the active bar's height, growing from [`BAR_FROM`] to full.
    pub fn bar_scale(self) -> f32 {
        BAR_FROM + (1.0 - BAR_FROM) * self.active
    }
}

/// One eased value moving towards a target. Retargeting starts from the value on screen, so an
/// interrupted transition never jumps. Rising values ease out and falling values ease in, so a
/// button arrives gracefully and leaves promptly.
#[derive(Clone, Copy, Debug)]
struct Channel {
    from: f32,
    to: f32,
    started: Instant,
    duration: Duration,
}

impl Channel {
    fn resting(value: f32, now: Instant) -> Self {
        Self {
            from: value,
            to: value,
            started: now,
            duration: Duration::ZERO,
        }
    }

    fn progress(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        let elapsed = now.saturating_duration_since(self.started).as_secs_f32();
        (elapsed / self.duration.as_secs_f32()).clamp(0.0, 1.0)
    }

    fn value(&self, now: Instant) -> f32 {
        if self.from == self.to {
            return self.to;
        }
        let t = self.progress(now);
        let eased = if self.to > self.from {
            1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
        } else {
            t * t
        };
        self.from + (self.to - self.from) * eased
    }

    /// Moves towards `to`. A snap, or a zero duration, lands at once.
    fn retarget(&mut self, to: f32, now: Instant, snap: bool, rise: Duration, fall: Duration) {
        if to == self.to {
            return;
        }
        let duration = if to > self.to { rise } else { fall };
        self.from = if snap || duration.is_zero() {
            to
        } else {
            self.value(now)
        };
        self.to = to;
        self.started = now;
        self.duration = if snap { Duration::ZERO } else { duration };
    }

    fn settle(&mut self) {
        self.from = self.to;
    }

    fn is_active(&self, now: Instant) -> bool {
        self.from != self.to && now.saturating_duration_since(self.started) < self.duration
    }
}

#[derive(Clone, Copy, Debug)]
struct RailTrack {
    key: RailKey,
    press: Channel,
    active: Channel,
}

/// What one animation frame of the rail needs from the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RailFrame {
    /// Repaint the rail. True while a value moves, and once more on the frame that lands it.
    pub repaint: bool,
    /// Another frame is needed. The window's frame timer runs only while this is true.
    pub running: bool,
}

/// Press and active motion for every rail button. Hover is not tracked here: the chrome already
/// fades it, and painting reads that. Values are pure functions of the instant they are sampled at.
/// Tracks are reused, so steady-state frames do not allocate. With `snap` set, as when Windows has
/// client-area animation off, every change lands at once and no frame is ever requested.
#[derive(Debug)]
pub struct RailMotion {
    tracks: Vec<RailTrack>,
    snap: bool,
    /// Whether anything was still moving at the last [`step`](Self::step).
    moving: bool,
}

impl Default for RailMotion {
    fn default() -> Self {
        Self::new(false)
    }
}

impl RailMotion {
    pub fn new(snap: bool) -> Self {
        Self {
            tracks: Vec::with_capacity(32),
            snap,
            moving: false,
        }
    }

    /// Turns animation off or on. Turning it off lands every running value at once. Returns
    /// whether anything was running, so the caller can repaint the settled rail.
    pub fn set_snap(&mut self, snap: bool, now: Instant) -> bool {
        let was_running = self.moving || self.animating(now);
        self.snap = snap;
        if !snap {
            return false;
        }
        for track in &mut self.tracks {
            track.press.settle();
            track.active.settle();
        }
        self.moving = false;
        was_running
    }

    /// Follows the rail's entries: `pressed` is the button under the mouse button, and a button is
    /// active when its entry is, or when its panel is `open`. A button seen for the first time
    /// appears at rest, so nothing draws itself on at start-up. A target that has not changed keeps
    /// its running transition.
    pub fn sync(
        &mut self,
        entries: &[RailEntry<'_>],
        pressed: Option<RailKey>,
        open: Option<RailKey>,
        now: Instant,
    ) {
        self.tracks
            .retain(|track| entries.iter().any(|entry| entry.key == track.key));
        let snap = self.snap;
        for entry in entries {
            let press = flag(pressed == Some(entry.key));
            let active = flag(entry.active || open == Some(entry.key));
            match self.tracks.iter_mut().find(|track| track.key == entry.key) {
                Some(track) => {
                    track.press.retarget(press, now, snap, PRESS_IN, PRESS_OUT);
                    track.active.retarget(active, now, snap, ACTIVE_IN, ACTIVE_OUT);
                }
                None => self.tracks.push(RailTrack {
                    key: entry.key,
                    press: Channel::resting(press, now),
                    active: Channel::resting(active, now),
                }),
            }
        }
    }

    /// The motion of `key` at `now`, with the hover the caller read from the chrome's fades. A
    /// button with no track is at rest.
    pub fn amounts(&self, key: RailKey, hover: f32, now: Instant) -> RailAmounts {
        let (press, active) = self
            .tracks
            .iter()
            .find(|track| track.key == key)
            .map_or((0.0, 0.0), |track| {
                (track.press.value(now), track.active.value(now))
            });
        RailAmounts {
            hover,
            press,
            active,
        }
    }

    /// True while any press or active value is still moving at `now`.
    pub fn animating(&self, now: Instant) -> bool {
        !self.snap
            && self
                .tracks
                .iter()
                .any(|track| track.press.is_active(now) || track.active.is_active(now))
    }

    /// Advances one frame. The frame after the last value stops still repaints, so the settled
    /// pixels land.
    pub fn step(&mut self, now: Instant) -> RailFrame {
        let running = self.animating(now);
        let repaint = running || self.moving;
        self.moving = running;
        RailFrame { repaint, running }
    }
}

fn flag(value: bool) -> f32 {
    if value { 1.0 } else { 0.0 }
}

/// Whether pointer movement has crossed the rail's four-pixel drag threshold.
pub fn drag_started(origin: (i32, i32), current: (i32, i32)) -> bool {
    let dx = i64::from(current.0) - i64::from(origin.0);
    let dy = i64::from(current.1) - i64::from(origin.1);
    dx * dx + dy * dy >= 16
}

/// Steps keyboard focus between rail items, clamped at the ends.
pub fn step_focus(keys: &[RailKey], current: Option<RailKey>, delta: i32) -> Option<RailKey> {
    if keys.is_empty() {
        return None;
    }
    let index = current.and_then(|key| keys.iter().position(|candidate| *candidate == key));
    let index = match (index, delta.signum()) {
        (Some(index), direction) => (index as i64 + i64::from(direction))
            .clamp(0, keys.len() as i64 - 1) as usize,
        (None, direction) if direction < 0 => keys.len() - 1,
        (None, _) => 0,
    };
    keys.get(index).copied()
}

/// Returns a final-list index and insertion line for a valid same-section rail drop.
pub fn drop_indicator(
    geometry: &RailGeometry,
    source: RailKey,
    x: i32,
    y: i32,
) -> Option<(usize, RailDropIndicator)> {
    let wanted = section(source);
    if !matches!(
        source,
        RailKey::Workspace(_) | RailKey::App(_) | RailKey::Bookmarks | RailKey::History
    ) {
        return None;
    }
    let movable = |key| {
        matches!(
            key,
            RailKey::Workspace(_) | RailKey::App(_) | RailKey::Bookmarks | RailKey::History
        )
    };
    let mut items: Vec<_> = geometry
        .items
        .iter()
        .filter(|item| section(item.key) == wanted && movable(item.key))
        .collect();
    items.sort_by_key(|item| item.rect.top);
    let first = *items.first()?;
    let last = *items.last()?;
    if x < first.rect.left - 4
        || x > first.rect.right + 4
        || y < first.rect.top
        || y > last.rect.bottom
    {
        return None;
    }
    items.iter().position(|item| item.key == source)?;
    let remaining: Vec<_> = items
        .into_iter()
        .filter(|item| item.key != source)
        .collect();
    let target = remaining
        .iter()
        .position(|item| y < item.rect.top + item.rect.height() / 2)
        .unwrap_or(remaining.len());
    let line_y = remaining.get(target).map_or_else(
        || remaining.last().map_or(first.rect.top, |item| item.rect.bottom),
        |item| item.rect.top,
    );
    Some((target, RailDropIndicator { y: line_y }))
}

/// The final-list index a workspace or app drop lands on, in the same terms as [`drop_indicator`]:
/// `keys` is the rail in its display order, `source` is the dragged button, and `before` is the
/// button it is dropped in front of (`None` for the end of the section). The index counts the
/// section's movable items without `source`, so it is the index the native drop commits.
///
/// `None` when `source` is not a workspace or app in `keys`, when `before` is not a movable item of
/// the same section (or is `source`), or when the drop would leave `source` where it is.
pub fn reorder_index(keys: &[RailKey], source: RailKey, before: Option<RailKey>) -> Option<usize> {
    let movable = |key: RailKey| {
        matches!(key, RailKey::Workspace(_) | RailKey::App(_) | RailKey::Bookmarks | RailKey::History)
    };
    if !matches!(source, RailKey::Workspace(_) | RailKey::App(_)) {
        return None;
    }
    let wanted = section(source);
    let items: Vec<RailKey> = keys
        .iter()
        .copied()
        .filter(|key| movable(*key) && section(*key) == wanted)
        .collect();
    let from = items.iter().position(|key| *key == source)?;
    let remaining: Vec<RailKey> = items.into_iter().filter(|key| *key != source).collect();
    let target = match before {
        None => remaining.len(),
        Some(key) if key == source => return None,
        Some(key) => remaining.iter().position(|candidate| *candidate == key)?,
    };
    (target != from).then_some(target)
}

/// Places the rail's buttons inside `pill`, which is the rect from `layout::left_rail`.
///
/// `scroll` is clamped to the workspaces-and-apps overflow, so a caller can pass any offset. The
/// bottom group is always placed at the pill's bottom edge, whatever the height.
pub fn geometry(pill: Rect, entries: &[RailEntry<'_>], scroll: i32, dpi: u32) -> RailGeometry {
    let m = Metrics::new(dpi);
    let pad = m.px(PAD);
    let button = m.px(BUTTON);
    let gap = m.px(BUTTON_GAP);
    let section_gap = m.px(SECTION_GAP);
    let rule_inset = m.px(RULE_INSET);
    let column_left = pill.left + (pill.width() - button) / 2;

    let members = |wanted: Section| -> Vec<(usize, RailKey)> {
        entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| section(entry.key) == wanted)
            .map(|(index, entry)| (index, entry.key))
            .collect()
    };
    let top = members(Section::Top);
    let workspaces = members(Section::Workspaces);
    let apps = members(Section::Apps);
    let bottom = members(Section::Bottom);

    let stack_height = |count: usize| -> i32 {
        if count == 0 {
            0
        } else {
            count as i32 * button + (count as i32 - 1) * gap
        }
    };
    let top_h = stack_height(top.len());
    let bottom_h = stack_height(bottom.len());
    let workspaces_h = stack_height(workspaces.len());
    let apps_h = stack_height(apps.len());
    let both_middle = !workspaces.is_empty() && !apps.is_empty();
    let middle_content = workspaces_h + if both_middle { section_gap } else { 0 } + apps_h;
    let has_middle = !workspaces.is_empty() || !apps.is_empty();

    let top_bottom = pill.top + pad + top_h;
    let bottom_top = pill.bottom - pad - bottom_h;
    let middle_top = if top.is_empty() {
        pill.top + pad
    } else {
        top_bottom + section_gap
    };
    let middle_bottom = if bottom.is_empty() {
        pill.bottom - pad
    } else {
        bottom_top - section_gap
    };
    let middle = Rect::new(
        pill.left,
        middle_top,
        pill.right,
        middle_bottom.max(middle_top),
    );
    let upper_bottom = if bottom.is_empty() {
        pill.bottom
    } else {
        bottom_top - section_gap
    };
    let upper = Rect::new(pill.left, pill.top, pill.right, upper_bottom.max(pill.top));

    let max_scroll = (middle_content - middle.height()).max(0);
    let scroll = scroll.clamp(0, max_scroll);

    let mut items = Vec::with_capacity(entries.len());
    let mut rules = Vec::new();
    let rule = |y: i32, clip: Rect| RailRule {
        rect: Rect::new(pill.left + rule_inset, y, pill.right - rule_inset, y + 1),
        clip,
    };

    place_stack(
        &mut items,
        &top,
        column_left,
        pill.top + pad,
        button,
        gap,
        upper,
    );
    if !top.is_empty() && (has_middle || !bottom.is_empty()) {
        rules.push(rule(top_bottom + section_gap / 2, upper));
    }

    let middle_y = middle_top - scroll;
    place_stack(
        &mut items,
        &workspaces,
        column_left,
        middle_y,
        button,
        gap,
        middle,
    );
    let apps_y = middle_y + workspaces_h + if both_middle { section_gap } else { 0 };
    if both_middle {
        rules.push(rule(middle_y + workspaces_h + section_gap / 2, middle));
    }
    place_stack(&mut items, &apps, column_left, apps_y, button, gap, middle);

    place_stack(
        &mut items,
        &bottom,
        column_left,
        bottom_top,
        button,
        gap,
        pill,
    );
    if !bottom.is_empty() && (!top.is_empty() || has_middle) {
        rules.push(rule(bottom_top - section_gap / 2, pill));
    }

    RailGeometry {
        items,
        rules,
        scroll,
        max_scroll,
        middle,
    }
}

fn place_stack(
    items: &mut Vec<RailItem>,
    members: &[(usize, RailKey)],
    left: i32,
    top: i32,
    size: i32,
    gap: i32,
    clip: Rect,
) {
    for (slot, &(index, key)) in members.iter().enumerate() {
        let y = top + slot as i32 * (size + gap);
        items.push(RailItem {
            key,
            index,
            rect: Rect::new(left, y, left + size, y + size),
            clip,
        });
    }
}

/// Moves a scroll offset by `delta` physical pixels (positive scrolls the content up), clamped to
/// `0..=max_scroll`.
pub fn scroll_by(current: i32, delta: i32, max_scroll: i32) -> i32 {
    current.saturating_add(delta).clamp(0, max_scroll.max(0))
}

/// A square of side `size` centred in `outer`.
pub fn centred(outer: Rect, size: i32) -> Rect {
    let left = outer.left + (outer.width() - size) / 2;
    let top = outer.top + (outer.height() - size) / 2;
    Rect::new(left, top, left + size, top + size)
}

/// The text of a button's tooltip. A workspace or app shows its own name.
pub fn rail_tooltip(key: RailKey, entries: &[RailEntry<'_>]) -> String {
    let label = entries
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| entry.label.trim())
        .filter(|label| !label.is_empty());
    match key {
        RailKey::Home => "Home".to_owned(),
        RailKey::Customize => "Customize Bowser".to_owned(),
        RailKey::Workspace(_) => match label {
            Some(name) => format!("Workspace: {name}"),
            None => "Workspace".to_owned(),
        },
        RailKey::AddWorkspace => "New workspace".to_owned(),
        RailKey::App(_) => label.unwrap_or("App").to_owned(),
        RailKey::AddApp => "Add app".to_owned(),
        RailKey::Bookmarks => "Bookmarks".to_owned(),
        RailKey::History => "History".to_owned(),
        RailKey::Menu => "More".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::layout::left_rail;

    const DPIS: [u32; 4] = [96, 120, 144, 192];
    /// Client heights from degenerate to tall, including ones too short for the bottom group.
    const HEIGHTS: [i32; 9] = [0, 1, 12, 40, 120, 240, 420, 700, 1200];

    fn entry(key: RailKey) -> RailEntry<'static> {
        RailEntry {
            key,
            icon: RailIcon::Plus,
            label: format!("{key:?}"),
            active: false,
            dot: false,
            accent: None,
            favicon: None,
            count: 0,
            emoji: None,
        }
    }

    /// Top, two workspaces with their add button, two apps with their add button, bottom group.
    fn sample() -> Vec<RailEntry<'static>> {
        [
            RailKey::Home,
            RailKey::Customize,
            RailKey::Workspace(1),
            RailKey::Workspace(2),
            RailKey::AddWorkspace,
            RailKey::App(1),
            RailKey::App(2),
            RailKey::AddApp,
            RailKey::Bookmarks,
            RailKey::History,
            RailKey::Menu,
        ]
        .into_iter()
        .map(entry)
        .collect()
    }

    /// Many workspaces and apps, which overflow any short window.
    fn crowded() -> Vec<RailEntry<'static>> {
        let mut keys = vec![RailKey::Home, RailKey::Customize];
        keys.extend((0..30).map(RailKey::Workspace));
        keys.push(RailKey::AddWorkspace);
        keys.extend((0..30).map(RailKey::App));
        keys.push(RailKey::AddApp);
        keys.extend([RailKey::Bookmarks, RailKey::History, RailKey::Menu]);
        keys.into_iter().map(entry).collect()
    }

    fn pill(height: i32, dpi: u32) -> Rect {
        left_rail(height, dpi)
    }

    #[test]
    fn every_entry_gets_exactly_one_item_in_order() {
        let entries = sample();
        let geo = geometry(pill(700, 96), &entries, 0, 96);

        assert_eq!(geo.items.len(), entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let item = geo.item(entry.key).expect("every key is placed");
            assert_eq!(item.index, index);
        }
    }

    #[test]
    fn visible_buttons_are_inside_the_rail_and_never_overlap() {
        for dpi in DPIS {
            for height in HEIGHTS {
                for entries in [sample(), crowded()] {
                    let p = pill(height, dpi);
                    let geo = geometry(p, &entries, 37, dpi);

                    for item in &geo.items {
                        let shown = item.visible();
                        if shown.is_empty() {
                            continue;
                        }
                        assert!(
                            p.contains_rect(shown),
                            "{:?} visible {shown:?} escapes the rail {p:?}",
                            item.key
                        );
                    }
                    for (i, a) in geo.items.iter().enumerate() {
                        for b in &geo.items[i + 1..] {
                            assert!(
                                a.visible().intersect(b.visible()).is_empty(),
                                "{:?} overlaps {:?} at dpi {dpi}, height {height}",
                                a.key,
                                b.key
                            );
                        }
                        for rule in &geo.rules {
                            assert!(
                                a.visible().intersect(rule.visible()).is_empty(),
                                "{:?} is crossed by a section rule at dpi {dpi}, height {height}",
                                a.key
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn bottom_group_is_anchored_to_the_bottom_edge_at_every_height_and_scroll() {
        let m = Metrics::new(144);
        for height in HEIGHTS {
            let p = pill(height, 144);
            let expected_last_bottom = p.bottom - m.px(PAD);
            for scroll in [0, 9, 10_000] {
                let geo = geometry(p, &sample(), scroll, 144);
                let last = geo.item(RailKey::Menu).expect("menu is placed");
                assert_eq!(
                    last.rect.bottom, expected_last_bottom,
                    "height {height}, scroll {scroll}"
                );
                let bottom_first = geo.item(RailKey::Bookmarks).expect("bookmarks is placed");
                let unscrolled = geometry(p, &sample(), 0, 144);
                assert_eq!(
                    bottom_first.rect,
                    unscrolled.item(RailKey::Bookmarks).expect("placed").rect,
                    "scrolling never moves the bottom group"
                );
            }
        }
    }

    #[test]
    fn a_short_window_clips_the_top_group_above_the_bottom_group() {
        for dpi in DPIS {
            for height in HEIGHTS {
                let p = pill(height, dpi);
                let geo = geometry(p, &sample(), 0, dpi);
                let bottom = geo.item(RailKey::Bookmarks).expect("placed").rect.top;
                for key in [RailKey::Home, RailKey::Customize] {
                    let shown = geo.item(key).expect("placed").visible();
                    assert!(
                        shown.is_empty() || shown.bottom <= bottom,
                        "{key:?} shows below the bottom group's top {bottom} at height {height}"
                    );
                }
            }
        }
    }

    #[test]
    fn scroll_clamps_to_the_overflow_and_moves_the_middle_exactly() {
        let p = pill(300, 96);
        let entries = crowded();
        let at_rest = geometry(p, &entries, 0, 96);
        assert!(at_rest.max_scroll > 0, "30 workspaces overflow 300 px");

        let below = geometry(p, &entries, -500, 96);
        assert_eq!(below.scroll, 0, "negative scroll clamps to the top");

        let past_end = geometry(p, &entries, 1_000_000, 96);
        assert_eq!(
            past_end.scroll, past_end.max_scroll,
            "overscroll clamps to the bottom"
        );

        let step = 40;
        let scrolled = geometry(p, &entries, step, 96);
        let before = at_rest.item(RailKey::Workspace(3)).expect("placed").rect;
        let after = scrolled.item(RailKey::Workspace(3)).expect("placed").rect;
        assert_eq!(
            before.top - after.top,
            step,
            "content moves by exactly the offset"
        );
        assert_eq!(
            scrolled.middle, at_rest.middle,
            "the scrolling window itself stays put"
        );

        let fits = geometry(pill(6000, 96), &entries, 500, 96);
        assert_eq!(fits.max_scroll, 0, "a tall rail has nothing to scroll");
        assert_eq!(fits.scroll, 0);
    }

    #[test]
    fn scroll_by_stays_in_range_and_tolerates_an_empty_overflow() {
        assert_eq!(scroll_by(0, -30, 100), 0);
        assert_eq!(scroll_by(90, 30, 100), 100);
        assert_eq!(scroll_by(50, 25, 100), 75);
        assert_eq!(scroll_by(10, 25, -4), 0, "no overflow means no scrolling");
    }

    #[test]
    fn hit_test_maps_each_button_centre_to_its_key_at_every_dpi() {
        for dpi in DPIS {
            let p = pill(700, dpi);
            let entries = sample();
            let geo = geometry(p, &entries, 0, dpi);
            for item in geo.items.iter().filter(|item| !item.visible().is_empty()) {
                let centre = item.visible();
                let (x, y) = (
                    centre.left + centre.width() / 2,
                    centre.top + centre.height() / 2,
                );
                assert_eq!(
                    geo.hit(x, y),
                    Some(item.key),
                    "centre of {:?} at dpi {dpi}",
                    item.key
                );
            }
            let gutter = (p.left + 1, p.top + 40);
            assert_eq!(
                geo.hit(gutter.0, gutter.1),
                None,
                "the pill's edge is not a button"
            );
            assert_eq!(
                geo.hit(p.left + p.width() / 2, p.top + 2),
                None,
                "space above the first button"
            );
        }
    }

    #[test]
    fn hit_test_follows_the_scrolled_buttons_and_ignores_clipped_ones() {
        let p = pill(300, 96);
        let entries = crowded();
        let geo = geometry(p, &entries, 60, 96);
        let shown = geo.item(RailKey::Workspace(2)).expect("placed").visible();
        assert!(!shown.is_empty(), "the second workspace is in view");
        assert_eq!(
            geo.hit(shown.left + 2, shown.top + 2),
            Some(RailKey::Workspace(2))
        );

        // A clipped button can sit under a fixed group, but it must never take that group's hits.
        for item in geo.items.iter().filter(|item| item.visible().is_empty()) {
            let r = item.rect;
            assert_ne!(
                geo.hit(r.left + 2, r.top + 2),
                Some(item.key),
                "{:?} is clipped away",
                item.key
            );
        }
    }

    #[test]
    fn tiny_rails_place_nothing_visible_and_never_panic() {
        for dpi in DPIS {
            for height in [0, 1, 5, 10] {
                for width in [0, 1, 2, 3] {
                    let p = Rect::new(0, 0, width, height);
                    let geo = geometry(p, &crowded(), 99, dpi);
                    assert!(geo.max_scroll >= 0);
                    for item in &geo.items {
                        assert!(
                            item.visible().is_empty() || p.contains_rect(item.visible()),
                            "{:?} escapes a {width}x{height} rail",
                            item.key
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn buttons_scale_with_dpi() {
        let m = Metrics::new(192);
        let geo = geometry(pill(700, 192), &sample(), 0, 192);
        let home = geo.item(RailKey::Home).expect("placed").rect;
        assert_eq!(home.width(), m.px(BUTTON));
        assert_eq!(home.height(), m.px(BUTTON));
    }

    #[test]
    fn dirty_rect_covers_only_the_requested_buttons() {
        let geo = geometry(pill(700, 96), &sample(), 0, 96);
        let one = geo.dirty_for(&[RailKey::History]);
        assert_eq!(one, geo.item(RailKey::History).expect("placed").visible());

        let two = geo.dirty_for(&[RailKey::Home, RailKey::Menu]);
        let home = geo.item(RailKey::Home).expect("placed").visible();
        let menu = geo.item(RailKey::Menu).expect("placed").visible();
        assert!(two.contains_rect(home) && two.contains_rect(menu));
        assert!(
            geo.dirty_for(&[RailKey::App(99)]).is_empty(),
            "unknown keys dirty nothing"
        );
    }

    #[test]
    fn tooltips_name_each_button_and_use_entry_labels() {
        let entries = [RailEntry {
            label: "  Work  ".to_owned(),
            ..entry(RailKey::Workspace(7))
        }];
        assert_eq!(
            rail_tooltip(RailKey::Workspace(7), &entries),
            "Workspace: Work"
        );
        assert_eq!(rail_tooltip(RailKey::Workspace(8), &entries), "Workspace");
        assert_eq!(rail_tooltip(RailKey::Home, &[]), "Home");
        assert_eq!(rail_tooltip(RailKey::Customize, &[]), "Customize Bowser");
        assert_eq!(rail_tooltip(RailKey::AddWorkspace, &[]), "New workspace");
        assert_eq!(rail_tooltip(RailKey::AddApp, &[]), "Add app");
        assert_eq!(rail_tooltip(RailKey::Bookmarks, &[]), "Bookmarks");
        assert_eq!(rail_tooltip(RailKey::History, &[]), "History");
        assert_eq!(rail_tooltip(RailKey::Menu, &[]), "More");
        assert_eq!(rail_tooltip(RailKey::App(3), &[]), "App");
        let named = [RailEntry {
            label: "Mail".to_owned(),
            ..entry(RailKey::App(3))
        }];
        assert_eq!(rail_tooltip(RailKey::App(3), &named), "Mail");
    }

    #[test]
    fn keyboard_focus_steps_between_items_and_clamps_at_the_ends() {
        let keys = [RailKey::Home, RailKey::Workspace(1), RailKey::Bookmarks];
        assert_eq!(step_focus(&keys, None, 1), Some(RailKey::Home));
        assert_eq!(step_focus(&keys, Some(RailKey::Home), -1), Some(RailKey::Home));
        assert_eq!(step_focus(&keys, Some(RailKey::Workspace(1)), 1), Some(RailKey::Bookmarks));
        assert_eq!(step_focus(&keys, None, -1), Some(RailKey::Bookmarks));
        assert_eq!(step_focus(&[], None, 1), None);
    }

    #[test]
    fn rail_drag_requires_four_pixels_of_pointer_motion() {
        assert!(!drag_started((10, 10), (12, 12)));
        assert!(drag_started((10, 10), (14, 10)));
        assert!(drag_started((10, 10), (10, 14)));
    }

    #[test]
    fn rail_drop_index_stays_inside_the_source_section() {
        let entries = sample();
        let geo = geometry(pill(700, 96), &entries, 0, 96);
        let first = geo.item(RailKey::Workspace(1)).unwrap().rect;
        let second = geo.item(RailKey::Workspace(2)).unwrap().rect;
        assert_eq!(
            drop_indicator(&geo, RailKey::Workspace(1), first.left, second.top + second.height() / 2)
                .map(|(index, _)| index),
            Some(1)
        );
        let app = geo.item(RailKey::App(1)).unwrap().rect;
        assert_eq!(drop_indicator(&geo, RailKey::Workspace(1), first.left, app.top + 2), None);
        assert_eq!(drop_indicator(&geo, RailKey::Workspace(1), first.left - 20, first.top), None);
    }

    const FRAME: Duration = Duration::from_millis(16);

    fn after(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    /// A rail motion that has seen `key` at rest, so the next `sync` is a real change.
    fn resting_motion(snap: bool, key: RailKey, start: Instant) -> (RailMotion, Vec<RailEntry<'static>>) {
        let entries = vec![entry(key)];
        let mut motion = RailMotion::new(snap);
        motion.sync(&entries, None, None, start);
        (motion, entries)
    }

    #[test]
    fn a_new_button_appears_at_rest_even_when_active() {
        let start = Instant::now();
        let mut active = entry(RailKey::Home);
        active.active = true;
        let mut motion = RailMotion::new(false);
        motion.sync(&[active], Some(RailKey::Home), None, start);
        let amounts = motion.amounts(RailKey::Home, 0.0, start);
        assert_eq!((amounts.press, amounts.active), (1.0, 1.0));
        assert!(!motion.animating(start), "nothing draws itself on at start-up");
    }

    #[test]
    fn press_lands_instantly_and_eases_back_out() {
        let start = Instant::now();
        let key = RailKey::History;
        let (mut motion, entries) = resting_motion(false, key, start);

        motion.sync(&entries, Some(key), None, start);
        assert_eq!(motion.amounts(key, 0.0, start).press, 1.0, "scale 0.96 on the very frame");
        assert!(!motion.animating(start), "an instant press needs no frames");
        assert!((motion.amounts(key, 0.0, start).icon_scale() - 0.96).abs() < 1e-6);

        let released = after(start, 100);
        motion.sync(&entries, None, None, released);
        assert_eq!(motion.amounts(key, 0.0, released).press, 1.0, "release starts from the press");
        assert!(motion.animating(released));
        let mid = motion.amounts(key, 0.0, after(released, 70)).press;
        assert!(mid > 0.0 && mid < 1.0, "release is mid-flight at 70 ms, got {mid}");
        assert_eq!(motion.amounts(key, 0.0, after(released, 140)).press, 0.0);
        assert!(!motion.animating(after(released, 140)), "settled");
    }

    #[test]
    fn active_eases_to_its_target_then_settles() {
        let start = Instant::now();
        let key = RailKey::Bookmarks;
        let (mut motion, mut entries) = resting_motion(false, key, start);
        entries[0].active = true;
        motion.sync(&entries, None, None, start);

        let mut previous = 0.0;
        let mut ms = 0;
        while ms <= 180 {
            let value = motion.amounts(key, 0.0, after(start, ms)).active;
            assert!(value >= previous, "monotonic rise at {ms} ms: {value} < {previous}");
            previous = value;
            ms += 16;
        }
        assert_eq!(motion.amounts(key, 0.0, after(start, 180)).active, 1.0, "reaches the target");
        assert!(motion.animating(after(start, 100)));
        assert!(!motion.animating(after(start, 180)), "settles at the end of the duration");
        // Ease-out: well past half of the way by half of the time.
        assert!(motion.amounts(key, 0.0, after(start, 90)).active > 0.7);
    }

    #[test]
    fn an_open_panel_makes_its_button_active() {
        let start = Instant::now();
        let key = RailKey::History;
        let (mut motion, entries) = resting_motion(false, key, start);
        motion.sync(&entries, None, Some(key), start);
        assert!(motion.animating(start));
        assert_eq!(motion.amounts(key, 0.0, after(start, 400)).active, 1.0);
    }

    #[test]
    fn retargeting_mid_flight_starts_from_the_value_on_screen() {
        let start = Instant::now();
        let key = RailKey::Home;
        let (mut motion, mut entries) = resting_motion(false, key, start);
        entries[0].active = true;
        motion.sync(&entries, None, None, start);
        let turn = after(start, 60);
        let before = motion.amounts(key, 0.0, turn).active;
        entries[0].active = false;
        motion.sync(&entries, None, None, turn);
        let after_turn = motion.amounts(key, 0.0, turn).active;
        assert!((before - after_turn).abs() < 1e-6, "no jump when the target flips");
        assert!(motion.amounts(key, 0.0, after(turn, 40)).active < before, "and it falls from there");
        assert_eq!(motion.amounts(key, 0.0, after(turn, 120)).active, 0.0);
    }

    #[test]
    fn a_repeated_target_keeps_the_running_transition() {
        let start = Instant::now();
        let key = RailKey::Home;
        let (mut motion, mut entries) = resting_motion(false, key, start);
        entries[0].active = true;
        motion.sync(&entries, None, None, start);
        let early = motion.amounts(key, 0.0, after(start, 90)).active;
        motion.sync(&entries, None, None, after(start, 80));
        assert!(
            (motion.amounts(key, 0.0, after(start, 90)).active - early).abs() < 1e-6,
            "syncing the same target again does not restart the ease"
        );
    }

    #[test]
    fn reduced_motion_snaps_every_change_and_never_asks_for_frames() {
        let start = Instant::now();
        let key = RailKey::Home;
        let (mut motion, mut entries) = resting_motion(true, key, start);
        entries[0].active = true;
        motion.sync(&entries, Some(key), None, start);
        let now = motion.amounts(key, 0.0, start);
        assert_eq!((now.press, now.active), (1.0, 1.0), "end states land at once");
        assert!(!motion.animating(start));
        let frame = motion.step(start);
        assert_eq!(frame, RailFrame { repaint: false, running: false });

        entries[0].active = false;
        motion.sync(&entries, None, None, after(start, 10));
        let released = motion.amounts(key, 0.0, after(start, 10));
        assert_eq!((released.press, released.active), (0.0, 0.0));
    }

    #[test]
    fn turning_reduced_motion_on_lands_a_running_transition() {
        let start = Instant::now();
        let key = RailKey::Home;
        let (mut motion, mut entries) = resting_motion(false, key, start);
        entries[0].active = true;
        motion.sync(&entries, None, None, start);
        let mid = after(start, 50);
        assert!(motion.animating(mid));
        assert!(motion.set_snap(true, mid), "something was running");
        assert_eq!(motion.amounts(key, 0.0, mid).active, 1.0);
        assert!(!motion.animating(mid));
        assert!(!motion.set_snap(false, mid), "turning animation back on moves nothing");
    }

    #[test]
    fn the_frame_that_lands_a_transition_still_repaints_then_frames_stop() {
        let start = Instant::now();
        let key = RailKey::Home;
        let (mut motion, mut entries) = resting_motion(false, key, start);
        entries[0].active = true;
        motion.sync(&entries, None, None, start);

        let mut frames = 0;
        let mut now = start;
        loop {
            now += FRAME;
            let frame = motion.step(now);
            if !frame.running {
                assert!(frame.repaint, "the landing frame repaints the settled pixels");
                break;
            }
            frames += 1;
            assert!(frames < 40, "the frame timer must stop");
        }
        assert_eq!(
            motion.step(now + FRAME),
            RailFrame { repaint: false, running: false },
            "idle rails ask for nothing"
        );
    }

    #[test]
    fn steady_state_syncs_do_not_grow_the_track_storage() {
        let start = Instant::now();
        let entries = sample();
        let mut motion = RailMotion::new(false);
        motion.sync(&entries, None, None, start);
        let capacity = motion.tracks.capacity();
        for frame in 0..200 {
            let pressed = [None, Some(RailKey::Home), Some(RailKey::Menu)][frame % 3];
            motion.sync(&entries, pressed, None, after(start, frame as u64 * 16));
        }
        assert_eq!(motion.tracks.capacity(), capacity, "reused, not reallocated");
        assert_eq!(motion.tracks.len(), entries.len());
    }

    #[test]
    fn tracks_follow_the_entry_list() {
        let start = Instant::now();
        let mut entries = sample();
        let mut motion = RailMotion::new(false);
        motion.sync(&entries, None, None, start);
        let removed = entries.remove(2).key;
        motion.sync(&entries, None, None, start);
        assert_eq!(motion.tracks.len(), entries.len());
        assert_eq!(
            motion.amounts(removed, 0.4, start),
            RailAmounts { hover: 0.4, press: 0.0, active: 0.0 },
            "an unknown button is at rest and keeps the hover it was handed"
        );
    }

    #[test]
    fn icon_pose_lifts_on_hover_sinks_on_press_and_bar_grows_with_active() {
        let rest = RailAmounts::default();
        assert_eq!(rest.icon_scale(), 1.0);
        assert_eq!(rest.pen_scale(), 1.0);
        let hover = RailAmounts { hover: 1.0, ..rest };
        assert!(hover.icon_scale() > 1.0 && hover.pen_scale() > 1.0);
        let press = RailAmounts { press: 1.0, ..rest };
        assert!((press.icon_scale() - 0.96).abs() < 1e-6);
        let both = RailAmounts { hover: 1.0, press: 1.0, ..rest };
        assert!(
            (both.icon_scale() - 0.96).abs() < 1e-6 && both.pen_scale() == 1.0,
            "a press on a hovered button lands on 0.96, not on the hover lift"
        );
        assert!(both.icon_scale() < hover.icon_scale());
        assert_eq!(rest.bar_scale(), BAR_FROM);
        assert_eq!(RailAmounts { active: 1.0, ..rest }.bar_scale(), 1.0);
    }

    #[test]
    fn centred_squares_sit_in_the_middle_of_their_container() {
        let outer = Rect::new(10, 20, 40, 60);
        assert_eq!(centred(outer, 10), Rect::new(20, 35, 30, 45));
        assert_eq!(centred(outer, 0).width(), 0);
    }

    #[test]
    fn reorder_index_counts_the_section_without_the_source() {
        let keys = [
            RailKey::Home,
            RailKey::Customize,
            RailKey::Workspace(1),
            RailKey::Workspace(2),
            RailKey::Workspace(3),
            RailKey::AddWorkspace,
            RailKey::App(10),
            RailKey::App(11),
            RailKey::AddApp,
            RailKey::Bookmarks,
            RailKey::History,
            RailKey::Menu,
        ];
        // Ws 1 before ws 3: remaining is [2, 3], so it lands at 0.
        assert_eq!(reorder_index(&keys, RailKey::Workspace(1), Some(RailKey::Workspace(3))), Some(1));
        // Ws 3 before ws 1: remaining is [1, 2], so it lands at 0.
        assert_eq!(reorder_index(&keys, RailKey::Workspace(3), Some(RailKey::Workspace(1))), Some(0));
        // End of the section.
        assert_eq!(reorder_index(&keys, RailKey::Workspace(1), None), Some(2));
        // Apps are their own section.
        assert_eq!(reorder_index(&keys, RailKey::App(11), Some(RailKey::App(10))), Some(0));
        assert_eq!(reorder_index(&keys, RailKey::App(10), None), Some(1));
    }

    #[test]
    fn reorder_index_refuses_no_ops_and_cross_section_targets() {
        let keys = [
            RailKey::Workspace(1),
            RailKey::Workspace(2),
            RailKey::AddWorkspace,
            RailKey::App(10),
            RailKey::AddApp,
            RailKey::Bookmarks,
        ];
        // Already in front of its neighbour, or already last.
        assert_eq!(reorder_index(&keys, RailKey::Workspace(1), Some(RailKey::Workspace(2))), None);
        assert_eq!(reorder_index(&keys, RailKey::Workspace(2), None), None);
        // Dropped onto itself.
        assert_eq!(reorder_index(&keys, RailKey::Workspace(1), Some(RailKey::Workspace(1))), None);
        // Targets in another section, the add buttons, and fixed buttons are not drop targets.
        assert_eq!(reorder_index(&keys, RailKey::Workspace(1), Some(RailKey::App(10))), None);
        assert_eq!(reorder_index(&keys, RailKey::Workspace(1), Some(RailKey::AddWorkspace)), None);
        assert_eq!(reorder_index(&keys, RailKey::Workspace(1), Some(RailKey::Bookmarks)), None);
        // Unknown source, and sources that never move through this path.
        assert_eq!(reorder_index(&keys, RailKey::Workspace(9), None), None);
        assert_eq!(reorder_index(&keys, RailKey::Bookmarks, None), None);
        assert_eq!(reorder_index(&keys, RailKey::Home, None), None);
        // An unknown target.
        assert_eq!(reorder_index(&keys, RailKey::Workspace(1), Some(RailKey::Workspace(7))), None);
    }
}
