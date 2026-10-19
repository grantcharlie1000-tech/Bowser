//! Local, DPAPI-protected password storage and import planning.

use std::{
    fmt, fs,
    io::{self, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use zeroize::{Zeroize, Zeroizing};

const MAGIC: &[u8; 8] = b"BWSRPW01";
const VERSION: u32 = 2;
const LEGACY_VERSION: u32 = 1;
pub const MAX_IMPORT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IMPORT_ROWS: usize = 10_000;
const MAX_STORE_BYTES: usize = 32 * 1024 * 1024;
const MAX_PASSWORD_BYTES: usize = 16_384;
const MAX_NEVER_SITES: usize = 10_000;
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
    never_sites: Vec<String>,
}

/// Secret text that zeroizes on drop and never prints its contents through Debug.
pub struct SecretString(Zeroizing<String>);

impl SecretString {
    fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }
}

impl std::ops::Deref for SecretString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.0.as_str()
    }
}

impl AsRef<str> for SecretString {
    fn as_ref(&self) -> &str {
        self
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl fmt::Debug for PasswordStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PasswordStore")
            .field("path", &"[LOCAL VAULT]")
            .field("records", &self.records.len())
            .field("never_sites", &self.never_sites.len())
            .finish()
    }
}

/// Opaque index for one entry in a currently opened vault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CredentialId(usize);

/// Non-secret metadata for a vault entry. The password is never included in this value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CredentialSummary {
    pub id: CredentialId,
    pub origin: String,
    pub host: String,
    pub username: String,
}

/// Non-secret result for comparing a submitted login with an entry for the same site and user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginMatch {
    Missing,
    Unchanged(CredentialId),
    Changed(CredentialId),
}

/// A parsed row supplied by an import adapter. The password is zeroized when this value is dropped.
pub struct ParsedCredential {
    pub url: String,
    pub username: String,
    password: Zeroizing<String>,
}

impl ParsedCredential {
    pub fn new(
        url: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            username: username.into(),
            password: Zeroizing::new(password.into()),
        }
    }
}

impl fmt::Debug for ParsedCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParsedCredential")
            .field("url", &self.url)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
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
    Disabled,
    InsecureOrigin,
    Duplicate,
    Conflict,
    MissingEntry,
    InvalidGenerator,
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
            Self::Disabled => "Password storage is unavailable in this profile",
            Self::InsecureOrigin => "Passwords require HTTPS, except on localhost",
            Self::Duplicate => "That password entry already exists",
            Self::Conflict => "A different password already exists for this site and username",
            Self::MissingEntry => "That password entry is no longer available",
            Self::InvalidGenerator => "Password generator settings are invalid",
        })
    }
}
impl std::error::Error for PasswordError {}
impl From<io::Error> for PasswordError {
    fn from(_: io::Error) -> Self {
        Self::Io
    }
}

/// The cheap startup check behind `PasswordStore::login_events_available`: a missing vault file is
/// fine (the first save creates it), an existing one must be a regular file `open` would accept by
/// size. Metadata only; the file is neither read nor decrypted.
fn vault_file_usable(path: &std::path::Path) -> bool {
    match fs::metadata(path) {
        Ok(meta) => meta.is_file() && meta.len() <= MAX_STORE_BYTES as u64,
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

impl PasswordStore {
    /// Open the vault of the running profile (`profile::active`). DPAPI uses current-user scope only.
    pub fn open_default() -> Result<Self, PasswordError> {
        let profile = crate::profile::active().map_err(|_| PasswordError::Io)?;
        if profile.is_guest() {
            return Err(PasswordError::Disabled);
        }
        Self::open(profile.passwords_file())
    }

    /// Whether login events may reach the vault of the running profile. Decided from file
    /// metadata alone: nothing is read or decrypted until a login event actually asks for the
    /// vault (`open_default` then). Guest profiles stay disabled.
    pub fn login_events_available() -> bool {
        crate::profile::active()
            .is_ok_and(|profile| !profile.is_guest() && vault_file_usable(&profile.passwords_file()))
    }

    /// Open a vault at an explicit local path (useful for isolated app profiles and tests).
    fn open(path: impl Into<PathBuf>) -> Result<Self, PasswordError> {
        let path = path.into();
        let encrypted = match fs::read(&path) {
            Ok(encrypted) => encrypted,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self {
                    path,
                    records: Vec::new(),
                    never_sites: Vec::new(),
                });
            }
            Err(error) => return Err(error.into()),
        };
        if encrypted.len() > MAX_STORE_BYTES {
            return Err(PasswordError::InvalidStore);
        }
        let plain = Zeroizing::new(dpapi_unprotect(&encrypted)?);
        let (records, never_sites) = decode_store(&plain)?;
        Ok(Self {
            path,
            records,
            never_sites,
        })
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Lists all entries without retrieving password values.
    pub fn list(&self) -> Vec<CredentialSummary> {
        self.summaries(self.records.iter().enumerate())
    }

    /// Searches the origin host and username without retrieving password values.
    pub fn search(&self, query: &str) -> Vec<CredentialSummary> {
        let needle = query.to_lowercase();
        self.summaries(self.records.iter().enumerate())
            .into_iter()
            .filter(|entry| {
                needle.is_empty()
                    || entry.host.to_lowercase().contains(&needle)
                    || entry.username.to_lowercase().contains(&needle)
            })
            .collect()
    }

    /// Returns entries whose complete scheme/host/port origin exactly matches this secure URL.
    /// Subdomains are intentionally distinct; the public suffix list rejects public-suffix-only
    /// hosts while URL origin serialization handles case, default ports, and path removal.
    pub fn get_for_origin(&self, value: &str) -> Vec<CredentialSummary> {
        let Some(origin) = secure_origin(value) else {
            return Vec::new();
        };
        self.summaries(self.records.iter().enumerate())
            .into_iter()
            .filter(|entry| entry.origin == origin)
            .collect()
    }

    /// Compares a submitted value without returning any stored password bytes.
    pub fn match_login(&self, value: &str, username: &str, password: &str) -> LoginMatch {
        let Some(origin) = secure_origin(value) else {
            return LoginMatch::Missing;
        };
        let mut matches = self.records.iter().enumerate().filter(|(_, record)| {
            secure_origin(&record.url).as_deref() == Some(origin.as_str())
                && record.username == username
        });
        let Some((first_index, first_record)) = matches.next() else {
            return LoginMatch::Missing;
        };
        if first_record.password == password {
            return LoginMatch::Unchanged(CredentialId(first_index));
        }
        matches
            .find(|(_, record)| record.password == password)
            .map_or(
                LoginMatch::Changed(CredentialId(first_index)),
                |(index, _)| LoginMatch::Unchanged(CredentialId(index)),
            )
    }

    /// Returns a password only through this explicit reveal operation.
    pub fn reveal(&self, id: CredentialId) -> Result<SecretString, PasswordError> {
        self.records
            .get(id.0)
            .map(|record| SecretString::new(record.password.clone()))
            .ok_or(PasswordError::MissingEntry)
    }

    /// Adds a new HTTPS credential, or an HTTP credential for localhost.
    pub fn add(
        &mut self,
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<CredentialId, PasswordError> {
        let origin = secure_origin(url).ok_or(PasswordError::InsecureOrigin)?;
        if !valid_record(&origin, username, password) {
            return Err(PasswordError::InvalidStore);
        }
        let candidate = Record {
            url: origin,
            username: username.to_owned(),
            password: password.to_owned(),
        };
        match classify(&candidate, self.records.iter()) {
            Classification::ExactDuplicate => return Err(PasswordError::Duplicate),
            Classification::Conflict => return Err(PasswordError::Conflict),
            Classification::Reject => return Err(PasswordError::InvalidStore),
            Classification::New => {}
        }
        let id = CredentialId(self.records.len());
        self.records.push(candidate);
        if let Err(error) = self.persist() {
            self.records.pop();
            return Err(error);
        }
        Ok(id)
    }

    /// Replaces one entry after checking that it does not create a duplicate or conflict.
    pub fn update(
        &mut self,
        id: CredentialId,
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<(), PasswordError> {
        let Some(existing) = self.records.get(id.0) else {
            return Err(PasswordError::MissingEntry);
        };
        let origin = secure_origin(url).ok_or(PasswordError::InsecureOrigin)?;
        if !valid_record(&origin, username, password) {
            return Err(PasswordError::InvalidStore);
        }
        let candidate = Record {
            url: origin,
            username: username.to_owned(),
            password: password.to_owned(),
        };
        let other_records = self
            .records
            .iter()
            .enumerate()
            .filter_map(|(index, record)| (index != id.0).then_some(record));
        match classify(&candidate, other_records) {
            Classification::ExactDuplicate => return Err(PasswordError::Duplicate),
            Classification::Conflict => return Err(PasswordError::Conflict),
            Classification::Reject => return Err(PasswordError::InvalidStore),
            Classification::New => {}
        }
        if existing == &candidate {
            return Ok(());
        }
        let old = std::mem::replace(&mut self.records[id.0], candidate);
        if let Err(error) = self.persist() {
            self.records[id.0] = old;
            return Err(error);
        }
        Ok(())
    }

    /// Deletes an entry and restores it in memory if the atomic write fails.
    pub fn delete(&mut self, id: CredentialId) -> Result<(), PasswordError> {
        if id.0 >= self.records.len() {
            return Err(PasswordError::MissingEntry);
        }
        let removed = self.records.remove(id.0);
        if let Err(error) = self.persist() {
            self.records.insert(id.0, removed);
            return Err(error);
        }
        Ok(())
    }

    /// Deletes every saved login (Delete Browsing Data, after the user confirmed it) and restores
    /// them in memory if the atomic write fails. The "never save for this site" choices stay.
    /// Returns how many logins were removed.
    pub fn delete_all(&mut self) -> Result<usize, PasswordError> {
        if self.records.is_empty() {
            return Ok(0);
        }
        let removed = std::mem::take(&mut self.records);
        if let Err(error) = self.persist() {
            self.records = removed;
            return Err(error);
        }
        Ok(removed.len())
    }

    pub fn is_never_for_site(&self, value: &str) -> bool {
        secure_origin(value).is_some_and(|origin| self.never_sites.contains(&origin))
    }

    /// Adds/removes a per-profile "Never for this site" decision in the encrypted vault.
    pub fn mark_never_for_site(&mut self, value: &str, never: bool) -> Result<(), PasswordError> {
        let origin = secure_origin(value).ok_or(PasswordError::InsecureOrigin)?;
        let Some(index) = self.never_sites.iter().position(|site| site == &origin) else {
            if !never {
                return Ok(());
            }
            if self.never_sites.len() >= MAX_NEVER_SITES {
                return Err(PasswordError::InvalidStore);
            }
            self.never_sites.push(origin);
            if let Err(error) = self.persist() {
                self.never_sites.pop();
                return Err(error);
            }
            return Ok(());
        };
        if never {
            return Ok(());
        }
        let removed = self.never_sites.remove(index);
        if let Err(error) = self.persist() {
            self.never_sites.insert(index, removed);
            return Err(error);
        }
        Ok(())
    }

    fn summaries<'a>(
        &self,
        records: impl Iterator<Item = (usize, &'a Record)>,
    ) -> Vec<CredentialSummary> {
        records
            .map(|(index, record)| {
                let host = url::Url::parse(&record.url)
                    .ok()
                    .and_then(|url| url.host_str().map(str::to_owned))
                    .unwrap_or_default();
                CredentialSummary {
                    id: CredentialId(index),
                    origin: record.url.clone(),
                    host,
                    username: record.username.clone(),
                }
            })
            .collect()
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
        if table.len().saturating_sub(1) > MAX_IMPORT_ROWS {
            return Err(PasswordError::TooManyRows);
        }
        // A blank line is not a login, so it is not a rejected row either.
        table.retain(|fields| !(fields.len() == 1 && fields[0].trim().is_empty()));
        let parsed = table.iter().map(|fields| {
            ParsedCredential::new(
                fields
                    .get(url_col)
                    .map(|value| value.trim())
                    .unwrap_or("")
                    .to_owned(),
                fields
                    .get(user_col)
                    .map(|value| value.trim())
                    .unwrap_or("")
                    .to_owned(),
                fields.get(pass_col).cloned().unwrap_or_default(),
            )
        });
        self.preview_credentials(parsed)
    }

    /// Classifies parsed rows from CSV or another local adapter against this vault and each earlier
    /// accepted row. Adapters share one duplicate/conflict policy without sharing browser-store code.
    pub fn preview_credentials(
        &self,
        credentials: impl IntoIterator<Item = ParsedCredential>,
    ) -> Result<ImportPlan, PasswordError> {
        let mut rows: Vec<(Option<Record>, Classification)> = Vec::new();
        let mut preview = Vec::new();
        for (index, credential) in credentials.into_iter().enumerate() {
            if index >= MAX_IMPORT_ROWS {
                return Err(PasswordError::TooManyRows);
            }
            let url = credential.url.trim();
            let username = credential.username.trim();
            let password = credential.password.as_str();
            let record = secure_origin(url)
                .filter(|origin| valid_record(origin, username, password))
                .map(|origin| Record {
                    url: origin,
                    username: username.to_owned(),
                    password: password.to_owned(),
                });
            let class = match &record {
                None => Classification::Reject,
                Some(candidate) => classify(
                    candidate,
                    self.records
                        .iter()
                        .chain(rows.iter().filter_map(|(record, class)| {
                            (*class != Classification::Reject)
                                .then_some(record.as_ref())
                                .flatten()
                        })),
                ),
            };
            preview.push(PreviewRow {
                row: index + 2,
                classification: class,
                url: record.as_ref().map(|record| record.url.clone()),
                username: record.as_ref().map(|record| record.username.clone()),
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
        let plain = encode_store(&self.records, &self.never_sites)?;
        let encrypted = dpapi_protect(&plain)?;
        let parent = self.path.parent().ok_or(PasswordError::Io)?;
        fs::create_dir_all(parent)?;
        let leaf = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Passwords.dat");
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".{leaf}.{}.{}.tmp", std::process::id(), sequence));
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(&encrypted)?;
            file.sync_all()?;
            drop(file);
            replace_file(&temp, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
}

#[cfg(windows)]
fn replace_file(
    temp: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), PasswordError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        core::PCWSTR,
    };
    let temp: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe {
        MoveFileExW(
            PCWSTR(temp.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|_| PasswordError::Io)
}

#[cfg(not(windows))]
fn replace_file(
    temp: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), PasswordError> {
    fs::rename(temp, destination).map_err(|_| PasswordError::Io)
}

fn valid_record(url: &str, username: &str, password: &str) -> bool {
    !url.is_empty()
        && !username.is_empty()
        && !password.is_empty()
        && url.len() <= 4096
        && username.len() <= 4096
        && password.len() <= MAX_PASSWORD_BYTES
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

/// Canonical secure origin for credential retrieval and save flows. HTTPS is accepted for public
/// sites; HTTP is restricted to the literal localhost host. The URL crate canonicalizes the origin
/// and the PSL rejects public-suffix-only domain names.
pub(crate) fn secure_origin(value: &str) -> Option<String> {
    let parsed = url::Url::parse(value.trim()).ok()?;
    if !matches!(parsed.scheme(), "https" | "http")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    let host = parsed.host_str()?;
    if parsed.scheme() == "http" && !host.eq_ignore_ascii_case("localhost") {
        return None;
    }
    if !host.eq_ignore_ascii_case("localhost")
        && host.parse::<std::net::IpAddr>().is_err()
        && psl::domain(host.as_bytes()).is_none()
    {
        return None;
    }
    Some(parsed.origin().ascii_serialization())
}

pub(crate) const MAX_PAGE_MESSAGE_BYTES: usize = 32 * 1024;
const MAX_PAGE_USERNAME_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginMessageKind {
    FormSeen,
    Submitted,
}

/// Validated page data. Secret fields zeroize on drop and Debug never includes their contents.
pub struct LoginMessage {
    pub(crate) kind: LoginMessageKind,
    pub(crate) origin: String,
    pub(crate) username: Option<SecretString>,
    pub(crate) password: Option<SecretString>,
}

impl fmt::Debug for LoginMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginMessage")
            .field("kind", &self.kind)
            .field("origin", &self.origin)
            .field("username", &self.username.as_ref().map(|_| "[PRESENT]"))
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

/// Accepts only the injected script's exact JSON shapes and the actual top-frame/current-origin
/// pair supplied by WebView2. The origin is never read from the message payload.
pub(crate) fn validate_login_message(
    json: &str,
    source_url: &str,
    top_url: &str,
    top_frame_event: bool,
) -> Option<LoginMessage> {
    if json.len() > MAX_PAGE_MESSAGE_BYTES || !top_frame_event {
        return None;
    }
    let origin = secure_origin(top_url)?;
    if secure_origin(source_url).as_deref() != Some(origin.as_str()) {
        return None;
    }
    parse_login_message(json, origin)
}

fn parse_login_message(json: &str, origin: String) -> Option<LoginMessage> {
    let mut at = 0;
    skip_json_space(json, &mut at);
    expect_json_byte(json, &mut at, b'{')?;
    let key = parse_json_string(json, &mut at)?;
    if key.as_str() != "kind" {
        return None;
    }
    skip_json_space(json, &mut at);
    expect_json_byte(json, &mut at, b':')?;
    let kind = parse_json_string(json, &mut at)?;
    match kind.as_str() {
        "login-form-seen" => {
            skip_json_space(json, &mut at);
            expect_json_byte(json, &mut at, b'}')?;
            skip_json_space(json, &mut at);
            (at == json.len()).then_some(LoginMessage {
                kind: LoginMessageKind::FormSeen,
                origin,
                username: None,
                password: None,
            })
        }
        "login-submitted" => {
            skip_json_space(json, &mut at);
            expect_json_byte(json, &mut at, b',')?;
            let key = parse_json_string(json, &mut at)?;
            if key.as_str() != "user" {
                return None;
            }
            skip_json_space(json, &mut at);
            expect_json_byte(json, &mut at, b':')?;
            let mut username = parse_json_string(json, &mut at)?;
            if username.len() > MAX_PAGE_USERNAME_BYTES {
                return None;
            }
            skip_json_space(json, &mut at);
            expect_json_byte(json, &mut at, b',')?;
            let key = parse_json_string(json, &mut at)?;
            if key.as_str() != "pass" {
                return None;
            }
            skip_json_space(json, &mut at);
            expect_json_byte(json, &mut at, b':')?;
            let mut password = parse_json_string(json, &mut at)?;
            if password.is_empty() || password.len() > MAX_PASSWORD_BYTES {
                return None;
            }
            skip_json_space(json, &mut at);
            expect_json_byte(json, &mut at, b'}')?;
            skip_json_space(json, &mut at);
            (at == json.len()).then_some(LoginMessage {
                kind: LoginMessageKind::Submitted,
                origin,
                username: Some(SecretString::new(std::mem::take(&mut *username))),
                password: Some(SecretString::new(std::mem::take(&mut *password))),
            })
        }
        _ => None,
    }
}

fn skip_json_space(json: &str, at: &mut usize) {
    while json
        .as_bytes()
        .get(*at)
        .is_some_and(u8::is_ascii_whitespace)
    {
        *at += 1;
    }
}

fn expect_json_byte(json: &str, at: &mut usize, expected: u8) -> Option<()> {
    skip_json_space(json, at);
    if json.as_bytes().get(*at) == Some(&expected) {
        *at += 1;
        Some(())
    } else {
        None
    }
}

fn parse_json_string(json: &str, at: &mut usize) -> Option<Zeroizing<String>> {
    let bytes = json.as_bytes();
    expect_json_byte(json, at, b'"')?;
    let mut output = Zeroizing::new(String::with_capacity(json.len().saturating_sub(*at)));
    while *at < bytes.len() {
        match bytes[*at] {
            b'"' => {
                *at += 1;
                return Some(output);
            }
            b'\\' => {
                *at += 1;
                let escaped = *bytes.get(*at)?;
                *at += 1;
                match escaped {
                    b'"' => output.push('"'),
                    b'\\' => output.push('\\'),
                    b'/' => output.push('/'),
                    b'b' => output.push('\u{0008}'),
                    b'f' => output.push('\u{000C}'),
                    b'n' => output.push('\n'),
                    b'r' => output.push('\r'),
                    b't' => output.push('\t'),
                    b'u' => {
                        let first = parse_json_hex_unit(bytes, at)?;
                        let scalar = if (0xD800..=0xDBFF).contains(&first) {
                            if bytes.get(*at..*at + 2)? != b"\\u" {
                                return None;
                            }
                            *at += 2;
                            let second = parse_json_hex_unit(bytes, at)?;
                            if !(0xDC00..=0xDFFF).contains(&second) {
                                return None;
                            }
                            0x1_0000
                                + ((u32::from(first) - 0xD800) << 10)
                                + (u32::from(second) - 0xDC00)
                        } else if (0xDC00..=0xDFFF).contains(&first) {
                            return None;
                        } else {
                            u32::from(first)
                        };
                        output.push(char::from_u32(scalar)?);
                    }
                    _ => return None,
                }
            }
            byte if byte < 0x20 => return None,
            _ => {
                let character = json.get(*at..)?.chars().next()?;
                output.push(character);
                *at += character.len_utf8();
            }
        }
        if output.len() > MAX_PAGE_MESSAGE_BYTES {
            return None;
        }
    }
    None
}

fn parse_json_hex_unit(bytes: &[u8], at: &mut usize) -> Option<u16> {
    let hex = bytes.get(*at..*at + 4)?;
    *at += 4;
    let mut value = 0u16;
    for byte in hex {
        value = (value << 4)
            | match byte {
                b'0'..=b'9' => u16::from(*byte - b'0'),
                b'a'..=b'f' => u16::from(*byte - b'a' + 10),
                b'A'..=b'F' => u16::from(*byte - b'A' + 10),
                _ => return None,
            };
    }
    Some(value)
}

/// Builds a top-document-only fill script. The output itself is a zeroizing secret buffer because
/// it contains the password until WebView2 copies the script for execution.
pub(crate) fn password_fill_script(
    origin: &str,
    username: &str,
    password: &str,
) -> Zeroizing<String> {
    let capacity = origin
        .len()
        .saturating_add(username.len())
        .saturating_add(password.len())
        .saturating_mul(6)
        .saturating_add(1200);
    let mut script = Zeroizing::new(String::with_capacity(capacity));
    script.push_str("(()=>{if(window.top!==window)return false;if(location.origin!==");
    append_js_string(&mut script, origin);
    script.push_str(")return false;const v=e=>{if(!e||e.hidden||e.disabled||e.readOnly)return false;const r=e.getBoundingClientRect(),s=getComputedStyle(e);return r.width>0&&r.height>0&&r.bottom>0&&r.right>0&&r.top<innerHeight&&r.left<innerWidth&&s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)>0};const p=[...document.querySelectorAll('input[type=password]')].find(e=>e.type==='password'&&v(e));if(!p)return false;const sel='input[autocomplete=username],input[type=email],input[type=text]';const before=e=>!!(e.compareDocumentPosition(p)&Node.DOCUMENT_POSITION_FOLLOWING);const isUser=e=>(e.type==='text'||e.type==='email')&&e!==p&&v(e);const u=p.form?[...p.form.querySelectorAll(sel)].filter(e=>isUser(e)&&before(e)).at(-1):p.parentElement?[...p.parentElement.querySelectorAll('input[type=email],input[type=text]')].filter(e=>isUser(e)&&before(e)).at(-1):null;const s=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;if(u){s.call(u,");
    append_js_string(&mut script, username);
    script.push_str(");u.dispatchEvent(new Event('input',{bubbles:true}));u.dispatchEvent(new Event('change',{bubbles:true}));}s.call(p,");
    append_js_string(&mut script, password);
    script.push_str(");p.dispatchEvent(new Event('input',{bubbles:true}));p.dispatchEvent(new Event('change',{bubbles:true}));return true})()");
    script
}

fn append_js_string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{2028}' => output.push_str("\\u2028"),
            '\u{2029}' => output.push_str("\\u2029"),
            character if character.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(output, "\\u{:04x}", character as u32);
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

/// Generates a password with the Windows system-preferred BCrypt CSPRNG and rejection sampling.
/// The caller supplies a unique set of printable ASCII characters and a length from 8 to 256.
pub fn generate_password(length: usize, charset: &[u8]) -> Result<SecretString, PasswordError> {
    if !(8..=256).contains(&length)
        || !(2..=95).contains(&charset.len())
        || charset.iter().any(|byte| !(b' '..=b'~').contains(byte))
    {
        return Err(PasswordError::InvalidGenerator);
    }
    let mut seen = [false; 256];
    for byte in charset {
        if std::mem::replace(&mut seen[*byte as usize], true) {
            return Err(PasswordError::InvalidGenerator);
        }
    }
    generate_password_with(length, charset)
}

#[cfg(windows)]
fn generate_password_with(length: usize, charset: &[u8]) -> Result<SecretString, PasswordError> {
    use windows::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
    };
    let accepted_below = 256 / charset.len() * charset.len();
    let mut output = Zeroizing::new(String::with_capacity(length));
    let mut bytes = [0u8; 64];
    while output.len() < length {
        let remaining = length - output.len();
        let chunk_len = remaining.min(bytes.len());
        let chunk = &mut bytes[..chunk_len];
        if unsafe { BCryptGenRandom(None, chunk, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.0 < 0 {
            bytes.zeroize();
            return Err(PasswordError::Protection);
        }
        for byte in chunk.iter().copied() {
            if usize::from(byte) < accepted_below {
                output.push(charset[usize::from(byte) % charset.len()] as char);
                if output.len() == length {
                    break;
                }
            }
        }
    }
    bytes.zeroize();
    Ok(SecretString::new(std::mem::take(&mut *output)))
}

#[cfg(not(windows))]
fn generate_password_with(_length: usize, _charset: &[u8]) -> Result<SecretString, PasswordError> {
    Err(PasswordError::Protection)
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

fn encode_store(
    records: &[Record],
    never_sites: &[String],
) -> Result<Zeroizing<Vec<u8>>, PasswordError> {
    if records.len() > MAX_IMPORT_ROWS || never_sites.len() > MAX_NEVER_SITES {
        return Err(PasswordError::InvalidStore);
    }
    let mut capacity = 20usize;
    for record in records {
        for value in [&record.url, &record.username, &record.password] {
            u32::try_from(value.len()).map_err(|_| PasswordError::InvalidStore)?;
            capacity = capacity
                .checked_add(4 + value.len())
                .ok_or(PasswordError::InvalidStore)?;
        }
    }
    for site in never_sites {
        if secure_origin(site).as_deref() != Some(site.as_str()) {
            return Err(PasswordError::InvalidStore);
        }
        u32::try_from(site.len()).map_err(|_| PasswordError::InvalidStore)?;
        capacity = capacity
            .checked_add(4 + site.len())
            .ok_or(PasswordError::InvalidStore)?;
    }
    if capacity > MAX_STORE_BYTES {
        return Err(PasswordError::InvalidStore);
    }
    let mut out = Zeroizing::new(Vec::with_capacity(capacity));
    out.extend_from_slice(MAGIC);
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
    out.extend(
        u32::try_from(never_sites.len())
            .map_err(|_| PasswordError::InvalidStore)?
            .to_le_bytes(),
    );
    for site in never_sites {
        out.extend(
            u32::try_from(site.len())
                .map_err(|_| PasswordError::InvalidStore)?
                .to_le_bytes(),
        );
        out.extend(site.as_bytes());
    }
    if out.len() > MAX_STORE_BYTES {
        return Err(PasswordError::InvalidStore);
    }
    Ok(out)
}
fn decode_store(bytes: &[u8]) -> Result<(Vec<Record>, Vec<String>), PasswordError> {
    if bytes.len() < 16 || &bytes[..8] != MAGIC {
        return Err(PasswordError::InvalidStore);
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    if !matches!(version, LEGACY_VERSION | VERSION) {
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
    let mut never_sites = Vec::new();
    if version == VERSION {
        if at + 4 > bytes.len() {
            return Err(PasswordError::InvalidStore);
        }
        let count = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        at += 4;
        if count > MAX_NEVER_SITES {
            return Err(PasswordError::InvalidStore);
        }
        never_sites.reserve(count);
        for _ in 0..count {
            if at + 4 > bytes.len() {
                return Err(PasswordError::InvalidStore);
            }
            let n = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            at += 4;
            if n > bytes.len().saturating_sub(at) {
                return Err(PasswordError::InvalidStore);
            }
            let site = std::str::from_utf8(&bytes[at..at + n])
                .map_err(|_| PasswordError::InvalidStore)?
                .to_owned();
            at += n;
            if secure_origin(&site).as_deref() != Some(site.as_str()) || never_sites.contains(&site)
            {
                return Err(PasswordError::InvalidStore);
            }
            never_sites.push(site);
        }
    }
    if at != bytes.len() {
        return Err(PasswordError::InvalidStore);
    }
    Ok((records, never_sites))
}

#[cfg(test)]
fn encode_v1_store(records: &[Record]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend(LEGACY_VERSION.to_le_bytes());
    out.extend((records.len() as u32).to_le_bytes());
    for record in records {
        for value in [&record.url, &record.username, &record.password] {
            out.extend((value.len() as u32).to_le_bytes());
            out.extend(value.as_bytes());
        }
    }
    out
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
            never_sites: Vec::new(),
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

    /// P-24: blank lines between rows or at the end are skipped, not reported as failed rows.
    #[test]
    fn blank_csv_lines_are_not_rejected_rows() {
        let s = store();
        let csv = b"url,username,password\r\n\r\nhttps://one.test,a,secret-1\n\nhttps://two.test,b,secret-2\r\n\r\n";
        let p = s.preview_csv(csv).unwrap();
        let classes: Vec<_> = p.rows().iter().map(|row| row.classification).collect();
        assert_eq!(classes, [Classification::New, Classification::New]);
        // A row with fields that are all empty is still a row, and still rejected.
        let p = s.preview_csv(b"url,username,password\n,,\n").unwrap();
        assert_eq!(p.rows().len(), 1);
        assert_eq!(p.rows()[0].classification, Classification::Reject);
    }

    #[test]
    fn parsed_source_rows_share_csv_preview_duplicate_and_conflict_rules() {
        let vault = store();
        let parsed = vec![
            ParsedCredential::new(
                "https://example.test/login",
                "alice",
                "synthetic-secret-731",
            ),
            ParsedCredential::new("https://example.test", "alice", "different-secret"),
            ParsedCredential::new("https://other.test", "bob", "new-secret"),
            ParsedCredential::new("file:///tmp/login", "guest", "rejected-secret"),
        ];
        assert!(!format!("{:?}", parsed[2]).contains("new-secret"));
        let plan = vault.preview_credentials(parsed).unwrap();
        assert_eq!(
            plan.rows()[0].classification,
            Classification::ExactDuplicate
        );
        assert_eq!(plan.rows()[1].classification, Classification::Conflict);
        assert_eq!(plan.rows()[2].classification, Classification::New);
        assert_eq!(plan.rows()[3].classification, Classification::Reject);
        assert!(!format!("{plan:?}").contains("different-secret"));
    }
    #[test]
    fn supports_vendor_headers_and_embedded_newlines() {
        let s = PasswordStore {
            path: PathBuf::new(),
            records: vec![],
            never_sites: Vec::new(),
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
        let bytes = encode_store(&records, &[]).unwrap();
        assert_eq!(decode_store(&bytes).unwrap().0, records);
        let mut bad = bytes;
        bad.push(0);
        assert_eq!(decode_store(&bad).unwrap_err(), PasswordError::InvalidStore);
    }

    #[test]
    fn fill_script_checks_origin_and_only_uses_a_safe_same_form_username_field() {
        let script = password_fill_script(
            "https://example.test",
            "alice\";alert(1)//",
            "synthetic-pass",
        );
        assert!(script.starts_with("(()=>{if(window.top!==window)return false;if(location.origin!==\"https://example.test\")return false;"));
        assert!(script.contains("alice\\\";alert(1)//"));
        assert!(script.contains("e.type==='password'"));
        assert!(script.contains("p.form.querySelectorAll(sel)"));
        assert!(
            script
                .contains("p.parentElement.querySelectorAll('input[type=email],input[type=text]')")
        );
        assert!(script.contains("getBoundingClientRect"));
        assert!(script.contains("getComputedStyle"));
        assert!(script.contains("e.type==='text'||e.type==='email'"));
        assert!(!script.contains("input[type=search]"));
        assert!(!script.contains("p.focus()"));
    }

    #[test]
    fn opening_a_non_file_vault_path_fails_closed() {
        let root = std::env::temp_dir().join(format!(
            "bowser-vault-dir-{}-{}",
            std::process::id(),
            TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();

        assert!(matches!(PasswordStore::open(&root), Err(PasswordError::Io)));
        assert!(
            root.is_dir(),
            "an unreadable vault path must not be replaced"
        );

        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn canonical_origin_and_sequential_import_duplicates() {
        let s = PasswordStore {
            path: PathBuf::new(),
            records: vec![],
            never_sites: Vec::new(),
        };
        let p = s.preview_csv(b"url,username,password\nhttps://Example.test/a?x=1,u,p\nhttps://example.test/b#frag,u,p\nhttps://example.test/,u,q\nfile:///tmp,u,p\nhttps://user:pass@example.test,u,p\nhttps://,u,p\nhttp://example.test,u,p\n").unwrap();
        assert_eq!(p.rows()[0].classification, Classification::New);
        assert_eq!(p.rows()[0].url.as_deref(), Some("https://example.test"));
        assert_eq!(p.rows()[1].classification, Classification::ExactDuplicate);
        assert_eq!(p.rows()[2].classification, Classification::Conflict);
        assert_eq!(p.rows()[3].classification, Classification::Reject);
        assert_eq!(p.rows()[4].classification, Classification::Reject);
        assert_eq!(p.rows()[5].classification, Classification::Reject);
        assert_eq!(p.rows()[6].classification, Classification::Reject);
    }
    #[cfg(windows)]
    #[test]
    fn vault_crud_search_and_reveal_are_explicit_and_persisted() {
        let root = temp_vault_root();
        let path = root.join("Passwords.dat");
        let mut vault = PasswordStore::open(&path).unwrap();
        let id = vault
            .add(
                "https://Example.test/account",
                "Alice@Example.test",
                "crud-secret-a",
            )
            .unwrap();

        let list = vault.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].username, "Alice@Example.test");
        assert_eq!(vault.search("EXAMPLE.TEST").len(), 1);
        assert_eq!(vault.search("alice@").len(), 1);
        assert!(!format!("{list:?}").contains("crud-secret-a"));
        assert_eq!(
            vault.match_login(
                "https://example.test",
                "Alice@Example.test",
                "crud-secret-a"
            ),
            LoginMatch::Unchanged(id)
        );
        assert_eq!(
            vault.match_login("https://example.test", "Alice@Example.test", "changed"),
            LoginMatch::Changed(id)
        );
        assert_eq!(
            vault.match_login(
                "https://evil-example.test",
                "Alice@Example.test",
                "crud-secret-a"
            ),
            LoginMatch::Missing
        );
        assert_eq!(&*vault.reveal(id).unwrap(), "crud-secret-a");

        vault
            .update(
                id,
                "https://example.test/login",
                "alice@example.test",
                "crud-secret-b",
            )
            .unwrap();
        assert_eq!(&*vault.reveal(id).unwrap(), "crud-secret-b");
        drop(vault);

        let mut reopened = PasswordStore::open(&path).unwrap();
        assert_eq!(reopened.len(), 1);
        reopened.delete(id).unwrap();
        assert!(reopened.is_empty());
        assert!(PasswordStore::open(&path).unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn delete_all_removes_every_login_on_disk_and_keeps_the_never_save_sites() {
        let root = temp_vault_root();
        let path = root.join("Passwords.dat");
        let mut vault = PasswordStore::open(&path).unwrap();
        assert_eq!(vault.delete_all().unwrap(), 0, "an empty vault has nothing to delete");
        vault.add("https://a.test/", "alice", "pw-a").unwrap();
        vault.add("https://b.test/", "bob", "pw-b").unwrap();
        vault.mark_never_for_site("https://never.test", true).unwrap();
        assert_eq!(vault.delete_all().unwrap(), 2);
        assert!(vault.is_empty());
        drop(vault);
        let reopened = PasswordStore::open(&path).unwrap();
        assert!(reopened.is_empty(), "the file holds no login any more");
        assert!(reopened.is_never_for_site("https://never.test"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn origin_lookup_is_exact_and_rejects_lookalikes_and_other_subdomains() {
        let vault = store();
        assert_eq!(vault.get_for_origin("https://example.test/login").len(), 1);
        assert!(
            vault
                .get_for_origin("https://evil-example.test/")
                .is_empty()
        );
        assert!(
            vault
                .get_for_origin("https://login.example.test/")
                .is_empty()
        );
        assert!(vault.get_for_origin("http://example.test/").is_empty());
        assert!(
            vault
                .get_for_origin("https://example.test.evil/")
                .is_empty()
        );
    }

    #[cfg(windows)]
    #[test]
    fn only_localhost_can_use_http_and_never_sites_survive_vault_roundtrip() {
        let root = temp_vault_root();
        let path = root.join("Passwords.dat");
        let mut vault = PasswordStore::open(&path).unwrap();
        assert!(
            vault
                .add("http://localhost:8080", "dev", "local-secret")
                .is_ok()
        );
        assert_eq!(
            vault.add("http://example.test", "remote", "remote-secret"),
            Err(PasswordError::InsecureOrigin)
        );
        vault
            .mark_never_for_site("https://Example.test/login", true)
            .unwrap();
        drop(vault);
        let reopened = PasswordStore::open(&path).unwrap();
        assert!(reopened.is_never_for_site("https://example.test/"));
        assert!(!reopened.is_never_for_site("https://sub.example.test/"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_one_vault_migrates_to_the_version_two_codec() {
        let records = vec![Record {
            url: "https://example.test".into(),
            username: "alice".into(),
            password: "legacy-secret".into(),
        }];
        let legacy = encode_v1_store(&records);
        let (decoded, never_sites) = decode_store(&legacy).unwrap();
        assert_eq!(decoded, records);
        assert!(never_sites.is_empty());
        let migrated = encode_store(&decoded, &never_sites).unwrap();
        assert_eq!(u32::from_le_bytes(migrated[8..12].try_into().unwrap()), 2);
        assert_eq!(decode_store(&migrated).unwrap().0, records);
    }

    #[cfg(windows)]
    #[test]
    fn encrypted_version_one_file_opens_and_rewrites_as_version_two() {
        let root = temp_vault_root();
        fs::create_dir_all(&root).unwrap();
        let path = root.join("Passwords.dat");
        let records = vec![Record {
            url: "https://legacy.example.test".into(),
            username: "alice".into(),
            password: "synthetic-legacy-secret".into(),
        }];
        fs::write(&path, dpapi_protect(&encode_v1_store(&records)).unwrap()).unwrap();
        let mut vault = PasswordStore::open(&path).unwrap();
        assert_eq!(vault.len(), 1);
        vault
            .update(
                CredentialId(0),
                "https://legacy.example.test",
                "alice",
                "synthetic-updated-secret",
            )
            .unwrap();
        let plain = Zeroizing::new(dpapi_unprotect(&fs::read(&path).unwrap()).unwrap());
        assert_eq!(
            u32::from_le_bytes(plain[8..12].try_into().unwrap()),
            VERSION
        );
        assert_eq!(PasswordStore::open(&path).unwrap().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn generated_password_obeys_requested_length_charset_and_is_not_biased() {
        // Aggregate many passwords so the bias bound can be wide (>6 sigma)
        // while a real bias (modulo bias, missing symbol) still fails.
        // 200 * 256 = 51200 samples over 4 symbols: expect 12800 each, sd ~98.
        let mut counts = [0usize; 4];
        for _ in 0..200 {
            let password = generate_password(256, b"abcd").unwrap();
            assert_eq!(password.len(), 256);
            assert!(password.bytes().all(|byte| b"abcd".contains(&byte)));
            for byte in password.bytes() {
                counts[(byte - b'a') as usize] += 1;
            }
        }
        assert!(
            counts
                .into_iter()
                .all(|count| (12_100..=13_500).contains(&count)),
            "biased symbol counts: {counts:?}"
        );
        assert_eq!(
            generate_password(7, b"abcdef").unwrap_err(),
            PasswordError::InvalidGenerator
        );
        assert_eq!(
            generate_password(12, b"a").unwrap_err(),
            PasswordError::InvalidGenerator
        );
    }

    #[test]
    fn page_login_messages_are_size_limited_shape_limited_and_origin_bound() {
        let accepted = validate_login_message(
            r#"{"kind":"login-submitted","user":"alice","pass":"message-secret"}"#,
            "https://login.example.test/sign-in",
            "https://login.example.test/home",
            true,
        )
        .unwrap();
        assert_eq!(accepted.kind, LoginMessageKind::Submitted);
        assert_eq!(
            accepted.username.as_ref().map(|value| value.as_ref()),
            Some("alice")
        );
        assert_eq!(
            accepted.password.as_ref().map(|value| value.as_ref()),
            Some("message-secret")
        );
        assert!(!format!("{accepted:?}").contains("message-secret"));

        assert!(
            validate_login_message(
                "{broken",
                "https://example.test/",
                "https://example.test/",
                true
            )
            .is_none()
        );
        assert!(
            validate_login_message(
                &format!("{}", "x".repeat(MAX_PAGE_MESSAGE_BYTES + 1)),
                "https://example.test/",
                "https://example.test/",
                true
            )
            .is_none()
        );
        assert!(
            validate_login_message(
                r#"{"kind":"login-submitted","user":"u","pass":"p","extra":true}"#,
                "https://example.test/",
                "https://example.test/",
                true
            )
            .is_none()
        );
        assert!(
            validate_login_message(
                r#"{"kind":"login-submitted","user":"u","pass":"p"}"#,
                "https://evil-example.test/",
                "https://example.test/",
                true
            )
            .is_none()
        );
        assert!(
            validate_login_message(
                r#"{"kind":"login-submitted","user":"u","pass":"p"}"#,
                "https://example.test/",
                "https://example.test/",
                false
            )
            .is_none()
        );
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

    #[test]
    fn startup_predicate_checks_metadata_without_decrypting() {
        let root = temp_vault_root();
        fs::create_dir_all(&root).unwrap();
        let path = root.join("passwords.bin");
        // No vault yet: login events stay on so the first save can create it.
        assert!(vault_file_usable(&path));
        // Bytes DPAPI could never unprotect: the predicate must not look inside the file.
        fs::write(&path, b"not a vault").unwrap();
        assert!(vault_file_usable(&path));
        assert!(PasswordStore::open(&path).is_err());
        // Not a regular file: no.
        let dir = root.join("dir");
        fs::create_dir(&dir).unwrap();
        assert!(!vault_file_usable(&dir));
        fs::remove_dir_all(root).unwrap();
    }

    fn temp_vault_root() -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "bowser-password-crud-{}-{nonce}",
            std::process::id()
        ))
    }
}
