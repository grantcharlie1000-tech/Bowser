//! The `state` message the page and panel hosts take (fix group B): the settings page, history,
//! downloads, the bookmarks bar, the split view and the rail flyouts and dialogs. They have no
//! toolbar fields, but they follow the same text size, font, density, menu rows and window look as
//! the toolbar does (`html_sync`), so a row on the settings page acts on them too.

use crate::chrome_host::protocol::{Security, ViewState};
use crate::layout_prefs::UiDensity;
use crate::settings::Settings;
use crate::window_prefs::Look;

/// The settings-driven part of a host's state.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct HostLook {
    pub font_scale: u32,
    pub ui_font: String,
    pub density: UiDensity,
    pub compact_menus: bool,
    pub look: Look,
}

impl HostLook {
    /// From the person's settings, with the window in front (`background` false) or behind.
    pub(super) fn of(settings: &Settings, background: bool) -> Self {
        Self {
            font_scale: crate::layout_prefs::chrome_scale_percent(settings),
            ui_font: settings.ui_font.clone(),
            density: settings.ui_density,
            compact_menus: settings.compact_menus,
            look: Look::of(&settings.window, background),
        }
    }

    /// No app to read settings from (a host outliving its window): Bowser's own defaults.
    pub(super) fn neutral() -> Self {
        Self { font_scale: 100, ui_font: String::new(), density: UiDensity::Regular, compact_menus: false, look: Look::default() }
    }
}

/// A page or panel host's `state`: every toolbar field neutral, the look from `look`.
pub(super) fn page_host_view(scale: f32, solid: bool, dark: bool, look: HostLook, theme: Vec<(&'static str, String)>) -> ViewState {
    ViewState {
        scale,
        pillx: 0,
        pillw: 0,
        menux: 0,
        solid,
        dark,
        back: false,
        forward: false,
        loading: false,
        split: false,
        bookmarked: false,
        progress: 0,
        show_progress: true,
        security: Security::None,
        label: String::new(),
        permblocked: false,
        zoom: 100,
        show_zoom: false,
        private: false,
        reader: crate::reader::Pill::Hidden,
        translate: crate::translate::Pill::Hidden,
        font_scale: look.font_scale,
        ui_font: look.ui_font,
        density: look.density,
        compact_menus: look.compact_menus,
        look: look.look,
        bar: Default::default(),
        theme,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row the page and panel hosts used to ignore, set away from its default.
    fn changed() -> Settings {
        let mut s = Settings::default();
        s.font_size = crate::settings::FontSize::Large;
        s.ui_zoom = 150;
        s.ui_font = "Consolas".to_owned();
        s.ui_density = UiDensity::Compact;
        s.compact_menus = true;
        s.window.animations = false;
        s.window.simple_scrollbars = true;
        s.window.mono_icons = true;
        s.window.fade_background = true;
        s.window.panel_close_button = false;
        s.window.panel_folder_counts = false;
        s
    }

    fn json(solid: bool, look: HostLook) -> String {
        page_host_view(1.0, solid, true, look, Vec::new()).to_json()
    }

    #[test]
    fn page_host_state_carries_the_persons_text_density_and_look() {
        // split_host_state: the settings page, history, downloads, the bookmarks bar, the split view.
        let state = json(true, HostLook::of(&changed(), true));
        assert!(state.contains(r#""fontScale":173"#), "Large (115%) times 150% UI zoom: {state}");
        assert!(state.contains(r#""uiFont":"Consolas""#), "{state}");
        assert!(state.contains(r#""density":"compact""#), "{state}");
        assert!(state.contains(r#""compactMenus":true"#), "{state}");
        assert!(
            state.contains(r#""look":{"motion":false,"scroll":true,"mono":true,"fade":true,"bg":true,"panelClose":false,"folderCounts":false}"#),
            "{state}"
        );
        assert!(state.contains(r#""solid":true"#), "{state}");
    }

    #[test]
    fn panel_and_dialog_state_carries_the_persons_text_density_and_look() {
        // html_panel_state: the rail flyouts (html_panel.rs), html_dialog.rs and address_dialog.rs.
        let state = json(false, HostLook::of(&changed(), false));
        assert!(state.contains(r#""fontScale":173"#), "{state}");
        assert!(state.contains(r#""uiFont":"Consolas""#), "{state}");
        assert!(state.contains(r#""density":"compact""#), "{state}");
        assert!(state.contains(r#""compactMenus":true"#), "{state}");
        assert!(state.contains(r#""mono":true"#) && state.contains(r#""scroll":true"#) && state.contains(r#""panelClose":false"#), "{state}");
        assert!(state.contains(r#""fade":true,"bg":false"#), "fading waits for the window to go behind: {state}");
        assert!(state.contains(r#""solid":false"#), "{state}");
    }

    #[test]
    fn default_settings_keep_the_stock_look_and_no_toolbar_fields() {
        let from_settings = json(true, HostLook::of(&Settings::default(), false));
        assert_eq!(from_settings, json(true, HostLook::neutral()), "the stock settings send what a host without an app sends");
        assert!(from_settings.contains(r#""fontScale":100,"uiFont":"","density":"regular","compactMenus":false"#), "{from_settings}");
        assert!(!from_settings.contains("\"items\""), "no address bar items for a page host: {from_settings}");
    }
}
