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
enum SchemeFilter {
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
    if !allow {
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

fn strip_typed_prefixes(typed: &str) -> (SchemeFilter, &str) {
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
fn host_part(text: &str) -> &str {
    text.split(['/', '?', '#']).next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

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
