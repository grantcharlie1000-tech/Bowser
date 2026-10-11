//! Pure vertical sidebar geometry, in physical pixels.

use crate::chrome::layout::{self, Metrics, Rect};

pub const MIN_WIDTH: i32 = 180;
pub const MAX_WIDTH: i32 = 360;
pub const DEFAULT_WIDTH: i32 = 260;
pub const RESIZE_HANDLE_WIDTH: i32 = 5;
pub const HEADER_HEIGHT: i32 = 46;
pub const TAB_ROW_HEIGHT: i32 = 36;

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
        let sidebar = Rect::new(0, 0, side_width, height);
        let header_height = m.px(HEADER_HEIGHT).min(height);
        let header = Rect::new(0, 0, side_width, header_height);
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
