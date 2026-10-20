//! User-editable settings read once at startup from `%LOCALAPPDATA%\Bowser\settings.txt`.
//!
//! The file is `key=value` per line. Blank lines and lines starting with `#` are ignored, as are
//! unknown keys and values that do not parse, so a typo never prevents startup. An invalid explicit
//! search engine falls back to Optimised. Values are clamped to a safe range.
//!
//! ```text
//! sleep_enabled=true
//! sleep_after_secs=300
//! blocker_enabled=true
//! blocker_allow=example.com,other.org
//! search_engine=optimised
//! automatic_tab_grouping=true
//! tab_layout=horizontal
//! sidebar_width=260
//! efficiency_mode=auto
//! tracking_prevention=balanced
//! ```

use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};

use crate::navigation::SearchEngine;
pub use crate::settings_privacy::{BlockerLevel, HistoryRetention, PrivateTheme, WebsiteAppearance};
use crate::site_prefs::{ZOOM_MAX as DEFAULT_ZOOM_MAX, ZOOM_MIN as DEFAULT_ZOOM_MIN};

pub const FILE_NAME: &str = "settings.txt";
/// Starter file written by "Open settings file" when none exists. Every setting is commented out.
pub const TEMPLATE: &str = "# Bowser settings. Restart Bowser after editing.\r\n\
    # tab_layout=horizontal\r\n\
    # sidebar_width=260\r\n\
    # side_panel_width=340\r\n\
    # sleep_enabled=true\r\n\
    # sleep_after_secs=300\r\n\
    # blocker_enabled=true\r\n\
    # blocker_allow=example.com,other.org\r\n\
    # search_engine=optimised\r\n\
    # automatic_tab_grouping=true\r\n\
    # efficiency_mode=auto\r\n\
    # tracking_prevention=balanced\r\n\
    # theme=helium\r\n\
    # theme_accent=\r\n\
    # toolbar_buttons=back,forward,reload,site-info,split,reader,extensions,downloads\r\n\
    # centred_address_bar=true\r\n\
    # minimal_url=true\r\n\
    # show_load_progress=true\r\n\
    # frameless=false\r\n\
    # auto_hide_bars=tabs,address,bookmarks\r\n\
    # auto_hide_fullscreen=true\r\n\
    # rounded_content_frame=true\r\n\
    # show_home_button=true\r\n\
    # bookmarks_bar=never\r\n\
    # bookmarks_bar_tab_groups=false\r\n\
    # side_panel_position=left\r\n\
    # tab_preview_images=true\r\n\
    # tab_preview_memory=true\r\n\
    # font_size=medium\r\n\
    # ui_font=\r\n\
    # default_zoom=100\r\n\
    # show_zoom_indicator=true\r\n\
    # new_tab_beside_active=true\r\n\
    # ctrl_tab_mru=true\r\n\
    # shift_right_click_context=false\r\n\
    # ctrl_shift_c_copy_link=true\r\n\
    # ctrl_s_vertical_tabs=false\r\n\
    # pin_new_tab_groups=false\r\n\
    # split_edge_drop=true\r\n\
    # block_youtube_video_ads=true\r\n\
    # download_dir=\r\n\
    # ask_download_location=false\r\n\
    # translate_offer=true\r\n\
    # translate_auto=false\r\n\
    # translate_target=\r\n\
    # blocker_level=ads\r\n\
    # cookie_notices=false\r\n\
    # history_retention=3m\r\n\
    # website_appearance=auto\r\n\
    # force_dark_websites=false\r\n\
    # private_theme=follow\r\n\
    # tab_bar_visible=true\r\n\
    # address_bar_position=top\r\n\
    # address_bar_visible=true\r\n\
    # show_title_bar=false\r\n\
    # status_bar=overlay\r\n\
    # ui_density=regular\r\n\
    # compact_menus=false\r\n\
    # ui_zoom=100\r\n\
    # toolbar_tabs=closed-tabs\r\n\
    # toolbar_panel=\r\n\
    # feeds_detect=true\r\n\
    # feeds_refresh_minutes=60\r\n\
    # start_group_reopen=last\r\n\
    # start_titles=always\r\n\
    # start_add_button=true\r\n\
    # start_columns=6\r\n\
    # start_thumb_size=3\r\n\
    # start_drag_reorder=true\r\n\
    # homepage=\r\n\
    # startup=start_page\r\n\
    # startup_pages=\r\n\
    # start_widgets=false\r\n\
    # start_widget_columns=2\r\n\
    # start_widget_height=regular\r\n\
    # start_widget_bg=translucent\r\n\
    # start_widget_color=#1f2933\r\n\
    # start_widget_drag=true\r\n\
    # tabs_new_tab_page=start\r\n\
    # tabs_close_activation=right\r\n\
    # tabs_keep_window=true\r\n\
    # (more tabs_* options: see Settings > Tabs)\r\n\
    # theme_schedule=system\r\n\
    # (more theme_* options: see Settings > Appearance)\r\n";
/// Largest settings file that will be read; a larger file is ignored entirely. Holds every setting
/// with each list at its cap (`a_profile_with_every_list_at_its_cap_fits_the_file`).
pub const MAX_FILE_BYTES: usize = 64 * 1024;
pub const DEFAULT_SLEEP_AFTER_SECS: u64 = 300;
/// Hidden tabs are suspended after 30 s, so sleeping earlier would never be eligible.
pub const MIN_SLEEP_AFTER_SECS: u64 = 30;
pub const MAX_SLEEP_AFTER_SECS: u64 = 7 * 24 * 60 * 60;

/// Most `blocker_allow` entries kept; the rest of a longer list is ignored.
pub const MAX_BLOCKER_ALLOW: usize = 500;
/// Longest `blocker_allow` value in bytes (the sites and the commas between them): 500 sites of up
/// to 40 characters. Keeps the page's `save` and the file within their caps (the page's
/// `MAX_ALLOW_BYTES`); the sites past it in a longer list are ignored.
pub const MAX_BLOCKER_ALLOW_BYTES: usize = 20 * 1024;

/// `efficiency_mode`: when Bowser trims background work to save power. `Auto` follows the power
/// source (battery or Battery Saver), `On` and `Off` ignore it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EfficiencyMode {
    #[default]
    Auto,
    On,
    Off,
}

impl EfficiencyMode {
    pub fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::On => "on",
            Self::Off => "off",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "on" => Some(Self::On),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}

/// `tracking_prevention`: WebView2's tracking-prevention level. Balanced is the default; Strict
/// blocks more trackers and can break some sites. Applied when the engine starts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TrackingLevel {
    #[default]
    Balanced,
    Strict,
}

impl TrackingLevel {
    pub fn key(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Strict => "strict",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "balanced" => Some(Self::Balanced),
            "strict" => Some(Self::Strict),
            _ => None,
        }
    }
}

/// Arrangement of the browser chrome: where the toolbar and the tab list sit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TabLayout {
    /// Toolbar row on top, full-width tab row below it. The key stays `horizontal` so settings
    /// files written before the other layouts existed keep working.
    #[default]
    ToolbarTop,
    /// Tab row on top (with the window buttons), toolbar row below it.
    TabsTop,
    /// One row: navigation and address bar, then the tabs, then the window buttons.
    SingleRow,
    /// Sidebar tab list at the left; the top tab row is absent.
    Vertical,
    // --- layout (thread T6) ---
    /// Sidebar tab list at the right, under the toolbar row; the top tab row is absent.
    VerticalRight,
    /// Toolbar row on top, full-width tab row along the bottom edge of the window.
    TabsBottom,
    // --- end layout ---
}

impl TabLayout {
    /// Every layout, in the order the settings dropdown lists them.
    pub const ALL: [TabLayout; 6] = [Self::ToolbarTop, Self::TabsTop, Self::SingleRow, Self::Vertical, Self::VerticalRight, Self::TabsBottom];

    /// The value saved in `settings.txt` and exchanged with the settings page.
    pub fn key(self) -> &'static str {
        match self {
            Self::ToolbarTop => "horizontal",
            Self::TabsTop => "tabs-top",
            Self::SingleRow => "single-row",
            Self::Vertical => "vertical",
            Self::VerticalRight => "vertical-right",
            Self::TabsBottom => "bottom",
        }
    }

    /// The layout named by `key`, in any letter case.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layout| layout.key().eq_ignore_ascii_case(key))
    }

    /// Whether the tab list is a sidebar (left or right) rather than a row of tabs in the chrome.
    pub fn is_vertical(self) -> bool {
        matches!(self, Self::Vertical | Self::VerticalRight)
    }

    // --- layout (thread T6) ---
    /// Which edge the sidebar is docked to, for the two sidebar layouts.
    pub fn sidebar_side(self) -> Option<SidePanelPosition> {
        match self {
            Self::Vertical => Some(SidePanelPosition::Left),
            Self::VerticalRight => Some(SidePanelPosition::Right),
            _ => None,
        }
    }

    /// Where the tab bar sits: `top`, `left`, `right` or `bottom` (the settings page's Tab bar row).
    pub fn tab_bar_edge(self) -> &'static str {
        match self {
            Self::ToolbarTop | Self::TabsTop | Self::SingleRow => "top",
            Self::Vertical => "left",
            Self::VerticalRight => "right",
            Self::TabsBottom => "bottom",
        }
    }

    /// The layouts whose tab bar is a row at the top, in page order: the "top style" sub-choice.
    pub const TOP_STYLES: [TabLayout; 3] = [Self::ToolbarTop, Self::TabsTop, Self::SingleRow];
    // --- end layout ---
}

/// `theme=helium|system` (default `helium`): Bowser's own fixed colours, or the Windows accent and
/// light/dark choice. `theme_accent` can override either with a chosen colour.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Theme {
    #[default]
    Helium,
    System,
}

impl Theme {
    pub fn key(self) -> &'static str {
        match self {
            Self::Helium => "helium",
            Self::System => "system",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "helium" => Some(Self::Helium),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

/// Longest `ui_font` value kept, in characters.
pub const MAX_UI_FONT_CHARS: usize = 64;

/// Whether `value` is exactly `#` followed by six hex digits (lower or upper case).
fn is_hex_colour(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit)
}

/// Parses a `theme_accent` value: empty (the theme's own colour) or `#rrggbb`, canonicalised to
/// lower case. `None` for anything else, including a value of the wrong length.
pub fn parse_theme_accent(value: &str) -> Option<Option<String>> {
    if value.is_empty() {
        return Some(None);
    }
    is_hex_colour(value).then(|| Some(value.to_ascii_lowercase()))
}

/// The buttons the native (GDI) settings window lists for the toolbar. The HTML chrome's toolbar
/// editor (`crate::toolbar_prefs`) offers more, and keeps them when this window saves.
pub const KNOWN_TOOLBAR_BUTTONS: [&str; 8] = [
    "back", "forward", "reload", "site-info", "bookmark", "newtab", "split", "reader",
];

/// Parses a `toolbar_buttons` value (the address bar's items, `crate::toolbar_prefs`): known button ids
/// and command buttons, each at most once. `None` for an unknown id, a repeated id or too many.
pub fn parse_toolbar_buttons(value: &str) -> Option<Vec<String>> {
    crate::toolbar_prefs::parse_list(crate::toolbar_prefs::Bar::Address, value)
}

fn default_toolbar_buttons() -> Vec<String> {
    crate::toolbar_prefs::Bar::Address.default_list()
}

/// Parses a `ui_font` value: any text up to [`MAX_UI_FONT_CHARS`] characters with no control
/// characters. Empty means the system font. `None` (keep the previous value) otherwise.
pub fn parse_ui_font(value: &str) -> Option<String> {
    (value.chars().count() <= MAX_UI_FONT_CHARS && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

/// Longest `download_dir` kept, in characters (Windows' MAX_PATH).
pub const MAX_DOWNLOAD_DIR_CHARS: usize = 260;

/// Parses a `download_dir` value: empty (the Downloads folder) or an absolute path of at most
/// [`MAX_DOWNLOAD_DIR_CHARS`] characters with no control characters or characters Windows rejects in
/// a path. `None` (keep the previous value) otherwise. Whether the folder exists is decided when a
/// download starts.
pub fn parse_download_dir(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return Some(String::new());
    }
    let valid = value.chars().count() <= MAX_DOWNLOAD_DIR_CHARS
        && !value.chars().any(|c| c.is_control() || "<>\"|?*".contains(c))
        && Path::new(value).is_absolute();
    valid.then(|| value.to_owned())
}

/// Parses a `translate_target` value: empty (the Windows display language, resolved when first needed)
/// or a language tag reduced to its primary subtag, for example `en-GB` to `en`. `None` (keep the
/// previous value) for anything else.
pub fn parse_translate_target(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return Some(String::new());
    }
    crate::translate::normalize_lang(value)
}

pub const MIN_DEFAULT_ZOOM: u16 = DEFAULT_ZOOM_MIN;
pub const MAX_DEFAULT_ZOOM: u16 = DEFAULT_ZOOM_MAX;

/// `bookmarks_bar=always|never|newtab` (default `never`): when the bookmarks bar is shown.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BookmarksBar {
    Always,
    #[default]
    Never,
    NewTab,
}

impl BookmarksBar {
    pub fn key(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Never => "never",
            Self::NewTab => "newtab",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            "newtab" => Some(Self::NewTab),
            _ => None,
        }
    }
}

/// `side_panel_position=left|right` (default `left`): which edge the rail panels (Helium panels)
/// are docked to.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SidePanelPosition {
    #[default]
    Left,
    Right,
}

impl SidePanelPosition {
    pub const ALL: [SidePanelPosition; 2] = [Self::Left, Self::Right]; // --- layout (thread T6) ---

    pub fn key(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}

/// `font_size=small|medium|large` (default `medium`): the chrome's text size.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FontSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl FontSize {
    pub fn key(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "small" => Some(Self::Small),
            "medium" => Some(Self::Medium),
            "large" => Some(Self::Large),
            _ => None,
        }
    }

    /// The chrome's text size as a percentage of the system message font, for the native chrome and
    /// the HTML chrome's root font-size alike.
    pub fn scale_percent(self) -> u32 {
        match self {
            Self::Small => 90,
            Self::Medium => 100,
            Self::Large => 115,
        }
    }
}

pub const MIN_SIDEBAR_WIDTH: u16 = 180;
/// Logical width of the collapsed (icons-only) vertical sidebar.
pub const COLLAPSED_SIDEBAR_WIDTH: u16 = 48;
pub const MAX_SIDEBAR_WIDTH: u16 = 360;
pub const DEFAULT_SIDEBAR_WIDTH: u16 = 260;
pub const MIN_SIDE_PANEL_WIDTH: u16 = 280;
pub const MAX_SIDE_PANEL_WIDTH: u16 = 520;
pub const DEFAULT_SIDE_PANEL_WIDTH: u16 = 340;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settings {
    pub tab_layout: TabLayout,
    /// Sidebar width in logical 96 DPI pixels.
    pub sidebar_width: u16,
    /// Vertical sidebar collapsed to an icons-only strip. `sidebar_width` is kept so the
    /// expanded width survives a collapse.
    pub sidebar_collapsed: bool,
    /// Opera-style rail panel width in logical 96 DPI pixels.
    pub side_panel_width: u16,
    /// Whether hidden tabs sleep (are discarded) after `sleep_after_secs` even without memory
    /// pressure. Under memory pressure the long-hidden tabs are discarded regardless.
    pub sleep_enabled: bool,
    pub sleep_after_secs: u64,
    /// `blocker_enabled`: master switch for the ad/tracker blocker (default on).
    pub blocker_enabled: bool,
    /// `blocker_allow=example.com,other.org`: sites where the blocker never blocks.
    pub blocker_allow: Vec<String>,
    /// `search_engine=optimised`, `google` or `duckduckgo`. Existing saved choices are preserved;
    /// without this key, a valid legacy `search-provider.txt` remains in use, otherwise Optimised.
    pub search_engine: Option<SearchEngine>,
    /// `automatic_tab_grouping`: whether tabs are grouped by site without being asked (default on).
    /// Read at startup. When off, no new automatic groups form; explicit groups still work.
    pub automatic_tab_grouping: bool,
    /// `efficiency_mode=auto|on|off` (default auto). Applies immediately.
    pub efficiency_mode: EfficiencyMode,
    /// `tracking_prevention=balanced|strict` (default balanced). Applies on the next start.
    pub tracking_prevention: TrackingLevel,
    /// `theme=helium|system` (default helium). Applies immediately.
    pub theme: Theme,
    /// `theme_accent`: an override colour, or `None` for the theme's own. Applies immediately.
    pub theme_accent: Option<String>,
    /// `toolbar_buttons`: the address bar's buttons, in the order shown (`crate::toolbar_prefs`). Applies at once.
    pub toolbar_buttons: Vec<String>,
    /// `toolbar_tabs`: the tab bar's buttons. Applies at once.
    pub toolbar_tabs: Vec<String>,
    /// `toolbar_panel`: the extra buttons on the panel bar. Applies at once.
    pub toolbar_panel: Vec<String>,
    /// `feeds_detect` (default true): the address bar shows a button when the page offers a feed.
    pub feeds_detect: bool,
    /// `feeds_refresh_minutes` (0, 15, 30, 60, 180 or 720; default 60): the Feeds panel fetches the
    /// feeds older than this when it opens; 0 fetches only when Refresh is pressed.
    pub feeds_refresh_minutes: u32,
    /// `centred_address_bar` (default true). Applies immediately.
    pub centred_address_bar: bool,
    /// `minimal_url` (default true): the registrable host (and a path when useful) while unfocused,
    /// the full address while focused. Applies immediately.
    pub minimal_url: bool,
    /// `show_load_progress` (default true). Applies immediately.
    pub show_load_progress: bool,
    /// `frameless` (default false): the master "Auto-hide toolbars and tabs" switch. The bars chosen in
    /// `auto_hide` leave the page's layout and float over it when the pointer rests at their edge
    /// (`src/auto_hide.rs`). The key keeps its name so a saved value carries over. Applies immediately.
    pub frameless: bool,
    /// `rounded_content_frame` (default true). Applies immediately.
    pub rounded_content_frame: bool,
    /// `show_home_button` (default true): the Home button in the rail. Applies immediately.
    pub show_home_button: bool,
    /// `bookmarks_bar=always|never|newtab` (default never). Applies immediately (`App::apply_live_settings`).
    pub bookmarks_bar: BookmarksBar,
    /// `bookmarks_bar_tab_groups` (default false). Applies immediately (`App::apply_live_settings`).
    pub bookmarks_bar_tab_groups: bool,
    /// `side_panel_position=left|right` (default left). Read from the file each time the side panel
    /// opens, so it applies from the next panel opened; memory keeps the startup value.
    pub side_panel_position: SidePanelPosition,
    /// `tab_preview_images` (default true): the hover card's thumbnail. Applies immediately.
    pub tab_preview_images: bool,
    /// `tab_preview_memory` (default true): the hover card's memory line. Applies immediately.
    pub tab_preview_memory: bool,
    /// `font_size=small|medium|large` (default medium). Applies immediately.
    pub font_size: FontSize,
    /// `ui_font` (default empty, the system font). Applies immediately.
    pub ui_font: String,
    /// `default_zoom` in percent, 25 to 500 (default 100): the page zoom new tabs start at. A
    /// site's own stored zoom still wins over this.
    pub default_zoom: u16,
    /// `show_zoom_indicator` (default true): the toolbar's zoom chip while not at 100%.
    pub show_zoom_indicator: bool,
    /// `new_tab_beside_active` (default true). Applies immediately.
    pub new_tab_beside_active: bool,
    /// `ctrl_tab_mru` (default true): Ctrl+Tab and Ctrl+Shift+Tab swap to the tab last active
    /// instead of stepping through strip order.
    pub ctrl_tab_mru: bool,
    /// `shift_right_click_context` (default false): Shift+Right Click forces the system context
    /// menu over Bowser's own tab and rail menus. Right click is unaffected when this is off.
    pub shift_right_click_context: bool,
    /// `ctrl_shift_c_copy_link` (default true): Ctrl+Shift+C copies the active tab's address.
    pub ctrl_shift_c_copy_link: bool,
    /// `ctrl_s_vertical_tabs` (default false): Ctrl+S toggles the vertical sidebar's collapsed
    /// state while the vertical layout is active; otherwise Ctrl+S is left for the page.
    pub ctrl_s_vertical_tabs: bool,
    /// `pin_new_tab_groups` (default false): a tab group formed from now on starts pinned.
    pub pin_new_tab_groups: bool,
    /// `split_edge_drop` (default true): dropping a dragged tab on the window's left or right edge
    /// starts a split.
    pub split_edge_drop: bool,
    /// `block_youtube_video_ads` (default true). Read by the blocker; this module only stores it.
    pub block_youtube_video_ads: bool,
    /// `download_dir` (default empty: the Downloads known folder): an absolute folder downloads are
    /// saved to. Applies immediately; a folder that cannot be used falls back to the default.
    pub download_dir: String,
    /// `ask_download_location` (default false): show a Save dialog for each download first.
    pub ask_download_location: bool,
    /// `translate_offer` (default true): show the toolbar's translate button on a page in another
    /// language. It only offers; nothing is downloaded or changed until the user clicks.
    pub translate_offer: bool,
    /// `translate_auto` (default false): translate a site automatically when it is set to "Always
    /// translate this site". Off, that choice only keeps the button offered.
    pub translate_auto: bool,
    /// `translate_target` (default empty: the Windows display language): the language pages are
    /// translated into, as a primary subtag such as `en`.
    pub translate_target: String,
    // --- privacy, history and website appearance (thread T3) ---
    /// `blocker_level` (default ads): what the blocker stops while `blocker_enabled` is on.
    pub blocker_level: BlockerLevel,
    /// `cookie_notices` (default false): hide cookie-consent banners with the bundled and added
    /// cookie lists, with or without the ad blocker. A site can switch it off on its own.
    pub cookie_notices: bool,
    /// `history_retention` (default 3m): how long the history list keeps a visit.
    pub history_retention: HistoryRetention,
    /// `website_appearance` (default auto): the colour scheme sites are told the browser prefers.
    pub website_appearance: WebsiteAppearance,
    /// `force_dark_websites` (default false): darken pages that have no dark theme of their own.
    pub force_dark_websites: bool,
    /// `private_theme` (default follow): the light or dark look while a private tab is shown.
    pub private_theme: PrivateTheme,
    // --- end privacy, history and website appearance ---
    // --- auto-hide (T11) ---
    /// Which bars auto-hide and whether full screen always hides them (`src/auto_hide.rs`).
    pub auto_hide: crate::auto_hide::AutoHideSettings,
    // --- end auto-hide ---
    // --- start page ---
    /// The start page, speed dials, dashboard widgets, homepage and startup (`src/start_page.rs`).
    pub start: crate::start_page::StartSettings,
    // --- end start page ---
    // --- address bar, search and bookmarks (src/address_settings.rs) ---
    /// The address bar, drop-down, search field, scoped nickname and bookmark options. Stored as
    /// further `key=value` lines of this file; changed one key at a time by the settings page.
    pub address: crate::address_settings::AddressSettings,
    // --- end address bar, search and bookmarks ---
    // --- page tools (T7): capture, spatial navigation, reader defaults, zoom scope, pop-out video ---
    /// The `pt_*` options (src/page_tools.rs), one line each in `settings.txt`.
    pub page_tools: crate::page_tools::PageTools,
    // --- end page tools ---
    // --- tab options (src/tab_options.rs) ---
    /// The `tabs_*` options of the settings page's Tabs section.
    pub tabs: crate::tab_options::TabOptions,
    // --- end tab options ---
    // --- layout (thread T6, src/layout_prefs.rs) ---
    /// `tab_bar_visible` (default true): off, the tab row or sidebar is not laid out at all.
    pub tab_bar_visible: bool,
    /// `address_bar_position=top|bottom` (default top): the toolbar row's edge.
    pub address_bar_position: crate::layout_prefs::BarPosition,
    /// `address_bar_visible` (default true): off, the toolbar row is not laid out; Ctrl+L brings it
    /// back until the address field loses focus.
    pub address_bar_visible: bool,
    /// `show_title_bar` (default false): with the tab bar on a side, a title row above the
    /// toolbar holds the page title and the window buttons.
    pub show_title_bar: bool,
    /// `status_bar=overlay|hidden` (default overlay): the page's link-address overlay.
    pub status_bar: crate::layout_prefs::StatusBar,
    /// `ui_density=regular|compact` (default regular): list row heights in the chrome.
    pub ui_density: crate::layout_prefs::UiDensity,
    /// `compact_menus` (default false): shorter menu rows.
    pub compact_menus: bool,
    /// `ui_zoom` (50 to 200, default 100): scales the chrome's text with `font_size`.
    pub ui_zoom: u16,
    /// `layout_saved`: the user's own layout as `LayoutConfig::key`, or empty. The settings page's
    /// Saved tile.
    pub layout_saved: String,
    // --- end layout ---
    // --- theme options (src/theme_custom.rs) ---
    /// The `theme_*` options of the settings page's Appearance area: colours, accent mode, schedule.
    pub themes: crate::theme_custom::ThemeOptions,
    // --- end theme options ---
    // --- window and panels (thread T9) --- src/window_prefs.rs
    pub window: crate::window_prefs::WindowPrefs,
    // --- end window and panels ---
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tab_layout: TabLayout::ToolbarTop,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            sidebar_collapsed: false,
            side_panel_width: DEFAULT_SIDE_PANEL_WIDTH,
            sleep_enabled: true,
            sleep_after_secs: DEFAULT_SLEEP_AFTER_SECS,
            blocker_enabled: true,
            blocker_allow: Vec::new(),
            search_engine: None,
            automatic_tab_grouping: true,
            efficiency_mode: EfficiencyMode::Auto,
            tracking_prevention: TrackingLevel::Balanced,
            theme: Theme::Helium,
            theme_accent: None,
            toolbar_buttons: default_toolbar_buttons(),
            toolbar_tabs: crate::toolbar_prefs::Bar::Tabs.default_list(),
            toolbar_panel: crate::toolbar_prefs::Bar::Panel.default_list(),
            feeds_detect: true,
            feeds_refresh_minutes: crate::feeds::DEFAULT_REFRESH_MINUTES,
            centred_address_bar: true,
            minimal_url: true,
            show_load_progress: true,
            frameless: false,
            rounded_content_frame: true,
            show_home_button: true,
            bookmarks_bar: BookmarksBar::Never,
            bookmarks_bar_tab_groups: false,
            side_panel_position: SidePanelPosition::Left,
            tab_preview_images: true,
            tab_preview_memory: true,
            font_size: FontSize::Medium,
            ui_font: String::new(),
            default_zoom: crate::site_prefs::ZOOM_DEFAULT,
            show_zoom_indicator: true,
            new_tab_beside_active: true,
            ctrl_tab_mru: true,
            shift_right_click_context: false,
            ctrl_shift_c_copy_link: true,
            ctrl_s_vertical_tabs: false,
            pin_new_tab_groups: false,
            split_edge_drop: true,
            block_youtube_video_ads: true,
            download_dir: String::new(),
            ask_download_location: false,
            translate_offer: true,
            translate_auto: false,
            translate_target: String::new(),
            blocker_level: BlockerLevel::Ads,
            cookie_notices: false,
            history_retention: HistoryRetention::Months3,
            website_appearance: WebsiteAppearance::Auto,
            force_dark_websites: false,
            private_theme: PrivateTheme::Follow,
            auto_hide: crate::auto_hide::AutoHideSettings::default(),
            start: crate::start_page::StartSettings::default(),
            address: crate::address_settings::AddressSettings::default(),
            page_tools: crate::page_tools::PageTools::default(),
            tabs: crate::tab_options::TabOptions::default(),
            // --- layout (thread T6) ---
            tab_bar_visible: true,
            address_bar_position: crate::layout_prefs::BarPosition::Top,
            address_bar_visible: true,
            show_title_bar: false,
            status_bar: crate::layout_prefs::StatusBar::Overlay,
            ui_density: crate::layout_prefs::UiDensity::Regular,
            compact_menus: false,
            ui_zoom: crate::layout_prefs::DEFAULT_UI_ZOOM,
            layout_saved: String::new(),
            // --- end layout ---
            themes: crate::theme_custom::ThemeOptions::default(),
            window: crate::window_prefs::WindowPrefs::default(), // --- window and panels (thread T9) ---
        }
    }
}

impl Settings {
    /// The sidebar width used for layout: the collapsed strip width, or the saved width.
    pub fn effective_sidebar_width(&self) -> u16 {
        if self.sidebar_collapsed {
            COLLAPSED_SIDEBAR_WIDTH
        } else {
            self.sidebar_width
        }
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

impl Settings {
    pub fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "tab_layout" => {
                    settings.tab_layout = TabLayout::from_key(value).unwrap_or(settings.tab_layout);
                }
                "sidebar_width" => {
                    if let Ok(width) = value.parse::<u16>() {
                        settings.sidebar_width = width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
                    }
                }
                "sidebar_collapsed" => {
                    if let Some(on) = parse_bool(value) {
                        settings.sidebar_collapsed = on;
                    }
                }
                "side_panel_width" => {
                    if let Ok(width) = value.parse::<u16>() {
                        settings.side_panel_width = width.clamp(MIN_SIDE_PANEL_WIDTH, MAX_SIDE_PANEL_WIDTH);
                    }
                }
                "sleep_enabled" => {
                    if let Some(on) = parse_bool(value) {
                        settings.sleep_enabled = on;
                    }
                }
                "blocker_enabled" => {
                    if let Some(on) = parse_bool(value) {
                        settings.blocker_enabled = on;
                    }
                }
                "blocker_allow" => {
                    settings.blocker_allow = value
                        .split(',')
                        .map(str::trim)
                        .filter(|site| !site.is_empty())
                        // Same character rule as `parse_blocker_allow`: one hand-edited bad entry
                        // is dropped rather than making the whole settings snapshot unsavable.
                        .filter(|site| !site.chars().any(|c| c.is_whitespace() || c.is_control()))
                        .take(MAX_BLOCKER_ALLOW)
                        .scan(0usize, |used, site| {
                            // Whole sites only, while the joined list stays within its byte cap.
                            *used += site.len() + usize::from(*used > 0);
                            (*used <= MAX_BLOCKER_ALLOW_BYTES).then(|| site.to_owned())
                        })
                        .collect();
                }
                "sleep_after_secs" => {
                    if let Ok(secs) = value.parse::<u64>() {
                        settings.sleep_after_secs =
                            secs.clamp(MIN_SLEEP_AFTER_SECS, MAX_SLEEP_AFTER_SECS);
                    }
                }
                "search_engine" => {
                    if let Some(engine) = SearchEngine::parse(value) {
                        settings.search_engine = Some(engine);
                    } else if settings.search_engine.is_none() {
                        settings.search_engine = Some(SearchEngine::Optimised);
                    }
                }
                "automatic_tab_grouping" => {
                    if let Some(on) = parse_bool(value) {
                        settings.automatic_tab_grouping = on;
                    }
                }
                "efficiency_mode" => {
                    if let Some(mode) = EfficiencyMode::parse(value) {
                        settings.efficiency_mode = mode;
                    }
                }
                "tracking_prevention" => {
                    if let Some(level) = TrackingLevel::parse(value) {
                        settings.tracking_prevention = level;
                    }
                }
                "theme" => {
                    if let Some(theme) = Theme::parse(value) {
                        settings.theme = theme;
                    }
                }
                "theme_accent" => {
                    if let Some(accent) = parse_theme_accent(value) {
                        settings.theme_accent = accent;
                    }
                }
                "toolbar_buttons" => {
                    // A retired or hand-edited `cmd:` button is dropped; the rest of the list stays.
                    if let Some(buttons) = crate::toolbar_prefs::parse_stored_list(crate::toolbar_prefs::Bar::Address, value) {
                        settings.toolbar_buttons = buttons;
                    }
                }
                // --- toolbars and feeds (thread T13) --- a bad value keeps the previous one.
                "toolbar_tabs" | "toolbar_panel" => {
                    let _ = crate::toolbar_prefs::apply_stored_list(&mut settings, key, value);
                }
                "feeds_detect" | "feeds_refresh_minutes" => {
                    let mut probe = settings.clone();
                    if crate::toolbar_prefs::apply_assignment(&mut probe, &format!("{key}={value}")) {
                        settings = probe;
                    }
                }
                // --- end toolbars and feeds ---
                "centred_address_bar" => {
                    if let Some(on) = parse_bool(value) {
                        settings.centred_address_bar = on;
                    }
                }
                "minimal_url" => {
                    if let Some(on) = parse_bool(value) {
                        settings.minimal_url = on;
                    }
                }
                "show_load_progress" => {
                    if let Some(on) = parse_bool(value) {
                        settings.show_load_progress = on;
                    }
                }
                "frameless" => {
                    if let Some(on) = parse_bool(value) {
                        settings.frameless = on;
                    }
                }
                "rounded_content_frame" => {
                    if let Some(on) = parse_bool(value) {
                        settings.rounded_content_frame = on;
                    }
                }
                "show_home_button" => {
                    if let Some(on) = parse_bool(value) {
                        settings.show_home_button = on;
                    }
                }
                "bookmarks_bar" => {
                    if let Some(mode) = BookmarksBar::parse(value) {
                        settings.bookmarks_bar = mode;
                    }
                }
                "bookmarks_bar_tab_groups" => {
                    if let Some(on) = parse_bool(value) {
                        settings.bookmarks_bar_tab_groups = on;
                    }
                }
                "side_panel_position" => {
                    if let Some(position) = SidePanelPosition::parse(value) {
                        settings.side_panel_position = position;
                    }
                }
                "tab_preview_images" => {
                    if let Some(on) = parse_bool(value) {
                        settings.tab_preview_images = on;
                    }
                }
                "tab_preview_memory" => {
                    if let Some(on) = parse_bool(value) {
                        settings.tab_preview_memory = on;
                    }
                }
                "font_size" => {
                    if let Some(size) = FontSize::parse(value) {
                        settings.font_size = size;
                    }
                }
                "ui_font" => {
                    if let Some(font) = parse_ui_font(value) {
                        settings.ui_font = font;
                    }
                }
                "default_zoom" => {
                    if let Ok(zoom) = value.parse::<u16>() {
                        settings.default_zoom = zoom.clamp(MIN_DEFAULT_ZOOM, MAX_DEFAULT_ZOOM);
                    }
                }
                "show_zoom_indicator" => {
                    if let Some(on) = parse_bool(value) {
                        settings.show_zoom_indicator = on;
                    }
                }
                "new_tab_beside_active" => {
                    if let Some(on) = parse_bool(value) {
                        settings.new_tab_beside_active = on;
                    }
                }
                "ctrl_tab_mru" => {
                    if let Some(on) = parse_bool(value) {
                        settings.ctrl_tab_mru = on;
                    }
                }
                "shift_right_click_context" => {
                    if let Some(on) = parse_bool(value) {
                        settings.shift_right_click_context = on;
                    }
                }
                "ctrl_shift_c_copy_link" => {
                    if let Some(on) = parse_bool(value) {
                        settings.ctrl_shift_c_copy_link = on;
                    }
                }
                "ctrl_s_vertical_tabs" => {
                    if let Some(on) = parse_bool(value) {
                        settings.ctrl_s_vertical_tabs = on;
                    }
                }
                "pin_new_tab_groups" => {
                    if let Some(on) = parse_bool(value) {
                        settings.pin_new_tab_groups = on;
                    }
                }
                "split_edge_drop" => {
                    if let Some(on) = parse_bool(value) {
                        settings.split_edge_drop = on;
                    }
                }
                "block_youtube_video_ads" => {
                    if let Some(on) = parse_bool(value) {
                        settings.block_youtube_video_ads = on;
                    }
                }
                "download_dir" => {
                    if let Some(dir) = parse_download_dir(value) {
                        settings.download_dir = dir;
                    }
                }
                "ask_download_location" => {
                    if let Some(on) = parse_bool(value) {
                        settings.ask_download_location = on;
                    }
                }
                "translate_offer" => {
                    if let Some(on) = parse_bool(value) {
                        settings.translate_offer = on;
                    }
                }
                "translate_auto" => {
                    if let Some(on) = parse_bool(value) {
                        settings.translate_auto = on;
                    }
                }
                "translate_target" => {
                    if let Some(target) = parse_translate_target(value) {
                        settings.translate_target = target;
                    }
                }
                // --- tab options ---
                key if key.starts_with("tabs_") => {
                    crate::tab_options::read_file_line(&mut settings.tabs, key, value);
                }
                // --- end tab options ---
                // --- theme options ---
                key if crate::theme_custom::KEYS.contains(&key) => {
                    crate::theme_custom::read_file_line(&mut settings.themes, key, value);
                }
                // --- end theme options ---
                // --- privacy, history and website appearance (thread T3) ---
                "blocker_level" => {
                    if let Some(level) = BlockerLevel::parse(value) {
                        settings.blocker_level = level;
                    }
                }
                "cookie_notices" => {
                    if let Some(on) = parse_bool(value) {
                        settings.cookie_notices = on;
                    }
                }
                "history_retention" => {
                    if let Some(retention) = HistoryRetention::parse(value) {
                        settings.history_retention = retention;
                    }
                }
                "website_appearance" => {
                    if let Some(appearance) = WebsiteAppearance::parse(value) {
                        settings.website_appearance = appearance;
                    }
                }
                "force_dark_websites" => {
                    if let Some(on) = parse_bool(value) {
                        settings.force_dark_websites = on;
                    }
                }
                "private_theme" => {
                    if let Some(theme) = PrivateTheme::parse(value) {
                        settings.private_theme = theme;
                    }
                }
                // --- end privacy, history and website appearance ---
                // --- layout (thread T6) ---
                "tab_bar_visible" => {
                    if let Some(on) = parse_bool(value) {
                        settings.tab_bar_visible = on;
                    }
                }
                "address_bar_position" => {
                    if let Some(position) = crate::layout_prefs::BarPosition::parse(value) {
                        settings.address_bar_position = position;
                    }
                }
                "address_bar_visible" => {
                    if let Some(on) = parse_bool(value) {
                        settings.address_bar_visible = on;
                    }
                }
                "show_title_bar" => {
                    if let Some(on) = parse_bool(value) {
                        settings.show_title_bar = on;
                    }
                }
                "status_bar" => {
                    if let Some(status) = crate::layout_prefs::StatusBar::parse(value) {
                        settings.status_bar = status;
                    }
                }
                "ui_density" => {
                    if let Some(density) = crate::layout_prefs::UiDensity::parse(value) {
                        settings.ui_density = density;
                    }
                }
                "compact_menus" => {
                    if let Some(on) = parse_bool(value) {
                        settings.compact_menus = on;
                    }
                }
                "ui_zoom" => {
                    if let Ok(percent) = value.parse::<u16>() {
                        settings.ui_zoom = percent.clamp(crate::layout_prefs::MIN_UI_ZOOM, crate::layout_prefs::MAX_UI_ZOOM);
                    }
                }
                "layout_saved" => {
                    if crate::layout_prefs::is_layout_saved(value) {
                        settings.layout_saved = value.to_owned();
                    }
                }
                // --- end layout ---
                // --- start page --- (`start_*`, `homepage`, `startup`, `startup_pages`)
                // Address bar, search and bookmark options (src/address_settings.rs) take what the start page does not.
                _ => {
                    if !settings.auto_hide.parse_key(key, value) /* --- auto-hide (T11) --- */
                        && !settings.start.parse_key(key, value)
                        && !settings.window.parse_key(key, value) // --- window and panels (thread T9) ---
                    {
                        if crate::page_tools::PageTools::owns(key) {
                            settings.page_tools.parse_line(key, value); // --- page tools (T7) ---
                        } else {
                            settings.address.parse_line(key, value);
                        }
                    }
                }
                // --- end start page ---
            }
        }
        // --- fix group C: checks across keys, made once the whole file is read ---
        settings.themes.check_schedule();
        settings.address.dedupe_scope_words();
        // --- end fix group C ---
        settings
    }

    /// Missing, unreadable, oversized or non-UTF-8 files give the defaults.
    pub fn load(path: &Path) -> Self {
        let Ok(file) = File::open(path) else {
            return Self::default();
        };
        let mut bytes = Vec::new();
        if file
            .take(MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() > MAX_FILE_BYTES
        {
            return Self::default();
        }
        String::from_utf8(bytes)
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }
}

/// Validates the delay typed into the settings window. Out-of-range input is refused, not clamped,
/// so the user sees the value they entered is not the one that will be saved.
pub fn parse_sleep_after_secs(text: &str) -> Result<u64, String> {
    let secs = text
        .trim()
        .parse::<u64>()
        .map_err(|_| "Enter the delay as whole seconds, for example 300.".to_owned())?;
    if !(MIN_SLEEP_AFTER_SECS..=MAX_SLEEP_AFTER_SECS).contains(&secs) {
        return Err(format!(
            "Enter a delay from {MIN_SLEEP_AFTER_SECS} to {MAX_SLEEP_AFTER_SECS} seconds."
        ));
    }
    Ok(secs)
}

/// Validates the default zoom percent typed into the settings window. Out-of-range input is
/// refused, not clamped, matching `parse_sleep_after_secs`.
pub fn parse_default_zoom(text: &str) -> Result<u16, String> {
    let percent = text
        .trim()
        .parse::<u16>()
        .map_err(|_| "Enter the zoom as a whole percentage, for example 100.".to_owned())?;
    if !(MIN_DEFAULT_ZOOM..=MAX_DEFAULT_ZOOM).contains(&percent) {
        return Err(format!("Enter a zoom from {MIN_DEFAULT_ZOOM} to {MAX_DEFAULT_ZOOM} percent."));
    }
    Ok(percent)
}

/// Validates the comma-separated allow list typed into the settings window. Entries are trimmed
/// and empty ones dropped, matching `parse`, so a saved list reads back unchanged.
pub fn parse_blocker_allow(text: &str) -> Result<Vec<String>, String> {
    let sites: Vec<String> = text
        .split(',')
        .map(str::trim)
        .filter(|site| !site.is_empty())
        .map(str::to_owned)
        .collect();
    if sites.len() > MAX_BLOCKER_ALLOW {
        return Err(format!("Allow at most {MAX_BLOCKER_ALLOW} sites."));
    }
    if sites.iter().map(|site| site.len() + 1).sum::<usize>().saturating_sub(1) > MAX_BLOCKER_ALLOW_BYTES {
        return Err(format!("The allowed sites can use at most {MAX_BLOCKER_ALLOW_BYTES} characters in all."));
    }
    if let Some(site) = sites
        .iter()
        .find(|site| site.chars().any(|c| c.is_whitespace() || c.is_control()))
    {
        return Err(format!(
            "\"{site}\" has a space in it. Separate sites with commas only."
        ));
    }
    Ok(sites)
}

fn bool_text(on: bool) -> &'static str {
    if on { "true" } else { "false" }
}

/// Returns `existing` with the setting lines replaced by `settings`. Comments, unknown keys and the
/// file's line endings are kept. A setting missing from the file is appended, and a repeated
/// setting keeps only its first line, since `parse` reads the last value anyway. An unset search
/// engine removes its line, so the file falls back to `search-provider.txt` or the default.
pub fn render_updated(existing: &str, settings: &Settings) -> String {
    let newline = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let values: [(&str, Option<String>); _] = [
        ("tab_layout", Some(settings.tab_layout.key().to_owned())),
        ("sidebar_width", Some(settings.sidebar_width.to_string())),
        (
            "sidebar_collapsed",
            Some(bool_text(settings.sidebar_collapsed).to_owned()),
        ),
        ("side_panel_width", Some(settings.side_panel_width.to_string())),
        (
            "sleep_enabled",
            Some(bool_text(settings.sleep_enabled).to_owned()),
        ),
        (
            "sleep_after_secs",
            Some(settings.sleep_after_secs.to_string()),
        ),
        (
            "blocker_enabled",
            Some(bool_text(settings.blocker_enabled).to_owned()),
        ),
        ("blocker_allow", Some(settings.blocker_allow.join(","))),
        (
            "search_engine",
            settings.search_engine.map(|engine| engine.key().to_owned()),
        ),
        (
            "automatic_tab_grouping",
            Some(bool_text(settings.automatic_tab_grouping).to_owned()),
        ),
        (
            "efficiency_mode",
            Some(settings.efficiency_mode.key().to_owned()),
        ),
        (
            "tracking_prevention",
            Some(settings.tracking_prevention.key().to_owned()),
        ),
        ("theme", Some(settings.theme.key().to_owned())),
        ("theme_accent", Some(settings.theme_accent.clone().unwrap_or_default())),
        ("toolbar_buttons", Some(settings.toolbar_buttons.join(","))),
        ("toolbar_tabs", Some(settings.toolbar_tabs.join(","))),
        ("toolbar_panel", Some(settings.toolbar_panel.join(","))),
        ("feeds_detect", Some(bool_text(settings.feeds_detect).to_owned())),
        ("feeds_refresh_minutes", Some(settings.feeds_refresh_minutes.to_string())),
        ("centred_address_bar", Some(bool_text(settings.centred_address_bar).to_owned())),
        ("minimal_url", Some(bool_text(settings.minimal_url).to_owned())),
        ("show_load_progress", Some(bool_text(settings.show_load_progress).to_owned())),
        ("frameless", Some(bool_text(settings.frameless).to_owned())),
        ("rounded_content_frame", Some(bool_text(settings.rounded_content_frame).to_owned())),
        ("show_home_button", Some(bool_text(settings.show_home_button).to_owned())),
        ("bookmarks_bar", Some(settings.bookmarks_bar.key().to_owned())),
        ("bookmarks_bar_tab_groups", Some(bool_text(settings.bookmarks_bar_tab_groups).to_owned())),
        ("side_panel_position", Some(settings.side_panel_position.key().to_owned())),
        ("tab_preview_images", Some(bool_text(settings.tab_preview_images).to_owned())),
        ("tab_preview_memory", Some(bool_text(settings.tab_preview_memory).to_owned())),
        ("font_size", Some(settings.font_size.key().to_owned())),
        ("ui_font", Some(settings.ui_font.clone())),
        ("default_zoom", Some(settings.default_zoom.to_string())),
        ("show_zoom_indicator", Some(bool_text(settings.show_zoom_indicator).to_owned())),
        ("new_tab_beside_active", Some(bool_text(settings.new_tab_beside_active).to_owned())),
        ("ctrl_tab_mru", Some(bool_text(settings.ctrl_tab_mru).to_owned())),
        ("shift_right_click_context", Some(bool_text(settings.shift_right_click_context).to_owned())),
        ("ctrl_shift_c_copy_link", Some(bool_text(settings.ctrl_shift_c_copy_link).to_owned())),
        ("ctrl_s_vertical_tabs", Some(bool_text(settings.ctrl_s_vertical_tabs).to_owned())),
        ("pin_new_tab_groups", Some(bool_text(settings.pin_new_tab_groups).to_owned())),
        ("split_edge_drop", Some(bool_text(settings.split_edge_drop).to_owned())),
        ("block_youtube_video_ads", Some(bool_text(settings.block_youtube_video_ads).to_owned())),
        ("download_dir", Some(settings.download_dir.clone())),
        ("ask_download_location", Some(bool_text(settings.ask_download_location).to_owned())),
        ("translate_offer", Some(bool_text(settings.translate_offer).to_owned())),
        ("translate_auto", Some(bool_text(settings.translate_auto).to_owned())),
        ("translate_target", Some(settings.translate_target.clone())),
        // --- privacy, history and website appearance (thread T3) ---
        ("blocker_level", Some(settings.blocker_level.key().to_owned())),
        ("cookie_notices", Some(bool_text(settings.cookie_notices).to_owned())),
        ("history_retention", Some(settings.history_retention.key().to_owned())),
        ("website_appearance", Some(settings.website_appearance.key().to_owned())),
        ("force_dark_websites", Some(bool_text(settings.force_dark_websites).to_owned())),
        ("private_theme", Some(settings.private_theme.key().to_owned())),
        // --- end privacy, history and website appearance ---
        // --- layout (thread T6) ---
        ("tab_bar_visible", Some(bool_text(settings.tab_bar_visible).to_owned())),
        ("address_bar_position", Some(settings.address_bar_position.key().to_owned())),
        ("address_bar_visible", Some(bool_text(settings.address_bar_visible).to_owned())),
        ("show_title_bar", Some(bool_text(settings.show_title_bar).to_owned())),
        ("status_bar", Some(settings.status_bar.key().to_owned())),
        ("ui_density", Some(settings.ui_density.key().to_owned())),
        ("compact_menus", Some(bool_text(settings.compact_menus).to_owned())),
        ("ui_zoom", Some(settings.ui_zoom.to_string())),
        ("layout_saved", Some(settings.layout_saved.clone())),
        // --- end layout ---
    ];
    // --- start page ---
    let mut values = Vec::from(values);
    values.extend(settings.auto_hide.pairs().into_iter().map(|(key, value)| (key, Some(value)))); // --- auto-hide (T11) ---
    values.extend(settings.start.pairs().into_iter().map(|(key, value)| (key, Some(value))));
    // --- end start page ---
    let mut written = vec![false; values.len()];
    let mut out = String::with_capacity(existing.len() + 128);
    for line in existing.lines() {
        let trimmed = line.trim();
        let slot = if trimmed.starts_with('#') {
            None
        } else {
            trimmed
                .split_once('=')
                .and_then(|(key, _)| values.iter().position(|(name, _)| *name == key.trim()))
        };
        match slot {
            Some(index) if written[index] => {}
            Some(index) => {
                written[index] = true;
                if let Some(value) = &values[index].1 {
                    out.push_str(&format!("{}={value}{newline}", values[index].0));
                }
            }
            None => {
                out.push_str(line);
                out.push_str(newline);
            }
        }
    }
    for (index, (name, value)) in values.iter().enumerate() {
        if let (false, Some(value)) = (written[index], value) {
            out.push_str(&format!("{name}={value}{newline}"));
        }
    }
    // --- tab options ---
    let out = crate::tab_options::render_into(&out, &settings.tabs, newline);
    // --- end tab options ---
    // --- theme options ---
    let out = crate::theme_custom::render_into(&out, &settings.themes, newline);
    // --- end theme options ---
    // --- window and panels (thread T9) ---
    let out = crate::window_prefs::render_into(&out, &settings.window, newline);
    // --- end window and panels ---
    // Address bar, search and bookmark options (src/address_settings.rs).
    let out = settings.address.render_into(&out, newline);
    // Page tools (src/page_tools.rs).
    settings.page_tools.render_into(&out, newline)
}

// --- wave-1 fixes: key-level merge for saves from memory ---
/// The `key=value` lines `settings` renders to, by key. A key with no line (an unset search
/// engine) is absent.
fn rendered_keys(settings: &Settings) -> std::collections::BTreeMap<String, String> {
    render_updated("", settings)
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_owned(), value.to_owned()))
        .collect()
}

/// A three-way merge for a save from memory: each key the running app changed since `base` (what
/// memory last agreed with the file) takes `memory`'s value; every other key keeps `file`'s, so a
/// value the settings page wrote straight to the file is never put back to an older one.
pub fn merge_changed(base: &Settings, memory: &Settings, file: &Settings) -> Settings {
    let (base, memory, file) = (rendered_keys(base), rendered_keys(memory), rendered_keys(file));
    let keys: std::collections::BTreeSet<&String> = base.keys().chain(memory.keys()).chain(file.keys()).collect();
    let mut text = String::new();
    for key in keys {
        let mine = memory.get(key);
        let pick = if mine != base.get(key) { mine } else { file.get(key) };
        if let Some(value) = pick {
            text.push_str(key);
            text.push('=');
            text.push_str(value);
            text.push('\n');
        }
    }
    Settings::parse(&text)
}

/// What memory agrees with the file on after `file` was just written: each key where `memory` holds
/// `file`'s value takes it; every other key keeps `synced`'s, so it still counts as changed by the app.
pub fn mark_agreed(synced: &Settings, memory: &Settings, file: &Settings) -> Settings {
    let (synced, memory, file) = (rendered_keys(synced), rendered_keys(memory), rendered_keys(file));
    let keys: std::collections::BTreeSet<&String> = synced.keys().chain(memory.keys()).chain(file.keys()).collect();
    let mut text = String::new();
    for key in keys {
        let pick = if memory.get(key) == file.get(key) { file.get(key) } else { synced.get(key) };
        if let Some(value) = pick {
            text.push_str(key);
            text.push('=');
            text.push_str(value);
            text.push('\n');
        }
    }
    Settings::parse(&text)
}

/// Reset settings: every option in the file goes back to its default except how long history is
/// kept and the three Keyboard-section options, which the page's description promises it leaves
/// alone (history itself and the shortcuts are not in this file).
pub fn reset_defaults(current: &Settings) -> Settings {
    Settings {
        history_retention: current.history_retention,
        // --- fix group C: the Keyboard section is not reset ---
        ctrl_shift_c_copy_link: current.ctrl_shift_c_copy_link,
        shift_right_click_context: current.shift_right_click_context,
        ctrl_s_vertical_tabs: current.ctrl_s_vertical_tabs,
        // --- end fix group C ---
        ..Settings::default()
    }
}
// --- end wave-1 fixes ---

/// Reads the file as text, refusing anything over `MAX_FILE_BYTES` or not valid UTF-8. Unlike
/// `load`, the reason is returned, so the settings window can say what is wrong with the file.
fn read_text(path: &Path) -> io::Result<String> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_FILE_BYTES as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("settings file is larger than the {} KiB limit", MAX_FILE_BYTES / 1024),
        ));
    }
    let bytes = std::fs::read(path)?;
    String::from_utf8(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "settings file is not UTF-8 text",
        )
    })
}

/// Strict counterpart of `load` for the settings window: a missing file gives the defaults, while
/// an unreadable, oversized or non-UTF-8 file is an error the window can show.
pub fn read_strict(path: &Path) -> io::Result<Settings> {
    match read_text(path) {
        Ok(text) => Ok(Settings::parse(&text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(error),
    }
}

/// Writes `settings` into the file at `path` through `render_updated`. A missing file starts empty.
/// The text goes to a temporary file that then replaces the original, so an interrupted save never
/// leaves a partial settings file. A file too large to read in full is refused, not overwritten.
pub fn save(path: &Path, settings: &Settings) -> io::Result<()> {
    let existing = match read_text(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let text = render_updated(&existing, settings);
    // `load` ignores files over the limit, so writing one would silently reset every setting.
    if text.len() > MAX_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "settings would exceed the size limit",
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("txt.{}.tmp", std::process::id()));
    let written = (|| {
        let mut file = File::create(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()
    })();
    if let Err(error) = written.and_then(|()| std::fs::rename(&temporary, path)) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    SAVES.fetch_add(1, std::sync::atomic::Ordering::Release);
    Ok(())
}

/// Counts the settings files `save` has written in this process.
static SAVES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Moves on whenever `save` writes a settings file, so a value cached from one (the side panel's
/// shared width) is read again after a Save or Reset on the settings page, even when the file's
/// time and length alone would not show the change (two saves inside one clock tick).
pub fn save_generation() -> u64 {
    SAVES.load(std::sync::atomic::Ordering::Acquire)
}

/// Creates `path` from `TEMPLATE` when it does not exist yet. An existing file is left alone.
pub fn create_template_if_missing(path: &Path) -> io::Result<()> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file.write_all(TEMPLATE.as_bytes()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_apply_to_empty_and_unknown_input() {
        assert_eq!(Settings::parse(""), Settings::default());
        assert_eq!(
            Settings::parse("# c\nnonsense\nfoo=bar\n"),
            Settings::default()
        );
        assert!(Settings::default().sleep_enabled);
        assert_eq!(Settings::default().sleep_after_secs, 300);
    }

    #[test]
    fn parses_values_with_whitespace_comments_and_case() {
        let s = Settings::parse("# hi\n  sleep_enabled = FALSE \r\nsleep_after_secs= 120\r\n");
        assert_eq!(
            s,
            Settings {
                sleep_enabled: false,
                sleep_after_secs: 120,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn parses_layout_and_clamps_sidebar_width() {
        let settings = Settings::parse("tab_layout=VERTICAL\nsidebar_width=400\n");
        assert_eq!(settings.tab_layout, TabLayout::Vertical);
        assert_eq!(settings.sidebar_width, MAX_SIDEBAR_WIDTH);
        assert_eq!(
            Settings::parse("tab_layout=sideways").tab_layout,
            TabLayout::ToolbarTop
        );
        assert_eq!(
            Settings::parse("sidebar_width=30").sidebar_width,
            MIN_SIDEBAR_WIDTH
        );
    }

    #[test]
    fn parses_every_browser_layout_key() {
        for (key, layout) in [
            ("horizontal", TabLayout::ToolbarTop),
            ("tabs-top", TabLayout::TabsTop),
            ("single-row", TabLayout::SingleRow),
            ("vertical", TabLayout::Vertical),
        ] {
            assert_eq!(layout.key(), key);
            assert_eq!(Settings::parse(&format!("tab_layout={key}\n")).tab_layout, layout, "{key}");
            assert_eq!(Settings::parse(&format!("tab_layout={}\n", key.to_ascii_uppercase())).tab_layout, layout, "{key}");
        }
        // The default is the toolbar-above-tabs layout, and a file without the key keeps it.
        assert_eq!(Settings::default().tab_layout, TabLayout::ToolbarTop);
        assert_eq!(Settings::parse("sleep_enabled=true\n").tab_layout, TabLayout::ToolbarTop);
        // An unknown value keeps whatever the earlier lines set rather than resetting to the default.
        assert_eq!(Settings::parse("tab_layout=single-row\ntab_layout=sideways\n").tab_layout, TabLayout::SingleRow);
        assert_eq!(TabLayout::from_key("toolbar-top"), None);
    }

    #[test]
    fn every_browser_layout_survives_a_save_round_trip() {
        for layout in TabLayout::ALL {
            let settings = Settings { tab_layout: layout, ..sample() };
            let text = render_updated("", &settings);
            assert!(text.contains(&format!("tab_layout={}\n", layout.key())), "{layout:?}");
            assert_eq!(Settings::parse(&text).tab_layout, layout, "{layout:?}");
            // Rewriting an existing file with another layout replaces the line in place.
            let rewritten = render_updated(&text, &Settings { tab_layout: TabLayout::TabsTop, ..settings });
            assert_eq!(rewritten.matches("tab_layout").count(), 1);
            assert_eq!(Settings::parse(&rewritten).tab_layout, TabLayout::TabsTop);
        }
    }

    #[test]
    fn invalid_values_keep_defaults_and_do_not_affect_other_keys() {
        let s = Settings::parse("sleep_enabled=maybe\nsleep_after_secs=soon\n");
        assert_eq!(s, Settings::default());
        let s = Settings::parse("sleep_after_secs=-5\nsleep_enabled=0");
        assert_eq!(s.sleep_after_secs, 300);
        assert!(!s.sleep_enabled);
    }

    #[test]
    fn sleep_delay_is_clamped() {
        assert_eq!(Settings::parse("sleep_after_secs=1").sleep_after_secs, 30);
        assert_eq!(
            Settings::parse("sleep_after_secs=99999999999").sleep_after_secs,
            MAX_SLEEP_AFTER_SECS
        );
    }

    #[test]
    fn missing_and_oversized_files_give_defaults() {
        let dir = std::env::temp_dir().join(format!("bowser-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(Settings::load(&dir.join("nope.txt")), Settings::default());
        let big = dir.join("big.txt");
        std::fs::write(
            &big,
            format!("sleep_enabled=false\n{}", "#".repeat(MAX_FILE_BYTES)),
        )
        .unwrap();
        assert_eq!(Settings::load(&big), Settings::default());
        let ok = dir.join("ok.txt");
        std::fs::write(&ok, "sleep_enabled=false\n").unwrap();
        assert!(!Settings::load(&ok).sleep_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn blocker_settings_parse_and_default_on() {
        assert!(Settings::default().blocker_enabled);
        let s = Settings::parse("blocker_enabled=off\nblocker_allow= a.com , ,b.org\n");
        assert!(!s.blocker_enabled);
        assert_eq!(
            s.blocker_allow,
            vec!["a.com".to_owned(), "b.org".to_owned()]
        );
        assert!(Settings::parse("blocker_enabled=maybe").blocker_enabled);
    }

    #[test]
    fn a_hand_edited_bad_blocker_allow_entry_is_dropped_so_the_snapshot_stays_valid() {
        let s = Settings::parse("blocker_allow=a.com,a b.com,b.org\n");
        assert_eq!(s.blocker_allow, vec!["a.com".to_owned(), "b.org".to_owned()]);
        assert_eq!(parse_blocker_allow(&s.blocker_allow.join(",")), Ok(s.blocker_allow.clone()));
    }

    #[test]
    fn side_panel_width_is_additive_and_clamped() {
        assert_eq!(Settings::parse("side_panel_width=200").side_panel_width, MIN_SIDE_PANEL_WIDTH);
        assert_eq!(Settings::parse("side_panel_width=900").side_panel_width, MAX_SIDE_PANEL_WIDTH);
        let mut settings = Settings::default();
        settings.side_panel_width = 410;
        let saved = render_updated("# keep\nunknown=value\n", &settings);
        assert!(saved.contains("side_panel_width=410\n"));
        assert!(saved.contains("unknown=value\n"));
        assert_eq!(Settings::parse(&saved).side_panel_width, 410);
    }

    #[test]
    fn sidebar_collapsed_round_trips_and_keeps_the_expanded_width() {
        assert!(!Settings::default().sidebar_collapsed);
        assert!(Settings::parse("sidebar_collapsed=true").sidebar_collapsed);
        assert!(!Settings::parse("sidebar_collapsed=banana").sidebar_collapsed);
        let mut settings = Settings::parse("sidebar_width=300
");
        settings.sidebar_collapsed = true;
        let saved = render_updated("sidebar_width=300
", &settings);
        assert!(saved.contains("sidebar_collapsed=true
"));
        let back = Settings::parse(&saved);
        assert!(back.sidebar_collapsed);
        assert_eq!(back.sidebar_width, 300);
    }

    fn sample() -> Settings {
        Settings {
            tab_layout: TabLayout::Vertical,
            sidebar_width: 300,
            // --- layout (thread T6) ---
            tab_bar_visible: true,
            address_bar_position: crate::layout_prefs::BarPosition::Bottom,
            address_bar_visible: true,
            show_title_bar: true,
            status_bar: crate::layout_prefs::StatusBar::Hidden,
            ui_density: crate::layout_prefs::UiDensity::Compact,
            compact_menus: true,
            ui_zoom: 125,
            layout_saved: String::new(),
            // --- end layout ---
            window: crate::window_prefs::WindowPrefs::default(), // --- window and panels (thread T9) ---
            sidebar_collapsed: false,
            side_panel_width: 340,
            sleep_enabled: false,
            sleep_after_secs: 900,
            blocker_enabled: false,
            blocker_allow: vec!["a.com".to_owned(), "b.org".to_owned()],
            automatic_tab_grouping: false,
            search_engine: Some(SearchEngine::DuckDuckGo),
            efficiency_mode: EfficiencyMode::On,
            tracking_prevention: TrackingLevel::Strict,
            theme: Theme::System,
            theme_accent: Some("#1a2b3c".to_owned()),
            toolbar_buttons: vec!["back".to_owned(), "reload".to_owned()],
            toolbar_tabs: vec!["closed-tabs".to_owned(), "cmd:new-tab".to_owned()],
            toolbar_panel: vec!["feeds".to_owned()],
            feeds_detect: false,
            feeds_refresh_minutes: 180,
            centred_address_bar: false,
            minimal_url: false,
            show_load_progress: false,
            frameless: true,
            rounded_content_frame: false,
            show_home_button: false,
            bookmarks_bar: BookmarksBar::Always,
            bookmarks_bar_tab_groups: true,
            side_panel_position: SidePanelPosition::Right,
            tab_preview_images: false,
            tab_preview_memory: false,
            font_size: FontSize::Large,
            ui_font: "Consolas".to_owned(),
            default_zoom: 125,
            show_zoom_indicator: false,
            new_tab_beside_active: false,
            ctrl_tab_mru: false,
            shift_right_click_context: true,
            ctrl_shift_c_copy_link: false,
            ctrl_s_vertical_tabs: true,
            pin_new_tab_groups: true,
            split_edge_drop: false,
            block_youtube_video_ads: false,
            download_dir: r"D:\Downloads\Bowser".to_owned(),
            ask_download_location: true,
            translate_offer: false,
            translate_auto: true,
            translate_target: "fr".to_owned(),
            blocker_level: BlockerLevel::Trackers,
            cookie_notices: true,
            history_retention: HistoryRetention::Week,
            website_appearance: WebsiteAppearance::Dark,
            force_dark_websites: true,
            private_theme: PrivateTheme::Dark,
            auto_hide: crate::auto_hide::AutoHideSettings { bars: crate::auto_hide::Bars::PANEL, in_fullscreen: false },
            start: crate::start_page::StartSettings {
                group_reopen: crate::start_page::GroupReopen::First,
                titles: crate::start_page::TitleMode::Auto,
                add_button: false,
                columns: 9,
                thumb_size: 5,
                drag_reorder: false,
                homepage: "https://home.example/".to_owned(),
                startup: crate::start_page::Startup::SpecificPages,
                startup_pages: vec!["https://a.example/".to_owned(), "https://b.example/".to_owned()],
                widgets: true,
                widget_columns: 4,
                widget_height: crate::start_page::WidgetHeight::Tall,
                widget_bg: crate::start_page::WidgetBackground::Custom,
                widget_color: "#336699".to_owned(),
                widget_drag: false,
            },
            address: crate::address_settings::AddressSettings::default(),
            page_tools: crate::page_tools::PageTools::default(),
            tabs: crate::tab_options::TabOptions::default(),
            themes: crate::theme_custom::ThemeOptions::default(),
        }
    }

    #[test]
    fn theme_options_round_trip_through_the_file_and_leave_other_lines_alone() {
        let mut custom = Settings { theme: Theme::System, theme_accent: Some("#aa5500".to_owned()), ..sample() };
        for (key, value) in [
            ("theme_schedule", "time"), ("theme_look", "dark"), ("theme_light_from", "06:15"), ("theme_dark_from", "21:45"),
            ("theme_background", "#102030"), ("theme_foreground", "#eeeeee"), ("theme_highlight", "#336699"),
            ("theme_colouring", "unified"), ("theme_accent_page", "true"), ("theme_saturation", "45"), ("theme_contrast", "high"),
        ] {
            assert!(custom.themes.set(key, value), "{key}");
        }
        let written = render_updated("# kept\nunknown_key=1\n", &custom);
        assert!(written.starts_with("# kept\nunknown_key=1\n"));
        assert!(written.contains("theme_background=#102030\n") && written.contains("theme_saturation=45\n") && written.contains("theme_schedule=time\n"));
        assert_eq!(Settings::parse(&written), custom);
        assert_eq!(render_updated(&written, &Settings::parse(&written)), written, "saving again repeats nothing");
        // A profile that never touched the theme options has none of their lines.
        assert_eq!(render_updated("", &sample()).matches("theme_").count(), 1, "only the older theme_accent line");
        // A hand-written file may use the usual spellings; bad values keep the default.
        let hand = Settings::parse("theme_colouring=Unified\ntheme_accent_page=ON\ntheme_saturation=500\ntheme_background=blue\ntheme_light_from=7:00\n");
        assert_eq!(hand.themes.colouring, crate::theme_custom::Colouring::Unified);
        assert!(hand.themes.accent_from_page);
        assert_eq!((hand.themes.saturation, hand.themes.background, hand.themes.light_from), (100, None, 7 * 60));
    }

    #[test]
    fn tab_options_round_trip_through_the_file_and_leave_other_lines_alone() {
        let custom = Settings {
            tabs: crate::tab_options::TabOptions {
                close_activation: crate::tab_options::CloseActivation::Recent,
                keep_window: false,
                confirm_close: crate::tab_options::Bounded(4),
                new_tab_url: crate::tab_options::PageUrl("https://example.com/new".to_owned()),
                new_tab_page: crate::tab_options::NewTabPage::Url,
                mute_mode: crate::tab_options::MuteMode::Prioritise,
                ..Default::default()
            },
            ..sample()
        };
        let written = render_updated("# kept
unknown_key=1
", &custom);
        assert!(written.contains("# kept
unknown_key=1
"));
        assert!(written.contains("tabs_close_activation=recent
"));
        assert!(written.contains("tabs_keep_window=false
"));
        assert!(written.contains("tabs_confirm_close=4
"));
        assert!(!written.contains("tabs_scroll"), "an option still at its default is not written");
        let back = Settings::parse(&written);
        assert_eq!(back.tabs, custom.tabs);
        assert_eq!(back, custom);
        // Saving again does not repeat the lines.
        let again = render_updated(&written, &back);
        assert_eq!(again, written);
        // An option set back to its default keeps its line, now with the default value.
        let mut reset = back.clone();
        reset.tabs.keep_window = true;
        assert!(render_updated(&written, &reset).contains("tabs_keep_window=true
"));
        // A file of all-default options gets no tab lines at all.
        assert_eq!(render_updated("", &sample()).matches("tabs_").count(), 0);
        // A hand-written file may use the usual spellings, and bad values keep the default.
        let hand = Settings::parse("tabs_keep_window=OFF
tabs_close_activation=Left
tabs_confirm_close=500
tabs_new_tab_url=javascript:1
");
        assert!(!hand.tabs.keep_window);
        assert_eq!(hand.tabs.close_activation, crate::tab_options::CloseActivation::Left);
        assert_eq!(hand.tabs.confirm_close.0, 0, "out of range keeps the default");
        assert_eq!(hand.tabs.new_tab_url.0, "", "a script address is never kept");
        assert_eq!(Settings::parse("").tabs, crate::tab_options::TabOptions::default());
    }

    #[test]
    fn search_engine_defaults_to_optimised_and_reads_all_choices() {
        assert_eq!(Settings::default().search_engine, None);
        assert_eq!(Settings::parse("sleep_enabled=false").search_engine, None);
        assert_eq!(
            Settings::parse("search_engine=google").search_engine,
            Some(SearchEngine::Google)
        );
        assert_eq!(
            Settings::parse("search_engine=DuckDuckGo").search_engine,
            Some(SearchEngine::DuckDuckGo)
        );
        assert_eq!(
            Settings::parse("search_engine=optimised").search_engine,
            Some(SearchEngine::Optimised)
        );
    }

    #[test]
    fn unknown_search_engine_values_use_optimised_and_keep_other_settings() {
        for value in [
            "bing",
            "ddg",
            "",
            "google.com",
            "https://evil.example/?q={query}",
        ] {
            let s = Settings::parse(&format!("search_engine={value}\nsleep_enabled=off\n"));
            assert_eq!(s.search_engine, Some(SearchEngine::Optimised), "value: {value:?}");
            assert!(!s.sleep_enabled, "value: {value:?}");
        }
        // A later invalid line does not erase a valid choice, matching the other keys.
        assert_eq!(Settings::parse("search_engine=google\nsearch_engine=nope").search_engine, Some(SearchEngine::Google));
    }

    #[test]
    fn search_engine_round_trips_through_the_file_with_comments_kept() {
        let existing = "# my notes\r\nsearch_engine=google\r\nfoo=bar\r\n";
        let text = render_updated(existing, &sample());
        assert_eq!(text.matches("search_engine").count(), 1);
        assert!(text.contains("search_engine=duckduckgo\r\n"));
        assert!(text.starts_with("# my notes\r\n"));
        assert!(text.contains("foo=bar\r\n"));
        assert_eq!(Settings::parse(&text), sample());
    }

    #[test]
    fn legacy_explicit_google_choice_survives_unrelated_settings_save() {
        let legacy = "# saved before Optimised existed\nsearch_engine=google\nunknown=keep\n";
        let mut settings = Settings::parse(legacy);
        settings.sleep_enabled = false;
        let saved = render_updated(legacy, &settings);
        assert!(saved.contains("search_engine=google\n"), "{saved}");
        assert!(saved.contains("unknown=keep\n"), "{saved}");
        assert_eq!(Settings::parse(&saved).search_engine, Some(SearchEngine::Google));
    }

    #[test]
    fn clearing_the_engine_removes_its_line_so_the_fallback_applies_again() {
        let existing = "# keep\nsearch_engine=google\nsleep_enabled=true\n";
        let cleared = Settings {
            search_engine: None,
            ..sample()
        };
        let text = render_updated(existing, &cleared);
        assert!(!text.contains("search_engine"), "{text}");
        assert!(text.starts_with("# keep\n"));
        assert_eq!(Settings::parse(&text).search_engine, None);
    }

    #[test]
    fn saving_other_settings_leaves_an_unset_engine_and_legacy_content_unchanged() {
        let existing = "# Bowser settings.\nsleep_enabled=true\n";
        let saved = Settings {
            sleep_enabled: false,
            ..Settings::default()
        };
        let text = render_updated(existing, &saved);
        assert!(!text.contains("search_engine"), "{text}");
        assert!(text.starts_with("# Bowser settings.\n"));
        assert!(text.contains("sleep_enabled=false\n"));
        assert_eq!(Settings::parse(&text).search_engine, None);
    }

    #[test]
    fn template_lists_the_engine_setting_commented_out() {
        assert!(TEMPLATE.contains("# search_engine=optimised\r\n"));
        assert_eq!(Settings::parse(TEMPLATE).search_engine, None);
    }

    #[test]
    fn automatic_tab_grouping_defaults_on_and_reads_off() {
        assert!(Settings::default().automatic_tab_grouping);
        assert!(Settings::parse("").automatic_tab_grouping);
        assert!(!Settings::parse("automatic_tab_grouping=off").automatic_tab_grouping);
        assert!(Settings::parse("automatic_tab_grouping=maybe").automatic_tab_grouping);
        assert!(TEMPLATE.contains("# automatic_tab_grouping=true\r\n"));
    }

    #[test]
    fn automatic_tab_grouping_is_written_and_read_back() {
        let off = Settings {
            automatic_tab_grouping: false,
            ..Settings::default()
        };
        let text = render_updated("sleep_enabled=true\r\n", &off);
        assert!(text.contains("automatic_tab_grouping=false\r\n"));
        assert!(!Settings::parse(&text).automatic_tab_grouping);
    }

    #[test]
    fn efficiency_mode_defaults_to_auto_and_ignores_bad_values() {
        assert_eq!(Settings::default().efficiency_mode, EfficiencyMode::Auto);
        assert_eq!(Settings::parse("efficiency_mode=ON").efficiency_mode, EfficiencyMode::On);
        assert_eq!(Settings::parse("efficiency_mode=off").efficiency_mode, EfficiencyMode::Off);
        assert_eq!(Settings::parse("efficiency_mode=Auto").efficiency_mode, EfficiencyMode::Auto);
        for bad in ["", "true", "battery", "1"] {
            assert_eq!(
                Settings::parse(&format!("efficiency_mode={bad}")).efficiency_mode,
                EfficiencyMode::Auto,
                "value: {bad:?}"
            );
        }
        // An invalid later line does not erase a valid earlier choice.
        assert_eq!(
            Settings::parse("efficiency_mode=off\nefficiency_mode=nope").efficiency_mode,
            EfficiencyMode::Off
        );
    }

    #[test]
    fn tracking_prevention_defaults_to_balanced_and_strict_is_opt_in() {
        assert_eq!(Settings::default().tracking_prevention, TrackingLevel::Balanced);
        assert_eq!(Settings::parse("").tracking_prevention, TrackingLevel::Balanced);
        assert_eq!(Settings::parse("tracking_prevention=STRICT").tracking_prevention, TrackingLevel::Strict);
        for bad in ["", "off", "basic", "true"] {
            assert_eq!(
                Settings::parse(&format!("tracking_prevention={bad}")).tracking_prevention,
                TrackingLevel::Balanced,
                "value: {bad:?}"
            );
        }
    }

    #[test]
    fn download_settings_round_trip_and_reject_unusable_folders() {
        let defaults = Settings::default();
        assert_eq!((defaults.download_dir.as_str(), defaults.ask_download_location), ("", false));
        let parsed = Settings::parse("download_dir = D:/Stuff/Saved \r\nask_download_location=yes\n");
        assert_eq!((parsed.download_dir.as_str(), parsed.ask_download_location), ("D:/Stuff/Saved", true));
        let text = render_updated("", &parsed);
        assert!(text.contains("download_dir=D:/Stuff/Saved\n") && text.contains("ask_download_location=true\n"));
        assert_eq!(Settings::parse(&text), parsed);
        // An empty value goes back to the default folder; a bad later value keeps the good earlier one.
        assert_eq!(Settings::parse("download_dir=D:/a\ndownload_dir=\n").download_dir, "");
        assert_eq!(Settings::parse("download_dir=D:/a\ndownload_dir=relative/x\n").download_dir, "D:/a");
        let too_long = format!("D:/{}", "x".repeat(300));
        for bad in ["relative/dir", "downloads", "D:/a<b", "D:/a|b", "D:/a?b", "D:/a\tb", too_long.as_str()] {
            assert_eq!(parse_download_dir(bad), None, "{bad:?}");
        }
        assert!(TEMPLATE.contains("# download_dir=\r\n") && TEMPLATE.contains("# ask_download_location=false\r\n"));
        assert_eq!(Settings::parse(TEMPLATE), Settings::default());
    }

    #[test]
    fn efficiency_and_tracking_round_trip_and_the_template_lists_them() {
        let text = render_updated("efficiency_mode=off\n", &sample());
        assert_eq!(text.matches("efficiency_mode").count(), 1);
        assert!(text.contains("efficiency_mode=on\n"));
        assert!(text.contains("tracking_prevention=strict\n"));
        assert_eq!(Settings::parse(&text), sample());
        assert!(TEMPLATE.contains("# efficiency_mode=auto\r\n"));
        assert!(TEMPLATE.contains("# tracking_prevention=balanced\r\n"));
        assert_eq!(Settings::parse(TEMPLATE), Settings::default());
    }

    #[test]
    fn start_page_keys_load_from_the_file_and_a_bad_value_keeps_the_default() {
        let settings = Settings::parse("start_columns=4\nstart_titles=never\nhomepage=https://home.example/\nstartup=homepage\nstart_widgets=true\nstart_widget_bg=transparent\nstart_thumb_size=banana\nstartup_pages=javascript:alert(1)\n");
        assert_eq!(settings.start.columns, 4);
        assert_eq!(settings.start.titles, crate::start_page::TitleMode::Never);
        assert_eq!(settings.start.homepage, "https://home.example/");
        assert_eq!(settings.start.startup, crate::start_page::Startup::Homepage);
        assert!(settings.start.widgets);
        assert_eq!(settings.start.widget_bg, crate::start_page::WidgetBackground::Transparent);
        assert_eq!(settings.start.thumb_size, crate::start_page::DEFAULT_THUMB_SIZE, "a value that does not parse is ignored");
        assert!(settings.start.startup_pages.is_empty(), "a non-web address never becomes a startup page");
        assert_eq!(Settings::parse(&render_updated("", &settings)).start, settings.start);
        assert_eq!(Settings::default().start, crate::start_page::StartSettings::default());
    }

    #[test]
    fn render_into_empty_text_roundtrips() {
        let text = render_updated("", &sample());
        assert_eq!(Settings::parse(&text), sample());
        assert!(text.contains("sleep_after_secs=900\n"));
    }

    #[test]
    fn render_keeps_comments_unknown_keys_and_crlf() {
        let existing = "# my notes\r\nfoo=bar\r\nnotes_panel_width=360\r\nsleep_enabled=true\r\n# blocker_allow=old.com\r\n";
        let text = render_updated(existing, &sample());
        assert_eq!(
            text,
            "# my notes\r\nfoo=bar\r\nnotes_panel_width=360\r\nsleep_enabled=false\r\n# blocker_allow=old.com\r\n\
             tab_layout=vertical\r\nsidebar_width=300\r\nsidebar_collapsed=false\r\nside_panel_width=340\r\nsleep_after_secs=900\r\n\
             blocker_enabled=false\r\nblocker_allow=a.com,b.org\r\nsearch_engine=duckduckgo\r\n\
             automatic_tab_grouping=false\r\nefficiency_mode=on\r\ntracking_prevention=strict\r\n\
             theme=system\r\ntheme_accent=#1a2b3c\r\ntoolbar_buttons=back,reload\r\n\
             toolbar_tabs=closed-tabs,cmd:new-tab\r\ntoolbar_panel=feeds\r\nfeeds_detect=false\r\nfeeds_refresh_minutes=180\r\n\
             centred_address_bar=false\r\nminimal_url=false\r\nshow_load_progress=false\r\n\
             frameless=true\r\nrounded_content_frame=false\r\nshow_home_button=false\r\n\
             bookmarks_bar=always\r\nbookmarks_bar_tab_groups=true\r\nside_panel_position=right\r\n\
             tab_preview_images=false\r\ntab_preview_memory=false\r\nfont_size=large\r\nui_font=Consolas\r\n\
             default_zoom=125\r\nshow_zoom_indicator=false\r\nnew_tab_beside_active=false\r\nctrl_tab_mru=false\r\n\
             shift_right_click_context=true\r\nctrl_shift_c_copy_link=false\r\nctrl_s_vertical_tabs=true\r\n\
             pin_new_tab_groups=true\r\nsplit_edge_drop=false\r\nblock_youtube_video_ads=false\r\n\
             download_dir=D:\\Downloads\\Bowser\r\nask_download_location=true\r\n\
             translate_offer=false\r\ntranslate_auto=true\r\ntranslate_target=fr\r\n\
             blocker_level=trackers\r\ncookie_notices=true\r\nhistory_retention=1w\r\n\
             website_appearance=dark\r\nforce_dark_websites=true\r\nprivate_theme=dark\r\n\
             tab_bar_visible=true\r\naddress_bar_position=bottom\r\naddress_bar_visible=true\r\nshow_title_bar=true\r\n\
             status_bar=hidden\r\nui_density=compact\r\ncompact_menus=true\r\nui_zoom=125\r\nlayout_saved=\r\n\
             auto_hide_bars=panel\r\nauto_hide_fullscreen=false\r\n\
             start_group_reopen=first\r\nstart_titles=auto\r\nstart_add_button=false\r\nstart_columns=9\r\nstart_thumb_size=5\r\n\
             start_drag_reorder=false\r\nhomepage=https://home.example/\r\nstartup=specific_pages\r\n\
             startup_pages=https://a.example/ https://b.example/\r\nstart_widgets=true\r\nstart_widget_columns=4\r\n\
             start_widget_height=tall\r\nstart_widget_bg=custom\r\nstart_widget_color=#336699\r\nstart_widget_drag=false\r\n".to_owned()
                // The address bar, search and bookmark options follow, one line each (address_settings.rs).
                + &crate::address_settings::AddressSettings::default().render_into("", "\r\n")
                + &crate::page_tools::PageTools::default().render_into("", "\r\n")
        );
        assert_eq!(Settings::parse(&text), sample());
    }

    // --- wave-1 fixes: key-level merge ---
    #[test]
    fn merge_with_no_changes_keeps_every_value() {
        assert_eq!(merge_changed(&sample(), &sample(), &sample()), sample());
        assert_eq!(merge_changed(&Settings::default(), &Settings::default(), &sample()), sample());
        assert_eq!(merge_changed(&sample(), &sample(), &Settings::default()), Settings::default());
    }

    /// The reported revert: the page changes Tracking prevention in the file, then the app saves
    /// "Ask where to save" from memory. Several pairs: one key the app changed, one the page changed.
    #[test]
    fn merge_keeps_the_files_newer_value_for_keys_memory_did_not_change() {
        let base = Settings::default();
        type Change = fn(&mut Settings);
        let pairs: [(Change, Change); 5] = [
            (|s| s.ask_download_location = true, |s| s.tracking_prevention = TrackingLevel::Strict),
            (|s| s.translate_offer = false, |s| s.blocker_level = BlockerLevel::Trackers),
            (|s| s.sidebar_collapsed = true, |s| s.minimal_url = false),
            (|s| s.tabs.keep_window = !s.tabs.keep_window, |s| s.theme = Theme::System),
            (|s| s.download_dir = "D:\\Saved".into(), |s| s.address.select_on_activate = !s.address.select_on_activate),
        ];
        for (app_change, page_change) in pairs {
            let mut memory = base.clone();
            app_change(&mut memory);
            let mut file = base.clone();
            page_change(&mut file);
            let mut want = file.clone();
            app_change(&mut want);
            assert_eq!(merge_changed(&base, &memory, &file), want);
        }
    }

    #[test]
    fn merge_lets_a_key_memory_changed_win_over_the_file() {
        let base = Settings::default();
        let memory = Settings { ask_download_location: true, ..Settings::default() };
        let file = Settings { ask_download_location: false, search_engine: Some(SearchEngine::Google), ..Settings::default() };
        let merged = merge_changed(&base, &memory, &file);
        assert!(merged.ask_download_location);
        assert_eq!(merged.search_engine, Some(SearchEngine::Google), "an engine only the file names stays");
    }

    #[test]
    fn reset_puts_every_option_back_but_keeps_history_retention() {
        let reset = reset_defaults(&sample());
        assert_eq!(reset.history_retention, sample().history_retention);
        assert_ne!(sample().history_retention, Settings::default().history_retention, "the sample proves it");
        let defaults = Settings::default();
        assert_eq!(
            Settings {
                history_retention: defaults.history_retention,
                ctrl_shift_c_copy_link: defaults.ctrl_shift_c_copy_link,
                shift_right_click_context: defaults.shift_right_click_context,
                ctrl_s_vertical_tabs: defaults.ctrl_s_vertical_tabs,
                ..reset
            },
            defaults
        );
    }

    // --- fix group C ---
    #[test]
    fn reset_keeps_the_three_keyboard_section_options() {
        let defaults = Settings::default();
        let current = Settings {
            ctrl_shift_c_copy_link: !defaults.ctrl_shift_c_copy_link,
            shift_right_click_context: !defaults.shift_right_click_context,
            ctrl_s_vertical_tabs: !defaults.ctrl_s_vertical_tabs,
            minimal_url: !defaults.minimal_url,
            ..Settings::default()
        };
        let reset = reset_defaults(&current);
        assert_eq!(reset.ctrl_shift_c_copy_link, current.ctrl_shift_c_copy_link);
        assert_eq!(reset.shift_right_click_context, current.shift_right_click_context);
        assert_eq!(reset.ctrl_s_vertical_tabs, current.ctrl_s_vertical_tabs);
        assert_eq!(reset.minimal_url, defaults.minimal_url, "other options are reset");
    }

    /// The saved pair dark 06:00 then light 19:00: the light start equals the default dark start, so
    /// a check made while the file is read in key order refused it, and a save from memory (which
    /// re-reads its merged text in sorted key order, dark before light) wrote the reverted value back.
    #[test]
    fn a_saved_schedule_survives_load_and_merge_in_any_key_order() {
        for text in [
            "theme_light_from=19:00
theme_dark_from=06:00
",
            "theme_dark_from=06:00
theme_light_from=19:00
",
            "theme_dark_from=07:00
theme_light_from=20:00
",
        ] {
            let file = Settings::parse(text);
            let (light, dark) = text.lines().fold((0, 0), |(light, dark), line| {
                let (key, value) = line.split_once('=').unwrap();
                let minutes = crate::theme_custom::parse_clock(value).unwrap();
                if key == "theme_light_from" { (minutes, dark) } else { (light, minutes) }
            });
            assert_eq!((file.themes.light_from, file.themes.dark_from), (light, dark), "{text}");
            let merged = merge_changed(&file, &file, &file);
            assert_eq!(merged.themes, file.themes, "{text}");
            let memory = Settings { minimal_url: !file.minimal_url, ..file.clone() };
            assert_eq!(merge_changed(&file, &memory, &file).themes, file.themes, "{text}");
        }
        let equal = Settings::parse("theme_light_from=08:00
theme_dark_from=08:00
");
        let defaults = crate::theme_custom::ThemeOptions::default();
        assert_eq!((equal.themes.light_from, equal.themes.dark_from), (defaults.light_from, defaults.dark_from));
    }

    #[test]
    fn scope_words_survive_load_and_merge_in_any_key_order() {
        for text in ["nick_tabs=t
nick_history=tabs
", "nick_history=tabs
nick_tabs=t
"] {
            let file = Settings::parse(text);
            assert_eq!((file.address.nick_history.as_str(), file.address.nick_tabs.as_str()), ("tabs", "t"), "{text}");
            assert_eq!(merge_changed(&file, &file, &file).address, file.address, "{text}");
        }
    }
    // --- end fix group C ---
    // --- end wave-1 fixes ---

    #[test]
    fn render_collapses_duplicate_setting_lines() {
        let existing = "sleep_after_secs=60\nsleep_after_secs=120\n";
        let text = render_updated(existing, &sample());
        assert_eq!(text.matches("sleep_after_secs").count(), 1);
        assert_eq!(Settings::parse(&text).sleep_after_secs, 900);
    }

    #[test]
    fn save_writes_atomically_and_loads_back() {
        let dir = std::env::temp_dir().join(format!("bowser-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.txt");
        std::fs::write(&path, "# keep me\nfoo=1\n").unwrap();
        save(&path, &sample()).unwrap();
        assert_eq!(Settings::load(&path), sample());
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .starts_with("# keep me\nfoo=1\n")
        );
        let fresh = dir.join("fresh.txt");
        save(&fresh, &Settings::default()).unwrap();
        assert_eq!(Settings::load(&fresh), Settings::default());
        let leftovers = std::fs::read_dir(&dir)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains(".tmp")
            })
            .count();
        assert_eq!(leftovers, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_refuses_oversized_files_without_touching_them() {
        let dir = std::env::temp_dir().join(format!("bowser-big-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.txt");
        let original = "#".repeat(MAX_FILE_BYTES + 1);
        std::fs::write(&path, &original).unwrap();
        assert!(save(&path, &sample()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_refuses_output_that_would_exceed_the_load_limit() {
        let dir = std::env::temp_dir().join(format!("bowser-limit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.txt");
        let _ = std::fs::remove_file(&path);
        // Sites are fine individually but the joined line pushes the saved file past the limit.
        let site = format!("{}.example", "a".repeat(200));
        let huge = Settings {
            blocker_allow: vec![site; MAX_BLOCKER_ALLOW],
            ..Settings::default()
        };
        assert!(save(&path, &huge).is_err());
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 500 allow-list sites within `MAX_BLOCKER_ALLOW_BYTES`, the longest homepage and eight of the
    /// longest startup pages: the page can send all of it, so the file must hold it and read it back.
    #[test]
    fn a_profile_with_every_list_at_its_cap_fits_the_file() {
        let dir = std::env::temp_dir().join(format!("bowser-caps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.txt");
        std::fs::write(&path, TEMPLATE).unwrap();
        let address = |n: usize| format!("https://example.com/{n}/{}", "a".repeat(crate::start_page::MAX_URL_CHARS - 21 - n.to_string().len()));
        let mut full = Settings {
            blocker_allow: (0..MAX_BLOCKER_ALLOW).map(|n| format!("{n:03}{}.example", "a".repeat(28))).collect(),
            ui_font: "f".repeat(MAX_UI_FONT_CHARS),
            download_dir: format!("C:\\{}", "d".repeat(MAX_DOWNLOAD_DIR_CHARS - 3)),
            ..sample()
        };
        full.start.homepage = address(9);
        full.start.startup = crate::start_page::Startup::SpecificPages;
        full.start.startup_pages = (0..crate::start_page::MAX_STARTUP_PAGES).map(address).collect();
        full.tabs.new_tab_url = crate::tab_options::PageUrl(address(10));
        assert!(full.blocker_allow.join(",").len() <= MAX_BLOCKER_ALLOW_BYTES);
        assert!(full.start.startup_pages.iter().all(|page| page.len() == crate::start_page::MAX_URL_CHARS));
        let written = render_updated(TEMPLATE, &full);
        assert!(written.len() <= MAX_FILE_BYTES, "{} bytes", written.len());
        save(&path, &full).unwrap();
        let back = read_strict(&path).unwrap();
        assert_eq!(back.blocker_allow, full.blocker_allow);
        assert_eq!(back.start, full.start);
        assert_eq!(back, full);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_allow_list_over_its_byte_cap_is_refused_or_cut_at_a_whole_site() {
        let site = format!("{}.example", "a".repeat(50));
        let over = vec![site.as_str(); MAX_BLOCKER_ALLOW].join(",");
        assert!(over.len() > MAX_BLOCKER_ALLOW_BYTES);
        assert!(parse_blocker_allow(&over).unwrap_err().contains("characters"), "the dialog and the page say why");
        let read = Settings::parse(&format!("blocker_allow={over}\n")).blocker_allow;
        assert_eq!(read.len(), (MAX_BLOCKER_ALLOW_BYTES + 1) / (site.len() + 1), "whole sites up to the cap");
        assert!(read.iter().all(|kept| *kept == site));
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let dir = std::env::temp_dir().join(format!("bowser-parent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("profile").join("settings.txt");
        save(&path, &sample()).unwrap();
        assert_eq!(Settings::load(&path), sample());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strict_read_reports_bad_files_and_defaults_missing_ones() {
        let dir = std::env::temp_dir().join(format!("bowser-strict-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            read_strict(&dir.join("none.txt")).unwrap(),
            Settings::default()
        );
        let big = dir.join("big.txt");
        std::fs::write(&big, "#".repeat(MAX_FILE_BYTES + 1)).unwrap();
        assert!(read_strict(&big).is_err());
        let bad = dir.join("bad.txt");
        std::fs::write(&bad, [0xFFu8, 0xFE, b'=']).unwrap();
        assert!(read_strict(&bad).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn form_validation_accepts_bounds_and_rejects_the_rest() {
        assert_eq!(parse_sleep_after_secs(" 30 "), Ok(30));
        assert_eq!(
            parse_sleep_after_secs(&MAX_SLEEP_AFTER_SECS.to_string()),
            Ok(MAX_SLEEP_AFTER_SECS)
        );
        assert!(parse_sleep_after_secs("29").is_err());
        assert!(parse_sleep_after_secs("604801").is_err());
        assert!(parse_sleep_after_secs("1.5").is_err());
        assert!(parse_sleep_after_secs("").is_err());

        assert_eq!(
            parse_blocker_allow(" a.com, ,b.org,"),
            Ok(vec!["a.com".to_owned(), "b.org".to_owned()])
        );
        assert_eq!(parse_blocker_allow(""), Ok(Vec::new()));
        assert!(parse_blocker_allow("a b.com").is_err());
        let too_many = vec!["x.com"; MAX_BLOCKER_ALLOW + 1].join(",");
        assert!(parse_blocker_allow(&too_many).is_err());

        assert_eq!(parse_default_zoom(" 100 "), Ok(100));
        assert_eq!(parse_default_zoom(&MIN_DEFAULT_ZOOM.to_string()), Ok(MIN_DEFAULT_ZOOM));
        assert_eq!(parse_default_zoom(&MAX_DEFAULT_ZOOM.to_string()), Ok(MAX_DEFAULT_ZOOM));
        assert!(parse_default_zoom("24").is_err());
        assert!(parse_default_zoom("501").is_err());
        assert!(parse_default_zoom("1.5").is_err());
        assert!(parse_default_zoom("").is_err());
    }

    #[test]
    fn theme_accent_parses_hex_and_folds_case() {
        assert_eq!(parse_theme_accent(""), Some(None));
        assert_eq!(parse_theme_accent("#1a2b3c"), Some(Some("#1a2b3c".to_owned())));
        assert_eq!(parse_theme_accent("#1A2B3C"), Some(Some("#1a2b3c".to_owned())));
        for bad in ["1a2b3c", "#1a2b3", "#1a2b3cd", "red", "#gggggg"] {
            assert_eq!(parse_theme_accent(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_hand_edited_unknown_command_button_drops_only_itself_from_a_bar() {
        let s = Settings::parse("toolbar_buttons=back,cmd:no-such-command,reload
toolbar_tabs=cmd:nope,closed-tabs
");
        assert_eq!(s.toolbar_buttons, vec!["back".to_owned(), "reload".to_owned()]);
        assert_eq!(s.toolbar_tabs, vec!["closed-tabs".to_owned()]);
        assert_eq!(Settings::parse("toolbar_buttons=back,bogus
").toolbar_buttons, Settings::default().toolbar_buttons, "any other bad id keeps the default");
    }

    #[test]
    fn toolbar_buttons_reject_unknown_and_duplicate_ids_and_cap_the_list() {
        assert_eq!(parse_toolbar_buttons(""), Some(Vec::new()));
        assert_eq!(parse_toolbar_buttons("back,reload"), Some(vec!["back".to_owned(), "reload".to_owned()]));
        assert_eq!(parse_toolbar_buttons(" back , reload "), Some(vec!["back".to_owned(), "reload".to_owned()]));
        assert_eq!(parse_toolbar_buttons(&KNOWN_TOOLBAR_BUTTONS.join(",")), Some(default_toolbar_buttons()));
        assert_eq!(parse_toolbar_buttons("back,bogus"), None);
        assert_eq!(parse_toolbar_buttons("back,back"), None, "no duplicates");
        assert_eq!(parse_toolbar_buttons("Back"), None, "ids are exact case");
    }

    #[test]
    fn template_creates_only_when_missing() {
        let dir = std::env::temp_dir().join(format!("bowser-template-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.txt");
        let _ = std::fs::remove_file(&path);
        create_template_if_missing(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), TEMPLATE);
        std::fs::write(&path, "sleep_enabled=false\n").unwrap();
        create_template_if_missing(&path).unwrap();
        assert!(!Settings::load(&path).sleep_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn translate_settings_default_to_offer_only_and_round_trip() {
        let defaults = Settings::default();
        assert!(defaults.translate_offer, "the pill is on by default");
        assert!(!defaults.translate_auto, "nothing translates by itself by default");
        assert_eq!(defaults.translate_target, "", "empty means the Windows display language");
        let text = render_updated("translate_auto=false\n", &sample());
        assert_eq!(text.matches("translate_auto").count(), 1);
        assert!(text.contains("translate_offer=false\n"));
        assert!(text.contains("translate_auto=true\n"));
        assert!(text.contains("translate_target=fr\n"));
        assert_eq!(Settings::parse(&text), sample());
        assert!(TEMPLATE.contains("# translate_offer=true\r\n"));
        assert!(TEMPLATE.contains("# translate_auto=false\r\n"));
        assert_eq!(Settings::parse(TEMPLATE), Settings::default());
    }

    #[test]
    fn translate_target_accepts_tags_and_empty_and_ignores_junk() {
        assert_eq!(Settings::parse("translate_target=en-GB").translate_target, "en");
        assert_eq!(Settings::parse("translate_target=FR").translate_target, "fr");
        assert_eq!(Settings::parse("translate_target=").translate_target, "");
        assert_eq!(Settings::parse("translate_target=fr\ntranslate_target=").translate_target, "");
        // A bad value keeps the previous one.
        assert_eq!(Settings::parse("translate_target=de\ntranslate_target=<x>").translate_target, "de");
        assert_eq!(Settings::parse("translate_target=klingon-ish!").translate_target, "");
        assert_eq!(parse_translate_target(" es "), Some("es".to_owned()));
        assert_eq!(parse_translate_target("1234"), None);
        assert!(Settings::parse("translate_offer=maybe").translate_offer, "an unreadable value keeps the default");
    }
}
