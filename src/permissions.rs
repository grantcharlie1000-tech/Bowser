//! Per-site decisions for the camera, microphone, location and notification permissions.
//!
//! Pure logic and file I/O only. The WebView2 `PermissionRequested` handler asks [`PermissionStore::allows`]
//! for every request, and the Site information panel reads and writes the same store, so a change
//! applies to the next request. Nothing is asked of the user: a site with no stored decision is
//! blocked, which is what Bowser did before this store existed.
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
/// Largest number of stored decisions, across all sites.
pub const MAX_RULES: usize = 2048;

const HEADER: &str = "# Bowser per-site permissions: <origin> <allow|block> <permission>.\n\
# Permissions: microphone, camera, geolocation, notifications. A site without a line is blocked.\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Permission {
    Microphone,
    Camera,
    Geolocation,
    Notifications,
}

impl Permission {
    pub const ALL: [Self; 4] = [
        Self::Microphone,
        Self::Camera,
        Self::Geolocation,
        Self::Notifications,
    ];

    /// The word used in `permissions.txt`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Microphone => "microphone",
            Self::Camera => "camera",
            Self::Geolocation => "geolocation",
            Self::Notifications => "notifications",
        }
    }

    /// The row label shown in the Site information panel.
    pub fn label(self) -> &'static str {
        match self {
            Self::Microphone => "Microphone",
            Self::Camera => "Camera",
            Self::Geolocation => "Location",
            Self::Notifications => "Notifications",
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
    rules: Rules,
}

impl PermissionStore {
    /// Loads `path`. A missing, unreadable or oversized file gives an empty store, so every site is
    /// blocked until the user chooses otherwise.
    pub fn load(path: &Path) -> Self {
        let rules = read_capped(path)
            .map(|text| parse(&text))
            .unwrap_or_default();
        Self {
            path: Some(path.to_path_buf()),
            rules,
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

    /// The four permissions for one origin, in [`Permission::ALL`] order.
    pub fn site_decisions(&self, origin: &str) -> [Option<Decision>; 4] {
        Permission::ALL.map(|permission| self.decision(origin, permission))
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
            [None, Some(Decision::Block), None, None]
        );
        store
            .set("https://a.test", Permission::Camera, None)
            .unwrap();
        assert_eq!(store.site_decisions("https://a.test"), [None; 4]);
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
        assert_eq!(store.site_decisions("https://a.test"), [None; 4]);
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
    fn the_rule_count_is_capped() {
        let mut text = String::new();
        for index in 0..MAX_RULES + 10 {
            text.push_str(&format!("https://site{index}.test allow camera\n"));
        }
        assert_eq!(parse(&text).len(), MAX_RULES);
    }
}
