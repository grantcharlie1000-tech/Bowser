//! Automatic tab organisation policy: manual placement of tabs into or out of domain groups and
//! into groups the user made, the settle rule for navigations in flight, drop placement, suggested
//! island names, and a bounded undo history.
//!
//! How automatic and manual grouping meet: a tab the user put in a manual group
//! ([`Membership::Manual`]) is out of reach of every automatic rule (site grouping, "Group all tabs
//! by website", "New tab position: grouped with related"). Only the user takes it out again: by
//! "Remove from group", "Ungroup", dragging it out, moving it to another group, or closing it.
//!
//! Pure Rust. Nothing here touches windows, WebViews, timers, or files. The app keeps the state and
//! applies these rules whenever it projects its tab strip, so nothing in this module runs on its own.

use std::collections::{HashMap, VecDeque};

use crate::tab_groups::{DomainKey, GroupMode, GroupPreferences, ManualGroupId, TabGroupInput, TabGroupKey};
use crate::tabs::TabId;

/// Most undo steps kept. Pushing past the limit drops the oldest step.
pub const UNDO_LIMIT: usize = 32;

/// A tab's manual placement. Automatic grouping always respects it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Membership {
    /// Placed by the automatic rules: its own registrable domain.
    #[default]
    Automatic,
    /// Dragged into the group for this domain. Overrides the tab's own domain.
    Joined(DomainKey),
    /// Dragged out of its group. The automatic rules never regroup it.
    Ungrouped,
    /// In a group the user made from the tab menu. Overrides the tab's site entirely.
    Manual(ManualGroupId),
}

impl Membership {
    /// The placement that puts a tab in the group `key`.
    pub fn joining(key: &TabGroupKey) -> Self {
        match key {
            TabGroupKey::Domain(domain) => Self::Joined(domain.clone()),
            TabGroupKey::Manual(id) => Self::Manual(*id),
        }
    }
}

/// Whether `id` still has a member among `memberships`. A manual group with none is forgotten
/// ([`GroupPreferences::retain_manual_groups`]).
pub fn manual_group_in_use(memberships: &HashMap<TabId, Membership>, id: ManualGroupId) -> bool {
    memberships.values().any(|placement| *placement == Membership::Manual(id))
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
///   below the mode's threshold.
/// - An `Ungrouped` tab loses its domain, so no automatic group reaches it.
/// - A `Manual` tab names its group and loses its domain, so no site group reaches it.
/// - With `automatic` off, `Auto`-mode domains form nothing new. Explicit `Always` groups and manual
///   members are not automatic, so they still apply.
/// - Pinned tabs are left alone, because grouping already excludes them.
pub fn apply_membership(
    inputs: &mut [TabGroupInput],
    memberships: &HashMap<TabId, Membership>,
    preferences: &GroupPreferences,
    automatic: bool,
) {
    for input in inputs.iter_mut() {
        if input.pinned {
            continue;
        }
        match memberships.get(&input.id).cloned().unwrap_or_default() {
            Membership::Joined(domain) => {
                input.domain = Some(domain);
                input.manual = true;
            }
            Membership::Ungrouped => input.domain = None,
            Membership::Manual(group) => {
                input.domain = None;
                input.manual = false;
                input.group = Some(group);
            }
            Membership::Automatic => {
                if !automatic
                    && input
                        .domain
                        .as_ref()
                        .is_some_and(|domain| preferences.mode(domain) == GroupMode::Auto)
                {
                    input.domain = None;
                }
            }
        }
    }
}

/// Placement for a tab that was just dropped, from the group keys of the model tabs on either side
/// of it after the move. `current` is the tab's group before the drag.
///
/// - Dropped between two tabs of one group (a site's or a manual one), the tab joins that group,
///   unless it is already in that group.
/// - Dropped between two tabs of the group it is already in, nothing changes.
/// - Dropped anywhere else, a tab that was grouped leaves its group. Otherwise `None`: no change.
pub fn drop_membership(
    current: Option<&TabGroupKey>,
    left: Option<&TabGroupKey>,
    right: Option<&TabGroupKey>,
) -> Option<Membership> {
    let inside_one_group = left.is_some() && left == right;
    match left {
        _ if inside_one_group && left == current => None,
        Some(key) if inside_one_group => Some(Membership::joining(key)),
        _ if current.is_some() => Some(Membership::Ungrouped),
        _ => None,
    }
}

/// Group keys on either side of a drop slot, in strip order. `order` is the strip as it was drawn
/// before the drop ([`crate::tab_groups::strip_order`]) without the dragged tab; `before` is the tab
/// the dragged one lands in front of, `None` for the end of the strip (or a tab no longer there).
pub fn strip_neighbours(
    order: &[(TabId, Option<TabGroupKey>)],
    before: Option<TabId>,
) -> (Option<&TabGroupKey>, Option<&TabGroupKey>) {
    let slot = before
        .and_then(|before| order.iter().position(|(id, _)| *id == before))
        .unwrap_or(order.len());
    let left = slot.checked_sub(1).and_then(|left| order.get(left)).and_then(|(_, key)| key.as_ref());
    let right = order.get(slot).and_then(|(_, key)| key.as_ref());
    (left, right)
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

    fn input(id: u32, domain: Option<&str>, pinned: bool) -> TabGroupInput {
        TabGroupInput {
            id: TabId(id),
            domain: domain.map(|host| {
                DomainKey::from_url(&Url::parse(&format!("https://{host}/")).unwrap()).unwrap()
            }),
            pinned,
            manual: false,
            group: None,
        }
    }

    fn domain_key(domain: &str) -> TabGroupKey {
        TabGroupKey::Domain(key(domain))
    }

    #[test]
    fn a_drop_reads_its_neighbours_in_strip_order_not_model_order() {
        // Model: x1, n1, x2, n2. The strip draws the x.com group as one run: x1 x2 n1 n2.
        let tabs = [
            input(1, Some("x.com"), false),
            input(2, None, false),
            input(3, Some("x.com"), false),
            input(4, None, false),
        ];
        let keys = crate::tab_groups::group_keys(&tabs, &GroupPreferences::default());
        let order = crate::tab_groups::strip_order(&tabs, &keys);
        assert_eq!(order.iter().map(|(id, _)| id.0).collect::<Vec<_>>(), [1, 3, 2, 4]);
        // n2 dragged between x1 and x2 on screen (in front of x2): both sides are the group, so it joins.
        let others: Vec<_> = order.into_iter().filter(|(id, _)| *id != TabId(4)).collect();
        let (left, right) = strip_neighbours(&others, Some(TabId(3)));
        assert_eq!((left, right), (Some(&domain_key("x.com")), Some(&domain_key("x.com"))));
        assert_eq!(drop_membership(None, left, right), Some(Membership::Joined(key("x.com"))));
        // In front of n1 (after the group's last member): only one side is the group, so no join.
        let (left, right) = strip_neighbours(&others, Some(TabId(2)));
        assert_eq!((left, right), (Some(&domain_key("x.com")), None));
        assert_eq!(drop_membership(None, left, right), None);
        // The end of the strip, and a tab that is gone, read as the end.
        assert_eq!(strip_neighbours(&others, None), (None, None));
        assert_eq!(strip_neighbours(&others, Some(TabId(99))), (None, None));
    }

    fn placed(entries: &[(u32, Membership)]) -> HashMap<TabId, Membership> {
        entries
            .iter()
            .map(|(id, membership)| (TabId(*id), membership.clone()))
            .collect()
    }

    #[test]
    fn joined_tab_takes_the_joined_domain() {
        let mut tabs = [
            input(1, Some("alpha.example.com"), false),
            input(2, Some("beta.example.net"), false),
        ];
        let memberships = placed(&[(2, Membership::Joined(key("example.com")))]);

        apply_membership(&mut tabs, &memberships, &GroupPreferences::default(), true);

        assert_eq!(tabs[1].domain, Some(key("example.com")));
        assert!(tabs[1].manual);
        assert!(!tabs[0].manual);
    }

    #[test]
    fn a_tab_in_a_manual_group_is_out_of_reach_of_site_grouping() {
        let mut preferences = GroupPreferences::default();
        let group = preferences.create_manual_group().unwrap();
        // Two GitHub tabs would be a site group (and an `Always` site even more so).
        preferences.set_mode(&key("github.com"), GroupMode::Always).unwrap();
        let mut tabs = [input(1, Some("github.com"), false), input(2, Some("github.com"), false)];
        let memberships = placed(&[(1, Membership::Manual(group))]);

        apply_membership(&mut tabs, &memberships, &preferences, true);

        assert_eq!((tabs[0].domain.clone(), tabs[0].group, tabs[0].manual), (None, Some(group), false));
        let keys = crate::tab_groups::group_keys(&tabs, &preferences);
        assert_eq!(keys[0], Some(TabGroupKey::Manual(group)));
        assert_eq!(keys[1], Some(domain_key("github.com")), "the other tab keeps its own site group");
    }

    #[test]
    fn manual_groups_survive_automatic_grouping_being_off() {
        let mut preferences = GroupPreferences::default();
        let group = preferences.create_manual_group().unwrap();
        let mut tabs = [input(1, Some("github.com"), false)];
        apply_membership(&mut tabs, &placed(&[(1, Membership::Manual(group))]), &preferences, false);
        assert_eq!(crate::tab_groups::group_keys(&tabs, &preferences), vec![Some(TabGroupKey::Manual(group))]);
    }

    #[test]
    fn dropping_between_two_members_of_a_manual_group_joins_it() {
        let group = TabGroupKey::Manual(ManualGroupId(2));
        assert_eq!(drop_membership(None, Some(&group), Some(&group)), Some(Membership::Manual(ManualGroupId(2))));
        assert_eq!(drop_membership(Some(&group), Some(&group), Some(&group)), None);
        // Out of a manual group onto open strip: it leaves the group.
        assert_eq!(drop_membership(Some(&group), None, None), Some(Membership::Ungrouped));
        // From a site group into a manual group.
        assert_eq!(
            drop_membership(Some(&domain_key("x.com")), Some(&group), Some(&group)),
            Some(Membership::Manual(ManualGroupId(2)))
        );
        // Between a manual group and a site group: neither.
        assert_eq!(drop_membership(None, Some(&group), Some(&domain_key("x.com"))), None);
    }

    #[test]
    fn joining_maps_each_kind_of_group_to_its_placement() {
        assert_eq!(Membership::joining(&domain_key("x.com")), Membership::Joined(key("x.com")));
        assert_eq!(Membership::joining(&TabGroupKey::Manual(ManualGroupId(4))), Membership::Manual(ManualGroupId(4)));
        let memberships = placed(&[(1, Membership::Manual(ManualGroupId(4))), (2, Membership::Ungrouped)]);
        assert!(manual_group_in_use(&memberships, ManualGroupId(4)));
        assert!(!manual_group_in_use(&memberships, ManualGroupId(5)));
    }

    #[test]
    fn ungrouped_tab_loses_its_domain() {
        let mut tabs = [
            input(1, Some("alpha.example.com"), false),
            input(2, Some("alpha.example.com"), false),
        ];
        let memberships = placed(&[(2, Membership::Ungrouped)]);

        apply_membership(&mut tabs, &memberships, &GroupPreferences::default(), true);

        assert_eq!(tabs[1].domain, None);
        assert_eq!(tabs[0].domain, Some(key("example.com")));
    }

    #[test]
    fn automation_off_stops_auto_domains_but_keeps_explicit_groups() {
        let mut preferences = GroupPreferences::default();
        preferences
            .set_mode(&key("example.net"), GroupMode::Always)
            .unwrap();
        let mut tabs = [
            input(1, Some("alpha.example.com"), false),
            input(2, Some("alpha.example.com"), false),
            input(3, Some("beta.example.net"), false),
            input(4, Some("beta.example.net"), false),
        ];

        apply_membership(&mut tabs, &HashMap::new(), &preferences, false);

        assert_eq!(tabs[0].domain, None, "auto-mode domain forms nothing new");
        assert_eq!(tabs[1].domain, None);
        assert_eq!(
            tabs[2].domain,
            Some(key("example.net")),
            "always mode stays"
        );
    }

    #[test]
    fn automation_off_keeps_manually_joined_members() {
        let mut tabs = [input(1, Some("alpha.example.com"), false)];
        let memberships = placed(&[(1, Membership::Joined(key("example.com")))]);

        apply_membership(&mut tabs, &memberships, &GroupPreferences::default(), false);

        assert_eq!(tabs[0].domain, Some(key("example.com")));
        assert!(tabs[0].manual);
    }

    #[test]
    fn pinned_tabs_are_left_unchanged_by_placement() {
        let mut tabs = [input(1, Some("alpha.example.com"), true)];
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
