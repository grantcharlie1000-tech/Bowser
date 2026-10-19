//! Per-site decisions for the camera, microphone, location, notification and popup permissions.
//!
//! Pure logic and file I/O only. The WebView2 `PermissionRequested` handler asks [`PermissionStore::allows`]
//! for every request, and the Site information panel and the Privacy dashboard read and write the
//! same store, so a change applies to the next request. Nothing is asked of the user: a site with no
//! stored decision is blocked, which is what Bowser did before this store existed. Popups are not a
//! WebView2 permission: the new-window handler asks the same store whether a window a page opened
//! from script may become a new tab. A user-initiated http(s) link click opens a tab regardless.
//!
//! The file is `permissions.txt` in the profile directory, one decision per line:
//!
//! ```text
//! https://meet.example allow microphone
//! http://localhost:8080 block camera
//! ```
//!
//! Each line is `<origin> <decision> <permission>`. The origin is the page's scheme, host and port
//! (`https://example.com`, with the default port dropped). Blank lines, `#` comments and malformed
//! lines are ignored, and the file is capped at [`MAX_FILE_BYTES`].

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use url::Url;

pub const FILE_NAME: &str = "permissions.txt";
pub const MAX_FILE_BYTES: usize = 256 * 1024;
/// Why a change was refused when the file could not be read: saving would replace every stored line.
const UNREADABLE: &str = "the site permissions file could not be read, so it was not changed";
/// Largest number of stored decisions, across all sites.
pub const MAX_RULES: usize = 2048;
/// Longest origin text accepted from the user interface.
pub const MAX_ORIGIN_CHARS: usize = 256;
/// How many denied requests the in-memory log keeps.
pub const DENIED_LOG_CAP: usize = 50;
/// Number of permission kinds, the length of [`Permission::ALL`].
pub const PERMISSION_COUNT: usize = 5;

const HEADER: &str = "# Bowser per-site permissions: <origin> <allow|block> <permission>.\n\
# Permissions: microphone, camera, geolocation, notifications, popups. A site without a line is blocked.\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Permission {
    Microphone,
    Camera,
    Geolocation,
    Notifications,
    /// Windows a page opens from script, with no click behind them.
    Popups,
}

impl Permission {
    pub const ALL: [Self; PERMISSION_COUNT] = [
        Self::Microphone,
        Self::Camera,
        Self::Geolocation,
        Self::Notifications,
        Self::Popups,
    ];

    /// The word used in `permissions.txt`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Microphone => "microphone",
            Self::Camera => "camera",
            Self::Geolocation => "geolocation",
            Self::Notifications => "notifications",
            Self::Popups => "popups",
        }
    }

    /// The kind name on the Privacy dashboard wire. It differs from [`Self::name`] only for location.
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Geolocation => "location",
            other => other.name(),
        }
    }

    pub fn from_wire(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.wire_name() == text)
    }

    /// The row label shown in the Site information panel.
    pub fn label(self) -> &'static str {
        match self {
            Self::Microphone => "Microphone",
            Self::Camera => "Camera",
            Self::Geolocation => "Location",
            Self::Notifications => "Notifications",
            Self::Popups => "Popups",
        }
    }

    pub fn from_name(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == text)
    }
}

/// A stored answer. A site with no stored answer is blocked, so there is no separate "ask" state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Block,
}

impl Decision {
    pub fn name(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Block => "block",
        }
    }

    fn from_name(text: &str) -> Option<Self> {
        match text {
            "allow" => Some(Self::Allow),
            "block" => Some(Self::Block),
            _ => None,
        }
    }
}

/// `text` when it is already a canonical http(s) origin (`origin_of(text) == text`, within
/// [`MAX_ORIGIN_CHARS`]). The interface sends origins it received from Rust, so anything else, such
/// as a URL with a path, an uppercase host or another scheme, is refused rather than repaired.
pub fn canonical_origin(text: &str) -> Option<String> {
    if text.is_empty() || text.chars().count() > MAX_ORIGIN_CHARS {
        return None;
    }
    origin_of(text).filter(|origin| origin == text)
}

/// The canonical origin (`scheme://host[:port]`) of an http or https URL, or `None` for any other
/// scheme, opaque origins and unparsable text. Host case and default ports are normalised, so
/// `HTTPS://Example.com:443/a` and `https://example.com/b` share one entry.
pub fn origin_of(uri: &str) -> Option<String> {
    let url = Url::parse(uri).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let origin = url.origin().ascii_serialization();
    (origin != "null").then_some(origin)
}

type Rules = BTreeMap<String, BTreeMap<Permission, Decision>>;

/// Stored decisions, keyed by canonical origin. Changes are written to disk before they count, so the
/// in-memory store always matches the file.
#[derive(Debug, Default)]
pub struct PermissionStore {
    path: Option<PathBuf>,
    /// The file is there but could not be read, so nothing is saved over it.
    unreadable: bool,
    rules: Rules,
}

impl PermissionStore {
    /// Loads `path`. A missing, unreadable or oversized file gives an empty store, so every site is
    /// blocked until the user chooses otherwise. An unreadable or oversized file's store refuses every
    /// change, so the file is never overwritten.
    pub fn load(path: &Path) -> Self {
        let text = read_capped(path);
        Self {
            path: Some(path.to_path_buf()),
            rules: text.as_deref().map(parse).unwrap_or_default(),
            unreadable: text.is_none(),
        }
    }

    /// The stored decision for an origin, or `None` when nothing is stored.
    pub fn decision(&self, origin: &str, permission: Permission) -> Option<Decision> {
        self.rules.get(origin)?.get(&permission).copied()
    }

    /// Whether a request from `uri` for `permission` may be granted. Unknown origins, other schemes
    /// and unset permissions are all refused.
    pub fn allows(&self, uri: &str, permission: Permission) -> bool {
        origin_of(uri)
            .and_then(|origin| self.decision(&origin, permission))
            .is_some_and(|decision| decision == Decision::Allow)
    }

    /// Every permission for one origin, in [`Permission::ALL`] order.
    pub fn site_decisions(&self, origin: &str) -> [Option<Decision>; PERMISSION_COUNT] {
        Permission::ALL.map(|permission| self.decision(origin, permission))
    }

    /// Every origin with at least one stored decision, in order.
    pub fn origins(&self) -> impl Iterator<Item = &str> {
        self.rules.keys().map(String::as_str)
    }

    /// How many decisions are stored, across all sites.
    pub fn rule_count(&self) -> usize {
        self.rules.values().map(BTreeMap::len).sum()
    }

    /// Every explicitly stored decision, grouped deterministically by origin then kind.
    pub fn all_decisions(&self) -> Vec<(String, Permission, Decision)> {
        self.rules
            .iter()
            .flat_map(|(origin, decisions)| {
                decisions
                    .iter()
                    .map(move |(permission, decision)| (origin.clone(), *permission, *decision))
            })
            .collect()
    }

    /// Stores `decision` for one origin and permission, or clears it with `None`, then saves. On a
    /// failed save the previous value is restored and the error returned.
    pub fn set(
        &mut self,
        origin: &str,
        permission: Permission,
        decision: Option<Decision>,
    ) -> io::Result<()> {
        let previous = self.decision(origin, permission);
        if previous == decision {
            return Ok(());
        }
        if previous.is_none() && decision.is_some() && self.rule_count() >= MAX_RULES {
            return Err(io::Error::other("too many stored site permissions"));
        }
        self.write_rule(origin, permission, decision);
        if let Err(error) = self.save() {
            self.write_rule(origin, permission, previous);
            return Err(error);
        }
        Ok(())
    }

    /// Clears every stored decision for one origin. Returns how many were removed.
    pub fn reset_site(&mut self, origin: &str) -> io::Result<usize> {
        let Some(previous) = self.rules.remove(origin) else {
            return Ok(0);
        };
        let removed = previous.len();
        if let Err(error) = self.save() {
            self.rules.insert(origin.to_owned(), previous);
            return Err(error);
        }
        Ok(removed)
    }

    /// Clears every stored decision, for every site. Returns how many were removed.
    pub fn reset_all(&mut self) -> io::Result<usize> {
        if self.rules.is_empty() {
            return Ok(0);
        }
        let removed = self.rule_count();
        let previous = std::mem::take(&mut self.rules);
        if let Err(error) = self.save() {
            self.rules = previous;
            return Err(error);
        }
        Ok(removed)
    }

    fn write_rule(&mut self, origin: &str, permission: Permission, decision: Option<Decision>) {
        match decision {
            Some(decision) => {
                self.rules
                    .entry(origin.to_owned())
                    .or_default()
                    .insert(permission, decision);
            }
            None => {
                if let Some(site) = self.rules.get_mut(origin) {
                    site.remove(&permission);
                    if site.is_empty() {
                        self.rules.remove(origin);
                    }
                }
            }
        }
    }

    fn save(&self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if self.unreadable {
            return Err(io::Error::other(UNREADABLE));
        }
        write_atomic(path, serialise(&self.rules).as_bytes())
    }
}

fn parse(text: &str) -> Rules {
    let mut rules = Rules::new();
    let mut count = 0;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let (Some(origin), Some(decision), Some(permission), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let (Some(origin), Some(decision), Some(permission)) = (
            origin_of(origin),
            Decision::from_name(decision),
            Permission::from_name(permission),
        ) else {
            continue;
        };
        if count >= MAX_RULES {
            break;
        }
        let site = rules.entry(origin).or_default();
        if site.insert(permission, decision).is_none() {
            count += 1;
        }
    }
    rules
}

fn serialise(rules: &Rules) -> String {
    let mut text = String::from(HEADER);
    for (origin, site) in rules {
        for (permission, decision) in site {
            text.push_str(&format!(
                "{origin} {} {}\n",
                decision.name(),
                permission.name()
            ));
        }
    }
    text
}

/// The file's text: empty when there is no file. A byte that is not UTF-8 (a hand edit saved as
/// ANSI) spoils only its own line, never every other one. `None` when a file is there but cannot be
/// read (locked, oversized): the store then refuses to save, so a change cannot replace what is in it.
fn read_capped(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Some(String::new()),
        Err(_) => return None,
    };
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_FILE_BYTES {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = path.with_extension("txt.tmp");
    let mut file = File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path)
}

/// One denied request, as the Privacy dashboard lists it. Only the site's origin, the kind and the
/// time are kept: never a URL path, title or any page content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeniedAttempt {
    pub origin: String,
    pub kind: Permission,
    /// Seconds since the Unix epoch of the most recent request.
    pub at_secs: u64,
    /// How many identical requests in a row this entry stands for (at most [`MAX_DENIED_REPEATS`]).
    pub count: u32,
}

/// Cap on [`DeniedAttempt::count`], so a page that asks in a loop cannot overflow the counter.
pub const MAX_DENIED_REPEATS: u32 = 999;

/// The most recent denied requests, newest first. Held in memory only: it is never written to disk
/// and is gone when Bowser exits.
#[derive(Debug, Default)]
pub struct DeniedLog {
    entries: std::collections::VecDeque<DeniedAttempt>,
}

impl DeniedLog {
    /// Records a denied request. A request identical to the newest entry (same origin and kind)
    /// is folded into it, so a page that asks on every load does not push everything else out.
    /// Origins that are not canonical http(s) origins are ignored.
    pub fn record(&mut self, origin: &str, kind: Permission, now_secs: u64) {
        if canonical_origin(origin).is_none() {
            return;
        }
        if let Some(newest) = self.entries.front_mut()
            && newest.origin == origin
            && newest.kind == kind
        {
            newest.count = (newest.count + 1).min(MAX_DENIED_REPEATS);
            newest.at_secs = now_secs;
            return;
        }
        self.entries.push_front(DeniedAttempt {
            origin: origin.to_owned(),
            kind,
            at_secs: now_secs,
            count: 1,
        });
        self.entries.truncate(DENIED_LOG_CAP);
    }

    /// Newest first.
    pub fn recent(&self) -> impl Iterator<Item = &DeniedAttempt> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Seconds since the Unix epoch, or 0 when the clock is before it.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bowser-permissions-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join(FILE_NAME)
    }

    #[test]
    fn origins_are_scheme_host_and_port_with_default_ports_dropped() {
        assert_eq!(
            origin_of("https://Example.com/a?b#c").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            origin_of("HTTPS://Example.com:443/").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            origin_of("http://localhost:8080/meet").as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(
            origin_of("https://example.com:8443/").as_deref(),
            Some("https://example.com:8443")
        );
    }

    #[test]
    fn only_web_origins_are_permission_origins() {
        for uri in [
            "file:///C:/Users/charlie/page.html",
            "about:blank",
            "data:text/html,hi",
            "edge://settings",
            "chrome://gpu",
            "not a url",
            "",
        ] {
            assert_eq!(origin_of(uri), None, "{uri}");
        }
    }

    #[test]
    fn scheme_and_port_make_different_origins() {
        assert_ne!(
            origin_of("http://example.com").unwrap(),
            origin_of("https://example.com").unwrap()
        );
        assert_ne!(
            origin_of("https://example.com").unwrap(),
            origin_of("https://example.com:8443").unwrap()
        );
    }

    #[test]
    fn parse_skips_comments_blanks_and_malformed_lines() {
        let rules = parse(
            "# comment\n\n\
             https://a.test allow camera\n\
             https://a.test block camera\n\
             https://a.test allow\n\
             https://a.test allow camera extra\n\
             file:///x.html allow camera\n\
             https://a.test ask microphone\n\
             https://a.test allow fingerprinting\n\
             https://B.test allow microphone\n",
        );
        assert_eq!(rules.len(), 2);
        assert_eq!(
            rules["https://a.test"][&Permission::Camera],
            Decision::Block,
            "a later line replaces an earlier one"
        );
        assert_eq!(
            rules["https://b.test"][&Permission::Microphone],
            Decision::Allow
        );
    }

    #[test]
    fn serialise_round_trips_and_is_sorted() {
        let mut rules = Rules::new();
        rules
            .entry("https://z.test".to_owned())
            .or_default()
            .insert(Permission::Notifications, Decision::Allow);
        rules
            .entry("https://a.test:8443".to_owned())
            .or_default()
            .insert(Permission::Geolocation, Decision::Block);
        let text = serialise(&rules);
        assert!(text.ends_with(
            "https://a.test:8443 block geolocation\nhttps://z.test allow notifications\n"
        ));
        assert_eq!(parse(&text), rules);
    }

    #[test]
    fn lookup_matches_only_the_exact_origin_and_permission() {
        let path = temp_file("lookup");
        let mut store = PermissionStore::load(&path);
        store
            .set(
                "https://meet.test",
                Permission::Camera,
                Some(Decision::Allow),
            )
            .unwrap();
        assert!(store.allows("https://meet.test/room", Permission::Camera));
        assert!(!store.allows("https://meet.test/room", Permission::Microphone));
        assert!(!store.allows("http://meet.test/room", Permission::Camera));
        assert!(!store.allows("https://meet.test:8443/room", Permission::Camera));
        assert!(!store.allows("https://sub.meet.test/room", Permission::Camera));
        assert!(!store.allows("file:///C:/page.html", Permission::Camera));
        assert_eq!(
            store.decision("https://meet.test", Permission::Microphone),
            None,
            "an unset permission is not a block entry"
        );
    }

    #[test]
    fn a_block_entry_refuses_and_clearing_it_removes_the_line() {
        let path = temp_file("clear");
        let mut store = PermissionStore::load(&path);
        store
            .set("https://a.test", Permission::Camera, Some(Decision::Block))
            .unwrap();
        assert!(!store.allows("https://a.test", Permission::Camera));
        assert_eq!(
            store.site_decisions("https://a.test"),
            [None, Some(Decision::Block), None, None, None]
        );
        store
            .set("https://a.test", Permission::Camera, None)
            .unwrap();
        assert_eq!(store.site_decisions("https://a.test"), [None; PERMISSION_COUNT]);
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            !text.lines().any(|line| line.starts_with("https://")),
            "{text}"
        );
    }

    #[test]
    fn decisions_persist_across_a_reload() {
        let path = temp_file("persist");
        {
            let mut store = PermissionStore::load(&path);
            store
                .set(
                    "https://a.test",
                    Permission::Microphone,
                    Some(Decision::Allow),
                )
                .unwrap();
            store
                .set(
                    "http://localhost:8080",
                    Permission::Geolocation,
                    Some(Decision::Allow),
                )
                .unwrap();
        }
        let reloaded = PermissionStore::load(&path);
        assert!(reloaded.allows("https://a.test", Permission::Microphone));
        assert!(reloaded.allows("http://localhost:8080/x", Permission::Geolocation));
        assert!(!reloaded.allows("https://a.test", Permission::Camera));
    }

    #[test]
    fn reset_site_clears_only_that_origin() {
        let path = temp_file("reset");
        let mut store = PermissionStore::load(&path);
        store
            .set("https://a.test", Permission::Camera, Some(Decision::Allow))
            .unwrap();
        store
            .set(
                "https://a.test",
                Permission::Notifications,
                Some(Decision::Block),
            )
            .unwrap();
        store
            .set("https://b.test", Permission::Camera, Some(Decision::Allow))
            .unwrap();
        assert_eq!(store.reset_site("https://a.test").unwrap(), 2);
        assert_eq!(store.reset_site("https://a.test").unwrap(), 0);
        let reloaded = PermissionStore::load(&path);
        assert!(!reloaded.allows("https://a.test", Permission::Camera));
        assert!(reloaded.allows("https://b.test", Permission::Camera));
    }

    #[test]
    fn all_decisions_lists_stored_sites_by_origin_and_permission() {
        let path = temp_file("all-decisions");
        let mut store = PermissionStore::load(&path);
        store
            .set("https://b.test", Permission::Camera, Some(Decision::Allow))
            .unwrap();
        store
            .set(
                "https://a.test",
                Permission::Microphone,
                Some(Decision::Block),
            )
            .unwrap();

        assert_eq!(
            store.all_decisions(),
            vec![
                (
                    "https://a.test".to_owned(),
                    Permission::Microphone,
                    Decision::Block
                ),
                (
                    "https://b.test".to_owned(),
                    Permission::Camera,
                    Decision::Allow
                ),
            ]
        );
    }

    #[test]
    fn a_failed_save_restores_the_previous_decision() {
        let dir =
            std::env::temp_dir().join(format!("bowser-permissions-{}-missing", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut store = PermissionStore::load(&dir.join(FILE_NAME));
        let result = store.set("https://a.test", Permission::Camera, Some(Decision::Allow));
        assert!(result.is_err(), "the parent directory does not exist");
        assert!(!store.allows("https://a.test", Permission::Camera));
        assert_eq!(store.site_decisions("https://a.test"), [None; PERMISSION_COUNT]);
    }

    #[test]
    fn oversized_or_unreadable_files_give_an_empty_store() {
        let path = temp_file("oversized");
        let mut big = String::new();
        while big.len() <= MAX_FILE_BYTES {
            big.push_str("https://a.test allow camera\n");
        }
        fs::write(&path, big).unwrap();
        let store = PermissionStore::load(&path);
        assert!(!store.allows("https://a.test", Permission::Camera));

        let missing = PermissionStore::load(&path.with_file_name("absent.txt"));
        assert!(!missing.allows("https://a.test", Permission::Camera));
    }

    #[test]
    fn one_byte_that_is_not_utf8_keeps_every_other_permission_and_the_next_change_keeps_them_too() {
        let path = temp_file("bad-byte");
        let mut bytes = b"https://meet.test allow camera\n# caf".to_vec();
        bytes.extend_from_slice(&[0xE9, b'\n']);
        fs::write(&path, bytes).unwrap();
        let mut store = PermissionStore::load(&path);
        assert!(store.allows("https://meet.test/", Permission::Camera), "the valid line is read");
        store.set("https://b.test", Permission::Camera, Some(Decision::Block)).unwrap();
        let reloaded = PermissionStore::load(&path);
        assert!(reloaded.allows("https://meet.test/", Permission::Camera), "a later change keeps it");
        assert_eq!(reloaded.decision("https://b.test", Permission::Camera), Some(Decision::Block));
    }

    #[test]
    fn a_file_that_cannot_be_read_is_never_overwritten_by_a_change() {
        let path = temp_file("oversized-kept");
        let mut big = String::new();
        while big.len() <= MAX_FILE_BYTES {
            big.push_str("https://a.test allow camera\n");
        }
        fs::write(&path, &big).unwrap();
        let mut store = PermissionStore::load(&path);
        assert!(store.set("https://b.test", Permission::Camera, Some(Decision::Allow)).is_err());
        assert_eq!(store.decision("https://b.test", Permission::Camera), None);
        assert_eq!(fs::read_to_string(&path).unwrap(), big, "the file is unchanged");
        let mut missing = PermissionStore::load(&path.with_file_name("absent.txt"));
        missing.set("https://b.test", Permission::Camera, Some(Decision::Allow)).unwrap();
    }

    #[test]
    fn popups_are_a_stored_kind_that_defaults_to_blocked() {
        let path = temp_file("popups");
        let mut store = PermissionStore::load(&path);
        assert!(!store.allows("https://a.test/", Permission::Popups));
        store
            .set("https://a.test", Permission::Popups, Some(Decision::Allow))
            .unwrap();
        assert!(store.allows("https://a.test/page", Permission::Popups));
        assert!(!store.allows("https://b.test/", Permission::Popups));
        assert!(
            !store.allows("https://a.test/", Permission::Camera),
            "a popup decision grants nothing else"
        );
        let reloaded = PermissionStore::load(&path);
        assert!(reloaded.allows("https://a.test/", Permission::Popups));
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("https://a.test allow popups\n"), "{text}");
        // A file written by this version stays readable: unknown kinds are skipped, not fatal.
        assert_eq!(parse("https://a.test allow popups\nhttps://a.test allow bluetooth\n").len(), 1);
    }

    #[test]
    fn wire_names_round_trip_and_only_location_differs() {
        for kind in Permission::ALL {
            assert_eq!(Permission::from_wire(kind.wire_name()), Some(kind));
        }
        assert_eq!(Permission::Geolocation.wire_name(), "location");
        assert_eq!(Permission::Geolocation.name(), "geolocation");
        assert_eq!(Permission::from_wire("geolocation"), None);
        assert_eq!(Permission::from_wire("mic"), None);
        assert_eq!(Permission::from_wire(""), None);
        assert_eq!(Permission::from_wire("Camera"), None);
    }

    #[test]
    fn canonical_origin_accepts_only_origins_already_in_canonical_form() {
        assert_eq!(
            canonical_origin("https://example.com").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            canonical_origin("http://localhost:8080").as_deref(),
            Some("http://localhost:8080")
        );
        for text in [
            "",
            "https://example.com/",
            "https://example.com/path",
            "https://Example.com",
            "https://example.com:443",
            "file:///C:/x",
            "ftp://example.com",
            "example.com",
            "https://example.com?x=1",
            "https://user@example.com",
        ] {
            assert_eq!(canonical_origin(text), None, "{text}");
        }
        let long = format!("https://{}.test", "a".repeat(MAX_ORIGIN_CHARS));
        assert_eq!(canonical_origin(&long), None, "over the length cap");
    }

    #[test]
    fn reset_all_clears_every_site_and_persists() {
        let path = temp_file("reset-all");
        let mut store = PermissionStore::load(&path);
        assert_eq!(store.reset_all().unwrap(), 0);
        store
            .set("https://a.test", Permission::Camera, Some(Decision::Allow))
            .unwrap();
        store
            .set("https://b.test", Permission::Popups, Some(Decision::Block))
            .unwrap();
        assert_eq!(store.rule_count(), 2);
        assert_eq!(store.origins().collect::<Vec<_>>(), ["https://a.test", "https://b.test"]);
        assert_eq!(store.reset_all().unwrap(), 2);
        assert_eq!(store.rule_count(), 0);
        let reloaded = PermissionStore::load(&path);
        assert_eq!(reloaded.rule_count(), 0);
    }

    #[test]
    fn a_failed_reset_all_keeps_the_rules() {
        let dir = std::env::temp_dir().join(format!(
            "bowser-permissions-{}-reset-all-fail",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        let mut store = PermissionStore::load(&path);
        store
            .set("https://a.test", Permission::Camera, Some(Decision::Allow))
            .unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert!(store.reset_all().is_err());
        assert!(store.allows("https://a.test", Permission::Camera));
    }

    #[test]
    fn set_refuses_a_new_decision_past_the_rule_cap() {
        let mut store = PermissionStore::default();
        for index in 0..MAX_RULES {
            store
                .set(
                    &format!("https://site{index}.test"),
                    Permission::Camera,
                    Some(Decision::Allow),
                )
                .unwrap();
        }
        assert!(store
            .set("https://extra.test", Permission::Camera, Some(Decision::Allow))
            .is_err());
        assert!(!store.allows("https://extra.test", Permission::Camera));
        // Changing or clearing an existing decision still works at the cap.
        store
            .set("https://site0.test", Permission::Camera, Some(Decision::Block))
            .unwrap();
        store
            .set("https://site1.test", Permission::Camera, None)
            .unwrap();
        assert_eq!(store.rule_count(), MAX_RULES - 1);
    }

    #[test]
    fn denied_log_keeps_the_newest_fifty_and_never_a_path() {
        let mut log = DeniedLog::default();
        for index in 0..DENIED_LOG_CAP + 10 {
            log.record(&format!("https://site{index}.test"), Permission::Camera, index as u64);
        }
        assert_eq!(log.len(), DENIED_LOG_CAP);
        let newest = log.recent().next().unwrap();
        assert_eq!(newest.origin, format!("https://site{}.test", DENIED_LOG_CAP + 9));
        assert_eq!(log.recent().last().unwrap().origin, "https://site10.test");
        // Anything that is not a canonical origin (a URL with a path, a file page) is not recorded.
        let before = log.len();
        log.record("https://a.test/secret/path?q=1", Permission::Camera, 1);
        log.record("file:///C:/x.html", Permission::Camera, 1);
        log.record("", Permission::Camera, 1);
        assert_eq!(log.len(), before);
        log.clear();
        assert!(log.is_empty());
    }

    #[test]
    fn denied_log_folds_an_identical_newest_request_and_caps_the_count() {
        let mut log = DeniedLog::default();
        log.record("https://a.test", Permission::Notifications, 10);
        log.record("https://a.test", Permission::Notifications, 12);
        log.record("https://a.test", Permission::Camera, 13);
        log.record("https://a.test", Permission::Notifications, 14);
        let entries: Vec<_> = log.recent().cloned().collect();
        assert_eq!(entries.len(), 3, "only the newest entry is folded into");
        assert_eq!(entries[2].count, 2);
        assert_eq!(entries[2].at_secs, 12);
        assert_eq!(entries[0].count, 1);
        for _ in 0..2000 {
            log.record("https://a.test", Permission::Notifications, 20);
        }
        assert_eq!(log.recent().next().unwrap().count, MAX_DENIED_REPEATS);
    }

    #[test]
    fn the_rule_count_is_capped() {
        let mut text = String::new();
        for index in 0..MAX_RULES + 10 {
            text.push_str(&format!("https://site{index}.test allow camera\n"));
        }
        assert_eq!(parse(&text).len(), MAX_RULES);
    }
}
