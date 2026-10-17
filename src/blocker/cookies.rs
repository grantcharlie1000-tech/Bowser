//! Cookie-notice rules: element-hiding rules that remove cookie-consent banners, kept apart from
//! the ad and tracker rules so they work without the blocker switched on.
//!
//! Rule sources are the bundled `cookie-notices.txt` and any "cookie notice" list the user added
//! (an EasyList Cookie List or "I don't care about cookies" style list, saved from a file or
//! fetched from a URL the user typed). Only element-hiding lines (`domain##selector`) are read;
//! network rules, exceptions and every other line are ignored, so a list can hide banners but
//! never block a request.
//!
//! The only output is CSS. Every selector is checked against a strict grammar before it is kept,
//! because the CSS is injected into pages: no braces, quotes outside an attribute value,
//! backslashes, parentheses, semicolons or pseudo-classes. A hiding rule may not end in `html`,
//! `body` or `*`, so no list can blank a whole page. `##@scroll:selector` lines give scrolling
//! back to a page a banner left locked; they only ever set `overflow`.

use super::{domain_list_matches, valid_rule_host};

/// The list shipped inside the program.
pub const BUNDLED_LIST: &str = include_str!("../cookie-notices.txt");

/// Most generic (every-site) hiding rules kept across all lists.
pub const MAX_GENERIC_RULES: usize = 1_500;
/// Most site-specific hiding rules kept across all lists.
pub const MAX_SCOPED_RULES: usize = 6_000;
/// Most scroll-unlock rules kept.
pub const MAX_UNLOCK_RULES: usize = 64;
/// Longest selector kept, in bytes.
const MAX_SELECTOR_BYTES: usize = 200;
/// Largest CSS string returned for one page, in bytes.
pub const MAX_CSS_BYTES: usize = 96 * 1024;

#[derive(Default)]
pub struct CookieRules {
    generic: Vec<Box<str>>,
    scoped: Vec<(Vec<Box<str>>, Box<str>)>,
    unlock: Vec<(Vec<Box<str>>, Box<str>)>,
}

impl CookieRules {
    /// The bundled list alone.
    pub fn bundled() -> Self {
        let mut rules = Self::default();
        rules.add_list(BUNDLED_LIST);
        rules
    }

    /// Adds every valid element-hiding or scroll rule in `text`; returns how many were added.
    pub fn add_list(&mut self, text: &str) -> usize {
        let mut added = 0;
        for line in text.lines() {
            let Some((domains, selector, unlock)) = parse_line(line) else {
                continue;
            };
            if unlock {
                if self.unlock.len() < MAX_UNLOCK_RULES {
                    self.unlock.push((domains, selector));
                    added += 1;
                }
            } else if domains.is_empty() {
                if self.generic.len() < MAX_GENERIC_RULES && !self.generic.contains(&selector) {
                    self.generic.push(selector);
                    added += 1;
                }
            } else if self.scoped.len() < MAX_SCOPED_RULES {
                self.scoped.push((domains, selector));
                added += 1;
            }
        }
        added
    }

    pub fn rule_count(&self) -> usize {
        self.generic.len() + self.scoped.len() + self.unlock.len()
    }

    /// The CSS that hides the cookie notices on `page_host` (and restores scrolling), or `None`
    /// when no rule applies. One rule per selector, so a selector the browser rejects cannot
    /// disable the others.
    pub fn css_for_host(&self, page_host: &str) -> Option<String> {
        let mut css = String::new();
        let mut push = |selector: &str, declaration: &str| {
            if css.len() + selector.len() + declaration.len() + 2 <= MAX_CSS_BYTES {
                css.push_str(selector);
                css.push('{');
                css.push_str(declaration);
                css.push('}');
            }
        };
        for selector in &self.generic {
            push(selector, "display:none!important");
        }
        for (domains, selector) in &self.scoped {
            if domain_list_matches(page_host, domains) {
                push(selector, "display:none!important");
            }
        }
        for (domains, selector) in &self.unlock {
            if domain_list_matches(page_host, domains) {
                push(selector, "overflow:auto!important");
            }
        }
        (!css.is_empty()).then_some(css)
    }
}

/// `(domains, selector, unlock)` for one list line, or `None` when it is not a rule Bowser keeps.
fn parse_line(line: &str) -> Option<(Vec<Box<str>>, Box<str>, bool)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('!') || line.starts_with('[') {
        return None;
    }
    // Exceptions and every other cosmetic dialect are not element hiding.
    if ["#@#", "#?#", "#$#", "#%#"].iter().any(|marker| line.contains(marker)) {
        return None;
    }
    let (domain_part, rest) = line.split_once("##")?;
    let (selector, unlock) = match rest.strip_prefix("@scroll:") {
        Some(selector) => (selector.trim(), true),
        None => (rest.trim(), false),
    };
    if !valid_selector(selector, unlock) {
        return None;
    }
    let domain_part = domain_part.trim();
    // A rule for every site must name something specific, or it hides ordinary page structure everywhere.
    if domain_part.is_empty() && !specific_selector(selector) {
        return None;
    }
    let mut domains: Vec<Box<str>> = Vec::new();
    if !domain_part.is_empty() {
        for entry in domain_part.split(',') {
            let entry = entry.trim().to_ascii_lowercase();
            if entry.contains('~') || !valid_rule_host(&entry) {
                return None;
            }
            domains.push(entry.into());
        }
    }
    Some((domains, selector.into(), unlock))
}

/// Whether `selector` is safe to put in an injected style rule. See the module notes.
pub fn valid_selector(selector: &str, unlock: bool) -> bool {
    if selector.is_empty() || selector.len() > MAX_SELECTOR_BYTES || !selector.is_ascii() {
        return false;
    }
    let mut in_quote = false;
    let mut in_bracket = false;
    for byte in selector.bytes() {
        if in_quote {
            match byte {
                b'"' => in_quote = false,
                b if b.is_ascii_alphanumeric() => {}
                b'_' | b'-' | b'.' | b' ' | b'/' | b':' | b'#' | b'%' | b'@' => {}
                _ => return false,
            }
            continue;
        }
        match byte {
            b'"' if in_bracket => in_quote = true,
            b'[' if !in_bracket => in_bracket = true,
            b']' if in_bracket => in_bracket = false,
            b if b.is_ascii_alphanumeric() => {}
            b'_' | b'-' | b'.' | b'#' | b' ' | b'>' | b'+' | b',' => {}
            b'=' | b'*' | b'^' | b'$' | b'~' | b'|' if in_bracket => {}
            _ => return false,
        }
    }
    if in_quote || in_bracket {
        return false;
    }
    selector.split(',').all(|part| {
        let part = part.trim();
        if part.is_empty() || part.starts_with(['>', '+']) || part.ends_with(['>', '+']) {
            return false;
        }
        if unlock {
            return true;
        }
        // The compound that is actually hidden is the last one.
        let last = last_compound(part);
        let tag_end = last.find(['.', '#', '[']).unwrap_or(last.len());
        let tag = &last[..tag_end];
        !(tag.eq_ignore_ascii_case("html") || tag.eq_ignore_ascii_case("body") || tag == "*")
    })
}

/// Whether every comma-separated part of `selector` ends in a compound that names a class, an id or an
/// attribute value (`.x`, `#x`, `[a="b"]`), so a generic rule cannot be a bare `div` or `[class]`.
/// Expects a selector [`valid_selector`] accepted.
fn specific_selector(selector: &str) -> bool {
    selector.split(',').all(|part| {
        let mut in_bracket = false;
        last_compound(part.trim()).bytes().any(|byte| match byte {
            b'[' => {
                in_bracket = true;
                false
            }
            b']' => {
                in_bracket = false;
                false
            }
            b'=' => in_bracket,
            b'.' | b'#' => !in_bracket,
            _ => false,
        })
    })
}

/// The last compound selector of one comma-free selector: what follows the final space, `>` or
/// `+` that is not inside an attribute test (a space inside `[a="b c"]` does not split it).
fn last_compound(part: &str) -> &str {
    let mut start = 0;
    let mut in_bracket = false;
    for (index, byte) in part.bytes().enumerate() {
        match byte {
            b'[' => in_bracket = true,
            b']' => in_bracket = false,
            b' ' | b'>' | b'+' if !in_bracket => start = index + 1,
            _ => {}
        }
    }
    &part[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_space_inside_an_attribute_value_does_not_hide_the_page_body() {
        assert!(!valid_selector("body[x=\"a b\"]", false));
        assert!(!valid_selector("div > body[x=\"a > b\"]", false));
        assert!(valid_selector("div[x=\"a body\"]", false));
        assert_eq!(last_compound(".a > .b[c=\"d e\"]"), ".b[c=\"d e\"]");
    }

    #[test]
    fn a_generic_rule_must_name_a_class_an_id_or_an_attribute_value() {
        for vague in ["##div", "##[class]", "##div > span", "##.banner, div", "##[id] p", "##@scroll:div", "##div[data-x]"] {
            assert!(parse_line(vague).is_none(), "{vague}");
        }
        for specific in ["##.cookie-banner", "###consent", "##div[class=\"cookie\"]", "##[id^=cookie]", "##.a, div#b", "##.wrap > div.notice", "##@scroll:html.no-scroll"] {
            assert!(parse_line(specific).is_some(), "{specific}");
        }
        // A rule scoped to a site may stay broad: it only runs there.
        assert!(parse_line("example.com##div").is_some());
        assert!(parse_line("example.com##[class]").is_some());
        // A dot or hash inside an attribute test does not count; only its value does.
        assert!(!specific_selector("[a.b]"));
    }

    #[test]
    fn every_bundled_rule_passes_the_selector_grammar() {
        let mut bad = Vec::new();
        for line in BUNDLED_LIST.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('!') {
                continue;
            }
            if parse_line(line).is_none() {
                bad.push(line.to_owned());
            }
        }
        assert!(bad.is_empty(), "bundled lines the parser refuses: {bad:?}");
        let rules = CookieRules::bundled();
        assert!(rules.rule_count() > 100, "{}", rules.rule_count());
    }

    #[test]
    fn a_generic_rule_applies_everywhere_and_a_scoped_rule_only_on_its_site() {
        let mut rules = CookieRules::default();
        assert_eq!(
            rules.add_list("##.cookie-banner\nexample.com##.promo-consent\n"),
            2
        );
        let anywhere = rules.css_for_host("other.org").unwrap();
        assert_eq!(anywhere, ".cookie-banner{display:none!important}");
        let here = rules.css_for_host("www.example.com").unwrap();
        assert!(here.contains(".cookie-banner{display:none!important}"));
        assert!(here.contains(".promo-consent{display:none!important}"));
        assert!(!rules.css_for_host("notexample.com").unwrap().contains("promo"));
    }

    #[test]
    fn scroll_rules_only_set_overflow() {
        let mut rules = CookieRules::default();
        assert_eq!(rules.add_list("##@scroll:body.cookie-modal-open\n"), 1);
        assert_eq!(
            rules.css_for_host("a.com").unwrap(),
            "body.cookie-modal-open{overflow:auto!important}"
        );
    }

    #[test]
    fn selectors_that_could_break_out_of_the_style_rule_are_refused() {
        for bad in [
            ".a{background:url(//evil.test/x)}",
            ".a}.b{color:red",
            ".a;b",
            ".a\\62",
            "a[href=\"x\"]{x:y}",
            ".a:hover",
            ".a:not(.b)",
            "a[b=\"c]",
            "a[b=\"c\"",
            "a(b)",
            "",
            ">.a",
            ".a>",
            "html",
            "body",
            "body.has-banner",
            ".wrap > body",
            "*",
            ".a, *",
            "div[b=\"x\"{",
            "a[b=\"'\"]",
            "a[b=\"<\"]",
            ".é",
        ] {
            assert!(!valid_selector(bad, false), "{bad:?}");
            assert_eq!(CookieRules::default().add_list(&format!("##{bad}\n")), 0, "{bad:?}");
        }
        for good in [
            ".cookie-banner",
            "#onetrust-banner-sdk",
            "div[id^=\"sp_message_container_\"]",
            "div[role=\"dialog\"][aria-label=\"Cookie banner\"]",
            ".a > .b + .c, #d",
            "[data-x=\"a b\"]",
        ] {
            assert!(valid_selector(good, false), "{good:?}");
        }
        assert!(valid_selector("body.cookie-modal-open", true));
    }

    #[test]
    fn other_list_syntax_is_ignored() {
        let mut rules = CookieRules::default();
        let text = "! comment\n[Adblock Plus 2.0]\n||cookiebot.com^\n@@||x.com^\nexample.com#@#.cookie-banner\n\
                    example.com#?#div:has(.x)\n~example.com##.a\nnot a host##.b\n";
        assert_eq!(rules.add_list(text), 0);
        assert_eq!(rules.rule_count(), 0);
        assert_eq!(rules.css_for_host("example.com"), None);
    }

    #[test]
    fn the_css_stays_under_its_cap() {
        let mut rules = CookieRules::default();
        let text: String = (0..MAX_GENERIC_RULES).map(|n| format!("##.{}-{n}\n", "x".repeat(150))).collect();
        rules.add_list(&text);
        let css = rules.css_for_host("a.com").unwrap();
        assert!(css.len() <= MAX_CSS_BYTES);
    }
}
