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
//! Three more decisions live alongside the matcher and are consumed only by the WebView2 hook:
//! * [`blocked_url_patterns`] turns the stored host rules into DevTools
//!   `Network.setBlockedURLs` patterns, so a listed host fails at the network layer (a real
//!   connection failure) instead of getting an HTTP response a page's own reachability probe
//!   would see as "not blocked";
//! * [`Blocker::cosmetic_css_for_host`] turns the `##` rules for a page into a CSS string the
//!   hook injects as a `<style>` element;
//! * [`BlockerState::player_ads_rewrite`] picks the YouTube player responses whose ad schedule
//!   the hook strips (see [`youtube`]), under the same switch, site exceptions and private-tab
//!   rule as every request rule.

pub(crate) mod json;
pub mod youtube;

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
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
const MAX_ALLOWED_SITES: usize = 1000;

#[derive(Debug, Eq, PartialEq)]
enum Rule {
    Host(String),
    Path {
        host: String,
        pattern: String,
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

/// The registrable domain (eTLD+1) of a lowercase host, or the host itself when it has none
/// (IP addresses, `localhost`, bare suffixes).
pub fn registrable_domain(host: &str) -> &str {
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
        .flat_map(|host| [format!("*://{host}/*"), format!("*://*.{host}/*")])
        .collect();
    patterns.sort_unstable();
    patterns
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
        let mut added = 0;
        for line in text.lines() {
            let Some(rule) = parse_line(line) else {
                continue;
            };
            match rule {
                Rule::Host(host) => {
                    if self.hosts.len() < MAX_HOST_RULES && self.hosts.insert(host.into()) {
                        added += 1;
                    }
                }
                Rule::Path { host, pattern } => {
                    if self.path_count < MAX_PATH_RULES {
                        self.paths
                            .entry(host.into())
                            .or_default()
                            .push(pattern.into());
                        self.path_count += 1;
                        added += 1;
                    }
                }
                Rule::Substring {
                    pattern,
                    third_party_only,
                    domains,
                } => {
                    if self.substrings.len() < MAX_SUBSTRING_RULES {
                        self.substrings.push(SubstringRule {
                            pattern: pattern.into(),
                            third_party_only,
                            domains: domains.into_iter().map(Into::into).collect(),
                        });
                        added += 1;
                    }
                }
                Rule::Cosmetic { domains, selector } => {
                    if self.cosmetics.len() < MAX_COSMETIC_RULES {
                        self.cosmetics.push(CosmeticRule {
                            domains: domains.into_iter().map(Into::into).collect(),
                            selector: selector.into(),
                        });
                        added += 1;
                    }
                }
            }
        }
        added
    }

    pub fn rule_count(&self) -> usize {
        self.hosts.len() + self.path_count + self.substrings.len() + self.cosmetics.len()
    }

    /// Every stored host rule (not path or substring rules), for building DevTools patterns.
    pub fn hosts(&self) -> impl Iterator<Item = &str> {
        self.hosts.iter().map(|host| host.as_ref())
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
/// Path and substring rules are not in the host pattern list, so the hook answers those itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockHit {
    None,
    Host,
    Rule,
}

/// Matcher plus exceptions plus the enable switch; shared (behind `Rc<RefCell<..>>`) by the
/// WebView2 hook and the UI toggle.
pub struct BlockerState {
    pub enabled: bool,
    matcher: Blocker,
    config_allow: HashSet<String>,
    user_allow: HashSet<String>,
    sites_path: Option<PathBuf>,
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

impl BlockerState {
    pub fn new(matcher: Blocker, config_allow: &[String]) -> Self {
        Self {
            enabled: true,
            matcher,
            config_allow: config_allow
                .iter()
                .filter_map(|site| normalise_site(site))
                .collect(),
            user_allow: HashSet::new(),
            sites_path: None,
        }
    }

    /// Loads the user list and persisted site exceptions from `dir`, plus the built-in list.
    pub fn load(dir: &Path, config_allow: &[String]) -> Self {
        let user_list = read_capped(&dir.join(USER_LIST_NAME), MAX_USER_LIST_BYTES);
        let mut state = Self::new(Blocker::from_lists(user_list.as_deref()), config_allow);
        let path = dir.join(SITES_FILE_NAME);
        if let Some(text) = read_capped(&path, MAX_SITES_BYTES) {
            state.user_allow = text
                .lines()
                .filter_map(normalise_site)
                .take(MAX_ALLOWED_SITES)
                .collect();
        }
        state.sites_path = Some(path);
        state
    }

    /// A state with only the rule lists from `dir` (the built-in list and the user list): no
    /// site exceptions are read or saved. Private tabs use it when the global blocker is off.
    pub fn matcher_only(dir: &Path) -> Self {
        let user_list = read_capped(&dir.join(USER_LIST_NAME), MAX_USER_LIST_BYTES);
        Self::new(Blocker::from_lists(user_list.as_deref()), &[])
    }

    pub fn rule_count(&self) -> usize {
        self.matcher.rule_count()
    }

    /// True when blocking is switched off for the page on `top_host`.
    pub fn site_allowed(&self, top_host: &str) -> bool {
        host_suffixes(top_host)
            .any(|suffix| self.config_allow.contains(suffix) || self.user_allow.contains(suffix))
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
        if !self.is_blocking(top_host, private) {
            return BlockHit::None;
        }
        let Some((host, path)) = split_url(request_url) else {
            return BlockHit::None;
        };
        let first_party = !top_host.is_empty() && same_site(&host, top_host);
        if !first_party && self.matcher.host_listed(&host) {
            return BlockHit::Host;
        }
        if self
            .matcher
            .matches_parts(&host, path, request_url, first_party, top_host)
        {
            BlockHit::Rule
        } else {
            BlockHit::None
        }
    }

    /// Whether the blocker is in effect at all for a document on `top_host`: honours the enable
    /// switch and site exceptions for a normal tab, and is always true for a private one.
    pub fn is_blocking(&self, top_host: &str, private: bool) -> bool {
        crate::private::blocker_applies(private, self.enabled, self.site_allowed(top_host))
    }

    /// The cosmetic CSS to inject for a normal tab's document on `page_host`, or `None` when the
    /// blocker does not apply (disabled, or the site is excepted) or no rule matches the page.
    pub fn cosmetic_css_for_host(&self, page_host: &str) -> Option<String> {
        self.cosmetic(page_host, false)
    }

    /// As [`Self::cosmetic_css_for_host`], but for a private tab: the blocker always applies and
    /// site exceptions never do.
    pub fn cosmetic_css_for_host_private(&self, page_host: &str) -> Option<String> {
        self.cosmetic(page_host, true)
    }

    fn cosmetic(&self, page_host: &str, private: bool) -> Option<String> {
        if !self.is_blocking(page_host, private) {
            return None;
        }
        self.matcher.cosmetic_css_for_host(page_host)
    }

    /// DevTools `Network.setBlockedURLs` patterns for a normal tab's document on `page_host`
    /// (empty when the blocker does not apply there).
    pub fn blocked_url_patterns(&self, page_host: &str) -> Vec<String> {
        blocked_url_patterns(self.matcher.hosts(), page_host, self.is_blocking(page_host, false))
    }

    /// As [`Self::blocked_url_patterns`], but for a private tab.
    pub fn blocked_url_patterns_private(&self, page_host: &str) -> Vec<String> {
        blocked_url_patterns(self.matcher.hosts(), page_host, self.is_blocking(page_host, true))
    }

    /// DevTools Response-stage patterns for the YouTube player responses, whenever the blocker is
    /// switched on for the tab at all. They do not follow the current page's site exception, so
    /// the first watch page opened from an excepted site still pauses; each paused response is
    /// then decided by [`Self::player_ads_rewrite`].
    pub fn response_patterns(&self, private: bool) -> &'static [youtube::ResponsePattern] {
        if crate::private::blocker_applies(private, self.enabled, false) {
            &youtube::RESPONSE_PATTERNS
        } else {
            &[]
        }
    }

    /// Which player response to strip of its ads for a response to `url`, or `None` to pass it
    /// through. A watch page is the tab's next document, so its own site decides; any other
    /// response follows the page on `top_host`, like every request rule. A private tab always
    /// strips; a normal tab honours the switch and site exceptions.
    pub fn player_ads_rewrite(
        &self,
        url: &str,
        top_host: &str,
        private: bool,
    ) -> Option<youtube::PlayerPayload> {
        let payload = youtube::player_payload(url)?;
        let blocking = match payload {
            youtube::PlayerPayload::WatchPage => {
                let (host, _) = split_url(url)?;
                self.is_blocking(&host, private)
            }
            youtube::PlayerPayload::Json => self.is_blocking(top_host, private),
        };
        blocking.then_some(payload)
    }

    /// Flips blocking for the site of `top_host` and persists the choice.
    pub fn toggle_site(&mut self, top_host: &str) -> SiteToggle {
        let top_host = top_host.to_ascii_lowercase();
        if top_host.is_empty() || !top_host.contains('.') {
            return SiteToggle::NoSite;
        }
        let site = registrable_domain(&top_host).to_owned();
        if host_suffixes(&top_host).any(|suffix| self.config_allow.contains(suffix)) {
            return SiteToggle::ConfiguredInSettings;
        }
        let result = if self.user_allow.remove(&site) {
            SiteToggle::Blocking
        } else {
            if self.user_allow.len() >= MAX_ALLOWED_SITES {
                return SiteToggle::NoSite;
            }
            self.user_allow.insert(site);
            SiteToggle::Allowed
        };
        let _ = self.save_sites();
        result
    }

    fn save_sites(&self) -> io::Result<()> {
        let Some(path) = &self.sites_path else {
            return Ok(());
        };
        let mut sites: Vec<&str> = self.user_allow.iter().map(String::as_str).collect();
        sites.sort_unstable();
        let mut text =
            String::from("# Sites where Bowser's blocker is switched off (Ctrl+Shift+B).\n");
        for site in sites {
            text.push_str(site);
            text.push('\n');
        }
        write_atomic(path, text.as_bytes())
    }
}

fn normalise_site(entry: &str) -> Option<String> {
    let entry = entry.trim().trim_start_matches("*.").trim_matches('.');
    let entry = entry.to_ascii_lowercase();
    (!entry.starts_with('#') && valid_rule_host(&entry)).then_some(entry)
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
        use youtube::PlayerPayload::{Json, WatchPage};
        let beacon = "https://www.youtube.com/api/stats/ads?ver=2";
        let mut s = builtin();
        assert!(s.should_block(beacon, YOUTUBE));
        assert_eq!(
            s.player_ads_rewrite(YOUTUBE_PLAYER, YOUTUBE, false),
            Some(Json)
        );
        assert_eq!(
            s.player_ads_rewrite(YOUTUBE_WATCH, "news.example", false),
            Some(WatchPage)
        );
        let elsewhere = "https://news.example/youtubei/v1/player";
        assert_eq!(s.player_ads_rewrite(elsewhere, YOUTUBE, false), None);
        assert_eq!(
            s.response_patterns(false).len(),
            youtube::RESPONSE_PATTERNS.len()
        );

        // Ctrl+Shift+B on YouTube: a normal tab is left alone, a private one is not.
        assert_eq!(s.toggle_site(YOUTUBE), SiteToggle::Allowed);
        assert!(!s.should_block(beacon, YOUTUBE));
        assert_eq!(s.player_ads_rewrite(YOUTUBE_PLAYER, YOUTUBE, false), None);
        // The watch page's own site decides, wherever the tab was before it.
        assert_eq!(
            s.player_ads_rewrite(YOUTUBE_WATCH, "news.example", false),
            None
        );
        assert!(
            !s.response_patterns(false).is_empty(),
            "each response is decided on its own"
        );
        assert!(s.should_block_private(beacon, YOUTUBE));
        assert_eq!(
            s.player_ads_rewrite(YOUTUBE_PLAYER, YOUTUBE, true),
            Some(Json)
        );
        assert_eq!(
            s.player_ads_rewrite(YOUTUBE_WATCH, "", true),
            Some(WatchPage)
        );

        // The switch off: nothing for a normal tab, no Response-stage pauses at all.
        s.enabled = false;
        assert!(!s.should_block(beacon, "news.example"));
        assert_eq!(
            s.player_ads_rewrite(YOUTUBE_PLAYER, "news.example", false),
            None
        );
        assert!(s.response_patterns(false).is_empty());
        assert!(!s.response_patterns(true).is_empty());
        assert!(s.should_block_private(beacon, "news.example"));

        // blocker_allow in settings exempts a normal tab the same way.
        let configured = BlockerState::new(Blocker::from_lists(None), &["youtube.com".to_owned()]);
        assert!(!configured.should_block(beacon, YOUTUBE));
        assert_eq!(
            configured.player_ads_rewrite(YOUTUBE_PLAYER, YOUTUBE, false),
            None
        );
        assert!(configured.should_block_private(beacon, YOUTUBE));
        assert_eq!(
            configured.player_ads_rewrite(YOUTUBE_PLAYER, YOUTUBE, true),
            Some(Json)
        );
    }

    #[test]
    fn substring_match_inside_the_cap_still_blocks() {
        let s = state("/banner-ad/*.gif\n", &[]);
        let url = format!("https://site.test/{}/banner-ad/x.gif", "p".repeat(500));
        assert!(s.should_block(&url, "news.example"));
        let far = format!("https://site.test/{}/banner-ad/x.gif", "p".repeat(3000));
        assert!(!s.should_block(&far, "news.example"));
    }
}
