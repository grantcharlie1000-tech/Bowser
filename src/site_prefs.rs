//! Per-site preferences, keyed by origin like [`crate::permissions`]. Today that is the page zoom;
//! the line format has a field name so later preferences can share the file.
//!
//! Pure logic and file I/O only. The file is `site-prefs.txt` in the profile directory, one value
//! per line:
//!
//! ```text
//! https://docs.example zoom 125
//! http://localhost:8080 zoom 80
//! ```
//!
//! Each line is `<origin> <field> <value>`. The origin is the page's scheme, host and port (default
//! port dropped). A zoom of 100 is the default and is never stored. Blank lines, `#` comments,
//! unknown fields and malformed lines are ignored, and the file is capped at [`MAX_FILE_BYTES`].
//! Guest sessions use [`SitePrefsStore::in_memory`], which never touches the disk.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

pub use crate::permissions::origin_of;

pub const FILE_NAME: &str = "site-prefs.txt";
pub const MAX_FILE_BYTES: usize = 256 * 1024;
/// Largest number of sites with stored preferences.
pub const MAX_SITES: usize = 2048;

/// Page zoom in percent. 100 is the default.
pub const ZOOM_DEFAULT: u16 = 100;
pub const ZOOM_MIN: u16 = 25;
pub const ZOOM_MAX: u16 = 500;
/// The zoom levels Ctrl+Plus and Ctrl+Minus step through, as in Chrome.
pub const ZOOM_STEPS: [u16; 17] = [
    25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500,
];

const HEADER: &str = "# Bowser per-site preferences: <origin> <field> <value>.\n\
# Fields: zoom (percent, 25 to 500). A site without a line uses the defaults.\n";

/// The next zoom level above `percent`, or the maximum. Works from any value, not just a step.
pub fn zoom_in(percent: u16) -> u16 {
    ZOOM_STEPS
        .into_iter()
        .find(|step| *step > percent)
        .unwrap_or(ZOOM_MAX)
}

/// The next zoom level below `percent`, or the minimum.
pub fn zoom_out(percent: u16) -> u16 {
    ZOOM_STEPS
        .into_iter()
        .rev()
        .find(|step| *step < percent)
        .unwrap_or(ZOOM_MIN)
}

/// The zoom percent for a WebView2 zoom factor, clamped to the supported range. A non-finite factor
/// reads as the default.
pub fn percent_of_factor(factor: f64) -> u16 {
    if !factor.is_finite() {
        return ZOOM_DEFAULT;
    }
    (factor * 100.0)
        .round()
        .clamp(f64::from(ZOOM_MIN), f64::from(ZOOM_MAX)) as u16
}

/// A zoom command: Ctrl+Plus, Ctrl+Minus or Ctrl+0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomStep {
    In,
    Out,
    Reset,
}

impl ZoomStep {
    /// The zoom this command leads to from `current`.
    pub fn apply(self, current: u16) -> u16 {
        match self {
            Self::In => zoom_in(current),
            Self::Out => zoom_out(current),
            Self::Reset => ZOOM_DEFAULT,
        }
    }
}

/// The WebView2 zoom factor for a percent.
pub fn factor_of_percent(percent: u16) -> f64 {
    f64::from(percent.clamp(ZOOM_MIN, ZOOM_MAX)) / 100.0
}

/// The store shared between the app (zoom commands, navigation commits) and the user interface.
pub type SitePrefsHandle = std::rc::Rc<std::cell::RefCell<SitePrefsStore>>;

/// What is stored for one site. Every field is optional: `None` means the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SitePrefs {
    pub zoom: Option<u16>,
}

impl SitePrefs {
    fn is_default(self) -> bool {
        self.zoom.is_none()
    }
}

type Sites = BTreeMap<String, SitePrefs>;

/// Stored preferences, keyed by canonical origin. A change is written to disk before it counts, so
/// the in-memory store always matches the file.
#[derive(Debug, Default)]
pub struct SitePrefsStore {
    path: Option<PathBuf>,
    sites: Sites,
}

impl SitePrefsStore {
    /// Loads `path`. A missing, unreadable or oversized file gives an empty store.
    pub fn load(path: &Path) -> Self {
        let sites = read_capped(path)
            .map(|text| parse(&text))
            .unwrap_or_default();
        Self {
            path: Some(path.to_path_buf()),
            sites,
        }
    }

    /// A store that is never written: guest sessions remember zoom for the session only.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Whether changes are saved to disk.
    pub fn persists(&self) -> bool {
        self.path.is_some()
    }

    /// The stored zoom percent for an origin, or `None` for the default.
    pub fn zoom(&self, origin: &str) -> Option<u16> {
        self.sites.get(origin)?.zoom
    }

    /// The zoom a page at `uri` should show: its stored value, or 100. Non-web addresses are 100.
    pub fn zoom_for_uri(&self, uri: &str) -> u16 {
        origin_of(uri)
            .and_then(|origin| self.zoom(&origin))
            .unwrap_or(ZOOM_DEFAULT)
    }

    /// Stores a zoom percent for one origin (`None` or 100 clears it), then saves. A value outside
    /// the supported range is clamped. On a failed save the previous value is restored.
    pub fn set_zoom(&mut self, origin: &str, percent: Option<u16>) -> io::Result<()> {
        let wanted = percent
            .map(|value| value.clamp(ZOOM_MIN, ZOOM_MAX))
            .filter(|value| *value != ZOOM_DEFAULT);
        let previous = self.sites.get(origin).copied();
        if previous.and_then(|prefs| prefs.zoom) == wanted {
            return Ok(());
        }
        if previous.is_none() && wanted.is_some() && self.sites.len() >= MAX_SITES {
            return Err(io::Error::other("too many sites with stored preferences"));
        }
        let mut next = previous.unwrap_or_default();
        next.zoom = wanted;
        self.write(origin, next);
        if let Err(error) = self.save() {
            match previous {
                Some(prefs) => self.write(origin, prefs),
                None => {
                    self.sites.remove(origin);
                }
            }
            return Err(error);
        }
        Ok(())
    }

    /// Number of sites with at least one stored preference.
    pub fn len(&self) -> usize {
        self.sites.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sites.is_empty()
    }

    fn write(&mut self, origin: &str, prefs: SitePrefs) {
        if prefs.is_default() {
            self.sites.remove(origin);
        } else {
            self.sites.insert(origin.to_owned(), prefs);
        }
    }

    fn save(&self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        write_atomic(path, serialise(&self.sites).as_bytes())
    }
}

fn parse(text: &str) -> Sites {
    let mut sites = Sites::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let (Some(origin), Some(field), Some(value), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let Some(origin) = origin_of(origin) else {
            continue;
        };
        match field {
            "zoom" => {
                let Some(zoom) = value
                    .parse::<u16>()
                    .ok()
                    .filter(|zoom| (ZOOM_MIN..=ZOOM_MAX).contains(zoom) && *zoom != ZOOM_DEFAULT)
                else {
                    continue;
                };
                if !sites.contains_key(&origin) && sites.len() >= MAX_SITES {
                    break;
                }
                sites.entry(origin).or_default().zoom = Some(zoom);
            }
            _ => continue,
        }
    }
    sites
}

fn serialise(sites: &Sites) -> String {
    let mut text = String::from(HEADER);
    for (origin, prefs) in sites {
        if let Some(zoom) = prefs.zoom {
            text.push_str(&format!("{origin} zoom {zoom}\n"));
        }
    }
    text
}

fn read_capped(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_FILE_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = path.with_extension("txt.tmp");
    let mut file = File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bowser-site-prefs-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join(FILE_NAME)
    }

    #[test]
    fn zoom_steps_are_the_chrome_levels_in_order() {
        assert_eq!(ZOOM_STEPS.first(), Some(&ZOOM_MIN));
        assert_eq!(ZOOM_STEPS.last(), Some(&ZOOM_MAX));
        assert!(ZOOM_STEPS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(ZOOM_STEPS.contains(&ZOOM_DEFAULT));
    }

    #[test]
    fn zooming_in_and_out_walks_the_steps_and_stops_at_the_limits() {
        assert_eq!(zoom_in(100), 110);
        assert_eq!(zoom_in(110), 125);
        assert_eq!(zoom_out(100), 90);
        assert_eq!(zoom_out(25), 25);
        assert_eq!(zoom_in(500), 500);
        assert_eq!(zoom_out(26), 25);
        // From a value between steps (a pinch or wheel result), the next step in that direction.
        assert_eq!(zoom_in(105), 110);
        assert_eq!(zoom_out(105), 100);
        // Walking all the way up and down visits every step once.
        let mut value = ZOOM_MIN;
        let mut up = vec![value];
        while value != ZOOM_MAX {
            value = zoom_in(value);
            up.push(value);
        }
        assert_eq!(up, ZOOM_STEPS);
        let mut down = vec![value];
        while value != ZOOM_MIN {
            value = zoom_out(value);
            down.push(value);
        }
        down.reverse();
        assert_eq!(down, ZOOM_STEPS);
    }

    #[test]
    fn zoom_commands_step_from_the_current_value_and_reset_goes_to_100() {
        assert_eq!(ZoomStep::In.apply(100), 110);
        assert_eq!(ZoomStep::Out.apply(100), 90);
        assert_eq!(ZoomStep::Reset.apply(250), 100);
        assert_eq!(ZoomStep::Reset.apply(100), 100);
        assert_eq!(ZoomStep::In.apply(ZOOM_MAX), ZOOM_MAX);
        assert_eq!(ZoomStep::Out.apply(ZOOM_MIN), ZOOM_MIN);
    }

    #[test]
    fn factors_convert_to_clamped_percent_and_back() {
        assert_eq!(percent_of_factor(1.0), 100);
        assert_eq!(percent_of_factor(1.25), 125);
        assert_eq!(percent_of_factor(0.8), 80);
        assert_eq!(percent_of_factor(0.01), ZOOM_MIN);
        assert_eq!(percent_of_factor(99.0), ZOOM_MAX);
        assert_eq!(percent_of_factor(f64::NAN), ZOOM_DEFAULT);
        assert_eq!(percent_of_factor(f64::INFINITY), ZOOM_DEFAULT);
        assert!((factor_of_percent(125) - 1.25).abs() < 1e-9);
        assert!((factor_of_percent(5) - 0.25).abs() < 1e-9);
        assert!((factor_of_percent(900) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn parse_is_tolerant_and_keeps_only_valid_zoom_lines() {
        let sites = parse(
            "# comment\n\n\
             https://a.test zoom 125\n\
             https://a.test zoom 150\n\
             https://b.test zoom 100\n\
             https://c.test zoom 24\n\
             https://c.test zoom 501\n\
             https://c.test zoom abc\n\
             https://c.test zoom -5\n\
             https://c.test zoom\n\
             https://c.test zoom 125 extra\n\
             https://c.test colour blue\n\
             file:///x.html zoom 150\n\
             HTTPS://D.test:443 zoom 200\n",
        );
        assert_eq!(sites.len(), 2, "{sites:?}");
        assert_eq!(sites["https://a.test"].zoom, Some(150), "a later line replaces an earlier one");
        assert_eq!(sites["https://d.test"].zoom, Some(200));
    }

    #[test]
    fn serialise_round_trips_and_is_sorted() {
        let mut sites = Sites::new();
        sites.insert("https://z.test".to_owned(), SitePrefs { zoom: Some(80) });
        sites.insert("https://a.test:8443".to_owned(), SitePrefs { zoom: Some(175) });
        sites.insert("https://none.test".to_owned(), SitePrefs { zoom: None });
        let text = serialise(&sites);
        assert!(text.ends_with("https://a.test:8443 zoom 175\nhttps://z.test zoom 80\n"), "{text}");
        let parsed = parse(&text);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed["https://z.test"].zoom, Some(80));
    }

    #[test]
    fn zoom_persists_across_a_reload_and_is_keyed_by_exact_origin() {
        let path = temp_file("persist");
        {
            let mut store = SitePrefsStore::load(&path);
            assert!(store.persists());
            store.set_zoom("https://a.test", Some(125)).unwrap();
            store.set_zoom("http://localhost:8080", Some(80)).unwrap();
        }
        let reloaded = SitePrefsStore::load(&path);
        assert_eq!(reloaded.zoom_for_uri("https://a.test/some/page?q=1"), 125);
        assert_eq!(reloaded.zoom_for_uri("http://localhost:8080/x"), 80);
        assert_eq!(reloaded.zoom_for_uri("http://a.test/"), 100, "scheme differs");
        assert_eq!(reloaded.zoom_for_uri("https://sub.a.test/"), 100, "host differs");
        assert_eq!(reloaded.zoom_for_uri("https://a.test:8443/"), 100, "port differs");
        assert_eq!(reloaded.zoom_for_uri("file:///C:/x.html"), 100);
        assert_eq!(reloaded.zoom_for_uri("not a url"), 100);
    }

    #[test]
    fn resetting_to_the_default_removes_the_line() {
        let path = temp_file("reset");
        let mut store = SitePrefsStore::load(&path);
        store.set_zoom("https://a.test", Some(150)).unwrap();
        store.set_zoom("https://a.test", Some(100)).unwrap();
        assert!(store.is_empty());
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.lines().any(|line| line.starts_with("https://")), "{text}");
        store.set_zoom("https://a.test", Some(150)).unwrap();
        store.set_zoom("https://a.test", None).unwrap();
        assert_eq!(store.zoom("https://a.test"), None);
    }

    #[test]
    fn out_of_range_values_are_clamped_when_set() {
        let mut store = SitePrefsStore::in_memory();
        store.set_zoom("https://a.test", Some(9000)).unwrap();
        assert_eq!(store.zoom("https://a.test"), Some(ZOOM_MAX));
        store.set_zoom("https://a.test", Some(1)).unwrap();
        assert_eq!(store.zoom("https://a.test"), Some(ZOOM_MIN));
    }

    #[test]
    fn an_in_memory_store_remembers_but_never_writes() {
        let mut store = SitePrefsStore::in_memory();
        assert!(!store.persists());
        store.set_zoom("https://a.test", Some(150)).unwrap();
        assert_eq!(store.zoom_for_uri("https://a.test/"), 150);
    }

    #[test]
    fn a_failed_save_restores_the_previous_value() {
        let dir = std::env::temp_dir().join(format!(
            "bowser-site-prefs-{}-missing",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let mut store = SitePrefsStore::load(&dir.join(FILE_NAME));
        assert!(store.set_zoom("https://a.test", Some(150)).is_err(), "the parent directory does not exist");
        assert_eq!(store.zoom("https://a.test"), None);
        assert!(store.is_empty());
    }

    #[test]
    fn oversized_or_unreadable_files_give_an_empty_store() {
        let path = temp_file("oversized");
        let mut big = String::new();
        while big.len() <= MAX_FILE_BYTES {
            big.push_str("https://a.test zoom 150\n");
        }
        fs::write(&path, big).unwrap();
        assert_eq!(SitePrefsStore::load(&path).zoom("https://a.test"), None);
        assert!(SitePrefsStore::load(&path.with_file_name("absent.txt")).is_empty());
        fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert!(SitePrefsStore::load(&path).is_empty(), "not UTF-8");
    }

    #[test]
    fn the_site_count_is_capped_in_parse_and_set() {
        let mut text = String::new();
        for index in 0..MAX_SITES + 10 {
            text.push_str(&format!("https://site{index}.test zoom 150\n"));
        }
        assert_eq!(parse(&text).len(), MAX_SITES);
        let mut store = SitePrefsStore::in_memory();
        for index in 0..MAX_SITES {
            store.set_zoom(&format!("https://site{index}.test"), Some(150)).unwrap();
        }
        assert!(store.set_zoom("https://extra.test", Some(150)).is_err());
        assert_eq!(store.zoom("https://extra.test"), None);
        store.set_zoom("https://site0.test", Some(200)).unwrap();
        store.set_zoom("https://site1.test", None).unwrap();
        assert_eq!(store.len(), MAX_SITES - 1);
    }
}
