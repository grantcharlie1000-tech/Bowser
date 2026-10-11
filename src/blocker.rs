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
//! * `||host^` / `||host` / `||host^|`      block `host` and all of its subdomains;
//! * `||host/path*pattern` / `||host^`      block URLs on that host whose path+query starts with the
//!   pattern (`*` is any run, `^` is a separator or the end);
//! * `/plain/substring*`                    block URLs containing the text (at most
//!   [`MAX_SUBSTRING_RULES`] of these are kept; scanned linearly);
//! * the only option understood is `$third-party` (`$3p`); any other option, `$domain=`,
//!   exception rules (`@@`), cosmetic rules (`##`, `#@#`, ...), regex rules, comments and
//!   headers are ignored, so a rule is never applied more broadly than it was written.
//!
//! Safety rules that hold regardless of the lists:
//! * only `http`/`https` URLs are considered, and the hook never sees main-frame documents;
//! * a request whose registrable domain equals the page's (first-party) is never blocked;
//! * sites on the allow list (settings `blocker_allow=a.com,b.org` plus the persisted runtime
//!   toggle in `blocker-sites.txt`) are never blocked.

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
const MAX_PATTERN_BYTES: usize = 160;
const MAX_ALLOWED_SITES: usize = 1000;

#[derive(Debug, Eq, PartialEq)]
enum Rule {
    Host(String),
    Path { host: String, pattern: String },
    Substring(String),
}

/// Parses one filter line. `None` means "not supported / not a rule".
fn parse_line(line: &str) -> Option<Rule> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('!') || line.starts_with('[') || line.starts_with("@@") {
        return None;
    }
    if ["##", "#@#", "#?#", "#$#", "#%#"]
        .iter()
        .any(|marker| line.contains(marker))
    {
        return None;
    }
    let (pattern, options) = match line.rfind('$') {
        Some(index) => (&line[..index], Some(&line[index + 1..])),
        None => (line, None),
    };
    if let Some(options) = options {
        // Only `third-party` is understood; first-party requests are never blocked anyway.
        let understood = options
            .split(',')
            .all(|option| matches!(option.trim(), "third-party" | "3p"));
        if !understood || options.is_empty() {
            return None;
        }
    }
    let pattern = pattern.to_ascii_lowercase();
    if !pattern.is_ascii()
        || pattern.len() > MAX_PATTERN_BYTES
        || pattern.contains(char::is_whitespace)
    {
        return None;
    }
    if let Some(rest) = pattern.strip_prefix("||") {
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
    Some(Rule::Substring(pattern.to_owned()))
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

/// The matcher. Cheap to query, built once at startup.
#[derive(Default)]
pub struct Blocker {
    hosts: HashSet<Box<str>>,
    paths: HashMap<Box<str>, Vec<Box<str>>>,
    path_count: usize,
    substrings: Vec<Box<str>>,
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
                Rule::Substring(text) => {
                    if self.substrings.len() < MAX_SUBSTRING_RULES {
                        self.substrings.push(text.into());
                        added += 1;
                    }
                }
            }
        }
        added
    }

    pub fn rule_count(&self) -> usize {
        self.hosts.len() + self.path_count + self.substrings.len()
    }

    /// Whether the rules (ignoring first-party and exceptions) match this URL.
    pub fn matches(&self, url: &str) -> bool {
        let Some((host, path)) = split_url(url) else {
            return false;
        };
        self.matches_parts(&host, path, url)
    }

    fn matches_parts(&self, host: &str, path: &str, url: &str) -> bool {
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
        let url = capped(url, MAX_MATCH_TEXT_BYTES);
        self.substrings
            .iter()
            .any(|needle| contains_glob(needle.as_bytes(), url.as_bytes()))
    }
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
        if !self.enabled || self.site_allowed(top_host) {
            return false;
        }
        let Some((host, path)) = split_url(request_url) else {
            return false;
        };
        if !top_host.is_empty() && same_site(&host, top_host) {
            return false;
        }
        self.matcher.matches_parts(&host, path, request_url)
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
            Some(Rule::Substring("/banner/ads/".into()))
        );
    }

    #[test]
    fn parser_ignores_unsupported_lines() {
        for line in [
            "",
            "! comment",
            "[Adblock Plus 2.0]",
            "example.com##.ad",
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

    #[test]
    fn substring_match_inside_the_cap_still_blocks() {
        let s = state("/banner-ad/*.gif\n", &[]);
        let url = format!("https://site.test/{}/banner-ad/x.gif", "p".repeat(500));
        assert!(s.should_block(&url, "news.example"));
        let far = format!("https://site.test/{}/banner-ad/x.gif", "p".repeat(3000));
        assert!(!s.should_block(&far, "news.example"));
    }
}
