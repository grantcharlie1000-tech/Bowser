//! Theme options of the settings page's Appearance area (thread T8): the colours a person chooses,
//! where the accent colours the chrome, the light/dark schedule, and the theme file that carries a
//! look from one profile to another.
//!
//! Every option is one `theme_*` line in `settings.txt` (the older `theme` and `theme_accent` keep
//! their own fields). The table in [`ThemeOptions`] is the single source of the defaults, the file
//! keys, the settings snapshot and the validation of a value that arrives from the page or from an
//! imported theme file, so a key cannot be saved without being read. The palette arithmetic that
//! acts on the colours lives in `crate::theme`, where its contrast tests are.
//!
//! Windows/WebView2 notes (Vivaldi options that have no meaning here are not offered): Vivaldi's
//! "prefer system colour" is the existing Theme row (System uses the Windows accent), and its
//! colour sampling from a page is limited to the page's `theme-color` meta and the favicon Bowser
//! already holds, so no request is made for it.

use crate::{
    settings::{parse_theme_accent, Settings, Theme},
    tab_options::Value,
    theme::{Appearance, Rgba},
};

macro_rules! keyed {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $key:literal),+ $(,)? } default $default:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $($variant),+ }

        impl Default for $name {
            fn default() -> Self {
                Self::$default
            }
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub fn key(self) -> &'static str {
                match self { $(Self::$variant => $key),+ }
            }

            pub fn parse(text: &str) -> Option<Self> {
                match text.to_ascii_lowercase().as_str() {
                    $($key => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

keyed! {
    /// `theme_schedule`: how light or dark is chosen. `system` follows the Windows app mode (what
    /// Bowser always did); `none` keeps the one look `theme_look` names; `time` switches at the two
    /// times of day.
    Schedule { None = "none", System = "system", Time = "time" } default System
}

keyed! {
    /// `theme_colouring`: which bars the accent colour tints.
    Colouring { Off = "off", TabBar = "tab_bar", AddressBar = "address_bar", Unified = "unified" } default Off
}

keyed! {
    /// `theme_contrast`: the least contrast between text and what it sits on. `standard` is WCAG AA
    /// (4.5:1 text, 3:1 for focus rings and the caret); `high` asks for 7:1 and 4.5:1 where the
    /// surface allows it, and never less than standard.
    Contrast { Standard = "standard", High = "high" } default Standard
}

impl Colouring {
    pub fn tints_tab_bar(self) -> bool {
        matches!(self, Self::TabBar | Self::Unified)
    }

    pub fn tints_address_bar(self) -> bool {
        matches!(self, Self::AddressBar | Self::Unified)
    }
}

impl Contrast {
    /// The least text contrast ratio the palette keeps.
    pub fn text_minimum(self) -> f64 {
        match self {
            Self::Standard => 4.5,
            Self::High => 7.0,
        }
    }

    /// The least contrast ratio of focus rings, the progress bar and the caret.
    pub fn non_text_minimum(self) -> f64 {
        match self {
            Self::Standard => 3.0,
            Self::High => 4.5,
        }
    }
}

/// Minutes in a day.
const DAY: u16 = 24 * 60;

/// `HH:MM` (24 hours) as minutes since midnight. Exactly five characters.
pub fn parse_clock(text: &str) -> Option<u16> {
    let bytes = text.as_bytes();
    if bytes.len() != 5 || bytes[2] != b':' || ![0, 1, 3, 4].iter().all(|&i| bytes[i].is_ascii_digit()) {
        return None;
    }
    let hours = u16::from(bytes[0] - b'0') * 10 + u16::from(bytes[1] - b'0');
    let minutes = u16::from(bytes[3] - b'0') * 10 + u16::from(bytes[4] - b'0');
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

pub fn clock_text(minutes: u16) -> String {
    format!("{:02}:{:02}", (minutes % DAY) / 60, minutes % 60)
}

/// `#rrggbb` (any case) only: exactly six hex digits after the `#`, nothing else.
pub fn parse_colour(text: &str) -> Option<Rgba> {
    let bytes = text.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' || !bytes[1..].iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let value = u32::from_str_radix(&text[1..], 16).ok()?;
    Some(Rgba::hex(value, 0xFF))
}

pub fn colour_text(colour: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b)
}

/// The settings page's theme options, as they are in force.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeOptions {
    pub schedule: Schedule,
    /// The look while `schedule` is `none`.
    pub look: Appearance,
    /// Minutes since midnight at which the light look starts and the dark look starts, while
    /// `schedule` is `time`. Never equal.
    pub light_from: u16,
    pub dark_from: u16,
    pub background: Option<Rgba>,
    pub foreground: Option<Rgba>,
    pub highlight: Option<Rgba>,
    pub colouring: Colouring,
    /// Take the accent from the page's `theme-color` meta or its favicon.
    pub accent_from_page: bool,
    /// Accents are limited to this saturation, in percent (100 leaves them as they are).
    pub saturation: u8,
    pub contrast: Contrast,
}

impl Default for ThemeOptions {
    fn default() -> Self {
        Self {
            schedule: Schedule::System,
            look: Appearance::Light,
            light_from: 7 * 60,
            dark_from: 19 * 60,
            background: None,
            foreground: None,
            highlight: None,
            colouring: Colouring::Off,
            accent_from_page: false,
            saturation: 100,
            contrast: Contrast::Standard,
        }
    }
}

pub const KEYS: [&str; 11] = [
    "theme_schedule",
    "theme_look",
    "theme_light_from",
    "theme_dark_from",
    "theme_background",
    "theme_foreground",
    "theme_highlight",
    "theme_colouring",
    "theme_accent_page",
    "theme_saturation",
    "theme_contrast",
];

/// The options a theme file carries (the schedule is a habit of the person, not of the colours).
pub const FILE_OPTION_KEYS: [&str; 7] = [
    "theme_background",
    "theme_foreground",
    "theme_highlight",
    "theme_colouring",
    "theme_accent_page",
    "theme_saturation",
    "theme_contrast",
];

fn parse_look(text: &str) -> Option<Appearance> {
    match text.to_ascii_lowercase().as_str() {
        "light" => Some(Appearance::Light),
        "dark" => Some(Appearance::Dark),
        _ => None,
    }
}

fn parse_bool(text: &str) -> Option<bool> {
    match text.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// An optional colour: empty (the theme's own) or `#rrggbb`.
fn parse_optional_colour(text: &str) -> Option<Option<Rgba>> {
    if text.is_empty() {
        return Some(None);
    }
    parse_colour(text).map(Some)
}

fn optional_colour_text(colour: Option<Rgba>) -> String {
    colour.map(colour_text).unwrap_or_default()
}

impl ThemeOptions {
    /// Sets one option from its text. `false` (and nothing changed) for an unknown key or a value
    /// the option refuses.
    pub fn set(&mut self, key: &str, text: &str) -> bool {
        match key {
            "theme_schedule" => match Schedule::parse(text) {
                Some(value) => self.schedule = value,
                None => return false,
            },
            "theme_look" => match parse_look(text) {
                Some(value) => self.look = value,
                None => return false,
            },
            "theme_light_from" => match parse_clock(text) {
                Some(value) if value != self.dark_from => self.light_from = value,
                _ => return false,
            },
            "theme_dark_from" => match parse_clock(text) {
                Some(value) if value != self.light_from => self.dark_from = value,
                _ => return false,
            },
            "theme_background" => match parse_optional_colour(text) {
                Some(value) => self.background = value,
                None => return false,
            },
            "theme_foreground" => match parse_optional_colour(text) {
                Some(value) => self.foreground = value,
                None => return false,
            },
            "theme_highlight" => match parse_optional_colour(text) {
                Some(value) => self.highlight = value,
                None => return false,
            },
            "theme_colouring" => match Colouring::parse(text) {
                Some(value) => self.colouring = value,
                None => return false,
            },
            "theme_accent_page" => match parse_bool(text) {
                Some(value) => self.accent_from_page = value,
                None => return false,
            },
            "theme_saturation" => match text.parse::<u8>() {
                Ok(value) if value <= 100 && text.bytes().all(|b| b.is_ascii_digit()) => self.saturation = value,
                _ => return false,
            },
            "theme_contrast" => match Contrast::parse(text) {
                Some(value) => self.contrast = value,
                None => return false,
            },
            _ => return false,
        }
        true
    }

    /// Every option as its file text, in [`KEYS`] order.
    pub fn pairs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("theme_schedule", self.schedule.key().to_owned()),
            ("theme_look", if self.look == Appearance::Dark { "dark" } else { "light" }.to_owned()),
            ("theme_light_from", clock_text(self.light_from)),
            ("theme_dark_from", clock_text(self.dark_from)),
            ("theme_background", optional_colour_text(self.background)),
            ("theme_foreground", optional_colour_text(self.foreground)),
            ("theme_highlight", optional_colour_text(self.highlight)),
            ("theme_colouring", self.colouring.key().to_owned()),
            ("theme_accent_page", self.accent_from_page.to_string()),
            ("theme_saturation", self.saturation.to_string()),
            ("theme_contrast", self.contrast.key().to_owned()),
        ]
    }

    /// Every option as the settings snapshot carries it.
    pub fn entries(&self) -> Vec<(&'static str, Value)> {
        self.pairs()
            .into_iter()
            .map(|(key, text)| {
                let value = match key {
                    "theme_accent_page" => Value::Bool(self.accent_from_page),
                    "theme_saturation" => Value::Uint(u64::from(self.saturation)),
                    _ => Value::Text(text),
                };
                (key, value)
            })
            .collect()
    }

    /// The light or dark look the schedule asks for at `minutes` since local midnight. `None`
    /// follows the Windows app mode.
    pub fn scheduled_look(&self, minutes: u16) -> Option<Appearance> {
        match self.schedule {
            Schedule::System => None,
            Schedule::None => Some(self.look),
            Schedule::Time => {
                let minutes = minutes % DAY;
                let light = if self.light_from < self.dark_from {
                    minutes >= self.light_from && minutes < self.dark_from
                } else {
                    minutes >= self.light_from || minutes < self.dark_from
                };
                Some(if light { Appearance::Light } else { Appearance::Dark })
            }
        }
    }
}

/// Applies one `key=value` line from the page to `settings`. `false` (and nothing changed) when the
/// key is not one of the options or the value is refused.
pub fn apply_assignment(settings: &mut Settings, text: &str) -> bool {
    let Some((key, value)) = text.split_once('=') else {
        return false;
    };
    settings.themes.set(key, value)
}

/// Whether `key=value` is a line the page may send: a known key and a value of that key's kind. The
/// two schedule times are checked against each other when they are applied, with the settings in
/// force.
pub fn accepts(text: &str) -> bool {
    let Some((key, value)) = text.split_once('=') else {
        return false;
    };
    match key {
        "theme_light_from" | "theme_dark_from" => parse_clock(value).is_some(),
        _ => ThemeOptions::default().set(key, value),
    }
}

/// Reads a `theme_*` line of `settings.txt`. A value the option refuses leaves what it was. The two
/// schedule times are read on their own, whatever order the file has them in, and checked against
/// each other once the whole file is read ([`ThemeOptions::check_schedule`]).
pub fn read_file_line(options: &mut ThemeOptions, key: &str, value: &str) {
    let value = value.trim();
    match key {
        "theme_light_from" => {
            if let Some(minutes) = parse_clock(value) {
                options.light_from = minutes;
            }
        }
        "theme_dark_from" => {
            if let Some(minutes) = parse_clock(value) {
                options.dark_from = minutes;
            }
        }
        _ => {
            options.set(key, value);
        }
    }
}

impl ThemeOptions {
    /// The check after a file is read: a schedule whose light and dark start at the same time has
    /// no light (or no dark) part, so both times go back to their defaults.
    pub fn check_schedule(&mut self) {
        if self.light_from == self.dark_from {
            let defaults = Self::default();
            self.light_from = defaults.light_from;
            self.dark_from = defaults.dark_from;
        }
    }
}

/// Returns `text` with the theme option lines replaced by `options`, keeping everything else (comments,
/// other keys, the line ending). A line already in the file is updated; one that is not is added
/// only when it differs from the default, so a profile that never touched the theme keeps a
/// settings file without theme lines.
pub fn render_into(text: &str, options: &ThemeOptions, newline: &str) -> String {
    let pairs = options.pairs();
    let defaults = ThemeOptions::default().pairs();
    let mut written = vec![false; pairs.len()];
    let mut out = String::with_capacity(text.len() + 64);
    for line in text.lines() {
        let slot = if line.trim_start().starts_with('#') {
            None
        } else {
            line.split_once('=').and_then(|(key, _)| pairs.iter().position(|(name, _)| *name == key.trim()))
        };
        match slot {
            Some(index) if written[index] => {}
            Some(index) => {
                written[index] = true;
                out.push_str(&format!("{}={}{newline}", pairs[index].0, pairs[index].1));
            }
            None => {
                out.push_str(line);
                out.push_str(newline);
            }
        }
    }
    for (index, (key, value)) in pairs.iter().enumerate() {
        if !written[index] && *value != defaults[index].1 {
            out.push_str(&format!("{key}={value}{newline}"));
        }
    }
    out
}

/// Puts the theme back as a fresh profile has it: the Helium palette, no accent, and every option at
/// its default. The website appearance and the private tab theme are not part of it.
pub fn reset(settings: &mut Settings) {
    settings.theme = Theme::Helium;
    settings.theme_accent = None;
    settings.themes = ThemeOptions::default();
}

// ---- the accent ----

/// The theme the chrome is drawn with: what the settings choose, with the accent already resolved.
/// `page_accent` is the colour sampled from the active page (used only while the option is on).
pub fn user_theme(settings: &Settings, system_accent: Option<Rgba>, page_accent: Option<Rgba>) -> crate::theme::UserTheme {
    let options = &settings.themes;
    let page = if options.accent_from_page { page_accent } else { None };
    let accent_from_page = page.is_some();
    let accent = page
        .or_else(|| crate::theme::resolved_accent(settings.theme, settings.theme_accent.as_deref(), system_accent))
        .map(|accent| crate::theme::limit_saturation(accent, options.saturation));
    crate::theme::UserTheme {
        accent,
        background: options.background,
        foreground: options.foreground,
        highlight: options.highlight,
        colouring: options.colouring,
        contrast: options.contrast,
        accent_from_page,
    }
}

/// Reads the answer of the page-colour script: the page's `theme-color` as the canvas normalises it
/// (`#rrggbb` or `rgba(r, g, b, a)`), JSON-quoted. `None` for no colour, an unreadable answer or a
/// colour that is mostly transparent.
pub fn parse_page_colour(answer: &str) -> Option<Rgba> {
    let text = answer.trim().trim_matches('"').trim();
    if text.is_empty() || text.len() > 40 {
        return None;
    }
    // The script starts the canvas at this colour, so it comes back unchanged for a colour the
    // canvas could not read.
    const UNSET: &str = "#010203";
    if text.eq_ignore_ascii_case(UNSET) {
        return None;
    }
    if let Some(colour) = parse_colour(text) {
        return Some(colour);
    }
    let inner = text.strip_prefix("rgba(")?.strip_suffix(')')?;
    let mut parts = inner.split(',').map(str::trim);
    let mut channel = || parts.next()?.parse::<u8>().ok();
    let (r, g, b) = (channel()?, channel()?, channel()?);
    let alpha = parts.next()?.parse::<f32>().ok()?;
    if parts.next().is_some() || !(0.9..=1.0).contains(&alpha) {
        return None;
    }
    Some(Rgba { r, g, b, a: 0xFF })
}

/// The script that reads the page's `theme-color` (the first `meta` whose media query matches) in
/// the page's own terms and lets the page's canvas normalise it. It reads one attribute: no
/// network, no layout, nothing written to the page.
pub const PAGE_COLOUR_SCRIPT: &str = "(()=>{try{const ms=Array.from(document.querySelectorAll('meta[name=\"theme-color\"]'));const m=ms.find(x=>!x.media||matchMedia(x.media).matches);if(!m||!m.content)return'';const c=document.createElement('canvas').getContext('2d');c.fillStyle='#010203';c.fillStyle=m.content;return c.fillStyle}catch(e){return''}})()";

// ---- the theme file ----

/// Largest theme file read. A real one is a few hundred bytes.
pub const MAX_THEME_FILE_BYTES: usize = 2048;
const MAX_THEME_FILE_LINES: usize = 40;
const MAX_THEME_FILE_LINE: usize = 100;
pub const FILE_EXTENSION: &str = "bowsertheme";
pub const FILE_NAME: &str = "bowser.bowsertheme";
const FILE_VERSION_KEY: &str = "bowser_theme";
const FILE_VERSION: &str = "1";

/// Why a theme file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileError {
    TooLarge,
    NotText,
    NotATheme,
    /// A line that is not `key=value`, a key a theme file does not have, or a key twice.
    BadLine(usize),
    /// A value the key refuses.
    BadValue(&'static str),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => f.write_str("That file is too big to be a theme file."),
            Self::NotText => f.write_str("That file is not text, so it is not a theme file."),
            Self::NotATheme => f.write_str("That is not a Bowser theme file."),
            Self::BadLine(line) => write!(f, "Line {line} of the theme file is not allowed."),
            Self::BadValue(key) => write!(f, "The theme file has a value Bowser does not accept for {key}."),
        }
    }
}

/// A theme file's content, checked: nothing in it is applied until the caller does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeFile {
    pub theme: Theme,
    pub accent: Option<String>,
    pub options: ThemeOptions,
}

/// The text of the theme file for `settings`: the colours and where they apply, no schedule.
pub fn export_text(settings: &Settings) -> String {
    let mut out = String::from("# Bowser theme file. Import it from Settings > Appearance > Theme file.\r\n");
    out.push_str(&format!("{FILE_VERSION_KEY}={FILE_VERSION}\r\n"));
    out.push_str(&format!("theme={}\r\n", settings.theme.key()));
    out.push_str(&format!("theme_accent={}\r\n", settings.theme_accent.as_deref().unwrap_or("")));
    let pairs = settings.themes.pairs();
    for key in FILE_OPTION_KEYS {
        if let Some((_, value)) = pairs.iter().find(|(name, _)| *name == key) {
            out.push_str(&format!("{key}={value}\r\n"));
        }
    }
    out
}

/// Checks `bytes` as an imported theme file. Untrusted input: a size cap, UTF-8 text with no control
/// characters, one `key=value` per line from a fixed list (each at most once, the version first),
/// every value parsed by the same rules as a typed one. A key that is missing keeps its default;
/// anything else is refused whole.
pub fn parse_theme_file(bytes: &[u8]) -> Result<ThemeFile, FileError> {
    if bytes.len() > MAX_THEME_FILE_BYTES {
        return Err(FileError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| FileError::NotText)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.chars().any(|c| c.is_control() && !matches!(c, '\r' | '\n')) {
        return Err(FileError::NotText);
    }
    let mut file = ThemeFile { theme: Theme::Helium, accent: None, options: ThemeOptions::default() };
    let mut seen: Vec<&str> = Vec::new();
    let mut versioned = false;
    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        if number > MAX_THEME_FILE_LINES || raw.len() > MAX_THEME_FILE_LINE {
            return Err(FileError::BadLine(number));
        }
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(FileError::BadLine(number));
        };
        let (key, value) = (key.trim(), value.trim());
        if seen.contains(&key) {
            return Err(FileError::BadLine(number));
        }
        if key == FILE_VERSION_KEY {
            if !seen.is_empty() || value != FILE_VERSION {
                return Err(FileError::NotATheme);
            }
            versioned = true;
            seen.push(key);
            continue;
        }
        if !versioned {
            return Err(FileError::NotATheme);
        }
        match key {
            "theme" => file.theme = Theme::parse(value).ok_or(FileError::BadValue("theme"))?,
            "theme_accent" => file.accent = parse_theme_accent(value).ok_or(FileError::BadValue("theme_accent"))?,
            _ => {
                let Some(&known) = FILE_OPTION_KEYS.iter().find(|name| **name == key) else {
                    return Err(FileError::BadLine(number));
                };
                if !file.options.set(known, value) {
                    return Err(FileError::BadValue(known));
                }
            }
        }
        seen.push(key);
    }
    if !versioned {
        return Err(FileError::NotATheme);
    }
    Ok(file)
}

/// Puts an imported theme into `settings`: the file's theme, accent and colour options replace the
/// current ones (a key the file leaves out goes back to its default); the schedule stays.
pub fn apply_theme_file(settings: &mut Settings, file: &ThemeFile) {
    settings.theme = file.theme;
    settings.theme_accent = file.accent.clone();
    let schedule = (
        settings.themes.schedule,
        settings.themes.look,
        settings.themes.light_from,
        settings.themes.dark_from,
    );
    settings.themes = file.options.clone();
    (settings.themes.schedule, settings.themes.look, settings.themes.light_from, settings.themes.dark_from) = schedule;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_times_parse_strictly() {
        assert_eq!(parse_clock("07:00"), Some(420));
        assert_eq!(parse_clock("23:59"), Some(23 * 60 + 59));
        for bad in ["7:00", "24:00", "12:60", "12-00", "1200", "ab:cd", "+1:00", "12:0", "12:000", ""] {
            assert_eq!(parse_clock(bad), None, "{bad}");
        }
        assert_eq!(clock_text(420), "07:00");
    }

    /// Reads `lines` the way `settings.txt` is read: each line, then the one check after the file.
    fn read_lines(lines: &[(&str, &str)]) -> ThemeOptions {
        let mut options = ThemeOptions::default();
        for (key, value) in lines {
            read_file_line(&mut options, key, value);
        }
        options.check_schedule();
        options
    }

    #[test]
    fn schedule_times_read_in_either_order() {
        // Light from 19:00 equals the default dark start; dark from 06:00 equals neither default.
        let pair = [("theme_light_from", "19:00"), ("theme_dark_from", "06:00")];
        let reversed = [pair[1], pair[0]];
        for lines in [&pair[..], &reversed[..]] {
            let options = read_lines(lines);
            assert_eq!((options.light_from, options.dark_from), (19 * 60, 6 * 60), "{lines:?}");
        }
        // Dark from 07:00 equals the default light start.
        let options = read_lines(&[("theme_dark_from", "07:00"), ("theme_light_from", "20:00")]);
        assert_eq!((options.light_from, options.dark_from), (20 * 60, 7 * 60));
    }

    #[test]
    fn equal_schedule_times_in_the_file_give_the_defaults() {
        let defaults = ThemeOptions::default();
        let options = read_lines(&[("theme_light_from", "08:00"), ("theme_dark_from", "08:00")]);
        assert_eq!((options.light_from, options.dark_from), (defaults.light_from, defaults.dark_from));
        // A bad time keeps its default and is then checked against the other one.
        let options = read_lines(&[("theme_light_from", "25:00"), ("theme_dark_from", "07:00")]);
        assert_eq!((options.light_from, options.dark_from), (defaults.light_from, defaults.dark_from));
    }

    #[test]
    fn colours_are_exactly_hash_and_six_hex_digits() {
        assert_eq!(parse_colour("#1A2b3c"), Some(Rgba::hex(0x1a2b3c, 0xFF)));
        for bad in ["1a2b3c", "#1a2b3", "#1a2b3cd", "", "#gggggg", "#+a+b+c", "red", "#1a2b3c ", "rgb(1,2,3)"] {
            assert_eq!(parse_colour(bad), None, "{bad}");
        }
    }

    #[test]
    fn options_round_trip_through_their_text() {
        let mut options = ThemeOptions::default();
        for (key, value) in [
            ("theme_schedule", "time"),
            ("theme_look", "dark"),
            ("theme_light_from", "06:30"),
            ("theme_dark_from", "21:15"),
            ("theme_background", "#102030"),
            ("theme_foreground", "#f0f0f0"),
            ("theme_highlight", "#336699"),
            ("theme_colouring", "unified"),
            ("theme_accent_page", "true"),
            ("theme_saturation", "40"),
            ("theme_contrast", "high"),
        ] {
            assert!(options.set(key, value), "{key}");
        }
        let mut again = ThemeOptions::default();
        for (key, text) in options.pairs() {
            assert!(again.set(key, &text), "{key}={text}");
        }
        assert_eq!(again, options);
        assert_eq!(options.pairs().iter().map(|(key, _)| *key).collect::<Vec<_>>(), KEYS.to_vec());
    }

    #[test]
    fn options_refuse_what_they_cannot_hold_and_keep_their_value() {
        let mut options = ThemeOptions::default();
        let before = options.clone();
        for (key, value) in [
            ("theme_schedule", "sometimes"),
            ("theme_look", "grey"),
            ("theme_light_from", "25:00"),
            ("theme_dark_from", "07:00"), // equal to the light start
            ("theme_background", "red"),
            ("theme_foreground", "#12345"),
            ("theme_highlight", "#12345g"),
            ("theme_colouring", "rainbow"),
            ("theme_accent_page", "maybe"),
            ("theme_saturation", "101"),
            ("theme_saturation", "-1"),
            ("theme_saturation", "+5"),
            ("theme_saturation", ""),
            ("theme_contrast", "extreme"),
            ("theme_nothing", "1"),
        ] {
            assert!(!options.set(key, value), "{key}={value}");
        }
        assert_eq!(options, before);
    }

    #[test]
    fn the_schedule_picks_the_look() {
        let mut options = ThemeOptions::default();
        assert_eq!(options.scheduled_look(0), None, "system follows Windows");
        options.schedule = Schedule::None;
        options.look = Appearance::Dark;
        assert_eq!(options.scheduled_look(12 * 60), Some(Appearance::Dark), "none keeps the chosen look");
        options.schedule = Schedule::Time;
        assert_eq!(options.scheduled_look(6 * 60 + 59), Some(Appearance::Dark));
        assert_eq!(options.scheduled_look(7 * 60), Some(Appearance::Light));
        assert_eq!(options.scheduled_look(18 * 60 + 59), Some(Appearance::Light));
        assert_eq!(options.scheduled_look(19 * 60), Some(Appearance::Dark));
        assert_eq!(options.scheduled_look(23 * 60 + 59), Some(Appearance::Dark));
        assert_eq!(options.scheduled_look(0), Some(Appearance::Dark));
        // Light starting after dark wraps midnight.
        options.light_from = 22 * 60;
        options.dark_from = 5 * 60;
        assert_eq!(options.scheduled_look(23 * 60), Some(Appearance::Light));
        assert_eq!(options.scheduled_look(2 * 60), Some(Appearance::Light));
        assert_eq!(options.scheduled_look(12 * 60), Some(Appearance::Dark));
    }

    #[test]
    fn the_file_keeps_theme_lines_apart_from_other_lines_and_defaults_stay_out() {
        let mut settings = Settings::default();
        assert_eq!(render_into("# kept\nother=1\n", &settings.themes, "\n"), "# kept\nother=1\n", "defaults write nothing");
        settings.themes.set("theme_colouring", "tab_bar");
        let written = render_into("# kept\nother=1\n", &settings.themes, "\n");
        assert_eq!(written, "# kept\nother=1\ntheme_colouring=tab_bar\n");
        assert_eq!(render_into(&written, &settings.themes, "\n"), written, "saving again repeats nothing");
        settings.themes.set("theme_colouring", "off");
        assert_eq!(render_into(&written, &settings.themes, "\n"), "# kept\nother=1\ntheme_colouring=off\n");
    }

    #[test]
    fn a_theme_exports_and_imports_back_without_the_schedule() {
        let mut settings = Settings::default();
        settings.theme = Theme::System;
        settings.theme_accent = Some("#aa5500".to_owned());
        for (key, value) in [
            ("theme_background", "#202830"),
            ("theme_foreground", "#eeeeee"),
            ("theme_highlight", "#3366aa"),
            ("theme_colouring", "address_bar"),
            ("theme_accent_page", "true"),
            ("theme_saturation", "55"),
            ("theme_contrast", "high"),
            ("theme_schedule", "time"),
            ("theme_dark_from", "20:00"),
        ] {
            assert!(settings.themes.set(key, value));
        }
        let text = export_text(&settings);
        assert!(text.len() < MAX_THEME_FILE_BYTES);
        assert!(!text.contains("theme_schedule") && !text.contains("theme_dark_from"), "the schedule is not part of a theme");
        let file = parse_theme_file(text.as_bytes()).expect("own export parses");
        let mut other = Settings::default();
        other.themes.set("theme_schedule", "none");
        apply_theme_file(&mut other, &file);
        assert_eq!(other.theme, Theme::System);
        assert_eq!(other.theme_accent.as_deref(), Some("#aa5500"));
        assert_eq!(other.themes.background, settings.themes.background);
        assert_eq!(other.themes.colouring, Colouring::AddressBar);
        assert_eq!(other.themes.saturation, 55);
        assert_eq!(other.themes.schedule, Schedule::None, "the importing profile keeps its own schedule");
    }

    #[test]
    fn an_import_replaces_the_whole_theme_and_a_missing_key_is_the_default() {
        let mut settings = Settings::default();
        settings.themes.set("theme_foreground", "#ffffff");
        settings.themes.set("theme_saturation", "10");
        let file = parse_theme_file(b"bowser_theme=1\ntheme_background=#101010\n").unwrap();
        apply_theme_file(&mut settings, &file);
        assert_eq!(settings.themes.background, Some(Rgba::hex(0x101010, 0xFF)));
        assert_eq!(settings.themes.foreground, None);
        assert_eq!(settings.themes.saturation, 100);
    }

    #[test]
    fn an_untrusted_theme_file_is_checked_strictly() {
        let ok = "bowser_theme=1\ntheme=helium\ntheme_accent=#112233\n";
        assert!(parse_theme_file(ok.as_bytes()).is_ok());
        assert!(parse_theme_file(format!("\u{feff}# hi\r\n{ok}").as_bytes()).is_ok(), "a BOM, comments and CRLF are fine");

        assert_eq!(parse_theme_file(&vec![b'#'; MAX_THEME_FILE_BYTES + 1]), Err(FileError::TooLarge));
        assert_eq!(parse_theme_file(&[0xff, 0xfe, 0x00, 0x41]), Err(FileError::NotText));
        assert_eq!(parse_theme_file(b"bowser_theme=1\ntheme=helium\0\n"), Err(FileError::NotText), "no control characters");
        assert_eq!(parse_theme_file(b""), Err(FileError::NotATheme), "no version line");
        assert_eq!(parse_theme_file(b"theme=helium\n"), Err(FileError::NotATheme), "the version comes first");
        assert_eq!(parse_theme_file(b"bowser_theme=2\n"), Err(FileError::NotATheme));
        assert_eq!(parse_theme_file(b"bowser_theme=1\nbowser_theme=1\n"), Err(FileError::BadLine(2)), "a key twice");
        assert_eq!(parse_theme_file(b"bowser_theme=1\ntheme=helium\ntheme=system\n"), Err(FileError::BadLine(3)));
        assert_eq!(parse_theme_file(b"bowser_theme=1\nnot a line\n"), Err(FileError::BadLine(2)));
        // Only the theme's own keys: not settings, not the schedule, not paths or scripts.
        for key in ["download_dir", "homepage", "theme_schedule", "theme_look", "start_widget_color", "blocker_enabled", "../../x", "ui_font"] {
            assert_eq!(parse_theme_file(format!("bowser_theme=1\n{key}=C:\\Windows\n").as_bytes()), Err(FileError::BadLine(2)), "{key}");
        }
        for (key, value) in [
            ("theme", "<script>"),
            ("theme_accent", "javascript:alert(1)"),
            ("theme_accent", "#12345"),
            ("theme_background", "url(http://evil.example/x)"),
            ("theme_foreground", "#zzzzzz"),
            ("theme_colouring", "../../etc"),
            ("theme_saturation", "9999"),
            ("theme_contrast", ""),
        ] {
            assert!(matches!(parse_theme_file(format!("bowser_theme=1\n{key}={value}\n").as_bytes()), Err(FileError::BadValue(_))), "{key}={value}");
        }
        assert_eq!(parse_theme_file(format!("bowser_theme=1\n{}\n", "#".repeat(MAX_THEME_FILE_LINE + 1)).as_bytes()), Err(FileError::BadLine(2)), "a long comment line");
        let many = format!("bowser_theme=1\n{}", "# x\n".repeat(MAX_THEME_FILE_LINES));
        assert_eq!(parse_theme_file(many.as_bytes()), Err(FileError::BadLine(MAX_THEME_FILE_LINES + 1)));
    }

    #[test]
    fn reset_puts_the_whole_theme_back() {
        let mut settings = Settings::default();
        settings.theme = Theme::System;
        settings.theme_accent = Some("#112233".to_owned());
        settings.themes.set("theme_schedule", "time");
        settings.themes.set("theme_background", "#000000");
        settings.website_appearance = crate::settings::WebsiteAppearance::Dark;
        reset(&mut settings);
        assert_eq!(settings.theme, Theme::Helium);
        assert_eq!(settings.theme_accent, None);
        assert_eq!(settings.themes, ThemeOptions::default());
        assert_eq!(settings.website_appearance, crate::settings::WebsiteAppearance::Dark, "website appearance is not a colour of the theme");
    }

    #[test]
    fn the_page_colour_answer_is_read_as_a_colour_or_nothing() {
        assert_eq!(parse_page_colour("\"#1a73e8\""), Some(Rgba::hex(0x1a73e8, 0xFF)));
        assert_eq!(parse_page_colour("\"rgba(26, 115, 232, 1)\""), Some(Rgba::hex(0x1a73e8, 0xFF)));
        assert_eq!(parse_page_colour("\"rgba(26, 115, 232, 0.95)\""), Some(Rgba::hex(0x1a73e8, 0xFF)));
        for bad in ["\"\"", "", "\"#010203\"", "\"rgba(1, 2, 3, 0.2)\"", "\"red\"", "\"rgb(1,2,3)\"", "\"#12345\"", "null", "\"rgba(300, 1, 1, 1)\"",
            "\"javascript:alert(1)\"", &format!("\"{}\"", "a".repeat(100))] {
            assert_eq!(parse_page_colour(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_user_theme_resolves_the_accent_in_order_and_limits_its_saturation() {
        let mut settings = Settings::default();
        let system = Some(Rgba::hex(0x00FF00, 0xFF));
        let page = Some(Rgba::hex(0x0000FF, 0xFF));
        assert_eq!(user_theme(&settings, system, page).accent, None, "helium with nothing chosen keeps the curated colours");
        settings.theme = Theme::System;
        assert_eq!(user_theme(&settings, system, page).accent, system);
        settings.theme_accent = Some("#ff0000".to_owned());
        assert_eq!(user_theme(&settings, system, page).accent, Some(Rgba::hex(0xFF0000, 0xFF)), "an explicit accent wins over Windows");
        settings.themes.accent_from_page = true;
        assert_eq!(user_theme(&settings, system, page).accent, page, "the page's colour wins while the option is on");
        assert_eq!(user_theme(&settings, system, None).accent, Some(Rgba::hex(0xFF0000, 0xFF)), "a page without a colour falls back");
        settings.themes.saturation = 0;
        let grey = user_theme(&settings, system, page).accent.unwrap();
        assert!(grey.r == grey.g && grey.g == grey.b, "saturation 0 is grey: {grey:?}");
    }
    /// Regression (v0.0.19 "the colours have been desaturated"): the settings Bowser had before the
    /// theme options (nothing, `theme=system`, `theme_accent`) draw exactly v0.0.18's palettes. A picked
    /// accent alone was pushed towards white or black to reach 3:1 over every glass backdrop, so
    /// #bf2b2b showed as #e5aaaa on dark glass.
    #[test]
    fn the_settings_v0_0_18_had_draw_v0_0_18s_palettes() {
        use crate::theme::{palette_base, themed, with_accent, Appearance};
        const VARIANTS: [(Appearance, bool); 4] = [(Appearance::Light, false), (Appearance::Dark, false), (Appearance::Light, true), (Appearance::Dark, true)];
        // v0.0.18's own colours (src/theme.rs at tag v0.0.18): window, focus ring, progress bar.
        let v0_0_18 = |appearance: Appearance, solid: bool| match (appearance, solid) {
            (Appearance::Light, false) => (Rgba::hex(0xF4F6F8, 0xCC), Rgba::hex(0x4A6D73, 0xFF), Rgba::hex(0x0067C0, 0xFF)),
            (Appearance::Dark, false) => (Rgba::hex(0x20242C, 0xCC), Rgba::hex(0xB3C9CE, 0xFF), Rgba::hex(0x84C8FF, 0xFF)),
            (Appearance::Light, true) => (Rgba::hex(0xF4F6F8, 0xFF), Rgba::hex(0x008A94, 0xFF), Rgba::hex(0x0067C0, 0xFF)),
            (Appearance::Dark, true) => (Rgba::hex(0x20242C, 0xFF), Rgba::hex(0x5FD8DB, 0xFF), Rgba::hex(0x6CB4FF, 0xFF)),
        };
        let windows_accent = Some(Rgba::hex(0x0078D7, 0xFF));

        // An empty settings file: nothing chosen, Bowser's own palettes, untouched.
        let settings = Settings::default();
        let plain = user_theme(&settings, windows_accent, Some(Rgba::hex(0x00FF00, 0xFF)));
        assert!(plain.is_plain(), "{plain:?}");
        for (appearance, solid) in VARIANTS {
            let (p, look) = themed(appearance, solid, &plain);
            assert_eq!(&p, palette_base(appearance, solid), "{appearance}/{solid}");
            assert_eq!(look, appearance);
            assert_eq!((p.window, p.omnibox_focus_border, p.progress_fill), v0_0_18(appearance, solid), "{appearance}/{solid}");
        }

        // `theme=system` and `theme_accent`: the accent is used exactly as chosen, nothing else moves.
        let red = Rgba::hex(0xBF2B2B, 0xFF);
        let mut system = Settings::default();
        system.theme = Theme::System;
        let mut picked = Settings::default();
        picked.theme_accent = Some("#bf2b2b".to_owned());
        for settings in [&system, &picked] {
            let user = user_theme(settings, Some(red), None);
            assert!(user.is_chosen_accent_only(), "{user:?}");
            for (appearance, solid) in VARIANTS {
                let (p, look) = themed(appearance, solid, &user);
                assert_eq!(p, with_accent(palette_base(appearance, solid), Some(red)), "{appearance}/{solid}");
                assert_eq!((p.omnibox_focus_border, p.progress_fill), (red, red), "{appearance}/{solid}: not washed out");
                assert_eq!(look, appearance);
            }
        }

        // The options still act: the saturation limit greys the picked accent on purpose...
        picked.themes.saturation = 40;
        let limited = user_theme(&picked, None, None);
        let limited_red = crate::theme::limit_saturation(red, 40);
        assert_eq!(limited.accent, Some(limited_red));
        assert_eq!(themed(Appearance::Dark, false, &limited).0.progress_fill, limited_red);
        // ...and a page's colour (it can be anything) is still kept visible over every backdrop.
        picked.themes.saturation = 100;
        picked.themes.accent_from_page = true;
        let page = user_theme(&picked, None, Some(Rgba::hex(0x101010, 0xFF)));
        assert!(page.accent_from_page && !page.is_chosen_accent_only());
        let p = themed(Appearance::Dark, false, &page).0;
        assert_ne!(p.progress_fill, Rgba::hex(0x101010, 0xFF), "a near-black page colour on dark glass is lifted");
        assert!(crate::theme::worst_contrast(p.progress_fill, &[vec![p.window], vec![p.window, p.omnibox_fill]]) >= 3.0);
    }
}
