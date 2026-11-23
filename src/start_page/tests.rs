use super::*;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bowser-start-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A store with two root folders (`Work` 1, `Play` 2), flagged as groups, and a sub-folder.
fn fixture() -> (BookmarkStore, SpeedDialConfig, [u64; 6]) {
    let mut store = BookmarkStore::new();
    let work = store.add_folder(None, "Work").unwrap();
    let play = store.add_folder(None, "Play").unwrap();
    let a = store.add_bookmark(Some(work), "Alpha", "https://alpha.example/").unwrap();
    let b = store.add_bookmark(Some(work), "Beta", "https://beta.example/").unwrap();
    let sub = store.add_folder(Some(work), "Docs").unwrap();
    let deep = store.add_bookmark(Some(sub), "Deep", "https://deep.example/").unwrap();
    let config = SpeedDialConfig { groups: vec![work, play], last: None, thumbs: HashMap::new() };
    (store, config, [work, play, a, b, sub, deep])
}

#[test]
fn settings_round_trip_through_the_flat_keys() {
    let mut settings = StartSettings::default();
    for (key, value) in [
        ("start_group_reopen", "first"),
        ("start_titles", "auto"),
        ("start_add_button", "false"),
        ("start_columns", "9"),
        ("start_thumb_size", "5"),
        ("start_drag_reorder", "false"),
        ("homepage", "https://example.com/home"),
        ("startup", "specific_pages"),
        ("startup_pages", "https://a.example/ https://b.example/x"),
        ("start_widgets", "true"),
        ("start_widget_columns", "4"),
        ("start_widget_height", "tall"),
        ("start_widget_bg", "custom"),
        ("start_widget_color", "#aabbcc"),
        ("start_widget_drag", "false"),
        ("start_dials", "true"),
    ] {
        assert!(settings.parse_key(key, value), "{key}");
    }
    assert_eq!(settings.columns, 9);
    assert!(settings.dials, "start_dials turns the speed dials on");
    assert!(!StartSettings::default().dials, "a new tab is blank by default");
    assert_eq!(settings.startup_pages, ["https://a.example/", "https://b.example/x"]);
    let pairs = settings.pairs();
    assert_eq!(pairs.iter().map(|(k, _)| *k).collect::<Vec<_>>(), SETTING_KEYS);
    let mut again = StartSettings::default();
    for (key, value) in &pairs {
        assert_eq!(again.apply_strict(key, value), Some(true), "{key}={value}");
    }
    assert_eq!(again, settings);
    assert!(!settings.parse_key("not_ours", "x"));
}

#[test]
fn tolerant_parse_clamps_and_ignores_garbage_but_strict_refuses_it() {
    let mut settings = StartSettings::default();
    settings.parse_key("start_columns", "99");
    settings.parse_key("start_thumb_size", "0");
    settings.parse_key("start_widget_columns", "9");
    assert_eq!((settings.columns, settings.thumb_size, settings.widget_columns), (MAX_COLUMNS, MIN_THUMB_SIZE, MAX_WIDGET_COLUMNS));
    settings.parse_key("start_titles", "sometimes");
    settings.parse_key("homepage", "javascript:alert(1)");
    settings.parse_key("startup_pages", "https://ok.example/ file:///etc/passwd");
    assert_eq!(settings.titles, TitleMode::Always);
    assert_eq!(settings.homepage, "");
    assert!(settings.startup_pages.is_empty());

    for (key, value) in [
        ("start_columns", "13"),
        ("start_columns", "0"),
        ("start_columns", "06"),
        ("start_thumb_size", "6"),
        ("start_titles", "Always"),
        ("start_add_button", "yes"),
        ("homepage", "ftp://example.com"),
        ("homepage", "https://user:pw@example.com/"),
        ("startup", "restore"),
        ("startup_pages", "https://a.example/ nope"),
        ("start_widget_color", "#abc"),
        ("start_widget_bg", "Solid"),
    ] {
        assert_eq!(StartSettings::default().apply_strict(key, value), None, "{key}={value}");
    }
    let nine = (0..9).map(|i| format!("https://p{i}.example/")).collect::<Vec<_>>().join(" ");
    assert_eq!(StartSettings::default().apply_strict("startup_pages", &nine), None, "at most eight startup pages");
}

#[test]
fn web_addresses_are_http_https_without_credentials_or_spaces() {
    assert_eq!(web_address(" https://Example.com/a?b=1 ").as_deref(), Some("https://example.com/a?b=1"));
    for bad in ["", "example.com", "about:blank", "javascript:alert(1)", "data:text/html,x", "https://a b.example/", "https://u:p@x.example/", "bowser://settings", "https://"] {
        assert_eq!(web_address(bad), None, "{bad}");
    }
    assert_eq!(web_address(&format!("https://x.example/{}", "a".repeat(MAX_URL_CHARS))), None);
}

#[test]
fn startup_modes_decide_what_the_first_window_opens() {
    let mut settings = StartSettings::default();
    assert_eq!(settings.startup, Startup::LastSession, "tabs come back on every start, so Continue is the default");
    assert_eq!(startup_plan(&settings, false), StartupPlan::ShowStartPage);
    settings.startup = Startup::Homepage;
    assert_eq!(startup_plan(&settings, false), StartupPlan::ShowStartPage, "no homepage set: nothing opens");
    settings.homepage = "https://home.example/".to_owned();
    assert_eq!(startup_plan(&settings, false), StartupPlan::Homepage(url::Url::parse("https://home.example/").unwrap()));
    assert_eq!(startup_plan(&settings, true), StartupPlan::ShowStartPage, "a command-line address or a safe restore wins");
    settings.startup = Startup::SpecificPages;
    assert_eq!(startup_plan(&settings, false), StartupPlan::ShowStartPage, "no pages listed");
    settings.startup_pages = vec!["https://a.example/".to_owned(), "https://b.example/".to_owned()];
    assert_eq!(
        startup_plan(&settings, false),
        StartupPlan::Pages(vec![url::Url::parse("https://a.example/").unwrap(), url::Url::parse("https://b.example/").unwrap()])
    );
    settings.startup = Startup::LastSession;
    assert_eq!(startup_plan(&settings, false), StartupPlan::ShowStartPage, "the session itself is restored where it is opened");
    assert_eq!(settings.home_target(), Some(url::Url::parse("https://home.example/").unwrap()));
    settings.homepage.clear();
    assert_eq!(settings.home_target(), None);
}

#[test]
fn the_old_start_page_startup_reads_as_continue_and_is_never_written() {
    assert_eq!(Startup::parse("start_page"), Some(Startup::LastSession));
    assert_eq!(Startup::parse(" Start_Page "), Some(Startup::LastSession));
    assert!(Startup::ALL.iter().all(|mode| mode.key() != LEGACY_START_PAGE), "no choice writes the old value");
    let mut settings = StartSettings::default();
    settings.startup = Startup::Homepage;
    assert!(settings.apply_strict("startup", "start_page").is_some(), "an older page's save is still taken");
    assert_eq!(settings.startup, Startup::LastSession);
    assert!(settings.apply_strict("startup", "Start_page").is_none(), "strict: the exact spelling only");
}

#[test]
fn startup_pages_open_in_front_of_restored_tabs_without_opening_twice() {
    let u = |s: &str| url::Url::parse(s).unwrap();
    let a = u("https://a.example/");
    let b = u("https://b.example/");
    // Nothing restored: the blank first tab takes the first page, the rest get tabs of their own.
    assert_eq!(startup_opens(&[a.clone(), b.clone()], &[None], true), [StartupOpen::FirstTab, StartupOpen::NewTab]);
    // Restored tabs stay as they are: the pages open beside them, and one already open is shown instead.
    let restored = [Some("https://x.example/".to_owned()), Some(b.as_str().to_owned())];
    assert_eq!(startup_opens(&[a.clone(), b.clone()], &restored, false), [StartupOpen::NewTab, StartupOpen::Existing(1)]);
    // A page listed twice opens once.
    assert_eq!(startup_opens(&[a.clone(), a.clone()], &[None], true), [StartupOpen::FirstTab, StartupOpen::Existing(0)]);
    assert_eq!(startup_opens(&[a.clone(), a.clone()], &restored, false), [StartupOpen::NewTab, StartupOpen::Existing(2)]);
    // A restored first tab is never taken over, even when it has no address.
    assert_eq!(startup_opens(&[a], &[None], false), [StartupOpen::NewTab]);
    assert_eq!(startup_opens(&[b], &[], true), [StartupOpen::NewTab], "no tab at all: nothing to take over");
}

#[test]
fn speed_dial_config_round_trips_and_drops_unsafe_thumbnail_names() {
    let mut config = SpeedDialConfig { groups: vec![3, 1, 2], last: Some(1), thumbs: HashMap::new() };
    config.thumbs.insert(7, "7.jpg".to_owned());
    let parsed = SpeedDialConfig::parse(&config.serialize());
    assert_eq!(parsed, config);
    let hostile = format!("{HEADER}\ngroup\t1\ngroup\t1\nthumb\t5\t..\\..\\evil.exe\nthumb\t6\t6.jpg\nbogus\t1\n");
    let parsed = SpeedDialConfig::parse(&hostile);
    assert_eq!(parsed.groups, [1], "a repeated group is kept once");
    assert_eq!(parsed.thumbs.get(&5), None);
    assert_eq!(parsed.thumbs.get(&6).map(String::as_str), Some("6.jpg"));
    assert_eq!(SpeedDialConfig::parse("not a speed dial file"), SpeedDialConfig::default());
}

#[test]
fn config_saves_and_loads_through_the_profile_folder() {
    let dir = temp_dir("config");
    assert_eq!(SpeedDialConfig::load(&dir), SpeedDialConfig::default());
    let config = SpeedDialConfig { groups: vec![4, 9], last: Some(9), thumbs: HashMap::new() };
    config.save(&dir).unwrap();
    assert_eq!(SpeedDialConfig::load(&dir), config);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_page_opens_on_the_last_or_the_first_group_as_set() {
    let (store, mut config, [work, play, ..]) = fixture();
    config.last = Some(play);
    assert_eq!(dial_view(&store, &config, GroupReopen::Last, None).group, play);
    assert_eq!(dial_view(&store, &config, GroupReopen::First, None).group, work);
    config.last = Some(999);
    assert_eq!(dial_view(&store, &config, GroupReopen::Last, None).group, work, "a vanished last group falls back to the first");
    config.last = None;
    assert_eq!(dial_view(&store, &config, GroupReopen::Last, None).group, work);
}

#[test]
fn a_view_lists_dials_in_order_and_opens_sub_folders_with_a_path() {
    let (store, config, [work, _, a, b, sub, deep]) = fixture();
    let view = dial_view(&store, &config, GroupReopen::First, None);
    assert_eq!(view.groups.iter().map(|g| g.title.as_str()).collect::<Vec<_>>(), ["Work", "Play"]);
    assert_eq!(view.groups[0].count, 3);
    assert_eq!(view.dials.iter().map(|d| d.id).collect::<Vec<_>>(), [a, b, sub]);
    assert_eq!(view.dials[2].kind, DialKind::Folder { count: 1 });
    assert_eq!(view.path, [Crumb { id: work, title: "Work".to_owned() }]);

    let inside = dial_view(&store, &config, GroupReopen::First, Some(sub));
    assert_eq!(inside.group, work);
    assert_eq!(inside.folder, sub);
    assert_eq!(inside.dials.iter().map(|d| d.id).collect::<Vec<_>>(), [deep]);
    assert_eq!(inside.path.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), ["Work", "Docs"]);
    assert_eq!(inside.dials[0].kind, DialKind::Link { url: "https://deep.example/".to_owned(), host: "deep.example".to_owned() });

    // A folder outside every group, or a link, is not a place the page can show.
    let mut outside = store.clone();
    let loose = outside.add_folder(None, "Loose").unwrap();
    assert_eq!(dial_view(&outside, &config, GroupReopen::First, Some(loose)).folder, work);
    assert_eq!(dial_view(&store, &config, GroupReopen::First, Some(a)).folder, work);
    assert!(dial_view(&outside, &config, GroupReopen::First, None).choices.iter().any(|c| c.id == loose), "the folder picker offers it");
    assert!(dial_view(&outside, &config, GroupReopen::First, None).choices.iter().all(|c| c.id != work), "but not a group");
}

#[test]
fn with_no_groups_the_view_is_empty_but_still_offers_folders() {
    let (store, mut config, _) = fixture();
    config.groups.clear();
    let view = dial_view(&store, &config, GroupReopen::Last, None);
    assert!(view.groups.is_empty() && view.dials.is_empty());
    assert_eq!(view.group, 0);
    assert_eq!(view.choices.len(), 3);
}

#[test]
fn adding_editing_and_removing_dials_is_the_bookmark_stores_own_work() {
    let (mut store, mut config, [work, _, a, _, sub, _]) = fixture();
    let new = add_dial(&mut store, &config, work, "", "https://new.example/page").unwrap();
    assert_eq!(find(&store, new).unwrap().title, "new.example", "an empty title is the host");
    assert!(add_dial(&mut store, &config, work, "x", "javascript:1").is_err());
    assert!(add_dial(&mut store, &config, work, "dup", "https://alpha.example/").is_err(), "the store refuses a duplicate address");
    let mut loose = store.clone();
    let loose_folder = loose.add_folder(None, "Loose").unwrap();
    assert!(add_dial(&mut loose, &config, loose_folder, "x", "https://x.example/").is_err(), "only groups take dials");

    edit_dial(&mut store, &config, a, "Alpha 2", "https://alpha2.example/").unwrap();
    assert_eq!(find(&store, a).unwrap().title, "Alpha 2");
    assert!(matches!(&find(&store, a).unwrap().kind, BookmarkKind::Link { url, .. } if url == "https://alpha2.example/"));
    edit_dial(&mut store, &config, sub, "Manuals", "").unwrap();
    assert_eq!(find(&store, sub).unwrap().title, "Manuals");
    assert!(edit_dial(&mut store, &config, a, "Alpha", "ftp://x").is_err());
    assert!(edit_dial(&mut store, &config, work, "Renamed", "").is_err(), "a group is renamed with its own command");

    let removed = remove_dial(&mut store, &mut config, None, sub).unwrap();
    assert_eq!(removed, 2, "the sub-folder and the link inside it");
    assert!(find(&store, sub).is_none());
    assert!(remove_dial(&mut store, &mut config, None, work).is_err(), "a group has no parent to be removed from");
}

#[test]
fn dials_reorder_within_a_folder_and_move_into_a_sub_folder() {
    let (mut store, config, [work, _, a, b, sub, deep]) = fixture();
    let order = |store: &BookmarkStore, parent: u64| store.children(Some(parent)).iter().map(|i| i.id).collect::<Vec<_>>();
    assert_eq!(order(&store, work), [a, b, sub]);
    reorder_dial(&mut store, &config, sub, Some(a)).unwrap();
    assert_eq!(order(&store, work), [sub, a, b]);
    reorder_dial(&mut store, &config, sub, None).unwrap();
    assert_eq!(order(&store, work), [a, b, sub]);
    reorder_dial(&mut store, &config, b, Some(a)).unwrap();
    assert_eq!(order(&store, work), [b, a, sub]);
    assert!(reorder_dial(&mut store, &config, a, Some(deep)).is_err(), "not a sibling");
    move_dial_into(&mut store, &config, a, sub).unwrap();
    assert_eq!(order(&store, work), [b, sub]);
    assert_eq!(order(&store, sub), [deep, a]);
    assert!(move_dial_into(&mut store, &config, sub, sub).is_err());
    assert!(move_dial_into(&mut store, &config, deep, work).is_ok());
}

#[test]
fn groups_are_added_flagged_unflagged_and_reordered_without_touching_bookmarks() {
    let (mut store, mut config, [work, play, ..]) = fixture();
    let new = add_group(&mut store, &mut config, "News").unwrap();
    assert_eq!(config.groups, [work, play, new]);
    assert_eq!(config.last, Some(new));

    let mut loose = store.clone();
    let folder = loose.add_folder(None, "Loose").unwrap();
    flag_group(&loose, &mut config, folder).unwrap();
    assert_eq!(config.groups.last(), Some(&folder));
    assert!(flag_group(&store, &mut config, 999).is_err());
    let sub = store.children(Some(work)).iter().find(|i| matches!(i.kind, BookmarkKind::Folder)).unwrap().id;
    assert!(flag_group(&store, &mut config, sub).is_err(), "a folder already inside a group is on the page");

    assert!(reorder_group(&mut config, new, Some(work)));
    assert_eq!(&config.groups[..3], [new, work, play]);
    assert!(!reorder_group(&mut config, new, Some(12345)));
    assert_eq!(&config.groups[..3], [new, work, play], "a bad target changes nothing");

    let count = store.items().len();
    assert!(unflag_group(&mut config, work));
    assert_eq!(store.items().len(), count, "the folder and its bookmarks stay");
    assert!(!config.groups.contains(&work));
    assert!(!unflag_group(&mut config, work));
    let mut full = SpeedDialConfig { groups: (1..=MAX_GROUPS as u64).collect(), ..SpeedDialConfig::default() };
    assert!(add_group(&mut store, &mut full, "One more").is_err());
}

#[test]
fn a_chosen_thumbnail_is_kept_small_forgotten_with_its_dial_and_sent_as_a_data_uri() {
    let dir = temp_dir("thumb");
    let (mut store, mut config, [_, _, a, ..]) = fixture();
    let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 1, 2, 3];
    set_thumb(&mut config, &dir, a, &jpeg).unwrap();
    assert!(thumb_path(&dir, a).exists());
    let uri = thumb_data_uri(&config, &dir, a).unwrap();
    assert!(uri.starts_with("data:image/jpeg;base64,/9j/"), "{uri}");
    assert!(set_thumb(&mut config, &dir, a, &[1, 2, 3, 4, 5]).is_err(), "not a JPEG");
    assert!(set_thumb(&mut config, &dir, a, &vec![0xFF; MAX_THUMB_BYTES + 1]).is_err(), "too large");
    assert!(dial_view(&store, &config, GroupReopen::First, None).dials[0].has_thumb);

    // Deleting the dial forgets the image and removes its file.
    remove_dial(&mut store, &mut config, Some(&dir), a).unwrap();
    assert!(config.thumbs.is_empty());
    assert!(!thumb_path(&dir, a).exists());
    assert_eq!(thumb_data_uri(&config, &dir, a), None);
    // A thumbnail file the config does not name is never read.
    fs::create_dir_all(dir.join(THUMB_DIR)).unwrap();
    fs::write(thumb_path(&dir, 77), jpeg).unwrap();
    assert_eq!(thumb_data_uri(&config, &dir, 77), None);
    let _ = fs::remove_dir_all(&dir);
}

/// R-20: a bookmark deleted outside the start page (Bookmarks panel or bar, the star, the palette)
/// took its chosen tile image with it only on the start page's own remove. Any save of the store
/// now deletes the image files and `thumb` lines of bookmarks that are gone, and keeps the rest.
#[test]
fn a_bookmark_deleted_outside_the_start_page_takes_its_thumbnail_with_it() {
    let dir = temp_dir("thumb-orphan");
    let (mut store, mut config, [work, _, a, b, sub, deep]) = fixture();
    store.save(&dir).unwrap();
    let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 1, 2, 3];
    for id in [a, b, deep] {
        set_thumb(&mut config, &dir, id, &jpeg).unwrap();
    }
    config.save(&dir).unwrap();

    // The Bookmarks panel deletes a link and a sub-folder (with the link inside it) and saves.
    store.delete(a);
    store.delete(sub);
    store.save(&dir).unwrap();
    assert!(!thumb_path(&dir, a).exists(), "deleted bookmark's image left on disk");
    assert!(!thumb_path(&dir, deep).exists(), "image of a bookmark inside a deleted folder left on disk");
    assert!(thumb_path(&dir, b).exists(), "a live dial's image was deleted");
    let reloaded = SpeedDialConfig::load(&dir);
    assert_eq!(reloaded.thumbs.keys().copied().collect::<Vec<_>>(), [b]);
    assert_eq!(reloaded.groups, config.groups, "groups kept");
    assert!(store.items().iter().any(|i| i.id == work));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn widgets_default_round_trip_and_reorder() {
    let mut set = WidgetSet::default();
    assert!(set.items.is_empty(), "a profile with no widget file has no widgets");
    let marks = set.add(WidgetKind::Bookmarks).unwrap();
    let other = set.add(WidgetKind::Bookmarks).unwrap();
    {
        let w = set.get_mut(marks).unwrap();
        w.folder = 42;
        w.view = BookmarksView::Tiles;
    }
    let again = WidgetSet::parse(&set.serialize()).unwrap();
    assert_eq!(again, set);

    assert!(set.reorder(other, Some(marks)));
    assert_eq!(set.items[0].id, other);
    assert!(set.reorder(other, None));
    assert_eq!(set.items.last().unwrap().id, other);
    assert!(!set.reorder(marks, Some(9999)));
    assert!(set.remove(other));
    assert!(!set.remove(other));
    assert!(WidgetSet::parse("nope").is_none());
    // A file from before the other widgets were removed: their lines are skipped, the bookmarks stay.
    let old = format!("{WIDGETS_HEADER}\nw\t1\tdate\t0\tlist\t\t\nw\t2\tbookmarks\t7\ttiles\t\t\nw\t3\twebpage\t0\tlist\thttps://x.example/\t\nw\t4\tnotes\t0\tlist\t\tbuy milk\nw\t2\tbookmarks\t0\tlist\t\t\nw\t5\tunknown\t0\tlist\t\t\n");
    let parsed = WidgetSet::parse(&old).unwrap();
    assert_eq!(parsed.items.len(), 1, "removed kinds, a repeated id and an unknown kind are dropped");
    assert_eq!((parsed.items[0].id, parsed.items[0].folder, parsed.items[0].view), (2, 7, BookmarksView::Tiles));
    let mut full = WidgetSet::default();
    for _ in 0..MAX_WIDGETS {
        let _ = full.add(WidgetKind::Bookmarks);
    }
    assert_eq!(full.items.len(), MAX_WIDGETS);
    assert!(full.add(WidgetKind::Bookmarks).is_err());
}

#[test]
fn widget_files_save_and_an_unreadable_one_falls_back_to_the_defaults() {
    let dir = temp_dir("widgets");
    assert_eq!(WidgetSet::load(&dir), WidgetSet::default());
    let mut set = WidgetSet::default();
    set.add(WidgetKind::Bookmarks).unwrap();
    set.save(&dir).unwrap();
    assert_eq!(WidgetSet::load(&dir), set);
    fs::write(dir.join(WIDGETS_FILE), "garbage").unwrap();
    assert_eq!(WidgetSet::load(&dir), WidgetSet::default());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bookmark_widget_lists_only_links_of_its_folder() {
    let (store, _, [work, _, a, b, ..]) = fixture();
    let links = widget_links(&store, work);
    assert_eq!(links.iter().map(|l| l.0).collect::<Vec<_>>(), [a, b], "the sub-folder is not a link");
    assert_eq!(links[0].1, "Alpha");
}
