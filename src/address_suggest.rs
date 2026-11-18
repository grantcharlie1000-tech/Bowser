//! The address drop-down's rows: which sources contribute, in which order, how the typed text is
//! read (scoped nicknames, engine nicknames, a calculation) and what is remembered as typed.
//!
//! Pure: no Win32, no disk (the typed history has its own load and save) and no network. The
//! suggestions an engine answers with are fetched elsewhere and handed in, already parsed with
//! [`parse_suggestions`]. Which typed text may be sent for them at all is [`remote_query`].

use std::{fs, io, path::Path};

use url::Url;

use crate::{
    address_input::{self, Destination, Outcome},
    address_settings::{AddressSettings, Scope, Source},
    calc,
    omnibox::{self, Completion, History, SuggestKind, SuggestSource, Suggestion},
    search_engines::{Engine, EngineStore},
};

/// A bookmark as the drop-down sees it.
#[derive(Clone, Copy, Debug)]
pub struct MarkSource<'a> {
    pub url: &'a str,
    pub title: &'a str,
    /// The word typed to open it directly; empty when it has none.
    pub nickname: &'a str,
}

// ----- what was typed -----

pub const TYPED_LIMIT: usize = 200;
const MAX_TYPED_CHARS: usize = 512;
pub(crate) const TYPED_FILE: &str = "typed-history.txt";

/// One thing typed into the address field and submitted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Typed {
    pub text: String,
    /// A search (the text was not an address).
    pub search: bool,
}

/// What was typed into the address field, oldest first. Kept in the profile unless the window is
/// private or history is off (the caller decides; this type only stores).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TypedHistory {
    entries: Vec<Typed>,
}

impl TypedHistory {
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remembers `text`, moving an earlier identical entry to the end. Empty, over-long and
    /// control-character text is ignored. The oldest entries go past [`TYPED_LIMIT`].
    pub fn record(&mut self, text: &str, search: bool) {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > MAX_TYPED_CHARS || text.chars().any(char::is_control) {
            return;
        }
        self.entries.retain(|entry| !(entry.text == text && entry.search == search));
        self.entries.push(Typed { text: text.to_owned(), search });
        if self.entries.len() > TYPED_LIMIT {
            self.entries.remove(0);
        }
    }

    /// The newest `limit` entries, newest first.
    pub fn recent(&self, limit: usize, include_searches: bool) -> Vec<&Typed> {
        self.entries.iter().rev().filter(|entry| include_searches || !entry.search).take(limit).collect()
    }

    /// Entries that contain `needle` (ASCII case aside), newest first; prefix matches first.
    pub fn matching(&self, needle: &str, searches: Option<bool>, limit: usize) -> Vec<&Typed> {
        if needle.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<(bool, &Typed)> = self
            .entries
            .iter()
            .rev()
            .filter(|entry| searches.is_none_or(|want| entry.search == want))
            .filter_map(|entry| {
                omnibox::find_ignore_ascii_case(&entry.text, needle).map(|at| (at == 0, entry))
            })
            .collect();
        hits.sort_by_key(|(prefix, _)| !*prefix);
        hits.into_iter().map(|(_, entry)| entry).take(limit).collect()
    }

    /// Forgets one entry. `true` when it was there.
    pub fn remove(&mut self, text: &str, search: bool) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| !(entry.text == text && entry.search == search));
        self.entries.len() != before
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn serialize(&self) -> String {
        let mut out = String::from("bowser-typed-v1\n");
        for entry in &self.entries {
            out.push_str(if entry.search { "S\t" } else { "A\t" });
            out.push_str(&entry.text);
            out.push('\n');
        }
        out
    }

    pub fn parse(text: &str) -> Self {
        let mut lines = text.lines();
        let mut history = Self::default();
        if lines.next() != Some("bowser-typed-v1") {
            return history;
        }
        for line in lines {
            if let Some((kind, text)) = line.split_once('\t') {
                if kind == "S" || kind == "A" {
                    history.record(text, kind == "S");
                }
            }
        }
        history
    }

    pub fn load(root: &Path) -> Self {
        match fs::read(root.join(TYPED_FILE)) {
            Ok(bytes) if bytes.len() <= 256 * 1024 => String::from_utf8(bytes).map(|text| Self::parse(&text)).unwrap_or_default(),
            _ => Self::default(),
        }
    }

    pub fn save(&self, root: &Path) -> io::Result<()> {
        let path = root.join(TYPED_FILE);
        fs::create_dir_all(root)?;
        let temporary = path.with_extension(format!("txt.{}.tmp", std::process::id()));
        let written = fs::write(&temporary, self.serialize().as_bytes());
        if let Err(error) = written.and_then(|()| fs::rename(&temporary, &path)) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(())
    }
}

// ----- an engine's suggestion answer -----

const MAX_SUGGESTION_CHARS: usize = 200;

/// The suggestions in an engine's answer: the OpenSearch form `["query", ["a", "b"]]` (Google,
/// Bing, DuckDuckGo, Brave, Wikipedia and YouTube all answer it) or DuckDuckGo's
/// `[{"phrase": "a"}]`. At most `limit` plain-text entries; anything else gives none.
pub fn parse_suggestions(body: &str, limit: usize) -> Vec<String> {
    let Some(value) = crate::translate::json::parse(body) else { return Vec::new() };
    let Some(top) = value.as_array() else { return Vec::new() };
    // `["query", [...]]`, or a bare list of `{"phrase": ...}` objects.
    let list = match top.get(1).and_then(|second| second.as_array()) {
        Some(list) => list,
        None if top.iter().all(|item| matches!(item, crate::translate::json::Value::Object(_))) => top,
        None => return Vec::new(),
    };
    let mut out: Vec<String> = Vec::new();
    for item in list {
        let text = match item {
            crate::translate::json::Value::Str(text) => Some(text.as_str()),
            other => other.get("phrase").and_then(|phrase| phrase.as_str()),
        };
        let Some(text) = text.map(str::trim) else { continue };
        if text.is_empty() || text.chars().count() > MAX_SUGGESTION_CHARS || text.chars().any(char::is_control) || out.iter().any(|seen| seen == text) {
            continue;
        }
        out.push(text.to_owned());
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// The text of an engine's answer. UTF-8 when it is valid; otherwise Latin-1, the one other charset
/// an engine answers in (Google's `client=firefox` answer can be ISO-8859-1 for some locales), so an
/// accented suggestion is read rather than turned into U+FFFD.
pub fn decode_answer(body: Vec<u8>) -> String {
    String::from_utf8(body).unwrap_or_else(|error| error.into_bytes().iter().map(|&byte| char::from(byte)).collect())
}

// ----- search suggestions for plain typing (`search_suggestions`) -----

/// Most rows the default engine's suggestions add to the drop-down.
pub const REMOTE_LIMIT: usize = 5;
/// Longer typed text is never sent for suggestions.
pub const MAX_REMOTE_QUERY_CHARS: usize = 200;

/// Text that is, or is on its way to being, an address, a path or an internal page: never sent. A
/// single word with a dot, slash, colon, backslash or `@` (`github.co`, `localhost:80`, `C:\x`,
/// `a@b`), anything with `://`, and anything that starts with `bowser:`.
fn looks_like_address(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("://")
        || lower.starts_with("bowser:")
        || (!text.contains(char::is_whitespace) && text.contains(['.', '/', ':', '\\', '@']))
}

/// Whether submitting `text` from the address field (a normal tab) searches the default engine for
/// exactly `text`: not an address, a scoped or engine nickname, an image search or a refusal (as when
/// searching from the address field is off). Picking a search row submits its text this way, so
/// only such text may be a row.
fn is_plain_search(text: &str, settings: &AddressSettings, engines: &EngineStore) -> bool {
    match address_input::resolve(text, settings, engines, address_input::Options::default()) {
        Outcome::Open { destination: Destination::Search { query, engine, .. }, .. } => query == text && engine == engines.effective(false).id,
        _ => false,
    }
}

/// The engine and the text to ask for suggestions while `typed` is in the address field with no
/// engine nickname, or `None` when nothing may be sent: the option is off, the "Suggestions" source
/// is off, the tab is private, the text is empty, long, address-like or not a plain search, or the
/// default engine has no suggestions address (such as an engine made from `search-provider.txt`).
pub fn remote_query<'e, 't>(typed: &'t str, settings: &AddressSettings, engines: &'e EngineStore, private: bool) -> Option<(&'e Engine, &'t str)> {
    if !settings.search_suggestions || private || !settings.has_source(Source::Search) {
        return None;
    }
    let text = typed.trim();
    if text.is_empty() || text.chars().count() > MAX_REMOTE_QUERY_CHARS || text.chars().any(char::is_control) || looks_like_address(text) {
        return None;
    }
    let engine = engines.effective(false);
    if engine.suggest.is_empty() || !is_plain_search(text, settings, engines) {
        return None;
    }
    Some((engine, text))
}

/// While the answer for `query` is on its way: the suggestions answered for an earlier `previous`
/// query that `query` extends, kept only where they still start with `query` (so the list does not
/// empty and refill on every keystroke). Nothing when `query` does not extend `previous`.
pub fn carry_over(previous: &str, suggestions: &[String], query: &str) -> Vec<String> {
    let (previous, query_lower) = (previous.to_lowercase(), query.to_lowercase());
    if previous.is_empty() || query_lower.len() <= previous.len() || !query_lower.starts_with(&previous) {
        return Vec::new();
    }
    suggestions.iter().filter(|text| text.to_lowercase().starts_with(&query_lower) && text.as_str() != query).cloned().collect()
}

/// The search rows for the default engine's suggestions under plain typing: each a search for
/// exactly its text, at most [`REMOTE_LIMIT`], none that repeats the typed text or a search row
/// already listed.
fn remote_rows(ctx: &Context<'_>, listed: &[Suggestion]) -> Vec<Suggestion> {
    let typed = ctx.typed.trim();
    if remote_query(typed, ctx.settings, ctx.engines, ctx.private).is_none() {
        return Vec::new();
    }
    let typed_lower = typed.to_lowercase();
    let mut rows: Vec<Suggestion> = Vec::new();
    for text in ctx.engine_suggestions {
        let text = text.trim();
        let repeats = text.to_lowercase() == typed_lower
            || listed.iter().chain(rows.iter()).any(|row| row.kind == SuggestKind::Search && row.target.eq_ignore_ascii_case(text));
        if text.is_empty() || repeats || !is_plain_search(text, ctx.settings, ctx.engines) {
            continue;
        }
        rows.push(search_row(text, text, ""));
        if rows.len() >= REMOTE_LIMIT {
            break;
        }
    }
    rows
}

// ----- composing the rows -----

/// Everything the rows depend on.
pub struct Context<'a> {
    pub typed: &'a str,
    pub completion: Option<&'a Completion>,
    pub typed_url: Option<&'a Url>,
    pub history: &'a History,
    pub bookmarks: &'a [MarkSource<'a>],
    /// Open tabs other than the shown one.
    pub tabs: &'a [SuggestSource<'a>],
    pub typed_history: &'a TypedHistory,
    pub engines: &'a EngineStore,
    pub settings: &'a AddressSettings,
    /// Suggestions an engine answered for the current query: the nickname's engine after a nickname
    /// (see [`Context::nickname`]), else the default engine when [`remote_query`] allows plain typing.
    pub engine_suggestions: &'a [String],
    pub private: bool,
}

impl Context<'_> {
    /// The engine and query of `nickname query` typed in the address field.
    pub fn nickname(&self) -> Option<(&Engine, &str)> {
        self.engines.split_nickname(self.typed)
    }
}

/// The label a search row carries beside its text: the engine's name, and its nickname when the
/// setting asks for it.
pub fn engine_label(engine: &Engine, show_nickname: bool) -> String {
    if show_nickname && !engine.nickname.is_empty() {
        format!("{} ({})", engine.name, engine.nickname)
    } else {
        engine.name.clone()
    }
}

fn search_row(text: &str, shown: &str, label: &str) -> Suggestion {
    Suggestion {
        kind: SuggestKind::Search,
        text: text.to_owned(),
        title: shown.to_owned(),
        url: label.to_owned(),
        target: text.to_owned(),
        tab: None,
    }
}

fn page_row(kind: SuggestKind, url: &str, title: &str, tab: Option<usize>) -> Suggestion {
    let display = omnibox::display_of(url);
    Suggestion {
        kind,
        text: display.clone(),
        title: if title.trim().is_empty() { display.clone() } else { title.trim().to_owned() },
        url: display,
        target: url.to_owned(),
        tab,
    }
}

/// The default row: what Enter does for the typed text. With `Source::Direct` on it follows the
/// inline completion and a typed address; off, it is the typed text taken literally.
fn default_row(ctx: &Context<'_>) -> Option<Suggestion> {
    let typed = ctx.typed.trim();
    if typed.is_empty() {
        return None;
    }
    if let Some((engine, query)) = ctx.nickname() {
        let mut row = search_row(typed, query, &engine_label(engine, ctx.settings.show_nickname));
        row.text = typed.to_owned();
        return Some(row);
    }
    if ctx.settings.has_source(Source::Direct) {
        return omnibox::default_suggestion(typed, ctx.completion, ctx.typed_url);
    }
    omnibox::default_suggestion(typed, None, ctx.typed_url)
}

/// Rows for the text typed after a scoped nickname.
fn scoped(scope: Scope, rest: &str, ctx: &Context<'_>, max: usize) -> Vec<Suggestion> {
    let needle = omnibox::strip_typed_prefixes(rest.trim()).1;
    let mut rows = Vec::new();
    match scope {
        Scope::Bookmarks => {
            let mut scored: Vec<(u32, usize)> = ctx
                .bookmarks
                .iter()
                .enumerate()
                .filter_map(|(index, mark)| {
                    if needle.is_empty() {
                        return Some((0, index));
                    }
                    let by_nick = (!mark.nickname.is_empty() && mark.nickname.eq_ignore_ascii_case(needle)).then_some(2000);
                    by_nick.or_else(|| omnibox::match_score(mark.url, mark.title, needle)).map(|score| (score, index))
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            for (_, index) in scored.into_iter().take(max) {
                let mark = &ctx.bookmarks[index];
                rows.push(page_row(SuggestKind::Bookmark, mark.url, mark.title, None));
            }
        }
        Scope::History => {
            let sequence = ctx.history.sequence();
            let mut scored: Vec<(u32, &str, &str)> = ctx
                .history
                .iter()
                .filter_map(|(url, title, visits, last)| {
                    let recency = 150u32.saturating_sub(sequence.saturating_sub(last).min(150) as u32);
                    let base = if needle.is_empty() { Some(0) } else { omnibox::match_score(url, title, needle) };
                    base.map(|score| (score + recency + visits.min(20) * 15, url, title))
                })
                .collect();
            scored.sort_by_key(|row| std::cmp::Reverse(row.0));
            for (_, url, title) in scored.into_iter().take(max) {
                rows.push(page_row(SuggestKind::History, url, title, None));
            }
        }
        Scope::Tabs => {
            for (index, tab) in ctx.tabs.iter().enumerate() {
                if needle.is_empty() || omnibox::match_score(tab.url, tab.title, needle).is_some() {
                    rows.push(page_row(SuggestKind::Tab, tab.url, tab.title, Some(index)));
                    if rows.len() >= max {
                        break;
                    }
                }
            }
        }
    }
    rows
}

/// Rows from one source for `needle` (the typed text without its scheme and `www.`).
fn source_rows(source: Source, ctx: &Context<'_>, needle: &str, max: usize) -> Vec<Suggestion> {
    let mut rows = Vec::new();
    match source {
        Source::Direct => {}
        Source::Bookmarks => {
            let mut scored: Vec<(u32, usize)> = ctx
                .bookmarks
                .iter()
                .enumerate()
                .filter_map(|(index, mark)| omnibox::match_score(mark.url, mark.title, needle).map(|score| (score, index)))
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            for (_, index) in scored.into_iter().take(max) {
                let mark = &ctx.bookmarks[index];
                rows.push(page_row(SuggestKind::Bookmark, mark.url, mark.title, None));
            }
        }
        Source::Nicknames => {
            let word = ctx.typed.trim();
            if !word.is_empty() && !word.contains(char::is_whitespace) {
                for mark in ctx.bookmarks.iter().filter(|mark| !mark.nickname.is_empty() && mark.nickname.eq_ignore_ascii_case(word)) {
                    rows.push(page_row(SuggestKind::Bookmark, mark.url, mark.title, None));
                }
            }
        }
        Source::Search => {
            if let Some((engine, _)) = ctx.nickname() {
                let label = engine_label(engine, ctx.settings.show_nickname);
                for suggestion in ctx.engine_suggestions.iter().take(max) {
                    let full = format!("{} {suggestion}", engine.nickname);
                    rows.push(search_row(&full, suggestion, &label));
                }
            }
        }
        Source::SearchHistory => {
            for entry in ctx.typed_history.matching(needle, Some(true), max) {
                rows.push(search_row(&entry.text, &entry.text, ""));
            }
        }
        Source::History => {
            for (index, tab) in ctx.tabs.iter().enumerate() {
                if omnibox::match_score(tab.url, tab.title, needle).is_some() {
                    rows.push(page_row(SuggestKind::Tab, tab.url, tab.title, Some(index)));
                }
            }
            let sequence = ctx.history.sequence();
            let mut scored: Vec<(u32, &str, &str)> = ctx
                .history
                .iter()
                .filter_map(|(url, title, visits, last)| {
                    let recency = 150u32.saturating_sub(sequence.saturating_sub(last).min(150) as u32);
                    omnibox::match_score(url, title, needle).map(|score| (score + recency + visits.min(20) * 15, url, title))
                })
                .collect();
            scored.sort_by_key(|row| std::cmp::Reverse(row.0));
            for (_, url, title) in scored.into_iter().take(max) {
                rows.push(page_row(SuggestKind::History, url, title, None));
            }
        }
        Source::Calculator => {
            if let Some(result) = calc::evaluate(ctx.typed) {
                rows.push(Suggestion {
                    kind: SuggestKind::Calc,
                    text: result.clone(),
                    title: format!("= {result}"),
                    url: ctx.typed.trim().to_owned(),
                    target: result,
                    tab: None,
                });
            }
        }
    }
    rows
}

/// Whether `row` is a known page whose host is exactly the typed text: the direct match that
/// `prioritise direct match` lifts under the default row.
fn is_direct_match(row: &Suggestion, needle: &str) -> bool {
    if !matches!(row.kind, SuggestKind::Bookmark | SuggestKind::History | SuggestKind::Tab) || needle.is_empty() {
        return false;
    }
    let body = omnibox::strip_typed_prefixes(&row.target).1;
    let host = omnibox::host_part(body);
    host.eq_ignore_ascii_case(needle.trim_end_matches('/'))
}

/// The drop-down rows for `ctx.typed`: the default row first, then the enabled sources in the order
/// of `dd_sources` (a priority option lifts bookmarks or the direct match), each page once, at most
/// `dd_max_results` rows in all. A scoped nickname (`@bookmarks rust`) shows only that scope.
pub fn compose(ctx: &Context<'_>) -> Vec<Suggestion> {
    let settings = ctx.settings;
    let max = usize::from(settings.max_results).max(1);
    let typed = ctx.typed.trim();
    if typed.is_empty() {
        return Vec::new();
    }
    if let Some((scope, rest)) = settings.parse_scope(typed) {
        // The first row stays what Enter does: the first result.
        return scoped(scope, rest, ctx, max);
    }
    let needle = omnibox::strip_typed_prefixes(typed).1;
    let mut rows: Vec<Suggestion> = default_row(ctx).into_iter().collect();
    if needle.is_empty() || needle.len() > 2048 {
        rows.truncate(max);
        return rows;
    }
    // Rows of the engine nickname's query are matched on the query, not on the nickname.
    let search_needle = ctx.nickname().map_or(needle, |(_, query)| query);

    let mut groups: Vec<(Source, Vec<Suggestion>)> = settings
        .sources
        .iter()
        .filter(|source| **source != Source::Direct)
        .map(|source| {
            let for_needle = if matches!(source, Source::Search | Source::Calculator | Source::Nicknames) { needle } else { search_needle };
            (*source, source_rows(*source, ctx, for_needle, max))
        })
        .collect();
    // Priorities: bookmarks first, then the direct match right under the default row.
    if settings.prioritise_bookmarks {
        if let Some(at) = groups.iter().position(|(source, _)| matches!(source, Source::Bookmarks)) {
            let group = groups.remove(at);
            groups.insert(0, group);
        }
    }
    let mut flat: Vec<Suggestion> = groups.into_iter().flat_map(|(_, rows)| rows).collect();
    if settings.prioritise_direct {
        if let Some(at) = flat.iter().position(|row| is_direct_match(row, needle)) {
            let row = flat.remove(at);
            flat.insert(0, row);
        }
    }
    // One row per page: the first kept, except that an open tab takes over a bookmark's or a
    // history row's place for the same page.
    let key_of = |row: &Suggestion| omnibox::dedupe_key(&row.target).to_ascii_lowercase();
    let mut taken: Vec<(String, SuggestKind)> = rows.iter().filter(|row| row.kind != SuggestKind::Search && row.kind != SuggestKind::Calc).map(|row| (key_of(row), row.kind)).collect();
    let mut kept: Vec<Suggestion> = Vec::new();
    for row in flat {
        if matches!(row.kind, SuggestKind::Search | SuggestKind::Calc) {
            if !rows.iter().chain(kept.iter()).any(|other| other.kind == row.kind && other.target == row.target) {
                kept.push(row);
            }
            continue;
        }
        let key = key_of(&row);
        match taken.iter().position(|(seen, _)| *seen == key) {
            None => {
                taken.push((key, row.kind));
                kept.push(row);
            }
            Some(at) => {
                if row.kind == SuggestKind::Tab && taken[at].1 != SuggestKind::Tab {
                    if let Some(slot) = kept.iter_mut().find(|other| key_of(other) == key) {
                        taken[at].1 = SuggestKind::Tab;
                        *slot = row;
                    }
                }
            }
        }
    }
    rows.extend(kept);
    // The default engine's suggestions come last, below every local row, and may take up to half
    // the list: the local rows give way to them only that far.
    let remote = if ctx.nickname().is_none() { remote_rows(ctx, &rows) } else { Vec::new() };
    rows.truncate(max - remote.len().min(max / 2));
    let room = max - rows.len();
    rows.extend(remote.into_iter().take(room));
    rows
}

/// Rows for the typed-history drop-down (an empty field was focused, or the history button was
/// pressed): what was typed before, newest first.
pub fn typed_rows(history: &TypedHistory, settings: &AddressSettings) -> Vec<Suggestion> {
    history
        .recent(usize::from(settings.max_results).max(1), settings.typed_include_searches)
        .into_iter()
        .map(|entry| {
            if entry.search {
                search_row(&entry.text, &entry.text, "")
            } else {
                page_row(SuggestKind::History, &entry.text, &entry.text, None)
            }
        })
        .map(|mut row| {
            // A typed address is submitted as typed, not as the display form.
            row.target = row.title.clone();
            row.text = row.title.clone();
            row
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address_settings::AddressSettings;

    fn history_of(entries: &[(&str, &str)]) -> History {
        let mut history = History::default();
        for (url, title) in entries {
            history.record(url, title);
        }
        history
    }

    struct Fixture {
        history: History,
        marks: Vec<(String, String, String)>,
        tabs: Vec<(String, String)>,
        typed: TypedHistory,
        engines: EngineStore,
        settings: AddressSettings,
        suggestions: Vec<String>,
        private: bool,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                history: history_of(&[
                    ("https://news.example/", "The Daily Paper"),
                    ("https://rust-lang.org/", "Rust"),
                    ("https://docs.rs/", "Docs.rs"),
                ]),
                marks: vec![
                    ("https://www.rust-lang.org/".into(), "Rust Language".into(), "rl".into()),
                    ("https://example.com/".into(), "Example".into(), String::new()),
                ],
                tabs: vec![("https://docs.rs/".into(), "Docs tab".into())],
                typed: TypedHistory::default(),
                engines: EngineStore::default(),
                settings: AddressSettings::default(),
                suggestions: Vec::new(),
                private: false,
            }
        }

        fn rows(&self, typed: &str) -> Vec<Suggestion> {
            let marks: Vec<MarkSource<'_>> = self.marks.iter().map(|(url, title, nickname)| MarkSource { url, title, nickname }).collect();
            let tabs: Vec<SuggestSource<'_>> = self.tabs.iter().map(|(url, title)| SuggestSource { url, title }).collect();
            let url = crate::navigation::resolve_input(typed, &crate::navigation::SearchProvider::from_config(None, Path::new("missing.txt")))
                .ok()
                .and_then(|target| match target {
                    crate::navigation::NavigationTarget::Web(url) => Some(url),
                    _ => None,
                });
            let ctx = Context {
                typed,
                completion: None,
                typed_url: url.as_ref(),
                history: &self.history,
                bookmarks: &marks,
                tabs: &tabs,
                typed_history: &self.typed,
                engines: &self.engines,
                settings: &self.settings,
                engine_suggestions: &self.suggestions,
                private: self.private,
            };
            compose(&ctx)
        }
    }

    fn kinds(rows: &[Suggestion]) -> Vec<SuggestKind> {
        rows.iter().map(|row| row.kind).collect()
    }

    #[test]
    fn typed_history_dedupes_caps_and_round_trips() {
        let mut history = TypedHistory::default();
        history.record("rust", true);
        history.record("example.com", false);
        history.record("rust", true);
        history.record("  ", true);
        history.record("a\u{7}b", true);
        assert_eq!(history.len(), 2);
        assert_eq!(history.recent(10, true).iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["rust", "example.com"]);
        assert_eq!(history.recent(10, false).iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["example.com"]);
        assert_eq!(history.recent(1, true).len(), 1);
        for n in 0..(TYPED_LIMIT + 20) {
            history.record(&format!("q{n}"), true);
        }
        assert_eq!(history.len(), TYPED_LIMIT);
        assert!(history.recent(1000, true).iter().all(|entry| entry.text != "rust"), "the oldest went first");
        let back = TypedHistory::parse(&history.serialize());
        assert_eq!(back, history);
        assert_eq!(TypedHistory::parse("nonsense\nS\tx\n"), TypedHistory::default(), "no header, no history");
        let mut small = TypedHistory::default();
        small.record("alpha beta", true);
        small.record("beta", true);
        small.record("gamma", true);
        let found: Vec<&str> = small.matching("beta", Some(true), 5).iter().map(|e| e.text.as_str()).collect();
        assert_eq!(found, ["beta", "alpha beta"], "prefix matches first");
        assert!(small.remove("gamma", true) && !small.remove("gamma", true));
        small.clear();
        assert!(small.is_empty());
    }

    #[test]
    fn typed_history_survives_a_file() {
        let dir = std::env::temp_dir().join(format!("bowser-typed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut history = TypedHistory::default();
        history.record("rust lang", true);
        history.record("https://a.example/x", false);
        history.save(&dir).unwrap();
        assert_eq!(TypedHistory::load(&dir), history);
        assert_eq!(TypedHistory::load(&dir.join("missing")), TypedHistory::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn engine_answers_in_both_shapes_parse_and_hostile_ones_do_not() {
        assert_eq!(parse_suggestions(r#"["rust",["rust lang","rustup","rust lang"]]"#, 5), ["rust lang", "rustup"]);
        assert_eq!(parse_suggestions(r#"["q",["a","b","c"],[],{"google:suggesttype":["QUERY"]}]"#, 2), ["a", "b"]);
        assert_eq!(parse_suggestions(r#"[{"phrase":"one"},{"phrase":"two"},{"x":1}]"#, 5), ["one", "two"]);
        for bad in ["", "null", "{}", r#"["q"]"#, r#"["q",[1,2]]"#, "[[[[[[1]]]]]]", "not json", r#"["q",["a\u0007b"]]"#] {
            assert!(parse_suggestions(bad, 5).is_empty(), "{bad}");
        }
        let long = format!(r#"["q",["{}"]]"#, "x".repeat(300));
        assert!(parse_suggestions(&long, 5).is_empty(), "an over-long entry is dropped");
    }

    #[test]
    fn default_row_comes_first_and_sources_follow_in_order() {
        let fixture = Fixture::new();
        let rows = fixture.rows("rust");
        assert_eq!(rows[0].kind, SuggestKind::Search);
        assert_eq!(rows[0].text, "rust");
        assert!(rows.iter().any(|row| row.kind == SuggestKind::Bookmark && row.target == "https://www.rust-lang.org/"));
        assert_eq!(
            rows.iter().filter(|row| row.target.contains("rust-lang.org")).count(),
            1,
            "the bookmark and the history entry for one page show once"
        );
        let first_bookmark = rows.iter().position(|row| row.kind == SuggestKind::Bookmark);
        let first_history = rows.iter().position(|row| row.kind == SuggestKind::History);
        assert!(first_bookmark < first_history || first_history.is_none(), "the default order lists bookmarks before history");
    }

    #[test]
    fn source_order_is_the_row_order_and_a_disabled_source_gives_no_rows() {
        let mut fixture = Fixture::new();
        fixture.history = history_of(&[("https://news.example/rustacean", "Rust news")]);
        fixture.settings.set("dd_sources", "history,bookmarks");
        let rows = fixture.rows("rust");
        assert_eq!(kinds(&rows), [SuggestKind::Search, SuggestKind::History, SuggestKind::Bookmark]);
        fixture.settings.set("dd_sources", "bookmarks");
        assert_eq!(kinds(&fixture.rows("rust")), [SuggestKind::Search, SuggestKind::Bookmark]);
        fixture.settings.set("dd_sources", "");
        assert_eq!(kinds(&fixture.rows("rust")), [SuggestKind::Search], "no sources, only the default row");
    }

    #[test]
    fn prioritising_bookmarks_lifts_them_over_an_earlier_source() {
        let mut fixture = Fixture::new();
        fixture.history = history_of(&[("https://news.example/rustacean", "Rust news")]);
        fixture.settings.set("dd_sources", "history,bookmarks");
        fixture.settings.set("dd_prioritise_bookmarks", "true");
        let rows = fixture.rows("rust");
        assert_eq!(kinds(&rows), [SuggestKind::Search, SuggestKind::Bookmark, SuggestKind::History]);
    }

    #[test]
    fn prioritising_the_direct_match_lifts_the_page_whose_host_was_typed() {
        let mut fixture = Fixture::new();
        fixture.history = history_of(&[("https://en.wikipedia.org/wiki/Rust", "Rust article")]);
        fixture.marks = vec![("http://wiki/".into(), "Team wiki".into(), String::new())];
        fixture.tabs.clear();
        fixture.settings.set("dd_sources", "history,bookmarks");
        fixture.settings.set("dd_prioritise_direct", "false");
        let plain = fixture.rows("wiki");
        assert_eq!(plain[1].target, "https://en.wikipedia.org/wiki/Rust", "history is listed first");
        fixture.settings.set("dd_prioritise_direct", "true");
        let lifted = fixture.rows("wiki");
        assert_eq!(lifted[0].kind, SuggestKind::Search, "the default row stays first");
        assert_eq!(lifted[1].target, "http://wiki/", "the page whose host is the typed text comes next");
        assert_eq!(plain.len(), lifted.len());
    }

    #[test]
    fn disabling_direct_match_takes_the_completion_out_of_the_default_row() {
        let fixture = Fixture::new();
        let marks: Vec<MarkSource<'_>> = Vec::new();
        let completion = Completion { full_url: "https://rust-lang.org/".into(), completed_suffix: "-lang.org/".into() };
        let mut settings = fixture.settings.clone();
        let build = |settings: &AddressSettings| {
            let ctx = Context {
                typed: "rust",
                completion: Some(&completion),
                typed_url: None,
                history: &fixture.history,
                bookmarks: &marks,
                tabs: &[],
                typed_history: &fixture.typed,
                engines: &fixture.engines,
                settings,
                engine_suggestions: &[],
                private: false,
            };
            compose(&ctx)
        };
        assert_eq!(build(&settings)[0].kind, SuggestKind::Url, "direct match: the completed page");
        settings.set("dd_sources", "history");
        assert_eq!(build(&settings)[0].kind, SuggestKind::Search, "without it the typed text is a search");
    }

    #[test]
    fn the_result_limit_applies_to_all_rows() {
        let mut fixture = Fixture::new();
        fixture.history = {
            let mut history = History::default();
            for n in 0..30 {
                history.record(&format!("https://site{n}.example/"), "");
            }
            history
        };
        for limit in [1u8, 3, 8, 20] {
            fixture.settings.set("dd_max_results", &limit.to_string());
            assert_eq!(fixture.rows("site").len(), usize::from(limit), "limit {limit}");
        }
    }

    #[test]
    fn an_open_tab_takes_over_the_row_of_its_page() {
        let mut fixture = Fixture::new();
        fixture.marks.push(("https://docs.rs/".into(), "Docs bookmark".into(), String::new()));
        let rows = fixture.rows("docs");
        let docs: Vec<&Suggestion> = rows.iter().filter(|row| row.target == "https://docs.rs/").collect();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].kind, SuggestKind::Tab);
        assert_eq!(docs[0].tab, Some(0));
    }

    #[test]
    fn bookmark_nicknames_open_the_bookmark_when_typed_exactly() {
        let mut fixture = Fixture::new();
        fixture.settings.set("dd_sources", "nicknames");
        let rows = fixture.rows("rl");
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[1].kind, rows[1].target.as_str()), (SuggestKind::Bookmark, "https://www.rust-lang.org/"));
        assert_eq!(fixture.rows("r").len(), 1, "a nickname must match whole");
        fixture.settings.set("dd_sources", "bookmarks");
        assert!(fixture.rows("rl").iter().all(|row| row.kind != SuggestKind::Bookmark), "with the source off the nickname does nothing");
    }

    #[test]
    fn an_engine_nickname_turns_the_default_row_into_that_engine_s_search_and_adds_its_suggestions() {
        let mut fixture = Fixture::new();
        fixture.marks.clear();
        fixture.history = History::default();
        fixture.suggestions = vec!["rust lang".into(), "rust book".into()];
        let rows = fixture.rows("g rust");
        assert_eq!(rows[0].kind, SuggestKind::Search);
        assert_eq!((rows[0].title.as_str(), rows[0].url.as_str()), ("rust", "Google (g)"));
        assert_eq!(rows[0].target, "g rust", "the engine is resolved again on submit");
        assert_eq!((rows[1].title.as_str(), rows[1].target.as_str()), ("rust lang", "g rust lang"));
        fixture.settings.set("search_show_nickname", "false");
        assert_eq!(fixture.rows("g rust")[0].url, "Google");
        fixture.settings.set("dd_sources", "bookmarks");
        assert_eq!(fixture.rows("g rust").len(), 1, "with the search source off there are no engine suggestions");
        // Without a nickname the engine's suggestions are never shown.
        let mut plain = Fixture::new();
        plain.suggestions = vec!["zzz".into()];
        assert!(plain.rows("rust").iter().all(|row| row.title != "zzz"));
    }

    #[test]
    fn search_history_and_calculator_rows() {
        let mut fixture = Fixture::new();
        fixture.typed.record("rust ownership", true);
        fixture.typed.record("rust.example", false);
        let rows = fixture.rows("rust");
        let searches: Vec<&str> = rows.iter().filter(|row| row.kind == SuggestKind::Search).map(|row| row.text.as_str()).collect();
        assert_eq!(searches, ["rust", "rust ownership"], "a past search offered once, an address typed before is not a search");
        let sum = fixture.rows("2+2*3");
        assert_eq!(sum[1].kind, SuggestKind::Calc);
        assert_eq!((sum[1].title.as_str(), sum[1].target.as_str(), sum[1].url.as_str()), ("= 8", "8", "2+2*3"));
        fixture.settings.set("dd_sources", "history");
        assert!(fixture.rows("2+2*3").iter().all(|row| row.kind != SuggestKind::Calc), "the calculator can be turned off");
    }

    #[test]
    fn scoped_nicknames_show_only_their_own_scope() {
        let mut fixture = Fixture::new();
        let bookmarks = fixture.rows("@bookmarks rust");
        assert_eq!(kinds(&bookmarks), [SuggestKind::Bookmark]);
        assert_eq!(bookmarks[0].target, "https://www.rust-lang.org/");
        let all_marks = fixture.rows("@bookmarks");
        assert_eq!(all_marks.len(), 2, "a bare scope lists the scope");
        let history = fixture.rows("@history daily");
        assert_eq!(kinds(&history), [SuggestKind::History]);
        assert_eq!(history[0].title, "The Daily Paper");
        let tabs = fixture.rows("@tabs doc");
        assert_eq!((tabs[0].kind, tabs[0].tab), (SuggestKind::Tab, Some(0)));
        assert!(fixture.rows("@tabs zzz").is_empty());
        // Renamed, off.
        fixture.settings.set("nick_tabs", "t");
        assert_eq!(fixture.rows("@t doc").len(), 1);
        assert_eq!(fixture.rows("@tabs doc")[0].kind, SuggestKind::Search, "the old word is only text now");
        fixture.settings.set("nick_history", "");
        assert_eq!(fixture.rows("@history daily")[0].kind, SuggestKind::Search);
        // A bookmark's nickname finds it inside the scope.
        assert_eq!(fixture.rows("@bookmarks rl")[0].target, "https://www.rust-lang.org/");
    }

    #[test]
    fn typed_rows_list_newest_first_and_honour_the_search_option() {
        let mut history = TypedHistory::default();
        history.record("https://a.example/", false);
        history.record("my search", true);
        let mut settings = AddressSettings::default();
        let rows = typed_rows(&history, &settings);
        assert_eq!(rows.iter().map(|row| row.text.as_str()).collect::<Vec<_>>(), ["my search", "https://a.example/"]);
        assert_eq!(rows[0].kind, SuggestKind::Search);
        assert_eq!(rows[1].kind, SuggestKind::History);
        assert_eq!(rows[1].target, "https://a.example/", "an address is submitted as typed");
        settings.set("dd_typed_include_searches", "false");
        assert_eq!(typed_rows(&history, &settings).len(), 1);
        settings.set("dd_typed_include_searches", "true");
        settings.set("dd_max_results", "1");
        assert_eq!(typed_rows(&history, &settings).len(), 1);
    }

    // ----- search suggestions for plain typing -----

    fn on() -> AddressSettings {
        AddressSettings { search_suggestions: true, ..AddressSettings::default() }
    }

    #[test]
    fn plain_typing_is_sent_only_with_the_option_on_and_never_in_private() {
        let engines = EngineStore::default();
        assert!(remote_query("rust", &AddressSettings::default(), &engines, false).is_none(), "off by default");
        let (engine, query) = remote_query("  rust lang ", &on(), &engines, false).unwrap();
        assert_eq!((engine.id.as_str(), query), ("optimised", "rust lang"), "the default engine, the trimmed text");
        assert!(remote_query("rust", &on(), &engines, true).is_none(), "never from a private tab");
        let mut no_source = on();
        assert!(no_source.set("dd_sources", "direct,bookmarks,history"));
        assert!(remote_query("rust", &no_source, &engines, false).is_none(), "the Suggestions source is off");
        let mut no_search = on();
        no_search.search_in_address = false;
        assert!(remote_query("rust", &no_search, &engines, false).is_none(), "the address field does not search");
        assert!(remote_query(&"a".repeat(MAX_REMOTE_QUERY_CHARS + 1), &on(), &engines, false).is_none(), "too long");
        assert!(remote_query(&"a".repeat(MAX_REMOTE_QUERY_CHARS), &on(), &engines, false).is_some());
    }

    #[test]
    fn addresses_internal_pages_nicknames_and_empty_text_are_never_sent() {
        let engines = EngineStore::default();
        for typed in [
            "", "   ", "github.com", "github.co", "news.example/today", "https://example.com/a b", "http://x", "bowser://settings",
            "bowser:", "BOWSER://history", "bowser: settings", "localhost:8080", "C:\\Users\\me", "me@example", "192.168.0.1",
            "g rust", "ddg rust", "@history rust", "@bookmarks", "img cats", "javascript:alert(1)", "about:blank", "a\u{7}b", "localhost",
        ] {
            assert!(remote_query(typed, &on(), &engines, false).is_none(), "{typed:?} was sent");
        }
        for typed in ["rust", "weather in paris", "Note: buy milk", "what is 2+2", "intranet"] {
            assert!(remote_query(typed, &on(), &engines, false).is_some(), "{typed:?} was not sent");
        }
    }

    #[test]
    fn the_effective_engine_decides_where_suggestions_come_from() {
        let mut engines = EngineStore::default();
        for (id, host) in [("google", "suggestqueries.google.com"), ("duckduckgo", "duckduckgo.com"), ("bing", "api.bing.com")] {
            engines.set_default(id).unwrap();
            let (engine, query) = remote_query("rust", &on(), &engines, false).unwrap();
            assert_eq!(engine.id, id);
            assert_eq!(engine.suggest_url(query).unwrap().host_str(), Some(host), "{id}");
        }
        // A private-window engine never matters: private tabs send nothing.
        engines.set_private("google").unwrap();
        assert!(remote_query("rust", &on(), &engines, true).is_none());
        // `search-provider.txt` makes an engine with no suggestions address: nothing is sent.
        let legacy = EngineStore::load(Path::new("missing-dir-for-search-suggestion-test"), None, Some("https://search.example/?q={query}"));
        assert_eq!(legacy.effective(false).id, "custom");
        assert!(remote_query("rust", &on(), &legacy, false).is_none());
    }

    #[test]
    fn suggestion_addresses_encode_the_query_as_one_value() {
        let engines = EngineStore::default();
        let url = |id: &str, query: &str| engines.find(id).unwrap().suggest_url(query).unwrap().to_string();
        assert_eq!(url("google", "a b&c=d#e"), "https://suggestqueries.google.com/complete/search?client=firefox&q=a%20b%26c%3Dd%23e");
        assert_eq!(url("duckduckgo", "caf\u{e9} +1"), "https://duckduckgo.com/ac/?q=caf%C3%A9%20%2B1&type=list");
        assert_eq!(url("bing", "100%"), "https://api.bing.com/osjson.aspx?query=100%25");
    }

    #[test]
    fn engine_answers_from_the_three_providers_parse() {
        // Google (client=firefox), DuckDuckGo (type=list) and Bing (osjson) all answer `[query, [..]]`.
        let google = r#"["rust",["rust","rust game","rust lang","rustup"],[],{"google:suggestsubtypes":[[512]]}]"#;
        assert_eq!(parse_suggestions(google, REMOTE_LIMIT), ["rust", "rust game", "rust lang", "rustup"]);
        assert_eq!(parse_suggestions(r#"["rust",["rust","rust lang"]]"#, REMOTE_LIMIT), ["rust", "rust lang"]);
        assert!(parse_suggestions(r#"["rust",[]]"#, REMOTE_LIMIT).is_empty());
        let long = format!(r#"["q",["{}","ok"]]"#, "x".repeat(201));
        assert_eq!(parse_suggestions(&long, REMOTE_LIMIT), ["ok"], "an over-long suggestion is dropped");
        let many = format!("[\"q\",[{}]]", (0..20).map(|n| format!("\"q{n}\"")).collect::<Vec<_>>().join(","));
        assert_eq!(parse_suggestions(&many, REMOTE_LIMIT).len(), REMOTE_LIMIT);
        for hostile in ["", "null", "<html>", r#"{"q":["a"]}"#, r#"["q","a"]"#, r#"["q",[1,2]]"#] {
            assert!(parse_suggestions(hostile, REMOTE_LIMIT).is_empty(), "{hostile}");
        }
    }

    fn texts(rows: &[Suggestion]) -> Vec<&str> {
        rows.iter().map(|row| row.title.as_str()).collect()
    }

    #[test]
    fn engine_suggestions_come_below_the_local_rows_as_searches() {
        let mut fixture = Fixture::new();
        fixture.settings = on();
        fixture.suggestions = ["Rust", "rust lang", "g rust", "rust-lang.org", "img rust", "rustup", "rust lang", "rust game", "rust book", "rust 2024", "rust x"]
            .map(str::to_owned)
            .to_vec();
        let rows = fixture.rows("rust");
        assert_eq!(rows[0].kind, SuggestKind::Search, "the default row is first");
        let first_remote = rows.iter().skip(1).position(|row| row.kind == SuggestKind::Search).map(|at| at + 1).unwrap();
        assert!(first_remote > 1, "local rows come first: {:?}", texts(&rows));
        assert!(rows[1..first_remote].iter().all(|row| row.kind != SuggestKind::Search));
        let remote = &rows[first_remote..];
        assert!(remote.iter().all(|row| row.kind == SuggestKind::Search && row.target == row.title && row.text == row.title && row.url.is_empty()));
        // The typed text again, a nickname, an address, an image search and a repeat are left out.
        assert_eq!(texts(remote), ["rust lang", "rustup", "rust game", "rust book", "rust 2024"][..remote.len()]);
        assert!(rows.len() <= usize::from(fixture.settings.max_results));
        assert!(remote.len() >= 4, "they take up to half the list: {:?}", texts(&rows));

        // With room to spare they fill it, up to the limit.
        fixture.settings.max_results = 20;
        let rows = fixture.rows("rust");
        let remote: Vec<&Suggestion> = rows.iter().skip(1).filter(|row| row.kind == SuggestKind::Search).collect();
        assert_eq!(remote.len(), REMOTE_LIMIT);
        assert_eq!(rows.last().unwrap().title, "rust 2024");

        // A short list keeps the default row and gives the suggestions at most half.
        fixture.settings.max_results = 1;
        assert_eq!(kinds(&fixture.rows("rust")), [SuggestKind::Search]);
        fixture.settings.max_results = 4;
        let rows = fixture.rows("rust");
        assert_eq!(rows.len(), 4);
        assert_eq!(texts(&rows[2..]), ["rust lang", "rustup"]);
    }

    #[test]
    fn engine_suggestions_are_not_listed_when_they_may_not_be_asked_for() {
        let mut fixture = Fixture::new();
        fixture.suggestions = vec!["rust lang".to_owned()];
        let local = fixture.rows("rust");
        assert!(texts(&local).iter().all(|text| *text != "rust lang"), "off: never listed");
        fixture.settings = on();
        fixture.private = true;
        assert_eq!(fixture.rows("rust"), local, "private: never listed");
        fixture.private = false;
        assert!(texts(&fixture.rows("rust")).contains(&"rust lang"));
        assert!(!texts(&fixture.rows("rust-lang.org")).contains(&"rust lang"), "address-like text lists none");
        // A search row already listed (a typed search) is not repeated.
        fixture.typed.record("rust lang", true);
        let rows = fixture.rows("rust");
        assert_eq!(rows.iter().filter(|row| row.target.eq_ignore_ascii_case("rust lang")).count(), 1);
    }

    #[test]
    fn answers_are_read_as_utf8_or_else_latin1() {
        assert_eq!(decode_answer("[\"caf\u{e9}\",[\"caf\u{e9} au lait\"]]".as_bytes().to_vec()), "[\"caf\u{e9}\",[\"caf\u{e9} au lait\"]]");
        let latin1 = b"[\"caf\xe9\",[\"caf\xe9 au lait\"]]".to_vec();
        assert_eq!(parse_suggestions(&decode_answer(latin1), REMOTE_LIMIT), ["caf\u{e9} au lait"]);
    }

    #[test]
    fn earlier_answers_carry_over_only_while_they_still_match() {
        let earlier: Vec<String> = ["rust lang", "Rustup", "rust", "ruby"].map(str::to_owned).to_vec();
        assert_eq!(carry_over("ru", &earlier, "rust"), ["rust lang", "Rustup"]);
        assert_eq!(carry_over("RU", &earlier, "rus"), ["rust lang", "Rustup", "rust"]);
        assert!(carry_over("rust", &earlier, "rust").is_empty(), "the same query is the answer itself, not a carry-over");
        assert!(carry_over("rust", &earlier, "rus").is_empty(), "deleting does not carry over");
        assert!(carry_over("go", &earlier, "rust").is_empty(), "another query does not carry over");
        assert!(carry_over("", &earlier, "rust").is_empty());
    }
}
