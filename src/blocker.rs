//! Request blocking and element hiding for ads and trackers: Bowser Shield plus per-site exceptions.
//!
//! The rule engine is [`crate::shield`] (SRL: `block`, `allow`, `hide`, `off`); this module wires
//! it to the browser. No Windows APIs here. The WebView2 hook lives in `engine/webview2.rs` and
//! only calls [`BlockerState::should_block_kind`] (or its private-tab twin) and
//! [`BlockerState::cosmetic_script`].
//!
//! Rule sources:
//! * the starter list, `extensions/bowser-shield/rules/starter.srl`, embedded at build time;
//! * the optional user list `shield.srl` in the profile directory (`%LOCALAPPDATA%\Bowser\` for the
//!   default profile), at most [`MAX_USER_LIST_BYTES`]. Edit it and restart Bowser; rules are read
//!   once at startup and Bowser never fetches a list itself. A bad line is skipped, not fatal.
//!
//! Safety rules that hold regardless of the lists:
//! * only `http`/`https` URLs are considered, and the hook never sees main-frame documents;
//! * a rule's `party` defaults to third, so a site's own requests are left alone unless a rule says
//!   `party any` or `party first`;
//! * sites on the allow list (settings `blocker_allow=a.com,b.org` plus the persisted runtime
//!   toggle in `blocker-sites.txt`) are never filtered, whatever the list says, in addition to any
//!   `off site:` line in the list itself;
//! * a private tab always filters: neither the enable switch, nor those exceptions, nor `off`
//!   lines apply to it.

use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use crate::shield::{self, RequestKind, Shield, TYPE_ALL, Verdict, host_suffixes};

pub const USER_LIST_NAME: &str = "shield.srl";
pub const SITES_FILE_NAME: &str = "blocker-sites.txt";
pub const MAX_USER_LIST_BYTES: usize = shield::MAX_LIST_BYTES;
pub const MAX_SITES_BYTES: usize = 64 * 1024;
const MAX_ALLOWED_SITES: usize = 1000;

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

/// The rules plus exceptions plus the enable switch; shared (behind `Rc<RefCell<..>>`) by the
/// WebView2 hook and the UI toggle.
pub struct BlockerState {
    pub enabled: bool,
    shield: Shield,
    config_allow: HashSet<String>,
    user_allow: HashSet<String>,
    sites_path: Option<PathBuf>,
    /// Bumped whenever a site exception changes, so a tab can tell its cosmetic script is stale.
    generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SiteToggle {
    /// Filtering is now off for this site.
    Allowed,
    /// Filtering is back on for this site.
    Blocking,
    /// The site is exempted by `blocker_allow` in settings.txt; edit that file instead.
    ConfiguredInSettings,
    /// The host has no usable site (new tab, IP address, ...).
    NoSite,
}

impl BlockerState {
    pub fn new(shield: Shield, config_allow: &[String]) -> Self {
        Self {
            enabled: true,
            shield,
            config_allow: config_allow
                .iter()
                .filter_map(|site| normalise_site(site))
                .collect(),
            user_allow: HashSet::new(),
            sites_path: None,
            generation: 0,
        }
    }

    /// Loads the starter list, the user's `shield.srl` and the persisted site exceptions from `dir`.
    pub fn load(dir: &Path, config_allow: &[String]) -> Self {
        let mut state = Self::new(load_shield(dir), config_allow);
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

    /// A state with only the rule lists from `dir` (the starter list and `shield.srl`): no site
    /// exceptions are read or saved. Private tabs use it when the global blocker is off.
    pub fn matcher_only(dir: &Path) -> Self {
        Self::new(load_shield(dir), &[])
    }

    pub fn rule_count(&self) -> usize {
        self.shield.rule_count()
    }

    /// Lines of `shield.srl` that were skipped, and why.
    pub fn rule_errors(&self) -> &[shield::Issue] {
        self.shield.errors()
    }

    /// True when filtering is switched off for the page on `top_host`.
    pub fn site_allowed(&self, top_host: &str) -> bool {
        host_suffixes(top_host)
            .any(|suffix| self.config_allow.contains(suffix) || self.user_allow.contains(suffix))
    }

    /// The decision for one subresource request made by a page on `top_host`, whatever its kind:
    /// a rule limited to some request kinds counts.
    pub fn should_block(&self, request_url: &str, top_host: &str) -> bool {
        self.decide(request_url, top_host, TYPE_ALL, false)
    }

    /// The decision for one subresource request of a known kind.
    pub fn should_block_kind(&self, request_url: &str, top_host: &str, kind: RequestKind) -> bool {
        self.decide(request_url, top_host, kind.mask(), false)
    }

    /// The decision for a request made by a private tab: the blocker is always on, so neither the
    /// enable switch nor any per-site exception nor `off site:` rule applies.
    pub fn should_block_private(&self, request_url: &str, top_host: &str) -> bool {
        self.decide(request_url, top_host, TYPE_ALL, true)
    }

    /// [`Self::should_block_private`] for a request of a known kind.
    pub fn should_block_private_kind(
        &self,
        request_url: &str,
        top_host: &str,
        kind: RequestKind,
    ) -> bool {
        self.decide(request_url, top_host, kind.mask(), true)
    }

    fn decide(&self, request_url: &str, top_host: &str, kinds: u16, private: bool) -> bool {
        if !crate::private::blocker_applies(private, self.enabled, self.site_allowed(top_host)) {
            return false;
        }
        self.shield
            .decide(request_url, top_host, kinds, !private)
            .verdict
            == Verdict::Block
    }

    /// The document-start script that hides the `hide` rules, built for a normal or a private tab.
    /// `None` when there is nothing to hide or the blocker is off for this kind of tab. A normal
    /// tab's script leaves out the sites in the exception lists; a private tab's does not.
    pub fn cosmetic_script(&self, private: bool) -> Option<String> {
        if !private && !self.enabled {
            return None;
        }
        let mut skip: Vec<String> = Vec::new();
        if !private {
            skip.extend(self.config_allow.iter().cloned());
            skip.extend(self.user_allow.iter().cloned());
            skip.sort_unstable();
        }
        self.shield.cosmetic_script(&skip, !private)
    }

    /// Changes when [`Self::cosmetic_script`] would give a different normal-tab script because a
    /// site exception was added or removed.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Flips filtering for the site of `top_host` and persists the choice.
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
        self.generation += 1;
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

fn load_shield(dir: &Path) -> Shield {
    let user_rules = read_rules(&dir.join(USER_LIST_NAME));
    Shield::with_starter(user_rules.as_deref())
}

fn normalise_site(entry: &str) -> Option<String> {
    let entry = entry.trim().trim_start_matches("*.").trim_matches('.');
    let entry = entry.to_ascii_lowercase();
    (!entry.starts_with('#') && valid_rule_host(&entry)).then_some(entry)
}

/// The text of a file, or `None` when it is missing, unreadable, not UTF-8 or over `cap` bytes.
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

/// The user's SRL text. At most one byte over the cap is read: the parser stops at the line that
/// passes the limit and keeps the rules before it, as the SRL specification says.
fn read_rules(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_USER_LIST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
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
        let allow: Vec<String> = allow.iter().map(|s| (*s).to_owned()).collect();
        BlockerState::new(Shield::compile(&[list]), &allow)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bowser-blocker-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn host_rules_match_subdomains_but_not_lookalikes() {
        let s = state("block host:tracker.net\n", &[]);
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
    fn path_and_label_rules_match_where_they_say() {
        let s = state(
            "block path:/ads/\nblock path:/track\nblock label:adserver\n",
            &[],
        );
        assert!(s.should_block("https://cdn.example.com/ads/a/b.js", "site.test"));
        assert!(s.should_block("https://CDN.example.com/ADS/x.JS?v=1", "site.test"));
        assert!(s.should_block("https://example.net/tracking?x=1", "site.test"));
        assert!(!s.should_block("https://cdn.example.com/lib/b.js", "site.test"));
        assert!(s.should_block("https://eu.adserver9.example/x", "site.test"));
        assert!(!s.should_block("https://cdn.example.com/adserver/x", "site.test"), "not the path");
        assert!(!s.should_block("https://ads.example.com/lib/x", "site.test"), "path rules skip the host");
    }

    #[test]
    fn first_party_requests_are_left_alone_unless_a_rule_says_party_any() {
        let s = state("block host:tracker.net\nblock host:ads.example.com\n", &[]);
        assert!(!s.should_block("https://tracker.net/a.js", "tracker.net"));
        assert!(!s.should_block("https://cdn.tracker.net/a.js", "tracker.net"));
        assert!(!s.should_block("https://tracker.net/a.js", "www.tracker.net"));
        assert!(!s.should_block("https://ads.example.com/a.js", "ads.example.com"));
        assert!(s.should_block("https://ads.example.com/a.js", "www.example.org"));
        // Unknown top site: nothing is first-party, rules still apply.
        assert!(s.should_block("https://tracker.net/a.js", ""));
        let any = state("block host:tracker.net party any\n", &[]);
        assert!(any.should_block("https://tracker.net/a.js", "tracker.net"));
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
            "block host:tracker.net\n",
            &["Shop.Example.com", "bad entry", "# no"],
        );
        assert!(!s.should_block("https://tracker.net/a.js", "shop.example.com"));
        assert!(!s.should_block("https://tracker.net/a.js", "m.shop.example.com"));
        assert!(s.should_block("https://tracker.net/a.js", "example.com"));
        s.enabled = false;
        assert!(!s.should_block("https://tracker.net/a.js", "example.org"));
    }

    #[test]
    fn site_exceptions_win_over_block_rules_even_party_any() {
        let s = state(
            "block host:tracker.net party any\nblock label:adserver\nblock path:/ads/\n",
            &["shop.example.com"],
        );
        for url in [
            "https://tracker.net/a.js",
            "https://adserver.cdn.example/a.js",
            "https://x.example/ads/a.js",
        ] {
            assert!(!s.should_block(url, "www.shop.example.com"), "{url}");
            assert!(s.should_block(url, "other.example"), "{url}");
        }
    }

    #[test]
    fn off_site_lines_in_the_list_work_in_addition_to_the_exception_files() {
        let s = state(
            "off site:bank.example\nblock host:tracker.net\n",
            &["shop.example.com"],
        );
        assert!(!s.should_block("https://tracker.net/a.js", "www.bank.example"));
        assert!(!s.should_block("https://tracker.net/a.js", "shop.example.com"));
        assert!(s.should_block("https://tracker.net/a.js", "notbank.example"));
    }

    #[test]
    fn an_allow_rule_beats_a_block_rule_for_the_request_it_covers() {
        let s = state(
            "block label:adserver\nallow host:adserver.friend.example\n",
            &[],
        );
        assert!(s.should_block("https://adserver.other.example/x", "news.example"));
        assert!(!s.should_block("https://adserver.friend.example/x", "news.example"));
    }

    #[test]
    fn request_kinds_reach_the_matcher() {
        let s = state("block host:tracker.net kind image\n", &[]);
        assert!(s.should_block_kind("https://tracker.net/p", "news.example", RequestKind::Image));
        assert!(!s.should_block_kind("https://tracker.net/p", "news.example", RequestKind::Script));
        assert!(!s.should_block_kind("https://tracker.net/p", "news.example", RequestKind::Xhr));
        assert!(s.should_block("https://tracker.net/p", "news.example"), "no kind means every kind");
        assert!(s.should_block_private_kind("https://tracker.net/p", "news.example", RequestKind::Image));
        assert!(!s.should_block_private_kind("https://tracker.net/p", "news.example", RequestKind::Ping));
    }

    #[test]
    fn private_requests_ignore_the_switch_and_every_site_exception() {
        let mut s = state(
            "block host:tracker.net\noff site:bank.example\n",
            &["shop.example.com"],
        );
        // A normal tab on an excepted site is not blocked; a private tab there is.
        assert!(!s.should_block("https://tracker.net/a.js", "shop.example.com"));
        assert!(s.should_block_private("https://tracker.net/a.js", "shop.example.com"));
        assert!(!s.should_block("https://tracker.net/a.js", "bank.example"));
        assert!(s.should_block_private("https://tracker.net/a.js", "bank.example"));
        s.enabled = false;
        assert!(!s.should_block("https://tracker.net/a.js", "example.org"));
        assert!(s.should_block_private("https://tracker.net/a.js", "example.org"));
        // First-party requests and unmatched requests are still left alone.
        assert!(!s.should_block_private("https://tracker.net/a.js", "tracker.net"));
        assert!(!s.should_block_private("https://fine.example/a.js", "example.org"));
    }

    #[test]
    fn runtime_toggle_is_per_registrable_domain_and_persists() {
        let dir = temp_dir("toggle");
        fs::write(dir.join(USER_LIST_NAME), "block host:extra-tracker.org\n").unwrap();
        let mut s = BlockerState::load(&dir, &["configured.com".to_owned()]);
        assert!(s.rule_count() > 40, "the starter list is loaded");
        assert!(s.should_block("https://extra-tracker.org/x", "page.example"));
        assert_eq!(s.toggle_site("localhost"), SiteToggle::NoSite);
        assert_eq!(s.generation(), 0, "nothing changed");
        assert_eq!(s.toggle_site("www.page.test"), SiteToggle::Allowed);
        assert_eq!(s.generation(), 1);
        assert!(!s.should_block("https://extra-tracker.org/x", "other.page.test"));
        assert_eq!(
            s.toggle_site("configured.com"),
            SiteToggle::ConfiguredInSettings
        );
        assert_eq!(s.generation(), 1);
        let reloaded = BlockerState::load(&dir, &[]);
        assert!(!reloaded.should_block("https://extra-tracker.org/x", "page.test"));
        assert_eq!(s.toggle_site("page.test"), SiteToggle::Blocking);
        assert_eq!(s.generation(), 2);
        assert!(s.should_block("https://extra-tracker.org/x", "page.test"));
        assert!(
            BlockerState::load(&dir, &[]).should_block("https://extra-tracker.org/x", "page.test")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn shield_srl_is_what_load_reads_and_filters_txt_is_not() {
        let dir = temp_dir("srl");
        fs::write(dir.join("filters.txt"), "||legacy-tracker.net^\n").unwrap();
        let before = BlockerState::load(&dir, &[]);
        assert!(!before.should_block("https://legacy-tracker.net/x", "page.example"));
        assert!(before.rule_errors().is_empty());
        fs::write(
            dir.join(USER_LIST_NAME),
            "# mine\nblock host:srl-tracker.net\nallow label:adserver only friend.example\n",
        )
        .unwrap();
        let after = BlockerState::load(&dir, &[]);
        assert!(after.should_block("https://srl-tracker.net/x", "page.example"));
        assert!(!after.should_block("https://legacy-tracker.net/x", "page.example"));
        // The starter list is still underneath, and a user allow rule can lift part of it.
        assert!(after.should_block("https://adserver.cdn.example/x.js", "page.example"));
        assert!(!after.should_block("https://adserver.cdn.example/x.js", "www.friend.example"));
        let matcher_only = BlockerState::matcher_only(&dir);
        assert!(matcher_only.should_block_private("https://srl-tracker.net/x", "page.example"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_line_in_shield_srl_is_skipped_and_reported() {
        let dir = temp_dir("bad");
        fs::write(
            dir.join(USER_LIST_NAME),
            "||easylist-style.net^\nblock host:good-tracker.net\nhide css:div{color:red}\n",
        )
        .unwrap();
        let s = BlockerState::load(&dir, &[]);
        assert!(s.should_block("https://good-tracker.net/x", "page.example"));
        assert!(!s.should_block("https://easylist-style.net/x", "page.example"));
        let codes: Vec<_> = s.rule_errors().iter().map(|e| (e.line, e.code)).collect();
        assert_eq!(codes, [(1, "bad-verb"), (3, "bad-css")]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_oversized_shield_srl_is_read_to_the_limit_and_no_further() {
        let dir = temp_dir("big");
        let mut big = String::from("block host:early-tracker.org\n");
        big.push_str(&"#".repeat(MAX_USER_LIST_BYTES));
        big.push_str("\nblock host:late-tracker.org\n");
        fs::write(dir.join(USER_LIST_NAME), big).unwrap();
        let s = BlockerState::load(&dir, &[]);
        assert!(s.should_block("https://early-tracker.org/", "page.test"), "before the limit");
        assert!(!s.should_block("https://late-tracker.org/", "page.test"), "after the limit");
        assert_eq!(
            s.rule_errors().iter().map(|e| (e.line, e.code)).collect::<Vec<_>>(),
            [(2, "list-too-large")]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_starter_list_blocks_ad_shaped_urls_and_spares_common_sites() {
        let s = BlockerState::new(Shield::with_starter(None), &[]);
        assert!(s.should_block("https://adserver.cdn.example/x.js", "news.example"));
        assert!(s.should_block("https://x.example/ad-slot/1", "news.example"));
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
    fn huge_urls_are_matched_in_bounded_time() {
        let s = state(
            "block path:/zz-ad/\nblock label:adserver\nblock host:example.net\n",
            &[],
        );
        let started = std::time::Instant::now();
        let long = format!("https://site.test/{}", "a".repeat(100_000));
        assert!(!s.should_block(&long, "news.example"));
        let beyond = format!("https://site.test/{}/zz-ad/x", "a".repeat(100_000));
        assert!(!s.should_block(&beyond, "news.example"), "the match lies beyond the cap");
        let inside = format!("https://site.test/{}/zz-ad/x", "p".repeat(500));
        assert!(s.should_block(&inside, "news.example"));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn the_cosmetic_script_follows_the_exceptions_and_the_private_rule() {
        let list = "hide css:div.sponsored\noff site:bank.example\n";
        let mut s = state(list, &["shop.example.com"]);
        let script = s.cosmetic_script(false).expect("script");
        assert!(script.contains("div.sponsored"));
        assert!(script.contains("\"shop.example.com\""), "settings exception");
        assert!(script.contains("\"bank.example\""), "off site: line");
        let generation = s.generation();
        assert_eq!(s.toggle_site("www.page.test"), SiteToggle::Allowed);
        assert_ne!(s.generation(), generation);
        assert!(s.cosmetic_script(false).expect("script").contains("\"page.test\""));
        // A private tab keeps no exception at all.
        let private = s.cosmetic_script(true).expect("script");
        assert!(private.contains("div.sponsored"));
        for site in ["shop.example.com", "bank.example", "page.test"] {
            assert!(!private.contains(site), "{site}");
        }
        // The switch turns the normal script off, not the private one.
        s.enabled = false;
        assert!(s.cosmetic_script(false).is_none());
        assert!(s.cosmetic_script(true).is_some());
        assert!(state("block host:a.example\n", &[]).cosmetic_script(true).is_none());
    }
}
