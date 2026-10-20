//! Page tools (thread T7): page capture, spatial navigation, reader view defaults, how page zoom is
//! kept, and the pop-out video entry.
//!
//! These live as extra `key=value` lines (all starting `pt_`) in the profile's `settings.txt`,
//! carried by [`crate::settings::Settings::page_tools`]. The settings page changes them one key at a
//! time through [`PageTools::set`], which refuses anything it does not understand, so a typo or a
//! hostile page value never reaches a consumer. The pure decisions (file names, folders, how a long
//! page is cut into slices, which zoom a navigation lands on) are here so each can be tested without
//! a window; `crate::capture` and `crate::app::page_tools` do the work with them.

use std::path::{Component, Path, PathBuf};

// ---------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------

/// The image format a capture is saved in (and copied to a note's file in).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CaptureFormat {
    #[default]
    Png,
    Jpeg,
}

impl CaptureFormat {
    pub fn key(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        [Self::Png, Self::Jpeg].into_iter().find(|format| format.key() == value)
    }
}

/// Whether a page's zoom belongs to the site or to the tab.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ZoomScope {
    /// Every tab on the same site shows it, and it comes back next time (what Bowser always did).
    #[default]
    Site,
    /// Only this tab, for every page it shows. Nothing is stored for the site.
    Tab,
}

impl ZoomScope {
    pub fn key(self) -> &'static str {
        match self {
            Self::Site => "site",
            Self::Tab => "tab",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        [Self::Site, Self::Tab].into_iter().find(|scope| scope.key() == value)
    }
}

/// The typeface reader view sets its text in.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReaderFont {
    #[default]
    Serif,
    Sans,
    Mono,
}

impl ReaderFont {
    pub const ALL: [Self; 3] = [Self::Serif, Self::Sans, Self::Mono];
    pub fn key(self) -> &'static str {
        match self {
            Self::Serif => "serif",
            Self::Sans => "sans",
            Self::Mono => "mono",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|font| font.key() == value)
    }
}

pub const READER_SIZE_RANGE: std::ops::RangeInclusive<u8> = crate::reader::MIN_SIZE..=crate::reader::MAX_SIZE;
/// Line height in tenths of the font size (16 is 1.6).
pub const READER_LINE_RANGE: std::ops::RangeInclusive<u8> = 12..=24;
/// Column width in `ch`.
pub const READER_WIDTH_RANGE: std::ops::RangeInclusive<u8> = 40..=120;
pub const MAX_TEMPLATE_CHARS: usize = 80;
pub const MAX_DIR_CHARS: usize = 240;
pub const DEFAULT_TEMPLATE: &str = "{host} {date} {time}";

/// Every option of this module. Defaults match what Bowser did before the options existed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageTools {
    /// `pt_spatial_nav`: Shift+arrow keys move between links on a page.
    pub spatial_nav: bool,
    /// `pt_capture_dir`: where captures are saved; empty is Pictures\Bowser.
    pub capture_dir: String,
    /// `pt_capture_name`: the file name, with `{host}`, `{title}`, `{date}`, `{time}` and `{mode}`.
    pub capture_name: String,
    /// `pt_capture_format`.
    pub capture_format: CaptureFormat,
    /// `pt_zoom_scope`.
    pub zoom_scope: ZoomScope,
    /// `pt_wheel_zoom`: Ctrl+scroll zooms the page.
    pub wheel_zoom: bool,
    /// `pt_pip_menu`: the tab menu offers "Picture in picture" for a playing video.
    pub pip_menu: bool,
    /// `pt_reader_*`: the reader view's defaults.
    pub reader_font: ReaderFont,
    pub reader_size: u8,
    pub reader_line: u8,
    pub reader_width: u8,
    pub reader_theme: crate::reader::Theme,
    pub reader_vertical: bool,
    /// The reader's control bar (its settings icon) is on the page.
    pub reader_icon: bool,
    /// The reader shows how long the page takes to read.
    pub reader_time: bool,
}

impl Default for PageTools {
    fn default() -> Self {
        Self {
            spatial_nav: false,
            capture_dir: String::new(),
            capture_name: DEFAULT_TEMPLATE.to_owned(),
            capture_format: CaptureFormat::Png,
            zoom_scope: ZoomScope::Site,
            wheel_zoom: true,
            pip_menu: true,
            reader_font: ReaderFont::Serif,
            reader_size: crate::reader::DEFAULT_SIZE,
            reader_line: 16,
            reader_width: 68,
            reader_theme: crate::reader::Theme::Auto,
            reader_vertical: false,
            reader_icon: true,
            reader_time: false,
        }
    }
}

/// How a key's value is typed on the wire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Bool,
    Int,
    Text,
}

/// Every key, in file order, with its wire type.
pub const KEYS: &[(&str, Kind)] = &[
    ("pt_spatial_nav", Kind::Bool),
    ("pt_capture_dir", Kind::Text),
    ("pt_capture_name", Kind::Text),
    ("pt_capture_format", Kind::Text),
    ("pt_zoom_scope", Kind::Text),
    ("pt_wheel_zoom", Kind::Bool),
    ("pt_pip_menu", Kind::Bool),
    ("pt_reader_font", Kind::Text),
    ("pt_reader_size", Kind::Int),
    ("pt_reader_line", Kind::Int),
    ("pt_reader_width", Kind::Int),
    ("pt_reader_theme", Kind::Text),
    ("pt_reader_vertical", Kind::Bool),
    ("pt_reader_icon", Kind::Bool),
    ("pt_reader_time", Kind::Bool),
];

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn theme_key(theme: crate::reader::Theme) -> &'static str {
    use crate::reader::Theme::{Auto, Dark, Light, Sepia};
    match theme {
        Auto => "auto",
        Light => "light",
        Sepia => "sepia",
        Dark => "dark",
    }
}

fn parse_theme(value: &str) -> Option<crate::reader::Theme> {
    use crate::reader::Theme::{Auto, Dark, Light, Sepia};
    [Auto, Light, Sepia, Dark].into_iter().find(|theme| theme_key(*theme) == value)
}

impl PageTools {
    /// Sets `key` to `value`. `false` (and no change) for an unknown key or a value that is not
    /// allowed. Used by both the file parser (which ignores the result) and the settings page.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let value = value.trim();
        let flag = |slot: &mut bool| match parse_bool(value) {
            Some(on) => {
                *slot = on;
                true
            }
            None => false,
        };
        let number = |slot: &mut u8, range: &std::ops::RangeInclusive<u8>| match value.parse::<u8>() {
            Ok(n) if range.contains(&n) => {
                *slot = n;
                true
            }
            _ => false,
        };
        match key {
            "pt_spatial_nav" => flag(&mut self.spatial_nav),
            "pt_capture_dir" => {
                if value.is_empty() || valid_dir(value) {
                    self.capture_dir = value.to_owned();
                    true
                } else {
                    false
                }
            }
            "pt_capture_name" => {
                if valid_template(value) {
                    self.capture_name = value.to_owned();
                    true
                } else {
                    false
                }
            }
            "pt_capture_format" => CaptureFormat::parse(value).map(|f| self.capture_format = f).is_some(),
            "pt_zoom_scope" => ZoomScope::parse(value).map(|s| self.zoom_scope = s).is_some(),
            "pt_wheel_zoom" => flag(&mut self.wheel_zoom),
            "pt_pip_menu" => flag(&mut self.pip_menu),
            "pt_reader_font" => ReaderFont::parse(value).map(|f| self.reader_font = f).is_some(),
            "pt_reader_size" => number(&mut self.reader_size, &READER_SIZE_RANGE),
            "pt_reader_line" => number(&mut self.reader_line, &READER_LINE_RANGE),
            "pt_reader_width" => number(&mut self.reader_width, &READER_WIDTH_RANGE),
            "pt_reader_theme" => parse_theme(value).map(|t| self.reader_theme = t).is_some(),
            "pt_reader_vertical" => flag(&mut self.reader_vertical),
            "pt_reader_icon" => flag(&mut self.reader_icon),
            "pt_reader_time" => flag(&mut self.reader_time),
            _ => false,
        }
    }

    /// The value of `key` as the file and the wire write it. `None` for an unknown key.
    pub fn get(&self, key: &str) -> Option<String> {
        let flag = |on: bool| Some(if on { "true" } else { "false" }.to_owned());
        match key {
            "pt_spatial_nav" => flag(self.spatial_nav),
            "pt_capture_dir" => Some(self.capture_dir.clone()),
            "pt_capture_name" => Some(self.capture_name.clone()),
            "pt_capture_format" => Some(self.capture_format.key().to_owned()),
            "pt_zoom_scope" => Some(self.zoom_scope.key().to_owned()),
            "pt_wheel_zoom" => flag(self.wheel_zoom),
            "pt_pip_menu" => flag(self.pip_menu),
            "pt_reader_font" => Some(self.reader_font.key().to_owned()),
            "pt_reader_size" => Some(self.reader_size.to_string()),
            "pt_reader_line" => Some(self.reader_line.to_string()),
            "pt_reader_width" => Some(self.reader_width.to_string()),
            "pt_reader_theme" => Some(theme_key(self.reader_theme).to_owned()),
            "pt_reader_vertical" => flag(self.reader_vertical),
            "pt_reader_icon" => flag(self.reader_icon),
            "pt_reader_time" => flag(self.reader_time),
            _ => None,
        }
    }

    /// Every key and its current value, in file order.
    pub fn pairs(&self) -> Vec<(&'static str, String)> {
        KEYS.iter().filter_map(|(key, _)| self.get(key).map(|value| (*key, value))).collect()
    }

    /// Whether `key` is one of this module's.
    pub fn owns(key: &str) -> bool {
        KEYS.iter().any(|(name, _)| *name == key)
    }

    /// Reads one `key=value` line from the file, ignoring what is not understood.
    pub fn parse_line(&mut self, key: &str, value: &str) {
        let _ = self.set(key, value);
    }

    /// `text` with this module's lines replaced in place (or appended), and every other line kept.
    /// Called by [`crate::settings::render_updated`] after it wrote its own keys.
    pub fn render_into(&self, text: &str, newline: &str) -> String {
        let values = self.pairs();
        let mut written = vec![false; values.len()];
        let mut out = String::with_capacity(text.len() + 512);
        for line in text.lines() {
            let trimmed = line.trim();
            let slot = if trimmed.starts_with('#') {
                None
            } else {
                trimmed.split_once('=').and_then(|(key, _)| values.iter().position(|(name, _)| *name == key.trim()))
            };
            match slot {
                Some(index) if written[index] => {}
                Some(index) => {
                    written[index] = true;
                    out.push_str(&format!("{}={}{newline}", values[index].0, values[index].1));
                }
                None => {
                    out.push_str(line);
                    out.push_str(newline);
                }
            }
        }
        for (index, (name, value)) in values.iter().enumerate() {
            if !written[index] {
                out.push_str(&format!("{name}={value}{newline}"));
            }
        }
        out
    }

    /// The reader view's defaults as the page script is told them.
    pub fn reader(&self) -> crate::reader::Typography {
        crate::reader::Typography {
            font: self.reader_font,
            line_tenths: self.reader_line,
            width_ch: self.reader_width,
            vertical: self.reader_vertical,
            controls: self.reader_icon,
            reading_time: self.reader_time,
        }
    }

    /// The theme and size the reader's session controls start from.
    pub fn reader_prefs(&self) -> crate::reader::Prefs {
        crate::reader::Prefs { theme: self.reader_theme, size: self.reader_size }
    }
}

// ---------------------------------------------------------------------------------------------
// Capture: where a file goes and what it is called
// ---------------------------------------------------------------------------------------------

/// A folder the user may type: a drive-letter path on this PC, no `..`, no device or UNC prefix, no
/// characters Windows refuses in a path. Bowser writes files there, so nothing that could reach
/// another machine or a device is accepted.
pub fn valid_dir(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.chars().count() <= MAX_DIR_CHARS
        && bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
        && !value.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
        && !value[2..].contains(':')
        && !value[2..].split(['\\', '/']).any(|part| part == ".." || part == "." || part.ends_with(' ') || (part.ends_with('.') && part.len() > 1))
        && Path::new(value).components().all(|c| !matches!(c, Component::ParentDir))
}

/// A file name template: at most [`MAX_TEMPLATE_CHARS`] characters, only the known `{tokens}`, and
/// no path separators (a name is never a path).
pub fn valid_template(value: &str) -> bool {
    if value.is_empty() || value.chars().count() > MAX_TEMPLATE_CHARS || value.chars().any(|c| c.is_control() || matches!(c, '\\' | '/')) {
        return false;
    }
    let mut rest = value;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { return false };
        if !matches!(&after[..close], "host" | "title" | "date" | "time" | "mode") {
            return false;
        }
        rest = &after[close + 1..];
    }
    !rest.contains('}')
}

/// A local date and time, for naming a file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Stamp {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl Stamp {
    /// The local time now.
    pub fn now() -> Self {
        #[cfg(windows)]
        {
            let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
            Self {
                year: t.wYear,
                month: t.wMonth as u8,
                day: t.wDay as u8,
                hour: t.wHour as u8,
                minute: t.wMinute as u8,
                second: t.wSecond as u8,
            }
        }
        #[cfg(not(windows))]
        {
            Self { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 }
        }
    }
}

/// What a capture covers, for the `{mode}` token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureMode {
    Visible,
    FullPage,
    Selection,
}

impl CaptureMode {
    pub fn key(self) -> &'static str {
        match self {
            Self::Visible => "visible",
            Self::FullPage => "full",
            Self::Selection => "selection",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Visible => "visible area",
            Self::FullPage => "full page",
            Self::Selection => "selection",
        }
    }
}

/// Where the picture goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureTarget {
    /// A file in the capture folder.
    File,
    /// The clipboard, as a bitmap.
    Clipboard,
    /// A file in the capture folder and a new note that points at it.
    Note,
}

/// Makes `text` safe as part of a Windows file name: control characters and `<>:"/\|?*` become
/// spaces, runs of spaces collapse, and the ends lose spaces and dots. Cut at `max` characters.
pub fn clean_name_part(text: &str, max: usize) -> String {
    let spaced: String = text.chars().map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { ' ' } else { c }).collect();
    let joined = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = joined.chars().take(max).collect();
    cut.trim_matches(|c: char| c == ' ' || c == '.').to_owned()
}

/// Whether Windows reads `stem` as a device name (`nul .x` is NUL); the same rule downloads use.
fn reserved(stem: &str) -> bool {
    crate::downloads::is_reserved_device_name(stem)
}

/// The file name (without extension) a template makes. An empty or reserved result becomes `capture`.
pub fn capture_stem(template: &str, host: &str, title: &str, mode: CaptureMode, at: Stamp) -> String {
    let template = if valid_template(template) { template } else { DEFAULT_TEMPLATE };
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after.find('}').unwrap_or(after.len());
        match &after[..close] {
            "host" => out.push_str(&clean_name_part(host, 60)),
            "title" => out.push_str(&clean_name_part(title, 60)),
            "date" => out.push_str(&format!("{:04}-{:02}-{:02}", at.year, at.month, at.day)),
            "time" => out.push_str(&format!("{:02}-{:02}-{:02}", at.hour, at.minute, at.second)),
            "mode" => out.push_str(mode.key()),
            _ => {}
        }
        rest = after.get(close + 1..).unwrap_or("");
    }
    out.push_str(rest);
    let stem = clean_name_part(&out, 120);
    if stem.is_empty() || reserved(&stem) { "capture".to_owned() } else { stem }
}

/// `dir` joined with `stem` and the format's extension, made unique with ` (2)`, ` (3)`... when
/// `exists` says a file is already there.
pub fn unique_path(dir: &Path, stem: &str, format: CaptureFormat, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let extension = format.extension();
    let first = dir.join(format!("{stem}.{extension}"));
    if !exists(&first) {
        return first;
    }
    for n in 2..1000 {
        let next = dir.join(format!("{stem} ({n}).{extension}"));
        if !exists(&next) {
            return next;
        }
    }
    dir.join(format!("{stem} ({}).{extension}", Stamp::now().second))
}

// ---------------------------------------------------------------------------------------------
// Capture: how big, and in what pieces
// ---------------------------------------------------------------------------------------------

/// The tallest page a full-page capture covers, in CSS pixels. Anything longer is cut off.
pub const MAX_PAGE_HEIGHT: u32 = 30_000;
/// The widest page a full-page capture covers.
pub const MAX_PAGE_WIDTH: u32 = 8_000;
/// The tallest piece asked of the browser at once (a single screenshot taller than a GPU texture
/// fails), stitched together afterwards.
pub const SLICE_HEIGHT: u32 = 6_000;
/// The most a captured selection may cover on either side.
pub const MAX_SELECTION_SIDE: u32 = 8_000;
/// The largest base64 answer taken from one screenshot call (an 8000 x 6000 PNG can reach this).
pub const MAX_SLICE_BASE64: usize = 64 * 1024 * 1024;

/// A page's size as the browser reports it, cut to what a capture covers. `None` when either side
/// is empty. The flag says the page was cut.
pub fn clamp_page(width: f64, height: f64) -> Option<(u32, u32, bool)> {
    if !(width.is_finite() && height.is_finite()) || width < 1.0 || height < 1.0 {
        return None;
    }
    let (w, h) = (width.ceil().min(1.0e9) as u32, height.ceil().min(1.0e9) as u32);
    let (cw, ch) = (w.min(MAX_PAGE_WIDTH), h.min(MAX_PAGE_HEIGHT));
    Some((cw, ch, cw != w || ch != h))
}

/// The `(top, height)` pieces a page `total` pixels tall is captured in: each at most
/// [`SLICE_HEIGHT`], in order, covering every pixel exactly once.
pub fn plan_slices(total: u32) -> Vec<(u32, u32)> {
    let mut slices = Vec::new();
    let mut top = 0;
    while top < total {
        let height = (total - top).min(SLICE_HEIGHT);
        slices.push((top, height));
        top += height;
    }
    slices
}

/// A selection's rectangle in page coordinates, cut to [`MAX_SELECTION_SIDE`]. `None` when it has no
/// area.
pub fn clamp_selection(x: f64, y: f64, width: f64, height: f64) -> Option<(u32, u32, u32, u32)> {
    if ![x, y, width, height].iter().all(|n| n.is_finite()) || width < 1.0 || height < 1.0 || x < -1.0e6 || y < -1.0e6 || x > 1.0e7 || y > 1.0e7 {
        return None;
    }
    let (x0, y0) = (x.floor().max(0.0), y.floor().max(0.0));
    let (x1, y1) = ((x + width).ceil().min(1.0e9), (y + height).ceil().min(1.0e9));
    let (w, h) = ((x1 - x0).max(1.0) as u32, (y1 - y0).max(1.0) as u32);
    Some((x0 as u32, y0 as u32, w.min(MAX_SELECTION_SIDE), h.min(MAX_SELECTION_SIDE)))
}

// ---------------------------------------------------------------------------------------------
// Zoom
// ---------------------------------------------------------------------------------------------

/// The zoom a page lands on when a navigation commits, or `None` to leave the tab's zoom alone.
///
/// - Site scope: the site's stored zoom, else the default zoom (what Bowser always did).
/// - Tab scope: a tab keeps the zoom the user gave it (`tab_set`) across every page it shows; until
///   then it follows the default zoom. The site's stored zoom is not consulted.
pub fn zoom_on_commit(scope: ZoomScope, tab_set: bool, current: u16, stored_for_site: u16, default: u16) -> Option<u16> {
    let target = match scope {
        ZoomScope::Site => stored_for_site,
        ZoomScope::Tab if tab_set => return None,
        ZoomScope::Tab => default,
    };
    (target != current).then_some(target)
}

/// Whether a zoom the user just set is stored for the site.
pub fn zoom_is_stored(scope: ZoomScope) -> bool {
    scope == ZoomScope::Site
}

// ---------------------------------------------------------------------------------------------
// Pop-out video
// ---------------------------------------------------------------------------------------------

/// Whether the tab menu offers "Picture in picture": the page has a playing video and the setting
/// is on. The Ctrl+Shift+Y command works either way.
pub fn pip_menu_entry(video_available: bool, setting: bool) -> bool {
    video_available && setting
}

// ---------------------------------------------------------------------------------------------
// Spatial navigation
// ---------------------------------------------------------------------------------------------

/// The page script that moves between links with Shift+arrow keys. It is added at document creation
/// only while the setting is on (so a page pays nothing otherwise), runs in the top frame only,
/// reads layout and focuses a link: it makes no request and sends nothing out of the page. Enter on a
/// focused link opens it, and Ctrl+Enter opens it in a new tab, as the browser does for any focused
/// link.
pub const SPATIAL_SCRIPT: &str = include_str!("spatial.js");

/// Turns the script off in a page that already has it (the setting was switched off).
pub const SPATIAL_OFF_SCRIPT: &str =
    "(function(){var s=window[Symbol.for('bowser.spatial.v1')];if(s&&typeof s.off==='function'){s.off();}})()";

// ---------------------------------------------------------------------------------------------
// What the engine reads
// ---------------------------------------------------------------------------------------------

static SPATIAL_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static WHEEL_ZOOM_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Publishes the settings the engine reads when it makes a tab. True when either changed.
pub fn apply_runtime(tools: &PageTools) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    let a = SPATIAL_ON.swap(tools.spatial_nav, Relaxed) != tools.spatial_nav;
    let b = WHEEL_ZOOM_ON.swap(tools.wheel_zoom, Relaxed) != tools.wheel_zoom;
    a || b
}

pub fn spatial_on() -> bool {
    SPATIAL_ON.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn wheel_zoom_on() -> bool {
    WHEEL_ZOOM_ON.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp() -> Stamp {
        Stamp { year: 2026, month: 3, day: 9, hour: 14, minute: 5, second: 7 }
    }

    #[test]
    fn defaults_round_trip_through_get_and_set() {
        let defaults = PageTools::default();
        let mut copy = PageTools {
            spatial_nav: true,
            capture_format: CaptureFormat::Jpeg,
            zoom_scope: ZoomScope::Tab,
            reader_size: 24,
            reader_font: ReaderFont::Mono,
            ..PageTools::default()
        };
        for (key, value) in defaults.pairs() {
            assert!(copy.set(key, &value), "{key}={value}");
        }
        assert_eq!(copy, defaults);
        assert_eq!(defaults.pairs().len(), KEYS.len());
    }

    #[test]
    fn every_key_is_typed_consistently_and_owned() {
        let settings = PageTools::default();
        for (key, kind) in KEYS {
            let value = settings.get(key).expect(key);
            match kind {
                Kind::Bool => assert!(value == "true" || value == "false", "{key}"),
                Kind::Int => assert!(value.parse::<u64>().is_ok(), "{key}"),
                Kind::Text => {}
            }
            assert!(PageTools::owns(key));
        }
        assert!(!PageTools::owns("dd_max_results") && !PageTools::owns(""));
    }

    #[test]
    fn invalid_values_are_refused_and_change_nothing() {
        let mut tools = PageTools::default();
        for (key, value) in [
            ("pt_spatial_nav", "maybe"),
            ("pt_capture_dir", "..\\up"),
            ("pt_capture_dir", "\\\\server\\share\\x"),
            ("pt_capture_dir", "\\\\?\\C:\\x"),
            ("pt_capture_dir", "C:\\a\\..\\b"),
            ("pt_capture_dir", "C:\\a|b"),
            ("pt_capture_dir", "relative\\dir"),
            ("pt_capture_name", "{unknown}"),
            ("pt_capture_name", "a/b"),
            ("pt_capture_name", "{host"),
            ("pt_capture_format", "gif"),
            ("pt_zoom_scope", "window"),
            ("pt_reader_font", "comic"),
            ("pt_reader_size", "13"),
            ("pt_reader_size", "31"),
            ("pt_reader_line", "11"),
            ("pt_reader_width", "39"),
            ("pt_reader_width", "121"),
            ("pt_reader_theme", "purple"),
            ("pt_nothing", "1"),
        ] {
            assert!(!tools.set(key, value), "{key}={value}");
        }
        assert_eq!(tools, PageTools::default());
    }

    #[test]
    fn render_replaces_in_place_and_keeps_other_lines() {
        let mut tools = PageTools::default();
        assert!(tools.set("pt_spatial_nav", "true"));
        let text = tools.render_into("# note\nfoo=bar\npt_spatial_nav=false\npt_spatial_nav=false\n", "\n");
        assert!(text.starts_with("# note\nfoo=bar\npt_spatial_nav=true\n"), "{text}");
        assert_eq!(text.matches("pt_spatial_nav").count(), 1, "a duplicate is dropped");
        assert_eq!(text.lines().count(), 2 + KEYS.len());
    }

    #[test]
    fn folders_accept_a_local_drive_path_and_nothing_else() {
        for ok in ["C:\\Users\\me\\Pictures", "D:/captures", "C:\\a b\\c", "E:\\"] {
            assert!(valid_dir(ok), "{ok}");
        }
        for bad in ["C:", "C:\\a\\.\\b", "C:\\a\\b.", "C:\\a\\b ", "C:\\a:b", "\\\\host\\share", "/etc", "C:\\a\\..", "C:\\con*", &"C:\\".repeat(121)] {
            assert!(!valid_dir(bad), "{bad}");
        }
    }

    #[test]
    fn templates_expand_known_tokens_and_never_make_a_path() {
        let at = stamp();
        assert_eq!(capture_stem("{host} {date} {time}", "example.com", "T", CaptureMode::Visible, at), "example.com 2026-03-09 14-05-07");
        assert_eq!(capture_stem("{title} - {mode}", "h", "A/B: C?", CaptureMode::FullPage, at), "A B C - full");
        assert_eq!(capture_stem("shot", "h", "t", CaptureMode::Selection, at), "shot");
        // A template the page of settings would never save still cannot escape the folder.
        assert_eq!(capture_stem("..\\..\\x", "h", "t", CaptureMode::Visible, at), "h 2026-03-09 14-05-07");
        assert_eq!(capture_stem("{title}", "h", "", CaptureMode::Visible, at), "capture", "empty falls back");
        assert_eq!(capture_stem("{title}", "h", "CON", CaptureMode::Visible, at), "capture", "reserved device names fall back");
        assert_eq!(capture_stem("{title}", "h", "com1.txt", CaptureMode::Visible, at), "capture");
        let long = capture_stem("{title}", "h", &"x".repeat(500), CaptureMode::Visible, at);
        assert_eq!(long.chars().count(), 60);
    }

    #[test]
    fn every_windows_device_name_falls_back() {
        let at = stamp();
        for device in ["nul .x", "NUL  .png", "con ", "CONIN$", "conout$.txt", "COM\u{b9}", "lpt\u{b2}.x", "Com\u{b3}", "LPT9", "aux.tar.gz"] {
            assert!(reserved(device), "{device:?}");
            assert_eq!(capture_stem("{title}", "h", device, CaptureMode::Visible, at), "capture", "{device:?}");
        }
        for fine in ["console", "COM0", "LPT10", "COM\u{b9}0", "nul x", "CONIN", "conin$x", "LPT\u{b4}", "null"] {
            assert!(!reserved(fine), "{fine:?}");
        }
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let dir = Path::new("C:\\caps");
        let taken = |p: &Path| matches!(p.file_name().and_then(|n| n.to_str()), Some("a.png") | Some("a (2).png"));
        assert_eq!(unique_path(dir, "a", CaptureFormat::Png, taken), dir.join("a (3).png"));
        assert_eq!(unique_path(dir, "b", CaptureFormat::Jpeg, taken), dir.join("b.jpg"));
    }

    #[test]
    fn long_pages_are_cut_at_30000_and_sliced_exactly() {
        assert_eq!(clamp_page(1280.0, 100_000.0), Some((1280, 30_000, true)));
        assert_eq!(clamp_page(20_000.0, 500.0), Some((8_000, 500, true)));
        assert_eq!(clamp_page(1280.4, 720.2), Some((1281, 721, false)));
        assert_eq!(clamp_page(0.0, 5.0), None);
        assert_eq!(clamp_page(f64::NAN, 5.0), None);
        assert_eq!(plan_slices(0), vec![]);
        assert_eq!(plan_slices(900), vec![(0, 900)]);
        assert_eq!(plan_slices(30_000), vec![(0, 6000), (6000, 6000), (12_000, 6000), (18_000, 6000), (24_000, 6000)]);
        let odd = plan_slices(13_001);
        assert_eq!(odd.iter().map(|s| s.1).sum::<u32>(), 13_001);
        assert!(odd.windows(2).all(|w| w[0].0 + w[0].1 == w[1].0));
    }

    #[test]
    fn selections_are_clamped_to_the_page_and_a_size() {
        assert_eq!(clamp_selection(10.4, 20.6, 100.0, 50.0), Some((10, 20, 101, 51)));
        assert_eq!(clamp_selection(-5.0, 0.0, 100.0, 50.0), Some((0, 0, 95, 50)));
        assert_eq!(clamp_selection(0.0, 0.0, 99_999.0, 99_999.0), Some((0, 0, 8_000, 8_000)));
        assert_eq!(clamp_selection(0.0, 0.0, 0.0, 10.0), None);
        assert_eq!(clamp_selection(f64::INFINITY, 0.0, 5.0, 5.0), None);
    }

    #[test]
    fn zoom_scope_decides_what_a_navigation_lands_on() {
        // Site scope: the stored zoom (or default) wins, as it always did.
        assert_eq!(zoom_on_commit(ZoomScope::Site, true, 150, 100, 100), Some(100));
        assert_eq!(zoom_on_commit(ZoomScope::Site, false, 100, 125, 100), Some(125));
        assert_eq!(zoom_on_commit(ZoomScope::Site, false, 125, 125, 100), None);
        // Tab scope: a zoom the user set stays with the tab; the site's stored zoom is ignored.
        assert_eq!(zoom_on_commit(ZoomScope::Tab, true, 150, 100, 100), None);
        assert_eq!(zoom_on_commit(ZoomScope::Tab, false, 100, 200, 125), Some(125));
        assert_eq!(zoom_on_commit(ZoomScope::Tab, false, 125, 200, 125), None);
        assert!(zoom_is_stored(ZoomScope::Site) && !zoom_is_stored(ZoomScope::Tab));
    }

    #[test]
    fn the_pip_entry_needs_a_video_and_the_setting() {
        assert!(pip_menu_entry(true, true));
        assert!(!pip_menu_entry(true, false));
        assert!(!pip_menu_entry(false, true));
    }

    #[test]
    fn the_spatial_script_is_self_contained() {
        for banned in ["fetch(", "XMLHttpRequest", "sendBeacon", "postMessage", "eval(", "new Function", "import(", "innerHTML", "WebSocket"] {
            assert!(!SPATIAL_SCRIPT.contains(banned), "{banned}");
        }
        assert!(SPATIAL_SCRIPT.contains("bowser.spatial.v1"));
    }
}
