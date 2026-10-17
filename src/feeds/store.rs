//! The profile's subscriptions and their items, in `feeds.txt` (plain text, one record per line, tab
//! separated, so a person can back it up or read it). Nothing here touches the network.
//!
//! ```text
//! F <id> <url> <title> <site> <fetched> <error>
//! I <feed id> <hash> <read 0|1> <published> <title> <link> <summary> <enclosure url> <enclosure type> <video id> <duration>
//! ```
//!
//! Fields are escaped (`\\`, `\t`, `\n`, `\r`). A line that does not parse is skipped, so one bad line
//! never loses the rest, and a file that is too big or has too many records is cut at the limits.

use std::{fs, io, path::Path};

use super::parse::{Enclosure, ParsedFeed, ParsedItem, MAX_TITLE_CHARS};

/// Most subscriptions.
pub const MAX_FEEDS: usize = 100;
/// Most items kept per feed (the newest).
pub const STORED_ITEMS: usize = 50;
/// Largest `feeds.txt` read, in bytes.
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Item {
    /// A stable, nonzero 32-bit hash of the item's guid, link or title: how a refresh recognises it.
    pub hash: u32,
    pub title: String,
    pub link: String,
    pub summary: String,
    pub published: u64,
    pub read: bool,
    pub enclosure: Option<Enclosure>,
    pub video_id: String,
    pub duration: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Feed {
    pub id: u32,
    pub url: String,
    pub title: String,
    pub site: String,
    /// When the last successful fetch finished, seconds since 1970; `0` before the first.
    pub fetched: u64,
    /// Why the last fetch failed, or empty.
    pub error: String,
    pub items: Vec<Item>,
}

impl Feed {
    pub fn unread(&self) -> usize {
        self.items.iter().filter(|item| !item.read).count()
    }

    /// The name the panel shows: the feed's title, else its host.
    pub fn display_title(&self) -> String {
        if !self.title.is_empty() {
            return self.title.clone();
        }
        url::Url::parse(&self.url).ok().and_then(|url| url.host_str().map(|host| host.trim_start_matches("www.").to_owned())).unwrap_or_else(|| self.url.clone())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddError {
    /// Already subscribed; the id is the existing subscription.
    Duplicate(u32),
    TooMany,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Store {
    pub feeds: Vec<Feed>,
}

/// A stable nonzero 32-bit FNV-1a hash.
pub fn hash_key(text: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in text.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash.max(1)
}

/// What identifies an item across refreshes: its guid, else its link, else its title and date.
fn item_key(item: &ParsedItem) -> String {
    if !item.guid.is_empty() {
        item.guid.clone()
    } else if !item.link.is_empty() {
        item.link.clone()
    } else {
        format!("{}\u{1}{}", item.title, item.published)
    }
}

/// The address with its fragment dropped and a trailing slash on a bare host kept: how two subscriptions are
/// compared.
fn comparable(url: &str) -> String {
    url::Url::parse(url).map(|mut url| {
        url.set_fragment(None);
        url.to_string()
    }).unwrap_or_else(|_| url.to_owned())
}

impl Store {
    pub fn feed(&self, id: u32) -> Option<&Feed> {
        self.feeds.iter().find(|feed| feed.id == id)
    }

    pub fn feed_mut(&mut self, id: u32) -> Option<&mut Feed> {
        self.feeds.iter_mut().find(|feed| feed.id == id)
    }

    pub fn by_url(&self, url: &str) -> Option<&Feed> {
        let wanted = comparable(url);
        self.feeds.iter().find(|feed| comparable(&feed.url) == wanted)
    }

    pub fn unread_total(&self) -> usize {
        self.feeds.iter().map(Feed::unread).sum()
    }

    /// Subscribes to `url` (already checked by the caller) under `title`.
    pub fn add(&mut self, url: &str, title: &str) -> Result<u32, AddError> {
        if let Some(existing) = self.by_url(url) {
            return Err(AddError::Duplicate(existing.id));
        }
        if self.feeds.len() >= MAX_FEEDS {
            return Err(AddError::TooMany);
        }
        let id = self.feeds.iter().map(|feed| feed.id).max().unwrap_or(0) + 1;
        self.feeds.push(Feed { id, url: comparable(url), title: super::parse::plain_text(title, MAX_TITLE_CHARS), ..Feed::default() });
        Ok(id)
    }

    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.feeds.len();
        self.feeds.retain(|feed| feed.id != id);
        self.feeds.len() != before
    }

    /// Puts a fetched feed's items into subscription `id`: new items are unread, items seen before keep
    /// their read state, the newest [`STORED_ITEMS`] are kept, newest first.
    pub fn merge(&mut self, id: u32, parsed: &ParsedFeed, now: u64) {
        let Some(feed) = self.feed_mut(id) else { return };
        if !parsed.title.is_empty() {
            feed.title = parsed.title.clone();
        }
        if !parsed.site.is_empty() {
            feed.site = parsed.site.clone();
        }
        feed.fetched = now;
        feed.error.clear();
        let mut items: Vec<Item> = parsed
            .items
            .iter()
            .map(|parsed_item| {
                let hash = hash_key(&item_key(parsed_item));
                Item {
                    hash,
                    title: parsed_item.title.clone(),
                    link: parsed_item.link.clone(),
                    summary: parsed_item.summary.clone(),
                    published: parsed_item.published,
                    read: feed.items.iter().any(|old| old.hash == hash && old.read),
                    enclosure: parsed_item.enclosure.clone(),
                    video_id: parsed_item.video_id.clone(),
                    duration: parsed_item.duration.clone(),
                }
            })
            .collect();
        // Two items with one key are one item.
        let mut seen = std::collections::HashSet::new();
        items.retain(|item| seen.insert(item.hash));
        // Newest first; items without a date keep the feed's own order after the dated ones.
        items.sort_by(|a, b| b.published.cmp(&a.published));
        items.truncate(STORED_ITEMS);
        feed.items = items;
    }

    pub fn set_error(&mut self, id: u32, error: &str) {
        if let Some(feed) = self.feed_mut(id) {
            feed.error = error.chars().filter(|c| !c.is_control()).take(200).collect();
        }
    }

    /// Marks one item read or unread. False when the feed or item is not there.
    pub fn mark(&mut self, id: u32, hash: u32, read: bool) -> bool {
        let Some(item) = self.feed_mut(id).and_then(|feed| feed.items.iter_mut().find(|item| item.hash == hash)) else { return false };
        item.read = read;
        true
    }

    /// Marks every item of one feed (or of all feeds) read; returns how many changed.
    pub fn mark_all(&mut self, id: Option<u32>) -> usize {
        let mut changed = 0;
        for feed in self.feeds.iter_mut().filter(|feed| id.is_none_or(|id| feed.id == id)) {
            for item in feed.items.iter_mut().filter(|item| !item.read) {
                item.read = true;
                changed += 1;
            }
        }
        changed
    }

    /// The feeds a refresh on open should fetch: those never fetched, or fetched longer ago than
    /// `interval_minutes`. `0` (never refresh on open) lists none.
    pub fn stale(&self, now: u64, interval_minutes: u32) -> Vec<u32> {
        if interval_minutes == 0 {
            return Vec::new();
        }
        let limit = u64::from(interval_minutes) * 60;
        self.feeds.iter().filter(|feed| feed.fetched == 0 || now.saturating_sub(feed.fetched) >= limit).map(|feed| feed.id).collect()
    }

    // ---- the file ----

    pub fn load(path: &Path) -> Store {
        match fs::metadata(path) {
            Ok(meta) if meta.len() <= MAX_FILE_BYTES => fs::read(path).map(|bytes| Store::parse(&String::from_utf8_lossy(&bytes))).unwrap_or_default(),
            _ => Store::default(),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        crate::productivity::atomic_write(path, self.to_text().as_bytes())
    }

    pub fn parse(text: &str) -> Store {
        let mut store = Store::default();
        for line in text.lines() {
            let fields: Vec<String> = line.split('\t').map(unescape).collect();
            match fields.first().map(String::as_str) {
                Some("F") if fields.len() >= 7 => {
                    let Ok(id) = fields[1].parse::<u32>() else { continue };
                    if id == 0 || store.feeds.len() >= MAX_FEEDS || store.feed(id).is_some() {
                        continue;
                    }
                    store.feeds.push(Feed {
                        id,
                        url: fields[2].clone(),
                        title: fields[3].clone(),
                        site: fields[4].clone(),
                        fetched: fields[5].parse().unwrap_or(0),
                        error: fields[6].clone(),
                        items: Vec::new(),
                    });
                }
                Some("I") if fields.len() >= 12 => {
                    let (Ok(id), Ok(hash)) = (fields[1].parse::<u32>(), fields[2].parse::<u32>()) else { continue };
                    let Some(feed) = store.feed_mut(id) else { continue };
                    if hash == 0 || feed.items.len() >= STORED_ITEMS || feed.items.iter().any(|item| item.hash == hash) {
                        continue;
                    }
                    let enclosure = (!fields[8].is_empty()).then(|| Enclosure { url: fields[8].clone(), mime: fields[9].clone() });
                    feed.items.push(Item {
                        hash,
                        read: fields[3] == "1",
                        published: fields[4].parse().unwrap_or(0),
                        title: fields[5].clone(),
                        link: fields[6].clone(),
                        summary: fields[7].clone(),
                        enclosure,
                        video_id: fields[10].clone(),
                        duration: fields[11].clone(),
                    });
                }
                _ => {}
            }
        }
        store
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for feed in &self.feeds {
            let id = feed.id.to_string();
            let fetched = feed.fetched.to_string();
            push_record(&mut out, &["F", &id, &feed.url, &feed.title, &feed.site, &fetched, &feed.error]);
            for item in &feed.items {
                let hash = item.hash.to_string();
                let published = item.published.to_string();
                let (enc_url, enc_mime) = item.enclosure.as_ref().map_or(("", ""), |e| (e.url.as_str(), e.mime.as_str()));
                push_record(
                    &mut out,
                    &["I", &id, &hash, if item.read { "1" } else { "0" }, &published, &item.title, &item.link, &item.summary, enc_url, enc_mime, &item.video_id, &item.duration],
                );
            }
        }
        out
    }
}

fn push_record(out: &mut String, fields: &[&str]) {
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            out.push('\t');
        }
        for c in field.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                '\t' => out.push_str("\\t"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                other => out.push(other),
            }
        }
    }
    out.push('\n');
}

fn unescape(field: &str) -> String {
    if !field.contains('\\') {
        return field.to_owned();
    }
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(guid: &str, title: &str, published: u64) -> ParsedItem {
        ParsedItem { guid: guid.into(), title: title.into(), link: format!("https://a.example/{guid}"), published, ..ParsedItem::default() }
    }

    fn parsed(items: Vec<ParsedItem>) -> ParsedFeed {
        ParsedFeed { title: "Site".into(), site: "https://a.example/".into(), items }
    }

    #[test]
    fn subscribing_twice_or_past_the_cap_is_refused() {
        let mut store = Store::default();
        let id = store.add("https://a.example/feed.xml#frag", "A").unwrap();
        assert_eq!(store.add("https://a.example/feed.xml", "A again"), Err(AddError::Duplicate(id)));
        assert_eq!(store.feeds[0].url, "https://a.example/feed.xml", "the fragment is dropped");
        for n in 1..MAX_FEEDS {
            store.add(&format!("https://f{n}.example/x"), "").unwrap();
        }
        assert_eq!(store.add("https://one-more.example/x", ""), Err(AddError::TooMany));
        assert!(store.remove(id));
        assert!(store.add("https://one-more.example/x", "").is_ok());
    }

    #[test]
    fn a_refresh_keeps_read_state_adds_new_items_and_drops_old_ones() {
        let mut store = Store::default();
        let id = store.add("https://a.example/feed.xml", "").unwrap();
        store.merge(id, &parsed(vec![item("1", "One", 100), item("2", "Two", 200)]), 1000);
        assert_eq!(store.feed(id).unwrap().unread(), 2);
        assert_eq!(store.feed(id).unwrap().items[0].title, "Two", "newest first");
        let hash_two = store.feed(id).unwrap().items[0].hash;
        assert!(store.mark(id, hash_two, true));
        store.merge(id, &parsed(vec![item("2", "Two (edited)", 200), item("3", "Three", 300)]), 2000);
        let feed = store.feed(id).unwrap();
        assert_eq!(feed.items.iter().map(|i| (i.title.as_str(), i.read)).collect::<Vec<_>>(), [("Three", false), ("Two (edited)", true)]);
        assert_eq!((feed.title.as_str(), feed.site.as_str(), feed.fetched), ("Site", "https://a.example/", 2000));
        assert_eq!(store.unread_total(), 1);
        assert_eq!(store.mark_all(None), 1);
        assert_eq!(store.unread_total(), 0);
        assert!(!store.mark(id, 12345, true));
        assert!(!store.mark(99, hash_two, true));
    }

    #[test]
    fn duplicate_keys_collapse_and_only_the_newest_fifty_are_kept() {
        let mut store = Store::default();
        let id = store.add("https://a.example/f", "").unwrap();
        let mut items: Vec<ParsedItem> = (0..80).map(|n| item(&n.to_string(), &format!("T{n}"), n + 1)).collect();
        items.push(item("5", "dup of five", 6));
        store.merge(id, &parsed(items), 1);
        let feed = store.feed(id).unwrap();
        assert_eq!(feed.items.len(), STORED_ITEMS);
        assert_eq!(feed.items[0].title, "T79");
        assert_eq!(feed.items.iter().filter(|i| i.hash == hash_key("5")).count(), 0, "old items fall off the end");
    }

    #[test]
    fn refresh_on_open_names_only_the_stale_feeds() {
        let mut store = Store::default();
        let (a, b, c) = (store.add("https://a.example/", "").unwrap(), store.add("https://b.example/", "").unwrap(), store.add("https://c.example/", "").unwrap());
        store.merge(a, &parsed(vec![]), 10_000);
        store.merge(b, &parsed(vec![]), 10_000 - 3600);
        assert_eq!(store.stale(10_100, 60), vec![b, c], "an hour old and never fetched are stale");
        assert_eq!(store.stale(10_100, 120), vec![c], "only the never-fetched one with a longer interval");
        assert!(store.stale(10_100, 0).is_empty(), "0 means only when the user presses Refresh");
        assert_eq!(store.stale(1, 30), vec![c], "a clock before the fetch time does not underflow");
    }

    #[test]
    fn the_file_round_trips_with_awkward_text_and_skips_bad_lines() {
        let mut store = Store::default();
        let id = store.add("https://a.example/feed.xml", "Tab\tand\\slash\nnewline").unwrap();
        let mut item_with = item("1", "Line\r\nbreak\tand \\ backslash", 5);
        item_with.enclosure = Some(Enclosure { url: "https://cdn.example/a.mp3".into(), mime: "audio/mpeg".into() });
        item_with.video_id = "dQw4w9WgXcQ".into();
        item_with.duration = "45:12".into();
        store.merge(id, &parsed(vec![item_with, item("2", "Two", 3)]), 77);
        store.mark(id, store.feed(id).unwrap().items[1].hash, true);
        store.set_error(id, "boom\nbad");
        let text = store.to_text();
        assert_eq!(Store::parse(&text), store);
        let damaged = format!("garbage\nI\t99\t1\t0\t0\ta\tb\tc\td\te\tf\tg\nF\tx\ty\n{text}I\t{id}\t0\t0\t0\ta\tb\tc\td\te\tf\tg\n");
        assert_eq!(Store::parse(&damaged), store, "unknown lines, a feed id that is not a number and a zero hash are skipped");
    }

    #[test]
    fn the_file_loads_from_disk_and_a_missing_or_huge_file_is_an_empty_store() {
        let dir = std::env::temp_dir().join(format!("bowser-feeds-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("feeds.txt");
        assert_eq!(Store::load(&path), Store::default());
        let mut store = Store::default();
        store.add("https://a.example/f", "A").unwrap();
        store.save(&path).unwrap();
        assert_eq!(Store::load(&path), store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_display_title_falls_back_to_the_host() {
        let mut store = Store::default();
        let id = store.add("https://www.news.example/rss", "").unwrap();
        assert_eq!(store.feed(id).unwrap().display_title(), "news.example");
        store.feed_mut(id).unwrap().title = "News".into();
        assert_eq!(store.feed(id).unwrap().display_title(), "News");
    }
}
