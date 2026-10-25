//! Pure vertical sidebar geometry, in physical pixels.

use crate::chrome::layout::{self, Metrics, Rect};
use crate::settings::{SidePanelPosition, TabLayout};

pub const MIN_WIDTH: i32 = 180;
/// Width of the collapsed, icons-only sidebar strip.
pub const COLLAPSED_WIDTH: i32 = 48;
/// How long an unpinned rail takes to open or close over the page, in milliseconds.
pub const FLYOUT_MS: u32 = 180;
/// Logical pixels of the page that stay visible beside an open flyout.
pub const FLYOUT_KEEP: i32 = 64;
pub const MAX_WIDTH: i32 = 360;
pub const DEFAULT_WIDTH: i32 = 260;
pub const RESIZE_HANDLE_WIDTH: i32 = 5;
/// Logical pixels of window (and page, on a flyout) kept clear around the sidebar so it reads as a
/// floating panel. The HTML host is this island; the gap is not part of the webview.
pub const ISLAND_INSET: i32 = 8;
pub const HEADER_HEIGHT: i32 = 46;
pub const TAB_ROW_HEIGHT: i32 = 36;
/// --- layout (thread T6) --- the row height at the compact UI density.
pub const COMPACT_TAB_ROW_HEIGHT: i32 = 30;
pub const NEW_TAB_BUTTON_SIZE: i32 = 28;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SidebarHit {
    Workspace,
    NewTab,
    Resize,
    Group(usize),
    Tab(usize),
    TabClose(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SidebarLayout {
    pub sidebar: Rect,
    pub header: Rect,
    pub rows: Rect,
    pub handle: Rect,
    pub viewport: Rect,
    pub max_scroll: i32,
    row_height: i32,
    /// Physical pixels the floating island is inset from [`Self::sidebar`] on every side.
    inset: i32,
    /// --- layout (thread T6) --- which edge the sidebar is docked to. The resize handle is on
    /// its inner edge: the right edge of a left sidebar, the left edge of a right one.
    pub side: SidePanelPosition,
}

/// Where a click in the sidebar slot but outside the floating island should go.
/// The island itself and the resize handle are `None` (the HTML host, or the native handle).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GapTarget {
    /// The point is also inside the page viewport, so the page window owns it.
    Page,
    /// Padding around the island. The page does not cover the inner gutter, so this drags the window.
    Drag,
}

/// Inset that keeps a slot of `slot_w` by `slot_h` physical pixels from inverting.
pub fn island_inset(slot_w: i32, slot_h: i32, dpi: u32) -> i32 {
    let want = Metrics::new(dpi).px(ISLAND_INSET);
    let room = slot_w.max(0).min(slot_h.max(0)) / 4;
    want.min(room).max(0)
}

fn inset_rect(rect: Rect, inset: i32) -> Rect {
    if inset <= 0 || rect.is_empty() {
        return rect;
    }
    let inset = inset.min(rect.width() / 4).min(rect.height() / 4);
    Rect::new(
        rect.left.saturating_add(inset),
        rect.top.saturating_add(inset),
        rect.right.saturating_sub(inset),
        rect.bottom.saturating_sub(inset),
    )
}

impl SidebarLayout {
    // --- layout (thread T6) ---
    /// The sidebar docked to `side`, spanning `top` to `bottom` (client y), with rows `row_height`
    /// logical pixels tall. A left sidebar sits right of the rail; a right one ends at the window's
    /// right edge (inside the resize rim). `viewport` is the page area beside it, which
    /// `layout::compute_arranged_in_frame` settles against the rows at the top and bottom.
    #[allow(clippy::too_many_arguments)]
    pub fn compute_in_frame_at(
        client_w: i32,
        client_h: i32,
        dpi: u32,
        requested_width: i32,
        maximised: bool,
        side: SidePanelPosition,
        top: i32,
        bottom: i32,
        row_height: i32,
    ) -> Self {
        let m = Metrics::new(dpi);
        let client_w = client_w.max(0);
        let client_h = client_h.max(0);
        let rail = layout::left_rail_inset(dpi);
        let top = top.clamp(0, client_h);
        let bottom = bottom.clamp(top, client_h);
        let rim = if maximised { 0 } else { m.px(Metrics::RESIZE_RIM).min(client_w / 2) };
        let available_width = (client_w - rail - rim).saturating_sub(1).max(0);
        let floor = if requested_width <= COLLAPSED_WIDTH { COLLAPSED_WIDTH } else { MIN_WIDTH };
        let min_width = m.px(floor).min(available_width);
        let max_width = m.px(MAX_WIDTH).min(available_width);
        let side_width = m.px(requested_width).clamp(min_width, max_width);
        let (left, right) = match side {
            SidePanelPosition::Left => (rail, rail + side_width),
            SidePanelPosition::Right => ((client_w - rim - side_width).max(rail), (client_w - rim).max(rail)),
        };
        let sidebar = Rect::new(left, top, right, bottom);
        let inset = island_inset(sidebar.width(), sidebar.height(), dpi);
        let header_height = m.px(HEADER_HEIGHT).min(bottom - top);
        let header = Rect::new(left, top, right, top + header_height);
        let rows = Rect::new(left, header.bottom, right, bottom);
        let handle_width = m.px(RESIZE_HANDLE_WIDTH).max(1);
        let handle = match side {
            SidePanelPosition::Left => Rect::new(right - handle_width, top, right, bottom),
            SidePanelPosition::Right => Rect::new(left, top, left + handle_width, bottom),
        };
        let viewport = match side {
            SidePanelPosition::Left => Rect::new(right, top, (client_w - rim).max(right), bottom),
            SidePanelPosition::Right => Rect::new(rail, top, left.max(rail), bottom),
        };
        Self {
            sidebar,
            header,
            rows,
            handle,
            viewport,
            max_scroll: 0,
            row_height: m.px(row_height).max(1),
            inset,
            side,
        }
    }

    /// Physical height of one tab row.
    pub fn row_height(&self) -> i32 {
        self.row_height
    }

    /// The row area's inner edges, clear of the resize handle whichever side it is on.
    pub fn content_left(&self) -> i32 {
        match self.side {
            SidePanelPosition::Left => self.rows.left,
            SidePanelPosition::Right => self.handle.right,
        }
    }

    pub fn content_right(&self) -> i32 {
        match self.side {
            SidePanelPosition::Left => self.handle.left,
            SidePanelPosition::Right => self.rows.right,
        }
    }

    /// The width (physical pixels) a drag of the resize handle to client `x` asks for.
    pub fn drag_width_px(&self, x: i32) -> i32 {
        match self.side {
            SidePanelPosition::Left => x,
            SidePanelPosition::Right => self.sidebar.right - x,
        }
    }
    // --- end layout ---

    pub fn compute(
        width: i32,
        height: i32,
        dpi: u32,
        requested_width: i32,
        maximised: bool,
    ) -> Self {
        let width = width.max(0);
        let height = height.max(0);
        let m = Metrics::new(dpi);
        // The sidebar replaces the tab row, so the page starts right under the toolbar.
        let content = layout::content_rect(TabLayout::Vertical, width, height, dpi, maximised);
        let available_width = content.right.saturating_sub(1).max(0);
        // A request at or below the collapsed width is honoured; anything else keeps MIN_WIDTH.
        let floor = if requested_width <= COLLAPSED_WIDTH { COLLAPSED_WIDTH } else { MIN_WIDTH };
        let min_width = m.px(floor).min(available_width);
        let max_width = m.px(MAX_WIDTH).min(available_width);
        let side_width = m.px(requested_width).clamp(min_width, max_width);
        let top_margin = m.px(Metrics::TOP_MARGIN).min(height);
        let sidebar = Rect::new(0, top_margin, side_width, height);
        let inset = island_inset(sidebar.width(), sidebar.height(), dpi);
        let header_height = m.px(HEADER_HEIGHT).min(height);
        let header = Rect::new(0, top_margin, side_width, top_margin.saturating_add(header_height).min(height));
        let row_top = header.bottom;
        let rows = Rect::new(0, row_top, side_width, content.bottom.max(row_top));
        let row_height = m.px(TAB_ROW_HEIGHT).max(1);
        let max_scroll = 0;
        let handle_width = m.px(RESIZE_HANDLE_WIDTH).max(1);
        let handle = Rect::new(side_width - handle_width, 0, side_width, height);
        let viewport = Rect::new(
            side_width,
            content.top,
            content.right.max(side_width),
            content.bottom,
        );
        Self {
            sidebar,
            header,
            rows,
            handle,
            viewport,
            max_scroll,
            row_height,
            inset,
            side: SidePanelPosition::Left,
        }
    }

    /// The sidebar laid out to the right of the left rail, in the same window as `compute`.
    pub fn compute_in_frame(
        width: i32,
        height: i32,
        dpi: u32,
        requested_width: i32,
        maximised: bool,
    ) -> Self {
        let inset = layout::left_rail_inset(dpi);
        let mut sidebar = Self::compute(
            width.max(0) - inset,
            height,
            dpi,
            requested_width,
            maximised,
        );
        sidebar.sidebar = sidebar.sidebar.shifted_right(inset);
        sidebar.header = sidebar.header.shifted_right(inset);
        sidebar.rows = sidebar.rows.shifted_right(inset);
        sidebar.handle = sidebar.handle.shifted_right(inset);
        sidebar.viewport = sidebar.viewport.shifted_right(inset);
        sidebar
    }

    pub fn with_tab_count(mut self, count: usize, dpi: u32) -> Self {
        // The row height set by the constructor (the density's); `compute` and `compute_in_frame`
        // use the regular one, which is what this read before.
        let row_height = if self.row_height > 0 { self.row_height } else { Metrics::new(dpi).px(TAB_ROW_HEIGHT).max(1) };
        let content_height = self.rows.height();
        self.row_height = row_height;
        self.max_scroll = (count.min(i32::MAX as usize) as i32)
            .saturating_mul(row_height)
            .saturating_sub(content_height)
            .max(0);
        self
    }

    pub fn scroll_for_row(&self, index: usize) -> i32 {
        (index.min(i32::MAX as usize) as i32)
            .saturating_mul(self.row_height)
            .clamp(0, self.max_scroll)
    }

    /// The floating panel inside the slot. The HTML tabs host uses this, not the full slot.
    pub fn island(&self) -> Rect {
        inset_rect(self.sidebar, self.inset)
    }

    pub fn new_tab_button(&self) -> Rect {
        let size = NEW_TAB_BUTTON_SIZE
            .min(self.header.height())
            .min(self.header.width());
        Rect::new(
            self.header.right - size - 8,
            self.header.top + (self.header.height() - size) / 2,
            self.header.right - 8,
            self.header.top + (self.header.height() + size) / 2,
        )
    }
}

/// Width of an overlay at progress `t` in `0.0..=1.0`, ease-out cubic.
/// `t <= 0` is `from`. Non-finite or `t >= 1` is `to`.
pub fn flyout_width_at(from: i32, to: i32, t: f32) -> i32 {
    if !t.is_finite() || t >= 1.0 {
        return to;
    }
    if t <= 0.0 {
        return from;
    }
    let eased = 1.0 - (1.0 - t).powi(3);
    from.saturating_add(((to - from) as f32 * eased).round() as i32)
}

/// Un-inset overlay while an unpinned rail is open. It grows from the sidebar's outer edge over
/// the page and does not move `viewport`. `width_px` and `keep_px` are physical pixels. The result
/// is never narrower than the laid-out sidebar. Animation compares this width with the slot.
pub fn flyout_span(layout: &SidebarLayout, width_px: i32, keep_px: i32) -> Rect {
    let rail = layout.sidebar.width().max(1);
    let width = width_px.max(rail);
    let keep = keep_px.max(0);
    match layout.side {
        SidePanelPosition::Left => {
            let max_right = (layout.viewport.right - keep).max(layout.sidebar.left.saturating_add(rail));
            let right = layout.sidebar.left.saturating_add(width).min(max_right);
            Rect::new(layout.sidebar.left, layout.sidebar.top, right, layout.sidebar.bottom)
        }
        SidePanelPosition::Right => {
            let min_left = (layout.viewport.left + keep).min(layout.sidebar.right.saturating_sub(rail));
            let left = layout.sidebar.right.saturating_sub(width).max(min_left);
            Rect::new(left, layout.sidebar.top, layout.sidebar.right, layout.sidebar.bottom)
        }
    }
}

/// Tabs-host rectangle while an unpinned rail is open: [`flyout_span`] inset to the floating island.
pub fn flyout_rect(layout: &SidebarLayout, width_px: i32, keep_px: i32) -> Rect {
    inset_rect(flyout_span(layout, width_px, keep_px), layout.inset)
}

/// A point in the sidebar slot that is neither the floating island nor the resize handle.
/// `None` when the HTML host or the native handle should keep the click.
pub fn gap_target(layout: &SidebarLayout, x: i32, y: i32) -> Option<GapTarget> {
    if !layout.sidebar.contains(x, y) || layout.island().contains(x, y) || layout.handle.contains(x, y) {
        return None;
    }
    if layout.viewport.contains(x, y) {
        Some(GapTarget::Page)
    } else {
        Some(GapTarget::Drag)
    }
}

/// Classifies a point in the sidebar after applying the current scroll offset.
pub fn hit_test(
    layout: &SidebarLayout,
    x: i32,
    y: i32,
    scroll: i32,
    tab_count: usize,
) -> Option<SidebarHit> {
    if layout.handle.contains(x, y) {
        return Some(SidebarHit::Resize);
    }
    if layout.new_tab_button().contains(x, y) {
        return Some(SidebarHit::NewTab);
    }
    if layout.header.contains(x, y) {
        return Some(SidebarHit::Workspace);
    }
    if !layout.rows.contains(x, y) {
        return None;
    }
    let row = (y - layout.rows.top + scroll.max(0)) / layout.row_height;
    if row < 0 || row as usize >= tab_count {
        return None;
    }
    let close_width = layout.row_height.min(layout.sidebar.width() / 3);
    if x >= layout.sidebar.right - close_width {
        Some(SidebarHit::TabClose(row as usize))
    } else {
        Some(SidebarHit::Tab(row as usize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sidebar_in_frame_sits_right_of_the_left_rail() {
        for dpi in [96, 144] {
            let sidebar = SidebarLayout::compute_in_frame(900, 700, dpi, 260, false);
            let rail = layout::left_rail(700, dpi);
            assert_eq!(sidebar.sidebar.left, layout::left_rail_inset(dpi));
            assert!(sidebar.sidebar.left >= rail.right, "dpi={dpi}");
            assert_eq!(sidebar.handle.right, sidebar.sidebar.right, "dpi={dpi}");
            assert_eq!(sidebar.viewport.left, sidebar.sidebar.right, "dpi={dpi}");
        }
    }

    #[test]
    fn sidebar_width_is_clamped_and_viewport_keeps_a_positive_width() {
        let layout = SidebarLayout::compute(900, 700, 96, 1200, false);
        assert_eq!(layout.sidebar.width(), MAX_WIDTH);
        assert_eq!(layout.viewport.left, layout.sidebar.right);
        assert!(layout.viewport.width() > 0);

        let narrow = SidebarLayout::compute(180, 500, 96, MIN_WIDTH, false);
        assert!(narrow.sidebar.width() < MIN_WIDTH);
        assert!(narrow.viewport.width() >= 1);
    }

    #[test]
    fn collapsed_width_is_honoured_and_larger_requests_keep_the_old_minimum() {
        for dpi in [96, 144] {
            let m = Metrics::new(dpi);
            let collapsed = SidebarLayout::compute(900, 700, dpi, COLLAPSED_WIDTH, false);
            assert_eq!(collapsed.sidebar.width(), m.px(COLLAPSED_WIDTH), "dpi={dpi}");
            assert_eq!(collapsed.viewport.left, collapsed.sidebar.right, "dpi={dpi}");
            let small = SidebarLayout::compute(900, 700, dpi, 100, false);
            assert_eq!(small.sidebar.width(), m.px(MIN_WIDTH), "dpi={dpi}");
        }
    }

    #[test]
    fn row_scroll_geometry_accounts_for_every_tab() {
        let layout = SidebarLayout::compute(900, 300, 144, 260, false).with_tab_count(260, 144);
        assert!(layout.max_scroll > 0);
        assert_eq!(layout.scroll_for_row(0), 0);
        assert_eq!(layout.scroll_for_row(1000), layout.max_scroll);
        assert_eq!(layout.handle.width(), 8);
    }

    #[test]
    fn viewport_preserves_the_existing_top_and_resize_rim() {
        let layout = SidebarLayout::compute(900, 700, 96, 260, false);
        assert_eq!(
            layout.viewport.top,
            crate::chrome::layout::content_rect(TabLayout::Vertical, 900, 700, 96, false).top
        );
        assert_eq!(layout.viewport.right, 896);
    }

    #[test]
    fn maximized_viewport_uses_full_right_edge_after_sidebar() {
        let layout = SidebarLayout::compute(900, 700, 96, 260, true);
        assert_eq!(layout.viewport.right, 900);
    }

    #[test]
    fn hit_test_tracks_tab_rows_close_targets_and_scroll_at_supported_dpis() {
        for dpi in [96, 120, 144, 192] {
            let layout = SidebarLayout::compute(1600, 1000, dpi, 260, true).with_tab_count(80, dpi);
            let row_height = Metrics::new(dpi).px(TAB_ROW_HEIGHT).max(1);
            let y = layout.rows.top + row_height / 2;
            assert_eq!(
                hit_test(&layout, 24, y, 0, 80),
                Some(SidebarHit::Tab(0)),
                "dpi={dpi}"
            );
            assert_eq!(
                hit_test(&layout, layout.sidebar.right - 20, y, 0, 80),
                Some(SidebarHit::TabClose(0)),
                "dpi={dpi}"
            );
            assert_eq!(
                hit_test(&layout, 24, y, row_height * 2, 80),
                Some(SidebarHit::Tab(2)),
                "dpi={dpi}"
            );
            assert_eq!(
                hit_test(
                    &layout,
                    layout.new_tab_button().left + 1,
                    layout.new_tab_button().top + 1,
                    0,
                    80
                ),
                Some(SidebarHit::NewTab),
                "dpi={dpi}"
            );
        }
    }

    #[test]
    fn flyout_grows_over_the_page_without_moving_the_viewport() {
        let layout = SidebarLayout::compute(900, 700, 96, COLLAPSED_WIDTH, false);
        let page = layout.viewport;
        let open = flyout_rect(&layout, 260, FLYOUT_KEEP);
        assert_eq!(layout.viewport, page, "the flyout must not push the page");
        assert_eq!(open.left, layout.sidebar.left + layout.inset);
        assert_eq!(open.top, layout.sidebar.top + layout.inset);
        assert_eq!(open.bottom, layout.sidebar.bottom - layout.inset);
        assert_eq!(open.right, layout.sidebar.left + 260 - layout.inset);
        assert!(open.width() > layout.island().width());
        assert!(open.right <= page.right - FLYOUT_KEEP);
        let span = flyout_span(&layout, 260, FLYOUT_KEEP);
        assert_eq!(span.right, layout.sidebar.left + 260, "animation still measures the un-inset span");

        let tight = SidebarLayout::compute(200, 400, 96, COLLAPSED_WIDTH, false);
        let clamped = flyout_rect(&tight, MAX_WIDTH, FLYOUT_KEEP);
        assert!(clamped.width() >= tight.island().width());
        assert!(clamped.right <= tight.viewport.right - FLYOUT_KEEP || clamped.width() == tight.island().width());
    }

    #[test]
    fn flyout_on_the_right_grows_toward_the_page() {
        let layout = SidebarLayout::compute_in_frame_at(
            900,
            700,
            96,
            COLLAPSED_WIDTH,
            false,
            SidePanelPosition::Right,
            8,
            700,
            TAB_ROW_HEIGHT,
        );
        let open = flyout_rect(&layout, 260, FLYOUT_KEEP);
        assert_eq!(open.right, layout.sidebar.right - layout.inset);
        assert_eq!(open.top, layout.sidebar.top + layout.inset);
        assert!(open.left < layout.sidebar.left);
        assert!(open.left >= layout.viewport.left + FLYOUT_KEEP || open.width() == layout.sidebar.width());
        assert_eq!(layout.viewport.right, layout.sidebar.left);
    }

    #[test]
    fn flyout_width_eases_out_and_settles_on_the_endpoints() {
        assert_eq!(flyout_width_at(48, 260, 0.0), 48);
        assert_eq!(flyout_width_at(48, 260, 1.0), 260);
        assert_eq!(flyout_width_at(48, 260, f32::NAN), 260);
        assert_eq!(flyout_width_at(260, 48, -1.0), 260);
        // Ease-out cubic at t=0.5 is 1 - 0.5^3 = 0.875.
        assert_eq!(flyout_width_at(0, 100, 0.5), 88);
        let early = flyout_width_at(48, 260, 0.2);
        assert!(early > 48 && early < 260);
    }

    #[test]
    fn the_sidebar_host_is_an_island_inset_from_the_slot() {
        for dpi in [96, 120, 144, 192] {
            let m = Metrics::new(dpi);
            let layout = SidebarLayout::compute(900, 700, dpi, 260, false);
            let want = m.px(ISLAND_INSET);
            assert_eq!(layout.inset, want, "dpi={dpi}");
            let island = layout.island();
            assert_eq!(island.left, layout.sidebar.left + want, "dpi={dpi}");
            assert_eq!(island.top, layout.sidebar.top + want, "dpi={dpi}");
            assert_eq!(island.right, layout.sidebar.right - want, "dpi={dpi}");
            assert_eq!(island.bottom, layout.sidebar.bottom - want, "dpi={dpi}");
            assert!(island.right <= layout.handle.left, "the island stays clear of the resize handle, dpi={dpi}");

            let collapsed = SidebarLayout::compute(900, 700, dpi, COLLAPSED_WIDTH, false);
            assert_eq!(collapsed.island().width(), m.px(COLLAPSED_WIDTH) - 2 * collapsed.inset, "dpi={dpi}");
            assert!(collapsed.island().width() > 0, "dpi={dpi}");
        }
        let tiny = SidebarLayout::compute(40, 80, 96, COLLAPSED_WIDTH, true);
        assert!(tiny.inset <= tiny.sidebar.width() / 4);
        assert!(tiny.island().width() > 0 && tiny.island().height() > 0);
    }

    #[test]
    fn gap_around_the_island_drags_and_the_handle_stays_a_resize() {
        let layout = SidebarLayout::compute(900, 700, 96, 260, false);
        let island = layout.island();
        assert_eq!(gap_target(&layout, island.left + 4, island.top + 8), None, "the island is the host");
        assert_eq!(gap_target(&layout, layout.handle.left + 1, island.top + 8), None, "the handle is a native resize");
        assert_eq!(gap_target(&layout, island.right + 1, island.top + 20), Some(GapTarget::Drag), "the inner gutter is not covered by the page");
        assert_eq!(gap_target(&layout, island.left + 6, layout.sidebar.top + 1), Some(GapTarget::Drag), "the outer padding drags the window");
        assert_eq!(gap_target(&layout, layout.viewport.left + 4, layout.viewport.top + 4), None);
        assert_eq!(gap_target(&layout, -4, island.top + 4), None);
    }
}
