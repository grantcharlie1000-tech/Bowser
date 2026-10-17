//! The Feeds panel's wire (`ui/src/panels/feeds.ts`): the `feeds` snapshot and the `{t:"feed",a,...}`
//! commands, validated here so the panel host only acts on a well-formed command.
//!
//! A command never carries an address to open: it names a subscription and an item by id and hash, and
//! the host finds the item in the stored feed again, so a list that went stale in the page can never
//! open or mark the wrong thing, and the page can never ask Bowser to open an address it made up.
//! The only address a command carries is the one the user typed or picked to preview or subscribe to,
//! and that goes through `feeds::fetch::parse_url` again.
//!
//! - `select` `{id}`: the subscription whose items are listed (`0`: every subscription).
//! - `preview` `{text}` / `closepreview`: look at an address (a feed, or a page that offers feeds).
//! - `subscribe` `{text}`: subscribe to an address (a feed address; a page's first feed).
//! - `unsubscribe` `{id}`.
//! - `refresh` `{id?}`: fetch one subscription, or all.
//! - `read` `{id,parent,on}`: one item read or unread (`parent` is the item's hash).
//! - `readall` `{id?}`.
//! - `open` `{id,parent}`: the item in a new tab (marks it read). `play` `{id,parent}`: its episode.
//! - `import`: choose an OPML file.

use super::super::{json::capped, protocol::{self, PanelFields}};
use crate::feeds::{
    self,
    discover::FeedLink,
    parse::ParsedItem,
    store::{Feed, Item, Store},
};

/// Longest address a command may carry, in characters (the fetch layer caps it in bytes too).
pub const MAX_ADDRESS_CHARS: usize = 2048;
/// Items listed for one view.
pub const MAX_VIEW_ITEMS: usize = 120;
/// Items listed in a preview.
pub const MAX_PREVIEW_ITEMS: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Select(u32),
    Preview(String),
    ClosePreview,
    Subscribe(String),
    Unsubscribe(u32),
    Refresh(Option<u32>),
    Read { feed: u32, item: u32, read: bool },
    ReadAll(Option<u32>),
    Open { feed: u32, item: u32 },
    Play { feed: u32, item: u32 },
    Import,
}

fn id32(value: Option<u64>) -> Option<u32> {
    value.and_then(|value| u32::try_from(value).ok())
}

fn address(f: &PanelFields) -> Option<String> {
    let text = f.text.as_deref()?.trim();
    (!text.is_empty() && text.chars().count() <= MAX_ADDRESS_CHARS).then(|| text.to_owned())
}

/// Maps a `feed` command to an [`Action`]: each with exactly its own fields.
pub fn map_command(f: &PanelFields) -> Option<Action> {
    let only = |id: bool, parent: bool, on: bool, text: bool| {
        f.id.is_some() == id && f.parent.is_some() == parent && f.on.is_some() == on && f.text.is_some() == text && f.before.is_none() && f.n.is_none() && f.url.is_none()
    };
    match f.a.as_str() {
        "select" if only(true, false, false, false) => Some(Action::Select(id32(f.id)?)),
        "preview" if only(false, false, false, true) => Some(Action::Preview(address(f)?)),
        "closepreview" if only(false, false, false, false) => Some(Action::ClosePreview),
        "subscribe" if only(false, false, false, true) => Some(Action::Subscribe(address(f)?)),
        "unsubscribe" if only(true, false, false, false) => Some(Action::Unsubscribe(id32(f.id).filter(|id| *id > 0)?)),
        "refresh" if only(false, false, false, false) => Some(Action::Refresh(None)),
        "refresh" if only(true, false, false, false) => Some(Action::Refresh(Some(id32(f.id).filter(|id| *id > 0)?))),
        "read" if only(true, true, true, false) => {
            Some(Action::Read { feed: id32(f.id).filter(|id| *id > 0)?, item: id32(f.parent).filter(|hash| *hash > 0)?, read: f.on? })
        }
        "readall" if only(false, false, false, false) => Some(Action::ReadAll(None)),
        "readall" if only(true, false, false, false) => Some(Action::ReadAll(Some(id32(f.id).filter(|id| *id > 0)?))),
        "open" if only(true, true, false, false) => Some(Action::Open { feed: id32(f.id).filter(|id| *id > 0)?, item: id32(f.parent).filter(|hash| *hash > 0)? }),
        "play" if only(true, true, false, false) => Some(Action::Play { feed: id32(f.id).filter(|id| *id > 0)?, item: id32(f.parent).filter(|hash| *hash > 0)? }),
        "import" if only(false, false, false, false) => Some(Action::Import),
        _ => None,
    }
}

/// An address the user is looking at before subscribing: a feed (its items) or a page (its feeds).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    /// The address that was looked at (after redirects, for a feed).
    pub url: String,
    pub title: String,
    pub site: String,
    pub items: Vec<ParsedItem>,
    /// The feeds a page offers (a page was looked at, not a feed).
    pub candidates: Vec<FeedLink>,
}

/// Everything the snapshot reports.
pub struct View<'a> {
    /// Guest profiles keep nothing: the panel says so and offers no subscribing.
    pub allowed: bool,
    pub busy: bool,
    pub refresh_minutes: u32,
    pub selected: u32,
    pub store: &'a Store,
    pub preview: Option<&'a Preview>,
    /// The feeds the page in front offers (the address bar's feed button).
    pub page: &'a [FeedLink],
    pub status: &'a str,
    pub error: &'a str,
}

fn kind_of(feed: &Feed) -> &'static str {
    if feeds::is_youtube_feed(&feed.url) {
        "video"
    } else if feed.items.iter().any(|item| feeds::play_target(item).is_some()) {
        "podcast"
    } else {
        "article"
    }
}

fn media_of(item: &Item) -> &'static str {
    match item.enclosure.as_ref() {
        Some(enclosure) if feeds::play_target(item).is_some() => {
            if enclosure.mime.to_ascii_lowercase().starts_with("video/") { "video" } else { "audio" }
        }
        _ => "",
    }
}

fn write_item(out: &mut super::super::json::Object<'_>, feed: &Feed, item: &Item, source: bool) {
    out.uint("feed", u64::from(feed.id));
    out.uint("h", u64::from(item.hash));
    out.string("title", capped(&item.title, 200));
    out.string("summary", capped(&item.summary, 400));
    out.uint("published", item.published);
    out.bool("read", item.read);
    out.string("media", media_of(item));
    out.string("duration", capped(&item.duration, 16));
    out.bool("watch", feeds::is_youtube_feed(&feed.url));
    if source {
        out.string("source", capped(&feed.display_title(), 120));
    }
}

/// The items of the selected view, newest first: one subscription's, or everyone's.
fn view_items(store: &Store, selected: u32) -> Vec<(&Feed, &Item)> {
    let mut items: Vec<(&Feed, &Item)> = match store.feed(selected) {
        Some(feed) => feed.items.iter().map(|item| (feed, item)).collect(),
        None => store.feeds.iter().flat_map(|feed| feed.items.iter().map(move |item| (feed, item))).collect(),
    };
    items.sort_by_key(|a| std::cmp::Reverse(a.1.published));
    items.truncate(MAX_VIEW_ITEMS);
    items
}

/// `{"v":1,"t":"snap","p":"feeds","d":{allowed,busy,refresh,selected,unread,feeds,items,preview,page,status,error}}`.
pub fn snapshot_json(view: &View<'_>) -> String {
    let selected = if view.store.feed(view.selected).is_some() { view.selected } else { 0 };
    protocol::snap_json("feeds", |out| {
        out.bool("allowed", view.allowed);
        out.bool("busy", view.busy);
        out.uint("refresh", u64::from(view.refresh_minutes));
        out.uint("selected", u64::from(selected));
        out.uint("unread", view.store.unread_total() as u64);
        out.array("feeds", |list| {
            for feed in &view.store.feeds {
                list.object(|o| {
                    o.uint("id", u64::from(feed.id));
                    o.string("title", capped(&feed.display_title(), 120));
                    o.uint("unread", feed.unread() as u64);
                    o.uint("total", feed.items.len() as u64);
                    o.uint("fetched", feed.fetched);
                    o.string("error", capped(&feed.error, 200));
                    o.string("kind", kind_of(feed));
                });
            }
        });
        out.array("items", |list| {
            for (feed, item) in view_items(view.store, selected) {
                list.object(|o| write_item(o, feed, item, selected == 0));
            }
        });
        match view.preview {
            None => out.null("preview"),
            Some(preview) => out.object("preview", |o| {
                o.string("url", capped(&preview.url, 2048));
                o.string("title", capped(&preview.title, 120));
                o.string("site", capped(&preview.site, 2048));
                o.bool("subscribed", view.store.by_url(&preview.url).is_some());
                o.array("items", |list| {
                    for item in preview.items.iter().take(MAX_PREVIEW_ITEMS) {
                        list.object(|i| {
                            i.string("title", capped(&item.title, 200));
                            i.string("summary", capped(&item.summary, 400));
                            i.uint("published", item.published);
                        });
                    }
                });
                o.array("candidates", |list| {
                    for link in &preview.candidates {
                        list.object(|i| {
                            i.string("url", capped(&link.url, 2048));
                            i.string("title", capped(&link.title, 100));
                        });
                    }
                });
            }),
        }
        out.array("page", |list| {
            for link in view.page {
                list.object(|o| {
                    o.string("url", capped(&link.url, 2048));
                    o.string("title", capped(&link.title, 100));
                    o.bool("subscribed", view.store.by_url(&link.url).is_some());
                });
            }
        });
        out.string("status", capped(view.status, 200));
        out.string("error", capped(view.error, 240));
    })
    .expect("feeds is a panel token")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feeds::parse::{Enclosure, ParsedFeed};

    fn fields(a: &str) -> PanelFields {
        PanelFields { a: a.into(), ..PanelFields::default() }
    }

    fn with(a: &str, id: Option<u64>, parent: Option<u64>, on: Option<bool>, text: Option<&str>) -> PanelFields {
        PanelFields { a: a.into(), id, parent, on, text: text.map(str::to_owned), ..PanelFields::default() }
    }

    #[test]
    fn commands_carry_exactly_their_own_fields() {
        assert_eq!(map_command(&with("select", Some(0), None, None, None)), Some(Action::Select(0)));
        assert_eq!(map_command(&with("select", Some(7), None, None, None)), Some(Action::Select(7)));
        assert_eq!(map_command(&with("preview", None, None, None, Some("  https://a.example/feed  "))), Some(Action::Preview("https://a.example/feed".into())));
        assert_eq!(map_command(&with("subscribe", None, None, None, Some("https://a.example/feed"))), Some(Action::Subscribe("https://a.example/feed".into())));
        assert_eq!(map_command(&fields("closepreview")), Some(Action::ClosePreview));
        assert_eq!(map_command(&with("unsubscribe", Some(3), None, None, None)), Some(Action::Unsubscribe(3)));
        assert_eq!(map_command(&fields("refresh")), Some(Action::Refresh(None)));
        assert_eq!(map_command(&with("refresh", Some(2), None, None, None)), Some(Action::Refresh(Some(2))));
        assert_eq!(map_command(&with("read", Some(2), Some(99), Some(true), None)), Some(Action::Read { feed: 2, item: 99, read: true }));
        assert_eq!(map_command(&with("read", Some(2), Some(99), Some(false), None)), Some(Action::Read { feed: 2, item: 99, read: false }));
        assert_eq!(map_command(&fields("readall")), Some(Action::ReadAll(None)));
        assert_eq!(map_command(&with("readall", Some(4), None, None, None)), Some(Action::ReadAll(Some(4))));
        assert_eq!(map_command(&with("open", Some(2), Some(99), None, None)), Some(Action::Open { feed: 2, item: 99 }));
        assert_eq!(map_command(&with("play", Some(2), Some(99), None, None)), Some(Action::Play { feed: 2, item: 99 }));
        assert_eq!(map_command(&fields("import")), Some(Action::Import));
    }

    #[test]
    fn wrong_extra_or_missing_fields_are_refused() {
        let long = "x".repeat(MAX_ADDRESS_CHARS + 1);
        for bad in [
            with("select", None, None, None, None),
            with("select", Some(1), Some(2), None, None),
            with("select", Some(u64::from(u32::MAX) + 1), None, None, None),
            with("preview", None, None, None, None),
            with("preview", None, None, None, Some("   ")),
            with("preview", None, None, None, Some(&long)),
            with("preview", Some(1), None, None, Some("https://a.example/")),
            with("subscribe", None, None, Some(true), Some("https://a.example/")),
            with("unsubscribe", Some(0), None, None, None),
            with("unsubscribe", None, None, None, None),
            with("refresh", Some(0), None, None, None),
            with("refresh", None, None, None, Some("https://a.example/")),
            with("read", Some(1), Some(2), None, None),
            with("read", Some(1), Some(0), Some(true), None),
            with("read", Some(0), Some(2), Some(true), None),
            with("read", Some(1), None, Some(true), None),
            with("open", Some(1), None, None, None),
            with("open", Some(1), Some(2), Some(true), None),
            with("play", Some(1), Some(2), None, Some("https://evil.example/")),
            with("readall", None, None, Some(true), None),
            with("import", None, None, None, Some("c:\\x.opml")),
            fields("bogus"),
            fields(""),
        ] {
            assert_eq!(map_command(&bad), None, "{bad:?}");
        }
        let mut with_url = with("subscribe", None, None, None, Some("https://a.example/"));
        with_url.url = Some("https://b.example/".into());
        assert_eq!(map_command(&with_url), None);
        let mut with_n = fields("closepreview");
        with_n.n = Some(1);
        assert_eq!(map_command(&with_n), None);
        let mut with_before = fields("refresh");
        with_before.before = Some(1);
        assert_eq!(map_command(&with_before), None);
    }

    fn sample_store() -> Store {
        let mut store = Store::default();
        let a = store.add("https://a.example/feed.xml", "").unwrap();
        let b = store.add("https://www.youtube.com/feeds/videos.xml?channel_id=UC1234567890abcdefghijkl", "").unwrap();
        let c = store.add("https://pod.example/rss", "").unwrap();
        let item = |guid: &str, title: &str, published: u64| ParsedItem { guid: guid.into(), title: title.into(), link: format!("https://a.example/{guid}"), summary: "About <it>".into(), published, ..ParsedItem::default() };
        store.merge(a, &ParsedFeed { title: "A \"News\"".into(), site: "https://a.example/".into(), items: vec![item("1", "One", 100), item("2", "Two", 300)] }, 5000);
        store.merge(b, &ParsedFeed { title: "Channel".into(), site: String::new(), items: vec![ParsedItem { video_id: "dQw4w9WgXcQ".into(), ..item("3", "Video", 200) }] }, 5000);
        store.merge(
            c,
            &ParsedFeed {
                title: "Pod".into(),
                site: String::new(),
                items: vec![ParsedItem { enclosure: Some(Enclosure { url: "https://cdn.example/e.mp3".into(), mime: "audio/mpeg".into() }), duration: "45:12".into(), ..item("4", "Episode", 50) }],
            },
            0,
        );
        store.set_error(c, "The server answered 404, not a feed.");
        let hash = store.feed(a).unwrap().items[0].hash;
        store.mark(a, hash, true);
        store
    }

    fn view<'a>(store: &'a Store, selected: u32, preview: Option<&'a Preview>, page: &'a [FeedLink]) -> View<'a> {
        View { allowed: true, busy: false, refresh_minutes: 60, selected, store, preview, page, status: "", error: "" }
    }

    #[test]
    fn the_snapshot_lists_subscriptions_and_the_selected_items() {
        let store = sample_store();
        let all = snapshot_json(&view(&store, 0, None, &[]));
        assert!(all.starts_with(r#"{"v":1,"t":"snap","p":"feeds","d":{"allowed":true,"busy":false,"refresh":60,"selected":0,"unread":3,"feeds":["#), "{all}");
        assert!(all.contains(r#""title":"A \"News\"","unread":1,"total":2,"fetched":5000,"error":"","kind":"article""#), "{all}");
        assert!(all.contains(r#""title":"Channel","unread":1,"total":1,"fetched":5000,"error":"","kind":"video""#), "{all}");
        assert!(all.contains(r#""title":"Pod","unread":1,"total":1,"fetched":0,"error":"The server answered 404, not a feed.","kind":"podcast""#), "{all}");
        // Every subscription's items, newest first, each with its source.
        let titles: Vec<usize> = ["\"Two\"", "\"Video\"", "\"One\"", "\"Episode\""].iter().map(|t| all.find(&format!("\"title\":{t}")).expect(t)).collect();
        assert!(titles.windows(2).all(|pair| pair[0] < pair[1]), "newest first: {all}");
        assert!(all.contains(r#""source":"A \"News\"""#));
        assert!(all.contains(r#""media":"audio","duration":"45:12","watch":false"#) && all.contains(r#""watch":true"#), "{all}");
        // One subscription: its items only, no source label.
        let one = snapshot_json(&view(&store, 1, None, &[]));
        assert!(one.contains(r#""selected":1"#) && one.contains("\"Two\"") && !one.contains("\"Video\"") && !one.contains("\"source\""), "{one}");
        // An id that is not a subscription falls back to all.
        assert!(snapshot_json(&view(&store, 77, None, &[])).contains(r#""selected":0"#));
    }

    #[test]
    fn a_preview_and_the_pages_feeds_are_reported_with_their_subscribed_state() {
        let store = sample_store();
        let preview = Preview {
            url: "https://a.example/feed.xml".into(),
            title: "A".into(),
            site: "https://a.example/".into(),
            items: (0..30).map(|n| ParsedItem { title: format!("P{n}"), ..ParsedItem::default() }).collect(),
            candidates: vec![FeedLink { url: "https://c.example/atom.xml".into(), title: "C".into() }],
        };
        let page = [FeedLink { url: "https://pod.example/rss".into(), title: "Pod".into() }, FeedLink { url: "https://new.example/rss".into(), title: "New".into() }];
        let json = snapshot_json(&view(&store, 0, Some(&preview), &page));
        assert!(json.contains(r#""preview":{"url":"https://a.example/feed.xml","title":"A","site":"https://a.example/","subscribed":true"#), "{json}");
        let shown = &json[json.find("\"preview\"").unwrap()..json.find("\"page\"").unwrap()];
        assert_eq!(shown.matches("\"title\":\"P").count(), MAX_PREVIEW_ITEMS, "a preview lists at most twenty items");
        assert!(json.contains(r#""candidates":[{"url":"https://c.example/atom.xml","title":"C"}]"#), "{json}");
        assert!(json.contains(r#""page":[{"url":"https://pod.example/rss","title":"Pod","subscribed":true},{"url":"https://new.example/rss","title":"New","subscribed":false}]"#), "{json}");
        assert!(snapshot_json(&view(&store, 0, None, &[])).contains(r#""preview":null"#));
    }

    #[test]
    fn feed_text_is_escaped_in_the_snapshot_and_never_reaches_it_as_markup() {
        let mut store = Store::default();
        let id = store.add("https://a.example/f", "").unwrap();
        store.merge(id, &ParsedFeed { title: "<img src=x onerror=alert(1)>".into(), site: String::new(), items: vec![ParsedItem { title: "</script><script>alert(1)</script>".into(), ..ParsedItem::default() }] }, 1);
        let json = snapshot_json(&view(&store, 0, None, &[]));
        assert!(!json.contains('<'), "every < is escaped: {json}");
        assert!(json.contains(r#"\u003c/script>"#));
    }

    #[test]
    fn a_long_list_is_cut_at_the_view_limit() {
        let mut store = Store::default();
        for n in 0..5 {
            let id = store.add(&format!("https://f{n}.example/x"), "").unwrap();
            let items = (0..50).map(|i| ParsedItem { guid: format!("{n}-{i}"), title: format!("t{n}-{i}"), published: 1 + i, ..ParsedItem::default() }).collect();
            store.merge(id, &ParsedFeed { items, ..ParsedFeed::default() }, 1);
        }
        let json = snapshot_json(&view(&store, 0, None, &[]));
        assert_eq!(json.matches("\"h\":").count(), MAX_VIEW_ITEMS);
    }

    #[test]
    fn feeds_messages_match_committed_fixture() {
        let store = sample_store();
        let preview = Preview {
            url: "https://new.example/feed".into(),
            title: "New feed".into(),
            site: "https://new.example/".into(),
            items: vec![ParsedItem { title: "First".into(), summary: "Summary".into(), published: 100, ..ParsedItem::default() }],
            candidates: Vec::new(),
        };
        let page = [FeedLink { url: "https://new.example/feed".into(), title: "New feed".into() }];
        let nothing = Store::default();
        let guest = View { allowed: false, ..view(&nothing, 0, None, &[]) };
        let busy = View { busy: true, status: "Refreshing 2 feeds…", ..view(&store, 0, None, &[]) };
        let content = format!(
            "{{\"snap\":{},\"preview\":{},\"busy\":{},\"empty\":{},\"guest\":{}}}\n",
            snapshot_json(&view(&store, 0, None, &[])),
            snapshot_json(&view(&store, 0, Some(&preview), &page)),
            snapshot_json(&busy),
            snapshot_json(&view(&Store::default(), 0, None, &[])),
            snapshot_json(&guest),
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/feeds-messages.json");
        if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some_and(|value| value == "1") {
            std::fs::create_dir_all(path.parent().expect("fixtures directory")).expect("create fixtures directory");
            std::fs::write(&path, content).expect("write feeds messages fixture");
        } else {
            assert_eq!(std::fs::read_to_string(path).expect("committed feeds messages fixture"), content);
        }
    }
}
