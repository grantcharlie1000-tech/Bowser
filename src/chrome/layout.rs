//! Pure chrome geometry and hit-testing (glass UI work in progress; see docs/superpowers/specs/2026-10-09-bowser-glass-ui-design.md)
//!
//! Coordinates are client-relative physical pixels, top to bottom:
//!
//! ```text
//! +-------------------------------------------------------------+
//! | tab strip (40): islands, "+", ........ caption buttons     |
//! +-------------------------------------------------------------+
//! | 8 px row gap                                                |
//! | omnibox row (36), 8 px inset from both sides                |
//! | 8 px margin                                                 |
//! +-------------------------------------------------------------+
//! | viewport (page content, to the bottom edge)                 |
//! ```
//!
//! Tab overflow: tabs shrink evenly to a minimum width. Past that, the visible window onto the
//! row shifts just enough to keep the active tab visible; the "+" button stays pinned just left of
//! the caption buttons. There is no scrolling UI.

use windows::Win32::Foundation::RECT;

/// Logical (96 dpi) chrome metrics, scaled to physical pixels with [`Metrics::px`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metrics {
    dpi: u32,
}

impl Metrics {
    pub const STRIP_HEIGHT: i32 = 40;
    pub const ISLAND_HEIGHT: i32 = 32;
    /// Space above and below an island inside the strip.
    pub const ISLAND_MARGIN: i32 = 4;
    /// Gap between neighbouring islands, and between the last island and "+".
    pub const ISLAND_GAP: i32 = 6;
    pub const TAB_MAX_WIDTH: i32 = 220;
    pub const TAB_MIN_WIDTH: i32 = 96;
    /// Titles are hidden in tabs narrower than this.
    pub const TITLE_MIN_TAB_WIDTH: i32 = 120;
    pub const NEW_TAB_SIZE: i32 = 32;
    pub const CAPTION_BUTTON_WIDTH: i32 = 46;
    pub const OMNIBOX_HEIGHT: i32 = 36;
    /// Inset of the omnibox text from the left and right edges of the field.
    pub const OMNIBOX_TEXT_INSET: i32 = 12;
    /// Inset of the omnibox from the window edges, and the margin below it.
    pub const OUTER_MARGIN: i32 = 8;
    pub const ROW_GAP: i32 = 8;
    /// Width of the translucent rim around the page on the left, right and bottom of a window that
    /// is not maximised. The page is a child window and wins every hit test over its own pixels, so
    /// without the rim there would be nowhere left to grab those three resize edges.
    pub const RESIZE_RIM: i32 = 4;
    pub const ISLAND_RADIUS: i32 = 10;
    pub const OMNIBOX_RADIUS: i32 = 18;
    pub const FAVICON_SIZE: i32 = 16;
    pub const CLOSE_SIZE: i32 = 20;
    /// Inset from the island's left edge to the favicon, and the gap before the title.
    pub const TAB_PADDING: i32 = 8;
    pub const TEXT_GAP: i32 = 6;
    /// Inset from the island's right edge to the close button.
    pub const CLOSE_INSET: i32 = 6;

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

    pub const fn omnibox_radius(self) -> i32 {
        self.px(Self::OMNIBOX_RADIUS)
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
}

/// Chrome geometry for one window size. Build with [`compute`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layout {
    pub metrics: Metrics,
    pub maximised: bool,
    pub client: Rect,
    pub strip: Rect,
    /// Horizontal span that tabs may occupy. Tabs are clipped to it, and anything past it is hidden.
    pub tab_zone: Rect,
    pub new_tab_button: Rect,
    /// Reserved for the min / max / close buttons, at the right end of the strip.
    pub caption_area: Rect,
    pub omnibox: Rect,
    pub viewport: Rect,
    tab_count: usize,
    tab_width: i32,
    tab_left: i32,
    first_visible: usize,
    visible_count: usize,
    island_top: i32,
    island_bottom: i32,
}

/// Height of the painted chrome: the strip, the omnibox row and the margin below it. This is where
/// `Layout::viewport` starts.
pub const fn chrome_height(dpi: u32) -> i32 {
    let m = Metrics::new(dpi);
    m.px(Metrics::STRIP_HEIGHT)
        + m.px(Metrics::ROW_GAP)
        + m.px(Metrics::OMNIBOX_HEIGHT)
        + m.px(Metrics::OUTER_MARGIN)
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
    let new_tab_size = m.px(Metrics::NEW_TAB_SIZE);
    let max_w = m.px(Metrics::TAB_MAX_WIDTH);
    let min_w = m.px(Metrics::TAB_MIN_WIDTH);

    let client = Rect::new(0, 0, client_w, client_h);
    let strip = Rect::new(0, 0, client_w, strip_h);

    let caption_w = 3 * m.px(Metrics::CAPTION_BUTTON_WIDTH);
    let caption_left = (client_w - caption_w).max(0);
    let caption_area = Rect::new(caption_left, 0, client_w, strip_h);

    // Tabs start at the same inset as the omnibox. "+" is pinned to the right of the tab zone,
    // so it is never clipped even when the tabs overflow.
    let tabs_left = margin;
    let zone_right = (caption_left - margin - new_tab_size - gap).max(tabs_left);
    let tab_zone = Rect::new(tabs_left, 0, zone_right, strip_h);

    let n = tab_count as i32;
    let mut tab_width = if tab_count == 0 {
        max_w
    } else {
        let available = zone_right - tabs_left - (n - 1) * gap;
        (available / n).clamp(min_w, max_w)
    };
    // Keep a final partially clipped island in the tab zone. It remains a usable hit target and
    // gives the active tab the same edge position as it moves through overflow.
    let stride_at_min = min_w + gap;
    let min_visible =
        ((zone_right - tabs_left + stride_at_min - 1) / stride_at_min).max(1) as usize;
    let overflow = tab_count > min_visible && tab_width == min_w;
    let visible_count = tab_count.min(min_visible);
    if overflow {
        tab_width = min_w;
    }
    let first_visible = if overflow {
        active_index
            .min(tab_count.saturating_sub(1))
            .saturating_sub(visible_count.saturating_sub(1))
            .min(tab_count.saturating_sub(visible_count))
    } else {
        0
    };
    let stride = tab_width + gap;
    let row_end = tabs_left
        + (visible_count as i32 - 1).max(0) * stride
        + if visible_count > 0 { tab_width } else { 0 };

    let new_tab_left = if tab_count == 0 {
        tabs_left
    } else if overflow {
        zone_right + gap
    } else {
        row_end.min(zone_right) + gap
    };
    let new_tab_top = (strip_h - new_tab_size) / 2;
    let new_tab_button = Rect::new(
        new_tab_left,
        new_tab_top,
        new_tab_left + new_tab_size,
        new_tab_top + new_tab_size,
    );

    let island_top = m.px(Metrics::ISLAND_MARGIN);
    let island_bottom = island_top + m.px(Metrics::ISLAND_HEIGHT);

    let omnibox_top = strip_h + m.px(Metrics::ROW_GAP);
    let omnibox_bottom = omnibox_top + m.px(Metrics::OMNIBOX_HEIGHT);
    let omnibox = Rect::new(margin, omnibox_top, client_w - margin, omnibox_bottom);
    let viewport_top = (omnibox_bottom + margin).min(client_h);
    let rim = if maximised {
        0
    } else {
        m.px(Metrics::RESIZE_RIM).min(client_w / 2)
    };
    let viewport = Rect::new(
        rim,
        viewport_top,
        client_w - rim,
        (client_h - rim).max(viewport_top),
    );

    Layout {
        metrics: m,
        maximised,
        client,
        strip,
        tab_zone,
        new_tab_button,
        caption_area,
        omnibox,
        viewport,
        tab_count,
        tab_width,
        tab_left: tabs_left,
        first_visible,
        visible_count,
        island_top,
        island_bottom,
    }
}

/// Which part of the chrome a client-relative point is over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Hit {
    None,
    Tab(usize),
    TabClose(usize),
    NewTab,
    Omnibox,
    Viewport,
    /// Empty strip. Dragging here moves the window.
    CaptionArea,
    CaptionMinimize,
    CaptionMaximize,
    CaptionClose,
}

impl Layout {
    pub fn tab_count(&self) -> usize {
        self.tab_count
    }

    pub fn first_visible_tab(&self) -> usize {
        self.first_visible
    }

    fn stride(&self) -> i32 {
        self.tab_width + self.metrics.px(Metrics::ISLAND_GAP)
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
        let left = self.tab_left + (index - self.first_visible) as i32 * self.stride();
        Some(Rect::new(
            left,
            self.island_top,
            left + self.tab_width,
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

        let favicon_left = tab.left + m.px(Metrics::TAB_PADDING);
        let favicon = square_in(favicon_left, favicon_size, tab);

        let close_left = tab.right - m.px(Metrics::CLOSE_INSET) - close_size;
        let close = square_in(close_left, close_size, tab);

        let title_left = favicon.right + gap;
        let title_right = close.left - gap;
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
        }
    }

    fn tab_centre(&self, index: usize) -> i32 {
        self.tab_left
            + (index as i32 - self.first_visible as i32) * self.stride()
            + self.tab_width / 2
    }

    /// Index of the tab slot under `x`, if `x` is inside a visible tab column. Vertical position
    /// is not checked.
    fn tab_index_at(&self, x: i32) -> Option<usize> {
        if x < self.tab_zone.left || x >= self.tab_zone.right || self.tab_count == 0 {
            return None;
        }
        let offset = x - self.tab_left;
        let slot = (offset / self.stride()) as usize;
        if slot >= self.visible_count || offset % self.stride() >= self.tab_width {
            return None;
        }
        Some(self.first_visible + slot)
    }

    /// Rects of the empty strip that drag the window. The result is the strip minus every
    /// control: each visible island, "+", and the caption area. Each island and "+" leaves the
    /// bands above and below it, so the strip is covered exactly and the rects do not overlap.
    /// [`hit_test`] returns `CaptionArea` for exactly these points.
    pub fn drag_region_rects(&self) -> Vec<Rect> {
        let mut controls: Vec<Rect> = Vec::with_capacity(self.visible_count + 2);
        for tab in self.tabs() {
            let visible = tab.intersect(self.tab_zone);
            if visible.is_empty() {
                break;
            }
            controls.push(visible);
        }
        controls.push(self.new_tab_button);
        controls.push(self.caption_area);

        let strip = self.strip;
        let mut rects = Vec::new();
        let mut cursor = strip.left;
        for control in controls {
            if control.left > cursor {
                rects.push(Rect::new(cursor, strip.top, control.left, strip.bottom));
            }
            if control.top > strip.top {
                rects.push(Rect::new(
                    control.left,
                    strip.top,
                    control.right,
                    control.top,
                ));
            }
            if control.bottom < strip.bottom {
                rects.push(Rect::new(
                    control.left,
                    control.bottom,
                    control.right,
                    strip.bottom,
                ));
            }
            cursor = cursor.max(control.right);
        }
        rects
    }
}

fn square_in(left: i32, size: i32, band: Rect) -> Rect {
    let top = band.top + (band.height() - size) / 2;
    Rect::new(left, top, left + size, top + size)
}

/// Classifies a client-relative point. Points in the gaps between the omnibox, viewport and strip
/// return `None`.
pub fn hit_test(layout: &Layout, x: i32, y: i32) -> Hit {
    if !layout.client.contains(x, y) {
        return Hit::None;
    }
    if layout.viewport.contains(x, y) {
        return Hit::Viewport;
    }
    if layout.omnibox.contains(x, y) {
        return Hit::Omnibox;
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
    if layout.new_tab_button.contains(x, y) {
        return Hit::NewTab;
    }
    let Some(index) = layout.tab_index_at(x) else {
        return Hit::CaptionArea;
    };
    let Some(island) = layout.tab(index) else {
        return Hit::CaptionArea;
    };
    if !island.contains(x, y) {
        return Hit::CaptionArea;
    }
    if layout.tab_parts(island).close.contains(x, y) {
        Hit::TabClose(index)
    } else {
        Hit::Tab(index)
    }
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

        assert_eq!(m.px(Metrics::STRIP_HEIGHT), 40);
        assert_eq!(m.px(Metrics::ISLAND_HEIGHT), 32);
        assert_eq!(m.px(Metrics::ISLAND_MARGIN), 4);
        assert_eq!(m.px(Metrics::ISLAND_GAP), 6);
        assert_eq!(m.px(Metrics::TAB_MAX_WIDTH), 220);
        assert_eq!(m.px(Metrics::TAB_MIN_WIDTH), 96);
        assert_eq!(m.px(Metrics::NEW_TAB_SIZE), 32);
        assert_eq!(m.px(Metrics::CAPTION_BUTTON_WIDTH), 46);
        assert_eq!(m.px(Metrics::OMNIBOX_HEIGHT), 36);
        assert_eq!(m.px(Metrics::OUTER_MARGIN), 8);
        assert_eq!(m.px(Metrics::ROW_GAP), 8);
        assert_eq!(m.island_radius(), 10);
        assert_eq!(m.omnibox_radius(), 18);
    }

    #[test]
    fn metrics_scale_at_120_dpi_with_round_half_up() {
        let m = Metrics::new(120);

        assert_eq!(m.px(Metrics::STRIP_HEIGHT), 50);
        assert_eq!(m.px(Metrics::ISLAND_HEIGHT), 40);
        assert_eq!(m.px(Metrics::ISLAND_MARGIN), 5);
        assert_eq!(m.px(Metrics::ISLAND_GAP), 8, "7.5 rounds up");
        assert_eq!(m.px(Metrics::TAB_MAX_WIDTH), 275);
        assert_eq!(m.px(Metrics::TAB_MIN_WIDTH), 120);
        assert_eq!(m.px(Metrics::TITLE_MIN_TAB_WIDTH), 150);
        assert_eq!(m.px(Metrics::NEW_TAB_SIZE), 40);
        assert_eq!(m.px(Metrics::CAPTION_BUTTON_WIDTH), 58, "57.5 rounds up");
        assert_eq!(m.px(Metrics::OMNIBOX_HEIGHT), 45);
        assert_eq!(m.px(Metrics::OUTER_MARGIN), 10);
        assert_eq!(m.px(Metrics::ROW_GAP), 10);
        assert_eq!(m.island_radius(), 13, "12.5 rounds up");
        assert_eq!(m.omnibox_radius(), 23, "22.5 rounds up");
    }

    #[test]
    fn metrics_scale_at_144_dpi() {
        let m = Metrics::new(144);

        assert_eq!(m.px(Metrics::STRIP_HEIGHT), 60);
        assert_eq!(m.px(Metrics::ISLAND_HEIGHT), 48);
        assert_eq!(m.px(Metrics::ISLAND_MARGIN), 6);
        assert_eq!(m.px(Metrics::ISLAND_GAP), 9);
        assert_eq!(m.px(Metrics::TAB_MAX_WIDTH), 330);
        assert_eq!(m.px(Metrics::TAB_MIN_WIDTH), 144);
        assert_eq!(m.px(Metrics::NEW_TAB_SIZE), 48);
        assert_eq!(m.px(Metrics::CAPTION_BUTTON_WIDTH), 69);
        assert_eq!(m.px(Metrics::OMNIBOX_HEIGHT), 54);
        assert_eq!(m.px(Metrics::OUTER_MARGIN), 12);
        assert_eq!(m.px(Metrics::ROW_GAP), 12);
        assert_eq!(m.island_radius(), 15);
        assert_eq!(m.omnibox_radius(), 27);
    }

    #[test]
    fn metrics_scale_at_192_dpi() {
        let m = Metrics::new(192);

        assert_eq!(m.px(Metrics::STRIP_HEIGHT), 80);
        assert_eq!(m.px(Metrics::ISLAND_HEIGHT), 64);
        assert_eq!(m.px(Metrics::ISLAND_MARGIN), 8);
        assert_eq!(m.px(Metrics::ISLAND_GAP), 12);
        assert_eq!(m.px(Metrics::TAB_MAX_WIDTH), 440);
        assert_eq!(m.px(Metrics::TAB_MIN_WIDTH), 192);
        assert_eq!(m.px(Metrics::NEW_TAB_SIZE), 64);
        assert_eq!(m.px(Metrics::CAPTION_BUTTON_WIDTH), 92);
        assert_eq!(m.px(Metrics::OMNIBOX_HEIGHT), 72);
        assert_eq!(m.px(Metrics::OUTER_MARGIN), 16);
        assert_eq!(m.px(Metrics::ROW_GAP), 16);
        assert_eq!(m.island_radius(), 20);
        assert_eq!(m.omnibox_radius(), 36);
    }

    #[test]
    fn zero_dpi_is_treated_as_96() {
        assert_eq!(Metrics::new(0), Metrics::new(96));
        assert_eq!(Metrics::new(0).px(Metrics::STRIP_HEIGHT), 40);
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
        let l = layout(1024, 96, 1);

        assert_eq!(l.strip, Rect::new(0, 0, 1024, 40));
        assert_eq!(l.caption_area, Rect::new(886, 0, 1024, 40));
        assert_eq!(l.tab_zone, Rect::new(8, 0, 840, 40));
        assert_eq!(l.tab(0), Some(Rect::new(8, 4, 228, 36)));
        assert_eq!(l.new_tab_button, Rect::new(234, 4, 266, 36));
        assert_eq!(l.omnibox, Rect::new(8, 48, 1016, 84));
        assert_eq!(l.viewport, Rect::new(4, 92, 1020, 696));
    }

    #[test]
    fn eight_tabs_at_1024_shrink_evenly_without_overflow() {
        let l = layout(1024, 96, 8);

        // (832 - 7 * 6) / 8 = 98
        assert_eq!(l.tab(0), Some(Rect::new(8, 4, 106, 36)));
        assert_eq!(l.tab(7), Some(Rect::new(736, 4, 834, 36)));
        assert_eq!(l.new_tab_button.left, 840, "+ follows the last tab");
        assert!(l.tabs().all(|tab| tab.intersect(l.tab_zone) == tab));
    }

    #[test]
    fn forty_tabs_overflow_at_min_width_and_pin_new_tab() {
        let l = layout(768, 96, 40);

        assert_eq!(l.tab(0).map(Rect::width), Some(96));
        assert_eq!(l.tab_zone, Rect::new(8, 0, 584, 40));
        assert_eq!(l.new_tab_button, Rect::new(590, 4, 622, 36));
        assert_eq!(l.caption_area.left, 630);

        assert_eq!(l.tab(6), None, "tabs after the visible window are hidden");
        assert_eq!(l.tab(39), None, "the default active tab is index zero");
    }

    #[test]
    fn active_tab_is_kept_visible_when_tabs_overflow() {
        let l = compute_with_active(768, HEIGHT, 96, 40, 39, false);

        assert_eq!(l.first_visible_tab(), 34);
        assert_eq!(l.tab(39), Some(Rect::new(518, 4, 614, 36)));
        assert_eq!(hit_test(&l, 550, 20), Hit::Tab(39));
        assert_eq!(l.tabs().count(), 6);
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
    fn new_tab_sits_after_tabs_or_at_zone_edge() {
        for dpi in DPIS {
            for width in WIDTHS {
                for tabs in [1, 8, 40] {
                    let l = layout(width, dpi, tabs);
                    let plus = l.new_tab_button;
                    assert!(
                        plus.right <= l.caption_area.left - l.metrics.px(Metrics::OUTER_MARGIN)
                    );
                    assert_eq!(plus.width(), l.metrics.px(Metrics::NEW_TAB_SIZE));
                    for (name, rect) in visible_controls(&l) {
                        if name.starts_with("tab ") {
                            assert!(rect.right <= plus.left, "{name} runs under +");
                        }
                    }
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
                let row_gap = m.px(Metrics::ROW_GAP);

                assert_eq!(l.strip.top, 0);
                assert_eq!(l.strip.bottom, l.omnibox.top - row_gap);
                assert_eq!(l.omnibox.bottom + margin, l.viewport.top);
                let rim = m.px(Metrics::RESIZE_RIM);
                assert_eq!(l.viewport.bottom, HEIGHT - rim);
                assert_eq!(l.omnibox.height(), m.px(Metrics::OMNIBOX_HEIGHT));
                assert_eq!(l.omnibox.left, margin);
                assert_eq!(l.omnibox.right, width - margin);
                assert_eq!(l.viewport.left, rim);
                assert_eq!(l.viewport.right, width - rim);

                let max = compute(width, HEIGHT, dpi, 3, true);
                assert_eq!(max.viewport, Rect::new(0, l.viewport.top, width, HEIGHT));
            }
        }
    }

    #[test]
    fn islands_are_centred_in_strip_with_margin() {
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            let l = layout(1024, dpi, 3);
            let tab = l.tab(0).expect("tab");

            assert_eq!(tab.top, m.px(Metrics::ISLAND_MARGIN));
            assert_eq!(tab.height(), m.px(Metrics::ISLAND_HEIGHT));
            assert_eq!(tab.bottom + m.px(Metrics::ISLAND_MARGIN), l.strip.bottom);
            assert_eq!(
                l.new_tab_button.top * 2 + l.new_tab_button.height(),
                l.strip.bottom
            );
        }
    }

    #[test]
    fn tabs_are_separated_by_scaled_gap() {
        let m = Metrics::new(144);
        let l = layout(1440, 144, 4);

        for index in 1..4 {
            let prev = l.tab(index - 1).expect("previous tab");
            let next = l.tab(index).expect("tab");
            assert_eq!(next.left - prev.right, m.px(Metrics::ISLAND_GAP));
        }
    }

    #[test]
    fn single_tab_is_capped_at_max_width() {
        let l = layout(1440, 96, 1);

        assert_eq!(l.tab(0).map(Rect::width), Some(220));
    }

    #[test]
    fn zero_tabs_leave_plus_at_left_edge() {
        let l = compute(1024, HEIGHT, 96, 0, false);

        assert_eq!(l.tab_count(), 0);
        assert_eq!(l.tab(0), None);
        assert_eq!(l.new_tab_button.left, 8);
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
            assert!((96..=220).contains(&width));
            previous = width;
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
        assert_disjoint(&tiny);
        assert_eq!(hit_test(&tiny, -1, 0), Hit::None);
    }

    #[test]
    fn hit_test_identifies_each_region_at_1024() {
        // Three tabs at 96 dpi: islands at 8, 234, 460 (width 220); "+" at 686..718.
        let l = layout(1024, 96, 3);

        assert_eq!(hit_test(&l, 20, 20), Hit::Tab(0));
        assert_eq!(hit_test(&l, 340, 20), Hit::Tab(1));
        assert_eq!(hit_test(&l, 500, 20), Hit::Tab(2));
        assert_eq!(
            hit_test(&l, 215, 20),
            Hit::TabClose(0),
            "close is at 202..222"
        );
        assert_eq!(
            hit_test(&l, 215, 2),
            Hit::CaptionArea,
            "above the island is drag area"
        );
        assert_eq!(
            hit_test(&l, 230, 20),
            Hit::CaptionArea,
            "gap between tabs is drag area"
        );
        assert_eq!(
            hit_test(&l, 3, 20),
            Hit::CaptionArea,
            "left margin is drag area"
        );
        assert_eq!(hit_test(&l, 700, 20), Hit::NewTab);
        assert_eq!(hit_test(&l, 750, 20), Hit::CaptionArea);
        assert_eq!(hit_test(&l, 900, 20), Hit::CaptionMinimize);
        assert_eq!(hit_test(&l, 950, 20), Hit::CaptionMaximize);
        assert_eq!(hit_test(&l, 1000, 20), Hit::CaptionClose);
        assert_eq!(hit_test(&l, 500, 60), Hit::Omnibox);
        assert_eq!(hit_test(&l, 500, 46), Hit::None, "row gap");
        assert_eq!(hit_test(&l, 500, 88), Hit::None, "margin above viewport");
        assert_eq!(hit_test(&l, 500, 200), Hit::Viewport);
        assert_eq!(hit_test(&l, 1024, 10), Hit::None, "right of client");
        assert_eq!(hit_test(&l, -1, 10), Hit::None, "left of client");
    }

    #[test]
    fn hit_test_respects_tab_zone_when_overflowing() {
        let l = compute_with_active(768, HEIGHT, 96, 40, 9, false);

        assert_eq!(hit_test(&l, 583, 20), Hit::Tab(9), "last visible column");
        assert_eq!(
            hit_test(&l, 586, 20),
            Hit::CaptionArea,
            "clipped tab area is not a tab"
        );
        assert_eq!(hit_test(&l, 600, 20), Hit::NewTab, "+ stays reachable");
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
    fn drag_regions_match_caption_area_hits_exactly() {
        for dpi in DPIS {
            for width in [768, 1024] {
                for tabs in [1, 8, 40] {
                    let l = layout(width, dpi, tabs);
                    let rects = l.drag_region_rects();
                    for y in l.strip.top..l.strip.bottom {
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
                for y in l.strip.top..l.strip.bottom {
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
                let tab = l.tab(39).expect("active tab is visible");
                let (x, y) = ((tab.left + tab.right) / 2, tab.top + 1);
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
    fn drag_region_rects_stay_inside_strip() {
        let l = layout(1024, 96, 5);

        for rect in l.drag_region_rects() {
            assert!(rect.intersect(l.strip) == rect, "{rect:?} leaves the strip");
            assert!(!rect.is_empty());
        }
    }

    #[test]
    fn tab_parts_fit_inside_island_without_overlap() {
        let l = layout(1440, 120, 3);
        let tab = l.tab(0).expect("tab");
        let parts = l.tab_parts(tab);

        assert!(parts.favicon.left >= tab.left);
        assert!(parts.close.right <= tab.right);
        assert!(parts.favicon.right <= parts.title.left);
        assert!(parts.title.right <= parts.close.left);
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
        assert_eq!(tab.width(), 96);
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
        // Centres: 118, 344, 570.

        assert_eq!(reorder_target(&l, 0, 118), 0);
        assert_eq!(reorder_target(&l, 0, 300), 0);
        assert_eq!(reorder_target(&l, 0, 350), 1);
        assert_eq!(reorder_target(&l, 0, 700), 2);
    }

    #[test]
    fn reorder_target_moves_left_past_neighbours() {
        let l = layout(1024, 96, 3);

        assert_eq!(reorder_target(&l, 2, 570), 2);
        assert_eq!(reorder_target(&l, 2, 340), 1);
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
}
