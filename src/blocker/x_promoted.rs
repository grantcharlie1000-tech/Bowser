//! X (x.com / twitter.com) promoted posts.
//!
//! X serves promoted posts from the same GraphQL responses as normal ones, with class names that
//! change on every deploy, so there is no request rule and no stable CSS selector. A small page
//! script ([`SCRIPT`]) does two things:
//!
//! - It drops the entries X itself flags as promoted (an entryId starting `promoted-` /
//!   `promotedTweet`, or `promotedMetadata` on the entry) from X's own `/graphql/` timeline
//!   responses as the page reads them, so they never render. This does not depend on markup or on
//!   the UI language, which is what made the label-only version miss ads.
//! - As a fallback it marks the cell of a post whose header row shows the "Ad" label with
//!   `data-bowser-ad`, and one fixed CSS rule ([`CSS`]) hides marked cells.
//!
//! Both apply only on X hosts, and only where the blocker is at the "trackers and ads" level for
//! the site (a site exception or a lower level turns them off), so there is no setting of its own.
//! The script reads only X's own API responses and post labels inside the page; it never makes a
//! request, keeps nothing and sends nothing.

/// The page script. Needs the `bowser-cosmetic` style to be installed (it stops when it goes).
pub const SCRIPT: &str = include_str!("x_promoted.js");

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
        for host in ["x.com", "www.x.com", "mobile.x.com", "twitter.com", "mobile.twitter.com"] {
            assert!(is_x_host(host), "{host}");
        }
        for host in ["", "box.com", "notx.com", "x.com.evil.example", "twitter.com.au", "fakex.com"] {
            assert!(!is_x_host(host), "{host}");
        }
    }

    #[test]
    fn the_rule_is_valid_cosmetic_css() {
        assert!(CSS.ends_with("{display:none!important}"));
    }

    #[test]
    fn the_script_drops_promoted_timeline_entries_without_making_requests() {
        // The main path works on X's own timeline data, not on the localized label alone: a
        // label-only script missed promoted posts whenever the label or its markup differed.
        for marker in ["promotedMetadata", "promoted(?:-|tweet)", "/graphql\\/", "XMLHttpRequest.prototype", "Response.prototype"] {
            assert!(SCRIPT.contains(marker), "{marker}");
        }
        // Off when the cosmetic style is gone (a site exception or a lower level).
        assert!(SCRIPT.contains("getElementById('bowser-cosmetic')"));
        // It only wraps the page's readers; it never sends anything of its own.
        for request in ["fetch(", "new XMLHttpRequest", "sendBeacon", "WebSocket", "postMessage", ".send("] {
            assert!(!SCRIPT.contains(request), "{request}");
        }
    }
}
