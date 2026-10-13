//! Privacy dashboard permissions: the sites table, the recent denied list and the commands that
//! change them. Pure: the stores are read by the caller and passed in, and every command is
//! validated here before anything touches the permission store.
//!
//! Wire (snapshot `privacy`, see docs/html-chrome-protocol.md):
//! `sites`: `[{origin, current, kinds:[camera, microphone, location, notifications, popups]}]`, each
//! kind `"allow"`, `"block"` or `"default"`; `denied`: `[{origin, kind, ago, n}]`, newest first.
//! Commands (`{t:"priv"}`): `perm-set` (`text` origin, `id` kind index, `n` 0 allow / 1 block /
//! 2 default), `perm-reset` (`text` origin), `perm-reset-all`, `denied-clear`.

use super::super::{json::Object, protocol::PanelFields};
use crate::permissions::{
    self, DeniedAttempt, Decision, MAX_ORIGIN_CHARS, Permission, PermissionStore,
};

/// Column order on the dashboard and on the wire (`kinds`, and the `id` of `perm-set`).
pub(crate) const KIND_ORDER: [Permission; 5] = [
    Permission::Camera,
    Permission::Microphone,
    Permission::Geolocation,
    Permission::Notifications,
    Permission::Popups,
];

/// Most sites in one snapshot. The page budget is 4096 keys; each site costs three.
pub(crate) const MAX_SITES: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SiteRow {
    pub origin: String,
    /// The active tab's site, listed first even when nothing is stored for it.
    pub current: bool,
    pub kinds: [Option<Decision>; 5],
}

/// Every site with a stored decision, the active tab's site, and the sites whose requests were
/// recently denied (`also`, so the user can change them from the dashboard). The current site comes
/// first; the rest are in origin order, at most [`MAX_SITES`] in all. Origins that are not canonical
/// are ignored.
pub(crate) fn site_rows(
    store: &PermissionStore,
    current_origin: Option<&str>,
    also: &[&str],
) -> Vec<SiteRow> {
    let current = current_origin.and_then(permissions::canonical_origin);
    let row = |origin: &str, is_current: bool| SiteRow {
        origin: origin.to_owned(),
        current: is_current,
        kinds: KIND_ORDER.map(|kind| store.decision(origin, kind)),
    };
    let mut rows = Vec::new();
    if let Some(origin) = current.as_deref() {
        rows.push(row(origin, true));
    }
    let others: std::collections::BTreeSet<&str> = store
        .origins()
        .chain(also.iter().copied().filter(|origin| permissions::canonical_origin(origin).is_some()))
        .filter(|origin| current.as_deref() != Some(*origin))
        .collect();
    for origin in others {
        if rows.len() >= MAX_SITES {
            break;
        }
        rows.push(row(origin, false));
    }
    rows
}

fn choice_name(decision: Option<Decision>) -> &'static str {
    match decision {
        Some(Decision::Allow) => "allow",
        Some(Decision::Block) => "block",
        None => "default",
    }
}

/// Writes the `sites` and `denied` members of the privacy snapshot. `now` is Unix seconds; each
/// denied entry carries its age instead of a clock time, so the page needs no timezone.
pub(crate) fn write_permissions(
    out: &mut Object<'_>,
    sites: &[SiteRow],
    denied: &[DeniedAttempt],
    now: u64,
) {
    out.array("sites", |items| {
        for site in sites.iter().take(MAX_SITES) {
            items.object(|item| {
                item.string("origin", &site.origin);
                item.bool("current", site.current);
                item.array("kinds", |kinds| {
                    for decision in site.kinds {
                        kinds.string(choice_name(decision));
                    }
                });
            });
        }
    });
    out.array("denied", |items| {
        for attempt in denied.iter().take(permissions::DENIED_LOG_CAP) {
            items.object(|item| {
                item.string("origin", &attempt.origin);
                item.string("kind", attempt.kind.wire_name());
                item.uint("ago", now.saturating_sub(attempt.at_secs));
                item.uint("n", u64::from(attempt.count));
            });
        }
    });
}

/// A validated dashboard permission command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Set {
        origin: String,
        kind: Permission,
        /// `None` is Default: the stored line is removed and the site is blocked.
        decision: Option<Decision>,
    },
    ResetSite(String),
    ResetAll,
    ClearDenied,
}

/// Maps one `priv` panel command to an [`Action`], or `None` when it is not a permission command or
/// does not validate: exact fields only, a canonical http(s) origin of at most
/// [`MAX_ORIGIN_CHARS`] characters, a kind index inside [`KIND_ORDER`], a choice of 0 to 2.
pub(crate) fn map_command(f: &PanelFields) -> Option<Action> {
    let no_ids = f.parent.is_none() && f.before.is_none() && f.on.is_none() && f.url.is_none();
    if !no_ids {
        return None;
    }
    let origin = || {
        f.text
            .as_deref()
            .filter(|text| text.chars().count() <= MAX_ORIGIN_CHARS)
            .and_then(permissions::canonical_origin)
    };
    match f.a.as_str() {
        "perm-set" => {
            let kind = *KIND_ORDER.get(usize::try_from(f.id?).ok()?)?;
            let decision = match f.n? {
                0 => Some(Decision::Allow),
                1 => Some(Decision::Block),
                2 => None,
                _ => return None,
            };
            Some(Action::Set {
                origin: origin()?,
                kind,
                decision,
            })
        }
        "perm-reset" if f.id.is_none() && f.n.is_none() => Some(Action::ResetSite(origin()?)),
        "perm-reset-all" if f.id.is_none() && f.n.is_none() && f.text.is_none() => {
            Some(Action::ResetAll)
        }
        "denied-clear" if f.id.is_none() && f.n.is_none() && f.text.is_none() => {
            Some(Action::ClearDenied)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_host::{json::Json, protocol};

    fn fields(a: &str) -> PanelFields {
        PanelFields {
            a: a.to_owned(),
            ..PanelFields::default()
        }
    }

    fn set(origin: &str, id: u64, n: i32) -> PanelFields {
        PanelFields {
            id: Some(id),
            n: Some(n),
            text: Some(origin.to_owned()),
            ..fields("perm-set")
        }
    }

    fn store_with(rules: &[(&str, Permission, Decision)]) -> PermissionStore {
        let mut store = PermissionStore::default();
        for (origin, kind, decision) in rules {
            store.set(origin, *kind, Some(*decision)).unwrap();
        }
        store
    }

    #[test]
    fn kind_order_covers_every_permission_once() {
        let mut sorted = KIND_ORDER;
        sorted.sort();
        let mut all = Permission::ALL;
        all.sort();
        assert_eq!(sorted, all);
        assert_eq!(
            KIND_ORDER.map(Permission::wire_name),
            ["camera", "microphone", "location", "notifications", "popups"]
        );
    }

    #[test]
    fn set_validates_origin_kind_and_choice() {
        assert_eq!(
            map_command(&set("https://meet.test", 1, 0)),
            Some(Action::Set {
                origin: "https://meet.test".into(),
                kind: Permission::Microphone,
                decision: Some(Decision::Allow),
            })
        );
        assert_eq!(
            map_command(&set("http://localhost:8080", 4, 2)),
            Some(Action::Set {
                origin: "http://localhost:8080".into(),
                kind: Permission::Popups,
                decision: None,
            })
        );
        assert_eq!(
            map_command(&set("https://a.test", 0, 1)),
            Some(Action::Set {
                origin: "https://a.test".into(),
                kind: Permission::Camera,
                decision: Some(Decision::Block),
            })
        );
        // Kind index outside the five, choice outside 0..=2.
        assert_eq!(map_command(&set("https://a.test", 5, 0)), None);
        assert_eq!(map_command(&set("https://a.test", u64::MAX, 0)), None);
        assert_eq!(map_command(&set("https://a.test", 0, 3)), None);
        assert_eq!(map_command(&set("https://a.test", 0, -1)), None);
        // The origin must already be canonical: no path, no uppercase host, no default port, no
        // other scheme, no length over the cap.
        for origin in [
            "",
            "https://a.test/",
            "https://a.test/x",
            "https://A.test",
            "https://a.test:443",
            "file:///C:/x",
            "javascript:alert(1)",
            "a.test",
            "https://user@a.test",
            "https://a.test https://b.test",
        ] {
            assert_eq!(map_command(&set(origin, 0, 0)), None, "{origin:?}");
        }
        let long = format!("https://{}.test", "a".repeat(MAX_ORIGIN_CHARS));
        assert_eq!(map_command(&set(&long, 0, 0)), None);
    }

    #[test]
    fn set_needs_every_field_and_nothing_extra() {
        let mut missing_id = set("https://a.test", 0, 0);
        missing_id.id = None;
        assert_eq!(map_command(&missing_id), None);
        let mut missing_n = set("https://a.test", 0, 0);
        missing_n.n = None;
        assert_eq!(map_command(&missing_n), None);
        let mut missing_text = set("https://a.test", 0, 0);
        missing_text.text = None;
        assert_eq!(map_command(&missing_text), None);
        for extra in [
            PanelFields { parent: Some(1), ..set("https://a.test", 0, 0) },
            PanelFields { before: Some(1), ..set("https://a.test", 0, 0) },
            PanelFields { on: Some(true), ..set("https://a.test", 0, 0) },
            PanelFields { url: Some("https://a.test".into()), ..set("https://a.test", 0, 0) },
        ] {
            assert_eq!(map_command(&extra), None, "{extra:?}");
        }
    }

    #[test]
    fn reset_and_clear_commands_are_exact() {
        let site = PanelFields {
            text: Some("https://a.test".into()),
            ..fields("perm-reset")
        };
        assert_eq!(map_command(&site), Some(Action::ResetSite("https://a.test".into())));
        assert_eq!(map_command(&PanelFields { text: None, ..site.clone() }), None);
        assert_eq!(map_command(&PanelFields { id: Some(0), ..site.clone() }), None);
        assert_eq!(map_command(&PanelFields { n: Some(0), ..site.clone() }), None);
        assert_eq!(
            map_command(&PanelFields { text: Some("https://a.test/x".into()), ..site }),
            None
        );
        assert_eq!(map_command(&fields("perm-reset-all")), Some(Action::ResetAll));
        assert_eq!(map_command(&fields("denied-clear")), Some(Action::ClearDenied));
        for a in ["perm-reset-all", "denied-clear"] {
            for extra in [
                PanelFields { id: Some(1), ..fields(a) },
                PanelFields { n: Some(1), ..fields(a) },
                PanelFields { text: Some("https://a.test".into()), ..fields(a) },
                PanelFields { on: Some(true), ..fields(a) },
            ] {
                assert_eq!(map_command(&extra), None, "{extra:?}");
            }
        }
        for a in ["permission", "perm", "perm-set-all", "reset", "clear-stats", "clear-data", ""] {
            assert_eq!(map_command(&fields(a)), None, "{a}");
        }
    }

    #[test]
    fn command_actions_pass_the_wire_grammar() {
        for raw in [
            r#"{"v":1,"t":"priv","a":"perm-set","id":4,"n":2,"text":"https://a.test"}"#,
            r#"{"v":1,"t":"priv","a":"perm-reset","text":"https://a.test"}"#,
            r#"{"v":1,"t":"priv","a":"perm-reset-all"}"#,
            r#"{"v":1,"t":"priv","a":"denied-clear"}"#,
        ] {
            let command = protocol::parse_command(raw).unwrap_or_else(|error| panic!("{raw}: {error:?}"));
            let protocol::Command::Panel { f, .. } = command else {
                panic!("{raw}");
            };
            assert!(map_command(&f).is_some(), "{raw}");
        }
    }

    #[test]
    fn site_rows_list_the_current_site_first_then_stored_sites() {
        let store = store_with(&[
            ("https://b.test", Permission::Camera, Decision::Allow),
            ("https://a.test", Permission::Popups, Decision::Allow),
            ("https://a.test", Permission::Microphone, Decision::Block),
        ]);
        let rows = site_rows(&store, Some("https://c.test"), &[]);
        assert_eq!(
            rows.iter().map(|row| (row.origin.as_str(), row.current)).collect::<Vec<_>>(),
            [("https://c.test", true), ("https://a.test", false), ("https://b.test", false)]
        );
        assert_eq!(rows[0].kinds, [None; 5], "the current site has nothing stored yet");
        assert_eq!(
            rows[1].kinds,
            [None, Some(Decision::Block), None, None, Some(Decision::Allow)]
        );
        // A stored current site is listed once, as current.
        let rows = site_rows(&store, Some("https://b.test"), &[]);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].current && rows[0].origin == "https://b.test");
        // No current site, a non-web page, or an origin that is not canonical.
        assert_eq!(site_rows(&store, None, &[]).len(), 2);
        assert_eq!(site_rows(&store, Some("file:///C:/x"), &[]).len(), 2);
        assert_eq!(site_rows(&store, Some("https://b.test/path"), &[]).len(), 2);
    }

    #[test]
    fn recently_denied_sites_are_listed_so_they_can_be_changed() {
        let store = store_with(&[("https://b.test", Permission::Camera, Decision::Allow)]);
        let rows = site_rows(
            &store,
            Some("https://c.test"),
            &["https://z.test", "https://b.test", "https://a.test", "https://z.test", "not an origin", "https://c.test"],
        );
        assert_eq!(
            rows.iter().map(|row| row.origin.as_str()).collect::<Vec<_>>(),
            ["https://c.test", "https://a.test", "https://b.test", "https://z.test"],
            "each origin once, the current site first, junk dropped"
        );
        assert_eq!(rows[1].kinds, [None; 5], "a denied site has nothing stored yet");
        assert_eq!(rows[2].kinds[0], Some(Decision::Allow), "camera is the first kind");
    }

    #[test]
    fn site_rows_are_capped() {
        let mut store = PermissionStore::default();
        for index in 0..MAX_SITES + 20 {
            store
                .set(&format!("https://site{index:04}.test"), Permission::Camera, Some(Decision::Allow))
                .unwrap();
        }
        let rows = site_rows(&store, Some("https://current.test"), &[]);
        assert_eq!(rows.len(), MAX_SITES);
        assert!(rows[0].current);
    }

    #[test]
    fn permissions_snapshot_is_stable_and_never_carries_page_content() {
        let store = store_with(&[
            ("https://a.test", Permission::Camera, Decision::Allow),
            ("https://a.test", Permission::Geolocation, Decision::Block),
        ]);
        let mut log = permissions::DeniedLog::default();
        log.record("https://a.test", Permission::Notifications, 90);
        log.record("https://a.test", Permission::Notifications, 95);
        log.record("https://b.test", Permission::Popups, 100);
        let denied: Vec<_> = log.recent().cloned().collect();
        let json = Json::object(|out| {
            write_permissions(out, &site_rows(&store, Some("https://b.test"), &[]), &denied, 160);
        });
        assert_eq!(
            json,
            concat!(
                r#"{"sites":["#,
                r#"{"origin":"https://b.test","current":true,"kinds":["default","default","default","default","default"]},"#,
                r#"{"origin":"https://a.test","current":false,"kinds":["allow","default","block","default","default"]}],"#,
                r#""denied":[{"origin":"https://b.test","kind":"popups","ago":60,"n":1},"#,
                r#"{"origin":"https://a.test","kind":"notifications","ago":65,"n":2}]}"#
            )
        );
    }

    #[test]
    fn privacy_dashboard_fixture_matches_the_ui_contract() {
        let store = store_with(&[
            ("https://meet.example", Permission::Camera, Decision::Allow),
            ("https://meet.example", Permission::Microphone, Decision::Allow),
            ("http://localhost:8080", Permission::Geolocation, Decision::Block),
            ("https://news.example", Permission::Popups, Decision::Allow),
        ]);
        let mut log = permissions::DeniedLog::default();
        log.record("https://ads.example", Permission::Popups, 940);
        log.record("https://chat.example", Permission::Notifications, 970);
        log.record("https://chat.example", Permission::Notifications, 975);
        log.record("https://maps.example", Permission::Geolocation, 990);
        let denied: Vec<_> = log.recent().cloned().collect();
        let also: Vec<&str> = denied.iter().map(|attempt| attempt.origin.as_str()).collect();
        let sites = site_rows(&store, Some("https://news.example"), &also);
        let snapshot = protocol::snap_json("privacy", |out| {
            out.string("profile", "Default");
            out.bool("guest", false);
            out.object("stats", |stats| {
                stats.uint("allTime", 4);
                stats.uint("session", 2);
                stats.uint("currentSite", 1);
                stats.array("topSites", |items| {
                    items.object(|item| {
                        item.string("host", "tracker.example");
                        item.uint("count", 4);
                    });
                });
                stats.array("days", |items| {
                    items.object(|item| {
                        item.string("day", "2026-10-12");
                        item.uint("count", 4);
                    });
                });
            });
            out.string("site", "news.example");
            out.string("security", "secure");
            out.bool("blocking", true);
            out.string("tracking", "Balanced");
            out.string("trackingSetting", "balanced");
            out.bool("efficiencyActive", false);
            out.bool("smartScreen", true);
            out.bool("blockerSetting", true);
            out.bool("sleeping", false);
            out.uint("sleepAfter", 30);
            write_permissions(out, &sites, &denied, 1000);
            out.null("status");
        })
        .expect("privacy is a valid panel token");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("ui/test/fixtures/privacy-snapshot.json");
        let fixture = format!("{snapshot}\n");
        if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some() {
            std::fs::write(&path, &fixture).expect("write privacy fixture");
        }
        assert_eq!(
            std::fs::read_to_string(&path).expect("privacy fixture"),
            fixture,
            "set BOWSER_WRITE_FIXTURES=1 cargo test privacy_dashboard_fixture --lib"
        );
    }
}
