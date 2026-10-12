//! Pure chrome geometry and hit-testing (glass UI work in progress; see docs/superpowers/specs/2026-10-09-bowser-glass-ui-design.md)
//!
//! Coordinates are client-relative physical pixels, top to bottom:
//!
//! ```text
//! +-------------------------------------------------------------+
//! | toolbar: navigation, omnibox, main menu, tab list, "+", captions |
//! | tab row: tabs, full width                                   |
//! +-------------------------------------------------------------+
//! | 2 px row gap                                                |
//! | back, forward, reload, omnibox (28, to 2 px from the edge)  |
//! | 2 px margin                                                 |
//! +-------------------------------------------------------------+
//! | viewport (page content, to the bottom edge)                 |
//! ```
//!
//! Tab overflow: tabs shrink evenly to favicon circles. The visible window onto the row shifts just
//! enough to keep the active tab visible. The tab list and "+" buttons live in the toolbar, left of
//! the window captions, so they never scroll with the tabs.

use windows::Win32::Foundation::RECT;

use crate::chrome::rail::{self, RailKey, RailView};

/// Logical (96 dpi) chrome metrics, scaled to physical pixels with [`Metrics::px`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metrics {
    dpi: u32,
}

impl Metrics {
    /// Height of the toolbar row, which is also the caption button height.
    pub const STRIP_HEIGHT: i32 = 34;
    /// Height of the full-width tab row under the toolbar.
    pub const TAB_ROW_HEIGHT: i32 = 28;
    pub const ISLAND_HEIGHT: i32 = 24;
    pub const TAB_RADIUS: i32 = 9;
    /// The omnibox stays this wide at most and is centred in the toolbar.
    pub const OMNIBOX_MAX_WIDTH: i32 = 624;
    /// Space above and below an island inside the strip.
    pub const ISLAND_MARGIN: i32 = 2;
    /// Gap between neighbouring islands and the pinned tab controls.
    pub const ISLAND_GAP: i32 = 3;
    pub const TAB_MAX_WIDTH: i32 = 100_000;
    pub const TAB_MIN_WIDTH: i32 = 26;
    /// Titles are hidden in tabs narrower than this.
    pub const TITLE_MIN_TAB_WIDTH: i32 = 120;
    /// Diameter of the pill slot used when a collapsed group has no selected member.
    pub const GROUP_CAPSULE_WIDTH: i32 = Self::ISLAND_HEIGHT;
    /// Width reserved for a leading ball plus the shifted close and favicon affordances.
    pub const GROUP_BALL_SLOT_WIDTH: i32 = 72;
    /// Minimum width reserved for a collapsed selected group's capped fan and tab controls.
    pub const GROUP_STACK_SLOT_WIDTH: i32 = 96;
    pub const NEW_TAB_SIZE: i32 = 28;
    pub const TAB_MENU_SIZE: i32 = 26;
    pub const CAPTION_BUTTON_WIDTH: i32 = 46;
    pub const OMNIBOX_HEIGHT: i32 = 26;
    /// Inset of the omnibox text from the right edge of the field.
    pub const OMNIBOX_TEXT_INSET: i32 = 12;
    /// Inset of the omnibox text from the left edge of the field, clear of the search icon.
    pub const OMNIBOX_TEXT_LEFT: i32 = 32;
    /// Left inset and square size of the connection (site information) button inside the field.
    pub const SITE_BUTTON_INSET: i32 = 3;
    pub const SITE_BUTTON_SIZE: i32 = 24;
    pub const SPLIT_BUTTON_INSET: i32 = 2;
    pub const SPLIT_BUTTON_SIZE: i32 = 22;
    /// Square back / forward / reload buttons at the left of the omnibox row.
    pub const NAV_BUTTON_SIZE: i32 = 28;
    pub const NAV_BUTTON_GAP: i32 = 3;
    /// Inset of the first navigation button from the window edge.
    pub const NAV_LEFT_INSET: i32 = 4;
    /// Width of the main-menu bubble, which sits right of the omnibox at the omnibox's height.
    pub const MENU_BUTTON_SIZE: i32 = 28;
    /// Inset of the tabs and omnibox from the window edges, and the margin below the omnibox.
    pub const OUTER_MARGIN: i32 = 2;
    pub const ROW_GAP: i32 = 2;
    /// Width of the translucent rim around the page on the left, right and bottom of a window that
    /// is not maximised. The page is a child window and wins every hit test over its own pixels, so
    /// without the rim there would be nowhere left to grab those three resize edges.
    pub const RESIZE_RIM: i32 = 4;
    pub const ISLAND_RADIUS: i32 = 13;
    pub const OMNIBOX_RADIUS: i32 = 13;
    pub const FAVICON_SIZE: i32 = 16;
    pub const CLOSE_SIZE: i32 = 18;
    /// Inset from the island's left edge to the favicon, and the gap before the title.
    pub const TAB_PADDING: i32 = 8;
    pub const TEXT_GAP: i32 = 6;
    /// Inset from the island's left edge to the hover close button.
    pub const CLOSE_INSET: i32 = 4;

    /// A dpi of 0 (unknown) is treated as 96.
    pub const fn new(dpi: u32) -> Self {
        Self {
            dpi: if dpi == 0 { 96 } else { dpi },
        }
    }

    pub const fn dpi(self) -> u32 {
        self.dpi
    }

    /// Scales a logical length to physical pixels, rounding halves up.
    pub const fn px(self, logical: i32) -> i32 {
        (logical * self.dpi as i32 + 48) / 96
    }

    pub const fn island_radius(self) -> i32 {
        self.px(Self::ISLAND_RADIUS)
    }

    pub const fn tab_radius(self) -> i32 {
        self.px(Self::TAB_RADIUS)
    }

    pub const fn omnibox_radius(self) -> i32 {
        self.px(Self::OMNIBOX_RADIUS)
    }

    pub const fn window_radius(self) -> i32 {
        self.px(Self::ISLAND_RADIUS)
    }
}

/// Axis-aligned rectangle with half-open bounds: `left` and `top` are inside, `right` and
/// `bottom` are outside.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn width(self) -> i32 {
        (self.right - self.left).max(0)
    }

    pub fn height(self) -> i32 {
        (self.bottom - self.top).max(0)
    }

    pub fn is_empty(self) -> bool {
        self.width() == 0 || self.height() == 0
    }

    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    /// Whether `other` lies entirely inside `self`. An empty `other` is contained only if its
    /// origin is inside.
    pub fn contains_rect(self, other: Rect) -> bool {
        other.left >= self.left
            && other.top >= self.top
            && other.right <= self.right
            && other.bottom <= self.bottom
    }

    pub fn intersect(self, other: Rect) -> Rect {
        Rect::new(
            self.left.max(other.left),
            self.top.max(other.top),
            self.right.min(other.right),
            self.bottom.min(other.bottom),
        )
    }

    /// The same rect moved `dx` pixels to the right.
    pub fn shifted_right(self, dx: i32) -> Rect {
        Rect::new(self.left + dx, self.top, self.right + dx, self.bottom)
    }

    /// The smallest rect covering both. An empty rect contributes nothing.
    pub fn union(self, other: Rect) -> Rect {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Rect::new(
            self.left.min(other.left),
            self.top.min(other.top),
            self.right.max(other.right),
            self.bottom.max(other.bottom),
        )
    }
}

impl From<RECT> for Rect {
    fn from(rect: RECT) -> Self {
        Rect::new(rect.left, rect.top, rect.right, rect.bottom)
    }
}

/// Sub-rects of one tab island. `title` has zero width when titles are hidden.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TabParts {
    pub favicon: Rect,
    pub title: Rect,
    pub close: Rect,
    /// Narrow active tabs replace their favicon with the close affordance.
    pub active_close: bool,
}

/// Chrome geometry for one window size. Build with [`compute`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layout {
    pub metrics: Metrics,
    pub maximised: bool,
    pub client: Rect,
    /// Toolbar row: navigation, omnibox, menus, "+" and caption buttons.
    pub strip: Rect,
    /// Full-width row of tabs under the toolbar.
    pub tab_row: Rect,
    /// Horizontal span that tabs may occupy. Tabs are clipped to it, and anything past it is hidden.
    pub tab_zone: Rect,
    /// Opens the tab list. Sits in the toolbar, left of "+".
    pub tab_menu_button: Rect,
    /// Opens a new tab. Sits in the toolbar, left of the caption buttons.
    pub new_tab_button: Rect,
    /// Reserved for the min / max / close buttons, at the right end of the strip.
    pub caption_area: Rect,
    pub back_button: Rect,
    pub forward_button: Rect,
    pub reload_button: Rect,
    pub omnibox: Rect,
    /// Opens the main menu (profiles, settings). A bubble at the omnibox's height, directly right of
    /// the omnibox. Empty when the toolbar is too narrow for it.
    pub main_menu_button: Rect,
    pub viewport: Rect,
    /// Full-height pill at the left edge of the client. Everything else starts to its right.
    pub rail: Rect,
    tab_count: usize,
    inactive_tab_width: i32,
    active_tab_width: i32,
    active_index: usize,
    tab_left: i32,
    first_visible: usize,
    visible_count: usize,
    island_top: i32,
    island_bottom: i32,
    /// Bit per slot index: a capsule represents a collapsed group with no selected member.
    pill_group_slots: [u64; GROUP_SLOT_WORDS],
    /// Bit per slot index: the expanded group leader has a leading ball.
    ball_group_slots: [u64; GROUP_SLOT_WORDS],
    /// Bit per slot index: the slot reserves space for a collapsed selected group's ball fan.
    stack_group_slots: [u64; GROUP_SLOT_WORDS],
}

const GROUP_SLOT_WORDS: usize = 8;

/// Height of the painted chrome: the strip, the omnibox row and the margin below it. This is where
/// `Layout::viewport` starts.
pub const fn chrome_height(dpi: u32) -> i32 {
    let m = Metrics::new(dpi);
    m.px(Metrics::STRIP_HEIGHT) + m.px(Metrics::TAB_ROW_HEIGHT) + m.px(Metrics::OUTER_MARGIN)
}

/// Logical width of the left rail. The rail is a pill, rounded at both ends. It holds 30-pixel
/// buttons with a 5-pixel margin either side, which fits the 18-pixel icons at 96 dpi.
pub const LEFT_RAIL_WIDTH: i32 = 40;

/// Physical pixels the left rail takes from the window edge, including the gaps either side of it.
/// Everything else in the window, the chrome and the page viewport, starts at this x.
pub fn left_rail_inset(dpi: u32) -> i32 {
    let m = Metrics::new(dpi);
    m.px(Metrics::OUTER_MARGIN) + m.px(LEFT_RAIL_WIDTH) + m.px(Metrics::OUTER_MARGIN)
}

/// The left rail for a client of `client_h` physical pixels: full height, inset by the outer margin.
pub fn left_rail(client_h: i32, dpi: u32) -> Rect {
    let m = Metrics::new(dpi);
    let margin = m.px(Metrics::OUTER_MARGIN);
    let height = client_h.max(0);
    Rect::new(
        margin,
        margin.min(height),
        margin + m.px(LEFT_RAIL_WIDTH),
        (height - margin).max(margin.min(height)),
    )
}

/// Computes the chrome for a window that has the left rail. The chrome is laid out in the space to
/// the right of the rail and then moved there, so no toolbar, tab or viewport rect overlaps it.
pub fn compute_in_frame(
    client_w: i32,
    client_h: i32,
    dpi: u32,
    tab_count: usize,
    active_index: usize,
    maximised: bool,
) -> Layout {
    let client_w = client_w.max(0);
    let client_h = client_h.max(0);
    let inset = left_rail_inset(dpi);
    let mut layout = compute_with_active(
        client_w - inset,
        client_h,
        dpi,
        tab_count,
        active_index,
        maximised,
    )
    .shifted_right(inset);
    layout.client = Rect::new(0, 0, client_w, client_h);
    layout.rail = left_rail(client_h, dpi);
    layout
}

/// Rounded page host bounds in client-relative physical pixels. The margins preserve the native
/// resize rim while keeping the content inset aligned to the device-pixel chrome metrics.
pub fn content_rect(client_w: i32, client_h: i32, dpi: u32, maximised: bool) -> Rect {
    let m = Metrics::new(dpi);
    let width = client_w.max(0);
    let height = client_h.max(0);
    let margin = m.px(Metrics::OUTER_MARGIN);
    let top = (m.px(Metrics::STRIP_HEIGHT + Metrics::TAB_ROW_HEIGHT) + margin).min(height);
    let rim = if maximised {
        0
    } else {
        m.px(Metrics::RESIZE_RIM).min(width / 2)
    };
    Rect::new(rim, top, (width - rim).max(rim), (height - rim).max(top))
}

/// The three adjacent rectangles used by a split view. `divider_left` is clamped so both panes
/// retain at least `min_pane_width` pixels. A too-narrow host collapses the split by returning
/// `None`; callers can hide one pane and restore a single full-width view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitPaneLayout {
    pub left: Rect,
    pub divider: Rect,
    pub right: Rect,
}

pub fn split_pane_layout(
    content: Rect,
    divider_left: i32,
    min_pane_width: i32,
    divider_width: i32,
) -> Option<SplitPaneLayout> {
    let min_pane_width = min_pane_width.max(1);
    let divider_width = divider_width.max(1);
    let min_left = content.left.saturating_add(min_pane_width);
    let max_left = content
        .right
        .saturating_sub(min_pane_width.saturating_add(divider_width));
    if max_left < min_left || content.bottom <= content.top {
        return None;
    }
    let divider_left = divider_left.clamp(min_left, max_left);
    let divider_right = divider_left.saturating_add(divider_width);
    Some(SplitPaneLayout {
        left: Rect::new(content.left, content.top, divider_left, content.bottom),
        divider: Rect::new(divider_left, content.top, divider_right, content.bottom),
        right: Rect::new(divider_right, content.top, content.right, content.bottom),
    })
}

/// Computes the chrome for a client area of `client_w` x `client_h` physical pixels.
///
/// Any compensation for the invisible frame belongs to the Win32 layer, which supplies `client_w`
/// and `client_h`. `maximised` only removes the resize rim around the page and lets the painter
/// pick the restore glyph.
pub fn compute(
    client_w: i32,
    client_h: i32,
    dpi: u32,
    tab_count: usize,
    maximised: bool,
) -> Layout {
    compute_with_active(client_w, client_h, dpi, tab_count, 0, maximised)
}

/// Computes chrome geometry and shifts the visible tab window to include `active_index`.
pub fn compute_with_active(
    client_w: i32,
    client_h: i32,
    dpi: u32,
    tab_count: usize,
    active_index: usize,
    maximised: bool,
) -> Layout {
    let m = Metrics::new(dpi);
    let client_w = client_w.max(0);
    let client_h = client_h.max(0);

    let margin = m.px(Metrics::OUTER_MARGIN);
    let gap = m.px(Metrics::ISLAND_GAP);
    let strip_h = m.px(Metrics::STRIP_HEIGHT);
    let tab_row_h = m.px(Metrics::TAB_ROW_HEIGHT);
    let new_tab_size = m.px(Metrics::NEW_TAB_SIZE);
    let list_size = m.px(Metrics::TAB_MENU_SIZE);
    let max_w = m.px(Metrics::TAB_MAX_WIDTH);
    let min_w = m.px(Metrics::TAB_MIN_WIDTH);

    let client = Rect::new(0, 0, client_w, client_h);
    let strip = Rect::new(0, 0, client_w, strip_h);
    let tab_row = Rect::new(0, strip_h, client_w, strip_h + tab_row_h);

    let caption_w = 3 * m.px(Metrics::CAPTION_BUTTON_WIDTH);
    let caption_left = (client_w - caption_w).max(0);
    let caption_area = Rect::new(caption_left, 0, client_w, strip_h);

    // The tab overflow picker occupies the trailing edge of the tab row, beside the tab slots.
    let tabs_left = margin;
    let row_right = (client_w - margin).max(tabs_left);
    let active_index = active_index.min(tab_count.saturating_sub(1));
    let active_premium = (min_w / 2).max(1);
    let initial_capacity =
        ((row_right - tabs_left + gap - active_premium) / (min_w + gap)).max(0) as usize;
    let overflow = tab_count > initial_capacity;
    let tab_menu_button = if overflow {
        let left = (row_right - list_size).max(tabs_left);
        Rect::new(
            left,
            tab_row.top + (tab_row_h - list_size) / 2,
            row_right,
            tab_row.top + (tab_row_h + list_size) / 2,
        )
    } else {
        Rect::default()
    };
    let zone_right = if overflow {
        (tab_menu_button.left - gap).max(tabs_left)
    } else {
        row_right
    };
    let tab_zone = Rect::new(tabs_left, tab_row.top, zone_right, tab_row.bottom);
    let available = (zone_right - tabs_left).max(0);
    // Reserve half an inactive tab as an active-tab premium. Widths depend on strip capacity and
    // tab count, never on hover or animation, so selection changes do not cause layout jitter.
    let min_visible = ((available + gap - active_premium) / (min_w + gap)).max(0) as usize;
    let visible_count = tab_count.min(min_visible);
    let first_visible = if overflow {
        active_index
            .min(tab_count.saturating_sub(1))
            .saturating_sub(visible_count.saturating_sub(1))
            .min(tab_count.saturating_sub(visible_count))
    } else {
        0
    };
    let shown = visible_count as i32;
    let inner_available = (available - shown.saturating_sub(1) * gap).max(0);
    let (inactive_tab_width, active_tab_width) = if shown == 0 {
        (max_w, max_w)
    } else if shown == 1 {
        (inner_available, inner_available)
    } else {
        let base = inner_available.saturating_mul(2) / (2 * shown + 1);
        let active = ((base * 3 + 1) / 2).min(max_w);
        let inactive = ((inner_available - active).max(0) / (shown - 1)).clamp(min_w, max_w);
        (inactive, active.max(min_w))
    };

    let island_top = tab_row.top + m.px(Metrics::TAB_ROW_HEIGHT - Metrics::ISLAND_HEIGHT) / 2;
    let island_bottom = island_top + m.px(Metrics::ISLAND_HEIGHT);

    // Toolbar: navigation on the left, "+" just left of the captions, and the omnibox centred in
    // the window with the main-menu bubble directly right of it. The omnibox gives way first when
    // the two sides need it, so the bubble stays stuck to the address pill.
    let nav_size = m.px(Metrics::NAV_BUTTON_SIZE);
    let nav_gap = m.px(Metrics::NAV_BUTTON_GAP);
    let nav_left = m.px(Metrics::NAV_LEFT_INSET);
    let nav_top = (strip_h - nav_size) / 2;
    let nav_button = |slot: i32| {
        let left = match slot {
            0 => nav_left,
            1 => nav_left + nav_size,
            _ => nav_left + 2 * nav_size + nav_gap,
        };
        Rect::new(left, nav_top, left + nav_size, nav_top + nav_size)
    };
    let nav_right = nav_left + 3 * nav_size + nav_gap;

    let menu_w = m.px(Metrics::MENU_BUTTON_SIZE);
    let mut right_edge = caption_left - margin;
    let mut toolbar_fits = true;
    // "+" drops out from the left once it would crowd navigation. This keeps it pinned to the
    // right edge and avoids a middle gap.
    let mut place = |size: i32| {
        let left = right_edge - size;
        if toolbar_fits && left >= nav_right + gap {
            right_edge = left - gap;
            let top = (strip_h - size) / 2;
            Rect::new(left, top, left + size, top + size)
        } else {
            toolbar_fits = false;
            Rect::default()
        }
    };
    let new_tab_button = place(new_tab_size);
    let omnibox_min = nav_right + gap;
    let group_right = right_edge.max(omnibox_min);
    // The bubble only fits when the omnibox keeps a non-negative width beside it.
    let menu_fits = toolbar_fits && group_right - menu_w - gap >= omnibox_min;
    let omnibox_max = if menu_fits {
        group_right - menu_w - gap
    } else {
        group_right
    };
    let omnibox_h = m.px(Metrics::OMNIBOX_HEIGHT);
    let omnibox_w = (omnibox_max - omnibox_min).min(m.px(Metrics::OMNIBOX_MAX_WIDTH));
    let omnibox_left = (client_w / 2 - omnibox_w / 2)
        .clamp(omnibox_min, (omnibox_max - omnibox_w).max(omnibox_min));
    let omnibox_top = (strip_h - omnibox_h) / 2;
    let omnibox = Rect::new(
        omnibox_left,
        omnibox_top,
        omnibox_left + omnibox_w,
        omnibox_top + omnibox_h,
    );
    let main_menu_button = if menu_fits {
        let left = omnibox.right + gap;
        Rect::new(left, omnibox_top, left + menu_w, omnibox_top + omnibox_h)
    } else {
        Rect::default()
    };

    let viewport = content_rect(client_w, client_h, dpi, maximised);

    Layout {
        metrics: m,
        maximised,
        client,
        strip,
        tab_row,
        tab_zone,
        tab_menu_button,
        new_tab_button,
        caption_area,
        back_button: nav_button(0),
        forward_button: nav_button(1),
        reload_button: nav_button(2),
        omnibox,
        main_menu_button,
        viewport,
        rail: Rect::default(),
        tab_count,
        inactive_tab_width,
        active_tab_width,
        active_index,
        tab_left: tabs_left,
        first_visible,
        visible_count,
        island_top,
        island_bottom,
        pill_group_slots: [0; GROUP_SLOT_WORDS],
        ball_group_slots: [0; GROUP_SLOT_WORDS],
        stack_group_slots: [0; GROUP_SLOT_WORDS],
    }
}

/// Which part of the chrome a client-relative point is over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Hit {
    None,
    Tab(usize),
    TabClose(usize),
    SidebarWorkspace,
    SidebarNewTab,
    SidebarResize,
    SidebarGroup(usize),
    SidebarTab(usize),
    SidebarTabClose(usize),
    GroupBall(usize),
    /// A button of the left rail. Only given when the caller supplies the rail's entries.
    Rail(RailKey),
    /// The rail's pill outside every button.
    RailBackground,
    NewTab,
    TabMenu,
    Back,
    Forward,
    Reload,
    Omnibox,
    /// The connection icon at the left of the address pill. It opens Site information.
    SiteInfo,
    Split,
    SplitDivider,
    MainMenu,
    Viewport,
    /// Empty strip. Dragging here moves the window.
    CaptionArea,
    CaptionMinimize,
    CaptionMaximize,
    CaptionClose,
}

impl Layout {
    /// Moves every horizontal position of the chrome `dx` pixels right. The rail and client are left
    /// as they are; the caller sets them.
    fn shifted_right(mut self, dx: i32) -> Self {
        self.strip = self.strip.shifted_right(dx);
        self.tab_row = self.tab_row.shifted_right(dx);
        self.tab_zone = self.tab_zone.shifted_right(dx);
        self.tab_menu_button = self.tab_menu_button.shifted_right(dx);
        self.new_tab_button = self.new_tab_button.shifted_right(dx);
        self.caption_area = self.caption_area.shifted_right(dx);
        self.back_button = self.back_button.shifted_right(dx);
        self.forward_button = self.forward_button.shifted_right(dx);
        self.reload_button = self.reload_button.shifted_right(dx);
        self.omnibox = self.omnibox.shifted_right(dx);
        self.main_menu_button = self.main_menu_button.shifted_right(dx);
        self.viewport = self.viewport.shifted_right(dx);
        self.tab_left += dx;
        self
    }

    /// The connection icon at the left of the address pill, centred vertically in it.
    pub fn site_info_button(&self) -> Rect {
        let m = self.metrics;
        let size = m.px(Metrics::SITE_BUTTON_SIZE);
        let left = self.omnibox.left + m.px(Metrics::SITE_BUTTON_INSET);
        let top = self.omnibox.top + (self.omnibox.height() - size) / 2;
        Rect::new(left, top, left + size, top + size)
    }

    pub fn split_button(&self) -> Rect {
        let m = self.metrics;
        let size = m.px(Metrics::SPLIT_BUTTON_SIZE);
        let right = self.omnibox.right - m.px(Metrics::SPLIT_BUTTON_INSET);
        let top = self.omnibox.top + (self.omnibox.height() - size) / 2;
        Rect::new(right - size, top, right, top + size)
    }

    pub fn tab_count(&self) -> usize {
        self.tab_count
    }

    pub fn first_visible_tab(&self) -> usize {
        self.first_visible
    }

    /// Slot indexes outside the visible tab window, in tab order.
    pub fn hidden_tab_indices(&self) -> impl Iterator<Item = usize> + '_ {
        let visible_end = (self.first_visible + self.visible_count).min(self.tab_count);
        (0..self.first_visible).chain(visible_end..self.tab_count)
    }

    fn is_group_pill(&self, index: usize) -> bool {
        self.pill_group_slots
            .get(index / 64)
            .is_some_and(|word| word >> (index % 64) & 1 == 1)
    }

    fn is_group_stack(&self, index: usize) -> bool {
        self.stack_group_slots
            .get(index / 64)
            .is_some_and(|word| word >> (index % 64) & 1 == 1)
    }

    fn is_group_ball(&self, index: usize) -> bool {
        self.ball_group_slots
            .get(index / 64)
            .is_some_and(|word| word >> (index % 64) & 1 == 1)
    }

    /// Reserves space for group controls and redistributes remaining width among tabs. Slots past
    /// the first `GROUP_SLOT_WORDS * 64` keep full width.
    pub fn with_group_slots(
        mut self,
        pills: impl IntoIterator<Item = bool>,
        balls: impl IntoIterator<Item = bool>,
        stacks: impl IntoIterator<Item = bool>,
    ) -> Self {
        self.pill_group_slots = [0; GROUP_SLOT_WORDS];
        for (index, flag) in pills.into_iter().enumerate().take(GROUP_SLOT_WORDS * 64) {
            if flag {
                self.pill_group_slots[index / 64] |= 1 << (index % 64);
            }
        }
        self.ball_group_slots = [0; GROUP_SLOT_WORDS];
        for (index, flag) in balls.into_iter().enumerate().take(GROUP_SLOT_WORDS * 64) {
            if flag {
                self.ball_group_slots[index / 64] |= 1 << (index % 64);
            }
        }
        self.stack_group_slots = [0; GROUP_SLOT_WORDS];
        for (index, flag) in stacks.into_iter().enumerate().take(GROUP_SLOT_WORDS * 64) {
            if flag {
                self.stack_group_slots[index / 64] |= 1 << (index % 64);
            }
        }
        let m = self.metrics;
        let original_visible = self.visible_count;
        let original_first = self.first_visible;
        let visible_end = self.first_visible + self.visible_count;
        if !(self.first_visible..visible_end).any(|index| {
            self.is_group_pill(index) || self.is_group_ball(index) || self.is_group_stack(index)
        }) {
            return self;
        }
        let max_w = m.px(Metrics::TAB_MAX_WIDTH);
        let min_w = m.px(Metrics::TAB_MIN_WIDTH);
        let mut shown = original_visible;
        let (inactive, active) = loop {
            self.visible_count = shown;
            self.first_visible = if shown == original_visible {
                original_first
            } else {
                self.active_index
                    .saturating_sub(shown.saturating_sub(1))
                    .min(self.tab_count.saturating_sub(shown))
            };
            let first = self.first_visible;
            let end = first + shown;
            let pills = (first..end)
                .filter(|&index| self.is_group_pill(index))
                .count() as i32;
            let regular = shown as i32 - pills;
            let gaps = shown.saturating_sub(1) as i32 * m.px(Metrics::ISLAND_GAP);
            let inner_available =
                (self.tab_zone.width() - gaps - pills * m.px(Metrics::GROUP_CAPSULE_WIDTH)).max(0);
            let active_regular = self.active_index >= first
                && self.active_index < end
                && !self.is_group_pill(self.active_index);
            let widths = if regular <= 0 {
                (max_w, max_w)
            } else if regular == 1 {
                (inner_available, inner_available)
            } else if active_regular {
                let base = inner_available.saturating_mul(2) / (2 * regular + 1);
                let active = ((base * 3 + 1) / 2).min(max_w);
                let inactive =
                    ((inner_available - active).max(0) / (regular - 1)).clamp(min_w, max_w);
                (inactive, active.max(min_w))
            } else {
                let equal = (inner_available / regular).clamp(min_w, max_w);
                (equal, equal)
            };
            let used = (first..end)
                .filter(|&index| !self.is_group_pill(index))
                .map(|index| {
                    let width = if index == self.active_index {
                        widths.1
                    } else {
                        widths.0
                    };
                    let minimum = if self.is_group_stack(index) {
                        m.px(Metrics::GROUP_STACK_SLOT_WIDTH)
                    } else if self.is_group_ball(index) {
                        m.px(Metrics::GROUP_BALL_SLOT_WIDTH)
                    } else {
                        0
                    };
                    width.max(minimum)
                })
                .sum::<i32>();
            if used <= inner_available {
                break widths;
            }
            if self.tab_menu_button.is_empty() {
                let margin = m.px(Metrics::OUTER_MARGIN);
                let list_size = m.px(Metrics::TAB_MENU_SIZE);
                let row_right = (self.client.right - margin).max(self.tab_zone.left);
                let left = (row_right - list_size).max(self.tab_zone.left);
                let top = self.tab_row.top + (self.tab_row.height() - list_size) / 2;
                self.tab_menu_button = Rect::new(left, top, row_right, top + list_size);
                self.tab_zone.right = (left - m.px(Metrics::ISLAND_GAP)).max(self.tab_zone.left);
                continue;
            }
            if shown <= 1 {
                break widths;
            }
            shown -= 1;
        };
        self.inactive_tab_width = inactive;
        self.active_tab_width = active;
        self
    }

    fn tab_width_at(&self, index: usize) -> i32 {
        if self.is_group_pill(index) {
            return self.metrics.px(Metrics::GROUP_CAPSULE_WIDTH);
        }
        let width = if index == self.active_index {
            self.active_tab_width
        } else {
            self.inactive_tab_width
        };
        let minimum = if self.is_group_stack(index) {
            self.metrics.px(Metrics::GROUP_STACK_SLOT_WIDTH)
        } else if self.is_group_ball(index) {
            self.metrics.px(Metrics::GROUP_BALL_SLOT_WIDTH)
        } else {
            0
        };
        width.max(minimum)
    }

    /// Gap between neighbouring tabs and group capsules.
    fn gap_before(&self, _index: usize) -> i32 {
        self.metrics.px(Metrics::ISLAND_GAP)
    }

    fn tab_left_at(&self, index: usize) -> i32 {
        (self.first_visible..index).fold(self.tab_left, |left, slot| {
            left + self.tab_width_at(slot) + self.gap_before(slot + 1)
        })
    }

    /// The rect of a caption button, or an empty rect for any other `Hit`. The painter, the
    /// invalidation code and [`hit_test`] all agree on these.
    pub fn caption_button(&self, button: Hit) -> Rect {
        let width = self.metrics.px(Metrics::CAPTION_BUTTON_WIDTH);
        let slot = match button {
            Hit::CaptionMinimize => 3,
            Hit::CaptionMaximize => 2,
            Hit::CaptionClose => 1,
            _ => return Rect::default(),
        };
        let area = self.caption_area;
        Rect::new(
            area.right - slot * width,
            area.top,
            area.right - (slot - 1) * width,
            area.bottom,
        )
    }

    /// Nominal island rect for tab `index`. Only the part inside `tab_zone` is visible.
    pub fn tab(&self, index: usize) -> Option<Rect> {
        if index >= self.tab_count {
            return None;
        }
        if index < self.first_visible || index >= self.first_visible + self.visible_count {
            return None;
        }
        let left = self.tab_left_at(index);
        let width = self.tab_width_at(index);
        Some(Rect::new(
            left,
            self.island_top,
            left + width,
            self.island_bottom,
        ))
    }

    /// Nominal island rects for every tab, in order. Rects beyond `tab_zone` are hidden.
    pub fn tabs(&self) -> impl Iterator<Item = Rect> + '_ {
        (self.first_visible..self.first_visible + self.visible_count)
            .filter_map(move |index| self.tab(index))
    }

    /// Sub-rects for an island. `tab` should come from [`Layout::tab`].
    pub fn tab_parts(&self, tab: Rect) -> TabParts {
        let m = self.metrics;
        let favicon_size = m.px(Metrics::FAVICON_SIZE);
        let close_size = m.px(Metrics::CLOSE_SIZE);
        let gap = m.px(Metrics::TEXT_GAP);

        let close_minimum_width = m.px(Metrics::FAVICON_SIZE)
            + m.px(Metrics::CLOSE_SIZE)
            + m.px(Metrics::TEXT_GAP)
            + 2 * m.px(Metrics::CLOSE_INSET);
        let close_fits = tab.width() >= close_minimum_width;
        let compact = tab.width() < m.px(Metrics::TITLE_MIN_TAB_WIDTH);
        let close = if close_fits {
            square_in(tab.left + m.px(Metrics::CLOSE_INSET), close_size, tab)
        } else {
            Rect::default()
        };
        let favicon_left = if compact && !close_fits {
            tab.left + (tab.width() - favicon_size) / 2
        } else {
            close.right + gap
        };
        let favicon = square_in(favicon_left, favicon_size, tab);

        let title_left = favicon.right + gap;
        let title_right = tab.right - m.px(Metrics::TAB_PADDING);
        let title = if tab.width() < m.px(Metrics::TITLE_MIN_TAB_WIDTH) || title_right <= title_left
        {
            Rect::new(title_left, tab.top, title_left, tab.bottom)
        } else {
            Rect::new(title_left, tab.top, title_right, tab.bottom)
        };

        TabParts {
            favicon,
            title,
            close,
            active_close: !close_fits,
        }
    }

    /// Sub-rects for a group tab, shortened vertically and shifted clear of its leading ball stack.
    pub fn group_tab_parts(&self, tab: Rect, ball_inset: i32) -> TabParts {
        let tab = crate::chrome::group_ui::shrink_tab(tab, self.metrics);
        let mut parts = self.tab_parts(tab);
        for rect in [&mut parts.favicon, &mut parts.close] {
            if !rect.is_empty() {
                rect.left += ball_inset;
                rect.right += ball_inset;
            }
        }
        if !parts.title.is_empty() {
            parts.title.left += ball_inset;
        }
        parts
    }

    fn tab_centre(&self, index: usize) -> i32 {
        self.tab_left_at(index) + self.tab_width_at(index) / 2
    }

    /// Index of the tab slot under `x`, if `x` is inside a visible tab column. Vertical position
    /// is not checked.
    fn tab_index_at(&self, x: i32) -> Option<usize> {
        if x < self.tab_zone.left || x >= self.tab_zone.right || self.tab_count == 0 {
            return None;
        }
        for index in self.first_visible..self.first_visible + self.visible_count {
            let Some(tab) = self.tab(index) else { continue };
            if tab.contains(x, self.island_top) {
                return Some(index);
            }
        }
        None
    }

    /// Rects of the empty chrome that drag the window. This is the toolbar minus its controls plus
    /// the tab row minus its tab islands. [`hit_test`] returns `CaptionArea` for exactly these points.
    pub fn drag_region_rects(&self) -> Vec<Rect> {
        // Left to right, as the sweep below needs.
        let mut controls: Vec<Rect> = [
            self.back_button,
            self.forward_button,
            self.reload_button,
            self.omnibox,
            self.main_menu_button,
            self.tab_menu_button,
            self.new_tab_button,
            self.caption_area,
        ]
        .into_iter()
        .filter(|control| !control.is_empty())
        .collect();
        controls.sort_by_key(|control| control.left);

        let mut rects = Vec::new();
        append_drag_regions(self.strip, &controls, &mut rects);
        let tabs: Vec<Rect> = (self.first_visible..self.first_visible + self.visible_count)
            .filter_map(|index| self.tab(index))
            .chain(std::iter::once(self.tab_menu_button).filter(|rect| !rect.is_empty()))
            .collect();
        append_drag_regions(self.tab_row, &tabs, &mut rects);
        rects
    }
}

fn append_drag_regions(band: Rect, controls: &[Rect], rects: &mut Vec<Rect>) {
    let mut cursor = band.left;
    for control in controls {
        let control = control.intersect(band);
        if control.is_empty() {
            continue;
        }
        if control.left > cursor {
            rects.push(Rect::new(cursor, band.top, control.left, band.bottom));
        }
        if control.top > band.top {
            rects.push(Rect::new(
                control.left,
                band.top,
                control.right,
                control.top,
            ));
        }
        if control.bottom < band.bottom {
            rects.push(Rect::new(
                control.left,
                control.bottom,
                control.right,
                band.bottom,
            ));
        }
        cursor = cursor.max(control.right);
    }
    if cursor < band.right {
        rects.push(Rect::new(cursor, band.top, band.right, band.bottom));
    }
}

fn square_in(left: i32, size: i32, band: Rect) -> Rect {
    let top = band.top + (band.height() - size) / 2;
    Rect::new(left, top, left + size, top + size)
}

/// Classifies a client-relative point. Points in the gaps between the omnibox, viewport and strip
/// return `None`. The rail reports `RailBackground` here, since without entries its buttons are
/// unknown; use [`hit_test_with_rail`] to name them.
pub fn hit_test(layout: &Layout, x: i32, y: i32) -> Hit {
    hit_test_with_rail(layout, None, x, y)
}

/// [`hit_test`] with the left rail's entries. A point on a visible rail button is `Hit::Rail`, and
/// the rest of the rail is `Hit::RailBackground`, which takes priority over everything else in its
/// area.
pub fn hit_test_with_rail(layout: &Layout, rail: Option<&RailView<'_>>, x: i32, y: i32) -> Hit {
    hit_test_native(layout, rail, &[], x, y)
}

/// [`hit_test_with_rail`] where `html_regions` are regions drawn by the HTML chrome (the toolbar
/// and, when active, the tab row). Points in them are not native chrome, so they give `Hit::None`
/// and no native control is ever hovered or shown.
pub fn hit_test_native(
    layout: &Layout,
    rail: Option<&RailView<'_>>,
    html_regions: &[Rect],
    x: i32,
    y: i32,
) -> Hit {
    if !layout.client.contains(x, y) {
        return Hit::None;
    }
    if layout.rail.contains(x, y) {
        let key = rail.and_then(|view| {
            rail::geometry(layout.rail, view.entries, view.scroll, layout.metrics.dpi()).hit(x, y)
        });
        return key.map_or(Hit::RailBackground, Hit::Rail);
    }
    if html_regions.iter().any(|region| region.contains(x, y)) {
        return Hit::None;
    }
    // The margin left of the toolbar, beside the rail, drags the window like the rest of the strip.
    // Below the toolbar that margin is outside the chrome and takes no hit.
    if y < layout.strip.bottom && x < layout.strip.left {
        return Hit::CaptionArea;
    }
    if layout.viewport.contains(x, y) {
        return Hit::Viewport;
    }
    if layout.site_info_button().contains(x, y) {
        return Hit::SiteInfo;
    }
    if layout.split_button().contains(x, y) {
        return Hit::Split;
    }
    if layout.omnibox.contains(x, y) {
        return Hit::Omnibox;
    }
    if layout.main_menu_button.contains(x, y) {
        return Hit::MainMenu;
    }
    if layout.back_button.contains(x, y) {
        return Hit::Back;
    }
    if layout.forward_button.contains(x, y) {
        return Hit::Forward;
    }
    if layout.reload_button.contains(x, y) {
        return Hit::Reload;
    }
    if layout.new_tab_button.contains(x, y) {
        return Hit::NewTab;
    }
    if layout.tab_menu_button.contains(x, y) {
        return Hit::TabMenu;
    }
    if layout.tab_row.contains(x, y) {
        let Some(index) = layout.tab_index_at(x) else {
            return Hit::CaptionArea;
        };
        let Some(island) = layout.tab(index) else {
            return Hit::CaptionArea;
        };
        if !island.contains(x, y) {
            return Hit::CaptionArea;
        }
        let parts = layout.tab_parts(island);
        return if parts.close.contains(x, y)
            || (index == layout.active_index && parts.active_close && parts.favicon.contains(x, y))
        {
            Hit::TabClose(index)
        } else {
            Hit::Tab(index)
        };
    }
    if !layout.strip.contains(x, y) {
        return Hit::None;
    }
    if layout.caption_area.contains(x, y) {
        if layout.caption_button(Hit::CaptionClose).contains(x, y) {
            return Hit::CaptionClose;
        }
        if layout.caption_button(Hit::CaptionMaximize).contains(x, y) {
            return Hit::CaptionMaximize;
        }
        return Hit::CaptionMinimize;
    }
    Hit::CaptionArea
}

/// Gives a group's ball or stack first claim on its geometry, before the containing tab receives a
/// hit. The caller supplies current animated hit rectangles, in slot order.
pub fn hit_test_with_group_balls(layout: &Layout, balls: &[(usize, Rect)], x: i32, y: i32) -> Hit {
    if !layout.client.contains(x, y) {
        return Hit::None;
    }
    if let Some((index, _)) = balls.iter().find(|(_, rect)| rect.contains(x, y)) {
        return Hit::GroupBall(*index);
    }
    hit_test(layout, x, y)
}

/// Target index for a tab dragged to horizontal position `x`. `x` is the dragged island's
/// centre. The tab moves past a neighbour once its centre crosses that neighbour's centre. The
/// result is an index in the list after the move, which suits [`crate::tabs::TabList::move_tab`].
pub fn reorder_target(layout: &Layout, dragged_index: usize, x: i32) -> usize {
    let count = layout.tab_count;
    if count <= 1 {
        return 0;
    }
    let target = (0..count)
        .filter(|&index| index != dragged_index)
        .filter(|&index| layout.tab_centre(index) < x)
        .count();
    target.min(count - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DPIS: [u32; 4] = [96, 120, 144, 192];
    const WIDTHS: [i32; 3] = [768, 1024, 1440];
    const HEIGHT: i32 = 700;

    fn layout(width: i32, dpi: u32, tabs: usize) -> Layout {
        compute(width, HEIGHT, dpi, tabs, false)
    }

    /// Every rect of a visible control, labelled for failure messages.
    fn visible_controls(l: &Layout) -> Vec<(String, Rect)> {
        let mut out = Vec::new();
        for (index, tab) in l.tabs().enumerate() {
            let visible = tab.intersect(l.tab_zone);
            if !visible.is_empty() {
                out.push((format!("tab {index}"), visible));
            }
        }
        out.push(("tab menu".to_owned(), l.tab_menu_button));
        out.push(("new tab".to_owned(), l.new_tab_button));
        out.push(("caption".to_owned(), l.caption_area));
        out
    }

    fn assert_disjoint(l: &Layout) {
        let controls = visible_controls(l);
        for (i, (name_a, a)) in controls.iter().enumerate() {
            for (name_b, b) in &controls[i + 1..] {
                assert!(
                    a.intersect(*b).is_empty(),
                    "{name_a} {a:?} overlaps {name_b} {b:?}"
                );
            }
        }
    }

    fn assert_inside_client(l: &Layout) {
        for (name, rect) in visible_controls(l) {
            assert!(
                rect.intersect(l.client) == rect,
                "{name} {rect:?} escapes client {:?}",
                l.client
            );
        }
        for rect in [l.omnibox, l.viewport] {
            assert!(rect.intersect(l.client) == rect, "{rect:?} escapes client");
        }
    }

    #[test]
    fn metrics_scale_at_96_dpi_to_logical_values() {
        let m = Metrics::new(96);

        assert_eq!(m.px(Metrics::STRIP_HEIGHT), 34);
        assert_eq!(m.px(Metrics::TAB_ROW_HEIGHT), 28);
        assert_eq!(m.px(Metrics::ISLAND_HEIGHT), 24);
        assert_eq!(m.px(Metrics::ISLAND_MARGIN), 2);
        assert_eq!(m.px(Metrics::ISLAND_GAP), 3);
        assert_eq!(m.px(Metrics::TAB_MAX_WIDTH), 100_000);
        assert_eq!(m.px(Metrics::TAB_MIN_WIDTH), 26);
        assert_eq!(m.px(Metrics::NEW_TAB_SIZE), 28);
        assert_eq!(m.px(Metrics::CAPTION_BUTTON_WIDTH), 46);
        assert_eq!(m.px(Metrics::OMNIBOX_HEIGHT), 26);
        assert_eq!(m.px(Metrics::OUTER_MARGIN), 2);
        assert_eq!(m.px(Metrics::ROW_GAP), 2);
        assert_eq!(m.px(Metrics::NAV_BUTTON_SIZE), 28);
        assert_eq!(m.px(Metrics::NAV_BUTTON_GAP), 3);
        assert_eq!(m.px(Metrics::NAV_LEFT_INSET), 4);
        assert_eq!(m.island_radius(), 13);
        assert_eq!(m.tab_radius(), 9);
        assert_eq!(m.omnibox_radius(), 13);
    }

    #[test]
    fn metrics_scale_at_120_dpi_with_round_half_up() {
        let m = Metrics::new(120);

        assert_eq!(m.px(Metrics::STRIP_HEIGHT), 43, "42.5 rounds up");
        assert_eq!(m.px(Metrics::TAB_ROW_HEIGHT), 35);
        assert_eq!(m.px(Metrics::ISLAND_HEIGHT), 30);
        assert_eq!(m.px(Metrics::ISLAND_MARGIN), 3, "2.5 rounds up");
        assert_eq!(m.px(Metrics::ISLAND_GAP), 4, "3.75 rounds up");
        assert_eq!(m.px(Metrics::TAB_MAX_WIDTH), 125_000);
        assert_eq!(m.px(Metrics::TAB_MIN_WIDTH), 33, "32.5 rounds up");
        assert_eq!(m.px(Metrics::TITLE_MIN_TAB_WIDTH), 150);
        assert_eq!(m.px(Metrics::NEW_TAB_SIZE), 35, "35 is exact");
        assert_eq!(m.px(Metrics::CAPTION_BUTTON_WIDTH), 58, "57.5 rounds up");
        assert_eq!(m.px(Metrics::OMNIBOX_HEIGHT), 33, "32.5 rounds up");
        assert_eq!(m.px(Metrics::OUTER_MARGIN), 3, "2.5 rounds up");
        assert_eq!(m.px(Metrics::ROW_GAP), 3, "2.5 rounds up");
        assert_eq!(m.px(Metrics::NAV_BUTTON_SIZE), 35);
        assert_eq!(m.px(Metrics::NAV_BUTTON_GAP), 4, "3.75 rounds up");
        assert_eq!(m.px(Metrics::NAV_LEFT_INSET), 5, "5 is exact");
        assert_eq!(m.island_radius(), 16, "16.25 rounds down");
        assert_eq!(m.tab_radius(), 11, "11.25 rounds down");
        assert_eq!(m.omnibox_radius(), 16, "16.25 rounds down");
    }

    #[test]
    fn metrics_scale_at_144_dpi() {
        let m = Metrics::new(144);

        assert_eq!(m.px(Metrics::STRIP_HEIGHT), 51);
        assert_eq!(m.px(Metrics::TAB_ROW_HEIGHT), 42);
        assert_eq!(m.px(Metrics::ISLAND_HEIGHT), 36);
        assert_eq!(m.px(Metrics::ISLAND_MARGIN), 3);
        assert_eq!(m.px(Metrics::ISLAND_GAP), 5, "4.5 rounds up");
        assert_eq!(m.px(Metrics::TAB_MAX_WIDTH), 150_000);
        assert_eq!(m.px(Metrics::TAB_MIN_WIDTH), 39);
        assert_eq!(m.px(Metrics::NEW_TAB_SIZE), 42);
        assert_eq!(m.px(Metrics::CAPTION_BUTTON_WIDTH), 69);
        assert_eq!(m.px(Metrics::OMNIBOX_HEIGHT), 39);
        assert_eq!(m.px(Metrics::OUTER_MARGIN), 3);
        assert_eq!(m.px(Metrics::ROW_GAP), 3);
        assert_eq!(m.px(Metrics::NAV_BUTTON_SIZE), 42);
        assert_eq!(m.px(Metrics::NAV_BUTTON_GAP), 5, "4.5 rounds up");
        assert_eq!(m.px(Metrics::NAV_LEFT_INSET), 6);
        assert_eq!(m.island_radius(), 20);
        assert_eq!(m.omnibox_radius(), 20);
    }

    #[test]
    fn metrics_scale_at_192_dpi() {
        let m = Metrics::new(192);

        assert_eq!(m.px(Metrics::STRIP_HEIGHT), 68);
        assert_eq!(m.px(Metrics::TAB_ROW_HEIGHT), 56);
        assert_eq!(m.px(Metrics::ISLAND_HEIGHT), 48);
        assert_eq!(m.px(Metrics::ISLAND_MARGIN), 4);
        assert_eq!(m.px(Metrics::ISLAND_GAP), 6);
        assert_eq!(m.px(Metrics::TAB_MAX_WIDTH), 200_000);
        assert_eq!(m.px(Metrics::TAB_MIN_WIDTH), 52);
        assert_eq!(m.px(Metrics::NEW_TAB_SIZE), 56);
        assert_eq!(m.px(Metrics::CAPTION_BUTTON_WIDTH), 92);
        assert_eq!(m.px(Metrics::OMNIBOX_HEIGHT), 52);
        assert_eq!(m.px(Metrics::OUTER_MARGIN), 4);
        assert_eq!(m.px(Metrics::ROW_GAP), 4);
        assert_eq!(m.px(Metrics::NAV_BUTTON_SIZE), 56);
        assert_eq!(m.px(Metrics::NAV_BUTTON_GAP), 6);
        assert_eq!(m.px(Metrics::NAV_LEFT_INSET), 8);
        assert_eq!(m.island_radius(), 26);
        assert_eq!(m.omnibox_radius(), 26);
    }

    #[test]
    fn zero_dpi_is_treated_as_96() {
        assert_eq!(Metrics::new(0), Metrics::new(96));
        assert_eq!(Metrics::new(0).px(Metrics::STRIP_HEIGHT), 34);
    }

    #[test]
    fn rect_contains_is_half_open() {
        let r = Rect::new(10, 20, 30, 40);

        assert!(r.contains(10, 20));
        assert!(r.contains(29, 39));
        assert!(!r.contains(30, 20));
        assert!(!r.contains(10, 40));
        assert!(!r.contains(9, 20));
        assert_eq!((r.width(), r.height()), (20, 20));
    }

    #[test]
    fn rect_intersect_and_empty() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 20, 20);

        assert_eq!(a.intersect(b), Rect::new(5, 5, 10, 10));
        assert!(a.intersect(Rect::new(10, 0, 20, 10)).is_empty());
        assert_eq!(
            Rect::new(10, 0, 4, 10).width(),
            0,
            "inverted rect has no width"
        );
        assert!(Rect::new(10, 0, 4, 10).is_empty());
    }

    #[test]
    fn rect_from_win32_rect_keeps_bounds() {
        let rect = RECT {
            left: 1,
            top: 2,
            right: 3,
            bottom: 4,
        };

        assert_eq!(Rect::from(rect), Rect::new(1, 2, 3, 4));
    }

    #[test]
    fn one_tab_at_1024_matches_hand_computed_geometry() {
        // The left rail takes 44 px at 96 dpi (2 px margin, 40 px rail, 2 px margin). The chrome is
        // laid out in the 980 px to its right, so every toolbar rect is 44 px further right than it
        // would be without the rail.
        let inset = left_rail_inset(96);
        assert_eq!(inset, 44);
        let l = compute_in_frame(1024, HEIGHT, 96, 1, 0, false);

        assert_eq!(l.strip, Rect::new(inset, 0, 1024, 34));
        assert_eq!(l.tab_row, Rect::new(inset, 34, 1024, 62));
        assert_eq!(l.caption_area, Rect::new(886, 0, 1024, 34));
        assert_eq!(l.tab_zone, Rect::new(46, 34, 1022, 62));
        assert_eq!(
            l.tab(0),
            Some(Rect::new(46, 36, 1022, 60)),
            "a lone tab fills the full-width row"
        );
        assert_eq!(l.new_tab_button, Rect::new(856, 3, 884, 31));
        assert!(
            l.tab_menu_button.is_empty(),
            "no overflow menu when tabs fit"
        );
        assert_eq!(l.main_menu_button, Rect::new(825, 4, 853, 30));
        assert_eq!(
            l.main_menu_button.left,
            l.omnibox.right + 3,
            "the bubble sits directly right of the address pill"
        );
        assert_eq!(l.back_button, Rect::new(48, 3, 76, 31));
        assert_eq!(l.forward_button, Rect::new(76, 3, 104, 31));
        assert_eq!(l.reload_button, Rect::new(107, 3, 135, 31));
        assert_eq!(l.omnibox, Rect::new(198, 4, 822, 30));
        assert_eq!(hit_test(&l, 839, 17), Hit::MainMenu);
        assert_eq!(l.viewport, Rect::new(48, 64, 1020, 696));
        assert_eq!(l.rail, Rect::new(2, 2, 42, 698));
        assert_eq!(hit_test(&l, 22, 20), Hit::RailBackground);
    }

    #[test]
    fn omnibox_is_centred_in_the_window_when_the_toolbar_has_room() {
        // At 1024 the menu bubble pushes the omnibox left of centre, so centring is checked wider.
        let l = layout(1440, 96, 1);

        assert_eq!(l.omnibox, Rect::new(408, 4, 1032, 30));
        assert_eq!(
            (l.omnibox.left + l.omnibox.right) / 2,
            720,
            "the omnibox is centred in the window"
        );
    }

    #[test]
    fn eight_tabs_at_1024_shrink_evenly_without_overflow() {
        let l = layout(1024, 96, 8);

        assert_eq!(l.tab(0).map(Rect::width), Some(176));
        assert_eq!(l.tab(7).map(Rect::width), Some(117));
        assert!(l.tabs().all(|tab| tab.intersect(l.tab_zone) == tab));
        assert!(
            l.tab_menu_button.is_empty(),
            "all tabs fit, so hide the overflow menu"
        );
    }

    #[test]
    fn forty_tabs_overflow_with_whole_islands() {
        let l = layout(768, 96, 40);

        // 26 whole islands fit at the 26 px minimum across the full row.
        assert_eq!(l.tab(0).map(Rect::width), Some(39));
        assert_eq!(l.tab_zone, Rect::new(2, 34, 737, 62));
        assert_eq!(l.new_tab_button, Rect::new(600, 3, 628, 31));
        assert_eq!(l.tab_menu_button, Rect::new(740, 35, 766, 61));
        assert_eq!(l.caption_area.left, 630);
        assert!(
            !l.tab_menu_button.is_empty(),
            "overflow menu stays available"
        );

        assert!(
            l.tab(23).is_some_and(|tab| tab.right <= l.tab_zone.right),
            "the last visible island is whole and inside the zone"
        );
        assert_eq!(l.tab(25), None, "tabs after the visible window are hidden");
        assert!(
            l.tab(24).is_some_and(|tab| tab.right <= l.tab_zone.right),
            "the 25th island still fits"
        );
        assert_eq!(l.tab(39), None, "the default active tab is index zero");
        assert_eq!(
            l.hidden_tab_indices().collect::<Vec<_>>(),
            (25..40).collect::<Vec<_>>()
        );
    }

    #[test]
    fn active_tab_is_kept_visible_when_tabs_overflow() {
        let l = compute_with_active(768, HEIGHT, 96, 40, 39, false);

        assert_eq!(l.first_visible_tab(), 15);
        let active = l.tab(39).expect("active tab is visible");
        assert!(active.right <= l.tab_zone.right);
        assert_eq!(hit_test(&l, active.left + 3, 48), Hit::Tab(39));
        assert_eq!(l.tabs().count(), 25);
        assert_eq!(
            l.hidden_tab_indices().collect::<Vec<_>>(),
            (0..15).collect::<Vec<_>>()
        );
    }

    #[test]
    fn layouts_have_no_overlaps_and_stay_inside_client() {
        for dpi in DPIS {
            for width in WIDTHS {
                for tabs in [1, 8, 40] {
                    let l = layout(width, dpi, tabs);
                    assert_disjoint(&l);
                    assert_inside_client(&l);
                }
            }
        }
    }

    #[test]
    fn left_rail_is_a_full_height_pill_clear_of_the_chrome() {
        for dpi in DPIS {
            for width in WIDTHS {
                for tabs in [0, 1, 8, 40] {
                    let l = compute_in_frame(width, HEIGHT, dpi, tabs, 0, false);
                    let m = Metrics::new(dpi);
                    let margin = m.px(Metrics::OUTER_MARGIN);
                    let rail = l.rail;
                    assert_eq!(rail.width(), m.px(LEFT_RAIL_WIDTH));
                    assert_eq!((rail.left, rail.top), (margin, margin));
                    assert_eq!(rail.bottom, HEIGHT - margin);

                    let mut chrome = visible_controls(&l);
                    chrome.extend([
                        ("strip".to_owned(), l.strip),
                        ("viewport".to_owned(), l.viewport),
                        ("back".to_owned(), l.back_button),
                        ("forward".to_owned(), l.forward_button),
                        ("reload".to_owned(), l.reload_button),
                        ("omnibox".to_owned(), l.omnibox),
                        ("main menu".to_owned(), l.main_menu_button),
                    ]);
                    for (name, rect) in chrome {
                        if rect.is_empty() {
                            continue;
                        }
                        assert!(
                            rect.left >= rail.right,
                            "{name} {rect:?} starts left of the rail {rail:?}"
                        );
                    }
                    assert_disjoint(&l);
                    assert_inside_client(&l);
                }
            }
        }
    }

    #[test]
    fn tab_controls_sit_in_the_toolbar_beside_the_captions() {
        for dpi in DPIS {
            for width in WIDTHS {
                for tabs in [1, 8, 40] {
                    let l = layout(width, dpi, tabs);
                    let m = l.metrics;
                    let margin = m.px(Metrics::OUTER_MARGIN);
                    let gap = m.px(Metrics::ISLAND_GAP);
                    let (new_tab, list, menu) =
                        (l.new_tab_button, l.tab_menu_button, l.main_menu_button);

                    assert_eq!(new_tab.width(), m.px(Metrics::NEW_TAB_SIZE));
                    assert_eq!(new_tab.right, l.caption_area.left - margin, "dpi {dpi}");
                    if tabs > l.tabs().count() {
                        assert_eq!(list.width(), m.px(Metrics::TAB_MENU_SIZE));
                        assert_eq!(l.tab_zone.right + gap, list.left, "dpi {dpi}");
                        assert!(l.tab_row.contains_rect(list), "dpi {dpi}");
                    } else {
                        assert!(list.is_empty(), "hide the menu while every tab fits");
                        assert!(menu.right + gap <= new_tab.left, "dpi {dpi}");
                    }
                    for (name, rect) in [("new tab", new_tab), ("tab menu", list)] {
                        if !rect.is_empty() {
                            let band = if name == "tab menu" {
                                l.tab_row
                            } else {
                                l.strip
                            };
                            assert!(band.contains_rect(rect), "{name} leaves its row");
                        }
                    }
                    assert!(!l.tab_row.contains_rect(new_tab));
                    if list.is_empty() {
                        assert_eq!(l.tab_zone.right, l.client.right - margin);
                    }
                    assert_eq!(l.tab_zone.bottom, l.tab_row.bottom);
                }
            }
        }
    }

    #[test]
    fn strip_omnibox_and_viewport_tile_the_client_vertically() {
        for dpi in DPIS {
            for width in WIDTHS {
                let m = Metrics::new(dpi);
                let l = layout(width, dpi, 3);
                let margin = m.px(Metrics::OUTER_MARGIN);
                assert_eq!(l.strip.top, 0);
                assert_eq!(l.strip.bottom, l.tab_row.top);
                assert_eq!(l.tab_row.bottom + margin, l.viewport.top);
                let rim = m.px(Metrics::RESIZE_RIM);
                assert_eq!(l.viewport.bottom, HEIGHT - rim);
                assert_eq!(l.omnibox.height(), m.px(Metrics::OMNIBOX_HEIGHT));
                assert!(l.omnibox.left >= l.reload_button.right + m.px(Metrics::ISLAND_GAP));
                assert_eq!(l.new_tab_button.right, l.caption_area.left - margin);
                assert_eq!(l.viewport.left, rim);

                assert_eq!(l.viewport.right, width - rim);

                let max = compute(width, HEIGHT, dpi, 3, true);
                assert_eq!(max.viewport, Rect::new(0, l.viewport.top, width, HEIGHT));
            }
        }
    }

    #[test]
    fn islands_and_tab_controls_are_centred_in_the_tab_row() {
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            let l = layout(1024, dpi, 3);
            let tab = l.tab(0).expect("tab");

            assert_eq!(tab.height(), m.px(Metrics::ISLAND_HEIGHT));
            // The row height can be an odd number of pixels, so the island can sit one pixel off
            // centre at fractional dpi.
            let above = tab.top - l.tab_row.top;
            let below = l.tab_row.bottom - tab.bottom;
            assert!(
                (above - below).abs() <= 1,
                "dpi {dpi}: {above} above, {below} below"
            );
            for (name, rect) in [
                ("omnibox", l.omnibox),
                ("back", l.back_button),
                ("new tab", l.new_tab_button),
            ] {
                let (above, below) = (rect.top - l.strip.top, l.strip.bottom - rect.bottom);
                assert!(
                    (above - below).abs() <= 1,
                    "dpi {dpi}: {name} is {above} above and {below} below"
                );
            }
            assert!(
                l.tab_menu_button.is_empty(),
                "three tabs fit without overflow"
            );
        }
    }

    #[test]
    fn tabs_are_separated_by_the_island_gap_at_every_dpi() {
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            let l = layout(1440, dpi, 4);

            for index in 1..4 {
                let prev = l.tab(index - 1).expect("previous tab");
                let next = l.tab(index).expect("tab");
                assert_eq!(
                    next.left - prev.right,
                    m.px(Metrics::ISLAND_GAP),
                    "dpi {dpi}"
                );
            }
        }
    }

    #[test]
    fn single_tab_spans_the_row() {
        let l = layout(1440, 96, 1);

        assert_eq!(l.tab(0).map(Rect::width), Some(l.tab_zone.width()));
    }

    #[test]
    fn zero_tabs_leave_plus_pinned_to_the_toolbar() {
        let l = compute(1024, HEIGHT, 96, 0, false);

        assert_eq!(l.tab_count(), 0);
        assert_eq!(l.tab(0), None);
        assert_eq!(l.new_tab_button.left, 856);
        assert_eq!(l.tabs().count(), 0);
    }

    #[test]
    fn tab_widths_shrink_as_count_grows() {
        let mut previous = i32::MAX;
        for tabs in 1..=40 {
            let width = layout(1024, 96, tabs).tab(0).expect("tab").width();
            assert!(
                width <= previous,
                "width grew from {previous} to {width} at {tabs} tabs"
            );
            assert!(width >= 26);
            previous = width;
        }
    }

    #[test]
    fn active_tab_is_wider_without_changing_the_slot_count() {
        let selected = compute_with_active(900, HEIGHT, 96, 6, 3, false);
        let first = selected.tab(0).unwrap();
        let active = selected.tab(3).unwrap();
        let last = selected.tab(5).unwrap();
        assert!(active.width() > first.width());
        assert_eq!(selected.tabs().count(), 6);
        assert!(last.right <= selected.tab_zone.right);
    }

    #[test]
    fn narrow_tab_close_threshold_uses_scaled_metrics() {
        for dpi in [96, 120, 144, 192] {
            let metrics = Metrics::new(dpi);
            let threshold = metrics.px(Metrics::FAVICON_SIZE)
                + metrics.px(Metrics::CLOSE_SIZE)
                + metrics.px(Metrics::TEXT_GAP)
                + 2 * metrics.px(Metrics::CLOSE_INSET);
            let too_narrow = compute_with_active(threshold - 1, HEIGHT, dpi, 1, 0, false);
            let narrow_tab = too_narrow.tab(0).unwrap();
            let parts = too_narrow.tab_parts(narrow_tab);
            assert!(parts.active_close, "DPI {dpi}");
            assert!(parts.close.is_empty(), "DPI {dpi}");

            let enough_room = compute_with_active(
                threshold + 2 * metrics.px(Metrics::OUTER_MARGIN) + 1,
                HEIGHT,
                dpi,
                1,
                0,
                false,
            );
            let parts = enough_room.tab_parts(enough_room.tab(0).unwrap());
            assert!(!parts.active_close, "DPI {dpi}");
            assert!(!parts.close.is_empty(), "DPI {dpi}");
        }
    }

    #[test]
    fn maximised_flag_is_carried_and_only_drops_the_resize_rim() {
        let windowed = compute(1024, HEIGHT, 120, 5, false);
        let maximised = compute(1024, HEIGHT, 120, 5, true);

        assert!(!windowed.maximised);
        assert!(maximised.maximised);
        let rim = windowed.metrics.px(Metrics::RESIZE_RIM);
        assert_eq!(rim, 5);
        assert_eq!(
            Layout {
                maximised: false,
                viewport: windowed.viewport,
                ..maximised
            },
            windowed
        );
        assert_eq!(
            maximised.viewport,
            Rect::new(0, windowed.viewport.top, 1024, HEIGHT)
        );
        assert_eq!(
            windowed.viewport,
            Rect::new(rim, windowed.viewport.top, 1024 - rim, HEIGHT - rim)
        );
    }

    #[test]
    fn caption_buttons_tile_the_caption_area_and_match_hit_testing() {
        for dpi in DPIS {
            let l = layout(1024, dpi, 3);
            let buttons = [
                (Hit::CaptionMinimize, l.caption_button(Hit::CaptionMinimize)),
                (Hit::CaptionMaximize, l.caption_button(Hit::CaptionMaximize)),
                (Hit::CaptionClose, l.caption_button(Hit::CaptionClose)),
            ];
            assert_eq!(buttons[0].1.left, l.caption_area.left);
            assert_eq!(buttons[0].1.right, buttons[1].1.left);
            assert_eq!(buttons[1].1.right, buttons[2].1.left);
            assert_eq!(buttons[2].1.right, l.caption_area.right);
            for (hit, rect) in buttons {
                assert_eq!(rect.height(), l.strip.height());
                assert_eq!(hit_test(&l, rect.left, rect.top), hit, "dpi {dpi}");
                assert_eq!(hit_test(&l, rect.right - 1, rect.bottom - 1), hit);
            }
            assert!(l.caption_button(Hit::Omnibox).is_empty());
        }
    }

    #[test]
    fn the_resize_rim_leaves_the_page_off_the_window_edges() {
        let l = layout(1024, 96, 1);
        let rim = l.metrics.px(Metrics::RESIZE_RIM);
        assert!(rim > 0);
        for (x, y) in [(0, 400), (rim - 1, 400), (1023, 400), (500, HEIGHT - 1)] {
            assert_eq!(hit_test(&l, x, y), Hit::None, "({x},{y}) is the rim");
        }
        assert_eq!(hit_test(&l, rim, 400), Hit::Viewport);
    }

    #[test]
    fn degenerate_client_sizes_do_not_panic() {
        let empty = compute(0, 0, 96, 5, false);
        assert_eq!(hit_test(&empty, 0, 0), Hit::None);

        let tiny = compute(4, 4, 96, 40, false);
        assert_eq!(hit_test(&tiny, -1, 0), Hit::None);

        // Check disjointness at the narrowest width where both tab controls fit.
        assert_disjoint(&compute(204, 4, 96, 40, false));
    }

    #[test]
    fn hit_test_identifies_each_region_at_1024() {
        // The active slot is visibly wider; all tab hits remain within their own island.
        // Window 1024 wide with the rail's 44 px inset. Chrome x positions are the old ones plus 44,
        // except the caption buttons, which sit at the window's right edge either way.
        let l = compute_in_frame(1024, HEIGHT, 96, 3, 0, false);

        assert_eq!(hit_test(&l, 144, 48), Hit::Tab(0));
        assert_eq!(hit_test(&l, 544, 48), Hit::Tab(1));
        assert_eq!(hit_test(&l, 844, 48), Hit::Tab(2));
        assert_eq!(
            hit_test(&l, 839, 15),
            Hit::MainMenu,
            "the main-menu bubble sits right of the address pill"
        );
        assert_eq!(
            hit_test(&l, 884, 48),
            Hit::Tab(2),
            "the tab row has no controls at its right end"
        );
        assert_eq!(
            hit_test(&l, 54, 48),
            Hit::TabClose(0),
            "close is on the left edge of the tab"
        );
        assert_eq!(
            hit_test(&l, 144, 35),
            Hit::CaptionArea,
            "the row margin is a window drag area"
        );
        let gap_x = (l.tab(0).unwrap().right + l.tab(1).unwrap().left) / 2;
        assert_eq!(
            hit_test(&l, gap_x, 48),
            Hit::CaptionArea,
            "the gap between islands is a window drag area"
        );
        assert_eq!(hit_test(&l, 544, 48), Hit::Tab(1));
        assert_eq!(
            hit_test(&l, 1, 15),
            Hit::CaptionArea,
            "left margin is drag area"
        );
        assert_eq!(hit_test(&l, 870, 15), Hit::NewTab);
        assert_eq!(hit_test(&l, 194, 15), Hit::CaptionArea);
        assert_eq!(hit_test(&l, 544, 15), Hit::Omnibox);
        assert_eq!(hit_test(&l, 544, 1), Hit::CaptionArea, "above the omnibox");

        assert_eq!(hit_test(&l, 900, 20), Hit::CaptionMinimize);
        assert_eq!(hit_test(&l, 950, 20), Hit::CaptionMaximize);
        assert_eq!(hit_test(&l, 1000, 20), Hit::CaptionClose);
        assert_eq!(hit_test(&l, 544, 63), Hit::None, "margin above viewport");
        assert_eq!(hit_test(&l, 544, 200), Hit::Viewport);
        assert_eq!(hit_test(&l, 1024, 10), Hit::None, "right of client");
        assert_eq!(hit_test(&l, -1, 10), Hit::None, "left of client");
        assert_eq!(
            hit_test(&l, 22, 20),
            Hit::RailBackground,
            "the rail is not chrome"
        );
    }

    #[test]
    fn rail_buttons_take_hits_before_the_chrome_at_every_dpi() {
        use crate::chrome::rail::{RailEntry, RailIcon};

        let entries: Vec<RailEntry<'_>> = [RailKey::Home, RailKey::Workspace(9), RailKey::Menu]
            .into_iter()
            .map(|key| RailEntry {
                key,
                icon: RailIcon::Plus,
                label: String::new(),
                active: false,
                dot: false,
                accent: None,
                favicon: None,
            })
            .collect();
        let view = RailView {
            entries: &entries,
            fades: &[],
            open_panel: None,
            scroll: 0,
            drop_indicator: None,
            focused: None,
            tooltip: None,
        };
        for dpi in DPIS {
            for width in [768, 1024] {
                let l = compute_in_frame(width, HEIGHT, dpi, 2, 0, false);
                let geo = rail::geometry(l.rail, &entries, 0, dpi);
                for item in &geo.items {
                    let shown = item.visible();
                    let (x, y) = (
                        shown.left + shown.width() / 2,
                        shown.top + shown.height() / 2,
                    );
                    assert_eq!(
                        hit_test_with_rail(&l, Some(&view), x, y),
                        Hit::Rail(item.key),
                        "centre of {:?} at dpi {dpi}, width {width}",
                        item.key
                    );
                }
                let first = geo.items[0].rect;
                let between_first_two = first.bottom + 1;
                let rail_x = l.rail.left + l.rail.width() / 2;
                assert_eq!(
                    hit_test_with_rail(&l, Some(&view), rail_x, between_first_two),
                    Hit::RailBackground,
                    "the gap between two buttons is the rail itself"
                );
                assert_eq!(
                    hit_test(&l, rail_x, between_first_two),
                    Hit::RailBackground,
                    "without entries the whole rail is background"
                );
                assert_eq!(
                    hit_test_with_rail(&l, Some(&view), l.viewport.left + 10, 200),
                    Hit::Viewport,
                    "the page starts right of the rail"
                );
            }
        }
    }

    #[test]
    fn html_toolbar_region_gives_no_native_hits() {
        let l = compute_in_frame(1024, 600, 96, 3, 0, false);
        let region = Rect::new(
            l.back_button.left,
            l.strip.top,
            l.main_menu_button.right,
            l.strip.bottom,
        );
        let (x, y) = (
            (l.omnibox.left + l.omnibox.right) / 2,
            (l.omnibox.top + l.omnibox.bottom) / 2,
        );
        assert_eq!(hit_test(&l, x, y), Hit::Omnibox);
        assert_eq!(
            hit_test_native(&l, None, &[region], x, y),
            Hit::None,
            "omnibox inside the HTML region"
        );
        assert_eq!(
            hit_test_native(&l, None, &[region], l.back_button.left + 2, l.back_button.top + 2),
            Hit::None
        );
        // Outside the region the native hit is unchanged.
        assert_eq!(
            hit_test_native(&l, None, &[region], l.new_tab_button.left + 2, l.new_tab_button.top + 2),
            hit_test(&l, l.new_tab_button.left + 2, l.new_tab_button.top + 2)
        );
        assert_eq!(hit_test_native(&l, None, &[], x, y), Hit::Omnibox);
    }

    #[test]
    fn html_tab_row_region_gives_no_tab_or_group_hits() {
        let l = compute_in_frame(1024, 600, 96, 3, 0, false);
        let row = l.tab_row;
        let (x, y) = (row.left + 20, row.top + row.height() / 2);
        assert!(
            matches!(hit_test(&l, x, y), Hit::Tab(_) | Hit::TabClose(_) | Hit::CaptionArea),
            "{:?}",
            hit_test(&l, x, y)
        );
        assert_eq!(hit_test_native(&l, None, &[row], x, y), Hit::None);
        // The strip above the row keeps its native hits, and so does the toolbar's "+".
        let plus = (l.new_tab_button.left + 2, l.new_tab_button.top + 2);
        assert_eq!(hit_test_native(&l, None, &[row], plus.0, plus.1), hit_test(&l, plus.0, plus.1));
        assert_eq!(hit_test_native(&l, None, &[row], l.strip.left + 2, l.strip.top + 2), hit_test(&l, l.strip.left + 2, l.strip.top + 2));
    }

    #[test]
    fn hit_test_respects_tab_zone_when_overflowing() {
        let l = compute_with_active(768, HEIGHT, 96, 40, 9, false);

        let last_index = l.first_visible_tab() + l.tabs().count() - 1;
        let last = l.tab(last_index).expect("last visible column");
        assert_eq!(hit_test(&l, last.left + 3, 48), Hit::Tab(last_index));
        assert_eq!(
            hit_test(&l, last.right + 1, 48),
            Hit::CaptionArea,
            "the gap past the last island is a window drag area"
        );
        let picker_x = l.tab_menu_button.left + 2;
        let picker_y = (l.tab_menu_button.top + l.tab_menu_button.bottom) / 2;
        assert_eq!(
            hit_test(&l, picker_x, picker_y),
            Hit::TabMenu,
            "tab list stays reachable inside the tab row"
        );
        assert_eq!(
            hit_test(&l, 615, 15),
            Hit::NewTab,
            "+ stays reachable in the toolbar"
        );
        assert_eq!(
            hit_test(&l, picker_x, picker_y),
            Hit::TabMenu,
            "the overflow picker has its own hit target"
        );
    }

    #[test]
    fn hit_test_scales_close_button_with_dpi() {
        let l = layout(1024, 192, 1);
        let tab = l.tab(0).expect("tab");
        let close = l.tab_parts(tab).close;

        assert_eq!(
            hit_test(&l, close.left + 1, close.top + 1),
            Hit::TabClose(0)
        );
        assert_eq!(hit_test(&l, tab.left + 8, tab.top + 1), Hit::Tab(0));
    }

    #[test]
    fn compact_overflow_tabs_keep_inactive_favicons_and_close_active_tab() {
        let l = layout(768, 96, 40);
        let tab = l.tab(0).expect("first compact tab");
        assert!(tab.width() > Metrics::new(96).px(Metrics::TAB_MIN_WIDTH));
        let parts = l.tab_parts(tab);
        assert!(parts.active_close);
        assert!(parts.close.is_empty());
        assert_eq!(
            hit_test(
                &l,
                (parts.favicon.left + parts.favicon.right) / 2,
                (parts.favicon.top + parts.favicon.bottom) / 2
            ),
            Hit::TabClose(0)
        );
        let inactive = l.tab_parts(l.tab(1).expect("inactive tab"));
        assert!(inactive.close.is_empty());
        assert!(!inactive.favicon.is_empty());
        assert_eq!(
            hit_test(&l, inactive.favicon.left + 1, inactive.favicon.top + 1),
            Hit::Tab(1)
        );
    }

    /// The back, forward and reload buttons, left to right, each with the hit it should return.
    fn nav_buttons(l: &Layout) -> [(Hit, Rect); 3] {
        [
            (Hit::Back, l.back_button),
            (Hit::Forward, l.forward_button),
            (Hit::Reload, l.reload_button),
        ]
    }

    #[test]
    fn nav_buttons_are_disjoint_square_and_left_of_the_omnibox() {
        for dpi in DPIS {
            for width in WIDTHS {
                let m = Metrics::new(dpi);
                let l = layout(width, dpi, 3);
                let buttons = nav_buttons(&l);
                for (i, (hit_a, a)) in buttons.iter().enumerate() {
                    assert_eq!(
                        a.width(),
                        m.px(Metrics::NAV_BUTTON_SIZE),
                        "{hit_a:?} dpi {dpi}"
                    );
                    assert_eq!(
                        a.height(),
                        m.px(Metrics::NAV_BUTTON_SIZE),
                        "{hit_a:?} dpi {dpi}"
                    );
                    assert!(
                        a.right <= l.omnibox.left,
                        "{hit_a:?} {a:?} is not left of the omnibox {:?}",
                        l.omnibox
                    );
                    for (hit_b, b) in &buttons[i + 1..] {
                        assert!(
                            a.intersect(*b).is_empty(),
                            "{hit_a:?} {a:?} overlaps {hit_b:?} {b:?} (dpi {dpi}, width {width})"
                        );
                        assert!(a.right <= b.left, "{hit_a:?} is not left of {hit_b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn nav_buttons_are_centred_in_the_toolbar() {
        for dpi in DPIS {
            for width in WIDTHS {
                let l = layout(width, dpi, 3);
                for (hit, rect) in nav_buttons(&l) {
                    let (above, below) = (rect.top - l.strip.top, l.strip.bottom - rect.bottom);
                    assert!((above - below).abs() <= 1, "{hit:?} dpi {dpi}");
                    assert!(rect.intersect(l.client) == rect, "{hit:?} escapes client");
                }
                assert_eq!(
                    l.back_button.left,
                    Metrics::new(dpi).px(Metrics::NAV_LEFT_INSET),
                    "dpi {dpi}"
                );
            }
        }
    }

    #[test]
    fn hit_test_identifies_nav_buttons_at_every_dpi() {
        for dpi in DPIS {
            for width in WIDTHS {
                let l = layout(width, dpi, 3);
                for (hit, rect) in nav_buttons(&l) {
                    let centre = ((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2);
                    for (x, y) in [
                        (rect.left, rect.top),
                        centre,
                        (rect.right - 1, rect.bottom - 1),
                    ] {
                        assert_eq!(hit_test(&l, x, y), hit, "({x},{y}) dpi {dpi} width {width}");
                    }
                    assert_ne!(
                        hit_test(&l, rect.left - 1, centre.1),
                        hit,
                        "left of {hit:?}"
                    );
                    assert_ne!(hit_test(&l, rect.right, centre.1), hit, "right of {hit:?}");
                    assert_ne!(hit_test(&l, centre.0, rect.top - 1), hit, "above {hit:?}");
                    assert_ne!(hit_test(&l, centre.0, rect.bottom), hit, "below {hit:?}");
                }
            }
        }
    }

    #[test]
    fn main_menu_bubble_sits_right_of_the_address_pill_without_overlap() {
        // 520 logical px is the window's minimum track width.
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            for logical_width in [520, 600, 768, 1024, 1440] {
                for tabs in [1, 8, 40] {
                    let l = layout(m.px(logical_width), dpi, tabs);
                    let gap = m.px(Metrics::ISLAND_GAP);
                    let menu = l.main_menu_button;
                    let context = format!("dpi {dpi} width {logical_width} tabs {tabs}");

                    assert!(!menu.is_empty(), "bubble is shown; {context}");
                    assert_eq!(
                        menu.left - l.omnibox.right,
                        gap,
                        "bubble is stuck to the pill; {context}"
                    );
                    assert_eq!(
                        (menu.top, menu.bottom),
                        (l.omnibox.top, l.omnibox.bottom),
                        "bubble is as tall as the pill; {context}"
                    );
                    assert_eq!(menu.width(), m.px(Metrics::MENU_BUTTON_SIZE), "{context}");
                    if !l.new_tab_button.is_empty() {
                        assert!(menu.right + gap <= l.new_tab_button.left, "{context}");
                    }

                    let controls = [
                        ("back", l.back_button),
                        ("forward", l.forward_button),
                        ("reload", l.reload_button),
                        ("address pill", l.omnibox),
                        ("main menu", menu),
                        ("new tab", l.new_tab_button),
                        ("captions", l.caption_area),
                    ];
                    for (i, (name_a, a)) in controls.iter().enumerate() {
                        for (name_b, b) in &controls[i + 1..] {
                            if a.is_empty() || b.is_empty() {
                                continue;
                            }
                            assert!(
                                a.intersect(*b).is_empty(),
                                "{name_a} {a:?} overlaps {name_b} {b:?}; {context}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn drag_regions_match_caption_area_hits_exactly() {
        for dpi in DPIS {
            for width in [768, 1024] {
                for tabs in [1, 8, 40] {
                    let l = layout(width, dpi, tabs);
                    let rects = l.drag_region_rects();
                    for y in l.strip.top..l.tab_row.bottom {
                        for x in l.client.left..l.client.right {
                            let hit = hit_test(&l, x, y);
                            let in_regions = rects.iter().filter(|r| r.contains(x, y)).count();
                            assert!(in_regions <= 1, "({x},{y}) is in {in_regions} drag rects");
                            assert_eq!(
                                hit == Hit::CaptionArea,
                                in_regions == 1,
                                "({x},{y}) hit {hit:?} but drag rects say {in_regions} (dpi {dpi}, width {width}, tabs {tabs})"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn drag_regions_follow_the_visible_window_when_tabs_are_scrolled() {
        for dpi in DPIS {
            for width in [768, 1024] {
                let l = compute_with_active(width, HEIGHT, dpi, 40, 39, false);
                assert!(l.first_visible_tab() > 0, "tabs must be scrolled");
                let rects = l.drag_region_rects();
                for y in l.strip.top..l.tab_row.bottom {
                    for x in l.client.left..l.client.right {
                        let hit = hit_test(&l, x, y);
                        let in_regions = rects.iter().filter(|r| r.contains(x, y)).count();
                        assert!(in_regions <= 1, "({x},{y}) is in {in_regions} drag rects");
                        assert_eq!(
                            hit == Hit::CaptionArea,
                            in_regions == 1,
                            "({x},{y}) hit {hit:?} but drag rects say {in_regions} (dpi {dpi}, width {width})"
                        );
                    }
                }
                let visible = l
                    .tab(39)
                    .expect("active tab is visible")
                    .intersect(l.tab_zone);
                let (x, y) = ((visible.left + visible.right) / 2, visible.top + 1);
                assert!(
                    !rects.iter().any(|r| r.contains(x, y)),
                    "the active island must not drag the window"
                );
            }
        }
    }

    #[test]
    fn chrome_height_is_where_the_viewport_starts() {
        for dpi in DPIS {
            let l = compute(1000, 700, dpi, 1, false);
            assert_eq!(chrome_height(dpi), l.viewport.top, "dpi {dpi}");
        }
    }

    #[test]
    fn drag_region_rects_stay_inside_chrome() {
        let l = layout(1024, 96, 5);

        for rect in l.drag_region_rects() {
            assert!(
                rect.intersect(l.strip) == rect || rect.intersect(l.tab_row) == rect,
                "{rect:?} leaves the chrome"
            );
            assert!(!rect.is_empty());
        }
    }

    #[test]
    fn tab_parts_fit_inside_island_without_overlap() {
        let l = layout(1440, 120, 3);
        let tab = l.tab(0).expect("tab");
        let parts = l.tab_parts(tab);

        assert!(parts.favicon.left >= tab.left);
        assert!(parts.close.right <= parts.favicon.left);
        assert!(parts.favicon.right <= parts.title.left);
        assert!(parts.title.right <= tab.right - Metrics::new(120).px(Metrics::TAB_PADDING));
        for rect in [parts.favicon, parts.close] {
            assert_eq!(rect.width(), rect.height());
            assert!(rect.top >= tab.top && rect.bottom <= tab.bottom);
        }
        assert_eq!(
            parts.favicon.width(),
            Metrics::new(120).px(Metrics::FAVICON_SIZE)
        );
        assert_eq!(
            parts.close.width(),
            Metrics::new(120).px(Metrics::CLOSE_SIZE)
        );
    }

    #[test]
    fn title_is_hidden_below_threshold_width() {
        let narrow = layout(768, 96, 40);
        let tab = narrow.tab(0).expect("tab");
        assert!(tab.width() > 26);
        assert_eq!(narrow.tab_parts(tab).title.width(), 0);

        let wide = layout(1024, 96, 1);
        let tab = wide.tab(0).expect("tab");
        assert!(wide.tab_parts(tab).title.width() > 0);
    }

    #[test]
    fn title_threshold_scales_with_dpi() {
        // The threshold is 120 logical px, which is 150 px at 120 dpi.
        let l = layout(1440, 120, 1);
        let tab = l.tab(0).expect("tab");
        let wide = Rect::new(tab.left, tab.top, tab.left + 149, tab.bottom);
        let just_wide = Rect::new(tab.left, tab.top, tab.left + 150, tab.bottom);

        assert_eq!(l.tab_parts(wide).title.width(), 0);
        assert!(l.tab_parts(just_wide).title.width() > 0);
    }

    #[test]
    fn reorder_target_moves_past_neighbours_at_centre_crossing() {
        let l = layout(1024, 96, 3);
        // Centres: 171, 512, 853.

        assert_eq!(reorder_target(&l, 0, 171), 0);
        assert_eq!(reorder_target(&l, 0, 400), 0);
        assert_eq!(reorder_target(&l, 0, 600), 1);
        assert_eq!(reorder_target(&l, 0, 1000), 2);
    }

    #[test]
    fn reorder_target_moves_left_past_neighbours() {
        let l = layout(1024, 96, 3);

        assert_eq!(reorder_target(&l, 2, 853), 2);
        assert_eq!(reorder_target(&l, 2, 400), 1);
        assert_eq!(reorder_target(&l, 2, 100), 0);
    }

    #[test]
    fn reorder_target_edge_cases() {
        let single = layout(1024, 96, 1);
        assert_eq!(
            reorder_target(&single, 0, 5000),
            0,
            "single tab cannot move"
        );

        let none = compute(1024, HEIGHT, 96, 0, false);
        assert_eq!(reorder_target(&none, 0, 100), 0);

        let l = layout(1024, 96, 3);
        assert_eq!(reorder_target(&l, 9, -100), 0);
        assert_eq!(
            reorder_target(&l, 9, 5000),
            2,
            "out-of-range drag stays in bounds"
        );
    }

    #[test]
    fn reorder_target_past_last_tab_lands_last() {
        // The result is a final index, the same one move_tab expects.
        let l = layout(1024, 96, 4);
        let target = reorder_target(&l, 0, 5000);

        assert_eq!(target, 3);
    }

    #[test]
    fn drag_regions_are_non_empty_with_many_tabs() {
        let l = layout(768, 96, 40);

        assert!(!l.drag_region_rects().is_empty());
    }

    #[test]
    fn split_panes_clamp_divider_and_fit_at_minimum_window_width() {
        let content = content_rect(520, 420, 96, false);
        let panes = split_pane_layout(content, content.left + 100, 220, 6)
            .expect("the minimum window still fits two panes");
        assert_eq!(panes.left.left, content.left);
        assert_eq!(panes.right.right, content.right);
        assert_eq!(panes.divider.width(), 6);
        assert!(panes.left.width() >= 220);
        assert!(panes.right.width() >= 220);
        assert_eq!(panes.left.right, panes.divider.left);
        assert_eq!(panes.right.left, panes.divider.right);
    }

    #[test]
    fn split_panes_collapse_when_the_content_cannot_fit_both_minimum_widths() {
        let content = Rect::new(0, 0, 400, 300);
        assert!(split_pane_layout(content, 200, 220, 6).is_none());
    }

    #[test]
    fn content_rect_keeps_consistent_physical_pixel_margins() {
        for dpi in [96, 120, 144, 192] {
            let rect = content_rect(1200, 800, dpi, false);
            let m = Metrics::new(dpi);
            assert_eq!(rect.left, m.px(Metrics::RESIZE_RIM));
            assert_eq!(rect.right, 1200 - m.px(Metrics::RESIZE_RIM));
            assert_eq!(rect.bottom, 800 - m.px(Metrics::RESIZE_RIM));
            assert!(rect.top > m.px(Metrics::STRIP_HEIGHT + Metrics::TAB_ROW_HEIGHT));
        }
    }
}
