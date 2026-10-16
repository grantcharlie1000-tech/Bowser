//! Request blocking for ads and trackers: a compact in-memory matcher plus per-site exceptions.
//!
//! Pure logic, no Windows APIs. The WebView2 hook lives in `engine/webview2.rs` and only calls
//! [`BlockerState::should_block`].
//!
//! Rule sources, all in ADBLOCK-lite syntax (a subset of EasyList/EasyPrivacy):
//! * the built-in curated list `src/blocklist.txt` (embedded at build time);
//! * the optional user list `filters.txt` in the profile directory (`%LOCALAPPDATA%\Bowser\` for
//!   the default profile), at most [`MAX_USER_LIST_BYTES`]. To update it, save an EasyList or
//!   EasyPrivacy text file over it and restart Bowser. Bowser never fetches filter lists itself.
//!
//! Supported lines (everything else is silently ignored):
//! * `||host^` / `||host` / `||host^|`      block `host` and all of its subdomains, third-party
//!   requests only;
//! * `||host/path*pattern` / `||host^`      block URLs on that host whose path+query starts with the
//!   pattern (`*` is any run, `^` is a separator or the end), third-party requests only;
//! * `/plain/substring*`                    block URLs containing the text (at most
//!   [`MAX_SUBSTRING_RULES`] of these are kept; scanned linearly). Applies to every request,
//!   first-party included, unless `$third-party` restricts it, and to every page unless
//!   `$domain=` restricts it;
//! * `domain##selector` / `##selector`      hide `selector` on `domain` (comma-separated list;
//!   `##` on its own applies on every page). `selector` is one or more `.class` / `#id` tokens
//!   concatenated with no separator (ASCII `[A-Za-z_][A-Za-z0-9_-]*` per token) — no spaces,
//!   attributes, commas or pseudo-classes;
//! * recognised options on a substring rule are `$third-party` (`$3p`) and `$domain=a|b`,
//!   together or alone; a domain entry with `~` (negation) drops the whole rule. Any other
//!   option (including `$domain=` on a host or path rule), exception rules (`@@`), the other
//!   cosmetic markers (`#@#`, `#?#`, `#$#`, `#%#`), regex rules, comments and headers are
//!   ignored, so a rule is never applied more broadly than it was written.
//!
//! Safety rules that hold regardless of the lists:
//! * only `http`/`https` URLs are considered for subresource requests;
//! * a host or path rule never blocks a request whose registrable domain equals the page's
//!   (first-party); a substring rule does too, unless written with `$third-party`;
//! * sites on the allow list (settings `blocker_allow=a.com,b.org` plus the persisted runtime
//!   toggle in `blocker-sites.txt`) are never blocked.
//!
//! Four more decisions live alongside the matcher and are consumed only by the WebView2 hook:
//! * [`BlockerState::blocked_url_patterns`] turns every rule a DevTools URL pattern can express
//!   (host rules, `||host/path|` rules and plain `||host/path` prefix rules) into
//!   `Network.setBlockedURLs` patterns, so those requests fail at the network layer (a real
//!   connection failure) instead of getting an HTTP response a page's own reachability probe
//!   would see as "not blocked", and never wait on the UI thread;
//! * [`Blocker::request_filters`] narrows the WebView2 `WebResourceRequested` filters to the
//!   requests only the in-process matcher can decide (substring rules, and path rules whose case,
//!   `?` or `^` a URL pattern would get wrong), so other requests are never routed through it;
//! * [`Blocker::cosmetic_css_for_host`] turns the `##` rules for a page into a CSS string the
//!   hook injects as a `<style>` element;
//! * [`BlockerState::player_ads_rewrite`] picks the YouTube player responses whose ad schedule
//!   the hook strips (see [`youtube`]), under the same switch, site exceptions and private-tab
//!   rule as every request rule.

pub mod cookies;
pub mod fetch;
pub(crate) mod json;
pub mod site_rules;
pub mod sources;
pub mod youtube;

use std::{
    borrow::Cow,
    cell::{OnceCell, RefCell},
    collections::{BTreeSet, HashMap, HashSet},
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

pub const BUILTIN_LIST: &str = include_str!("blocklist.txt");
pub const USER_LIST_NAME: &str = "filters.txt";
pub const SITES_FILE_NAME: &str = "blocker-sites.txt";
pub const MAX_USER_LIST_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SITES_BYTES: usize = 64 * 1024;
pub const MAX_HOST_RULES: usize = 250_000;
pub const MAX_PATH_RULES: usize = 25_000;
pub const MAX_SUBSTRING_RULES: usize = 256;
pub const MAX_COSMETIC_RULES: usize = 2_048;
const MAX_PATTERN_BYTES: usize = 160;
/// How much the blocker stops on a page: nothing, trackers only, or trackers and ads.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Level {
    None,
    Trackers,
    #[default]
    Ads,
}

impl Level {
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Trackers => "trackers",
            Self::Ads => "ads",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "trackers" => Some(Self::Trackers),
            "ads" => Some(Self::Ads),
            _ => None,
        }
    }

    /// The global level the settings describe: `blocker_enabled=false` is "none".
    pub fn from_settings(enabled: bool, level: crate::settings::BlockerLevel) -> Self {
        match (enabled, level) {
            (false, _) => Self::None,
            (true, crate::settings::BlockerLevel::Trackers) => Self::Trackers,
            (true, crate::settings::BlockerLevel::Ads) => Self::Ads,
        }
    }
}

/// Which kind of rule a list line is: set by `! @kind trackers|ads` marker lines inside the
/// built-in list, or by the kind of the source a list was added as.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListKind {
    Trackers,
    Ads,
}

/// Which lines of a list [`add_kinded`] reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KindSelect {
    /// Honour the list's `! @kind` markers (ads until the first one) and keep only this kind.
    Markers(ListKind),
    /// Ignore markers: every rule is of this kind.
    All(ListKind),
}

fn marker_kind(line: &str) -> Option<ListKind> {
    match line.trim().strip_prefix("! @kind ")?.trim() {
        "trackers" => Some(ListKind::Trackers),
        "ads" => Some(ListKind::Ads),
        _ => None,
    }
}

/// Adds the rules of `text` selected by `select` to `all` (the trackers-and-ads matcher) and, for a
/// tracker rule, to `trackers` too. Returns how many rules went into `all`.
pub(crate) fn add_kinded(all: &mut Blocker, trackers: &mut Blocker, text: &str, select: KindSelect) -> usize {
    let mut current = match select {
        KindSelect::Markers(_) => ListKind::Ads,
        KindSelect::All(kind) => kind,
    };
    let mut added = 0;
    for line in text.lines() {
        if matches!(select, KindSelect::Markers(_)) {
            if let Some(kind) = marker_kind(line) {
                current = kind;
                continue;
            }
        }
        let wanted = match select {
            KindSelect::Markers(only) => current == only,
            KindSelect::All(_) => true,
        };
        if !wanted {
            continue;
        }
        let Some(rule) = parse_line(line) else {
            continue;
        };
        if current == ListKind::Trackers {
            trackers.add_rule(rule.clone());
        }
        if all.add_rule(rule) {
            added += 1;
        }
    }
    added
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Rule {
    Host(String),
    Path {
        host: String,
        pattern: String,
    },
    Exact {
        host: String,
        path: String,
    },
    Substring {
        pattern: String,
        third_party_only: bool,
        domains: Vec<String>,
    },
    Cosmetic {
        domains: Vec<String>,
        selector: String,
    },
}

/// Parses one filter line. `None` means "not supported / not a rule".
fn parse_line(line: &str) -> Option<Rule> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('!') || line.starts_with('[') || line.starts_with("@@") {
        return None;
    }
    if ["#@#", "#?#", "#$#", "#%#"]
        .iter()
        .any(|marker| line.contains(marker))
    {
        return None;
    }
    if let Some((domain_part, selector)) = line.split_once("##") {
        return parse_cosmetic(domain_part, selector);
    }
    let (pattern, options) = match line.rfind('$') {
        Some(index) => (&line[..index], Some(&line[index + 1..])),
        None => (line, None),
    };
    let (third_party_only, domains) = match options {
        Some(options) => parse_options(options)?,
        None => (false, Vec::new()),
    };
    let pattern = pattern.to_ascii_lowercase();
    if !pattern.is_ascii()
        || pattern.len() > MAX_PATTERN_BYTES
        || pattern.contains(char::is_whitespace)
    {
        return None;
    }
    if let Some(rest) = pattern.strip_prefix("||") {
        // `$domain=` is understood only on substring rules; host/path rules stay third-party
        // only, unconditionally, so a domain-scoped host/path rule is unsupported and dropped.
        if !domains.is_empty() {
            return None;
        }
        let end = rest
            .find(['^', '/', '*', '?', ':', '|'])
            .unwrap_or(rest.len());
        let host = &rest[..end];
        if !valid_rule_host(host) {
            return None;
        }
        let tail = &rest[end..];
        return match tail {
            "" | "^" | "|" | "^|" => Some(Rule::Host(host.to_owned())),
            // `||host/path|`: that one URL and nothing longer. Only plain path characters, so the
            // same text is also a literal DevTools URL pattern (`:`, `(`, `{`, `+`, `*` and `?`
            // all mean something in one).
            _ if tail.starts_with('/')
                && tail.ends_with('|')
                && tail[..tail.len() - 1].bytes().all(|b| {
                    b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-' | b'~' | b'%')
                }) =>
            {
                Some(Rule::Exact {
                    host: host.to_owned(),
                    path: tail.trim_end_matches('|').to_owned(),
                })
            }
            _ if tail.starts_with('/') || tail.starts_with('?') => {
                let pattern = tail.trim_end_matches('|');
                Some(Rule::Path {
                    host: host.to_owned(),
                    pattern: pattern.to_owned(),
                })
            }
            _ => None,
        };
    }
    if pattern.starts_with('|') || pattern.contains("://") {
        return None;
    }
    // `/.../` is a regular expression in ADBLOCK syntax.
    if pattern.len() > 2
        && pattern.starts_with('/')
        && pattern.ends_with('/')
        && pattern[1..].contains(['\\', '(', '[', '+', '{'])
    {
        return None;
    }
    let pattern = pattern.trim_end_matches('|');
    if pattern.len() < 6 || pattern.contains('|') {
        return None;
    }
    Some(Rule::Substring {
        pattern: pattern.to_owned(),
        third_party_only,
        domains,
    })
}

/// Parses the options after `$`: `third-party`/`3p`, and `domain=a|b|...`. Any other option, an
/// empty `domain=`, or a domain entry containing `~` (negation), drops the whole rule.
fn parse_options(options: &str) -> Option<(bool, Vec<String>)> {
    let mut third_party_only = false;
    let mut domains = Vec::new();
    for option in options.split(',') {
        let option = option.trim();
        match option {
            "third-party" | "3p" => third_party_only = true,
            _ if option.starts_with("domain=") => {
                let list = &option["domain=".len()..];
                if list.is_empty() {
                    return None;
                }
                for entry in list.split('|') {
                    if entry.contains('~') {
                        return None;
                    }
                    let entry = entry.trim().to_ascii_lowercase();
                    if !valid_rule_host(&entry) {
                        return None;
                    }
                    domains.push(entry);
                }
            }
            _ => return None,
        }
    }
    Some((third_party_only, domains))
}

/// Parses `domain,domain2##selector` (`domain_part` empty means `##selector` on its own).
fn parse_cosmetic(domain_part: &str, selector: &str) -> Option<Rule> {
    if domain_part.contains('~') {
        return None;
    }
    let selector = selector.trim();
    if !valid_cosmetic_selector(selector) {
        return None;
    }
    let domain_part = domain_part.trim();
    let mut domains = Vec::new();
    if !domain_part.is_empty() {
        for entry in domain_part.split(',') {
            let entry = entry.trim().to_ascii_lowercase();
            if !valid_rule_host(&entry) {
                return None;
            }
            domains.push(entry);
        }
    }
    Some(Rule::Cosmetic {
        domains,
        selector: selector.to_owned(),
    })
}

/// `selector` is one or more `.class` / `#id` tokens concatenated with no separator: the only
/// shape the cosmetic injector can safely turn into `selector{display:none!important}` and embed
/// in a JS string (no quotes, backslashes, spaces, attributes, commas, pseudo-classes or braces).
fn valid_cosmetic_selector(selector: &str) -> bool {
    if selector.is_empty() || !selector.is_ascii() {
        return false;
    }
    let bytes = selector.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !matches!(bytes[i], b'.' | b'#') {
            return false;
        }
        i += 1;
        if i >= bytes.len() || !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
            return false;
        }
        i += 1;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'-'))
        {
            i += 1;
        }
    }
    true
}

fn valid_rule_host(host: &str) -> bool {
    host.len() <= 253
        && host.contains('.')
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_')
                })
        })
}

fn is_separator(byte: u8) -> bool {
    !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'%'))
}

/// ADBLOCK-style glob anchored at the start of `text`, with an implicit trailing wildcard.
/// `pattern` must already be lowercase; `text` is compared case-insensitively.
fn glob_prefix(pattern: &[u8], text: &[u8]) -> bool {
    glob_core(pattern, text, true)
}

/// Single-pass glob matcher (`*` and `^`) with one backtrack point, so the cost is bounded by
/// `pattern.len() * text.len()` and never grows with the number of wildcards. When `anchored` is
/// false the match may start at any offset in `text` (a leading `*`).
fn glob_core(pattern: &[u8], text: &[u8], anchored: bool) -> bool {
    let (mut p, mut t) = (0usize, 0usize);
    // (pattern index just after the last `*`, text index that `*` currently consumes up to)
    let mut star: Option<(usize, usize)> = (!anchored).then_some((0, 0));
    loop {
        if p == pattern.len() {
            return true;
        }
        match pattern[p] {
            b'*' => {
                p += 1;
                star = Some((p, t));
                continue;
            }
            b'^' => {
                if t == text.len() {
                    p += 1;
                    continue;
                }
                if is_separator(text[t]) {
                    p += 1;
                    t += 1;
                    continue;
                }
            }
            byte => {
                if t < text.len() && text[t].to_ascii_lowercase() == byte {
                    p += 1;
                    t += 1;
                    continue;
                }
            }
        }
        match star {
            Some((star_p, star_t)) if star_t < text.len() => {
                star = Some((star_p, star_t + 1));
                p = star_p;
                t = star_t + 1;
            }
            _ => return false,
        }
    }
}

fn contains_glob(pattern: &[u8], text: &[u8]) -> bool {
    if !pattern.iter().any(|b| matches!(b, b'*' | b'^')) {
        return text.len() >= pattern.len()
            && text
                .windows(pattern.len())
                .any(|window| window.eq_ignore_ascii_case(pattern));
    }
    glob_core(pattern, text, false)
}

/// Longest URL text (in bytes) that substring and path rules are run against. Anything beyond
/// this is ignored for matching, which keeps every rule's work bounded regardless of input size.
pub const MAX_MATCH_TEXT_BYTES: usize = 2048;

/// The longest prefix of `text` that is at most `max` bytes and ends on a character boundary.
fn capped(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Splits an `http(s)` URL into (lowercased host, path+query). `None` for other schemes,
/// IPv6 literals and malformed input. Allocates only when the host contains uppercase.
fn split_url(url: &str) -> Option<(Cow<'_, str>, &str)> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| {
            let (scheme, rest) = url.split_once("://")?;
            (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
                .then_some(rest)
        })?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let authority = authority.rsplit('@').next()?;
    if authority.starts_with('[') {
        return None;
    }
    let host = authority.split(':').next()?;
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() {
        return None;
    }
    let tail = tail.split('#').next().unwrap_or("");
    let path = if tail.is_empty() { "/" } else { tail };
    let host = if host.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(host.to_ascii_lowercase())
    } else {
        Cow::Borrowed(host)
    };
    Some((host, path))
}

/// Yields `host` and then each parent domain that still contains a dot: `a.b.c` -> `a.b.c`, `b.c`.
fn host_suffixes(host: &str) -> impl Iterator<Item = &str> {
    let mut next = Some(host);
    std::iter::from_fn(move || {
        let current = next?;
        next = current
            .split_once('.')
            .map(|(_, rest)| rest)
            .filter(|rest| rest.contains('.'));
        Some(current)
    })
}

/// Most hosts [`registrable_domain`] remembers before it starts over.
const REGISTRABLE_DOMAIN_CACHE: usize = 256;

thread_local! {
    /// Host -> byte offset of its registrable domain. A page's subresources come from a handful
    /// of hosts, so the public-suffix walk runs once per host instead of once per request.
    static REGISTRABLE_DOMAINS: RefCell<HashMap<Box<str>, usize>> = RefCell::new(HashMap::new());
}

/// The registrable domain (eTLD+1) of a lowercase host, or the host itself when it has none
/// (IP addresses, `localhost`, bare suffixes). Results are cached per thread in a small table
/// that is cleared when it fills; the answer is always [`registrable_domain_uncached`]'s.
pub fn registrable_domain(host: &str) -> &str {
    if host.len() > 253 {
        return registrable_domain_uncached(host);
    }
    let start = REGISTRABLE_DOMAINS.try_with(|cache| {
        if let Some(&start) = cache.borrow().get(host) {
            return start;
        }
        let start = host.len() - registrable_domain_uncached(host).len();
        let mut cache = cache.borrow_mut();
        if cache.len() >= REGISTRABLE_DOMAIN_CACHE {
            cache.clear();
        }
        cache.insert(host.into(), start);
        start
    });
    match start {
        Ok(start) => &host[start..],
        Err(_) => registrable_domain_uncached(host),
    }
}

/// [`registrable_domain`] without the cache. The result is always a suffix of `host`.
fn registrable_domain_uncached(host: &str) -> &str {
    psl::domain(host.as_bytes())
        .and_then(|domain| std::str::from_utf8(domain.as_bytes()).ok())
        .filter(|domain| host.ends_with(domain))
        .map(|domain| &host[host.len() - domain.len()..])
        .unwrap_or(host)
}

/// True when both hosts share a registrable domain.
pub fn same_site(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b) || registrable_domain(a).eq_ignore_ascii_case(registrable_domain(b))
}

/// DevTools `Network.setBlockedURLs` patterns for every host in `hosts`, excluding any whose
/// registrable domain equals `page_host`'s (first-party), or an empty list when `blocking` is
/// false. Both the bare host and its subdomains are covered (`Network.setBlockedURLs` does no
/// suffix matching of its own). The result is sorted, so it is exact to compare in tests.
pub fn blocked_url_patterns<'a>(
    hosts: impl IntoIterator<Item = &'a str>,
    page_host: &str,
    blocking: bool,
) -> Vec<String> {
    if !blocking {
        return Vec::new();
    }
    let mut patterns: Vec<String> = hosts
        .into_iter()
        .filter(|host| !same_site(host, page_host))
        .flat_map(host_url_patterns)
        .collect();
    patterns.sort_unstable();
    patterns
}

/// A host rule's two DevTools patterns: the bare host and its subdomains.
fn host_url_patterns(host: &str) -> [String; 2] {
    [format!("*://{host}/*"), format!("*://*.{host}/*")]
}

/// A `||host/path|` rule's two DevTools patterns (see [`exact_url_patterns`]).
fn exact_rule_url_patterns(host: &str, path: &str) -> [String; 2] {
    [format!("*://{host}{path}?#"), format!("*://*.{host}{path}?#")]
}

/// The DevTools pattern path for a `||host/path` prefix rule, when there is one that matches
/// only URLs the rule matches: a path of plain characters and `*`, starting with `/`, with no dot
/// segment a URL pattern could fold away. `^` (separator or end), `?` (the query) and URL-pattern
/// syntax have no such path, so those rules stay with the in-process matcher alone.
///
/// The DevTools pattern is a subset and not always the same set: it compares case-sensitively
/// (the rule is lowercase), and its `*` stays inside the path while the rule's can run into the
/// query. [`path_rule_is_exact_in_devtools`] says when the two are the same.
fn devtools_rule_path(pattern: &str) -> Option<&str> {
    let plain = pattern.starts_with('/')
        && !pattern.contains("**")
        && pattern.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-' | b'~' | b'%' | b'*')
        })
        && !pattern
            .trim_end_matches('*')
            .split('/')
            .any(|segment| matches!(segment, "." | ".."));
    plain.then(|| pattern.trim_end_matches('*'))
}

/// True when a prefix rule's DevTools pattern matches exactly the URLs the rule does, so the rule
/// needs no WebView2 request filter: no letter whose case DevTools would compare, and no `*`
/// before the implicit trailing one.
fn path_rule_is_exact_in_devtools(pattern: &str) -> bool {
    devtools_rule_path(pattern).is_some_and(|path| {
        !path.contains('*') && !path.bytes().any(|b| b.is_ascii_alphabetic())
    })
}

/// A `||host/path` prefix rule's two DevTools patterns, when it has a [`devtools_rule_path`]. The
/// query and fragment stay open, as the rule's implicit trailing wildcard leaves them.
fn path_rule_url_patterns(host: &str, pattern: &str) -> Option<[String; 2]> {
    let path = devtools_rule_path(pattern)?;
    Some([format!("*://{host}{path}*"), format!("*://*.{host}{path}*")])
}

/// Most WebView2 request filters [`Blocker::request_filters`] returns before it asks for `*`
/// instead. Each filter is one `AddWebResourceRequestedFilter` call per resource context.
pub const MAX_REQUEST_FILTERS: usize = 256;

/// Turns rule text (lowercase, `*` and `^` wildcards) into WebView2 filter text that matches at
/// least every URL text the rule matches. WebView2 filters are globs over the whole URI (`*` any
/// run, `?` one character, `\` escapes); whether they compare case-insensitively is not
/// documented, so every letter becomes `?` and a URL in any case still reaches the matcher. `^`
/// becomes `*`, and a literal `?` is escaped. The matcher still makes the decision.
fn request_filter_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    for byte in text.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'\\' => out.push('?'),
            b'*' | b'^' => out.push('*'),
            b'?' => out.push_str("\\?"),
            other => out.push(other as char),
        }
    }
    out
}

/// DevTools patterns for `||host/path|` rules: that one URL on the host and on its subdomains.
/// The trailing `?#` pins the (empty) query and fragment, which a URL pattern otherwise leaves
/// open, so `https://host/path?x` and `https://host/path/more` still load. Paths are lowercase,
/// as in the list; the in-process 403 fallback is the case-insensitive backstop. Same
/// first-party and `blocking` rules as [`blocked_url_patterns`].
pub fn exact_url_patterns<'a>(
    urls: impl IntoIterator<Item = (&'a str, &'a str)>,
    page_host: &str,
    blocking: bool,
) -> Vec<String> {
    if !blocking {
        return Vec::new();
    }
    urls.into_iter()
        .filter(|(host, _)| !same_site(host, page_host))
        .flat_map(|(host, path)| exact_rule_url_patterns(host, path))
        .collect()
}

/// Every DevTools pattern the rules produce, built once in the order a page's list is sent in
/// (host patterns, then `||host/path|` patterns, then prefix-rule patterns, each sorted), each
/// tagged with the registrable domain of its rule host. A page's list is this one minus the
/// patterns of its own site, so a site change filters instead of formatting every pattern again.
struct DevtoolsPatterns {
    /// (index into `sites`, pattern)
    patterns: Vec<(u32, Box<str>)>,
    sites: HashMap<Box<str>, u32>,
}

impl DevtoolsPatterns {
    fn new(matcher: &Blocker) -> Self {
        let mut sites: HashMap<Box<str>, u32> = HashMap::new();
        let mut site_of = |host: &str| -> u32 {
            let site = registrable_domain(host);
            if let Some(&id) = sites.get(site) {
                return id;
            }
            let id = sites.len() as u32;
            sites.insert(site.into(), id);
            id
        };
        let mut patterns: Vec<(u32, Box<str>)> = Vec::new();
        let groups: [Vec<(&str, String)>; 3] = [
            matcher
                .hosts()
                .flat_map(|host| host_url_patterns(host).map(|pattern| (host, pattern)))
                .collect(),
            matcher
                .exact_urls()
                .flat_map(|(host, path)| {
                    exact_rule_url_patterns(host, path).map(|pattern| (host, pattern))
                })
                .collect(),
            matcher
                .path_rules()
                .filter_map(|(host, rule)| {
                    path_rule_url_patterns(host, rule).map(|pair| pair.map(|pattern| (host, pattern)))
                })
                .flatten()
                .collect(),
        ];
        for mut group in groups {
            group.sort_unstable_by(|a, b| a.1.cmp(&b.1));
            group.dedup_by(|a, b| a.1 == b.1);
            patterns.extend(
                group
                    .into_iter()
                    .map(|(host, pattern)| (site_of(host), pattern.into_boxed_str())),
            );
        }
        Self { patterns, sites }
    }

    /// The patterns for a document on `page_host` (lowercase, as a URL host is): every pattern
    /// whose rule host is not on the page's registrable domain. Same exemption as [`same_site`].
    fn for_page(&self, page_host: &str) -> Vec<String> {
        let own = self.sites.get(registrable_domain(page_host)).copied();
        self.patterns
            .iter()
            .filter(|(site, _)| Some(*site) != own)
            .map(|(_, pattern)| pattern.to_string())
            .collect()
    }
}

struct SubstringRule {
    pattern: Box<str>,
    third_party_only: bool,
    domains: Vec<Box<str>>,
}

struct CosmeticRule {
    domains: Vec<Box<str>>,
    selector: Box<str>,
}

/// True when `domains` is empty (applies everywhere) or `page_host` is one of its entries or a
/// subdomain of one, using the same suffix test as `registrable_domain`'s callers.
fn domain_list_matches(page_host: &str, domains: &[Box<str>]) -> bool {
    domains.is_empty()
        || host_suffixes(page_host).any(|suffix| domains.iter().any(|domain| domain.as_ref() == suffix))
}

/// The matcher. Cheap to query, built once at startup.
#[derive(Default)]
pub struct Blocker {
    hosts: HashSet<Box<str>>,
    paths: HashMap<Box<str>, Vec<Box<str>>>,
    /// `||host/path|` rules: whole-URL paths, matched exactly and also failed by DevTools.
    exact: HashMap<Box<str>, Vec<Box<str>>>,
    path_count: usize,
    substrings: Vec<SubstringRule>,
    cosmetics: Vec<CosmeticRule>,
}

impl Blocker {
    pub fn from_lists(user_list: Option<&str>) -> Self {
        let mut blocker = Self::default();
        blocker.add_list(BUILTIN_LIST);
        if let Some(text) = user_list {
            blocker.add_list(text);
        }
        blocker
    }

    /// Adds every supported rule in `text`; returns how many were added.
    pub fn add_list(&mut self, text: &str) -> usize {
        text.lines()
            .filter_map(parse_line)
            .filter(|rule| self.add_rule(rule.clone()))
            .count()
    }

    /// Stores one parsed rule; `false` when a cap is full or the rule is already stored.
    fn add_rule(&mut self, rule: Rule) -> bool {
        match rule {
            Rule::Host(host) => self.hosts.len() < MAX_HOST_RULES && self.hosts.insert(host.into()),
            Rule::Path { host, pattern } => {
                if self.path_count >= MAX_PATH_RULES {
                    return false;
                }
                self.paths.entry(host.into()).or_default().push(pattern.into());
                self.path_count += 1;
                true
            }
            Rule::Exact { host, path } => {
                if self.path_count >= MAX_PATH_RULES {
                    return false;
                }
                let paths = self.exact.entry(host.into()).or_default();
                if paths.iter().any(|known| **known == *path) {
                    return false;
                }
                paths.push(path.into());
                self.path_count += 1;
                true
            }
            Rule::Substring {
                pattern,
                third_party_only,
                domains,
            } => {
                if self.substrings.len() >= MAX_SUBSTRING_RULES {
                    return false;
                }
                self.substrings.push(SubstringRule {
                    pattern: pattern.into(),
                    third_party_only,
                    domains: domains.into_iter().map(Into::into).collect(),
                });
                true
            }
            Rule::Cosmetic { domains, selector } => {
                if self.cosmetics.len() >= MAX_COSMETIC_RULES {
                    return false;
                }
                self.cosmetics.push(CosmeticRule {
                    domains: domains.into_iter().map(Into::into).collect(),
                    selector: selector.into(),
                });
                true
            }
        }
    }

    pub fn rule_count(&self) -> usize {
        self.hosts.len() + self.path_count + self.substrings.len() + self.cosmetics.len()
    }

    /// Every stored host rule (not path or substring rules), for building DevTools patterns.
    pub fn hosts(&self) -> impl Iterator<Item = &str> {
        self.hosts.iter().map(|host| host.as_ref())
    }

    /// Every `||host/path|` rule as (host, path).
    pub fn exact_urls(&self) -> impl Iterator<Item = (&str, &str)> {
        self.exact
            .iter()
            .flat_map(|(host, paths)| paths.iter().map(move |path| (host.as_ref(), path.as_ref())))
    }

    /// Every `||host/path` prefix rule as (host, path pattern).
    fn path_rules(&self) -> impl Iterator<Item = (&str, &str)> {
        self.paths
            .iter()
            .flat_map(|(host, rules)| rules.iter().map(move |rule| (host.as_ref(), rule.as_ref())))
    }

    /// WebView2 `WebResourceRequested` filter URIs for the requests the in-process matcher still
    /// has to see: the rules DevTools `Network.setBlockedURLs` cannot enforce exactly. Host rules
    /// and plain lowercase `||host/path` rules are not here; they fail in the network stack.
    ///
    /// * a substring rule: `*text*`, since it can match on any host;
    /// * a prefix rule DevTools cannot match exactly (a letter whose case DevTools would compare,
    ///   `^`, `?`, or a `*` that may cross into the query): `*://*host` plus the path, then `*`;
    /// * a `||host/path|` rule with a letter in its path: `*://*host` plus the path, nothing after.
    ///
    /// Letters become `?` (see [`request_filter_text`]), so a filter never depends on how
    /// WebView2 compares case. Filters may let through more requests than their rule blocks; the
    /// matcher decides those. Empty means no request needs the matcher; `["*"]` means one rule
    /// can match any URL, or there would be more than [`MAX_REQUEST_FILTERS`]. Sorted.
    pub fn request_filters(&self) -> Vec<String> {
        let mut filters = BTreeSet::new();
        for rule in &self.substrings {
            filters.insert(format!("*{}*", request_filter_text(&rule.pattern)));
        }
        for (host, pattern) in self.path_rules() {
            if !path_rule_is_exact_in_devtools(pattern) {
                filters.insert(format!("*://*{host}{}*", request_filter_text(pattern)));
            }
        }
        for (host, path) in self.exact_urls() {
            if path.bytes().any(|b| b.is_ascii_alphabetic()) {
                filters.insert(format!("*://*{host}{}", request_filter_text(path)));
            }
        }
        if filters.len() > MAX_REQUEST_FILTERS
            || filters.iter().any(|filter| filter.bytes().all(|b| b == b'*'))
        {
            return vec!["*".to_owned()];
        }
        filters.into_iter().collect()
    }

    /// True when `host` or one of its parent domains is a host rule.
    fn host_listed(&self, host: &str) -> bool {
        host_suffixes(host).any(|suffix| self.hosts.contains(suffix))
    }

    /// Whether the rules (ignoring first-party and exceptions) match this URL.
    pub fn matches(&self, url: &str) -> bool {
        let Some((host, path)) = split_url(url) else {
            return false;
        };
        self.matches_parts(&host, path, url, false, "")
    }

    fn matches_parts(
        &self,
        host: &str,
        path: &str,
        url: &str,
        first_party: bool,
        page_host: &str,
    ) -> bool {
        if !first_party {
            let path = capped(path, MAX_MATCH_TEXT_BYTES);
            for suffix in host_suffixes(host) {
                if self.hosts.contains(suffix) {
                    return true;
                }
                if let Some(exact) = self.exact.get(suffix)
                    && exact.iter().any(|exact| path.eq_ignore_ascii_case(exact))
                {
                    return true;
                }
                if let Some(patterns) = self.paths.get(suffix)
                    && patterns
                        .iter()
                        .any(|pattern| glob_prefix(pattern.as_bytes(), path.as_bytes()))
                {
                    return true;
                }
            }
        }
        let url = capped(url, MAX_MATCH_TEXT_BYTES);
        self.substrings.iter().any(|rule| {
            if rule.third_party_only && first_party {
                return false;
            }
            if !domain_list_matches(page_host, &rule.domains) {
                return false;
            }
            contains_glob(rule.pattern.as_bytes(), url.as_bytes())
        })
    }

    /// The cosmetic CSS for `page_host`: every matching rule's selector turned into
    /// `selector{display:none!important}` and concatenated, or `None` when nothing matches.
    pub fn cosmetic_css_for_host(&self, page_host: &str) -> Option<String> {
        let mut css = String::new();
        for rule in &self.cosmetics {
            if domain_list_matches(page_host, &rule.domains) {
                css.push_str(&rule.selector);
                css.push_str("{display:none!important}");
            }
        }
        (!css.is_empty()).then_some(css)
    }
}

/// Why a request was blocked, so the WebView2 hook can fail it the right way.
///
/// A host rule has to fail in the network stack. An HTTP response, including 403, still
/// resolves a page's `no-cors` fetch, and reachability probes count that as "not blocked".
/// A request DevTools did not fail (a substring rule, a path rule's case or query variant, a
/// worker's request) reaches the hook only through [`Blocker::request_filters`], and the hook
/// answers it itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockHit {
    None,
    Host,
    Rule,
}

/// Matcher plus exceptions plus the enable switch; shared (behind `Rc<RefCell<..>>`) by the
/// WebView2 hook and the UI toggle.
pub struct BlockerState {
    /// The master switch (`blocker_enabled`): off means the global level is "none".
    pub enabled: bool,
    /// The level while `enabled` (`blocker_level`).
    pub level: Level,
    /// Trackers and ads: what the "trackers and ads" level matches with.
    matcher: Blocker,
    /// Trackers only: what the "trackers" level matches with.
    trackers: Blocker,
    cookies: cookies::CookieRules,
    /// Rules each enabled source contributed to the last build, by source id.
    source_counts: HashMap<String, usize>,
    /// Cookie-notice hiding (`cookie_notices`); a site's own choice wins.
    pub cookie_notices: bool,
    config_allow: HashSet<String>,
    site_rules: site_rules::SiteRules,
    sites_path: Option<PathBuf>,
    /// Built on first use and rebuilt after the rules change.
    devtools: OnceCell<DevtoolsPatterns>,
    devtools_trackers: OnceCell<DevtoolsPatterns>,
    /// Bumped whenever the rules or the per-site choices change, so a tab that already pushed its
    /// DevTools patterns knows to push them again.
    generation: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SiteToggle {
    /// Blocking is now off for this site.
    Allowed,
    /// Blocking is back on for this site.
    Blocking,
    /// The site is exempted by `blocker_allow` in settings.txt; edit that file instead.
    ConfiguredInSettings,
    /// The host has no usable site (new tab, IP address, ...).
    NoSite,
}

/// How a level or cookie-notice choice made in the shield ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SiteChoice {
    /// Saved (or, when it equals the global choice, the site's own choice was cleared).
    Saved,
    /// The site is exempted by `blocker_allow` in settings.txt; edit that file instead.
    ConfiguredInSettings,
    /// The host has no usable site, or the list of sites is full.
    NoSite,
}

impl BlockerState {
    /// A state around one matcher that holds both trackers and ads (no kind information): the
    /// "trackers" level then matches nothing. `load` builds the real split.
    pub fn new(matcher: Blocker, config_allow: &[String]) -> Self {
        Self {
            enabled: true,
            level: Level::Ads,
            matcher,
            trackers: Blocker::default(),
            cookies: cookies::CookieRules::default(),
            source_counts: HashMap::new(),
            cookie_notices: false,
            config_allow: config_allow
                .iter()
                .filter_map(|site| normalise_site(site))
                .collect(),
            site_rules: site_rules::SiteRules::default(),
            sites_path: None,
            devtools: OnceCell::new(),
            devtools_trackers: OnceCell::new(),
            generation: 0,
        }
    }

    /// Loads the enabled rule sources (see [`sources`]) and the persisted per-site choices from
    /// `dir`.
    pub fn load(dir: &Path, config_allow: &[String]) -> Self {
        let mut state = Self::matcher_only(dir);
        state.set_config_allow(config_allow);
        let path = dir.join(SITES_FILE_NAME);
        if let Some(text) = read_capped(&path, MAX_SITES_BYTES) {
            state.site_rules = site_rules::SiteRules::parse(&text);
        }
        state.sites_path = Some(path);
        state
    }

    /// A state with only the rule sources from `dir`: no site choices are read or saved. Private
    /// tabs use it when the global blocker is off.
    pub fn matcher_only(dir: &Path) -> Self {
        let built = sources::build(dir, &sources::Registry::load(dir));
        let mut state = Self::new(built.all, &[]);
        state.trackers = built.trackers;
        state.cookies = built.cookies;
        state.source_counts = built.counts;
        state
    }

    /// How many rules the enabled source `id` contributed to the last build (0 when it is off).
    pub fn source_rule_count(&self, id: &str) -> usize {
        self.source_counts.get(id).copied().unwrap_or(0)
    }

    /// Replaces the rules with a fresh build of the enabled sources in `dir` (after a source was
    /// switched, added or removed). Per-site choices and switches stay.
    pub fn reload_sources(&mut self, dir: &Path) {
        let built = sources::build(dir, &sources::Registry::load(dir));
        self.matcher = built.all;
        self.trackers = built.trackers;
        self.cookies = built.cookies;
        self.source_counts = built.counts;
        self.rules_changed();
    }

    /// Forget what was derived from the rules, and tell tabs to push their patterns again.
    fn rules_changed(&mut self) {
        self.devtools = OnceCell::new();
        self.devtools_trackers = OnceCell::new();
        self.generation = self.generation.wrapping_add(1);
    }

    /// Takes the blocker settings (`blocker_enabled`, `blocker_level`, `cookie_notices` and the
    /// `blocker_allow` sites) from `settings`. Pages already open keep what they loaded; tabs push
    /// new DevTools patterns on their next navigation.
    pub fn apply_settings(&mut self, settings: &crate::settings::Settings) {
        let level = match settings.blocker_level {
            crate::settings::BlockerLevel::Trackers => Level::Trackers,
            crate::settings::BlockerLevel::Ads => Level::Ads,
        };
        let allow: HashSet<String> = settings.blocker_allow.iter().filter_map(|site| normalise_site(site)).collect();
        let changed = self.enabled != settings.blocker_enabled || self.level != level || self.config_allow != allow;
        self.enabled = settings.blocker_enabled;
        self.level = level;
        self.cookie_notices = settings.cookie_notices;
        self.config_allow = allow;
        if changed {
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// Replaces the sites `blocker_allow` in settings.txt exempts.
    pub fn set_config_allow(&mut self, config_allow: &[String]) {
        self.config_allow = config_allow.iter().filter_map(|site| normalise_site(site)).collect();
        self.rules_changed();
    }

    pub fn rule_count(&self) -> usize {
        self.matcher.rule_count()
    }

    pub fn cookie_rule_count(&self) -> usize {
        self.cookies.rule_count()
    }

    /// Changes when the rules or any site choice change: a tab compares it with the value it last
    /// pushed DevTools patterns for.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// The global level: the switch and the chosen level together.
    pub fn global_level(&self) -> Level {
        if self.enabled { self.level } else { Level::None }
    }

    /// How much the blocker stops for a document on `top_host`. A private tab always gets the full
    /// level and ignores every exception; a normal tab takes the site's `blocker_allow` entry
    /// first, then its own saved level, then the global level.
    pub fn level_for(&self, top_host: &str, private: bool) -> Level {
        if private {
            return Level::Ads;
        }
        if self.config_allow_hit(top_host) {
            return Level::None;
        }
        self.site_rules.level_for(top_host).unwrap_or_else(|| self.global_level())
    }

    fn config_allow_hit(&self, top_host: &str) -> bool {
        host_suffixes(top_host).any(|suffix| self.config_allow.contains(suffix))
    }

    /// Whether cookie notices are hidden on a document on `top_host`. A private tab follows the
    /// global switch only: a site's saved choice is a preference Bowser keeps, and a private tab
    /// ignores those.
    pub fn cookie_notices_for(&self, top_host: &str, private: bool) -> bool {
        if private {
            return self.cookie_notices;
        }
        self.site_rules.cookies_for(top_host).unwrap_or(self.cookie_notices)
    }

    /// True when blocking is switched off for the page on `top_host` by a `blocker_allow` entry or
    /// the site's own saved level (the old per-site exception).
    pub fn site_allowed(&self, top_host: &str) -> bool {
        self.config_allow_hit(top_host) || self.site_rules.level_for(top_host) == Some(Level::None)
    }

    /// Every site with a saved choice, as (site, rule), in site order.
    pub fn site_rules(&self) -> Vec<(String, site_rules::SiteRule)> {
        self.site_rules.iter().map(|(site, rule)| (site.to_owned(), rule)).collect()
    }

    /// The sites `blocker_allow` exempts, sorted.
    pub fn config_allowed_sites(&self) -> Vec<String> {
        let mut sites: Vec<String> = self.config_allow.iter().cloned().collect();
        sites.sort();
        sites
    }

    /// Saves a level for one site (`None` goes back to the global level). `false` when the site
    /// is not a usable one or the list is full.
    pub fn set_site_level(&mut self, site: &str, level: Option<Level>) -> bool {
        let Some(site) = normalise_site(site) else {
            return false;
        };
        let changed = self.site_rules.set_level(&site, level);
        if changed {
            self.sites_changed();
        }
        changed
    }

    /// Saves the cookie-notice choice for one site (`None` goes back to the global switch).
    pub fn set_site_cookies(&mut self, site: &str, cookies: Option<bool>) -> bool {
        let Some(site) = normalise_site(site) else {
            return false;
        };
        let changed = self.site_rules.set_cookies(&site, cookies);
        if changed {
            self.sites_changed();
        }
        changed
    }

    /// Forgets every saved choice for a site. `true` when there was one.
    pub fn remove_site_rule(&mut self, site: &str) -> bool {
        let Some(site) = normalise_site(site) else {
            return false;
        };
        let removed = self.site_rules.remove(&site);
        if removed {
            self.sites_changed();
        }
        removed
    }

    /// The shield's level choice for the page on `top_host`: what this site should get from now on.
    /// A level that equals the global one clears the site's own choice instead of saving a copy of
    /// it, so the site follows the global level again; any other level is saved. The rule in force
    /// (it may be for a subdomain) is the one that changes, else the site's own.
    pub fn choose_level_for_page(&mut self, top_host: &str, level: Level) -> SiteChoice {
        let top_host = top_host.to_ascii_lowercase();
        if top_host.is_empty() || !top_host.contains('.') {
            return SiteChoice::NoSite;
        }
        if self.config_allow_hit(&top_host) {
            return SiteChoice::ConfiguredInSettings;
        }
        let key = host_suffixes(&top_host)
            .find(|suffix| self.site_rules.get(suffix).is_some_and(|rule| rule.level.is_some()))
            .map_or_else(|| registrable_domain(&top_host).to_owned(), str::to_owned);
        let wanted = (level != self.global_level()).then_some(level);
        if self.site_rules.set_level(&key, wanted) {
            self.sites_changed();
            SiteChoice::Saved
        } else {
            SiteChoice::NoSite
        }
    }

    /// The shield's cookie-notice choice for the page on `top_host`: hide them or leave them. A
    /// choice that equals the global switch clears the site's own choice.
    pub fn choose_cookie_notices_for_page(&mut self, top_host: &str, hide: bool) -> SiteChoice {
        let top_host = top_host.to_ascii_lowercase();
        if top_host.is_empty() || !top_host.contains('.') {
            return SiteChoice::NoSite;
        }
        let key = host_suffixes(&top_host)
            .find(|suffix| self.site_rules.get(suffix).is_some_and(|rule| rule.cookies.is_some()))
            .map_or_else(|| registrable_domain(&top_host).to_owned(), str::to_owned);
        let wanted = (hide != self.cookie_notices).then_some(hide);
        if self.site_rules.set_cookies(&key, wanted) {
            self.sites_changed();
            SiteChoice::Saved
        } else {
            SiteChoice::NoSite
        }
    }

    fn sites_changed(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        let _ = self.save_sites();
    }

    /// The decision for one subresource request made by a page on `top_host`.
    pub fn should_block(&self, request_url: &str, top_host: &str) -> bool {
        self.decide(request_url, top_host, false)
    }

    /// The decision for a request made by a private tab: the blocker is always on, so neither the
    /// enable switch nor any per-site exception applies.
    pub fn should_block_private(&self, request_url: &str, top_host: &str) -> bool {
        self.decide(request_url, top_host, true)
    }

    /// As [`Self::should_block`], but distinguishes a host-rule hit from any other rule.
    pub fn block_hit(&self, request_url: &str, top_host: &str) -> BlockHit {
        self.classify(request_url, top_host, false)
    }

    /// As [`Self::block_hit`], for a private tab.
    pub fn block_hit_private(&self, request_url: &str, top_host: &str) -> BlockHit {
        self.classify(request_url, top_host, true)
    }

    fn decide(&self, request_url: &str, top_host: &str, private: bool) -> bool {
        self.classify(request_url, top_host, private) != BlockHit::None
    }

    fn classify(&self, request_url: &str, top_host: &str, private: bool) -> BlockHit {
        let matcher = match self.level_for(top_host, private) {
            Level::None => return BlockHit::None,
            Level::Trackers => &self.trackers,
            Level::Ads => &self.matcher,
        };
        let Some((host, path)) = split_url(request_url) else {
            return BlockHit::None;
        };
        let first_party = !top_host.is_empty() && same_site(&host, top_host);
        if !first_party && matcher.host_listed(&host) {
            return BlockHit::Host;
        }
        if matcher.matches_parts(&host, path, request_url, first_party, top_host) {
            BlockHit::Rule
        } else {
            BlockHit::None
        }
    }

    /// Whether the blocker stops anything at all for a document on `top_host`: its level is not
    /// "none". Always true for a private tab.
    pub fn is_blocking(&self, top_host: &str, private: bool) -> bool {
        self.level_for(top_host, private) != Level::None
    }

    /// What changes the DevTools patterns and cosmetic rules of a document on `top_host`: its
    /// level and the rule generation. A tab sends them again only when this changes.
    pub fn filter_signature(&self, top_host: &str, private: bool) -> (Level, u32) {
        (self.level_for(top_host, private), self.generation)
    }

    /// The CSS to inject for a normal tab's document on `page_host`: the element-hiding rules of
    /// the "trackers and ads" level when the site is at that level, then the cookie-notice rules
    /// when they apply to the site. `None` when neither adds anything.
    pub fn cosmetic_css_for_host(&self, page_host: &str) -> Option<String> {
        self.cosmetic(page_host, false)
    }

    /// As [`Self::cosmetic_css_for_host`], but for a private tab: the blocker always applies and
    /// site exceptions never do.
    pub fn cosmetic_css_for_host_private(&self, page_host: &str) -> Option<String> {
        self.cosmetic(page_host, true)
    }

    fn cosmetic(&self, page_host: &str, private: bool) -> Option<String> {
        let mut css = String::new();
        if self.level_for(page_host, private) == Level::Ads {
            if let Some(ads) = self.matcher.cosmetic_css_for_host(page_host) {
                css.push_str(&ads);
            }
        }
        if self.cookie_notices_for(page_host, private) {
            if let Some(notices) = self.cookies.css_for_host(page_host) {
                css.push_str(&notices);
            }
        }
        (!css.is_empty()).then_some(css)
    }

    /// DevTools `Network.setBlockedURLs` patterns for a normal tab's document on `page_host`
    /// (empty when the blocker does not apply there).
    pub fn blocked_url_patterns(&self, page_host: &str) -> Vec<String> {
        self.devtools_patterns(page_host, self.level_for(page_host, false))
    }

    /// As [`Self::blocked_url_patterns`], but for a private tab.
    pub fn blocked_url_patterns_private(&self, page_host: &str) -> Vec<String> {
        self.devtools_patterns(page_host, self.level_for(page_host, true))
    }

    /// Host patterns, then the exact-URL patterns, then the prefix-rule patterns, each sorted,
    /// minus those for `page_host`'s own site. Each level's full list is built once (see
    /// [`DevtoolsPatterns`]); each call only filters it.
    fn devtools_patterns(&self, page_host: &str, level: Level) -> Vec<String> {
        match level {
            Level::None => Vec::new(),
            Level::Trackers => self
                .devtools_trackers
                .get_or_init(|| DevtoolsPatterns::new(&self.trackers))
                .for_page(page_host),
            Level::Ads => self
                .devtools
                .get_or_init(|| DevtoolsPatterns::new(&self.matcher))
                .for_page(page_host),
        }
    }

    /// WebView2 request filters for the rules DevTools cannot enforce; see
    /// [`Blocker::request_filters`]. The same for every page, so a tab installs them once.
    pub fn request_filters(&self) -> Vec<String> {
        self.matcher.request_filters()
    }

    /// DevTools Response-stage patterns for the YouTube player responses, for a document on
    /// `page_host`: only when that is YouTube (`youtube.com` / `youtube-nocookie.com` or a
    /// subdomain), the blocker is switched on for the tab at all and the `block_youtube_video_ads`
    /// option is on. They do not follow the current page's site exception, so the first watch page
    /// opened from an excepted site still pauses; each paused response is then decided by
    /// [`Self::player_ads_rewrite`]. Any other site pauses nothing, so its responses never wait.
    ///
    /// The option is read from the active profile's `settings.txt` (reread only when the file
    /// changes) so toggling it off takes effect without a restart: a navigation with the option off
    /// enables no Response-stage pause. A non-YouTube page does not look at the file at all.
    pub fn response_patterns(
        &self,
        page_host: &str,
        private: bool,
    ) -> &'static [youtube::ResponsePattern] {
        if !youtube::is_youtube_host(page_host) {
            return &[];
        }
        self.response_patterns_gated(page_host, private, youtube_video_ads_enabled())
    }

    fn response_patterns_gated(
        &self,
        page_host: &str,
        private: bool,
        video_ads_on: bool,
    ) -> &'static [youtube::ResponsePattern] {
        // Ads are only stripped at the "trackers and ads" level; a site's own level may raise or
        // lower the global one, so any site at that level keeps the pause available.
        let ads_possible = private
            || self.global_level() == Level::Ads
            || self.site_rules.any_level(Level::Ads);
        if video_ads_on && youtube::is_youtube_host(page_host) && ads_possible {
            &youtube::RESPONSE_PATTERNS
        } else {
            &[]
        }
    }

    /// Which player response to strip of its ads for a response to `url`, or `None` to pass it
    /// through. A watch page is the tab's next document, so its own site decides; any other
    /// response follows the page on `top_host`, like every request rule. A private tab always
    /// strips; a normal tab honours the switch and site exceptions.
    ///
    /// Returns `None` for every response when the `block_youtube_video_ads` option is off, read
    /// fresh from the active profile's `settings.txt` so the choice takes effect without a restart.
    /// Video and audio come from googlevideo.com and are never touched here either way.
    pub fn player_ads_rewrite(
        &self,
        url: &str,
        top_host: &str,
        private: bool,
    ) -> Option<youtube::PlayerPayload> {
        self.player_ads_rewrite_gated(url, top_host, private, youtube_video_ads_enabled())
    }

    fn player_ads_rewrite_gated(
        &self,
        url: &str,
        top_host: &str,
        private: bool,
        video_ads_on: bool,
    ) -> Option<youtube::PlayerPayload> {
        if !video_ads_on {
            return None;
        }
        let payload = youtube::player_payload(url)?;
        let blocking = match payload {
            youtube::PlayerPayload::WatchPage => {
                let (host, _) = split_url(url)?;
                self.level_for(&host, private) == Level::Ads
            }
            youtube::PlayerPayload::Json => self.level_for(top_host, private) == Level::Ads,
        };
        blocking.then_some(payload)
    }

    /// Flips blocking for the site of `top_host` (Ctrl+Shift+B) and persists the choice: a site
    /// that is being blocked is switched to "none"; a site that is not goes back to the global
    /// level, or to the full level when the global one is "none".
    pub fn toggle_site(&mut self, top_host: &str) -> SiteToggle {
        let top_host = top_host.to_ascii_lowercase();
        if top_host.is_empty() || !top_host.contains('.') {
            return SiteToggle::NoSite;
        }
        let site = registrable_domain(&top_host).to_owned();
        if self.config_allow_hit(&top_host) {
            return SiteToggle::ConfiguredInSettings;
        }
        let (level, result) = if self.level_for(&top_host, false) != Level::None {
            (Some(Level::None), SiteToggle::Allowed)
        } else if self.global_level() == Level::None {
            (Some(Level::Ads), SiteToggle::Blocking)
        } else {
            (None, SiteToggle::Blocking)
        };
        // Change the saved rule that is in force (it may be for a subdomain), else the site's own.
        let key = host_suffixes(&top_host)
            .find(|suffix| self.site_rules.get(suffix).is_some_and(|rule| rule.level.is_some()))
            .map_or(site, str::to_owned);
        if !self.site_rules.set_level(&key, level) {
            return SiteToggle::NoSite;
        }
        self.sites_changed();
        result
    }

    fn save_sites(&self) -> io::Result<()> {
        let Some(path) = &self.sites_path else {
            return Ok(());
        };
        write_atomic(path, self.site_rules.render().as_bytes())
    }
}

fn normalise_site(entry: &str) -> Option<String> {
    let entry = entry.trim().trim_start_matches("*.").trim_matches('.');
    let entry = entry.to_ascii_lowercase();
    (!entry.starts_with('#') && valid_rule_host(&entry)).then_some(entry)
}

/// Whether YouTube video-ad stripping is switched on. The settings file is reread only when its
/// length or modified time changes, so a save on the settings page still takes effect without a
/// restart. No active profile, or a missing or unreadable file, means on — the default Bowser
/// has always used.
fn youtube_video_ads_enabled() -> bool {
    let Ok(profile) = crate::profile::active() else {
        return true;
    };
    youtube::video_ads_blocked_in_file(&profile.settings_file(), crate::settings::MAX_FILE_BYTES)
}

fn read_capped(path: &Path, cap: usize) -> Option<String> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > cap {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = path.with_extension("txt.tmp");
    let mut file = File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(list: &str, allow: &[&str]) -> BlockerState {
        let mut matcher = Blocker::default();
        matcher.add_list(list);
        let allow: Vec<String> = allow.iter().map(|s| (*s).to_owned()).collect();
        BlockerState::new(matcher, &allow)
    }

    #[test]
    fn parser_accepts_host_path_and_substring_rules() {
        assert_eq!(
            parse_line("||Ads.Example.com^"),
            Some(Rule::Host("ads.example.com".into()))
        );
        assert_eq!(
            parse_line("||ads.example.com^$third-party"),
            Some(Rule::Host("ads.example.com".into()))
        );
        assert_eq!(
            parse_line("||example.com"),
            Some(Rule::Host("example.com".into()))
        );
        assert_eq!(
            parse_line("||example.com/ads/*.js|"),
            Some(Rule::Path {
                host: "example.com".into(),
                pattern: "/ads/*.js".into()
            })
        );
        assert_eq!(
            parse_line("/banner/ads/"),
            Some(Rule::Substring {
                pattern: "/banner/ads/".into(),
                third_party_only: false,
                domains: Vec::new(),
            })
        );
    }

    #[test]
    fn parser_ignores_unsupported_lines() {
        for line in [
            "",
            "! comment",
            "[Adblock Plus 2.0]",
            "example.com#@#.ad",
            "@@||example.com^",
            "||example.com^$script",
            "||example.com^$domain=a.com",
            "||example.com^$~third-party",
            "|https://example.com/x",
            "/ads?id=\\d+/",
            "||localhost^",
            "||ex ample.com^",
            "||example.com:8080^",
            "||ads*.example.com^",
            "short",
        ] {
            assert_eq!(parse_line(line), None, "{line:?}");
        }
    }

    #[test]
    fn parser_accepts_cosmetic_rules_with_valid_selectors() {
        assert_eq!(
            parse_line("adblock.turtlecute.org##.adbox.banner_ads.adsbox"),
            Some(Rule::Cosmetic {
                domains: vec!["adblock.turtlecute.org".into()],
                selector: ".adbox.banner_ads.adsbox".into(),
            })
        );
        assert_eq!(
            parse_line("##.textads"),
            Some(Rule::Cosmetic {
                domains: Vec::new(),
                selector: ".textads".into(),
            })
        );
        assert_eq!(
            parse_line("a.com,b.com##.ad"),
            Some(Rule::Cosmetic {
                domains: vec!["a.com".into(), "b.com".into()],
                selector: ".ad".into(),
            })
        );
    }

    #[test]
    fn parser_rejects_cosmetic_rules_with_bad_grammar_or_negated_domains() {
        for line in [
            "example.com#@#.ad",            // exception, still ignored
            "example.com##.adbox .banner",  // space
            "example.com##div",             // bare tag, not a class/id
            "example.com##.ads[href]",      // attribute selector
            "example.com##.a{color:red}",   // braces
            "example.com##<script>",        // angle brackets
            "~example.com##.ad",            // negated domain
            "a.com,~b.com##.ad",            // negated domain in a list
        ] {
            assert_eq!(parse_line(line), None, "{line:?}");
        }
    }

    #[test]
    fn parser_accepts_domain_and_third_party_options_on_substring_rules() {
        assert_eq!(
            parse_line("/prebid.js"),
            Some(Rule::Substring {
                pattern: "/prebid.js".into(),
                third_party_only: false,
                domains: Vec::new(),
            })
        );
        assert_eq!(
            parse_line("/pagead.js$domain=adblock.turtlecute.org"),
            Some(Rule::Substring {
                pattern: "/pagead.js".into(),
                third_party_only: false,
                domains: vec!["adblock.turtlecute.org".into()],
            })
        );
        assert_eq!(
            parse_line("/ads/banner.gif$third-party,domain=a.com|b.com"),
            Some(Rule::Substring {
                pattern: "/ads/banner.gif".into(),
                third_party_only: true,
                domains: vec!["a.com".into(), "b.com".into()],
            })
        );
    }

    #[test]
    fn parser_rejects_domain_option_on_host_and_path_rules() {
        for line in [
            "||example.com^$domain=a.com",
            "||example.com/ads/*.js$domain=a.com",
        ] {
            assert_eq!(parse_line(line), None, "{line:?}");
        }
    }

    #[test]
    fn parser_rejects_negated_or_empty_domain_lists() {
        for line in [
            "/track1$domain=~a.com",
            "/track2$domain=",
            "/track3$domain=a.com|~b.com",
        ] {
            assert_eq!(parse_line(line), None, "{line:?}");
        }
    }

    #[test]
    fn bare_substring_rules_now_match_first_party_requests() {
        let s = state("/prebid.js\n", &[]);
        assert!(s.should_block("https://news.example/prebid.js", "news.example"));
        assert!(!s.should_block("https://news.example/app.js", "news.example"));
    }

    #[test]
    fn third_party_substring_rules_still_exempt_first_party() {
        let s = state("/track-me$third-party\n", &[]);
        assert!(!s.should_block("https://news.example/track-me", "news.example"));
        assert!(s.should_block("https://ads.example/track-me", "news.example"));
    }

    #[test]
    fn domain_restricted_substring_rules_apply_only_on_listed_pages() {
        let s = state("/pagead.js$domain=adblock.turtlecute.org\n", &[]);
        assert!(s.should_block(
            "https://adblock.turtlecute.org/js/pagead.js",
            "adblock.turtlecute.org"
        ));
        assert!(!s.should_block(
            "https://adblock.turtlecute.org/js/pagead.js",
            "news.example"
        ));
        assert!(!s.should_block("https://news.example/js/pagead.js", "news.example"));
    }

    #[test]
    fn host_and_path_rules_stay_third_party_only_with_the_new_parser() {
        let s = state("||tracker.net^\n||cdn.example.com/ads/*.js\n", &[]);
        assert!(!s.should_block("https://tracker.net/a.js", "tracker.net"));
        assert!(s.should_block("https://tracker.net/a.js", "news.example"));
        assert!(!s.should_block("https://cdn.example.com/ads/a.js", "cdn.example.com"));
    }

    #[test]
    fn cosmetic_css_for_host_matches_domain_and_generic_rules() {
        let s = state(
            "adblock.turtlecute.org##.adbox.banner_ads.adsbox\nadblock.turtlecute.org##.textads\n",
            &[],
        );
        let css = s
            .cosmetic_css_for_host("adblock.turtlecute.org")
            .expect("rules match");
        assert!(css.contains(".adbox.banner_ads.adsbox{display:none!important}"));
        assert!(css.contains(".textads{display:none!important}"));
        assert_eq!(s.cosmetic_css_for_host("news.example"), None);
    }

    #[test]
    fn cosmetic_css_for_host_includes_generic_rules_everywhere() {
        let s = state("##.textads\n", &[]);
        assert_eq!(
            s.cosmetic_css_for_host("anything.example"),
            Some(".textads{display:none!important}".to_owned())
        );
    }

    #[test]
    fn cosmetic_css_for_host_drops_rules_with_bad_selector_grammar() {
        let mut matcher = Blocker::default();
        assert_eq!(matcher.add_list("example.com##.ad .bad\n"), 0);
        let s = BlockerState::new(matcher, &[]);
        assert_eq!(s.cosmetic_css_for_host("example.com"), None);
    }

    #[test]
    fn cosmetic_css_for_host_respects_the_switch_and_site_exceptions() {
        let mut s = state("##.textads\n", &[]);
        assert!(s.cosmetic_css_for_host("news.example").is_some());
        s.toggle_site("news.example");
        assert_eq!(s.cosmetic_css_for_host("news.example"), None);
        assert!(s.cosmetic_css_for_host_private("news.example").is_some());
        s.enabled = false;
        assert_eq!(s.cosmetic_css_for_host("other.example"), None);
        assert!(s.cosmetic_css_for_host_private("other.example").is_some());
    }

    #[test]
    fn blocked_url_patterns_covers_host_and_subdomain_wildcards() {
        let patterns = blocked_url_patterns(["doubleclick.net"], "news.example", true);
        assert_eq!(
            patterns,
            vec![
                "*://*.doubleclick.net/*".to_owned(),
                "*://doubleclick.net/*".to_owned(),
            ]
        );
    }

    #[test]
    fn blocked_url_patterns_excludes_same_site_hosts_and_honours_the_blocking_flag() {
        assert!(blocked_url_patterns(["doubleclick.net"], "ad.doubleclick.net", true).is_empty());
        assert!(blocked_url_patterns(["doubleclick.net"], "news.example", false).is_empty());
        assert!(!blocked_url_patterns(["doubleclick.net"], "", true).is_empty());
    }

    #[test]
    fn blocker_state_blocked_url_patterns_reflects_enable_and_site_state() {
        let mut s = state("||doubleclick.net^\n", &[]);
        assert!(!s.blocked_url_patterns("news.example").is_empty());
        assert!(s.blocked_url_patterns("ad.doubleclick.net").is_empty());
        s.toggle_site("news.example");
        assert!(s.blocked_url_patterns("news.example").is_empty());
        assert!(!s.blocked_url_patterns_private("news.example").is_empty());
    }

    #[test]
    fn a_third_party_host_rule_is_a_host_hit_and_a_substring_is_not() {
        let s = state("||doubleclick.net^\n/pagead.js\n", &[]);
        assert_eq!(
            s.block_hit("https://ad.doubleclick.net/favicon.ico?x=1", "news.example"),
            BlockHit::Host
        );
        assert_eq!(
            s.block_hit("https://ad.doubleclick.net/favicon.ico", "ad.doubleclick.net"),
            BlockHit::None
        );
        assert_eq!(
            s.block_hit("https://adblock.turtlecute.org/js/pagead.js", "adblock.turtlecute.org"),
            BlockHit::Rule
        );
        let mut off = state("||doubleclick.net^\n", &[]);
        off.enabled = false;
        assert_eq!(
            off.block_hit("https://ad.doubleclick.net/favicon.ico", "news.example"),
            BlockHit::None
        );
        assert_eq!(
            off.block_hit_private("https://ad.doubleclick.net/favicon.ico", "news.example"),
            BlockHit::Host
        );
    }

    #[test]
    fn document_requests_use_the_same_third_party_host_rule() {
        let s = state("||doubleclick.net^\n", &[]);
        assert!(s.should_block(
            "https://ad.doubleclick.net/fakepage.html",
            "adblock.turtlecute.org"
        ));
        assert!(!s.should_block(
            "https://ad.doubleclick.net/fakepage.html",
            "ad.doubleclick.net"
        ));
        assert!(s.should_block("https://ad.doubleclick.net/fakepage.html", ""));
    }

    #[test]
    fn builtin_blocklist_blocks_the_turtlecute_ad_scripts() {
        let s = BlockerState::new(Blocker::from_lists(None), &[]);
        assert!(s.should_block(
            "https://adblock.turtlecute.org/js/pagead.js",
            "adblock.turtlecute.org"
        ));
        assert!(s.should_block(
            "https://adblock.turtlecute.org/js/widget/ads.js",
            "adblock.turtlecute.org"
        ));
        assert!(!s.should_block(
            "https://adblock.turtlecute.org/js/pagead.js",
            "news.example"
        ));
    }

    #[test]
    fn builtin_blocklist_covers_the_superadblocktest_host_gaps() {
        const NEW_HOSTS: &[&str] = &[
            "advertising-api-eu.amazon.com",
            "aan.amazon.com",
            "bingads.microsoft.com",
            "ads.microsoft.com",
            "c.bing.com",
            "onclickads.net",
            "liftoff.io",
            "htlbid.com",
            "advertising.yahoo.com",
            "unityads.unity3d.com",
            "advertising.yandex.ru",
            "ironsource.mobi",
            "smartyads.com",
            "ad.gt",
            "tagmanager.google.com",
            "analytics.adobe.io",
            "metrics.adobe.com",
            "cloudflareinsights.com",
            "us.i.posthog.com",
            "rudderlabs.com",
            "snowplowanalytics.com",
            "permutive.com",
            "uidapi.com",
            "api2.branch.io",
            "bnc.lt",
            "wzrkt.com",
            "ingest.sentry.io",
            "browser-intake-datadoghq.com",
            "lr-ingest.com",
            "coinimp.com",
            "webminepool.com",
            "minero.cc",
            "mineralt.io",
            "monerominer.rocks",
            "statdynamic.com",
            "tr.facebook.com",
            "ads.snapchat.com",
            "sc-analytics.appspot.com",
            "ads-api.x.com",
            "analytics.x.com",
            "ads.x.com",
            "pixel.redditmedia.com",
            "d.reddit.com",
            "mon.byteoversea.com",
            "mcs-va.tiktokv.com",
            "mon.tiktokv.com",
            "pixel.quora.com",
            "qevents.quora.com",
            "px.srvcs.tumblr.com",
            "ads.vk.com",
            "ad.mail.ru",
            "xp.apple.com",
            "ads.huawei.com",
            "adlog.vivo.com",
            "ads-api.vivo.com",
            "a.lenovo.com",
            "lgsmartad.com",
            "us.ibs.lgappstv.com",
            "ad.lgappstv.com",
            "yumenetworks.com",
            "smartclip.com",
            "settings-win.data.microsoft.com",
            "vortex.data.microsoft.com",
            "vortex-win.data.microsoft.com",
            "telemetry.microsoft.com",
            "browser.events.data.msn.com",
            "device-metrics-us.amazon.com",
            "device-metrics-us-2.amazon.com",
            "mads-eu.amazon.com",
            "logs.roku.com",
            "ads.roku.com",
            "amoeba.web.roku.com",
            "ads.vizio.com",
            "tvinteractive.tv",
            "tvpixel.com",
            "firebase-settings.crashlytics.com",
            "geolocation.onetrust.com",
            "cmp.usercentrics.eu",
            "sourcepoint.mgr.consensu.org",
            "impactradius-event.com",
            "partnerstack.com",
            "refersion.com",
            "pepperjamnetwork.com",
            "redirectingat.com",
            "events.launchdarkly.com",
            "track.hubspot.com",
            "click.mailchimp.com",
            "braze.com",
            "customer.io",
            "jwpltx.com",
            "connatix.com",
            "metrics.brightcove.com",
        ];
        let s = BlockerState::new(Blocker::from_lists(None), &[]);
        let patterns = s.blocked_url_patterns("superadblocktest.com");
        for host in NEW_HOSTS {
            assert_eq!(
                s.block_hit(&format!("https://{host}/"), "superadblocktest.com"),
                BlockHit::Host,
                "{host}"
            );
            assert!(
                patterns.contains(&format!("*://{host}/*")),
                "{host}"
            );
        }
        for url in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://s.youtube.com/api/stats/playback?ns=yt",
            "https://redirector.googlevideo.com/videoplayback?id=x&itag=22",
            "https://vk.com/",
            "https://www.facebook.com/",
            "https://www.google.com/",
            "https://posthog.com/",
            "https://sentry.io/",
        ] {
            assert_eq!(
                s.block_hit(url, "superadblocktest.com"),
                BlockHit::None,
                "{url}"
            );
        }
        for pattern in [
            "*://youtube.com/*",
            "*://*.youtube.com/*",
            "*://googlevideo.com/*",
            "*://*.googlevideo.com/*",
        ] {
            assert!(!patterns.contains(&pattern.to_owned()), "{pattern}");
        }
    }

    #[test]
    fn builtin_blocklist_cosmetic_rules_hide_the_turtlecute_ad_nodes() {
        let s = BlockerState::new(Blocker::from_lists(None), &[]);
        let css = s
            .cosmetic_css_for_host("adblock.turtlecute.org")
            .expect("built-in cosmetic rules match the tester page");
        assert!(css.contains(".adbox.banner_ads.adsbox{display:none!important}"));
        assert!(css.contains(".textads{display:none!important}"));
    }

    #[test]
    fn a_previously_missing_tester_host_is_now_blocked() {
        let s = BlockerState::new(Blocker::from_lists(None), &[]);
        assert!(s.should_block(
            "https://click.googleanalytics.com/collect",
            "adblock.turtlecute.org"
        ));
    }

    #[test]
    fn host_rules_match_subdomains_but_not_lookalikes() {
        let s = state("||tracker.net^\n", &[]);
        for url in [
            "https://tracker.net/x.js",
            "https://cdn.tracker.net/x.js",
            "http://A.B.TRACKER.NET:8080/p?q=1",
            "https://user@tracker.net/",
        ] {
            assert!(s.should_block(url, "news.example"), "{url}");
        }
        for url in [
            "https://nottracker.net/x.js",
            "https://tracker.net.evil.example/x.js",
            "https://example.org/tracker.net",
            "ftp://tracker.net/",
            "https://[::1]/",
        ] {
            assert!(!s.should_block(url, "news.example"), "{url}");
        }
    }

    #[test]
    fn path_rules_glob_and_separator() {
        let s = state("||cdn.example.com/ads/*.js\n||example.net/track^\n", &[]);
        assert!(s.should_block("https://cdn.example.com/ads/a/b.js", "site.test"));
        assert!(s.should_block("https://CDN.example.com/ADS/x.JS?v=1", "site.test"));
        assert!(!s.should_block("https://cdn.example.com/lib/b.js", "site.test"));
        assert!(!s.should_block("https://cdn.example.com/ads/b.css", "site.test"));
        assert!(s.should_block("https://example.net/track", "site.test"));
        assert!(s.should_block("https://www.example.net/track?x=1", "site.test"));
        assert!(!s.should_block("https://example.net/tracker", "site.test"));
    }

    #[test]
    fn substring_rules_are_case_insensitive_and_capped() {
        let s = state("/adserver/banner.\n", &[]);
        assert!(s.should_block("https://x.example.com/AdServer/Banner.gif", "site.test"));
        assert!(!s.should_block("https://x.example.com/adserver/other.gif", "site.test"));
        let many: String = (0..1000)
            .map(|i| format!("/unique-path-{i}-x/\n"))
            .collect();
        let mut blocker = Blocker::default();
        assert_eq!(blocker.add_list(&many), MAX_SUBSTRING_RULES);
    }

    #[test]
    fn first_party_requests_are_never_blocked() {
        let s = state("||tracker.net^\n||ads.example.com^\n", &[]);
        assert!(!s.should_block("https://tracker.net/a.js", "tracker.net"));
        assert!(!s.should_block("https://cdn.tracker.net/a.js", "www.tracker.net"));
        assert!(!s.should_block("https://ads.example.com/a.js", "www.example.com"));
        assert!(s.should_block("https://ads.example.com/a.js", "www.example.org"));
        // Unknown top site: nothing is first-party, rules still apply.
        assert!(s.should_block("https://tracker.net/a.js", ""));
    }

    #[test]
    fn registrable_domain_uses_the_public_suffix_list() {
        assert_eq!(registrable_domain("a.b.example.co.uk"), "example.co.uk");
        assert_eq!(registrable_domain("example.com"), "example.com");
        assert_eq!(registrable_domain("localhost"), "localhost");
        assert!(same_site("a.example.co.uk", "b.example.co.uk"));
        assert!(!same_site("a.co.uk", "b.co.uk"));
    }

    #[test]
    fn allow_list_and_disabled_switch_stop_blocking() {
        let mut s = state(
            "||tracker.net^\n",
            &["Shop.Example.com", "bad entry", "# no"],
        );
        assert!(!s.should_block("https://tracker.net/a.js", "shop.example.com"));
        assert!(!s.should_block("https://tracker.net/a.js", "m.shop.example.com"));
        assert!(s.should_block("https://tracker.net/a.js", "example.com"));
        s.enabled = false;
        assert!(!s.should_block("https://tracker.net/a.js", "example.org"));
    }

    #[test]
    fn private_requests_ignore_the_switch_and_every_site_exception() {
        let mut s = state("||tracker.net^
", &["shop.example.com"]);
        // A normal tab on an excepted site is not blocked; a private tab there is.
        assert!(!s.should_block("https://tracker.net/a.js", "shop.example.com"));
        assert!(s.should_block_private("https://tracker.net/a.js", "shop.example.com"));
        s.enabled = false;
        assert!(!s.should_block("https://tracker.net/a.js", "example.org"));
        assert!(s.should_block_private("https://tracker.net/a.js", "example.org"));
        // First-party requests and unmatched requests are still left alone.
        assert!(!s.should_block_private("https://tracker.net/a.js", "tracker.net"));
        assert!(!s.should_block_private("https://fine.example/a.js", "example.org"));
    }

    #[test]
    fn runtime_toggle_is_per_registrable_domain_and_persists() {
        let dir = std::env::temp_dir().join(format!("bowser-blocker-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(USER_LIST_NAME), "||extra-tracker.org^\n").unwrap();
        let mut s = BlockerState::load(&dir, &["configured.com".to_owned()]);
        assert!(s.rule_count() > 100, "built-in list is loaded");
        assert!(s.should_block("https://extra-tracker.org/x", "page.example"));
        assert_eq!(s.toggle_site("localhost"), SiteToggle::NoSite);
        assert_eq!(s.toggle_site("www.page.test"), SiteToggle::Allowed);
        assert!(!s.should_block("https://extra-tracker.org/x", "other.page.test"));
        assert_eq!(
            s.toggle_site("configured.com"),
            SiteToggle::ConfiguredInSettings
        );
        let reloaded = BlockerState::load(&dir, &[]);
        assert!(!reloaded.should_block("https://extra-tracker.org/x", "page.test"));
        assert_eq!(s.toggle_site("page.test"), SiteToggle::Blocking);
        assert!(s.should_block("https://extra-tracker.org/x", "page.test"));
        assert!(
            BlockerState::load(&dir, &[]).should_block("https://extra-tracker.org/x", "page.test")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_user_list_is_ignored() {
        let dir = std::env::temp_dir().join(format!("bowser-blocker-big-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut big = String::from("||huge-tracker.org^\n");
        big.push_str(&"!".repeat(MAX_USER_LIST_BYTES));
        fs::write(dir.join(USER_LIST_NAME), big).unwrap();
        let s = BlockerState::load(&dir, &[]);
        assert!(!s.should_block("https://huge-tracker.org/", "page.test"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn builtin_list_blocks_known_trackers_and_spares_common_sites() {
        let s = BlockerState::new(Blocker::from_lists(None), &[]);
        assert!(s.should_block(
            "https://www.google-analytics.com/analytics.js",
            "news.example"
        ));
        assert!(s.should_block(
            "https://securepubads.g.doubleclick.net/tag/js/gpt.js",
            "news.example"
        ));
        for ok in [
            "https://www.google.com/search?q=x",
            "https://fonts.gstatic.com/s/a.woff2",
            "https://cdn.jsdelivr.net/npm/x.js",
            "https://www.youtube.com/embed/x",
            "https://github.githubassets.com/a.js",
        ] {
            assert!(!s.should_block(ok, "news.example"), "{ok}");
        }
    }

    #[test]
    fn glob_matching_is_correct_with_many_wildcards() {
        assert!(glob_core(b"a*b*c", b"xxaqbqqcxx", false));
        assert!(!glob_core(b"a*b*c", b"xxaqbqqxx", false));
        assert!(glob_core(b"a*b", b"ab", true));
        assert!(!glob_core(b"b*", b"ab", true), "anchored at the start");
        assert!(glob_core(b"b*", b"ab", false));
        assert!(glob_core(b"a^b", b"a/b", true));
        assert!(
            glob_core(b"a^", b"a", true),
            "separator matches end of text"
        );
        assert!(!glob_core(b"a^b", b"axb", true));
    }

    #[test]
    fn huge_urls_are_matched_in_bounded_time() {
        // A rule whose backtracking would be quadratic on an all-'a' URL, against a 100 KB URL.
        let s = state("/a*a*a*a*a*a*a*a*b\n||example.net/x*y*z\n", &[]);
        let long = format!("https://site.test/{}", "a".repeat(100_000));
        let started = std::time::Instant::now();
        assert!(!s.should_block(&long, "news.example"));
        let mut tail = format!("https://site.test/{}", "a".repeat(100_000));
        tail.push_str("b");
        assert!(
            !s.should_block(&tail, "news.example"),
            "the match lies beyond the cap, so it is not seen"
        );
        let path_long = format!("https://example.net/x{}z", "y".repeat(100_000));
        assert!(
            !s.should_block(&path_long, "news.example"),
            "z lies beyond the cap"
        );
        assert!(s.should_block("https://example.net/xAyBz", "news.example"));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "took {:?}",
            started.elapsed()
        );
    }

    const YOUTUBE: &str = "www.youtube.com";
    const YOUTUBE_WATCH: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    const YOUTUBE_PLAYER: &str = "https://www.youtube.com/youtubei/v1/player?prettyPrint=false";

    fn builtin() -> BlockerState {
        BlockerState::new(Blocker::from_lists(None), &[])
    }

    #[test]
    fn a_youtube_watch_page_is_never_host_blocked() {
        let s = builtin();
        for top in ["", YOUTUBE, "www.google.com", "news.example"] {
            assert_eq!(s.block_hit(YOUTUBE_WATCH, top), BlockHit::None, "{top:?}");
            assert_eq!(
                s.block_hit_private(YOUTUBE_WATCH, top),
                BlockHit::None,
                "{top:?}"
            );
        }
        assert!(!s.should_block("https://www.youtube.com/embed/dQw4w9WgXcQ", "news.example"));
        for host in [
            "youtube.com",
            YOUTUBE,
            "m.youtube.com",
            "rr3---sn-aigl6nzr.googlevideo.com",
            "i.ytimg.com",
        ] {
            assert!(
                !s.matcher.host_listed(host),
                "{host} must not be a host rule"
            );
        }
        let media_hosts = [
            "*://youtube.com/*",
            "*://*.youtube.com/*",
            "*://googlevideo.com/*",
            "*://*.googlevideo.com/*",
        ];
        for page in ["", YOUTUBE, "news.example"] {
            let patterns = s.blocked_url_patterns(page);
            assert!(!patterns.is_empty());
            for pattern in patterns.iter().chain(&s.blocked_url_patterns_private(page)) {
                assert!(
                    !media_hosts.contains(&pattern.as_str()),
                    "{pattern} on {page:?}"
                );
            }
        }
    }

    #[test]
    fn an_exact_url_rule_blocks_that_url_and_nothing_longer() {
        let s = state("||redirector.example.net/|\n||redirector.example.net/robots.txt|\n", &[]);
        for url in [
            "https://redirector.example.net/",
            "https://redirector.example.net",
            "https://REDIRECTOR.example.net/robots.txt",
            "https://sub.redirector.example.net/",
        ] {
            assert_eq!(s.block_hit(url, "news.example"), BlockHit::Rule, "{url}");
            assert_eq!(s.block_hit_private(url, "news.example"), BlockHit::Rule, "{url}");
        }
        for url in [
            "https://redirector.example.net/videoplayback?itag=18",
            "https://redirector.example.net/?x=1",
            "https://redirector.example.net/robots.txt?x",
            "https://redirector.example.net/robots.txt/more",
            "https://other.example.net/",
        ] {
            assert_eq!(s.block_hit(url, "news.example"), BlockHit::None, "{url}");
        }
        // First-party, and off for a normal tab; a private tab ignores the switch.
        assert_eq!(s.block_hit("https://redirector.example.net/", "example.net"), BlockHit::None);
        let mut off = state("||redirector.example.net/|\n", &[]);
        off.enabled = false;
        assert_eq!(off.block_hit("https://redirector.example.net/", "news.example"), BlockHit::None);
        assert_eq!(off.block_hit_private("https://redirector.example.net/", "news.example"), BlockHit::Rule);
        // Anything that is pattern syntax in DevTools keeps the old prefix meaning: not an exact rule.
        for line in ["a*b", "a^", "a?b", "a:b", "a+b.js", "a(b)", "a{b}", "a#b", "a\\b", "a b"] {
            let prefix = state(&format!("||redirector.example.net/{line}|\n"), &[]);
            assert_eq!(prefix.matcher.exact_urls().count(), 0, "{line}");
        }
        // A repeated rule is one rule.
        let twice = state("||redirector.example.net/x.js|\n||redirector.example.net/x.js|\n", &[]);
        assert_eq!(twice.matcher.exact_urls().count(), 1);
    }

    #[test]
    fn exact_url_rules_become_literal_devtools_patterns() {
        let s = state("||redirector.example.net/robots.txt|\n||doubleclick.net^\n", &[]);
        let patterns = s.blocked_url_patterns("news.example");
        for pattern in [
            "*://doubleclick.net/*",
            "*://redirector.example.net/robots.txt?#",
            "*://*.redirector.example.net/robots.txt?#",
        ] {
            assert!(patterns.contains(&pattern.to_owned()), "{pattern}");
        }
        // The empty query and fragment are pinned, so a longer URL is not matched.
        assert!(patterns.iter().filter(|p| p.contains("redirector")).all(|p| p.ends_with("?#")));
        assert!(s.blocked_url_patterns("example.net").iter().all(|p| !p.contains("redirector")));
        assert!(!s.blocked_url_patterns_private("news.example").is_empty());
        let mut off = state("||redirector.example.net/robots.txt|\n", &[]);
        off.enabled = false;
        assert!(off.blocked_url_patterns("news.example").is_empty());
    }

    #[test]
    fn the_youtube_video_ad_host_probes_fail_and_real_traffic_still_loads() {
        let s = builtin();
        for host in ["s.youtube.com", "redirector.googlevideo.com", "ads.youtube.com"] {
            for path in ["/", "/favicon.ico", "/robots.txt"] {
                let url = format!("https://{host}{path}");
                assert_ne!(s.block_hit(&url, "superadblocktest.com"), BlockHit::None, "{url}");
                assert_ne!(s.block_hit_private(&url, "superadblocktest.com"), BlockHit::None, "{url}");
            }
        }
        let patterns = s.blocked_url_patterns("superadblocktest.com");
        for host in ["s.youtube.com", "redirector.googlevideo.com"] {
            for path in ["/", "/favicon.ico", "/robots.txt"] {
                assert!(patterns.contains(&format!("*://{host}{path}?#")), "{host}{path}");
            }
        }
        // Playback and watch-history traffic on those same hosts is untouched, on YouTube and off it.
        for top in [YOUTUBE, "news.example", ""] {
            for url in [
                "https://redirector.googlevideo.com/videoplayback?id=x&itag=22&source=youtube",
                "https://redirector.googlevideo.com/initplayback?source=youtube&oeis=1",
                "https://s.youtube.com/api/stats/playback?ns=yt&el=detailpage&docid=x",
                "https://s.youtube.com/api/stats/watchtime?ns=yt&el=detailpage&docid=x",
            ] {
                assert_eq!(s.block_hit(url, top), BlockHit::None, "{url} on {top:?}");
            }
        }
    }

    #[test]
    fn the_reachability_probe_hosts_fail_and_functional_traffic_still_loads() {
        let s = builtin();
        let page = "superadblocktest.com";
        let probes = [
            "app.posthog.com", "eu.posthog.com", "ngfts.lge.com", "g.jwpsrv.com",
            "widgets.pinterest.com", "connect.facebook.net", "static.klaviyo.com", "a.klaviyo.com", "cmp.quantcast.com", "consent.cookiebot.com", "cdn.cookielaw.org", "consent.trustarc.com", "sdk.privacy-center.org", "sourcepoint.privacy-mgmt.com", "cmp.osano.com", "fingerprintjs.com", "api.fpjs.io", "cdn.siftscience.com",
            "graph.facebook.com", "graph.instagram.com", "i.instagram.com", "dai.google.com",
            "widget.intercom.io", "js.driftt.com", "clientstream.launchdarkly.com",
            "privacyportal.onetrust.com", "app.usercentrics.eu",
        ];
        let patterns = s.blocked_url_patterns(page);
        for host in probes {
            for path in ["/", "/favicon.ico", "/robots.txt"] {
                let url = format!("https://{host}{path}");
                assert_ne!(s.block_hit(&url, page), BlockHit::None, "{url}");
                assert_ne!(s.block_hit_private(&url, page), BlockHit::None, "{url}");
                // Failed in the renderer, so a no-cors probe is rejected rather than answered.
                assert!(
                    host_suffixes(host).any(|suffix| {
                        [
                            format!("*://{suffix}{path}?#"),
                            format!("*://*.{suffix}{path}?#"),
                            format!("*://{suffix}/*"),
                            format!("*://*.{suffix}/*"),
                        ]
                        .iter()
                        .any(|pattern| patterns.contains(pattern))
                    }),
                    "{url}"
                );
            }
        }
        assert_eq!(s.block_hit("https://vk.com/rtrg", page), BlockHit::Rule);
        assert!(patterns.contains(&"*://vk.com/rtrg?#".to_owned()));
        assert_eq!(s.block_hit("https://vk.com/rtrg?p=x", page), BlockHit::Rule);
        assert_eq!(s.block_hit("https://vk.com/id1", page), BlockHit::None);
        for url in [
            "https://connect.facebook.net/en_US/sdk.js",
            "https://graph.facebook.com/v18.0/me?access_token=x",
            "https://graph.instagram.com/me/media?access_token=x",
            "https://dai.google.com/ondemand/hls/content/x/vid/y/stream/z/master.m3u8",
            "https://widget.intercom.io/widget/abcd1234",
            "https://js.driftt.com/include/1/abc.js",
            "https://clientstream.launchdarkly.com/all?auth=x",
            "https://privacyportal.onetrust.com/request/v1/consentreceipts",
            "https://app.usercentrics.eu/browser-ui/latest/loader.js",
            "https://consent.cookiebot.com/uc.js",
            "https://cdn.cookielaw.org/scripttemplates/otSDKStub.js",
            "https://static.klaviyo.com/onsite/js/klaviyo.js",
            "https://a.klaviyo.com/client/subscriptions",
            "https://cmp.quantcast.com/choice/abc/example.com/choice.js",
            "https://sourcepoint.privacy-mgmt.com/index.html",
            "https://cdn.siftscience.com/s.js",
            "https://api.fpjs.io/v3/",
        ] {
            assert_eq!(s.block_hit(url, page), BlockHit::None, "{url}");
        }
    }

    #[test]
    fn googlevideo_media_and_the_youtube_player_are_never_blocked() {
        let s = builtin();
        for url in [
            // Video and audio bytes, including the itag the player chose, a SABR stream, a live
            // manifest and the CDN warm-up ping.
            "https://rr3---sn-aigl6nzr.googlevideo.com/videoplayback?expire=1760000000&ei=x&ip=1.2.3.4&id=o-AB&itag=137&aitags=133%2C134%2C135%2C136%2C137&source=youtube&requiressl=yes&mime=video%2Fmp4&dur=212.040&ctier=L&rn=3&range=0-1048575",
            "https://rr3---sn-aigl6nzr.googlevideo.com/videoplayback?expire=1760000000&itag=251&source=youtube&mime=audio%2Fwebm&rn=4",
            "https://rr1---sn-aigl6nzr.googlevideo.com/videoplayback?sabr=1&rqh=1&source=youtube&cpn=abc",
            "https://rr1---sn-aigl6nzr.googlevideo.com/initplayback?source=youtube&oeis=1",
            "https://rr1---sn-aigl6nzr.googlevideo.com/generate_204",
            "https://manifest.googlevideo.com/api/manifest/hls_variant/expire/1/id/x/source/yt_live_broadcast/file/index.m3u8",
            "https://redirector.googlevideo.com/videoplayback?id=x&itag=22&source=youtube",
            // The player, its API and everything it needs to keep playing.
            YOUTUBE_WATCH,
            YOUTUBE_PLAYER,
            "https://www.youtube.com/youtubei/v1/next?prettyPrint=false",
            "https://www.youtube.com/youtubei/v1/player/heartbeat?alt=json",
            "https://www.youtube.com/youtubei/v1/att/get?prettyPrint=false",
            "https://www.youtube.com/s/player/abcd1234/player_ias.vflset/en_US/base.js",
            "https://www.youtube.com/api/timedtext?v=dQw4w9WgXcQ&lang=en",
            "https://jnn-pa.googleapis.com/$rpc/google.internal.waa.v1.Waa/GenerateIT",
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg",
            "https://i.ytimg.com/sb/dQw4w9WgXcQ/storyboard3_L2/M0.jpg",
            "https://yt3.ggpht.com/ytc/abc=s88-c-k-c0x00ffffff-no-rj",
            // Content playback stats: watch history, resume position and playback health.
            "https://s.youtube.com/api/stats/playback?ns=yt&el=detailpage&docid=x",
            "https://s.youtube.com/api/stats/watchtime?ns=yt&el=detailpage&docid=x",
            "https://s.youtube.com/api/stats/qoe?event=streamingstats&el=detailpage&docid=x",
        ] {
            for top in [YOUTUBE, "news.example"] {
                assert_eq!(s.block_hit(url, top), BlockHit::None, "{url} on {top}");
                assert_eq!(
                    s.block_hit_private(url, top),
                    BlockHit::None,
                    "{url} on {top}"
                );
            }
        }
    }

    #[test]
    fn youtube_ads_and_ad_beacons_are_blocked_on_youtube_and_in_embeds() {
        let s = builtin();
        for url in [
            "https://www.youtube.com/pagead/adview?ai=x&sigh=y",
            "https://www.youtube.com/pagead/paralleladview?ai=x",
            "https://www.youtube.com/pagead/viewthroughconversion/962985656/?backend=innertube",
            "https://www.youtube.com/api/stats/ads?ver=2&ns=1&event=2",
            "https://s.youtube.com/api/stats/atr?docid=x&ei=y",
            "https://s.youtube.com/api/stats/qoe?event=streamingstats&el=adunit&docid=x",
            "https://s.youtube.com/api/stats/playback?ns=yt&el=adunit&docid=x",
            "https://s.youtube.com/api/stats/watchtime?ns=yt&el=adunit&docid=x",
            "https://www.youtube.com/ptracking?ptk=youtube_single&video_id=x",
            "https://www.youtube.com/pcs/activeview?xai=x&sig=y",
            "https://www.youtube.com/get_midroll_info?ei=x",
            "https://www.youtube.com/youtubei/v1/player/ad_break?prettyPrint=false",
            "https://www.youtube.com/youtubei/v1/log_event?alt=json",
            "https://www.youtube.com/csi_204?v=3&s=youtube&action=watch",
            "https://www.youtube-nocookie.com/pagead/adview?ai=x",
            "https://www.youtube-nocookie.com/api/stats/ads?ver=2",
        ] {
            for top in [YOUTUBE, "news.example"] {
                // A path rule on YouTube's own domain: answered by the hook, never a host failure.
                assert_eq!(s.block_hit(url, top), BlockHit::Rule, "{url} on {top}");
                assert_eq!(
                    s.block_hit_private(url, top),
                    BlockHit::Rule,
                    "{url} on {top}"
                );
            }
        }
        for url in [
            "https://googleads.g.doubleclick.net/pagead/id",
            "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js",
            "https://www.google.com/pagead/lvz?evtid=x",
            "https://www.googleadservices.com/pagead/conversion/1/",
            "https://play.google.com/log?format=json&hasfast=true",
        ] {
            assert!(s.should_block(url, YOUTUBE), "{url}");
            assert!(s.should_block_private(url, YOUTUBE), "{url}");
        }
    }

    #[test]
    fn youtube_cosmetic_rules_hide_the_masthead_and_companion_ads() {
        let css = builtin()
            .cosmetic_css_for_host(YOUTUBE)
            .expect("YouTube cosmetic rules");
        assert!(css.contains("#masthead-ad{display:none!important}"));
        assert!(css.contains("#player-ads{display:none!important}"));
    }

    #[test]
    fn youtube_rules_leave_most_of_the_substring_budget_to_user_lists() {
        assert!(Blocker::from_lists(None).substrings.len() <= MAX_SUBSTRING_RULES / 8);
    }

    #[test]
    fn the_player_rewrite_follows_the_switch_site_exceptions_and_private_tabs() {
        // The gated core, exercised with the `block_youtube_video_ads` option on, so the test does
        // not touch the active profile's settings file. The option-off path has its own test.
        use youtube::PlayerPayload::{Json, WatchPage};
        let beacon = "https://www.youtube.com/api/stats/ads?ver=2";
        let mut s = builtin();
        assert!(s.should_block(beacon, YOUTUBE));
        assert_eq!(
            s.player_ads_rewrite_gated(YOUTUBE_PLAYER, YOUTUBE, false, true),
            Some(Json)
        );
        assert_eq!(
            s.player_ads_rewrite_gated(YOUTUBE_WATCH, "news.example", false, true),
            Some(WatchPage)
        );
        let elsewhere = "https://news.example/youtubei/v1/player";
        assert_eq!(
            s.player_ads_rewrite_gated(elsewhere, YOUTUBE, false, true),
            None
        );
        assert_eq!(
            s.response_patterns_gated(YOUTUBE, false, true).len(),
            youtube::RESPONSE_PATTERNS.len()
        );

        // Ctrl+Shift+B on YouTube: a normal tab is left alone, a private one is not.
        assert_eq!(s.toggle_site(YOUTUBE), SiteToggle::Allowed);
        assert!(!s.should_block(beacon, YOUTUBE));
        assert_eq!(
            s.player_ads_rewrite_gated(YOUTUBE_PLAYER, YOUTUBE, false, true),
            None
        );
        // The watch page's own site decides, wherever the tab was before it.
        assert_eq!(
            s.player_ads_rewrite_gated(YOUTUBE_WATCH, "news.example", false, true),
            None
        );
        assert!(
            !s.response_patterns_gated(YOUTUBE, false, true).is_empty(),
            "each response is decided on its own"
        );
        assert!(s.should_block_private(beacon, YOUTUBE));
        assert_eq!(
            s.player_ads_rewrite_gated(YOUTUBE_PLAYER, YOUTUBE, true, true),
            Some(Json)
        );
        assert_eq!(
            s.player_ads_rewrite_gated(YOUTUBE_WATCH, "", true, true),
            Some(WatchPage)
        );

        // The switch off: nothing for a normal tab, no Response-stage pauses at all.
        s.enabled = false;
        assert!(!s.should_block(beacon, "news.example"));
        assert_eq!(
            s.player_ads_rewrite_gated(YOUTUBE_PLAYER, "news.example", false, true),
            None
        );
        assert!(s.response_patterns_gated(YOUTUBE, false, true).is_empty());
        assert!(!s.response_patterns_gated(YOUTUBE, true, true).is_empty());
        assert!(s.should_block_private(beacon, "news.example"));

        // blocker_allow in settings exempts a normal tab the same way.
        let configured = BlockerState::new(Blocker::from_lists(None), &["youtube.com".to_owned()]);
        assert!(!configured.should_block(beacon, YOUTUBE));
        assert_eq!(
            configured.player_ads_rewrite_gated(YOUTUBE_PLAYER, YOUTUBE, false, true),
            None
        );
        assert!(configured.should_block_private(beacon, YOUTUBE));
        assert_eq!(
            configured.player_ads_rewrite_gated(YOUTUBE_PLAYER, YOUTUBE, true, true),
            Some(Json)
        );
    }

    #[test]
    fn block_youtube_video_ads_off_pauses_and_rewrites_nothing() {
        use youtube::PlayerPayload::Json;
        let s = builtin();
        // On, every path that would strip does strip (same as the switch/exception test above).
        assert_eq!(
            s.player_ads_rewrite_gated(YOUTUBE_PLAYER, YOUTUBE, false, true),
            Some(Json)
        );
        assert_eq!(
            s.player_ads_rewrite_gated(YOUTUBE_WATCH, "news.example", true, true),
            Some(youtube::PlayerPayload::WatchPage)
        );
        assert!(!s.response_patterns_gated(YOUTUBE, false, true).is_empty());

        // Off: no Response-stage pause for either tab kind, and no response is ever rewritten,
        // even in a private tab where the blocker otherwise always applies. The video and audio
        // on googlevideo.com are untouched here regardless, as always.
        assert!(s.response_patterns_gated(YOUTUBE, false, false).is_empty());
        assert!(s.response_patterns_gated(YOUTUBE, true, false).is_empty());
        for private in [false, true] {
            assert_eq!(
                s.player_ads_rewrite_gated(YOUTUBE_PLAYER, YOUTUBE, private, false),
                None
            );
            assert_eq!(
                s.player_ads_rewrite_gated(YOUTUBE_WATCH, "news.example", private, false),
                None
            );
        }
    }

    #[test]
    fn substring_match_inside_the_cap_still_blocks() {
        let s = state("/banner-ad/*.gif\n", &[]);
        let url = format!("https://site.test/{}/banner-ad/x.gif", "p".repeat(500));
        assert!(s.should_block(&url, "news.example"));
        let far = format!("https://site.test/{}/banner-ad/x.gif", "p".repeat(3000));
        assert!(!s.should_block(&far, "news.example"));
    }

    /// `*` any run; with `one_char`, also `?` one character and `\` escaping the next one: the
    /// WebView2 filter glob. Without it, the URL-pattern wildcard inside one component.
    fn wildcard(pattern: &[u8], text: &[u8], one_char: bool) -> bool {
        match pattern.split_first() {
            None => text.is_empty(),
            Some((b'*', rest)) => (0..=text.len()).any(|skip| wildcard(rest, &text[skip..], one_char)),
            Some((b'?', rest)) if one_char => !text.is_empty() && wildcard(rest, &text[1..], one_char),
            Some((b'\\', rest)) if one_char && !rest.is_empty() => {
                text.first() == rest.first() && wildcard(&rest[1..], &text[1..], one_char)
            }
            Some((byte, rest)) => text.first() == Some(byte) && wildcard(rest, &text[1..], one_char),
        }
    }

    /// Whether a WebView2 request filter sees `url`. Compared case-sensitively, the stricter of
    /// the two possible readings: a case-insensitive WebView2 only sees more.
    fn filter_sees(filter: &str, url: &url::Url) -> bool {
        wildcard(filter.as_bytes(), url.as_str().as_bytes(), true)
    }

    /// Whether a `Network.setBlockedURLs` pattern of the shapes the blocker builds
    /// (`*://host/path`, optionally `?search` and `#hash`) blocks `url`: URL-pattern semantics, so
    /// case-sensitive, the port must be the default, and an absent search or hash is a wildcard.
    fn devtools_blocks(pattern: &str, url: &url::Url) -> bool {
        let rest = pattern.strip_prefix("*://").expect("scheme wildcard");
        let slash = rest.find('/').expect("path");
        let (host, rest) = rest.split_at(slash);
        let (path, search) = match rest.split_once('?') {
            Some((path, search)) => (path, Some(search.split('#').next().unwrap_or(""))),
            None => (rest, None),
        };
        matches!(url.scheme(), "http" | "https")
            && url.port().is_none()
            && wildcard(host.as_bytes(), url.host_str().unwrap_or("").as_bytes(), false)
            && wildcard(path.as_bytes(), url.path().as_bytes(), false)
            && search.is_none_or(|search| url.query().unwrap_or("") == search)
    }

    /// The decision after the change, for a script request from a page on `page_host`: failed
    /// by a DevTools pattern for that page, or seen by a request filter and blocked by the matcher.
    fn new_decision(s: &BlockerState, filters: &[String], url: &str, page_host: &str) -> bool {
        let parsed = url::Url::parse(url).expect("test URL");
        s.blocked_url_patterns(page_host)
            .iter()
            .any(|pattern| devtools_blocks(pattern, &parsed))
            || (filters.iter().any(|filter| filter_sees(filter, &parsed))
                && s.should_block(parsed.as_str(), page_host))
    }

    const DECISION_LIST: &str = "||doubleclick.net^\n\
        ||google.com/pagead/\n\
        ||connect.facebook.net/*/fbevents.js\n\
        ||facebook.com/tr?\n\
        ||ads.example/123/\n\
        ||redirector.example.net/robots.txt|\n\
        ||redirector.example.net/|\n\
        /banner-ad/\n\
        /tracker.js$third-party\n";

    #[test]
    fn network_blocklist_and_request_filters_keep_every_old_decision() {
        let s = state(DECISION_LIST, &[]);
        let filters = s.request_filters();
        assert_ne!(filters, vec!["*".to_owned()]);
        // (url, page host, old decision: the matcher on every request, as with the `*` filter)
        let table = [
            // Host rule, and its first-party exemption.
            ("https://ad.doubleclick.net/x.js", "news.example", true),
            ("https://ad.doubleclick.net/x.js", "doubleclick.net", false),
            ("https://doubleclick.net.evil.example/x.js", "news.example", false),
            ("https://news.example/a?ref=doubleclick.net", "news.example", false),
            // Path rule: query string, case difference, first-party exemption.
            ("https://www.google.com/pagead/ads?x=1", "news.example", true),
            ("https://www.google.com/pagead/", "news.example", true),
            ("https://www.google.com/PageAd/ads", "news.example", true),
            ("https://www.google.com/pagead/ads", "www.google.com", false),
            ("https://www.google.com/search?q=/pagead/", "news.example", false),
            ("https://connect.facebook.net/en_US/fbevents.js", "news.example", true),
            ("https://connect.facebook.net/?a=/fbevents.js", "news.example", true),
            ("https://connect.facebook.net/en_US/sdk.js", "news.example", false),
            ("https://www.facebook.com/tr?id=1", "news.example", true),
            ("https://www.facebook.com/TR?id=1", "news.example", true),
            ("https://www.facebook.com/tr/?id=1", "news.example", false),
            ("https://www.facebook.com/tr?id=1", "facebook.com", false),
            ("https://ads.example/123/x.js", "news.example", true),
            ("https://ads.example/123/x.js?y=1", "news.example", true),
            ("https://ads.example/1234", "news.example", false),
            ("https://ads.example/123/x.js", "ads.example", false),
            // `||host/path|`: that URL in any case, nothing longer.
            ("https://redirector.example.net/robots.txt", "news.example", true),
            ("https://redirector.example.net/ROBOTS.TXT", "news.example", true),
            ("https://redirector.example.net/robots.txt?x", "news.example", false),
            ("https://redirector.example.net/", "news.example", true),
            ("https://redirector.example.net/?x=1", "news.example", false),
            ("https://redirector.example.net/robots.txt", "example.net", false),
            // Substring rules apply first-party too, unless `$third-party`.
            ("https://cdn.site.test/banner-ad/x.gif", "site.test", true),
            ("https://cdn.site.test/Banner-Ad/x.gif", "news.example", true),
            ("https://cdn.site.test/banner/x.gif", "news.example", false),
            ("https://cdn.other.test/tracker.js", "other.test", false),
            ("https://cdn.other.test/tracker.js", "news.example", true),
            ("https://cdn.other.test/TRACKER.JS?v=2", "news.example", true),
        ];
        for (url, page, old) in table {
            assert_eq!(s.should_block(url, page), old, "old decision: {url} on {page}");
            assert_eq!(new_decision(&s, &filters, url, page), old, "new decision: {url} on {page}");
        }
    }

    #[test]
    fn the_builtin_list_keeps_its_decisions_with_the_narrow_filters() {
        let s = builtin();
        let filters = s.request_filters();
        assert!(!filters.is_empty());
        assert!(filters.len() <= MAX_REQUEST_FILTERS);
        assert!(!filters.iter().any(|filter| filter == "*"));
        for (url, page) in [
            ("https://www.google.com/pagead/conversion.js", "news.example"),
            ("https://www.google.com/PAGEAD/conversion.js", "news.example"),
            ("https://www.google.com/search?q=x", "news.example"),
            ("https://www.youtube.com/api/stats/ads?ver=2", "www.youtube.com"),
            ("https://www.youtube.com/watch?v=dQw4w9WgXcQ", "www.youtube.com"),
            ("https://www.youtube.com/youtubei/v1/player?key=x", "www.youtube.com"),
            ("https://www.youtube.com/pagead/viewthroughconversion", "news.example"),
            ("https://rr1---sn-a.googlevideo.com/videoplayback?itag=18", "www.youtube.com"),
            ("https://connect.facebook.net/en_US/fbevents.js", "news.example"),
            ("https://connect.facebook.net/favicon.ico", "news.example"),
            ("https://connect.facebook.net/FAVICON.ICO", "news.example"),
            ("https://www.facebook.com/tr?id=1&ev=PageView", "news.example"),
            ("https://play.google.com/log?format=json", "news.example"),
            ("https://adblock.turtlecute.org/js/pagead.js", "adblock.turtlecute.org"),
            ("https://cdn.site.test/ads/prebid.js", "site.test"),
            ("https://cdn.site.test/static/app.js", "site.test"),
            ("https://example.com/", "news.example"),
        ] {
            assert_eq!(
                new_decision(&s, &filters, url, page),
                s.should_block(url, page),
                "{url} on {page}"
            );
        }
    }

    #[test]
    fn request_filters_cover_only_the_rules_devtools_cannot_enforce() {
        // Host rules and letter-free plain paths fail in the network stack: no filter at all.
        assert!(state("||doubleclick.net^\n||ads.example/123/\n||ads.example/|\n", &[])
            .request_filters()
            .is_empty());
        // A letter, `?` or `^`, or a substring rule, keeps a narrow filter for that rule only.
        assert_eq!(
            state("||google.com/pagead/\n", &[]).request_filters(),
            vec!["*://*google.com/??????/*".to_owned()]
        );
        assert_eq!(
            state("||facebook.com/tr?\n", &[]).request_filters(),
            vec!["*://*facebook.com/??\\?*".to_owned()]
        );
        assert_eq!(
            state("||ads.example/123^\n", &[]).request_filters(),
            vec!["*://*ads.example/123**".to_owned()]
        );
        assert_eq!(
            state("||s.example.net/robots.txt|\n", &[]).request_filters(),
            vec!["*://*s.example.net/??????.???".to_owned()]
        );
        assert_eq!(
            state("/banner-ad/*.gif\n", &[]).request_filters(),
            vec!["*/??????-??/*.???*".to_owned()]
        );
        // `*` only when a rule can match any URL, or there are too many filters to install.
        assert_eq!(state("******\n", &[]).request_filters(), vec!["*".to_owned()]);
        let many: String = (0..=MAX_REQUEST_FILTERS)
            .map(|index| format!("||h{index}.example/x{index}/\n"))
            .collect();
        assert_eq!(state(&many, &[]).request_filters(), vec!["*".to_owned()]);
    }

    #[test]
    fn prefix_rules_reach_the_network_blocklist_only_as_exact_or_narrower_patterns() {
        let s = state(
            "||google.com/pagead/\n||connect.facebook.net/*/fbevents.js\n\
             ||facebook.com/tr?\n||ads.example/123^\n||ads.example/a/../b\n",
            &[],
        );
        let patterns = s.blocked_url_patterns("news.example");
        for pattern in [
            "*://google.com/pagead/*",
            "*://*.google.com/pagead/*",
            "*://connect.facebook.net/*/fbevents.js*",
            "*://*.connect.facebook.net/*/fbevents.js*",
        ] {
            assert!(patterns.contains(&pattern.to_owned()), "{pattern}");
        }
        // `?`, `^` and dot segments have no faithful URL pattern.
        assert!(!patterns.iter().any(|p| p.contains("facebook.com/tr")));
        assert!(!patterns.iter().any(|p| p.contains("ads.example")));
        // Third-party only, like every host rule.
        let on_google = s.blocked_url_patterns("www.google.com");
        assert!(!on_google.iter().any(|p| p.contains("google.com/")));
        assert!(on_google.iter().any(|p| p.contains("facebook")));
        let mut off = state("||google.com/pagead/\n", &[]);
        off.enabled = false;
        assert!(off.blocked_url_patterns("news.example").is_empty());
        assert!(!off.blocked_url_patterns_private("news.example").is_empty());
    }

    #[test]
    fn the_precomputed_network_blocklist_matches_a_fresh_build_for_every_page() {
        let s = builtin();
        for page in [
            "",
            "news.example",
            "www.youtube.com",
            "ad.doubleclick.net",
            "www.google.com",
            "connect.facebook.net",
            "localhost",
            "127.0.0.1",
        ] {
            let mut fresh = blocked_url_patterns(s.matcher.hosts(), page, true);
            let mut exact = exact_url_patterns(s.matcher.exact_urls(), page, true);
            exact.sort_unstable();
            fresh.append(&mut exact);
            let mut prefix: Vec<String> = s
                .matcher
                .path_rules()
                .filter(|(host, _)| !same_site(host, page))
                .filter_map(|(host, rule)| path_rule_url_patterns(host, rule))
                .flatten()
                .collect();
            prefix.sort_unstable();
            prefix.dedup();
            fresh.append(&mut prefix);
            // Twice: the second call filters the cached list again.
            assert_eq!(s.blocked_url_patterns(page), fresh, "{page}");
            assert_eq!(s.blocked_url_patterns(page), fresh, "{page}");
        }
    }

    #[test]
    fn the_registrable_domain_cache_never_changes_the_answer() {
        let hosts = [
            "",
            "localhost",
            "127.0.0.1",
            "example.com",
            "www.example.com",
            "a.b.example.co.uk",
            "example.co.uk",
            "co.uk",
            "user.github.io",
            "x.user.github.io",
            "news.example",
        ];
        for _ in 0..2 {
            for host in hosts {
                assert_eq!(registrable_domain(host), registrable_domain_uncached(host), "{host}");
            }
        }
        // Filling the cache past its size clears it; answers stay the same.
        for index in 0..REGISTRABLE_DOMAIN_CACHE * 2 {
            let host = format!("h{index}.example.co.uk");
            assert_eq!(registrable_domain(&host), "example.co.uk");
        }
        REGISTRABLE_DOMAINS.with(|cache| assert!(cache.borrow().len() <= REGISTRABLE_DOMAIN_CACHE));
        for host in hosts {
            assert_eq!(registrable_domain(host), registrable_domain_uncached(host), "{host}");
        }
        assert!(same_site("ads.example.co.uk", "www.example.co.uk"));
        assert!(!same_site("example.co.uk", "other.co.uk"));
    }

    #[test]
    fn youtube_player_responses_pause_only_on_youtube_pages() {
        let s = builtin();
        for page in ["www.youtube.com", "youtube.com", "m.youtube.com", "www.youtube-nocookie.com"] {
            assert_eq!(
                s.response_patterns_gated(page, false, true).len(),
                youtube::RESPONSE_PATTERNS.len(),
                "{page}"
            );
        }
        for page in ["", "news.example", "notyoutube.com", "youtube.com.evil.example"] {
            assert!(s.response_patterns_gated(page, false, true).is_empty(), "{page}");
            assert!(s.response_patterns_gated(page, true, true).is_empty(), "{page}");
            // The public call answers without reading the settings file for these pages.
            assert!(s.response_patterns(page, false).is_empty(), "{page}");
            assert!(s.response_patterns(page, true).is_empty(), "{page}");
        }
    }
}

/// Levels, per-site choices, cookie notices and the sources, on the real built-in lists (thread T3).
#[cfg(test)]
mod level_tests {
    use super::*;

    const TRACKER: &str = "https://www.google-analytics.com/collect?v=1";
    const AD: &str = "https://securepubads.g.doubleclick.net/tag/js/gpt.js";
    const PAGE: &str = "news.example.com";

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bowser-levels-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn state(name: &str) -> (BlockerState, PathBuf) {
        let dir = dir(name);
        let mut state = BlockerState::load(&dir, &[]);
        state.cookie_notices = false;
        (state, dir)
    }

    #[test]
    fn the_three_global_levels_block_nothing_trackers_or_trackers_and_ads() {
        let (mut s, _dir) = state("global");
        s.enabled = true;
        s.level = Level::Ads;
        assert_eq!(s.level_for(PAGE, false), Level::Ads);
        assert!(s.should_block(TRACKER, PAGE) && s.should_block(AD, PAGE), "trackers and ads");
        s.level = Level::Trackers;
        assert_eq!(s.level_for(PAGE, false), Level::Trackers);
        assert!(s.should_block(TRACKER, PAGE), "a tracker is blocked at the trackers level");
        assert!(!s.should_block(AD, PAGE), "an ad is not");
        s.enabled = false;
        assert_eq!(s.global_level(), Level::None);
        assert!(!s.should_block(TRACKER, PAGE) && !s.should_block(AD, PAGE), "none blocks nothing");
        assert_eq!(s.block_hit(TRACKER, PAGE), BlockHit::None);
    }

    #[test]
    fn a_site_level_overrides_the_global_one_both_ways_and_the_nearer_site_wins() {
        let (mut s, _dir) = state("site");
        s.level = Level::Ads;
        assert!(s.set_site_level("example.com", Some(Level::None)));
        assert!(!s.should_block(TRACKER, PAGE), "blocking is off on that site");
        assert!(s.should_block(TRACKER, "other.org"), "and still on elsewhere");
        assert!(s.set_site_level("news.example.com", Some(Level::Trackers)));
        assert!(s.should_block(TRACKER, PAGE) && !s.should_block(AD, PAGE), "the subdomain's own level wins");
        // A site can be stricter than the global level, even when blocking is off globally.
        s.enabled = false;
        assert!(!s.should_block(TRACKER, "other.org"));
        assert!(s.set_site_level("strict.test", Some(Level::Ads)));
        assert!(s.should_block(TRACKER, "strict.test") && s.should_block(AD, "www.strict.test"));
        // Following the global level again.
        assert!(s.set_site_level("strict.test", None));
        assert!(!s.should_block(TRACKER, "strict.test"));
        assert!(!s.set_site_level("not a site", Some(Level::None)), "text that is not a site is refused");
        assert!(!s.set_site_level("localhost", Some(Level::None)));
    }

    #[test]
    fn the_exceptions_list_and_private_tabs_follow_their_old_rules() {
        let dir = dir("exceptions");
        let mut s = BlockerState::load(&dir, &["example.com".to_owned()]);
        assert_eq!(s.level_for(PAGE, false), Level::None, "blocker_allow turns blocking off for a site and its subdomains");
        assert!(s.site_allowed(PAGE));
        assert!(!s.should_block(TRACKER, PAGE));
        assert!(s.set_site_level("example.com", Some(Level::Ads)));
        assert_eq!(s.level_for(PAGE, false), Level::None, "the settings exception still wins");
        // A private tab always gets the full level and ignores every exception and the switch.
        s.enabled = false;
        assert_eq!(s.level_for(PAGE, true), Level::Ads);
        assert!(s.should_block_private(TRACKER, PAGE) && s.should_block_private(AD, PAGE));
        assert!(s.is_blocking(PAGE, true));
    }

    #[test]
    fn the_devtools_patterns_follow_the_level() {
        let (mut s, _dir) = state("patterns");
        let host_pattern = |patterns: &[String], host: &str| patterns.iter().any(|pattern| pattern.contains(host));
        s.level = Level::Ads;
        let ads = s.blocked_url_patterns(PAGE);
        assert!(host_pattern(&ads, "google-analytics.com") && host_pattern(&ads, "doubleclick.net"));
        s.level = Level::Trackers;
        let trackers = s.blocked_url_patterns(PAGE);
        assert!(host_pattern(&trackers, "google-analytics.com"));
        assert!(!host_pattern(&trackers, "doubleclick.net"), "an ad host is not network-blocked at the trackers level");
        assert!(trackers.len() < ads.len());
        s.enabled = false;
        assert!(s.blocked_url_patterns(PAGE).is_empty());
        // A site at the full level gets the full list while the global level is none.
        assert!(s.set_site_level(PAGE, Some(Level::Ads)));
        assert!(host_pattern(&s.blocked_url_patterns(PAGE), "doubleclick.net"));
        // A private tab gets it too.
        assert!(host_pattern(&s.blocked_url_patterns_private("other.org"), "doubleclick.net"));
    }

    #[test]
    fn element_hiding_of_ads_needs_the_full_level_but_cookie_notices_do_not() {
        let (mut s, _dir) = state("cosmetic");
        let youtube = "www.youtube.com";
        s.level = Level::Ads;
        assert!(s.cosmetic_css_for_host(youtube).is_some_and(|css| css.contains("#masthead-ad")), "the ad rule at the full level");
        s.level = Level::Trackers;
        assert_eq!(s.cosmetic_css_for_host(youtube), None, "no ad hiding at the trackers level");
        // Cookie notices work with no blocking at all.
        s.enabled = false;
        assert_eq!(s.cosmetic_css_for_host(PAGE), None, "off by default");
        s.cookie_notices = true;
        let css = s.cosmetic_css_for_host(PAGE).expect("cookie notices hidden without the ad blocker");
        assert!(css.contains("#onetrust-banner-sdk{display:none!important}"));
        assert!(!css.contains("#masthead-ad"), "and no ad rules came with them");
        // The per-site off switch, then the per-site on switch with the global one off.
        assert!(s.set_site_cookies("example.com", Some(false)));
        assert_eq!(s.cosmetic_css_for_host(PAGE), None, "switched off for this site");
        assert!(s.cosmetic_css_for_host("other.org").is_some());
        assert!(s.set_site_cookies("example.com", None));
        assert!(s.cosmetic_css_for_host(PAGE).is_some(), "following the global switch again");
        s.cookie_notices = false;
        assert!(s.set_site_cookies("shown.test", Some(true)));
        assert!(s.cosmetic_css_for_host("shown.test").is_some(), "a site can opt in");
        assert_eq!(s.cosmetic_css_for_host("other.org"), None);
        // A private tab follows the global switch only.
        assert_eq!(s.cosmetic_css_for_host_private("shown.test").is_some(), s.cookie_notices);
        s.cookie_notices = true;
        assert!(s.set_site_cookies("example.com", Some(false)));
        assert!(s.cosmetic_css_for_host_private(PAGE).is_some(), "a private tab ignores the site's saved choice");
    }

    #[test]
    fn the_signature_changes_when_the_level_the_rules_or_a_site_choice_change() {
        let (mut s, dir) = state("signature");
        let before = s.filter_signature(PAGE, false);
        assert_eq!(before.0, Level::Ads);
        assert_eq!(s.filter_signature(PAGE, false), before, "stable while nothing changes");
        s.level = Level::Trackers;
        assert_ne!(s.filter_signature(PAGE, false), before, "a new level");
        let trackers = s.filter_signature(PAGE, false);
        assert!(s.set_site_level("example.com", Some(Level::None)));
        let none = s.filter_signature(PAGE, false);
        assert_ne!(none, trackers);
        assert_eq!(none.0, Level::None);
        let mut registry = sources::Registry::load(&dir);
        registry.set_enabled(sources::ID_ADS, false);
        registry.save(&dir).unwrap();
        s.reload_sources(&dir);
        assert_ne!(s.filter_signature(PAGE, false).1, none.1, "a list switched off");
        let generation = s.generation();
        let settings = crate::settings::Settings { blocker_allow: vec!["other.org".to_owned()], ..Default::default() };
        s.apply_settings(&settings);
        assert_ne!(s.generation(), generation, "the exceptions changed");
        let applied = s.generation();
        s.apply_settings(&settings);
        assert_eq!(s.generation(), applied, "the same settings change nothing");
    }

    #[test]
    fn the_settings_set_the_switch_the_level_the_cookie_switch_and_the_exceptions() {
        let (mut s, _dir) = state("apply");
        let mut settings = crate::settings::Settings {
            blocker_enabled: true,
            blocker_level: crate::settings::BlockerLevel::Trackers,
            cookie_notices: true,
            blocker_allow: vec!["skip.test".to_owned()],
            ..Default::default()
        };
        s.apply_settings(&settings);
        assert_eq!(s.global_level(), Level::Trackers);
        assert!(s.cookie_notices);
        assert_eq!(s.level_for("skip.test", false), Level::None);
        assert_eq!(s.config_allowed_sites(), ["skip.test"]);
        settings.blocker_enabled = false;
        s.apply_settings(&settings);
        assert_eq!(s.global_level(), Level::None, "blocker_enabled=false is the none level");
        assert_eq!(Level::from_settings(true, crate::settings::BlockerLevel::Ads), Level::Ads);
        assert_eq!(Level::from_settings(false, crate::settings::BlockerLevel::Ads), Level::None);
    }

    #[test]
    fn ctrl_shift_b_flips_the_site_between_none_and_the_global_level() {
        let (mut s, dir) = state("toggle");
        assert_eq!(s.toggle_site(PAGE), SiteToggle::Allowed);
        assert_eq!(s.level_for(PAGE, false), Level::None);
        assert_eq!(s.toggle_site(PAGE), SiteToggle::Blocking);
        assert_eq!(s.level_for(PAGE, false), Level::Ads);
        assert!(s.site_rules().is_empty(), "going back to the global level removes the saved rule");
        // With blocking off globally, the toggle turns it on for the site instead.
        s.enabled = false;
        assert_eq!(s.toggle_site(PAGE), SiteToggle::Blocking);
        assert_eq!(s.level_for(PAGE, false), Level::Ads);
        assert!(s.should_block(TRACKER, PAGE));
        assert_eq!(s.toggle_site(PAGE), SiteToggle::Allowed);
        assert_eq!(s.toggle_site("localhost"), SiteToggle::NoSite);
        // The choices persist: a state loaded from the same folder sees them.
        s.enabled = true;
        assert!(s.set_site_level("kept.test", Some(Level::Trackers)));
        assert!(s.set_site_cookies("kept.test", Some(true)));
        let reloaded = BlockerState::load(&dir, &[]);
        assert_eq!(reloaded.level_for("www.kept.test", false), Level::Trackers);
        assert_eq!(reloaded.site_rules().iter().find(|(site, _)| site == "kept.test").map(|(_, rule)| rule.cookies), Some(Some(true)));
    }

    #[test]
    fn an_old_blocker_sites_file_still_means_blocking_is_off_for_those_sites() {
        let dir = dir("legacy");
        fs::write(dir.join(SITES_FILE_NAME), "# Sites where Bowser's blocker is switched off (Ctrl+Shift+B).\nold-exception.test\n").unwrap();
        let s = BlockerState::load(&dir, &[]);
        assert_eq!(s.level_for("www.old-exception.test", false), Level::None);
        assert!(!s.should_block(TRACKER, "old-exception.test"));
        assert!(s.should_block(TRACKER, PAGE));
    }

    #[test]
    fn youtube_ad_stripping_needs_the_full_level() {
        let (mut s, _dir) = state("youtube");
        let player = "https://www.youtube.com/youtubei/v1/player?key=x";
        s.level = Level::Ads;
        assert!(s.player_ads_rewrite_gated(player, "www.youtube.com", false, true).is_some());
        s.level = Level::Trackers;
        assert_eq!(s.player_ads_rewrite_gated(player, "www.youtube.com", false, true), None, "video ads are ads");
        assert!(s.response_patterns_gated("www.youtube.com", false, true).is_empty());
        assert!(s.player_ads_rewrite_gated(player, "www.youtube.com", true, true).is_some(), "private tabs always strip");
        assert!(s.set_site_level("youtube.com", Some(Level::Ads)));
        assert!(s.player_ads_rewrite_gated(player, "www.youtube.com", false, true).is_some(), "a site at the full level strips");
        assert!(!s.response_patterns_gated("www.youtube.com", false, true).is_empty());
    }

    #[test]
    fn the_shield_saves_a_level_only_when_it_differs_from_the_global_one() {
        let (mut s, dir) = state("shield");
        s.level = Level::Ads;
        assert_eq!(s.choose_level_for_page(PAGE, Level::Trackers), SiteChoice::Saved);
        assert_eq!(s.level_for(PAGE, false), Level::Trackers);
        assert_eq!(s.site_rules().len(), 1, "one saved choice, for the site");
        assert_eq!(s.site_rules()[0].0, "example.com", "keyed by the registrable domain");
        assert!(s.should_block(TRACKER, PAGE) && !s.should_block(AD, PAGE));
        // Choosing the global level again forgets the site's own choice instead of saving a copy.
        assert_eq!(s.choose_level_for_page(PAGE, Level::Ads), SiteChoice::Saved);
        assert!(s.site_rules().is_empty());
        assert_eq!(s.level_for(PAGE, false), Level::Ads);
        // None on this site, then back: persisted in between.
        assert_eq!(s.choose_level_for_page(PAGE, Level::None), SiteChoice::Saved);
        assert_eq!(BlockerState::load(&dir, &[]).level_for(PAGE, false), Level::None);
        // A rule for a parent is the one that changes when the page is on a subdomain.
        assert_eq!(s.choose_level_for_page("shop.example.com", Level::Trackers), SiteChoice::Saved);
        assert_eq!(s.site_rules().len(), 1, "the parent's rule changed; no second rule appeared");
        assert_eq!(s.level_for("example.com", false), Level::Trackers);
        // The settings exception and unusable hosts are refused.
        let mut excepted = BlockerState::load(&dir, &["skip.test".to_owned()]);
        assert_eq!(excepted.choose_level_for_page("skip.test", Level::Ads), SiteChoice::ConfiguredInSettings);
        assert_eq!(s.choose_level_for_page("localhost", Level::Ads), SiteChoice::NoSite);
        assert_eq!(s.choose_level_for_page("", Level::Ads), SiteChoice::NoSite);
    }

    #[test]
    fn the_shield_cookie_choice_follows_the_global_switch_unless_it_differs() {
        let (mut s, dir) = state("shield-cookies");
        s.cookie_notices = true;
        assert!(s.cookie_notices_for(PAGE, false));
        assert_eq!(s.choose_cookie_notices_for_page(PAGE, false), SiteChoice::Saved);
        assert!(!s.cookie_notices_for(PAGE, false), "hidden everywhere, shown here");
        assert!(s.cosmetic_css_for_host(PAGE).is_none(), "no notice rule is injected on that site");
        assert_eq!(BlockerState::load(&dir, &[]).site_rules().len(), 1, "persisted");
        assert_eq!(s.choose_cookie_notices_for_page(PAGE, true), SiteChoice::Saved);
        assert!(s.site_rules().is_empty(), "back to the global choice");
        assert!(s.cookie_notices_for(PAGE, false));
        s.cookie_notices = false;
        assert_eq!(s.choose_cookie_notices_for_page("news.test", true), SiteChoice::Saved);
        assert!(s.cookie_notices_for("www.news.test", false) && !s.cookie_notices_for(PAGE, false));
        assert_eq!(s.choose_cookie_notices_for_page("localhost", true), SiteChoice::NoSite);
    }

    #[test]
    fn the_tracker_level_is_a_real_subset_of_the_list() {
        let (s, _dir) = state("subset");
        let trackers = s.trackers.rule_count();
        let everything = s.matcher.rule_count();
        assert!(trackers > 150, "{trackers}");
        assert!(everything > trackers + 150, "{everything} vs {trackers}");
        // Every tracker host is also in the full matcher.
        assert!(s.trackers.hosts().all(|host| s.matcher.host_listed(host)));
    }
}
