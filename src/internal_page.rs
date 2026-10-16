//! Bowser's internal pages: `bowser://settings` and its sections (docs/design/settings-page.md),
//! `bowser://history` (docs/design/history-page.md) and `bowser://downloads`
//! (docs/design/downloads.md).
//!
//! An internal page is a Bowser route, not a web address. A tab showing one has no content
//! WebView2: Bowser draws the page in its own trusted chrome host (`src/win32/settings_page.rs`,
//! `src/win32/history_page.rs`, `src/win32/downloads_page.rs`).
//! This module is the pure part: parsing typed text and stored addresses, the canonical form, and
//! which addresses the session may keep. Nothing here touches a window or the engine.

use url::Url;

/// The scheme every internal page uses. The content engine never navigates it (tab navigation is
/// http/https only), so only Bowser's own entry points can put a tab on one of these addresses.
pub const SCHEME: &str = "bowser";
/// The settings page's host.
pub const SETTINGS_HOST: &str = "settings";
/// Title of a settings tab (strip, window title, session).
pub const SETTINGS_TITLE: &str = "Settings";
/// The settings gear as a 32px PNG favicon (icons/settings-favicon.png, drawn from
/// icons/menu/gear.svg in a slate that reads on dark and light strips).
pub const SETTINGS_FAVICON: &[u8] = include_bytes!("../icons/settings-favicon.png");
/// The history page's host (`bowser://history`, no sections).
pub const HISTORY_HOST: &str = "history";
/// Title of a history tab.
pub const HISTORY_TITLE: &str = "History";
/// The history clock as a 32px PNG favicon (icons/history-favicon.png, drawn from
/// icons/rail/clock.svg in the settings gear's slate).
pub const HISTORY_FAVICON: &[u8] = include_bytes!("../icons/history-favicon.png");
/// The downloads page's host (`bowser://downloads`, no sections).
pub const DOWNLOADS_HOST: &str = "downloads";
/// Title of a downloads tab.
pub const DOWNLOADS_TITLE: &str = "Downloads";
/// The download arrow as a 32px PNG favicon (icons/downloads-favicon.png, drawn from
/// icons/extra/download.svg in the settings gear's slate and the history clock's weight).
pub const DOWNLOADS_FAVICON: &[u8] = include_bytes!("../icons/downloads-favicon.png");

/// A section of the settings page, in the order the page lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Profile,
    Appearance,
    /// The start page, speed dials, homepage and startup (docs/design/start-page.md).
    StartPage,
    Privacy,
    Performance,
    Search,
    Keyboard,
    Commands,
    Downloads,
    Media,
    Extensions,
    Updates,
    About,
}

impl Section {
    pub const ALL: [Self; 13] = [
        Self::Profile,
        Self::Appearance,
        Self::StartPage,
        Self::Privacy,
        Self::Performance,
        Self::Search,
        Self::Keyboard,
        Self::Commands,
        Self::Downloads,
        Self::Media,
        Self::Extensions,
        Self::Updates,
        Self::About,
    ];

    /// The path segment and wire key (`bowser://settings/<key>`, `settingsgo.s`).
    pub fn key(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Appearance => "appearance",
            Self::StartPage => "startpage",
            Self::Privacy => "privacy",
            Self::Performance => "performance",
            Self::Search => "search",
            Self::Keyboard => "keyboard",
            Self::Commands => "commands",
            Self::Downloads => "downloads",
            Self::Media => "media",
            Self::Extensions => "extensions",
            Self::Updates => "updates",
            Self::About => "about",
        }
    }

    /// Exact (lower-case) key only: what the page sends back is never case-folded.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|section| section.key() == key)
    }
}

/// An internal page. `Settings(None)` is the top of the settings page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InternalPage {
    Settings(Option<Section>),
    History,
    Downloads,
}

impl InternalPage {
    /// The canonical address, as the omnibox shows it and the session stores it.
    pub fn url(self) -> Url {
        let text = match self {
            Self::Settings(None) => format!("{SCHEME}://{SETTINGS_HOST}"),
            Self::Settings(Some(section)) => format!("{SCHEME}://{SETTINGS_HOST}/{}", section.key()),
            Self::History => format!("{SCHEME}://{HISTORY_HOST}"),
            Self::Downloads => format!("{SCHEME}://{DOWNLOADS_HOST}"),
        };
        Url::parse(&text).expect("canonical internal URL parses")
    }

    /// The settings section (`None` for the top of settings and for every other page).
    pub fn section(self) -> Option<Section> {
        match self {
            Self::Settings(section) => section,
            Self::History | Self::Downloads => None,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Settings(_) => SETTINGS_TITLE,
            Self::History => HISTORY_TITLE,
            Self::Downloads => DOWNLOADS_TITLE,
        }
    }

    /// The tab's favicon (a 32px PNG every favicon surface accepts).
    pub fn favicon(self) -> &'static [u8] {
        match self {
            Self::Settings(_) => SETTINGS_FAVICON,
            Self::History => HISTORY_FAVICON,
            Self::Downloads => DOWNLOADS_FAVICON,
        }
    }

    /// The same page, whatever the section: a tab showing it is reused instead of a second one.
    pub fn same_page(self, other: Self) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }

    pub fn is_settings(self) -> bool {
        matches!(self, Self::Settings(_))
    }

    pub fn is_history(self) -> bool {
        matches!(self, Self::History)
    }

    pub fn is_downloads(self) -> bool {
        matches!(self, Self::Downloads)
    }
}

/// Whether `url` uses the internal scheme at all (handled or not). Such an address must never be
/// sent to the content engine.
pub fn is_internal(url: &Url) -> bool {
    url.scheme().eq_ignore_ascii_case(SCHEME)
}

/// The page an internal address names. Scheme, host and section are case-insensitive, one
/// trailing slash is allowed, query and fragment are ignored, and an unknown settings section is
/// the top of the settings page. History and downloads have no sections: any path is not a page.
/// User info, a port, a deeper path or any other host are not a page.
pub fn parse(url: &Url) -> Option<InternalPage> {
    if !is_internal(url) || !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return None;
    }
    let host = url.host_str()?;
    let settings = host.eq_ignore_ascii_case(SETTINGS_HOST);
    let history = host.eq_ignore_ascii_case(HISTORY_HOST);
    if !settings && !history && !host.eq_ignore_ascii_case(DOWNLOADS_HOST) {
        return None;
    }
    let path = url.path();
    let path = path.strip_prefix('/').unwrap_or(path);
    let path = path.strip_suffix('/').unwrap_or(path);
    if path.contains('/') {
        return None;
    }
    if !settings {
        return path.is_empty().then_some(if history { InternalPage::History } else { InternalPage::Downloads });
    }
    let section = Section::from_key(&path.to_ascii_lowercase());
    Some(InternalPage::Settings(section))
}

/// The page typed (or pasted) omnibox text names, or `None` when the text is not an internal
/// address at all. Only the full `bowser://` form counts; surrounding whitespace is ignored.
pub fn parse_input(text: &str) -> Option<InternalPage> {
    let text = text.trim();
    if text.len() > 256 || text.chars().any(char::is_control) {
        return None;
    }
    let (scheme, rest) = text.split_once("://")?;
    if !scheme.eq_ignore_ascii_case(SCHEME) || rest.is_empty() {
        return None;
    }
    parse(&Url::parse(text).ok()?)
}

/// Whether the session may store `url`: exactly a canonical settings, history or downloads address, nothing
/// else on the internal scheme (so a hand-edited session file cannot smuggle in an odd form).
pub fn persistable(url: &Url) -> bool {
    parse(url).is_some_and(|page| page.url().as_str() == url.as_str())
}

/// Where an opened page goes: an existing tab showing that page (selected, and for settings moved
/// to the section), the current tab when it is a blank normal tab and the caller allows it, or a
/// new tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenPlan<T> {
    Existing(T),
    Current(T),
    NewTab,
}

/// Pure decision for [`OpenPlan`]: `tabs` are `(id, shows_this_page)` in strip order (a tab on
/// another internal page is `false`, see [`InternalPage::same_page`]), `current` is
/// `(id, blank, private)` for the shown tab.
pub fn open_plan<T: Copy + PartialEq>(tabs: &[(T, bool)], current: (T, bool, bool), reuse_blank: bool) -> OpenPlan<T> {
    let (current_id, blank, private) = current;
    if tabs.iter().any(|(id, settings)| *settings && *id == current_id) {
        return OpenPlan::Existing(current_id);
    }
    if let Some((id, _)) = tabs.iter().find(|(_, settings)| *settings) {
        return OpenPlan::Existing(*id);
    }
    if reuse_blank && blank && !private {
        return OpenPlan::Current(current_id);
    }
    OpenPlan::NewTab
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn canonical_urls_round_trip() {
        assert_eq!(InternalPage::Settings(None).url().as_str(), "bowser://settings");
        for section in Section::ALL {
            let page = InternalPage::Settings(Some(section));
            assert_eq!(page.url().as_str(), format!("bowser://settings/{}", section.key()));
            assert_eq!(parse(&page.url()), Some(page));
            assert_eq!(Section::from_key(section.key()), Some(section));
        }
        assert_eq!(Section::ALL.len(), 13);
        assert_eq!(parse_input("bowser://settings/downloads"), Some(InternalPage::Settings(Some(Section::Downloads))));
    }

    #[test]
    fn typed_settings_addresses_open_the_page() {
        let top = Some(InternalPage::Settings(None));
        for text in ["bowser://settings", "bowser://settings/", " BOWSER://Settings ", "bowser://settings?x=1", "bowser://settings#top"] {
            assert_eq!(parse_input(text), top, "{text}");
        }
        assert_eq!(parse_input("bowser://settings/appearance"), Some(InternalPage::Settings(Some(Section::Appearance))));
        assert_eq!(parse_input("bowser://settings/Search/"), Some(InternalPage::Settings(Some(Section::Search))));
        assert_eq!(parse_input("bowser://settings/privacy#x"), Some(InternalPage::Settings(Some(Section::Privacy))));
        assert_eq!(parse_input("bowser://settings/updates"), Some(InternalPage::Settings(Some(Section::Updates))));
        assert_eq!(parse_input("bowser://settings/about"), Some(InternalPage::Settings(Some(Section::About))));
        // An unknown section is the top of the page, not an error.
        assert_eq!(parse_input("bowser://settings/nonsense"), top);
    }

    #[test]
    fn other_text_is_not_an_internal_page() {
        for text in [
            "settings",
            "bowser:settings",
            "bowser://",
            "bowser://newtab",
            "bowser://settings.evil.example",
            "bowser://user@settings",
            "bowser://settings:8080",
            "bowser://settings/privacy/extra",
            "https://settings",
            "https://example.com/bowser://settings",
            "bowser-ui://app/index.html?view=settings",
            "chrome://settings",
            "bowser://set\ntings",
            "",
        ] {
            assert_eq!(parse_input(text), None, "{text:?}");
        }
        assert_eq!(parse_input(&format!("bowser://settings/{}", "a".repeat(300))), None);
    }

    #[test]
    fn only_bowser_addresses_are_internal_and_web_ones_are_not() {
        assert!(is_internal(&url("bowser://settings")));
        assert!(is_internal(&url("bowser://newtab")));
        assert!(!is_internal(&url("https://example.com/")));
        assert!(!is_internal(&url("bowser-ui://app/index.html")));
        assert!(parse(&url("https://settings/")).is_none());
    }

    #[test]
    fn the_session_keeps_only_canonical_settings_addresses() {
        assert!(persistable(&url("bowser://settings")));
        assert!(persistable(&url("bowser://settings/privacy")));
        for text in ["bowser://settings/", "bowser://Settings", "bowser://settings/Privacy", "bowser://settings?x", "bowser://settings#a", "bowser://settings/nope", "bowser://newtab", "https://example.com/"] {
            assert!(!persistable(&url(text)), "{text}");
        }
    }

    #[test]
    fn open_reuses_a_settings_tab_then_a_blank_normal_tab_then_opens_one() {
        // The shown tab is already the page.
        assert_eq!(open_plan(&[(1, false), (2, true)], (2, false, false), true), OpenPlan::Existing(2));
        // Another tab is the page: it is selected, never duplicated.
        assert_eq!(open_plan(&[(1, false), (2, true), (3, true)], (1, true, false), true), OpenPlan::Existing(2));
        // A blank normal tab becomes the page (the omnibox), unless the caller wants a new tab (menu).
        assert_eq!(open_plan(&[(1, false)], (1, true, false), true), OpenPlan::Current(1));
        assert_eq!(open_plan(&[(1, false)], (1, true, false), false), OpenPlan::NewTab);
        // A private tab or a tab with a page never becomes the page.
        assert_eq!(open_plan(&[(1, false)], (1, true, true), true), OpenPlan::NewTab);
        assert_eq!(open_plan(&[(1, false)], (1, false, false), true), OpenPlan::NewTab);
    }

    // ---- bowser://downloads (docs/design/downloads.md) ----

    #[test]
    fn the_downloads_page_has_one_canonical_address_and_no_sections() {
        let page = InternalPage::Downloads;
        assert_eq!(page.url().as_str(), "bowser://downloads");
        assert_eq!(parse(&page.url()), Some(page));
        assert_eq!(page.title(), "Downloads");
        assert_eq!(page.section(), None);
        assert!(page.is_downloads() && !page.is_history() && !page.is_settings());
        assert!(page.same_page(InternalPage::Downloads));
        assert!(!page.same_page(InternalPage::History));
        assert!(!page.same_page(InternalPage::Settings(None)));
    }

    #[test]
    fn typed_downloads_addresses_open_the_page() {
        for text in ["bowser://downloads", "bowser://downloads/", " BOWSER://Downloads ", "bowser://DOWNLOADS/", "bowser://downloads?q=zip", "bowser://downloads#today", "\tbowser://downloads\n"] {
            assert_eq!(parse_input(text), Some(InternalPage::Downloads), "{text:?}");
        }
        for text in [
            "downloads",
            "bowser:downloads",
            "bowser://downloads/today",
            "bowser://downloads/x/y",
            "bowser://downloads.evil.example",
            "bowser://user@downloads",
            "bowser://downloads:80",
            "bowser://downloadsx",
            "bowser://download",
            "https://downloads",
            "chrome://downloads",
            "edge://downloads",
            "bowser-ui://app/index.html?view=downloads",
        ] {
            assert_eq!(parse_input(text), None, "{text:?}");
        }
    }

    #[test]
    fn the_session_keeps_only_the_canonical_downloads_address() {
        assert!(persistable(&url("bowser://downloads")));
        for text in ["bowser://downloads/", "bowser://Downloads", "bowser://downloads?q=x", "bowser://downloads#a", "bowser://downloads/today"] {
            assert!(!persistable(&url(text)), "{text}");
        }
    }

    #[test]
    fn open_reuses_an_existing_downloads_tab_and_never_another_internal_page() {
        let shows = |tabs: &[(u32, Option<InternalPage>)], page: InternalPage| -> Vec<(u32, bool)> {
            tabs.iter().map(|(id, shown)| (*id, shown.is_some_and(|shown| shown.same_page(page)))).collect()
        };
        let tabs = [(1, None), (2, Some(InternalPage::History)), (3, Some(InternalPage::Downloads))];
        assert_eq!(open_plan(&shows(&tabs, InternalPage::Downloads), (1, false, false), false), OpenPlan::Existing(3));
        assert_eq!(open_plan(&shows(&tabs, InternalPage::History), (3, false, false), false), OpenPlan::Existing(2));
        assert_eq!(open_plan(&shows(&tabs, InternalPage::Downloads), (3, false, false), true), OpenPlan::Existing(3));
        let history_only = [(1, None), (2, Some(InternalPage::History))];
        assert_eq!(open_plan(&shows(&history_only, InternalPage::Downloads), (2, false, false), true), OpenPlan::NewTab);
        assert_eq!(open_plan(&shows(&history_only, InternalPage::Downloads), (1, true, false), true), OpenPlan::Current(1));
        assert_eq!(open_plan(&shows(&history_only, InternalPage::Downloads), (1, true, true), true), OpenPlan::NewTab);
    }

    #[test]
    fn the_favicon_is_a_small_png_the_strip_accepts() {
        for favicon in [SETTINGS_FAVICON, HISTORY_FAVICON, DOWNLOADS_FAVICON] {
            assert!(favicon.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]));
            assert!(favicon.len() <= crate::chrome_host::protocol::MAX_FAVICON_BYTES);
            assert_eq!(u32::from_be_bytes(favicon[16..20].try_into().unwrap()), 32);
            assert_eq!(u32::from_be_bytes(favicon[20..24].try_into().unwrap()), 32);
            assert!(crate::chrome_host::protocol::favicon_data_uri(favicon).is_some());
        }
        assert_ne!(SETTINGS_FAVICON, HISTORY_FAVICON);
        assert_ne!(DOWNLOADS_FAVICON, HISTORY_FAVICON);
        assert_ne!(DOWNLOADS_FAVICON, SETTINGS_FAVICON);
        assert_eq!(InternalPage::Downloads.favicon(), DOWNLOADS_FAVICON);
        assert_eq!(InternalPage::History.favicon(), HISTORY_FAVICON);
        assert_eq!(InternalPage::Settings(Some(Section::About)).favicon(), SETTINGS_FAVICON);
    }

    // ---- bowser://history (docs/design/history-page.md) ----

    #[test]
    fn the_history_page_has_one_canonical_address_and_no_sections() {
        let page = InternalPage::History;
        assert_eq!(page.url().as_str(), "bowser://history");
        assert_eq!(parse(&page.url()), Some(page));
        assert_eq!(page.title(), "History");
        assert_eq!(page.section(), None);
        assert!(page.is_history() && !page.is_settings());
        assert!(page.same_page(InternalPage::History));
        assert!(!page.same_page(InternalPage::Settings(None)));
        assert!(!page.same_page(InternalPage::Downloads) && !page.is_downloads());
        assert!(InternalPage::Settings(None).same_page(InternalPage::Settings(Some(Section::Privacy))));
    }

    #[test]
    fn typed_history_addresses_open_the_page() {
        for text in ["bowser://history", "bowser://history/", " BOWSER://History ", "bowser://HISTORY/", "bowser://history?q=rust", "bowser://history#today", "\tbowser://history\n"] {
            assert_eq!(parse_input(text), Some(InternalPage::History), "{text:?}");
        }
        for text in [
            "history",
            "bowser:history",
            "bowser://history/today",
            "bowser://history/x/y",
            "bowser://history.evil.example",
            "bowser://user@history",
            "bowser://history:80",
            "bowser://historyx",
            "bowser://histor",
            "https://history",
            "chrome://history",
            "edge://history",
            "bowser-ui://app/index.html?view=history",
            "bowser://bookmarks",
        ] {
            assert_eq!(parse_input(text), None, "{text:?}");
        }
    }

    #[test]
    fn the_session_keeps_only_the_canonical_history_address() {
        assert!(persistable(&url("bowser://history")));
        for text in ["bowser://history/", "bowser://History", "bowser://history?q=x", "bowser://history#a", "bowser://history/today"] {
            assert!(!persistable(&url(text)), "{text}");
        }
    }

    #[test]
    fn open_reuses_an_existing_history_tab_and_never_a_settings_tab() {
        // The caller marks only tabs that show the same page: a settings tab is not a history tab.
        let shows = |tabs: &[(u32, Option<InternalPage>)], page: InternalPage| -> Vec<(u32, bool)> {
            tabs.iter().map(|(id, shown)| (*id, shown.is_some_and(|shown| shown.same_page(page)))).collect()
        };
        let tabs = [(1, None), (2, Some(InternalPage::Settings(None))), (3, Some(InternalPage::History))];
        assert_eq!(open_plan(&shows(&tabs, InternalPage::History), (1, false, false), false), OpenPlan::Existing(3));
        assert_eq!(open_plan(&shows(&tabs, InternalPage::Settings(Some(Section::Privacy))), (3, false, false), false), OpenPlan::Existing(2));
        // The history tab is the shown tab: it stays, nothing is duplicated.
        assert_eq!(open_plan(&shows(&tabs, InternalPage::History), (3, false, false), true), OpenPlan::Existing(3));
        // Only a settings tab exists: history opens beside it, or in a blank normal tab.
        let settings_only = [(1, None), (2, Some(InternalPage::Settings(None)))];
        assert_eq!(open_plan(&shows(&settings_only, InternalPage::History), (2, false, false), true), OpenPlan::NewTab);
        assert_eq!(open_plan(&shows(&settings_only, InternalPage::History), (1, true, false), true), OpenPlan::Current(1));
        // A blank private tab never becomes the page.
        assert_eq!(open_plan(&shows(&settings_only, InternalPage::History), (1, true, true), true), OpenPlan::NewTab);
    }
}
