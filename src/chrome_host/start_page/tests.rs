use super::*;
use crate::start_page::{GroupReopen, SpeedDialConfig, Widget};
use std::collections::HashMap;

fn png(len: usize) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 16, 0, 0, 0, 16];
    bytes.resize(len, 0);
    bytes
}

struct World {
    store: BookmarkStore,
    config: SpeedDialConfig,
    settings: StartSettings,
    widgets: WidgetSet,
    top: Vec<TopSite>,
    privacy: PrivacySummary,
}

fn world() -> World {
    let mut store = BookmarkStore::new();
    let work = store.add_folder(None, "Work").unwrap();
    let play = store.add_folder(None, "Play <b>").unwrap();
    store.add_bookmark(Some(work), "Docs \"quoted\"", "https://docs.rs/").unwrap();
    store.add_bookmark(Some(work), "Rust", "https://www.rust-lang.org/learn").unwrap();
    store.add_folder(Some(work), "Reading").unwrap();
    store.add_bookmark(Some(play), "Chess", "https://chess.example/").unwrap();
    let config = SpeedDialConfig { groups: vec![work, play], last: None, thumbs: HashMap::new() };
    let mut widgets = WidgetSet::default();
    let marks = widgets.add(crate::start_page::WidgetKind::Bookmarks).unwrap();
    let note = widgets.add(crate::start_page::WidgetKind::Notes).unwrap();
    let page = widgets.add(crate::start_page::WidgetKind::Webpage).unwrap();
    {
        let w: &mut Widget = widgets.get_mut(marks).unwrap();
        w.folder = work;
        w.view = BookmarksView::Tiles;
    }
    widgets.get_mut(note).unwrap().text = "buy milk\nand <script>".to_owned();
    widgets.get_mut(page).unwrap().url = "https://widget.example/clock".to_owned();
    World {
        store,
        config,
        settings: StartSettings { widgets: true, ..StartSettings::default() },
        widgets,
        top: vec![TopSite { host: "docs.rs".to_owned(), title: "Docs.rs".to_owned(), url: "https://docs.rs/".to_owned() }],
        privacy: PrivacySummary { today: 7, total: 1234, top_host: "tracker.example".to_owned(), top_count: 99 },
    }
}

fn json_of(w: &World, view: &DialView, ack: Ack, favicon: impl Fn(&str) -> Option<Vec<u8>>) -> String {
    snapshot_json_with(
        &Snapshot { guest: false, settings: &w.settings, view, store: &w.store, config: &w.config, widgets: &w.widgets, top: &w.top, privacy: Some(&w.privacy), ack },
        favicon,
    )
}

#[test]
fn the_snapshot_carries_groups_dials_settings_widgets_and_stays_text() {
    let w = world();
    let view = crate::start_page::dial_view(&w.store, &w.config, GroupReopen::First, None);
    let json = json_of(&w, &view, Ack::None, |_| None);
    assert!(json.starts_with(r#"{"v":1,"t":"snap","p":"startpage","d":{"kind":"page","guest":false,"settings":{"columns":6,"thumbSize":3,"titles":"always","addButton":true,"dragReorder":true,"widgets":true,"#), "{json}");
    assert!(json.contains(r#""groups":[{"id":1,"title":"Work","n":3},{"id":2,"title":"Play \u003cb>","n":1}]"#), "{json}");
    assert!(json.contains(r#""group":1,"folder":1,"path":[{"id":1,"title":"Work"}]"#), "{json}");
    assert!(json.contains(r#"{"id":3,"title":"Docs \"quoted\"","kind":"link","url":"https://docs.rs/","host":"docs.rs","n":0,"thumb":false}"#), "{json}");
    assert!(json.contains(r#""kind":"folder","url":"","host":"","n":0,"thumb":false}"#), "{json}");
    assert!(json.contains(r#""kind":"topsites","sites":[{"host":"docs.rs","title":"Docs.rs","url":"https://docs.rs/"}]"#), "{json}");
    assert!(json.contains(r#""kind":"bookmarks","folder":1,"view":"tiles","links":["#), "{json}");
    assert!(json.contains(r#""kind":"privacy","today":7,"total":1234,"topHost":"tracker.example","topCount":99"#), "{json}");
    assert!(json.contains(r#""kind":"webpage","url":"https://widget.example/clock""#), "{json}");
    assert!(json.contains(r#""kind":"date""#), "{json}");
    // Page-controlled text is escaped, never raw markup.
    assert!(!json.contains('<'), "{json}");
    assert!(json.ends_with(r#""favs":{},"ack":"none","error":""}}"#), "{json}");
}

#[test]
fn widgets_switched_off_send_none_and_the_dial_settings_still_apply() {
    let mut w = world();
    w.settings = StartSettings { widgets: false, columns: 3, thumb_size: 5, titles: crate::start_page::TitleMode::Never, add_button: false, drag_reorder: false, ..StartSettings::default() };
    let view = crate::start_page::dial_view(&w.store, &w.config, GroupReopen::First, None);
    let json = json_of(&w, &view, Ack::None, |_| None);
    assert!(json.contains(r#""settings":{"columns":3,"thumbSize":5,"titles":"never","addButton":false,"dragReorder":false,"widgets":false,"#), "{json}");
    assert!(json.contains(r#""widgets":[]"#), "{json}");
}

#[test]
fn favicons_ride_along_for_dial_and_widget_hosts_within_a_budget() {
    let w = world();
    let view = crate::start_page::dial_view(&w.store, &w.config, GroupReopen::First, None);
    let icon = png(60);
    let json = json_of(&w, &view, Ack::None, |host| (host == "docs.rs" || host == "www.rust-lang.org").then(|| icon.clone()));
    assert!(json.contains(r#""favs":{"docs.rs":"data:image/png;base64,"#), "{json}");
    assert!(json.contains(r#""www.rust-lang.org":"data:image/png;base64,"#), "{json}");
    assert!(!json.contains("chess.example\":\"data"), "{json}");
    // A favicon that is not a sendable PNG is dropped, not sent.
    let bad = json_of(&w, &view, Ack::None, |_| Some(vec![1, 2, 3]));
    assert!(bad.contains(r#""favs":{}"#), "{bad}");
    assert!(json.len() < MAX_SNAPSHOT_BYTES);
}

#[test]
fn the_answer_to_a_failed_change_is_sent_once_with_its_words() {
    let w = world();
    let view = crate::start_page::dial_view(&w.store, &w.config, GroupReopen::First, None);
    let json = json_of(&w, &view, Ack::Failed("bookmark already exists".to_owned()), |_| None);
    assert!(json.ends_with(r#""ack":"failed","error":"bookmark already exists"}}"#), "{json}");
}

#[test]
fn thumbs_are_split_into_messages_under_the_limit_and_oversized_ones_are_skipped() {
    let uri = |n: usize| format!("data:image/jpeg;base64,{}", "A".repeat(n));
    let items: Vec<(u64, String)> = (1..=40).map(|id| (id, uri(30_000))).collect();
    let messages = thumbs_messages(&items);
    assert!(messages.len() >= 8, "{}", messages.len());
    for message in &messages {
        assert!(message.len() <= MAX_THUMBS_BYTES + 512, "{}", message.len());
        assert!(message.starts_with(r#"{"v":1,"t":"snap","p":"startpage","d":{"kind":"thumbs","thumbs":{"#), "{message}");
    }
    let sent: usize = messages.iter().map(|m| m.matches("data:image/jpeg").count()).sum();
    assert_eq!(sent, 40);
    assert!(thumbs_messages(&[(1, uri(MAX_THUMBS_BYTES))]).is_empty());
    assert!(thumbs_messages(&[]).is_empty());
}

fn f(a: &str) -> PanelFields {
    PanelFields { a: a.to_owned(), ..PanelFields::default() }
}

#[test]
fn every_command_is_accepted_with_exactly_its_own_fields() {
    let mut open = f("open");
    open.id = Some(5);
    assert_eq!(map_command(&open), Some(Action::Open { id: 5, new_tab: false }));
    open.on = Some(true);
    assert_eq!(map_command(&open), Some(Action::Open { id: 5, new_tab: true }));

    let mut openurl = f("openurl");
    openurl.url = Some("https://a.example/x".to_owned());
    assert_eq!(map_command(&openurl), Some(Action::OpenUrl { url: "https://a.example/x".to_owned(), new_tab: false }));

    let mut add = f("add");
    add.parent = Some(2);
    add.text = Some("  Site  ".to_owned());
    add.url = Some("https://site.example/".to_owned());
    assert_eq!(map_command(&add), Some(Action::Add { folder: 2, title: "Site".to_owned(), url: "https://site.example/".to_owned() }));
    add.text = None;
    assert_eq!(map_command(&add), Some(Action::Add { folder: 2, title: String::new(), url: "https://site.example/".to_owned() }));

    let mut edit = f("edit");
    edit.id = Some(3);
    edit.text = Some("New".to_owned());
    assert_eq!(map_command(&edit), Some(Action::Edit { id: 3, title: "New".to_owned(), url: String::new() }), "a folder edit has no address");
    edit.url = Some("https://new.example/".to_owned());
    assert_eq!(map_command(&edit), Some(Action::Edit { id: 3, title: "New".to_owned(), url: "https://new.example/".to_owned() }));

    let mut mv = f("move");
    mv.id = Some(4);
    assert_eq!(map_command(&mv), Some(Action::Move { id: 4, before: None }));
    mv.before = Some(9);
    assert_eq!(map_command(&mv), Some(Action::Move { id: 4, before: Some(9) }));

    let mut wadd = f("wadd");
    wadd.text = Some("webpage".to_owned());
    assert_eq!(map_command(&wadd), Some(Action::WidgetAdd(WidgetKind::Webpage)));
    let mut wnote = f("wnote");
    wnote.id = Some(2);
    wnote.text = Some("a\nb".to_owned());
    assert_eq!(map_command(&wnote), Some(Action::WidgetNote { id: 2, text: "a\nb".to_owned() }));
    let mut wurl = f("wurl");
    wurl.id = Some(2);
    wurl.url = Some(String::new());
    assert_eq!(map_command(&wurl), Some(Action::WidgetUrl { id: 2, url: String::new() }), "an empty address clears the widget");
    wurl.url = Some("https://x.example/".to_owned());
    assert_eq!(map_command(&wurl), Some(Action::WidgetUrl { id: 2, url: "https://x.example/".to_owned() }));

    for bare in ["settings", "refresh"] {
        assert!(map_command(&f(bare)).is_some(), "{bare}");
    }
    for with_id in ["show", "flaggroup", "removegroup", "remove", "thumb", "thumbclear", "wremove", "wsavenote"] {
        let mut c = f(with_id);
        c.id = Some(7);
        assert!(map_command(&c).is_some(), "{with_id}");
        c.text = Some("extra".to_owned());
        assert_eq!(map_command(&c), None, "{with_id} with an extra field");
    }
}

#[test]
fn bad_fields_unknown_actions_and_extras_are_refused() {
    let mut c = f("open");
    assert_eq!(map_command(&c), None, "no id");
    c.id = Some(1);
    c.url = Some("https://x.example/".to_owned());
    assert_eq!(map_command(&c), None, "an extra field");
    assert_eq!(map_command(&f("bogus")), None);
    assert_eq!(map_command(&f("Open")), None);
    let mut c = f("openurl");
    for url in ["javascript:alert(1)", "bowser://settings", "file:///c:/x", "https://u:p@x.example/", "not a url", ""] {
        c.url = Some(url.to_owned());
        assert_eq!(map_command(&c), None, "{url}");
    }
    let mut add = f("add");
    add.parent = Some(1);
    add.url = Some("data:text/html,x".to_owned());
    assert_eq!(map_command(&add), None);
    add.url = Some("https://ok.example/".to_owned());
    add.text = Some("x".repeat(MAX_TITLE_INPUT + 1));
    assert_eq!(map_command(&add), None, "title too long");
    add.text = Some("bad\u{1}title".to_owned());
    assert_eq!(map_command(&add), None, "control character");
    let mut note = f("wnote");
    note.id = Some(1);
    note.text = Some("n".repeat(MAX_TEXT_INPUT + 1));
    assert_eq!(map_command(&note), None);
    let mut wid = f("wremove");
    wid.id = Some(u64::from(u32::MAX) + 1);
    assert_eq!(map_command(&wid), None, "widget ids are 32 bit");
    let mut wadd = f("wadd");
    wadd.text = Some("iframe".to_owned());
    assert_eq!(map_command(&wadd), None, "only the known widget kinds");
    let mut wview = f("wview");
    wview.id = Some(1);
    wview.text = Some("grid".to_owned());
    assert_eq!(map_command(&wview), None);
    let mut group = f("addgroup");
    group.text = Some("   ".to_owned());
    assert_eq!(map_command(&group), None, "a blank group name");
    let mut on = f("settings");
    on.on = Some(true);
    assert_eq!(map_command(&on), None);
}

#[test]
fn start_page_messages_match_committed_fixture() {
    let mut w = world();
    let icon = png(40);
    let favs = |host: &str| (host == "docs.rs").then(|| icon.clone());
    let first = crate::start_page::dial_view(&w.store, &w.config, GroupReopen::First, None);
    let page = json_of(&w, &first, Ack::None, favs);
    let play = crate::start_page::dial_view(&w.store, &w.config, GroupReopen::First, Some(2));
    let other = json_of(&w, &play, Ack::None, |_| None);
    let failed = json_of(&w, &first, Ack::Failed("bookmark already exists".to_owned()), |_| None);
    w.settings.widgets = false;
    w.config.groups.clear();
    let empty_view = crate::start_page::dial_view(&w.store, &w.config, GroupReopen::First, None);
    let empty = json_of(&w, &empty_view, Ack::None, |_| None);
    let thumbs = thumbs_messages(&[(3, "data:image/jpeg;base64,/9j/4AAQ".to_owned()), (4, "data:image/jpeg;base64,/9j/4AAR".to_owned())]);
    assert_eq!(thumbs.len(), 1);
    let content = format!("{{\"page\":{page},\"other\":{other},\"failed\":{failed},\"empty\":{empty},\"thumbs\":{}}}\n", thumbs[0]);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/start-page-messages.json");
    if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some_and(|value| value == "1") {
        std::fs::create_dir_all(path.parent().expect("fixtures directory")).expect("create fixtures directory");
        std::fs::write(&path, content).expect("write start page fixture");
    } else {
        let committed = std::fs::read_to_string(path).expect("committed start page fixture");
        assert_eq!(committed.replace("\r\n", "\n"), content);
    }
}
