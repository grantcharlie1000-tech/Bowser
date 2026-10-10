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

use std::{fs::File, io::Read, path::Path};

pub const FILE_NAME: &str = "settings.txt";
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
}
