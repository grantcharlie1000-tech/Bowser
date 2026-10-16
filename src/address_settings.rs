//! Address bar, search field, drop-down, search nickname and bookmark options.
//!
//! These live as extra `key=value` lines in the profile's `settings.txt`, carried by
//! [`crate::settings::Settings::address`]. The settings page changes them one key at a time through
//! [`AddressSettings::set`], which refuses anything it does not understand, so a typo or a hostile
//! page value never reaches a consumer. The search engines themselves are in
//! [`crate::search_engines`]; the keys here only point at them (`search_last_engine`).

use std::collections::HashSet;

/// How the search field next to the address field is drawn, when it is.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SearchField {
    /// No search field: searching happens in the address field.
    #[default]
    Off,
    /// A text field.
    Field,
    /// A button that opens the field when pressed.
    Button,
}

impl SearchField {
    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Field => "field",
            Self::Button => "button",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        [Self::Off, Self::Field, Self::Button].into_iter().find(|mode| mode.key() == value)
    }
}

/// Where the bookmarks bar sits.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BarPosition {
    #[default]
    Top,
    Bottom,
}

impl BarPosition {
    pub fn key(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        [Self::Top, Self::Bottom].into_iter().find(|position| position.key() == value)
    }
}

/// What a bookmark on the bar shows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BarDisplay {
    #[default]
    TextIcon,
    Text,
    Icon,
    /// A folder-less bookmark shows only its icon, every folder keeps its name.
    BookmarkIcon,
}

impl BarDisplay {
    pub const ALL: [Self; 4] = [Self::TextIcon, Self::Text, Self::Icon, Self::BookmarkIcon];
    pub fn key(self) -> &'static str {
        match self {
            Self::TextIcon => "text-icon",
            Self::Text => "text",
            Self::Icon => "icon",
            Self::BookmarkIcon => "bookmark-icon",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|display| display.key() == value)
    }
}

/// One place the address drop-down draws suggestions from. The order of the enabled sources in
/// `dd_sources` is the order of their rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Source {
    /// What was typed, as an address or a search (the default row).
    Direct,
    Bookmarks,
    /// A bookmark whose nickname is what was typed.
    Nicknames,
    /// Suggestions fetched from the engine named by a typed nickname.
    Search,
    /// Searches typed before.
    SearchHistory,
    History,
    Calculator,
}

impl Source {
    pub const ALL: [Self; 7] = [
        Self::Direct,
        Self::Bookmarks,
        Self::Nicknames,
        Self::Search,
        Self::SearchHistory,
        Self::History,
        Self::Calculator,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Bookmarks => "bookmarks",
            Self::Nicknames => "nicknames",
            Self::Search => "search",
            Self::SearchHistory => "search-history",
            Self::History => "history",
            Self::Calculator => "calculator",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|source| source.key() == value)
    }
}

/// The three scoped nicknames of the address field: `@bookmarks`, `@history`, `@tabs` by default.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scope {
    Bookmarks,
    History,
    Tabs,
}

pub const MIN_DROPDOWN_RESULTS: u8 = 1;
pub const MAX_DROPDOWN_RESULTS: u8 = 20;
pub const DEFAULT_DROPDOWN_RESULTS: u8 = 8;
/// Fewest bookmarks that make "open all" ask first (and the most the setting allows).
pub const MIN_CONFIRM_OPEN: u8 = 2;
pub const MAX_CONFIRM_OPEN: u8 = 100;
const MAX_SUFFIX_CHARS: usize = 24;
const MAX_NICKNAME_CHARS: usize = 16;
const MAX_BAR_GROUPS: usize = 32;
const MAX_DOMAIN_CHARS: usize = 253;
const MAX_ENGINE_ID_CHARS: usize = 32;

/// Every option of this module. Defaults match what Bowser did before the options existed, except
/// where Vivaldi's own default makes the feature visible (the scoped nicknames).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddressSettings {
    // --- address bar ---
    /// `address_bar_show`: the address field is drawn. Off, it appears only while it has focus.
    pub bar_show: bool,
    /// `address_autocomplete`: inline completion of the typed text from history.
    pub autocomplete: bool,
    /// `address_show_full`: the unfocused field shows the whole address, scheme and `www.` too.
    pub show_full: bool,
    /// `address_select_on_activate`: clicking or tabbing into the field selects its text.
    pub select_on_activate: bool,
    /// `address_copy_encoded`: copying or cutting the whole address puts the percent-encoded form
    /// on the clipboard.
    pub copy_encoded: bool,
    /// `address_qr_button`: a QR code button in the address field.
    pub qr_button: bool,
    // --- drop-down ---
    /// `dd_show_while_typing`: the drop-down opens as you type. Off, it opens only with Down.
    pub dropdown_while_typing: bool,
    /// `dd_always_badge`: every row carries its kind badge, not only bookmarks with a favicon.
    pub always_badge: bool,
    /// `dd_typed_history`: focusing an empty field lists what was typed before.
    pub typed_history: bool,
    /// `dd_typed_include_searches`: that list includes searches.
    pub typed_include_searches: bool,
    /// `dd_history_button`: a button at the end of the field opens that list.
    pub history_button: bool,
    /// `dd_max_results`: most rows in the drop-down, the default row included.
    pub max_results: u8,
    /// `dd_prioritise_bookmarks`: bookmark rows come before the other sources.
    pub prioritise_bookmarks: bool,
    /// `dd_prioritise_direct`: the default row (what Enter does) stays first.
    pub prioritise_direct: bool,
    /// `dd_sources`: the enabled sources, in row order.
    pub sources: Vec<Source>,
    // --- security ---
    /// `sec_always_https`: a typed `http://` address, or a bare host, loads over HTTPS.
    pub always_https: bool,
    /// `sec_strip_js`: a pasted `javascript:` prefix is removed.
    pub strip_javascript: bool,
    /// `sec_highlight_base`: only the base domain is emphasised in the unfocused address.
    pub highlight_base: bool,
    /// `domain_expansion_suffix`: Ctrl+Enter turns `name` into `www.name.<suffix>`; empty turns the
    /// shortcut off.
    pub expansion_suffix: String,
    // --- search ---
    /// `search_field`: the separate search field.
    pub search_field: SearchField,
    /// `search_show_nickname`: engines list their nickname.
    pub show_nickname: bool,
    /// `search_new_tab`: searches open in a new tab.
    pub search_new_tab: bool,
    /// `search_show_icon`: the engine's icon in the search field.
    pub show_engine_icon: bool,
    /// `search_in_address`: text that is not an address is searched for from the address field.
    pub search_in_address: bool,
    /// `search_keep_engine`: the engine chosen in the search field survives a restart.
    pub keep_engine: bool,
    /// `search_keep_text`: the search field keeps its text after a search.
    pub keep_text: bool,
    /// `search_selection_background`: "search for the selection" opens in a background tab.
    pub selection_background: bool,
    /// `search_suggest_address`: engine suggestions in the address field, only after a nickname.
    pub suggest_in_address: bool,
    /// `search_suggest_field`: engine suggestions in the search field.
    pub suggest_in_field: bool,
    /// `search_last_engine`: the id of the engine last chosen in the search field.
    pub last_engine: String,
    /// `nick_bookmarks`, `nick_history`, `nick_tabs`: scoped nickname words (without `@`); empty
    /// turns that scope off.
    pub nick_bookmarks: String,
    pub nick_history: String,
    pub nick_tabs: String,
    // --- bookmarks ---
    /// `bm_open_new_tab`: a bookmark opens in a new tab.
    pub bookmark_new_tab: bool,
    /// `bm_panel_single_click`: panel items open with one click (else two).
    pub panel_single_click: bool,
    /// `bm_confirm_open`: opening this many bookmarks at once asks first; 0 never asks.
    pub confirm_open: u8,
    /// `bm_fetch_metadata`: a new bookmark takes its title and description from the page.
    pub fetch_metadata: bool,
    /// `bm_always_dialog`: saving a bookmark always opens the bookmark dialog.
    pub always_dialog: bool,
    /// `bm_bar_folder`: id of the folder the bar shows; 0 is the top level.
    pub bar_folder: u64,
    /// `bm_bar_position`.
    pub bar_position: BarPosition,
    /// `bm_bar_display`.
    pub bar_display: BarDisplay,
    /// `bar_groups`: domains of the tab groups pinned to the bar.
    pub bar_groups: Vec<String>,
}

impl Default for AddressSettings {
    fn default() -> Self {
        Self {
            bar_show: true,
            autocomplete: true,
            show_full: false,
            select_on_activate: true,
            copy_encoded: true,
            qr_button: false,
            dropdown_while_typing: true,
            always_badge: false,
            typed_history: false,
            typed_include_searches: true,
            history_button: false,
            max_results: DEFAULT_DROPDOWN_RESULTS,
            prioritise_bookmarks: false,
            prioritise_direct: true,
            sources: Source::ALL.to_vec(),
            always_https: false,
            strip_javascript: true,
            highlight_base: false,
            expansion_suffix: "com".to_owned(),
            search_field: SearchField::Off,
            show_nickname: true,
            search_new_tab: false,
            show_engine_icon: true,
            search_in_address: true,
            keep_engine: false,
            keep_text: false,
            selection_background: false,
            suggest_in_address: true,
            suggest_in_field: true,
            last_engine: String::new(),
            nick_bookmarks: "bookmarks".to_owned(),
            nick_history: "history".to_owned(),
            nick_tabs: "tabs".to_owned(),
            bookmark_new_tab: false,
            panel_single_click: true,
            confirm_open: 10,
            fetch_metadata: true,
            always_dialog: false,
            bar_folder: 0,
            bar_position: BarPosition::Top,
            bar_display: BarDisplay::TextIcon,
            bar_groups: Vec::new(),
        }
    }
}

/// How a key's value is typed on the wire and in the page's controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Bool,
    Int,
    Text,
}

/// Every key, in file order, with its wire type.
pub const KEYS: &[(&str, Kind)] = &[
    ("address_bar_show", Kind::Bool),
    ("address_autocomplete", Kind::Bool),
    ("address_show_full", Kind::Bool),
    ("address_select_on_activate", Kind::Bool),
    ("address_copy_encoded", Kind::Bool),
    ("address_qr_button", Kind::Bool),
    ("dd_show_while_typing", Kind::Bool),
    ("dd_always_badge", Kind::Bool),
    ("dd_typed_history", Kind::Bool),
    ("dd_typed_include_searches", Kind::Bool),
    ("dd_history_button", Kind::Bool),
    ("dd_max_results", Kind::Int),
    ("dd_prioritise_bookmarks", Kind::Bool),
    ("dd_prioritise_direct", Kind::Bool),
    ("dd_sources", Kind::Text),
    ("sec_always_https", Kind::Bool),
    ("sec_strip_js", Kind::Bool),
    ("sec_highlight_base", Kind::Bool),
    ("domain_expansion_suffix", Kind::Text),
    ("search_field", Kind::Text),
    ("search_show_nickname", Kind::Bool),
    ("search_new_tab", Kind::Bool),
    ("search_show_icon", Kind::Bool),
    ("search_in_address", Kind::Bool),
    ("search_keep_engine", Kind::Bool),
    ("search_keep_text", Kind::Bool),
    ("search_selection_background", Kind::Bool),
    ("search_suggest_address", Kind::Bool),
    ("search_suggest_field", Kind::Bool),
    ("search_last_engine", Kind::Text),
    ("nick_bookmarks", Kind::Text),
    ("nick_history", Kind::Text),
    ("nick_tabs", Kind::Text),
    ("bm_open_new_tab", Kind::Bool),
    ("bm_panel_single_click", Kind::Bool),
    ("bm_confirm_open", Kind::Int),
    ("bm_fetch_metadata", Kind::Bool),
    ("bm_always_dialog", Kind::Bool),
    ("bm_bar_folder", Kind::Int),
    ("bm_bar_position", Kind::Text),
    ("bm_bar_display", Kind::Text),
    ("bar_groups", Kind::Text),
];

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn valid_nickname_word(value: &str) -> bool {
    value.is_empty()
        || (value.chars().count() <= MAX_NICKNAME_CHARS
            && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'))
}

fn valid_suffix(value: &str) -> bool {
    value.is_empty()
        || (value.chars().count() <= MAX_SUFFIX_CHARS
            && !value.starts_with(['.', '-'])
            && !value.ends_with(['.', '-'])
            && !value.contains("..")
            && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.'))
}

/// A hostname-ish token the bar's tab-group list may hold: no spaces, controls, commas or slashes.
fn valid_domain(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= MAX_DOMAIN_CHARS
        && !value.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, ',' | '/' | '\\' | '=' | '#'))
}

fn valid_engine_id(value: &str) -> bool {
    value.is_empty()
        || (value.len() <= MAX_ENGINE_ID_CHARS
            && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'))
}

fn parse_sources(value: &str) -> Option<Vec<Source>> {
    let mut seen = HashSet::new();
    let mut list = Vec::new();
    for word in value.split(',').map(str::trim).filter(|word| !word.is_empty()) {
        let source = Source::parse(word)?;
        if !seen.insert(source) {
            return None;
        }
        list.push(source);
    }
    Some(list)
}

fn parse_groups(value: &str) -> Option<Vec<String>> {
    let mut list: Vec<String> = Vec::new();
    for word in value.split(',').map(str::trim).filter(|word| !word.is_empty()) {
        let word = word.to_ascii_lowercase();
        if !valid_domain(&word) || list.contains(&word) {
            return None;
        }
        list.push(word);
    }
    (list.len() <= MAX_BAR_GROUPS).then_some(list)
}

impl AddressSettings {
    /// Sets `key` to `value`. `false` (and no change) for an unknown key or a value that is not
    /// allowed. Used by both the file parser (which ignores the result) and the settings page.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let value = value.trim();
        let flag = |slot: &mut bool| match parse_bool(value) {
            Some(on) => {
                *slot = on;
                true
            }
            None => false,
        };
        match key {
            "address_bar_show" => flag(&mut self.bar_show),
            "address_autocomplete" => flag(&mut self.autocomplete),
            "address_show_full" => flag(&mut self.show_full),
            "address_select_on_activate" => flag(&mut self.select_on_activate),
            "address_copy_encoded" => flag(&mut self.copy_encoded),
            "address_qr_button" => flag(&mut self.qr_button),
            "dd_show_while_typing" => flag(&mut self.dropdown_while_typing),
            "dd_always_badge" => flag(&mut self.always_badge),
            "dd_typed_history" => flag(&mut self.typed_history),
            "dd_typed_include_searches" => flag(&mut self.typed_include_searches),
            "dd_history_button" => flag(&mut self.history_button),
            "dd_max_results" => match value.parse::<u8>() {
                Ok(count) if (MIN_DROPDOWN_RESULTS..=MAX_DROPDOWN_RESULTS).contains(&count) => {
                    self.max_results = count;
                    true
                }
                _ => false,
            },
            "dd_prioritise_bookmarks" => flag(&mut self.prioritise_bookmarks),
            "dd_prioritise_direct" => flag(&mut self.prioritise_direct),
            "dd_sources" => match parse_sources(value) {
                Some(list) => {
                    self.sources = list;
                    true
                }
                None => false,
            },
            "sec_always_https" => flag(&mut self.always_https),
            "sec_strip_js" => flag(&mut self.strip_javascript),
            "sec_highlight_base" => flag(&mut self.highlight_base),
            "domain_expansion_suffix" => {
                let suffix = value.trim_start_matches('.').to_ascii_lowercase();
                if valid_suffix(&suffix) {
                    self.expansion_suffix = suffix;
                    true
                } else {
                    false
                }
            }
            "search_field" => match SearchField::parse(value) {
                Some(mode) => {
                    self.search_field = mode;
                    true
                }
                None => false,
            },
            "search_show_nickname" => flag(&mut self.show_nickname),
            "search_new_tab" => flag(&mut self.search_new_tab),
            "search_show_icon" => flag(&mut self.show_engine_icon),
            "search_in_address" => flag(&mut self.search_in_address),
            "search_keep_engine" => flag(&mut self.keep_engine),
            "search_keep_text" => flag(&mut self.keep_text),
            "search_selection_background" => flag(&mut self.selection_background),
            "search_suggest_address" => flag(&mut self.suggest_in_address),
            "search_suggest_field" => flag(&mut self.suggest_in_field),
            "search_last_engine" => {
                if valid_engine_id(value) {
                    self.last_engine = value.to_owned();
                    true
                } else {
                    false
                }
            }
            "nick_bookmarks" | "nick_history" | "nick_tabs" => self.set_scope_word(key, value),
            "bm_open_new_tab" => flag(&mut self.bookmark_new_tab),
            "bm_panel_single_click" => flag(&mut self.panel_single_click),
            "bm_confirm_open" => match value.parse::<u8>() {
                Ok(0) => {
                    self.confirm_open = 0;
                    true
                }
                Ok(count) if (MIN_CONFIRM_OPEN..=MAX_CONFIRM_OPEN).contains(&count) => {
                    self.confirm_open = count;
                    true
                }
                _ => false,
            },
            "bm_fetch_metadata" => flag(&mut self.fetch_metadata),
            "bm_always_dialog" => flag(&mut self.always_dialog),
            "bm_bar_folder" => match value.parse::<u64>() {
                Ok(id) => {
                    self.bar_folder = id;
                    true
                }
                Err(_) => false,
            },
            "bm_bar_position" => match BarPosition::parse(value) {
                Some(position) => {
                    self.bar_position = position;
                    true
                }
                None => false,
            },
            "bm_bar_display" => match BarDisplay::parse(value) {
                Some(display) => {
                    self.bar_display = display;
                    true
                }
                None => false,
            },
            "bar_groups" => match parse_groups(value) {
                Some(list) => {
                    self.bar_groups = list;
                    true
                }
                None => false,
            },
            _ => false,
        }
    }

    /// A scope word may be empty (the scope is off) or a short lower-case word that no other scope
    /// uses.
    fn set_scope_word(&mut self, key: &str, value: &str) -> bool {
        let word = value.trim_start_matches('@').to_ascii_lowercase();
        if !valid_nickname_word(&word) {
            return false;
        }
        let others: [&str; 2] = match key {
            "nick_bookmarks" => [&self.nick_history, &self.nick_tabs],
            "nick_history" => [&self.nick_bookmarks, &self.nick_tabs],
            _ => [&self.nick_bookmarks, &self.nick_history],
        };
        if !word.is_empty() && others.contains(&word.as_str()) {
            return false;
        }
        match key {
            "nick_bookmarks" => self.nick_bookmarks = word,
            "nick_history" => self.nick_history = word,
            _ => self.nick_tabs = word,
        }
        true
    }

    /// The value of `key` as the file and the wire write it. `None` for an unknown key.
    pub fn get(&self, key: &str) -> Option<String> {
        let flag = |on: bool| Some(if on { "true" } else { "false" }.to_owned());
        match key {
            "address_bar_show" => flag(self.bar_show),
            "address_autocomplete" => flag(self.autocomplete),
            "address_show_full" => flag(self.show_full),
            "address_select_on_activate" => flag(self.select_on_activate),
            "address_copy_encoded" => flag(self.copy_encoded),
            "address_qr_button" => flag(self.qr_button),
            "dd_show_while_typing" => flag(self.dropdown_while_typing),
            "dd_always_badge" => flag(self.always_badge),
            "dd_typed_history" => flag(self.typed_history),
            "dd_typed_include_searches" => flag(self.typed_include_searches),
            "dd_history_button" => flag(self.history_button),
            "dd_max_results" => Some(self.max_results.to_string()),
            "dd_prioritise_bookmarks" => flag(self.prioritise_bookmarks),
            "dd_prioritise_direct" => flag(self.prioritise_direct),
            "dd_sources" => Some(self.sources.iter().map(|source| source.key()).collect::<Vec<_>>().join(",")),
            "sec_always_https" => flag(self.always_https),
            "sec_strip_js" => flag(self.strip_javascript),
            "sec_highlight_base" => flag(self.highlight_base),
            "domain_expansion_suffix" => Some(self.expansion_suffix.clone()),
            "search_field" => Some(self.search_field.key().to_owned()),
            "search_show_nickname" => flag(self.show_nickname),
            "search_new_tab" => flag(self.search_new_tab),
            "search_show_icon" => flag(self.show_engine_icon),
            "search_in_address" => flag(self.search_in_address),
            "search_keep_engine" => flag(self.keep_engine),
            "search_keep_text" => flag(self.keep_text),
            "search_selection_background" => flag(self.selection_background),
            "search_suggest_address" => flag(self.suggest_in_address),
            "search_suggest_field" => flag(self.suggest_in_field),
            "search_last_engine" => Some(self.last_engine.clone()),
            "nick_bookmarks" => Some(self.nick_bookmarks.clone()),
            "nick_history" => Some(self.nick_history.clone()),
            "nick_tabs" => Some(self.nick_tabs.clone()),
            "bm_open_new_tab" => flag(self.bookmark_new_tab),
            "bm_panel_single_click" => flag(self.panel_single_click),
            "bm_confirm_open" => Some(self.confirm_open.to_string()),
            "bm_fetch_metadata" => flag(self.fetch_metadata),
            "bm_always_dialog" => flag(self.always_dialog),
            "bm_bar_folder" => Some(self.bar_folder.to_string()),
            "bm_bar_position" => Some(self.bar_position.key().to_owned()),
            "bm_bar_display" => Some(self.bar_display.key().to_owned()),
            "bar_groups" => Some(self.bar_groups.join(",")),
            _ => None,
        }
    }

    /// Every key and its current value, in file order.
    pub fn pairs(&self) -> Vec<(&'static str, String)> {
        KEYS.iter().filter_map(|(key, _)| self.get(key).map(|value| (*key, value))).collect()
    }

    /// Whether `key` is one of this module's.
    pub fn owns(key: &str) -> bool {
        KEYS.iter().any(|(name, _)| *name == key)
    }

    /// The word a scoped nickname is typed as, with its `@`, or `None` when that scope is off.
    pub fn scope_word(&self, scope: Scope) -> Option<String> {
        let word = match scope {
            Scope::Bookmarks => &self.nick_bookmarks,
            Scope::History => &self.nick_history,
            Scope::Tabs => &self.nick_tabs,
        };
        (!word.is_empty()).then(|| format!("@{word}"))
    }

    /// The scope and the rest of the input for `@word rest`, matching the word exactly (ASCII case
    /// aside) and only while that scope is on.
    pub fn parse_scope<'a>(&self, input: &'a str) -> Option<(Scope, &'a str)> {
        let text = input.trim_start();
        let at = text.strip_prefix('@')?;
        let (word, rest) = match at.split_once(char::is_whitespace) {
            Some((word, rest)) => (word, rest.trim_start()),
            None => (at, ""),
        };
        for scope in [Scope::Bookmarks, Scope::History, Scope::Tabs] {
            if self.scope_word(scope).is_some_and(|own| own[1..].eq_ignore_ascii_case(word)) {
                return Some((scope, rest));
            }
        }
        None
    }

    /// Whether opening `count` bookmarks at once asks first (`bm_confirm_open`; 0 never asks).
    pub fn confirm_needed(&self, count: usize) -> bool {
        self.confirm_open != 0 && count >= usize::from(self.confirm_open)
    }

    /// Whether `source` is enabled.
    pub fn has_source(&self, source: Source) -> bool {
        self.sources.contains(&source)
    }

    /// Reads one `key=value` line from the file, ignoring what is not understood.
    pub fn parse_line(&mut self, key: &str, value: &str) {
        let _ = self.set(key, value);
    }

    /// `text` with this module's lines replaced in place (or appended), and every other line kept.
    /// Called by [`crate::settings::render_updated`] after it wrote its own keys.
    pub fn render_into(&self, text: &str, newline: &str) -> String {
        let values = self.pairs();
        let mut written = vec![false; values.len()];
        let mut out = String::with_capacity(text.len() + 1024);
        for line in text.lines() {
            let trimmed = line.trim();
            let slot = if trimmed.starts_with('#') {
                None
            } else {
                trimmed.split_once('=').and_then(|(key, _)| values.iter().position(|(name, _)| *name == key.trim()))
            };
            match slot {
                Some(index) if written[index] => {}
                Some(index) => {
                    written[index] = true;
                    out.push_str(&format!("{}={}{newline}", values[index].0, values[index].1));
                }
                None => {
                    out.push_str(line);
                    out.push_str(newline);
                }
            }
        }
        for (index, (name, value)) in values.iter().enumerate() {
            if !written[index] {
                out.push_str(&format!("{name}={value}{newline}"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_through_get_and_set() {
        let defaults = AddressSettings::default();
        let mut copy = AddressSettings { max_results: 3, bar_show: false, ..AddressSettings::default() };
        for (key, value) in defaults.pairs() {
            assert!(copy.set(key, &value), "{key}={value}");
        }
        assert_eq!(copy, defaults);
        assert_eq!(defaults.pairs().len(), KEYS.len());
    }

    #[test]
    fn every_key_is_typed_consistently() {
        let settings = AddressSettings::default();
        for (key, kind) in KEYS {
            let value = settings.get(key).expect(key);
            match kind {
                Kind::Bool => assert!(value == "true" || value == "false", "{key}"),
                Kind::Int => assert!(value.parse::<u64>().is_ok(), "{key}"),
                Kind::Text => {}
            }
        }
        assert!(!AddressSettings::owns(""));
        assert!(AddressSettings::owns("dd_max_results"));
        assert!(!AddressSettings::owns("tab_layout"));
    }

    #[test]
    fn invalid_values_are_refused_and_change_nothing() {
        let mut settings = AddressSettings::default();
        for (key, value) in [
            ("dd_max_results", "0"),
            ("dd_max_results", "21"),
            ("dd_max_results", "x"),
            ("address_show_full", "maybe"),
            ("dd_sources", "direct,direct"),
            ("dd_sources", "direct,nonsense"),
            ("search_field", "both"),
            ("bm_bar_position", "left"),
            ("bm_bar_display", "big"),
            ("bm_confirm_open", "1"),
            ("bm_confirm_open", "101"),
            ("domain_expansion_suffix", "co m"),
            ("domain_expansion_suffix", "c/om"),
            ("domain_expansion_suffix", &"a".repeat(25)),
            ("nick_tabs", "Bad Word"),
            ("nick_tabs", "history"),
            ("search_last_engine", "../x"),
            ("bar_groups", "a.example,a.example"),
            ("bar_groups", "bad domain"),
            ("unknown_key", "true"),
        ] {
            let before = settings.clone();
            assert!(!settings.set(key, value), "{key}={value}");
            assert_eq!(settings, before, "{key}={value} changed something");
        }
        let groups = (0..40).map(|n| format!("d{n}.example")).collect::<Vec<_>>().join(",");
        assert!(!settings.set("bar_groups", &groups), "more groups than the bar holds");
    }

    #[test]
    fn values_are_normalised() {
        let mut settings = AddressSettings::default();
        assert!(settings.set("domain_expansion_suffix", ".CO.uk"));
        assert_eq!(settings.expansion_suffix, "co.uk");
        assert!(settings.set("domain_expansion_suffix", ""));
        assert_eq!(settings.expansion_suffix, "");
        assert!(settings.set("nick_bookmarks", "@Marks"));
        assert_eq!(settings.nick_bookmarks, "marks");
        assert!(settings.set("bar_groups", "A.example, b.example"));
        assert_eq!(settings.bar_groups, ["a.example", "b.example"]);
        assert!(settings.set("dd_sources", "history, direct"));
        assert_eq!(settings.sources, [Source::History, Source::Direct]);
        assert!(settings.set("dd_sources", ""));
        assert!(settings.sources.is_empty(), "every source off is allowed");
        assert!(settings.set("address_show_full", "yes"));
        assert!(settings.show_full);
        assert!(settings.set("bm_confirm_open", "0"));
        assert_eq!(settings.confirm_open, 0, "0 means never ask");
    }

    #[test]
    fn scoped_nicknames_can_be_renamed_and_turned_off() {
        let mut settings = AddressSettings::default();
        assert_eq!(settings.parse_scope("@bookmarks rust"), Some((Scope::Bookmarks, "rust")));
        assert_eq!(settings.parse_scope("  @History   news today"), Some((Scope::History, "news today")));
        assert_eq!(settings.parse_scope("@tabs"), Some((Scope::Tabs, "")));
        assert_eq!(settings.parse_scope("@tabsx foo"), None);
        assert_eq!(settings.parse_scope("bookmarks foo"), None);
        assert!(settings.set("nick_tabs", "t"));
        assert_eq!(settings.parse_scope("@t foo"), Some((Scope::Tabs, "foo")));
        assert_eq!(settings.parse_scope("@tabs foo"), None, "the old word no longer scopes");
        assert!(settings.set("nick_history", ""));
        assert_eq!(settings.scope_word(Scope::History), None);
        assert_eq!(settings.parse_scope("@history foo"), None, "an off scope is just text");
        // A word another scope uses is refused.
        assert!(!settings.set("nick_bookmarks", "t"));
        assert!(!settings.set("nick_history", "bookmarks"));
    }

    #[test]
    fn render_replaces_in_place_appends_and_keeps_everything_else() {
        let mut settings = AddressSettings::default();
        settings.max_results = 12;
        settings.search_field = SearchField::Button;
        let existing = "# c\r\ntab_layout=vertical\r\ndd_max_results=3\r\nsearch_field=off\r\nunknown=x\r\ndd_max_results=5\r\n";
        let text = settings.render_into(existing, "\r\n");
        assert!(text.starts_with("# c\r\ntab_layout=vertical\r\ndd_max_results=12\r\nsearch_field=button\r\nunknown=x\r\n"));
        assert_eq!(text.matches("dd_max_results=").count(), 1, "a repeated key keeps one line");
        assert!(text.contains("address_bar_show=true\r\n"), "missing keys are appended");
        let mut parsed = AddressSettings::default();
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                parsed.parse_line(key.trim(), value);
            }
        }
        assert_eq!(parsed, settings);
    }

    #[test]
    fn opening_many_bookmarks_asks_from_the_chosen_count_and_never_when_zero() {
        let mut settings = AddressSettings::default();
        settings.set("bm_confirm_open", "5");
        assert!(!settings.confirm_needed(4));
        assert!(settings.confirm_needed(5));
        assert!(settings.confirm_needed(50));
        settings.set("bm_confirm_open", "0");
        assert!(!settings.confirm_needed(500));
    }

    #[test]
    fn enum_keys_parse_exactly() {
        assert_eq!(SearchField::parse("field"), Some(SearchField::Field));
        assert_eq!(SearchField::parse("Field"), None);
        assert_eq!(BarDisplay::parse("bookmark-icon"), Some(BarDisplay::BookmarkIcon));
        for display in BarDisplay::ALL {
            assert_eq!(BarDisplay::parse(display.key()), Some(display));
        }
        for source in Source::ALL {
            assert_eq!(Source::parse(source.key()), Some(source));
        }
    }
}
