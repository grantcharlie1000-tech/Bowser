use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, RECT};

use super::glass::CaptionButtons;
use crate::chrome::layout::Metrics;

/// Logical width of the left, right, bottom and corner resize grips.
const RESIZE_GRIP: i32 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChromeHit {
    None,
    Interactive,
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

/// Height of the tab strip, which is also the caption button height.
pub fn caption_strip_height(dpi: u32) -> i32 {
    Metrics::new(dpi).px(Metrics::STRIP_HEIGHT)
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
    let m = Metrics::new(dpi);
    let (width, height) = (client_size.0.max(0), client_size.1.max(0));
    let (x, y) = (point.x, point.y);
    let edge = m.px(RESIZE_GRIP);
    let top_edge = m.px(Metrics::ISLAND_MARGIN);
    if x < -edge || y < -edge || x >= width.saturating_add(edge) || y >= height.saturating_add(edge)
    {
        return FrameHit::Client;
    }

    if !maximised {
        let left = x < edge;
        let right = x >= width.saturating_sub(edge);
        let top_corner = y < edge;
        let top = y < top_edge;
        let bottom = y >= height.saturating_sub(edge);
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
        return FrameHit::Client;
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
) -> RECT {
    if !maximised {
        return RECT {
            left: window_rect.left,
            top: window_rect.top,
            right: window_rect.right,
            bottom: window_rect.bottom,
        };
    }

    let (frame_x, frame_y) = (frame.0.max(0), frame.1.max(0));
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
        for y in 0..4 {
            assert_eq!(hit(780, y, ChromeHit::None), FrameHit::Top, "close y {y}");
            assert_eq!(hit(730, y, ChromeHit::None), FrameHit::Top, "max y {y}");
            assert_eq!(hit(680, y, ChromeHit::None), FrameHit::Top, "min y {y}");
        }
        assert_eq!(hit(780, 4, ChromeHit::None), FrameHit::Close);
        assert_eq!(hit(730, 5, ChromeHit::None), FrameHit::Maximize);
        assert_eq!(hit(680, 5, ChromeHit::None), FrameHit::Minimize);
    }

    #[test]
    fn top_grip_does_not_overlap_the_tab_islands() {
        // Islands start 4 px below the top, at x >= 8, so a click on a tab never starts a resize.
        assert_eq!(hit(100, 3, ChromeHit::Interactive), FrameHit::Top);
        for y in 4..36 {
            assert_eq!(
                hit(100, y, ChromeHit::Interactive),
                FrameHit::Client,
                "y {y}"
            );
        }
        let island_top = Metrics::new(120).px(Metrics::ISLAND_MARGIN);
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
        assert_eq!(
            hit(730, 36, ChromeHit::None),
            FrameHit::Maximize,
            "the caption button hit target covers the full 40 px strip"
        );
        assert_eq!(hit(730, 39, ChromeHit::None), FrameHit::Maximize);
        assert_eq!(hit(730, 40, ChromeHit::None), FrameHit::Client);
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
        assert_eq!(caption_strip_height(120), 50);
    }
}
