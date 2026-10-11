//! Smart tab group primitives: registrable-domain keys and per-domain group
//! preferences.
//!
//! This module is pure metadata. It never touches tabs, WebViews, or timers.
//! Keys are derived only from a committed HTTP(S) [`Url`] using the static
//! Public Suffix List embedded in the `psl` crate (private suffixes included),
//! so no network lookup or refresh ever happens.
//!
//! [`GroupPreferences`] stores only a per-domain [`GroupMode`] and collapsed
//! flag. Load it once at startup with [`GroupPreferences::load`] and call
//! [`GroupPreferences::save`] after an explicit user change.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use crate::tabs::TabId;
use url::{Host, Url};

/// File name within Bowser's local settings directory (`%LOCALAPPDATA%\Bowser`).
pub const FILE_NAME: &str = "tab-groups.txt";
/// Largest settings file that will be read or written.
pub const MAX_FILE_BYTES: usize = 256 * 1024;
/// Largest number of non-default domain entries kept.
pub const MAX_ENTRIES: usize = 512;

const HEADER: &str = "bowser-tab-groups 1";
const MAX_DOMAIN_BYTES: usize = 253;

/// Canonical registrable domain (for example `example.co.uk`) in lowercase
/// ASCII/punycode. A `DomainKey` can only be built through validation, so it
/// can never hold a URL, path, port, credentials, or an IP address.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DomainKey(String);

impl DomainKey {
    /// Key for a committed HTTP(S) URL, or `None` for non-web schemes, IP
    /// addresses, single-label hosts, and hosts with no registrable domain.
    /// Scheme and port never affect the key.
    pub fn from_url(url: &Url) -> Option<Self> {
        if !matches!(url.scheme(), "http" | "https") {
            return None;
        }
        match url.host()? {
            Host::Domain(host) => Self::from_host(host),
            Host::Ipv4(_) | Host::Ipv6(_) => None,
        }
    }

    /// Parses an already-canonical key, as stored in the settings file.
    /// Rejects anything that is not exactly its own registrable domain.
    pub fn parse(text: &str) -> Option<Self> {
        let key = Self::from_host(text)?;
        (key.0 == text).then_some(key)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn from_host(host: &str) -> Option<Self> {
        // One trailing dot denotes a fully qualified name; it is not part of
        // the grouping identity.
        let host = host.strip_suffix('.').unwrap_or(host);
        if host.is_empty()
            || host.len() > MAX_DOMAIN_BYTES
            || !host.contains('.')
            || !host.split('.').all(valid_label)
            // psl expects lowercase ASCII; `Url` already supplies it.
            || host.bytes().any(|byte| byte.is_ascii_uppercase())
        {
            return None;
        }
        // `None` when the host is itself a public suffix.
        let domain = psl::domain(host.as_bytes())?;
        let domain = std::str::from_utf8(domain.as_bytes()).ok()?;
        Some(Self(domain.to_owned()))
    }
}

impl fmt::Display for DomainKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// How a domain's tabs are grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupMode {
    /// Group once at least two unpinned tabs share the domain.
    #[default]
    Auto,
    /// Group even a single unpinned tab.
    Always,
    /// Never group this domain.
    Never,
}

impl GroupMode {
    /// Whether `unpinned_members` tabs of one domain form a group. Callers
    /// must exclude pinned tabs from the count.
    pub fn forms_group(self, unpinned_members: usize) -> bool {
        match self {
            Self::Auto => unpinned_members >= 2,
            Self::Always => unpinned_members >= 1,
            Self::Never => false,
        }
    }

    fn token(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }

    fn from_token(token: &str) -> Option<Self> {
        match token {
            "auto" => Some(Self::Auto),
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Entry {
    mode: GroupMode,
    collapsed: bool,
}

impl Entry {
    fn is_default(self) -> bool {
        self == Self::default()
    }
}

#[derive(Debug)]
pub enum PreferencesError {
    /// Setting this would exceed [`MAX_ENTRIES`] non-default domains.
    TooManyEntries,
    /// The file is larger than [`MAX_FILE_BYTES`].
    TooLarge,
    /// The file is not a valid version-1 preferences file.
    Malformed,
    Io(io::Error),
}

impl fmt::Display for PreferencesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyEntries => write!(f, "Too many tab group preferences."),
            Self::TooLarge => write!(f, "The tab group preferences file is too large."),
            Self::Malformed => write!(f, "The tab group preferences file is not valid."),
            Self::Io(_) => write!(f, "The tab group preferences could not be read or saved."),
        }
    }
}

impl std::error::Error for PreferencesError {}

impl From<io::Error> for PreferencesError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Per-domain grouping mode and collapsed state. Domains left at the default
/// (Auto, expanded) are not stored.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupPreferences {
    entries: BTreeMap<DomainKey, Entry>,
}

impl GroupPreferences {
    pub fn mode(&self, key: &DomainKey) -> GroupMode {
        self.entries.get(key).map_or(GroupMode::Auto, |e| e.mode)
    }

    pub fn is_collapsed(&self, key: &DomainKey) -> bool {
        self.entries.get(key).is_some_and(|e| e.collapsed)
    }

    /// Whether `unpinned_members` tabs of `key` form a group under the user's
    /// mode for that domain.
    pub fn forms_group(&self, key: &DomainKey, unpinned_members: usize) -> bool {
        self.mode(key).forms_group(unpinned_members)
    }

    /// Sets the mode; returns whether anything changed (and so needs saving).
    pub fn set_mode(&mut self, key: &DomainKey, mode: GroupMode) -> Result<bool, PreferencesError> {
        self.update(key, |entry| entry.mode = mode)
    }

    /// Sets the collapsed flag; returns whether anything changed.
    pub fn set_collapsed(
        &mut self,
        key: &DomainKey,
        collapsed: bool,
    ) -> Result<bool, PreferencesError> {
        self.update(key, |entry| entry.collapsed = collapsed)
    }

    fn update(
        &mut self,
        key: &DomainKey,
        change: impl FnOnce(&mut Entry),
    ) -> Result<bool, PreferencesError> {
        let before = self.entries.get(key).copied();
        let mut entry = before.unwrap_or_default();
        change(&mut entry);
        if before.unwrap_or_default() == entry {
            return Ok(false);
        }
        if entry.is_default() {
            self.entries.remove(key);
        } else {
            if before.is_none() && self.entries.len() >= MAX_ENTRIES {
                return Err(PreferencesError::TooManyEntries);
            }
            self.entries.insert(key.clone(), entry);
        }
        Ok(true)
    }

    /// Strict parser for the version-1 text format. Any deviation (header,
    /// unknown token, duplicate or unsorted domain, default-valued entry, bad
    /// domain, size or count over limits) rejects the whole file.
    pub fn parse(text: &str) -> Result<Self, PreferencesError> {
        if text.len() > MAX_FILE_BYTES {
            return Err(PreferencesError::TooLarge);
        }
        let mut lines = text.split('\n');
        if lines.next() != Some(HEADER) {
            return Err(PreferencesError::Malformed);
        }
        let mut lines = lines.peekable();
        let mut preferences = Self::default();
        let mut previous: Option<DomainKey> = None;
        while let Some(line) = lines.next() {
            if line.is_empty() && lines.peek().is_none() {
                break; // single trailing newline
            }
            let mut fields = line.split(' ');
            let (Some(domain), Some(mode), Some(state), None) =
                (fields.next(), fields.next(), fields.next(), fields.next())
            else {
                return Err(PreferencesError::Malformed);
            };
            let key = DomainKey::parse(domain).ok_or(PreferencesError::Malformed)?;
            let collapsed = match state {
                "collapsed" => true,
                "expanded" => false,
                _ => return Err(PreferencesError::Malformed),
            };
            let entry = Entry {
                mode: GroupMode::from_token(mode).ok_or(PreferencesError::Malformed)?,
                collapsed,
            };
            // Sorted-and-unique is what `serialize` emits; requiring it also
            // rejects duplicates.
            if entry.is_default()
                || preferences.entries.len() >= MAX_ENTRIES
                || previous.as_ref().is_some_and(|prev| *prev >= key)
            {
                return Err(PreferencesError::Malformed);
            }
            previous = Some(key.clone());
            preferences.entries.insert(key, entry);
        }
        Ok(preferences)
    }

    /// Canonical version-1 text. Never exceeds [`MAX_FILE_BYTES`].
    pub fn serialize(&self) -> String {
        let mut text = String::from(HEADER);
        text.push('\n');
        for (key, entry) in &self.entries {
            text.push_str(key.as_str());
            text.push(' ');
            text.push_str(entry.mode.token());
            text.push(' ');
            text.push_str(if entry.collapsed {
                "collapsed"
            } else {
                "expanded"
            });
            text.push('\n');
        }
        text
    }

    /// Reads preferences once. A missing file yields defaults; an oversized,
    /// malformed, or unreadable file is an error.
    pub fn read(path: &Path) -> Result<Self, PreferencesError> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(PreferencesError::TooLarge);
        }
        let text = String::from_utf8(bytes).map_err(|_| PreferencesError::Malformed)?;
        Self::parse(&text)
    }

    /// Startup load: any problem falls back to defaults. The bad file is left
    /// in place until the next [`save`](Self::save) replaces it.
    pub fn load(path: &Path) -> Self {
        Self::read(path).unwrap_or_default()
    }

    /// Atomically replaces `path`: writes and flushes a sibling temporary
    /// file, then renames it over the target. Call only after a user change.
    pub fn save(&self, path: &Path) -> Result<(), PreferencesError> {
        let text = self.serialize();
        debug_assert!(text.len() <= MAX_FILE_BYTES);
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let mut temp = path.as_os_str().to_owned();
        temp.push(".tmp");
        let temp = std::path::PathBuf::from(temp);
        let written = (|| {
            let mut file = File::create(&temp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp, path)
        })();
        if written.is_err() {
            let _ = fs::remove_file(&temp);
        }
        Ok(written?)
    }
}

/// One tab's immutable grouping inputs for the current committed address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabGroupInput {
    pub id: TabId,
    pub domain: Option<DomainKey>,
    pub pinned: bool,
    /// Immediate opener, when this tab was created from a page's new-window request.
    pub opener: Option<TabId>,
}

/// Identity of a projected tab group.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TabGroupKey {
    /// Group formed by tabs sharing a registrable domain.
    Domain(DomainKey),
    /// Group formed by a tab and tabs opened from it, keyed by the smallest tab id in the group.
    Opener(TabId),
}

/// One visible island in the strip. A group's members are emitted as one contiguous run at its
/// first member's position, in model order, even when other tabs sit between them in the model; a
/// collapsed group has one slot at that position and represents the active member when the active
/// tab belongs to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripSlot {
    /// Index in the unmodified tab model whose title/favicon this island represents.
    pub tab_index: usize,
    /// Present when this island participates in a group.
    pub domain: Option<DomainKey>,
    /// Present when this island participates in either a domain or opener group.
    pub group_key: Option<TabGroupKey>,
    /// Number of unpinned group members, including this tab where applicable.
    pub group_size: usize,
    /// True for the one member island carrying the group's collapse/expand control.
    pub group_control: bool,
    /// True when this one slot stands in for a collapsed group.
    pub collapsed: bool,
}

/// Projects ordered model tabs into visible tab-strip slots without mutating or
/// reordering the source tabs.
pub fn project_strip(
    tabs: &[TabGroupInput],
    active_index: usize,
    preferences: &GroupPreferences,
) -> Vec<StripSlot> {
    let groups = opener_groups(tabs)
        .into_iter()
        .chain(domain_groups(tabs, preferences))
        .collect::<BTreeMap<_, _>>();
    let mut group_for_index = vec![None; tabs.len()];
    for (key, indices) in &groups {
        for &index in indices {
            group_for_index[index] = Some(key);
        }
    }
    let mut emitted_groups = std::collections::BTreeSet::new();
    let mut slots = Vec::with_capacity(tabs.len());
    for index in 0..tabs.len() {
        let Some(group_key) = group_for_index[index] else {
            slots.push(StripSlot {
                tab_index: index,
                domain: None,
                group_key: None,
                group_size: 1,
                group_control: false,
                collapsed: false,
            });
            continue;
        };
        // The first time a group is reached is at its lowest model index, so its members are
        // emitted here as one run and later members are skipped.
        if !emitted_groups.insert(group_key) {
            continue;
        }
        let mut group_members = groups[group_key].clone();
        group_members.sort_unstable();
        let domain = match group_key {
            TabGroupKey::Domain(domain) => Some(domain.clone()),
            TabGroupKey::Opener(_) => None,
        };
        let collapsed = domain
            .as_ref()
            .is_some_and(|domain| preferences.is_collapsed(domain));
        if collapsed {
            let representative = if group_members.contains(&active_index) {
                active_index
            } else {
                group_members[0]
            };
            slots.push(StripSlot {
                tab_index: representative,
                domain,
                group_key: Some(group_key.clone()),
                group_size: group_members.len(),
                group_control: true,
                collapsed: true,
            });
        } else {
            for (position, &member) in group_members.iter().enumerate() {
                slots.push(StripSlot {
                    tab_index: member,
                    domain: domain.clone(),
                    group_key: Some(group_key.clone()),
                    group_size: group_members.len(),
                    group_control: position == 0,
                    collapsed: false,
                });
            }
        }
    }
    slots
}

/// Group identity of each tab, by model index: `None` for a tab in no group. Uses the same rules as
/// [`project_strip`], so a tab's key here matches the `group_key` of its strip slot.
pub fn group_keys(tabs: &[TabGroupInput], preferences: &GroupPreferences) -> Vec<Option<TabGroupKey>> {
    let mut keys = vec![None; tabs.len()];
    for (key, indices) in opener_groups(tabs)
        .into_iter()
        .chain(domain_groups(tabs, preferences))
    {
        for index in indices {
            keys[index] = Some(key.clone());
        }
    }
    keys
}

/// Connected components of unpinned tabs linked by opener edges. Only edges between two
/// unpinned, present tabs count, so a pinned tab never joins and a closed opener forms no edge.
/// A component of at least two tabs is a group keyed by its smallest tab id.
fn opener_groups(tabs: &[TabGroupInput]) -> Vec<(TabGroupKey, Vec<usize>)> {
    let positions = tabs
        .iter()
        .enumerate()
        .map(|(index, tab)| (tab.id, index))
        .collect::<BTreeMap<_, _>>();
    let mut parent = (0..tabs.len()).collect::<Vec<_>>();
    for (index, tab) in tabs.iter().enumerate() {
        if tab.pinned {
            continue;
        }
        let Some(opener) = tab.opener.and_then(|id| positions.get(&id).copied()) else {
            continue;
        };
        if !tabs[opener].pinned {
            union_roots(&mut parent, index, opener);
        }
    }

    let mut components = BTreeMap::<usize, Vec<usize>>::new();
    for index in (0..tabs.len()).filter(|&index| !tabs[index].pinned) {
        let root = find_root(&mut parent, index);
        components.entry(root).or_default().push(index);
    }
    components
        .into_values()
        .filter(|members| members.len() >= 2)
        .map(|members| {
            let key = members
                .iter()
                .map(|&index| tabs[index].id)
                .min()
                .expect("opener group has members");
            (TabGroupKey::Opener(key), members)
        })
        .collect()
}

/// Domain groups over unpinned tabs that are not already in an opener group. Each domain's
/// members must satisfy its group mode.
fn domain_groups(
    tabs: &[TabGroupInput],
    preferences: &GroupPreferences,
) -> Vec<(TabGroupKey, Vec<usize>)> {
    let opener_grouped = opener_groups(tabs)
        .into_iter()
        .flat_map(|(_, members)| members)
        .collect::<std::collections::BTreeSet<_>>();
    let mut members = BTreeMap::<DomainKey, Vec<usize>>::new();
    for (index, tab) in tabs.iter().enumerate() {
        if !tab.pinned
            && !opener_grouped.contains(&index)
            && let Some(domain) = &tab.domain
        {
            members.entry(domain.clone()).or_default().push(index);
        }
    }
    members
        .into_iter()
        .filter(|(domain, indices)| preferences.forms_group(domain, indices.len()))
        .map(|(domain, indices)| (TabGroupKey::Domain(domain), indices))
        .collect()
}

fn find_root(parent: &mut [usize], index: usize) -> usize {
    if parent[index] != index {
        parent[index] = find_root(parent, parent[index]);
    }
    parent[index]
}

fn union_roots(parent: &mut [usize], left: usize, right: usize) {
    let left_root = find_root(parent, left);
    let right_root = find_root(parent, right);
    if left_root != right_root {
        parent[right_root] = left_root;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir() -> PathBuf {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("bowser-tab-groups-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn key_of(url: &str) -> Option<String> {
        DomainKey::from_url(&Url::parse(url).unwrap()).map(|key| key.0)
    }

    fn key(domain: &str) -> DomainKey {
        DomainKey::parse(domain).unwrap()
    }

    fn input(domain: Option<&str>, pinned: bool) -> TabGroupInput {
        TabGroupInput {
            id: crate::tabs::TabId(0),
            domain: domain.map(|host| {
                DomainKey::from_url(&Url::parse(&format!("https://{host}/")).unwrap()).unwrap()
            }),
            pinned,
            opener: None,
        }
    }

    fn linked_input(id: u32, domain: &str, opener: Option<u32>, pinned: bool) -> TabGroupInput {
        TabGroupInput {
            id: crate::tabs::TabId(id),
            domain: Some(
                DomainKey::from_url(&Url::parse(&format!("https://{domain}/")).unwrap()).unwrap(),
            ),
            pinned,
            opener: opener.map(crate::tabs::TabId),
        }
    }

    #[test]
    fn ctrl_click_child_groups_with_parent_across_domains() {
        let tabs = [
            linked_input(10, "parent.example", None, false),
            linked_input(11, "child.other", Some(10), false),
        ];

        let slots = project_strip(&tabs, 1, &GroupPreferences::default());

        assert_eq!(
            slots.iter().map(|slot| slot.tab_index).collect::<Vec<_>>(),
            [0, 1]
        );
        assert!(slots[0].group_control);
        assert!(!slots[1].group_control);
        assert_eq!(
            slots[0].group_key,
            Some(TabGroupKey::Opener(crate::tabs::TabId(10)))
        );
        assert_eq!(slots[1].group_key, slots[0].group_key);
    }

    #[test]
    fn unrelated_different_domain_tab_does_not_join_opener_group() {
        let tabs = [
            linked_input(10, "parent.example", None, false),
            linked_input(11, "child.other", Some(10), false),
            linked_input(12, "unrelated.third", None, false),
        ];

        let slots = project_strip(&tabs, 1, &GroupPreferences::default());

        assert_eq!(slots.len(), 3);
        assert_eq!(slots[2].tab_index, 2);
        assert_eq!(slots[2].group_key, None);
    }

    #[test]
    fn child_with_cleared_opener_after_parent_close_is_ungrouped_across_domains() {
        let tabs = [linked_input(11, "child.other", None, false)];

        let slots = project_strip(&tabs, 0, &GroupPreferences::default());

        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].group_key, None);
        assert!(!slots[0].group_control);
    }

    #[test]
    fn children_of_closed_opener_keep_a_group_only_when_they_share_a_domain() {
        // Parent was closed, so its children no longer have an opener link.
        let tabs = [
            linked_input(11, "shared.other", None, false),
            linked_input(12, "shared.other", None, false),
            linked_input(13, "lonely.third", None, false),
        ];

        let slots = project_strip(&tabs, 0, &GroupPreferences::default());

        assert_eq!(slots[0].group_size, 2);
        assert_eq!(slots[1].group_size, 2);
        assert!(matches!(slots[0].group_key, Some(TabGroupKey::Domain(_))));
        assert_eq!(slots[2].group_key, None);
    }

    #[test]
    fn pinned_opener_and_child_never_join() {
        let tabs = [
            linked_input(10, "parent.example", None, true),
            linked_input(11, "child.other", Some(10), false),
            linked_input(12, "parent.example", Some(10), true),
        ];

        let slots = project_strip(&tabs, 1, &GroupPreferences::default());

        assert_eq!(slots.len(), 3);
        assert!(slots.iter().all(|slot| slot.group_key.is_none()));
        assert!(slots.iter().all(|slot| !slot.group_control));
    }

    #[test]
    fn opening_a_second_chatgpt_tab_automatically_groups_it_with_the_first() {
        let tabs = [
            input(Some("chatgpt.com"), false),
            input(Some("chatgpt.com"), false),
        ];

        let slots = project_strip(&tabs, 1, &GroupPreferences::default());

        assert_eq!(slots.len(), 2);
        assert!(slots[0].group_control);
        assert_eq!(slots[0].group_size, 2);
        assert_eq!(slots[0].domain, slots[1].domain);
        assert!(!slots[1].group_control);
    }

    #[test]
    fn projection_pulls_noncontiguous_group_members_together_without_reordering_the_model() {
        let tabs = [
            input(Some("alpha.example.com"), false),
            input(Some("beta.example.net"), false),
            input(Some("alpha.example.com"), false),
        ];

        let slots = project_strip(&tabs, 1, &GroupPreferences::default());

        // Alpha's pair is shown together at the first alpha's position; beta follows.
        assert_eq!(
            slots.iter().map(|slot| slot.tab_index).collect::<Vec<_>>(),
            [0, 2, 1]
        );
        assert!(slots[0].group_control);
        assert!(!slots[1].group_control);
        assert!(!slots[2].group_control);
        assert_eq!(slots[0].group_size, 2);
        assert_eq!(slots[1].group_size, 2);
        assert_eq!(slots[1].domain.as_ref().unwrap().as_str(), "example.com");
        assert_eq!(slots[2].group_key, None);
    }

    #[test]
    fn opener_child_is_projected_directly_after_its_parent_across_domains() {
        let tabs = [
            linked_input(10, "chatgpt.com", None, false),
            linked_input(11, "beta.example.net", None, false),
            linked_input(12, "claude.ai", Some(10), false),
        ];

        let slots = project_strip(&tabs, 0, &GroupPreferences::default());

        assert_eq!(
            slots.iter().map(|slot| slot.tab_index).collect::<Vec<_>>(),
            [0, 2, 1]
        );
        assert!(slots[0].group_control);
        assert!(!slots[1].group_control);
        assert_eq!(slots[0].group_key, slots[1].group_key);
        assert_eq!(slots[0].group_size, 2);
        assert_eq!(slots[2].group_key, None);
    }

    #[test]
    fn group_keys_match_the_strip_slots_they_project_to() {
        let tabs = [
            input(Some("alpha.example.com"), false),
            input(Some("beta.example.net"), false),
            input(Some("alpha.example.com"), false),
            linked_input(13, "parent.example", None, false),
            linked_input(14, "child.other", Some(13), false),
            input(None, false),
        ];
        let preferences = GroupPreferences::default();

        let keys = group_keys(&tabs, &preferences);
        let slots = project_strip(&tabs, 0, &preferences);

        assert_eq!(keys[1], None);
        assert_eq!(keys[5], None);
        assert_eq!(keys[0], keys[2]);
        assert!(keys[0].is_some());
        assert_eq!(keys[3], keys[4]);
        for slot in &slots {
            assert_eq!(keys[slot.tab_index], slot.group_key, "slot {}", slot.tab_index);
        }
    }

    #[test]
    fn group_keys_skip_pinned_tabs_and_singletons() {
        let tabs = [
            input(Some("alpha.example.com"), true),
            input(Some("alpha.example.com"), true),
            linked_input(20, "solo.example", None, false),
        ];

        let keys = group_keys(&tabs, &GroupPreferences::default());

        assert_eq!(keys, vec![None, None, None]);
    }

    #[test]
    fn collapsed_projection_hides_members_but_keeps_active_one_in_first_slot() {
        let tabs = [
            input(Some("alpha.example.com"), false),
            input(Some("beta.example.net"), false),
            input(Some("alpha.example.com"), false),
        ];
        let alpha = key("example.com");
        let mut preferences = GroupPreferences::default();
        preferences.set_collapsed(&alpha, true).unwrap();

        let slots = project_strip(&tabs, 2, &preferences);

        assert_eq!(
            slots.iter().map(|slot| slot.tab_index).collect::<Vec<_>>(),
            [2, 1]
        );
        assert_eq!(slots[0].domain.as_ref(), Some(&alpha));
        assert_eq!(slots[0].group_size, 2);
        assert!(slots[0].collapsed && slots[0].group_control);
    }

    #[test]
    fn pinned_tabs_are_never_members_and_manual_modes_control_singletons() {
        let tabs = [
            input(Some("alpha.example.com"), true),
            input(Some("alpha.example.com"), false),
        ];
        let alpha = key("example.com");
        assert_eq!(
            project_strip(&tabs, 0, &GroupPreferences::default()).len(),
            2
        );

        let mut preferences = GroupPreferences::default();
        preferences.set_mode(&alpha, GroupMode::Always).unwrap();
        let slots = project_strip(&tabs, 0, &preferences);
        assert_eq!(slots.len(), 2);
        assert!(!slots[0].group_control);
        assert!(slots[1].group_control);
        assert_eq!(slots[1].group_size, 1);

        preferences.set_mode(&alpha, GroupMode::Never).unwrap();
        assert!(
            project_strip(&tabs, 0, &preferences)
                .iter()
                .all(|slot| slot.domain.is_none())
        );
    }

    #[test]
    fn registrable_domain_ignores_subdomains_scheme_and_port() {
        for url in [
            "https://example.com/",
            "http://www.example.com/a?b#c",
            "https://a.b.example.com:8443/",
            "https://EXAMPLE.com./",
        ] {
            assert_eq!(key_of(url).as_deref(), Some("example.com"), "{url}");
        }
    }

    #[test]
    fn multi_label_and_private_suffixes_are_respected() {
        for (url, expected) in [
            ("https://www.bbc.co.uk/news", "bbc.co.uk"),
            ("https://a.b.example.com.au/", "example.com.au"),
            ("https://alice.github.io/", "alice.github.io"),
            ("https://www.alice.github.io/", "alice.github.io"),
            ("https://bob.github.io/", "bob.github.io"),
            ("https://site.blogspot.com/", "site.blogspot.com"),
        ] {
            assert_eq!(key_of(url).as_deref(), Some(expected), "{url}");
        }
        assert_ne!(
            key_of("https://alice.github.io/"),
            key_of("https://bob.github.io/")
        );
    }

    #[test]
    fn idn_hosts_use_punycode_and_match_their_ascii_form() {
        let unicode = key_of("https://www.münich.example/");
        assert_eq!(unicode.as_deref(), Some("xn--mnich-kva.example"));
        assert_eq!(unicode, key_of("https://xn--mnich-kva.example:444/x"));
        assert_eq!(
            key_of("https://news.例え.jp/").as_deref(),
            Some("xn--r8jz45g.jp")
        );
    }

    #[test]
    fn non_web_ip_single_label_and_suffix_only_hosts_have_no_key() {
        for url in [
            "file:///C:/a.txt",
            "about:blank",
            "data:text/plain,hi",
            "ftp://example.com/",
            "chrome://settings/",
            "https://127.0.0.1/",
            "https://127.0.0.1:8080/",
            "https://[::1]/",
            "https://[2001:db8::1]:8443/",
            "https://localhost/",
            "https://intranet/",
            "https://com/",
            "https://co.uk/",
            "https://github.io/",
            "https://co.uk./",
        ] {
            assert_eq!(key_of(url), None, "{url}");
        }
    }

    #[test]
    fn parse_accepts_only_canonical_registrable_domains() {
        assert!(DomainKey::parse("example.co.uk").is_some());
        for bad in [
            "",
            "www.example.com",
            "Example.com",
            "example.com.",
            "co.uk",
            "localhost",
            "127.0.0.1",
            "https://example.com",
            "example.com/path",
            "user@example.com",
            "example.com:80",
            "exa mple.com",
        ] {
            assert!(DomainKey::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn group_thresholds_follow_the_mode() {
        for (mode, counts) in [
            (GroupMode::Auto, [false, false, true, true]),
            (GroupMode::Always, [false, true, true, true]),
            (GroupMode::Never, [false, false, false, false]),
        ] {
            for (members, expected) in counts.iter().enumerate() {
                assert_eq!(mode.forms_group(members), *expected, "{mode:?} {members}");
            }
        }
    }

    #[test]
    fn preferences_default_to_auto_expanded_and_prune_defaults() {
        let mut prefs = GroupPreferences::default();
        let example = key("example.com");
        assert_eq!(prefs.mode(&example), GroupMode::Auto);
        assert!(!prefs.is_collapsed(&example));
        assert!(!prefs.forms_group(&example, 1));
        assert!(prefs.forms_group(&example, 2));

        assert!(!prefs.set_mode(&example, GroupMode::Auto).unwrap());
        assert!(prefs.set_mode(&example, GroupMode::Always).unwrap());
        assert!(prefs.forms_group(&example, 1));
        assert!(prefs.set_collapsed(&example, true).unwrap());
        assert!(!prefs.set_collapsed(&example, true).unwrap());
        assert!(prefs.set_mode(&example, GroupMode::Never).unwrap());
        assert!(!prefs.forms_group(&example, 5));
        assert!(prefs.is_collapsed(&example));

        assert!(prefs.set_mode(&example, GroupMode::Auto).unwrap());
        assert!(prefs.set_collapsed(&example, false).unwrap());
        assert_eq!(prefs, GroupPreferences::default());
    }

    #[test]
    fn entry_count_is_bounded() {
        let mut prefs = GroupPreferences::default();
        for index in 0..MAX_ENTRIES {
            assert!(
                prefs
                    .set_collapsed(&key(&format!("site{index}.example")), true)
                    .unwrap()
            );
        }
        assert!(matches!(
            prefs.set_collapsed(&key("one-too-many.example"), true),
            Err(PreferencesError::TooManyEntries)
        ));
        // Existing entries remain editable and removable at the cap.
        assert!(
            prefs
                .set_mode(&key("site0.example"), GroupMode::Never)
                .unwrap()
        );
        assert!(prefs.set_collapsed(&key("site0.example"), false).unwrap());
        assert!(
            prefs
                .set_mode(&key("site0.example"), GroupMode::Auto)
                .unwrap()
        );
        assert!(
            prefs
                .set_collapsed(&key("one-too-many.example"), true)
                .is_ok()
        );
        assert!(prefs.serialize().len() <= MAX_FILE_BYTES);
    }

    #[test]
    fn serialized_preferences_round_trip_without_urls_or_credentials() {
        let mut prefs = GroupPreferences::default();
        prefs
            .set_mode(&key("zebra.example"), GroupMode::Always)
            .unwrap();
        prefs.set_collapsed(&key("alpha.co.uk"), true).unwrap();
        prefs
            .set_mode(&key("alpha.co.uk"), GroupMode::Never)
            .unwrap();
        prefs.set_collapsed(&key("mid.example"), true).unwrap();

        let text = prefs.serialize();
        assert_eq!(
            text,
            "bowser-tab-groups 1\n\
             alpha.co.uk never collapsed\n\
             mid.example auto collapsed\n\
             zebra.example always expanded\n"
        );
        assert!(!text.contains("://") && !text.contains('@'));
        assert_eq!(GroupPreferences::parse(&text).unwrap(), prefs);
        assert_eq!(
            GroupPreferences::parse("bowser-tab-groups 1\n").unwrap(),
            GroupPreferences::default()
        );
        // The trailing newline is optional.
        assert_eq!(GroupPreferences::parse(text.trim_end()).unwrap(), prefs);
    }

    #[test]
    fn malformed_preferences_are_rejected() {
        let long_domain = format!("{}.example", "a".repeat(250));
        let cases = [
            "".to_owned(),
            "bowser-tab-groups 2\n".to_owned(),
            "bowser-tab-groups 1 \n".to_owned(),
            "Bowser-Tab-Groups 1\n".to_owned(),
            "bowser-tab-groups 1\r\nexample.com always expanded\r\n".to_owned(),
            "bowser-tab-groups 1\nexample.com always\n".to_owned(),
            "bowser-tab-groups 1\nexample.com always expanded extra\n".to_owned(),
            "bowser-tab-groups 1\nexample.com sometimes expanded\n".to_owned(),
            "bowser-tab-groups 1\nexample.com always hidden\n".to_owned(),
            "bowser-tab-groups 1\nexample.com auto expanded\n".to_owned(),
            "bowser-tab-groups 1\nexample.com  always expanded\n".to_owned(),
            "bowser-tab-groups 1\n\nexample.com always expanded\n".to_owned(),
            "bowser-tab-groups 1\nexample.com always expanded\n\n".to_owned(),
            "bowser-tab-groups 1\nwww.example.com always expanded\n".to_owned(),
            "bowser-tab-groups 1\nhttps://example.com always expanded\n".to_owned(),
            "bowser-tab-groups 1\nexample.com always expanded\nexample.com never expanded\n"
                .to_owned(),
            "bowser-tab-groups 1\nb.example always expanded\na.example always expanded\n"
                .to_owned(),
            format!("bowser-tab-groups 1\n{long_domain} always expanded\n"),
        ];
        for case in cases {
            assert!(
                matches!(
                    GroupPreferences::parse(&case),
                    Err(PreferencesError::Malformed)
                ),
                "{case:?}"
            );
        }

        let mut too_many = String::from("bowser-tab-groups 1\n");
        for index in 0..=MAX_ENTRIES {
            too_many.push_str(&format!("site{index:04}.example always expanded\n"));
        }
        assert!(matches!(
            GroupPreferences::parse(&too_many),
            Err(PreferencesError::Malformed)
        ));
        assert!(matches!(
            GroupPreferences::parse(&format!(
                "bowser-tab-groups 1\n{}",
                "x".repeat(MAX_FILE_BYTES)
            )),
            Err(PreferencesError::TooLarge)
        ));
    }

    #[test]
    fn save_and_read_round_trip_atomically() {
        let dir = temp_dir();
        let path = dir.join("nested").join(FILE_NAME);
        assert_eq!(
            GroupPreferences::read(&path).unwrap(),
            GroupPreferences::default(),
            "a missing file is not an error"
        );

        let mut prefs = GroupPreferences::default();
        prefs
            .set_mode(&key("example.com"), GroupMode::Always)
            .unwrap();
        prefs.set_collapsed(&key("example.com"), true).unwrap();
        prefs.save(&path).unwrap();
        assert_eq!(GroupPreferences::read(&path).unwrap(), prefs);
        assert_eq!(GroupPreferences::load(&path), prefs);

        // Overwriting replaces the previous contents and leaves no temp file.
        prefs.set_collapsed(&key("example.com"), false).unwrap();
        prefs.save(&path).unwrap();
        assert_eq!(GroupPreferences::read(&path).unwrap(), prefs);
        let names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from(FILE_NAME)]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_oversized_and_non_utf8_files_fall_back_to_defaults() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        let oversized = vec![b'a'; MAX_FILE_BYTES + 1];
        for contents in [
            b"not preferences".to_vec(),
            b"bowser-tab-groups 1\nexample.com always expanded"
                .to_vec()
                .into_iter()
                .chain(*b"\xff\xfe")
                .collect(),
            oversized,
        ] {
            fs::write(&path, &contents).unwrap();
            assert!(GroupPreferences::read(&path).is_err());
            assert_eq!(GroupPreferences::load(&path), GroupPreferences::default());
        }
        assert!(matches!(
            fs::write(&path, vec![b'a'; MAX_FILE_BYTES + 1]).map(|_| GroupPreferences::read(&path)),
            Ok(Err(PreferencesError::TooLarge))
        ));

        // A subsequent explicit save replaces the corrupt file.
        GroupPreferences::default().save(&path).unwrap();
        assert_eq!(
            GroupPreferences::read(&path).unwrap(),
            GroupPreferences::default()
        );
        let _ = fs::remove_dir_all(dir);
    }
}
