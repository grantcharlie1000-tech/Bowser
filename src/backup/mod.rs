//! Back up and restore (docs/design/backup.md): one password-encrypted file holding a profile's
//! cookies, saved passwords, history, bookmarks, settings and the other files listed in [`FILES`],
//! for a reinstall or a new PC.
//!
//! # File
//!
//! ```text
//! magic "BWSRBAK1" | version u8 (1) | kdf u8 (1 = PBKDF2-HMAC-SHA256) | iterations u32 LE
//! | salt [16] | nonce [12] | AES-256-GCM ciphertext | tag [16]
//! ```
//!
//! The 42-byte header is the cipher's associated data, so none of it can be changed without the tag
//! failing. The key is PBKDF2-HMAC-SHA256 of the password (UTF-8) and the salt. A wrong password and
//! a damaged file read the same: [`BackupError::Password`]. The plaintext is a list of named entries
//! ([`encode_entries`]): a file from [`FILES`] by its name, [`PASSWORDS_ENTRY`] (Bowser's password CSV)
//! and [`COOKIES_ENTRY`] ([`encode_cookies`]).
//!
//! # Restore
//!
//! Cookies and passwords are added to the running profile at once. The files are written to
//! [`PENDING_DIR`] ([`stage_restore`]) and Bowser restarts; the closing process moves them into the
//! profile after its own last writes ([`apply_pending_restore`], from `main`), so nothing in memory
//! writes an old copy over them.
//!
//! Everything here but the cipher (`cng`, Windows CNG) is pure and unit-tested.

use std::path::{Path, PathBuf};

use zeroize::{Zeroize, Zeroizing};

#[cfg(windows)]
mod cng;

pub const MAGIC: &[u8; 8] = b"BWSRBAK1";
pub const VERSION: u8 = 1;
const KDF_PBKDF2_SHA256: u8 = 1;
/// OWASP's 2023 figure for PBKDF2-HMAC-SHA256. About half a second on a current PC.
pub const ITERATIONS: u32 = 600_000;
/// What a file may ask for: below the floor is too weak to trust, above the ceiling would hang the window.
const MIN_ITERATIONS: u32 = 100_000;
const MAX_ITERATIONS: u32 = 10_000_000;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
pub const HEADER_LEN: usize = 8 + 1 + 1 + 4 + SALT_LEN + NONCE_LEN;
/// Largest backup read or written. History is the big one; this leaves plenty of room.
pub const MAX_FILE_BYTES: usize = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 256;
const MAX_NAME_BYTES: usize = 64;
/// Shortest password accepted for a new backup.
pub const MIN_PASSWORD_CHARS: usize = 8;

/// Bowser's own password CSV (`PasswordStore::export_csv`), merged on restore.
pub const PASSWORDS_ENTRY: &str = "passwords.csv";
/// The profile's cookies ([`encode_cookies`]), added on restore.
pub const COOKIES_ENTRY: &str = "cookies.tsv";

/// The profile files a backup carries, by name in the profile folder. Only these are written on
/// restore, so a crafted file can never put anything else (or anywhere else) in the profile.
/// Left out on purpose: the crash-recovery session (`session.txt`, `running.lock`), the
/// Windows-account-locked vaults (`Passwords.dat` travels as [`PASSWORDS_ENTRY`]; `Autofill.dat`
/// stays), the WebView2 folder (cookies travel as [`COOKIES_ENTRY`]), extensions and caches.
pub const FILES: &[&str] = &[
    "settings.txt",
    "profile.txt",
    "history.txt",
    "history-enabled.txt",
    "typed-history.txt",
    "bookmarks-v2.txt",
    "bookmark-extras.txt",
    "downloads.txt",
    "sidebar.txt",
    "panel_prefs.txt",
    "speeddial.txt",
    "start-widgets.txt",
    "notes.txt",
    "notes-meta.txt",
    "tasks.txt",
    "sessions.txt",
    "workspaces.txt",
    "workspace-rules.txt",
    "tab-groups.txt",
    "keybindings.txt",
    "chains.txt",
    "search-engines.txt",
    "search-provider.txt",
    "permissions.txt",
    "site-prefs.txt",
    "blocker-sites.txt",
    "filters.txt",
    "blocker-sources.txt",
    "feeds.txt",
];

/// Where staged files wait for the restart, inside the profile folder.
pub const PENDING_DIR: &str = "restore-pending";
/// Written last into [`PENDING_DIR`]: its Unix time. Without it (a crash mid-way) nothing is applied.
const READY_MARKER: &str = ".ready";
/// A staged restore older than this is dropped instead of applied: it belongs to an import whose
/// restart never happened, and must not overwrite a profile days later.
const PENDING_MAX_AGE_SECS: u64 = 15 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupError {
    /// Wrong password, or the file was changed or damaged (indistinguishable by design).
    Password,
    /// Not a Bowser backup, or one this version cannot read.
    Format(&'static str),
    TooLarge,
    /// The cipher or the random source failed.
    Crypto,
    Io(String),
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Password => f.write_str("The password is wrong, or the file is damaged."),
            Self::Format(what) => write!(f, "This is not a Bowser backup file ({what})."),
            Self::TooLarge => f.write_str("The backup is too large."),
            Self::Crypto => f.write_str("Windows could not encrypt or decrypt the backup."),
            Self::Io(error) => write!(f, "The file could not be read or written: {error}"),
        }
    }
}

impl From<std::io::Error> for BackupError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// One named part of a backup. `data` is wiped when dropped: it may be passwords or cookies.
#[derive(Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub data: Zeroizing<Vec<u8>>,
}

impl Entry {
    pub fn new(name: &str, data: Vec<u8>) -> Self {
        Self { name: name.to_owned(), data: Zeroizing::new(data) }
    }
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Entry({}, {} bytes)", self.name, self.data.len())
    }
}

/// Whether `name` may be an entry of a backup: a listed file, or one of the two special entries.
pub fn known_entry(name: &str) -> bool {
    name == PASSWORDS_ENTRY || name == COOKIES_ENTRY || FILES.contains(&name)
}

// ---- entries ----

/// `count u32 LE`, then per entry `name_len u16 LE | name | data_len u64 LE | data`.
pub fn encode_entries(entries: &[Entry]) -> Result<Zeroizing<Vec<u8>>, BackupError> {
    if entries.len() > MAX_ENTRIES {
        return Err(BackupError::TooLarge);
    }
    let total: usize = entries.iter().map(|e| 2 + e.name.len() + 8 + e.data.len()).sum::<usize>() + 4;
    if total > MAX_FILE_BYTES {
        return Err(BackupError::TooLarge);
    }
    let mut out = Zeroizing::new(Vec::with_capacity(total));
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for entry in entries {
        if entry.name.is_empty() || entry.name.len() > MAX_NAME_BYTES {
            return Err(BackupError::Format("entry name"));
        }
        out.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
        out.extend_from_slice(entry.name.as_bytes());
        out.extend_from_slice(&(entry.data.len() as u64).to_le_bytes());
        out.extend_from_slice(&entry.data);
    }
    Ok(out)
}

/// Reads [`encode_entries`]' form strictly: exact lengths, no trailing bytes, each name once and
/// known ([`known_entry`]).
pub fn decode_entries(bytes: &[u8]) -> Result<Vec<Entry>, BackupError> {
    let mut at = 0usize;
    let mut take = |len: usize| -> Result<&[u8], BackupError> {
        let end = at.checked_add(len).filter(|end| *end <= bytes.len()).ok_or(BackupError::Format("truncated"))?;
        let part = &bytes[at..end];
        at = end;
        Ok(part)
    };
    let count = u32::from_le_bytes(take(4)?.try_into().map_err(|_| BackupError::Format("count"))?) as usize;
    if count > MAX_ENTRIES {
        return Err(BackupError::Format("too many entries"));
    }
    let mut entries: Vec<Entry> = Vec::with_capacity(count);
    for _ in 0..count {
        let name_len = u16::from_le_bytes(take(2)?.try_into().map_err(|_| BackupError::Format("name"))?) as usize;
        if name_len == 0 || name_len > MAX_NAME_BYTES {
            return Err(BackupError::Format("entry name"));
        }
        let name = std::str::from_utf8(take(name_len)?).map_err(|_| BackupError::Format("entry name"))?.to_owned();
        if !known_entry(&name) || entries.iter().any(|e| e.name == name) {
            return Err(BackupError::Format("unknown or repeated entry"));
        }
        let data_len = u64::from_le_bytes(take(8)?.try_into().map_err(|_| BackupError::Format("length"))?);
        let data_len = usize::try_from(data_len).map_err(|_| BackupError::TooLarge)?;
        let data = take(data_len)?.to_vec();
        entries.push(Entry { name, data: Zeroizing::new(data) });
    }
    if at != bytes.len() {
        return Err(BackupError::Format("trailing bytes"));
    }
    Ok(entries)
}

// ---- header ----

/// The parameters at the front of a backup file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub iterations: u32,
    pub salt: [u8; SALT_LEN],
    pub nonce: [u8; NONCE_LEN],
}

impl Header {
    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[..8].copy_from_slice(MAGIC);
        out[8] = VERSION;
        out[9] = KDF_PBKDF2_SHA256;
        out[10..14].copy_from_slice(&self.iterations.to_le_bytes());
        out[14..14 + SALT_LEN].copy_from_slice(&self.salt);
        out[14 + SALT_LEN..].copy_from_slice(&self.nonce);
        out
    }

    /// Reads the header of `file` and returns it with the rest (ciphertext and tag).
    pub fn parse(file: &[u8]) -> Result<(Self, &[u8]), BackupError> {
        if file.len() > MAX_FILE_BYTES {
            return Err(BackupError::TooLarge);
        }
        if file.len() < HEADER_LEN + TAG_LEN || &file[..8] != MAGIC {
            return Err(BackupError::Format("not a backup"));
        }
        if file[8] != VERSION {
            return Err(BackupError::Format("made by a newer Bowser"));
        }
        if file[9] != KDF_PBKDF2_SHA256 {
            return Err(BackupError::Format("key derivation"));
        }
        let iterations = u32::from_le_bytes(file[10..14].try_into().map_err(|_| BackupError::Format("iterations"))?);
        if !(MIN_ITERATIONS..=MAX_ITERATIONS).contains(&iterations) {
            return Err(BackupError::Format("iterations"));
        }
        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&file[14..14 + SALT_LEN]);
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&file[14 + SALT_LEN..HEADER_LEN]);
        Ok((Self { iterations, salt, nonce }, &file[HEADER_LEN..]))
    }
}

/// A backup password on its way from the page: wiped when dropped, never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct BackupPassword(Zeroizing<String>);

impl BackupPassword {
    pub fn new(text: &str) -> Self {
        Self(Zeroizing::new(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for BackupPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BackupPassword(..)")
    }
}

/// What an export or a restore did, for the page's message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub files: usize,
    pub passwords: usize,
    /// `None`: cookies could not be reached (no web page open, or the engine refused).
    pub cookies: Option<usize>,
}

/// Whether `password` is good enough for a new backup (re-checked by the page).
pub fn password_ok(password: &str) -> bool {
    password.chars().count() >= MIN_PASSWORD_CHARS && password.chars().count() <= 1024
}

/// Encrypts `entries` under `password` into a complete backup file.
#[cfg(windows)]
pub fn seal(password: &str, entries: &[Entry]) -> Result<Vec<u8>, BackupError> {
    if !password_ok(password) {
        return Err(BackupError::Password);
    }
    let plain = encode_entries(entries)?;
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    cng::random(&mut salt)?;
    cng::random(&mut nonce)?;
    let header = Header { iterations: ITERATIONS, salt, nonce };
    let head = header.to_bytes();
    let key = cng::pbkdf2_sha256(password.as_bytes(), &salt, ITERATIONS)?;
    let (cipher, tag) = cng::aes_gcm_seal(&key, &nonce, &head, &plain)?;
    let mut out = Vec::with_capacity(HEADER_LEN + cipher.len() + TAG_LEN);
    out.extend_from_slice(&head);
    out.extend_from_slice(&cipher);
    out.extend_from_slice(&tag);
    Ok(out)
}

/// Decrypts a backup file and returns its entries.
#[cfg(windows)]
pub fn open(password: &str, file: &[u8]) -> Result<Vec<Entry>, BackupError> {
    let (header, rest) = Header::parse(file)?;
    let (cipher, tag) = rest.split_at(rest.len() - TAG_LEN);
    let tag: [u8; TAG_LEN] = tag.try_into().map_err(|_| BackupError::Format("tag"))?;
    let key = cng::pbkdf2_sha256(password.as_bytes(), &header.salt, header.iterations)?;
    let plain = cng::aes_gcm_open(&key, &header.nonce, &file[..HEADER_LEN], cipher, &tag)?;
    decode_entries(&plain)
}

// ---- cookies ----

/// One cookie as WebView2 reports it (`ICoreWebView2Cookie`).
#[derive(Clone, PartialEq)]
pub struct CookieRecord {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    /// Unix seconds; ignored for a session cookie.
    pub expires: f64,
    pub http_only: bool,
    pub secure: bool,
    /// `COREWEBVIEW2_COOKIE_SAME_SITE_KIND`: 0 none, 1 lax, 2 strict.
    pub same_site: i32,
    pub session: bool,
}

impl Drop for CookieRecord {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

impl std::fmt::Debug for CookieRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cookie({} @ {}{})", self.name, self.domain, self.path)
    }
}

/// `\`, tab, CR and LF escaped so one cookie is one line of tab-separated fields.
fn escape_field(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            ch => out.push(ch),
        }
    }
}

fn unescape_field(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        out.push(match chars.next()? {
            '\\' => '\\',
            't' => '\t',
            'n' => '\n',
            'r' => '\r',
            _ => return None,
        });
    }
    Some(out)
}

/// One line per cookie: `name value domain path expires http_only secure same_site session`.
pub fn encode_cookies(cookies: &[CookieRecord]) -> Zeroizing<Vec<u8>> {
    let mut text = Zeroizing::new(String::new());
    for cookie in cookies {
        for (index, field) in [&cookie.name, &cookie.value, &cookie.domain, &cookie.path].into_iter().enumerate() {
            if index > 0 {
                text.push('\t');
            }
            escape_field(field, &mut text);
        }
        let expires = if cookie.expires.is_finite() { cookie.expires } else { -1.0 };
        text.push_str(&format!(
            "\t{expires}\t{}\t{}\t{}\t{}\n",
            u8::from(cookie.http_only),
            u8::from(cookie.secure),
            cookie.same_site.clamp(0, 2),
            u8::from(cookie.session)
        ));
    }
    Zeroizing::new(std::mem::take(&mut *text).into_bytes())
}

/// Reads [`encode_cookies`]' form. A line that is not exactly a cookie is skipped, as is a cookie
/// without a name or domain: one bad line never loses the rest.
pub fn decode_cookies(bytes: &[u8]) -> Vec<CookieRecord> {
    let Ok(text) = std::str::from_utf8(bytes) else { return Vec::new() };
    let flag = |text: &str| match text {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    };
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let [name, value, domain, path, expires, http_only, secure, same_site, session] = fields[..] else {
                return None;
            };
            let cookie = CookieRecord {
                name: unescape_field(name)?,
                value: unescape_field(value)?,
                domain: unescape_field(domain)?,
                path: unescape_field(path)?,
                expires: expires.parse::<f64>().ok().filter(|e| e.is_finite())?,
                http_only: flag(http_only)?,
                secure: flag(secure)?,
                same_site: same_site.parse::<i32>().ok().filter(|s| (0..=2).contains(s))?,
                session: flag(session)?,
            };
            (!cookie.name.is_empty() && !cookie.domain.is_empty()).then_some(cookie)
        })
        .collect()
}

/// A cookie worth restoring: a session cookie, or one that has not expired at `now_secs`.
pub fn cookie_alive(cookie: &CookieRecord, now_secs: f64) -> bool {
    cookie.session || cookie.expires > now_secs
}

// ---- profile files ----

/// Reads every [`FILES`] entry that exists in `profile_dir`. A file over the size limit fails the
/// whole export rather than silently leaving it out.
pub fn collect_files(profile_dir: &Path) -> Result<Vec<Entry>, BackupError> {
    let mut entries = Vec::new();
    let mut total = 0usize;
    for name in FILES {
        let path = profile_dir.join(name);
        match std::fs::read(&path) {
            Ok(bytes) => {
                total += bytes.len();
                if total > MAX_FILE_BYTES {
                    return Err(BackupError::TooLarge);
                }
                entries.push(Entry::new(name, bytes));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(BackupError::Io(format!("{name}: {error}"))),
        }
    }
    Ok(entries)
}

/// Writes the file entries of a restore to `profile_dir\restore-pending\`, then the ready marker.
/// The special entries are not files and are skipped. Returns how many files were staged.
pub fn stage_restore(profile_dir: &Path, entries: &[Entry], now_secs: u64) -> Result<usize, BackupError> {
    let dir = profile_dir.join(PENDING_DIR);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    let mut staged = 0;
    for entry in entries.iter().filter(|e| FILES.contains(&e.name.as_str())) {
        std::fs::write(dir.join(&entry.name), &*entry.data)?;
        staged += 1;
    }
    std::fs::write(dir.join(READY_MARKER), now_secs.to_string())?;
    Ok(staged)
}

/// What [`apply_pending_restore`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// No staged restore.
    Nothing,
    /// Incomplete or too old: removed without touching the profile.
    Dropped,
    /// These files replaced the profile's.
    Restored(Vec<String>),
}

/// Moves a staged restore into `profile_dir` (from `main`, once this process has written all it
/// will). Each file replaces the profile's by rename, so a file is either the old or the new one.
/// Only names in [`FILES`] are moved. The staging folder is removed in every case.
pub fn apply_pending_restore(profile_dir: &Path, now_secs: u64) -> Result<Applied, BackupError> {
    let dir = profile_dir.join(PENDING_DIR);
    if !dir.is_dir() {
        return Ok(Applied::Nothing);
    }
    let fresh = std::fs::read_to_string(dir.join(READY_MARKER))
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .is_some_and(|at| at <= now_secs && now_secs - at <= PENDING_MAX_AGE_SECS);
    if !fresh {
        std::fs::remove_dir_all(&dir)?;
        return Ok(Applied::Dropped);
    }
    let mut moved = Vec::new();
    let mut failed = None;
    for name in FILES {
        let from = dir.join(name);
        if !from.is_file() {
            continue;
        }
        match replace_file(&from, &profile_dir.join(name)) {
            Ok(()) => moved.push((*name).to_owned()),
            Err(error) => failed = Some(format!("{name}: {error}")),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    match failed {
        Some(error) => Err(BackupError::Io(error)),
        None => Ok(Applied::Restored(moved)),
    }
}

/// `from` takes `to`'s place (same volume: the staging folder is inside the profile).
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};
        let a: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            MoveFileExW(
                windows::core::PCWSTR(a.as_ptr()),
                windows::core::PCWSTR(b.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|error| std::io::Error::other(error.to_string()))
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(from, to)
    }
}

/// The suggested file name for a new backup: `Bowser backup 2026-11-18.bowserbackup`.
pub fn default_file_name(civil_date: &str) -> String {
    format!("Bowser backup {civil_date}.{EXTENSION}")
}

pub const EXTENSION: &str = "bowserbackup";

/// The staging folder of `profile_dir` (for callers that only need the path).
pub fn pending_dir(profile_dir: &Path) -> PathBuf {
    profile_dir.join(PENDING_DIR)
}

#[cfg(test)]
mod tests;
