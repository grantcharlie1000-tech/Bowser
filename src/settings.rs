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
    # tracking_prevention=balanced\r\n";
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
    let values: [(&str, Option<String>); 12] = [
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
    ];
    let mut written = [false; 12];
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
             automatic_tab_grouping=false\r\nefficiency_mode=on\r\ntracking_prevention=strict\r\n"
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
}
