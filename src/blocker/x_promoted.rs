//! X (x.com / twitter.com) promoted posts: hide the timeline cell of every post labelled "Ad".
//!
//! X serves promoted posts from the same GraphQL responses as normal ones, with class names that
//! change on every deploy, so there is no request rule and no stable CSS selector. Instead a
//! small page script ([`SCRIPT`]) marks the cell of a promoted post with `data-bowser-ad` and one
//! fixed CSS rule ([`CSS`]) hides marked cells. Both apply only on X hosts, and only where the
//! blocker is at the "trackers and ads" level for the site (a site exception or a lower level
//! turns them off), so there is no setting of its own. Nothing from the page is read beyond the
//! badge text/accessibility labels of timeline posts, and nothing leaves the page.

/// The page script. The engine calls [`STOP_SCRIPT`] when its site policy stops applying.
pub const SCRIPT: &str = include_str!("x_promoted.js");

/// Stop observing and reveal recycled/marked cells even if cookie-notice CSS remains installed.
pub const STOP_SCRIPT: &str = "window.__bowserXPromoted?.stop()";

/// The cosmetic rule that hides the cells [`SCRIPT`] marks.
pub const CSS: &str = "[data-bowser-ad]{display:none!important}";

/// True for `x.com`, `twitter.com` and their subdomains, by exact suffix.
pub fn is_x_host(host: &str) -> bool {
    ["x.com", "twitter.com"].iter().any(|site| {
        host.strip_suffix(site)
            .is_some_and(|rest| rest.is_empty() || rest.ends_with('.'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_x_and_twitter_hosts_exactly() {
        for host in ["x.com", "www.x.com", "twitter.com", "mobile.twitter.com"] {
            assert!(is_x_host(host), "{host}");
        }
        for host in ["", "box.com", "notx.com", "x.com.evil.example", "twitter.com.au", "fakex.com"] {
            assert!(!is_x_host(host), "{host}");
        }
    }

    #[test]
    fn the_rule_is_valid_cosmetic_css() {
        assert!(CSS.ends_with("{display:none!important}"));
        assert!(!SCRIPT.contains("fetch(") && !SCRIPT.contains("XMLHttpRequest"));
    }
}
