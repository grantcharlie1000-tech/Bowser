//! How websites are asked to look: the colour scheme they are told the browser prefers
//! (`website_appearance`) and whether pages without a dark theme of their own are darkened
//! (`force_dark_websites`).
//!
//! The choice is process-wide (one profile per process), so a tab that is created later starts with
//! it and a change reaches every open tab. The engine applies it: the preferred colour scheme on the
//! browsing profile, and Chromium's automatic dark mode per page through the DevTools protocol.
//! Bowser's own pages (settings, history, the toolbar) live in a separate WebView2 environment and
//! never see either, so the browser's chrome does not change with this.

use std::sync::atomic::{AtomicU8, Ordering};

use crate::settings::WebsiteAppearance;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebsiteLook {
    pub appearance: WebsiteAppearance,
    pub force_dark: bool,
}

impl Default for WebsiteLook {
    fn default() -> Self {
        Self { appearance: WebsiteAppearance::Auto, force_dark: false }
    }
}

impl WebsiteLook {
    pub fn from_settings(settings: &crate::settings::Settings) -> Self {
        Self { appearance: settings.website_appearance, force_dark: settings.force_dark_websites }
    }

    /// Parameters of the DevTools call `Emulation.setAutoDarkModeOverride` for this look: turn
    /// automatic dark mode on, or (empty parameters) clear any override so the page is untouched.
    pub fn auto_dark_params(self) -> &'static str {
        if self.force_dark { r#"{"enabled":true}"# } else { "{}" }
    }
}

const FORCE_BIT: u8 = 0b100;

fn pack(look: WebsiteLook) -> u8 {
    let appearance = match look.appearance {
        WebsiteAppearance::Auto => 0,
        WebsiteAppearance::Light => 1,
        WebsiteAppearance::Dark => 2,
    };
    appearance | if look.force_dark { FORCE_BIT } else { 0 }
}

fn unpack(bits: u8) -> WebsiteLook {
    WebsiteLook {
        appearance: match bits & 0b11 {
            1 => WebsiteAppearance::Light,
            2 => WebsiteAppearance::Dark,
            _ => WebsiteAppearance::Auto,
        },
        force_dark: bits & FORCE_BIT != 0,
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The look in force.
pub fn current() -> WebsiteLook {
    unpack(CURRENT.load(Ordering::Relaxed))
}

/// Records `look`; `true` when it differs from the one in force (the engine then re-applies it).
pub fn set(look: WebsiteLook) -> bool {
    CURRENT.swap(pack(look), Ordering::Relaxed) != pack(look)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_look_survives_packing() {
        for appearance in [WebsiteAppearance::Auto, WebsiteAppearance::Light, WebsiteAppearance::Dark] {
            for force_dark in [false, true] {
                let look = WebsiteLook { appearance, force_dark };
                assert_eq!(unpack(pack(look)), look);
            }
        }
    }

    #[test]
    fn forcing_dark_sends_the_enable_call_and_not_forcing_clears_the_override() {
        let on = WebsiteLook { appearance: WebsiteAppearance::Auto, force_dark: true };
        assert_eq!(on.auto_dark_params(), r#"{"enabled":true}"#);
        assert_eq!(WebsiteLook::default().auto_dark_params(), "{}");
    }

    #[test]
    fn the_settings_choose_the_look_and_a_change_is_reported_once() {
        let settings = crate::settings::Settings::parse("website_appearance=dark\nforce_dark_websites=true\n");
        let look = WebsiteLook::from_settings(&settings);
        assert_eq!(look, WebsiteLook { appearance: WebsiteAppearance::Dark, force_dark: true });
        assert_eq!(WebsiteLook::from_settings(&crate::settings::Settings::default()), WebsiteLook::default());
        // The static is shared with other tests only through this one, so it sets and restores.
        let before = current();
        set(WebsiteLook::default());
        assert!(set(look), "a different look is a change");
        assert!(!set(look), "the same look is not");
        assert_eq!(current(), look);
        set(before);
    }
}
