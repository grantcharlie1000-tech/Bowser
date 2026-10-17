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
use crate::settings::{
    BookmarksBar, FontSize, SidePanelPosition, Settings, TabLayout, Theme,
};

/// Longest `save` payload (the page's `MAX_TEXT`). Raised from 2048 alongside the page's `MAX_TEXT`
/// to fit the full set of profile settings; still well under `settings::MAX_FILE_BYTES` (16 KiB).
pub const MAX_SAVE_TEXT: usize = 4096;
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
    /// Whether Bowser is the current HKCU http default: `"yes"`, `"no"` or `"unknown"` (detection
    /// failed or is not supported). Read-only: never part of `save`.
    pub default_browser: &'static str,
    /// The folder downloads are saved to, as shown (the setting, else the Downloads folder).
    pub download_dir: &'a str,
    /// "Ask where to save each file before downloading".
    pub download_ask: bool,
    /// --- tab stacks, tiling and workspaces (T10) --- the workspace rules as (site, workspace), and
    /// the workspaces a rule can name, in rail order.
    pub workspace_rules: Vec<(String, String)>,
    pub workspace_names: Vec<String>,
}

/// --- tab stacks (T10) --- longest `wsrule` text: a host, a tab, a workspace name and its verb.
const MAX_RULE_TEXT: usize = 600;

const MAX_DOWNLOAD_DIR_CHARS: usize = 260;
/// Longest `key=value` text of one tab option (the new tab address is the longest value).
const MAX_TAB_OPTION_TEXT: usize = 2100;
const MAX_THEME_OPTION_TEXT: usize = 64;

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
        out.string("theme", settings.theme.key());
        out.string("themeAccent", settings.theme_accent.as_deref().unwrap_or(""));
        out.string("toolbarButtons", &settings.toolbar_buttons.join(","));
        out.bool("centredAddressBar", settings.centred_address_bar);
        out.bool("minimalUrl", settings.minimal_url);
        out.bool("showLoadProgress", settings.show_load_progress);
        out.bool("frameless", settings.frameless);
        out.string("autoHideBars", &settings.auto_hide.bars.text()); // --- auto-hide (T11) ---
        out.bool("autoHideFullscreen", settings.auto_hide.in_fullscreen);
        out.bool("roundedContentFrame", settings.rounded_content_frame);
        out.bool("showHomeButton", settings.show_home_button);
        out.string("bookmarksBar", settings.bookmarks_bar.key());
        out.bool("bookmarksBarTabGroups", settings.bookmarks_bar_tab_groups);
        out.string("sidePanelPosition", settings.side_panel_position.key());
        out.bool("tabPreviewImages", settings.tab_preview_images);
        out.bool("tabPreviewMemory", settings.tab_preview_memory);
        out.string("fontSize", settings.font_size.key());
        out.string("uiFont", &settings.ui_font);
        out.uint("defaultZoom", u64::from(settings.default_zoom));
        out.bool("showZoomIndicator", settings.show_zoom_indicator);
        out.bool("newTabBesideActive", settings.new_tab_beside_active);
        out.bool("ctrlTabMru", settings.ctrl_tab_mru);
        out.bool("shiftRightClickContext", settings.shift_right_click_context);
        out.bool("ctrlShiftCCopyLink", settings.ctrl_shift_c_copy_link);
        out.bool("ctrlSVerticalTabs", settings.ctrl_s_vertical_tabs);
        out.bool("pinNewTabGroups", settings.pin_new_tab_groups);
        out.bool("splitEdgeDrop", settings.split_edge_drop);
        out.bool("blockYoutubeVideoAds", settings.block_youtube_video_ads);
        // --- tab options ---
        out.object("tabOptions", |options| {
            for (key, value) in crate::tab_options::snapshot_entries(settings) {
                match value {
                    crate::tab_options::Value::Bool(on) => options.bool(key, on),
                    crate::tab_options::Value::Text(text) => options.string(key, &text),
                    crate::tab_options::Value::Uint(n) => options.uint(key, n),
                }
            }
        });
        // --- end tab options ---
        // --- theme options ---
        out.object("themeOptions", |options| {
            for (key, value) in settings.themes.entries() {
                match value {
                    crate::tab_options::Value::Bool(on) => options.bool(key, on),
                    crate::tab_options::Value::Text(text) => options.string(key, &text),
                    crate::tab_options::Value::Uint(n) => options.uint(key, n),
                }
            }
        });
        // --- end theme options ---
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
        out.string("defaultBrowser", s.default_browser);
        out.string("downloadDir", capped(s.download_dir, MAX_DOWNLOAD_DIR_CHARS));
        out.bool("downloadAsk", s.download_ask);
        out.bool("translateOffer", settings.translate_offer);
        out.bool("translateAuto", settings.translate_auto);
        out.string("translateTarget", &settings.translate_target);
        out.array("translateTargets", |items| for code in crate::translate::target_languages() { items.string(code); });
        // --- start page --- one object, so the strict key list of the snapshot grows by one
        out.object("start", |start| start_snapshot(&settings.start, start));
        // --- end start page ---
        // --- tab stacks, tiling and workspaces (T10) --- the workspace rules editor's list.
        out.object("workspaceRules", |rules| {
            rules.array("rules", |items| {
                for (site, workspace) in s.workspace_rules.iter().take(crate::workspace_rules::MAX_RULES) {
                    items.object(|rule| {
                        rule.string("site", site);
                        rule.string("workspace", capped(workspace, 128));
                    });
                }
            });
            rules.array("workspaces", |items| for name in s.workspace_names.iter().take(128) { items.string(capped(name, 128)); });
        });
        // --- end tab stacks ---
        // --- layout (thread T6) --- one object: the Layout block's options (src/layout_prefs.rs)
        out.object("layout", |layout| {
            layout.bool("tabBarVisible", settings.tab_bar_visible);
            layout.string("addressBarPosition", settings.address_bar_position.key());
            layout.bool("addressBarVisible", settings.address_bar_visible);
            layout.bool("showTitleBar", settings.show_title_bar);
            layout.string("statusBar", settings.status_bar.key());
            layout.string("uiDensity", settings.ui_density.key());
            layout.bool("compactMenus", settings.compact_menus);
            layout.uint("uiZoom", u64::from(settings.ui_zoom));
            layout.string("saved", &settings.layout_saved);
        });
        // --- end layout ---
        // --- window and panels (thread T9) --- one object of booleans (src/window_prefs.rs)
        out.object("window", |window| {
            for (key, value) in settings.window.pairs() {
                window.bool(&snake_to_camel(key), value == "true");
            }
        });
        // --- end window and panels ---
    })
    .expect("settings is a panel token")
}

// --- start page ---
/// The `start` object of the snapshot: every start-page and startup setting (docs/design/start-page.md).
fn start_snapshot(start: &crate::start_page::StartSettings, out: &mut super::json::Object<'_>) {
    out.string("groupReopen", start.group_reopen.key());
    out.string("titles", start.titles.key());
    out.bool("addButton", start.add_button);
    out.uint("columns", u64::from(start.columns));
    out.uint("thumbSize", u64::from(start.thumb_size));
    out.bool("dragReorder", start.drag_reorder);
    out.string("homepage", &start.homepage);
    out.string("startup", start.startup.key());
    out.array("startupPages", |items| for page in &start.startup_pages { items.string(page); });
    out.bool("widgets", start.widgets);
    out.uint("widgetColumns", u64::from(start.widget_columns));
    out.string("widgetHeight", start.widget_height.key());
    out.string("widgetBg", start.widget_bg.key());
    out.string("widgetColor", &start.widget_color);
    out.bool("widgetDrag", start.widget_drag);
}
// --- end start page ---

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
    /// "Make Bowser the default browser": registers Bowser under HKCU, then asks Windows to show
    /// its own defaults UI. Any failure comes back through `ack`/`error`.
    SetDefault,
    /// "Ask where to save each file": the profile's switch, applied at once.
    DownloadAsk(bool),
    /// "Change" the download folder: Bowser shows the native folder picker and keeps the choice.
    DownloadFolder,
    /// "Offer to translate pages in other languages": applied at once.
    TranslateOffer(bool),
    /// "Translate automatically on sites set to always translate": applied at once.
    TranslateAuto(bool),
    /// The language pages are translated into: empty (the Windows display language) or one of
    /// [`crate::translate::target_languages`]. Anything else is refused here.
    TranslateTarget(String),
    // --- tab options ---
    /// One `key=value` change of the Tabs section, checked by `tab_options::apply_assignment`.
    TabOption(String),
    // --- end tab options ---
    // --- theme options ---
    /// One `key=value` change of the theme options, checked by `theme_custom::apply_assignment`.
    ThemeOption(String),
    /// "Export theme": Bowser shows the native save dialog and writes the theme file.
    ThemeExport,
    /// "Import theme": Bowser shows the native open dialog and checks the file it reads.
    ThemeImport,
    /// "Reset theme settings": the theme, accent and every theme option back to the defaults.
    ThemeReset,
    // --- end theme options ---
    // --- tab stacks (T10) ---
    /// "Workspace rules": `site` (a plain host) opens in the workspace named `workspace`.
    WorkspaceRuleAdd { site: String, workspace: String },
    /// Removes the rule for `site`.
    WorkspaceRuleRemove(String),
    // --- end tab stacks ---
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
        "downloadask" if on_only => Action::DownloadAsk(f.on?),
        "translateoffer" if on_only => Action::TranslateOffer(f.on?),
        "translateauto" if on_only => Action::TranslateAuto(f.on?),
        "translatetarget" if text_only => {
            let text = f.text.as_deref()?;
            (text.is_empty() || crate::translate::target_languages().contains(&text)).then(|| Action::TranslateTarget(text.to_owned()))?
        }
        // --- tab options ---
        "tabopt" if text_only => {
            let text = f.text.as_deref()?;
            let mut probe = Settings::default();
            if text.len() > MAX_TAB_OPTION_TEXT || !crate::tab_options::apply_assignment(&mut probe, text) { return None; }
            Action::TabOption(text.to_owned())
        }
        // --- end tab options ---
        // --- theme options ---
        "themeopt" if text_only => {
            let text = f.text.as_deref()?;
            if text.len() > MAX_THEME_OPTION_TEXT || !crate::theme_custom::accepts(text) { return None; }
            Action::ThemeOption(text.to_owned())
        }
        "themeexport" if bare => Action::ThemeExport,
        "themeimport" if bare => Action::ThemeImport,
        "themereset" if bare => Action::ThemeReset,
        // --- end theme options ---
        // --- tab stacks (T10) --- `add<TAB>site<TAB>workspace` or `del<TAB>site`.
        "wsrule" if text_only => {
            let text = f.text.as_deref()?;
            if text.len() > MAX_RULE_TEXT { return None; }
            let mut parts = text.split('\t');
            match (parts.next()?, parts.next(), parts.next(), parts.next()) {
                ("add", Some(site), Some(workspace), None) => {
                    let site = crate::workspace_rules::normalise_site(site)?;
                    let workspace = workspace.trim();
                    if workspace.is_empty() || workspace.chars().any(char::is_control) { return None; }
                    Action::WorkspaceRuleAdd { site, workspace: workspace.to_owned() }
                }
                ("del", Some(site), None, None) => Action::WorkspaceRuleRemove(crate::workspace_rules::normalise_site(site)?),
                _ => return None,
            }
        }
        // --- end tab stacks ---
        "reset" if bare => Action::Reset,
        "openfile" if bare => Action::OpenFile,
        "checkupdates" if bare => Action::CheckUpdates,
        "restartupdate" if bare => Action::RestartUpdate,
        "privacy" if bare => Action::Privacy,
        "profiles" if bare => Action::Profiles,
        "import" if bare => Action::Import,
        "refresh" if bare => Action::Refresh,
        "setdefault" if bare => Action::SetDefault,
        "downloadfolder" if bare => Action::DownloadFolder,
        _ => return None,
    })
}

/// --- wave-1 fixes --- [`apply_settings_text`] for the page's Save over the file's `settings`. The
/// blocking level is saved by its own control (the protection commands, the Privacy panel), so the
/// page's copy of `blocker_enabled` is only a mirror that can be older than the file: the file's
/// value is kept, and a level chosen elsewhere while the page is open is never put back.
pub fn apply_page_save(settings: &mut Settings, text: &str) -> bool {
    let blocker_enabled = settings.blocker_enabled;
    let applied = apply_settings_text(settings, text);
    settings.blocker_enabled = blocker_enabled;
    applied
}

/// Applies a `save` payload: every profile setting exactly once, exact keys and values, nothing
/// else (the update switch is install-wide and never part of it). `false` leaves the caller's copy
/// to be discarded.
/// `range_buttons` -> `rangeButtons` (the snapshot's key style).
fn snake_to_camel(key: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in key.chars() {
        if c == '_' { up = true; } else if up { out.push(c.to_ascii_uppercase()); up = false; } else { out.push(c); }
    }
    out
}

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
            "theme" => { let Some(value) = Theme::parse(value) else { return false; }; settings.theme = value; }
            "theme_accent" => { let Some(value) = crate::settings::parse_theme_accent(value) else { return false; }; settings.theme_accent = value; }
            "toolbar_buttons" => { let Some(value) = crate::settings::parse_toolbar_buttons(value) else { return false; }; settings.toolbar_buttons = value; }
            "centred_address_bar" => settings.centred_address_bar = match value { "true" => true, "false" => false, _ => return false },
            "minimal_url" => settings.minimal_url = match value { "true" => true, "false" => false, _ => return false },
            "show_load_progress" => settings.show_load_progress = match value { "true" => true, "false" => false, _ => return false },
            "frameless" => settings.frameless = match value { "true" => true, "false" => false, _ => return false },
            "rounded_content_frame" => settings.rounded_content_frame = match value { "true" => true, "false" => false, _ => return false },
            "show_home_button" => settings.show_home_button = match value { "true" => true, "false" => false, _ => return false },
            "bookmarks_bar" => { let Some(value) = BookmarksBar::parse(value) else { return false; }; settings.bookmarks_bar = value; }
            "bookmarks_bar_tab_groups" => settings.bookmarks_bar_tab_groups = match value { "true" => true, "false" => false, _ => return false },
            "side_panel_position" => { let Some(value) = SidePanelPosition::parse(value) else { return false; }; settings.side_panel_position = value; }
            "tab_preview_images" => settings.tab_preview_images = match value { "true" => true, "false" => false, _ => return false },
            "tab_preview_memory" => settings.tab_preview_memory = match value { "true" => true, "false" => false, _ => return false },
            "font_size" => { let Some(value) = FontSize::parse(value) else { return false; }; settings.font_size = value; }
            "ui_font" => { let Some(value) = crate::settings::parse_ui_font(value) else { return false; }; settings.ui_font = value; }
            "default_zoom" => { let Ok(value) = value.parse::<u16>() else { return false; }; if !(crate::settings::MIN_DEFAULT_ZOOM..=crate::settings::MAX_DEFAULT_ZOOM).contains(&value) { return false; } settings.default_zoom = value; }
            "show_zoom_indicator" => settings.show_zoom_indicator = match value { "true" => true, "false" => false, _ => return false },
            "new_tab_beside_active" => settings.new_tab_beside_active = match value { "true" => true, "false" => false, _ => return false },
            "ctrl_tab_mru" => settings.ctrl_tab_mru = match value { "true" => true, "false" => false, _ => return false },
            "shift_right_click_context" => settings.shift_right_click_context = match value { "true" => true, "false" => false, _ => return false },
            "ctrl_shift_c_copy_link" => settings.ctrl_shift_c_copy_link = match value { "true" => true, "false" => false, _ => return false },
            "ctrl_s_vertical_tabs" => settings.ctrl_s_vertical_tabs = match value { "true" => true, "false" => false, _ => return false },
            "pin_new_tab_groups" => settings.pin_new_tab_groups = match value { "true" => true, "false" => false, _ => return false },
            "split_edge_drop" => settings.split_edge_drop = match value { "true" => true, "false" => false, _ => return false },
            "block_youtube_video_ads" => settings.block_youtube_video_ads = match value { "true" => true, "false" => false, _ => return false },
            // --- auto-hide (T11) --- optional, like the start page's: a payload without them keeps them
            key if crate::auto_hide::SETTING_KEYS.contains(&key) => { if settings.auto_hide.apply_strict(key, value).is_none() { return false; } }
            // --- end auto-hide ---
            // --- start page --- optional: a payload without them leaves the start page settings as they are
            key if crate::start_page::SETTING_KEYS.contains(&key) => { if settings.start.apply_strict(key, value).is_none() { return false; } }
            // --- end start page ---
            // --- layout (thread T6) --- optional too: a payload without them leaves the layout options as they are
            "tab_bar_visible" => settings.tab_bar_visible = match value { "true" => true, "false" => false, _ => return false },
            "address_bar_position" => { let Some(value) = crate::layout_prefs::BarPosition::ALL.into_iter().find(|p| p.key() == value) else { return false; }; settings.address_bar_position = value; }
            "address_bar_visible" => settings.address_bar_visible = match value { "true" => true, "false" => false, _ => return false },
            "show_title_bar" => settings.show_title_bar = match value { "true" => true, "false" => false, _ => return false },
            "status_bar" => { let Some(value) = crate::layout_prefs::StatusBar::ALL.into_iter().find(|s| s.key() == value) else { return false; }; settings.status_bar = value; }
            "ui_density" => { let Some(value) = crate::layout_prefs::UiDensity::ALL.into_iter().find(|d| d.key() == value) else { return false; }; settings.ui_density = value; }
            "compact_menus" => settings.compact_menus = match value { "true" => true, "false" => false, _ => return false },
            "ui_zoom" => { let Ok(value) = value.parse::<u16>() else { return false; }; if !(crate::layout_prefs::MIN_UI_ZOOM..=crate::layout_prefs::MAX_UI_ZOOM).contains(&value) { return false; } settings.ui_zoom = value; }
            "layout_saved" => { if !crate::layout_prefs::is_layout_saved(value) { return false; } settings.layout_saved = value.to_owned(); }
            // --- end layout ---
            // --- window and panels (thread T9) ---
            key if crate::window_prefs::KEYS.contains(&key) => { if settings.window.apply_strict(key, value).is_none() { return false; } }
            // --- end window and panels ---
            _ => return false,
        }
    }
    [
        "tab_layout", "sidebar_width", "sleep_enabled", "sleep_after_secs", "blocker_enabled", "blocker_allow",
        "automatic_tab_grouping", "efficiency_mode", "tracking_prevention", "theme", "theme_accent",
        "toolbar_buttons", "centred_address_bar", "minimal_url", "show_load_progress", "frameless",
        "rounded_content_frame", "show_home_button", "bookmarks_bar", "bookmarks_bar_tab_groups",
        "side_panel_position", "tab_preview_images", "tab_preview_memory", "font_size", "ui_font",
        "default_zoom", "show_zoom_indicator", "new_tab_beside_active", "ctrl_tab_mru",
        "shift_right_click_context", "ctrl_shift_c_copy_link", "ctrl_s_vertical_tabs", "pin_new_tab_groups",
        "split_edge_drop", "block_youtube_video_ads",
    ].iter().all(|key| seen.contains(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(a: &str) -> PanelFields {
        PanelFields { a: a.to_owned(), id: None, parent: None, before: None, n: None, on: None, text: None, url: None }
    }

    // --- wave-1 fixes ---
    /// The level was set to None from the shield or Privacy panel while the page was open: the
    /// page's older draft (blocker_enabled=true) saves another row and must not turn blocking back on.
    #[test]
    fn a_page_save_keeps_the_blocking_level_the_file_holds() {
        let page = "tab_layout=vertical\nsidebar_width=300\nsleep_enabled=false\nsleep_after_secs=900\nblocker_enabled=true\nblocker_allow=\nautomatic_tab_grouping=false\nefficiency_mode=on\ntracking_prevention=strict\n\
            theme=system\ntheme_accent=\ntoolbar_buttons=back,reload\ncentred_address_bar=false\nminimal_url=false\nshow_load_progress=false\n\
            frameless=true\nrounded_content_frame=false\nshow_home_button=false\nbookmarks_bar=always\nbookmarks_bar_tab_groups=true\n\
            side_panel_position=right\ntab_preview_images=false\ntab_preview_memory=false\nfont_size=large\nui_font=\n\
            default_zoom=125\nshow_zoom_indicator=false\nnew_tab_beside_active=false\nctrl_tab_mru=false\nshift_right_click_context=true\n\
            ctrl_shift_c_copy_link=false\nctrl_s_vertical_tabs=true\npin_new_tab_groups=true\nsplit_edge_drop=false\nblock_youtube_video_ads=false";
        let mut off = Settings { blocker_enabled: false, ..Settings::default() };
        assert!(apply_page_save(&mut off, page));
        assert!(!off.blocker_enabled, "None chosen elsewhere stays None");
        assert_eq!(off.theme, Theme::System, "the rest of the save still applies");
        let mut on = Settings::default();
        assert!(apply_page_save(&mut on, &page.replace("blocker_enabled=true", "blocker_enabled=false")));
        assert!(on.blocker_enabled, "a stale None in the draft does not turn blocking off either");
        assert!(!apply_page_save(&mut Settings::default(), "tab_layout=wrong"), "a bad payload is still refused");
    }

    #[test]
    fn settings_payload_updates_only_known_values_and_requires_every_control() {
        let mut settings = Settings::default();
        let valid = "tab_layout=vertical\nsidebar_width=300\nsleep_enabled=false\nsleep_after_secs=900\nblocker_enabled=true\nblocker_allow=example.com\nautomatic_tab_grouping=false\nefficiency_mode=on\ntracking_prevention=strict\n\
            theme=system\ntheme_accent=#1a2b3c\ntoolbar_buttons=back,reload\ncentred_address_bar=false\nminimal_url=false\nshow_load_progress=false\n\
            frameless=true\nrounded_content_frame=false\nshow_home_button=false\nbookmarks_bar=always\nbookmarks_bar_tab_groups=true\n\
            side_panel_position=right\ntab_preview_images=false\ntab_preview_memory=false\nfont_size=large\nui_font=Consolas\n\
            default_zoom=125\nshow_zoom_indicator=false\nnew_tab_beside_active=false\nctrl_tab_mru=false\nshift_right_click_context=true\n\
            ctrl_shift_c_copy_link=false\nctrl_s_vertical_tabs=true\npin_new_tab_groups=true\nsplit_edge_drop=false\nblock_youtube_video_ads=false";
        assert!(apply_settings_text(&mut settings, valid));
        assert_eq!(settings.tab_layout, TabLayout::Vertical);
        assert_eq!(settings.sidebar_width, 300);
        assert!(!settings.sleep_enabled);
        assert!(!settings.automatic_tab_grouping);
        assert_eq!(settings.efficiency_mode, crate::settings::EfficiencyMode::On);
        assert_eq!(settings.tracking_prevention, crate::settings::TrackingLevel::Strict);
        assert_eq!(settings.theme, Theme::System);
        assert_eq!(settings.theme_accent.as_deref(), Some("#1a2b3c"));
        assert_eq!(settings.toolbar_buttons, vec!["back".to_owned(), "reload".to_owned()]);
        assert_eq!(settings.bookmarks_bar, BookmarksBar::Always);
        assert_eq!(settings.side_panel_position, SidePanelPosition::Right);
        assert_eq!(settings.font_size, FontSize::Large);
        assert_eq!(settings.ui_font, "Consolas");
        assert_eq!(settings.default_zoom, 125);
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
        // The download settings have their own commands (`downloadask`, `downloadfolder`): a save
        // neither carries nor changes them, and a path from the page is never accepted.
        assert!(!apply_settings_text(&mut settings, &format!("{valid}\ndownload_dir=C:\\Windows")));
        assert!(!apply_settings_text(&mut settings, &format!("{valid}\nask_download_location=true")));
        assert!(apply_settings_text(&mut settings, valid));
        assert_eq!((settings.download_dir.as_str(), settings.ask_download_location), ("", false));
        assert!(!apply_settings_text(&mut settings, &format!("{valid}\nsearch_engine=bing")));
        assert!(apply_settings_text(&mut settings, &format!("{valid}\nsearch_engine=google")));
        assert!(!apply_settings_text(&mut settings, &"x".repeat(MAX_SAVE_TEXT + 1)));
        // Every new key is required, exact-match, and validated the same way as the file parser.
        assert!(!apply_settings_text(&mut settings, &valid.replace("\ntheme=system", "")), "theme is required");
        assert!(apply_settings_text(&mut settings, &valid.replace("theme=system", "theme=Helium")), "theme folds case like efficiency_mode and tracking_prevention");
        assert_eq!(settings.theme, Theme::Helium);
        assert!(apply_settings_text(&mut settings, &valid.replace("theme_accent=#1a2b3c", "theme_accent=#1A2B3C")), "accent case is folded like other tokens");
        assert_eq!(settings.theme_accent.as_deref(), Some("#1a2b3c"));
        assert!(!apply_settings_text(&mut settings, &valid.replace("theme_accent=#1a2b3c", "theme_accent=red")));
        assert!(!apply_settings_text(&mut settings, &valid.replace("theme_accent=#1a2b3c", "theme_accent=1a2b3c")), "the # is required");
        assert!(apply_settings_text(&mut settings, &valid.replace("theme_accent=#1a2b3c", "theme_accent=")), "empty clears the accent");
        assert_eq!(settings.theme_accent, None);
        assert!(!apply_settings_text(&mut settings, &valid.replace("toolbar_buttons=back,reload", "toolbar_buttons=back,back")), "no duplicates");
        assert!(!apply_settings_text(&mut settings, &valid.replace("toolbar_buttons=back,reload", "toolbar_buttons=back,bogus")));
        assert!(!apply_settings_text(&mut settings, &valid.replace("bookmarks_bar=always", "bookmarks_bar=sometimes")));
        assert!(!apply_settings_text(&mut settings, &valid.replace("side_panel_position=right", "side_panel_position=centre")));
        assert!(!apply_settings_text(&mut settings, &valid.replace("font_size=large", "font_size=huge")));
        assert!(!apply_settings_text(&mut settings, &valid.replace("default_zoom=125", "default_zoom=24")), "below the minimum");
        assert!(!apply_settings_text(&mut settings, &valid.replace("default_zoom=125", "default_zoom=501")), "above the maximum");
        assert!(!apply_settings_text(&mut settings, &valid.replace("frameless=true", "frameless=maybe")));
        assert!(!apply_settings_text(&mut settings, &format!("{valid}\nblock_youtube_video_ads=true")), "duplicate key");
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
        let mut ask = fields("downloadask");
        assert_eq!(map_command(&ask), None, "downloadask needs its switch");
        ask.on = Some(true);
        assert_eq!(map_command(&ask), Some(Action::DownloadAsk(true)));
        ask.on = Some(false);
        assert_eq!(map_command(&ask), Some(Action::DownloadAsk(false)));
        ask.text = Some("C:\\Windows".into());
        assert_eq!(map_command(&ask), None, "downloadask carries no text");
        // The folder is picked by Bowser's own dialog: the command carries nothing, least of all a path.
        let mut folder = fields("downloadfolder");
        assert_eq!(map_command(&folder), Some(Action::DownloadFolder));
        folder.text = Some("C:\\Windows".into());
        assert_eq!(map_command(&folder), None);
        folder.text = None;
        folder.on = Some(true);
        assert_eq!(map_command(&folder), None);
        let mut section = fields("section");
        section.text = Some("downloads".into());
        assert_eq!(map_command(&section), Some(Action::Section(Some(Section::Downloads))));

        for (name, action) in [("reset", Action::Reset), ("openfile", Action::OpenFile), ("checkupdates", Action::CheckUpdates), ("restartupdate", Action::RestartUpdate), ("privacy", Action::Privacy), ("profiles", Action::Profiles), ("import", Action::Import), ("refresh", Action::Refresh), ("setdefault", Action::SetDefault)] {
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

    // --- theme options ---
    #[test]
    fn theme_commands_carry_exactly_their_own_fields_and_only_legal_values() {
        let mut change = fields("themeopt");
        assert_eq!(map_command(&change), None, "a theme option needs its text");
        for good in ["theme_schedule=time", "theme_look=dark", "theme_light_from=06:30", "theme_dark_from=22:00", "theme_background=#102030", "theme_foreground=", "theme_highlight=#FFCC00",
            "theme_colouring=unified", "theme_accent_page=true", "theme_saturation=40", "theme_contrast=high"] {
            change.text = Some(good.into());
            assert_eq!(map_command(&change), Some(Action::ThemeOption(good.into())), "{good}");
        }
        for bad in ["theme_schedule=weekly", "theme_background=red", "theme_background=#12345", "theme_saturation=101", "theme_light_from=25:00", "theme_nothing=1", "tabs_scroll=true", "theme_look", "=", "theme_accent=#112233"] {
            change.text = Some(bad.into());
            assert_eq!(map_command(&change), None, "{bad}");
        }
        change.text = Some(format!("theme_foreground={}", "#".repeat(80)));
        assert_eq!(map_command(&change), None, "an over-long value");
        change.text = Some("theme_look=dark".into());
        change.on = Some(true);
        assert_eq!(map_command(&change), None, "a theme option carries no switch");
        change.on = None;
        change.url = Some("https://example.com/".into());
        assert_eq!(map_command(&change), None);
        for (name, action) in [("themeexport", Action::ThemeExport), ("themeimport", Action::ThemeImport), ("themereset", Action::ThemeReset)] {
            assert_eq!(map_command(&fields(name)), Some(action), "{name}");
            // The file is picked by Bowser's own dialog: the command carries nothing, least of all a path.
            let mut with_text = fields(name);
            with_text.text = Some("C:\\Windows\\win.ini".into());
            assert_eq!(map_command(&with_text), None, "{name} with a path");
            let mut with_on = fields(name);
            with_on.on = Some(true);
            assert_eq!(map_command(&with_on), None, "{name} with on");
        }
    }

    #[test]
    fn the_snapshot_carries_every_theme_option_with_the_values_the_page_reads() {
        let mut settings = Settings::default();
        let json = snapshot_json(&sample(&settings));
        assert!(json.contains(r#""themeOptions":{"theme_schedule":"system","theme_look":"light","theme_light_from":"07:00","theme_dark_from":"19:00","theme_background":"","theme_foreground":"","theme_highlight":"","theme_colouring":"off","theme_accent_page":false,"theme_saturation":100,"theme_contrast":"standard"}"#), "{json}");
        assert!(settings.themes.set("theme_background", "#102030") && settings.themes.set("theme_accent_page", "true") && settings.themes.set("theme_saturation", "35"));
        let json = snapshot_json(&sample(&settings));
        assert!(json.contains(r##""theme_background":"#102030""##) && json.contains(r#""theme_accent_page":true,"theme_saturation":35"#), "{json}");
    }
    // --- end theme options ---

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
            default_browser: "yes",
            download_dir: "C:\\Users\\Work\\Downloads",
            download_ask: false,
            workspace_rules: vec![("example.org".into(), "Work".into())],
            workspace_names: vec!["Default".into(), "Work".into()],
        }
    }

    /// --- tab stacks (T10) --- the workspace rules editor's list and its two commands.
    #[test]
    fn workspace_rules_ride_in_the_snapshot_and_their_commands_carry_a_plain_site() {
        let json = snapshot_json(&sample(&Settings::default()));
        assert!(json.contains(r#""workspaceRules":{"rules":[{"site":"example.org","workspace":"Work"}],"workspaces":["Default","Work"]},"layout":{"#), "{json}");
        let mut rule = fields("wsrule");
        rule.text = Some("add\thttps://www.News.example.com/x\tReading".into());
        assert_eq!(map_command(&rule), Some(Action::WorkspaceRuleAdd { site: "news.example.com".into(), workspace: "Reading".into() }));
        rule.text = Some("del\texample.org".into());
        assert_eq!(map_command(&rule), Some(Action::WorkspaceRuleRemove("example.org".into())));
        for bad in ["add\tnot a site\tWork", "add\texample.org\t", "add\texample.org", "del", "del\texample.org\tWork", "move\texample.org", "add\texample.org\tW\u{1}"] {
            rule.text = Some(bad.into());
            assert_eq!(map_command(&rule), None, "{bad:?}");
        }
        rule.text = None;
        rule.on = Some(true);
        assert_eq!(map_command(&rule), None, "a rule carries text only");
    }

    #[test]
    fn translate_commands_carry_exactly_their_own_field() {
        let mut offer = fields("translateoffer");
        assert_eq!(map_command(&offer), None, "needs its switch");
        offer.on = Some(false);
        assert_eq!(map_command(&offer), Some(Action::TranslateOffer(false)));
        offer.text = Some("x".into());
        assert_eq!(map_command(&offer), None);
        let mut auto = fields("translateauto");
        auto.on = Some(true);
        assert_eq!(map_command(&auto), Some(Action::TranslateAuto(true)));
        let mut target = fields("translatetarget");
        assert_eq!(map_command(&target), None, "needs its text");
        for (text, expected) in [("", Some("")), ("en", Some("en")), ("fr", Some("fr")), ("ja", Some("ja")), ("fil", Some("fil")), ("EN", None), ("en-GB", None), ("xx", None), ("auto", None), ("<b>", None), ("fr ", None)] {
            target.text = Some(text.into());
            assert_eq!(map_command(&target), expected.map(|e| Action::TranslateTarget(e.into())), "{text:?}");
        }
        target.on = Some(true);
        assert_eq!(map_command(&target), None, "a language carries no switch");
    }

    #[test]
    fn the_snapshot_reports_the_translate_settings_and_the_languages_to_pick_from() {
        let mut settings = Settings::default();
        let json = snapshot_json(&sample(&settings));
        assert!(json.contains(r#""translateOffer":true,"translateAuto":false,"translateTarget":"","translateTargets":["en","#), "{json}");
        settings.translate_target = "fr".into();
        settings.translate_auto = true;
        let json = snapshot_json(&sample(&settings));
        assert!(json.contains(r#""translateAuto":true,"translateTarget":"fr""#), "{json}");
        assert!(json.contains(r##""widgetColor":"#1f2933","widgetDrag":true},"workspaceRules":{"##), "{json}");
        // --- layout (thread T6) --- the Layout block's object closes the snapshot.
        assert!(json.contains(r##"]},"layout":{"tabBarVisible":true,"##), "{json}"); // --- T10 --- after the workspace rules
        // --- window and panels (thread T9) --- the Window object closes the snapshot.
        assert!(json.contains(r##""uiZoom":100,"saved":""},"window":{"##), "{json}");
        assert!(json.ends_with(r##""panelAutoClose":true}}}"##), "{json}");
    }

    // --- layout (thread T6) ---
    #[test]
    fn the_snapshot_and_save_carry_the_layout_options_and_refuse_bad_values() {
        let mut settings = Settings::default();
        settings.tab_layout = TabLayout::VerticalRight;
        settings.tab_bar_visible = false;
        settings.address_bar_position = crate::layout_prefs::BarPosition::Bottom;
        settings.show_title_bar = true;
        settings.status_bar = crate::layout_prefs::StatusBar::Hidden;
        settings.ui_density = crate::layout_prefs::UiDensity::Compact;
        settings.compact_menus = true;
        settings.ui_zoom = 150;
        settings.layout_saved = crate::layout_prefs::Preset::Bottom.config().key();
        let json = snapshot_json(&sample(&settings));
        assert!(json.contains(r#""tabLayout":"vertical-right""#), "{json}");
        assert!(json.contains(&format!(r#""layout":{{"tabBarVisible":false,"addressBarPosition":"bottom","addressBarVisible":true,"showTitleBar":true,"statusBar":"hidden","uiDensity":"compact","compactMenus":true,"uiZoom":150,"saved":"{}"}}"#, settings.layout_saved)), "{json}");

        let required = "tab_layout=bottom\nsidebar_width=260\nsleep_enabled=true\nsleep_after_secs=300\nblocker_enabled=true\nblocker_allow=\nautomatic_tab_grouping=true\nefficiency_mode=auto\ntracking_prevention=balanced\n\
            theme=helium\ntheme_accent=\ntoolbar_buttons=back\ncentred_address_bar=true\nminimal_url=true\nshow_load_progress=true\nframeless=false\nrounded_content_frame=true\nshow_home_button=true\n\
            bookmarks_bar=never\nbookmarks_bar_tab_groups=false\nside_panel_position=right\ntab_preview_images=true\ntab_preview_memory=true\nfont_size=medium\nui_font=\ndefault_zoom=100\n\
            show_zoom_indicator=true\nnew_tab_beside_active=true\nctrl_tab_mru=true\nshift_right_click_context=false\nctrl_shift_c_copy_link=true\nctrl_s_vertical_tabs=false\npin_new_tab_groups=false\nsplit_edge_drop=true\nblock_youtube_video_ads=true";
        let layout = "tab_bar_visible=false\naddress_bar_position=bottom\naddress_bar_visible=false\nshow_title_bar=true\nstatus_bar=hidden\nui_density=compact\ncompact_menus=true\nui_zoom=75\nlayout_saved=tabs=single-row,tabbar=true,address=top,addressbar=true,panel=left,status=hidden,title=false";
        let mut applied = Settings::default();
        assert!(apply_settings_text(&mut applied, &format!("{required}\n{layout}")));
        assert_eq!(applied.tab_layout, TabLayout::TabsBottom);
        assert_eq!(applied.side_panel_position, SidePanelPosition::Right);
        assert!(!applied.tab_bar_visible);
        assert_eq!(applied.address_bar_position, crate::layout_prefs::BarPosition::Bottom);
        assert!(!applied.address_bar_visible);
        assert!(applied.show_title_bar);
        assert_eq!(applied.status_bar, crate::layout_prefs::StatusBar::Hidden);
        assert_eq!(applied.ui_density, crate::layout_prefs::UiDensity::Compact);
        assert!(applied.compact_menus);
        assert_eq!(applied.ui_zoom, 75);
        assert_eq!(crate::layout_prefs::LayoutConfig::parse(&applied.layout_saved).map(|c| c.tab_layout), Some(TabLayout::SingleRow));
        // Every new tab layout key is accepted; the two new ones are nothing else.
        for key in ["vertical-right", "bottom"] {
            assert!(apply_settings_text(&mut applied, &required.replace("tab_layout=bottom", &format!("tab_layout={key}"))), "{key}");
        }
        // Without the layout lines (another thread's payload) the options stay as they were.
        let mut kept = Settings::default();
        kept.ui_zoom = 125;
        assert!(apply_settings_text(&mut kept, required));
        assert_eq!(kept.ui_zoom, 125);
        // A value that does not parse refuses the whole payload.
        for bad in ["tab_bar_visible=yes", "address_bar_position=middle", "status_bar=show", "ui_density=dense", "compact_menus=1", "ui_zoom=49", "ui_zoom=201", "ui_zoom=100%", "layout_saved=tabs=sideways"] {
            assert!(!apply_settings_text(&mut Settings::default(), &format!("{required}\n{bad}")), "{bad}");
        }
    }
    // --- end layout ---

    #[test]
    fn the_snapshot_and_save_carry_the_start_page_settings_and_old_payloads_still_apply() {
        let mut settings = Settings::default();
        settings.start.columns = 9;
        settings.start.startup = crate::start_page::Startup::Homepage;
        settings.start.homepage = "https://home.example/".into();
        settings.start.startup_pages = vec!["https://a.example/".into()];
        let json = snapshot_json(&sample(&settings));
        assert!(json.contains(r#""start":{"groupReopen":"last","titles":"always","addButton":true,"columns":9,"thumbSize":3,"dragReorder":true,"homepage":"https://home.example/","startup":"homepage","startupPages":["https://a.example/"],"widgets":false,"#), "{json}");

        // The page's save text carries the start keys; Rust applies them.
        let required = "tab_layout=horizontal
sidebar_width=260
sleep_enabled=true
sleep_after_secs=300
blocker_enabled=true
blocker_allow=
automatic_tab_grouping=true
efficiency_mode=auto
tracking_prevention=balanced
theme=helium
theme_accent=
toolbar_buttons=back,forward,reload,site-info,bookmark,newtab,split,reader
centred_address_bar=true
minimal_url=true
show_load_progress=true
frameless=false
rounded_content_frame=true
show_home_button=true
bookmarks_bar=never
bookmarks_bar_tab_groups=false
side_panel_position=left
tab_preview_images=true
tab_preview_memory=true
font_size=medium
ui_font=
default_zoom=100
show_zoom_indicator=true
new_tab_beside_active=true
ctrl_tab_mru=true
shift_right_click_context=false
ctrl_shift_c_copy_link=true
ctrl_s_vertical_tabs=false
pin_new_tab_groups=false
split_edge_drop=true
block_youtube_video_ads=true".to_owned();
        let tuned = format!("{required}\nstart_columns=3\nstart_titles=never\nhomepage=https://x.example/\nstartup=last_session\nstartup_pages=\nstart_widgets=true\nstart_widget_columns=4");
        let mut applied = Settings::default();
        assert!(apply_settings_text(&mut applied, &tuned), "{tuned}");
        assert_eq!(applied.start.columns, 3);
        assert_eq!(applied.start.titles, crate::start_page::TitleMode::Never);
        assert_eq!(applied.start.homepage, "https://x.example/");
        assert_eq!(applied.start.startup, crate::start_page::Startup::LastSession);
        assert!(applied.start.widgets);
        assert_eq!(applied.start.widget_columns, 4);
        // Without the start keys (an older page, another thread's test payload) the settings stay as they were.
        let mut before = Settings::default();
        before.start.columns = 7;
        assert!(apply_settings_text(&mut before, &required));
        assert_eq!(before.start.columns, 7);
        // A value that does not parse refuses the whole payload.
        for bad in ["start_columns=13", "start_columns=0", "start_titles=Never", "homepage=ftp://x.example/", "startup=restore", "startup_pages=https://a.example/ nope", "start_widget_color=red", "start_widgets=yes"] {
            let mut target = Settings::default();
            assert!(!apply_settings_text(&mut target, &format!("{required}\n{bad}")), "{bad}");
            assert_eq!(target.start, crate::start_page::StartSettings::default(), "a refused payload changes nothing: {bad}");
        }
        // And a repeated key is refused like any other.
        assert!(!apply_settings_text(&mut Settings::default(), &format!("{required}\nstart_columns=3\nstart_columns=4")));
    }

    // --- auto-hide (T11) --- UI -> Rust save -> snapshot -> file, for the master switch and its two rows.
    #[test]
    fn the_auto_hide_rows_save_snapshot_and_reach_the_file() {
        let required = "tab_layout=horizontal\nsidebar_width=260\nsleep_enabled=true\nsleep_after_secs=300\nblocker_enabled=true\nblocker_allow=\nautomatic_tab_grouping=true\nefficiency_mode=auto\ntracking_prevention=balanced\ntheme=helium\ntheme_accent=\ntoolbar_buttons=back,forward,reload,site-info,bookmark,newtab,split,reader\ncentred_address_bar=true\nminimal_url=true\nshow_load_progress=true\nframeless=false\nrounded_content_frame=true\nshow_home_button=true\nbookmarks_bar=never\nbookmarks_bar_tab_groups=false\nside_panel_position=left\ntab_preview_images=true\ntab_preview_memory=true\nfont_size=medium\nui_font=\ndefault_zoom=100\nshow_zoom_indicator=true\nnew_tab_beside_active=true\nctrl_tab_mru=true\nshift_right_click_context=false\nctrl_shift_c_copy_link=true\nctrl_s_vertical_tabs=false\npin_new_tab_groups=false\nsplit_edge_drop=true\nblock_youtube_video_ads=true";
        // The snapshot carries the master switch (the old frameless key) and both rows.
        let json = snapshot_json(&sample(&Settings::default()));
        assert!(json.contains(r#""frameless":false,"autoHideBars":"tabs,address,bookmarks","autoHideFullscreen":true,"#), "{json}");
        // A save with them applies them; the master switch rides on the old key.
        let payload = format!("{}\nauto_hide_bars=tabs,panel\nauto_hide_fullscreen=false", required.replace("frameless=false", "frameless=true"));
        let mut applied = Settings::default();
        assert!(apply_settings_text(&mut applied, &payload), "{payload}");
        assert!(applied.frameless);
        assert_eq!(applied.auto_hide.bars, crate::auto_hide::Bars::TABS.union(crate::auto_hide::Bars::PANEL));
        assert!(!applied.auto_hide.in_fullscreen);
        let again = snapshot_json(&sample(&applied));
        assert!(again.contains(r#""frameless":true,"autoHideBars":"tabs,panel","autoHideFullscreen":false,"#), "{again}");
        // They are optional (older pages): a payload without them leaves them as they were.
        let mut before = Settings::default();
        before.auto_hide.bars = crate::auto_hide::Bars::PANEL;
        assert!(apply_settings_text(&mut before, required));
        assert_eq!(before.auto_hide.bars, crate::auto_hide::Bars::PANEL);
        // A value Rust would not write refuses the whole payload and changes nothing.
        for bad in ["auto_hide_bars=address,tabs", "auto_hide_bars=tabs,tabs", "auto_hide_bars=status", "auto_hide_bars=tabs,", "auto_hide_fullscreen=yes", "auto_hide_fullscreen=1"] {
            let mut target = Settings::default();
            assert!(!apply_settings_text(&mut target, &format!("{required}\n{bad}")), "{bad}");
            assert_eq!(target.auto_hide, crate::auto_hide::AutoHideSettings::default(), "{bad}");
        }
        assert!(!apply_settings_text(&mut Settings::default(), &format!("{required}\nauto_hide_bars=tabs\nauto_hide_bars=panel")), "a repeated key");
        // And the file: saved, read back, and the page's other lines left alone.
        let dir = std::env::temp_dir().join(format!("bowser-auto-hide-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.txt");
        crate::settings::save(&path, &applied).unwrap();
        let back = Settings::load(&path);
        assert!(back.frameless);
        assert_eq!(back.auto_hide, applied.auto_hide);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("auto_hide_bars=tabs,panel") && text.contains("auto_hide_fullscreen=false") && text.contains("frameless=true"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
    // --- end auto-hide ---

    #[test]
    fn the_snapshot_reports_every_field_and_guests_never_record_history() {
        let mut settings = Settings::default();
        settings.blocker_allow = vec!["example.com".into()];
        let json = snapshot_json(&sample(&settings));
        assert!(json.starts_with(r#"{"v":1,"t":"snap","p":"settings","d":{"tabLayout":"horizontal","sidebarWidth":260,"#), "{json}");
        assert!(json.contains(r#""searchEngine":null,"searchFallback":"default""#), "{json}");
        assert!(json.contains(r#""profile":"Work \"\u003cb>\"""#), "{json}");
        assert!(json.contains(r#""ack":"saved","error":"","readable":true,"defaultBrowser":"yes","downloadDir":"C:\\Users\\Work\\Downloads","downloadAsk":false,"translateOffer":true,"#), "{json}");
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
        let guest = snapshot_json(&Snapshot { guest: true, profile: "Guest", avatar: "G", update: "unavailable", update_version: "", last_check: "none", check_result: "none", ack: Ack::None, custom_search: true, download_dir: "", download_ask: true, ..sample(&guest_settings) });
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
