//! Per-web-panel options (thread T9), kept in the profile's `panel_prefs.txt`: a zoom, a mute, a
//! reload interval, a width and the title-notification switch for each pinned web app, plus the
//! web apps removed from the rail so they can be restored. They live on each panel's own rail menu,
//! not on the settings page.
//!
//! ```text
//! app=3 zoom=125 muted=true reload=10 width=420 notify=false
//! removed=Mail<TAB>https://mail.example/
//! ```
//! A default value is not written. Unknown or malformed lines are dropped.

use std::{collections::BTreeMap, fs, io, path::Path, time::Duration};

pub const FILE_NAME: &str = "panel_prefs.txt";
const MAX_BYTES: usize = 64 * 1024;
const MAX_APPS: usize = 128;
/// How many removed web apps are kept for "Restore".
pub const MAX_REMOVED: usize = 12;
/// The reload intervals the menu offers, in minutes (0 is off). Any value 1 to 30 is accepted from the file.
pub const RELOAD_CHOICES: [u8; 7] = [0, 1, 2, 5, 10, 15, 30];
pub const MAX_RELOAD_MINUTES: u8 = 30;
pub const MIN_WIDTH: u16 = 280;
pub const MAX_WIDTH: u16 = 520;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PanelPrefs {
    /// Zoom in percent (`site_prefs` range).
    pub zoom: u16,
    pub muted: bool,
    /// Minutes between reloads while the panel's page is loaded; 0 is never.
    pub reload_minutes: u8,
    /// A logical (96 dpi) width of its own; `None` uses the shared side panel width.
    pub width: Option<u16>,
    /// A count in the page title, such as "(3) Inbox", marks the panel's rail button.
    pub notify: bool,
}

impl Default for PanelPrefs {
    fn default() -> Self {
        Self { zoom: crate::site_prefs::ZOOM_DEFAULT, muted: false, reload_minutes: 0, width: None, notify: true }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Removed {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PanelPrefsFile {
    apps: BTreeMap<u32, PanelPrefs>,
    pub removed: Vec<Removed>,
}

fn clean(text: &str) -> bool {
    !text.is_empty() && text.len() <= 2048 && !text.chars().any(|c| c.is_control())
}

impl PanelPrefsFile {
    pub fn get(&self, app: u32) -> PanelPrefs {
        self.apps.get(&app).copied().unwrap_or_default()
    }

    pub fn set(&mut self, app: u32, prefs: PanelPrefs) {
        if prefs == PanelPrefs::default() {
            self.apps.remove(&app);
        } else if self.apps.contains_key(&app) || self.apps.len() < MAX_APPS {
            self.apps.insert(app, prefs);
        }
    }

    /// Forgets an app's options (it was removed for good).
    pub fn forget(&mut self, app: u32) {
        self.apps.remove(&app);
    }

    /// Remembers a removed web app, newest first, one entry per address.
    pub fn remember_removed(&mut self, name: &str, url: &str) {
        if !clean(name) || !clean(url) {
            return;
        }
        self.removed.retain(|entry| !entry.url.eq_ignore_ascii_case(url));
        self.removed.insert(0, Removed { name: name.to_owned(), url: url.to_owned() });
        self.removed.truncate(MAX_REMOVED);
    }

    /// Takes the removed app at `index` out of the list (it is being restored).
    pub fn take_removed(&mut self, index: usize) -> Option<Removed> {
        (index < self.removed.len()).then(|| self.removed.remove(index))
    }

    pub fn parse(text: &str) -> Self {
        let mut file = Self::default();
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if let Some(rest) = line.strip_prefix("removed=") {
                if let Some((name, url)) = rest.split_once('\t') {
                    if clean(name) && clean(url) && file.removed.len() < MAX_REMOVED {
                        file.removed.push(Removed { name: name.to_owned(), url: url.to_owned() });
                    }
                }
                continue;
            }
            let Some(rest) = line.strip_prefix("app=") else { continue };
            let mut parts = rest.split_whitespace();
            let Some(id) = parts.next().and_then(|id| id.parse::<u32>().ok()) else { continue };
            let mut prefs = PanelPrefs::default();
            for part in parts {
                let Some((key, value)) = part.split_once('=') else { continue };
                match key {
                    "zoom" => {
                        if let Ok(zoom) = value.parse::<u16>() {
                            if (crate::site_prefs::ZOOM_MIN..=crate::site_prefs::ZOOM_MAX).contains(&zoom) {
                                prefs.zoom = zoom;
                            }
                        }
                    }
                    "muted" => prefs.muted = value == "true",
                    "reload" => {
                        if let Ok(minutes) = value.parse::<u8>() {
                            if minutes <= MAX_RELOAD_MINUTES {
                                prefs.reload_minutes = minutes;
                            }
                        }
                    }
                    "width" => {
                        if let Ok(width) = value.parse::<u16>() {
                            if (MIN_WIDTH..=MAX_WIDTH).contains(&width) {
                                prefs.width = Some(width);
                            }
                        }
                    }
                    "notify" => prefs.notify = value != "false",
                    _ => {}
                }
            }
            file.set(id, prefs);
        }
        file
    }

    pub fn serialize(&self) -> String {
        let mut out = String::new();
        for (id, prefs) in &self.apps {
            out.push_str(&format!("app={id}"));
            let defaults = PanelPrefs::default();
            if prefs.zoom != defaults.zoom {
                out.push_str(&format!(" zoom={}", prefs.zoom));
            }
            if prefs.muted {
                out.push_str(" muted=true");
            }
            if prefs.reload_minutes != 0 {
                out.push_str(&format!(" reload={}", prefs.reload_minutes));
            }
            if let Some(width) = prefs.width {
                out.push_str(&format!(" width={width}"));
            }
            if !prefs.notify {
                out.push_str(" notify=false");
            }
            out.push('\n');
        }
        for entry in &self.removed {
            out.push_str(&format!("removed={}\t{}\n", entry.name, entry.url));
        }
        out
    }

    /// Whether any panel reloads on a timer.
    pub fn any_reload(&self) -> bool {
        self.apps.values().any(|prefs| prefs.reload_minutes > 0)
    }

    pub fn load(profile_dir: &Path) -> Self {
        match fs::read(profile_dir.join(FILE_NAME)) {
            Ok(bytes) if bytes.len() <= MAX_BYTES => Self::parse(&String::from_utf8_lossy(&bytes)),
            _ => Self::default(),
        }
    }

    pub fn save(&self, profile_dir: &Path) -> io::Result<()> {
        let path = profile_dir.join(FILE_NAME);
        let text = self.serialize();
        if text.is_empty() {
            return match fs::remove_file(&path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            };
        }
        let temp = path.with_extension("tmp");
        fs::write(&temp, text)?;
        fs::rename(&temp, &path)
    }
}

/// Runs `edit` on the active profile's panel options and saves the result (a guest's are not kept).
pub fn update<R>(edit: impl FnOnce(&mut PanelPrefsFile) -> R) -> Option<R> {
    let profile = crate::profile::active().ok()?;
    let mut file = PanelPrefsFile::load(profile.root());
    let result = edit(&mut file);
    if !profile.is_guest() {
        let _ = file.save(profile.root());
    }
    Some(result)
}

/// The active profile's options for one app.
pub fn current(app: u32) -> PanelPrefs {
    crate::profile::active().ok().map_or_else(PanelPrefs::default, |profile| PanelPrefsFile::load(profile.root()).get(app))
}

/// Whether a panel last reloaded `since` ago is due for its reload.
pub fn reload_due(minutes: u8, since: Duration) -> bool {
    minutes > 0 && since >= Duration::from_secs(u64::from(minutes.min(MAX_RELOAD_MINUTES)) * 60)
}

/// The unread count a page title announces: a leading or trailing `(N)` or `[N]`, 1 to 999.
pub fn title_notifications(title: &str) -> u32 {
    let title = title.trim();
    let digits = |inner: &str| -> Option<u32> {
        (!inner.is_empty() && inner.len() <= 3 && inner.bytes().all(|b| b.is_ascii_digit()))
            .then(|| inner.parse::<u32>().ok())
            .flatten()
            .filter(|count| *count > 0)
    };
    let bracketed = |text: &str, open: char, close: char| -> Option<u32> {
        let inner = text.strip_prefix(open)?.split_once(close)?.0;
        digits(inner.trim())
    };
    let trailing = |text: &str, open: char, close: char| -> Option<u32> {
        let inner = text.strip_suffix(close)?.rsplit_once(open)?.1;
        digits(inner.trim())
    };
    bracketed(title, '(', ')')
        .or_else(|| bracketed(title, '[', ']'))
        .or_else(|| trailing(title, '(', ')'))
        .or_else(|| trailing(title, '[', ']'))
        .unwrap_or(0)
}

/// The label of a zoom step for the menu: `Zoom (125%)`.
pub fn zoom_label(percent: u16) -> String {
    format!("Reset zoom ({percent}%)")
}

/// "Every 5 minutes" and friends, for the reload menu.
pub fn reload_label(minutes: u8) -> String {
    match minutes {
        0 => "Never".to_owned(),
        1 => "Every minute".to_owned(),
        n => format!("Every {n} minutes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_not_written_and_everything_round_trips() {
        let mut file = PanelPrefsFile::default();
        file.set(3, PanelPrefs::default());
        assert_eq!(file.serialize(), "");
        file.set(3, PanelPrefs { zoom: 125, muted: true, reload_minutes: 10, width: Some(420), notify: false });
        file.set(7, PanelPrefs { muted: true, ..PanelPrefs::default() });
        file.remember_removed("Mail", "https://mail.example/");
        let text = file.serialize();
        assert_eq!(text, "app=3 zoom=125 muted=true reload=10 width=420 notify=false\napp=7 muted=true\nremoved=Mail\thttps://mail.example/\n");
        assert_eq!(PanelPrefsFile::parse(&text), file);
        assert_eq!(file.get(99), PanelPrefs::default());
    }

    #[test]
    fn out_of_range_and_garbled_values_keep_the_defaults() {
        let file = PanelPrefsFile::parse("app=1 zoom=9 reload=31 width=100 muted=maybe notify=x\napp=x zoom=150\nnonsense\nremoved=NoTab\n");
        assert_eq!(file.get(1), PanelPrefs::default());
        assert!(file.removed.is_empty());
        assert!(!PanelPrefsFile::parse("app=2 reload=30\n").get(2).muted);
        assert_eq!(PanelPrefsFile::parse("app=2 reload=30\n").get(2).reload_minutes, 30);
    }

    #[test]
    fn removed_apps_are_kept_newest_first_once_per_address_and_capped() {
        let mut file = PanelPrefsFile::default();
        file.remember_removed("A", "https://a.example/");
        file.remember_removed("B", "https://b.example/");
        file.remember_removed("A again", "https://A.example/");
        assert_eq!(file.removed.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["A again", "B"]);
        for i in 0..20 {
            file.remember_removed("X", &format!("https://x{i}.example/"));
        }
        assert_eq!(file.removed.len(), MAX_REMOVED);
        file.remember_removed("", "https://empty.example/");
        file.remember_removed("tab\tname", "https://t.example/");
        assert_eq!(file.removed.len(), MAX_REMOVED, "an empty or control-character name is refused");
        let first = file.removed[0].clone();
        assert_eq!(file.take_removed(0), Some(first));
        assert_eq!(file.take_removed(99), None);
    }

    #[test]
    fn reload_is_due_only_after_its_interval() {
        assert!(!reload_due(0, Duration::from_secs(86_400)));
        assert!(!reload_due(1, Duration::from_secs(59)));
        assert!(reload_due(1, Duration::from_secs(60)));
        assert!(!reload_due(30, Duration::from_secs(29 * 60)));
        assert!(reload_due(30, Duration::from_secs(30 * 60)));
        assert!(PanelPrefsFile::parse("app=1 reload=5\n").any_reload());
        assert!(!PanelPrefsFile::parse("app=1 zoom=150\n").any_reload());
    }

    #[test]
    fn title_counts_are_read_from_the_usual_places_and_nothing_else() {
        for (title, count) in [("(3) Inbox", 3), ("[12] Chat", 12), ("Inbox (4)", 4), ("Chat [999]", 999), ("  (7)  Mail ", 7)] {
            assert_eq!(title_notifications(title), count, "{title}");
        }
        for title in ["Inbox", "(0) Inbox", "(1000) Inbox", "(a) Inbox", "()", "Windows (R) 11", "2024 report", "(3", "Version (1.2)"] {
            assert_eq!(title_notifications(title), 0, "{title}");
        }
    }

    #[test]
    fn menu_words() {
        assert_eq!(reload_label(0), "Never");
        assert_eq!(reload_label(1), "Every minute");
        assert_eq!(reload_label(15), "Every 15 minutes");
        assert_eq!(zoom_label(125), "Reset zoom (125%)");
        assert!(RELOAD_CHOICES.iter().all(|m| *m <= MAX_RELOAD_MINUTES));
    }
}
