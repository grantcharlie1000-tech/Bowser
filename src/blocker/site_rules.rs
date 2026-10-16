//! Per-site blocker choices, kept in the profile's `blocker-sites.txt`.
//!
//! Each line names a site and, after it, any of `level=none|trackers|ads` and
//! `cookies=on|off`. A bare site (what Ctrl+Shift+B wrote before levels existed) means
//! `level=none`. A rule for `example.com` covers its subdomains; a rule for a subdomain wins over
//! its parent's. A missing choice follows the global setting.
//!
//! Lines are untrusted input: a site must pass the same host check as every blocker rule, a bad
//! token drops its line, and at most [`MAX_SITES`] sites are kept.

use std::collections::BTreeMap;

use super::{Level, host_suffixes, normalise_site};

pub const MAX_SITES: usize = 1000;
const HEADER: &str = "# Per-site blocker choices (Ctrl+Shift+B, the shield and Settings). One site per line,\n\
                      # then level=none|trackers|ads and/or cookies=on|off. A bare site means level=none.\n";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SiteRule {
    /// `Some` overrides the global blocking level on this site.
    pub level: Option<Level>,
    /// `Some(false)` keeps cookie notices visible on this site; `Some(true)` hides them even when
    /// the global switch is off.
    pub cookies: Option<bool>,
}

impl SiteRule {
    pub fn is_empty(&self) -> bool {
        self.level.is_none() && self.cookies.is_none()
    }
}

#[derive(Clone, Debug, Default)]
pub struct SiteRules {
    map: BTreeMap<String, SiteRule>,
}

impl SiteRules {
    pub fn parse(text: &str) -> Self {
        let mut map = BTreeMap::new();
        for line in text.lines() {
            if map.len() >= MAX_SITES {
                break;
            }
            let mut words = line.split_whitespace();
            let Some(site) = words.next().and_then(normalise_site) else {
                continue;
            };
            let mut rule = SiteRule::default();
            let mut valid = true;
            let mut tokens = 0;
            for word in words {
                tokens += 1;
                match word.split_once('=') {
                    Some(("level", value)) => match Level::parse(value) {
                        Some(level) => rule.level = Some(level),
                        None => valid = false,
                    },
                    Some(("cookies", "on")) => rule.cookies = Some(true),
                    Some(("cookies", "off")) => rule.cookies = Some(false),
                    _ => valid = false,
                }
            }
            if !valid {
                continue;
            }
            if tokens == 0 {
                rule.level = Some(Level::None);
            }
            if !rule.is_empty() {
                map.insert(site, rule);
            }
        }
        Self { map }
    }

    pub fn render(&self) -> String {
        let mut text = String::from(HEADER);
        for (site, rule) in &self.map {
            text.push_str(site);
            match (rule.level, rule.cookies) {
                (Some(Level::None), None) => {}
                (level, cookies) => {
                    if let Some(level) = level {
                        text.push_str(" level=");
                        text.push_str(level.key());
                    }
                    if let Some(on) = cookies {
                        text.push_str(if on { " cookies=on" } else { " cookies=off" });
                    }
                }
            }
            text.push('\n');
        }
        text
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, SiteRule)> {
        self.map.iter().map(|(site, rule)| (site.as_str(), *rule))
    }

    pub fn get(&self, site: &str) -> Option<SiteRule> {
        self.map.get(site).copied()
    }

    /// The level chosen for `host`: the most specific rule that sets one.
    pub fn level_for(&self, host: &str) -> Option<Level> {
        host_suffixes(host).find_map(|suffix| self.map.get(suffix).and_then(|rule| rule.level))
    }

    /// The cookie-notice choice for `host`: the most specific rule that sets one.
    pub fn cookies_for(&self, host: &str) -> Option<bool> {
        host_suffixes(host).find_map(|suffix| self.map.get(suffix).and_then(|rule| rule.cookies))
    }

    /// Whether any site overrides the level to `level`.
    pub fn any_level(&self, level: Level) -> bool {
        self.map.values().any(|rule| rule.level == Some(level))
    }

    /// Sets (or with `None` clears) the level for `site`. `false` when the list is full.
    pub fn set_level(&mut self, site: &str, level: Option<Level>) -> bool {
        self.edit(site, |rule| rule.level = level)
    }

    /// Sets (or with `None` clears) the cookie-notice choice for `site`.
    pub fn set_cookies(&mut self, site: &str, cookies: Option<bool>) -> bool {
        self.edit(site, |rule| rule.cookies = cookies)
    }

    /// Forgets every choice for `site`. `true` when there was one.
    pub fn remove(&mut self, site: &str) -> bool {
        self.map.remove(site).is_some()
    }

    fn edit(&mut self, site: &str, change: impl FnOnce(&mut SiteRule)) -> bool {
        if !self.map.contains_key(site) && self.map.len() >= MAX_SITES {
            return false;
        }
        let mut rule = self.map.get(site).copied().unwrap_or_default();
        change(&mut rule);
        if rule.is_empty() {
            self.map.remove(site);
        } else {
            self.map.insert(site.to_owned(), rule);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_site_is_the_old_off_switch() {
        let rules = SiteRules::parse("# old file\nexample.com\nnews.test\n");
        assert_eq!(rules.level_for("example.com"), Some(Level::None));
        assert_eq!(rules.level_for("www.example.com"), Some(Level::None), "subdomains follow");
        assert_eq!(rules.level_for("other.org"), None);
        assert_eq!(rules.cookies_for("example.com"), None);
    }

    #[test]
    fn levels_and_cookie_choices_round_trip_and_the_nearer_site_wins() {
        let text = "example.com level=trackers\nshop.example.com level=ads cookies=off\nnews.test cookies=on\n";
        let rules = SiteRules::parse(text);
        assert_eq!(rules.level_for("example.com"), Some(Level::Trackers));
        assert_eq!(rules.level_for("a.example.com"), Some(Level::Trackers));
        assert_eq!(rules.level_for("shop.example.com"), Some(Level::Ads));
        assert_eq!(rules.cookies_for("shop.example.com"), Some(false));
        assert_eq!(rules.cookies_for("x.shop.example.com"), Some(false));
        assert_eq!(rules.cookies_for("example.com"), None);
        assert_eq!(rules.cookies_for("news.test"), Some(true));
        assert_eq!(rules.level_for("news.test"), None, "a cookie-only rule sets no level");
        let again = SiteRules::parse(&rules.render());
        assert_eq!(again.render(), rules.render());
        assert_eq!(again.level_for("shop.example.com"), Some(Level::Ads));
        assert!(rules.render().contains("\nexample.com level=trackers\n"));
    }

    #[test]
    fn a_level_none_rule_is_written_as_a_bare_site_for_older_readers() {
        let mut rules = SiteRules::default();
        assert!(rules.set_level("example.com", Some(Level::None)));
        assert!(rules.render().ends_with("\nexample.com\n"), "{}", rules.render());
        assert!(rules.set_cookies("example.com", Some(false)));
        assert!(rules.render().ends_with("\nexample.com level=none cookies=off\n"));
    }

    #[test]
    fn bad_lines_are_dropped_and_clearing_every_choice_removes_the_site() {
        let rules = SiteRules::parse(
            "example.com level=sometimes\nexample.org cookies=maybe\nexample.net junk\nnot a host\n\
             ok.test level=ads\n../evil level=none\nhttp://x.test level=none\n",
        );
        assert_eq!(rules.len(), 1, "{:?}", rules.render());
        assert_eq!(rules.level_for("ok.test"), Some(Level::Ads));
        let mut rules = rules;
        assert!(rules.set_level("ok.test", None));
        assert!(rules.is_empty());
        assert!(!rules.remove("ok.test"));
    }

    #[test]
    fn the_list_is_capped() {
        let text: String = (0..MAX_SITES + 50).map(|n| format!("site{n}.test level=none\n")).collect();
        let mut rules = SiteRules::parse(&text);
        assert_eq!(rules.len(), MAX_SITES);
        assert!(!rules.set_level("one-more.test", Some(Level::Ads)));
        assert!(rules.set_level("site1.test", Some(Level::Ads)), "an existing site can still change");
    }
}
