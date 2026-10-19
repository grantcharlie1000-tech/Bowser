use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, RECT};

use super::glass::CaptionButtons;
use crate::chrome::layout::Metrics;

/// Logical width of the left, right, bottom and corner resize grips.
const RESIZE_GRIP: i32 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChromeHit {
    None,
    Interactive,
    /// Empty chrome the layout calls `Hit::CaptionArea` (a drag band) wherever it
    /// is: below the caption strip too, as with a toolbar along the bottom edge.
    Drag,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameHit {
    Client,
    Caption,
    Minimize,
    Maximize,
    Close,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl FrameHit {
    pub fn lresult(self) -> LRESULT {
        use windows::Win32::UI::WindowsAndMessaging::*;
        LRESULT(match self {
            Self::Client => HTCLIENT as isize,
            Self::Caption => HTCAPTION as isize,
            Self::Minimize => HTMINBUTTON as isize,
            Self::Maximize => HTMAXBUTTON as isize,
            Self::Close => HTCLOSE as isize,
            Self::Left => HTLEFT as isize,
            Self::Right => HTRIGHT as isize,
            Self::Top => HTTOP as isize,
            Self::Bottom => HTBOTTOM as isize,
            Self::TopLeft => HTTOPLEFT as isize,
            Self::TopRight => HTTOPRIGHT as isize,
            Self::BottomLeft => HTBOTTOMLEFT as isize,
            Self::BottomRight => HTBOTTOMRIGHT as isize,
        })
    }
}

/// Height from the top edge through the caption hit strip.
pub fn caption_strip_height(dpi: u32) -> i32 {
    let m = Metrics::new(dpi);
    m.px(Metrics::TOP_MARGIN) + m.px(Metrics::STRIP_HEIGHT)
}

pub fn content_top_inset(dpi: u32) -> i32 {
    caption_strip_height(dpi)
}

/// Maps a client-relative point to a non-client hit code.
///
/// Resize edges come first, so the top corner and the top edge stay grabbable over the caption
/// buttons. The top edge is only as thick as the gap above the tab islands; a wider grip would
/// swallow the top of every tab.
pub fn frame_hit_test(
    point: POINT,
    client_size: (i32, i32),
    dpi: u32,
    maximised: bool,
    chrome_hit: ChromeHit,
) -> FrameHit {
    frame_hit_test_with(point, client_size, dpi, maximised, chrome_hit, false)
}

/// --- layout (thread T6) --- [`frame_hit_test`] with the bottom grip as thin as the top one when
/// `thin_bottom` is set: a tab row or the toolbar along the bottom edge would otherwise lose its
/// lower part to the resize edge, as the tabs at the top would.
pub fn frame_hit_test_with(
    point: POINT,
    client_size: (i32, i32),
    dpi: u32,
    maximised: bool,
    chrome_hit: ChromeHit,
    thin_bottom: bool,
) -> FrameHit {
    let m = Metrics::new(dpi);
    let (width, height) = (client_size.0.max(0), client_size.1.max(0));
    let (x, y) = (point.x, point.y);
    let edge = m.px(RESIZE_GRIP);
    let top_edge = m.px(Metrics::ISLAND_MARGIN);
    let bottom_edge = if thin_bottom { top_edge } else { edge };
    if x < -edge || y < -edge || x >= width.saturating_add(edge) || y >= height.saturating_add(edge)
    {
        return FrameHit::Client;
    }

    if !maximised {
        let left = x < edge;
        let right = x >= width.saturating_sub(edge);
        let top_corner = y < edge;
        let top = y < top_edge;
        let bottom = y >= height.saturating_sub(bottom_edge);
        match (top_corner, bottom, left, right) {
            (true, _, true, _) => return FrameHit::TopLeft,
            (true, _, _, true) => return FrameHit::TopRight,
            (_, true, true, _) => return FrameHit::BottomLeft,
            (_, true, _, true) => return FrameHit::BottomRight,
            _ if top => return FrameHit::Top,
            (_, true, _, _) => return FrameHit::Bottom,
            (_, _, true, _) => return FrameHit::Left,
            (_, _, _, true) => return FrameHit::Right,
            _ => {}
        }
    }

    let strip = caption_strip_height(dpi).min(height);
    if y >= strip {
        // A drag band lower down (the bottom toolbar's empty space) moves the
        // window like the one at the top; the caption buttons stay in the top strip.
        return if chrome_hit == ChromeHit::Drag { FrameHit::Caption } else { FrameHit::Client };
    }
    let buttons = CaptionButtons::for_client_width(width, dpi);
    let button_x = x.clamp(0, width.saturating_sub(1));
    let button_y = y.clamp(0, strip.saturating_sub(1));
    if buttons.close.contains(button_x, button_y) {
        return FrameHit::Close;
    }
    if buttons.max.contains(button_x, button_y) {
        return FrameHit::Maximize;
    }
    if buttons.min.contains(button_x, button_y) {
        return FrameHit::Minimize;
    }
    if y < m.px(Metrics::TOP_MARGIN) {
        return FrameHit::Caption;
    }
    if chrome_hit == ChromeHit::Interactive {
        return FrameHit::Client;
    }
    FrameHit::Caption
}

pub fn client_rect_for_nccalc(
    window_rect: RECT,
    maximised: bool,
    dpi: u32,
    frame: (i32, i32),
    work: RECT,
) -> RECT {
    let (frame_x, frame_y) = (frame.0.max(0), frame.1.max(0));
    if !maximised {
        // Require multiple work-area edges to align before treating a restored window as snapped.
        // This avoids trimming a floating window that happens to cross one edge.
        let near_left = (window_rect.left - work.left).abs() <= frame_x;
        let near_right = (window_rect.right - work.right).abs() <= frame_x;
        let near_top = (window_rect.top - work.top).abs() <= frame_y;
        let near_bottom = (window_rect.bottom - work.bottom).abs() <= frame_y;
        let snapped = [near_left, near_right, near_top, near_bottom]
            .into_iter()
            .filter(|aligned| *aligned)
            .count()
            >= 2;
        let left =
            if snapped && window_rect.left < work.left && work.left - window_rect.left <= frame_x {
                work.left
            } else {
                window_rect.left
            };
        let right = if snapped
            && window_rect.right > work.right
            && window_rect.right - work.right <= frame_x
        {
            work.right
        } else {
            window_rect.right
        };
        let bottom = if snapped
            && window_rect.bottom > work.bottom
            && window_rect.bottom - work.bottom <= frame_y
        {
            work.bottom
        } else {
            window_rect.bottom
        };
        return RECT {
            left,
            top: window_rect.top,
            right,
            bottom,
        };
    }

    let border = Metrics::new(dpi).px(1).max(1);
    RECT {
        left: window_rect.left + frame_x,
        top: window_rect.top + frame_y,
        right: window_rect.right - frame_x,
        bottom: window_rect.bottom - frame_y - border,
    }
}

pub fn point_from_lparam(lparam: LPARAM) -> POINT {
    POINT {
        x: (lparam.0 as u16 as i16) as i32,
        y: ((lparam.0 >> 16) as u16 as i16) as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (i32, i32) = (800, 600);

    fn p(x: i32, y: i32) -> POINT {
        POINT { x, y }
    }

    fn hit(x: i32, y: i32, chrome: ChromeHit) -> FrameHit {
        frame_hit_test(p(x, y), SIZE, 96, false, chrome)
    }

    #[test]
    fn resize_edges_and_corners_precede_caption() {
        assert_eq!(hit(1, 40, ChromeHit::None), FrameHit::Left);
        assert_eq!(hit(-7, 40, ChromeHit::None), FrameHit::Left);
        assert_eq!(hit(804, 40, ChromeHit::None), FrameHit::Right);
        assert_eq!(hit(400, 1, ChromeHit::None), FrameHit::Top);
        assert_eq!(hit(1, 1, ChromeHit::None), FrameHit::TopLeft);
        assert_eq!(hit(799, 599, ChromeHit::None), FrameHit::BottomRight);
    }

    #[test]
    fn top_right_corner_and_top_edge_resize_over_the_caption_buttons() {
        // Close button x 754..800, maximise 708..754, minimise 662..708.
        for y in 1..8 {
            assert_eq!(hit(799, y, ChromeHit::None), FrameHit::TopRight, "y {y}");
            assert_eq!(hit(795, y, ChromeHit::None), FrameHit::TopRight, "y {y}");
        }
        for y in 0..2 {
            assert_eq!(hit(780, y, ChromeHit::None), FrameHit::Top, "close y {y}");
            assert_eq!(hit(730, y, ChromeHit::None), FrameHit::Top, "max y {y}");
            assert_eq!(hit(680, y, ChromeHit::None), FrameHit::Top, "min y {y}");
        }
        assert_eq!(hit(780, 2, ChromeHit::None), FrameHit::Close);
        assert_eq!(hit(730, 5, ChromeHit::None), FrameHit::Maximize);
        assert_eq!(hit(680, 5, ChromeHit::None), FrameHit::Minimize);
    }

    #[test]
    fn top_grip_does_not_overlap_the_tab_islands() {
        // The top resize grip is the island margin; it sits above the toolbar and the tab islands.
        for y in 0..Metrics::ISLAND_MARGIN {
            assert_eq!(hit(100, y, ChromeHit::Interactive), FrameHit::Top, "grip at y {y}");
        }
        for y in Metrics::ISLAND_MARGIN..40 {
            assert_eq!(
                hit(100, y, ChromeHit::Interactive),
                FrameHit::Client,
                "y {y}"
            );
        }
        let m = Metrics::new(120);
        let island_top = m.px(Metrics::TOP_MARGIN)
            + m.px(Metrics::STRIP_HEIGHT)
            + m.px(Metrics::STRIP_GAP)
            + m.px(Metrics::TAB_ROW_HEIGHT - Metrics::ISLAND_HEIGHT) / 2;
        assert_eq!(
            frame_hit_test(p(100, island_top), SIZE, 120, false, ChromeHit::Interactive),
            FrameHit::Client,
            "the top grip scales with the island margin"
        );
    }

    #[test]
    fn caption_buttons_and_interactive_chrome_are_not_draggable() {
        assert_eq!(hit(400, 20, ChromeHit::None), FrameHit::Caption);
        assert_eq!(hit(400, 20, ChromeHit::Interactive), FrameHit::Client);
        assert_eq!(hit(790, 20, ChromeHit::None), FrameHit::Close);
        let strip = caption_strip_height(96);
        assert_eq!(
            strip,
            Metrics::TOP_MARGIN + Metrics::STRIP_HEIGHT,
            "the caption hit strip is the toolbar row; there is no headroom above it"
        );
        assert_eq!(
            hit(730, strip - 1, ChromeHit::None),
            FrameHit::Maximize,
            "the caption hit target covers the toolbar row"
        );
        assert_eq!(hit(730, strip, ChromeHit::None), FrameHit::Client);
    }

    #[test]
    fn caption_hit_targets_match_the_painted_layout_at_every_dpi() {
        for dpi in [96, 120, 144, 192] {
            let l = crate::chrome::layout::compute(1000, 700, dpi, 1, false);
            let size = (l.client.width(), l.client.height());
            let at = |x: i32, y: i32| frame_hit_test(p(x, y), size, dpi, false, ChromeHit::None);
            let y = l.strip.bottom - 1;
            assert_eq!(at(l.caption_area.left, y), FrameHit::Minimize, "dpi {dpi}");
            assert_eq!(
                at(l.caption_area.left - 1, y),
                FrameHit::Caption,
                "dpi {dpi}"
            );
            assert_eq!(
                at(l.caption_area.right - 30, y),
                FrameHit::Close,
                "dpi {dpi}"
            );
            assert_eq!(
                at(l.caption_area.right - 30, y + 1),
                FrameHit::Client,
                "dpi {dpi}"
            );
        }
    }

    #[test]
    fn maximised_windows_have_no_resize_grips_and_scaled_strip() {
        assert_eq!(
            frame_hit_test(p(0, 0), (1600, 900), 192, true, ChromeHit::None),
            FrameHit::Caption
        );
        assert_eq!(
            frame_hit_test(p(200, 80), (1600, 900), 192, true, ChromeHit::None),
            FrameHit::Client
        );
        assert_eq!(
            caption_strip_height(120),
            Metrics::new(120).px(Metrics::TOP_MARGIN) + Metrics::new(120).px(Metrics::STRIP_HEIGHT),
            "headroom plus strip scale at 125%"
        );
        assert_eq!(caption_strip_height(120), 38, "37.5 rounds up");
    }

    // --- layout (thread T6) ---
    #[test]
    fn a_row_along_the_bottom_edge_keeps_the_bottom_grip_as_thin_as_the_top_one() {
        let size = (1000, 700);
        let low = POINT { x: 500, y: 694 };
        assert_eq!(frame_hit_test(low, size, 96, false, ChromeHit::Interactive), FrameHit::Bottom, "8px grip by default");
        assert_eq!(frame_hit_test_with(low, size, 96, false, ChromeHit::Interactive, true), FrameHit::Client, "a tab there is a tab");
        let edge = POINT { x: 500, y: 699 };
        assert_eq!(frame_hit_test_with(edge, size, 96, false, ChromeHit::Interactive, true), FrameHit::Bottom, "the last 2px still resize");
        let corner = POINT { x: 996, y: 694 };
        assert_eq!(frame_hit_test_with(corner, size, 96, false, ChromeHit::None, true), FrameHit::Right, "the side grips keep their width");
        assert_eq!(frame_hit_test_with(low, size, 96, true, ChromeHit::Interactive, true), FrameHit::Client, "maximised: no grips at all");
        assert_eq!(frame_hit_test_with(POINT { x: 500, y: 10 }, size, 96, false, ChromeHit::None, true), FrameHit::Caption, "the top is unchanged");
    }
    // --- end layout ---

    #[test]
    fn a_drag_band_below_the_caption_strip_moves_the_window() {
        let size = (1000, 700);
        let bottom_toolbar = POINT { x: 500, y: 680 };
        for thin in [false, true] {
            assert_eq!(frame_hit_test_with(bottom_toolbar, size, 96, false, ChromeHit::Drag, thin), FrameHit::Caption, "thin {thin}");
            assert_eq!(frame_hit_test_with(bottom_toolbar, size, 96, false, ChromeHit::None, thin), FrameHit::Client, "thin {thin}");
            assert_eq!(frame_hit_test_with(bottom_toolbar, size, 96, false, ChromeHit::Interactive, thin), FrameHit::Client, "thin {thin}");
        }
        assert_eq!(frame_hit_test_with(POINT { x: 500, y: 699 }, size, 96, false, ChromeHit::Drag, true), FrameHit::Bottom, "the grip still comes first");
        assert_eq!(frame_hit_test_with(POINT { x: 980, y: 10 }, size, 96, false, ChromeHit::Drag, true), FrameHit::Close, "the caption buttons stay at the top");
    }
}
