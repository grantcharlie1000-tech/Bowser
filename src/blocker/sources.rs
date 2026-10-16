//! The list of rule sources the blocker is built from, and the files behind them.
//!
//! Sources live in the profile's blocker folder:
//! * `blocker-sources.txt` records which sources exist and which are switched on;
//! * `blocker-lists/<id>.txt` holds the text of a list the user added (from a file, or fetched
//!   from a URL the user typed, see [`super::fetch`]).
//!
//! Three sources ship inside the program (trackers, ads, cookie notices) and can be switched off
//! but not removed. A `filters.txt` the user already had in the profile folder shows up as one more
//! source. Nothing here touches the network and nothing updates itself: a list only changes when
//! the user adds it again or presses update on it.

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use super::{
    BUILTIN_LIST, Blocker, KindSelect, ListKind, MAX_USER_LIST_BYTES, USER_LIST_NAME, add_kinded, cookies::CookieRules,
    fetch, read_capped, write_atomic,
};

pub const REGISTRY_FILE: &str = "blocker-sources.txt";
pub const LISTS_DIR: &str = "blocker-lists";
/// Most sources of every kind, built-in ones included.
pub const MAX_SOURCES: usize = 24;
/// Longest source name kept, in characters.
pub const MAX_NAME_CHARS: usize = 60;

pub const ID_TRACKERS: &str = "bowser-trackers";
pub const ID_ADS: &str = "bowser-ads";
pub const ID_COOKIES: &str = "bowser-cookies";
pub const ID_USER_FILTERS: &str = "user-filters";

/// What a source's rules do.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceKind {
    /// Tracking and measurement hosts and paths: blocked at the "trackers" level and above.
    Trackers,
    /// Ad hosts, paths and element hiding: blocked at the "trackers and ads" level.
    Ads,
    /// Cookie-notice element hiding.
    Cookies,
}

impl SourceKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Trackers => "trackers",
            Self::Ads => "ads",
            Self::Cookies => "cookies",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "trackers" => Some(Self::Trackers),
            "ads" => Some(Self::Ads),
            "cookies" => Some(Self::Cookies),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Origin {
    /// Shipped inside the program.
    Builtin,
    /// The `filters.txt` file in the profile folder.
    UserFilters,
    /// A file the user picked; its text was copied into the lists folder.
    File,
    /// A list the user fetched from this https address; the text is in the lists folder.
    Url(String),
}

impl Origin {
    pub fn key(&self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::UserFilters => "filters",
            Self::File => "file",
            Self::Url(_) => "url",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub kind: SourceKind,
    pub enabled: bool,
    pub origin: Origin,
    /// Rules this source contributed to the last build (not saved).
    pub rules: usize,
}

impl Source {
    pub fn removable(&self) -> bool {
        matches!(self.origin, Origin::File | Origin::Url(_))
    }
}

/// Why a list could not be added.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceError {
    TooMany,
    /// The text is not a usable filter list; the string says why.
    Invalid(String),
    Io(String),
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooMany => write!(f, "Bowser keeps at most {MAX_SOURCES} lists. Remove one first."),
            Self::Invalid(why) | Self::Io(why) => f.write_str(why),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Registry {
    sources: Vec<Source>,
}

fn builtin(id: &str, name: &str, kind: SourceKind) -> Source {
    Source { id: id.to_owned(), name: name.to_owned(), kind, enabled: true, origin: Origin::Builtin, rules: 0 }
}

fn defaults() -> Vec<Source> {
    vec![
        builtin(ID_TRACKERS, "Bowser tracker list", SourceKind::Trackers),
        builtin(ID_ADS, "Bowser ad list", SourceKind::Ads),
        builtin(ID_COOKIES, "Bowser cookie notices", SourceKind::Cookies),
    ]
}

/// `[a-z0-9-]`, 1 to 24 characters: safe as a file name.
fn valid_id(id: &str) -> bool {
    (1..=24).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A name for the list: control characters dropped, at most [`MAX_NAME_CHARS`] characters.
pub fn clean_name(name: &str) -> String {
    let cleaned: String = name.chars().filter(|c| !c.is_control()).collect();
    cleaned.trim().chars().take(MAX_NAME_CHARS).collect()
}

fn lists_dir(dir: &Path) -> PathBuf {
    dir.join(LISTS_DIR)
}

fn list_path(dir: &Path, id: &str) -> Option<PathBuf> {
    valid_id(id).then(|| lists_dir(dir).join(format!("{id}.txt")))
}

impl Registry {
    pub fn sources(&self) -> &[Source] {
        &self.sources
    }

    pub fn get(&self, id: &str) -> Option<&Source> {
        self.sources.iter().find(|source| source.id == id)
    }

    /// The registry for the profile folder `dir`: the built-in sources, the user's `filters.txt` if
    /// there is one, then every source recorded in `blocker-sources.txt`.
    pub fn load(dir: &Path) -> Self {
        let mut sources = defaults();
        if dir.join(USER_LIST_NAME).is_file() {
            sources.push(Source {
                id: ID_USER_FILTERS.to_owned(),
                name: "My filters (filters.txt)".to_owned(),
                kind: SourceKind::Ads,
                enabled: true,
                origin: Origin::UserFilters,
                rules: 0,
            });
        }
        let text = read_capped(&dir.join(REGISTRY_FILE), 64 * 1024).unwrap_or_default();
        for line in text.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            let Some(&id) = fields.first() else { continue };
            let enabled = fields.get(1).copied() != Some("0");
            if let Some(known) = sources.iter_mut().find(|source| source.id == id) {
                known.enabled = enabled;
                continue;
            }
            // A user source: id, enabled, kind, origin, name.
            if fields.len() < 5 || sources.len() >= MAX_SOURCES || !valid_id(id) {
                continue;
            }
            let (Some(kind), Some(origin)) = (SourceKind::parse(fields[2]), parse_origin(fields[3])) else {
                continue;
            };
            let name = clean_name(&fields[4..].join(" "));
            if name.is_empty() {
                continue;
            }
            sources.push(Source { id: id.to_owned(), name, kind, enabled, origin, rules: 0 });
        }
        Self { sources }
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        let mut text = String::from("# Sources the blocker is built from. Edit them in Settings, Privacy and security.\n");
        for source in &self.sources {
            let flag = if source.enabled { 1 } else { 0 };
            match &source.origin {
                Origin::Builtin | Origin::UserFilters => text.push_str(&format!("{}\t{flag}\n", source.id)),
                Origin::File => text.push_str(&format!("{}\t{flag}\t{}\tfile\t{}\n", source.id, source.kind.key(), source.name)),
                Origin::Url(url) => {
                    text.push_str(&format!("{}\t{flag}\t{}\turl:{url}\t{}\n", source.id, source.kind.key(), source.name))
                }
            }
        }
        write_atomic(&dir.join(REGISTRY_FILE), text.as_bytes())
    }

    /// Switches a source on or off. `false` when there is no such source.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> bool {
        match self.sources.iter_mut().find(|source| source.id == id) {
            Some(source) => {
                source.enabled = enabled;
                true
            }
            None => false,
        }
    }

    fn next_id(&self) -> String {
        (1..)
            .map(|n| format!("u{n}"))
            .find(|id| self.get(id).is_none())
            .unwrap_or_else(|| "u1".to_owned())
    }

    /// The id of the source already fetched from `url`, if any.
    pub fn id_for_url(&self, url: &str) -> Option<&str> {
        self.sources
            .iter()
            .find(|source| matches!(&source.origin, Origin::Url(known) if known == url))
            .map(|source| source.id.as_str())
    }

    /// Adds a list whose text has already been checked with [`fetch::validate_list`]. The text is
    /// written to the lists folder, the source is recorded, and its id is returned. Adding a URL
    /// that is already a source replaces that source's text instead.
    pub fn add(&mut self, dir: &Path, name: &str, kind: SourceKind, origin: Origin, text: &str) -> Result<String, SourceError> {
        debug_assert!(matches!(origin, Origin::File | Origin::Url(_)));
        if text.len() > MAX_USER_LIST_BYTES {
            return Err(SourceError::Invalid("That list is larger than Bowser keeps (8 MB).".to_owned()));
        }
        let existing = match &origin {
            Origin::Url(url) => self.id_for_url(url).map(str::to_owned),
            _ => None,
        };
        let id = match existing {
            Some(id) => id,
            None => {
                if self.sources.len() >= MAX_SOURCES {
                    return Err(SourceError::TooMany);
                }
                self.next_id()
            }
        };
        let path = list_path(dir, &id).ok_or_else(|| SourceError::Io("The list could not be named.".to_owned()))?;
        fs::create_dir_all(lists_dir(dir)).map_err(|error| SourceError::Io(format!("The list could not be saved: {error}")))?;
        write_atomic(&path, text.as_bytes()).map_err(|error| SourceError::Io(format!("The list could not be saved: {error}")))?;
        let name = {
            let cleaned = clean_name(name);
            if cleaned.is_empty() { "Added list".to_owned() } else { cleaned }
        };
        match self.sources.iter_mut().find(|source| source.id == id) {
            Some(source) => {
                source.name = name;
                source.kind = kind;
                source.enabled = true;
            }
            None => self.sources.push(Source { id: id.clone(), name, kind, enabled: true, origin, rules: 0 }),
        }
        self.save(dir).map_err(|error| SourceError::Io(format!("The list could not be recorded: {error}")))?;
        Ok(id)
    }

    /// Removes an added list and its file. Built-in sources and `filters.txt` cannot be removed.
    pub fn remove(&mut self, dir: &Path, id: &str) -> io::Result<bool> {
        let Some(index) = self.sources.iter().position(|source| source.id == id && source.removable()) else {
            return Ok(false);
        };
        self.sources.remove(index);
        if let Some(path) = list_path(dir, id) {
            let _ = fs::remove_file(path);
        }
        self.save(dir)?;
        Ok(true)
    }
}

fn parse_origin(text: &str) -> Option<Origin> {
    match text {
        "file" => Some(Origin::File),
        _ => {
            let url = text.strip_prefix("url:")?;
            fetch::parse_https_url(url).ok().map(|url| Origin::Url(url.to_string()))
        }
    }
}

/// Everything the blocker matches with, built from the enabled sources.
#[derive(Default)]
pub struct Built {
    /// Trackers and ads: the "trackers and ads" level.
    pub all: Blocker,
    /// Trackers only: the "trackers" level.
    pub trackers: Blocker,
    pub cookies: CookieRules,
    /// Rules each enabled source contributed, by source id.
    pub counts: HashMap<String, usize>,
}

/// Builds the matchers from the enabled sources of `registry` stored in `dir`.
pub fn build(dir: &Path, registry: &Registry) -> Built {
    let mut built = Built::default();
    for source in registry.sources().iter().filter(|source| source.enabled) {
        let count = match (&source.origin, source.kind) {
            (Origin::Builtin, SourceKind::Trackers) => {
                add_kinded(&mut built.all, &mut built.trackers, BUILTIN_LIST, KindSelect::Markers(ListKind::Trackers))
            }
            (Origin::Builtin, SourceKind::Ads) => {
                add_kinded(&mut built.all, &mut built.trackers, BUILTIN_LIST, KindSelect::Markers(ListKind::Ads))
            }
            (Origin::Builtin, SourceKind::Cookies) => built.cookies.add_list(super::cookies::BUNDLED_LIST),
            (Origin::UserFilters, _) => {
                let text = read_capped(&dir.join(USER_LIST_NAME), MAX_USER_LIST_BYTES).unwrap_or_default();
                add_kinded(&mut built.all, &mut built.trackers, &text, KindSelect::All(ListKind::Ads))
            }
            (_, kind) => {
                let text = list_path(dir, &source.id)
                    .and_then(|path| read_capped(&path, MAX_USER_LIST_BYTES))
                    .unwrap_or_default();
                match kind {
                    SourceKind::Trackers => add_kinded(&mut built.all, &mut built.trackers, &text, KindSelect::All(ListKind::Trackers)),
                    SourceKind::Ads => add_kinded(&mut built.all, &mut built.trackers, &text, KindSelect::All(ListKind::Ads)),
                    SourceKind::Cookies => built.cookies.add_list(&text),
                }
            }
        };
        built.counts.insert(source.id.clone(), count);
    }
    built
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bowser-sources-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_fresh_profile_lists_the_three_built_in_sources_and_builds_from_them() {
        let dir = temp("fresh");
        let registry = Registry::load(&dir);
        let ids: Vec<&str> = registry.sources().iter().map(|source| source.id.as_str()).collect();
        assert_eq!(ids, [ID_TRACKERS, ID_ADS, ID_COOKIES]);
        assert!(registry.sources().iter().all(|source| source.enabled && !source.removable()));
        let built = build(&dir, &registry);
        assert!(built.trackers.rule_count() > 100, "{}", built.trackers.rule_count());
        assert!(built.all.rule_count() > built.trackers.rule_count());
        assert!(built.cookies.rule_count() > 100);
        assert!(built.counts[ID_TRACKERS] > 0 && built.counts[ID_ADS] > 0 && built.counts[ID_COOKIES] > 0);
        // The tracker level is a subset: a tracker host is in both, an ad host only in "all".
        assert!(built.all.matches("https://www.google-analytics.com/collect") && built.trackers.matches("https://www.google-analytics.com/collect"));
        assert!(built.all.matches("https://securepubads.g.doubleclick.net/x.js") && !built.trackers.matches("https://securepubads.g.doubleclick.net/x.js"));
    }

    #[test]
    fn switching_a_source_off_removes_its_rules_and_survives_a_reload() {
        let dir = temp("toggle");
        let mut registry = Registry::load(&dir);
        assert!(registry.set_enabled(ID_ADS, false));
        assert!(registry.set_enabled(ID_COOKIES, false));
        registry.save(&dir).unwrap();
        let reloaded = Registry::load(&dir);
        assert!(!reloaded.get(ID_ADS).unwrap().enabled);
        assert!(reloaded.get(ID_TRACKERS).unwrap().enabled);
        let built = build(&dir, &reloaded);
        assert_eq!(built.all.rule_count(), built.trackers.rule_count(), "no ad rules left");
        assert_eq!(built.cookies.rule_count(), 0);
        assert!(!built.all.matches("https://securepubads.g.doubleclick.net/x.js"));
        assert!(!registry.set_enabled("nope", true));
    }

    #[test]
    fn an_added_list_is_stored_built_toggled_updated_and_removed() {
        let dir = temp("added");
        let mut registry = Registry::load(&dir);
        let id = registry
            .add(&dir, "Test list", SourceKind::Ads, Origin::File, "||only-in-my-list.example^\n")
            .unwrap();
        assert_eq!(id, "u1");
        assert!(lists_dir(&dir).join("u1.txt").is_file());
        let built = build(&dir, &Registry::load(&dir));
        assert!(built.all.matches("https://only-in-my-list.example/x.js"));
        assert!(!built.trackers.matches("https://only-in-my-list.example/x.js"), "an ads list is not a trackers list");
        assert_eq!(built.counts["u1"], 1);

        let url = "https://lists.example/easy.txt";
        let tracker_id = registry
            .add(&dir, "From URL", SourceKind::Trackers, Origin::Url(url.to_owned()), "||tracker-in-url-list.example^\n")
            .unwrap();
        assert_eq!(tracker_id, "u2");
        let built = build(&dir, &Registry::load(&dir));
        assert!(built.trackers.matches("https://tracker-in-url-list.example/p"));
        // The same URL again replaces its text instead of adding a second source.
        let again = registry
            .add(&dir, "From URL", SourceKind::Trackers, Origin::Url(url.to_owned()), "||second-version.example^\n")
            .unwrap();
        assert_eq!(again, "u2");
        assert_eq!(registry.sources().len(), 5);
        let built = build(&dir, &Registry::load(&dir));
        assert!(built.trackers.matches("https://second-version.example/p"));
        assert!(!built.trackers.matches("https://tracker-in-url-list.example/p"));

        assert!(registry.set_enabled("u1", false));
        registry.save(&dir).unwrap();
        assert!(!build(&dir, &Registry::load(&dir)).all.matches("https://only-in-my-list.example/x.js"));

        assert!(registry.remove(&dir, "u1").unwrap());
        assert!(!lists_dir(&dir).join("u1.txt").exists());
        assert!(Registry::load(&dir).get("u1").is_none());
        assert!(!registry.remove(&dir, ID_ADS).unwrap(), "built-in sources stay");
        assert!(!registry.remove(&dir, "u9").unwrap());
    }

    #[test]
    fn a_cookie_list_feeds_the_cookie_rules_not_the_request_matchers() {
        let dir = temp("cookies");
        let mut registry = Registry::load(&dir);
        registry
            .add(&dir, "Cookie list", SourceKind::Cookies, Origin::File, "example.com##.my-own-consent-banner\n||ignored.example^\n")
            .unwrap();
        let built = build(&dir, &Registry::load(&dir));
        let css = built.cookies.css_for_host("example.com").unwrap();
        assert!(css.contains(".my-own-consent-banner{display:none!important}"));
        assert!(!built.all.matches("https://ignored.example/x"), "a cookie list never blocks a request");
    }

    #[test]
    fn the_old_filters_file_becomes_a_source_and_is_ads_level() {
        let dir = temp("filters");
        fs::write(dir.join(USER_LIST_NAME), "||legacy-filter.example^\n").unwrap();
        let registry = Registry::load(&dir);
        let source = registry.get(ID_USER_FILTERS).expect("listed");
        assert_eq!((source.kind, source.origin.clone()), (SourceKind::Ads, Origin::UserFilters));
        let built = build(&dir, &registry);
        assert!(built.all.matches("https://legacy-filter.example/a"));
        assert!(!built.trackers.matches("https://legacy-filter.example/a"));
    }

    #[test]
    fn the_source_count_and_names_are_capped_and_ids_cannot_escape_the_folder() {
        let dir = temp("caps");
        let mut registry = Registry::load(&dir);
        for n in 0..MAX_SOURCES - 3 {
            registry.add(&dir, &format!("L{n}"), SourceKind::Ads, Origin::File, "||x.example^\n").unwrap();
        }
        assert_eq!(
            registry.add(&dir, "one too many", SourceKind::Ads, Origin::File, "||x.example^\n"),
            Err(SourceError::TooMany)
        );
        assert_eq!(clean_name(&"é\u{7}x".repeat(100)).chars().count(), MAX_NAME_CHARS);
        assert!(list_path(&dir, "../../evil").is_none());
        assert!(list_path(&dir, "a/b").is_none());
        assert!(list_path(&dir, "").is_none());
        // A hand-edited registry with a bad id, kind or origin is skipped, not trusted.
        fs::write(
            dir.join(REGISTRY_FILE),
            "../x\t1\tads\tfile\tbad id\nu1\t1\tmystery\tfile\tbad kind\nu2\t1\tads\turl:http://plain.example/x\tnot https\nu3\t1\tads\tfile\tfine\n",
        )
        .unwrap();
        let reloaded = Registry::load(&dir);
        assert_eq!(reloaded.sources().len(), 4);
        assert_eq!(reloaded.get("u3").unwrap().name, "fine");
    }
}
