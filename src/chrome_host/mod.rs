//! HTML chrome host: the WebView2 toolbar bridge, its wire protocol and embedded assets.
//! See docs/html-chrome-protocol.md.

pub mod assets;
pub mod host;
pub mod protocol;

use std::sync::OnceLock;

use windows::Win32::Foundation::RECT;

use crate::chrome::layout::Layout;
use crate::site_security::SiteSecurity;
use crate::theme::{Palette, Rgba};
use protocol::Security;

/// Wire value for a site classification. The mapping is part of the bridge contract.
pub fn security_of(site: SiteSecurity) -> Security {
    match site {
        SiteSecurity::Secure => Security::Secure,
        SiteSecurity::NotSecure => Security::NotSecure,
        SiteSecurity::CertificateProblem => Security::CertProblem,
        SiteSecurity::Internal => Security::Internal,
        SiteSecurity::Unknown => Security::Unknown,
        SiteSecurity::NoSite => Security::None,
    }
}

/// CSS colour for a palette entry: `#rrggbb` when opaque, otherwise `rgba(r,g,b,a)` with alpha as
/// a fraction. Every output passes `protocol::is_safe_css_colour`.
pub fn css_colour(colour: Rgba) -> String {
    if colour.a == 0xFF {
        format!("#{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b)
    } else {
        format!(
            "rgba({},{},{},{:.3})",
            colour.r,
            colour.g,
            colour.b,
            f32::from(colour.a) / 255.0
        )
    }
}

/// The toolbar's theme tokens, mapped from the native palette (see docs/html-chrome-protocol.md).
pub fn theme_tokens(palette: &Palette) -> Vec<(&'static str, String)> {
    vec![
        ("bg", css_colour(palette.window)),
        ("fill", css_colour(palette.omnibox_fill)),
        ("border", css_colour(palette.omnibox_border)),
        ("focusBorder", css_colour(palette.omnibox_focus_border)),
        ("text", css_colour(palette.omnibox_text)),
        ("placeholder", css_colour(palette.omnibox_placeholder)),
        ("completion", css_colour(palette.omnibox_completion)),
        ("selection", css_colour(palette.omnibox_selection)),
        ("hover", css_colour(palette.tab_hover)),
        ("progress", css_colour(palette.progress_fill)),
    ]
}

/// Records `json` as the last state sent. Returns true only when it differs from the last one, so
/// an unchanged state is never posted again.
pub fn state_changed(last: &mut Option<String>, json: &str) -> bool {
    if last.as_deref() == Some(json) {
        return false;
    }
    *last = Some(json.to_string());
    true
}

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
    fn css_colour_renders_opaque_and_translucent_forms() {
        assert_eq!(css_colour(Rgba::hex(0x1A2B3C, 0xFF)), "#1a2b3c");
        assert_eq!(css_colour(Rgba { r: 0, g: 0, b: 0, a: 0 }), "rgba(0,0,0,0.000)");
        assert_eq!(css_colour(Rgba { r: 255, g: 255, b: 255, a: 128 }), "rgba(255,255,255,0.502)");
        for colour in [
            Rgba::hex(0x000000, 0xFF),
            Rgba { r: 9, g: 8, b: 7, a: 1 },
            Rgba { r: 255, g: 0, b: 128, a: 254 },
        ] {
            assert!(protocol::is_safe_css_colour(&css_colour(colour)), "{colour:?}");
        }
    }

    #[test]
    fn theme_tokens_cover_every_token_with_safe_colours() {
        for (appearance, solid) in [
            (crate::theme::Appearance::Light, false),
            (crate::theme::Appearance::Dark, false),
            (crate::theme::Appearance::Light, true),
            (crate::theme::Appearance::Dark, true),
        ] {
            let tokens = theme_tokens(crate::theme::palette(appearance, solid));
            let keys: Vec<&str> = tokens.iter().map(|(key, _)| *key).collect();
            assert_eq!(keys, protocol::THEME_TOKENS, "{appearance} solid={solid}");
            for (key, value) in &tokens {
                assert!(protocol::is_safe_css_colour(value), "{key}={value}");
            }
        }
    }

    #[test]
    fn site_security_maps_to_the_wire_values() {
        let cases = [
            (SiteSecurity::Secure, "secure"),
            (SiteSecurity::NotSecure, "notsecure"),
            (SiteSecurity::CertificateProblem, "certproblem"),
            (SiteSecurity::Internal, "internal"),
            (SiteSecurity::Unknown, "unknown"),
            (SiteSecurity::NoSite, "none"),
        ];
        for (site, wire) in cases {
            assert_eq!(security_of(site).as_str(), wire, "{site:?}");
        }
    }

    #[test]
    fn state_diff_skips_unchanged_json() {
        let mut last = None;
        assert!(state_changed(&mut last, "{\"a\":1}"));
        assert!(!state_changed(&mut last, "{\"a\":1}"));
        assert!(state_changed(&mut last, "{\"a\":2}"));
        assert!(state_changed(&mut last, "{\"a\":1}"));
        assert_eq!(last.as_deref(), Some("{\"a\":1}"));
    }

    #[test]
    fn toolbar_bounds_is_none_when_the_span_has_no_width() {
        let mut layout = compute_in_frame(1200, 700, 96, 3, 0, false);
        // Menu button ends left of the back button: the span is negative.
        layout.main_menu_button = Rect::new(0, 0, layout.back_button.left - 10, 10);
        assert!(toolbar_bounds(&layout).is_none());
    }
}
