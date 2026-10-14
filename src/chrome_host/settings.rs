//! The settings page's wire and the pure checks behind its commands (docs/design/settings-page.md).
//!
//! - `settingsgo` tells the page which section to show (`""` is the top).
//! - `snap` with `p:"settings"` carries the profile's settings plus the read-only state the page
//!   shows (profile, guest, version, updates, the answer to the last change).
//! - `{t:"set",a:...}` commands come back; [`map_command`] accepts each action only with exactly its
//!   own fields, and [`apply_settings_text`] keeps the dialog's exact-match `save` rules.

use super::json::{capped, Json};
use super::protocol::{snap_json, PanelFields};
use crate::internal_page::Section;
use crate::settings::{Settings, TabLayout};

/// Longest `save` payload (the page's `MAX_TEXT`).
pub const MAX_SAVE_TEXT: usize = 2048;
const MAX_PROFILE_CHARS: usize = 64;
const MAX_AVATAR_CHARS: usize = 4;
const MAX_ERROR_CHARS: usize = 240;

/// `{"v":1,"t":"settingsgo","s":"<section key>"}`; `s` is empty for the top of the page.
pub fn go_json(section: Option<Section>) -> String {
    Json::object(|o| {
        o.int("v", 1);
        o.string("t", "settingsgo");
        o.string("s", section.map_or("", Section::key));
    })
}

/// How Bowser answered the page's last change. Sent once, with the snapshot that follows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ack {
    #[default]
    None,
    Saved,
    Failed,
}

impl Ack {
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Saved => "saved",
            Self::Failed => "failed",
        }
    }
}

/// Everything the snapshot reports. Strings are capped on the way out.
#[derive(Clone, Debug)]
pub struct Snapshot<'a> {
    pub settings: &'a Settings,
    /// `settings.txt` could be read (otherwise the defaults are shown and Save is refused).
    pub readable: bool,
    /// No engine is chosen and a valid `search-provider.txt` is in use.
    pub custom_search: bool,
    pub efficiency_active: bool,
    pub auto_update: bool,
    pub history_enabled: bool,
    pub guest: bool,
    pub profile: &'a str,
    pub avatar: &'a str,
    pub version: &'a str,
    /// `unavailable`, `checking`, `ready` or `idle`.
    pub update: &'static str,
    /// The staged version when `update` is `ready`, else empty.
    pub update_version: &'a str,
    /// `none`, `uptodate`, `failed` or `other`: the last background or manual check.
    pub last_check: &'static str,
    /// The page's own last "Check for updates": `none`, `staged`, `uptodate`, `unverified`, `busy`,
    /// `unavailable` or `failed`.
    pub check_result: &'static str,
    pub ack: Ack,
    pub error: &'a str,
}

pub fn snapshot_json(s: &Snapshot<'_>) -> String {
    let settings = s.settings;
    snap_json("settings", |out| {
        out.string("tabLayout", settings.tab_layout.key());
        out.uint("sidebarWidth", u64::from(settings.sidebar_width));
        out.bool("sleepEnabled", settings.sleep_enabled);
        out.uint("sleepAfterSecs", settings.sleep_after_secs);
        out.bool("blockerEnabled", settings.blocker_enabled);
        out.array("blockerAllow", |items| for site in &settings.blocker_allow { items.string(site); });
        if let Some(engine) = settings.search_engine { out.string("searchEngine", engine.key()); } else { out.null("searchEngine"); }
        out.string("searchFallback", if settings.search_engine.is_none() && s.custom_search { "custom" } else { "default" });
        out.bool("automaticTabGrouping", settings.automatic_tab_grouping);
        out.string("efficiencyMode", settings.efficiency_mode.key());
        out.bool("efficiencyActive", s.efficiency_active);
        out.string("trackingPrevention", settings.tracking_prevention.key());
        out.bool("autoUpdate", s.auto_update);
        out.bool("historyEnabled", s.history_enabled && !s.guest);
        out.bool("guest", s.guest);
        out.string("profile", capped(s.profile, MAX_PROFILE_CHARS));
        out.string("avatar", capped(s.avatar, MAX_AVATAR_CHARS));
        out.string("version", capped(s.version, 32));
        out.string("update", s.update);
        out.string("updateVersion", capped(s.update_version, 32));
        out.string("lastCheck", s.last_check);
        out.string("checkResult", s.check_result);
        out.string("ack", s.ack.key());
        out.string("error", capped(s.error, MAX_ERROR_CHARS));
        out.bool("readable", s.readable);
    })
    .expect("settings is a panel token")
}

/// A validated command from the settings page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// The complete `key=value` text (checked again by [`apply_settings_text`]).
    Save(String),
    Reset,
    OpenFile,
    /// Install-wide, applied at once.
    AutoUpdate(bool),
    /// The profile's history switch.
    History(bool),
    /// The page moved to a section (`None` is the top): the tab's address follows.
    Section(Option<Section>),
    CheckUpdates,
    RestartUpdate,
    Privacy,
    Profiles,
    Import,
    /// The page wants a fresh snapshot (it regained focus).
    Refresh,
}

/// Maps `{t:"set",a,...}` to an [`Action`]: each action with exactly its own fields, nothing else.
pub fn map_command(f: &PanelFields) -> Option<Action> {
    if f.id.is_some() || f.parent.is_some() || f.before.is_some() || f.n.is_some() || f.url.is_some() {
        return None;
    }
    let bare = f.on.is_none() && f.text.is_none();
    let text_only = f.on.is_none() && f.text.is_some();
    let on_only = f.text.is_none() && f.on.is_some();
    Some(match f.a.as_str() {
        "save" if text_only => {
            let text = f.text.as_deref()?;
            if text.len() > MAX_SAVE_TEXT { return None; }
            Action::Save(text.to_owned())
        }
        "section" if text_only => match f.text.as_deref()? {
            "" => Action::Section(None),
            key => Action::Section(Some(Section::from_key(key)?)),
        },
        "autoupdate" if on_only => Action::AutoUpdate(f.on?),
        "history" if on_only => Action::History(f.on?),
        "reset" if bare => Action::Reset,
        "openfile" if bare => Action::OpenFile,
        "checkupdates" if bare => Action::CheckUpdates,
        "restartupdate" if bare => Action::RestartUpdate,
        "privacy" if bare => Action::Privacy,
        "profiles" if bare => Action::Profiles,
        "import" if bare => Action::Import,
        "refresh" if bare => Action::Refresh,
        _ => return None,
    })
}

/// Applies a `save` payload: every profile setting exactly once, exact keys and values, nothing
/// else (the update switch is install-wide and never part of it). `false` leaves the caller's copy
/// to be discarded.
pub fn apply_settings_text(settings: &mut Settings, text: &str) -> bool {
    if text.len() > MAX_SAVE_TEXT { return false; }
    let mut seen = std::collections::HashSet::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { return false; };
        if !seen.insert(key) { return false; }
        match key {
            "tab_layout" => settings.tab_layout = match TabLayout::ALL.into_iter().find(|layout| layout.key() == value) { Some(layout) => layout, None => return false },
            "sidebar_width" => { let Ok(width) = value.parse::<u16>() else { return false; }; if !(crate::settings::MIN_SIDEBAR_WIDTH..=crate::settings::MAX_SIDEBAR_WIDTH).contains(&width) { return false; } settings.sidebar_width = width; }
            "sleep_enabled" => settings.sleep_enabled = match value { "true" => true, "false" => false, _ => return false },
            "sleep_after_secs" => { let Ok(value) = crate::settings::parse_sleep_after_secs(value) else { return false; }; settings.sleep_after_secs = value; }
            "blocker_enabled" => settings.blocker_enabled = match value { "true" => true, "false" => false, _ => return false },
            "blocker_allow" => { let Ok(value) = crate::settings::parse_blocker_allow(value) else { return false; }; settings.blocker_allow = value; }
            "automatic_tab_grouping" => settings.automatic_tab_grouping = match value { "true" => true, "false" => false, _ => return false },
            "search_engine" => { let Some(value) = crate::navigation::SearchEngine::parse(value) else { return false; }; settings.search_engine = Some(value); }
            "efficiency_mode" => { let Some(value) = crate::settings::EfficiencyMode::parse(value) else { return false; }; settings.efficiency_mode = value; }
            "tracking_prevention" => { let Some(value) = crate::settings::TrackingLevel::parse(value) else { return false; }; settings.tracking_prevention = value; }
            _ => return false,
        }
    }
    ["tab_layout", "sidebar_width", "sleep_enabled", "sleep_after_secs", "blocker_enabled", "blocker_allow", "automatic_tab_grouping", "efficiency_mode", "tracking_prevention"].iter().all(|key| seen.contains(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(a: &str) -> PanelFields {
        PanelFields { a: a.to_owned(), id: None, parent: None, before: None, n: None, on: None, text: None, url: None }
    }

    #[test]
    fn settings_payload_updates_only_known_values_and_requires_every_control() {
        let mut settings = Settings::default();
        let valid = "tab_layout=vertical\nsidebar_width=300\nsleep_enabled=false\nsleep_after_secs=900\nblocker_enabled=true\nblocker_allow=example.com\nautomatic_tab_grouping=false\nefficiency_mode=on\ntracking_prevention=strict";
        assert!(apply_settings_text(&mut settings, valid));
        assert_eq!(settings.tab_layout, TabLayout::Vertical);
        assert_eq!(settings.sidebar_width, 300);
        assert!(!settings.sleep_enabled);
        assert!(!settings.automatic_tab_grouping);
        assert_eq!(settings.efficiency_mode, crate::settings::EfficiencyMode::On);
        assert_eq!(settings.tracking_prevention, crate::settings::TrackingLevel::Strict);
        for layout in TabLayout::ALL {
            assert!(apply_settings_text(&mut settings, &valid.replace("tab_layout=vertical", &format!("tab_layout={}", layout.key()))), "{layout:?}");
            assert_eq!(settings.tab_layout, layout);
        }
        assert!(!apply_settings_text(&mut settings, &valid.replace("tab_layout=vertical", "tab_layout=Tabs-Top")), "keys are exact, not case-insensitive");
        assert!(!apply_settings_text(&mut settings, &valid.replace("tab_layout=vertical", "tab_layout=toolbar-top")), "the persisted key for toolbar-above-tabs is horizontal");
        assert!(!apply_settings_text(&mut settings, "tab_layout=wrong"));
        assert!(!apply_settings_text(&mut settings, &valid.replace("sidebar_width=300", "sidebar_width=179")));
        assert!(!apply_settings_text(&mut settings, &valid.replace("sleep_after_secs=900", "sleep_after_secs=29")));
        assert!(!apply_settings_text(&mut settings, &valid.replace("blocker_allow=example.com", "blocker_allow=bad site.example")));
        assert!(!apply_settings_text(&mut settings, &valid.replace("efficiency_mode=on", "efficiency_mode=sometimes")));
        assert!(!apply_settings_text(&mut settings, &valid.replace("tracking_prevention=strict", "tracking_prevention=off")));
        assert!(!apply_settings_text(&mut settings, &valid.replace("\nefficiency_mode=on", "")));
        assert!(!apply_settings_text(&mut settings, &format!("{valid}\nunknown=x")));
        assert!(!apply_settings_text(&mut settings, &format!("{valid}\nblocker_enabled=false")));
        assert!(!apply_settings_text(&mut settings, &format!("{valid}\nauto_update=false")), "the update switch is not a profile setting");
        assert!(!apply_settings_text(&mut settings, &format!("{valid}\nsearch_engine=bing")));
        assert!(apply_settings_text(&mut settings, &format!("{valid}\nsearch_engine=google")));
        assert!(!apply_settings_text(&mut settings, &"x".repeat(MAX_SAVE_TEXT + 1)));
    }

    #[test]
    fn commands_carry_exactly_their_own_fields() {
        let mut save = fields("save");
        save.text = Some("tab_layout=vertical".into());
        assert_eq!(map_command(&save), Some(Action::Save("tab_layout=vertical".into())));
        let mut extra = save.clone();
        extra.on = Some(true);
        assert_eq!(map_command(&extra), None);
        let mut id = save.clone();
        id.id = Some(1);
        assert_eq!(map_command(&id), None);
        let mut long = fields("save");
        long.text = Some("x".repeat(MAX_SAVE_TEXT + 1));
        assert_eq!(map_command(&long), None);
        assert_eq!(map_command(&fields("save")), None, "save needs its text");

        let mut section = fields("section");
        section.text = Some("privacy".into());
        assert_eq!(map_command(&section), Some(Action::Section(Some(Section::Privacy))));
        section.text = Some(String::new());
        assert_eq!(map_command(&section), Some(Action::Section(None)));
        for bad in ["Privacy", "nope", "privacy/x", " privacy"] {
            section.text = Some(bad.into());
            assert_eq!(map_command(&section), None, "{bad}");
        }

        let mut update = fields("autoupdate");
        assert_eq!(map_command(&update), None);
        update.on = Some(false);
        assert_eq!(map_command(&update), Some(Action::AutoUpdate(false)));
        update.text = Some("x".into());
        assert_eq!(map_command(&update), None);
        let mut history = fields("history");
        history.on = Some(true);
        assert_eq!(map_command(&history), Some(Action::History(true)));

        for (name, action) in [("reset", Action::Reset), ("openfile", Action::OpenFile), ("checkupdates", Action::CheckUpdates), ("restartupdate", Action::RestartUpdate), ("privacy", Action::Privacy), ("profiles", Action::Profiles), ("import", Action::Import), ("refresh", Action::Refresh)] {
            assert_eq!(map_command(&fields(name)), Some(action), "{name}");
            let mut with_on = fields(name);
            with_on.on = Some(true);
            assert_eq!(map_command(&with_on), None, "{name} with on");
            let mut with_url = fields(name);
            with_url.url = Some("https://example.com/".into());
            assert_eq!(map_command(&with_url), None, "{name} with url");
        }
        for unknown in ["open", "dismiss", "clear-data", "exit", "navigate"] {
            assert_eq!(map_command(&fields(unknown)), None, "{unknown}");
        }
    }

    fn sample<'a>(settings: &'a Settings) -> Snapshot<'a> {
        Snapshot {
            settings,
            readable: true,
            custom_search: false,
            efficiency_active: true,
            auto_update: true,
            history_enabled: true,
            guest: false,
            profile: "Work \"<b>\"",
            avatar: "W",
            version: "0.4.2",
            update: "ready",
            update_version: "0.4.3",
            last_check: "uptodate",
            check_result: "staged",
            ack: Ack::Saved,
            error: "",
        }
    }

    #[test]
    fn the_snapshot_reports_every_field_and_guests_never_record_history() {
        let mut settings = Settings::default();
        settings.blocker_allow = vec!["example.com".into()];
        let json = snapshot_json(&sample(&settings));
        assert!(json.starts_with(r#"{"v":1,"t":"snap","p":"settings","d":{"tabLayout":"horizontal","sidebarWidth":260,"#), "{json}");
        assert!(json.contains(r#""searchEngine":null,"searchFallback":"default""#), "{json}");
        assert!(json.contains(r#""profile":"Work \"\u003cb>\"""#), "{json}");
        assert!(json.ends_with(r#""ack":"saved","error":"","readable":true}}"#), "{json}");
        let guest = Snapshot { guest: true, ..sample(&settings) };
        assert!(snapshot_json(&guest).contains(r#""historyEnabled":false,"guest":true"#));
        let custom = Snapshot { custom_search: true, ..sample(&settings) };
        assert!(snapshot_json(&custom).contains(r#""searchFallback":"custom""#));
        settings.search_engine = Some(crate::navigation::SearchEngine::Google);
        let chosen = Snapshot { custom_search: true, ..sample(&settings) };
        assert!(snapshot_json(&chosen).contains(r#""searchEngine":"google","searchFallback":"default""#), "a chosen engine wins over the file");
        assert_eq!(go_json(Some(Section::Updates)), r#"{"v":1,"t":"settingsgo","s":"updates"}"#);
        assert_eq!(go_json(None), r#"{"v":1,"t":"settingsgo","s":""}"#);
    }

    #[test]
    fn settings_messages_match_committed_fixture() {
        let mut settings = Settings::default();
        settings.blocker_allow = vec!["example.com".into(), "news.example".into()];
        settings.search_engine = Some(crate::navigation::SearchEngine::Optimised);
        let snap = snapshot_json(&sample(&settings));
        let guest_settings = Settings::default();
        let guest = snapshot_json(&Snapshot { guest: true, profile: "Guest", avatar: "G", update: "unavailable", update_version: "", last_check: "none", check_result: "none", ack: Ack::None, custom_search: true, ..sample(&guest_settings) });
        let content = format!(
            "{{\"snap\":{snap},\"guest\":{guest},\"go\":{},\"goTop\":{}}}\n",
            go_json(Some(Section::Privacy)),
            go_json(None),
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/settings-messages.json");
        if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some_and(|value| value == "1") {
            std::fs::create_dir_all(path.parent().expect("fixtures directory")).expect("create fixtures directory");
            std::fs::write(&path, content).expect("write settings messages fixture");
        } else {
            assert_eq!(std::fs::read_to_string(path).expect("committed settings messages fixture"), content);
        }
    }
}
