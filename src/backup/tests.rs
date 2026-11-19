use super::*;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("bowser-backup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cookie(name: &str, value: &str) -> CookieRecord {
    CookieRecord {
        name: name.into(),
        value: value.into(),
        domain: ".example.com".into(),
        path: "/".into(),
        expires: 1_900_000_000.5,
        http_only: true,
        secure: true,
        same_site: 1,
        session: false,
    }
}

// ---- entries ----

#[test]
fn entries_round_trip_exactly() {
    let entries = vec![
        Entry::new("settings.txt", b"theme=helium\n".to_vec()),
        Entry::new(PASSWORDS_ENTRY, b"name,url,username,password,note\n".to_vec()),
        Entry::new(COOKIES_ENTRY, Vec::new()),
    ];
    let bytes = encode_entries(&entries).unwrap();
    assert_eq!(decode_entries(&bytes).unwrap(), entries);
}

#[test]
fn entries_are_read_strictly() {
    let good = encode_entries(&[Entry::new("history.txt", b"1\thttps://a.test/\tA\n".to_vec())]).unwrap();
    assert!(decode_entries(&good[..good.len() - 1]).is_err(), "truncated");
    let mut trailing = good.to_vec();
    trailing.push(0);
    assert_eq!(decode_entries(&trailing).unwrap_err(), BackupError::Format("trailing bytes"));
    for name in ["../settings.txt", "session.txt", "Passwords.dat", "C:\\x", "settings.txt\0"] {
        let bad = encode_entries(&[Entry::new(name, vec![1])]).unwrap();
        assert_eq!(decode_entries(&bad).unwrap_err(), BackupError::Format("unknown or repeated entry"), "{name}");
    }
    let twice = encode_entries(&[Entry::new("feeds.txt", vec![1]), Entry::new("feeds.txt", vec![2])]).unwrap();
    assert!(decode_entries(&twice).is_err(), "repeated");
    let mut huge = 1u32.to_le_bytes().to_vec();
    huge.extend_from_slice(&12u16.to_le_bytes());
    huge.extend_from_slice(b"settings.txt");
    huge.extend_from_slice(&u64::MAX.to_le_bytes());
    assert!(decode_entries(&huge).is_err(), "a length past the end");
    assert!(decode_entries(&(MAX_ENTRIES as u32 + 1).to_le_bytes()).is_err());
}

#[test]
fn every_listed_file_is_a_plain_name() {
    for name in FILES {
        assert!(!name.contains(['/', '\\', ':']) && !name.starts_with('.') && name.len() <= MAX_NAME_BYTES, "{name}");
        assert!(known_entry(name));
    }
    for left_out in ["session.txt", "running.lock", "Passwords.dat", "Autofill.dat", "WebView2", "Extensions"] {
        assert!(!known_entry(left_out), "{left_out}");
    }
}

// ---- header ----

#[test]
fn the_header_round_trips_and_refuses_what_it_cannot_trust() {
    let header = Header { iterations: ITERATIONS, salt: [7; 16], nonce: [9; 12] };
    let mut file = header.to_bytes().to_vec();
    assert_eq!(file.len(), HEADER_LEN);
    file.extend_from_slice(&[0; 16]);
    let (parsed, rest) = Header::parse(&file).unwrap();
    assert_eq!(parsed, header);
    assert_eq!(rest.len(), 16);

    assert_eq!(Header::parse(&file[..HEADER_LEN + 15]).unwrap_err(), BackupError::Format("not a backup"));
    let mut other = file.clone();
    other[0] = b'X';
    assert_eq!(Header::parse(&other).unwrap_err(), BackupError::Format("not a backup"));
    let mut newer = file.clone();
    newer[8] = 2;
    assert_eq!(Header::parse(&newer).unwrap_err(), BackupError::Format("made by a newer Bowser"));
    for iterations in [1u32, MIN_ITERATIONS - 1, MAX_ITERATIONS + 1] {
        let mut weak = file.clone();
        weak[10..14].copy_from_slice(&iterations.to_le_bytes());
        assert_eq!(Header::parse(&weak).unwrap_err(), BackupError::Format("iterations"), "{iterations}");
    }
}

#[test]
fn passwords_need_eight_characters() {
    assert!(!password_ok("seven77"));
    assert!(password_ok("eight888"));
    assert!(password_ok("ünïcødé!"));
    assert!(!password_ok(&"x".repeat(1025)));
}

// ---- cookies ----

#[test]
fn cookies_round_trip_with_awkward_values() {
    let mut odd = cookie("sid", "a\tb\\c\nd\re=f");
    odd.session = true;
    odd.expires = -1.0;
    odd.same_site = 2;
    odd.http_only = false;
    let cookies = vec![cookie("pref", "dark"), odd];
    let decoded = decode_cookies(&encode_cookies(&cookies));
    assert_eq!(decoded, cookies);
}

#[test]
fn a_bad_cookie_line_is_skipped_and_the_rest_kept() {
    let mut text = String::from_utf8(encode_cookies(&[cookie("a", "1"), cookie("b", "2")]).to_vec()).unwrap();
    text.insert_str(0, "junk\n\tvalue\t.x.test\t/\t0\t0\t0\t0\t0\nn\tv\t.x.test\t/\tNaN\t0\t0\t0\t0\nn\tv\t.x.test\t/\t1\t0\t0\t9\t0\nn\tv\t.x.test\t/\t1\t0\t0\t0\t0\tx\n");
    let names: Vec<String> = decode_cookies(text.as_bytes()).iter().map(|c| c.name.clone()).collect();
    assert_eq!(names, ["a", "b"]);
    assert!(decode_cookies(&[0xff, 0xfe]).is_empty());
}

#[test]
fn expired_cookies_are_not_restored() {
    let mut old = cookie("old", "1");
    old.expires = 100.0;
    let mut session = cookie("s", "1");
    session.session = true;
    session.expires = -1.0;
    assert!(!cookie_alive(&old, 200.0));
    assert!(cookie_alive(&session, 200.0));
    assert!(cookie_alive(&cookie("live", "1"), 200.0));
}

// ---- files and restore ----

#[test]
fn only_listed_files_are_collected() {
    let tmp = TempDir::new("collect");
    std::fs::write(tmp.0.join("settings.txt"), "theme=helium\n").unwrap();
    std::fs::write(tmp.0.join("history.txt"), "1\thttps://a.test/\tA\n").unwrap();
    std::fs::write(tmp.0.join("session.txt"), "crash recovery").unwrap();
    std::fs::write(tmp.0.join("Passwords.dat"), "locked").unwrap();
    let names: Vec<String> = collect_files(&tmp.0).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["settings.txt", "history.txt"]);
}

#[test]
fn a_staged_restore_replaces_the_profiles_files_once() {
    let tmp = TempDir::new("restore");
    std::fs::write(tmp.0.join("settings.txt"), "old").unwrap();
    std::fs::write(tmp.0.join("notes.txt"), "kept").unwrap();
    let entries = vec![
        Entry::new("settings.txt", b"new".to_vec()),
        Entry::new("bookmarks-v2.txt", b"marks".to_vec()),
        Entry::new(PASSWORDS_ENTRY, b"secret".to_vec()),
        Entry::new(COOKIES_ENTRY, b"cookie".to_vec()),
    ];
    assert_eq!(stage_restore(&tmp.0, &entries, 1_000).unwrap(), 2, "passwords and cookies are not files");
    assert!(!pending_dir(&tmp.0).join(PASSWORDS_ENTRY).exists(), "no secret is written in the clear");
    assert_eq!(std::fs::read_to_string(tmp.0.join("settings.txt")).unwrap(), "old", "nothing moves before the restart");
    // A file someone dropped into the folder by hand is never moved.
    std::fs::write(pending_dir(&tmp.0).join("session.txt"), "x").unwrap();

    let applied = apply_pending_restore(&tmp.0, 1_060).unwrap();
    assert_eq!(applied, Applied::Restored(vec!["settings.txt".into(), "bookmarks-v2.txt".into()]));
    assert_eq!(std::fs::read_to_string(tmp.0.join("settings.txt")).unwrap(), "new");
    assert_eq!(std::fs::read_to_string(tmp.0.join("bookmarks-v2.txt")).unwrap(), "marks");
    assert_eq!(std::fs::read_to_string(tmp.0.join("notes.txt")).unwrap(), "kept", "files the backup lacks stay");
    assert!(!tmp.0.join("session.txt").exists());
    assert!(!pending_dir(&tmp.0).exists());
    assert_eq!(apply_pending_restore(&tmp.0, 1_070).unwrap(), Applied::Nothing);
}

#[test]
fn an_incomplete_or_stale_restore_is_dropped_untouched() {
    let tmp = TempDir::new("stale");
    std::fs::write(tmp.0.join("settings.txt"), "old").unwrap();
    stage_restore(&tmp.0, &[Entry::new("settings.txt", b"new".to_vec())], 1_000).unwrap();
    assert_eq!(apply_pending_restore(&tmp.0, 1_000 + PENDING_MAX_AGE_SECS + 1).unwrap(), Applied::Dropped);
    assert_eq!(std::fs::read_to_string(tmp.0.join("settings.txt")).unwrap(), "old");
    assert!(!pending_dir(&tmp.0).exists());

    stage_restore(&tmp.0, &[Entry::new("settings.txt", b"new".to_vec())], 1_000).unwrap();
    std::fs::remove_file(pending_dir(&tmp.0).join(READY_MARKER)).unwrap();
    assert_eq!(apply_pending_restore(&tmp.0, 1_001).unwrap(), Applied::Dropped, "no marker: cut short");
    assert_eq!(std::fs::read_to_string(tmp.0.join("settings.txt")).unwrap(), "old");

    stage_restore(&tmp.0, &[Entry::new("settings.txt", b"new".to_vec())], 2_000).unwrap();
    assert_eq!(apply_pending_restore(&tmp.0, 1_000).unwrap(), Applied::Dropped, "a marker from the future");
}

#[test]
fn the_suggested_name_carries_the_date_and_extension() {
    assert_eq!(default_file_name("2026-11-18"), "Bowser backup 2026-11-18.bowserbackup");
}

// ---- Windows CNG: known answers and the sealed file ----

#[cfg(windows)]
fn hex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
}

#[cfg(windows)]
#[test]
fn pbkdf2_matches_the_published_vectors() {
    use super::cng::pbkdf2_sha256;
    // PBKDF2-HMAC-SHA256, P = "password", S = "salt", dkLen = 32.
    assert_eq!(
        pbkdf2_sha256(b"password", b"salt", 1).unwrap().to_vec(),
        hex("120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b")
    );
    assert_eq!(
        pbkdf2_sha256(b"password", b"salt", 4096).unwrap().to_vec(),
        hex("c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a")
    );
    // RFC 7914 section 11, first 32 bytes of P = "passwd", S = "salt", c = 1.
    assert_eq!(
        pbkdf2_sha256(b"passwd", b"salt", 1).unwrap().to_vec(),
        hex("55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc")
    );
}

#[cfg(windows)]
#[test]
fn aes_gcm_matches_the_published_vectors() {
    use super::cng::{aes_gcm_open, aes_gcm_seal};
    // GCM specification test cases 13 and 14: 256-bit zero key, zero IV.
    let key = [0u8; 32];
    let nonce = [0u8; 12];
    let (cipher, tag) = aes_gcm_seal(&key, &nonce, &[], &[]).unwrap();
    assert!(cipher.is_empty());
    assert_eq!(tag.to_vec(), hex("530f8afbc74536b9a963b4f1c4cb738b"));
    let (cipher, tag) = aes_gcm_seal(&key, &nonce, &[], &[0u8; 16]).unwrap();
    assert_eq!(cipher, hex("cea7403d4d606b6e074ec5d3baf39d18"));
    assert_eq!(tag.to_vec(), hex("d0d1c8a799996bf0265b98b5d48ab919"));
    assert_eq!(aes_gcm_open(&key, &nonce, &[], &cipher, &tag).unwrap().to_vec(), vec![0u8; 16]);
    let mut bad = tag;
    bad[0] ^= 1;
    assert_eq!(aes_gcm_open(&key, &nonce, &[], &cipher, &bad).unwrap_err(), BackupError::Password);
}

#[cfg(windows)]
#[test]
fn a_sealed_backup_opens_with_its_password_only_and_detects_any_change() {
    let entries = vec![
        Entry::new("settings.txt", b"theme=helium\n".to_vec()),
        Entry::new(COOKIES_ENTRY, encode_cookies(&[cookie("sid", "secret")]).to_vec()),
    ];
    let file = seal("correct horse", &entries).unwrap();
    assert_eq!(&file[..8], MAGIC);
    assert!(!file.windows(6).any(|w| w == b"secret"), "nothing readable in the file");
    assert_eq!(open("correct horse", &file).unwrap(), entries);
    assert_eq!(open("wrong horse!", &file).unwrap_err(), BackupError::Password);
    for at in [9usize, 20, HEADER_LEN + 1, file.len() - 1] {
        let mut changed = file.clone();
        changed[at] ^= 0x40;
        assert!(open("correct horse", &changed).is_err(), "byte {at}");
    }
    let mut salt_changed = file.clone();
    salt_changed[15] ^= 1;
    assert_eq!(open("correct horse", &salt_changed).unwrap_err(), BackupError::Password, "the header is authenticated");
    assert_ne!(seal("correct horse", &entries).unwrap(), file, "a fresh salt and nonce every time");
    assert_eq!(seal("short", &entries).unwrap_err(), BackupError::Password);
}
