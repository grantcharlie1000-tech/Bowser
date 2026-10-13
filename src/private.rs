//! Private (InPrivate) tab rules.
//!
//! A private tab is an ordinary tab of the window whose WebView2 controller is created with
//! `IsInPrivateModeEnabled = true` in one shared, in-memory profile ([`PRIVATE_PROFILE`]). The
//! profile keeps cookies, storage and cache in memory and WebView2 discards it when its last
//! controller closes. Everything in this module is pure, so the persistence rules can be tested
//! without a window: callers ask [`may_persist`] before they write anything that a private tab
//! produced, and use the small decision helpers below instead of re-deriving the policy.

use url::Url;

use crate::engine::TrackingPreventionLevel;

/// WebView2 profile name of the shared private profile. It differs from the HTML chrome's own
/// profile (`BowserChrome`), so private pages and the chrome never share an in-memory store.
pub const PRIVATE_PROFILE: &str = "BowserPrivate";

/// Something Bowser can write to disk (or keep beyond the private tab's life).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Store {
    /// `history.txt`.
    History,
    /// The in-memory address-bar history that feeds suggestions and inline completion.
    SuggestionHistory,
    /// `session` crash-recovery and restore snapshot.
    SessionRestore,
    /// Named sessions saved from the main menu.
    SavedSessions,
    /// `tab-groups.txt` (site modes, names, colours, collapse state) and domain group membership.
    TabGroups,
    /// Tab membership saved with workspaces.
    WorkspaceTabs,
    /// `permissions.txt`, including every Allow or Block decision.
    PermissionDecisions,
    /// Per-site blocker exceptions (`blocker-sites.txt`).
    BlockerExceptions,
    /// `privacy-stats.txt` and the session blocked-request counter.
    BlockerStats,
    /// Favicons kept on disk.
    Favicons,
    /// Sign-in capture and save prompts.
    SignInCapture,
    /// A bookmark. Allowed only when the user explicitly saves it.
    Bookmark,
}

/// Whether `store` may receive data produced by a tab. A normal tab follows the profile's existing
/// rules. A private tab writes nothing, except that a bookmark may be added by an explicit user
/// action (`explicit`), because the user is then choosing to keep that address.
pub fn may_persist(store: Store, private: bool, explicit: bool) -> bool {
    if !private {
        return true;
    }
    matches!(store, Store::Bookmark) && explicit
}

/// The stored permission decision (`stored_allow`) applies only to normal tabs. A private tab has
/// no stored allows: every request is denied.
pub fn permission_allowed(private: bool, stored_allow: bool) -> bool {
    !private && stored_allow
}

/// Tracking-prevention level a tab's profile is set to: private tabs are always Strict; normal tabs
/// keep the level the window asked for (`None` leaves the runtime default).
pub fn tracking_level(
    private: bool,
    window: Option<TrackingPreventionLevel>,
) -> Option<TrackingPreventionLevel> {
    if private {
        Some(TrackingPreventionLevel::Strict)
    } else {
        window
    }
}

/// The blocker's verdict for one subresource request. A normal tab honours the global switch and
/// the per-site exceptions. A private tab ignores both: the blocker is always on.
pub fn blocker_applies(private: bool, enabled: bool, site_allowed: bool) -> bool {
    private || (enabled && !site_allowed)
}

/// The address a snapshot (session restore, saved sessions, workspace membership) may record for a
/// tab: nothing for a private tab.
pub fn snapshot_url(private: bool, url: Option<&Url>) -> Option<&Url> {
    if may_persist(Store::SessionRestore, private, false) {
        url
    } else {
        None
    }
}

/// A tab opened by a page's new-window request stays in the opener's privacy mode.
pub fn inherits_privacy(opener_private: bool) -> bool {
    opener_private
}

/// Whether the shared private profile needs to exist: some private tab is open.
pub fn profile_in_use(private_tabs: usize) -> bool {
    private_tabs > 0
}

/// The private profile name is a valid WebView2 profile name (1 to 64 ASCII letters, digits,
/// `_` or `-`) and is not the chrome's.
pub fn profile_name_valid() -> bool {
    !PRIVATE_PROFILE.is_empty()
        && PRIVATE_PROFILE.len() <= 64
        && PRIVATE_PROFILE
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        && !PRIVATE_PROFILE.eq_ignore_ascii_case("BowserChrome")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Store; 12] = [
        Store::History,
        Store::SuggestionHistory,
        Store::SessionRestore,
        Store::SavedSessions,
        Store::TabGroups,
        Store::WorkspaceTabs,
        Store::PermissionDecisions,
        Store::BlockerExceptions,
        Store::BlockerStats,
        Store::Favicons,
        Store::SignInCapture,
        Store::Bookmark,
    ];

    #[test]
    fn normal_tabs_keep_every_existing_write() {
        for store in ALL {
            assert!(may_persist(store, false, false), "{store:?}");
            assert!(may_persist(store, false, true), "{store:?}");
        }
    }

    #[test]
    fn private_tabs_write_nothing_on_their_own() {
        for store in ALL {
            assert!(!may_persist(store, true, false), "{store:?}");
        }
    }

    #[test]
    fn an_explicit_user_action_may_only_save_a_bookmark() {
        for store in ALL {
            assert_eq!(
                may_persist(store, true, true),
                store == Store::Bookmark,
                "{store:?}"
            );
        }
    }

    #[test]
    fn private_tabs_have_no_stored_permission_allows() {
        assert!(permission_allowed(false, true));
        assert!(!permission_allowed(false, false));
        assert!(!permission_allowed(true, true));
        assert!(!permission_allowed(true, false));
    }

    #[test]
    fn private_tabs_are_always_strict_and_normal_tabs_keep_the_window_level() {
        for window in [
            None,
            Some(TrackingPreventionLevel::Balanced),
            Some(TrackingPreventionLevel::Strict),
        ] {
            assert_eq!(
                tracking_level(true, window),
                Some(TrackingPreventionLevel::Strict)
            );
            assert_eq!(tracking_level(false, window), window);
        }
    }

    #[test]
    fn private_tabs_ignore_the_blocker_switch_and_site_exceptions() {
        for enabled in [false, true] {
            for site_allowed in [false, true] {
                assert!(blocker_applies(true, enabled, site_allowed));
            }
        }
        assert!(blocker_applies(false, true, false));
        assert!(!blocker_applies(false, false, false));
        assert!(!blocker_applies(false, true, true));
    }

    #[test]
    fn snapshots_never_record_a_private_tab_address() {
        let url = Url::parse("https://example.com/secret").unwrap();
        assert_eq!(snapshot_url(false, Some(&url)), Some(&url));
        assert_eq!(snapshot_url(true, Some(&url)), None);
        assert_eq!(snapshot_url(true, None), None);
        assert_eq!(snapshot_url(false, None), None);
    }

    #[test]
    fn new_window_requests_inherit_the_opener_mode() {
        assert!(inherits_privacy(true));
        assert!(!inherits_privacy(false));
    }

    #[test]
    fn the_private_profile_exists_only_while_a_private_tab_is_open() {
        assert!(!profile_in_use(0));
        assert!(profile_in_use(1));
        assert!(profile_in_use(7));
    }

    #[test]
    fn profile_name_is_valid_and_distinct_from_the_chrome_profile() {
        assert!(profile_name_valid());
        assert_eq!(PRIVATE_PROFILE, "BowserPrivate");
    }
}
