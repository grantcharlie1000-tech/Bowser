//! Layout options (thread T6): where the bars sit and how dense the chrome is. The values live in
//! `settings.txt` next to `tab_layout` (see `settings.rs`); this module holds their types, the
//! presets the settings page offers and the arrangement the chrome geometry is built from
//! (`chrome::layout::Arrangement`).
//!
//! ```text
//! tab_layout=horizontal|tabs-top|single-row|vertical|vertical-right|bottom
//! tab_bar_visible=true
//! address_bar_position=top|bottom
//! address_bar_visible=true
//! show_title_bar=false
//! status_bar=overlay|hidden
//! ui_density=regular|compact
//! compact_menus=false
//! ui_zoom=100
//! layout_saved=
//! ```

use crate::settings::{Settings, SidePanelPosition, TabLayout};

/// `address_bar_position=top|bottom` (default `top`): the toolbar row at the top of the window, or
/// along its bottom edge.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BarPosition {
    #[default]
    Top,
    Bottom,
}

impl BarPosition {
    pub const ALL: [BarPosition; 2] = [Self::Top, Self::Bottom];

    pub fn key(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "top" => Some(Self::Top),
            "bottom" => Some(Self::Bottom),
            _ => None,
        }
    }
}

/// `status_bar=overlay|hidden` (default `overlay`). Bowser has no status bar of its own; the page
/// shows the address of a link under the pointer in a small overlay at the bottom of the page
/// (WebView2's status bar), and `hidden` switches that overlay off in every tab.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatusBar {
    #[default]
    Overlay,
    Hidden,
}

impl StatusBar {
    pub const ALL: [StatusBar; 2] = [Self::Overlay, Self::Hidden];

    pub fn key(self) -> &'static str {
        match self {
            Self::Overlay => "overlay",
            Self::Hidden => "hidden",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "overlay" => Some(Self::Overlay),
            "hidden" => Some(Self::Hidden),
            _ => None,
        }
    }

    /// Whether pages may show the link-address overlay.
    pub fn overlay_shown(self) -> bool {
        self == Self::Overlay
    }
}

/// `ui_density=regular|compact` (default `regular`): the height of list rows in the sidebar, the
/// panels and Bowser's own pages. The toolbar row never changes height (it is the native strip).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UiDensity {
    #[default]
    Regular,
    Compact,
}

impl UiDensity {
    pub const ALL: [UiDensity; 2] = [Self::Regular, Self::Compact];

    pub fn key(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Compact => "compact",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "regular" => Some(Self::Regular),
            "compact" => Some(Self::Compact),
            _ => None,
        }
    }

    /// Logical height of a native sidebar tab row at this density (the GDI fallback's list).
    pub fn sidebar_row_height(self) -> i32 {
        match self {
            Self::Regular => crate::vertical_tabs::TAB_ROW_HEIGHT,
            Self::Compact => crate::vertical_tabs::COMPACT_TAB_ROW_HEIGHT,
        }
    }
}

/// The status-bar choice in force, process-wide (one profile per process): a tab created later
/// starts with it and a change reaches every open tab through the engine (`apply_status_overlay`).
static STATUS_OVERLAY_SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_status_overlay(status: StatusBar) {
    STATUS_OVERLAY_SHOWN.store(status.overlay_shown(), std::sync::atomic::Ordering::Relaxed);
}

/// Whether pages may show the link-address overlay right now.
pub fn status_overlay_shown() -> bool {
    STATUS_OVERLAY_SHOWN.load(std::sync::atomic::Ordering::Relaxed)
}

pub const MIN_UI_ZOOM: u16 = 50;
pub const MAX_UI_ZOOM: u16 = 200;
pub const DEFAULT_UI_ZOOM: u16 = 100;
/// The slider's step, and the step the typed value is rounded to.
pub const UI_ZOOM_STEP: u16 = 5;

/// `ui_zoom` as a percentage, 50 to 200. Out-of-range input is refused, not clamped, like the
/// other typed settings.
pub fn parse_ui_zoom(text: &str) -> Result<u16, String> {
    let percent = text
        .trim()
        .trim_end_matches('%')
        .trim()
        .parse::<u16>()
        .map_err(|_| "Enter the zoom as a whole percentage, for example 100.".to_owned())?;
    if !(MIN_UI_ZOOM..=MAX_UI_ZOOM).contains(&percent) {
        return Err(format!("Enter a zoom from {MIN_UI_ZOOM} to {MAX_UI_ZOOM} percent."));
    }
    Ok(percent)
}

/// The chrome's text scale: `font_size` (small, medium, large) times `ui_zoom`, kept in the range
/// the HTML chrome and the native text painter accept (50 to 200 percent).
pub fn chrome_scale_percent(settings: &Settings) -> u32 {
    let base = settings.font_size.scale_percent();
    let zoom = u32::from(settings.ui_zoom.clamp(MIN_UI_ZOOM, MAX_UI_ZOOM));
    ((base * zoom + 50) / 100).clamp(u32::from(MIN_UI_ZOOM), u32::from(MAX_UI_ZOOM))
}

/// Longest `layout_saved` value kept.
pub const MAX_LAYOUT_SAVED: usize = 160;

/// Whether `value` may be stored as `layout_saved`: a `LayoutConfig` text, or empty for none.
pub fn is_layout_saved(value: &str) -> bool {
    value.is_empty() || (value.len() <= MAX_LAYOUT_SAVED && LayoutConfig::parse(value).is_some())
}

/// The settings a layout preset chooses, as one value. `key` renders it, `parse` reads it back; the
/// settings page keeps the user's own arrangement in `layout_saved` in this form (the Saved tile).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayoutConfig {
    pub tab_layout: TabLayout,
    pub tab_bar_visible: bool,
    pub address_bar_position: BarPosition,
    pub address_bar_visible: bool,
    pub side_panel_position: SidePanelPosition,
    pub status_bar: StatusBar,
    pub show_title_bar: bool,
}

impl LayoutConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            tab_layout: settings.tab_layout,
            tab_bar_visible: settings.tab_bar_visible,
            address_bar_position: settings.address_bar_position,
            address_bar_visible: settings.address_bar_visible,
            side_panel_position: settings.side_panel_position,
            status_bar: settings.status_bar,
            show_title_bar: settings.show_title_bar,
        }
    }

    pub fn apply_to(self, settings: &mut Settings) {
        settings.tab_layout = self.tab_layout;
        settings.tab_bar_visible = self.tab_bar_visible;
        settings.address_bar_position = self.address_bar_position;
        settings.address_bar_visible = self.address_bar_visible;
        settings.side_panel_position = self.side_panel_position;
        settings.status_bar = self.status_bar;
        settings.show_title_bar = self.show_title_bar;
    }

    /// `tabs=vertical,tabbar=true,address=top,addressbar=true,panel=left,status=overlay,title=false`.
    pub fn key(self) -> String {
        format!(
            "tabs={},tabbar={},address={},addressbar={},panel={},status={},title={}",
            self.tab_layout.key(),
            self.tab_bar_visible,
            self.address_bar_position.key(),
            self.address_bar_visible,
            self.side_panel_position.key(),
            self.status_bar.key(),
            self.show_title_bar,
        )
    }

    /// Reads [`LayoutConfig::key`]'s form: every field once, exact keys, exact values.
    pub fn parse(text: &str) -> Option<Self> {
        let mut config = Self::from_settings(&Settings::default());
        let mut seen = 0u8;
        for pair in text.split(',') {
            let (key, value) = pair.split_once('=')?;
            let bit = match key {
                "tabs" => { config.tab_layout = TabLayout::ALL.into_iter().find(|layout| layout.key() == value)?; 1 }
                "tabbar" => { config.tab_bar_visible = parse_flag(value)?; 2 }
                "address" => { config.address_bar_position = BarPosition::ALL.into_iter().find(|p| p.key() == value)?; 4 }
                "addressbar" => { config.address_bar_visible = parse_flag(value)?; 8 }
                "panel" => { config.side_panel_position = SidePanelPosition::ALL.into_iter().find(|p| p.key() == value)?; 16 }
                "status" => { config.status_bar = StatusBar::ALL.into_iter().find(|s| s.key() == value)?; 32 }
                "title" => { config.show_title_bar = parse_flag(value)?; 64 }
                _ => return None,
            };
            if seen & bit != 0 {
                return None;
            }
            seen |= bit;
        }
        (seen == 127).then_some(config)
    }
}

fn parse_flag(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// The layout presets, in the order the page shows them. `Saved` is the user's own configuration
/// (`layout_saved`) and is not listed here. Auto-hide is a later thread's (T11) and is left out
/// until it exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Preset {
    Simple,
    Classic,
    VerticalLeft,
    VerticalRight,
    Bottom,
}

impl Preset {
    pub const ALL: [Preset; 5] = [Self::Simple, Self::Classic, Self::VerticalLeft, Self::VerticalRight, Self::Bottom];

    pub fn key(self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Classic => "classic",
            Self::VerticalLeft => "vertical-left",
            Self::VerticalRight => "vertical-right",
            Self::Bottom => "bottom",
        }
    }

    /// The settings the preset chooses. Everything else the user set stays.
    pub fn config(self) -> LayoutConfig {
        let base = LayoutConfig::from_settings(&Settings::default());
        match self {
            // One row, nothing else: the quietest chrome.
            Self::Simple => LayoutConfig { tab_layout: TabLayout::SingleRow, status_bar: StatusBar::Hidden, ..base },
            // Tabs above the address bar, as browsers have always had them.
            Self::Classic => LayoutConfig { tab_layout: TabLayout::TabsTop, ..base },
            Self::VerticalLeft => LayoutConfig { tab_layout: TabLayout::Vertical, side_panel_position: SidePanelPosition::Left, ..base },
            Self::VerticalRight => LayoutConfig { tab_layout: TabLayout::VerticalRight, side_panel_position: SidePanelPosition::Right, ..base },
            Self::Bottom => LayoutConfig { tab_layout: TabLayout::TabsBottom, ..base },
        }
    }

    /// The preset whose configuration `config` is, if any.
    pub fn matching(config: LayoutConfig) -> Option<Preset> {
        Self::ALL.into_iter().find(|preset| preset.config() == config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_round_trip_through_their_keys() {
        for position in BarPosition::ALL {
            assert_eq!(BarPosition::parse(position.key()), Some(position));
            assert_eq!(BarPosition::parse(&position.key().to_ascii_uppercase()), Some(position));
        }
        for status in StatusBar::ALL {
            assert_eq!(StatusBar::parse(status.key()), Some(status));
        }
        for density in UiDensity::ALL {
            assert_eq!(UiDensity::parse(density.key()), Some(density));
        }
        assert_eq!(BarPosition::parse("middle"), None);
        assert_eq!(StatusBar::parse("show"), None, "Bowser has no status bar to show");
        assert_eq!(UiDensity::parse("dense"), None);
        assert!(StatusBar::Overlay.overlay_shown());
        assert!(!StatusBar::Hidden.overlay_shown());
    }

    #[test]
    fn ui_zoom_is_typed_as_a_whole_percentage_in_range() {
        assert_eq!(parse_ui_zoom("100"), Ok(100));
        assert_eq!(parse_ui_zoom(" 125% "), Ok(125));
        assert_eq!(parse_ui_zoom("50"), Ok(50));
        assert_eq!(parse_ui_zoom("200"), Ok(200));
        assert!(parse_ui_zoom("49").is_err());
        assert!(parse_ui_zoom("201").is_err());
        assert!(parse_ui_zoom("big").is_err());
        assert!(parse_ui_zoom("100.5").is_err());
    }

    #[test]
    fn the_chrome_scale_multiplies_font_size_by_ui_zoom_within_the_protocol_range() {
        let mut settings = Settings::default();
        assert_eq!(chrome_scale_percent(&settings), 100);
        settings.ui_zoom = 150;
        assert_eq!(chrome_scale_percent(&settings), 150);
        settings.font_size = crate::settings::FontSize::Large;
        assert_eq!(chrome_scale_percent(&settings), 173, "115% of 150%, rounded");
        settings.ui_zoom = 200;
        assert_eq!(chrome_scale_percent(&settings), 200, "never past what the HTML chrome accepts");
        settings.font_size = crate::settings::FontSize::Small;
        settings.ui_zoom = 50;
        assert_eq!(chrome_scale_percent(&settings), 50, "never below it either");
    }

    #[test]
    fn a_layout_config_round_trips_and_rejects_anything_else() {
        for preset in Preset::ALL {
            let config = preset.config();
            assert_eq!(LayoutConfig::parse(&config.key()), Some(config), "{preset:?}");
            assert!(is_layout_saved(&config.key()));
            assert_eq!(Preset::matching(config), Some(preset), "{preset:?}");
        }
        let defaults = LayoutConfig::from_settings(&Settings::default());
        assert_eq!(defaults.key(), "tabs=horizontal,tabbar=true,address=top,addressbar=true,panel=left,status=overlay,title=false");
        assert_eq!(Preset::matching(defaults), None, "the default (toolbar above tabs) is the user's own, not a preset");
        assert!(is_layout_saved(""));
        for bad in [
            "tabs=horizontal",
            "tabs=sideways,tabbar=true,address=top,addressbar=true,panel=left,status=overlay,title=false",
            "tabs=horizontal,tabbar=yes,address=top,addressbar=true,panel=left,status=overlay,title=false",
            "tabs=horizontal,tabbar=true,address=top,addressbar=true,panel=left,status=overlay,title=false,title=true",
            "tabs=horizontal,tabbar=true,address=top,addressbar=true,panel=left,status=overlay,title=false,x=1",
            "TABS=horizontal,tabbar=true,address=top,addressbar=true,panel=left,status=overlay,title=false",
        ] {
            assert_eq!(LayoutConfig::parse(bad), None, "{bad}");
            assert!(!is_layout_saved(bad), "{bad}");
        }
        assert!(!is_layout_saved(&"a".repeat(MAX_LAYOUT_SAVED + 1)));
    }

    #[test]
    fn presets_choose_distinct_arrangements_and_apply_only_layout_keys() {
        let configs: Vec<_> = Preset::ALL.iter().map(|preset| preset.config()).collect();
        for (i, a) in configs.iter().enumerate() {
            for b in &configs[i + 1..] {
                assert_ne!(a, b);
            }
        }
        let mut settings = Settings { sleep_after_secs: 999, ..Settings::default() };
        Preset::VerticalRight.config().apply_to(&mut settings);
        assert_eq!(settings.tab_layout, TabLayout::VerticalRight);
        assert_eq!(settings.side_panel_position, SidePanelPosition::Right);
        assert_eq!(settings.sleep_after_secs, 999, "a preset touches layout keys only");
        assert_eq!(Preset::Simple.config().status_bar, StatusBar::Hidden);
        assert_eq!(Preset::Bottom.config().tab_layout, TabLayout::TabsBottom);
    }
}
