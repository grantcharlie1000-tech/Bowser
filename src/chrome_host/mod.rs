//! HTML chrome host: the WebView2 toolbar bridge, its wire protocol and embedded assets.
//! See docs/html-chrome-protocol.md.

pub mod assets;
pub mod host;
pub mod protocol;

use std::sync::OnceLock;

use windows::Win32::Foundation::RECT;

use crate::chrome::layout::Layout;

/// Whether the HTML toolbar is enabled. Read once from `BOWSER_UI`; unset means the GDI chrome.
pub fn html_ui_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| parse_ui_mode(std::env::var("BOWSER_UI").ok().as_deref()))
}

/// Only the value `html`, in any letter case, selects the HTML chrome.
pub fn parse_ui_mode(value: Option<&str>) -> bool {
    value.is_some_and(|value| value.eq_ignore_ascii_case("html"))
}

/// Client-relative physical pixels of the HTML toolbar: from the back button's left edge to the
/// main menu button's right edge, spanning the strip's height. `None` when the main menu button is
/// empty (the toolbar is too narrow for it) or the span has no width.
pub fn toolbar_bounds(layout: &Layout) -> Option<RECT> {
    if layout.main_menu_button.is_empty() {
        return None;
    }
    let left = layout.back_button.left;
    let right = layout.main_menu_button.right;
    if right - left <= 0 {
        return None;
    }
    Some(RECT {
        left,
        top: layout.strip.top,
        right,
        bottom: layout.strip.bottom,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::layout::{Rect, compute_in_frame};

    #[test]
    fn only_html_enables_the_html_chrome() {
        assert!(parse_ui_mode(Some("html")));
        assert!(parse_ui_mode(Some("HTML")));
        assert!(parse_ui_mode(Some("Html")));
        for value in [None, Some(""), Some("gdi"), Some("html "), Some(" html"), Some("htm"), Some("1")] {
            assert!(!parse_ui_mode(value), "{value:?}");
        }
    }

    #[test]
    fn toolbar_bounds_span_back_button_to_main_menu() {
        let layout = compute_in_frame(1200, 700, 96, 3, 0, false);
        let bounds = toolbar_bounds(&layout).expect("wide window has a toolbar");
        assert_eq!(bounds.left, layout.back_button.left);
        assert_eq!(bounds.right, layout.main_menu_button.right);
        assert_eq!(bounds.top, layout.strip.top);
        assert_eq!(bounds.bottom, layout.strip.bottom);
        assert!(bounds.right > bounds.left);
    }

    #[test]
    fn toolbar_bounds_is_none_without_a_main_menu_button() {
        let mut layout = compute_in_frame(1200, 700, 96, 3, 0, false);
        layout.main_menu_button = Rect::new(0, 0, 0, 0);
        assert!(toolbar_bounds(&layout).is_none());
    }

    #[test]
    fn toolbar_bounds_is_none_when_the_span_has_no_width() {
        let mut layout = compute_in_frame(1200, 700, 96, 3, 0, false);
        // Menu button ends left of the back button: the span is negative.
        layout.main_menu_button = Rect::new(0, 0, layout.back_button.left - 10, 10);
        assert!(toolbar_bounds(&layout).is_none());
    }
}
