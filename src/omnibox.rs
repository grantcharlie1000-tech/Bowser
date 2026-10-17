//! Session history and inline completion for the omnibox (see docs/superpowers/specs/2026-10-09-bowser-glass-ui-design.md).
//!
//! Pure: no Win32. History lives in memory for the session only. URL-or-search classification is
//! `navigation::resolve_input`, not repeated here.

use std::time::{Duration, Instant};
use url::Url;

pub const HISTORY_LIMIT: usize = 500;
/// Longer URLs are ignored before parsing, so one oversized page cannot take up the history.
const MAX_URL_BYTES: usize = 2048;
const MAX_TITLE_CHARS: usize = 256;
const HTTP_PREFIX: &str = "http://";
const HTTPS_PREFIX: &str = "https://";
const WWW_PREFIX: &str = "www.";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    /// Fragment-free URL. This is the dedupe key.
    url: String,
    /// Omnibox form, see [`display_form`]. Matching runs on this string, so no allocation is needed per keystroke.
    display: String,
    title: String,
    visits: u32,
    /// Sequence number of the most recent visit. Higher means more recent.
    last_visit: u64,
}

#[derive(Debug, Default)]
pub struct History {
    entries: Vec<Entry>,
    sequence: u64,
}

impl History {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every entry as (address, title, visits, sequence of the last visit), for
    /// `address_suggest`. Order is unspecified.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &str, u32, u64)> {
        self.entries.iter().map(|entry| (entry.url.as_str(), entry.title.as_str(), entry.visits, entry.last_visit))
    }

    /// The sequence number of the most recent visit; `last_visit` values count up to it.
    pub(crate) fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Records a visit. Only credential-free http(s) URLs of at most `MAX_URL_BYTES` are kept, and
    /// titles are cut to `MAX_TITLE_CHARS`. A repeat visit increments the count, and a non-empty
    /// title replaces the stored one. When the history is full, the entry with the fewest visits
    /// (then the oldest) is dropped.
    pub fn record(&mut self, url: &str, title: &str) {
        if url.len() > MAX_URL_BYTES {
            return;
        }
        let Ok(mut parsed) = Url::parse(url) else {
            return;
        };
        if !matches!(parsed.scheme(), "http" | "https")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return;
        }
        parsed.set_fragment(None);
        self.sequence = self.sequence.saturating_add(1);
        let title: String = title.trim().chars().take(MAX_TITLE_CHARS).collect();
        let key = parsed.as_str();

        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.url == key) {
            entry.visits = entry.visits.saturating_add(1);
            entry.last_visit = self.sequence;
            if !title.is_empty() {
                entry.title = title;
            }
            return;
        }

        if self.entries.len() >= HISTORY_LIMIT {
            let least_valuable = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| (entry.visits, entry.last_visit))
                .map(|(index, _)| index);
            if let Some(index) = least_valuable {
                self.entries.swap_remove(index);
            }
        }
        self.entries.push(Entry {
            url: key.to_owned(),
            display: display_form(&parsed),
            title,
            visits: 1,
            last_visit: self.sequence,
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// The stored URL, for example `https://www.wikipedia.org/`.
    pub full_url: String,
    /// Text to append after the typed input, for example `pedia.org/` after `wiki`.
    pub completed_suffix: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchemeFilter {
    Any,
    Http,
    Https,
}

/// Returns the best completion for `typed`, or `None`.
///
/// Matching runs on the omnibox form, so `https://`, `www.` and case are ignored. Completion is
/// suppressed after Backspace or Delete (`allow == false`), for empty or whitespace input, and when
/// the typed host already has a dot: the suggestion then extends only the last label, so it never
/// adds a registrable-looking domain. Among matches, the most visited wins, then the most recent.
pub fn inline_completion(history: &History, typed: &str, allow: bool) -> Option<Completion> {
    if !allow || !AUTOCOMPLETE.with(std::cell::Cell::get) {
        return None;
    }
    let (scheme, needle) = strip_typed_prefixes(typed);
    if needle.is_empty() || needle.contains(|ch: char| ch.is_whitespace() || ch.is_control()) {
        return None;
    }
    let typing_host = !needle.contains(['/', '?', '#']);
    let host_has_dot = typing_host && needle.contains('.');

    let mut best: Option<(&Entry, &str)> = None;
    for entry in &history.entries {
        let (is_http, body) = split_display_scheme(&entry.display);
        match scheme {
            SchemeFilter::Http if !is_http => continue,
            SchemeFilter::Https if is_http => continue,
            _ => {}
        }
        let Some(suffix) = suffix_after(body, needle) else {
            continue;
        };
        if suffix.is_empty() || (host_has_dot && host_part(suffix).contains('.')) {
            continue;
        }
        let better = best.is_none_or(|(current, _)| {
            (entry.visits, entry.last_visit) > (current.visits, current.last_visit)
        });
        if better {
            best = Some((entry, suffix));
        }
    }
    best.map(|(entry, suffix)| Completion {
        full_url: entry.url.clone(),
        completed_suffix: suffix.to_owned(),
    })
}

/// The address as shown in the omnibox. `https://` and a leading `www.` are hidden. `http://` stays
/// visible so insecure pages are obvious. Hosts stay in punycode, so lookalike domains show as they are.
pub fn display_form(url: &Url) -> String {
    let text = url.as_str();
    let (scheme, rest) = if let Some(rest) = text.strip_prefix(HTTPS_PREFIX) {
        ("", rest)
    } else if let Some(rest) = text.strip_prefix(HTTP_PREFIX) {
        (HTTP_PREFIX, rest)
    } else {
        ("", text)
    };
    // `www.com` is itself a host, so `www.` is hidden only when a dot follows it.
    let rest = match rest.strip_prefix(WWW_PREFIX) {
        Some(stripped) if host_part(stripped).contains('.') => stripped,
        _ => rest,
    };
    format!("{scheme}{rest}")
}

/// The condensed address shown when `minimal_url` is on and the field is unfocused: the registrable
/// host, plus the path when it is more than just `/`. Query and fragment are dropped, since they are
/// rarely useful once condensed. Falls back to [`display_form`] for a URL with no host (`file://`,
/// internal pages).
pub fn minimal_display_form(url: &Url) -> String {
    let Some(host) = url.host_str() else {
        return display_form(url);
    };
    let host = crate::blocker::registrable_domain(host);
    let path = url.path();
    if path.is_empty() || path == "/" {
        host.to_owned()
    } else {
        format!("{host}{path}")
    }
}

// `address_show_full`: the unfocused address shows everything (see [`set_show_full_address`]).
thread_local! {
    static SHOW_FULL_ADDRESS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// `address_autocomplete`: inline completion is offered (on by default).
    static AUTOCOMPLETE: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Turns inline completion of the typed text on or off ("Complete addresses as you type").
pub fn set_autocomplete(on: bool) {
    AUTOCOMPLETE.with(|flag| flag.set(on));
}

/// Turns "show the full address" on or off (on the UI thread, which draws every address). The next
/// address shown follows it.
pub fn set_show_full_address(on: bool) {
    SHOW_FULL_ADDRESS.with(|flag| flag.set(on));
}

/// [`display_form`], condensed through [`minimal_display_form`] when `minimal` is set. The unfocused
/// address display calls this with `minimal_url`; the suggestion dropdown and the focused field
/// always want the full form, so they call `display_form` directly.
pub fn address_display(url: &Url, minimal: bool) -> String {
    // "Show the full address": the whole address, scheme and `www.` included, whatever `minimal` says.
    if SHOW_FULL_ADDRESS.with(std::cell::Cell::get) {
        return url.as_str().to_owned();
    }
    if minimal {
        minimal_display_form(url)
    } else {
        display_form(url)
    }
}

/// Whether a navigation may replace the address text with the page's URL. While the field has keyboard
/// focus the text and selection belong to the user, so the URL is only remembered until focus leaves.
pub fn should_sync_displayed_url(field_focused: bool) -> bool {
    !field_focused
}

/// The UTF-16 range `(start, end)` of the word under `index`, for a double-click. A word is a run of
/// alphanumeric characters or `_`. Any other run, such as punctuation or spaces, is selected as one unit.
/// An index past the end selects the last run.
pub fn word_range(text: &str, index: usize) -> (usize, usize) {
    let mut runs: Vec<(usize, usize, bool)> = Vec::new();
    let mut offset = 0;
    for ch in text.chars() {
        let width = ch.len_utf16();
        let word = ch.is_alphanumeric() || ch == '_';
        match runs.last_mut() {
            Some(run) if run.2 == word => run.1 = offset + width,
            _ => runs.push((offset, offset + width, word)),
        }
        offset += width;
    }
    runs.iter()
        .find(|(_, end, _)| index < *end)
        .or(runs.last())
        .map_or((0, 0), |&(start, end, _)| (start, end))
}

/// The click count of a press at `now`, given the previous press and its count. A press within `window`
/// of the previous one continues its run, so the second press selects a word and the third selects the
/// field. A fourth quick press starts again at one.
pub fn click_count(previous: Option<(Instant, u8)>, now: Instant, window: Duration) -> u8 {
    match previous {
        Some((at, count)) if now.saturating_duration_since(at) <= window => count % 3 + 1,
        _ => 1,
    }
}

pub(crate) fn strip_typed_prefixes(typed: &str) -> (SchemeFilter, &str) {
    let (scheme, rest) = if let Some(rest) = strip_prefix_ignore_case(typed, HTTPS_PREFIX) {
        (SchemeFilter::Https, rest)
    } else if let Some(rest) = strip_prefix_ignore_case(typed, HTTP_PREFIX) {
        (SchemeFilter::Http, rest)
    } else {
        (SchemeFilter::Any, typed)
    };
    let rest = strip_prefix_ignore_case(rest, WWW_PREFIX).unwrap_or(rest);
    (scheme, rest)
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.as_bytes().get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix.as_bytes()) {
        text.get(prefix.len()..)
    } else {
        None
    }
}

/// Returns `(is_http, body)`, where `body` is the display form without its `http://` prefix.
fn split_display_scheme(display: &str) -> (bool, &str) {
    match display.strip_prefix(HTTP_PREFIX) {
        Some(body) => (true, body),
        None => (false, display),
    }
}

/// The part of `body` after `needle`, when `needle` is an ASCII-case-insensitive prefix of `body`.
fn suffix_after<'a>(body: &'a str, needle: &str) -> Option<&'a str> {
    let head = body.as_bytes().get(..needle.len())?;
    if head.eq_ignore_ascii_case(needle.as_bytes()) {
        body.get(needle.len()..)
    } else {
        None
    }
}

/// The text before the first path, query or fragment delimiter.
pub(crate) fn host_part(text: &str) -> &str {
    text.split(['/', '?', '#']).next().unwrap_or("")
}

// --- omnibox suggest ---
/// Most rows in the suggestion dropdown, including the default (what-you-typed) row.
pub const MAX_SUGGESTIONS: usize = 8;

/// What a suggestion row stands for. The wire names are `search`, `url`, `hist`, `bm` and `tab`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuggestKind {
    Search,
    Url,
    History,
    Bookmark,
    Tab,
    /// The local calculator's answer (`address_suggest`).
    Calc,
}

impl SuggestKind {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Url => "url",
            Self::History => "hist",
            Self::Bookmark => "bm",
            Self::Tab => "tab",
            Self::Calc => "calc",
        }
    }
}

/// A bookmark or open tab offered to [`suggest`]: a full URL and its title.
#[derive(Clone, Copy, Debug)]
pub struct SuggestSource<'a> {
    pub url: &'a str,
    pub title: &'a str,
}

/// One row of the dropdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub kind: SuggestKind,
    /// What the omnibox shows while the row is selected (display form of the URL, or the query).
    pub text: String,
    pub title: String,
    /// Display form of the URL; empty for a search row.
    pub url: String,
    /// What picking the row submits: the full URL, or the query for a search row.
    pub target: String,
    /// Index into the `tabs` slice given to [`suggest`], for a tab row.
    pub tab: Option<usize>,
}

/// The URL text without `https://`, `http://` and a leading `www.`, the same body the omnibox
/// matches on, found without parsing (bookmarks can number thousands).
fn match_body(url: &str) -> &str {
    let rest = strip_prefix_ignore_case(url, HTTPS_PREFIX)
        .or_else(|| strip_prefix_ignore_case(url, HTTP_PREFIX))
        .unwrap_or(url);
    strip_prefix_ignore_case(rest, WWW_PREFIX).unwrap_or(rest)
}

/// Byte offset of the first ASCII-case-insensitive occurrence of `needle` in `hay`. No allocation.
pub fn find_ignore_ascii_case(hay: &str, needle: &str) -> Option<usize> {
    let (hay, needle) = (hay.as_bytes(), needle.as_bytes());
    let (&first, rest) = needle.split_first()?;
    if needle.len() > hay.len() {
        return None;
    }
    let first = first.to_ascii_lowercase();
    let last = hay.len() - needle.len();
    let mut at = 0;
    while at <= last {
        if hay[at].to_ascii_lowercase() == first && hay[at + 1..at + needle.len()].eq_ignore_ascii_case(rest) {
            return Some(at);
        }
        at += 1;
    }
    None
}

/// Relevance of one candidate for `needle` (already stripped of scheme and `www.`), or `None`
/// when it does not match. Host prefix beats a later host label, which beats a title word, which
/// beats any other substring.
pub(crate) fn match_score(url: &str, title: &str, needle: &str) -> Option<u32> {
    let body = match_body(url);
    let host = host_part(body);
    let url_score = match find_ignore_ascii_case(body, needle) {
        Some(0) => Some(1000),
        Some(at) if at < host.len() && host.as_bytes()[at - 1] == b'.' => Some(600),
        Some(_) => Some(250),
        None => None,
    };
    let title_score = find_ignore_ascii_case(title, needle).map(|at| {
        let word_start = at == 0 || !title.as_bytes()[at - 1].is_ascii_alphanumeric();
        if word_start { 450 } else { 200 }
    });
    url_score.max(title_score)
}

/// Fragment-free key for de-duplicating rows that point at the same page.
pub(crate) fn dedupe_key(url: &str) -> &str {
    let body = match_body(url);
    let body = body.split('#').next().unwrap_or(body);
    body.strip_suffix('/').unwrap_or(body)
}

pub(crate) fn display_of(url: &str) -> String {
    Url::parse(url).map_or_else(|_| url.to_owned(), |parsed| display_form(&parsed))
}

/// Ranked dropdown rows for `typed`: `first` (the default row: what Enter does) and then open
/// tabs, bookmarks and history that match, at most [`MAX_SUGGESTIONS`] in all. Matching ignores
/// scheme, `www.` and ASCII case; each candidate scores by match quality plus frecency (visit count
/// and recency for history, a fixed boost for tabs and bookmarks). A page appears once: an open tab
/// beats a bookmark, which beats history. Pure and allocation-light: only the kept rows allocate.
pub fn suggest(
    history: &History,
    bookmarks: &[SuggestSource<'_>],
    tabs: &[SuggestSource<'_>],
    typed: &str,
    first: Option<Suggestion>,
) -> Vec<Suggestion> {
    let mut rows: Vec<Suggestion> = first.into_iter().collect();
    let (_, needle) = strip_typed_prefixes(typed.trim());
    if needle.is_empty() || needle.len() > MAX_URL_BYTES {
        return rows;
    }
    // (score, kind, index): on equal scores a tab ranks before a bookmark before history.
    let mut scored: Vec<(u32, SuggestKind, usize)> = Vec::new();
    for (index, tab) in tabs.iter().enumerate() {
        if let Some(score) = match_score(tab.url, tab.title, needle) {
            scored.push((score + 300, SuggestKind::Tab, index));
        }
    }
    for (index, mark) in bookmarks.iter().enumerate() {
        if let Some(score) = match_score(mark.url, mark.title, needle) {
            scored.push((score + 200, SuggestKind::Bookmark, index));
        }
    }
    for (index, entry) in history.entries.iter().enumerate() {
        if let Some(score) = match_score(&entry.url, &entry.title, needle) {
            let age = history.sequence.saturating_sub(entry.last_visit);
            let recency = 150u32.saturating_sub(age.min(150) as u32);
            let frequency = entry.visits.min(20) * 15;
            scored.push((score + recency + frequency, SuggestKind::History, index));
        }
    }
    let rank = |kind: SuggestKind| match kind {
        SuggestKind::Tab => 0,
        SuggestKind::Bookmark => 1,
        _ => 2,
    };
    scored.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(rank(a.1).cmp(&rank(b.1))).then(a.2.cmp(&b.2)));

    let source = |kind: SuggestKind, index: usize| -> (&str, &str) {
        match kind {
            SuggestKind::Tab => (tabs[index].url, tabs[index].title),
            SuggestKind::Bookmark => (bookmarks[index].url, bookmarks[index].title),
            _ => (&history.entries[index].url, &history.entries[index].title),
        }
    };
    let key_of = |url: &str| dedupe_key(url).to_ascii_lowercase();
    let mut taken: Vec<String> = rows
        .iter()
        .filter(|row| row.kind != SuggestKind::Search)
        .map(|row| key_of(&row.target))
        .collect();
    // The best kind per page, so an open tab or bookmark wins its page wherever it ranks.
    let mut best_kind: Vec<(String, SuggestKind)> = Vec::new();
    for &(_, kind, index) in scored.iter().take(MAX_SUGGESTIONS * 8) {
        let key = key_of(source(kind, index).0);
        match best_kind.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) if rank(kind) < rank(entry.1) => entry.1 = kind,
            Some(_) => {}
            None => best_kind.push((key, kind)),
        }
    }
    for &(_, kind, index) in scored.iter().take(MAX_SUGGESTIONS * 8) {
        if rows.len() >= MAX_SUGGESTIONS {
            break;
        }
        let (url, title) = source(kind, index);
        let key = key_of(url);
        if taken.contains(&key) || best_kind.iter().any(|(k, best)| *k == key && *best != kind) {
            continue;
        }
        taken.push(key);
        let display = display_of(url);
        rows.push(Suggestion {
            kind,
            text: display.clone(),
            title: if title.trim().is_empty() { display.clone() } else { title.trim().to_owned() },
            url: display,
            target: url.to_owned(),
            tab: (kind == SuggestKind::Tab).then_some(index),
        });
    }
    rows
}

/// The default row for `typed`: the inline completion's page when there is one, else the typed
/// address when it is one (`url` is its normalised URL), else a search for the text.
pub fn default_suggestion(typed: &str, completion: Option<&Completion>, url: Option<&Url>) -> Option<Suggestion> {
    let text = typed.trim();
    if text.is_empty() {
        return None;
    }
    let page = |target: String, display: String| Suggestion {
        kind: SuggestKind::Url,
        text: display.clone(),
        title: display.clone(),
        url: display,
        target,
        tab: None,
    };
    if let Some(completion) = completion {
        return Some(page(completion.full_url.clone(), display_of(&completion.full_url)));
    }
    Some(match url {
        Some(url) => page(url.as_str().to_owned(), display_form(url)),
        None => Suggestion {
            kind: SuggestKind::Search,
            text: text.to_owned(),
            title: text.to_owned(),
            url: String::new(),
            target: text.to_owned(),
            tab: None,
        },
    })
}
// --- end omnibox suggest ---

#[cfg(test)]
mod tests {
    use super::*;

    // --- omnibox suggest ---
    fn src<'a>(url: &'a str, title: &'a str) -> SuggestSource<'a> {
        SuggestSource { url, title }
    }

    fn kinds(rows: &[Suggestion]) -> Vec<(SuggestKind, &str)> {
        rows.iter().map(|row| (row.kind, row.target.as_str())).collect()
    }

    #[test]
    fn suggest_ranks_host_prefix_over_title_and_substring_matches() {
        let history = history_of(&[
            ("https://example.org/wiki", "A page"),
            ("https://en.wikipedia.org/", "Wikipedia"),
            ("https://wikipedia.org/", "Wikipedia"),
            ("https://other.example/kiwi", "Nothing"),
        ]);
        let rows = suggest(&history, &[], &[], "wiki", None);
        assert_eq!(rows[0].target, "https://wikipedia.org/", "host prefix first");
        assert_eq!(rows[1].target, "https://en.wikipedia.org/", "host label next");
        assert_eq!(rows[2].target, "https://example.org/wiki", "path substring last");
        assert!(rows.iter().all(|row| row.kind == SuggestKind::History));
        assert!(!rows.iter().any(|row| row.target.contains("kiwi")));
    }

    #[test]
    fn suggest_dedupes_by_page_and_prefers_tab_then_bookmark() {
        let history = history_of(&[("https://rust-lang.org/", "Rust"), ("https://docs.rs/", "Docs")]);
        let marks = [src("https://www.rust-lang.org/", "Rust bookmark"), src("https://docs.rs/", "Docs")];
        let tabs = [src("https://rust-lang.org/", "Rust tab")];
        let rows = suggest(&history, &marks, &tabs, "rust", None);
        assert_eq!(kinds(&rows), vec![(SuggestKind::Tab, "https://rust-lang.org/")]);
        assert_eq!(rows[0].tab, Some(0));
        assert_eq!(rows[0].title, "Rust tab");
        let rows = suggest(&history, &marks, &[], "docs", None);
        assert_eq!(kinds(&rows), vec![(SuggestKind::Bookmark, "https://docs.rs/")]);
    }

    #[test]
    fn suggest_caps_rows_and_keeps_the_default_row_first() {
        let mut history = History::default();
        for index in 0..40 {
            history.record(&format!("https://site{index}.example/"), "");
        }
        let first = default_suggestion("site", None, None);
        let rows = suggest(&history, &[], &[], "site", first);
        assert_eq!(rows.len(), MAX_SUGGESTIONS);
        assert_eq!(rows[0].kind, SuggestKind::Search);
        assert_eq!(rows[0].text, "site");
        // The most recent visit wins among equal matches; an untitled page shows its address.
        assert_eq!(rows[1].target, "https://site39.example/");
        assert_eq!(rows[1].title, "site39.example/");
    }

    #[test]
    fn default_row_follows_completion_then_url_then_search() {
        let history = history_of(&[("https://www.wikipedia.org/", "Wikipedia")]);
        let completion = inline_completion(&history, "wiki", true);
        let row = default_suggestion("wiki", completion.as_ref(), None).unwrap();
        assert_eq!((row.kind, row.text.as_str()), (SuggestKind::Url, "wikipedia.org/"));
        // The default row's page is not repeated as a history row.
        let rows = suggest(&history, &[], &[], "wiki", Some(row));
        assert_eq!(rows.len(), 1);

        let url = Url::parse("https://example.com/").unwrap();
        let row = default_suggestion("example.com", None, Some(&url)).unwrap();
        assert_eq!((row.kind, row.target.as_str()), (SuggestKind::Url, "https://example.com/"));
        assert_eq!(default_suggestion("   ", None, None), None);
    }

    #[test]
    fn suggest_matches_titles_case_insensitively_and_ignores_blank_input() {
        let history = history_of(&[("https://news.example/", "The Daily Paper")]);
        assert_eq!(suggest(&history, &[], &[], "daily", None).len(), 1);
        assert_eq!(suggest(&history, &[], &[], "DAILY", None).len(), 1);
        assert!(suggest(&history, &[], &[], "", None).is_empty());
        assert!(suggest(&history, &[], &[], "https://www.", None).is_empty());
        assert!(suggest(&history, &[], &[], &"x".repeat(10_000), None).is_empty());
        assert_eq!(find_ignore_ascii_case("Hello", "LL"), Some(2));
        assert_eq!(find_ignore_ascii_case("é", "x"), None);
    }

    #[test]
    fn suggest_over_a_full_history_and_many_bookmarks_is_fast() {
        let mut history = History::default();
        for index in 0..HISTORY_LIMIT {
            history.record(&format!("https://host{index}.example.com/path/{index}"), &format!("Page title number {index}"));
        }
        let urls: Vec<String> = (0..5000).map(|i| format!("https://bookmark{i}.example.net/")).collect();
        let marks: Vec<SuggestSource<'_>> = urls.iter().map(|u| src(u, "Bookmark title")).collect();
        let runs = 50;
        let start = Instant::now();
        for _ in 0..runs {
            let rows = suggest(&history, &marks, &[], "host1", default_suggestion("host1", None, None));
            assert_eq!(rows.len(), MAX_SUGGESTIONS);
        }
        let each = start.elapsed() / runs;
        eprintln!("suggest over {HISTORY_LIMIT} history + 5000 bookmarks: {each:?} per keystroke");
        assert!(each < Duration::from_millis(if cfg!(debug_assertions) { 60 } else { 5 }));
    }
    // --- end omnibox suggest ---

    fn history_of(entries: &[(&str, &str)]) -> History {
        let mut history = History::default();
        for (url, title) in entries {
            history.record(url, title);
        }
        history
    }

    fn suffix(history: &History, typed: &str) -> Option<String> {
        inline_completion(history, typed, true).map(|completion| completion.completed_suffix)
    }

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn show_full_address_keeps_scheme_and_www_and_wins_over_the_minimal_form() {
        let url = Url::parse("https://www.example.com/a/b?q=1").unwrap();
        set_show_full_address(false);
        assert_eq!(address_display(&url, false), "example.com/a/b?q=1");
        assert_eq!(address_display(&url, true), "example.com/a/b");
        set_show_full_address(true);
        assert_eq!(address_display(&url, false), "https://www.example.com/a/b?q=1");
        assert_eq!(address_display(&url, true), "https://www.example.com/a/b?q=1");
        set_show_full_address(false);
    }

    #[test]
    fn autocomplete_off_offers_no_completion() {
        let history = history_of(&[("https://www.wikipedia.org/", "Wikipedia")]);
        assert!(inline_completion(&history, "wiki", true).is_some());
        set_autocomplete(false);
        assert_eq!(inline_completion(&history, "wiki", true), None);
        set_autocomplete(true);
        assert!(inline_completion(&history, "wiki", true).is_some());
    }

    #[test]
    fn host_prefix_completes_to_the_stored_host_without_scheme_or_www() {
        let history = history_of(&[("https://www.wikipedia.org/", "Wikipedia")]);
        assert_eq!(
            inline_completion(&history, "wiki", true),
            Some(Completion {
                full_url: "https://www.wikipedia.org/".into(),
                completed_suffix: "pedia.org/".into(),
            })
        );
    }

    #[test]
    fn typed_scheme_and_www_are_matched_but_not_repeated_in_the_suffix() {
        let history = history_of(&[("https://www.wikipedia.org/", "")]);
        assert_eq!(
            suffix(&history, "https://wiki").as_deref(),
            Some("pedia.org/")
        );
        assert_eq!(suffix(&history, "WWW.WIKI").as_deref(), Some("pedia.org/"));
        assert_eq!(suffix(&history, "www.wi").as_deref(), Some("kipedia.org/"));
        assert_eq!(
            suffix(&history, "https://www.wiki").as_deref(),
            Some("pedia.org/")
        );
    }

    #[test]
    fn http_and_https_entries_are_not_mixed_when_the_scheme_is_typed() {
        let history = history_of(&[("http://intranet.local/", "")]);
        assert_eq!(suffix(&history, "intr").as_deref(), Some("anet.local/"));
        assert_eq!(
            suffix(&history, "http://intr").as_deref(),
            Some("anet.local/")
        );
        assert_eq!(suffix(&history, "https://intr"), None);
    }

    #[test]
    fn display_form_hides_https_and_leading_www_but_keeps_http_and_path() {
        assert_eq!(
            display_form(&url("https://www.example.com/")),
            "example.com/"
        );
        assert_eq!(
            display_form(&url("https://example.com/a/b?c=1")),
            "example.com/a/b?c=1"
        );
        assert_eq!(
            display_form(&url("http://www.example.com/")),
            "http://example.com/"
        );
        assert_eq!(
            display_form(&url("https://example.com:8443/")),
            "example.com:8443/"
        );
        assert_eq!(display_form(&url("https://www.com/")), "www.com/");
    }

    #[test]
    fn empty_and_blank_input_never_completes() {
        let history = history_of(&[("https://wikipedia.org/", "")]);
        for typed in ["", "   ", "https://", "http://", "www.", "https://www."] {
            assert_eq!(
                inline_completion(&history, typed, true),
                None,
                "typed: {typed:?}"
            );
        }
    }

    #[test]
    fn whitespace_inside_the_input_suppresses_completion() {
        let history = history_of(&[("https://wikipedia.org/", "")]);
        assert_eq!(suffix(&history, "wiki "), None);
        assert_eq!(suffix(&history, "wiki\t"), None);
    }

    #[test]
    fn backspace_or_delete_suppresses_completion() {
        let history = history_of(&[("https://wikipedia.org/", "")]);
        assert_eq!(inline_completion(&history, "wiki", false), None);
        assert!(inline_completion(&history, "wiki", true).is_some());
    }

    #[test]
    fn exact_matches_and_unmatched_text_have_nothing_to_complete() {
        let history = history_of(&[("https://wikipedia.org/", "")]);
        assert_eq!(suffix(&history, "wikipedia.org/"), None);
        assert_eq!(suffix(&history, "zzz"), None);
        assert_eq!(suffix(&history, "wikipedia.org/x"), None);
    }

    #[test]
    fn dotted_host_is_extended_only_within_its_last_label() {
        let history = history_of(&[
            ("https://example.com/", ""),
            ("https://example.com.evil.net/", ""),
        ]);
        assert_eq!(suffix(&history, "example.co").as_deref(), Some("m/"));
        assert_eq!(suffix(&history, "example.com").as_deref(), Some("/"));

        let lookalike_only = history_of(&[("https://example.com.evil.net/", "")]);
        assert_eq!(suffix(&lookalike_only, "example.com"), None);
        assert_eq!(suffix(&lookalike_only, "example.co"), None);
        assert_eq!(suffix(&lookalike_only, "en.wiki"), None);
    }

    #[test]
    fn paths_complete_only_along_stored_paths() {
        let history = history_of(&[("https://en.wikipedia.org/wiki/Rust", "Rust")]);
        assert_eq!(
            suffix(&history, "en.wikipedia.org/").as_deref(),
            Some("wiki/Rust")
        );
        assert_eq!(
            suffix(&history, "en.wikipedia.org/wi").as_deref(),
            Some("ki/Rust")
        );
        assert_eq!(suffix(&history, "en.wikipedia.org/x"), None);
        assert_eq!(suffix(&history, "en.wikipedia.org/wiki/Rust"), None);
    }

    #[test]
    fn most_visited_candidate_wins_then_most_recent() {
        let by_visits = history_of(&[
            ("https://wikipedia.org/", ""),
            ("https://wikihow.com/", ""),
            ("https://wikihow.com/", ""),
            ("https://wikihow.com/", ""),
        ]);
        assert_eq!(suffix(&by_visits, "wiki").as_deref(), Some("how.com/"));

        let by_recency = history_of(&[
            ("https://wikipedia.org/", ""),
            ("https://wikinews.org/", ""),
        ]);
        assert_eq!(suffix(&by_recency, "wiki").as_deref(), Some("news.org/"));
    }

    #[test]
    fn repeat_visits_dedupe_by_url_without_fragment_and_keep_the_last_real_title() {
        let mut history = history_of(&[("https://example.com/a#one", "First")]);
        history.record("https://example.com/a#two", "");
        assert_eq!(history.len(), 1);
        assert_eq!(history.entries[0].visits, 2);
        assert_eq!(history.entries[0].title, "First");
        history.record("https://example.com/a", "Second");
        assert_eq!(history.len(), 1);
        assert_eq!(history.entries[0].title, "Second");
    }

    #[test]
    fn only_credential_free_web_urls_are_recorded() {
        let history = history_of(&[
            ("javascript:alert(1)", ""),
            ("file:///C:/secret.txt", ""),
            ("about:blank", ""),
            ("https://user:pass@example.com/", ""),
            ("not a url", ""),
        ]);
        assert!(history.is_empty());
        assert_eq!(History::default().len(), 0);
    }

    #[test]
    fn urls_over_2048_bytes_are_rejected_and_titles_are_capped_at_256_chars() {
        let prefix = "https://example.com/";
        let at_limit = format!("{prefix}{}", "a".repeat(MAX_URL_BYTES - prefix.len()));
        let over_limit = format!("{prefix}{}", "a".repeat(MAX_URL_BYTES - prefix.len() + 1));
        assert_eq!(at_limit.len(), MAX_URL_BYTES);
        assert_eq!(over_limit.len(), MAX_URL_BYTES + 1);

        let mut history = History::default();
        history.record(&over_limit, "");
        assert!(history.is_empty());

        history.record(&at_limit, "");
        assert_eq!(history.len(), 1);
        assert_eq!(history.entries[0].url, at_limit);

        // Titles count characters, not bytes, so multi-byte text is cut on a character boundary.
        let long_title = "é".repeat(300);
        history.record("https://title.example/", &format!("  {long_title}  "));
        let stored = &history.entries[1].title;
        assert_eq!(stored.chars().count(), MAX_TITLE_CHARS);
        assert!(stored.chars().all(|ch| ch == 'é'));
    }

    #[test]
    fn history_is_bounded_and_evicts_the_least_valuable_entry() {
        let mut history = History::default();
        history.record("https://site0.example/", "");
        for _ in 0..4 {
            history.record("https://site0.example/", "");
        }
        for index in 1..HISTORY_LIMIT {
            history.record(&format!("https://site{index}.example/"), "");
        }
        assert_eq!(history.len(), HISTORY_LIMIT);

        for index in HISTORY_LIMIT..HISTORY_LIMIT + 100 {
            history.record(&format!("https://site{index}.example/"), "");
        }
        assert_eq!(history.len(), HISTORY_LIMIT);
        assert_eq!(suffix(&history, "site0.e").as_deref(), Some("xample/"));
        assert_eq!(suffix(&history, "site1.e"), None);
        assert_eq!(suffix(&history, "site100.e"), None);
        assert_eq!(suffix(&history, "site101.e").as_deref(), Some("xample/"));
        assert_eq!(suffix(&history, "site599.e").as_deref(), Some("xample/"));
    }

    #[test]
    fn unicode_hosts_are_matched_in_punycode_only() {
        let history = history_of(&[("https://münich.example/", "")]);
        assert_eq!(
            display_form(&url("https://münich.example/")),
            "xn--mnich-kva.example/"
        );
        assert_eq!(suffix(&history, "münich"), None);
        assert_eq!(
            suffix(&history, "xn--mn").as_deref(),
            Some("ich-kva.example/")
        );
    }

    #[test]
    fn lookalike_unicode_characters_do_not_complete_to_ascii_hosts() {
        let history = history_of(&[("https://wikipedia.org/", "")]);
        assert_eq!(suffix(&history, "wіkipedia"), None);
        assert_eq!(suffix(&history, "wikipediа"), None);
        assert_eq!(suffix(&history, "wiki\u{0301}"), None);
    }

    #[test]
    fn navigation_never_replaces_the_text_while_the_field_has_focus() {
        assert!(!should_sync_displayed_url(true));
        assert!(should_sync_displayed_url(false));
    }

    #[test]
    fn double_click_selects_the_word_under_the_pointer() {
        let text = "https://en.wikipedia.org/wiki/Rust";
        assert_eq!(word_range(text, 0), (0, 5), "https");
        assert_eq!(word_range(text, 10), (10, 11), "the dot is its own run");
        assert_eq!(word_range(text, 12), (11, 20), "wikipedia");
        assert_eq!(word_range(text, 26), (25, 29), "wiki");
        assert_eq!(word_range(text, 29), (29, 30), "the slash before Rust");
        assert_eq!(
            word_range(text, 100),
            (30, 34),
            "past the end selects the last run"
        );
    }

    #[test]
    fn word_ranges_use_utf16_offsets_and_keep_underscores_together() {
        let text = "é😀 foo_bar";
        // 'é' is one unit, the emoji two units, the space one more, so "foo_bar" starts at unit 4.
        assert_eq!(word_range(text, 0), (0, 1));
        // The emoji and the space are both non-word, so they form one run.
        assert_eq!(word_range(text, 1), (1, 4));
        assert_eq!(word_range(text, 3), (1, 4));
        assert_eq!(word_range(text, 5), (4, 11));
        assert_eq!(word_range("", 0), (0, 0));
    }

    #[test]
    fn quick_presses_count_up_to_three_then_restart() {
        let window = Duration::from_millis(500);
        let start = Instant::now();
        assert_eq!(click_count(None, start, window), 1);
        let second = click_count(Some((start, 1)), start + Duration::from_millis(100), window);
        assert_eq!(second, 2);
        let third = click_count(
            Some((start, second)),
            start + Duration::from_millis(200),
            window,
        );
        assert_eq!(third, 3);
        let fourth = click_count(
            Some((start, third)),
            start + Duration::from_millis(300),
            window,
        );
        assert_eq!(fourth, 1);
    }

    #[test]
    fn a_slow_press_starts_a_new_run() {
        let window = Duration::from_millis(500);
        let start = Instant::now();
        let late = start + Duration::from_millis(501);
        assert_eq!(click_count(Some((start, 2)), late, window), 1);
    }

    #[test]
    fn very_long_input_never_completes_or_panics() {
        let history = history_of(&[("https://a.example/", "")]);
        let long_text = "a".repeat(10_000);
        assert_eq!(suffix(&history, &long_text), None);
        assert_eq!(suffix(&history, &"/".repeat(10_000)), None);
        assert_eq!(suffix(&history, &format!("a{}", "é".repeat(5_000))), None);
    }
}
