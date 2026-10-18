//! Pure chrome geometry and hit-testing (glass UI work in progress; see docs/superpowers/specs/2026-10-09-bowser-glass-ui-design.md)
//!
//! Coordinates are client-relative physical pixels. The chrome has one of four arrangements
//! ([`TabLayout`]); the caption buttons are always at the top right, 30 logical pixels tall.
//!
//! `ToolbarTop` (the default), top to bottom:
//!
//! ```text
//! +-------------------------------------------------------------+
//! | toolbar: navigation, omnibox, main menu, captions           |
//! | 2 px row gap                                                |
//! | tab row: tabs, tab list, "+" at the far right end           |
//! | 2 px margin                                                 |
//! +-------------------------------------------------------------+
//! | viewport (page content, to the bottom edge)                 |
//! ```
//!
//! `TabsTop` puts the tab row (tabs, tab list, "+", captions) in the top 30 px and the toolbar
//! (navigation, omnibox, main menu) below it. `SingleRow` has one 30 px row: navigation, a
//! limited-width omnibox, the main menu, the tabs, then tab list, "+" and captions. `Vertical` keeps
//! the toolbar row but has no tab row at all: the tabs are in the left sidebar and the viewport
//! starts right under the toolbar.
//!
//! Tab overflow: tabs shrink evenly to favicon circles. The visible window onto the row shifts just
//! enough to keep the active tab visible. The tab list button appears at the end of the tab zone
//! once tabs overflow, so it never scrolls with the tabs.

use windows::Win32::Foundation::RECT;

use crate::chrome::rail::{self, RailKey, RailView};
use crate::layout_prefs::BarPosition; // --- layout (thread T6) ---
use crate::settings::{SidePanelPosition, TabLayout};

/// Logical (96 dpi) chrome metrics, scaled to physical pixels with [`Metrics::px`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metrics {
    dpi: u32,
}

impl Metrics {
    /// Height of the toolbar controls row below the top inset.
    pub const STRIP_HEIGHT: i32 = 30;
    /// Clear draggable space above the toolbar controls.
    pub const TOP_MARGIN: i32 = 0;
    /// Clear space between the toolbar controls and the tab row.
    pub const STRIP_GAP: i32 = 2;
    /// Height of the full-width tab row under the toolbar.
    pub const TAB_ROW_HEIGHT: i32 = 28;
    // --- two-level stacks (T10b) --- the second tab row; ui tokens.css `--bw-stack-row-h` agrees.
    /// Height of the second row of two-level stacks: the main tab row's own height.
    pub const STACK_ROW_HEIGHT: i32 = Self::TAB_ROW_HEIGHT;
    /// Its height with "Compact second row": islands 20 tall instead of 26.
    pub const STACK_ROW_COMPACT_HEIGHT: i32 = 22;
    // --- end two-level stacks ---
    pub const ISLAND_HEIGHT: i32 = Self::OMNIBOX_HEIGHT;
    pub const TAB_RADIUS: i32 = 9;
    /// The omnibox stays this wide at most and is centred in the toolbar.
    pub const OMNIBOX_MAX_WIDTH: i32 = 624;
    /// In the single-row layout the omnibox is this wide, next to navigation, until the row gets
    /// tight. It then shrinks to the minimum before the tabs lose their share.
    pub const SINGLE_ROW_OMNIBOX_WIDTH: i32 = 320;
    pub const SINGLE_ROW_OMNIBOX_MIN_WIDTH: i32 = 160;
    /// Width the single-row layout tries to keep for the tabs while the omnibox gives way.
    pub const SINGLE_ROW_TABS_MIN_WIDTH: i32 = 160;
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
    /// Extra clear space between the chrome above the page and the viewport, on top of `OUTER_MARGIN`.
    /// It comes off the viewport, so the page host starts this much lower.
    pub const VIEWPORT_GAP: i32 = 1;
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
    /// Which arrangement this geometry is for.
    pub variant: TabLayout,
    /// Toolbar row: navigation, omnibox and the main menu, plus the caption buttons in the layouts
    /// that have the toolbar at the top (and "+" in `Vertical`). In `SingleRow` the tab row sits
    /// inside it.
    pub strip: Rect,
    /// Row the tabs live in, with the tab list at its end. Empty in the vertical layout. In
    /// `ToolbarTop` it is the full-width row under the toolbar and contains "+" at its right end; in
    /// `TabsTop` and `SingleRow` it stops before "+" and the caption buttons, and in `SingleRow` it
    /// starts right of the main menu.
    pub tab_row: Rect,
    /// Horizontal span that tabs may occupy. Tabs are clipped to it, and anything past it is hidden.
    pub tab_zone: Rect,
    /// Opens the tab list. Sits at the end of the tab row once tabs overflow.
    pub tab_menu_button: Rect,
    /// Opens a new tab. Sits at the right end of the tab row in `ToolbarTop`, left of the caption
    /// buttons beside the tabs in `TabsTop` and `SingleRow`, and in the toolbar in `Vertical`.
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
    // --- layout (thread T6) ---
    /// A row above everything else holding only the window buttons and the page title: shown when
    /// the user asks for one with the tabs on a side, or when neither a tab row nor the toolbar is
    /// at the top to hold the window buttons. Empty otherwise.
    pub title_bar: Rect,
    /// Where the page area starts and ends vertically before the bookmarks bar takes its share:
    /// `viewport.top` as laid out, and the top of the rows along the bottom edge (`client.bottom`
    /// when there are none). The native paint fills the chrome bands between these and the edges.
    pub top_chrome: i32,
    pub bottom_chrome: i32,
    // --- end layout ---
    /// --- two-level stacks (T10b) --- the second tab row, under the tab row (the toolbar row in
    /// `SingleRow`): the tabs of the stack in use. Empty unless the arrangement asks for one.
    pub stack_row: Rect,
    tab_count: usize,
    inactive_tab_width: i32,
    active_tab_width: i32,
    active_index: usize,
    tab_left: i32,
    /// Right edge the tab list button may use: the end of the tab row.
    tab_controls_right: i32,
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

/// Height of the painted chrome for `variant`: the top inset, the toolbar, the tab row (when the
/// layout has one above the page) and the margin below it. This is where `Layout::viewport` starts.
pub const fn chrome_height(variant: TabLayout, dpi: u32) -> i32 {
    let m = Metrics::new(dpi);
    let top_row = m.px(Metrics::TOP_MARGIN) + m.px(Metrics::STRIP_HEIGHT);
    let rows = match variant {
        TabLayout::ToolbarTop => top_row + m.px(Metrics::STRIP_GAP) + m.px(Metrics::TAB_ROW_HEIGHT),
        // The top row holds the tabs and the captions; the toolbar is a second, equally tall row.
        TabLayout::TabsTop => top_row + m.px(Metrics::STRIP_HEIGHT),
        // One row, or a toolbar with the tabs in a sidebar or along the bottom edge: nothing
        // reserved for a tab row at the top.
        TabLayout::SingleRow | TabLayout::Vertical | TabLayout::VerticalRight | TabLayout::TabsBottom => top_row,
    };
    rows + m.px(Metrics::OUTER_MARGIN) + m.px(Metrics::VIEWPORT_GAP)
}

// --- layout (thread T6) ---
/// Height of the chrome along the bottom edge for `variant`: the tab row of `TabsBottom` with its
/// margin below and the gap above it; zero for every other variant. The viewport ends this far
/// above the client's bottom edge.
pub const fn bottom_chrome_height(variant: TabLayout, dpi: u32) -> i32 {
    let m = Metrics::new(dpi);
    match variant {
        TabLayout::TabsBottom => {
            m.px(Metrics::TAB_ROW_HEIGHT) + 2 * m.px(Metrics::OUTER_MARGIN) + m.px(Metrics::VIEWPORT_GAP)
        }
        _ => 0,
    }
}
// --- end layout ---

/// Logical width of the left rail. The rail is a pill, rounded at both ends. It is exactly as wide as the omnibox is tall
/// and holds 22-pixel buttons with a 2-pixel margin either side, which fits the 16-pixel icons.
pub const LEFT_RAIL_WIDTH: i32 = Metrics::OMNIBOX_HEIGHT;

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
    let top_margin = m.px(Metrics::TOP_MARGIN);
    let height = client_h.max(0);
    Rect::new(
        margin,
        top_margin.saturating_add(margin).min(height),
        margin + m.px(LEFT_RAIL_WIDTH),
        (height - margin).max(top_margin.saturating_add(margin).min(height)),
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
    compute_in_frame_for(TabLayout::ToolbarTop, client_w, client_h, dpi, tab_count, active_index, maximised)
}

/// [`compute_in_frame`] for any horizontal-family `variant`. `Vertical` has its own constructor,
/// [`compute_vertical_in_frame`], because the sidebar decides where its toolbar starts.
pub fn compute_in_frame_for(
    variant: TabLayout,
    client_w: i32,
    client_h: i32,
    dpi: u32,
    tab_count: usize,
    active_index: usize,
    maximised: bool,
) -> Layout {
    compute_in_frame_for_with(variant, client_w, client_h, dpi, tab_count, active_index, LayoutOptions { maximised, centred: true })
}

/// Window-state and address-bar preferences shared by chrome layout constructors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayoutOptions {
    pub maximised: bool,
    pub centred: bool,
}

/// [`compute_in_frame_for`] with the omnibox centred in the toolbar only when `centred` is set; off,
/// the omnibox sits left-aligned next to navigation, matching `centred_address_bar`.
pub fn compute_in_frame_for_with(
    variant: TabLayout,
    client_w: i32,
    client_h: i32,
    dpi: u32,
    tab_count: usize,
    active_index: usize,
    options: LayoutOptions,
) -> Layout {
    let LayoutOptions { maximised, centred } = options;
    let client_w = client_w.max(0);
    let client_h = client_h.max(0);
    let inset = left_rail_inset(dpi);
    let mut layout = compute_for_with(variant, client_w - inset, client_h, dpi, tab_count, active_index, LayoutOptions { maximised, centred })
    .shifted_right(inset);
    layout.client = Rect::new(0, 0, client_w, client_h);
    layout.rail = left_rail(client_h, dpi);
    layout
}

/// Rounded page host bounds in client-relative physical pixels. The margins preserve the native
/// resize rim while keeping the content inset aligned to the device-pixel chrome metrics.
pub fn content_rect(variant: TabLayout, client_w: i32, client_h: i32, dpi: u32, maximised: bool) -> Rect {
    let m = Metrics::new(dpi);
    let width = client_w.max(0);
    let height = client_h.max(0);
    let top = chrome_height(variant, dpi).min(height);
    let rim = if maximised {
        0
    } else {
        m.px(Metrics::RESIZE_RIM).min(width / 2)
    };
    // --- layout (thread T6) --- a tab row along the bottom edge takes its band off the page.
    let bottom_inset = rim.max(bottom_chrome_height(variant, dpi));
    Rect::new(rim, top, (width - rim).max(rim), (height - bottom_inset).max(top))
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

/// The stacked form of [`split_pane_layout`]: `left` is the top page, `right` the bottom page and
/// `divider` the horizontal gap between them, whose top edge is `divider_top` clamped so both pages
/// keep at least `min_pane_height` pixels. `None` when the content is too short for two pages.
pub fn split_pane_layout_stacked(
    content: Rect,
    divider_top: i32,
    min_pane_height: i32,
    divider_height: i32,
) -> Option<SplitPaneLayout> {
    let min_pane_height = min_pane_height.max(1);
    let divider_height = divider_height.max(1);
    let min_top = content.top.saturating_add(min_pane_height);
    let max_top = content
        .bottom
        .saturating_sub(min_pane_height.saturating_add(divider_height));
    if max_top < min_top || content.right <= content.left {
        return None;
    }
    let divider_top = divider_top.clamp(min_top, max_top);
    let divider_bottom = divider_top.saturating_add(divider_height);
    Some(SplitPaneLayout {
        left: Rect::new(content.left, content.top, content.right, divider_top),
        divider: Rect::new(content.left, divider_top, content.right, divider_bottom),
        right: Rect::new(content.left, divider_bottom, content.right, content.bottom),
    })
}

/// A shown split of two to four panes as the overlay draws it (`split_view::geometry` in [`Rect`]s).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SplitPanes {
    /// Each shown pane by `split_view::Pane::index`; a pane hidden by the collapse rule has none.
    pub panes: [Option<Rect>; 4],
    /// The focused pane's index.
    pub focused: usize,
    /// The gaps: `[primary, cross of the first half, cross of the second half]`.
    pub dividers: [Option<Rect>; 3],
    /// Where the primary gap meets the cross gaps' line (a 2 x 2's centre, or a T's junction).
    pub crossing: Option<Rect>,
}

impl SplitPanes {
    pub fn from_geometry(geometry: &crate::split_view::Geometry, view: crate::split_view::SplitView) -> Self {
        let rect = |(l, t, r, b): crate::split_view::Bounds| Rect::new(l, t, r, b);
        let primary = rect(geometry.primary);
        let crossing = geometry.cross.iter().flatten().next().map(|&(l, t, r, b)| match view.orientation {
            crate::split_view::Orientation::SideBySide => Rect::new(primary.left, t, primary.right, b),
            crate::split_view::Orientation::Stacked => Rect::new(l, primary.top, r, primary.bottom),
        });
        Self {
            panes: geometry.panes.0.map(|pane| pane.map(rect)),
            focused: view.focused.index(),
            dividers: [Some(primary), geometry.cross[0].map(rect), geometry.cross[1].map(rect)],
            crossing,
        }
    }
    /// The pane at `(x, y)`, or the nearest shown pane (a click in a gap or on a pane's rim).
    pub fn pane_near(&self, x: i32, y: i32) -> Option<crate::split_view::Pane> {
        let mut map = crate::split_view::PaneMap::default();
        for pane in crate::split_view::Pane::ALL {
            map.set(pane, self.panes[pane.index()].map(|r| (r.left, r.top, r.right, r.bottom)));
        }
        crate::split_view::Geometry { panes: map, ..Default::default() }.pane_near(x, y)
    }
}

/// The overlay layout of `view` inside `content`: the primary gap's leading edge at `primary_at`,
/// the cross gaps' at `cross_at`, minimum pane sizes and the gap from `split_view`, scaled by `m`.
/// `None` when the content cannot hold two panes (the focused pane then shows alone).
pub fn split_panes(content: Rect, view: crate::split_view::SplitView, primary_at: i32, cross_at: i32, m: Metrics) -> Option<SplitPanes> {
    use crate::split_view::{geometry, MIN_PANE_DIP, MIN_PANE_HEIGHT_DIP};
    let page = (content.left, content.top, content.right, content.bottom);
    // --- tab stacks (T10) --- the gap follows "Remove the space around tiles" (`stacks::gap_dip`).
    let gap = m.px(crate::stacks::gap_dip());
    let geometry = geometry(view, page, primary_at, cross_at, m.px(MIN_PANE_DIP), m.px(MIN_PANE_HEIGHT_DIP), gap)?;
    Some(SplitPanes::from_geometry(&geometry, view))
}

/// [`split_pane_layout`] or [`split_pane_layout_stacked`] for `orientation`. `divider` is the gap's
/// leading edge along the split axis (x side by side, y stacked); `min_pane` is that axis' minimum.
pub fn split_pane_layout_oriented(
    content: Rect,
    orientation: crate::split_view::Orientation,
    divider: i32,
    min_pane: i32,
    gap: i32,
) -> Option<SplitPaneLayout> {
    match orientation {
        crate::split_view::Orientation::SideBySide => split_pane_layout(content, divider, min_pane, gap),
        crate::split_view::Orientation::Stacked => split_pane_layout_stacked(content, divider, min_pane, gap),
    }
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
    compute_for(TabLayout::ToolbarTop, client_w, client_h, dpi, tab_count, active_index, maximised)
}

/// [`compute_with_active`] for `variant`. `Vertical` is laid out like `ToolbarTop` and then loses
/// its tab row; [`compute_vertical_in_frame`] adds the sidebar.
pub fn compute_for(
    variant: TabLayout,
    client_w: i32,
    client_h: i32,
    dpi: u32,
    tab_count: usize,
    active_index: usize,
    maximised: bool,
) -> Layout {
    compute_for_with(variant, client_w, client_h, dpi, tab_count, active_index, LayoutOptions { maximised, centred: true })
}

/// [`compute_for`] with the omnibox centred in the toolbar only when `centred` is set; off, the
/// omnibox sits left-aligned next to navigation, matching `centred_address_bar`. `SingleRow` and
/// `TabsTop` already have no dead space to centre in, so `centred` only changes `ToolbarTop` and
/// `Vertical`.
pub fn compute_for_with(
    variant: TabLayout,
    client_w: i32,
    client_h: i32,
    dpi: u32,
    tab_count: usize,
    active_index: usize,
    options: LayoutOptions,
) -> Layout {
    let LayoutOptions { maximised, centred } = options;
    let m = Metrics::new(dpi);
    let client_w = client_w.max(0);
    let client_h = client_h.max(0);

    let margin = m.px(Metrics::OUTER_MARGIN);
    let gap = m.px(Metrics::ISLAND_GAP);
    let strip_h = m.px(Metrics::STRIP_HEIGHT);
    let top_margin = m.px(Metrics::TOP_MARGIN);
    let tab_row_h = m.px(Metrics::TAB_ROW_HEIGHT);
    let new_tab_size = m.px(Metrics::NEW_TAB_SIZE);
    let list_size = m.px(Metrics::TAB_MENU_SIZE);
    let max_w = m.px(Metrics::TAB_MAX_WIDTH);
    let min_w = m.px(Metrics::TAB_MIN_WIDTH);

    let client = Rect::new(0, 0, client_w, client_h);
    let caption_h = top_margin + strip_h;
    let caption_w = 3 * m.px(Metrics::CAPTION_BUTTON_WIDTH);
    let caption_left = (client_w - caption_w).max(0);
    let caption_area = Rect::new(caption_left, 0, client_w, caption_h);

    // The toolbar row. In `TabsTop` it sits under the top row, which holds the tabs and the captions.
    let strip_top = if variant == TabLayout::TabsTop { caption_h } else { top_margin };
    let strip = Rect::new(0, strip_top, client_w, strip_top + strip_h);

    // Navigation sits at the left of the toolbar row in every layout.
    let nav_size = m.px(Metrics::NAV_BUTTON_SIZE);
    let nav_gap = m.px(Metrics::NAV_BUTTON_GAP);
    let nav_left = m.px(Metrics::NAV_LEFT_INSET);
    let nav_top = strip.top + (strip_h - nav_size) / 2;
    let nav_button = |slot: i32| {
        let left = match slot {
            0 => nav_left,
            1 => nav_left + nav_size,
            _ => nav_left + 2 * nav_size + nav_gap,
        };
        Rect::new(left, nav_top, left + nav_size, nav_top + nav_size)
    };
    let nav_right = nav_left + 3 * nav_size + nav_gap;
    let omnibox_min = nav_right + gap;
    let menu_w = m.px(Metrics::MENU_BUTTON_SIZE);
    let omnibox_h = m.px(Metrics::OMNIBOX_HEIGHT);
    let omnibox_top = strip.top + (strip_h - omnibox_h) / 2;

    // The tabs start at `tabs_left` and the tab list button may reach `row_right`. `tab_row` is the
    // row they live in, and `new_tab_button` is "+" wherever the layout puts it.
    let tabs_left;
    let row_right;
    let tab_row;
    let new_tab_button;
    let omnibox;
    let main_menu_button;
    if variant == TabLayout::SingleRow {
        // One row: navigation, then an omnibox of limited width and the main menu right of it, then
        // the tabs, then "+" just left of the captions. The omnibox shrinks to its minimum before
        // the tabs are squeezed, and "+" and the menu drop out before the omnibox goes below it.
        let right_limit = (caption_left - margin).max(omnibox_min);
        let avail = right_limit - omnibox_min;
        let menu_total = menu_w + gap;
        let plus_total = new_tab_size + gap;
        let wanted = m.px(Metrics::SINGLE_ROW_OMNIBOX_WIDTH).min(
            (avail - menu_total - plus_total - m.px(Metrics::SINGLE_ROW_TABS_MIN_WIDTH))
                .max(m.px(Metrics::SINGLE_ROW_OMNIBOX_MIN_WIDTH)),
        );
        let plus_fits = wanted + menu_total + plus_total <= avail;
        let menu_fits = wanted + menu_total <= avail;
        let omnibox_w = wanted.min(avail - if menu_fits { menu_total } else { 0 });
        omnibox = Rect::new(omnibox_min, omnibox_top, omnibox_min + omnibox_w, omnibox_top + omnibox_h);
        main_menu_button = if menu_fits {
            let left = omnibox.right + gap;
            Rect::new(left, omnibox_top, left + menu_w, omnibox_top + omnibox_h)
        } else {
            Rect::default()
        };
        let tabs_start = if menu_fits { main_menu_button.right } else { omnibox.right };
        tabs_left = tabs_start + gap;
        let plus_left = right_limit - new_tab_size;
        if plus_fits {
            new_tab_button = Rect::new(
                plus_left,
                strip.top + (strip_h - new_tab_size) / 2,
                right_limit,
                strip.top + (strip_h + new_tab_size) / 2,
            );
            row_right = (plus_left - gap).max(tabs_left);
        } else {
            new_tab_button = Rect::default();
            row_right = right_limit.max(tabs_left);
        }
        // The islands share the omnibox's top and height.
        let tab_top = omnibox_top - m.px(Metrics::TAB_ROW_HEIGHT - Metrics::ISLAND_HEIGHT) / 2;
        tab_row = Rect::new(tabs_start, tab_top, row_right.max(tabs_start), tab_top + tab_row_h);
    } else {
        // Toolbar: navigation on the left, the omnibox centred in the window with the main-menu
        // bubble directly right of it. The omnibox gives way first when the two sides need it, so
        // the bubble stays stuck to the address pill. In `TabsTop` the toolbar row has the whole
        // width, since "+" and the captions are in the top row. In `ToolbarTop` "+" is in the tab row.
        let mut right_edge = if variant == TabLayout::TabsTop { client_w - margin } else { caption_left - margin };
        let mut toolbar_fits = true;
        // "+" drops out from the left once it would crowd navigation. This keeps it pinned to the
        // right edge and avoids a middle gap.
        let mut place = |size: i32| {
            let left = right_edge - size;
            if toolbar_fits && left >= nav_right + gap {
                right_edge = left - gap;
                let top = strip.top + (strip_h - size) / 2;
                Rect::new(left, top, left + size, top + size)
            } else {
                toolbar_fits = false;
                Rect::default()
            }
        };
        // In `ToolbarTop` (and `TabsBottom`) "+" is at the end of the tab row instead, so the
        // toolbar keeps its width.
        let strip_new_tab = if matches!(variant, TabLayout::TabsTop | TabLayout::ToolbarTop | TabLayout::TabsBottom) { Rect::default() } else { place(new_tab_size) };
        let group_right = right_edge.max(omnibox_min);
        // The bubble only fits when the omnibox keeps a non-negative width beside it.
        let menu_fits = toolbar_fits && group_right - menu_w - gap >= omnibox_min;
        let omnibox_max = if menu_fits {
            group_right - menu_w - gap
        } else {
            group_right
        };
        // `TabsTop` has no spare room to centre in: the omnibox fills the row from navigation to the
        // menu, so there is no dead space either side of it.
        let (omnibox_w, omnibox_left) = if variant == TabLayout::TabsTop {
            let width = (omnibox_max - omnibox_min).max(0);
            (width, omnibox_min)
        } else {
            let width = (omnibox_max - omnibox_min).min(m.px(Metrics::OMNIBOX_MAX_WIDTH));
            let left = if centred {
                (client_w / 2 - width / 2).clamp(omnibox_min, (omnibox_max - width).max(omnibox_min))
            } else {
                // `centred_address_bar` off: left-aligned next to navigation instead of centred.
                omnibox_min
            };
            (width, left)
        };
        omnibox = Rect::new(
            omnibox_left,
            omnibox_top,
            omnibox_left + omnibox_w,
            omnibox_top + omnibox_h,
        );
        main_menu_button = if menu_fits {
            let left = omnibox.right + gap;
            Rect::new(left, omnibox_top, left + menu_w, omnibox_top + omnibox_h)
        } else {
            Rect::default()
        };

        tabs_left = margin;
        if variant == TabLayout::TabsTop {
            // The top row: tabs from the left edge, "+" just left of the captions, and the tab
            // list at the end of the tabs. The row starts at the island margin so the islands sit
            // just below the window's top resize edge.
            let tab_top = top_margin + (m.px(Metrics::ISLAND_MARGIN) - m.px(Metrics::TAB_ROW_HEIGHT - Metrics::ISLAND_HEIGHT) / 2).max(0);
            let right_limit = (caption_left - margin).max(tabs_left);
            let plus_left = right_limit - new_tab_size;
            // "+" drops out once it would leave less than one minimum-width tab beside it.
            if plus_left - gap >= tabs_left + min_w {
                new_tab_button = Rect::new(
                    plus_left,
                    tab_top + (tab_row_h - new_tab_size) / 2,
                    right_limit,
                    tab_top + (tab_row_h + new_tab_size) / 2,
                );
                row_right = plus_left - gap;
            } else {
                new_tab_button = Rect::default();
                row_right = right_limit;
            }
            tab_row = Rect::new(0, tab_top, row_right, tab_top + tab_row_h);
        } else {
            // --- layout (thread T6) --- `TabsBottom` is the full-width row of `ToolbarTop` laid
            // along the bottom edge: its margin below, and the viewport ends above it.
            let tab_top = if variant == TabLayout::TabsBottom {
                (client_h - margin - tab_row_h).max(strip.bottom + m.px(Metrics::STRIP_GAP))
            } else {
                strip.bottom + m.px(Metrics::STRIP_GAP)
            };
            if matches!(variant, TabLayout::ToolbarTop | TabLayout::TabsBottom) {
                // The full-width tab row: tabs, tab list, then "+" pinned to the right end. Like in
                // `TabsTop`, "+" drops out once it would leave less than one minimum-width tab.
                let right_limit = (client_w - margin).max(tabs_left);
                let plus_left = right_limit - new_tab_size;
                if plus_left - gap >= tabs_left + min_w {
                    new_tab_button = Rect::new(
                        plus_left,
                        tab_top + (tab_row_h - new_tab_size) / 2,
                        right_limit,
                        tab_top + (tab_row_h + new_tab_size) / 2,
                    );
                    row_right = plus_left - gap;
                } else {
                    new_tab_button = Rect::default();
                    row_right = right_limit;
                }
            } else {
                // `Vertical`: "+" stays in the toolbar row.
                new_tab_button = strip_new_tab;
                row_right = (client_w - margin).max(tabs_left);
            }
            tab_row = Rect::new(0, tab_top, client_w, tab_top + tab_row_h);
        }
    }

    // The tab overflow picker occupies the trailing edge of the tab row, beside the tab slots.
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

    let viewport = content_rect(variant, client_w, client_h, dpi, maximised);

    let mut layout = Layout {
        metrics: m,
        maximised,
        client,
        variant,
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
        // --- layout (thread T6) ---
        title_bar: Rect::default(),
        top_chrome: viewport.top,
        bottom_chrome: if variant == TabLayout::TabsBottom { viewport.bottom } else { client_h },
        stack_row: Rect::default(), // --- two-level stacks (T10b) ---
        // --- end layout ---
        tab_count,
        inactive_tab_width,
        active_tab_width,
        active_index,
        tab_left: tabs_left,
        tab_controls_right: row_right,
        first_visible,
        visible_count,
        island_top,
        island_bottom,
        pill_group_slots: [0; GROUP_SLOT_WORDS],
        ball_group_slots: [0; GROUP_SLOT_WORDS],
        stack_group_slots: [0; GROUP_SLOT_WORDS],
    };
    if variant.is_vertical() {
        // The sidebar holds the tabs: no row, no zone and no overflow picker, which would otherwise
        // sit in a row that is not there. The caller replaces the viewport with the sidebar's.
        layout.tab_row = Rect::default();
        layout.tab_zone = Rect::default();
        layout.tab_menu_button = Rect::default();
        layout.first_visible = 0;
        layout.visible_count = tab_count;
    }
    layout
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
    /// --- two-level stacks (T10b) --- a tab of the second row (by its place in that row), and its
    /// close button.
    StackTab(usize),
    StackTabClose(usize),
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
        self.tab_controls_right += dx;
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
        // The vertical sidebar has no tab row: fitting group slots would place an overflow picker above the window.
        if self.tab_row.is_empty() {
            return self;
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
                let list_size = m.px(Metrics::TAB_MENU_SIZE);
                let row_right = self.tab_controls_right.max(self.tab_zone.left);
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
        // The sidebar holds the tabs, so none of them has an island in the top chrome.
        if index >= self.tab_count || self.variant.is_vertical() {
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

    /// Rects of the empty chrome that drag the window. This is the top rows (the inset above the
    /// toolbar, the toolbar and the tab row) minus their controls and visible tab islands.
    /// [`hit_test`] returns `CaptionArea` for exactly these points.
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
            // Drag rects are never under the rail, which wins every hit test over its own area.
            self.rail,
        ]
        .into_iter()
        .chain(self.tabs().map(|island| island.intersect(self.tab_zone)))
        .filter(|control| !control.is_empty())
        .collect();
        controls.sort_by_key(|control| control.left);

        let mut rects = Vec::new();
        // The band above the toolbar: the inset, or in `TabsTop` the row of tabs and captions.
        // --- layout (thread T6) --- never past where the page starts: a toolbar along the bottom
        // edge has the page above it, and that is not a drag band.
        let above = Rect::new(self.client.left, self.client.top, self.client.right, self.strip.top.min(self.top_chrome));
        // --- two-level stacks (T10b) --- the second tab row is tabs, never a drag band: each band
        // is swept above and below it.
        let stack_row = self.stack_row;
        let sweep = |band: Rect, rects: &mut Vec<Rect>| {
            if stack_row.is_empty() || band.intersect(stack_row).is_empty() {
                append_drag_regions(band, &controls, rects);
                return;
            }
            append_drag_regions(Rect::new(band.left, band.top, band.right, stack_row.top.max(band.top)), &controls, rects);
            append_drag_regions(Rect::new(band.left, stack_row.bottom.min(band.bottom), band.right, band.bottom), &controls, rects);
        };
        sweep(above, &mut rects);
        sweep(self.strip, &mut rects);
        // `SingleRow` and `TabsTop` keep the tab row inside a band swept above.
        if !above.contains_rect(self.tab_row) && !self.strip.contains_rect(self.tab_row) {
            sweep(self.tab_row, &mut rects);
        }
        // --- end two-level stacks ---
        rects
    }

    // --- two-level stacks (T10b) ---
    /// The islands of the second tab row for `count` tabs, left to right: the row shared evenly,
    /// an island gap apart and a pixel inside the row's top and bottom, never narrower than a tab
    /// can be ([`Metrics::TAB_MIN_WIDTH`]). Islands past the row's right end are left out. Empty
    /// without a second row.
    pub fn stack_tabs(&self, count: usize) -> Vec<Rect> {
        let row = self.stack_row;
        if row.is_empty() || count == 0 {
            return Vec::new();
        }
        let m = self.metrics;
        let gap = m.px(Metrics::ISLAND_GAP);
        let inset = m.px(1);
        let left = row.left + m.px(Metrics::ISLAND_MARGIN);
        let right = row.right - m.px(Metrics::ISLAND_MARGIN);
        let n = i32::try_from(count).unwrap_or(i32::MAX);
        let width = ((right - left - gap * (n - 1)) / n).max(m.px(Metrics::TAB_MIN_WIDTH));
        (0..n)
            .map(|index| left + index * (width + gap))
            .take_while(|x| x + width <= right)
            .map(|x| Rect::new(x, row.top + inset, x + width, row.bottom - inset))
            .collect()
    }

    /// Puts the second tab row of two-level stacks under the row that holds the tabs (the toolbar
    /// row in `SingleRow`). At the top, a toolbar under that row moves down by the second row and
    /// its gap; along the bottom edge the tab row moves up by as much and the second row takes the
    /// bottom of its place, so whatever sits below (the toolbar) stays put. Nothing without a tab
    /// row (the sidebars) or with [`StackRow::None`].
    fn with_stack_row(self, row: StackRow) -> Self {
        let placed = self.place_stack_row(row);
        // A window too short for it keeps its rows as they were: the second row never covers the
        // toolbar or the title row at the other edge.
        let clear = [placed.strip, placed.tab_row, placed.title_bar]
            .iter()
            .all(|other| other.is_empty() || other.intersect(placed.stack_row).is_empty());
        let tabs_clear = placed.tab_row.top >= placed.title_bar.bottom
            && (placed.strip.is_empty() || placed.strip.contains_rect(placed.tab_row) || placed.strip.intersect(placed.tab_row).is_empty());
        // Every row stays on its edge (the page area is settled from which rows are at the top).
        let middle = self.client.height() / 2;
        let sides_kept = (placed.strip.top < middle) == (self.strip.top < middle) && (placed.tab_row.top < middle) == (self.tab_row.top < middle);
        if clear && tabs_clear && sides_kept && placed.stack_row.bottom <= placed.client.bottom { placed } else { self }
    }

    fn place_stack_row(mut self, row: StackRow) -> Self {
        let m = self.metrics;
        let height = m.px(row.height_dip());
        if height == 0 || self.tab_row.is_empty() {
            return self;
        }
        let gap = m.px(Metrics::STRIP_GAP);
        let band = gap + height;
        let holder = if self.strip.contains_rect(self.tab_row) { self.strip } else { self.tab_row };
        if self.tab_row.top < self.client.height() / 2 {
            // A row gap under the tabs, and never inside the toolbar row that holds them.
            let top = (self.tab_row.bottom + gap).max(holder.bottom);
            if !self.strip.is_empty() && self.strip.top >= holder.bottom && self.strip.top < self.client.height() / 2 {
                self.shift_toolbar(band);
            }
            self.stack_row = Rect::new(self.tab_row.left, top, self.tab_row.right, top + height);
        } else {
            let bottom = holder.bottom;
            if holder == self.strip {
                self.shift_toolbar(-band);
            } else {
                self.shift_tab_row(-band);
            }
            self.stack_row = Rect::new(self.tab_row.left, bottom - height, self.tab_row.right, bottom);
        }
        self
    }
    // --- end two-level stacks ---
}

/// Computes vertical-mode chrome using the native sidebar's width as the toolbar's left origin.
/// The tab row remains absent; the viewport starts at the sidebar's right edge.
pub fn compute_vertical_in_frame(
    client_w: i32,
    client_h: i32,
    dpi: u32,
    tab_count: usize,
    active_index: usize,
    maximised: bool,
    sidebar_width: i32,
) -> (Layout, crate::vertical_tabs::SidebarLayout) {
    compute_vertical_in_frame_with(client_w, client_h, dpi, tab_count, active_index, sidebar_width, LayoutOptions { maximised, centred: true })
}

/// [`compute_vertical_in_frame`] with the omnibox centred in the toolbar only when `centred` is set.
pub fn compute_vertical_in_frame_with(
    client_w: i32,
    client_h: i32,
    dpi: u32,
    tab_count: usize,
    active_index: usize,
    sidebar_width: i32,
    options: LayoutOptions,
) -> (Layout, crate::vertical_tabs::SidebarLayout) {
    let LayoutOptions { maximised, centred } = options;
    let sidebar = crate::vertical_tabs::SidebarLayout::compute_in_frame(
        client_w, client_h, dpi, sidebar_width, maximised,
    );
    let offset = sidebar.sidebar.right;
    let mut layout = compute_for_with(TabLayout::Vertical, client_w.saturating_sub(offset), client_h, dpi, tab_count, active_index, LayoutOptions { maximised, centred })
        .shifted_right(offset);
    layout.client = Rect::new(0, 0, client_w.max(0), client_h.max(0));
    layout.rail = left_rail(client_h, dpi);
    layout.viewport = sidebar.viewport;
    (layout, sidebar)
}

fn append_drag_regions(band: Rect, controls: &[Rect], rects: &mut Vec<Rect>) {
    // A zero-height band (the inset above the toolbar when TOP_MARGIN is 0) has nothing to drag.
    if band.is_empty() {
        return;
    }
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
    // An HTML region wins over every native rule, the rail included: its pixels belong to a page.
    if html_regions.iter().any(|region| region.contains(x, y)) {
        return Hit::None;
    }
    if layout.rail.contains(x, y) {
        let key = rail.and_then(|view| {
            rail::geometry(layout.rail, view.entries, view.scroll, layout.metrics.dpi()).hit(x, y)
        });
        return key.map_or(Hit::RailBackground, Hit::Rail);
    }
    // The margin left of the toolbar, beside the rail, drags the window like the rest of the strip.
    // Below the toolbar that margin is outside the chrome and takes no hit.
    // The band above the toolbar ends where the page starts, and a toolbar along
    // the bottom edge has its own band: the rows over the page are not drag bands
    // (`drag_region_rects` sweeps the same two bands).
    let above_bottom = layout.strip.top.min(layout.top_chrome);
    let in_strip_band = y >= layout.strip.top && y < layout.strip.bottom;
    if (y < above_bottom || in_strip_band) && x < layout.strip.left {
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
    if layout.caption_area.contains(x, y) {
        if layout.caption_button(Hit::CaptionClose).contains(x, y) {
            return Hit::CaptionClose;
        }
        if layout.caption_button(Hit::CaptionMaximize).contains(x, y) {
            return Hit::CaptionMaximize;
        }
        return Hit::CaptionMinimize;
    }
    // --- layout (thread T6) --- the title row is all drag handle outside its buttons.
    if layout.title_bar.contains(x, y) {
        return Hit::CaptionArea;
    }
    // Only above the page: with the toolbar at the bottom everything over it is
    // the page and its margins, not caption.
    if y >= 0 && y < layout.strip.top.min(layout.top_chrome) {
        return Hit::CaptionArea;
    }
    if !layout.strip.contains(x, y) {
        return Hit::None;
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
// --- layout (thread T6) ---
/// How the chrome is arranged, from the layout settings: which tab layout and whether the tab bar
/// shows at all, which edge the toolbar sits on and whether it shows, and whether a title row is
/// wanted with the tabs on a side. [`compute_arranged_in_frame`] builds the geometry from it; the
/// window, the page host bounds and the native paint all read the same result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Arrangement {
    pub tabs: TabLayout,
    pub tab_bar: bool,
    pub address_bar: BarPosition,
    pub address_bar_visible: bool,
    pub title_bar: bool,
    /// --- two-level stacks (T10b) --- the second tab row, set with [`Arrangement::with_stack_row`].
    pub stack_row: StackRow,
}

impl Default for Arrangement {
    fn default() -> Self {
        Self { tabs: TabLayout::ToolbarTop, tab_bar: true, address_bar: BarPosition::Top, address_bar_visible: true, title_bar: false, stack_row: StackRow::None }
    }
}

// --- two-level stacks (T10b) ---
/// The second tab row of two-level stacks (`tab_options::StackStyle::TwoLevel`): none, the tab
/// row's own height, or the compact one. It is laid out only while the tab in use is in a stack
/// (`App::stack_row`), so every other case costs nothing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StackRow {
    #[default]
    None,
    Regular,
    Compact,
}

impl StackRow {
    /// The row's height in DIP (0 for none).
    pub const fn height_dip(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Regular => Metrics::STACK_ROW_HEIGHT,
            Self::Compact => Metrics::STACK_ROW_COMPACT_HEIGHT,
        }
    }

    /// The word the tab strip is told (`tabopts.stackRow`).
    pub const fn key(self) -> &'static str {
        match self {
            Self::None => "off",
            Self::Regular => "regular",
            Self::Compact => "compact",
        }
    }
}
// --- end two-level stacks ---

impl Arrangement {
    pub fn from_settings(settings: &crate::settings::Settings) -> Self {
        Self {
            tabs: settings.tab_layout,
            tab_bar: settings.tab_bar_visible,
            address_bar: settings.address_bar_position,
            address_bar_visible: settings.address_bar_visible,
            title_bar: settings.show_title_bar,
            stack_row: StackRow::None,
        }
    }

    /// --- two-level stacks (T10b) --- with the second tab row `row`, kept only where there is a
    /// tab row to put it under: a shown tab bar at the top or the bottom. With the tab bar on a
    /// side the sidebar shows stacks its own way and nothing is reserved.
    pub fn with_stack_row(self, row: StackRow) -> Self {
        let row = if self.tab_bar && self.sidebar_side().is_none() { row } else { StackRow::None };
        Self { stack_row: row, ..self }
    }

    /// [`Arrangement::from_settings`] with the toolbar shown whatever the setting says: Ctrl+L on a
    /// hidden address bar brings it back until the address field loses focus.
    pub fn with_address_bar_revealed(self, revealed: bool) -> Self {
        Self { address_bar_visible: self.address_bar_visible || revealed, ..self }
    }

    /// The toolbar is away from the top: along the bottom edge, or not shown.
    fn address_bar_moved(self) -> bool {
        !self.address_bar_visible || self.address_bar == BarPosition::Bottom
    }

    /// The variant the geometry starts from. A hidden tab bar is `Vertical` without a sidebar: the
    /// toolbar row alone, "+" in it. With the toolbar away from the top, a tab row at the top must
    /// hold the window buttons, so the two toolbar-first layouts become `TabsTop` for the purpose.
    pub fn base_variant(self) -> TabLayout {
        if !self.tab_bar {
            return TabLayout::Vertical;
        }
        match self.tabs {
            TabLayout::ToolbarTop | TabLayout::SingleRow if self.address_bar_moved() => TabLayout::TabsTop,
            tabs => tabs,
        }
    }

    /// Which edge the tab sidebar is on, when the tab bar is a shown sidebar.
    pub fn sidebar_side(self) -> Option<SidePanelPosition> {
        if self.tab_bar { self.tabs.sidebar_side() } else { None }
    }

    /// Whether a title row is laid out above everything: asked for (`show_title_bar`) with the tabs
    /// on a side, or needed because the toolbar is away from the top and no tab row is there to
    /// hold the window buttons.
    pub fn has_title_row(self) -> bool {
        if self.address_bar_moved() {
            self.base_variant() != TabLayout::TabsTop
        } else {
            self.title_bar && self.sidebar_side().is_some()
        }
    }

    pub fn address_bar_at_bottom(self) -> bool {
        self.address_bar_visible && self.address_bar == BarPosition::Bottom
    }

    /// The tab row is along the bottom edge (shown, `TabsBottom`).
    pub fn tabs_at_bottom(self) -> bool {
        self.tab_bar && self.tabs == TabLayout::TabsBottom
    }

    /// The frame's bottom resize grip must stay thin (like the top one) when a row of controls sits
    /// along the bottom edge, or the grip would swallow the bottom of every tab or button there.
    pub fn thin_bottom_edge(self) -> bool {
        self.tabs_at_bottom() || self.address_bar_at_bottom()
    }
}

impl Layout {
    /// Moves the toolbar row and everything in it by `dy`. The tab row follows only when it sits
    /// inside the toolbar row (`SingleRow`).
    fn shift_toolbar(&mut self, dy: i32) {
        let in_strip = |rect: Rect| !rect.is_empty() && self.strip.contains_rect(rect);
        let row_in_strip = in_strip(self.tab_row);
        let plus_in_strip = in_strip(self.new_tab_button);
        let shift = |rect: &mut Rect| {
            if !rect.is_empty() {
                *rect = Rect::new(rect.left, rect.top + dy, rect.right, rect.bottom + dy);
            }
        };
        shift(&mut self.strip);
        shift(&mut self.back_button);
        shift(&mut self.forward_button);
        shift(&mut self.reload_button);
        shift(&mut self.omnibox);
        shift(&mut self.main_menu_button);
        if plus_in_strip {
            shift(&mut self.new_tab_button);
        }
        if row_in_strip {
            self.shift_tab_row(dy);
        }
    }

    /// Moves the tab row and everything in it by `dy` ("+" only when it is in the row).
    fn shift_tab_row(&mut self, dy: i32) {
        let plus_in_row = !self.new_tab_button.is_empty() && self.tab_row.contains_rect(self.new_tab_button);
        let shift = |rect: &mut Rect| {
            if !rect.is_empty() {
                *rect = Rect::new(rect.left, rect.top + dy, rect.right, rect.bottom + dy);
            }
        };
        shift(&mut self.tab_row);
        shift(&mut self.tab_zone);
        shift(&mut self.tab_menu_button);
        if plus_in_row {
            shift(&mut self.new_tab_button);
        }
        self.island_top += dy;
        self.island_bottom += dy;
    }

    /// Puts a title row across the top (from `left` to the client's right edge; the rail keeps its
    /// space) and moves every row at the top down by its height. The caption buttons stay where
    /// they are: they now sit in the title row.
    fn with_title_row(mut self, left: i32) -> Self {
        let m = self.metrics;
        let height = m.px(Metrics::TOP_MARGIN) + m.px(Metrics::STRIP_HEIGHT);
        self.title_bar = Rect::new(left.min(self.client.right), 0, self.client.right, height.min(self.client.bottom));
        let strip_at_top = !self.strip.is_empty() && self.strip.top < self.client.height() / 2;
        let row_at_top = !self.tab_row.is_empty() && self.tab_row.top < self.client.height() / 2;
        if strip_at_top {
            self.shift_toolbar(height);
        }
        if row_at_top && !self.strip.contains_rect(self.tab_row) {
            self.shift_tab_row(height);
        }
        self
    }

    /// Moves the toolbar row to the bottom edge (its margin below it). A `TabsBottom` tab row goes
    /// just above it.
    fn with_toolbar_at_bottom(mut self) -> Self {
        let m = self.metrics;
        let margin = m.px(Metrics::OUTER_MARGIN);
        let gap = m.px(Metrics::STRIP_GAP);
        let strip_h = self.strip.height();
        let strip_top = (self.client.bottom - margin - strip_h).max(self.strip.top);
        self.shift_toolbar(strip_top - self.strip.top);
        if self.variant == TabLayout::TabsBottom && !self.tab_row.is_empty() {
            let row_top = (self.strip.top - gap - self.tab_row.height()).max(0);
            self.shift_tab_row(row_top - self.tab_row.top);
        }
        self
    }

    /// Removes the toolbar row: no navigation, no address field, no main menu button ("+" too when
    /// it was in the row). The HTML toolbar host hides with it (`toolbar_bounds` is `None`).
    fn with_toolbar_hidden(mut self) -> Self {
        if !self.new_tab_button.is_empty() && self.strip.contains_rect(self.new_tab_button) {
            self.new_tab_button = Rect::default();
        }
        self.strip = Rect::default();
        self.back_button = Rect::default();
        self.forward_button = Rect::default();
        self.reload_button = Rect::default();
        self.omnibox = Rect::default();
        self.main_menu_button = Rect::default();
        self
    }

    /// Recomputes the page area from where the rows ended up: below the lowest row at the top,
    /// above the highest row at the bottom, between the rail (or a left sidebar) and the window's
    /// right edge (or a right sidebar). `top_chrome` and `bottom_chrome` follow.
    fn settle_viewport(&mut self, sidebar: Option<&crate::vertical_tabs::SidebarLayout>, left_edge: i32) {
        let m = self.metrics;
        let margin = m.px(Metrics::OUTER_MARGIN);
        let gap = m.px(Metrics::VIEWPORT_GAP);
        let rim = if self.maximised { 0 } else { m.px(Metrics::RESIZE_RIM).min(self.client.width() / 2) };
        let middle = self.client.height() / 2;
        let mut top = self.title_bar.bottom;
        let mut bottom = self.client.bottom;
        // --- two-level stacks (T10b) --- the second row is on the tab row's side, however low it reaches.
        let tabs_at_top = self.tab_row.top < middle;
        for (row, at_top) in [(self.strip, self.strip.top < middle), (self.tab_row, tabs_at_top), (self.stack_row, tabs_at_top)] {
            if row.is_empty() {
                continue;
            }
            if at_top {
                top = top.max(row.bottom);
            } else {
                bottom = bottom.min(row.top);
            }
        }
        let top = if top > 0 { top + margin + gap } else { rim }.min(self.client.bottom);
        let bottom = if bottom < self.client.bottom { bottom - margin - gap } else { self.client.bottom - rim }.max(top);
        let (left, right) = match sidebar.map(|s| (s.side, s.sidebar)) {
            Some((SidePanelPosition::Left, rect)) => (rect.right, (self.client.right - rim).max(rect.right)),
            Some((SidePanelPosition::Right, rect)) => ((left_edge + rim).min(rect.left), rect.left),
            None => (left_edge + rim, (self.client.right - rim).max(left_edge + rim)),
        };
        self.viewport = Rect::new(left, top, right, bottom);
        self.top_chrome = top;
        self.bottom_chrome = if bottom < self.client.bottom - rim { bottom } else { self.client.bottom };
    }

    /// Whether the toolbar row is along the bottom edge (below the page).
    pub fn toolbar_at_bottom(&self) -> bool {
        !self.strip.is_empty() && self.strip.top >= self.viewport.top
    }
}

/// The chrome for `arrangement` in a window with the left rail: the geometry every consumer (the
/// window's hit tests and paint, the HTML host bounds, the page bounds) shares. The sidebar comes
/// back with it for the two sidebar layouts.
#[allow(clippy::too_many_arguments)]
pub fn compute_arranged_in_frame(
    arrangement: Arrangement,
    client_w: i32,
    client_h: i32,
    dpi: u32,
    tab_count: usize,
    active_index: usize,
    maximised: bool,
    sidebar_width: i32,
    centred: bool,
    sidebar_row_height: i32,
) -> (Layout, Option<crate::vertical_tabs::SidebarLayout>) {
    use crate::vertical_tabs::SidebarLayout;
    let client_w = client_w.max(0);
    let client_h = client_h.max(0);
    let m = Metrics::new(dpi);
    let inset = left_rail_inset(dpi);
    let base = arrangement.base_variant();
    let title = arrangement.has_title_row();
    let title_h = if title { m.px(Metrics::TOP_MARGIN) + m.px(Metrics::STRIP_HEIGHT) } else { 0 };
    let rim = if maximised { 0 } else { m.px(Metrics::RESIZE_RIM).min(client_w / 2) };
    let (mut layout, mut sidebar) = match arrangement.sidebar_side() {
        Some(SidePanelPosition::Left) => {
            // The sidebar spans the window's height under the title row (to the bottom edge, as it
            // always has); the toolbar sits right of it.
            let sidebar = SidebarLayout::compute_in_frame_at(
                client_w, client_h, dpi, sidebar_width, maximised, SidePanelPosition::Left, title_h, client_h, sidebar_row_height,
            );
            let offset = sidebar.sidebar.right;
            let mut layout = compute_for_with(TabLayout::Vertical, client_w.saturating_sub(offset), client_h, dpi, tab_count, active_index, LayoutOptions { maximised, centred })
                .shifted_right(offset);
            layout.client = Rect::new(0, 0, client_w, client_h);
            layout.rail = left_rail(client_h, dpi);
            (layout, Some(sidebar))
        }
        Some(SidePanelPosition::Right) => {
            // The toolbar keeps the full width (the window buttons stay at the top right); the
            // sidebar hangs under the top rows, down to the bottom rows.
            let mut layout = compute_in_frame_for_with(TabLayout::Vertical, client_w, client_h, dpi, tab_count, active_index, LayoutOptions { maximised, centred });
            layout.variant = TabLayout::VerticalRight;
            (layout, None)
        }
        None => (compute_in_frame_for_with(base, client_w, client_h, dpi, tab_count, active_index, LayoutOptions { maximised, centred }), None),
    };
    if title {
        layout = layout.with_title_row(inset);
    }
    if arrangement.address_bar_at_bottom() {
        layout = layout.with_toolbar_at_bottom();
    } else if !arrangement.address_bar_visible {
        layout = layout.with_toolbar_hidden();
    }
    // --- two-level stacks (T10b) --- the second tab row, once the rows above and below are placed.
    if arrangement.sidebar_side().is_none() {
        layout = layout.with_stack_row(arrangement.stack_row);
    }
    if arrangement.sidebar_side() == Some(SidePanelPosition::Right) {
        // Between the rows at the top and the rows at the bottom, now that they are placed.
        let gap = m.px(Metrics::STRIP_GAP);
        let middle = client_h / 2;
        let mut top = title_h;
        let mut bottom = client_h - rim;
        for row in [layout.strip, layout.tab_row] {
            if row.is_empty() {
                continue;
            }
            if row.top < middle { top = top.max(row.bottom + gap); } else { bottom = bottom.min(row.top - gap); }
        }
        sidebar = Some(SidebarLayout::compute_in_frame_at(
            client_w, client_h, dpi, sidebar_width, maximised, SidePanelPosition::Right, top, bottom.max(top), sidebar_row_height,
        ));
    }
    layout.settle_viewport(sidebar.as_ref(), inset);
    if let Some(sidebar) = sidebar.as_mut() {
        sidebar.viewport = layout.viewport;
    }
    (layout, sidebar)
}
// --- end layout ---

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

// --- auto-hide (T11) --- geometry for "Auto-hide toolbars and tabs" (docs/design/t11-auto-hide.md).
//
// Auto-hide never relays out the page when a bar appears: the page is laid out as if the hidden bars
// were gone, and a revealed bar floats over it. With auto-hide off every function below returns what
// the function it wraps returns, field for field (`auto_hide_off_changes_nothing`).

/// Which chrome the page's rectangle no longer leaves room for.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AutoHide {
    /// The top chrome: the toolbar and, in the horizontal layouts, the tab row (they share the top edge).
    pub top: bool,
    /// The vertical layout's tab sidebar.
    pub sidebar: bool,
}

impl AutoHide {
    pub const OFF: Self = Self { top: false, sidebar: false };

    pub const fn is_off(self) -> bool {
        !self.top && !self.sidebar
    }
}

/// Where the page starts when the top chrome is out of the layout: flush with the screen edge when
/// the window is maximised or full screen, else the same native rim the other three sides keep (the
/// window stays resizable from its top edge).
pub fn auto_hide_top(dpi: u32, maximised: bool) -> i32 {
    if maximised {
        0
    } else {
        Metrics::new(dpi).px(Metrics::RESIZE_RIM)
    }
}

/// `normal` (the page rectangle with every bar laid out) widened over the hidden bars. The left rail
/// is not a bar that hides, so the page never goes under it.
pub fn auto_hide_viewport(normal: Rect, hide: AutoHide, dpi: u32, maximised: bool) -> Rect {
    if hide.is_off() {
        return normal;
    }
    let mut viewport = normal;
    if hide.top {
        viewport.top = auto_hide_top(dpi, maximised).min(viewport.bottom);
    }
    if hide.sidebar {
        let rim = if maximised { 0 } else { Metrics::new(dpi).px(Metrics::RESIZE_RIM) };
        viewport.left = (left_rail_inset(dpi) + rim).min(viewport.right);
    }
    viewport
}

/// [`compute_in_frame_for_with`] with the chrome in `hide` taken out of the page's rectangle. The
/// bars keep the rectangles they have when shown, so a revealed bar is drawn exactly where it
/// always is. The horizontal layouts have no sidebar; `hide.sidebar` is ignored there.
#[allow(clippy::too_many_arguments)]
pub fn compute_in_frame_auto_hide(
    variant: TabLayout,
    client_w: i32,
    client_h: i32,
    dpi: u32,
    tab_count: usize,
    active_index: usize,
    maximised: bool,
    centred: bool,
    hide: AutoHide,
) -> Layout {
    let mut layout =
        compute_in_frame_for_with(variant, client_w, client_h, dpi, tab_count, active_index, LayoutOptions { maximised, centred });
    let hide = AutoHide { sidebar: false, ..hide };
    layout.viewport = auto_hide_viewport(layout.viewport, hide, dpi, maximised);
    layout
}

/// [`compute_vertical_in_frame_with`] with the chrome in `hide` taken out of the page's rectangle.
/// With the sidebar hidden the toolbar starts at the rail instead of at the sidebar's edge, so no
/// gap is left where the sidebar was; the returned sidebar keeps its shown rectangle, which is where
/// it floats when revealed.
#[allow(clippy::too_many_arguments)]
pub fn compute_vertical_in_frame_auto_hide(
    client_w: i32,
    client_h: i32,
    dpi: u32,
    tab_count: usize,
    active_index: usize,
    maximised: bool,
    sidebar_width: i32,
    centred: bool,
    hide: AutoHide,
) -> (Layout, crate::vertical_tabs::SidebarLayout) {
    let (mut layout, sidebar) = compute_vertical_in_frame_with(client_w, client_h, dpi, tab_count, active_index, sidebar_width, LayoutOptions { maximised, centred });
    if hide.is_off() {
        return (layout, sidebar);
    }
    if hide.sidebar {
        let inset = left_rail_inset(dpi);
        let normal_viewport = layout.viewport;
        layout = compute_for_with(TabLayout::Vertical, client_w.max(0) - inset, client_h, dpi, tab_count, active_index, LayoutOptions { maximised, centred })
        .shifted_right(inset);
        layout.client = Rect::new(0, 0, client_w.max(0), client_h.max(0));
        layout.rail = left_rail(client_h, dpi);
        layout.viewport = normal_viewport;
    }
    layout.viewport = auto_hide_viewport(layout.viewport, hide, dpi, maximised);
    (layout, sidebar)
}

/// The page rectangle for a window with the chrome in `hide` out of the layout (what the app sizes
/// a tab to before the window has laid itself out). Same rectangle as the layouts' `viewport`.
#[allow(clippy::too_many_arguments)]
pub fn auto_hide_page_rect(
    variant: TabLayout,
    client_w: i32,
    client_h: i32,
    dpi: u32,
    maximised: bool,
    sidebar_width: i32,
    hide: AutoHide,
) -> Rect {
    let normal = if variant.is_vertical() {
        crate::vertical_tabs::SidebarLayout::compute_in_frame(client_w, client_h, dpi, sidebar_width, maximised).viewport
    } else {
        let inset = left_rail_inset(dpi);
        content_rect(variant, client_w.max(0) - inset, client_h, dpi, maximised).shifted_right(inset)
    };
    let hide = if variant.is_vertical() { hide } else { AutoHide { sidebar: false, ..hide } };
    auto_hide_viewport(normal, hide, dpi, maximised)
}

/// Where the hidden bars are drawn while revealed (they float over the page).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AutoHideOverlay {
    /// The toolbar and tab row, from the client's top edge to where the shown page area begins.
    pub top: Rect,
    /// The vertical layout's sidebar (empty in the horizontal layouts).
    pub sidebar: Rect,
    /// --- layout (thread T6) --- the rows along the bottom edge (a bottom tab row or toolbar), from
    /// where the shown page area ends to the client's bottom edge; empty when nothing sits there.
    pub bottom: Rect,
}

// --- layout (thread T6) + auto-hide (T11) --- the arranged geometry with hidden bars.
//
// The window lays every arrangement out with `compute_arranged_in_frame`; these take the bars in
// `hide` out of that page rectangle the same way the plain functions above do, so T6's arrangement
// (edges, title row, density) and T11's auto-hide agree. With `AutoHide::OFF` the result is
// `compute_arranged_in_frame`'s, field for field (`arranged_auto_hide_off_changes_nothing`).

/// `normal` (the arranged page rectangle with every bar laid out) widened over the hidden bars:
/// `hide.top` takes the rows at the top out and, when the arrangement has rows along the bottom
/// edge, those too (the toolbar hides wherever it sits); `hide.sidebar` takes the tab sidebar out on
/// whichever side it is. The left rail never hides.
pub fn auto_hide_arranged_viewport(normal: Rect, client: Rect, arrangement: Arrangement, hide: AutoHide, dpi: u32, maximised: bool) -> Rect {
    if hide.is_off() {
        return normal;
    }
    let rim = if maximised { 0 } else { Metrics::new(dpi).px(Metrics::RESIZE_RIM) };
    let mut viewport = normal;
    if hide.top {
        viewport.top = auto_hide_top(dpi, maximised).min(viewport.bottom);
        if arrangement.thin_bottom_edge() {
            viewport.bottom = (client.bottom - rim).max(viewport.top);
        }
    }
    if hide.sidebar {
        match arrangement.sidebar_side() {
            Some(SidePanelPosition::Left) => viewport.left = (left_rail_inset(dpi) + rim).min(viewport.right),
            Some(SidePanelPosition::Right) => viewport.right = (client.right - rim).max(viewport.left),
            None => {}
        }
    }
    viewport
}

/// [`compute_arranged_in_frame`] with the chrome in `hide` taken out of the page's rectangle. The
/// bars keep the rectangles they have when shown (a revealed bar is drawn where it always is),
/// except that with the sidebar hidden the toolbar starts at the rail (left sidebar) or runs to the
/// window's edge (right sidebar), so no gap is left where the sidebar was; the returned sidebar
/// keeps its shown rectangle, which is where it floats when revealed.
#[allow(clippy::too_many_arguments)]
pub fn compute_arranged_auto_hide(
    arrangement: Arrangement,
    client_w: i32,
    client_h: i32,
    dpi: u32,
    tab_count: usize,
    active_index: usize,
    maximised: bool,
    sidebar_width: i32,
    centred: bool,
    sidebar_row_height: i32,
    hide: AutoHide,
) -> (Layout, Option<crate::vertical_tabs::SidebarLayout>) {
    let (mut layout, sidebar) = compute_arranged_in_frame(
        arrangement, client_w, client_h, dpi, tab_count, active_index, maximised, sidebar_width, centred, sidebar_row_height,
    );
    if hide.is_off() {
        return (layout, sidebar);
    }
    let hide = AutoHide { sidebar: hide.sidebar && arrangement.sidebar_side().is_some(), ..hide };
    if hide.sidebar {
        let normal_viewport = layout.viewport;
        let (moved, _) = compute_arranged_in_frame(
            Arrangement { tab_bar: false, ..arrangement },
            client_w, client_h, dpi, tab_count, active_index, maximised, sidebar_width, centred, sidebar_row_height,
        );
        layout = moved;
        layout.viewport = normal_viewport;
    }
    layout.viewport = auto_hide_arranged_viewport(layout.viewport, layout.client, arrangement, hide, dpi, maximised);
    (layout, sidebar)
}

/// The rectangles a revealed bar occupies in an arranged layout: `shown` is the layout with every
/// bar in place, `hidden` the one with the bars out. The top band runs from the client's top edge to
/// where the shown page begins; the bottom band from where the shown page ends to the client's
/// bottom edge, when rows sit there.
pub fn auto_hide_overlay_arranged(
    shown: &Layout,
    hidden: &Layout,
    sidebar: Option<&crate::vertical_tabs::SidebarLayout>,
    arrangement: Arrangement,
    dpi: u32,
    sidebar_hidden: bool,
) -> AutoHideOverlay {
    let client = hidden.client;
    let left = match (sidebar, arrangement.sidebar_side()) {
        (Some(sidebar), Some(SidePanelPosition::Left)) if !sidebar_hidden => sidebar.sidebar.right,
        _ => left_rail_inset(dpi),
    };
    let right = match (sidebar, arrangement.sidebar_side()) {
        (Some(sidebar), Some(SidePanelPosition::Right)) if !sidebar_hidden => sidebar.sidebar.left,
        _ => client.right,
    };
    let top = Rect::new(left.min(right), 0, right, shown.viewport.top.min(client.bottom));
    let bottom = if arrangement.thin_bottom_edge() {
        Rect::new(left.min(right), shown.viewport.bottom.max(0), right, client.bottom)
    } else {
        Rect::default()
    };
    AutoHideOverlay { top, sidebar: sidebar.map(|sidebar| sidebar.sidebar).unwrap_or_default(), bottom }
}
// --- end layout + auto-hide ---

/// The rectangles a revealed top bar and sidebar occupy. `sidebar_hidden` says whether the toolbar
/// was moved left over the sidebar's place (see [`compute_vertical_in_frame_auto_hide`]).
pub fn auto_hide_overlay(
    layout: &Layout,
    sidebar: Option<&crate::vertical_tabs::SidebarLayout>,
    dpi: u32,
    sidebar_hidden: bool,
) -> AutoHideOverlay {
    let height = chrome_height(layout.variant, dpi).min(layout.client.bottom);
    let left = match sidebar {
        Some(sidebar) if !sidebar_hidden => sidebar.sidebar.right,
        _ => left_rail_inset(dpi),
    };
    AutoHideOverlay {
        top: Rect::new(left.min(layout.client.right), 0, layout.client.right, height),
        sidebar: sidebar.map(|sidebar| sidebar.sidebar).unwrap_or_default(),
        bottom: Rect::default(),
    }
}
// --- end auto-hide ---

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

        assert_eq!(m.px(Metrics::TOP_MARGIN), 0);
        assert_eq!(m.px(Metrics::STRIP_HEIGHT), 30);
        assert_eq!(m.px(Metrics::STRIP_GAP), 2);
        assert_eq!(m.px(Metrics::TAB_ROW_HEIGHT), 28);
        assert_eq!(m.px(Metrics::ISLAND_HEIGHT), 26);
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

        assert_eq!(m.px(Metrics::STRIP_HEIGHT), 38, "37.5 rounds up");
        assert_eq!(m.px(Metrics::STRIP_GAP), 3, "2.5 rounds up");
        assert_eq!(m.px(Metrics::TAB_ROW_HEIGHT), 35);
        assert_eq!(m.px(Metrics::ISLAND_HEIGHT), 33);
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

        assert_eq!(m.px(Metrics::STRIP_HEIGHT), 45);
        assert_eq!(m.px(Metrics::STRIP_GAP), 3);
        assert_eq!(m.px(Metrics::TAB_ROW_HEIGHT), 42);
        assert_eq!(m.px(Metrics::ISLAND_HEIGHT), 39);
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

        assert_eq!(m.px(Metrics::STRIP_HEIGHT), 60);
        assert_eq!(m.px(Metrics::STRIP_GAP), 4);
        assert_eq!(m.px(Metrics::TAB_ROW_HEIGHT), 56);
        assert_eq!(m.px(Metrics::ISLAND_HEIGHT), 52);
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
        assert_eq!(Metrics::new(0).px(Metrics::STRIP_HEIGHT), 30);
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
        // The left rail takes 30 px at 96 dpi (2 px margin, 26 px rail, 2 px margin). The chrome is
        // laid out in the 994 px to its right, so every toolbar rect is 30 px further right than it
        // would be without the rail.
        let inset = left_rail_inset(96);
        assert_eq!(inset, 30);
        let l = compute_in_frame(1024, HEIGHT, 96, 1, 0, false);

        // The toolbar starts at the top edge (30 px tall); a 2 px gap separates it from the 28 px tab row.
        assert_eq!(l.strip, Rect::new(inset, 0, 1024, 30));
        assert_eq!(l.tab_row, Rect::new(inset, 32, 1024, 60));
        assert_eq!(l.caption_area, Rect::new(886, 0, 1024, 30));
        assert_eq!(l.tab_zone, Rect::new(32, 32, 991, 60));
        assert_eq!(
            l.tab(0),
            Some(Rect::new(32, 33, 991, 59)),
            "a lone tab fills the row up to \"+\""
        );
        assert_eq!(
            l.new_tab_button,
            Rect::new(994, 32, 1022, 60),
            "\"+\" is pinned to the far right end of the tab row"
        );
        assert!(
            l.tab_menu_button.is_empty(),
            "no overflow menu when tabs fit"
        );
        assert_eq!(l.main_menu_button, Rect::new(842, 2, 870, 28));
        assert_eq!(
            l.main_menu_button.left,
            l.omnibox.right + 3,
            "the bubble sits directly right of the address pill"
        );
        assert_eq!(l.back_button, Rect::new(34, 1, 62, 29));
        assert_eq!(l.forward_button, Rect::new(62, 1, 90, 29));
        assert_eq!(l.reload_button, Rect::new(93, 1, 121, 29));
        assert_eq!(l.omnibox, Rect::new(215, 2, 839, 28));
        assert_eq!(
            l.tab(0).unwrap().top - l.omnibox.bottom,
            5,
            "the omnibox sits 5 px above the tab islands"
        );
        assert_eq!(hit_test(&l, 856, 15), Hit::MainMenu);
        assert_eq!(hit_test(&l, 1008, 46), Hit::NewTab);
        assert_eq!(l.viewport, Rect::new(34, 63, 1020, 696));
        assert_eq!(l.rail, Rect::new(2, 2, 28, 698));
        assert_eq!(hit_test(&l, 22, 20), Hit::RailBackground);
    }

    #[test]
    fn omnibox_is_centred_in_the_window_when_the_toolbar_has_room() {
        // At 1024 the menu bubble pushes the omnibox left of centre, so centring is checked wider.
        let l = layout(1440, 96, 1);

        assert_eq!(l.omnibox, Rect::new(408, 2, 1032, 28));
        assert_eq!(
            (l.omnibox.left + l.omnibox.right) / 2,
            720,
            "the omnibox is centred in the window"
        );
    }

    /// --- wave-1 fixes --- `centred_address_bar` off: the omnibox moves left, next to navigation, at the
    /// same width; on, it stays centred.
    #[test]
    fn the_centred_address_bar_option_off_puts_the_omnibox_beside_navigation() {
        let centred = compute_for_with(TabLayout::ToolbarTop, 1440, 900, 96, 1, 0, LayoutOptions { maximised: false, centred: true });
        let left = compute_for_with(TabLayout::ToolbarTop, 1440, 900, 96, 1, 0, LayoutOptions { maximised: false, centred: false });
        assert_eq!((centred.omnibox.left + centred.omnibox.right) / 2, 720, "on: centred");
        assert!(left.omnibox.left < centred.omnibox.left, "off: further left");
        assert_eq!(left.omnibox.width(), centred.omnibox.width(), "same width either way");
        assert!(left.omnibox.left < 200, "beside the navigation buttons: {:?}", left.omnibox);
    }

    #[test]
    fn eight_tabs_at_1024_shrink_evenly_without_overflow() {
        let l = layout(1024, 96, 8);

        assert_eq!(l.tab(0).map(Rect::width), Some(170));
        assert_eq!(l.tab(7).map(Rect::width), Some(114));
        assert!(l.tabs().all(|tab| tab.intersect(l.tab_zone) == tab));
        assert!(
            l.tab_menu_button.is_empty(),
            "all tabs fit, so hide the overflow menu"
        );
    }

    #[test]
    fn vertical_layout_with_group_slots_never_places_an_overflow_picker() {
        let (l, _) = compute_vertical_in_frame(1100, 800, 96, 12, 11, false, 260);
        let l = l.with_group_slots(
            (0..12).map(|i| i == 2),
            (0..12).map(|_| false),
            (0..12).map(|_| false),
        );
        assert!(l.tab_menu_button.is_empty(), "no tab row, so no picker above the window");
    }

    #[test]
    fn forty_tabs_overflow_with_whole_islands() {
        let l = layout(768, 96, 40);

        // 23 whole islands fit in the row left of the tab list and "+".
        assert_eq!(l.tab(0).map(Rect::width), Some(41));
        assert_eq!(l.tab_zone, Rect::new(2, 32, 706, 60));
        assert_eq!(l.new_tab_button, Rect::new(738, 32, 766, 60));
        assert_eq!(l.tab_menu_button, Rect::new(709, 33, 735, 59));
        assert_eq!(l.caption_area.left, 630);
        assert!(
            !l.tab_menu_button.is_empty(),
            "overflow menu stays available"
        );

        assert!(
            l.tab(21).is_some_and(|tab| tab.right <= l.tab_zone.right),
            "the last visible island is whole and inside the zone"
        );
        assert_eq!(l.tab(23), None, "tabs after the visible window are hidden");
        assert!(
            l.tab(22).is_some_and(|tab| tab.right <= l.tab_zone.right),
            "the 23rd island still fits"
        );
        assert_eq!(l.tab(39), None, "the default active tab is index zero");
        assert_eq!(
            l.hidden_tab_indices().collect::<Vec<_>>(),
            (23..40).collect::<Vec<_>>()
        );
    }

    #[test]
    fn active_tab_is_kept_visible_when_tabs_overflow() {
        let l = compute_with_active(768, HEIGHT, 96, 40, 39, false);

        assert_eq!(l.first_visible_tab(), 17);
        let active = l.tab(39).expect("active tab is visible");
        assert!(active.right <= l.tab_zone.right);
        assert_eq!(hit_test(&l, active.left + 3, 48), Hit::Tab(39));
        assert_eq!(l.tabs().count(), 23);
        assert_eq!(
            l.hidden_tab_indices().collect::<Vec<_>>(),
            (0..17).collect::<Vec<_>>()
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
                    assert_eq!((rail.left, rail.top), (margin, m.px(Metrics::TOP_MARGIN) + margin));
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
    fn tab_controls_sit_at_the_right_end_of_the_tab_row() {
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
                    assert_eq!(new_tab.right, l.client.right - margin, "dpi {dpi}");
                    assert!(menu.right <= l.caption_area.left - margin, "dpi {dpi}");
                    if tabs > l.tabs().count() {
                        assert_eq!(list.width(), m.px(Metrics::TAB_MENU_SIZE));
                        assert_eq!(l.tab_zone.right + gap, list.left, "dpi {dpi}");
                        assert_eq!(list.right + gap, new_tab.left, "dpi {dpi}");
                    } else {
                        assert!(list.is_empty(), "hide the menu while every tab fits");
                        assert_eq!(l.tab_zone.right + gap, new_tab.left, "dpi {dpi}");
                    }
                    for (name, rect) in [("new tab", new_tab), ("tab menu", list)] {
                        if rect.is_empty() {
                            continue;
                        }
                        assert!(l.tab_row.contains_rect(rect), "{name} leaves the tab row");
                        assert!(l.strip.intersect(rect).is_empty(), "{name} is in the toolbar");
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
                assert_eq!(l.strip.top, m.px(Metrics::TOP_MARGIN));
                assert_eq!(l.strip.bottom + m.px(Metrics::STRIP_GAP), l.tab_row.top);
                assert_eq!(l.tab_row.bottom + margin + m.px(Metrics::VIEWPORT_GAP), l.viewport.top);
                let rim = m.px(Metrics::RESIZE_RIM);
                assert_eq!(l.viewport.bottom, HEIGHT - rim);
                assert_eq!(l.omnibox.height(), m.px(Metrics::OMNIBOX_HEIGHT));
                assert!(l.omnibox.left >= l.reload_button.right + m.px(Metrics::ISLAND_GAP));
                assert_eq!(l.new_tab_button.right, width - margin);
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
            ] {
                let (above, below) = (rect.top - l.strip.top, l.strip.bottom - rect.bottom);
                assert!(
                    (above - below).abs() <= 1,
                    "dpi {dpi}: {name} is {above} above and {below} below"
                );
            }
            let plus = l.new_tab_button;
            let (above, below) = (plus.top - l.tab_row.top, l.tab_row.bottom - plus.bottom);
            assert!(
                (above - below).abs() <= 1,
                "dpi {dpi}: \"+\" is {above} above and {below} below in the tab row"
            );
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
    fn zero_tabs_leave_plus_pinned_to_the_tab_row_end() {
        let l = compute(1024, HEIGHT, 96, 0, false);

        assert_eq!(l.tab_count(), 0);
        assert_eq!(l.tab(0), None);
        assert_eq!(l.new_tab_button, Rect::new(994, 32, 1022, 60));
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
                assert_eq!(rect.height(), l.caption_area.height());
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
        // Window 1024 wide with the rail's 30 px inset. Chrome x positions are the old ones plus 30,
        // except the caption buttons, which sit at the window's right edge either way.
        let l = compute_in_frame(1024, HEIGHT, 96, 3, 0, false);

        assert_eq!(hit_test(&l, 144, 48), Hit::Tab(0));
        assert_eq!(hit_test(&l, 544, 48), Hit::Tab(1));
        assert_eq!(hit_test(&l, 844, 48), Hit::Tab(2));
        assert_eq!(
            hit_test(&l, 856, 15),
            Hit::MainMenu,
            "the main-menu bubble sits right of the address pill"
        );
        assert_eq!(
            hit_test(&l, 884, 48),
            Hit::Tab(2),
            "the tab row has no controls in the middle"
        );
        assert_eq!(
            hit_test(&l, 1008, 46),
            Hit::NewTab,
            "\"+\" sits at the right end of the tab row"
        );
        assert_eq!(
            hit_test(&l, 40, 48),
            Hit::TabClose(0),
            "close is on the left edge of the tab"
        );
        assert_eq!(
            hit_test(&l, 144, l.tab_row.top),
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
        assert_eq!(hit_test(&l, 875, 15), Hit::CaptionArea, "toolbar background right of the menu drags");
        assert_eq!(hit_test(&l, 194, 15), Hit::CaptionArea);
        assert_eq!(hit_test(&l, 544, 15), Hit::Omnibox);
        assert_eq!(hit_test(&l, 544, 1), Hit::CaptionArea, "above the omnibox");

        assert_eq!(hit_test(&l, 900, 20), Hit::CaptionMinimize);
        assert_eq!(hit_test(&l, 950, 20), Hit::CaptionMaximize);
        assert_eq!(hit_test(&l, 1000, 20), Hit::CaptionClose);
        assert_eq!(
            hit_test(&l, 544, l.viewport.top - 1),
            Hit::None,
            "margin above viewport"
        );
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
                count: 0,
                emoji: None,
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
    fn html_rail_region_gives_no_rail_hits() {
        use crate::chrome::rail::{RailEntry, RailIcon, RailView};
        let l = compute_in_frame(1024, 600, 96, 3, 0, false);
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
                count: 0,
                emoji: None,
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
        let geometry = rail::geometry(l.rail, &entries, 0, l.metrics.dpi());
        let button = geometry.items[0].visible();
        let (x, y) = ((button.left + button.right) / 2, (button.top + button.bottom) / 2);
        assert_eq!(hit_test_with_rail(&l, Some(&view), x, y), Hit::Rail(RailKey::Home));
        // While the HTML rail is active its whole region is the page's: no Rail or RailBackground.
        assert_eq!(hit_test_native(&l, Some(&view), &[l.rail], x, y), Hit::None);
        assert_eq!(hit_test_native(&l, Some(&view), &[l.rail], l.rail.left + 1, l.rail.top + 1), Hit::None);
        // Outside the rail the native hit is unchanged.
        assert_ne!(hit_test_native(&l, Some(&view), &[l.rail], l.rail.right + 4, y), Hit::None);
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
        // The strip above the row keeps its native hits, and so does the tab row's "+" once the HTML
        // tab strip stops before it.
        let tab_strip = Rect::from(crate::chrome_host::tab_row_bounds(&l).expect("tab strip"));
        let plus = (l.new_tab_button.left + 2, l.new_tab_button.top + 2);
        assert_eq!(hit_test_native(&l, None, &[tab_strip], plus.0, plus.1), Hit::NewTab);
        assert_eq!(hit_test_native(&l, None, &[tab_strip], x, y), Hit::None);
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
            hit_test(&l, l.new_tab_button.left + 2, l.new_tab_button.top + 2),
            Hit::NewTab,
            "+ stays reachable at the end of the tab row"
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
            assert_eq!(chrome_height(TabLayout::ToolbarTop, dpi), l.viewport.top, "dpi {dpi}");
        }
    }

    #[test]
    fn top_margin_moves_chrome_and_keeps_caption_hits_at_the_top() {
        for dpi in [96, 120, 144, 192] {
            let m = Metrics::new(dpi);
            let top = m.px(Metrics::TOP_MARGIN);
            let layout = compute_in_frame(1200, 800, dpi, 1, 0, false);
            assert_eq!(layout.strip.top, top, "strip dpi={dpi}");
            assert_eq!(layout.strip.bottom, top + m.px(Metrics::STRIP_HEIGHT), "strip bottom dpi={dpi}");
            assert_eq!(layout.tab_row.top, layout.strip.bottom + m.px(Metrics::STRIP_GAP), "tabs dpi={dpi}");
            assert_eq!(
                layout.viewport.top,
                top + m.px(Metrics::STRIP_HEIGHT) + m.px(Metrics::STRIP_GAP) + m.px(Metrics::TAB_ROW_HEIGHT) + m.px(Metrics::OUTER_MARGIN) + m.px(Metrics::VIEWPORT_GAP),
                "viewport dpi={dpi}"
            );
            assert_eq!(layout.back_button.top, top + (m.px(Metrics::STRIP_HEIGHT) - m.px(Metrics::NAV_BUTTON_SIZE)) / 2, "nav dpi={dpi}");
            assert_eq!(layout.omnibox.top, top + (m.px(Metrics::STRIP_HEIGHT) - m.px(Metrics::OMNIBOX_HEIGHT)) / 2, "omnibox dpi={dpi}");
            assert_eq!(layout.caption_area.top, 0);
            assert_eq!(layout.caption_area.bottom, top + m.px(Metrics::STRIP_HEIGHT));
            let close = layout.caption_button(Hit::CaptionClose);
            assert_eq!((close.top, close.bottom), (0, top + m.px(Metrics::STRIP_HEIGHT)));
            if top > 0 {
                assert!(layout.drag_region_rects().iter().any(|r| r.top == 0 && r.bottom == top));
            }
            assert_eq!(hit_test(&layout, 100, top / 2), Hit::CaptionArea);
            let maximised = compute_in_frame(1200, 800, dpi, 1, 0, true);
            assert_eq!(maximised.strip.top, top, "maximised strip dpi={dpi}");
            assert_eq!(maximised.viewport.top, layout.viewport.top, "maximised viewport dpi={dpi}");
            assert_eq!(chrome_height(TabLayout::ToolbarTop, dpi), layout.viewport.top);
        }
    }

    #[test]
    fn drag_region_rects_stay_inside_chrome() {
        let l = layout(1024, 96, 5);

        for rect in l.drag_region_rects() {
            assert!(
                rect.intersect(l.strip) == rect || rect.intersect(l.tab_row) == rect
                    || rect.intersect(Rect::new(0, 0, l.client.right, l.strip.top)) == rect,
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
        let content = content_rect(TabLayout::ToolbarTop, 520, 420, 96, false);
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
    fn stacked_split_panes_share_the_height_with_the_gap_between_them() {
        use crate::split_view::{divider_for_ratio, Orientation, GAP_DIP, MIN_PANE_HEIGHT_DIP};
        for dpi in [96, 120, 144, 192] {
            let m = Metrics::new(dpi);
            let content = content_rect(TabLayout::ToolbarTop, 1200 * dpi as i32 / 96, 800 * dpi as i32 / 96, dpi, false);
            let gap = m.px(GAP_DIP);
            let min = m.px(MIN_PANE_HEIGHT_DIP);
            let divider = divider_for_ratio(content.top, content.bottom, 50, gap);
            let panes = split_pane_layout_oriented(content, Orientation::Stacked, divider, min, gap).expect("fits two pages");
            // Full width, one above the other, the gap exactly between them.
            for pane in [panes.left, panes.right, panes.divider] {
                assert_eq!((pane.left, pane.right), (content.left, content.right), "dpi {dpi}");
            }
            assert_eq!(panes.left.top, content.top);
            assert_eq!(panes.left.bottom, panes.divider.top);
            assert_eq!(panes.divider.height(), gap);
            assert_eq!(panes.right.top, panes.divider.bottom);
            assert_eq!(panes.right.bottom, content.bottom);
            assert!((panes.left.height() - panes.right.height()).abs() <= 1, "50/50 at dpi {dpi}");
            // The side-by-side form of the same call is the old layout.
            let side = split_pane_layout_oriented(content, Orientation::SideBySide, content.left + 300, m.px(240), gap).unwrap();
            assert_eq!(Some(side), split_pane_layout(content, content.left + 300, m.px(240), gap));
        }
    }

    /// The rectangles of a 1280 x 820 window's split layouts, drop halves and drop-zone map, as JSON.
    /// Written to `BOWSER_SPLIT_LAYOUT_DUMP` when set (the layout diagram in the split view
    /// screenshots draws exactly these); otherwise only checks that every rect is inside the page.
    #[test]
    fn split_layout_diagram_rects_stay_inside_the_page() {
        use crate::split_view::{axis_span, divider_for_ratio, drop_rect, drop_zone, DropZone, Orientation, GAP_DIP};
        let l = compute_in_frame(1280, 820, 96, 6, 2, false);
        let m = l.metrics;
        let page = l.viewport;
        let gap = m.px(GAP_DIP);
        let tuple = (page.left, page.top, page.right, page.bottom);
        let json_rect = |r: Rect| format!("[{},{},{},{}]", r.left, r.top, r.right, r.bottom);
        let panes = |orientation: Orientation, ratio: u8| {
            let (start, end) = axis_span(tuple, orientation);
            let divider = divider_for_ratio(start, end, ratio, gap);
            let panes = split_pane_layout_oriented(page, orientation, divider, m.px(orientation.min_pane_dip()), gap).unwrap();
            for pane in [panes.left, panes.divider, panes.right] {
                assert!(pane.left >= page.left && pane.right <= page.right && pane.top >= page.top && pane.bottom <= page.bottom);
            }
            format!("{{\"first\":{},\"gap\":{},\"second\":{}}}", json_rect(panes.left), json_rect(panes.divider), json_rect(panes.right))
        };
        let drops: Vec<String> = DropZone::ALL
            .iter()
            .map(|zone| {
                let (a, b, c, d) = drop_rect(tuple, *zone, gap);
                format!("\"{zone:?}\":[{a},{b},{c},{d}]")
            })
            .collect();
        let step = 8;
        let mut zones = Vec::new();
        for y in (page.top..page.bottom).step_by(step) {
            for x in (page.left..page.right).step_by(step) {
                let zone = drop_zone(tuple, x, y, None).expect("every point of the page has a zone");
                zones.push((DropZone::ALL.iter().position(|z| *z == zone).unwrap()).to_string());
            }
        }
        let json = format!(
            "{{\"window\":[1280,820],\"strip\":{},\"tabRow\":{},\"page\":{},\"gap\":{gap},\"radius\":{},\"grip\":[{},{}],\"sideBySide\":{},\"stacked\":{},\"stacked35\":{},\"drop\":{{{}}},\"zoneStep\":{step},\"zones\":\"{}\"}}\n",
            json_rect(l.strip),
            json_rect(l.tab_row),
            json_rect(page),
            m.px(9),
            m.px(4),
            m.px(32),
            panes(Orientation::SideBySide, 50),
            panes(Orientation::Stacked, 50),
            panes(Orientation::Stacked, 35),
            drops.join(","),
            zones.concat(),
        );
        if let Some(path) = std::env::var_os("BOWSER_SPLIT_LAYOUT_DUMP") {
            std::fs::write(path, json).expect("write the split layout dump");
        }
    }

    /// The rectangles of a 1280 x 820 window's layouts of three and four panes, every corner drop's
    /// preview card and the corner drop-zone map, as JSON (the quad split diagrams). Written to
    /// `BOWSER_QUAD_LAYOUT_DUMP` when set; always checks that every rect stays inside the page.
    #[test]
    fn quad_layout_diagram_rects_stay_inside_the_page() {
        use crate::split_view::{
            drop_target_rect, drop_target_zone, drop_zone_cornered, plan_drop, DropPlan, DropZone, Orientation, Pane, SplitView, GAP_DIP,
            MIN_PANE_DIP, MIN_PANE_HEIGHT_DIP,
        };
        use crate::tabs::TabId;
        let l = compute_in_frame(1280, 820, 96, 6, 2, false);
        let m = l.metrics;
        let page = l.viewport;
        let (gap, min_w, min_h) = (m.px(GAP_DIP), m.px(MIN_PANE_DIP), m.px(MIN_PANE_HEIGHT_DIP));
        let tuple = (page.left, page.top, page.right, page.bottom);
        let inside = |r: (i32, i32, i32, i32)| r.0 >= page.left && r.2 <= page.right && r.1 >= page.top && r.3 <= page.bottom && r.0 < r.2 && r.1 < r.3;
        let json_box = |r: (i32, i32, i32, i32)| format!("[{},{},{},{}]", r.0, r.1, r.2, r.3);
        let json_rect = |r: Rect| json_box((r.left, r.top, r.right, r.bottom));
        let zone_name = |zone: DropZone| crate::chrome_host::split::edge(zone);
        let layout_json = |view: SplitView| {
            let (primary, cross) = crate::split_view::dividers_for_ratios(view, tuple, gap);
            let panes = split_panes(page, view, primary, cross, m).expect("the layout fits a 1280 x 820 window");
            let list: Vec<String> = view
                .panes()
                .map(|(pane, tab)| {
                    let rect = panes.panes[pane.index()].expect("every pane shows");
                    assert!(inside((rect.left, rect.top, rect.right, rect.bottom)));
                    format!("{{\"id\":{},\"at\":\"{}\",\"rect\":{},\"focused\":{}}}", tab.0, zone_name(view.zone_of(pane)), json_rect(rect), pane == view.focused)
                })
                .collect();
            let dividers: Vec<String> = panes.dividers.iter().flatten().map(|r| json_rect(*r)).collect();
            format!(
                "{{\"panes\":[{}],\"dividers\":[{}],\"crossing\":{}}}",
                list.join(","),
                dividers.join(","),
                panes.crossing.map_or("null".to_owned(), json_rect)
            )
        };
        let t = TabId;
        let pair = SplitView::new(t(3), t(2), Pane::Left).unwrap();
        let drop_layout = |view: SplitView, dragged: u32, zone: DropZone| match plan_drop(Some(view), None, view.focused_tab(), t(dragged), zone) {
            Some(DropPlan::Layout { view, .. }) => view,
            other => panic!("{other:?}"),
        };
        let tee = drop_layout(pair, 4, DropZone::BottomLeft); // [3 / 4 | 2]
        let quad = drop_layout(tee, 5, DropZone::BottomRight); // [3 2 / 4 5]
        let mut quad_focus = quad;
        quad_focus.focus(Pane::Left);
        let mut previews = Vec::new();
        for (name, view) in [("none", None), ("pair", Some(pair)), ("tee", Some(tee)), ("quad", Some(quad_focus))] {
            for zone in DropZone::CORNERS {
                let corners = crate::split_view::corners_allowed(view, t(9));
                // The pointer near the corner: the zone the real function gives there.
                let (x, y) = match zone {
                    DropZone::TopLeft => (page.left + 40, page.top + 40),
                    DropZone::TopRight => (page.right - 40, page.top + 40),
                    DropZone::BottomLeft => (page.left + 40, page.bottom - 40),
                    _ => (page.right - 40, page.bottom - 40),
                };
                let hit = drop_zone_cornered(tuple, x, y, None, corners).unwrap();
                let plan = plan_drop(view, None, t(3), t(9), hit).unwrap();
                let rect = drop_target_rect(plan, t(9), hit, tuple, min_w, min_h, gap);
                assert!(inside(rect), "{name} {zone:?}");
                previews.push(format!(
                    "\"{name}-{}\":{{\"zone\":\"{}\",\"lands\":\"{}\",\"rect\":{},\"point\":[{x},{y}]}}",
                    zone_name(zone),
                    zone_name(hit),
                    zone_name(drop_target_zone(plan, t(9), hit)),
                    json_box(rect)
                ));
            }
        }
        let step = 8;
        let mut zones = String::new();
        let order = [DropZone::Left, DropZone::Right, DropZone::Top, DropZone::Bottom, DropZone::TopLeft, DropZone::TopRight, DropZone::BottomLeft, DropZone::BottomRight];
        for y in (page.top..page.bottom).step_by(step) {
            for x in (page.left..page.right).step_by(step) {
                let zone = drop_zone_cornered(tuple, x, y, None, true).expect("every point has a zone");
                zones.push_str(&order.iter().position(|z| *z == zone).unwrap().to_string());
            }
        }
        let stacked_tee = drop_layout(pair.with_orientation(Orientation::Stacked), 4, DropZone::TopRight);
        let json = format!(
            "{{\"window\":[1280,820],\"strip\":{},\"tabRow\":{},\"page\":{},\"gap\":{gap},\"radius\":{},\"grip\":[{},{}],\"pair\":{},\"tee\":{},\"stackedTee\":{},\"quad\":{},\"quadFocusTl\":{},\"previews\":{{{}}},\"zoneStep\":{step},\"zones\":\"{zones}\"}}\n",
            json_rect(l.strip),
            json_rect(l.tab_row),
            json_rect(page),
            m.px(9),
            m.px(4),
            m.px(32),
            layout_json(pair),
            layout_json(tee),
            layout_json(stacked_tee),
            layout_json(quad),
            layout_json(quad_focus),
            previews.join(","),
        );
        if let Some(path) = std::env::var_os("BOWSER_QUAD_LAYOUT_DUMP") {
            std::fs::write(path, json).expect("write the quad layout dump");
        }
    }

    #[test]
    fn stacked_split_panes_clamp_to_the_minimum_height_and_collapse_below_it() {
        let content = Rect::new(10, 100, 810, 600); // 500 tall
        let panes = split_pane_layout_stacked(content, -50, 160, 8).unwrap();
        assert_eq!(panes.left.height(), 160, "dragged past the top: the top page keeps its minimum");
        let panes = split_pane_layout_stacked(content, 5000, 160, 8).unwrap();
        assert_eq!(panes.right.height(), 160, "dragged past the bottom: the bottom page keeps its minimum");
        // Exactly 2 x 160 + 8 fits; one pixel less collapses to the focused page.
        assert!(split_pane_layout_stacked(Rect::new(0, 0, 800, 328), 160, 160, 8).is_some());
        assert!(split_pane_layout_stacked(Rect::new(0, 0, 800, 327), 160, 160, 8).is_none());
        assert!(split_pane_layout_stacked(Rect::new(0, 0, 0, 900), 400, 160, 8).is_none(), "no width, no pages");
    }

    #[test]
    fn content_rect_keeps_consistent_physical_pixel_margins() {
        for dpi in [96, 120, 144, 192] {
            let rect = content_rect(TabLayout::ToolbarTop, 1200, 800, dpi, false);
            let m = Metrics::new(dpi);
            assert_eq!(rect.left, m.px(Metrics::RESIZE_RIM));
            assert_eq!(rect.right, 1200 - m.px(Metrics::RESIZE_RIM));
            assert_eq!(rect.bottom, 800 - m.px(Metrics::RESIZE_RIM));
            assert!(rect.top > m.px(Metrics::STRIP_HEIGHT + Metrics::TAB_ROW_HEIGHT));
        }
    }

    // --- browser layouts ---

    const HORIZONTAL_FAMILY: [TabLayout; 3] = [TabLayout::ToolbarTop, TabLayout::TabsTop, TabLayout::SingleRow];

    fn variant_layout(variant: TabLayout, width: i32, dpi: u32, tabs: usize) -> Layout {
        compute_for(variant, width, HEIGHT, dpi, tabs, 0, false)
    }

    /// Every native control the chrome reserves room for, with the three caption buttons split out.
    fn chrome_controls(l: &Layout) -> Vec<(String, Rect)> {
        let mut out = visible_controls(l);
        out.retain(|(name, _)| name != "caption");
        for button in [Hit::CaptionMinimize, Hit::CaptionMaximize, Hit::CaptionClose] {
            out.push((format!("{button:?}"), l.caption_button(button)));
        }
        for (name, rect) in [
            ("back", l.back_button),
            ("forward", l.forward_button),
            ("reload", l.reload_button),
            ("omnibox", l.omnibox),
            ("main menu", l.main_menu_button),
        ] {
            out.push((name.to_owned(), rect));
        }
        out.retain(|(_, rect)| !rect.is_empty());
        out
    }

    fn assert_tiles_without_overlap(l: &Layout, context: &str) {
        let controls = chrome_controls(l);
        for (i, (name_a, a)) in controls.iter().enumerate() {
            assert!(l.client.contains_rect(*a), "{context}: {name_a} {a:?} escapes the client");
            for (name_b, b) in &controls[i + 1..] {
                assert!(a.intersect(*b).is_empty(), "{context}: {name_a} {a:?} overlaps {name_b} {b:?}");
            }
        }
        // No row extends into the page, and the page does not reach into a row.
        for row in [l.strip, l.tab_row] {
            assert!(row.is_empty() || row.bottom <= l.viewport.top, "{context}: row {row:?} reaches the page {:?}", l.viewport);
        }
    }

    #[test]
    fn toolbar_top_plus_is_pinned_to_the_far_right_end_of_the_tab_row() {
        use crate::chrome_host::tab_row_bounds;
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            let margin = m.px(Metrics::OUTER_MARGIN);
            let gap = m.px(Metrics::ISLAND_GAP);
            for logical in [200, 300, 420, 520, 640, 768, 1000, 1024, 1440, 1920, 2200] {
                for tabs in [0, 1, 8, 40] {
                    for framed in [false, true] {
                        let width = m.px(logical);
                        let l = if framed {
                            compute_in_frame_for(TabLayout::ToolbarTop, width, HEIGHT, dpi, tabs, tabs / 2, false)
                        } else {
                            compute_for(TabLayout::ToolbarTop, width, HEIGHT, dpi, tabs, tabs / 2, false)
                        };
                        let context = format!("dpi {dpi} width {width} tabs {tabs} framed {framed}");
                        let plus = l.new_tab_button;
                        let list = l.tab_menu_button;

                        // The toolbar row never holds "+".
                        assert!(l.strip.intersect(plus).is_empty(), "{context}: \"+\" in the toolbar row");
                        assert!(l.main_menu_button.right <= l.caption_area.left - margin || l.main_menu_button.is_empty(), "{context}");
                        // It is there whenever the row has room for a minimum tab beside it.
                        if width >= m.px(420) {
                            assert!(!plus.is_empty(), "{context}: \"+\" must be present");
                        }
                        assert_disjoint(&l);
                        assert_inside_client(&l);
                        if plus.is_empty() {
                            continue;
                        }

                        // Inside the tab row, flush to its right end, centred vertically.
                        assert!(l.tab_row.contains_rect(plus), "{context}");
                        assert_eq!(plus.right, l.client.right - margin, "{context}: flush right");
                        assert_eq!(plus.width(), m.px(Metrics::NEW_TAB_SIZE), "{context}");
                        assert_eq!(plus.height(), m.px(Metrics::NEW_TAB_SIZE), "{context}");
                        let (above, below) = (plus.top - l.tab_row.top, l.tab_row.bottom - plus.bottom);
                        assert!((above - below).abs() <= 1, "{context}: {above} above, {below} below");
                        // Tabs fill the space left of it.
                        assert!(l.tab_zone.right + gap <= plus.left, "{context}");
                        if list.is_empty() {
                            assert_eq!(l.tab_zone.right + gap, plus.left, "{context}: tabs reach \"+\"");
                        } else {
                            assert!(l.tab_row.contains_rect(list), "{context}");
                            assert_eq!(list.right + gap, plus.left, "{context}: tab list just left of \"+\"");
                            assert!(l.tab_zone.right + gap <= list.left, "{context}");
                        }
                        for tab in l.tabs() {
                            assert!(tab.intersect(l.tab_zone).intersect(plus).is_empty(), "{context}");
                        }
                        // The HTML tab strip stops before the tab list and "+".
                        if let Some(host) = tab_row_bounds(&l) {
                            assert!(host.right + gap <= plus.left, "{context}");
                            if !list.is_empty() {
                                assert!(host.right + gap <= list.left, "{context}");
                            }
                        }
                        // Narrower than this the captions overlap navigation, which is not about "+".
                        if width < m.px(420) {
                            continue;
                        }
                        // "+" is a button, not window background.
                        assert_eq!(hit_test(&l, plus.left + plus.width() / 2, plus.top + plus.height() / 2), Hit::NewTab, "{context}");
                        assert_eq!(hit_test(&l, plus.left, plus.top), Hit::NewTab, "{context}");
                        assert_eq!(hit_test(&l, plus.right - 1, plus.bottom - 1), Hit::NewTab, "{context}");
                        // The window drags by empty toolbar and tab row, never by a control.
                        let drags = l.drag_region_rects();
                        for rect in &drags {
                            let (x, y) = (rect.left + rect.width() / 2, rect.top + rect.height() / 2);
                            assert_eq!(hit_test(&l, x, y), Hit::CaptionArea, "{context}: drag rect {rect:?}");
                            assert!(rect.intersect(plus).is_empty(), "{context}: drag rect over \"+\"");
                        }
                        let tab_mid = l.tab_row.top + l.tab_row.height() / 2;
                        let strip_mid = l.strip.top + l.strip.height() / 2;
                        // Between the menu bubble and the captions, in the toolbar row.
                        if !l.main_menu_button.is_empty() && l.main_menu_button.right + 2 < l.caption_area.left {
                            let x = l.main_menu_button.right + 1;
                            assert_eq!(hit_test(&l, x, strip_mid), Hit::CaptionArea, "{context}");
                            assert!(drags.iter().any(|rect| rect.contains(x, strip_mid)), "{context}");
                        }
                        // The tab row's empty space: the gap between the last tab and "+", and with
                        // no tabs the whole row.
                        let probe = if tabs == 0 { l.tab_zone.left + l.tab_zone.width() / 2 } else { plus.left - 1 };
                        if l.tab_zone.width() > 0 && l.tab(0).is_none_or(|first| first.right < probe) && list.is_empty() {
                            assert_eq!(hit_test(&l, probe, tab_mid), Hit::CaptionArea, "{context}");
                            assert!(drags.iter().any(|rect| rect.contains(probe, tab_mid)), "{context}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn toolbar_top_omnibox_keeps_its_centring_and_maximum_width_with_the_freed_room() {
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            for logical in [1440, 2200] {
                let width = m.px(logical);
                let l = compute_for(TabLayout::ToolbarTop, width, HEIGHT, dpi, 4, 0, false);
                assert_eq!(l.omnibox.width(), m.px(Metrics::OMNIBOX_MAX_WIDTH), "dpi {dpi} width {width}");
                let centre = l.omnibox.left + l.omnibox.width() / 2;
                assert!((centre - width / 2).abs() <= 1, "dpi {dpi} width {width}: omnibox centred");
                assert_eq!(l.main_menu_button.left, l.omnibox.right + m.px(Metrics::ISLAND_GAP), "dpi {dpi}");
            }
            // When tight, the menu reaches up to the captions, not to the old "+" slot.
            let tight = compute_for(TabLayout::ToolbarTop, m.px(700), HEIGHT, dpi, 4, 0, false);
            let limit = tight.caption_area.left - m.px(Metrics::OUTER_MARGIN);
            assert!(tight.main_menu_button.right <= limit, "dpi {dpi}");
            assert!(
                tight.main_menu_button.right + m.px(Metrics::NEW_TAB_SIZE) > limit
                    || tight.omnibox.width() == m.px(Metrics::OMNIBOX_MAX_WIDTH),
                "dpi {dpi}: the toolbar uses the room \"+\" no longer takes"
            );
        }
    }

    #[test]
    fn every_layout_places_its_controls_without_overlap_at_all_sizes() {
        for variant in HORIZONTAL_FAMILY {
            for dpi in DPIS {
                // From the narrowest window the toolbar still fits (navigation plus the captions).
                for logical in (330..=1600).step_by(23) {
                    let width = Metrics::new(dpi).px(logical);
                    for tabs in [0, 1, 3, 12, 40] {
                        let l = variant_layout(variant, width, dpi, tabs);
                        assert_tiles_without_overlap(&l, &format!("{variant:?} dpi {dpi} width {width} tabs {tabs}"));
                    }
                }
            }
        }
    }

    #[test]
    fn rows_stack_above_the_viewport_for_each_layout() {
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            let margin = m.px(Metrics::OUTER_MARGIN);
            for width in [900, 1200, 1600].map(|logical| m.px(logical)) {
                let top = variant_layout(TabLayout::ToolbarTop, width, dpi, 5);
                assert_eq!(top.strip.top, 0, "dpi {dpi}");
                assert_eq!(top.tab_row.top, top.strip.bottom + m.px(Metrics::STRIP_GAP), "dpi {dpi}");
                assert_eq!(top.viewport.top, top.tab_row.bottom + margin + m.px(Metrics::VIEWPORT_GAP), "dpi {dpi}");

                // The tab row is the top 30 logical pixels (with the captions); the toolbar follows.
                let tabs_top = variant_layout(TabLayout::TabsTop, width, dpi, 5);
                let band = m.px(Metrics::TOP_MARGIN) + m.px(Metrics::STRIP_HEIGHT);
                assert_eq!(tabs_top.strip.top, band, "dpi {dpi}");
                assert_eq!(tabs_top.strip.height(), m.px(Metrics::STRIP_HEIGHT), "dpi {dpi}");
                assert!(tabs_top.tab_row.top >= 0 && tabs_top.tab_row.bottom <= band, "dpi {dpi}: {:?}", tabs_top.tab_row);
                assert_eq!(tabs_top.viewport.top, tabs_top.strip.bottom + margin + m.px(Metrics::VIEWPORT_GAP), "dpi {dpi}");
                assert_eq!(tabs_top.caption_area.bottom, band, "dpi {dpi}");
                // The islands sit just under the window's top resize edge, not inside it.
                let island = tabs_top.tab(0).expect("a tab");
                assert!(island.top >= m.px(Metrics::ISLAND_MARGIN), "dpi {dpi}: island {island:?}");
                assert_eq!(island.height(), m.px(Metrics::ISLAND_HEIGHT), "dpi {dpi}");
                assert!(tabs_top.tab_row.contains_rect(island), "dpi {dpi}");

                // One row: the tab row is inside the 30 px toolbar row, and the page starts under it.
                let single = variant_layout(TabLayout::SingleRow, width, dpi, 5);
                assert_eq!(single.strip, Rect::new(0, m.px(Metrics::TOP_MARGIN), width, band), "dpi {dpi}");
                assert!(single.strip.contains_rect(single.tab_row), "dpi {dpi}");
                assert_eq!(single.viewport.top, single.strip.bottom + margin + m.px(Metrics::VIEWPORT_GAP), "dpi {dpi}");
                let island = single.tab(0).expect("a tab");
                assert_eq!(island.top, single.omnibox.top, "dpi {dpi}: tabs and omnibox share a line");
                assert_eq!(island.height(), m.px(Metrics::ISLAND_HEIGHT), "dpi {dpi}");

                for variant in HORIZONTAL_FAMILY {
                    let l = variant_layout(variant, width, dpi, 5);
                    assert_eq!(l.viewport.top, chrome_height(variant, dpi), "{variant:?} dpi {dpi}");
                    assert_eq!(content_rect(variant, width, HEIGHT, dpi, false), l.viewport, "{variant:?} dpi {dpi}");
                    assert_eq!(l.variant, variant);
                    // The caption buttons stay at the top right, as tall as the frame's caption strip.
                    assert_eq!(l.caption_area.right, width, "{variant:?} dpi {dpi}");
                    assert_eq!(l.caption_area.top, 0, "{variant:?} dpi {dpi}");
                    assert_eq!(l.caption_area.bottom, crate::win32::frame::caption_strip_height(dpi), "{variant:?} dpi {dpi}");
                }
            }
        }
    }

    #[test]
    fn omnibox_sits_two_logical_pixels_from_the_top_in_toolbar_top_and_single_row() {
        for variant in [TabLayout::ToolbarTop, TabLayout::SingleRow] {
            let l = variant_layout(variant, 1200, 96, 4);
            assert_eq!(l.omnibox.top, 2, "{variant:?}");
            assert_eq!(l.omnibox.height(), 26, "{variant:?}");
            for dpi in DPIS {
                let l = variant_layout(variant, 1400, dpi, 4);
                let m = Metrics::new(dpi);
                assert_eq!(l.omnibox.top, (m.px(Metrics::STRIP_HEIGHT) - m.px(Metrics::OMNIBOX_HEIGHT)) / 2, "{variant:?} dpi {dpi}");
            }
        }
        // Below the top row of tabs: the toolbar row's own vertical centring.
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            let l = variant_layout(TabLayout::TabsTop, 1400, dpi, 4);
            let band = m.px(Metrics::TOP_MARGIN) + m.px(Metrics::STRIP_HEIGHT);
            assert_eq!(l.omnibox.top, band + (m.px(Metrics::STRIP_HEIGHT) - m.px(Metrics::OMNIBOX_HEIGHT)) / 2, "dpi {dpi}");
            assert!(l.strip.contains_rect(l.omnibox) && l.strip.contains_rect(l.back_button), "dpi {dpi}");
            assert!(l.strip.contains_rect(l.main_menu_button), "dpi {dpi}");
        }
        assert_eq!(variant_layout(TabLayout::TabsTop, 1400, 96, 4).omnibox.top, 32);
    }

    #[test]
    fn toolbar_top_geometry_is_unchanged_by_the_layout_parameter() {
        for dpi in DPIS {
            for width in [300, 768, 1024, 1600] {
                for tabs in [1, 7, 40] {
                    assert_eq!(
                        compute_for(TabLayout::ToolbarTop, width, HEIGHT, dpi, tabs, tabs / 2, false),
                        compute_with_active(width, HEIGHT, dpi, tabs, tabs / 2, false),
                        "dpi {dpi} width {width} tabs {tabs}"
                    );
                }
            }
        }
        let l = compute_in_frame_for(TabLayout::ToolbarTop, 1024, HEIGHT, 96, 1, 0, false);
        let inset = left_rail_inset(96);
        assert_eq!(l.strip, Rect::new(inset, 0, 1024, 30));
        assert_eq!(l.tab_row, Rect::new(inset, 32, 1024, 60));
        assert_eq!(l.viewport.top, 63);
    }

    #[test]
    fn tabs_top_puts_plus_and_the_tab_list_in_the_top_row() {
        for dpi in DPIS {
            let l = variant_layout(TabLayout::TabsTop, 1000, dpi, 60);
            assert!(!l.new_tab_button.is_empty() && !l.tab_menu_button.is_empty(), "dpi {dpi}");
            assert!(l.new_tab_button.bottom <= l.strip.top, "dpi {dpi}: \"+\" is above the toolbar row");
            assert!(l.tab_row.contains_rect(l.tab_menu_button), "dpi {dpi}");
            assert!(l.tab_menu_button.right < l.new_tab_button.left, "dpi {dpi}: tab list, then \"+\"");
            assert!(l.new_tab_button.right < l.caption_area.left, "dpi {dpi}");
            assert!(l.tab_zone.right < l.tab_menu_button.left, "dpi {dpi}");
            // The row stops before "+" so the HTML tab host never covers it.
            assert!(l.tab_row.right <= l.new_tab_button.left, "dpi {dpi}");
            let host = crate::chrome_host::tab_row_bounds(&l).expect("tab host");
            assert!(host.right <= l.tab_menu_button.left, "dpi {dpi}");
            // The omnibox is centred in the whole window, as in ToolbarTop.
            let centre = l.omnibox.left + l.omnibox.width() / 2;
            assert!((centre - 500).abs() <= 2 * Metrics::new(dpi).px(Metrics::MENU_BUTTON_SIZE), "dpi {dpi}");
        }
    }

    #[test]
    fn tabs_top_omnibox_fills_the_toolbar_with_no_dead_space() {
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            for width in [1000, 1400, 2200] {
                let l = variant_layout(TabLayout::TabsTop, m.px(width), dpi, 4);
                let gap = m.px(Metrics::ISLAND_GAP);
                assert_eq!(l.omnibox.left, l.reload_button.right + gap, "dpi {dpi} width {width}: omnibox starts at navigation");
                assert_eq!(l.main_menu_button.left, l.omnibox.right + gap, "dpi {dpi} width {width}");
                assert_eq!(l.main_menu_button.right, l.client.right - m.px(Metrics::OUTER_MARGIN), "dpi {dpi} width {width}: menu at the right edge");
            }
        }
    }

    #[test]
    fn single_row_orders_navigation_omnibox_menu_tabs_plus_and_captions() {
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            let l = variant_layout(TabLayout::SingleRow, m.px(1400), dpi, 6);
            let order = [
                l.back_button, l.forward_button, l.reload_button, l.omnibox, l.main_menu_button,
                l.tab_zone, l.new_tab_button, l.caption_area,
            ];
            for pair in order.windows(2) {
                assert!(pair[0].right <= pair[1].left, "dpi {dpi}: {:?} then {:?}", pair[0], pair[1]);
            }
            assert_eq!(l.omnibox.width(), m.px(Metrics::SINGLE_ROW_OMNIBOX_WIDTH), "dpi {dpi}: the omnibox has a fixed width, not the centred maximum");
            assert!(l.omnibox.width() < m.px(Metrics::OMNIBOX_MAX_WIDTH), "dpi {dpi}");
            assert_eq!(l.omnibox.left, l.reload_button.right + m.px(Metrics::ISLAND_GAP), "dpi {dpi}");
            assert_eq!(l.main_menu_button.left, l.omnibox.right + m.px(Metrics::ISLAND_GAP), "dpi {dpi}");
            // The tabs take everything else: up to "+", which is pinned against the captions.
            assert_eq!(l.new_tab_button.right, l.caption_area.left - m.px(Metrics::OUTER_MARGIN), "dpi {dpi}");
            assert_eq!(l.tab_zone.right, l.new_tab_button.left - m.px(Metrics::ISLAND_GAP), "dpi {dpi}");
            assert!(l.tab_zone.width() > 4 * m.px(Metrics::SINGLE_ROW_OMNIBOX_WIDTH) / 3, "dpi {dpi}");
            assert!(l.tab_menu_button.is_empty(), "dpi {dpi}: six tabs fit");

            // Overflow: the tab list closes the tab zone, right before "+".
            let crowded = variant_layout(TabLayout::SingleRow, m.px(900), dpi, 60);
            assert!(!crowded.tab_menu_button.is_empty(), "dpi {dpi}");
            assert!(crowded.tab_row.contains_rect(crowded.tab_menu_button), "dpi {dpi}");
            assert_eq!(crowded.tab_menu_button.right, crowded.new_tab_button.left - m.px(Metrics::ISLAND_GAP), "dpi {dpi}");
            assert!(crowded.tab_zone.right < crowded.tab_menu_button.left, "dpi {dpi}");

            // A narrower window shrinks the omnibox (to its minimum) before the tabs lose their share.
            let tight = variant_layout(TabLayout::SingleRow, m.px(700), dpi, 6);
            assert!(tight.omnibox.width() < l.omnibox.width(), "dpi {dpi}");
            assert!(tight.omnibox.width() >= m.px(Metrics::SINGLE_ROW_OMNIBOX_MIN_WIDTH), "dpi {dpi}");
            assert!(!tight.main_menu_button.is_empty() && !tight.new_tab_button.is_empty(), "dpi {dpi}");
        }
    }

    #[test]
    fn hit_testing_follows_each_layouts_controls_and_drag_regions() {
        for variant in HORIZONTAL_FAMILY {
            for dpi in DPIS {
                let l = compute_in_frame_for(variant, 1400, HEIGHT, dpi, 4, 1, false);
                let centre = |r: Rect| (r.left + r.width() / 2, r.top + r.height() / 2);
                let at = |r: Rect| { let (x, y) = centre(r); hit_test(&l, x, y) };
                let context = format!("{variant:?} dpi {dpi}");
                assert_eq!(at(l.back_button), Hit::Back, "{context}");
                assert_eq!(at(l.forward_button), Hit::Forward, "{context}");
                assert_eq!(at(l.reload_button), Hit::Reload, "{context}");
                assert_eq!(at(l.main_menu_button), Hit::MainMenu, "{context}");
                assert_eq!(at(l.new_tab_button), Hit::NewTab, "{context}");
                assert_eq!(at(l.caption_button(Hit::CaptionClose)), Hit::CaptionClose, "{context}");
                assert_eq!(at(l.caption_button(Hit::CaptionMaximize)), Hit::CaptionMaximize, "{context}");
                assert_eq!(at(l.caption_button(Hit::CaptionMinimize)), Hit::CaptionMinimize, "{context}");
                assert_eq!(at(l.omnibox.intersect(Rect::new(l.omnibox.left + l.omnibox.width() / 3, 0, l.omnibox.right - l.omnibox.width() / 3, HEIGHT))), Hit::Omnibox, "{context}");
                assert_eq!(at(l.viewport), Hit::Viewport, "{context}");
                for index in 0..4 {
                    let island = l.tab(index).expect("tab");
                    let (x, y) = centre(island);
                    assert_eq!(hit_test(&l, x, y), Hit::Tab(index), "{context} tab {index}");
                }

                // Drag regions are exactly the empty chrome: each one hits as the caption area, and
                // no control or tab sits under one.
                let drags = l.drag_region_rects();
                assert!(!drags.is_empty(), "{context}");
                for rect in &drags {
                    assert!(!rect.is_empty(), "{context}: empty drag rect");
                    let (x, y) = centre(*rect);
                    assert_eq!(hit_test(&l, x, y), Hit::CaptionArea, "{context}: drag rect {rect:?}");
                }
                for (name, control) in chrome_controls(&l) {
                    assert!(drags.iter().all(|rect| rect.intersect(control).is_empty()), "{context}: drag region over {name} {control:?}");
                }
                // Empty tab row space drags the window too.
                let spare = l.tab_zone.right - 2;
                let y = l.tab_row.top + l.tab_row.height() / 2;
                if l.tab(3).is_some_and(|last| last.right < spare) {
                    assert!(drags.iter().any(|rect| rect.contains(spare, y)), "{context}");
                    assert_eq!(hit_test(&l, spare, y), Hit::CaptionArea, "{context}");
                }
            }
        }
    }

    #[test]
    fn tab_list_placed_for_group_slots_stays_inside_the_tab_row() {
        for variant in HORIZONTAL_FAMILY {
            for dpi in DPIS {
                for logical in (500..1400).step_by(20) {
                    let width = Metrics::new(dpi).px(logical);
                    let l = compute_in_frame_for(variant, width, HEIGHT, dpi, 4, 0, false)
                        .with_group_slots([false; 4], [true; 4], [false; 4]);
                    let context = format!("{variant:?} dpi {dpi} width {width}");
                    if !l.tab_menu_button.is_empty() {
                        assert!(l.tab_row.contains_rect(l.tab_menu_button), "{context}: {:?} not in {:?}", l.tab_menu_button, l.tab_row);
                        assert!(l.new_tab_button.is_empty() || l.tab_menu_button.intersect(l.new_tab_button).is_empty(), "{context}");
                        assert!(l.tab_menu_button.intersect(l.caption_area).is_empty(), "{context}");
                        assert!(l.tab_zone.right <= l.tab_menu_button.left, "{context}");
                    }
                    assert_tiles_without_overlap(&l, &context);
                }
            }
        }
    }

    #[test]
    fn vertical_reserves_no_tab_row_and_the_page_starts_under_the_toolbar() {
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            for tabs in [1, 8, 60] {
                let (l, sidebar) = compute_vertical_in_frame(1300, HEIGHT, dpi, tabs, tabs - 1, false, 260);
                let context = format!("dpi {dpi} tabs {tabs}");
                assert_eq!(l.variant, TabLayout::Vertical);
                assert!(l.tab_row.is_empty() && l.tab_zone.is_empty() && l.tab_menu_button.is_empty(), "{context}");
                assert_eq!(l.tabs().count(), 0, "{context}: no tab islands in the top chrome");
                assert!(l.tab(0).is_none(), "{context}");
                assert_eq!(l.hidden_tab_indices().count(), 0, "{context}: nothing is hidden by overflow");
                // The toolbar is the only row: 30 logical pixels, then the margin, then the page.
                assert_eq!(l.strip.top, 0, "{context}");
                assert_eq!(l.strip.height(), m.px(Metrics::STRIP_HEIGHT), "{context}");
                assert_eq!(l.viewport.top, l.strip.bottom + m.px(Metrics::OUTER_MARGIN) + m.px(Metrics::VIEWPORT_GAP), "{context}");
                assert_eq!(l.viewport.top, chrome_height(TabLayout::Vertical, dpi), "{context}");
                assert_eq!(sidebar.viewport.top, l.viewport.top, "{context}");
                assert!(l.viewport.top < chrome_height(TabLayout::ToolbarTop, dpi), "{context}");
                assert_eq!(l.viewport.left, sidebar.sidebar.right, "{context}");
                // The toolbar starts right of the sidebar, with the caption buttons at the top right.
                assert_eq!(l.strip.left, sidebar.sidebar.right, "{context}");
                assert!(l.back_button.left >= sidebar.sidebar.right, "{context}");
                assert_eq!(l.caption_area.right, 1300, "{context}");

                // Nothing in the window hit-tests as a top tab, tab close or tab list, and the
                // space where the row would have been is page.
                for y in (0..HEIGHT).step_by(5) {
                    for x in (0..1300).step_by(9) {
                        let hit = hit_test(&l, x, y);
                        assert!(!matches!(hit, Hit::Tab(_) | Hit::TabClose(_) | Hit::TabMenu | Hit::GroupBall(_)), "{context}: {hit:?} at {x},{y}");
                    }
                }
                let old_row_y = l.strip.bottom + m.px(Metrics::STRIP_GAP) + m.px(Metrics::TAB_ROW_HEIGHT) / 2;
                assert_eq!(hit_test(&l, l.viewport.left + 40, old_row_y), Hit::Viewport, "{context}");
                // Drag regions cover only the toolbar row.
                for rect in l.drag_region_rects() {
                    assert!(l.strip.contains_rect(rect), "{context}: drag rect {rect:?} outside the toolbar row");
                }
                // The chrome host has no tab strip to cover, and the toolbar host is unchanged.
                assert!(crate::chrome_host::tab_row_bounds(&l).is_none(), "{context}");
                assert!(crate::chrome_host::toolbar_bounds(&l).is_some(), "{context}");
                assert_tiles_without_overlap(&l, &context);
            }
        }
    }

    #[test]
    fn layouts_keep_the_viewport_clear_of_every_row_in_a_narrow_window() {
        for variant in [TabLayout::ToolbarTop, TabLayout::TabsTop, TabLayout::SingleRow, TabLayout::Vertical] {
            for dpi in DPIS {
                let l = compute_for(variant, 300, 200, dpi, 5, 0, false);
                assert_eq!(l.viewport.top, chrome_height(variant, dpi).min(200), "{variant:?} dpi {dpi}");
                assert!(l.viewport.width() >= 0 && l.viewport.height() >= 0);
            }
        }
    }

    /// Page-overrun report (left rail + top strip, maximised 1920 x 1079 at 96 DPI): the page host
    /// bounds the app gives WebView2 (`content_rect` right of the rail, as `chrome_viewport_bounds`
    /// builds them) must be exactly the island the overlay frames and masks (`Layout::viewport`),
    /// clear of the rail and every chrome row, restored or maximised, at every DPI.
    #[test]
    fn page_bounds_are_the_framed_island_beside_the_rail() {
        for variant in HORIZONTAL_FAMILY {
            for dpi in DPIS {
                for (w, h) in [(1920, 1079), (1904, 1041), (1280, 720), (2560, 1400)] {
                    for maximised in [false, true] {
                        let context = format!("{variant:?} {w}x{h} dpi {dpi} maximised {maximised}");
                        let l = compute_in_frame_for_with(variant, w, h, dpi, 5, 0, LayoutOptions { maximised, centred: true });
                        let inset = left_rail_inset(dpi);
                        let page = content_rect(variant, w - inset, h, dpi, maximised).shifted_right(inset);
                        assert_eq!(page, l.viewport, "{context}");
                        assert!(page.left >= l.rail.right, "{context}: page {page:?} under the rail {:?}", l.rail);
                        assert!(page.top >= l.strip.bottom && page.top >= l.tab_row.bottom, "{context}: page {page:?} under a row");
                        assert_eq!(page.bottom, if maximised { h } else { h - Metrics::new(dpi).px(Metrics::RESIZE_RIM) }, "{context}");
                    }
                }
            }
        }
        // The configuration in the report: a 1920 x 1079 maximised client at 96 DPI.
        let l = compute_in_frame_for_with(TabLayout::ToolbarTop, 1920, 1079, 96, 5, 0, LayoutOptions { maximised: true, centred: true });
        assert_eq!(l.viewport, Rect::new(30, 63, 1920, 1079));
        assert_eq!(l.rail, Rect::new(2, 2, 28, 1077));
    }

    // --- auto-hide (T11) ---
    const SIZES: [(i32, i32); 3] = [(1920, 1079), (1280, 720), (800, 600)];

    /// The user's rule: auto-hide off, the layouts are exactly what they were. Every field of every
    /// layout, for every variant, DPI, size and window state.
    #[test]
    fn auto_hide_off_changes_nothing() {
        for variant in HORIZONTAL_FAMILY {
            for dpi in DPIS {
                for (w, h) in SIZES {
                    for maximised in [false, true] {
                        for centred in [false, true] {
                            let context = format!("{variant:?} {w}x{h} dpi {dpi} max {maximised} centred {centred}");
                            let before = compute_in_frame_for_with(variant, w, h, dpi, 5, 2, LayoutOptions { maximised, centred });
                            let after = compute_in_frame_auto_hide(variant, w, h, dpi, 5, 2, maximised, centred, AutoHide::OFF);
                            assert_eq!(before, after, "{context}");
                            assert_eq!(
                                auto_hide_page_rect(variant, w, h, dpi, maximised, 260, AutoHide::OFF),
                                before.viewport,
                                "{context}: the app's page rectangle"
                            );
                        }
                    }
                }
            }
        }
        for dpi in DPIS {
            for (w, h) in SIZES {
                for maximised in [false, true] {
                    let context = format!("vertical {w}x{h} dpi {dpi} max {maximised}");
                    let before = compute_vertical_in_frame_with(w, h, dpi, 5, 2, 260, LayoutOptions { maximised, centred: true });
                    let after = compute_vertical_in_frame_auto_hide(w, h, dpi, 5, 2, maximised, 260, true, AutoHide::OFF);
                    assert_eq!(before, after, "{context}");
                    assert_eq!(
                        auto_hide_page_rect(TabLayout::Vertical, w, h, dpi, maximised, 260, AutoHide::OFF),
                        before.0.viewport,
                        "{context}: the app's page rectangle"
                    );
                }
            }
        }
    }

    /// The hidden top chrome only moves the top of the page; the bars keep every rectangle they have
    /// when shown, so a revealed bar is drawn where it always is, and nothing else about the layout moves.
    #[test]
    fn hiding_the_top_chrome_moves_only_the_pages_top_edge() {
        for variant in HORIZONTAL_FAMILY {
            for dpi in DPIS {
                for (w, h) in SIZES {
                    for maximised in [false, true] {
                        let context = format!("{variant:?} {w}x{h} dpi {dpi} max {maximised}");
                        let shown = compute_in_frame_for_with(variant, w, h, dpi, 5, 2, LayoutOptions { maximised, centred: true });
                        let hide = AutoHide { top: true, sidebar: false };
                        let hidden = compute_in_frame_auto_hide(variant, w, h, dpi, 5, 2, maximised, true, hide);
                        assert_eq!(hidden.viewport.top, auto_hide_top(dpi, maximised), "{context}");
                        assert_eq!(Layout { viewport: shown.viewport, ..hidden }, shown, "{context}: only the viewport differs");
                        assert_eq!(
                            (hidden.viewport.left, hidden.viewport.right, hidden.viewport.bottom),
                            (shown.viewport.left, shown.viewport.right, shown.viewport.bottom),
                            "{context}"
                        );
                        assert!(hidden.viewport.top < shown.viewport.top, "{context}: the page gained the bars' room");
                        assert_eq!(
                            auto_hide_page_rect(variant, w, h, dpi, maximised, 260, hide),
                            hidden.viewport,
                            "{context}: the app and the window agree"
                        );
                        // Where the bars float when revealed: from the top edge to the shown page's top.
                        let overlay = auto_hide_overlay(&hidden, None, dpi, false);
                        assert_eq!(overlay.top, Rect::new(left_rail_inset(dpi), 0, w, shown.viewport.top), "{context}");
                        assert!(overlay.top.contains_rect(hidden.strip), "{context}: the toolbar is inside it");
                        assert!(overlay.sidebar.is_empty(), "{context}");
                    }
                }
            }
        }
    }

    #[test]
    fn hiding_the_sidebar_and_the_toolbar_in_the_vertical_layout() {
        for dpi in DPIS {
            for (w, h) in SIZES {
                for maximised in [false, true] {
                    let context = format!("vertical {w}x{h} dpi {dpi} max {maximised}");
                    let (shown, sidebar) = compute_vertical_in_frame_with(w, h, dpi, 5, 2, 260, LayoutOptions { maximised, centred: true });
                    let inset = left_rail_inset(dpi);
                    let rim = if maximised { 0 } else { Metrics::new(dpi).px(Metrics::RESIZE_RIM) };
                    // The sidebar alone: the page starts at the rail; the toolbar follows it left.
                    let hide = AutoHide { top: false, sidebar: true };
                    let (side_hidden, hidden_sidebar) = compute_vertical_in_frame_auto_hide(w, h, dpi, 5, 2, maximised, 260, true, hide);
                    assert_eq!(hidden_sidebar, sidebar, "{context}: the sidebar keeps its shown rectangle");
                    assert_eq!(side_hidden.viewport.left, inset + rim, "{context}");
                    assert_eq!(
                        (side_hidden.viewport.top, side_hidden.viewport.right, side_hidden.viewport.bottom),
                        (shown.viewport.top, shown.viewport.right, shown.viewport.bottom),
                        "{context}"
                    );
                    assert!(side_hidden.strip.left < shown.strip.left, "{context}: the toolbar moved into the sidebar's place");
                    assert!(side_hidden.strip.left >= inset, "{context}: and not under the rail");
                    assert_eq!(side_hidden.rail, shown.rail, "{context}");
                    assert_eq!(
                        auto_hide_page_rect(TabLayout::Vertical, w, h, dpi, maximised, 260, hide),
                        side_hidden.viewport,
                        "{context}: the app and the window agree"
                    );
                    // The toolbar alone: the sidebar stays and the page rises.
                    let hide = AutoHide { top: true, sidebar: false };
                    let (top_hidden, _) = compute_vertical_in_frame_auto_hide(w, h, dpi, 5, 2, maximised, 260, true, hide);
                    assert_eq!(top_hidden.strip, shown.strip, "{context}");
                    assert_eq!(top_hidden.viewport.left, shown.viewport.left, "{context}");
                    assert_eq!(top_hidden.viewport.top, auto_hide_top(dpi, maximised), "{context}");
                    assert_eq!(
                        auto_hide_page_rect(TabLayout::Vertical, w, h, dpi, maximised, 260, hide),
                        top_hidden.viewport,
                        "{context}"
                    );
                    // Both.
                    let hide = AutoHide { top: true, sidebar: true };
                    let (both, _) = compute_vertical_in_frame_auto_hide(w, h, dpi, 5, 2, maximised, 260, true, hide);
                    assert_eq!(both.viewport, Rect::new(inset + rim, auto_hide_top(dpi, maximised), shown.viewport.right, shown.viewport.bottom), "{context}");
                    assert_eq!(auto_hide_page_rect(TabLayout::Vertical, w, h, dpi, maximised, 260, hide), both.viewport, "{context}");
                    // Floating: the sidebar over the page's left, the toolbar from the rail (or the sidebar) on.
                    let overlay = auto_hide_overlay(&both, Some(&sidebar), dpi, true);
                    assert_eq!(overlay.sidebar, sidebar.sidebar, "{context}");
                    assert_eq!(overlay.top.left, inset, "{context}");
                    let overlay = auto_hide_overlay(&top_hidden, Some(&sidebar), dpi, false);
                    assert_eq!(overlay.top.left, sidebar.sidebar.right, "{context}: the shown sidebar is not covered");
                }
            }
        }
    }

    /// Revealing a bar is an overlay: the page rectangle is the same shown or hidden, so nothing
    /// relays out and the page never gets a `resize`.
    #[test]
    fn the_page_rectangle_does_not_depend_on_whether_a_hidden_bar_is_revealed() {
        let hide = AutoHide { top: true, sidebar: false };
        let a = compute_in_frame_auto_hide(TabLayout::ToolbarTop, 1280, 720, 144, 4, 1, false, true, hide);
        let b = compute_in_frame_auto_hide(TabLayout::ToolbarTop, 1280, 720, 144, 4, 1, false, true, hide);
        assert_eq!(a.viewport, b.viewport);
        let overlay = auto_hide_overlay(&a, None, 144, false);
        assert!(overlay.top.bottom > a.viewport.top, "the revealed bars cover the top of the page");
        assert_eq!(overlay.top.bottom, chrome_height(TabLayout::ToolbarTop, 144));
    }

    // --- layout (thread T6) + auto-hide (T11) ---
    fn arrangements() -> Vec<Arrangement> {
        let mut out = Vec::new();
        for tabs in TabLayout::ALL {
            for tab_bar in [true, false] {
                for address_bar in [BarPosition::Top, BarPosition::Bottom] {
                    for address_bar_visible in [true, false] {
                        for title_bar in [false, true] {
                            for stack_row in [StackRow::None, StackRow::Regular, StackRow::Compact] {
                                out.push(Arrangement { tabs, tab_bar, address_bar, address_bar_visible, title_bar, stack_row });
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// Auto-hide off, every arrangement is laid out exactly as `compute_arranged_in_frame` lays it
    /// out: the rule T6 and T11 share.
    #[test]
    fn arranged_auto_hide_off_changes_nothing() {
        for arrangement in arrangements() {
            for dpi in [96, 144] {
                for (w, h) in SIZES {
                    for maximised in [false, true] {
                        let context = format!("{arrangement:?} {w}x{h} dpi {dpi} max {maximised}");
                        let before = compute_arranged_in_frame(arrangement, w, h, dpi, 5, 2, maximised, 260, true, 36);
                        let after = compute_arranged_auto_hide(arrangement, w, h, dpi, 5, 2, maximised, 260, true, 36, AutoHide::OFF);
                        assert_eq!(before, after, "{context}");
                        assert_eq!(auto_hide_arranged_viewport(before.0.viewport, before.0.client, arrangement, AutoHide::OFF, dpi, maximised), before.0.viewport, "{context}");
                    }
                }
            }
        }
    }

    /// For the plain layouts the arranged auto-hide geometry is the plain auto-hide geometry: the
    /// page rectangle agrees with `compute_in_frame_auto_hide` / `compute_vertical_in_frame_auto_hide`.
    #[test]
    fn arranged_auto_hide_matches_the_plain_functions_for_the_plain_layouts() {
        for dpi in DPIS {
            for (w, h) in SIZES {
                for maximised in [false, true] {
                    for hide in [AutoHide { top: true, sidebar: false }, AutoHide { top: false, sidebar: true }, AutoHide { top: true, sidebar: true }] {
                        for variant in HORIZONTAL_FAMILY {
                            let arrangement = Arrangement { tabs: variant, ..Arrangement::default() };
                            let plain = compute_in_frame_auto_hide(variant, w, h, dpi, 5, 2, maximised, true, hide);
                            let (arranged, _) = compute_arranged_auto_hide(arrangement, w, h, dpi, 5, 2, maximised, 260, true, 36, hide);
                            assert_eq!(arranged.viewport, plain.viewport, "{variant:?} {w}x{h} dpi {dpi} max {maximised} {hide:?}");
                        }
                        let arrangement = Arrangement { tabs: TabLayout::Vertical, ..Arrangement::default() };
                        let (plain, plain_sidebar) = compute_vertical_in_frame_auto_hide(w, h, dpi, 5, 2, maximised, 260, true, hide);
                        let (arranged, sidebar) = compute_arranged_auto_hide(arrangement, w, h, dpi, 5, 2, maximised, 260, true, crate::layout_prefs::UiDensity::Regular.sidebar_row_height(), hide);
                        let context = format!("vertical {w}x{h} dpi {dpi} max {maximised} {hide:?}");
                        assert_eq!(arranged.viewport, plain.viewport, "{context}");
                        assert_eq!(sidebar.map(|s| s.sidebar), Some(plain_sidebar.sidebar), "{context}: the sidebar keeps its shown rectangle");
                        if hide.sidebar {
                            assert!(arranged.strip.left < compute_arranged_in_frame(arrangement, w, h, dpi, 5, 2, maximised, 260, true, crate::layout_prefs::UiDensity::Regular.sidebar_row_height()).0.strip.left, "{context}: the toolbar moved into the sidebar's place");
                        }
                    }
                }
            }
        }
    }

    /// The arrangements T11 never saw: a right sidebar widens the page to the right; rows along the
    /// bottom edge go with the top chrome; a revealed bar's overlay covers what the page gained.
    #[test]
    fn arranged_auto_hide_takes_right_sidebars_and_bottom_rows_out_too() {
        for dpi in DPIS {
            for maximised in [false, true] {
                let (w, h) = (1280, 720);
                let rim = if maximised { 0 } else { Metrics::new(dpi).px(Metrics::RESIZE_RIM) };
                let right = Arrangement { tabs: TabLayout::VerticalRight, ..Arrangement::default() };
                let (shown, shown_sidebar) = compute_arranged_in_frame(right, w, h, dpi, 5, 2, maximised, 260, true, 36);
                let hide = AutoHide { top: false, sidebar: true };
                let (hidden, sidebar) = compute_arranged_auto_hide(right, w, h, dpi, 5, 2, maximised, 260, true, 36, hide);
                assert_eq!(hidden.viewport.right, w - rim, "right sidebar hidden: the page runs to the window's edge (dpi {dpi} max {maximised})");
                assert_eq!((hidden.viewport.left, hidden.viewport.top, hidden.viewport.bottom), (shown.viewport.left, shown.viewport.top, shown.viewport.bottom));
                assert_eq!(sidebar.map(|s| s.sidebar), shown_sidebar.map(|s| s.sidebar), "the sidebar keeps its shown rectangle");
                let overlay = auto_hide_overlay_arranged(&shown, &hidden, sidebar.as_ref(), right, dpi, true);
                assert_eq!(overlay.sidebar, shown_sidebar.map(|s| s.sidebar).unwrap_or_default());
                assert!(overlay.bottom.is_empty());
                for bottom in [
                    Arrangement { tabs: TabLayout::TabsBottom, ..Arrangement::default() },
                    Arrangement { address_bar: BarPosition::Bottom, ..Arrangement::default() },
                    Arrangement { tabs: TabLayout::TabsBottom, address_bar: BarPosition::Bottom, ..Arrangement::default() },
                ] {
                    let (shown, _) = compute_arranged_in_frame(bottom, w, h, dpi, 5, 2, maximised, 260, true, 36);
                    let hide = AutoHide { top: true, sidebar: false };
                    let (hidden, _) = compute_arranged_auto_hide(bottom, w, h, dpi, 5, 2, maximised, 260, true, 36, hide);
                    let context = format!("{bottom:?} dpi {dpi} max {maximised}");
                    assert_eq!(hidden.viewport.top, auto_hide_top(dpi, maximised), "{context}");
                    assert_eq!(hidden.viewport.bottom, h - rim, "{context}: the bottom rows went too");
                    assert!(shown.viewport.bottom < hidden.viewport.bottom, "{context}");
                    assert_eq!(Layout { viewport: shown.viewport, ..hidden }, shown, "{context}: only the viewport differs");
                    let overlay = auto_hide_overlay_arranged(&shown, &hidden, None, bottom, dpi, false);
                    assert_eq!(overlay.top, Rect::new(left_rail_inset(dpi), 0, w, shown.viewport.top), "{context}");
                    assert_eq!(overlay.bottom, Rect::new(left_rail_inset(dpi), shown.viewport.bottom, w, h), "{context}");
                    assert!(overlay.bottom.contains_rect(hidden.strip) || overlay.bottom.contains_rect(hidden.tab_row), "{context}: a bottom row is inside the bottom band");
                }
                // A hidden tab bar has no sidebar to hide: `hide.sidebar` is nothing there.
                let no_tabs = Arrangement { tabs: TabLayout::Vertical, tab_bar: false, ..Arrangement::default() };
                let (shown, _) = compute_arranged_in_frame(no_tabs, w, h, dpi, 5, 2, maximised, 260, true, 36);
                let (hidden, sidebar) = compute_arranged_auto_hide(no_tabs, w, h, dpi, 5, 2, maximised, 260, true, 36, AutoHide { top: false, sidebar: true });
                assert_eq!(hidden, shown);
                assert!(sidebar.is_none());
            }
        }
    }
    // --- end layout + auto-hide ---
    // --- end auto-hide ---
}

// --- layout (thread T6) ---
#[cfg(test)]
mod arrangement_tests {
    use super::*;
    use crate::chrome::rail::RailView;
    use crate::vertical_tabs::{COMPACT_TAB_ROW_HEIGHT, SidebarHit, TAB_ROW_HEIGHT};

    const DPIS: [u32; 4] = [96, 120, 144, 192];

    fn arranged(arrangement: Arrangement, dpi: u32) -> (Layout, Option<crate::vertical_tabs::SidebarLayout>) {
        compute_arranged_in_frame(arrangement, 1400, 900, dpi, 5, 1, false, 260, true, TAB_ROW_HEIGHT)
    }

    fn hit(layout: &Layout, x: i32, y: i32) -> Hit {
        hit_test_native(layout, None::<&RailView<'_>>, &[], x, y)
    }

    #[test]
    fn the_default_arrangement_is_the_toolbar_above_tabs_layout_unchanged() {
        for dpi in DPIS {
            let (layout, sidebar) = arranged(Arrangement::default(), dpi);
            let plain = compute_in_frame_for_with(TabLayout::ToolbarTop, 1400, 900, dpi, 5, 1, LayoutOptions { maximised: false, centred: true });
            assert!(sidebar.is_none());
            assert_eq!(layout.strip, plain.strip, "dpi {dpi}");
            assert_eq!(layout.tab_row, plain.tab_row, "dpi {dpi}");
            assert_eq!(layout.omnibox, plain.omnibox, "dpi {dpi}");
            assert_eq!(layout.viewport, plain.viewport, "dpi {dpi}");
            assert!(layout.title_bar.is_empty());
            assert_eq!(layout.top_chrome, plain.viewport.top);
            assert_eq!(layout.bottom_chrome, 900, "no chrome along the bottom edge");
            assert!(!layout.toolbar_at_bottom());
        }
        // Every plain variant comes out of the arranged path the same way.
        for variant in [TabLayout::TabsTop, TabLayout::SingleRow] {
            let (layout, _) = arranged(Arrangement { tabs: variant, ..Default::default() }, 96);
            let plain = compute_in_frame_for_with(variant, 1400, 900, 96, 5, 1, LayoutOptions { maximised: false, centred: true });
            assert_eq!((layout.strip, layout.tab_row, layout.viewport), (plain.strip, plain.tab_row, plain.viewport), "{variant:?}");
        }
        let (layout, sidebar) = arranged(Arrangement { tabs: TabLayout::Vertical, ..Default::default() }, 96);
        let (plain, plain_sidebar) = compute_vertical_in_frame_with(1400, 900, 96, 5, 1, 260, LayoutOptions { maximised: false, centred: true });
        assert_eq!((layout.strip, layout.viewport), (plain.strip, plain.viewport));
        assert_eq!(sidebar.unwrap().sidebar, plain_sidebar.sidebar);
    }

    #[test]
    fn tabs_along_the_bottom_edge_sit_under_the_page_with_the_toolbar_still_on_top() {
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            let (layout, sidebar) = arranged(Arrangement { tabs: TabLayout::TabsBottom, ..Default::default() }, dpi);
            assert!(sidebar.is_none());
            assert_eq!(layout.strip.top, 0, "dpi {dpi}");
            assert_eq!(layout.tab_row.bottom, 900 - m.px(Metrics::OUTER_MARGIN), "dpi {dpi}");
            assert!(layout.tab_row.top > layout.strip.bottom);
            assert_eq!(layout.viewport.top, chrome_height(TabLayout::TabsBottom, dpi), "dpi {dpi}");
            assert_eq!(layout.viewport.bottom, layout.tab_row.top - m.px(Metrics::OUTER_MARGIN) - m.px(Metrics::VIEWPORT_GAP), "dpi {dpi}");
            assert_eq!(layout.bottom_chrome, layout.viewport.bottom);
            assert_eq!(layout.viewport.bottom, content_rect(TabLayout::TabsBottom, 1400 - left_rail_inset(dpi), 900, dpi, false).bottom, "content_rect agrees, dpi {dpi}");
            // The islands and "+" are in the bottom row, and a click there is a tab, not a drag.
            let island = layout.tab(1).expect("a tab island");
            assert!(layout.tab_row.contains_rect(island), "dpi {dpi}");
            assert!(layout.tab_row.contains_rect(layout.new_tab_button), "dpi {dpi}");
            assert_eq!(hit(&layout, (island.left + island.right) / 2, (island.top + island.bottom) / 2), Hit::Tab(1));
            assert_eq!(hit(&layout, layout.viewport.left + 10, layout.viewport.top + 10), Hit::Viewport);
            // The HTML tab strip host covers the bottom row.
            let bounds = crate::chrome_host::tab_row_bounds(&layout).expect("a tab row host");
            assert_eq!((bounds.top, bounds.bottom), (layout.tab_row.top, layout.tab_row.bottom));
            // No drag region ever lies over the page.
            for rect in layout.drag_region_rects() {
                assert!(rect.intersect(layout.viewport).is_empty(), "drag region {rect:?} over the page, dpi {dpi}");
            }
        }
        assert!(Arrangement { tabs: TabLayout::TabsBottom, ..Default::default() }.thin_bottom_edge());
        assert!(!Arrangement::default().thin_bottom_edge());
    }

    #[test]
    fn a_right_sidebar_hangs_under_the_toolbar_at_the_window_edge_with_its_handle_inside() {
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            let rim = m.px(Metrics::RESIZE_RIM);
            let (layout, sidebar) = arranged(Arrangement { tabs: TabLayout::VerticalRight, ..Default::default() }, dpi);
            let sidebar = sidebar.expect("a right sidebar");
            assert_eq!(sidebar.side, SidePanelPosition::Right);
            assert_eq!(sidebar.sidebar.right, 1400 - rim, "dpi {dpi}");
            assert_eq!(sidebar.sidebar.width(), m.px(260), "dpi {dpi}");
            assert_eq!(sidebar.sidebar.top, layout.strip.bottom + m.px(Metrics::STRIP_GAP), "under the toolbar, dpi {dpi}");
            assert_eq!(sidebar.handle.left, sidebar.sidebar.left, "the handle is on the inner edge, dpi {dpi}");
            assert_eq!(layout.viewport.right, sidebar.sidebar.left, "dpi {dpi}");
            assert_eq!(layout.viewport.left, left_rail_inset(dpi) + rim, "dpi {dpi}");
            assert_eq!(layout.viewport, sidebar.viewport);
            assert!(layout.tab_row.is_empty() && layout.tab(0).is_none(), "no row of tabs");
            // The toolbar keeps the full width: the window buttons stay at the top right.
            assert_eq!(layout.caption_area.right, 1400);
            assert!(layout.main_menu_button.right <= layout.caption_area.left);
            // The HTML sidebar host stops at the handle; the handle is a resize hit.
            let bounds = crate::chrome_host::sidebar_bounds(&sidebar).expect("a sidebar host");
            assert_eq!((bounds.left, bounds.right), (sidebar.handle.right, sidebar.sidebar.right));
            assert_eq!(crate::vertical_tabs::hit_test(&sidebar, sidebar.handle.left + 1, sidebar.rows.top + 5, 0, 5), Some(SidebarHit::Resize));
            // A drag of the handle measures the width from the window's edge.
            assert_eq!(sidebar.drag_width_px(sidebar.sidebar.right - 300), 300);
            assert_eq!(sidebar.content_left(), sidebar.handle.right);
            assert_eq!(sidebar.content_right(), sidebar.sidebar.right);
        }
        let (_, left) = arranged(Arrangement { tabs: TabLayout::Vertical, ..Default::default() }, 96);
        let left = left.unwrap();
        assert_eq!(left.drag_width_px(300), 300, "a left sidebar keeps measuring from the window's left edge");
        assert_eq!((left.content_left(), left.content_right()), (left.rows.left, left.handle.left));
    }

    #[test]
    fn a_hidden_tab_bar_leaves_the_toolbar_alone_with_the_new_tab_button_in_it() {
        for tabs in TabLayout::ALL {
            let (layout, sidebar) = arranged(Arrangement { tabs, tab_bar: false, ..Default::default() }, 96);
            assert!(sidebar.is_none(), "{tabs:?}");
            assert!(layout.tab_row.is_empty() && layout.tab(0).is_none(), "{tabs:?}");
            assert_eq!(crate::chrome_host::tab_row_bounds(&layout), None, "{tabs:?}");
            assert_eq!(layout.viewport.top, chrome_height(TabLayout::Vertical, 96), "{tabs:?}");
            assert_eq!(layout.viewport.bottom, 900 - Metrics::new(96).px(Metrics::RESIZE_RIM), "{tabs:?}");
            assert!(layout.strip.contains_rect(layout.new_tab_button), "{tabs:?}: + moves into the toolbar");
            assert!(layout.title_bar.is_empty());
        }
    }

    #[test]
    fn the_title_row_sits_above_everything_with_the_window_buttons_in_it() {
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            let strip_h = m.px(Metrics::STRIP_HEIGHT);
            for tabs in [TabLayout::Vertical, TabLayout::VerticalRight] {
                let (layout, sidebar) = arranged(Arrangement { tabs, title_bar: true, ..Default::default() }, dpi);
                let sidebar = sidebar.unwrap();
                assert_eq!(layout.title_bar, Rect::new(left_rail_inset(dpi), 0, 1400, strip_h), "{tabs:?} dpi {dpi}");
                assert_eq!(layout.strip.top, strip_h, "the toolbar moves down, {tabs:?} dpi {dpi}");
                assert!(layout.strip.contains_rect(layout.omnibox));
                assert!(layout.title_bar.contains_rect(layout.caption_button(Hit::CaptionClose)), "{tabs:?} dpi {dpi}");
                assert_eq!(layout.viewport.top, layout.strip.bottom + m.px(Metrics::OUTER_MARGIN) + m.px(Metrics::VIEWPORT_GAP), "{tabs:?} dpi {dpi}");
                assert_eq!(layout.top_chrome, layout.viewport.top);
                assert!(sidebar.sidebar.top >= strip_h, "the sidebar starts under the title row, {tabs:?} dpi {dpi}");
                assert_eq!(hit(&layout, layout.title_bar.left + 20, strip_h / 2), Hit::CaptionArea);
                assert_eq!(hit(&layout, 1400 - 10, strip_h / 2), Hit::CaptionClose);
                assert!(layout.drag_region_rects().iter().any(|rect| rect.top == 0 && rect.bottom == strip_h), "the title row drags the window");
            }
        }
        // Asked for with the tabs in a row, nothing changes: the toolbar row already holds the buttons.
        let (layout, _) = arranged(Arrangement { title_bar: true, ..Default::default() }, 96);
        assert!(layout.title_bar.is_empty());
        assert!(!Arrangement { title_bar: true, ..Default::default() }.has_title_row());
    }

    #[test]
    fn the_address_bar_along_the_bottom_edge_leaves_a_row_at_the_top_for_the_window_buttons() {
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            let margin = m.px(Metrics::OUTER_MARGIN);
            let gap = m.px(Metrics::VIEWPORT_GAP);
            let strip_h = m.px(Metrics::STRIP_HEIGHT);
            // Tabs in a row: the row of tabs takes the top with the window buttons.
            for tabs in [TabLayout::ToolbarTop, TabLayout::TabsTop, TabLayout::SingleRow] {
                let arrangement = Arrangement { tabs, address_bar: BarPosition::Bottom, ..Default::default() };
                assert_eq!(arrangement.base_variant(), TabLayout::TabsTop, "{tabs:?}");
                assert!(!arrangement.has_title_row(), "{tabs:?}");
                let (layout, _) = arranged(arrangement, dpi);
                assert!(layout.title_bar.is_empty());
                assert_eq!(layout.strip.bottom, 900 - margin, "{tabs:?} dpi {dpi}");
                assert!(layout.toolbar_at_bottom());
                assert!(layout.strip.contains_rect(layout.omnibox) && layout.strip.contains_rect(layout.main_menu_button), "{tabs:?} dpi {dpi}");
                assert!(layout.strip.contains_rect(layout.back_button), "{tabs:?} dpi {dpi}");
                assert!(layout.tab_row.top < strip_h, "the tabs stay at the top, {tabs:?} dpi {dpi}");
                assert!(layout.tab_row.contains_rect(layout.tab(1).unwrap()));
                assert_eq!(layout.viewport.top, layout.tab_row.bottom + margin + gap, "{tabs:?} dpi {dpi}");
                assert_eq!(layout.viewport.bottom, layout.strip.top - margin - gap, "{tabs:?} dpi {dpi}");
                assert_eq!(layout.bottom_chrome, layout.viewport.bottom);
                assert_eq!(hit(&layout, (layout.omnibox.left + layout.omnibox.right) / 2, (layout.omnibox.top + layout.omnibox.bottom) / 2), Hit::Omnibox);
                assert_eq!(hit(&layout, 1400 - 10, 5), Hit::CaptionClose);
                let bounds = crate::chrome_host::toolbar_bounds(&layout).expect("a toolbar host");
                assert_eq!((bounds.top, bounds.bottom), (layout.strip.top, layout.strip.bottom));
                for rect in layout.drag_region_rects() {
                    assert!(rect.intersect(layout.viewport).is_empty(), "drag region {rect:?} over the page, {tabs:?} dpi {dpi}");
                }
            }
            // Tabs on a side, along the bottom or hidden: a title row at the top holds the buttons.
            for (tabs, tab_bar) in [(TabLayout::Vertical, true), (TabLayout::VerticalRight, true), (TabLayout::TabsBottom, true), (TabLayout::ToolbarTop, false)] {
                let arrangement = Arrangement { tabs, tab_bar, address_bar: BarPosition::Bottom, ..Default::default() };
                assert!(arrangement.has_title_row(), "{tabs:?} {tab_bar}");
                let (layout, sidebar) = arranged(arrangement, dpi);
                assert_eq!(layout.title_bar.height(), strip_h, "{tabs:?} {tab_bar} dpi {dpi}");
                assert_eq!(layout.strip.bottom, 900 - margin, "{tabs:?} {tab_bar} dpi {dpi}");
                assert_eq!(layout.viewport.top, strip_h + margin + gap, "{tabs:?} {tab_bar} dpi {dpi}");
                if tabs == TabLayout::TabsBottom {
                    assert_eq!(layout.tab_row.bottom, layout.strip.top - m.px(Metrics::STRIP_GAP), "the tab row sits above the toolbar, dpi {dpi}");
                    assert_eq!(layout.viewport.bottom, layout.tab_row.top - margin - gap, "dpi {dpi}");
                } else {
                    assert_eq!(layout.viewport.bottom, layout.strip.top - margin - gap, "{tabs:?} {tab_bar} dpi {dpi}");
                }
                if let Some(sidebar) = sidebar {
                    assert!(sidebar.sidebar.top >= strip_h, "{tabs:?} dpi {dpi}");
                    if sidebar.side == SidePanelPosition::Right {
                        assert!(sidebar.sidebar.bottom <= layout.strip.top, "a right sidebar ends above the toolbar, dpi {dpi}");
                    }
                }
            }
        }
        assert!(Arrangement { address_bar: BarPosition::Bottom, ..Default::default() }.thin_bottom_edge());
    }

    /// With the toolbar along the bottom edge the hit test agrees with the layout's rects: a point
    /// is `CaptionArea` exactly where a drag region is, so the margins over the page (between it and
    /// the bottom toolbar, beside a sidebar) are not a caption, and the bottom toolbar's empty space is.
    #[test]
    fn caption_hits_follow_the_drag_regions_with_the_address_bar_at_the_bottom() {
        for dpi in [96, 144] {
            for tabs in TabLayout::ALL {
                for tab_bar in [true, false] {
                    let arrangement = Arrangement { tabs, tab_bar, address_bar: BarPosition::Bottom, ..Default::default() };
                    let (layout, _) = arranged(arrangement, dpi);
                    let context = format!("{tabs:?} tab bar {tab_bar} dpi {dpi}");
                    let drags = layout.drag_region_rects();
                    for y in (0..layout.client.bottom).step_by(3) {
                        // Left of the toolbar is the margin beside the rail: it drags in the
                        // toolbar's bands only, checked below.
                        for x in (layout.strip.left..layout.client.right).step_by(7) {
                            let caption = hit(&layout, x, y) == Hit::CaptionArea;
                            let dragged = drags.iter().any(|rect| rect.contains(x, y));
                            assert_eq!(caption, dragged, "{context}: ({x}, {y}) caption {caption} drag region {dragged}");
                        }
                    }
                    // The gap between the page and the toolbar is no caption.
                    let y = layout.strip.top - 1;
                    assert_ne!(hit(&layout, layout.viewport.left + 40, y), Hit::CaptionArea, "{context}");
                    assert_ne!(hit(&layout, layout.strip.left - 1, layout.viewport.top + 40), Hit::CaptionArea, "{context}: the margin beside the page");
                    assert_ne!(hit(&layout, 0, layout.viewport.top + 40), Hit::CaptionArea, "{context}: the margin beside the rail, by the page");
                    assert_eq!(hit(&layout, 0, layout.strip.top + 2), Hit::CaptionArea, "{context}: the margin beside the rail, by the toolbar");
                    // The bottom toolbar's empty space between its controls drags the window.
                    let spare = Rect::new(layout.omnibox.right, layout.strip.top, layout.main_menu_button.left, layout.strip.bottom);
                    if spare.width() > 2 {
                        assert_eq!(hit(&layout, spare.left + 1, spare.top + spare.height() / 2), Hit::CaptionArea, "{context}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_hidden_address_bar_has_no_toolbar_until_it_is_revealed() {
        let hidden = Arrangement { tabs: TabLayout::Vertical, address_bar_visible: false, ..Default::default() };
        let (layout, sidebar) = arranged(hidden, 96);
        assert!(layout.strip.is_empty() && layout.omnibox.is_empty() && layout.main_menu_button.is_empty() && layout.back_button.is_empty());
        assert_eq!(crate::chrome_host::toolbar_bounds(&layout), None, "the toolbar host hides");
        assert!(layout.new_tab_button.is_empty(), "the toolbar's + goes with it (the sidebar has its own)");
        assert_eq!(layout.title_bar.height(), 30, "the window buttons need a row");
        assert_eq!(layout.viewport.top, 33);
        assert!(sidebar.unwrap().sidebar.top >= 30);
        assert_eq!(hit(&layout, layout.viewport.left + 5, layout.viewport.bottom + 2), Hit::None, "nothing below the page");
        // Tabs in a row keep their row (with the buttons) and lose only the toolbar.
        let (rows, _) = arranged(Arrangement { tabs: TabLayout::ToolbarTop, address_bar_visible: false, ..Default::default() }, 96);
        assert!(rows.title_bar.is_empty());
        assert!(rows.strip.is_empty());
        assert!(rows.tab_row.top < 30 && rows.tab(1).is_some());
        assert_eq!(rows.viewport.top, rows.tab_row.bottom + 3);
        // Ctrl+L: the revealed arrangement is the visible one.
        let revealed = hidden.with_address_bar_revealed(true);
        assert!(revealed.address_bar_visible);
        assert_eq!(arranged(revealed, 96).0.strip, arranged(Arrangement { tabs: TabLayout::Vertical, ..Default::default() }, 96).0.strip);
        assert_eq!(Arrangement::default().with_address_bar_revealed(false), Arrangement::default());
    }

    #[test]
    fn the_compact_density_shortens_the_native_sidebar_rows() {
        let vertical = Arrangement { tabs: TabLayout::Vertical, ..Default::default() };
        let regular = compute_arranged_in_frame(vertical, 1400, 900, 96, 5, 1, false, 260, true, TAB_ROW_HEIGHT).1.unwrap();
        let compact = compute_arranged_in_frame(vertical, 1400, 900, 96, 5, 1, false, 260, true, COMPACT_TAB_ROW_HEIGHT).1.unwrap();
        assert_eq!(regular.row_height(), 36);
        assert_eq!(compact.row_height(), 30);
        assert_eq!(crate::layout_prefs::UiDensity::Regular.sidebar_row_height(), TAB_ROW_HEIGHT);
        assert_eq!(crate::layout_prefs::UiDensity::Compact.sidebar_row_height(), COMPACT_TAB_ROW_HEIGHT);
        let rows = crate::chrome::vertical_paint::row_bounds(&compact.with_tab_count(20, 96), 1, Metrics::new(96), 0);
        assert_eq!(rows.top, compact.rows.top + 30 + 2);
        assert_eq!(compact.with_tab_count(100, 96).max_scroll, 100 * 30 - compact.rows.height());
    }

    #[test]
    fn every_arrangement_keeps_the_page_inside_the_window_at_every_dpi() {
        for dpi in DPIS {
            for tabs in TabLayout::ALL {
                for tab_bar in [true, false] {
                    for address_bar in BarPosition::ALL {
                        for address_bar_visible in [true, false] {
                            for (title_bar, stack_row) in [true, false].into_iter().flat_map(|t| [StackRow::None, StackRow::Regular, StackRow::Compact].map(|r| (t, r))) {
                                let arrangement = Arrangement { tabs, tab_bar, address_bar, address_bar_visible, title_bar, stack_row: StackRow::None }.with_stack_row(stack_row);
                                for (w, h) in [(1400, 900), (500, 300), (120, 80)] {
                                    let (layout, sidebar) = compute_arranged_in_frame(arrangement, w, h, dpi, 5, 1, false, 260, true, TAB_ROW_HEIGHT);
                                    let v = layout.viewport;
                                    assert!(v.left >= 0 && v.top >= 0 && v.right <= w && v.bottom <= h && v.left <= v.right && v.top <= v.bottom, "{arrangement:?} {w}x{h} dpi {dpi}: {v:?}");
                                    assert!(layout.top_chrome <= layout.bottom_chrome, "{arrangement:?} {w}x{h} dpi {dpi}");
                                    for rect in [layout.strip, layout.tab_row, layout.title_bar, layout.stack_row] {
                                        assert!(rect.is_empty() || rect.intersect(v).is_empty(), "{arrangement:?} {w}x{h} dpi {dpi}: {rect:?} over the page {v:?}");
                                    }
                                    // --- two-level stacks (T10b) --- the second row overlaps no other row.
                                    for rect in [layout.strip, layout.tab_row, layout.title_bar] {
                                        assert!(layout.stack_row.is_empty() || rect.is_empty() || rect.intersect(layout.stack_row).is_empty(), "{arrangement:?} {w}x{h} dpi {dpi}: {rect:?} under the second row");
                                    }
                                    if let Some(sidebar) = sidebar {
                                        assert!(sidebar.sidebar.intersect(v).is_empty() || sidebar.sidebar.width() == 0, "{arrangement:?} {w}x{h} dpi {dpi}: sidebar over the page");
                                    }
                                    // The window buttons are always at the top right, where the frame expects them.
                                    assert_eq!(layout.caption_area.top, 0);
                                    assert_eq!(layout.caption_area.right, w);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // --- two-level stacks (T10b) ---
    /// The arrangements with a tab row at the top or the bottom, toolbar shown at the top or bottom.
    fn rows() -> Vec<Arrangement> {
        let mut out = Vec::new();
        for tabs in [TabLayout::ToolbarTop, TabLayout::TabsTop, TabLayout::SingleRow, TabLayout::TabsBottom] {
            for address_bar in BarPosition::ALL {
                out.push(Arrangement { tabs, address_bar, ..Default::default() });
            }
            out.push(Arrangement { tabs, address_bar_visible: false, ..Default::default() });
        }
        out
    }

    #[test]
    fn without_a_second_row_every_arrangement_is_laid_out_as_before() {
        for dpi in DPIS {
            for tabs in TabLayout::ALL {
                for tab_bar in [true, false] {
                    let plain = Arrangement { tabs, tab_bar, ..Default::default() };
                    let none = plain.with_stack_row(StackRow::None);
                    assert_eq!(none, plain);
                    let (layout, sidebar) = arranged(none, dpi);
                    assert_eq!((layout, sidebar), arranged(plain, dpi), "{tabs:?} dpi {dpi}");
                    assert!(layout.stack_row.is_empty() && layout.stack_tabs(3).is_empty());
                    if let Some(host) = crate::chrome_host::tab_row_bounds(&layout) {
                        assert_eq!((host.top, host.bottom), (layout.tab_row.top, layout.tab_row.bottom), "{tabs:?}: the strip host is the tab row alone");
                    }
                }
            }
        }
        // Only two-level stacks with the tab in use in a stack ask for a row; every other style, and
        // a tab in no stack, keep the layout above.
        use crate::tab_options::StackStyle;
        for style in StackStyle::ALL.iter().copied() {
            for compact in [false, true] {
                assert_eq!(crate::stacks::stack_row(style, compact, false), StackRow::None, "{style:?}");
                let wanted = match (style, compact) {
                    (StackStyle::TwoLevel, false) => StackRow::Regular,
                    (StackStyle::TwoLevel, true) => StackRow::Compact,
                    _ => StackRow::None,
                };
                assert_eq!(crate::stacks::stack_row(style, compact, true), wanted, "{style:?} compact {compact}");
            }
        }
    }

    #[test]
    fn a_sidebar_keeps_its_own_stacks_and_reserves_no_second_row() {
        for dpi in DPIS {
            for tabs in [TabLayout::Vertical, TabLayout::VerticalRight] {
                let plain = Arrangement { tabs, ..Default::default() };
                let asked = plain.with_stack_row(StackRow::Regular);
                assert_eq!(asked.stack_row, StackRow::None, "{tabs:?}");
                assert_eq!(arranged(asked, dpi), arranged(plain, dpi), "{tabs:?} dpi {dpi}");
                // Even an arrangement built by hand with a row lays none out beside a sidebar.
                let forced = Arrangement { stack_row: StackRow::Regular, ..plain };
                assert!(arranged(forced, dpi).0.stack_row.is_empty(), "{tabs:?} dpi {dpi}");
            }
            let hidden = Arrangement { tab_bar: false, ..Default::default() }.with_stack_row(StackRow::Regular);
            assert_eq!(hidden.stack_row, StackRow::None, "no tab bar, no second row");
        }
    }

    #[test]
    fn the_second_row_takes_its_height_and_a_gap_under_the_tab_row_at_every_dpi() {
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            for arrangement in rows() {
                let (plain, _) = arranged(arrangement, dpi);
                for row in [StackRow::Regular, StackRow::Compact] {
                    let (two, sidebar) = arranged(arrangement.with_stack_row(row), dpi);
                    let context = format!("{arrangement:?} {row:?} dpi {dpi}");
                    assert!(sidebar.is_none(), "{context}");
                    let band = m.px(Metrics::STRIP_GAP) + m.px(row.height_dip());
                    assert_eq!(two.stack_row.height(), m.px(row.height_dip()), "{context}");
                    assert_eq!((two.stack_row.left, two.stack_row.right), (two.tab_row.left, two.tab_row.right), "{context}: as wide as the tab row");
                    // The toolbar keeps STRIP_HEIGHT (`--bw-toolbar-h`), the tab row its own height.
                    assert_eq!(two.strip.height(), plain.strip.height(), "{context}");
                    assert_eq!(two.tab_row.height(), plain.tab_row.height(), "{context}");
                    assert!(two.stack_row.top >= two.tab_row.bottom + m.px(Metrics::STRIP_GAP), "{context}: under the tab row");
                    let host = crate::chrome_host::tab_row_bounds(&two).expect("a strip host");
                    assert_eq!((host.top, host.bottom), (two.tab_row.top, two.stack_row.bottom), "{context}: the strip page draws both rows");
                    assert!(!two.stack_tabs(2).is_empty(), "{context}");
                    if arrangement.tabs_at_bottom() {
                        assert_eq!(two.viewport.top, plain.viewport.top, "{context}");
                        assert_eq!(two.viewport.bottom, plain.viewport.bottom - band, "{context}: the page ends a row higher");
                        assert_eq!(two.tab_row.top, plain.tab_row.top - band, "{context}: the tab row moves up");
                        assert_eq!(two.stack_row.bottom, plain.tab_row.bottom, "{context}: the second row takes its place");
                        assert_eq!(two.strip, plain.strip, "{context}: a toolbar below stays put");
                    } else {
                        assert_eq!(two.viewport.bottom, plain.viewport.bottom, "{context}");
                        // The page keeps its margin under the lowest row at the top, now one the
                        // second row's band lower (a pixel or two less in `SingleRow`, whose tabs
                        // sit inside the taller toolbar row).
                        let lowest = |l: &Layout| [l.strip, l.tab_row, l.title_bar, l.stack_row].iter().filter(|r| !r.is_empty() && r.top < 450).map(|r| r.bottom).max().unwrap_or(0);
                        assert_eq!(two.viewport.top - lowest(&two), plain.viewport.top - lowest(&plain), "{context}");
                        let moved = two.viewport.top - plain.viewport.top;
                        if arrangement.tabs == TabLayout::SingleRow && two.strip.contains_rect(two.tab_row) {
                            assert!(moved <= band && moved >= band - m.px(Metrics::STRIP_GAP), "{context}: moved {moved}");
                        } else {
                            assert_eq!(moved, band, "{context}");
                        }
                        assert_eq!(two.tab_row, plain.tab_row, "{context}: the tab row stays put");
                        if !plain.strip.is_empty() && plain.strip.top > plain.tab_row.bottom && plain.strip.top < 450 {
                            assert_eq!(two.strip.top, plain.strip.top + band, "{context}: a toolbar under the tabs moves down");
                        }
                    }
                    for drag in two.drag_region_rects() {
                        assert!(drag.intersect(two.stack_row).is_empty(), "{context}: the second row is not a drag band");
                    }
                }
            }
        }
    }

    #[test]
    fn the_page_moves_by_exactly_the_gap_and_the_row_in_the_toolbar_above_tabs_layout() {
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            let (plain, _) = arranged(Arrangement::default(), dpi);
            for (row, dip) in [(StackRow::Regular, 28), (StackRow::Compact, 22)] {
                let (two, _) = arranged(Arrangement::default().with_stack_row(row), dpi);
                assert_eq!(two.viewport.top - plain.viewport.top, m.px(2) + m.px(dip), "{row:?} dpi {dpi}");
                assert_eq!(two.stack_row.top, plain.tab_row.bottom + m.px(Metrics::STRIP_GAP), "{row:?} dpi {dpi}");
            }
        }
        // The physical heights the row gets: 96, 120, 144 and 192 DPI.
        let heights = |dip: i32| DPIS.map(|dpi| Metrics::new(dpi).px(dip));
        assert_eq!(heights(Metrics::STACK_ROW_HEIGHT), [28, 35, 42, 56]);
        assert_eq!(heights(Metrics::STACK_ROW_COMPACT_HEIGHT), [22, 28, 33, 44]);
        assert_eq!(Metrics::STACK_ROW_HEIGHT, Metrics::TAB_ROW_HEIGHT, "the second row is as tall as the first");
    }

    #[test]
    fn the_second_row_shares_its_width_among_the_stack_and_keeps_tabs_readable() {
        let (layout, _) = arranged(Arrangement::default().with_stack_row(StackRow::Regular), 96);
        let row = layout.stack_row;
        let three = layout.stack_tabs(3);
        assert_eq!(three.len(), 3);
        assert!(three.windows(2).all(|pair| pair[1].left - pair[0].right == Metrics::ISLAND_GAP), "an island gap apart");
        assert!(three.iter().all(|tab| tab.top == row.top + 1 && tab.bottom == row.bottom - 1 && tab.width() == three[0].width()));
        assert!(three[2].right <= row.right && three[0].left >= row.left);
        let many = layout.stack_tabs(500);
        assert!(!many.is_empty() && many.len() < 500, "tabs past the row's end are left out");
        assert!(many.iter().all(|tab| tab.width() >= Metrics::TAB_MIN_WIDTH && tab.right <= row.right));
        let hit = three[1];
        assert!(layout.drag_region_rects().iter().all(|drag| !drag.contains(hit.left + 4, hit.top + 4)));
    }
    // --- end two-level stacks ---
}
// --- end layout ---
