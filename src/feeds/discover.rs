//! Finding the feeds a page offers: the `<link rel="alternate" type="application/rss+xml">` tags of its
//! head. The same filter ([`accept_link`]) judges what the page's own script reports (the address bar
//! button) and what a scan of a fetched page finds (subscribing from a page address).
//!
//! The scan is a forgiving tag reader, not an HTML parser: it never builds a tree, never runs or
//! follows anything, and looks at no more than [`SCAN_BYTES`] bytes.

use url::Url;

use super::parse::{plain_text, safe_link};

/// Most feeds listed for one page.
pub const MAX_LINKS: usize = 8;
/// How much of a page is scanned.
pub const SCAN_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedLink {
    /// An absolute http or https address.
    pub url: String,
    pub title: String,
}

const FEED_TYPES: [&str; 4] = ["application/rss+xml", "application/atom+xml", "application/rdf+xml", "application/feed+xml"];

/// A `<link>` as a feed: `rel` lists `alternate`, `mime` is a feed type and `href` is a plain web address
/// (resolved against `base`). The title is plain text.
pub fn accept_link(base: &Url, rel: &str, mime: &str, href: &str, title: &str) -> Option<FeedLink> {
    if !rel.split_whitespace().any(|token| token.eq_ignore_ascii_case("alternate")) {
        return None;
    }
    let mime = mime.trim().to_ascii_lowercase();
    let mime = mime.split(';').next().unwrap_or("").trim();
    if !FEED_TYPES.contains(&mime) {
        return None;
    }
    let url = safe_link(base, href)?;
    Some(FeedLink { url, title: plain_text(title, 100) })
}

/// Adds `link` to `out` unless the address is already there or the list is full.
pub fn push_unique(out: &mut Vec<FeedLink>, link: FeedLink) {
    if out.len() < MAX_LINKS && !out.iter().any(|other| other.url == link.url) {
        out.push(link);
    }
}

/// The feed links of the HTML in `bytes`, in page order.
pub fn scan_html(bytes: &[u8], base: &Url) -> Vec<FeedLink> {
    let head = &bytes[..bytes.len().min(SCAN_BYTES)];
    let text = String::from_utf8_lossy(head);
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = lower[at..].find('<') {
        let start = at + found;
        let rest = &lower[start..];
        if rest.starts_with("<!--") {
            at = lower[start..].find("-->").map_or(lower.len(), |end| start + end + 3);
            continue;
        }
        if rest.starts_with("<script") || rest.starts_with("<style") {
            let close = if rest.starts_with("<script") { "</script" } else { "</style" };
            at = lower[start..].find(close).map_or(lower.len(), |end| start + end + close.len());
            continue;
        }
        if rest.starts_with("<body") {
            break;
        }
        if rest.starts_with("<link") && rest[5..].starts_with(|c: char| c.is_ascii_whitespace() || c == '/') {
            let end = tag_end(&text, start);
            let attributes = read_attributes(&text[start + 5..end]);
            let get = |name: &str| attributes.iter().find(|(key, _)| key == name).map_or("", |(_, value)| value.as_str());
            if let Some(link) = accept_link(base, get("rel"), get("type"), get("href"), get("title")) {
                push_unique(&mut out, link);
                if out.len() >= MAX_LINKS {
                    break;
                }
            }
            at = end;
            continue;
        }
        at = start + 1;
    }
    out
}

/// The index of the `>` that closes the tag starting at `start` (quotes respected), or the text's end.
fn tag_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut quote = 0u8;
    let mut at = start + 1;
    while at < bytes.len() {
        let byte = bytes[at];
        if quote != 0 {
            if byte == quote {
                quote = 0;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = byte;
        } else if byte == b'>' {
            return at;
        }
        at += 1;
    }
    bytes.len()
}

/// The `name="value"` pairs of a tag's inside, names lower-cased and values with entities decoded.
fn read_attributes(inside: &str) -> Vec<(String, String)> {
    let bytes = inside.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() && out.len() < 16 {
        while at < bytes.len() && (bytes[at].is_ascii_whitespace() || bytes[at] == b'/') {
            at += 1;
        }
        let start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() && !matches!(bytes[at], b'=' | b'/' | b'>') {
            at += 1;
        }
        if at == start {
            at += 1;
            continue;
        }
        let name = inside[start..at].to_ascii_lowercase();
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let mut value = "";
        if bytes.get(at) == Some(&b'=') {
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            match bytes.get(at) {
                Some(&quote @ (b'"' | b'\'')) => {
                    let from = at + 1;
                    let to = inside[from..].find(quote as char).map_or(bytes.len(), |end| from + end);
                    value = &inside[from..to];
                    at = (to + 1).min(bytes.len());
                }
                _ => {
                    let from = at;
                    while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
                        at += 1;
                    }
                    value = &inside[from..at];
                }
            }
        }
        out.push((name, super::xml::decode_entities(value)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://blog.example/posts/index.html").unwrap()
    }

    #[test]
    fn head_links_are_found_in_order_and_resolved() {
        let html = r#"<!doctype html><html><head><meta charset=utf-8>
<link rel="stylesheet" href="/a.css">
<link rel="alternate" type="application/rss+xml" title="Posts &amp; more" href="/feed.xml">
<LINK REL='alternate' TYPE='application/atom+xml; charset=utf-8' HREF=atom.xml>
<link rel="alternate" type="text/html" href="/fr/">
<link rel="alternate" type="application/rss+xml" href="/feed.xml">
</head><body><link rel="alternate" type="application/rss+xml" href="/late.xml"></body></html>"#;
        let links = scan_html(html.as_bytes(), &base());
        assert_eq!(links, vec![
            FeedLink { url: "https://blog.example/feed.xml".into(), title: "Posts & more".into() },
            FeedLink { url: "https://blog.example/posts/atom.xml".into(), title: "".into() },
        ]);
    }

    #[test]
    fn commented_out_scripted_and_unsafe_links_are_ignored() {
        let html = r#"<head><!-- <link rel="alternate" type="application/rss+xml" href="/commented.xml"> -->
<script>var s = '<link rel="alternate" type="application/rss+xml" href="/script.xml">';</script>
<link rel="alternate" type="application/rss+xml" href="javascript:alert(1)">
<link rel="alternate" type="application/rss+xml" href="file:///c:/x.xml">
<link rel="alternate" type="application/rss+xml" href="https://user:pw@evil.example/x.xml">
<link rel="alternate" type="application/rss+xml" href="https://ok.example/x.xml"></head>"#;
        let links = scan_html(html.as_bytes(), &base());
        assert_eq!(links.iter().map(|l| l.url.as_str()).collect::<Vec<_>>(), ["https://ok.example/x.xml"]);
    }

    #[test]
    fn at_most_eight_links_and_only_the_start_of_a_page_are_read() {
        let many: String = (0..20).map(|n| format!(r#"<link rel="alternate" type="application/rss+xml" href="/f{n}.xml">"#)).collect();
        assert_eq!(scan_html(many.as_bytes(), &base()).len(), MAX_LINKS);
        let far = format!("{}{}", " ".repeat(SCAN_BYTES), r#"<link rel="alternate" type="application/rss+xml" href="/far.xml">"#);
        assert!(scan_html(far.as_bytes(), &base()).is_empty());
        assert!(scan_html(b"", &base()).is_empty());
        assert!(scan_html(b"<link rel=alternate type=application/rss+xml href=", &base()).is_empty(), "a cut-off tag is not a link");
    }

    #[test]
    fn script_reported_links_pass_the_same_gate() {
        let b = base();
        assert!(accept_link(&b, "alternate", "application/rss+xml", "/f.xml", "t").is_some());
        assert!(accept_link(&b, "alternate feed", "application/atom+xml", "/f.xml", "").is_some());
        assert!(accept_link(&b, "stylesheet", "application/rss+xml", "/f.xml", "").is_none());
        assert!(accept_link(&b, "alternate", "text/html", "/f.xml", "").is_none());
        assert!(accept_link(&b, "alternate", "application/rss+xml", "data:text/xml,x", "").is_none());
        let long = accept_link(&b, "alternate", "application/rss+xml", "/f.xml", &"<b>x</b>".repeat(100)).unwrap();
        assert!(long.title.chars().count() <= 100 && !long.title.contains('<'));
    }
}
