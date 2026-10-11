//! User-editable settings read once at startup from `%LOCALAPPDATA%\Bowser\settings.txt`.
//!
//! The file is `key=value` per line. Blank lines and lines starting with `#` are ignored, as are
//! unknown keys and values that do not parse, so a typo never prevents startup and never
//! changes any other setting. Values are clamped to a safe range.
//!
//! ```text
//! sleep_enabled=true
//! sleep_after_secs=300
//! blocker_enabled=true
//! blocker_allow=example.com,other.org
//! ```

use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};

pub const FILE_NAME: &str = "settings.txt";
/// Starter file written by "Open settings file" when none exists. Every setting is commented out.
pub const TEMPLATE: &str = "# Bowser settings. Restart Bowser after editing.\r\n\
    # sleep_enabled=true\r\n\
    # sleep_after_secs=300\r\n\
    # blocker_enabled=true\r\n\
    # blocker_allow=example.com,other.org\r\n";
/// Largest settings file that will be read; a larger file is ignored entirely.
pub const MAX_FILE_BYTES: usize = 16 * 1024;
pub const DEFAULT_SLEEP_AFTER_SECS: u64 = 300;
/// Hidden tabs are suspended after 30 s, so sleeping earlier would never be eligible.
pub const MIN_SLEEP_AFTER_SECS: u64 = 30;
pub const MAX_SLEEP_AFTER_SECS: u64 = 7 * 24 * 60 * 60;

/// Most `blocker_allow` entries kept; the rest of a longer list is ignored.
pub const MAX_BLOCKER_ALLOW: usize = 500;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settings {
    /// Whether hidden tabs sleep (are discarded) after `sleep_after_secs` even without memory
    /// pressure. Under memory pressure the long-hidden tabs are discarded regardless.
    pub sleep_enabled: bool,
    pub sleep_after_secs: u64,
    /// `blocker_enabled`: master switch for the ad/tracker blocker (default on).
    pub blocker_enabled: bool,
    /// `blocker_allow=example.com,other.org`: sites where the blocker never blocks.
    pub blocker_allow: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sleep_enabled: true,
            sleep_after_secs: DEFAULT_SLEEP_AFTER_SECS,
            blocker_enabled: true,
            blocker_allow: Vec::new(),
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

/// Returns `existing` with the four setting lines replaced by `settings`. Comments, unknown keys
/// and the file's line endings are kept. A setting missing from the file is appended, and a
/// repeated setting keeps only its first line, since `parse` reads the last value anyway.
pub fn render_updated(existing: &str, settings: &Settings) -> String {
    let newline = if existing.contains("\r\n") { "\r\n" } else { "\n" };
    let values = [
        ("sleep_enabled", bool_text(settings.sleep_enabled).to_owned()),
        ("sleep_after_secs", settings.sleep_after_secs.to_string()),
        ("blocker_enabled", bool_text(settings.blocker_enabled).to_owned()),
        ("blocker_allow", settings.blocker_allow.join(",")),
    ];
    let mut written = [false; 4];
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

    fn sample() -> Settings {
        Settings {
            sleep_enabled: false,
            sleep_after_secs: 900,
            blocker_enabled: false,
            blocker_allow: vec!["a.com".to_owned(), "b.org".to_owned()],
        }
    }

    #[test]
    fn render_into_empty_text_roundtrips() {
        let text = render_updated("", &sample());
        assert_eq!(Settings::parse(&text), sample());
        assert!(text.contains("sleep_after_secs=900\n"));
    }

    #[test]
    fn render_keeps_comments_unknown_keys_and_crlf() {
        let existing = "# my notes\r\nfoo=bar\r\nsleep_enabled=true\r\n# blocker_allow=old.com\r\n";
        let text = render_updated(existing, &sample());
        assert_eq!(
            text,
            "# my notes\r\nfoo=bar\r\nsleep_enabled=false\r\n# blocker_allow=old.com\r\n\
             sleep_after_secs=900\r\nblocker_enabled=false\r\nblocker_allow=a.com,b.org\r\n"
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
        assert!(std::fs::read_to_string(&path).unwrap().starts_with("# keep me\nfoo=1\n"));
        let fresh = dir.join("fresh.txt");
        save(&fresh, &Settings::default()).unwrap();
        assert_eq!(Settings::load(&fresh), Settings::default());
        let leftovers = std::fs::read_dir(&dir)
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().file_name().to_string_lossy().contains(".tmp"))
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
        assert_eq!(read_strict(&dir.join("none.txt")).unwrap(), Settings::default());
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
        assert_eq!(parse_sleep_after_secs(&MAX_SLEEP_AFTER_SECS.to_string()), Ok(MAX_SLEEP_AFTER_SECS));
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
