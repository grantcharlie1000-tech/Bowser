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
    # toolbar_buttons=back,forward,reload,site-info,bookmark,newtab,split,reader\r\n\
    # centred_address_bar=true\r\n\
    # minimal_url=true\r\n\
    # show_load_progress=true\r\n\
    # frameless=false\r\n\
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
    # translate_target=\r\n";
/// Largest settings file that will be read; a larger file is ignored entirely.
pub const MAX_FILE_BYTES: usize = 16 * 1024;
pub const DEFAULT_SLEEP_AFTER_SECS: u64 = 300;
/// Hidden tabs are suspended after 30 s, so sleeping earlier would never be eligible.
pub const MIN_SLEEP_AFTER_SECS: u64 = 30;
pub const MAX_SLEEP_AFTER_SECS: u64 = 7 * 24 * 60 * 60;

/// Most `blocker_allow` entries kept; the rest of a longer list is ignored.
pub const MAX_BLOCKER_ALLOW: usize = 500;

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
}

impl TabLayout {
    /// Every layout, in the order the settings dropdown lists them.
    pub const ALL: [TabLayout; 4] = [Self::ToolbarTop, Self::TabsTop, Self::SingleRow, Self::Vertical];

    /// The value saved in `settings.txt` and exchanged with the settings page.
    pub fn key(self) -> &'static str {
        match self {
            Self::ToolbarTop => "horizontal",
            Self::TabsTop => "tabs-top",
            Self::SingleRow => "single-row",
            Self::Vertical => "vertical",
        }
    }

    /// The layout named by `key`, in any letter case.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layout| layout.key().eq_ignore_ascii_case(key))
    }

    /// Whether the tab list is the left sidebar rather than a row of tabs in the top chrome.
    pub fn is_vertical(self) -> bool {
        self == Self::Vertical
    }
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

/// Toolbar buttons the "Customise your toolbar" row can show or hide, in the order they appear
/// when every one of them is on (the default).
pub const KNOWN_TOOLBAR_BUTTONS: [&str; 8] = [
    "back", "forward", "reload", "site-info", "bookmark", "newtab", "split", "reader",
];

/// Parses a `toolbar_buttons` value: a comma-separated list of [`KNOWN_TOOLBAR_BUTTONS`], each at
/// most once. `None` for an unknown id, a repeated id, or more entries than known buttons exist.
pub fn parse_toolbar_buttons(value: &str) -> Option<Vec<String>> {
    let mut seen = std::collections::HashSet::new();
    let mut buttons = Vec::new();
    for raw in value.split(',') {
        let id = raw.trim();
        if id.is_empty() {
            continue;
        }
        if !KNOWN_TOOLBAR_BUTTONS.contains(&id) || !seen.insert(id) {
            return None;
        }
        buttons.push(id.to_owned());
    }
    (buttons.len() <= KNOWN_TOOLBAR_BUTTONS.len()).then_some(buttons)
}

fn default_toolbar_buttons() -> Vec<String> {
    KNOWN_TOOLBAR_BUTTONS.iter().map(|&id| id.to_owned()).collect()
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
    /// `toolbar_buttons`: the toolbar's optional buttons, in the order shown. Applies after restart.
    pub toolbar_buttons: Vec<String>,
    /// `centred_address_bar` (default true). Applies immediately.
    pub centred_address_bar: bool,
    /// `minimal_url` (default true): the registrable host (and a path when useful) while unfocused,
    /// the full address while focused. Applies immediately.
    pub minimal_url: bool,
    /// `show_load_progress` (default true). Applies immediately.
    pub show_load_progress: bool,
    /// `frameless` (default false): hide the browser chrome until the pointer nears the window
    /// edge. Applies after restart.
    pub frameless: bool,
    /// `rounded_content_frame` (default true). Applies immediately.
    pub rounded_content_frame: bool,
    /// `show_home_button` (default true): the Home button in the rail. Applies immediately.
    pub show_home_button: bool,
    /// `bookmarks_bar=always|never|newtab` (default never). Applies after restart.
    pub bookmarks_bar: BookmarksBar,
    /// `bookmarks_bar_tab_groups` (default false). Applies after restart.
    pub bookmarks_bar_tab_groups: bool,
    /// `side_panel_position=left|right` (default left). Applies after restart.
    pub side_panel_position: SidePanelPosition,
    /// `tab_preview_images` (default true). Applies after restart.
    pub tab_preview_images: bool,
    /// `tab_preview_memory` (default true). Applies after restart.
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
                        .take(MAX_BLOCKER_ALLOW)
                        .map(str::to_owned)
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
                    if let Some(buttons) = parse_toolbar_buttons(value) {
                        settings.toolbar_buttons = buttons;
                    }
                }
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
                _ => {}
            }
        }
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
    let values: [(&str, Option<String>); 43] = [
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
    ];
    let mut written = [false; 43];
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
    out
}

/// Reads the file as text, refusing anything over `MAX_FILE_BYTES` or not valid UTF-8. Unlike
/// `load`, the reason is returned, so the settings window can say what is wrong with the file.
fn read_text(path: &Path) -> io::Result<String> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_FILE_BYTES as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "settings file is larger than the 16 KiB limit",
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
    Ok(())
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
        }
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
             centred_address_bar=false\r\nminimal_url=false\r\nshow_load_progress=false\r\n\
             frameless=true\r\nrounded_content_frame=false\r\nshow_home_button=false\r\n\
             bookmarks_bar=always\r\nbookmarks_bar_tab_groups=true\r\nside_panel_position=right\r\n\
             tab_preview_images=false\r\ntab_preview_memory=false\r\nfont_size=large\r\nui_font=Consolas\r\n\
             default_zoom=125\r\nshow_zoom_indicator=false\r\nnew_tab_beside_active=false\r\nctrl_tab_mru=false\r\n\
             shift_right_click_context=true\r\nctrl_shift_c_copy_link=false\r\nctrl_s_vertical_tabs=true\r\n\
             pin_new_tab_groups=true\r\nsplit_edge_drop=false\r\nblock_youtube_video_ads=false\r\n\
             download_dir=D:\\Downloads\\Bowser\r\nask_download_location=true\r\n\
             translate_offer=false\r\ntranslate_auto=true\r\ntranslate_target=fr\r\n"
        );
        assert_eq!(Settings::parse(&text), sample());
    }

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
        let site = format!("{}.example", "a".repeat(60));
        let huge = Settings {
            blocker_allow: vec![site; MAX_BLOCKER_ALLOW],
            ..Settings::default()
        };
        assert!(save(&path, &huge).is_err());
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
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
