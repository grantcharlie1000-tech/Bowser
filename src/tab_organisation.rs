//! Automatic tab organisation policy: manual placement of tabs into or out of domain groups, the
//! settle rule for navigations in flight, drop placement, suggested island names, and a bounded
//! undo history.
//!
//! Pure Rust. Nothing here touches windows, WebViews, timers, or files. The app keeps the state and
//! applies these rules whenever it projects its tab strip, so nothing in this module runs on its own.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::tab_groups::{DomainKey, GroupMode, GroupPreferences, TabGroupInput, TabGroupKey};
use crate::tabs::TabId;

/// Most undo steps kept. Pushing past the limit drops the oldest step.
pub const UNDO_LIMIT: usize = 32;

/// A tab's manual placement. Automatic grouping always respects it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Membership {
    /// Placed by the automatic rules: its own registrable domain and any opener link.
    #[default]
    Automatic,
    /// Dragged into the group for this domain. Overrides the tab's own domain and opener link.
    Joined(DomainKey),
    /// Dragged out of its group. The automatic rules never regroup it.
    Ungrouped,
}

/// Everything an undo step restores: group preferences and manual placement.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub preferences: GroupPreferences,
    pub memberships: HashMap<TabId, Membership>,
}

/// Applies manual placement and the automatic-grouping switch to the projection inputs.
///
/// - A `Joined` tab takes the joined domain and is marked manual, so it keeps the group formed even
///   below the mode's threshold. Its opener link no longer applies.
/// - An `Ungrouped` tab loses its domain and opener link, and no automatic edge reaches it.
/// - With `automatic` off, `Auto`-mode domains and opener links form nothing new. Explicit `Always`
///   groups and manual members are not automatic, so they still apply.
/// - Pinned tabs are left alone, because grouping already excludes them.
pub fn apply_membership(
    inputs: &mut [TabGroupInput],
    memberships: &HashMap<TabId, Membership>,
    preferences: &GroupPreferences,
    automatic: bool,
) {
    let membership_of = |id: TabId| memberships.get(&id).cloned().unwrap_or_default();
    // An opener edge only counts when the parent is placed automatically, so a manually placed
    // parent never pulls its children into an opener group.
    let automatic_ids: HashSet<TabId> = inputs
        .iter()
        .filter(|input| membership_of(input.id) == Membership::Automatic)
        .map(|input| input.id)
        .collect();
    for input in inputs.iter_mut() {
        if input.pinned {
            continue;
        }
        match membership_of(input.id) {
            Membership::Joined(domain) => {
                input.domain = Some(domain);
                input.opener = None;
                input.manual = true;
            }
            Membership::Ungrouped => {
                input.domain = None;
                input.opener = None;
            }
            Membership::Automatic => {
                if !automatic {
                    input.opener = None;
                    if input
                        .domain
                        .as_ref()
                        .is_some_and(|domain| preferences.mode(domain) == GroupMode::Auto)
                    {
                        input.domain = None;
                    }
                }
                if input
                    .opener
                    .is_some_and(|opener| !automatic_ids.contains(&opener))
                {
                    input.opener = None;
                }
            }
        }
    }
}

/// Placement for a tab that was just dropped, from the group keys of the model tabs on either side
/// of it after the move. `current` is the tab's group before the drag.
///
/// - Dropped between two tabs of one domain group, the tab joins that group, unless it is already in
///   that group.
/// - Dropped between two tabs of the group it is already in, nothing changes.
/// - Dropped anywhere else, a tab that was grouped leaves its group. Otherwise `None`: no change.
pub fn drop_membership(
    current: Option<&TabGroupKey>,
    left: Option<&TabGroupKey>,
    right: Option<&TabGroupKey>,
) -> Option<Membership> {
    let inside_one_group = left.is_some() && left == right;
    match (left, right) {
        _ if inside_one_group && left == current => None,
        (Some(TabGroupKey::Domain(left)), Some(TabGroupKey::Domain(right))) if left == right => {
            Some(Membership::Joined(left.clone()))
        }
        _ if current.is_some() => Some(Membership::Ungrouped),
        _ => None,
    }
}

/// Domains of tabs whose navigation is in flight. While one is, the tab keeps the domain it had
/// before the navigation began, so redirects and loading never regroup it. The committed domain
/// applies once the navigation finishes.
#[derive(Debug, Default)]
pub struct InFlightDomains {
    before: HashMap<TabId, Option<DomainKey>>,
}

impl InFlightDomains {
    /// Records the domain a tab had before its navigation began. The first record of a navigation
    /// wins, so a redirect cannot replace it.
    pub fn begin(&mut self, id: TabId, before: Option<DomainKey>) {
        self.before.entry(id).or_insert(before);
    }

    /// Ends the navigation. Call on navigation completed, and when the tab closes.
    pub fn finish(&mut self, id: TabId) {
        self.before.remove(&id);
    }

    /// The domain to group by: the pre-navigation domain while a navigation is in flight, otherwise
    /// `committed`.
    pub fn domain_for(&self, id: TabId, committed: Option<DomainKey>) -> Option<DomainKey> {
        match self.before.get(&id) {
            Some(before) => before.clone(),
            None => committed,
        }
    }
}

/// Sites whose island name is a brand rather than the first label of their domain.
const KNOWN_SITES: &[(&str, &str)] = &[
    ("amazon.co.uk", "Amazon"),
    ("amazon.com", "Amazon"),
    ("apple.com", "Apple"),
    ("bbc.co.uk", "BBC"),
    ("bbc.com", "BBC"),
    ("bing.com", "Bing"),
    ("chatgpt.com", "ChatGPT"),
    ("claude.ai", "Claude"),
    ("discord.com", "Discord"),
    ("duckduckgo.com", "DuckDuckGo"),
    ("facebook.com", "Facebook"),
    ("figma.com", "Figma"),
    ("github.com", "GitHub"),
    ("gitlab.com", "GitLab"),
    ("google.com", "Google"),
    ("instagram.com", "Instagram"),
    ("linkedin.com", "LinkedIn"),
    ("medium.com", "Medium"),
    ("microsoft.com", "Microsoft"),
    ("netflix.com", "Netflix"),
    ("notion.so", "Notion"),
    ("openai.com", "OpenAI"),
    ("reddit.com", "Reddit"),
    ("slack.com", "Slack"),
    ("spotify.com", "Spotify"),
    ("stackoverflow.com", "Stack Overflow"),
    ("twitch.tv", "Twitch"),
    ("twitter.com", "X"),
    ("wikipedia.org", "Wikipedia"),
    ("x.com", "X"),
    ("ycombinator.com", "Hacker News"),
    ("yahoo.com", "Yahoo"),
    ("youtu.be", "YouTube"),
    ("youtube.com", "YouTube"),
];

/// Name suggested for a domain group that the user has not named. It is only ever displayed, never
/// stored, so a name the user chose is never replaced by a suggestion.
pub fn suggested_name(key: &DomainKey) -> String {
    let domain = key.as_str();
    if let Some((_, name)) = KNOWN_SITES.iter().find(|(site, _)| *site == domain) {
        return (*name).to_owned();
    }
    // Punycode labels are not readable as words, so they keep the full domain.
    let label = domain.split('.').next().unwrap_or(domain);
    if label.starts_with("xn--") {
        return domain.to_owned();
    }
    let mut characters = label.chars();
    let capitalised: String = match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => return domain.to_owned(),
    };
    crate::tab_groups::clean_name(&capitalised).unwrap_or_else(|| domain.to_owned())
}

/// Bounded history of undo steps, newest last.
#[derive(Debug)]
pub struct UndoStack<T> {
    steps: VecDeque<T>,
    limit: usize,
}

impl<T> UndoStack<T> {
    pub fn new(limit: usize) -> Self {
        Self {
            steps: VecDeque::new(),
            limit,
        }
    }

    pub fn push(&mut self, step: T) {
        if self.steps.len() == self.limit {
            self.steps.pop_front();
        }
        self.steps.push_back(step);
    }

    pub fn pop(&mut self) -> Option<T> {
        self.steps.pop_back()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }
}

impl<T> Default for UndoStack<T> {
    fn default() -> Self {
        Self::new(UNDO_LIMIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn key(domain: &str) -> DomainKey {
        DomainKey::parse(domain).unwrap()
    }

    fn input(id: u32, domain: Option<&str>, opener: Option<u32>, pinned: bool) -> TabGroupInput {
        TabGroupInput {
            id: TabId(id),
            domain: domain.map(|host| {
                DomainKey::from_url(&Url::parse(&format!("https://{host}/")).unwrap()).unwrap()
            }),
            pinned,
            opener: opener.map(TabId),
            manual: false,
        }
    }

    fn domain_key(domain: &str) -> TabGroupKey {
        TabGroupKey::Domain(key(domain))
    }

    fn placed(entries: &[(u32, Membership)]) -> HashMap<TabId, Membership> {
        entries
            .iter()
            .map(|(id, membership)| (TabId(*id), membership.clone()))
            .collect()
    }

    #[test]
    fn joined_tab_takes_the_joined_domain_and_loses_its_opener() {
        let mut tabs = [
            input(1, Some("alpha.example.com"), None, false),
            input(2, Some("beta.example.net"), Some(1), false),
        ];
        let memberships = placed(&[(2, Membership::Joined(key("example.com")))]);

        apply_membership(&mut tabs, &memberships, &GroupPreferences::default(), true);

        assert_eq!(tabs[1].domain, Some(key("example.com")));
        assert_eq!(tabs[1].opener, None);
        assert!(tabs[1].manual);
        assert!(!tabs[0].manual);
    }

    #[test]
    fn ungrouped_tab_loses_its_domain_and_no_opener_edge_reaches_it() {
        let mut tabs = [
            input(1, Some("alpha.example.com"), None, false),
            input(2, Some("alpha.example.com"), Some(1), false),
        ];
        let memberships = placed(&[(2, Membership::Ungrouped)]);

        apply_membership(&mut tabs, &memberships, &GroupPreferences::default(), true);

        assert_eq!(tabs[1].domain, None);
        assert_eq!(tabs[1].opener, None);
        assert_eq!(tabs[0].domain, Some(key("example.com")));
    }

    #[test]
    fn child_of_a_manually_placed_parent_does_not_join_the_opener_group() {
        let mut tabs = [
            input(1, Some("alpha.example.com"), None, false),
            input(2, Some("beta.example.net"), Some(1), false),
        ];
        let memberships = placed(&[(1, Membership::Ungrouped)]);

        apply_membership(&mut tabs, &memberships, &GroupPreferences::default(), true);

        assert_eq!(tabs[1].opener, None);
    }

    #[test]
    fn automation_off_stops_auto_domains_and_openers_but_keeps_explicit_groups() {
        let mut preferences = GroupPreferences::default();
        preferences
            .set_mode(&key("example.net"), GroupMode::Always)
            .unwrap();
        let mut tabs = [
            input(1, Some("alpha.example.com"), None, false),
            input(2, Some("alpha.example.com"), None, false),
            input(3, Some("beta.example.net"), None, false),
            input(4, Some("beta.example.net"), None, false),
            input(5, Some("gamma.example.org"), Some(1), false),
        ];

        apply_membership(&mut tabs, &HashMap::new(), &preferences, false);

        assert_eq!(tabs[0].domain, None, "auto-mode domain forms nothing new");
        assert_eq!(tabs[1].domain, None);
        assert_eq!(
            tabs[2].domain,
            Some(key("example.net")),
            "always mode stays"
        );
        assert_eq!(tabs[4].opener, None, "no new opener group");
    }

    #[test]
    fn automation_off_keeps_manually_joined_members() {
        let mut tabs = [input(1, Some("alpha.example.com"), None, false)];
        let memberships = placed(&[(1, Membership::Joined(key("example.com")))]);

        apply_membership(&mut tabs, &memberships, &GroupPreferences::default(), false);

        assert_eq!(tabs[0].domain, Some(key("example.com")));
        assert!(tabs[0].manual);
    }

    #[test]
    fn pinned_tabs_are_left_unchanged_by_placement() {
        let mut tabs = [input(1, Some("alpha.example.com"), None, true)];
        let memberships = placed(&[(1, Membership::Ungrouped)]);

        apply_membership(&mut tabs, &memberships, &GroupPreferences::default(), false);

        assert_eq!(tabs[0].domain, Some(key("example.com")));
        assert!(!tabs[0].manual);
    }

    #[test]
    fn dropping_between_two_members_of_a_group_joins_it() {
        assert_eq!(
            drop_membership(
                None,
                Some(&domain_key("example.com")),
                Some(&domain_key("example.com")),
            ),
            Some(Membership::Joined(key("example.com")))
        );
    }

    #[test]
    fn dropping_inside_the_group_it_is_already_in_changes_nothing() {
        let group = domain_key("example.com");
        assert_eq!(
            drop_membership(Some(&group), Some(&group), Some(&group)),
            None
        );
    }

    #[test]
    fn dropping_a_grouped_tab_outside_its_group_ungroups_it() {
        let group = domain_key("example.com");
        assert_eq!(
            drop_membership(Some(&group), Some(&group), None),
            Some(Membership::Ungrouped)
        );
        assert_eq!(
            drop_membership(Some(&group), None, Some(&domain_key("example.net"))),
            Some(Membership::Ungrouped)
        );
    }

    #[test]
    fn dropping_an_ungrouped_tab_elsewhere_leaves_it_alone() {
        assert_eq!(drop_membership(None, None, None), None);
        assert_eq!(
            drop_membership(None, Some(&domain_key("example.net")), None),
            None
        );
    }

    #[test]
    fn dropping_into_a_mixed_run_does_not_join_either_group() {
        assert_eq!(
            drop_membership(
                None,
                Some(&domain_key("example.com")),
                Some(&domain_key("example.net")),
            ),
            None
        );
    }

    #[test]
    fn dropping_inside_an_opener_group_keeps_the_tab_in_it() {
        let opener = TabGroupKey::Opener(TabId(7));
        assert_eq!(
            drop_membership(Some(&opener), Some(&opener), Some(&opener)),
            None
        );
    }

    #[test]
    fn suggested_names_use_brands_then_the_first_domain_label() {
        for (domain, expected) in [
            ("chatgpt.com", "ChatGPT"),
            ("github.com", "GitHub"),
            ("bbc.co.uk", "BBC"),
            ("example.com", "Example"),
            ("alice.github.io", "Alice"),
            ("my-site.org", "My-site"),
        ] {
            assert_eq!(suggested_name(&key(domain)), expected, "{domain}");
        }
    }

    #[test]
    fn punycode_domains_keep_their_full_name() {
        assert_eq!(
            suggested_name(&key("xn--mnich-kva.example")),
            "xn--mnich-kva.example"
        );
    }

    #[test]
    fn suggested_names_respect_the_stored_name_limit() {
        let long = format!("{}.example", "a".repeat(60));
        assert!(suggested_name(&key(&long)).chars().count() <= crate::tab_groups::MAX_NAME_CHARS);
    }

    #[test]
    fn undo_stack_is_bounded_and_pops_newest_first() {
        let mut stack = UndoStack::new(2);
        assert!(stack.is_empty());
        stack.push(1);
        stack.push(2);
        stack.push(3);
        assert_eq!(stack.len(), 2);
        assert_eq!(stack.pop(), Some(3));
        assert_eq!(stack.pop(), Some(2));
        assert_eq!(stack.pop(), None);
        assert!(stack.is_empty());
    }

    #[test]
    fn default_undo_stack_uses_the_documented_limit() {
        let mut stack = UndoStack::<usize>::default();
        for step in 0..UNDO_LIMIT + 5 {
            stack.push(step);
        }
        assert_eq!(stack.len(), UNDO_LIMIT);
    }

    #[test]
    fn navigation_in_flight_keeps_the_domain_it_had_until_it_finishes() {
        let mut domains = InFlightDomains::default();
        let id = TabId(1);
        let before = Some(key("chatgpt.com"));

        domains.begin(id, before.clone());
        // A redirect to another site does not regroup the tab while it loads.
        domains.begin(id, Some(key("example.net")));
        assert_eq!(domains.domain_for(id, Some(key("example.net"))), before);

        domains.finish(id);
        assert_eq!(
            domains.domain_for(id, Some(key("example.net"))),
            Some(key("example.net")),
        );
    }

    #[test]
    fn tabs_without_a_navigation_in_flight_use_the_committed_domain() {
        let domains = InFlightDomains::default();
        assert_eq!(
            domains.domain_for(TabId(9), Some(key("example.com"))),
            Some(key("example.com"))
        );
        assert_eq!(domains.domain_for(TabId(9), None), None);
    }
}
