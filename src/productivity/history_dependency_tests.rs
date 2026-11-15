//! The dependency contract: a successful check permits reuse of the exact strict-read result.
use super::*;

fn folder(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("bowser-history-dependency-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn a_dependency_is_bound_to_its_history_file_and_external_edits_invalidate_it() {
    let root = folder("files");
    let a = root.join("a.txt");
    let b = root.join("b.txt");
    fs::write(&a, "1\thttps://a.test/\tA\n").unwrap();
    fs::write(&b, "1\thttps://b.test/\tB\n").unwrap();
    let (entries, dependency) = read_history_with_dependency(&a).unwrap();
    assert_eq!(entries, [(1, "https://a.test/".into(), "A".into())]);
    let dependency = dependency.unwrap();
    assert!(!history_dependency_current(&b, dependency));
    fs::write(&a, "2\thttps://outside.test/\tOutside edit\n").unwrap();
    assert!(!history_dependency_current(&a, dependency));
    assert_eq!(read_history_strict(&a).unwrap(), [(2, "https://outside.test/".into(), "Outside edit".into())]);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_revisit_changes_the_dependency_even_when_the_queue_length_does_not_change() {
    let root = folder("revisit");
    let path = root.join("history.txt");
    let io = lock_history(&HISTORY_IO);
    queue_history_visit(&path, (1, "https://same.test/".into(), "Before".into()));
    let (before, dependency) = read_history_with_dependency_locked(&path).unwrap();
    queue_history_visit(&path, (2, "https://same.test/".into(), "After".into()));
    drop(io);
    assert_eq!(before, [(1, "https://same.test/".into(), "Before".into())]);
    assert!(!history_dependency_current(&path, dependency.unwrap()));
    assert_eq!(read_history_strict(&path).unwrap(), [(2, "https://same.test/".into(), "After".into())]);
    flush_pending_history();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_flush_invalidates_the_queued_snapshot_without_changing_its_visible_entries() {
    let root = folder("flush");
    let path = root.join("history.txt");
    let io = lock_history(&HISTORY_IO);
    queue_history_visit(&path, (1, "https://flush.test/".into(), "Queued".into()));
    let (before, dependency) = read_history_with_dependency_locked(&path).unwrap();
    flush_history_locked(&path).unwrap();
    drop(io);
    assert!(!history_dependency_current(&path, dependency.unwrap()));
    assert_eq!(read_history_strict(&path).unwrap(), before);
    assert_eq!(fs::read_to_string(&path).unwrap(), "1\thttps://flush.test/\tQueued\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn an_evicted_parse_never_authorizes_a_different_snapshot() {
    let root = folder("eviction");
    let path = root.join("history.txt");
    let io = lock_history(&HISTORY_IO);
    fs::write(&path, "1\thttps://kept.test/\tKept\n").unwrap();
    let (before, dependency) = read_history_with_dependency_locked(&path).unwrap();
    for index in 0..HISTORY_CACHE_FILES {
        let other = root.join(format!("other-{index}.txt"));
        read_history_with_dependency_locked(&other).unwrap();
    }
    drop(io);
    assert!(!history_dependency_current(&path, dependency.unwrap()));
    assert_eq!(read_history_strict(&path).unwrap(), before);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_failed_flush_keeps_visits_and_repair_reads_the_repaired_file_with_them() {
    let root = folder("repair");
    let path = root.join("history.txt");
    let io = lock_history(&HISTORY_IO);
    queue_history_visit(&path, (2, "https://queued.test/".into(), "Queued".into()));
    let (_, dependency) = read_history_with_dependency_locked(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(flush_history_locked(&path).is_err());
    drop(io);
    assert!(!history_dependency_current(&path, dependency.unwrap()));
    assert!(read_history_strict(&path).is_err());
    fs::remove_dir(&path).unwrap();
    fs::write(&path, "1\thttps://repaired.test/\tRepaired\n").unwrap();
    assert_eq!(read_history_strict(&path).unwrap(), [
        (1, "https://repaired.test/".into(), "Repaired".into()),
        (2, "https://queued.test/".into(), "Queued".into()),
    ]);
    flush_pending_history();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn revision_exhaustion_is_sticky_and_never_authorizes_reuse() {
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    let revision = AtomicU64::new(u64::MAX - 1);
    assert_eq!(advance_history_revision(&revision), u64::MAX);
    assert_eq!(advance_history_revision(&revision), u64::MAX);
    assert_eq!(revision.load(Relaxed), u64::MAX);
    let root = folder("overflow");
    let path = root.join("history.txt");
    let (_, dependency) = read_history_with_dependency(&path).unwrap();
    let dependency = dependency.unwrap();
    assert!(!history_dependency_current(&path, HistoryDependency { cached: u64::MAX, ..dependency }));
    assert!(!history_dependency_current(&path, HistoryDependency { queued: u64::MAX, ..dependency }));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn changed_reads_return_the_visible_snapshot_without_flushing_on_the_calling_thread() {
    let root = folder("changed-read");
    let path = root.join("history.txt");
    fs::write(&path, "1\thttps://a.test/\tBefore\n").unwrap();
    let (before, dependency) = read_history_with_dependency(&path).unwrap();
    // Unrelated files may conservatively invalidate a token during parallel tests; either result
    // must authorize only these rows. A changed file must always return its new rows.
    if let Some((unchanged, _)) = read_history_if_changed(&path, dependency).unwrap() {
        assert_eq!(unchanged, before);
    }
    let writes = HISTORY_WRITES_ON_THIS_THREAD.with(std::cell::Cell::get);
    add_history_entry_at(&path, 2, "https://a.test/", "After").unwrap();
    let (changed, next) = read_history_if_changed(&path, dependency).unwrap().expect("the visit changed the snapshot");
    assert_eq!(changed, [(2, "https://a.test/".into(), "After".into())]);
    assert!(next.is_some());
    assert_eq!(HISTORY_WRITES_ON_THIS_THREAD.with(std::cell::Cell::get), writes);
    let (uncached, _) = read_history_if_changed(&path, None).unwrap().unwrap();
    assert_eq!(uncached, changed);
    flush_pending_history();
    let _ = fs::remove_dir_all(root);
}
