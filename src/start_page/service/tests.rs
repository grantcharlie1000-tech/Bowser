//! Command, file, snapshot: each start page command run against a real folder, then the snapshot the
//! page would get. These are the tests that prove a row or a button changes what the page shows.

use super::*;
use crate::start_page::{GroupReopen, TitleMode};

fn temp(name: &str) -> Paths {
    let dir = std::env::temp_dir().join(format!("bowser-start-svc-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    Paths::in_dir(&dir)
}

fn run(paths: &Paths, requested: &mut Option<u64>, action: Action) -> Effect {
    apply(paths, requested, action).unwrap_or_else(|e| panic!("refused: {e}"))
}

fn json(paths: &Paths, settings: &StartSettings, requested: Option<u64>) -> String {
    page_with(Some(paths), false, settings, requested, Ack::None, |_| None).json
}

fn add_group(paths: &Paths, requested: &mut Option<u64>, title: &str) -> u64 {
    run(paths, requested, Action::AddGroup(title.to_owned()));
    requested.expect("a new group is shown")
}

fn add_site(paths: &Paths, requested: &mut Option<u64>, folder: u64, title: &str, url: &str) {
    run(paths, requested, Action::Add { folder, title: title.to_owned(), url: url.to_owned() });
}

fn ids(paths: &Paths, folder: u64) -> Vec<u64> {
    load(paths).store.children(Some(folder)).iter().map(|i| i.id).collect()
}

#[test]
fn a_new_profile_shows_an_empty_page_and_adding_a_group_and_site_fills_it_and_the_bookmarks() {
    let paths = temp("add");
    let settings = StartSettings::default();
    let empty = json(&paths, &settings, None);
    assert!(empty.contains(r#""groups":[],"group":0,"folder":0,"path":[],"dials":[]"#), "{empty}");

    let mut shown = None;
    let work = add_group(&paths, &mut shown, "Work");
    add_site(&paths, &mut shown, work, "Rust", "https://www.rust-lang.org/");
    add_site(&paths, &mut shown, work, "", "https://docs.rs/");
    let page = json(&paths, &settings, shown);
    assert!(page.contains(r#""groups":[{"id":1,"title":"Work","n":2}]"#), "{page}");
    assert!(page.contains(r#""title":"Rust","kind":"link","url":"https://www.rust-lang.org/""#), "{page}");
    assert!(page.contains(r#""title":"docs.rs","kind":"link","url":"https://docs.rs/""#), "an empty name is the host: {page}");
    // Speed dials are bookmarks: the Bookmarks panel's own file has them.
    let bookmarks = fs::read_to_string(paths.dir.join("bookmarks-v2.txt")).unwrap();
    assert!(bookmarks.contains("Rust") && bookmarks.contains("https://docs.rs/"), "{bookmarks}");
    assert!(fs::read_to_string(paths.dir.join(model::FILE_NAME)).unwrap().contains("group\t1"));
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn edit_remove_reorder_and_sub_folders_change_the_bookmarks_and_the_page() {
    let paths = temp("edit");
    let settings = StartSettings::default();
    let mut shown = None;
    let work = add_group(&paths, &mut shown, "Work");
    for (title, url) in [("A", "https://a.example/"), ("B", "https://b.example/"), ("C", "https://c.example/")] {
        add_site(&paths, &mut shown, work, title, url);
    }
    let order = ids(&paths, work);
    assert_eq!(order.len(), 3);

    run(&paths, &mut shown, Action::Move { id: order[2], before: Some(order[0]) });
    assert_eq!(ids(&paths, work), [order[2], order[0], order[1]], "drag to reorder is saved in the bookmark tree");
    run(&paths, &mut shown, Action::Move { id: order[2], before: None });
    assert_eq!(ids(&paths, work), [order[0], order[1], order[2]]);

    run(&paths, &mut shown, Action::Edit { id: order[1], title: "Bee".to_owned(), url: "https://bee.example/".to_owned() });
    assert!(json(&paths, &settings, shown).contains(r#""title":"Bee","kind":"link","url":"https://bee.example/""#));

    run(&paths, &mut shown, Action::AddFolder { folder: work, title: "Docs".to_owned() });
    let folder = *ids(&paths, work).last().unwrap();
    run(&paths, &mut shown, Action::MoveInto { id: order[0], folder });
    assert_eq!(ids(&paths, folder), [order[0]]);
    run(&paths, &mut shown, Action::Show(folder));
    assert_eq!(shown, Some(folder));
    let inside = json(&paths, &settings, shown);
    assert!(inside.contains(&format!(r#""folder":{folder},"path":[{{"id":{work},"title":"Work"}},{{"id":{folder},"title":"Docs"}}]"#)), "{inside}");

    run(&paths, &mut shown, Action::Remove(order[0]));
    assert!(ids(&paths, folder).is_empty());
    run(&paths, &mut shown, Action::Remove(folder));
    assert!(!ids(&paths, work).contains(&folder));
    assert!(apply(&paths, &mut shown, Action::Remove(work)).is_err(), "a group is removed with its own command");
    assert!(apply(&paths, &mut shown, Action::Add { folder: 999, title: "x".to_owned(), url: "https://x.example/".to_owned() }).is_err());
    assert!(apply(&paths, &mut shown, Action::Add { folder: work, title: "dup".to_owned(), url: "https://c.example/".to_owned() }).unwrap_err().contains("already in your bookmarks"));
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn the_page_reopens_on_the_last_or_first_group_after_a_restart() {
    let paths = temp("reopen");
    let mut shown = None;
    let one = add_group(&paths, &mut shown, "One");
    let two = add_group(&paths, &mut shown, "Two");
    let three = add_group(&paths, &mut shown, "Three");
    run(&paths, &mut shown, Action::Show(two));
    // A fresh window knows nothing but the files.
    let last = StartSettings { group_reopen: GroupReopen::Last, ..StartSettings::default() };
    let first = StartSettings { group_reopen: GroupReopen::First, ..StartSettings::default() };
    assert!(json(&paths, &last, None).contains(&format!(r#""group":{two},"folder":{two}"#)));
    assert!(json(&paths, &first, None).contains(&format!(r#""group":{one},"folder":{one}"#)));
    assert_ne!(one, three);
    // Opening a sub-folder does not change which group is remembered.
    run(&paths, &mut shown, Action::AddFolder { folder: two, title: "Sub".to_owned() });
    let sub = *ids(&paths, two).last().unwrap();
    run(&paths, &mut shown, Action::Show(sub));
    run(&paths, &mut shown, Action::Show(three));
    run(&paths, &mut shown, Action::Show(sub));
    assert!(json(&paths, &last, None).contains(&format!(r#""group":{three},"#)));
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn groups_can_be_flagged_from_bookmarks_renamed_reordered_and_removed_without_losing_bookmarks() {
    let paths = temp("groups");
    let mut shown = None;
    // An existing bookmark folder with a bookmark in it.
    let mut store = BookmarkStore::new();
    let reading = store.add_folder(None, "Reading").unwrap();
    store.add_bookmark(Some(reading), "Blog", "https://blog.example/").unwrap();
    store.save(&paths.dir).unwrap();
    let work = add_group(&paths, &mut shown, "Work");
    let settings = StartSettings::default();
    assert!(json(&paths, &settings, Some(work)).contains(r#""pickable":[{"id":1,"path":"Reading"}]"#));
    run(&paths, &mut shown, Action::FlagGroup(reading));
    let page = json(&paths, &settings, shown);
    assert!(page.contains(r#""title":"Blog","kind":"link""#), "the flagged folder's bookmarks are the dials: {page}");

    run(&paths, &mut shown, Action::RenameGroup { id: reading, title: "Later".to_owned() });
    assert_eq!(load(&paths).store.items().iter().find(|i| i.id == reading).unwrap().title, "Later");
    run(&paths, &mut shown, Action::MoveGroup { id: reading, before: Some(work) });
    assert_eq!(load(&paths).config.groups, [reading, work]);
    run(&paths, &mut shown, Action::RemoveGroup(reading));
    assert_eq!(load(&paths).config.groups, [work]);
    assert_eq!(ids(&paths, reading).len(), 1, "unflagging keeps the folder and its bookmark");
    assert!(apply(&paths, &mut shown, Action::RenameGroup { id: reading, title: "x".to_owned() }).is_err(), "no longer a group");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn display_settings_change_the_snapshot_the_page_draws_from() {
    let paths = temp("display");
    let mut shown = None;
    let work = add_group(&paths, &mut shown, "Work");
    add_site(&paths, &mut shown, work, "A", "https://a.example/");
    let tuned = StartSettings {
        columns: 4,
        thumb_size: 5,
        titles: TitleMode::Auto,
        add_button: false,
        drag_reorder: false,
        widgets: true,
        widget_columns: 3,
        widget_height: model::WidgetHeight::Tall,
        widget_bg: model::WidgetBackground::Custom,
        widget_color: "#336699".to_owned(),
        widget_drag: false,
        ..StartSettings::default()
    };
    let page = json(&paths, &tuned, shown);
    assert!(page.contains(r##""settings":{"columns":4,"thumbSize":5,"titles":"auto","addButton":false,"dragReorder":false,"widgets":true,"widgetColumns":3,"widgetHeight":"tall","widgetBg":"custom","widgetColor":"#336699","widgetDrag":false}"##), "{page}");
    let plain = json(&paths, &StartSettings::default(), shown);
    assert!(plain.contains(r#""columns":6,"thumbSize":3,"titles":"always","addButton":true,"dragReorder":true,"widgets":false,"#), "{plain}");
    assert!(plain.contains(r#""widgets":[]"#), "widgets off sends none: {plain}");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn a_chosen_image_is_saved_sent_after_the_page_and_cleared_with_its_command() {
    let paths = temp("thumb");
    let mut shown = None;
    let work = add_group(&paths, &mut shown, "Work");
    add_site(&paths, &mut shown, work, "A", "https://a.example/");
    let dial = ids(&paths, work)[0];
    assert_eq!(run(&paths, &mut shown, Action::Thumb(dial)), Effect::PickThumb(dial));
    assert!(apply(&paths, &mut shown, Action::Thumb(work)).is_err(), "a group has no image");
    let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4];
    set_thumbnail(&paths, dial, &jpeg).unwrap();
    assert!(set_thumbnail(&paths, dial, &[1, 2, 3, 4, 5]).is_err());
    let built = page_with(Some(&paths), false, &StartSettings::default(), shown, Ack::None, |_| None);
    assert!(built.json.contains(r#""n":0,"thumb":true}"#), "{}", built.json);
    assert_eq!(built.thumbs.len(), 1);
    assert!(built.thumbs[0].contains(&format!(r#""{dial}":"data:image/jpeg;base64,/9j/"#)), "{}", built.thumbs[0]);

    run(&paths, &mut shown, Action::ThumbClear(dial));
    let cleared = page_with(Some(&paths), false, &StartSettings::default(), shown, Ack::None, |_| None);
    assert!(cleared.json.contains(r#""thumb":false"#) && cleared.thumbs.is_empty());
    assert!(!model::thumb_path(&paths.dir, dial).exists());
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn a_bookmark_widget_shows_a_chosen_image_and_any_bookmark_can_have_one() {
    let paths = temp("widget-thumb");
    let mut shown = None;
    let work = add_group(&paths, &mut shown, "Work");
    add_site(&paths, &mut shown, work, "A", "https://a.example/");
    // A bookmark in another folder, listed by a bookmarks widget.
    let mut store = load(&paths).store;
    let other = store.add_folder(None, "Other").unwrap();
    let outside = store.add_bookmark(Some(other), "Outside", "https://outside.example/").unwrap();
    store.save(&paths.dir).unwrap();
    run(&paths, &mut shown, Action::WidgetAdd(WidgetKind::Bookmarks));
    let marks = load(&paths).widgets.items.iter().find(|w| w.kind == WidgetKind::Bookmarks).unwrap().id;
    run(&paths, &mut shown, Action::WidgetFolder { id: marks, folder: other });
    assert_eq!(run(&paths, &mut shown, Action::Thumb(outside)), Effect::PickThumb(outside), "a widget's bookmark can be given an image");
    assert!(apply(&paths, &mut shown, Action::Thumb(other)).is_err(), "a folder cannot");
    set_thumbnail(&paths, outside, &[0xFF, 0xD8, 0xFF, 0xE0, 9, 9, 9, 9]).unwrap();
    let on = StartSettings { widgets: true, ..StartSettings::default() };
    let built = page_with(Some(&paths), false, &on, shown, Ack::None, |_| None);
    assert!(built.json.contains(r#""title":"Outside","url":"https://outside.example/","host":"outside.example","thumb":true}"#), "{}", built.json);
    assert_eq!(built.thumbs.len(), 1);
    assert!(built.thumbs[0].contains(&format!(r#""{outside}":"data:image/jpeg;base64,"#)));
    // With the dashboard off nothing of it is sent.
    let off = page_with(Some(&paths), false, &StartSettings::default(), shown, Ack::None, |_| None);
    assert!(off.thumbs.is_empty());
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn widgets_are_added_rearranged_configured_and_their_data_comes_from_the_profile() {
    let paths = temp("widgets");
    let mut shown = None;
    let work = add_group(&paths, &mut shown, "Work");
    add_site(&paths, &mut shown, work, "A", "https://a.example/");
    let on = StartSettings { widgets: true, ..StartSettings::default() };
    // History for top sites, statistics for the privacy widget.
    fs::write(&paths.history, "100\thttps://a.example/1\tA one\n200\thttps://a.example/2\tA two\n300\thttps://b.example/\tB\n").unwrap();
    let mut stats = PrivacyStats::default();
    stats.record_blocked(&privacy_stats::today_utc(), "tracker.example");
    stats.record_blocked(&privacy_stats::today_utc(), "tracker.example");
    stats.record_blocked("2020-01-01", "ads.example");
    stats.save(&paths.privacy).unwrap();

    let page = json(&paths, &on, shown);
    assert!(page.contains(r#""kind":"topsites","sites":[{"host":"a.example","title":"A two","url":"https://a.example/2"}"#), "{page}");
    assert!(page.contains(r#""kind":"privacy","today":2,"total":3,"topHost":"tracker.example","topCount":2"#), "{page}");
    assert!(page.contains(r#""kind":"date""#));

    run(&paths, &mut shown, Action::WidgetAdd(WidgetKind::Notes));
    run(&paths, &mut shown, Action::WidgetAdd(WidgetKind::Webpage));
    run(&paths, &mut shown, Action::WidgetAdd(WidgetKind::Bookmarks));
    let set = load(&paths).widgets;
    let note = set.items.iter().find(|w| w.kind == WidgetKind::Notes).unwrap().id;
    let web = set.items.iter().find(|w| w.kind == WidgetKind::Webpage).unwrap().id;
    let marks = set.items.iter().find(|w| w.kind == WidgetKind::Bookmarks).unwrap().id;

    run(&paths, &mut shown, Action::WidgetNote { id: note, text: "buy milk\nand eggs".to_owned() });
    run(&paths, &mut shown, Action::WidgetUrl { id: web, url: "https://widget.example/clock".to_owned() });
    run(&paths, &mut shown, Action::WidgetFolder { id: marks, folder: work });
    run(&paths, &mut shown, Action::WidgetView { id: marks, view: model::BookmarksView::Tiles });
    let page = json(&paths, &on, shown);
    assert!(page.contains(r#""kind":"notes","text":"buy milk\nand eggs""#), "{page}");
    assert!(page.contains(r#""kind":"webpage","url":"https://widget.example/clock""#), "{page}");
    assert!(page.contains(&format!(r#""kind":"bookmarks","folder":{work},"view":"tiles","links":[{{"id":2,"title":"A","url":"https://a.example/","host":"a.example","thumb":false}}"#)), "{page}");

    run(&paths, &mut shown, Action::WidgetMove { id: marks, before: Some(1) });
    assert_eq!(load(&paths).widgets.items[0].id, marks, "rearranging widgets is saved");
    run(&paths, &mut shown, Action::WidgetRemove(web));
    assert!(!load(&paths).widgets.items.iter().any(|w| w.id == web));
    assert!(apply(&paths, &mut shown, Action::WidgetUrl { id: note, url: "https://x.example/".to_owned() }).is_err(), "a note has no address");
    assert!(apply(&paths, &mut shown, Action::WidgetFolder { id: marks, folder: 9999 }).is_err(), "no such folder");

    // The sticky note goes to the Notes file on request, empty notes are refused.
    let Effect::SaveNote { title, body } = run(&paths, &mut shown, Action::WidgetSaveNote(note)) else { panic!("a save note effect") };
    assert_eq!((title.as_str(), body.as_str()), ("buy milk", "buy milk\nand eggs"));
    let notes = paths.dir.join("notes.txt");
    save_note(&notes, &title, &body).unwrap();
    let saved = productivity::read_note_list(&notes);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].title, "buy milk");
    assert_eq!(saved[0].body, "buy milk\nand eggs");
    run(&paths, &mut shown, Action::WidgetNote { id: note, text: "  ".to_owned() });
    assert!(apply(&paths, &mut shown, Action::WidgetSaveNote(note)).unwrap_err().contains("empty"));
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn opening_resolves_a_dial_in_a_group_and_nothing_else() {
    let paths = temp("open");
    let mut shown = None;
    let work = add_group(&paths, &mut shown, "Work");
    add_site(&paths, &mut shown, work, "A", "https://a.example/page");
    let dial = ids(&paths, work)[0];
    assert_eq!(
        run(&paths, &mut shown, Action::Open { id: dial, new_tab: true }),
        Effect::Open { url: url::Url::parse("https://a.example/page").unwrap(), new_tab: true }
    );
    // A bookmark outside every group is not a dial.
    let mut store = load(&paths).store;
    let loose = store.add_bookmark(None, "Loose", "https://loose.example/").unwrap();
    store.save(&paths.dir).unwrap();
    assert!(apply(&paths, &mut shown, Action::Open { id: loose, new_tab: false }).is_err());
    assert!(apply(&paths, &mut shown, Action::Open { id: work, new_tab: false }).is_err(), "a folder is not opened");
    assert_eq!(
        run(&paths, &mut shown, Action::OpenUrl { url: "https://top.example/".to_owned(), new_tab: false }),
        Effect::Open { url: url::Url::parse("https://top.example/").unwrap(), new_tab: false }
    );
    assert_eq!(run(&paths, &mut shown, Action::Settings), Effect::Settings);
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn a_guest_window_shows_nothing_of_the_profile() {
    let paths = temp("guest");
    let mut shown = None;
    let work = add_group(&paths, &mut shown, "Work");
    add_site(&paths, &mut shown, work, "A", "https://a.example/");
    let built = page_with(Some(&paths), true, &StartSettings { widgets: true, ..StartSettings::default() }, None, Ack::None, |_| None);
    assert!(built.json.contains(r#""guest":true"#) && built.json.contains(r#""groups":[]"#) && built.json.contains(r#""dials":[]"#), "{}", built.json);
    assert!(built.thumbs.is_empty());
    let none = page_with(None, false, &StartSettings::default(), None, Ack::None, |_| None);
    assert!(none.json.contains(r#""groups":[]"#));
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn the_stamp_tells_an_unchanged_page_from_a_changed_one() {
    let paths = temp("stamp");
    let before = stamp(&paths);
    assert!(before.iter().all(Option::is_none));
    let mut shown = None;
    let work = add_group(&paths, &mut shown, "Work");
    let after = stamp(&paths);
    assert_ne!(before, after);
    assert_eq!(stamp(&paths), after, "nothing changed");
    add_site(&paths, &mut shown, work, "A", "https://a.example/");
    assert_ne!(stamp(&paths), after);
    let _ = fs::remove_dir_all(&paths.dir);
}

/// What a new tab pays for the start page on the UI thread: the stamp check when nothing changed, and a
/// full rebuild when something did, against a large profile. Printed with `-- --nocapture` (an optimised
/// run on a quiet machine: a rebuild of 4000 bookmarks takes about 20 ms, the stamp check under 1 ms); the
/// bounds are loose on purpose and exist to catch a change that makes either one pathologically slow.
#[test]
fn a_new_tab_costs_little_on_the_ui_thread_even_with_a_large_profile() {
    use std::time::Instant;
    let paths = temp("cost");
    let mut store = BookmarkStore::new();
    let mut config = SpeedDialConfig::default();
    // 24 groups of 40 dials, plus 3000 bookmarks in 60 other folders: close to the store's ceiling.
    for g in 0..24 {
        let group = store.add_folder(None, &format!("Group {g}")).unwrap();
        config.groups.push(group);
        for d in 0..40 {
            store.add_bookmark(Some(group), &format!("Dial {g}-{d}"), &format!("https://dial{g}-{d}.example/page")).unwrap();
        }
    }
    for f in 0..60 {
        let folder = store.add_folder(None, &format!("Folder {f}")).unwrap();
        for b in 0..50 {
            store.add_bookmark(Some(folder), &format!("Bookmark {f}-{b}"), &format!("https://bm{f}-{b}.example/")).unwrap();
        }
    }
    store.save(&paths.dir).unwrap();
    config.last = config.groups.first().copied();
    config.save(&paths.dir).unwrap();
    let mut widgets = WidgetSet::default();
    for kind in [WidgetKind::Bookmarks, WidgetKind::Notes, WidgetKind::Webpage] {
        widgets.add(kind).unwrap();
    }
    widgets.save(&paths.dir).unwrap();
    let history: String = (0..500).map(|i| format!("{}\thttps://site{}.example/p{i}\tPage {i}\n", 1_700_000_000 + i, i % 40)).collect();
    fs::write(&paths.history, history).unwrap();
    let on = StartSettings { widgets: true, ..StartSettings::default() };

    // Where a first build spends its time (printed, not asserted).
    let t = Instant::now();
    let loaded = load(&paths);
    let t_load = t.elapsed();
    let t = Instant::now();
    let view = model::dial_view(&loaded.store, &loaded.config, GroupReopen::Last, None);
    let t_view = t.elapsed();
    let t = Instant::now();
    let _ = productivity::read_history(&paths.history);
    let t_history = t.elapsed();
    println!("  load {t_load:?} (store, config, widgets), dial_view {t_view:?}, history {t_history:?}, {} dials", view.dials.len());
    let t = Instant::now();
    let first = page_with(Some(&paths), false, &on, None, Ack::None, |_| None);
    let cold = t.elapsed();
    let rounds = 20;
    let t = Instant::now();
    for _ in 0..rounds {
        let _ = page_with(Some(&paths), false, &on, None, Ack::None, |_| None);
    }
    let warm = t.elapsed() / rounds;
    let t = Instant::now();
    for _ in 0..1000 {
        let _ = stamp(&paths);
    }
    let stamp_cost = t.elapsed() / 1000;
    println!("start page, large profile: first build {cold:?}, rebuild {warm:?}, stamp check {stamp_cost:?}, snapshot {} bytes", first.json.len());
    assert!(first.json.len() < wire::MAX_SNAPSHOT_BYTES, "the snapshot stays under the page's limit: {}", first.json.len());
    // Debug build, other tests running beside this one, a busy machine: the bounds are for a change that
    // makes these quadratic or reads the disk per tile, which costs seconds, not milliseconds.
    assert!(cold.as_millis() < 3000, "first build {cold:?}");
    assert!(warm.as_millis() < 2000, "rebuild {warm:?}");
    assert!(stamp_cost.as_millis() < 50, "stamp check {stamp_cost:?}");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn refused_changes_come_back_in_plain_words() {
    let paths = temp("words");
    let mut shown = None;
    let work = add_group(&paths, &mut shown, "Work");
    let bad = apply(&paths, &mut shown, Action::Add { folder: work, title: "x".to_owned(), url: "ftp://x.example/".to_owned() }).unwrap_err();
    assert_eq!(bad, "Enter an http or https address.");
    assert_eq!(friendly(&io::Error::other("something odd")), "That change could not be saved.");
    let page = page_with(Some(&paths), false, &StartSettings::default(), shown, Ack::Failed(bad), |_| None);
    assert!(page.json.ends_with(r#""ack":"failed","error":"Enter an http or https address."}}"#), "{}", page.json);
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn a_speed_dial_edit_refuses_and_keeps_every_bookmark_when_the_file_cannot_be_read() {
    let mut good = BookmarkStore::new();
    for n in 0..50 {
        good.add_bookmark(None, &format!("Site {n}"), &format!("https://site{n}.example/")).unwrap();
    }
    let mut not_utf8 = good.serialize().into_bytes();
    not_utf8.extend_from_slice(&[0xff, 0xfe, b'\n']);
    let mut too_large = good.serialize().into_bytes();
    too_large.resize(1024 * 1024 + 1, b'#');
    let bad_header = good.serialize().replacen("bowser-bookmarks-v2", "bowser-bookmarks-v9", 1).into_bytes();
    for (name, bytes) in [("not-utf8", not_utf8), ("too-large", too_large), ("bad-header", bad_header)] {
        let paths = temp(&format!("unreadable-{name}"));
        let file = paths.dir.join("bookmarks-v2.txt");
        fs::write(&file, &bytes).unwrap();
        let mut shown = None;
        for action in [
            Action::AddGroup("New group".to_owned()),
            Action::Add { folder: 1, title: "x".to_owned(), url: "https://x.example/".to_owned() },
            Action::RenameGroup { id: 1, title: "Renamed".to_owned() },
            Action::Remove(1),
            Action::Move { id: 1, before: None },
        ] {
            let refused = apply(&paths, &mut shown, action).expect_err(name);
            assert_eq!(refused, UNREADABLE, "{name}");
            assert!(fs::read(&file).unwrap() == bytes, "{name}: the bookmarks file changed");
        }
        assert_eq!(set_thumbnail(&paths, 1, b"jpeg").unwrap_err(), UNREADABLE, "{name}");
        assert!(fs::read(&file).unwrap() == bytes, "{name}: the bookmarks file changed");
        let _ = fs::remove_dir_all(&paths.dir);
    }
}

#[test]
fn a_speed_dial_edit_keeps_every_bookmark_when_one_line_is_malformed() {
    let mut good = BookmarkStore::new();
    for n in 0..50 {
        good.add_bookmark(None, &format!("Site {n}"), &format!("https://site{n}.example/")).unwrap();
    }
    let paths = temp("bad-line");
    fs::write(paths.dir.join("bookmarks-v2.txt"), format!("{}garbage line without tabs\n\n", good.serialize())).unwrap();
    let mut shown = None;
    let group = add_group(&paths, &mut shown, "New group");
    let after = BookmarkStore::load(&paths.dir).unwrap();
    assert_eq!(after.items().len(), 51, "every bookmark and the new group");
    for item in good.items() {
        assert!(after.items().contains(item), "{} kept", item.title);
    }
    assert!(after.items().iter().any(|i| i.id == group && i.title == "New group"));
    let _ = fs::remove_dir_all(&paths.dir);
}
