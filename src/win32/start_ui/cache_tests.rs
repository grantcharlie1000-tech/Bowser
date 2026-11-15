//! Cache behavior with real profile files and page builds, without a native window or WebView.
use super::*;
use std::{cell::Cell, fs};
use crate::{chrome_host::start_page::Action, start_page::{self as model, GroupReopen}};

struct Fixture {
    _profile: profile::TestProfileGuard,
    paths: Paths,
    owner: HWND,
    now: Instant,
    settings: StartSettings,
    builds: Cell<usize>,
}

impl Fixture {
    fn new() -> Self {
        let guard = profile::TestProfileGuard::isolated();
        let paths = Paths::in_dir(profile::active().unwrap().root());
        let owner = HWND(1usize as *mut _);
        RUNTIMES.with(|r| r.borrow_mut().remove(&(owner.0 as isize)));
        Self { _profile: guard, paths, owner, now: Instant::now(), settings: StartSettings::default(), builds: Cell::new(0) }
    }

    fn due(&self, tab: u32, appeared: bool) -> Option<String> {
        self.due_in(&self.paths, false, tab, appeared)
    }

    fn due_in(&self, paths: &Paths, guest: bool, tab: u32, appeared: bool) -> Option<String> {
        due_with(self.owner, self.settings.clone(), TabId(tab), appeared, self.now,
            || (Some(paths.clone()), guest),
            |paths, guest, settings, requested, ack| {
                self.builds.set(self.builds.get() + 1);
                service::page(paths, guest, settings, requested, ack)
            })
    }

    fn settle(&self) { posted_with(self.owner, |_| true); }

    fn group(&self, title: &str) -> u64 {
        let mut requested = None;
        service::apply(&self.paths, &mut requested, Action::AddGroup(title.into())).unwrap();
        requested.unwrap()
    }

    fn site(&self, group: u64) -> u64 {
        service::apply(&self.paths, &mut None, Action::Add {
            folder: group, title: "A site".into(), url: "https://a.example/".into(),
        }).unwrap();
        service::load(&self.paths).store.children(Some(group))[0].id
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        RUNTIMES.with(|r| r.borrow_mut().remove(&(self.owner.0 as isize)));
    }
}

#[test]
fn unchanged_blank_tab_switches_and_reopens_reuse_one_model() {
    let f = Fixture::new();
    f.site(f.group("Work"));
    let first = f.due(1, true).unwrap();
    assert!(first.contains("A site"));
    f.settle();
    for tab in [2, 1, 3, 3] {
        assert_eq!(f.due(tab, true).as_deref(), Some(first.as_str()));
        f.settle();
    }
    assert_eq!(f.builds.get(), 1, "tab identity and reappearance do not reload unchanged data");
    assert!(f.due(3, false).is_none());
}

#[test]
fn failed_delivery_retries_the_same_page_and_thumbnails_without_rebuilding() {
    let f = Fixture::new();
    let id = f.site(f.group("Work"));
    service::set_thumbnail(&f.paths, id, &[255, 216, 255, 224, 1]).unwrap();
    let first = f.due(1, true).unwrap();
    let thumbs = with(f.owner, |r| r.pending_thumbs.clone()).unwrap();
    assert_eq!(thumbs.len(), 1);
    assert_eq!(f.due(1, false), Some(first));
    assert_eq!(with(f.owner, |r| r.pending_thumbs.clone()).unwrap(), thumbs);
    assert_eq!(f.builds.get(), 1);
    f.settle();
    assert!(f.due(1, false).is_none());
}

#[test]
fn a_failed_thumbnail_post_keeps_the_page_due_for_retry() {
    let f = Fixture::new();
    let id = f.site(f.group("Work"));
    service::set_thumbnail(&f.paths, id, &[255, 216, 255, 224, 1]).unwrap();
    let page = f.due(1, true).unwrap();
    posted_with(f.owner, |_| false);
    assert_eq!(f.due(1, false), Some(page));
    let mut delivered = Vec::new();
    posted_with(f.owner, |message| { delivered.push(message.to_owned()); true });
    assert_eq!(delivered.len(), 1);
    assert!(delivered[0].contains("data:image/jpeg;base64,"));
    assert!(f.due(1, false).is_none());
    assert_eq!(f.builds.get(), 1);
}

#[test]
fn failed_acknowledgements_retry_without_reloading_and_do_not_leak_to_a_new_tab() {
    let f = Fixture::new();
    f.group("Work");
    let plain = f.due(1, true).unwrap();
    f.settle();
    fail(f.owner, "Refused \"change\"\nTry again");
    let failed = f.due(1, false).unwrap();
    assert!(failed.contains(r#""ack":"failed","error":"Refused \"change\"\nTry again""#));
    assert_eq!(f.due(1, false), Some(failed));
    assert_eq!(f.builds.get(), 1, "an answer is presentation, not changed profile data");
    f.settle();
    assert!(f.due(1, false).is_none());
    assert_eq!(f.due(2, true), Some(plain));
    assert_eq!(f.builds.get(), 1);
}

#[test]
fn reappearance_checks_files_inside_the_poll_interval() {
    let f = Fixture::new();
    let group = f.group("Work");
    f.due(1, true).unwrap();
    f.settle();
    f.site(group);
    let fresh = f.due(1, true).unwrap();
    assert!(fresh.contains("A site"));
    assert_eq!(f.builds.get(), 2);
}

#[test]
fn settings_and_explicit_refresh_invalidate_the_model() {
    let mut f = Fixture::new();
    f.due(1, true).unwrap();
    f.settle();
    f.settings.columns = 4;
    assert!(f.due(1, false).unwrap().contains(r#""columns":4"#));
    f.settle();
    with(f.owner, |r| r.dirty = true);
    assert!(f.due(1, false).is_some());
    assert_eq!(f.builds.get(), 3);
}

#[test]
fn polling_refreshes_history_privacy_and_widget_edits() {
    let mut f = Fixture::new();
    f.settings.widgets = true;
    f.due(1, true).unwrap();
    f.settle();
    fs::write(&f.paths.history, "100\thttps://visited.example/\tVisited today\n").unwrap();
    f.now += STAMP_EVERY;
    assert!(f.due(1, false).unwrap().contains("Visited today"));
    f.settle();
    let mut stats = crate::privacy_stats::PrivacyStats::default();
    stats.record_blocked(&crate::privacy_stats::today_utc(), "tracker.example");
    stats.save(&f.paths.privacy).unwrap();
    f.now += STAMP_EVERY;
    assert!(f.due(1, false).unwrap().contains(r#""today":1,"total":1"#));
    f.settle();
    service::apply(&f.paths, &mut None, Action::WidgetAdd(model::WidgetKind::Notes)).unwrap();
    f.now += STAMP_EVERY;
    assert!(f.due(1, false).unwrap().contains(r#""kind":"notes""#));
    assert_eq!(f.builds.get(), 4);
}

#[test]
fn profile_and_guest_changes_cannot_use_the_fast_path_of_another_profile() {
    let f = Fixture::new();
    f.site(f.group("Private profile data"));
    f.due(1, true).unwrap();
    f.settle();
    let guest = f.due_in(&f.paths, true, 1, false).expect("guest isolation is checked before throttling");
    assert!(guest.contains(r#""guest":true"#));
    assert!(!guest.contains("Private profile data") && !guest.contains("A site"));
    assert!(with(f.owner, |r| r.pending_thumbs.is_empty()).unwrap());
    f.settle();
    let other = Paths::in_dir(&f.paths.dir.join("other"));
    let empty = f.due_in(&other, false, 1, false).unwrap();
    assert!(empty.contains(r#""guest":false"#) && !empty.contains("Private profile data"));
    f.settle();
    assert!(f.due(1, false).unwrap().contains("Private profile data"));
}

#[test]
fn profile_identity_matters_even_when_every_file_stamp_matches() {
    let f = Fixture::new();
    f.due(1, true).unwrap();
    f.settle();
    let other = Paths::in_dir(&f.paths.dir.join("empty-other"));
    assert_eq!(service::stamp(&other), service::stamp(&f.paths));
    assert!(f.due_in(&other, false, 2, true).is_some());
    assert_eq!(f.builds.get(), 2);
}

#[test]
fn returning_from_a_subfolder_respects_first_and_last_group_settings() {
    for reopen in [GroupReopen::First, GroupReopen::Last] {
        let mut f = Fixture::new();
        f.settings.group_reopen = reopen;
        let first = f.group("First");
        let last = f.group("Last");
        let mut requested = None;
        service::apply(&f.paths, &mut requested, Action::AddFolder { folder: last, title: "Nested".into() }).unwrap();
        let nested = service::load(&f.paths).store.children(Some(last))[0].id;
        f.due(1, true).unwrap();
        f.settle();
        with(f.owner, |r| r.requested = Some(nested));
        assert!(f.due(1, false).unwrap().contains(&format!(r#""folder":{nested}"#)));
        f.settle();
        let expected = if reopen == GroupReopen::First { first } else { last };
        assert!(f.due(2, true).unwrap().contains(&format!(r#""folder":{expected}"#)));
    }
}

#[test]
fn selecting_the_default_group_does_not_force_a_reload_when_the_next_tab_opens() {
    let f = Fixture::new();
    let group = f.group("Work");
    f.due(1, true).unwrap();
    f.settle();
    with(f.owner, |r| r.requested = Some(group));
    f.due(1, false);
    f.settle();
    let before = f.builds.get();
    assert!(f.due(2, true).unwrap().contains(r#""title":"Work""#));
    assert_eq!(f.builds.get(), before, "the resolved default group has not changed");
}

#[test]
fn replacing_thumbnail_bytes_refreshes_even_without_a_config_change() {
    let f = Fixture::new();
    let id = f.site(f.group("Work"));
    service::set_thumbnail(&f.paths, id, &[255, 216, 255, 224, 1]).unwrap();
    let page = f.due(1, true).unwrap();
    let old = with(f.owner, |r| r.pending_thumbs.clone()).unwrap();
    f.settle();
    fs::write(model::thumb_path(&f.paths.dir, id), [255, 216, 255, 224, 2, 3]).unwrap();
    assert_eq!(f.due(1, true), Some(page.clone()), "only image bytes changed");
    let fresh = with(f.owner, |r| r.pending_thumbs.clone()).unwrap();
    assert_ne!(old, fresh, "the image bytes are a model dependency too");
    assert!(!settled(f.owner), "identical JSON still needs the fresh images posted");
    let mut delivered = Vec::new();
    posted_with(f.owner, |message| { delivered.push(message.to_owned()); true });
    assert_eq!(delivered, fresh);
    assert_eq!(f.due(2, true), Some(page));
    assert!(settled(f.owner), "a fully delivered model needs no duplicate image posts");
}

#[test]
fn a_favicon_arriving_after_the_first_snapshot_is_shown_on_reappearance() {
    let f = Fixture::new();
    f.site(f.group("Work"));
    let old = f.due(1, true).unwrap();
    f.settle();
    let png = [137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1];
    crate::favicon::store_favicon("a.example", &png).unwrap();
    let fresh = f.due(1, true).unwrap();
    assert_ne!(old, fresh);
    assert!(fresh.contains(r#""a.example":"data:image/png;base64,"#));
}

#[test]
fn private_tabs_never_receive_a_normal_start_page_snapshot() {
    let f = Fixture::new();
    f.site(f.group("Private profile data"));
    let app = crate::app::test_app();
    let private = App::new_private_tab(&app);
    assert!(due(f.owner, &app, private, true).is_none());
}

#[test]
fn a_failed_bookmark_read_is_retried_even_if_the_file_stamp_is_unchanged() {
    let mut f = Fixture::new();
    f.site(f.group("Work"));
    let path = f.paths.dir.join("bookmarks-v2.txt");
    let good = fs::read(&path).unwrap();
    fs::write(&path, vec![255; good.len()]).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let stamp = service::stamp(&f.paths);
    assert!(!f.due(1, true).unwrap().contains("A site"));
    f.settle();
    f.now += STAMP_EVERY;
    assert!(f.due(1, false).is_none(), "unchanged unreadable inputs do not cause a polling rebuild loop");
    fs::write(&path, good).unwrap();
    fs::OpenOptions::new().write(true).open(&path).unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified)).unwrap();
    assert_eq!(service::stamp(&f.paths), stamp);
    assert!(f.due(2, true).unwrap().contains("A site"));
    assert_eq!(f.builds.get(), 2);
}
