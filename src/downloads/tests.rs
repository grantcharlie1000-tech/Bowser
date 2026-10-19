use super::*;

const DAY: u64 = 86_400;
/// 2025-10-14 00:00:00 UTC.
const D0: u64 = 20_375 * DAY;
const NOON_MS: u64 = (D0 + 12 * 3600) * 1000;

fn time() -> Time {
    Time { now: D0 + 12 * 3600, offset_secs: 0 }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("bowser-downloads-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn path(name: &str) -> PathBuf {
    PathBuf::from(r"C:\Users\me\Downloads").join(name)
}

fn start(m: &mut DownloadManager, key: u64, name: &str, total: i64, now_ms: u64) -> u32 {
    m.on_event(
        DownloadEvent::Started { key, url: format!("https://www.Example.com/files/{name}"), path: path(name), total },
        now_ms,
    );
    m.entries().iter().find(|e| e.op == Some(key)).map(|e| e.id).expect("started")
}

fn progress(m: &mut DownloadManager, key: u64, got: u64, total: i64, now_ms: u64) -> Change {
    m.on_event(DownloadEvent::Progress { key, got, total }, now_ms)
}

fn finish(m: &mut DownloadManager, key: u64, got: u64, total: i64, now_ms: u64) {
    m.on_event(
        DownloadEvent::State { key, state: OpState::Completed, got, total, err: String::new(), can_resume: false },
        now_ms,
    );
}

fn fail(m: &mut DownloadManager, key: u64, err: &str, can_resume: bool, now_ms: u64) {
    m.on_event(
        DownloadEvent::State { key, state: OpState::Failed, got: 10, total: 100, err: err.to_owned(), can_resume },
        now_ms,
    );
}

fn set_state(m: &mut DownloadManager, key: u64, state: OpState, now_ms: u64) {
    m.on_event(DownloadEvent::State { key, state, got: 10, total: 100, err: String::new(), can_resume: false }, now_ms);
}

// --- names ---

#[test]
fn file_names_lose_paths_and_invalid_characters() {
    assert_eq!(sanitize_file_name("report.pdf"), "report.pdf");
    assert_eq!(sanitize_file_name(r"C:\Windows\System32\evil.dll"), "evil.dll");
    assert_eq!(sanitize_file_name("../../etc/passwd"), "passwd");
    assert_eq!(sanitize_file_name("a<b>c:d\"e|f?g*h.txt"), "a_b_c_d_e_f_g_h.txt");
    assert_eq!(sanitize_file_name("tab\tand\nnewline.txt"), "tab_and_newline.txt");
    assert_eq!(sanitize_file_name("photo\u{202e}gpj.exe"), "photo_gpj.exe", "direction overrides cannot disguise the extension");
    assert_eq!(sanitize_file_name("  spaced out.txt"), "spaced out.txt");
}

#[test]
fn file_names_never_end_in_a_dot_or_space_or_come_out_empty() {
    assert_eq!(sanitize_file_name("name. . "), "name");
    assert_eq!(sanitize_file_name(""), "download");
    assert_eq!(sanitize_file_name("..."), "download");
    assert_eq!(sanitize_file_name("dir/"), "download");
    assert_eq!(sanitize_file_name("   "), "download");
    assert_eq!(sanitize_file_name(".gitignore"), ".gitignore");
}

#[test]
fn reserved_windows_device_names_are_prefixed() {
    for name in ["con", "CON", "con.txt", "Nul.tar.gz", "COM1", "com9.log", "LPT3", "aux.", "prn .txt", "CONIN$"] {
        let clean = sanitize_file_name(name);
        assert!(clean.starts_with('_'), "{name} -> {clean}");
    }
    for name in ["console.txt", "com10.txt", "lpt0", "auxiliary", "nullable.zip", "com.txt"] {
        assert_eq!(sanitize_file_name(name), name);
    }
}

#[test]
fn long_names_are_capped_in_utf16_units_and_keep_their_extension() {
    let long = format!("{}.pdf", "a".repeat(400));
    let clean = sanitize_file_name(&long);
    assert_eq!(utf16_len(&clean), MAX_NAME_UNITS);
    assert!(clean.ends_with(".pdf"));
    // A surrogate pair is never cut in half.
    let emoji = format!("{}.png", "\u{1F600}".repeat(200));
    let clean = sanitize_file_name(&emoji);
    assert!(utf16_len(&clean) <= MAX_NAME_UNITS);
    assert!(clean.ends_with(".png") && clean.starts_with('\u{1F600}'));
    // An absurd "extension" is part of the name, not preserved.
    let odd = format!("x.{}", "e".repeat(300));
    assert!(utf16_len(&sanitize_file_name(&odd)) <= MAX_NAME_UNITS);
}

#[test]
fn name_budget_shrinks_for_deep_folders_but_stays_usable() {
    assert_eq!(name_budget(Path::new(r"C:\Downloads")), MAX_NAME_UNITS);
    let deep = PathBuf::from(format!(r"C:\{}", "d".repeat(200)));
    let budget = name_budget(&deep);
    assert!(budget < MAX_NAME_UNITS && budget >= 24);
    let huge = PathBuf::from(format!(r"C:\{}", "d".repeat(400)));
    assert_eq!(name_budget(&huge), 24);
}

#[test]
fn extensions_split_like_chrome() {
    assert_eq!(split_extension("report.pdf"), ("report", ".pdf"));
    assert_eq!(split_extension("my.file.v2.zip"), ("my.file.v2", ".zip"));
    assert_eq!(split_extension("README"), ("README", ""));
    assert_eq!(split_extension(".gitignore"), (".gitignore", ""));
    assert_eq!(split_extension("archive.tar.gz"), ("archive", ".tar.gz"));
    assert_eq!(split_extension("ARCHIVE.TAR.BZ2"), ("ARCHIVE", ".TAR.BZ2"));
    assert_eq!(split_extension("data.gz"), ("data", ".gz"));
    assert_eq!(split_extension("v1. final"), ("v1. final", ""));
    assert_eq!(split_extension("trailing."), ("trailing.", ""));
    assert_eq!(split_extension("é.tar.gz"), ("é", ".tar.gz"));
}

fn taken_set(names: &[&str]) -> impl Fn(&Path) -> bool {
    let set: Vec<PathBuf> = names.iter().map(|n| PathBuf::from(r"C:\d").join(n)).collect();
    move |p: &Path| set.iter().any(|s| s == p)
}

#[test]
fn a_free_name_is_used_as_is_and_a_clash_gets_chromes_counter() {
    let dir = Path::new(r"C:\d");
    assert_eq!(unique_path(dir, "a.txt", &taken_set(&[])).unwrap(), dir.join("a.txt"));
    assert_eq!(unique_path(dir, "a.txt", &taken_set(&["a.txt"])).unwrap(), dir.join("a (1).txt"));
    assert_eq!(unique_path(dir, "a.txt", &taken_set(&["a.txt", "a (1).txt", "a (2).txt"])).unwrap(), dir.join("a (3).txt"));
    // A gap is filled.
    assert_eq!(unique_path(dir, "a.txt", &taken_set(&["a.txt", "a (2).txt"])).unwrap(), dir.join("a (1).txt"));
}

#[test]
fn counters_go_before_the_last_extension_only() {
    let dir = Path::new(r"C:\d");
    assert_eq!(unique_path(dir, "my.file.zip", &taken_set(&["my.file.zip"])).unwrap(), dir.join("my.file (1).zip"));
    assert_eq!(unique_path(dir, "README", &taken_set(&["README"])).unwrap(), dir.join("README (1)"));
    assert_eq!(unique_path(dir, ".env", &taken_set(&[".env"])).unwrap(), dir.join(".env (1)"));
    assert_eq!(unique_path(dir, "src.tar.gz", &taken_set(&["src.tar.gz"])).unwrap(), dir.join("src (1).tar.gz"));
    // Chrome does not look inside an existing counter.
    assert_eq!(unique_path(dir, "a (1).txt", &taken_set(&["a (1).txt"])).unwrap(), dir.join("a (1) (1).txt"));
}

#[test]
fn a_long_name_still_fits_after_the_counter() {
    let dir = Path::new(r"C:\d");
    let name = format!("{}.pdf", "n".repeat(MAX_NAME_UNITS));
    let first = unique_path(dir, &name, &|_| false).unwrap();
    let taken = first.clone();
    let second = unique_path(dir, &name, &|p| p == taken).unwrap();
    assert_ne!(first, second);
    let file = second.file_name().unwrap().to_string_lossy().into_owned();
    assert!(file.ends_with(" (1).pdf") && utf16_len(&file) <= name_budget(dir));
}

#[test]
fn an_endless_clash_gives_up_instead_of_looping() {
    assert_eq!(unique_path(Path::new(r"C:\d"), "a.txt", &|_| true), None);
}

#[test]
fn the_plan_asks_or_saves_to_a_unique_sanitised_name() {
    let dir = Path::new(r"C:\d");
    let taken = taken_set(&["a.txt"]);
    assert_eq!(plan_destination(dir, false, "a.txt", &taken), DownloadPlan::Save { path: dir.join("a (1).txt") });
    assert_eq!(plan_destination(dir, false, "x/con.txt", &taken), DownloadPlan::Save { path: dir.join("_con.txt") });
    assert_eq!(
        plan_destination(dir, true, "a.txt", &taken),
        DownloadPlan::Ask { dir: dir.to_path_buf(), name: "a.txt".to_owned() },
        "the dialog confirms overwrites itself, so the plain name is offered"
    );
    assert_eq!(plan_destination(dir, false, "a.txt", &|_| true), DownloadPlan::Cancel);
}

#[test]
fn the_download_folder_is_the_setting_when_usable_else_the_default() {
    let tmp = TempDir::new("resolve");
    let default = tmp.path("default");
    let custom = tmp.path("custom").join("nested");
    assert_eq!(resolve_dir("", Some(&default)).unwrap(), default);
    assert!(default.is_dir(), "the default folder is created");
    assert_eq!(resolve_dir(&format!("  {}  ", custom.display()), Some(&default)).unwrap(), custom);
    assert!(custom.is_dir(), "a custom folder is created");
    assert_eq!(resolve_dir("relative\\folder", Some(&default)).unwrap(), default, "relative paths are not trusted");
    let file = tmp.path("a-file");
    std::fs::write(&file, b"x").unwrap();
    assert_eq!(resolve_dir(&file.to_string_lossy(), Some(&default)).unwrap(), default, "a file is not a folder");
    assert!(resolve_dir("", None).is_err());
    assert!(resolve_dir(&file.to_string_lossy(), Some(&file)).is_err());
}

#[test]
fn copy_link_text_is_http_only_and_without_credentials() {
    assert_eq!(copy_link_text("https://example.com/a?b=1").as_deref(), Some("https://example.com/a?b=1"));
    assert_eq!(copy_link_text("https://user:pw@example.com/a").as_deref(), Some("https://example.com/a"));
    assert_eq!(copy_link_text("javascript:alert(1)"), None);
    assert_eq!(copy_link_text("blob:https://example.com/uuid"), None);
    assert_eq!(copy_link_text("file:///C:/x"), None);
    assert_eq!(copy_link_text(""), None);
}

// --- events, progress, speed ---

#[test]
fn ids_start_at_one_and_only_grow() {
    let mut m = DownloadManager::in_memory(false);
    assert_eq!(start(&mut m, 10, "a.zip", 100, 1000), 1);
    assert_eq!(start(&mut m, 11, "b.zip", 100, 1000), 2);
    finish(&mut m, 11, 100, 100, 2000);
    assert!(m.remove(2));
    assert_eq!(start(&mut m, 12, "c.zip", 100, 3000), 3, "a removed id is never reused");
    assert_eq!(m.next_id(), 4);
}

#[test]
fn started_records_a_clean_entry() {
    let mut m = DownloadManager::in_memory(false);
    let id = start(&mut m, 1, "Movie.mp4", 5000, NOON_MS);
    let e = m.get(id).unwrap();
    assert_eq!((e.state, e.got, e.total, e.name.as_str()), (State::Active, 0, 5000, "Movie.mp4"));
    assert_eq!(e.url, "https://www.example.com/files/Movie.mp4");
    assert_eq!(e.host(), "example.com");
    assert_eq!(e.started, NOON_MS / 1000);
    assert_eq!(e.ended, 0);
    // A repeated Started for the same operation is ignored.
    let again = m.on_event(DownloadEvent::Started { key: 1, url: "https://x.test/".into(), path: path("z"), total: 1 }, 1);
    assert_eq!(again, Change::None);
    assert_eq!(m.entries().len(), 1);
}

#[test]
fn non_http_sources_are_not_recorded() {
    let mut m = DownloadManager::in_memory(false);
    m.on_event(DownloadEvent::Started { key: 1, url: "blob:https://a.test/uuid".into(), path: path("a.bin"), total: -1 }, 5);
    m.on_event(
        DownloadEvent::Started { key: 2, url: "https://u:p@a.test/x".into(), path: path("b.bin"), total: -1 },
        5,
    );
    assert_eq!(m.get(1).unwrap().url, "");
    assert_eq!(m.get(1).unwrap().host(), "");
    assert_eq!(m.get(2).unwrap().url, "https://a.test/x", "credentials are dropped");
    assert!(!m.allowed(1, Action::CopyLink) && !m.allowed(1, Action::Retry));
}

#[test]
fn speed_is_smoothed_and_eta_follows_it() {
    let mut m = DownloadManager::in_memory(false);
    let id = start(&mut m, 1, "a.bin", 10_000_000, 1_000);
    // 1 MB/s for the first two seconds.
    assert_eq!(progress(&mut m, 1, 1_000_000, 10_000_000, 2_000), Change::Progress);
    assert_eq!(m.get(id).unwrap().bps(), 1_000_000);
    assert_eq!(progress(&mut m, 1, 2_000_000, 10_000_000, 3_000), Change::Progress);
    assert_eq!(m.get(id).unwrap().bps(), 1_000_000);
    // A sudden 4 MB/s second moves the figure only part of the way (time constant 3 s).
    progress(&mut m, 1, 6_000_000, 10_000_000, 4_000);
    let bps = m.get(id).unwrap().bps();
    assert!(bps > 1_000_000 && bps < 2_000_000, "{bps}");
    let eta = m.get(id).unwrap().eta();
    assert_eq!(eta, eta_secs(6_000_000, 10_000_000, bps));
    assert!(eta > 0);
}

#[test]
fn bursts_of_tiny_samples_do_not_move_the_speed() {
    let mut m = DownloadManager::in_memory(false);
    let id = start(&mut m, 1, "a.bin", 1_000_000, 1_000);
    progress(&mut m, 1, 100_000, 1_000_000, 2_000);
    let before = m.get(id).unwrap().bps();
    // 10 ms later with a big jump: folded into the next real sample, not an instant 10 MB/s.
    progress(&mut m, 1, 200_000, 1_000_000, 2_010);
    assert_eq!(m.get(id).unwrap().bps(), before);
    assert_eq!(m.get(id).unwrap().got, 200_000, "bytes still advance");
}

#[test]
fn eta_is_unknown_without_a_size_or_a_speed_and_zero_when_done() {
    assert_eq!(eta_secs(0, -1, 100), -1);
    assert_eq!(eta_secs(0, 0, 100), -1);
    assert_eq!(eta_secs(0, 100, 0), -1);
    assert_eq!(eta_secs(0, 1000, 300), 4, "rounds up");
    assert_eq!(eta_secs(1000, 1000, 300), 0);
    assert_eq!(eta_secs(2000, 1000, 300), 0);
    let mut m = DownloadManager::in_memory(false);
    let id = start(&mut m, 1, "a.bin", -1, 1_000);
    progress(&mut m, 1, 500, -1, 2_000);
    assert_eq!(m.get(id).unwrap().eta(), -1);
    assert_eq!(m.get(id).unwrap().total, -1);
}

#[test]
fn progress_that_changes_nothing_is_not_a_change() {
    let mut m = DownloadManager::in_memory(false);
    start(&mut m, 1, "a.bin", 100, 1_000);
    progress(&mut m, 1, 10, 100, 1_500);
    let revision = m.revision();
    assert_eq!(progress(&mut m, 1, 10, 100, 1_600), Change::None);
    assert_eq!(m.revision(), revision);
    assert_eq!(progress(&mut m, 99, 10, 100, 1_700), Change::None, "an unknown operation is ignored");
}

#[test]
fn pause_and_resume_follow_the_engine_and_reset_the_speed() {
    let mut m = DownloadManager::in_memory(false);
    let id = start(&mut m, 1, "a.bin", 100, 1_000);
    progress(&mut m, 1, 20, 100, 2_000);
    set_state(&mut m, 1, OpState::Paused, 2_500);
    let e = m.get(id).unwrap();
    assert_eq!((e.state, e.resume, e.bps(), e.eta()), (State::Paused, true, 0, -1));
    assert_eq!(progress(&mut m, 1, 30, 100, 3_000), Change::None, "progress while paused is ignored");
    set_state(&mut m, 1, OpState::InProgress, 4_000);
    let e = m.get(id).unwrap();
    assert_eq!((e.state, e.resume), (State::Active, false));
    assert_eq!(e.bps(), 0, "speed starts over after a resume");
}

#[test]
fn completion_fills_in_the_size_and_marks_it_unseen() {
    let mut m = DownloadManager::in_memory(false);
    let id = start(&mut m, 1, "a.bin", 100, NOON_MS);
    finish(&mut m, 1, 97, 100, NOON_MS + 5_000);
    let e = m.get(id).unwrap();
    assert_eq!((e.state, e.got, e.total, e.ended, e.op, e.unseen), (State::Done, 100, 100, NOON_MS / 1000 + 5, None, true));
    // Unknown size becomes the received size.
    let id = start(&mut m, 2, "b.bin", -1, NOON_MS);
    finish(&mut m, 2, 4242, -1, NOON_MS);
    assert_eq!((m.get(id).unwrap().got, m.get(id).unwrap().total), (4242, 4242));
}

#[test]
fn failures_keep_a_bounded_reason_and_resumability() {
    let mut m = DownloadManager::in_memory(false);
    let id = start(&mut m, 1, "a.bin", 100, 1_000);
    fail(&mut m, 1, "Network error", true, 2_000);
    let e = m.get(id).unwrap();
    assert_eq!((e.state, e.err.as_str(), e.resume, e.op), (State::Failed, "Network error", true, Some(1)));
    let id2 = start(&mut m, 2, "b.bin", 100, 1_000);
    fail(&mut m, 2, &"x".repeat(200), false, 2_000);
    let e = m.get(id2).unwrap();
    assert_eq!((e.err.chars().count(), e.resume, e.op), (60, false, None));
    let id3 = start(&mut m, 3, "c.bin", 100, 1_000);
    fail(&mut m, 3, "  ", false, 2_000);
    assert_eq!(m.get(id3).unwrap().err, "Failed");
}

#[test]
fn cancelling_is_not_unseen_and_drops_the_handle() {
    let mut m = DownloadManager::in_memory(false);
    let id = start(&mut m, 1, "a.bin", 100, 1_000);
    set_state(&mut m, 1, OpState::Cancelled, 2_000);
    let e = m.get(id).unwrap();
    assert_eq!((e.state, e.op, e.unseen), (State::Cancelled, None, false));
    assert!(!m.snapshot_for_toolbar().on);
}

// --- toolbar ---

#[test]
fn the_toolbar_is_hidden_with_nothing_to_show() {
    let m = DownloadManager::in_memory(false);
    let t = m.snapshot_for_toolbar();
    assert_eq!((t.on, t.state, t.n, t.pct, t.burst), (false, ButtonState::Idle, 0, -1, 0));
    assert_eq!(t.json(), r#"{"v":1,"t":"dlbtn","on":false,"state":"idle","n":0,"pct":-1,"burst":0}"#);
}

#[test]
fn the_toolbar_aggregates_active_downloads() {
    let mut m = DownloadManager::in_memory(false);
    start(&mut m, 1, "a.bin", 1000, 1_000);
    start(&mut m, 2, "b.bin", 3000, 1_000);
    progress(&mut m, 1, 500, 1000, 1_500);
    progress(&mut m, 2, 500, 3000, 1_500);
    let t = m.snapshot_for_toolbar();
    assert_eq!((t.on, t.state, t.n, t.pct, t.burst), (true, ButtonState::Active, 2, 25, 2));
    assert_eq!(t.json(), r#"{"v":1,"t":"dlbtn","on":true,"state":"active","n":2,"pct":25,"burst":2}"#);
}

#[test]
fn the_toolbar_percentage_ignores_unknown_sizes_and_is_unknown_when_none_is_known() {
    let mut m = DownloadManager::in_memory(false);
    start(&mut m, 1, "a.bin", -1, 1_000);
    assert_eq!(m.snapshot_for_toolbar().pct, -1);
    start(&mut m, 2, "b.bin", 200, 1_000);
    progress(&mut m, 2, 100, 200, 1_500);
    assert_eq!(m.snapshot_for_toolbar().pct, 50);
    progress(&mut m, 2, 999, 200, 1_800);
    assert_eq!(m.snapshot_for_toolbar().pct, 100, "never over 100");
}

#[test]
fn paused_shows_only_when_nothing_is_active() {
    let mut m = DownloadManager::in_memory(false);
    start(&mut m, 1, "a.bin", 100, 1_000);
    start(&mut m, 2, "b.bin", 100, 1_000);
    set_state(&mut m, 1, OpState::Paused, 2_000);
    assert_eq!(m.snapshot_for_toolbar().state, ButtonState::Active);
    set_state(&mut m, 2, OpState::Paused, 2_000);
    let t = m.snapshot_for_toolbar();
    assert_eq!((t.state, t.n, t.on), (ButtonState::Paused, 2, true));
}

#[test]
fn a_finished_download_keeps_the_button_until_it_is_seen() {
    let mut m = DownloadManager::in_memory(false);
    start(&mut m, 1, "a.bin", 100, 1_000);
    finish(&mut m, 1, 100, 100, 2_000);
    let t = m.snapshot_for_toolbar();
    assert_eq!((t.on, t.state, t.n), (true, ButtonState::Done, 0));
    assert!(m.mark_seen());
    assert!(!m.snapshot_for_toolbar().on);
    assert!(!m.mark_seen(), "nothing left to mark");
}

#[test]
fn the_latest_unseen_outcome_picks_done_or_failed() {
    let mut m = DownloadManager::in_memory(false);
    start(&mut m, 1, "a.bin", 100, 1_000);
    start(&mut m, 2, "b.bin", 100, 1_000);
    fail(&mut m, 1, "Network error", false, 5_000);
    finish(&mut m, 2, 100, 100, 9_000);
    assert_eq!(m.snapshot_for_toolbar().state, ButtonState::Done, "the later outcome wins");
    start(&mut m, 3, "c.bin", 100, 10_000);
    fail(&mut m, 3, "Disk full", false, 12_000);
    assert_eq!(m.snapshot_for_toolbar().state, ButtonState::Failed);
    m.mark_seen();
    assert_eq!(m.snapshot_for_toolbar().state, ButtonState::Idle);
}

#[test]
fn a_download_that_finishes_after_the_bubble_opened_is_unseen_again() {
    let mut m = DownloadManager::in_memory(false);
    start(&mut m, 1, "a.bin", 100, 1_000);
    m.mark_seen(); // the bubble opened while it was running
    assert_eq!(m.snapshot_for_toolbar().state, ButtonState::Active);
    finish(&mut m, 1, 100, 100, 2_000);
    assert_eq!(m.snapshot_for_toolbar().state, ButtonState::Done);
}

#[test]
fn burst_counts_every_start() {
    let mut m = DownloadManager::in_memory(false);
    for key in 1..=3 {
        start(&mut m, key, &format!("{key}.bin"), 10, 1_000);
        assert_eq!(m.snapshot_for_toolbar().burst, key as u32);
    }
}

// --- bubble ---

#[test]
fn the_bubble_lists_active_first_then_newest_and_counts_the_rest() {
    let mut m = DownloadManager::in_memory(false);
    for key in 1..=8 {
        start(&mut m, key, &format!("f{key}.bin"), 100, key * 1_000);
    }
    for key in [2, 3, 4, 5, 6, 7, 8] {
        finish(&mut m, key, 100, 100, 20_000);
    }
    // 1 is still active (oldest); the others are done.
    let (rows, more) = m.bubble_rows(BUBBLE_ROWS);
    let ids: Vec<u32> = rows.iter().map(|e| e.id).collect();
    assert_eq!(ids, vec![1, 8, 7, 6, 5]);
    assert_eq!(more, 3);
    let json = m.snapshot_for_bubble(7, true, BUBBLE_ROWS, time());
    assert!(json.starts_with(r#"{"v":1,"t":"downloads","tok":7,"auto":true,"items":[{"id":1,"#), "{json}");
    assert!(json.ends_with(r#""more":3,"active":1}"#), "{json}");
}

#[test]
fn the_entry_object_has_exactly_the_documented_keys_in_order() {
    let mut m = DownloadManager::in_memory(false);
    let id = start(&mut m, 1, "My \"file\".zip", 1000, NOON_MS);
    progress(&mut m, 1, 250, 1000, NOON_MS + 1_000);
    let json = m.entry_json(id, Time { now: NOON_MS / 1000, offset_secs: 3600 }).unwrap();
    assert_eq!(
        json,
        concat!(
            r#"{"id":1,"name":"My \"file\".zip","host":"example.com","url":"https://www.example.com/files/My%20%22file%22.zip","#,
            r#""state":"active","got":250,"total":1000,"eta":3,"bps":250,"err":"","resume":false,"clock":"13:00"}"#
        )
    );
    assert_eq!(m.entry_json(99, time()), None);
}

#[test]
fn entry_fields_are_capped_and_err_only_shows_for_failed() {
    let mut m = DownloadManager::in_memory(false);
    let long_host = "h".repeat(150);
    m.on_event(
        DownloadEvent::Started {
            key: 1,
            url: format!("https://{long_host}.example.com/{}", "p".repeat(500)),
            path: path("a.bin"),
            total: 1,
        },
        NOON_MS,
    );
    let json = m.entry_json(1, time()).unwrap();
    let value = |key: &str| -> String {
        let at = json.find(&format!("\"{key}\":\"")).unwrap() + key.len() + 4;
        json[at..].split('"').next().unwrap().to_owned()
    };
    assert_eq!(value("url").chars().count(), 200);
    assert!(value("host").chars().count() <= 120);
    finish(&mut m, 1, 1, 1, NOON_MS);
    assert_eq!(value_of(&m, "err"), "");
    assert_eq!(value_of(&m, "state"), "done");
    m.refresh_missing(&|_| false);
    assert_eq!(value_of(&m, "state"), "missing");
}

fn value_of(m: &DownloadManager, key: &str) -> String {
    let json = m.entry_json(1, time()).unwrap();
    let at = json.find(&format!("\"{key}\":\"")).unwrap() + key.len() + 4;
    json[at..].split('"').next().unwrap().to_owned()
}

// --- page ---

#[test]
fn the_page_groups_by_local_day_with_today_and_yesterday_labels() {
    let mut m = DownloadManager::in_memory(false);
    let times = [D0 + 3600, D0 - 3600, D0 - 3 * DAY + 100, D0 + 7200];
    for (key, started) in times.iter().enumerate() {
        start(&mut m, key as u64 + 1, &format!("f{key}.bin"), 10, started * 1000);
        finish(&mut m, key as u64 + 1, 10, 10, started * 1000 + 1000);
    }
    let json = m.snapshot_for_page("", Ack::None, time());
    assert!(json.starts_with(r#"{"v":1,"t":"snap","p":"downloadspage","d":{"state":"on","query":"","rows":["#), "{json}");
    let a = json.find(r#""label":"Today","date":"2025-10-14","count":2"#).expect("today");
    let b = json.find(r#""label":"Yesterday","date":"2025-10-13","count":1"#).expect("yesterday");
    let c = json.find(r#""label":"2025-10-11","date":"2025-10-11","count":1"#).expect("old day");
    assert!(a < b && b < c);
    assert!(json.ends_with(r#""more":false,"undo":false,"ack":"none","active":0}}"#), "{json}");
    // Newest first inside a day: f3 (02:00) before f0 (01:00).
    assert!(json.find("f3.bin").unwrap() < json.find("f0.bin").unwrap());
}

#[test]
fn the_local_offset_moves_a_late_download_into_the_local_day() {
    let mut m = DownloadManager::in_memory(false);
    // 23:30 UTC on the 13th is 00:30 on the 14th at UTC+1.
    let started = D0 - 1800;
    start(&mut m, 1, "late.bin", 10, started * 1000);
    let utc = m.snapshot_for_page("", Ack::None, time());
    assert!(utc.contains(r#""label":"Yesterday""#));
    let local = m.snapshot_for_page("", Ack::None, Time { now: D0 + 12 * 3600, offset_secs: 3600 });
    assert!(local.contains(r#""label":"Today""#) && local.contains(r#""clock":"00:30""#), "{local}");
}

#[test]
fn search_matches_name_and_url_case_insensitively_newest_first() {
    let mut m = DownloadManager::in_memory(false);
    start(&mut m, 1, "Holiday Photos.zip", 10, 1_000_000);
    start(&mut m, 2, "invoice.pdf", 10, 2_000_000);
    m.on_event(
        DownloadEvent::Started { key: 3, url: "https://cdn.Photos-R-Us.com/x".into(), path: path("x.bin"), total: 1 },
        3_000_000,
    );
    let names = |q: &str| m.search(q).iter().map(|e| e.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(""), vec!["x.bin", "invoice.pdf", "Holiday Photos.zip"]);
    assert_eq!(names("PHOTOS"), vec!["x.bin", "Holiday Photos.zip"], "name or url");
    assert_eq!(names("  invoice "), vec!["invoice.pdf"]);
    assert!(names("nothing like this").is_empty());
    let json = m.snapshot_for_page("PHOTOS", Ack::None, time());
    assert!(json.contains(r#""query":"PHOTOS""#) && !json.contains("invoice.pdf"));
}

#[test]
fn the_page_reports_an_unreadable_file_and_guest_windows() {
    let tmp = TempDir::new("states");
    let file = tmp.path("downloads.txt");
    std::fs::create_dir(&file).unwrap(); // a folder where the file should be: cannot be read
    let m = DownloadManager::open(&file);
    assert_eq!(m.list_state(), ListState::Error);
    assert!(m.snapshot_for_page("", Ack::None, time()).contains(r#""state":"error","query":"","rows":[]"#));
    let guest = DownloadManager::in_memory(true);
    assert!(guest.snapshot_for_page("", Ack::Cleared, time()).contains(r#""state":"guest""#));
    assert!(guest.snapshot_for_page("", Ack::Cleared, time()).contains(r#""ack":"cleared""#));
}

#[test]
fn a_huge_list_is_shrunk_to_fit_the_page_budget() {
    let mut m = DownloadManager::in_memory(false);
    for key in 1..=MAX_ENTRIES as u64 {
        m.on_event(
            DownloadEvent::Started {
                key,
                url: format!("https://example.com/{}", "\u{20ac}".repeat(60)),
                path: path(&format!("{}{key}.bin", "\u{20ac}".repeat(140))),
                total: 1,
            },
            key * 1000,
        );
        finish(&mut m, key, 1, 1, key * 1000);
    }
    let json = m.snapshot_for_page("", Ack::None, time());
    assert!(json.len() <= MAX_SNAPSHOT_BYTES, "{}", json.len());
    assert!(m.entries().len() == MAX_ENTRIES);
}

#[test]
fn over_long_queries_are_cut() {
    let m = DownloadManager::in_memory(false);
    let json = m.snapshot_for_page(&"q".repeat(1000), Ack::None, time());
    assert!(json.contains(&format!(r#""query":"{}""#, "q".repeat(MAX_QUERY_CHARS))));
}

// --- actions ---

#[test]
fn action_codes_match_the_wire() {
    use Action::*;
    let all = [Pause, Resume, Cancel, Open, Show, Remove, CopyLink, Retry, OpenPage, OpenFolder, ClearAll];
    for (n, action) in all.into_iter().enumerate() {
        assert_eq!(Action::from_code(n as u8), Some(action));
    }
    assert_eq!(Action::from_code(11), None);
    assert_eq!(Action::from_code(255), None);
}

fn legal(m: &DownloadManager, id: u32) -> Vec<Action> {
    use Action::*;
    [Pause, Resume, Cancel, Open, Show, Remove, CopyLink, Retry].into_iter().filter(|a| m.allowed(id, *a)).collect()
}

#[test]
fn each_state_allows_exactly_its_actions() {
    use Action::*;
    let mut m = DownloadManager::in_memory(false);
    let active = start(&mut m, 1, "a.bin", 100, 1_000);
    assert_eq!(legal(&m, active), vec![Pause, Cancel, CopyLink]);
    set_state(&mut m, 1, OpState::Paused, 2_000);
    assert_eq!(legal(&m, active), vec![Resume, Cancel, CopyLink]);

    let done = start(&mut m, 2, "b.bin", 100, 1_000);
    finish(&mut m, 2, 100, 100, 2_000);
    assert_eq!(legal(&m, done), vec![Open, Show, Remove, CopyLink]);
    m.refresh_missing(&|_| false);
    assert_eq!(legal(&m, done), vec![Remove, CopyLink], "a missing file cannot be opened or shown");

    let resumable = start(&mut m, 3, "c.bin", 100, 1_000);
    fail(&mut m, 3, "Network error", true, 2_000);
    assert_eq!(legal(&m, resumable), vec![Resume, Remove, CopyLink, Retry]);
    let dead = start(&mut m, 4, "d.bin", 100, 1_000);
    fail(&mut m, 4, "Blocked", false, 2_000);
    assert_eq!(legal(&m, dead), vec![Remove, CopyLink, Retry]);
    let cancelled = start(&mut m, 5, "e.bin", 100, 1_000);
    set_state(&mut m, 5, OpState::Cancelled, 2_000);
    assert_eq!(legal(&m, cancelled), vec![Remove, CopyLink, Retry]);
}

#[test]
fn list_level_actions_need_id_zero_and_entry_actions_need_a_live_id() {
    let mut m = DownloadManager::in_memory(false);
    let id = start(&mut m, 1, "a.bin", 100, 1_000);
    for action in [Action::OpenPage, Action::OpenFolder, Action::ClearAll] {
        assert!(m.allowed(0, action));
        assert!(!m.allowed(id, action));
    }
    assert_eq!(m.act(0, Action::OpenPage), Outcome::OpenPage);
    assert_eq!(m.act(0, Action::OpenFolder), Outcome::OpenFolder);
    assert_eq!(m.act(id, Action::OpenPage), Outcome::Rejected);
    assert_eq!(m.act(0, Action::Pause), Outcome::Rejected, "id 0 is no entry");
    assert_eq!(m.act(77, Action::Pause), Outcome::Rejected, "unknown id");
    assert_eq!(m.act(id, Action::Open), Outcome::Rejected, "not finished");
    assert_eq!(m.act(id, Action::Remove), Outcome::Rejected, "still running");
}

#[test]
fn act_returns_what_to_do_from_recorded_data_only() {
    let mut m = DownloadManager::in_memory(false);
    let id = start(&mut m, 42, "a.bin", 100, 1_000);
    assert_eq!(m.act(id, Action::Pause), Outcome::Engine { key: 42, command: EngineCommand::Pause });
    assert_eq!(m.act(id, Action::Cancel), Outcome::Engine { key: 42, command: EngineCommand::Cancel });
    assert_eq!(m.act(id, Action::CopyLink), Outcome::CopyLink("https://www.example.com/files/a.bin".into()));
    set_state(&mut m, 42, OpState::Paused, 2_000);
    assert_eq!(m.act(id, Action::Resume), Outcome::Engine { key: 42, command: EngineCommand::Resume });
    finish(&mut m, 42, 100, 100, 3_000);
    assert_eq!(m.act(id, Action::Open), Outcome::OpenFile(path("a.bin")));
    assert_eq!(m.act(id, Action::Show), Outcome::ShowInFolder(path("a.bin")));
    let failed = start(&mut m, 43, "b.bin", 100, 1_000);
    fail(&mut m, 43, "Blocked", false, 2_000);
    assert_eq!(m.act(failed, Action::Retry), Outcome::Retry("https://www.example.com/files/b.bin".into()));
}

#[test]
fn removal_clear_and_undo() {
    let mut m = DownloadManager::in_memory(false);
    let a = start(&mut m, 1, "a.bin", 10, 1_000);
    let b = start(&mut m, 2, "b.bin", 10, 2_000);
    let c = start(&mut m, 3, "c.bin", 10, 3_000);
    finish(&mut m, 1, 10, 10, 4_000);
    finish(&mut m, 2, 10, 10, 4_000);
    assert!(!m.can_undo());
    assert_eq!(m.act(b, Action::Remove), Outcome::Changed);
    assert!(m.get(b).is_none() && m.can_undo());
    assert_eq!(m.undo(), 1);
    assert_eq!(m.entries().iter().map(|e| e.id).collect::<Vec<_>>(), vec![a, b, c], "restored in place");
    assert!(!m.can_undo());
    assert_eq!(m.undo(), 0);

    assert_eq!(m.act(0, Action::ClearAll), Outcome::Changed);
    assert_eq!(m.entries().iter().map(|e| e.id).collect::<Vec<_>>(), vec![c], "a running download survives Clear all");
    assert_eq!(m.undo(), 2);
    assert_eq!(m.entries().len(), 3);
    assert!(!m.remove(c), "a running download cannot be removed");
    assert!(!m.remove(99));
}

#[test]
fn a_new_removal_replaces_the_undo_slot() {
    let mut m = DownloadManager::in_memory(false);
    start(&mut m, 1, "a.bin", 10, 1_000);
    start(&mut m, 2, "b.bin", 10, 1_000);
    finish(&mut m, 1, 10, 10, 2_000);
    finish(&mut m, 2, 10, 10, 2_000);
    m.remove(1);
    m.remove(2);
    assert_eq!(m.undo(), 1);
    assert!(m.get(2).is_some() && m.get(1).is_none());
}

#[test]
fn removing_a_resumable_failure_releases_its_engine_handle() {
    let mut m = DownloadManager::in_memory(false);
    let id = start(&mut m, 9, "a.bin", 100, 1_000);
    fail(&mut m, 9, "Network error", true, 2_000);
    assert!(m.take_released().is_empty());
    m.remove(id);
    assert_eq!(m.take_released(), vec![9]);
    assert!(m.take_released().is_empty());
    m.undo();
    assert_eq!(m.get(id).unwrap().op, None);
    assert!(!m.get(id).unwrap().resume, "a restored failure cannot resume");
}

#[test]
fn trimming_keeps_the_newest_and_never_a_running_download() {
    let mut m = DownloadManager::in_memory(false);
    start(&mut m, 1, "running.bin", 10, 1_000); // oldest, still running
    for key in 2..=(MAX_ENTRIES as u64 + 5) {
        start(&mut m, key, &format!("{key}.bin"), 10, key * 1_000);
        finish(&mut m, key, 10, 10, key * 1_000);
    }
    assert_eq!(m.entries().len(), MAX_ENTRIES);
    assert!(m.get(1).is_some(), "the running one is kept");
    assert!(m.get(2).is_none() && m.get(6).is_none(), "the oldest finished ones went");
}

#[test]
fn files_that_vanish_read_missing_and_come_back() {
    let mut m = DownloadManager::in_memory(false);
    let id = start(&mut m, 1, "a.bin", 10, 1_000);
    let running = start(&mut m, 2, "b.bin", 10, 1_000);
    finish(&mut m, 1, 10, 10, 2_000);
    assert!(!m.refresh_missing(&|_| true));
    assert!(m.refresh_missing(&|_| false));
    assert_eq!(m.get(id).unwrap().state_key(), "missing");
    assert_eq!(m.get(running).unwrap().state_key(), "active", "only finished downloads are checked");
    assert!(!m.refresh_missing(&|_| false), "no change, no revision");
    assert!(m.refresh_missing(&|_| true));
    assert_eq!(m.get(id).unwrap().state_key(), "done");
}

#[test]
fn the_file_check_is_throttled() {
    let mut m = DownloadManager::in_memory(false);
    start(&mut m, 1, "a.bin", 10, 1_000);
    finish(&mut m, 1, 10, 10, 2_000);
    assert!(m.refresh_missing_if_stale(10_000, &|_| false));
    assert!(!m.refresh_missing_if_stale(10_500, &|_| true), "too soon");
    assert!(m.refresh_missing_if_stale(12_500, &|_| true));
}

#[test]
fn a_path_in_progress_is_in_use_case_insensitively() {
    let mut m = DownloadManager::in_memory(false);
    start(&mut m, 1, "Report.PDF", 10, 1_000);
    assert!(m.path_in_use(&path("report.pdf")));
    finish(&mut m, 1, 10, 10, 2_000);
    assert!(!m.path_in_use(&path("report.pdf")), "a finished file is for the disk check");
}

#[test]
fn the_revision_grows_with_every_change() {
    let mut m = DownloadManager::in_memory(false);
    let r0 = m.revision();
    start(&mut m, 1, "a.bin", 100, 1_000);
    let r1 = m.revision();
    progress(&mut m, 1, 50, 100, 2_000);
    let r2 = m.revision();
    finish(&mut m, 1, 100, 100, 3_000);
    let r3 = m.revision();
    m.mark_seen();
    assert!(r0 < r1 && r1 < r2 && r2 < r3 && r3 < m.revision());
}

#[test]
fn closing_a_window_interrupts_what_is_running() {
    let mut m = DownloadManager::in_memory(false);
    let a = start(&mut m, 1, "a.bin", 100, 1_000);
    let b = start(&mut m, 2, "b.bin", 100, 1_000);
    start(&mut m, 3, "c.bin", 100, 1_000);
    set_state(&mut m, 2, OpState::Paused, 2_000);
    finish(&mut m, 3, 100, 100, 2_000);
    let mut keys = m.shutdown(5_000_000);
    keys.sort();
    assert_eq!(keys, vec![1, 2]);
    for id in [a, b] {
        let e = m.get(id).unwrap();
        assert_eq!((e.state, e.err.as_str(), e.resume, e.op, e.ended), (State::Failed, "Interrupted", false, None, 5_000));
    }
    assert_eq!(m.in_progress_count(), 0);
}

#[test]
fn a_closed_webview_fails_only_its_downloads() {
    let mut m = DownloadManager::in_memory(false);
    let a = start(&mut m, 1, "a.bin", 100, 1_000);
    let b = start(&mut m, 2, "b.bin", 100, 1_000);
    assert_eq!(m.interrupt_keys(&[1, 55], 9_000), Change::State);
    assert_eq!(m.get(a).unwrap().err, "Interrupted");
    assert_eq!(m.get(b).unwrap().state, State::Active);
    assert_eq!(m.interrupt_keys(&[55], 9_000), Change::None);
}

// --- persistence ---

fn sample_file(tmp: &TempDir) -> PathBuf {
    tmp.path("downloads.txt")
}

#[test]
fn a_missing_file_is_an_empty_list_that_is_created_on_the_first_change() {
    let tmp = TempDir::new("fresh");
    let file = sample_file(&tmp);
    let mut m = DownloadManager::open(&file);
    assert_eq!((m.list_state(), m.entries().len(), m.next_id()), (ListState::On, 0, 1));
    assert!(!file.exists(), "reading never writes");
    start(&mut m, 1, "a.bin", 100, NOON_MS);
    assert!(file.exists());
}

#[test]
fn the_list_survives_a_restart_with_its_fields() {
    let tmp = TempDir::new("roundtrip");
    let file = sample_file(&tmp);
    let mut m = DownloadManager::open(&file);
    start(&mut m, 1, "tab\tname.zip", 500, NOON_MS);
    finish(&mut m, 1, 500, 500, NOON_MS + 9_000);
    let b = start(&mut m, 2, "b.bin", -1, NOON_MS + 1_000);
    fail(&mut m, 2, "Disk full", true, NOON_MS + 2_000);
    let c = start(&mut m, 3, "c.bin", 10, NOON_MS + 3_000);
    set_state(&mut m, 3, OpState::Cancelled, NOON_MS + 4_000);
    let before: Vec<DownloadEntry> = m.entries().to_vec();

    let reopened = DownloadManager::open(&file);
    assert_eq!(reopened.entries().len(), 3);
    let first = reopened.get(1).unwrap();
    // The name goes through the sanitiser on the way back in, so a hand-edited file cannot carry a path.
    assert_eq!((first.name.as_str(), first.state, first.got, first.total, first.ended), ("tab_name.zip", State::Done, 500, 500, NOON_MS / 1000 + 9));
    assert_eq!(first.url, before[0].url);
    assert_eq!(first.path, before[0].path);
    let failed = reopened.get(b).unwrap();
    assert_eq!((failed.state, failed.err.as_str(), failed.total), (State::Failed, "Disk full", 100));
    assert!(!failed.resume && failed.op.is_none(), "a handle does not survive a restart");
    assert_eq!(reopened.get(c).unwrap().state, State::Cancelled);
    assert!(reopened.entries().iter().all(|e| !e.unseen), "nothing asks for attention after a restart");
}

#[test]
fn entries_active_at_exit_come_back_failed_interrupted() {
    let tmp = TempDir::new("interrupted");
    let file = sample_file(&tmp);
    let mut m = DownloadManager::open(&file);
    let a = start(&mut m, 1, "a.bin", 100, NOON_MS);
    let b = start(&mut m, 2, "b.bin", 100, NOON_MS);
    progress(&mut m, 1, 40, 100, NOON_MS + 2_000);
    set_state(&mut m, 2, OpState::Paused, NOON_MS + 3_000);
    // No shutdown: the process died with both running.
    let reopened = DownloadManager::open(&file);
    for id in [a, b] {
        let e = reopened.get(id).unwrap();
        assert_eq!((e.state, e.err.as_str(), e.resume, e.op), (State::Failed, "Interrupted", false, None));
        assert!(e.ended >= e.started);
    }
    assert!(!reopened.snapshot_for_toolbar().on);
}

#[test]
fn ids_stay_monotonic_across_restarts_even_when_the_newest_was_removed() {
    let tmp = TempDir::new("highwater");
    let file = sample_file(&tmp);
    let mut m = DownloadManager::open(&file);
    start(&mut m, 1, "a.bin", 10, 1_000);
    let b = start(&mut m, 2, "b.bin", 10, 1_000);
    finish(&mut m, 1, 10, 10, 2_000);
    finish(&mut m, 2, 10, 10, 2_000);
    m.remove(b);
    let mut again = DownloadManager::open(&file);
    assert_eq!(again.next_id(), 3);
    assert_eq!(start(&mut again, 9, "c.bin", 10, 3_000), 3);
    // Cleared entirely: the mark remains.
    let mut cleared = DownloadManager::open(&file);
    cleared.clear_finished();
    assert_eq!(DownloadManager::open(&file).next_id(), 4);
}

#[test]
fn the_high_water_mark_also_covers_a_file_with_a_missing_header() {
    let tmp = TempDir::new("noheader");
    let file = sample_file(&tmp);
    std::fs::write(&file, "7\t100\t200\tdone\t5\t5\ta.bin\thttps://a.test/\tC:\\d\\a.bin\t\n").unwrap();
    let m = DownloadManager::open(&file);
    assert_eq!(m.next_id(), 8);
}

#[test]
fn an_unreadable_file_is_an_error_state_and_is_never_rewritten() {
    let tmp = TempDir::new("unreadable");
    let file = sample_file(&tmp);
    std::fs::create_dir(&file).unwrap();
    std::fs::write(file.join("keep.txt"), b"precious").unwrap();
    let mut m = DownloadManager::open(&file);
    assert_eq!(m.list_state(), ListState::Error);
    // Downloads still work in memory...
    let id = start(&mut m, 1, "a.bin", 10, 1_000);
    finish(&mut m, 1, 10, 10, 2_000);
    m.remove(id);
    m.save();
    // ...but nothing replaced the unreadable thing.
    assert!(file.is_dir() && file.join("keep.txt").exists());
    assert!(!tmp.path("downloads.tmp").exists());
}

#[test]
fn an_oversized_file_is_an_error_state_too() {
    let tmp = TempDir::new("big");
    let file = sample_file(&tmp);
    std::fs::write(&file, vec![b'x'; MAX_FILE_BYTES as usize + 1]).unwrap();
    let mut m = DownloadManager::open(&file);
    assert_eq!(m.list_state(), ListState::Error);
    start(&mut m, 1, "a.bin", 10, 1_000);
    assert_eq!(std::fs::metadata(&file).unwrap().len(), MAX_FILE_BYTES + 1, "left exactly as it was");
}

#[test]
fn a_guest_list_never_touches_the_disk() {
    let tmp = TempDir::new("guest");
    let mut m = DownloadManager::in_memory(true);
    start(&mut m, 1, "a.bin", 10, 1_000);
    finish(&mut m, 1, 10, 10, 2_000);
    m.shutdown(3_000);
    assert_eq!(m.list_state(), ListState::Guest);
    assert_eq!(std::fs::read_dir(&tmp.0).unwrap().count(), 0);
}

#[test]
fn malformed_lines_are_skipped_and_urls_are_revalidated() {
    let text = "\u{feff}#next\t50\n\
        garbage line\n\
        1\t10\t20\tdone\t5\t5\ta.bin\thttps://a.test/x\tC:\\d\\a.bin\t\n\
        2\tx\t20\tdone\t5\t5\tb.bin\thttps://a.test/x\tC:\\d\\b.bin\t\n\
        3\t10\t20\tweird\t5\t5\tc.bin\thttps://a.test/x\tC:\\d\\c.bin\t\n\
        4\t10\t20\tdone\t5\t5\td.bin\tjavascript:alert(1)\tC:\\d\\d.bin\t\n\
        5\t10\t20\tdone\t5\t5\te.bin\thttps://a.test/x\trelative\\e.bin\t\n\
        1\t10\t20\tdone\t5\t5\tdup.bin\thttps://a.test/x\tC:\\d\\dup.bin\t\n\
        0\t10\t20\tdone\t5\t5\tzero.bin\thttps://a.test/x\tC:\\d\\zero.bin\t\n\
        6\t10\t20\tfailed\t5\t-1\t..\\..\\evil.exe\thttps://u:p@a.test/x\tC:\\d\\f.bin\tNo\\tpe\r\n";
    let (entries, next) = parse(text);
    let ids: Vec<u32> = entries.iter().map(|e| e.id).collect();
    assert_eq!(ids, vec![1, 4, 6]);
    assert_eq!(next, 50);
    assert_eq!(entries[1].url, "", "a non-http source is dropped, the entry stays");
    assert_eq!(entries[2].name, "evil.exe");
    assert_eq!(entries[2].url, "https://a.test/x");
    assert_eq!(entries[2].err, "No\tpe");
}

#[test]
fn only_the_newest_entries_are_written_and_read() {
    let tmp = TempDir::new("cap");
    let file = sample_file(&tmp);
    let mut text = String::new();
    for id in 1..=(MAX_ENTRIES as u32 + 20) {
        text.push_str(&format!("{id}\t10\t20\tdone\t5\t5\tf{id}.bin\thttps://a.test/x\tC:\\d\\f{id}.bin\t\n"));
    }
    std::fs::write(&file, text).unwrap();
    let mut m = DownloadManager::open(&file);
    assert_eq!(m.entries().len(), MAX_ENTRIES);
    assert_eq!(m.entries()[0].id, 21);
    start(&mut m, 1, "new.bin", 10, 1_000);
    let written = std::fs::read_to_string(&file).unwrap();
    assert!(written.lines().count() <= MAX_ENTRIES + 1);
    assert!(written.starts_with("#next\t"));
}

#[test]
fn progress_alone_never_writes_the_file() {
    let tmp = TempDir::new("noprogresswrite");
    let file = sample_file(&tmp);
    let mut m = DownloadManager::open(&file);
    start(&mut m, 1, "a.bin", 100, 1_000);
    let before = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, "sentinel").unwrap();
    progress(&mut m, 1, 50, 100, 2_000);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "sentinel");
    assert!(before.contains("active"), "the start was saved so a crash leaves an Interrupted row");
}

#[test]
fn escaping_round_trips_awkward_text() {
    for text in ["plain", "tab\there", "line\nbreak", "back\\slash", "\\t literal", "end\\", "cr\r\n"] {
        assert_eq!(unescape(&escape(text)), text);
    }
}

#[test]
fn clocks_are_local_hours_and_minutes() {
    assert_eq!(clock_label(D0 + 3600 * 9 + 60 * 5, 0), "09:05");
    assert_eq!(clock_label(D0 + 23 * 3600 + 59 * 60, 3600), "00:59");
    assert_eq!(clock_label(D0, -3600), "23:00");
}

// --- wire fixture (ui/test/fixtures/downloads-messages.json) ---

/// A list with every row state, on two days, built with the engine events the real list gets.
fn fixture_list() -> DownloadManager {
    let mut m = DownloadManager::in_memory(false);
    let yesterday_ms = (D0 - 3600) * 1000;
    start(&mut m, 90, "old-report.pdf", 2_000_000, yesterday_ms);
    finish(&mut m, 90, 2_000_000, 2_000_000, yesterday_ms + 5_000);
    start(&mut m, 91, "cancelled.zip", 500_000, yesterday_ms + 6_000);
    set_state(&mut m, 91, OpState::Cancelled, yesterday_ms + 7_000);
    start(&mut m, 1, "ubuntu-24.04.iso", 6_000_000_000, NOON_MS - 60_000);
    progress(&mut m, 1, 1_500_000_000, 6_000_000_000, NOON_MS - 30_000);
    progress(&mut m, 1, 2_400_000_000, 6_000_000_000, NOON_MS);
    start(&mut m, 2, "stream-recording.mp4", -1, NOON_MS - 50_000);
    progress(&mut m, 2, 12_300_000, -1, NOON_MS - 25_000);
    progress(&mut m, 2, 25_000_000, -1, NOON_MS);
    start(&mut m, 3, "paused <b>.tar.gz", 90_000_000, NOON_MS - 40_000);
    progress(&mut m, 3, 30_000_000, 90_000_000, NOON_MS - 20_000);
    set_state(&mut m, 3, OpState::Paused, NOON_MS - 10_000);
    start(&mut m, 4, "photo.png", 48_000, NOON_MS - 3_600_000);
    finish(&mut m, 4, 48_000, 48_000, NOON_MS - 3_590_000);
    start(&mut m, 5, "setup.exe", 100_000_000, NOON_MS - 7_200_000);
    fail(&mut m, 5, "Network error", true, NOON_MS - 7_190_000);
    start(&mut m, 6, "gone.txt", 10, NOON_MS - 10_800_000);
    finish(&mut m, 6, 10, 10, NOON_MS - 10_790_000);
    m.refresh_missing(&|p| !p.ends_with("gone.txt"));
    m
}

#[test]
fn downloads_messages_match_committed_fixture() {
    let m = fixture_list();
    let empty = DownloadManager::in_memory(false);
    let mut one_done = DownloadManager::in_memory(false);
    start(&mut one_done, 1, "a.bin", 10, NOON_MS);
    finish(&mut one_done, 1, 10, 10, NOON_MS + 1_000);
    // The bubble was opened and closed since: nothing active and nothing unseen hides the button.
    let mut quiet = DownloadManager::in_memory(false);
    start(&mut quiet, 1, "a.bin", 10, NOON_MS);
    finish(&mut quiet, 1, 10, 10, NOON_MS + 1_000);
    quiet.mark_seen();
    let mut one_failed = DownloadManager::in_memory(false);
    start(&mut one_failed, 1, "a.bin", 10, NOON_MS);
    fail(&mut one_failed, 1, "Disk full", false, NOON_MS + 1_000);
    let mut paused_only = DownloadManager::in_memory(false);
    start(&mut paused_only, 1, "a.bin", 100, NOON_MS);
    set_state(&mut paused_only, 1, OpState::Paused, NOON_MS + 1_000);
    let mut removed = fixture_list();
    let gone = removed.entries().iter().find(|e| e.name == "photo.png").map(|e| e.id).unwrap();
    assert!(removed.remove(gone));
    let unreadable = TempDir::new("fixture-unreadable");
    std::fs::create_dir(unreadable.path("downloads.txt")).unwrap(); // a folder where the file should be
    let error = DownloadManager::open(&unreadable.path("downloads.txt"));
    assert_eq!(error.list_state(), ListState::Error);
    let content = format!(
        concat!(
            "{{\"dlbtnOff\":{},\"dlbtnActive\":{},\"dlbtnQuiet\":{},\"dlbtnDone\":{},\"dlbtnFailed\":{},\"dlbtnPaused\":{},",
            "\"bubbleAuto\":{},\"bubble\":{},\"bubbleEmpty\":{},",
            "\"page\":{},\"pageSearch\":{},\"pageNone\":{},\"pageEmpty\":{},\"pageRemoved\":{},\"pageGuest\":{},\"pageError\":{}}}\n",
        ),
        empty.snapshot_for_toolbar().json(),
        m.snapshot_for_toolbar().json(),
        quiet.snapshot_for_toolbar().json(),
        one_done.snapshot_for_toolbar().json(),
        one_failed.snapshot_for_toolbar().json(),
        paused_only.snapshot_for_toolbar().json(),
        m.snapshot_for_bubble(4, true, BUBBLE_ROWS, time()),
        m.snapshot_for_bubble(5, false, BUBBLE_ROWS, time()),
        empty.snapshot_for_bubble(6, false, BUBBLE_ROWS, time()),
        m.snapshot_for_page("", Ack::None, time()),
        m.snapshot_for_page("MP4", Ack::None, time()),
        m.snapshot_for_page("zzz", Ack::None, time()),
        empty.snapshot_for_page("", Ack::None, time()),
        removed.snapshot_for_page("", Ack::Removed, time()),
        DownloadManager::in_memory(true).snapshot_for_page("", Ack::None, time()),
        error.snapshot_for_page("", Ack::Failed, time()),
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/downloads-messages.json");
    if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some_and(|value| value == "1") {
        std::fs::create_dir_all(path.parent().expect("fixtures directory")).expect("create fixtures directory");
        std::fs::write(&path, content).expect("write downloads fixture");
    } else {
        assert_eq!(std::fs::read_to_string(path).expect("committed downloads fixture"), content);
    }
}

/// P-05: two windows of one profile are two processes, each with its own manager on the same
/// file. Neither may drop the other's entries, reuse its ids, or rewrite its finished or running
/// downloads as "Interrupted".
#[test]
fn two_windows_on_one_profile_keep_each_others_downloads() {
    let tmp = TempDir::new("twowindows");
    let file = sample_file(&tmp);
    let mut a = DownloadManager::open(&file);
    let mut b = DownloadManager::open(&file);
    let x = start(&mut a, 1, "x.bin", 100, NOON_MS);
    // B was opened before A's download and starts its own while A's is still running.
    let y = start(&mut b, 1, "y.bin", 100, NOON_MS + 1_000);
    assert_ne!(x, y, "ids are shared through the file");
    finish(&mut a, 1, 100, 100, NOON_MS + 2_000);
    finish(&mut b, 1, 100, 100, NOON_MS + 3_000);

    let reopened = DownloadManager::open(&file);
    let names: Vec<&str> = reopened.entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["x.bin", "y.bin"]);
    assert!(reopened.entries().iter().all(|e| e.state == State::Done), "{:?}", reopened.entries());
    assert_eq!(reopened.next_id(), y.max(x) + 1);
    // Each window's list picks the other's entry up as well.
    assert_eq!(b.get(x).map(|e| e.state), Some(State::Done));
}

#[test]
fn a_window_never_rewrites_a_download_another_window_is_running() {
    let tmp = TempDir::new("foreignactive");
    let file = sample_file(&tmp);
    let mut a = DownloadManager::open(&file);
    let x = start(&mut a, 1, "x.bin", 100, NOON_MS);
    // B opens while X runs in A, so B shows it as interrupted, then B saves for its own reasons.
    let mut b = DownloadManager::open(&file);
    assert_eq!(b.get(x).unwrap().err, "Interrupted");
    let y = start(&mut b, 1, "y.bin", 10, NOON_MS + 1_000);
    finish(&mut b, 1, 10, 10, NOON_MS + 1_500);
    finish(&mut a, 1, 100, 100, NOON_MS + 2_000);
    let reopened = DownloadManager::open(&file);
    assert_eq!(reopened.get(x).map(|e| (e.state, e.err.as_str())), Some((State::Done, "")));
    assert_eq!(reopened.get(y).map(|e| e.state), Some(State::Done));
}

#[test]
fn a_removal_in_one_window_is_not_undone_by_the_other() {
    let tmp = TempDir::new("foreignremove");
    let file = sample_file(&tmp);
    let mut a = DownloadManager::open(&file);
    let x = start(&mut a, 1, "x.bin", 10, NOON_MS);
    finish(&mut a, 1, 10, 10, NOON_MS + 500);
    let mut b = DownloadManager::open(&file);
    assert!(a.remove(x));
    // B still lists X; its next save must not bring X back.
    start(&mut b, 1, "y.bin", 10, NOON_MS + 1_000);
    let reopened = DownloadManager::open(&file);
    assert!(reopened.get(x).is_none(), "{:?}", reopened.entries());
    assert!(b.get(x).is_none());
    // B's own removal sticks too, even after A has picked the entry up.
    let y = b.entries().iter().find(|e| e.name == "y.bin").unwrap().id;
    finish(&mut b, 1, 10, 10, NOON_MS + 2_000);
    start(&mut a, 2, "z.bin", 10, NOON_MS + 3_000);
    assert!(a.get(y).is_some(), "A picked Y up");
    assert!(b.remove(y));
    finish(&mut a, 2, 10, 10, NOON_MS + 4_000);
    assert!(DownloadManager::open(&file).get(y).is_none());
    assert!(a.get(y).is_none());
}
