//! Window appearance and panel options (thread T9). One small struct stored as `key=value` lines
//! in `settings.txt` (`Settings::window`), the process-wide switches the native code reads, and the
//! per-web-panel options kept in `panel_prefs.txt`.
//!
//! ```text
//! animations=true            win_animations
//! range_buttons=false        +/- buttons instead of sliders on the settings page
//! fade_background=false      mute the chrome's colours while the window is not in front
//! simple_scrollbars=false    thin, quiet scrollbars in Bowser's own pages
//! mono_icons=false           settings page icons in one text colour
//! panel_close_button=true    the close button in a panel's header
//! panel_folder_counts=true   the item count on a bookmarks folder
//! panel_auto_close=true      a floating panel closes when you click elsewhere
//! ```

use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowPrefs {
    pub animations: bool,
    pub range_buttons: bool,
    pub fade_background: bool,
    pub simple_scrollbars: bool,
    pub mono_icons: bool,
    pub panel_close_button: bool,
    pub panel_folder_counts: bool,
    pub panel_auto_close: bool,
}

impl Default for WindowPrefs {
    fn default() -> Self {
        Self {
            animations: true,
            range_buttons: false,
            fade_background: false,
            simple_scrollbars: false,
            mono_icons: false,
            panel_close_button: true,
            panel_folder_counts: true,
            panel_auto_close: true,
        }
    }
}

/// Every key this module owns, in file order.
pub const KEYS: [&str; 8] = [
    "animations", "range_buttons", "fade_background", "simple_scrollbars", "mono_icons",
    "panel_close_button", "panel_folder_counts", "panel_auto_close",
];

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

impl WindowPrefs {
    fn slot(&mut self, key: &str) -> Option<&mut bool> {
        Some(match key {
            "animations" => &mut self.animations,
            "range_buttons" => &mut self.range_buttons,
            "fade_background" => &mut self.fade_background,
            "simple_scrollbars" => &mut self.simple_scrollbars,
            "mono_icons" => &mut self.mono_icons,
            "panel_close_button" => &mut self.panel_close_button,
            "panel_folder_counts" => &mut self.panel_folder_counts,
            "panel_auto_close" => &mut self.panel_auto_close,
            _ => return None,
        })
    }

    /// File reader: a bad value keeps the default. Returns whether the key is one of ours.
    pub fn parse_key(&mut self, key: &str, value: &str) -> bool {
        match self.slot(key) {
            Some(slot) => {
                if let Some(on) = parse_bool(value) {
                    *slot = on;
                }
                true
            }
            None => false,
        }
    }

    /// Settings-page reader: a bad value or an unknown key refuses the whole payload (`None`).
    pub fn apply_strict(&mut self, key: &str, value: &str) -> Option<()> {
        let slot = self.slot(key)?;
        *slot = match value {
            "true" => true,
            "false" => false,
            _ => return None,
        };
        Some(())
    }

    /// `(key, value)` lines in `KEYS` order.
    pub fn pairs(&self) -> [(&'static str, String); 8] {
        let v = [
            self.animations, self.range_buttons, self.fade_background, self.simple_scrollbars,
            self.mono_icons, self.panel_close_button, self.panel_folder_counts, self.panel_auto_close,
        ];
        std::array::from_fn(|i| (KEYS[i], v[i].to_string()))
    }
}

/// Rewrites `text` so each of our keys has one line; a key equal to its default is added only if
/// it was already present (a profile that never touched them keeps a file without these lines).
pub fn render_into(text: &str, prefs: &WindowPrefs, newline: &str) -> String {
    let pairs = prefs.pairs();
    let defaults = WindowPrefs::default().pairs();
    let mut written = [false; 8];
    let mut out = String::with_capacity(text.len() + 64);
    for line in text.lines() {
        let slot = if line.trim_start().starts_with('#') {
            None
        } else {
            line.split_once('=').and_then(|(key, _)| KEYS.iter().position(|name| *name == key.trim()))
        };
        match slot {
            Some(i) if written[i] => {}
            Some(i) => {
                written[i] = true;
                out.push_str(&format!("{}={}{newline}", pairs[i].0, pairs[i].1));
            }
            None => {
                out.push_str(line);
                out.push_str(newline);
            }
        }
    }
    for i in 0..8 {
        if !written[i] && pairs[i].1 != defaults[i].1 {
            out.push_str(&format!("{}={}{newline}", pairs[i].0, pairs[i].1));
        }
    }
    out
}

/// What the HTML chrome is told about the window's look (the `look` object of a `state` message).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Look {
    pub motion: bool,
    pub scroll: bool,
    pub mono: bool,
    pub fade: bool,
    /// The window is not the foreground window.
    pub bg: bool,
    pub panel_close: bool,
    pub folder_counts: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self::of(&WindowPrefs::default(), false)
    }
}

impl Look {
    /// The look for `prefs` while the window is (not) in the background.
    pub fn of(prefs: &WindowPrefs, background: bool) -> Self {
        Self {
            motion: prefs.animations,
            scroll: prefs.simple_scrollbars,
            mono: prefs.mono_icons,
            fade: prefs.fade_background,
            bg: background,
            panel_close: prefs.panel_close_button,
            folder_counts: prefs.panel_folder_counts,
        }
    }

    pub fn to_json(self) -> String {
        format!(
            r#"{{"motion":{},"scroll":{},"mono":{},"fade":{},"bg":{},"panelClose":{},"folderCounts":{}}}"#,
            self.motion, self.scroll, self.mono, self.fade, self.bg, self.panel_close, self.folder_counts
        )
    }
}

// ---- process-wide switches (one profile per process), read by native code ----

static ANIMATIONS: AtomicBool = AtomicBool::new(true);
static AUTO_CLOSE: AtomicBool = AtomicBool::new(true);

/// Publishes the live values the native code reads.
pub fn publish(prefs: &WindowPrefs) {
    ANIMATIONS.store(prefs.animations, Ordering::Relaxed);
    AUTO_CLOSE.store(prefs.panel_auto_close, Ordering::Relaxed);
}

/// Whether Bowser may animate: its own setting AND what Windows allows.
pub fn animate(system_allows: bool) -> bool {
    system_allows && ANIMATIONS.load(Ordering::Relaxed)
}

/// Whether a floating panel closes when it loses activation.
pub fn auto_close_panels() -> bool {
    AUTO_CLOSE.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_and_unknown_keys_are_not_ours() {
        let mut p = WindowPrefs::default();
        assert!(p.parse_key("mono_icons", "true"));
        assert!(p.mono_icons);
        assert!(p.parse_key("animations", "maybe"), "ours, bad value keeps the default");
        assert!(p.animations);
        assert!(!p.parse_key("ui_zoom", "100"));
    }

    #[test]
    fn the_settings_file_keeps_the_window_options_and_reset_puts_them_back() {
        use crate::settings::{reset_defaults, render_updated, Settings};
        let mut settings = Settings::default();
        assert!(!render_updated("", &settings).contains("simple_scrollbars"), "defaults add no lines");
        settings.window = WindowPrefs { animations: false, range_buttons: true, fade_background: true, simple_scrollbars: true, mono_icons: true, panel_close_button: false, panel_folder_counts: false, panel_auto_close: false };
        let text = render_updated("", &settings);
        assert_eq!(Settings::parse(&text).window, settings.window);
        let hand_edited = Settings::parse("animations=false\nmono_icons=true\nfade_background=banana\n");
        assert!(!hand_edited.window.animations && hand_edited.window.mono_icons && !hand_edited.window.fade_background);
        assert_eq!(reset_defaults(&settings).window, WindowPrefs::default(), "Reset settings reaches these options");
    }

    #[test]
    fn strict_refuses_bad_values() {
        let mut p = WindowPrefs::default();
        assert!(p.apply_strict("fade_background", "true").is_some());
        assert!(p.fade_background);
        assert!(p.apply_strict("fade_background", "1").is_none());
        assert!(p.apply_strict("nope", "true").is_none());
    }

    #[test]
    fn render_adds_only_non_default_lines_and_updates_existing() {
        let mut p = WindowPrefs::default();
        assert_eq!(render_into("a=1\n", &p, "\n"), "a=1\n");
        p.simple_scrollbars = true;
        assert_eq!(render_into("a=1\n", &p, "\n"), "a=1\nsimple_scrollbars=true\n");
        p.simple_scrollbars = false;
        assert_eq!(render_into("simple_scrollbars=true\nb=2\n", &p, "\n"), "simple_scrollbars=false\nb=2\n");
    }

    #[test]
    fn the_look_reflects_the_options_and_the_background() {
        let mut p = WindowPrefs::default();
        assert_eq!(Look::default().to_json(), r#"{"motion":true,"scroll":false,"mono":false,"fade":false,"bg":false,"panelClose":true,"folderCounts":true}"#);
        p.fade_background = true;
        p.animations = false;
        let look = Look::of(&p, true);
        assert!(look.fade && look.bg && !look.motion);
    }

    #[test]
    fn animation_needs_both_the_setting_and_windows() {
        publish(&WindowPrefs { animations: false, ..WindowPrefs::default() });
        assert!(!animate(true));
        publish(&WindowPrefs::default());
        assert!(animate(true));
        assert!(!animate(false));
        assert!(auto_close_panels());
        publish(&WindowPrefs { panel_auto_close: false, ..WindowPrefs::default() });
        assert!(!auto_close_panels());
        publish(&WindowPrefs::default());
    }
}
