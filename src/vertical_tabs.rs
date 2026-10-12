//! Pure vertical sidebar geometry, in physical pixels.

use crate::chrome::layout::{self, Metrics, Rect};

pub const MIN_WIDTH: i32 = 180;
pub const MAX_WIDTH: i32 = 360;
pub const DEFAULT_WIDTH: i32 = 260;
pub const RESIZE_HANDLE_WIDTH: i32 = 5;
pub const HEADER_HEIGHT: i32 = 46;
pub const TAB_ROW_HEIGHT: i32 = 36;
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
}

impl SidebarLayout {
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
        let content = layout::content_rect(width, height, dpi, maximised);
        let available_width = content.right.saturating_sub(1).max(0);
        let min_width = m.px(MIN_WIDTH).min(available_width);
        let max_width = m.px(MAX_WIDTH).min(available_width);
        let side_width = m.px(requested_width).clamp(min_width, max_width);
        let top_margin = m.px(Metrics::TOP_MARGIN).min(height);
        let sidebar = Rect::new(0, top_margin, side_width, height);
        let header_height = m.px(HEADER_HEIGHT).min(height);
        let header = Rect::new(0, top_margin, side_width, top_margin.saturating_add(header_height).min(height));
        let row_top = (content.top + header_height).min(height);
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
        let row_height = Metrics::new(dpi).px(TAB_ROW_HEIGHT).max(1);
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
            crate::chrome::layout::content_rect(900, 700, 96, false).top
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
}
