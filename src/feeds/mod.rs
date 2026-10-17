//! The feed reader's core (thread T13): what a feed is, how it is read, found, stored and fetched.
//!
//! - [`xml`]: a small safe XML reader (no DTD, no external anything, hard limits).
//! - [`parse`]: RSS, Atom and OPML into plain text and checked links.
//! - [`discover`]: the feeds a page offers (`<link rel="alternate">`).
//! - [`store`]: `feeds.txt`, the subscriptions and their items, with read and unread.
//! - [`fetch`]: the one network call, WinHTTP with the blocker's SSRF checks.
//!
//! Nothing in this module draws anything or knows about windows; the Feeds panel's wire is in
//! `chrome_host::panels::feeds` and its host in `win32::feeds_panel`. Docs: `docs/feeds.md`.

pub mod discover;
pub mod fetch;
pub mod parse;
pub mod store;
pub mod xml;

use url::Url;

use discover::FeedLink;
use fetch::FeedFetchError;
use parse::{ParseError, ParsedFeed};
use xml::XmlError;

/// "Refresh when I open Feeds" choices, in minutes: `0` is "only when I press Refresh".
pub const REFRESH_CHOICES: [u32; 6] = [0, 15, 30, 60, 180, 720];
/// The default: an hour. Feeds are fetched only when the panel is opened or Refresh is pressed.
pub const DEFAULT_REFRESH_MINUTES: u32 = 60;

/// What looking at an address found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The address is a feed (the final address after redirects, and what it holds).
    Feed { url: String, parsed: ParsedFeed },
    /// The address is a web page that offers these feeds.
    Candidates(Vec<FeedLink>),
}

/// Why looking at an address failed, in words for the panel.
pub fn error_text(error: &FeedFetchError) -> String {
    error.to_string()
}

fn parse_error_text(error: &ParseError) -> &'static str {
    match error {
        ParseError::NotAFeed => "That address is not a feed.",
        ParseError::Xml(XmlError::Doctype) => "That feed declares a DOCTYPE, which Bowser never reads.",
        ParseError::Xml(XmlError::TooBig) => "That feed is too large or too deeply nested to read.",
        ParseError::Xml(XmlError::Malformed) => "That is not a valid feed.",
    }
}

/// Fetches `text` (a feed or a web page) and says what it is: a feed, or a page and the feeds it
/// offers. A YouTube channel or playlist address is turned into its feed address first. Blocks; meant
/// for a worker thread.
pub fn look_at(text: &str) -> Result<Outcome, String> {
    let typed = fetch::parse_url(text).map_err(|error| error.to_string())?;
    let target = youtube_feed_address(&typed).unwrap_or(typed);
    let fetched = fetch::fetch(&target).map_err(|error| error_text(&error))?;
    classify(&fetched.url, &fetched.bytes)
}

/// Fetches a subscribed feed again and reads it. Blocks; meant for a worker thread.
pub fn refresh_one(url: &str) -> Result<ParsedFeed, String> {
    let url = fetch::parse_url(url).map_err(|error| error.to_string())?;
    let fetched = fetch::fetch(&url).map_err(|error| error_text(&error))?;
    parse::parse_feed(&fetched.bytes, &fetched.url).map_err(|error| parse_error_text(&error).to_owned())
}

/// A feed, or a page with feeds, or neither.
pub fn classify(url: &Url, bytes: &[u8]) -> Result<Outcome, String> {
    match parse::parse_feed(bytes, url) {
        Ok(parsed) => Ok(Outcome::Feed { url: url.to_string(), parsed }),
        Err(ParseError::NotAFeed) | Err(ParseError::Xml(_)) => {
            let links = discover::scan_html(bytes, url);
            if !links.is_empty() {
                return Ok(Outcome::Candidates(links));
            }
            let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).to_ascii_lowercase();
            if head.contains("<html") || head.contains("<!doctype html") {
                return Err("No feed found on that page.".to_owned());
            }
            match parse::parse_feed(bytes, url) {
                Err(error) => Err(parse_error_text(&error).to_owned()),
                Ok(_) => Err("No feed found there.".to_owned()),
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// YouTube: a channel or playlist address is a feed address in disguise, and a video opens on the
// cookie-free embed host.

fn youtube_host(url: &Url) -> bool {
    matches!(url.host_str().map(|host| host.to_ascii_lowercase()).as_deref(), Some("youtube.com" | "www.youtube.com" | "m.youtube.com"))
}

fn id_chars(text: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The feed address of a YouTube channel (`/channel/UC...`) or playlist (`/playlist?list=PL...`) page.
/// `None` for anything else, including `@handle` pages (those offer their feed in the page, which
/// [`look_at`] finds).
pub fn youtube_feed_address(url: &Url) -> Option<Url> {
    if !youtube_host(url) {
        return None;
    }
    let path = url.path().trim_end_matches('/');
    if let Some(id) = path.strip_prefix("/channel/")
        && id.starts_with("UC")
        && id_chars(id, 24, 24)
    {
        return Url::parse(&format!("https://www.youtube.com/feeds/videos.xml?channel_id={id}")).ok();
    }
    if path == "/playlist" {
        let list = url.query_pairs().find(|(key, _)| key == "list").map(|(_, value)| value.into_owned())?;
        if id_chars(&list, 10, 64) {
            return Url::parse(&format!("https://www.youtube.com/feeds/videos.xml?playlist_id={list}")).ok();
        }
    }
    None
}

/// Whether `url` is a YouTube feed address.
pub fn is_youtube_feed(url: &str) -> bool {
    Url::parse(url).is_ok_and(|url| youtube_host(&url) && url.path() == "/feeds/videos.xml")
}

/// The cookie-free embed address of a video id.
pub fn nocookie_embed(video_id: &str) -> Option<String> {
    parse::is_video_id(video_id).then(|| format!("https://www.youtube-nocookie.com/embed/{video_id}"))
}

/// The video id of a `youtube.com/watch?v=ID` or `youtu.be/ID` address.
fn video_id_of(link: &str) -> Option<String> {
    let url = Url::parse(link).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    let id = if matches!(host.as_str(), "youtube.com" | "www.youtube.com" | "m.youtube.com") && url.path() == "/watch" {
        url.query_pairs().find(|(key, _)| key == "v").map(|(_, value)| value.into_owned())?
    } else if host == "youtu.be" {
        url.path().trim_start_matches('/').to_owned()
    } else {
        return None;
    };
    parse::is_video_id(&id).then_some(id)
}

/// Where opening an item goes: for a YouTube feed, the video on the cookie-free host; otherwise the
/// item's own link. `None` when the item has no usable link.
pub fn open_target(feed_url: &str, item: &store::Item) -> Option<String> {
    if is_youtube_feed(feed_url) {
        let id = if item.video_id.is_empty() { video_id_of(&item.link) } else { Some(item.video_id.clone()) };
        if let Some(embed) = id.and_then(|id| nocookie_embed(&id)) {
            return Some(embed);
        }
    }
    (!item.link.is_empty()).then(|| item.link.clone())
}

/// The address a podcast episode plays from: its enclosure, when it is audio or video.
pub fn play_target(item: &store::Item) -> Option<String> {
    let enclosure = item.enclosure.as_ref()?;
    let mime = enclosure.mime.to_ascii_lowercase();
    let looks_media = mime.starts_with("audio/") || mime.starts_with("video/") || mime.is_empty() && {
        let path = Url::parse(&enclosure.url).ok()?.path().to_ascii_lowercase();
        [".mp3", ".m4a", ".aac", ".ogg", ".oga", ".opus", ".wav", ".mp4", ".webm", ".m4v"].iter().any(|ext| path.ends_with(ext))
    };
    looks_media.then(|| enclosure.url.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use parse::Enclosure;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn youtube_channels_and_playlists_have_feed_addresses() {
        let channel = "UC1234567890abcdefghijkl";
        assert_eq!(
            youtube_feed_address(&url(&format!("https://www.youtube.com/channel/{channel}/videos"))),
            None,
            "only the channel address itself is rewritten"
        );
        assert_eq!(
            youtube_feed_address(&url(&format!("https://www.youtube.com/channel/{channel}"))).unwrap().as_str(),
            format!("https://www.youtube.com/feeds/videos.xml?channel_id={channel}")
        );
        assert_eq!(
            youtube_feed_address(&url("https://m.youtube.com/playlist?list=PLabcdefghijklmnop")).unwrap().as_str(),
            "https://www.youtube.com/feeds/videos.xml?playlist_id=PLabcdefghijklmnop"
        );
        for other in [
            "https://www.youtube.com/@handle", "https://www.youtube.com/channel/NOTUC", "https://www.youtube.com/playlist?list=x",
            "https://evil.example/channel/UC1234567890abcdefghijkl", "https://www.youtube.com.evil.example/playlist?list=PLabcdefghijklmnop",
            "https://www.youtube.com/channel/UC1234567890abcdefghij&x",
        ] {
            assert_eq!(youtube_feed_address(&url(other)), None, "{other}");
        }
    }

    #[test]
    fn a_youtube_item_opens_on_the_cookie_free_host() {
        let feed = "https://www.youtube.com/feeds/videos.xml?channel_id=UC1234567890abcdefghijkl";
        let by_id = store::Item { video_id: "dQw4w9WgXcQ".into(), link: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(), ..store::Item::default() };
        assert_eq!(open_target(feed, &by_id).as_deref(), Some("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ"));
        let by_link = store::Item { link: "https://youtu.be/dQw4w9WgXcQ".into(), ..store::Item::default() };
        assert_eq!(open_target(feed, &by_link).as_deref(), Some("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ"));
        let other = store::Item { link: "https://blog.example/post".into(), video_id: "dQw4w9WgXcQ".into(), ..store::Item::default() };
        assert_eq!(open_target("https://blog.example/feed.xml", &other).as_deref(), Some("https://blog.example/post"), "only YouTube feeds are rewritten");
        assert_eq!(open_target(feed, &store::Item::default()), None);
        assert_eq!(nocookie_embed("not an id"), None);
        assert!(is_youtube_feed(feed) && !is_youtube_feed("https://blog.example/feeds/videos.xml"));
    }

    #[test]
    fn a_podcast_episode_plays_only_from_a_media_enclosure() {
        let mut item = store::Item { enclosure: Some(Enclosure { url: "https://cdn.example/a.mp3".into(), mime: "audio/mpeg".into() }), ..store::Item::default() };
        assert_eq!(play_target(&item).as_deref(), Some("https://cdn.example/a.mp3"));
        item.enclosure = Some(Enclosure { url: "https://cdn.example/a.MP3".into(), mime: String::new() });
        assert!(play_target(&item).is_some(), "no type: the file name decides");
        item.enclosure = Some(Enclosure { url: "https://cdn.example/a.exe".into(), mime: "application/octet-stream".into() });
        assert_eq!(play_target(&item), None);
        item.enclosure = Some(Enclosure { url: "https://cdn.example/a.exe".into(), mime: String::new() });
        assert_eq!(play_target(&item), None);
        item.enclosure = None;
        assert_eq!(play_target(&item), None);
    }

    #[test]
    fn a_fetched_body_is_a_feed_a_page_with_feeds_or_neither() {
        let page = url("https://blog.example/");
        let feed = br#"<rss version="2.0"><channel><title>B</title><item><title>x</title></item></channel></rss>"#;
        assert!(matches!(classify(&page, feed), Ok(Outcome::Feed { .. })));
        let html = br#"<html><head><link rel="alternate" type="application/atom+xml" href="/atom.xml"></head><body>hi</body></html>"#;
        assert_eq!(
            classify(&page, html),
            Ok(Outcome::Candidates(vec![FeedLink { url: "https://blog.example/atom.xml".into(), title: String::new() }]))
        );
        assert_eq!(classify(&page, b"<html><body>no feeds</body></html>").unwrap_err(), "No feed found on that page.");
        assert!(classify(&page, b"<!DOCTYPE rss [<!ENTITY a 'b'>]><rss/>").unwrap_err().contains("DOCTYPE"));
        assert!(classify(&page, b"just text").is_err());
    }

    #[test]
    fn the_refresh_choices_include_the_default_and_start_with_manual() {
        assert_eq!(REFRESH_CHOICES[0], 0);
        assert!(REFRESH_CHOICES.contains(&DEFAULT_REFRESH_MINUTES));
        assert!(REFRESH_CHOICES.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
