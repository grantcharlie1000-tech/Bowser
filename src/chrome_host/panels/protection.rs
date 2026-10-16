//! The Privacy section's blocking, cookie-notice, history and website-appearance controls: the
//! `protection` snapshot and the commands that change them. Pure: the caller reads the app and
//! passes plain data in, and every command is checked here before anything acts on it.
//!
//! Wire (snapshot `protection`, see docs/html-chrome-protocol.md):
//! `level` (`none|trackers|ads`), `cookieNotices`, `retention` (`session|1d|1w|1m|3m|6m|1y|forever`),
//! `websiteAppearance` (`auto|light|dark`), `forceDark`, `privateTheme` (`follow|light|dark`),
//! `readable`, `guest`, `current` (the active tab's site or `null`), `sites` (saved per-site
//! choices), `allow` (sites `blocker_allow` exempts), `sources` (the lists the blocker is built
//! from), `blocked` (counts), `result` (how the last command went) and `busy`.
//!
//! Commands (`{t:"priv"}`, one family with the permission dashboard's): `blocklevel` (`text` a
//! level), `cookienotices` (`on`), `site-level` (`text` site, `n` 0 none / 1 trackers / 2 trackers
//! and ads / 3 follow the global level), `site-cookies` (`text` site, `n` 0 show / 1 hide / 2
//! follow), `site-remove` (`text` site), `src-toggle` (`text` id, `on`), `src-add-url` (`url`,
//! `text` kind), `src-add-file` (`text` kind), `src-update` (`text` id), `src-remove` (`text` id),
//! `retention` (`text`), `webappearance` (`text`), `forcedark` (`on`), `privatetheme` (`text`) and
//! `delete-data` (`id` kinds bitmask, `n` range 0 to 4, `on` the saved sign-ins confirmation).

use super::super::{json::Object, protocol::PanelFields};
use crate::{
    blocker::{Level, site_rules::SiteRule, sources::SourceKind},
    browsing_data::{Range, Selection},
    settings::{BlockerLevel, HistoryRetention, PrivateTheme, Settings, WebsiteAppearance},
};

/// Most saved per-site choices in one snapshot (the file holds up to 1000; the page lists the first).
pub const MAX_SITES_SHOWN: usize = 200;
/// Most `blocker_allow` sites in one snapshot.
pub const MAX_ALLOW_SHOWN: usize = 100;
/// Longest site, list name, list id or message in the snapshot, in characters.
pub const MAX_FIELD_CHARS: usize = 253;
pub const MAX_RESULT_CHARS: usize = 400;
/// Longest address a list can be added from.
pub const MAX_URL_CHARS: usize = crate::blocker::fetch::MAX_URL_BYTES;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrentSite {
    pub host: String,
    pub level: Level,
    /// Whether cookie notices are hidden on it.
    pub cookies: bool,
    pub private: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRow {
    pub id: String,
    pub name: String,
    pub kind: SourceKind,
    pub enabled: bool,
    /// `builtin`, `filters`, `file` or `url`.
    pub origin: &'static str,
    pub url: String,
    pub rules: usize,
    pub removable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandResult {
    pub seq: u64,
    pub ok: bool,
    pub text: String,
}

/// Everything the snapshot reports.
#[derive(Clone, Debug)]
pub struct View {
    pub level: Level,
    pub cookie_notices: bool,
    pub retention: HistoryRetention,
    pub website_appearance: WebsiteAppearance,
    pub force_dark: bool,
    pub private_theme: PrivateTheme,
    pub readable: bool,
    pub guest: bool,
    pub current: Option<CurrentSite>,
    pub sites: Vec<(String, SiteRule)>,
    pub allow: Vec<String>,
    pub sources: Vec<SourceRow>,
    pub blocked_page: u32,
    pub blocked_session: u64,
    pub result: Option<CommandResult>,
    pub busy: bool,
}

fn capped(text: &str, max: usize) -> &str {
    crate::chrome_host::json::capped(text, max)
}

/// The `protection` snapshot's data object.
pub fn write(out: &mut Object<'_>, view: &View) {
    out.string("level", view.level.key());
    out.bool("cookieNotices", view.cookie_notices);
    out.string("retention", view.retention.key());
    out.string("websiteAppearance", view.website_appearance.key());
    out.bool("forceDark", view.force_dark);
    out.string("privateTheme", view.private_theme.key());
    out.bool("readable", view.readable);
    out.bool("guest", view.guest);
    match &view.current {
        Some(site) => out.object("current", |item| {
            item.string("host", capped(&site.host, MAX_FIELD_CHARS));
            item.string("level", site.level.key());
            item.bool("cookies", site.cookies);
            item.bool("private", site.private);
        }),
        None => out.null("current"),
    }
    out.array("sites", |items| {
        for (site, rule) in view.sites.iter().take(MAX_SITES_SHOWN) {
            items.object(|item| {
                item.string("site", capped(site, MAX_FIELD_CHARS));
                item.string("level", rule.level.map_or("default", Level::key));
                item.string("cookies", match rule.cookies {
                    Some(true) => "hide",
                    Some(false) => "show",
                    None => "default",
                });
            });
        }
    });
    out.array("allow", |items| {
        for site in view.allow.iter().take(MAX_ALLOW_SHOWN) {
            items.string(capped(site, MAX_FIELD_CHARS));
        }
    });
    out.array("sources", |items| {
        for source in view.sources.iter().take(crate::blocker::sources::MAX_SOURCES) {
            items.object(|item| {
                item.string("id", capped(&source.id, 24));
                item.string("name", capped(&source.name, crate::blocker::sources::MAX_NAME_CHARS));
                item.string("kind", source.kind.key());
                item.bool("enabled", source.enabled);
                item.string("origin", source.origin);
                item.string("url", capped(&source.url, MAX_URL_CHARS));
                item.uint("rules", source.rules as u64);
                item.bool("removable", source.removable);
            });
        }
    });
    out.object("blocked", |item| {
        item.uint("page", u64::from(view.blocked_page));
        item.uint("session", view.blocked_session);
    });
    match &view.result {
        Some(result) => out.object("result", |item| {
            item.uint("seq", result.seq);
            item.bool("ok", result.ok);
            item.string("text", capped(&result.text, MAX_RESULT_CHARS));
        }),
        None => out.null("result"),
    }
    out.bool("busy", view.busy);
}

/// A validated command from the Privacy section's blocking, history and appearance controls.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Level(Level),
    CookieNotices(bool),
    SiteLevel { site: String, level: Option<Level> },
    SiteCookies { site: String, hide: Option<bool> },
    SiteRemove(String),
    SourceToggle { id: String, on: bool },
    SourceAddUrl { url: String, kind: SourceKind },
    SourceAddFile { kind: SourceKind },
    SourceUpdate(String),
    SourceRemove(String),
    Retention(HistoryRetention),
    WebsiteAppearance(WebsiteAppearance),
    ForceDark(bool),
    PrivateTheme(PrivateTheme),
    DeleteData { selection: Selection, range: Range, logins_confirmed: bool },
}

/// Applies a command that is a saved setting to `settings`; `false` (nothing changed) for any
/// other command. The blocking level "none" is `blocker_enabled=false` and keeps the level that was
/// chosen before, so switching blocking back on returns to it.
pub fn apply_to_settings(action: &Action, settings: &mut Settings) -> bool {
    match action {
        Action::Level(Level::None) => settings.blocker_enabled = false,
        Action::Level(Level::Trackers) => {
            settings.blocker_enabled = true;
            settings.blocker_level = BlockerLevel::Trackers;
        }
        Action::Level(Level::Ads) => {
            settings.blocker_enabled = true;
            settings.blocker_level = BlockerLevel::Ads;
        }
        Action::CookieNotices(on) => settings.cookie_notices = *on,
        Action::Retention(retention) => settings.history_retention = *retention,
        Action::WebsiteAppearance(appearance) => settings.website_appearance = *appearance,
        Action::ForceDark(on) => settings.force_dark_websites = *on,
        Action::PrivateTheme(theme) => settings.private_theme = *theme,
        _ => return false,
    }
    true
}

/// A site as the page types it into the per-site list: a host with at least one dot, no scheme.
/// The blocker normalises it again; this only refuses text that could not be a site.
fn site_text(text: &str) -> Option<String> {
    let site = text.trim();
    let valid = !site.is_empty()
        && site.chars().count() <= MAX_FIELD_CHARS
        && site.contains('.')
        && !site.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '\\' | ':' | '@' | '?' | '#' | ','));
    valid.then(|| site.to_owned())
}

fn id_text(text: &str) -> Option<String> {
    let valid = (1..=24).contains(&text.len())
        && text.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    valid.then(|| text.to_owned())
}

/// Maps one `priv` panel command to an [`Action`], or `None` when it is not one of these or does
/// not validate: each action carries exactly its own fields and nothing else.
pub fn map_command(f: &PanelFields) -> Option<Action> {
    let text = f.text.as_deref();
    let only = |id: bool, parent: bool, before: bool, n: bool, on: bool, text_set: bool, url: bool| {
        f.id.is_some() == id
            && f.parent.is_some() == parent
            && f.before.is_some() == before
            && f.n.is_some() == n
            && f.on.is_some() == on
            && f.text.is_some() == text_set
            && f.url.is_some() == url
    };
    match f.a.as_str() {
        "blocklevel" if only(false, false, false, false, false, true, false) => Some(Action::Level(Level::parse(text?)?)),
        "cookienotices" if only(false, false, false, false, true, false, false) => Some(Action::CookieNotices(f.on?)),
        "site-level" if only(false, false, false, true, false, true, false) => {
            let level = match f.n? {
                0 => Some(Level::None),
                1 => Some(Level::Trackers),
                2 => Some(Level::Ads),
                3 => None,
                _ => return None,
            };
            Some(Action::SiteLevel { site: site_text(text?)?, level })
        }
        "site-cookies" if only(false, false, false, true, false, true, false) => {
            let hide = match f.n? {
                0 => Some(false),
                1 => Some(true),
                2 => None,
                _ => return None,
            };
            Some(Action::SiteCookies { site: site_text(text?)?, hide })
        }
        "site-remove" if only(false, false, false, false, false, true, false) => Some(Action::SiteRemove(site_text(text?)?)),
        "src-toggle" if only(false, false, false, false, true, true, false) => {
            Some(Action::SourceToggle { id: id_text(text?)?, on: f.on? })
        }
        "src-add-url" if only(false, false, false, false, false, true, true) => {
            let url = f.url.as_deref()?.trim();
            let kind = SourceKind::parse(text?)?;
            (url.chars().count() <= MAX_URL_CHARS && !url.is_empty())
                .then(|| Action::SourceAddUrl { url: url.to_owned(), kind })
        }
        "src-add-file" if only(false, false, false, false, false, true, false) => {
            Some(Action::SourceAddFile { kind: SourceKind::parse(text?)? })
        }
        "src-update" if only(false, false, false, false, false, true, false) => Some(Action::SourceUpdate(id_text(text?)?)),
        "src-remove" if only(false, false, false, false, false, true, false) => Some(Action::SourceRemove(id_text(text?)?)),
        "retention" if only(false, false, false, false, false, true, false) => Some(Action::Retention(HistoryRetention::parse(text?)?)),
        "webappearance" if only(false, false, false, false, false, true, false) => {
            Some(Action::WebsiteAppearance(WebsiteAppearance::parse(text?)?))
        }
        "forcedark" if only(false, false, false, false, true, false, false) => Some(Action::ForceDark(f.on?)),
        "privatetheme" if only(false, false, false, false, false, true, false) => Some(Action::PrivateTheme(PrivateTheme::parse(text?)?)),
        "delete-data" if only(true, false, false, true, true, false, false) => {
            let selection = Selection::from_bits(u32::try_from(f.id?).ok()?)?;
            let range = Range::from_index(u32::try_from(f.n?).ok()?)?;
            Some(Action::DeleteData { selection, range, logins_confirmed: f.on? })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_host::json::Json;

    fn fields(a: &str) -> PanelFields {
        PanelFields { a: a.to_owned(), ..PanelFields::default() }
    }

    fn with_text(a: &str, text: &str) -> PanelFields {
        PanelFields { text: Some(text.to_owned()), ..fields(a) }
    }

    fn view() -> View {
        View {
            level: Level::Trackers,
            cookie_notices: true,
            retention: HistoryRetention::Months3,
            website_appearance: WebsiteAppearance::Dark,
            force_dark: true,
            private_theme: PrivateTheme::Follow,
            readable: true,
            guest: false,
            current: Some(CurrentSite { host: "example.com".into(), level: Level::Ads, cookies: false, private: false }),
            sites: vec![
                ("a.test".into(), SiteRule { level: Some(Level::None), cookies: None }),
                ("b.test".into(), SiteRule { level: None, cookies: Some(true) }),
            ],
            allow: vec!["allowed.test".into()],
            sources: vec![SourceRow {
                id: "u1".into(),
                name: "My \"list\"".into(),
                kind: SourceKind::Cookies,
                enabled: false,
                origin: "url",
                url: "https://lists.example/x.txt".into(),
                rules: 12,
                removable: true,
            }],
            blocked_page: 3,
            blocked_session: 40,
            result: Some(CommandResult { seq: 2, ok: true, text: "Added.".into() }),
            busy: false,
        }
    }

    #[test]
    fn the_snapshot_has_exactly_the_keys_the_page_expects() {
        let json = Json::object(|out| write(out, &view()));
        assert_eq!(
            json,
            "{\"level\":\"trackers\",\"cookieNotices\":true,\"retention\":\"3m\",\"websiteAppearance\":\"dark\",\
             \"forceDark\":true,\"privateTheme\":\"follow\",\"readable\":true,\"guest\":false,\
             \"current\":{\"host\":\"example.com\",\"level\":\"ads\",\"cookies\":false,\"private\":false},\
             \"sites\":[{\"site\":\"a.test\",\"level\":\"none\",\"cookies\":\"default\"},\
             {\"site\":\"b.test\",\"level\":\"default\",\"cookies\":\"hide\"}],\
             \"allow\":[\"allowed.test\"],\
             \"sources\":[{\"id\":\"u1\",\"name\":\"My \\\"list\\\"\",\"kind\":\"cookies\",\"enabled\":false,\"origin\":\"url\",\
             \"url\":\"https://lists.example/x.txt\",\"rules\":12,\"removable\":true}],\
             \"blocked\":{\"page\":3,\"session\":40},\"result\":{\"seq\":2,\"ok\":true,\"text\":\"Added.\"},\"busy\":false}"
        );
        let mut quiet = view();
        quiet.current = None;
        quiet.result = None;
        quiet.sites.clear();
        let json = Json::object(|out| write(out, &quiet));
        assert!(json.contains("\"current\":null") && json.contains("\"result\":null") && json.contains("\"sites\":[]"));
    }

    #[test]
    fn protection_fixture_matches_the_ui_contract() {
        use crate::chrome_host::protocol;
        let mut view = view();
        view.sources.insert(
            0,
            SourceRow {
                id: "bowser-trackers".into(),
                name: "Bowser tracker list".into(),
                kind: SourceKind::Trackers,
                enabled: true,
                origin: "builtin",
                url: String::new(),
                rules: 412,
                removable: false,
            },
        );
        view.sources.push(SourceRow {
            id: "u2".into(),
            name: "my-filters.txt".into(),
            kind: SourceKind::Ads,
            enabled: true,
            origin: "file",
            url: String::new(),
            rules: 3,
            removable: true,
        });
        let snapshot = protocol::snap_json("protection", |out| write(out, &view)).expect("protection is a valid panel token");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/protection-snapshot.json");
        let fixture = format!("{snapshot}\n");
        if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some() {
            std::fs::write(&path, &fixture).expect("write protection fixture");
        }
        assert_eq!(
            std::fs::read_to_string(&path).expect("protection fixture").replace("\r\n", "\n"),
            fixture,
            "set BOWSER_WRITE_FIXTURES=1 cargo test protection_fixture --lib"
        );
    }

    #[test]
    fn long_lists_and_text_are_capped_on_the_way_out() {
        let mut big = view();
        big.sites = (0..MAX_SITES_SHOWN + 50).map(|n| (format!("s{n}.test"), SiteRule { level: Some(Level::Ads), cookies: None })).collect();
        big.allow = (0..MAX_ALLOW_SHOWN + 50).map(|n| format!("a{n}.test")).collect();
        big.result = Some(CommandResult { seq: 1, ok: false, text: "x".repeat(MAX_RESULT_CHARS * 2) });
        let json = Json::object(|out| write(out, &big));
        assert_eq!(json.matches("\"site\":").count(), MAX_SITES_SHOWN);
        assert_eq!(json.matches("a0.test").count() + json.matches("a99.test").count(), 2);
        assert!(!json.contains("a100.test"));
        assert!(json.contains(&"x".repeat(MAX_RESULT_CHARS)) && !json.contains(&"x".repeat(MAX_RESULT_CHARS + 1)));
    }

    #[test]
    fn level_and_toggle_commands_take_exactly_their_field() {
        assert_eq!(map_command(&with_text("blocklevel", "none")), Some(Action::Level(Level::None)));
        assert_eq!(map_command(&with_text("blocklevel", "ads")), Some(Action::Level(Level::Ads)));
        assert_eq!(map_command(&with_text("blocklevel", "everything")), None);
        assert_eq!(map_command(&fields("blocklevel")), None);
        assert_eq!(map_command(&PanelFields { on: Some(true), ..with_text("blocklevel", "ads") }), None, "no extra field");
        assert_eq!(map_command(&PanelFields { on: Some(true), ..fields("cookienotices") }), Some(Action::CookieNotices(true)));
        assert_eq!(map_command(&with_text("cookienotices", "on")), None);
        assert_eq!(map_command(&PanelFields { on: Some(false), ..fields("forcedark") }), Some(Action::ForceDark(false)));
        assert_eq!(map_command(&with_text("retention", "forever")), Some(Action::Retention(HistoryRetention::Forever)));
        assert_eq!(map_command(&with_text("retention", "2w")), None);
        assert_eq!(map_command(&with_text("webappearance", "dark")), Some(Action::WebsiteAppearance(WebsiteAppearance::Dark)));
        assert_eq!(map_command(&with_text("webappearance", "sepia")), None);
        assert_eq!(map_command(&with_text("privatetheme", "light")), Some(Action::PrivateTheme(PrivateTheme::Light)));
        assert_eq!(map_command(&with_text("privatetheme", "")), None);
    }

    #[test]
    fn per_site_commands_check_the_site_and_the_choice() {
        let level = |site: &str, n: i32| PanelFields { n: Some(n), ..with_text("site-level", site) };
        assert_eq!(map_command(&level("example.com", 0)), Some(Action::SiteLevel { site: "example.com".into(), level: Some(Level::None) }));
        assert_eq!(map_command(&level("example.com", 1)), Some(Action::SiteLevel { site: "example.com".into(), level: Some(Level::Trackers) }));
        assert_eq!(map_command(&level("example.com", 2)), Some(Action::SiteLevel { site: "example.com".into(), level: Some(Level::Ads) }));
        assert_eq!(map_command(&level(" example.com ", 3)), Some(Action::SiteLevel { site: "example.com".into(), level: None }));
        assert_eq!(map_command(&level("example.com", 4)), None);
        assert_eq!(map_command(&level("example.com", -1)), None);
        for bad in ["", "localhost", "https://example.com", "a b.com", "a.com/x", "a.com:8080", "u@a.com", "a.com,b.com", "a.com#x", &"a.".repeat(200)] {
            assert_eq!(map_command(&level(bad, 0)), None, "{bad:?}");
        }
        let cookies = |n: i32| PanelFields { n: Some(n), ..with_text("site-cookies", "example.com") };
        assert_eq!(map_command(&cookies(0)), Some(Action::SiteCookies { site: "example.com".into(), hide: Some(false) }));
        assert_eq!(map_command(&cookies(1)), Some(Action::SiteCookies { site: "example.com".into(), hide: Some(true) }));
        assert_eq!(map_command(&cookies(2)), Some(Action::SiteCookies { site: "example.com".into(), hide: None }));
        assert_eq!(map_command(&cookies(3)), None);
        assert_eq!(map_command(&with_text("site-remove", "example.com")), Some(Action::SiteRemove("example.com".into())));
        assert_eq!(map_command(&PanelFields { n: Some(1), ..with_text("site-remove", "example.com") }), None);
    }

    #[test]
    fn list_commands_check_ids_kinds_and_addresses() {
        assert_eq!(
            map_command(&PanelFields { on: Some(false), ..with_text("src-toggle", "bowser-ads") }),
            Some(Action::SourceToggle { id: "bowser-ads".into(), on: false })
        );
        for bad in ["", "../x", "A", "a/b", &"a".repeat(25)] {
            assert_eq!(map_command(&PanelFields { on: Some(true), ..with_text("src-toggle", bad) }), None, "{bad:?}");
            assert_eq!(map_command(&with_text("src-remove", bad)), None, "{bad:?}");
            assert_eq!(map_command(&with_text("src-update", bad)), None, "{bad:?}");
        }
        let add = |kind: &str, url: &str| PanelFields { url: Some(url.to_owned()), ..with_text("src-add-url", kind) };
        assert_eq!(
            map_command(&add("cookies", " https://lists.example/c.txt ")),
            Some(Action::SourceAddUrl { url: "https://lists.example/c.txt".into(), kind: SourceKind::Cookies })
        );
        assert_eq!(map_command(&add("mystery", "https://lists.example/c.txt")), None);
        assert_eq!(map_command(&add("ads", "")), None);
        assert_eq!(map_command(&add("ads", &"a".repeat(MAX_URL_CHARS + 1))), None);
        assert_eq!(map_command(&with_text("src-add-url", "ads")), None, "an address is required");
        assert_eq!(map_command(&with_text("src-add-file", "trackers")), Some(Action::SourceAddFile { kind: SourceKind::Trackers }));
        assert_eq!(map_command(&PanelFields { url: Some("x".into()), ..with_text("src-add-file", "ads") }), None);
        assert_eq!(map_command(&with_text("src-remove", "u3")), Some(Action::SourceRemove("u3".into())));
        assert_eq!(map_command(&with_text("src-update", "u3")), Some(Action::SourceUpdate("u3".into())));
    }

    #[test]
    fn delete_data_needs_all_three_fields_and_known_kinds_and_ranges() {
        let delete = |id: u64, n: i32, on: bool| PanelFields { id: Some(id), n: Some(n), on: Some(on), ..fields("delete-data") };
        assert_eq!(
            map_command(&delete(0b101, 3, false)),
            Some(Action::DeleteData {
                selection: Selection::from_bits(0b101).unwrap(),
                range: Range::FourWeeks,
                logins_confirmed: false
            })
        );
        assert!(matches!(map_command(&delete(0b111_1111, 4, true)), Some(Action::DeleteData { logins_confirmed: true, .. })));
        assert_eq!(map_command(&delete(0b1000_0000, 0, false)), None, "a kind that does not exist");
        assert_eq!(map_command(&delete(1, 5, false)), None, "a range that does not exist");
        assert_eq!(map_command(&delete(1, -1, false)), None);
        assert_eq!(map_command(&PanelFields { on: None, ..delete(1, 0, false) }), None, "the confirmation field is required");
        assert_eq!(map_command(&PanelFields { id: None, ..delete(1, 0, false) }), None);
        assert_eq!(map_command(&PanelFields { text: Some("x".into()), ..delete(1, 0, false) }), None);
        assert_eq!(map_command(&PanelFields { id: Some(u64::MAX), ..delete(1, 0, false) }), None);
    }

    #[test]
    fn each_setting_command_changes_its_setting_and_the_change_survives_the_file() {
        let base = Settings::default();
        let apply = |command: PanelFields| {
            let action = map_command(&command).expect("a valid command");
            let mut settings = base.clone();
            assert!(apply_to_settings(&action, &mut settings), "{action:?} is a saved setting");
            // The settings page saves through settings.txt; a restart reads it back.
            let reloaded = Settings::parse(&crate::settings::render_updated("", &settings));
            assert_eq!(reloaded, settings, "{action:?} is written and read back whole");
            // Only the chosen setting differs from the defaults.
            assert_ne!(settings, base, "{action:?} changed something");
            settings
        };
        assert!(!apply(with_text("blocklevel", "none")).blocker_enabled);
        let trackers = apply(with_text("blocklevel", "trackers"));
        assert!(trackers.blocker_enabled && trackers.blocker_level == BlockerLevel::Trackers);
        // Switching blocking off keeps the chosen level, and back on returns to it.
        let mut settings = trackers.clone();
        assert!(apply_to_settings(&Action::Level(Level::None), &mut settings));
        assert!(!settings.blocker_enabled && settings.blocker_level == BlockerLevel::Trackers);
        assert!(apply_to_settings(&Action::Level(Level::Ads), &mut settings));
        assert!(settings.blocker_enabled && settings.blocker_level == BlockerLevel::Ads);
        assert!(apply(PanelFields { on: Some(true), ..fields("cookienotices") }).cookie_notices);
        assert_eq!(apply(with_text("retention", "1w")).history_retention, HistoryRetention::Week);
        assert_eq!(apply(with_text("retention", "session")).history_retention, HistoryRetention::Session);
        assert_eq!(apply(with_text("webappearance", "dark")).website_appearance, WebsiteAppearance::Dark);
        assert_eq!(apply(with_text("webappearance", "light")).website_appearance, WebsiteAppearance::Light);
        assert!(apply(PanelFields { on: Some(true), ..fields("forcedark") }).force_dark_websites);
        assert_eq!(apply(with_text("privatetheme", "dark")).private_theme, PrivateTheme::Dark);
        // The other commands are not settings.
        let mut untouched = base.clone();
        for command in [
            PanelFields { n: Some(0), ..with_text("site-level", "a.test") },
            with_text("src-remove", "u1"),
            PanelFields { id: Some(1), n: Some(0), on: Some(false), ..fields("delete-data") },
        ] {
            let action = map_command(&command).unwrap();
            assert!(!apply_to_settings(&action, &mut untouched), "{action:?}");
        }
        assert_eq!(untouched, base);
    }

    #[test]
    fn unrelated_commands_are_left_for_the_permission_dashboard() {
        for a in ["perm-set", "perm-reset", "clear-stats", "clear-data", "denied-clear", "nonsense"] {
            assert_eq!(map_command(&fields(a)), None, "{a}");
        }
    }
}
