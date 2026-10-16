//! Tab behaviour options (the settings page's Tabs section) and the pure rules that act on them.
//!
//! Every option is one `tabs_*` line in `settings.txt`. The table in [`tab_options!`] is the single
//! source of the struct, its defaults, the file keys, the settings snapshot and the validation of a
//! value that arrives from the page, so a key cannot be saved without being read, or the other way
//! round. The functions below the table are the decisions the options make (where a new tab goes,
//! which tab a close activates, when to ask first, how a title notification is read), kept pure so
//! each one has a test that names the effect.

use crate::tabs::TabId;

/// A value as the settings snapshot carries it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    Text(String),
    Uint(u64),
}

/// One option value: its file text, its wire value and how a typed text becomes one.
pub trait OptValue: Sized + Clone {
    fn parse(text: &str) -> Option<Self>;
    fn text(&self) -> String;
    fn value(&self) -> Value;
}

impl OptValue for bool {
    fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Some(true),
            "false" | "0" | "no" | "off" => Some(false),
            _ => None,
        }
    }
    fn text(&self) -> String {
        self.to_string()
    }
    fn value(&self) -> Value {
        Value::Bool(*self)
    }
}

/// A whole number between `MIN` and `MAX` (inclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounded<const MIN: u32, const MAX: u32>(pub u32);

impl<const MIN: u32, const MAX: u32> OptValue for Bounded<MIN, MAX> {
    fn parse(text: &str) -> Option<Self> {
        let n = text.parse::<u32>().ok()?;
        (MIN..=MAX).contains(&n).then_some(Self(n))
    }
    fn text(&self) -> String {
        self.0.to_string()
    }
    fn value(&self) -> Value {
        Value::Uint(u64::from(self.0))
    }
}

/// An address typed for "specific page": empty, or an http(s) URL without control characters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageUrl(pub String);

/// Longest address kept for the new tab page.
pub const MAX_PAGE_URL: usize = 2048;

impl OptValue for PageUrl {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return Some(Self(String::new()));
        }
        if text.len() > MAX_PAGE_URL || text.chars().any(char::is_control) {
            return None;
        }
        let url = url::Url::parse(text).ok()?;
        matches!(url.scheme(), "http" | "https").then(|| Self(url.to_string()))
    }
    fn text(&self) -> String {
        self.0.clone()
    }
    fn value(&self) -> Value {
        Value::Text(self.0.clone())
    }
}

macro_rules! choice {
    ($(#[$doc:meta])* $name:ident { $($variant:ident = $key:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum $name { $($variant),+ }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub fn key(self) -> &'static str {
                match self { $(Self::$variant => $key),+ }
            }
        }
        impl OptValue for $name {
            fn parse(text: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|choice| choice.key() == text)
            }
            fn text(&self) -> String {
                self.key().to_owned()
            }
            fn value(&self) -> Value {
                Value::Text(self.key().to_owned())
            }
        }
    };
}

choice! {
    /// What a new tab shows: the start page (T5's speed dials), the homepage of Settings > Start page
    /// (`homepage`), a blank page with no start page over it, or the address in `tabs_new_tab_url`.
    NewTabPage { Start = "start", Homepage = "homepage", Blank = "blank", Url = "url" }
}
choice! {
    /// Where a duplicated tab goes.
    DuplicatePosition { Beside = "beside", Last = "last" }
}
choice! {
    /// Which tab a close activates when no related tab applies.
    CloseActivation { Right = "right", Left = "left", Recent = "recent" }
}
choice! {
    /// What a double click on a tab does.
    DoubleClick { Ignore = "ignore", Rename = "rename", Close = "close" }
}
choice! {
    /// How a page load shows on its tab.
    LoadIndicator { Spinner = "spinner", Bar = "bar", Favicon = "favicon", None = "none" }
}
choice! {
    /// Which tabs may play sound.
    MuteMode { All = "all", Active = "active", Prioritise = "prioritise" }
}
choice! {
    /// What closing a pinned tab does.
    PinnedClose { Close = "close", Keep = "keep", Minimise = "minimise" }
}

macro_rules! tab_options {
    ($($(#[$doc:meta])* $field:ident: $ty:ty = $default:expr, $key:literal;)*) => {
        /// The profile's tab options. Defaults keep the behaviour Bowser had before they existed.
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct TabOptions {
            $($(#[$doc])* pub $field: $ty,)*
        }

        impl Default for TabOptions {
            fn default() -> Self {
                Self { $($field: $default,)* }
            }
        }

        impl TabOptions {
            /// Every file key, in the order the snapshot lists them.
            pub const KEYS: &'static [&'static str] = &[$($key,)*];

            /// `settings.txt` line values as (key, text).
            pub fn pairs(&self) -> Vec<(&'static str, String)> {
                vec![$(($key, <$ty as OptValue>::text(&self.$field)),)*]
            }

            /// The snapshot's wire values as (key, value).
            pub fn entries(&self) -> Vec<(&'static str, Value)> {
                vec![$(($key, <$ty as OptValue>::value(&self.$field)),)*]
            }

            /// Sets the option `key` from `text`. False, with nothing changed, for an unknown key or a
            /// value the option does not accept.
            pub fn set(&mut self, key: &str, text: &str) -> bool {
                match key {
                    $($key => match <$ty as OptValue>::parse(text) {
                        Some(value) => { self.$field = value; true }
                        None => false,
                    },)*
                    _ => false,
                }
            }
        }
    };
}

tab_options! {
    /// What a new tab (Ctrl+T, the new tab button) shows.
    new_tab_page: NewTabPage = NewTabPage::Start, "tabs_new_tab_page";
    /// The address `NewTabPage::Url` opens.
    new_tab_url: PageUrl = PageUrl(String::new()), "tabs_new_tab_url";
    /// A tab opened from a page goes next to the tab that opened it and its other children.
    new_tab_related: bool = false, "tabs_new_tab_related";
    /// A link opened in a new tab loads without leaving the page being read.
    link_background: bool = false, "tabs_link_background";
    duplicate_position: DuplicatePosition = DuplicatePosition::Beside, "tabs_duplicate_position";
    /// Closing a tab returns to the tab that opened it, when that is still open.
    close_related: bool = false, "tabs_close_related";
    close_activation: CloseActivation = CloseActivation::Right, "tabs_close_activation";
    /// While the pointer stays on the strip after a close, the next tab's close button stays under it.
    align_close_button: bool = false, "tabs_align_close_button";
    /// Closing the last tab leaves a fresh tab instead of closing the window.
    keep_window: bool = true, "tabs_keep_window";
    /// Ask first when closing this many tabs at once. 0 never asks.
    confirm_close: Bounded<0, 99> = Bounded(0), "tabs_confirm_close";
    /// A new tab puts the keyboard in the page, not the address bar, when it has an address.
    focus_page: bool = false, "tabs_focus_page";
    double_click: DoubleClick = DoubleClick::Ignore, "tabs_double_click";
    /// Tabs keep their width and the strip scrolls sideways when they do not fit.
    scroll_tabs: bool = false, "tabs_scroll";
    /// Drops the gap above the tabs while the window is maximised.
    remove_spacing_maximised: bool = false, "tabs_remove_spacing_maximised";
    close_button_left: bool = false, "tabs_close_left";
    close_button_always: bool = false, "tabs_close_always";
    /// A title that starts with a count, such as "(3) Inbox", marks the tab with that count.
    title_notifications: bool = true, "tabs_title_notifications";
    /// A background tab that changed since you left it carries a dot.
    unread_indicator: bool = true, "tabs_unread_indicator";
    dim_hibernated: bool = true, "tabs_dim_hibernated";
    /// Narrowest the active tab gets, in pixels. 0 leaves it to the strip.
    active_min_width: Bounded<0, 400> = Bounded(0), "tabs_active_min_width";
    /// A tab not used for this many days is shown dimmed. 0 turns it off.
    inactive_days: Bounded<0, 365> = Bounded(0), "tabs_inactive_days";
    load_indicator: LoadIndicator = LoadIndicator::Spinner, "tabs_load_indicator";
    /// The wheel over the tab strip switches tabs.
    wheel_switch: bool = false, "tabs_wheel_switch";
    /// Clicking the active tab goes back to the tab used before it.
    minimise_active: bool = false, "tabs_minimise_active";
    /// Ctrl and Shift click select several tabs.
    selection: bool = true, "tabs_selection";
    /// The active tab counts as selected when other tabs are selected.
    selection_active: bool = true, "tabs_selection_active";
    mute_mode: MuteMode = MuteMode::All, "tabs_mute_mode";
    pinned_close: PinnedClose = PinnedClose::Close, "tabs_pinned_close";
    pinned_separator: bool = false, "tabs_pinned_separator";
    /// A pinned tab stays on its site: a link to another site opens in a new tab.
    pinned_restrict: bool = false, "tabs_pinned_restrict";
    /// Restored tabs load when first selected. Off, they all load at once.
    lazy_restore: bool = true, "tabs_lazy_restore";
    /// Restored pinned tabs load at once even when the others wait.
    load_pinned: bool = false, "tabs_load_pinned";
}

// ---- virtual options: shown by the page, stored in the older settings they belong to ----

/// Where a new tab opens, the three choices the page offers. Stored as `new_tab_beside_active` plus
/// `tabs_new_tab_related`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NewTabPosition {
    Related,
    Active,
    Last,
}

impl NewTabPosition {
    pub fn key(self) -> &'static str {
        match self {
            Self::Related => "related",
            Self::Active => "active",
            Self::Last => "last",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        [Self::Related, Self::Active, Self::Last].into_iter().find(|position| position.key() == text)
    }
}

/// The position the two stored settings add up to.
pub fn new_tab_position(beside_active: bool, related: bool) -> NewTabPosition {
    match (beside_active, related) {
        (false, _) => NewTabPosition::Last,
        (true, true) => NewTabPosition::Related,
        (true, false) => NewTabPosition::Active,
    }
}

/// The wire key of the position option in the snapshot.
pub const POSITION_KEY: &str = "tabs_new_tab_position";

/// Applies one `key=value` line from the page to `settings`. False when the key is unknown or the
/// value is refused, in which case nothing changed.
pub fn apply_assignment(settings: &mut crate::settings::Settings, text: &str) -> bool {
    let Some((key, value)) = text.split_once('=') else {
        return false;
    };
    if key == POSITION_KEY {
        let Some(position) = NewTabPosition::parse(value) else {
            return false;
        };
        settings.new_tab_beside_active = position != NewTabPosition::Last;
        settings.tabs.new_tab_related = position == NewTabPosition::Related;
        return true;
    }
    settings.tabs.set(key, value)
}

/// The snapshot's wire values: every stored option plus the virtual position.
pub fn snapshot_entries(settings: &crate::settings::Settings) -> Vec<(&'static str, Value)> {
    let mut entries = settings.tabs.entries();
    // The page sees the related flag only as part of the position.
    entries.retain(|(key, _)| *key != "tabs_new_tab_related");
    entries.push((
        POSITION_KEY,
        Value::Text(new_tab_position(settings.new_tab_beside_active, settings.tabs.new_tab_related).key().to_owned()),
    ));
    entries
}

/// Reads `tabs_*` lines of `settings.txt`. A value the option refuses leaves the default.
pub fn read_file_line(options: &mut TabOptions, key: &str, value: &str) {
    // The file folds case like its other keys (`Recent`, `TRUE`); an address keeps its own.
    if !options.set(key, value) {
        options.set(key, &value.to_ascii_lowercase());
    }
}

/// Returns `text` with the `tabs_*` lines replaced by `options`, keeping everything else (comments,
/// other keys, the line ending) as it was. A line already in the file is updated; one that is not is
/// added only when its option differs from the default, so a profile that never touched the Tabs
/// section keeps a settings file without tab lines.
pub fn render_into(text: &str, options: &TabOptions, newline: &str) -> String {
    let pairs = options.pairs();
    let defaults = TabOptions::default().pairs();
    let mut written = vec![false; pairs.len()];
    let mut out = String::with_capacity(text.len() + 64);
    for line in text.lines() {
        let slot = if line.trim_start().starts_with('#') {
            None
        } else {
            line.split_once('=').and_then(|(key, _)| pairs.iter().position(|(name, _)| *name == key.trim()))
        };
        match slot {
            Some(index) if written[index] => {}
            Some(index) => {
                written[index] = true;
                out.push_str(&format!("{}={}{newline}", pairs[index].0, pairs[index].1));
            }
            None => {
                out.push_str(line);
                out.push_str(newline);
            }
        }
    }
    for (index, (key, value)) in pairs.iter().enumerate() {
        if !written[index] && *value != defaults[index].1 {
            out.push_str(&format!("{key}={value}{newline}"));
        }
    }
    out
}

// ---- decisions ----

/// What the new tab page is, for the address a fresh tab should open. `homepage` is the start page
/// settings' homepage (`StartSettings::home_target`), None while it is empty.
pub fn new_tab_address(options: &TabOptions, homepage: Option<url::Url>) -> NewTabTarget {
    match options.new_tab_page {
        NewTabPage::Start | NewTabPage::Blank => NewTabTarget::Nothing,
        // With no homepage set a new tab falls back to the start page, as the Home button does.
        NewTabPage::Homepage => homepage.map_or(NewTabTarget::Nothing, NewTabTarget::Page),
        NewTabPage::Url => match url::Url::parse(&options.new_tab_url.0) {
            Ok(url) => NewTabTarget::Page(url),
            Err(_) => NewTabTarget::Nothing,
        },
    }
}

/// Whether a blank normal tab shows the start page (`win32::start_ui::enabled`). Only "Blank page"
/// turns it off; the homepage and address choices load a page, and while theirs is not set the start
/// page is what an empty tab shows.
pub fn start_page_shown(options: &TabOptions) -> bool {
    options.new_tab_page != NewTabPage::Blank
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NewTabTarget {
    /// The tab stays empty: the start page (or the empty tab) is what shows.
    Nothing,
    Page(url::Url),
}

/// Whether closing `count` tabs at once should ask first.
pub fn should_confirm_close(count: usize, threshold: u32) -> bool {
    threshold >= 2 && count >= threshold as usize
}

/// The words of the confirmation.
pub fn confirm_close_text(count: usize) -> String {
    format!("Close {count} tabs?")
}

/// The tab a close activates. `order` is the tabs of the window in strip order before the close,
/// `closed` the tab going away, `opener` the tab that opened it, `recent` the tabs last left first.
/// None means no preference: the caller keeps whatever the model chose.
pub fn close_target(
    options: &TabOptions,
    order: &[TabId],
    closed: TabId,
    opener: Option<TabId>,
    recent: &[TabId],
) -> Option<TabId> {
    let index = order.iter().position(|id| *id == closed)?;
    let others = || order.iter().copied().filter(move |id| *id != closed);
    if options.close_related
        && let Some(opener) = opener.filter(|opener| others().any(|id| id == *opener))
    {
        return Some(opener);
    }
    match options.close_activation {
        CloseActivation::Right => order.get(index + 1).copied().or_else(|| index.checked_sub(1).map(|left| order[left])),
        CloseActivation::Left => index.checked_sub(1).map(|left| order[left]).or_else(|| order.get(index + 1).copied()),
        CloseActivation::Recent => recent
            .iter()
            .rev()
            .copied()
            .find(|id| *id != closed && order.contains(id))
            .or_else(|| order.get(index + 1).copied())
            .or_else(|| index.checked_sub(1).map(|left| order[left])),
    }
}

/// A count at the start of a page title: "(3) Inbox" or "[12] Chat". The count is what the tab
/// shows. A bare year such as "(2024)" is a title, not a notification count.
pub fn title_notification(title: &str) -> Option<u32> {
    let title = title.trim_start();
    let (open, close) = match title.chars().next()? {
        '(' => ('(', ')'),
        '[' => ('[', ']'),
        _ => return None,
    };
    let rest = &title[open.len_utf8()..];
    let end = rest.find(close)?;
    let digits = &rest[..end];
    if digits.is_empty() || digits.len() > 3 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let count = digits.parse::<u32>().ok()?;
    (count > 0).then_some(count)
}

/// Whether the tab has gone unused long enough to show as inactive. `idle_secs` is how long since it
/// was last the active tab.
pub fn is_inactive(options: &TabOptions, idle_secs: u64) -> bool {
    options.inactive_days.0 > 0 && idle_secs >= u64::from(options.inactive_days.0) * 86_400
}

/// Whether `candidate` may play sound while `active` is the selected tab under `mode`.
/// `active_audible` is whether the selected tab is making sound right now.
pub fn tab_may_play(mode: MuteMode, is_active: bool, active_audible: bool) -> bool {
    match mode {
        MuteMode::All => true,
        MuteMode::Active => is_active,
        MuteMode::Prioritise => is_active || !active_audible,
    }
}

/// How closing a tab by the user's hand resolves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseKind {
    /// The tab closes (the last one is replaced by a fresh tab).
    Close,
    /// The tab stays; a pinned tab is not closed.
    Keep,
    /// The tab goes to sleep and another tab is shown.
    Minimise,
    /// The last tab closed and the window follows.
    CloseWindow,
}

pub fn close_kind(options: &TabOptions, pinned: bool, tab_count: usize) -> CloseKind {
    if pinned {
        match options.pinned_close {
            PinnedClose::Keep => return CloseKind::Keep,
            PinnedClose::Minimise => return CloseKind::Minimise,
            PinnedClose::Close => {}
        }
    }
    if tab_count <= 1 && !options.keep_window {
        CloseKind::CloseWindow
    } else {
        CloseKind::Close
    }
}

/// Whether a navigation of a pinned tab to `target` should open in a new tab instead, because
/// pinned tabs are restricted to the site they were pinned on. `pinned_site` is the registrable
/// host the tab had.
pub fn leaves_pinned_site(options: &TabOptions, pinned: bool, pinned_site: Option<&str>, target: &url::Url) -> bool {
    if !options.pinned_restrict || !pinned {
        return false;
    }
    let Some(site) = pinned_site else {
        return false;
    };
    match target.host_str() {
        Some(host) => registrable(host) != registrable(site),
        None => false,
    }
}

/// A host's site: its last two labels, three when the second to last is a short generic label
/// (`co.uk`). Good enough to tell `news.example.com` from `example.org`; the exact public suffix
/// list is not needed to keep a pinned tab on its site.
pub fn registrable(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() <= 2 || host.parse::<std::net::IpAddr>().is_ok() {
        return host;
    }
    let second = labels[labels.len() - 2];
    let take = if second.len() <= 3 && matches!(second, "co" | "com" | "org" | "net" | "gov" | "edu" | "ac") && labels[labels.len() - 1].len() == 2 { 3 } else { 2 };
    labels[labels.len() - take..].join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[u32]) -> Vec<TabId> {
        list.iter().map(|id| TabId(*id)).collect()
    }

    #[test]
    fn defaults_keep_the_behaviour_bowser_had() {
        let options = TabOptions::default();
        assert_eq!(options.new_tab_page, NewTabPage::Start);
        assert_eq!(options.close_activation, CloseActivation::Right);
        assert!(options.keep_window);
        assert_eq!(options.confirm_close.0, 0);
        assert_eq!(options.mute_mode, MuteMode::All);
        assert_eq!(options.pinned_close, PinnedClose::Close);
        assert!(options.lazy_restore);
        assert!(!options.close_related);
    }

    #[test]
    fn every_key_round_trips_through_its_text() {
        let defaults = TabOptions::default();
        let mut copy = TabOptions::default();
        for (key, text) in defaults.pairs() {
            assert!(copy.set(key, &text), "{key} refused its own text {text:?}");
        }
        assert_eq!(copy, defaults);
        assert_eq!(TabOptions::KEYS.len(), defaults.pairs().len());
        assert_eq!(defaults.entries().len(), TabOptions::KEYS.len());
    }

    #[test]
    fn values_are_checked_the_way_the_file_and_the_page_need() {
        let mut options = TabOptions::default();
        assert!(options.set("tabs_new_tab_page", "url"));
        assert_eq!(options.new_tab_page, NewTabPage::Url);
        assert!(!options.set("tabs_new_tab_page", "URL"), "keys are exact");
        assert!(options.set("tabs_new_tab_page", "homepage"), "the start page settings' homepage");
        assert_eq!(options.new_tab_page, NewTabPage::Homepage);
        assert!(options.set("tabs_new_tab_page", "blank"), "a blank page without the start page");
        assert_eq!(options.new_tab_page, NewTabPage::Blank);
        assert!(!options.set("tabs_new_tab_page", "speeddial"));
        assert!(options.set("tabs_confirm_close", "5"));
        assert_eq!(options.confirm_close.0, 5);
        assert!(!options.set("tabs_confirm_close", "100"));
        assert!(!options.set("tabs_confirm_close", "-1"));
        assert!(options.set("tabs_active_min_width", "400"));
        assert!(!options.set("tabs_active_min_width", "401"));
        assert!(options.set("tabs_inactive_days", "365"));
        assert!(!options.set("tabs_inactive_days", "366"));
        assert!(options.set("tabs_keep_window", "false"));
        assert!(!options.keep_window);
        assert!(options.set("tabs_keep_window", "yes"), "the file's usual yes/no spellings");
        assert!(options.keep_window);
        assert!(!options.set("tabs_keep_window", "maybe"));
        assert!(!options.set("tabs_nothing", "true"));
        assert!(options.set("tabs_new_tab_url", "https://example.com/start"));
        assert_eq!(options.new_tab_url.0, "https://example.com/start");
        assert!(!options.set("tabs_new_tab_url", "javascript:alert(1)"));
        assert!(!options.set("tabs_new_tab_url", "file:///c:/windows"));
        assert!(options.set("tabs_new_tab_url", ""));
        assert_eq!(options.new_tab_url.0, "");
        assert_eq!(options.new_tab_url.0, "");
    }

    #[test]
    fn the_page_changes_the_position_through_the_two_stored_settings() {
        let mut settings = crate::settings::Settings::default();
        assert_eq!(new_tab_position(settings.new_tab_beside_active, settings.tabs.new_tab_related), NewTabPosition::Active);
        assert!(apply_assignment(&mut settings, "tabs_new_tab_position=related"));
        assert!(settings.new_tab_beside_active && settings.tabs.new_tab_related);
        assert!(apply_assignment(&mut settings, "tabs_new_tab_position=last"));
        assert!(!settings.new_tab_beside_active);
        assert!(!settings.tabs.new_tab_related);
        assert!(apply_assignment(&mut settings, "tabs_new_tab_position=active"));
        assert!(settings.new_tab_beside_active && !settings.tabs.new_tab_related);
        assert!(!apply_assignment(&mut settings, "tabs_new_tab_position=stack"), "stacks wait for tab stacks");
        assert!(!apply_assignment(&mut settings, "tabs_new_tab_position"));
        assert!(!apply_assignment(&mut settings, "tabs_unknown=true"));
        assert!(apply_assignment(&mut settings, "tabs_double_click=close"));
        assert_eq!(settings.tabs.double_click, DoubleClick::Close);
        let entries = snapshot_entries(&settings);
        assert!(entries.contains(&(POSITION_KEY, Value::Text("active".into()))));
        assert!(entries.contains(&("tabs_double_click", Value::Text("close".into()))));
    }

    #[test]
    fn rendering_replaces_appends_and_keeps_everything_else() {
        let options = TabOptions { keep_window: false, close_activation: CloseActivation::Recent, ..TabOptions::default() };
        let existing = "# my settings\r\ntab_layout=vertical\r\ntabs_keep_window=true\r\ntabs_keep_window=true\r\n";
        let out = render_into(existing, &options, "\r\n");
        assert!(out.starts_with("# my settings\r\ntab_layout=vertical\r\ntabs_keep_window=false\r\n"), "{out}");
        assert_eq!(out.matches("tabs_keep_window").count(), 1, "a repeated line is collapsed");
        assert!(out.contains("tabs_close_activation=recent\r\n"));
        let mut parsed = TabOptions::default();
        for line in out.lines() {
            if let Some((key, value)) = line.split_once('=') {
                read_file_line(&mut parsed, key, value);
            }
        }
        assert_eq!(parsed, options);
    }

    #[test]
    fn a_new_tab_page_choice_decides_what_a_fresh_tab_opens() {
        let home = || Some(url::Url::parse("https://home.example/").unwrap());
        let mut options = TabOptions::default();
        assert_eq!(new_tab_address(&options, home()), NewTabTarget::Nothing, "the start page");
        assert!(start_page_shown(&options));
        options.new_tab_page = NewTabPage::Url;
        assert_eq!(new_tab_address(&options, home()), NewTabTarget::Nothing, "no address typed yet");
        options.new_tab_url = PageUrl("https://example.com/".into());
        assert_eq!(new_tab_address(&options, home()), NewTabTarget::Page(url::Url::parse("https://example.com/").unwrap()));
        assert!(start_page_shown(&options), "an address choice still shows the start page on a blank tab");
        options.new_tab_page = NewTabPage::Homepage;
        assert_eq!(new_tab_address(&options, home()), home().map(NewTabTarget::Page).unwrap());
        assert_eq!(new_tab_address(&options, None), NewTabTarget::Nothing, "no homepage set: the start page shows");
        options.new_tab_page = NewTabPage::Blank;
        assert_eq!(new_tab_address(&options, home()), NewTabTarget::Nothing);
        assert!(!start_page_shown(&options), "blank: no start page over the empty tab");
    }

    #[test]
    fn confirmation_needs_a_threshold_of_two_or_more() {
        assert!(!should_confirm_close(10, 0), "0 never asks");
        assert!(!should_confirm_close(10, 1), "1 is not a bulk close");
        assert!(!should_confirm_close(2, 3));
        assert!(should_confirm_close(3, 3));
        assert!(should_confirm_close(9, 3));
        assert_eq!(confirm_close_text(4), "Close 4 tabs?");
    }

    #[test]
    fn close_activation_follows_the_option() {
        let order = ids(&[1, 2, 3, 4]);
        let mut options = TabOptions::default();
        assert_eq!(close_target(&options, &order, TabId(2), None, &[]), Some(TabId(3)), "right by default");
        assert_eq!(close_target(&options, &order, TabId(4), None, &[]), Some(TabId(3)), "left at the end");
        options.close_activation = CloseActivation::Left;
        assert_eq!(close_target(&options, &order, TabId(3), None, &[]), Some(TabId(2)));
        assert_eq!(close_target(&options, &order, TabId(1), None, &[]), Some(TabId(2)), "right at the start");
        options.close_activation = CloseActivation::Recent;
        let recent = ids(&[4, 1, 2]);
        assert_eq!(close_target(&options, &order, TabId(2), None, &recent), Some(TabId(1)), "the one used just before");
        assert_eq!(close_target(&options, &order, TabId(1), None, &ids(&[1])), Some(TabId(2)), "history of only itself falls back");
        options.close_activation = CloseActivation::Right;
        options.close_related = true;
        assert_eq!(close_target(&options, &order, TabId(3), Some(TabId(1)), &[]), Some(TabId(1)), "back to the opener");
        assert_eq!(close_target(&options, &order, TabId(3), Some(TabId(9)), &[]), Some(TabId(4)), "a closed opener is ignored");
        assert_eq!(close_target(&options, &order, TabId(9), None, &[]), None);
    }

    #[test]
    fn a_count_at_the_start_of_a_title_is_a_notification() {
        assert_eq!(title_notification("(3) Inbox - Mail"), Some(3));
        assert_eq!(title_notification("  [12] Chat"), Some(12));
        assert_eq!(title_notification("(999) Feed"), Some(999));
        assert_eq!(title_notification("(0) Inbox"), None);
        assert_eq!(title_notification("(2024) Annual report"), None, "a year is a title");
        assert_eq!(title_notification("Inbox (3)"), None);
        assert_eq!(title_notification("(x) Inbox"), None);
        assert_eq!(title_notification("()"), None);
        assert_eq!(title_notification(""), None);
    }

    #[test]
    fn inactivity_needs_a_threshold_and_enough_idle_time() {
        let mut options = TabOptions::default();
        assert!(!is_inactive(&options, u64::MAX), "0 days is off");
        options.inactive_days = Bounded(2);
        assert!(!is_inactive(&options, 2 * 86_400 - 1));
        assert!(is_inactive(&options, 2 * 86_400));
    }

    #[test]
    fn mute_modes_decide_which_tabs_may_play() {
        assert!(tab_may_play(MuteMode::All, false, true));
        assert!(!tab_may_play(MuteMode::Active, false, false));
        assert!(tab_may_play(MuteMode::Active, true, false));
        assert!(!tab_may_play(MuteMode::Prioritise, false, true), "others wait while the active tab plays");
        assert!(tab_may_play(MuteMode::Prioritise, false, false), "others play when it is silent");
        assert!(tab_may_play(MuteMode::Prioritise, true, true));
    }

    #[test]
    fn closing_by_hand_follows_the_pinned_and_last_tab_options() {
        let mut options = TabOptions::default();
        assert_eq!(close_kind(&options, false, 3), CloseKind::Close);
        assert_eq!(close_kind(&options, false, 1), CloseKind::Close, "the window stays by default");
        options.keep_window = false;
        assert_eq!(close_kind(&options, false, 1), CloseKind::CloseWindow);
        assert_eq!(close_kind(&options, false, 2), CloseKind::Close);
        options.pinned_close = PinnedClose::Keep;
        assert_eq!(close_kind(&options, true, 3), CloseKind::Keep);
        assert_eq!(close_kind(&options, true, 1), CloseKind::Keep, "a kept pinned tab does not close the window");
        assert_eq!(close_kind(&options, false, 3), CloseKind::Close);
        options.pinned_close = PinnedClose::Minimise;
        assert_eq!(close_kind(&options, true, 3), CloseKind::Minimise);
    }

    #[test]
    fn a_pinned_tab_stays_on_its_site_when_restricted() {
        let mut options = TabOptions::default();
        let other = url::Url::parse("https://example.org/").unwrap();
        let same = url::Url::parse("https://news.example.com/a").unwrap();
        assert!(!leaves_pinned_site(&options, true, Some("example.com"), &other), "off by default");
        options.pinned_restrict = true;
        assert!(leaves_pinned_site(&options, true, Some("www.example.com"), &other));
        assert!(!leaves_pinned_site(&options, true, Some("www.example.com"), &same), "a subdomain is the same site");
        assert!(!leaves_pinned_site(&options, false, Some("example.com"), &other), "only pinned tabs");
        assert!(!leaves_pinned_site(&options, true, None, &other), "no site recorded yet");
        assert_eq!(registrable("a.b.example.co.uk"), "example.co.uk");
        assert_eq!(registrable("localhost"), "localhost");
        assert_eq!(registrable("127.0.0.1"), "127.0.0.1");
    }
}
