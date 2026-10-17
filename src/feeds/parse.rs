//! Reading a feed (RSS 2.0, RSS 1.0, Atom) and an OPML list into plain values. Everything that comes
//! out is text with the markup removed and every address checked: titles and summaries are plain text,
//! a link is an absolute http or https address, and nothing here is ever HTML the chrome could inject.

use url::Url;

use super::xml::{self, Element, XmlError};

/// Most items kept from one feed.
pub const MAX_ITEMS: usize = 100;
/// Longest title kept, in characters.
pub const MAX_TITLE_CHARS: usize = 200;
/// Longest summary kept, in characters.
pub const MAX_SUMMARY_CHARS: usize = 400;
/// Longest address kept, in bytes.
pub const MAX_URL_BYTES: usize = 2048;
/// Most feeds read from one OPML file.
pub const MAX_OPML_FEEDS: usize = 500;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Enclosure {
    pub url: String,
    pub mime: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedItem {
    /// The feed's own identifier for the item (guid or id); empty when it has none.
    pub guid: String,
    pub title: String,
    /// An absolute http or https address, or empty.
    pub link: String,
    pub summary: String,
    /// Seconds since 1970, `0` when the item has no readable date.
    pub published: u64,
    pub enclosure: Option<Enclosure>,
    /// The YouTube video id of a YouTube feed entry, else empty.
    pub video_id: String,
    /// The podcast's own length text (`45:12`), capped.
    pub duration: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedFeed {
    pub title: String,
    /// The site the feed belongs to: an absolute http or https address, or empty.
    pub site: String,
    pub items: Vec<ParsedItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    Xml(XmlError),
    /// A well-formed XML document that is not a feed.
    NotAFeed,
}

impl From<XmlError> for ParseError {
    fn from(error: XmlError) -> Self {
        Self::Xml(error)
    }
}

/// Parses the bytes of a feed fetched from `base` (relative links resolve against it).
pub fn parse_feed(bytes: &[u8], base: &Url) -> Result<ParsedFeed, ParseError> {
    let text = xml::decode(bytes)?;
    let root = xml::parse(&text)?;
    match root.local() {
        "rss" => {
            let channel = root.child("channel").ok_or(ParseError::NotAFeed)?;
            Ok(parse_rss(channel, channel.children_named("item"), base))
        }
        "RDF" => {
            let channel = root.child("channel").ok_or(ParseError::NotAFeed)?;
            Ok(parse_rss(channel, root.children_named("item"), base))
        }
        "feed" => Ok(parse_atom(&root, base)),
        _ => Err(ParseError::NotAFeed),
    }
}

fn parse_rss<'a>(channel: &Element, items: impl Iterator<Item = &'a Element>, base: &Url) -> ParsedFeed {
    ParsedFeed {
        title: plain_text(&channel.child_text("title").unwrap_or_default(), MAX_TITLE_CHARS),
        site: channel.child_text("link").and_then(|link| safe_link(base, &link)).unwrap_or_default(),
        items: items.take(MAX_ITEMS).map(|item| rss_item(item, base)).collect(),
    }
}

fn rss_item(item: &Element, base: &Url) -> ParsedItem {
    let link = item.child_text("link").and_then(|link| safe_link(base, &link)).unwrap_or_default();
    let guid = item.child_text("guid").or_else(|| item.child_text("id")).unwrap_or_default();
    let summary = item.child_text("description").or_else(|| item.child_text("content:encoded")).unwrap_or_default();
    let date = item.child_text("pubDate").or_else(|| item.child_text("dc:date")).or_else(|| item.child_text("date")).unwrap_or_default();
    let enclosure = item.children_named("enclosure").find_map(|enclosure| {
        let url = safe_link(base, enclosure.attribute("url")?)?;
        Some(Enclosure { url, mime: plain_text(enclosure.attribute("type").unwrap_or(""), 64) })
    });
    ParsedItem {
        guid: capped(&guid, 512),
        title: plain_text(&item.child_text("title").unwrap_or_default(), MAX_TITLE_CHARS),
        link,
        summary: plain_text(&summary, MAX_SUMMARY_CHARS),
        published: parse_date(&date).unwrap_or(0),
        enclosure,
        video_id: String::new(),
        duration: plain_text(&item.child_text("itunes:duration").unwrap_or_default(), 16),
    }
}

fn parse_atom(feed: &Element, base: &Url) -> ParsedFeed {
    let feed_base = feed.attribute("xml:base").and_then(|value| base.join(value).ok()).unwrap_or_else(|| base.clone());
    ParsedFeed {
        title: plain_text(&feed.child_text("title").unwrap_or_default(), MAX_TITLE_CHARS),
        site: atom_link(feed, &feed_base, &["alternate", ""]).unwrap_or_default(),
        items: feed.children_named("entry").take(MAX_ITEMS).map(|entry| atom_entry(entry, &feed_base)).collect(),
    }
}

/// The address of the first `<link>` whose `rel` is one of `rels` (`""` for no `rel`).
fn atom_link(parent: &Element, base: &Url, rels: &[&str]) -> Option<String> {
    parent.children_named("link").find_map(|link| {
        let rel = link.attribute("rel").unwrap_or("");
        if !rels.contains(&rel) {
            return None;
        }
        safe_link(base, link.attribute("href")?)
    })
}

fn atom_entry(entry: &Element, base: &Url) -> ParsedItem {
    let base = entry.attribute("xml:base").and_then(|value| base.join(value).ok()).unwrap_or_else(|| base.clone());
    let enclosure = entry.children_named("link").find_map(|link| {
        if link.attribute("rel") != Some("enclosure") {
            return None;
        }
        let url = safe_link(&base, link.attribute("href")?)?;
        Some(Enclosure { url, mime: plain_text(link.attribute("type").unwrap_or(""), 64) })
    });
    let summary = entry
        .child_text("summary")
        .or_else(|| entry.child_text("content"))
        .or_else(|| entry.child("media:group").and_then(|group| group.child_text("media:description")))
        .unwrap_or_default();
    let date = entry.child_text("published").or_else(|| entry.child_text("updated")).unwrap_or_default();
    let video_id = entry.child_text("yt:videoId").filter(|id| is_video_id(id)).unwrap_or_default();
    ParsedItem {
        guid: capped(&entry.child_text("id").unwrap_or_default(), 512),
        title: plain_text(&entry.child_text("title").unwrap_or_default(), MAX_TITLE_CHARS),
        link: atom_link(entry, &base, &["alternate", ""]).unwrap_or_default(),
        summary: plain_text(&summary, MAX_SUMMARY_CHARS),
        published: parse_date(&date).unwrap_or(0),
        enclosure,
        video_id,
        duration: String::new(),
    }
}

/// A YouTube video id: eleven URL-safe characters.
pub fn is_video_id(text: &str) -> bool {
    text.len() == 11 && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn capped(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect()
}

/// `raw` resolved against `base` as an absolute http or https address with no user name or password,
/// or `None`. The one gate every address from a feed passes.
pub fn safe_link(base: &Url, raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > MAX_URL_BYTES || raw.chars().any(char::is_control) {
        return None;
    }
    let url = base.join(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() || url.host_str().is_none() {
        return None;
    }
    let text = url.to_string();
    (text.len() <= MAX_URL_BYTES).then_some(text)
}

// ---------------------------------------------------------------------------------------------
// Text: the markup of a summary is removed, never kept.

/// `raw` as one line of plain text of at most `max` characters: tags removed (a `script` or `style`
/// element with its content), entities decoded, control characters dropped, white space collapsed.
pub fn plain_text(raw: &str, max: usize) -> String {
    let mut stripped = String::with_capacity(raw.len().min(4096));
    let mut rest = raw;
    // Feeds put escaped HTML in text; the XML reader decoded one level already, so tags are literal here.
    while let Some(open) = rest.find('<') {
        stripped.push_str(&rest[..open]);
        let after = &rest[open..];
        let lower: String = after.chars().take(9).collect::<String>().to_ascii_lowercase();
        if lower.starts_with("<script") || lower.starts_with("<style") {
            let name = if lower.starts_with("<script") { "</script" } else { "</style" };
            let haystack = after.to_ascii_lowercase();
            match haystack.find(name).and_then(|at| haystack[at..].find('>').map(|end| at + end + 1)) {
                Some(end) => rest = &after[end..],
                None => rest = "",
            }
            continue;
        }
        if after.starts_with("<!--") {
            match after.find("-->") {
                Some(end) => rest = &after[end + 3..],
                None => rest = "",
            }
            continue;
        }
        // A real tag starts with a letter, `/` or `!`; a lone `<` ("a < b") is text.
        let is_tag = after[1..].chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '/' || c == '!' || c == '?');
        if !is_tag {
            stripped.push('<');
            rest = &after[1..];
            continue;
        }
        stripped.push(' ');
        match after.find('>') {
            Some(end) => rest = &after[end + 1..],
            None => rest = "",
        }
    }
    stripped.push_str(rest);
    let decoded = decode_html_entities(&stripped);
    let mut out = String::new();
    let mut count = 0usize;
    let mut pending_space = false;
    let mut truncated = false;
    for c in decoded.chars() {
        if c.is_whitespace() || c == '\u{a0}' || c == '\u{200b}' {
            pending_space = !out.is_empty();
            continue;
        }
        if c.is_control() {
            continue;
        }
        if count + usize::from(pending_space) + 1 > max {
            truncated = true;
            break;
        }
        if pending_space {
            out.push(' ');
            count += 1;
            pending_space = false;
        }
        out.push(c);
        count += 1;
    }
    if truncated && max > 1 {
        if count >= max {
            out.pop();
        }
        out.push('\u{2026}');
    }
    out
}

/// The XML entities plus the handful of named HTML ones feeds use.
fn decode_html_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|&end| end <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let named = match name {
            "nbsp" => Some(' '),
            "hellip" => Some('\u{2026}'),
            "ndash" => Some('\u{2013}'),
            "mdash" => Some('\u{2014}'),
            "lsquo" => Some('\u{2018}'),
            "rsquo" => Some('\u{2019}'),
            "ldquo" => Some('\u{201c}'),
            "rdquo" => Some('\u{201d}'),
            "copy" => Some('\u{a9}'),
            "amp" | "lt" | "gt" | "quot" | "apos" => xml::decode_entities(&rest[..=end]).chars().next(),
            _ if name.starts_with('#') => xml::decode_entities(&rest[..=end]).chars().next().filter(|&c| c != '&'),
            _ => None,
        };
        match named {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------------------------
// Dates.

/// Seconds since 1970 for an RFC 822 date (`Mon, 02 Jan 2006 15:04:05 +0100`), an RFC 3339 / ISO 8601
/// one (`2006-01-02T15:04:05.5Z`) or a bare `2006-01-02`.
pub fn parse_date(text: &str) -> Option<u64> {
    let text = text.trim();
    if text.is_empty() || text.len() > 64 {
        return None;
    }
    parse_iso(text).or_else(|| parse_rfc822(text))
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn to_unix(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64, offset_minutes: i64) -> Option<u64> {
    if !(1970..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) || !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=60).contains(&second) {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset_minutes * 60;
    u64::try_from(seconds).ok()
}

fn parse_iso(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    if bytes.len() < 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    if bytes.len() == 10 {
        return to_unix(year, month, day, 0, 0, 0, 0);
    }
    if !matches!(bytes[10], b'T' | b't' | b' ') || bytes.len() < 16 || bytes[13] != b':' {
        return None;
    }
    let (hour, minute) = (number(11..13)?, number(14..16)?);
    let mut at = 16;
    let mut second = 0;
    if bytes.get(at) == Some(&b':') {
        second = number(at + 1..at + 3)?;
        at += 3;
    }
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
    }
    let offset = match bytes.get(at) {
        None => 0,
        Some(b'Z' | b'z') => 0,
        Some(&sign @ (b'+' | b'-')) => {
            let hours = number(at + 1..at + 3)?;
            let minutes = if bytes.get(at + 3) == Some(&b':') { number(at + 4..at + 6)? } else { number(at + 3..at + 5).unwrap_or(0) };
            let total = hours * 60 + minutes;
            if sign == b'-' { -total } else { total }
        }
        Some(_) => return None,
    };
    to_unix(year, month, day, hour, minute, second, offset)
}

fn parse_rfc822(text: &str) -> Option<u64> {
    let text = text.split_once(',').map_or(text, |(_, rest)| rest).trim();
    let mut parts = text.split_whitespace();
    let day: i64 = parts.next()?.parse().ok()?;
    let month = match parts.next()?.to_ascii_lowercase().get(..3)? {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    };
    let mut year: i64 = parts.next()?.parse().ok()?;
    if year < 100 {
        year += if year < 70 { 2000 } else { 1900 };
    }
    let mut clock = parts.next().unwrap_or("00:00:00").split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next().unwrap_or("0").parse().ok()?;
    let second: i64 = clock.next().unwrap_or("0").parse().ok()?;
    let zone = parts.next().unwrap_or("GMT");
    let offset = match zone.to_ascii_uppercase().as_str() {
        "GMT" | "UT" | "UTC" | "Z" => 0,
        "EST" => -300,
        "EDT" => -240,
        "CST" => -360,
        "CDT" => -300,
        "MST" => -420,
        "MDT" => -360,
        "PST" => -480,
        "PDT" => -420,
        numeric if numeric.len() == 5 && (numeric.starts_with('+') || numeric.starts_with('-')) => {
            let hours: i64 = numeric[1..3].parse().ok()?;
            let minutes: i64 = numeric[3..5].parse().ok()?;
            let total = hours * 60 + minutes;
            if numeric.starts_with('-') { -total } else { total }
        }
        _ => 0,
    };
    to_unix(year, month, day, hour, minute, second, offset)
}

// ---------------------------------------------------------------------------------------------
// OPML.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpmlFeed {
    pub url: String,
    pub title: String,
}

/// The feeds an OPML file lists: every `outline` with an `xmlUrl` that is a plain http or https address,
/// in order, without repeats, at most [`MAX_OPML_FEEDS`].
pub fn parse_opml(bytes: &[u8]) -> Result<Vec<OpmlFeed>, ParseError> {
    let text = xml::decode(bytes)?;
    let root = xml::parse(&text)?;
    if root.local() != "opml" {
        return Err(ParseError::NotAFeed);
    }
    let mut out: Vec<OpmlFeed> = Vec::new();
    let base = Url::parse("https://opml.invalid/").expect("static base");
    fn walk(element: &Element, base: &Url, out: &mut Vec<OpmlFeed>) {
        for child in element.elements() {
            if out.len() >= MAX_OPML_FEEDS {
                return;
            }
            if child.name == "outline" {
                let url = child.attributes.iter().find(|(key, _)| key.eq_ignore_ascii_case("xmlurl")).map(|(_, value)| value.as_str());
                // Only an absolute address counts: the base is a placeholder that never resolves a relative one.
                if let Some(url) = url.filter(|url| url.contains("://")).and_then(|url| safe_link(base, url))
                    && !out.iter().any(|feed| feed.url == url)
                {
                    let title = child.attribute("title").or_else(|| child.attribute("text")).unwrap_or("");
                    out.push(OpmlFeed { url, title: plain_text(title, MAX_TITLE_CHARS) });
                }
            }
            walk(child, base, out);
        }
    }
    walk(&root, &base, &mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://example.com/feeds/main.xml").unwrap()
    }

    const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel><title>Example &amp; Co</title><link>https://example.com/</link>
<item><title>First <b>post</b></title><link>/posts/1</link><guid>post-1</guid>
<description>&lt;p&gt;Hello &lt;script&gt;alert(1)&lt;/script&gt;&lt;a href="javascript:x"&gt;world&lt;/a&gt; &amp;amp; more&lt;/p&gt;</description>
<pubDate>Tue, 10 Jun 2025 04:00:00 GMT</pubDate></item>
<item><title>Episode 2</title><link>https://example.com/ep2</link><enclosure url="https://cdn.example.com/ep2.mp3" length="1" type="audio/mpeg"/><itunes:duration>45:12</itunes:duration>
<pubDate>Wed, 11 Jun 2025 05:30:00 +0100</pubDate></item>
<item><title>Bad links</title><link>javascript:alert(1)</link><enclosure url="file:///c:/x.mp3" type="audio/mpeg"/></item>
</channel></rss>"#;

    #[test]
    fn rss_items_become_plain_text_with_checked_links() {
        let feed = parse_feed(RSS.as_bytes(), &base()).unwrap();
        assert_eq!(feed.title, "Example & Co");
        assert_eq!(feed.site, "https://example.com/");
        assert_eq!(feed.items.len(), 3);
        let first = &feed.items[0];
        assert_eq!(first.title, "First post");
        assert_eq!(first.link, "https://example.com/posts/1", "relative links resolve against the feed");
        assert_eq!(first.guid, "post-1");
        assert_eq!(first.summary, "Hello world & more", "markup and script content are removed");
        assert_eq!(first.published, 1_749_528_000);
        let second = &feed.items[1];
        assert_eq!(second.enclosure, Some(Enclosure { url: "https://cdn.example.com/ep2.mp3".into(), mime: "audio/mpeg".into() }));
        assert_eq!(second.duration, "45:12");
        assert_eq!(second.published, 1_749_616_200, "the +0100 offset is applied");
        let bad = &feed.items[2];
        assert_eq!(bad.link, "", "a javascript: link is dropped");
        assert_eq!(bad.enclosure, None, "a file: enclosure is dropped");
    }

    #[test]
    fn atom_entries_read_links_dates_and_youtube_ids() {
        let atom = r#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:yt="http://www.youtube.com/xml/schemas/2015" xmlns:media="http://search.yahoo.com/mrss/">
<title>Channel</title><link rel="self" href="https://www.youtube.com/feeds/videos.xml?channel_id=UCx"/><link rel="alternate" href="https://www.youtube.com/channel/UCx"/>
<entry><id>yt:video:dQw4w9WgXcQ</id><yt:videoId>dQw4w9WgXcQ</yt:videoId><title>A video</title>
<link rel="alternate" href="https://www.youtube.com/watch?v=dQw4w9WgXcQ"/><published>2025-06-10T04:00:00+00:00</published><updated>2025-06-11T04:00:00+00:00</updated>
<media:group><media:title>A video</media:title><media:description>About it</media:description></media:group></entry>
<entry><id>2</id><yt:videoId>bad id!</yt:videoId><title>No id</title><link href="https://example.com/x"/><updated>2025-06-10</updated></entry></feed>"#;
        let feed = parse_feed(atom.as_bytes(), &Url::parse("https://www.youtube.com/feeds/videos.xml?channel_id=UCx").unwrap()).unwrap();
        assert_eq!(feed.title, "Channel");
        assert_eq!(feed.site, "https://www.youtube.com/channel/UCx");
        assert_eq!(feed.items[0].video_id, "dQw4w9WgXcQ");
        assert_eq!(feed.items[0].link, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        assert_eq!(feed.items[0].summary, "About it");
        assert_eq!(feed.items[0].published, 1_749_528_000);
        assert_eq!(feed.items[1].video_id, "", "an id that is not eleven URL-safe characters is dropped");
        assert_eq!(feed.items[1].link, "https://example.com/x");
        assert_eq!(feed.items[1].published, 1_749_513_600);
    }

    #[test]
    fn rss_one_rdf_feeds_are_read_too() {
        let rdf = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns="http://purl.org/rss/1.0/" xmlns:dc="http://purl.org/dc/elements/1.1/">
<channel><title>Old</title><link>http://old.example/</link></channel>
<item><title>One</title><link>http://old.example/1</link><dc:date>2005-03-04T05:06:07Z</dc:date></item></rdf:RDF>"#;
        let feed = parse_feed(rdf.as_bytes(), &Url::parse("http://old.example/rss").unwrap()).unwrap();
        assert_eq!(feed.title, "Old");
        assert_eq!(feed.items.len(), 1);
        assert_eq!(feed.items[0].published, 1_109_912_767);
    }

    #[test]
    fn a_page_or_a_doctype_is_not_a_feed() {
        assert_eq!(parse_feed(b"<html><body>hi</body></html>", &base()), Err(ParseError::NotAFeed));
        assert!(matches!(parse_feed(b"<!DOCTYPE rss><rss/>", &base()), Err(ParseError::Xml(XmlError::Doctype))));
        assert!(parse_feed(b"not xml", &base()).is_err());
        assert_eq!(parse_feed(b"<rss version='2.0'></rss>", &base()), Err(ParseError::NotAFeed));
    }

    #[test]
    fn at_most_a_hundred_items_are_kept() {
        let items: String = (0..150).map(|n| format!("<item><title>{n}</title></item>")).collect();
        let feed = parse_feed(format!("<rss><channel><title>t</title>{items}</channel></rss>").as_bytes(), &base()).unwrap();
        assert_eq!(feed.items.len(), MAX_ITEMS);
        assert_eq!(feed.items[0].title, "0");
    }

    #[test]
    fn plain_text_removes_tags_scripts_comments_and_long_runs() {
        assert_eq!(plain_text("<p>a&nbsp;&nbsp;b</p>\n<!-- hidden --><style>p{}</style>c &lt; d &#169;", 100), "a b c < d \u{a9}");
        assert_eq!(plain_text("a < b and c > d", 100), "a < b and c > d");
        assert_eq!(plain_text("<script>never", 100), "");
        assert_eq!(plain_text("x\u{0}y\u{7}z", 100), "xyz");
        let long = "word ".repeat(200);
        let cut = plain_text(&long, 20);
        assert!(cut.chars().count() <= 20 && cut.ends_with('\u{2026}'), "{cut:?}");
        assert_eq!(plain_text("   ", 10), "");
    }

    #[test]
    fn only_plain_web_addresses_pass_the_link_gate() {
        let b = base();
        assert_eq!(safe_link(&b, "https://a.example/x?y=1#z").as_deref(), Some("https://a.example/x?y=1#z"));
        assert_eq!(safe_link(&b, "../up").as_deref(), Some("https://example.com/up"));
        for bad in ["javascript:alert(1)", "data:text/html,x", "file:///c:/x", "ftp://a.example/", "https://user:pw@a.example/", "https://user@a.example/", "", "  ", "https://a.example/\u{7}"] {
            assert_eq!(safe_link(&b, bad), None, "{bad:?}");
        }
        assert_eq!(safe_link(&b, &format!("https://a.example/{}", "x".repeat(MAX_URL_BYTES))), None);
    }

    #[test]
    fn dates_read_both_formats_and_refuse_nonsense() {
        assert_eq!(parse_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(parse_date("1 Jan 70 00:00:01"), Some(1));
        assert_eq!(parse_date("2000-02-29T12:00:00Z"), Some(951_825_600));
        assert_eq!(parse_date("2000-02-29T12:00:00.123+02:00"), Some(951_825_600 - 7200));
        assert_eq!(parse_date("2000-02-29"), Some(951_782_400));
        assert_eq!(parse_date("Tue, 10 Jun 2025 04:00:00 EST"), Some(1_749_528_000 + 5 * 3600));
        for bad in ["", "yesterday", "2025-13-01", "2025-02-30T25:00:00Z", "Mon, 32 Jan 2025 00:00:00 GMT", "1969-12-31T23:59:59Z", "x".repeat(80).as_str()] {
            assert_eq!(parse_date(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn opml_lists_absolute_feeds_without_repeats_or_odd_schemes() {
        let opml = r#"<?xml version="1.0"?><opml version="2.0"><head><title>x</title></head><body>
<outline text="News"><outline type="rss" text="A &amp; B" xmlUrl="https://a.example/feed.xml" htmlUrl="https://a.example/"/>
<outline type="rss" title="Again" xmlUrl="https://a.example/feed.xml"/><outline text="Local" xmlUrl="file:///c:/x.xml"/><outline text="Rel" xmlUrl="/rel.xml"/></outline>
<outline text="B" xmlurl="http://b.example/rss"/></body></opml>"#;
        let feeds = parse_opml(opml.as_bytes()).unwrap();
        assert_eq!(feeds, vec![
            OpmlFeed { url: "https://a.example/feed.xml".into(), title: "A & B".into() },
            OpmlFeed { url: "http://b.example/rss".into(), title: "B".into() },
        ]);
        assert_eq!(parse_opml(b"<rss/>"), Err(ParseError::NotAFeed));
        assert!(parse_opml(b"<!DOCTYPE opml><opml/>").is_err());
        let many: String = (0..600).map(|n| format!("<outline xmlUrl=\"https://f{n}.example/x\"/>")).collect();
        assert_eq!(parse_opml(format!("<opml><body>{many}</body></opml>").as_bytes()).unwrap().len(), MAX_OPML_FEEDS);
    }
}
