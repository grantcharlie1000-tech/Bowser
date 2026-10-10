//! Local, DPAPI-protected password migration storage.
//!
//! This module only imports official CSV exports and stores encrypted records. It does not
//! integrate with WebView2 credential handling, reveal or export passwords, or autofill.

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};
use zeroize::{Zeroize, Zeroizing};

const MAGIC: &[u8; 8] = b"BWSRPW01";
const VERSION: u32 = 1;
pub const MAX_IMPORT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IMPORT_ROWS: usize = 10_000;
const MAX_STORE_BYTES: usize = 32 * 1024 * 1024;

#[derive(PartialEq, Eq)]
struct Record {
    url: String,
    username: String,
    password: String,
}
impl Drop for Record {
    fn drop(&mut self) {
        self.url.zeroize();
        self.username.zeroize();
        self.password.zeroize();
    }
}

impl fmt::Debug for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialRecord")
            .field("url", &self.url)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

/// Persisted vault. Its file is encrypted and authenticated by Windows DPAPI for the current user.
pub struct PasswordStore {
    path: PathBuf,
    records: Vec<Record>,
}

impl fmt::Debug for PasswordStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PasswordStore")
            .field("path", &"[LOCAL VAULT]")
            .field("records", &self.records.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Classification {
    New,
    ExactDuplicate,
    Conflict,
    Reject,
}

/// One import row with display-safe metadata only. No password is exposed through this type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewRow {
    pub row: usize,
    pub classification: Classification,
    pub url: Option<String>,
    pub username: Option<String>,
}

/// Import plan holds credentials only in memory until commit; Debug omits their contents.
pub struct ImportPlan {
    rows: Vec<(Option<Record>, Classification)>,
    preview: Vec<PreviewRow>,
}
impl fmt::Debug for ImportPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportPlan")
            .field("rows", &self.preview)
            .finish()
    }
}
impl ImportPlan {
    pub fn rows(&self) -> &[PreviewRow] {
        &self.preview
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordError {
    InvalidCsv,
    UnsupportedHeaders,
    TooLarge,
    TooManyRows,
    InvalidStore,
    Io,
    Protection,
}
impl fmt::Display for PasswordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidCsv => "Invalid password CSV",
            Self::UnsupportedHeaders => "Unsupported password CSV headers",
            Self::TooLarge => "Password CSV exceeds the size limit",
            Self::TooManyRows => "Password CSV exceeds the row limit",
            Self::InvalidStore => "Password vault is invalid",
            Self::Io => "Password vault could not be accessed",
            Self::Protection => "Windows user protection failed",
        })
    }
}
impl std::error::Error for PasswordError {}
impl From<io::Error> for PasswordError {
    fn from(_: io::Error) -> Self {
        Self::Io
    }
}

impl PasswordStore {
    /// Open the per-user Bowser vault under `%LOCALAPPDATA%`. DPAPI uses current-user scope only.
    pub fn open_default() -> Result<Self, PasswordError> {
        let base = std::env::var_os("LOCALAPPDATA").ok_or(PasswordError::Io)?;
        Self::open(Path::new(&base).join("Bowser").join("Passwords.dat"))
    }

    /// Open a vault at an explicit local path (useful for isolated app profiles and tests).
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, PasswordError> {
        let path = path.into();
        if !path.exists() {
            return Ok(Self {
                path,
                records: Vec::new(),
            });
        }
        let encrypted = fs::read(&path)?;
        if encrypted.len() > MAX_STORE_BYTES {
            return Err(PasswordError::InvalidStore);
        }
        let plain = Zeroizing::new(dpapi_unprotect(&encrypted)?);
        let records = decode_store(&plain)?;
        Ok(Self { path, records })
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Parse a user-selected official CSV export and classify against this vault.
    pub fn preview_csv(&self, bytes: &[u8]) -> Result<ImportPlan, PasswordError> {
        if bytes.len() > MAX_IMPORT_BYTES {
            return Err(PasswordError::TooLarge);
        }
        let mut table = Zeroizing::new(parse_csv(bytes)?);
        if table.is_empty() {
            return Err(PasswordError::InvalidCsv);
        }
        let headers = table
            .remove(0)
            .into_iter()
            .map(|s| normalize_header(&s))
            .collect::<Vec<_>>();
        let url_col = find_header(
            &headers,
            &["url", "uri", "website", "loginurl", "loginuri", "hostname"],
        );
        let user_col = find_header(
            &headers,
            &["username", "user", "login", "loginusername", "email"],
        );
        let pass_col = find_header(&headers, &["password", "pass", "loginpassword"]);
        let (Some(url_col), Some(user_col), Some(pass_col)) = (url_col, user_col, pass_col) else {
            return Err(PasswordError::UnsupportedHeaders);
        };
        let mut rows: Vec<(Option<Record>, Classification)> = Vec::new();
        let mut preview = Vec::new();
        for (index, fields) in table.iter().enumerate() {
            if index >= MAX_IMPORT_ROWS {
                return Err(PasswordError::TooManyRows);
            }
            let url = fields.get(url_col).map(|s| s.trim()).unwrap_or("");
            let username = fields.get(user_col).map(|s| s.trim()).unwrap_or("");
            let password = fields.get(pass_col).map(String::as_str).unwrap_or("");
            let canonical = canonical_origin(url);
            let record = canonical
                .filter(|_| valid_record(url, username, password))
                .map(|origin| Record {
                    url: origin,
                    username: username.to_owned(),
                    password: password.to_owned(),
                });
            let class = match &record {
                None => Classification::Reject,
                Some(r) => classify(
                    r,
                    self.records.iter().chain(rows.iter().filter_map(|(r, c)| {
                        (*c != Classification::Reject)
                            .then_some(r.as_ref())
                            .flatten()
                    })),
                ),
            };
            preview.push(PreviewRow {
                row: index + 2,
                classification: class,
                url: record.as_ref().map(|r| r.url.clone()),
                username: record.as_ref().map(|r| r.username.clone()),
            });
            rows.push((record, class));
        }
        Ok(ImportPlan { rows, preview })
    }

    /// Add only new entries. Existing exact duplicates and differing credentials are always skipped.
    pub fn commit(&mut self, plan: ImportPlan) -> Result<usize, PasswordError> {
        let previous_len = self.records.len();
        let mut added = 0;
        for (record, class) in plan.rows {
            if class != Classification::New {
                continue;
            }
            if let Some(record) = record {
                // Reclassify at commit in case the store changed since preview.
                if classify(&record, self.records.iter()) == Classification::New {
                    self.records.push(record);
                    added += 1;
                }
            }
        }
        if added > 0 {
            if let Err(error) = self.persist() {
                self.records.truncate(previous_len);
                return Err(error);
            }
        }
        Ok(added)
    }

    fn persist(&self) -> Result<(), PasswordError> {
        let plain = Zeroizing::new(encode_store(&self.records)?);
        let encrypted = dpapi_protect(&plain)?;
        let parent = self.path.parent().ok_or(PasswordError::Io)?;
        fs::create_dir_all(parent)?;
        let temp = self.path.with_extension("dat.tmp");
        fs::write(&temp, encrypted)?;
        fs::rename(&temp, &self.path)?;
        Ok(())
    }
}

fn valid_record(url: &str, username: &str, password: &str) -> bool {
    !url.is_empty()
        && !username.is_empty()
        && !password.is_empty()
        && url.len() <= 4096
        && username.len() <= 4096
        && password.len() <= 16_384
        && !url.chars().any(char::is_control)
        && canonical_origin(url).is_some()
}
fn canonical_origin(value: &str) -> Option<String> {
    let parsed = url::Url::parse(value.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    Some(parsed.origin().ascii_serialization())
}
fn classify<'a>(candidate: &Record, existing: impl Iterator<Item = &'a Record>) -> Classification {
    let matches = existing
        .filter(|r| r.url == candidate.url && r.username == candidate.username)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        Classification::New
    } else if matches.iter().any(|r| r.password == candidate.password) {
        Classification::ExactDuplicate
    } else {
        Classification::Conflict
    }
}
fn normalize_header(s: &str) -> String {
    s.trim()
        .trim_start_matches('\u{feff}')
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
fn find_header(headers: &[String], aliases: &[&str]) -> Option<usize> {
    headers.iter().position(|h| aliases.contains(&h.as_str()))
}

fn parse_csv(input: &[u8]) -> Result<Vec<Vec<String>>, PasswordError> {
    let input = input.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(input);
    let text = std::str::from_utf8(input).map_err(|_| PasswordError::InvalidCsv)?;
    let mut rows = Zeroizing::new(Vec::new());
    let mut row = Zeroizing::new(Vec::new());
    let mut field = Zeroizing::new(String::new());
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    let mut closed = false;
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => {
                    quoted = false;
                    closed = true;
                }
                _ => field.push(c),
            }
        } else if closed {
            match c {
                ',' => {
                    row.push(std::mem::take(&mut *field));
                    closed = false;
                }
                '\r' | '\n' => {
                    if c == '\r' && chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    row.push(std::mem::take(&mut *field));
                    rows.push(std::mem::take(&mut *row));
                    closed = false;
                }
                c if c.is_whitespace() => {}
                _ => return Err(PasswordError::InvalidCsv),
            }
        } else {
            match c {
                '"' if field.is_empty() => quoted = true,
                '"' => return Err(PasswordError::InvalidCsv),
                ',' => row.push(std::mem::take(&mut *field)),
                '\r' | '\n' => {
                    if c == '\r' && chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    row.push(std::mem::take(&mut *field));
                    rows.push(std::mem::take(&mut *row));
                }
                _ => field.push(c),
            }
        }
        if rows.len() > MAX_IMPORT_ROWS + 1 {
            return Err(PasswordError::TooManyRows);
        }
    }
    if quoted {
        return Err(PasswordError::InvalidCsv);
    }
    if closed || !field.is_empty() || !row.is_empty() {
        row.push(std::mem::take(&mut *field));
        rows.push(std::mem::take(&mut *row));
    }
    if rows.len() > MAX_IMPORT_ROWS + 1 {
        return Err(PasswordError::TooManyRows);
    }
    Ok(std::mem::take(&mut *rows))
}

fn encode_store(records: &[Record]) -> Result<Vec<u8>, PasswordError> {
    let mut out = MAGIC.to_vec();
    out.extend(VERSION.to_le_bytes());
    out.extend(
        u32::try_from(records.len())
            .map_err(|_| PasswordError::InvalidStore)?
            .to_le_bytes(),
    );
    for r in records {
        for s in [&r.url, &r.username, &r.password] {
            out.extend(
                u32::try_from(s.len())
                    .map_err(|_| PasswordError::InvalidStore)?
                    .to_le_bytes(),
            );
            out.extend(s.as_bytes());
        }
    }
    if out.len() > MAX_STORE_BYTES {
        return Err(PasswordError::InvalidStore);
    }
    Ok(out)
}
fn decode_store(bytes: &[u8]) -> Result<Vec<Record>, PasswordError> {
    if bytes.len() < 16
        || &bytes[..8] != MAGIC
        || u32::from_le_bytes(bytes[8..12].try_into().unwrap()) != VERSION
    {
        return Err(PasswordError::InvalidStore);
    }
    let count = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    if count > MAX_IMPORT_ROWS {
        return Err(PasswordError::InvalidStore);
    }
    let mut at = 16;
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        let mut vals = Vec::new();
        for _ in 0..3 {
            if at + 4 > bytes.len() {
                return Err(PasswordError::InvalidStore);
            }
            let n = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            at += 4;
            if n > bytes.len().saturating_sub(at) {
                return Err(PasswordError::InvalidStore);
            }
            vals.push(Zeroizing::new(
                std::str::from_utf8(&bytes[at..at + n])
                    .map_err(|_| PasswordError::InvalidStore)?
                    .to_owned(),
            ));
            at += n;
        }
        if !valid_record(&vals[0], &vals[1], &vals[2]) {
            return Err(PasswordError::InvalidStore);
        }
        records.push(Record {
            url: std::mem::take(&mut *vals.remove(0)),
            username: std::mem::take(&mut *vals.remove(0)),
            password: std::mem::take(&mut *vals.remove(0)),
        });
    }
    if at != bytes.len() {
        return Err(PasswordError::InvalidStore);
    }
    Ok(records)
}

#[cfg(windows)]
fn dpapi_protect(input: &[u8]) -> Result<Vec<u8>, PasswordError> {
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };
    let len = u32::try_from(input.len()).map_err(|_| PasswordError::InvalidStore)?;
    let blob = CRYPT_INTEGER_BLOB {
        cbData: len,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &blob,
            windows::core::PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|_| PasswordError::Protection)?;
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            output.pbData.cast(),
        )))
    };
    Ok(bytes)
}
#[cfg(windows)]
fn dpapi_unprotect(input: &[u8]) -> Result<Vec<u8>, PasswordError> {
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };
    let len = u32::try_from(input.len()).map_err(|_| PasswordError::InvalidStore)?;
    let blob = CRYPT_INTEGER_BLOB {
        cbData: len,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &blob,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|_| PasswordError::Protection)?;
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize).zeroize() };
    unsafe {
        windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            output.pbData.cast(),
        )))
    };
    Ok(bytes)
}
#[cfg(not(windows))]
fn dpapi_protect(_: &[u8]) -> Result<Vec<u8>, PasswordError> {
    Err(PasswordError::Protection)
}
#[cfg(not(windows))]
fn dpapi_unprotect(_: &[u8]) -> Result<Vec<u8>, PasswordError> {
    Err(PasswordError::Protection)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn store() -> PasswordStore {
        PasswordStore {
            path: PathBuf::new(),
            records: vec![Record {
                url: "https://example.test".into(),
                username: "alice".into(),
                password: "synthetic-secret-731".into(),
            }],
        }
    }
    #[test]
    fn parses_bom_quotes_and_crlf_and_classifies_duplicates_conflicts() {
        let s = store();
        let csv = b"\xef\xbb\xbfname,url,username,password\r\n\"Example, Inc\",https://example.test,alice,\"synthetic-secret-731\"\r\nExample,https://example.test,alice,different\r\nFirefox,https://other.test,bob,secret\r\n";
        let p = s.preview_csv(csv).unwrap();
        assert_eq!(p.rows()[0].classification, Classification::ExactDuplicate);
        assert_eq!(p.rows()[1].classification, Classification::Conflict);
        assert_eq!(p.rows()[2].classification, Classification::New);
        assert!(!format!("{p:?}").contains("synthetic-secret-731"));
    }
    #[test]
    fn supports_vendor_headers_and_embedded_newlines() {
        let s = PasswordStore {
            path: PathBuf::new(),
            records: vec![],
        };
        for csv in [
            b"url,username,password\nhttps://a.test,u,p\n".as_slice(),
            b"URL,Username,Password\r\nhttps://a.test,u,p\r\n",
            b"title,url,username,password\n\"A\nSite\",https://a.test,u,p\n",
            b"name,login_uri,login_username,login_password\nA,https://a.test,u,p\n",
            b"Title,URL,Username,Password\nA,https://a.test,u,p\n",
        ] {
            assert_eq!(
                s.preview_csv(csv).unwrap().rows()[0].classification,
                Classification::New
            );
        }
    }
    #[test]
    fn rejects_bad_csv_headers_limits_and_never_formats_passwords() {
        let s = store();
        assert_eq!(
            s.preview_csv(b"url,username,password\n\"unterminated,u,synthetic-secret-731")
                .unwrap_err(),
            PasswordError::InvalidCsv
        );
        assert_eq!(
            s.preview_csv(b"site,login,secret\na,b,c").unwrap_err(),
            PasswordError::UnsupportedHeaders
        );
        assert_eq!(
            s.preview_csv(&vec![b'x'; MAX_IMPORT_BYTES + 1])
                .unwrap_err(),
            PasswordError::TooLarge
        );
        assert!(!format!("{:?}", s.records[0]).contains("synthetic-secret-731"));
        assert!(
            !PasswordError::InvalidCsv
                .to_string()
                .contains("synthetic-secret-731")
        );
    }
    #[test]
    fn codec_roundtrip_and_trailing_data_rejection() {
        let records = store().records;
        let bytes = encode_store(&records).unwrap();
        assert_eq!(decode_store(&bytes).unwrap(), records);
        let mut bad = bytes;
        bad.push(0);
        assert_eq!(decode_store(&bad).unwrap_err(), PasswordError::InvalidStore);
    }
    #[test]
    fn canonical_origin_and_sequential_import_duplicates() {
        let s = PasswordStore {
            path: PathBuf::new(),
            records: vec![],
        };
        let p = s.preview_csv(b"url,username,password\nhttps://Example.test/a?x=1,u,p\nhttps://example.test/b#frag,u,p\nhttps://example.test/,u,q\nfile:///tmp,u,p\nhttps://user:pass@example.test,u,p\nhttps://,u,p\n").unwrap();
        assert_eq!(p.rows()[0].classification, Classification::New);
        assert_eq!(p.rows()[0].url.as_deref(), Some("https://example.test"));
        assert_eq!(p.rows()[1].classification, Classification::ExactDuplicate);
        assert_eq!(p.rows()[2].classification, Classification::Conflict);
        assert_eq!(p.rows()[3].classification, Classification::Reject);
        assert_eq!(p.rows()[4].classification, Classification::Reject);
        assert_eq!(p.rows()[5].classification, Classification::Reject);
    }
    #[cfg(windows)]
    #[test]
    fn windows_dpapi_file_roundtrip_and_replace_uses_synthetic_credentials() {
        let root =
            std::env::temp_dir().join(format!("bowser-password-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let path = root.join("Passwords.dat");
        let mut store = PasswordStore::open(&path).unwrap();
        let first = store
            .preview_csv(b"url,username,password\nhttps://a.test,u,synthetic-one\n")
            .unwrap();
        assert_eq!(store.commit(first).unwrap(), 1);
        let mut reopened = PasswordStore::open(&path).unwrap();
        assert_eq!(reopened.len(), 1);
        let second = reopened
            .preview_csv(b"url,username,password\nhttps://b.test,v,synthetic-two\n")
            .unwrap();
        assert_eq!(reopened.commit(second).unwrap(), 1);
        assert_eq!(PasswordStore::open(&path).unwrap().len(), 2);
        drop(reopened);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}
