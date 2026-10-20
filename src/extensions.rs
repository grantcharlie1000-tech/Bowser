//! Unpacked browser extensions for one profile.
//!
//! This module decides what may be installed and keeps the files on disk consistent. It never runs
//! extension code. WebView2 loads the copied folders through `ICoreWebView2Profile7`; the engine
//! reports load results back here so the extension menu can show why one failed.
//!
//! Layout inside a profile: `Extensions\<id>\` holds the copied folder and `extensions.txt` lists
//! one installed extension per line as `id<TAB>enabled(0|1)<TAB>name<TAB>version`.
//!
//! Scope: user-picked unpacked/CRX installs and verified Store packages. Store website script
//! cannot grant consent or load extension code.

use std::{
    collections::{BTreeMap, hash_map::RandomState},
    fs::{self, File},
    hash::{BuildHasher, Hasher},
    io::{self, Read, Write},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

mod artifacts;
mod script_review;

/// Largest `manifest.json` accepted.
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// Extensions one profile may hold.
pub const MAX_INSTALLED: usize = 64;
/// Files in one extension folder, including the manifest.
pub const MAX_FILES: usize = 2_000;
/// Total bytes of one extension folder.
pub const MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
/// Folder nesting below the extension root.
pub const MAX_DEPTH: usize = 12;

const MAX_JSON_DEPTH: usize = 32;
const MAX_NAME_CHARS: usize = 128;
const MAX_LIST_LEN: usize = 256;
const MAX_LIST_ITEM_CHARS: usize = 256;
const MAX_RECORD_TEXT_CHARS: usize = 120;
const MAX_FAILURE_CHARS: usize = 200;
const ID_PREFIX: &str = "ext-";
const ID_LEN: usize = ID_PREFIX.len() + 16;
const MANIFEST_FILE_NAME: &str = "manifest.json";
/// The one root entry starting with `_` that is accepted. WebView2 reserves the others for the
/// system (`E_ACCESSDENIED`), so they are refused before anything is copied or loaded.
const LOCALES_FOLDER: &str = "_locales";
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
/// Open a reparse point itself instead of the target it names.
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Limits {
    files: usize,
    bytes: u64,
    depth: usize,
}

const LIMITS: Limits = Limits {
    files: MAX_FILES,
    bytes: MAX_TOTAL_BYTES,
    depth: MAX_DEPTH,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionError {
    Io,
    NotAFolder,
    MissingManifest,
    ManifestTooLarge,
    InvalidManifest(&'static str),
    LinkNotAllowed,
    UnsupportedEntry,
    InvalidName,
    ReservedRootName,
    TooManyFiles,
    TooLarge,
    TooDeep,
    TooManyInstalled,
    NotInstalled,
    OutsideExtensions,
    InvalidRecords,
}

impl std::fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Io => "The extension files could not be read or copied.",
            Self::NotAFolder => "Choose the folder that contains the extension's manifest.json.",
            Self::MissingManifest => "The folder has no manifest.json file.",
            Self::ManifestTooLarge => "manifest.json is larger than Bowser accepts.",
            Self::InvalidManifest(reason) => reason,
            Self::LinkNotAllowed => "The folder contains a link or junction, which is not allowed.",
            Self::UnsupportedEntry => "The folder contains an entry that is not a file or folder.",
            Self::InvalidName => "The folder contains a file name Bowser does not accept.",
            Self::ReservedRootName => {
                "The folder has a top-level file or folder whose name starts with \"_\" (only _locales is allowed). WebView2 reserves those names."
            }
            Self::TooManyFiles => "The extension has too many files.",
            Self::TooLarge => "The extension is too large.",
            Self::TooDeep => "The extension folders are nested too deeply.",
            Self::TooManyInstalled => "This profile already has the maximum number of extensions.",
            Self::NotInstalled => "That extension is not installed.",
            Self::OutsideExtensions => {
                "Bowser refused to touch a folder outside the Extensions folder."
            }
            Self::InvalidRecords => "The installed extension list is damaged.",
        };
        f.write_str(text)
    }
}

impl std::error::Error for ExtensionError {}

/// The fields Bowser reads from a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub manifest_version: u8,
    pub permissions: Vec<String>,
    pub host_permissions: Vec<String>,
    /// Every `content_scripts[].matches` pattern, in manifest order.
    pub content_script_matches: Vec<String>,
    /// Complete ASCII canonical static declarations, explicit documented defaults and unknown fields.
    pub content_script_declarations: Vec<String>,
    pub optional_permissions: Vec<String>,
    pub optional_host_permissions: Vec<String>,
}

/// A folder that passed inspection, with the manifest and the size of its contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspection {
    pub manifest: Manifest,
    pub files: usize,
    pub bytes: u64,
}

/// One installed extension, as stored in `extensions.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub id: String,
    pub enabled: bool,
    pub name: String,
    pub version: String,
}

/// What engine startup needs: whether any extension is installed, and the enabled ones to load.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StartupPlan {
    pub installed: bool,
    pub enabled: Vec<(String, PathBuf)>,
}

/// Parses and validates a `manifest.json`. Only the fields Bowser shows or relies on are checked;
/// other keys are ignored. Input that is too large, too deeply nested, not UTF-8, not strict JSON,
/// or has duplicate keys is rejected.
pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, ExtensionError> {
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(ExtensionError::ManifestTooLarge);
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ExtensionError::InvalidManifest("manifest.json is not valid UTF-8."))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let root = json::parse(text).ok_or(ExtensionError::InvalidManifest(
        "manifest.json is not valid JSON.",
    ))?;
    let json::Value::Object(fields) = root else {
        return Err(ExtensionError::InvalidManifest(
            "manifest.json must contain a JSON object.",
        ));
    };
    let manifest_version = match json::get(&fields, "manifest_version") {
        Some(json::Value::Number(token)) if token == "2" => 2,
        Some(json::Value::Number(token)) if token == "3" => 3,
        _ => {
            return Err(ExtensionError::InvalidManifest(
                "manifest_version must be 2 or 3.",
            ));
        }
    };
    let name = match json::get(&fields, "name") {
        Some(json::Value::String(name)) if is_plain_text(name, MAX_NAME_CHARS) => name.clone(),
        _ => {
            return Err(ExtensionError::InvalidManifest(
                "name must be text of 1 to 128 characters.",
            ));
        }
    };
    let version = match json::get(&fields, "version") {
        Some(json::Value::String(version)) if is_valid_version(version) => version.clone(),
        _ => {
            return Err(ExtensionError::InvalidManifest(
                "version must be 1 to 4 dot-separated numbers, each 0 to 65535.",
            ));
        }
    };
    Ok(Manifest {
        name,
        version,
        manifest_version,
        permissions: string_list(&fields, "permissions")?,
        host_permissions: string_list(&fields, "host_permissions")?,
        content_script_matches: content_script_matches(&fields)?,
        content_script_declarations: script_review::declarations(&fields)?,
        optional_permissions: string_list(&fields, "optional_permissions")?,
        optional_host_permissions: string_list(&fields, "optional_host_permissions")?,
    })
}

/// The `matches` patterns of every content script, combined. Each script's list is checked like any
/// other list; the combined list has the same cap.
fn content_script_matches(fields: &[(String, json::Value)]) -> Result<Vec<String>, ExtensionError> {
    let invalid = ExtensionError::InvalidManifest("content_scripts must be a list of objects.");
    match json::get(fields, "content_scripts") {
        None => Ok(Vec::new()),
        Some(json::Value::Array(scripts)) => {
            if scripts.len() > MAX_LIST_LEN {
                return Err(invalid);
            }
            let mut matches = Vec::new();
            for script in scripts {
                let json::Value::Object(script) = script else {
                    return Err(invalid);
                };
                matches.extend(string_list(script, "matches")?);
                if matches.len() > MAX_LIST_LEN {
                    return Err(ExtensionError::InvalidManifest(
                        "content_scripts has too many match patterns.",
                    ));
                }
            }
            Ok(matches)
        }
        Some(_) => Err(invalid),
    }
}

fn string_list(
    fields: &[(String, json::Value)],
    key: &'static str,
) -> Result<Vec<String>, ExtensionError> {
    let invalid = ExtensionError::InvalidManifest("permissions must be a list of text values.");
    match json::get(fields, key) {
        None => Ok(Vec::new()),
        Some(json::Value::Array(items)) => {
            if items.len() > MAX_LIST_LEN {
                return Err(invalid);
            }
            items
                .iter()
                .map(|item| match item {
                    json::Value::String(text) if is_plain_text(text, MAX_LIST_ITEM_CHARS) => {
                        Ok(text.clone())
                    }
                    _ => Err(ExtensionError::InvalidManifest(
                        "permissions must be a list of text values.",
                    )),
                })
                .collect()
        }
        Some(_) => Err(invalid),
    }
}

/// Non-blank text of at most `max` characters with no control characters and no invisible format
/// characters. Such text is refused, never cleaned up: the user must be shown what the manifest says.
fn is_plain_text(text: &str, max: usize) -> bool {
    !text.trim().is_empty()
        && text.chars().count() <= max
        && !text.chars().any(|c| c.is_control() || is_invisible_format(c))
}

/// Unicode format characters (category Cf: bidi overrides and isolates, zero-width characters, the
/// soft hyphen, the BOM, tag characters and so on) plus the line and paragraph separators. They draw
/// nothing or reorder what is drawn, so they could disguise a name or a permission in a prompt.
pub(crate) fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{00ad}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061c}'
            | '\u{06dd}'
            | '\u{070f}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08e2}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{2028}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
            | '\u{110bd}'
            | '\u{110cd}'
            | '\u{13430}'..='\u{1343f}'
            | '\u{1bca0}'..='\u{1bca3}'
            | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0001}'
            | '\u{e0020}'..='\u{e007f}'
    )
}

/// One to four dot-separated integers, each 0 to 65535, as Chrome requires.
fn is_valid_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    (1..=4).contains(&parts.len())
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 5
                && part.bytes().all(|b| b.is_ascii_digit())
                && part.parse::<u32>().is_ok_and(|value| value <= 65_535)
        })
}

/// Checks a user-chosen folder without copying anything: it must be a plain folder with a plain
/// `manifest.json`, every entry must be a plain file or folder, and the size caps must hold.
pub fn inspect_source(source: &Path) -> Result<Inspection, ExtensionError> {
    inspect_with_limits(source, LIMITS)
}

fn inspect_with_limits(source: &Path, limits: Limits) -> Result<Inspection, ExtensionError> {
    if !is_plain_dir(source) {
        return Err(ExtensionError::NotAFolder);
    }
    let manifest_path = source.join(MANIFEST_FILE_NAME);
    let meta = fs::symlink_metadata(&manifest_path).map_err(|_| ExtensionError::MissingManifest)?;
    if !is_plain_file(&meta) {
        return Err(ExtensionError::MissingManifest);
    }
    if meta.len() > MAX_MANIFEST_BYTES {
        return Err(ExtensionError::ManifestTooLarge);
    }
    let bytes = fs::read(&manifest_path).map_err(|_| ExtensionError::Io)?;
    let manifest = parse_manifest(&bytes)?;
    let stats = walk_with_limits(source, limits, &mut |_, _| Ok(()))?;
    Ok(Inspection {
        manifest,
        files: stats.files,
        bytes: stats.bytes,
    })
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct TreeStats {
    files: usize,
    bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Folder,
    File,
}

/// Visits every entry under `root` (depth first) and fails on links, unsupported entries, invalid
/// names, a reserved root name or any cap. `visit` receives each entry's path relative to `root`.
fn walk_with_limits(
    root: &Path,
    limits: Limits,
    visit: &mut dyn FnMut(&Path, EntryKind) -> Result<(), ExtensionError>,
) -> Result<TreeStats, ExtensionError> {
    let mut stats = TreeStats::default();
    let mut pending: Vec<(PathBuf, usize)> = vec![(PathBuf::new(), 0)];
    while let Some((relative, depth)) = pending.pop() {
        let folder = root.join(&relative);
        // A folder that was a plain directory when its parent was listed may have been replaced by a
        // junction since; never list through a link.
        if !relative.as_os_str().is_empty() && !is_plain_dir(&folder) {
            return Err(ExtensionError::LinkNotAllowed);
        }
        let entries = fs::read_dir(&folder).map_err(|_| ExtensionError::Io)?;
        for entry in entries {
            let entry = entry.map_err(|_| ExtensionError::Io)?;
            let name = entry.file_name();
            let name = name.to_str().ok_or(ExtensionError::InvalidName)?;
            if !is_valid_entry_name(name) {
                return Err(ExtensionError::InvalidName);
            }
            let at_root = relative.as_os_str().is_empty();
            if at_root && name.starts_with('_') && name != LOCALES_FOLDER {
                return Err(ExtensionError::ReservedRootName);
            }
            let meta = fs::symlink_metadata(entry.path()).map_err(|_| ExtensionError::Io)?;
            if is_link(&meta) {
                return Err(ExtensionError::LinkNotAllowed);
            }
            if at_root && name == LOCALES_FOLDER && !meta.is_dir() {
                return Err(ExtensionError::ReservedRootName);
            }
            let child = relative.join(name);
            if meta.is_dir() {
                if depth + 1 > limits.depth {
                    return Err(ExtensionError::TooDeep);
                }
                visit(&child, EntryKind::Folder)?;
                pending.push((child, depth + 1));
            } else if meta.is_file() {
                stats.files += 1;
                stats.bytes = stats
                    .bytes
                    .checked_add(meta.len())
                    .ok_or(ExtensionError::TooLarge)?;
                if stats.files > limits.files {
                    return Err(ExtensionError::TooManyFiles);
                }
                if stats.bytes > limits.bytes {
                    return Err(ExtensionError::TooLarge);
                }
                visit(&child, EntryKind::File)?;
            } else {
                return Err(ExtensionError::UnsupportedEntry);
            }
        }
    }
    Ok(stats)
}

/// The existing public native-ID ownership map. Bounded, no links, and no inferred ownership
/// from an extension name/URL. A missing/damaged entry is not a grant to overwrite runtime data.
fn recorded_native_identity(records_file: &Path, id: &str) -> Option<String> {
    let path = records_file.with_file_name("extension_ids.txt");
    let mut file = fs::OpenOptions::new().read(true).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !is_plain_file(&meta) || meta.len() > 64 * 1024 { return None; }
    let mut text = String::new(); (&mut file).take(64 * 1024 + 1).read_to_string(&mut text).ok()?;
    if text.len() > 64 * 1024 { return None; }
    text.lines().find_map(|line| {
        let (bowser, native) = line.split_once('\t')?;
        (bowser == id && crate::crx_fetch::parse_extension_id(native).is_ok()).then(|| native.to_owned())
    })
}

/// Native Chromium identity claimed by a manifest public key. This is for duplicate/ownership
/// collision checks only, not signature verification or publisher attribution. Store packages
/// must still pass CRX3 verification before preparation.
pub fn manifest_key_id(folder: &Path) -> Result<Option<String>, ExtensionError> {
    let mut file = fs::OpenOptions::new().read(true).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(folder.join(MANIFEST_FILE_NAME)).map_err(|_| ExtensionError::Io)?;
    let meta = file.metadata().map_err(|_| ExtensionError::Io)?;
    if !is_plain_file(&meta) { return Err(ExtensionError::LinkNotAllowed); }
    if meta.len() > MAX_MANIFEST_BYTES { return Err(ExtensionError::ManifestTooLarge); }
    let mut text = String::new(); (&mut file).take(MAX_MANIFEST_BYTES + 1).read_to_string(&mut text).map_err(|_| ExtensionError::Io)?;
    if text.len() as u64 > MAX_MANIFEST_BYTES { return Err(ExtensionError::ManifestTooLarge); }
    let invalid = || ExtensionError::InvalidManifest("The manifest public key cannot be safely bound to its native identity.");
    let json::Value::Object(fields) = json::parse(&text).ok_or_else(invalid)? else { return Err(invalid()); };
    let Some(key) = json::get(&fields, "key") else { return Ok(None); };
    let json::Value::String(key) = key else { return Err(invalid()); };
    if key.is_empty() || key.len() > 16 * 1024 { return Err(invalid()); }
    let compact: String = key.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let bytes = crate::blocker::json::base64_decode(&compact).filter(|bytes| !bytes.is_empty()).ok_or_else(invalid)?;
    crate::crx::extension_id_from_public_key(&bytes).map(Some).ok_or(ExtensionError::Io)
}

/// A single plain name: no separators, no drive or stream syntax, no control characters, and no
/// trailing dot or space (Windows strips those).
fn is_valid_entry_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.len() <= 255
        && !name.ends_with(['.', ' '])
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        })
}

/// Copies a folder into `<extensions_dir>\<new id>` without asking anyone, then records it. Only for
/// callers that have already confirmed with the user; prefer [`install_confirmed`].
pub fn install(
    source: &Path,
    extensions_dir: &Path,
    records_file: &Path,
) -> Result<Record, ExtensionError> {
    install_confirmed(source, extensions_dir, records_file, &mut |_| true)?
        .ok_or(ExtensionError::Io)
}

/// Copies `source` into `<extensions_dir>\<new id>` first, then inspects that copy and passes its
/// manifest (including content script, optional and host permissions) to `confirm`. The copy is
/// the only thing that is ever recorded or loaded, so what the user approved is what runs.
///
/// Returns `Ok(None)` when `confirm` declines; the copy is then deleted and nothing is recorded.
/// Links, reparse points and other unsupported entries are refused during the copy: a file is
/// opened without following a link, and a folder is re-checked just before it is listed.
pub fn install_confirmed(
    source: &Path,
    extensions_dir: &Path,
    records_file: &Path,
    confirm: &mut dyn FnMut(&Inspection) -> bool,
) -> Result<Option<Record>, ExtensionError> {
    let prepared = prepare_install(source, extensions_dir, records_file)?;
    if !confirm(prepared.inspection()) {
        return Ok(None);
    }
    prepared.commit().map(Some)
}

/// A profile-owned, unrecorded copy. Dropping it (cancel, error, closed window) removes only this
/// newly-created folder. It cannot load until explicit approval calls `commit` and the UI asks the
/// owning profile's engine to load the returned record.
#[derive(Debug)]
pub struct PreparedInstall {
    extensions_dir: PathBuf,
    records_file: PathBuf,
    id: String,
    inspection: Inspection,
    fingerprint: [u8; 32],
    committed: bool,
    retained_original: Option<artifacts::Stage>,
}

/// Cross-process read/modify/write lease. The OS denies a second open while this handle lives,
/// and deletes the tiny lease file on close/crash. No persistent lock token or ACL changes.
fn lease_open_error_log_line(runtime: bool, code: Option<i32>) -> String {
    let kind = if runtime { "runtime" } else { "authority" };
    let code = code.map_or_else(|| "unavailable".to_owned(), |code| code.to_string());
    format!("extension call=lease_open kind={kind} result=error win32_code={code}")
}

pub(crate) struct RecordsLease { _file: File }
impl RecordsLease {
    /// Native lifecycle operations hold this separate lease until their async completion and
    /// owning record commit. Never wait/pump for it: a concurrent change fails honestly busy.
    pub(crate) fn runtime(records_file: &Path) -> Result<Self, ExtensionError> {
        Self::acquire(&records_file.with_file_name("extension-runtime"))
    }

    pub(crate) fn acquire(records_file: &Path) -> Result<Self, ExtensionError> {
        if let Some(parent) = records_file.parent().filter(|parent| !parent.as_os_str().is_empty()) { ensure_plain_dir(parent)?; }
        let mut path = records_file.as_os_str().to_owned(); path.push(".lease");
        let file = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
            // GENERIC_READ | GENERIC_WRITE | DELETE, required for DELETE_ON_CLOSE.
            .access_mode(0x8000_0000 | 0x4000_0000 | 0x0001_0000).share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | 0x0400_0000)
            .open(PathBuf::from(path)).map_err(|error| {
                // Existing explicit diagnostic opt-in only. Never print the authority path or
                // arbitrary error text; raw_os_error is the numeric failure from this exact open.
                if std::env::var_os("BOWSER_EXTENSION_TRACE").is_some() {
                    let runtime = records_file.file_name() == Some(std::ffi::OsStr::new("extension-runtime"));
                    eprintln!("{}", lease_open_error_log_line(runtime, error.raw_os_error()));
                }
                ExtensionError::Io
            })?;
        if !is_plain_file(&file.metadata().map_err(|_| ExtensionError::Io)?) { return Err(ExtensionError::LinkNotAllowed); }
        Ok(Self { _file: file })
    }
}

impl PreparedInstall {
    pub fn inspection(&self) -> &Inspection { &self.inspection }

    /// New Store installs retain a reverified original outside their loaded copy. This grants
    /// no update eligibility by itself and never upgrades an unauthenticated legacy folder.
    pub(crate) fn retain_verified_store_original(mut self, native_id: &str, bytes: &[u8]) -> Result<Self, ExtensionError> {
        self.retained_original = Some(artifacts::Stage::prepare(&self, native_id, bytes)?);
        Ok(self)
    }


    pub fn folder(&self) -> Result<PathBuf, ExtensionError> {
        contained_path(&self.extensions_dir, &self.id)
    }

    /// Re-read the actual copy and records after consent. No source folder is consulted and the
    /// reviewed declarations must still match. Writing the record is the install commit point.
    pub fn commit(self) -> Result<Record, ExtensionError> {
        let name = self.inspection.manifest.name.clone();
        self.commit_named(&name)
    }

    /// Store callers supply the resolved localized name shown in the matching permission review.
    /// This affects display only; all declarations and package bytes remain bound to the review.
    pub(crate) fn commit_named(self, name: &str) -> Result<Record, ExtensionError> {
        self.commit_checked_named(name, |_| Ok(()))
    }

    pub(crate) fn commit_checked_named(mut self, name: &str, check: impl FnOnce(&[Record]) -> Result<(), ExtensionError>) -> Result<Record, ExtensionError> {
        let _runtime = RecordsLease::runtime(&self.records_file)?;
        let _lease = RecordsLease::acquire(&self.records_file)?;
        if name.is_empty() || name.chars().count() > MAX_NAME_CHARS
            || name.chars().any(|c| c.is_control() || is_invisible_format(c))
        { return Err(ExtensionError::InvalidName); }
        if inspect_source(&self.folder()?)? != self.inspection
            || tree_fingerprint(&self.folder()?)? != self.fingerprint
        {
            return Err(ExtensionError::InvalidManifest("The staged extension changed. Review it again."));
        }
        let mut records = read_authority_records(&self.records_file)?;
        if let Some(identity) = manifest_key_id(&self.folder()?)? {
            for record in &records {
                let folder = contained_path(&self.extensions_dir, &record.id)?;
                if manifest_key_id(&folder).ok().flatten().as_deref() == Some(identity.as_str())
                    || recorded_native_identity(&self.records_file, &record.id).as_deref() == Some(identity.as_str())
                {
                    return Err(ExtensionError::InvalidManifest("This native extension identity is already installed in this profile."));
                }
            }
        }
        check(&records)?;
        if records.len() >= MAX_INSTALLED { return Err(ExtensionError::TooManyInstalled); }
        if records.iter().any(|record| record.id == self.id) { return Err(ExtensionError::InvalidRecords); }
        let record = Record {
            id: self.id.clone(), enabled: true,
            name: name.to_owned(),
            version: self.inspection.manifest.version.clone(),
        };
        records.push(record.clone());
        if let Some(original) = self.retained_original.as_mut() { original.publish()?; }
        write_records(&self.records_file, &records)?;
        if let Some(original) = self.retained_original.as_mut() { original.accept(); }
        self.committed = true;
        Ok(record)
    }
}

impl Drop for PreparedInstall {
    fn drop(&mut self) {
        if !self.committed { let _ = remove_folder(&self.extensions_dir, &self.id); }
    }
}

/// Prepare the exact profile-owned files for a full asynchronous permission review. This reuses
/// the same link/path/size-checked copy and inspection as local CRX/unpacked installation.
pub fn prepare_install(
    source: &Path, extensions_dir: &Path, records_file: &Path,
) -> Result<PreparedInstall, ExtensionError> {
    let records = read_records(records_file)?;
    if records.len() >= MAX_INSTALLED { return Err(ExtensionError::TooManyInstalled); }
    if !is_plain_dir(source) { return Err(ExtensionError::NotAFolder); }
    ensure_plain_dir(extensions_dir)?;
    let id = new_id(&records, extensions_dir)?;
    let target = contained_path(extensions_dir, &id)?;
    fs::create_dir(&target).map_err(|_| ExtensionError::Io)?;
    let inspection = match copy_tree(source, &target).and_then(|()| inspect_source(&target)) {
        Ok(inspection) => inspection,
        Err(error) => { let _ = remove_folder(extensions_dir, &id); return Err(error); }
    };
    let fingerprint = match tree_fingerprint(&target) {
        Ok(fingerprint) => fingerprint,
        Err(error) => { let _ = remove_folder(extensions_dir, &id); return Err(error); }
    };
    Ok(PreparedInstall {
        fingerprint,
        extensions_dir: extensions_dir.to_owned(), records_file: records_file.to_owned(),
        id, inspection, committed: false, retained_original: None,
    })
}

/// Deterministic SHA-256 of every staged path/type/length and file byte. The review binds the
/// entire copied package, not merely manifest declarations or total size. Bounded streaming,
/// no extra package-sized allocation and no symlink/reparse-point reads.
fn tree_fingerprint(root: &Path) -> Result<[u8; 32], ExtensionError> {
    let mut entries = Vec::new();
    walk_with_limits(root, LIMITS, &mut |path, kind| { entries.push((path.to_owned(), kind)); Ok(()) })?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hash = crate::crx::PackageSha256::new().ok_or(ExtensionError::Io)?;
    for (relative, kind) in entries {
        let name = relative.to_str().ok_or(ExtensionError::InvalidName)?.replace('\\', "/");
        if !hash.update(&(name.len() as u64).to_le_bytes()) || !hash.update(name.as_bytes()) {
            return Err(ExtensionError::Io);
        }
        match kind {
            EntryKind::Folder => { if !hash.update(b"D") { return Err(ExtensionError::Io); } }
            EntryKind::File => {
                let mut file = fs::OpenOptions::new().read(true).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                    .open(root.join(&relative)).map_err(|_| ExtensionError::Io)?;
                let meta = file.metadata().map_err(|_| ExtensionError::Io)?;
                if !is_plain_file(&meta) { return Err(ExtensionError::LinkNotAllowed); }
                if meta.len() > MAX_TOTAL_BYTES { return Err(ExtensionError::TooLarge); }
                if !hash.update(b"F") || !hash.update(&meta.len().to_le_bytes()) { return Err(ExtensionError::Io); }
                let mut read = 0u64; let mut buffer = [0u8; 64 * 1024];
                loop {
                    let count = file.read(&mut buffer).map_err(|_| ExtensionError::Io)?;
                    if count == 0 { break; }
                    read += count as u64;
                    if read > meta.len() || !hash.update(&buffer[..count]) { return Err(ExtensionError::Io); }
                }
                if read != meta.len() { return Err(ExtensionError::Io); }
            }
        }
    }
    hash.finish().ok_or(ExtensionError::Io)
}

/// Copies every entry of `source` under `target` using the same walk and caps as inspection.
fn copy_tree(source: &Path, target: &Path) -> Result<(), ExtensionError> {
    walk_with_limits(source, LIMITS, &mut |relative, kind| {
        let destination = target.join(relative);
        match kind {
            EntryKind::Folder => fs::create_dir(&destination).map_err(|_| ExtensionError::Io),
            EntryKind::File => copy_file_no_follow(&source.join(relative), &destination),
        }
    })
    .map(|_| ())
}

/// Copies one regular file. The entry is opened as itself (`FILE_FLAG_OPEN_REPARSE_POINT`), so a
/// link swapped in after the walk is refused rather than followed, and no more than the size the
/// walk saw is copied.
fn copy_file_no_follow(from: &Path, to: &Path) -> Result<(), ExtensionError> {
    let mut input = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(from)
        .map_err(|_| ExtensionError::Io)?;
    let meta = input.metadata().map_err(|_| ExtensionError::Io)?;
    if !is_plain_file(&meta) {
        return Err(ExtensionError::LinkNotAllowed);
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
        .map_err(|_| ExtensionError::Io)?;
    let copied = io::copy(&mut (&mut input).take(meta.len()), &mut output)
        .map_err(|_| ExtensionError::Io)?;
    if copied != meta.len() {
        return Err(ExtensionError::Io);
    }
    output.sync_all().map_err(|_| ExtensionError::Io)
}

/// Sets the enabled flag of one installed extension.
pub fn set_enabled(records_file: &Path, id: &str, enabled: bool) -> Result<(), ExtensionError> {
    let _runtime = RecordsLease::runtime(records_file)?;
    set_enabled_under_runtime(records_file, id, enabled)
}

/// Caller holds the profile runtime lease until the native operation and this commit finish.
pub(crate) fn set_enabled_under_runtime(records_file: &Path, id: &str, enabled: bool) -> Result<(), ExtensionError> {
    let _lease = RecordsLease::acquire(records_file)?;
    let mut records = read_authority_records(records_file)?;
    let record = records
        .iter_mut()
        .find(|record| record.id == id)
        .ok_or(ExtensionError::NotInstalled)?;
    record.enabled = enabled;
    write_records(records_file, &records)
}

/// Drops an extension from the list, then deletes its copied folder. Callers unload it from the
/// running profile first.
pub fn uninstall(
    extensions_dir: &Path,
    records_file: &Path,
    id: &str,
) -> Result<(), ExtensionError> {
    let _runtime = RecordsLease::runtime(records_file)?;
    uninstall_under_runtime(extensions_dir, records_file, id)
}

/// Caller owns the runtime lease through native Remove and this durable deletion.
pub(crate) fn uninstall_under_runtime(extensions_dir: &Path, records_file: &Path, id: &str) -> Result<(), ExtensionError> {
    let _lease = RecordsLease::acquire(records_file)?;
    let mut records = read_authority_records(records_file)?;
    let before = records.len();
    records.retain(|record| record.id != id);
    if records.len() == before {
        return Err(ExtensionError::NotInstalled);
    }
    write_records(records_file, &records)?;
    remove_folder(extensions_dir, id)?;
    artifacts::remove_for_record(extensions_dir, id)
}

/// Deletes `<extensions_dir>\<id>` only when it is a plain folder directly inside `extensions_dir`.
/// A missing folder is not an error.
fn remove_folder(extensions_dir: &Path, id: &str) -> Result<(), ExtensionError> {
    let target = contained_path(extensions_dir, id)?;
    let meta = match fs::symlink_metadata(&target) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(ExtensionError::Io),
    };
    if is_link(&meta) || !meta.is_dir() {
        return Err(ExtensionError::OutsideExtensions);
    }
    let root = fs::canonicalize(extensions_dir).map_err(|_| ExtensionError::Io)?;
    let canonical = fs::canonicalize(&target).map_err(|_| ExtensionError::Io)?;
    if canonical.parent() != Some(root.as_path()) {
        return Err(ExtensionError::OutsideExtensions);
    }
    fs::remove_dir_all(&target).map_err(|_| ExtensionError::Io)
}

/// What the engine needs at startup: whether to enable browser extensions at all (only when some
/// extension is installed) and the enabled folders to load. Unreadable lists count as empty.
pub fn startup_plan(extensions_dir: &Path, records_file: &Path) -> StartupPlan {
    let Ok(records) = read_records(records_file) else {
        return StartupPlan::default();
    };
    let enabled = records
        .iter()
        .filter(|record| record.enabled)
        .filter_map(|record| {
            let folder = contained_path(extensions_dir, &record.id).ok()?;
            let meta = fs::symlink_metadata(&folder).ok()?;
            (meta.is_dir() && !is_link(&meta)).then(|| (record.id.clone(), folder))
        })
        .collect();
    StartupPlan {
        installed: !records.is_empty(),
        enabled,
    }
}

/// Folder for an id, after checking the id. Ids are the only path component Bowser ever joins.
pub fn folder_for(extensions_dir: &Path, id: &str) -> Result<PathBuf, ExtensionError> {
    contained_path(extensions_dir, id)
}

fn contained_path(extensions_dir: &Path, id: &str) -> Result<PathBuf, ExtensionError> {
    if !is_valid_id(id) {
        return Err(ExtensionError::InvalidRecords);
    }
    Ok(extensions_dir.join(id))
}

/// `ext-` followed by 16 lowercase hex digits. Anything else is refused before it reaches a path.
pub fn is_valid_id(id: &str) -> bool {
    id.len() == ID_LEN
        && id.starts_with(ID_PREFIX)
        && id.as_bytes()[ID_PREFIX.len()..]
            .iter()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn new_id(existing: &[Record], extensions_dir: &Path) -> Result<String, ExtensionError> {
    for attempt in 0u32..8 {
        let mut hasher = RandomState::new().build_hasher();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        hasher.write_u128(nanos);
        hasher.write_u32(std::process::id());
        hasher.write_u32(attempt);
        let id = format!("{ID_PREFIX}{:016x}", hasher.finish());
        let taken = existing.iter().any(|record| record.id == id)
            || fs::symlink_metadata(extensions_dir.join(&id)).is_ok();
        if !taken {
            return Ok(id);
        }
    }
    Err(ExtensionError::Io)
}

/// Reads `extensions.txt`. A missing file is an empty list. Malformed lines are skipped.
pub fn read_records(records_file: &Path) -> Result<Vec<Record>, ExtensionError> {
    let text = match fs::read_to_string(records_file) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(ExtensionError::Io),
    };
    let mut records = Vec::new();
    for line in text.lines().take(MAX_INSTALLED * 2) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [id, enabled, name, version] = fields.as_slice() else {
            continue;
        };
        let enabled = match *enabled {
            "1" => true,
            "0" => false,
            _ => continue,
        };
        if !is_valid_id(id) || records.iter().any(|record: &Record| record.id == *id) {
            continue;
        }
        records.push(Record {
            id: (*id).to_owned(),
            enabled,
            name: (*name).to_owned(),
            version: (*version).to_owned(),
        });
        if records.len() == MAX_INSTALLED {
            break;
        }
    }
    Ok(records)
}

/// Strict authority reader for native lifecycle decisions. Unlike the display parser, a single
/// malformed/duplicate row, over-budget file or reparse point fails the entire operation closed.
pub(crate) fn read_authority_records(path: &Path) -> Result<Vec<Record>, ExtensionError> {
    let file = match fs::OpenOptions::new().read(true).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(ExtensionError::Io),
    };
    let meta = file.metadata().map_err(|_| ExtensionError::Io)?;
    if !is_plain_file(&meta) || meta.len() > 1024 * 1024 { return Err(ExtensionError::Io); }
    let mut text = String::new(); file.take(1024 * 1024 + 1).read_to_string(&mut text).map_err(|_| ExtensionError::Io)?;
    if text.len() > 1024 * 1024 { return Err(ExtensionError::Io); }
    let mut records = Vec::new();
    for line in text.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        let [id, enabled, name, version] = fields.as_slice() else { return Err(ExtensionError::Io); };
        if !is_valid_id(id) || !matches!(*enabled, "0" | "1") || records.iter().any(|record: &Record| record.id == *id)
            || name.is_empty() || name.chars().count() > MAX_NAME_CHARS || name.chars().any(|c| c.is_control() || is_invisible_format(c))
            || version.is_empty() || version.len() > 64 || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            || records.len() >= MAX_INSTALLED { return Err(ExtensionError::Io); }
        records.push(Record { id: (*id).into(), enabled: *enabled == "1", name: (*name).into(), version: (*version).into() });
    }
    Ok(records)
}

/// Replaces `extensions.txt` atomically: the new text is written and synced to a temporary file,
/// which then replaces the old one.
pub fn write_records(records_file: &Path, records: &[Record]) -> Result<(), ExtensionError> {
    if records.len() > MAX_INSTALLED {
        return Err(ExtensionError::TooManyInstalled);
    }
    let mut text = String::new();
    for record in records {
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            record.id,
            if record.enabled { 1 } else { 0 },
            sanitize_text(&record.name),
            sanitize_text(&record.version)
        ));
    }
    if let Some(parent) = records_file.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        ensure_plain_dir(parent)?;
    }
    let mut temporary = records_file.as_os_str().to_owned();
    temporary.push(format!(".tmp-{}", std::process::id()));
    let temporary = PathBuf::from(temporary);
    let mut created = false;
    let result = (|| -> io::Result<()> {
        // Never open an existing temporary file/link, and never delete somebody else's entry
        // when creation fails. Profile mutation callers already own the records/runtime leases.
        let mut file = fs::OpenOptions::new().write(true).create_new(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(&temporary)?;
        created = true;
        if !is_plain_file(&file.metadata()?) { return Err(io::Error::other("unsafe record temporary file")); }
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, records_file)
    })();
    result.map_err(|_| {
        if created { let _ = fs::remove_file(&temporary); }
        ExtensionError::Io
    })
}

/// Makes text safe for one tab-separated line: control characters become spaces, and the length
/// is capped.
fn sanitize_text(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_RECORD_TEXT_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Longest item shown in the summary, and longest run of items on one line. Longer items are cut
/// and the rest of the list is replaced by a count, so a padded manifest cannot push lines out of
/// the dialog.
const MAX_SUMMARY_ITEM_CHARS: usize = 56;
const MAX_SUMMARY_LIST_CHARS: usize = 120;

/// Upper bound on the text of the install confirmation (see [`install_prompt`]).
pub const MAX_PROMPT_CHARS: usize = 2_400;

/// A complete Store permission review must fit the existing 256 KiB trusted snapshot envelope.
/// Reserve room for catalogue/identity/status JSON rather than silently truncating declarations.
/// This is an admission bound, not an increased bridge message allowance.
pub fn store_review_fits(manifest: &Manifest) -> bool {
    const MAX_REVIEW_JSON_BYTES: usize = 180 * 1024;
    let lists = [&manifest.permissions, &manifest.host_permissions, &manifest.content_script_matches,
        &manifest.content_script_declarations, &manifest.optional_permissions, &manifest.optional_host_permissions];
    let bytes = lists.iter().flat_map(|list| list.iter()).try_fold(0usize, |total, value| {
        total.checked_add(crate::chrome_host::protocol::json_string(value).len() + 1)
    });
    bytes.is_some_and(|bytes| bytes <= MAX_REVIEW_JSON_BYTES)
}

/// Human-readable summary for the install confirmation. Counts come first, then a flag for access to
/// every website, then site access and content-script pages, then permissions. Each list is capped,
/// with the real count stated, so nothing can be padded out of view.
pub fn permission_summary(manifest: &Manifest) -> String {
    let count = |items: &[String], one: &str, many: &str| {
        format!("{} {}", items.len(), if items.len() == 1 { one } else { many })
    };
    let mut lines = vec![
        format!("Manifest version {}.", manifest.manifest_version),
        format!(
            "Declared: {}, {}, {}, {}, {}.",
            count(&manifest.permissions, "permission", "permissions"),
            count(&manifest.host_permissions, "site access", "site access"),
            count(
                &manifest.content_script_matches,
                "content-script match",
                "content-script matches"
            ),
            count(
                &manifest.optional_permissions,
                "optional permission",
                "optional permissions"
            ),
            count(
                &manifest.optional_host_permissions,
                "optional site access",
                "optional site access"
            ),
        ),
    ];
    let broad = |lists: &[&[String]]| {
        lists
            .iter()
            .any(|list| list.iter().any(|item| is_broad_pattern(item)))
    };
    if broad(&[
        &manifest.permissions,
        &manifest.host_permissions,
        &manifest.content_script_matches,
    ]) {
        lines.push("Every website: yes".to_owned());
    } else if broad(&[
        &manifest.optional_permissions,
        &manifest.optional_host_permissions,
    ]) {
        lines.push("Every website (optional): yes".to_owned());
    }
    for (label, items) in [
        ("Site access", &manifest.host_permissions),
        ("Content scripts run on", &manifest.content_script_matches),
        ("Permissions", &manifest.permissions),
        ("Optional site access", &manifest.optional_host_permissions),
        ("Optional permissions", &manifest.optional_permissions),
    ] {
        lines.push(format!("{label}: {}", capped_list(items)));
    }
    lines.join("\n")
}

/// The whole install confirmation: what is being installed, the capped permission summary, and what
/// Bowser does and does not do about it. WebView2 is not documented to limit an extension to less
/// than its manifest declares, so the dialog assumes it has everything it declares; Bowser cannot
/// narrow it afterwards, so the answer is all or nothing.
pub fn install_prompt(manifest: &Manifest) -> String {
    format!(
        "Install \"{}\" version {}?\n\n{}\n\nAssume an extension that is on has everything it declares. Bowser shows these but cannot narrow them. Only install extensions you trust. An extension can read and change the pages it has access to.",
        manifest.name,
        manifest.version,
        permission_summary(manifest)
    )
}

/// `<all_urls>`, or any `*`, `http` or `https` pattern whose host is exactly `*`.
fn is_broad_pattern(pattern: &str) -> bool {
    if pattern == "<all_urls>" {
        return true;
    }
    let Some((scheme, rest)) = pattern.split_once("://") else {
        return false;
    };
    matches!(scheme, "*" | "http" | "https") && rest.split('/').next() == Some("*")
}

/// The first items that fit on one line, each cut to a fixed length, then how many were left out.
fn capped_list(items: &[String]) -> String {
    if items.is_empty() {
        return "none".to_owned();
    }
    let mut shown = String::new();
    let mut used = 0;
    let mut listed = 0;
    for item in items {
        let text = if item.chars().count() > MAX_SUMMARY_ITEM_CHARS {
            let cut: String = item.chars().take(MAX_SUMMARY_ITEM_CHARS).collect();
            format!("{cut}...")
        } else {
            item.clone()
        };
        let width = text.chars().count() + if listed == 0 { 0 } else { 2 };
        if listed > 0 && used + width > MAX_SUMMARY_LIST_CHARS {
            break;
        }
        if listed > 0 {
            shown.push_str(", ");
        }
        shown.push_str(&text);
        used += width;
        listed += 1;
    }
    let left_out = items.len() - listed;
    if left_out > 0 {
        shown.push_str(&format!(", and {left_out} more"));
    }
    shown
}

/// The scheme WebView2 gave extension pages in the live probe (runtime 155.0.4283.45): both
/// `chrome.runtime.getURL` and the page origin used it. See `docs/extensions.md`.
pub const EXTENSION_PAGE_SCHEME: &str = "chrome-extension";

/// Which page of an extension Bowser may host itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    Popup,
    Options,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageError {
    /// The manifest declares no such page.
    NoPage,
    /// The declared path is not a plain relative path.
    UnsafePath,
    /// The path names no plain file inside the extension folder.
    MissingFile,
    /// The WebView2 id is not the 32-letter form the runtime reports.
    InvalidWebviewId,
    /// `manifest.json` could not be read or parsed.
    Unreadable,
}

impl std::fmt::Display for PageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoPage => "The extension does not declare that page.",
            Self::UnsafePath => "The extension declares a page path that Bowser will not open.",
            Self::MissingFile => "The extension's page file is missing.",
            Self::InvalidWebviewId => "WebView2 gave the extension an id Bowser does not recognise.",
            Self::Unreadable => "The extension's manifest could not be read.",
        })
    }
}

/// The raw page path a manifest declares, unvalidated. Popup: `action.default_popup`. Options:
/// `options_ui.page`, else `options_page`.
pub fn declared_page(manifest: &[u8], kind: PageKind) -> Option<String> {
    if manifest.len() as u64 > MAX_MANIFEST_BYTES {
        return None;
    }
    let text = std::str::from_utf8(manifest).ok()?;
    let json::Value::Object(fields) = json::parse(text.strip_prefix('\u{feff}').unwrap_or(text))?
    else {
        return None;
    };
    let text_member = |value: Option<&json::Value>, key: &str| match value {
        Some(json::Value::Object(inner)) => match json::get(inner, key) {
            Some(json::Value::String(text)) => Some(text.clone()),
            _ => None,
        },
        _ => None,
    };
    let text_field = |key: &str| match json::get(&fields, key) {
        Some(json::Value::String(text)) => Some(text.clone()),
        _ => None,
    };
    match kind {
        PageKind::Popup => text_member(json::get(&fields, "action"), "default_popup"),
        PageKind::Options => text_member(json::get(&fields, "options_ui"), "page")
            .or_else(|| text_field("options_page")),
    }
}

const MAX_PAGE_PATH_CHARS: usize = 256;

/// A plain relative path: `/`-separated segments of ASCII letters, digits, `.`, `_` and `-`. That
/// leaves no scheme, drive, backslash, query, fragment, percent escape or `.`/`..` segment to
/// interpret, so the path means the same thing in a URL and on disk.
fn is_plain_page_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PAGE_PATH_CHARS
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && !segment.ends_with('.')
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
}

/// The 32 letters `a` to `p` WebView2 reports as an extension id.
fn is_webview_extension_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| matches!(b, b'a'..=b'p'))
}

/// `chrome-extension://<id>/<path>` for a WebView2 id and a validated relative path.
pub fn page_url(webview_id: &str, path: &str) -> Result<url::Url, PageError> {
    if !is_webview_extension_id(webview_id) {
        return Err(PageError::InvalidWebviewId);
    }
    if !is_plain_page_path(path) {
        return Err(PageError::UnsafePath);
    }
    url::Url::parse(&format!("{EXTENSION_PAGE_SCHEME}://{webview_id}/{path}"))
        .map_err(|_| PageError::UnsafePath)
}

/// Whether `url` is exactly what [`page_url`] builds: the extension scheme, a 32-letter id as the
/// host, a plain path, and nothing else (no port, user, query or fragment).
pub fn is_page_url(url: &url::Url) -> bool {
    let (Some(host), Some(path)) = (url.host_str(), url.path().strip_prefix('/')) else {
        return false;
    };
    page_url(host, path).is_ok_and(|built| built == *url)
}

/// The bytes of a plain, size-capped `manifest.json` that parses as a JSON object.
fn read_page_manifest(folder: &Path) -> Result<Vec<u8>, PageError> {
    let manifest_path = folder.join(MANIFEST_FILE_NAME);
    let meta = fs::symlink_metadata(&manifest_path).map_err(|_| PageError::Unreadable)?;
    if !is_plain_file(&meta) || meta.len() > MAX_MANIFEST_BYTES {
        return Err(PageError::Unreadable);
    }
    let bytes = fs::read(&manifest_path).map_err(|_| PageError::Unreadable)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| PageError::Unreadable)?;
    match json::parse(text.strip_prefix('\u{feff}').unwrap_or(text)) {
        Some(json::Value::Object(_)) => Ok(bytes),
        _ => Err(PageError::Unreadable),
    }
}

/// Whether the manifest in `folder` names a page of this kind. It does not validate the path; the
/// extensions menu uses it to list only extensions that have something to open.
pub fn declares_page(folder: &Path, kind: PageKind) -> bool {
    read_page_manifest(folder).is_ok_and(|bytes| declared_page(&bytes, kind).is_some())
}

/// The manifest of an installed extension, read from Bowser's own copy in `folder`: the declarations
/// the user saw when they installed it. `None` when the copy is missing or no longer parses.
pub fn installed_manifest(folder: &Path) -> Option<Manifest> {
    parse_manifest(&read_page_manifest(folder).ok()?).ok()
}

/// Whether the manifest's required site access, permissions or content scripts cover every website.
pub fn reaches_every_site(manifest: &Manifest) -> bool {
    [
        &manifest.permissions,
        &manifest.host_permissions,
        &manifest.content_script_matches,
    ]
    .iter()
    .any(|list| list.iter().any(|item| is_broad_pattern(item)))
}

/// The URL of an extension's popup or options page, read from Bowser's own copy in `folder`. The
/// declared file must be a plain file reached through plain folders, so a link inside the copy
/// cannot point the page outside it.
pub fn extension_page_url(
    folder: &Path,
    kind: PageKind,
    webview_id: &str,
) -> Result<url::Url, PageError> {
    let bytes = read_page_manifest(folder)?;
    let path = declared_page(&bytes, kind).ok_or(PageError::NoPage)?;
    let url = page_url(webview_id, &path)?;
    let mut current = folder.to_path_buf();
    let segments: Vec<&str> = path.split('/').collect();
    for (index, segment) in segments.iter().enumerate() {
        current.push(segment);
        let plain = if index + 1 == segments.len() {
            fs::symlink_metadata(&current).is_ok_and(|meta| is_plain_file(&meta))
        } else {
            is_plain_dir(&current)
        };
        if !plain {
            return Err(PageError::MissingFile);
        }
    }
    Ok(url)
}

static LOAD_RESULTS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// Remembers whether an extension loaded in this session. A failure keeps its reason; success
/// clears it.
pub fn record_load_result(id: &str, result: Result<(), String>) {
    let Ok(mut results) = LOAD_RESULTS.lock() else {
        return;
    };
    match result {
        Ok(()) => {
            results.remove(id);
        }
        Err(reason) => {
            let reason: String = reason.chars().take(MAX_FAILURE_CHARS).collect();
            results.insert(id.to_owned(), reason);
        }
    }
}

/// The reason an extension failed to load or change state in this session, if any.
pub fn load_failure(id: &str) -> Option<String> {
    LOAD_RESULTS.lock().ok()?.get(id).cloned()
}

fn ensure_plain_dir(path: &Path) -> Result<(), ExtensionError> {
    fs::create_dir_all(path).map_err(|_| ExtensionError::Io)?;
    if is_plain_dir(path) {
        Ok(())
    } else {
        Err(ExtensionError::OutsideExtensions)
    }
}

fn is_link(meta: &fs::Metadata) -> bool {
    meta.file_type().is_symlink() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn is_plain_file(meta: &fs::Metadata) -> bool {
    meta.is_file() && !is_link(meta)
}

fn is_plain_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir() && !is_link(&meta))
}

/// A small strict JSON reader. It rejects anything outside RFC 8259 plus duplicate object keys,
/// and it caps nesting depth. Numbers are kept as their source text.
pub(crate) mod json {
    use super::MAX_JSON_DEPTH;

    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        Null,
        Bool(bool),
        Number(String),
        String(String),
        Array(Vec<Value>),
        Object(Vec<(String, Value)>),
    }

    pub fn get<'a>(fields: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
        fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    pub fn parse(text: &str) -> Option<Value> {
        let mut parser = Parser { text, pos: 0 };
        parser.skip_ws();
        let value = parser.value(0)?;
        parser.skip_ws();
        (parser.pos == text.len()).then_some(value)
    }

    struct Parser<'a> {
        text: &'a str,
        pos: usize,
    }

    impl Parser<'_> {
        fn bytes(&self) -> &[u8] {
            self.text.as_bytes()
        }

        fn peek(&self) -> Option<u8> {
            self.bytes().get(self.pos).copied()
        }

        fn eat(&mut self, byte: u8) -> Option<()> {
            (self.peek()? == byte).then(|| self.pos += 1)
        }

        fn skip_ws(&mut self) {
            while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                self.pos += 1;
            }
        }

        fn value(&mut self, depth: usize) -> Option<Value> {
            if depth > MAX_JSON_DEPTH {
                return None;
            }
            match self.peek()? {
                b'{' => self.object(depth),
                b'[' => self.array(depth),
                b'"' => self.string().map(Value::String),
                b't' => self.literal("true", Value::Bool(true)),
                b'f' => self.literal("false", Value::Bool(false)),
                b'n' => self.literal("null", Value::Null),
                b'-' | b'0'..=b'9' => self.number(),
                _ => None,
            }
        }

        fn literal(&mut self, word: &str, value: Value) -> Option<Value> {
            if self.bytes()[self.pos..].starts_with(word.as_bytes()) {
                self.pos += word.len();
                Some(value)
            } else {
                None
            }
        }

        fn digits(&mut self) -> Option<()> {
            let start = self.pos;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
            (self.pos > start).then_some(())
        }

        fn number(&mut self) -> Option<Value> {
            let start = self.pos;
            if self.peek() == Some(b'-') {
                self.pos += 1;
            }
            match self.peek()? {
                b'0' => self.pos += 1,
                b'1'..=b'9' => self.digits()?,
                _ => return None,
            }
            if self.peek() == Some(b'.') {
                self.pos += 1;
                self.digits()?;
            }
            if matches!(self.peek(), Some(b'e' | b'E')) {
                self.pos += 1;
                if matches!(self.peek(), Some(b'+' | b'-')) {
                    self.pos += 1;
                }
                self.digits()?;
            }
            Some(Value::Number(self.text[start..self.pos].to_owned()))
        }

        fn hex4(&mut self) -> Option<u32> {
            let digits = self.text.get(self.pos..self.pos + 4)?;
            if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            self.pos += 4;
            u32::from_str_radix(digits, 16).ok()
        }

        fn string(&mut self) -> Option<String> {
            self.eat(b'"')?;
            let mut out = String::new();
            loop {
                match self.peek()? {
                    b'"' => {
                        self.pos += 1;
                        return Some(out);
                    }
                    b'\\' => {
                        self.pos += 1;
                        let escape = self.peek()?;
                        self.pos += 1;
                        let c = match escape {
                            b'"' => '"',
                            b'\\' => '\\',
                            b'/' => '/',
                            b'b' => '\u{8}',
                            b'f' => '\u{c}',
                            b'n' => '\n',
                            b'r' => '\r',
                            b't' => '\t',
                            b'u' => self.unicode_escape()?,
                            _ => return None,
                        };
                        out.push(c);
                    }
                    0..=0x1F => return None,
                    _ => {
                        let c = self.text[self.pos..].chars().next()?;
                        out.push(c);
                        self.pos += c.len_utf8();
                    }
                }
            }
        }

        /// The part after `\u`: a BMP character, or a surrogate pair. A lone surrogate is refused.
        fn unicode_escape(&mut self) -> Option<char> {
            let unit = self.hex4()?;
            if (0xD800..0xDC00).contains(&unit) {
                self.eat(b'\\')?;
                self.eat(b'u')?;
                let low = self.hex4()?;
                if !(0xDC00..0xE000).contains(&low) {
                    return None;
                }
                char::from_u32(0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00))
            } else {
                char::from_u32(unit)
            }
        }

        fn array(&mut self, depth: usize) -> Option<Value> {
            self.eat(b'[')?;
            self.skip_ws();
            let mut items = Vec::new();
            if self.eat(b']').is_some() {
                return Some(Value::Array(items));
            }
            loop {
                items.push(self.value(depth + 1)?);
                self.skip_ws();
                if self.eat(b',').is_some() {
                    self.skip_ws();
                } else {
                    self.eat(b']')?;
                    return Some(Value::Array(items));
                }
            }
        }

        fn object(&mut self, depth: usize) -> Option<Value> {
            self.eat(b'{')?;
            self.skip_ws();
            let mut fields: Vec<(String, Value)> = Vec::new();
            if self.eat(b'}').is_some() {
                return Some(Value::Object(fields));
            }
            loop {
                let key = self.string()?;
                self.skip_ws();
                self.eat(b':')?;
                self.skip_ws();
                if fields.iter().any(|(name, _)| *name == key) {
                    return None;
                }
                let value = self.value(depth + 1)?;
                fields.push((key, value));
                self.skip_ws();
                if self.eat(b',').is_some() {
                    self.skip_ws();
                } else {
                    self.eat(b'}')?;
                    return Some(Value::Object(fields));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bowser-ext-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn manifest(body: &str) -> Vec<u8> {
        body.as_bytes().to_vec()
    }

    /// Creates an NTFS junction with `mklink /J`. Junctions need no privilege, unlike symlinks, and
    /// carry the same reparse-point attribute the link checks look for.
    fn make_junction(link: &Path, target: &Path) {
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .expect("cmd.exe runs");
        assert!(
            output.status.success(),
            "mklink /J failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(link.is_dir());
    }

    const MV3: &str = r#"{"manifest_version": 3, "name": "Demo", "version": "1.2.3",
        "permissions": ["storage", "tabs"], "host_permissions": ["https://example.com/*"]}"#;

    #[test]
    fn record_write_preserves_preexisting_temporary_file_and_authority() {
        let root = temp_dir("records-create-new-file"); let records = root.join("extensions.txt");
        let original = Record { id: "ext-0000000000000001".into(), enabled: true, name: "Original".into(), version: "1".into() };
        write_records(&records, std::slice::from_ref(&original)).unwrap();
        let before = fs::read(&records).unwrap();
        let temporary = PathBuf::from(format!("{}.tmp-{}", records.display(), std::process::id()));
        fs::write(&temporary, b"preexisting data must survive").unwrap();
        assert_eq!(write_records(&records, &[]), Err(ExtensionError::Io));
        assert_eq!(fs::read(&records).unwrap(), before);
        assert_eq!(fs::read(&temporary).unwrap(), b"preexisting data must survive");
        let _ = fs::remove_dir_all(root);
    }

    /// Pins the Windows sharing contract that `test-native-store-record-reader.ps1` models: a
    /// read-only observer that shares Read|Write|Delete never denies the atomic replacement and keeps
    /// the old generation, while one that withholds Delete sharing blocks it without damage.
    #[test]
    fn record_replacement_passes_delete_sharing_readers_and_stops_at_readwrite_only_readers() {
        use std::io::Read;
        let root = temp_dir("records-reader-sharing"); let records = root.join("extensions.txt");
        let record = |enabled| Record { id: "ext-0000000000000001".into(), enabled, name: "Synthetic".into(), version: "1.0".into() };
        write_records(&records, &[record(true)]).unwrap();
        let old = fs::read(&records).unwrap();
        let mut reader = fs::OpenOptions::new().read(true).share_mode(0x1 | 0x2 | 0x4).open(&records).unwrap();
        write_records(&records, &[record(false)]).unwrap();
        let mut held = Vec::new(); reader.read_to_end(&mut held).unwrap(); drop(reader);
        assert_eq!(held, old);
        assert_eq!(read_records(&records).unwrap(), vec![record(false)]);
        let new = fs::read(&records).unwrap();
        let control = fs::OpenOptions::new().read(true).share_mode(0x1 | 0x2).open(&records).unwrap();
        assert_eq!(write_records(&records, &[record(true)]), Err(ExtensionError::Io));
        drop(control);
        assert_eq!(fs::read(&records).unwrap(), new);
        assert!(!PathBuf::from(format!("{}.tmp-{}", records.display(), std::process::id())).exists());
        write_records(&records, &[record(true)]).unwrap();
        assert_eq!(fs::read(&records).unwrap(), old);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn record_write_preserves_preexisting_temporary_directory() {
        let root = temp_dir("records-create-new-dir"); let records = root.join("extensions.txt");
        let temporary = PathBuf::from(format!("{}.tmp-{}", records.display(), std::process::id()));
        fs::create_dir(&temporary).unwrap(); fs::write(temporary.join("keep"), b"untouched").unwrap();
        assert_eq!(write_records(&records, &[]), Err(ExtensionError::Io));
        assert!(!records.exists()); assert_eq!(fs::read(temporary.join("keep")).unwrap(), b"untouched");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn record_write_does_not_follow_or_remove_temporary_junction() {
        let root = temp_dir("records-create-new-junction"); let outside = temp_dir("records-create-new-junction-outside");
        let records = root.join("extensions.txt");
        let temporary = PathBuf::from(format!("{}.tmp-{}", records.display(), std::process::id()));
        fs::write(outside.join("keep"), b"untouched").unwrap(); make_junction(&temporary, &outside);
        assert_eq!(write_records(&records, &[]), Err(ExtensionError::Io));
        assert!(fs::symlink_metadata(&temporary).unwrap().file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0);
        assert_eq!(fs::read(outside.join("keep")).unwrap(), b"untouched"); assert!(!records.exists());
        fs::remove_dir(&temporary).unwrap(); let _ = fs::remove_dir_all(root); let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn record_write_refuses_reparse_parent_without_touching_target() {
        let root = temp_dir("records-parent-junction"); let outside = temp_dir("records-parent-junction-outside");
        fs::write(outside.join("keep"), b"untouched").unwrap(); let parent = root.join("linked"); make_junction(&parent, &outside);
        assert_eq!(write_records(&parent.join("extensions.txt"), &[]), Err(ExtensionError::OutsideExtensions));
        assert!(!outside.join("extensions.txt").exists()); assert_eq!(fs::read(outside.join("keep")).unwrap(), b"untouched");
        fs::remove_dir(&parent).unwrap(); let _ = fs::remove_dir_all(root); let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn strict_runtime_records_reject_malformed_duplicate_and_oversized_authority() {
        let root = temp_dir("strict-runtime-records"); let path = root.join("extensions.txt");
        let good = "ext-0123456789abcdef\t1\tFixture\t1.0\n";
        fs::write(&path, good).unwrap(); assert_eq!(read_authority_records(&path).unwrap().len(), 1);
        for bad in [format!("{good}broken\n"), format!("{good}{good}"), good.replace("\t1\t", "\tmaybe\t"), "x".repeat(1024 * 1024 + 1)] {
            fs::write(&path, bad).unwrap(); assert!(read_authority_records(&path).is_err());
        }
        assert!(read_authority_records(&root).is_err()); fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_lease_blocks_record_mutation_in_another_process() {
        let root = temp_dir("runtime-cross-process"); let path = root.join("extensions.txt");
        let guard = RecordsLease::runtime(&path).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "extensions::tests::runtime_lease_child", "--ignored", "--nocapture"])
            .env("BOWSER_TEST_RUNTIME_LEASE_PATH", &path).output().unwrap();
        assert!(output.status.success(), "cross-process exclusion failed: {} {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stdout).contains("RUNTIME_LEASE_CHILD=blocked"));
        drop(guard); drop(RecordsLease::runtime(&path).unwrap()); fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "child of the bounded cross-process runtime lease unit test"]
    fn runtime_lease_child() {
        let Some(path) = std::env::var_os("BOWSER_TEST_RUNTIME_LEASE_PATH") else { panic!("Only the owning lease test launches this child."); };
        assert!(RecordsLease::runtime(Path::new(&path)).is_err()); println!("RUNTIME_LEASE_CHILD=blocked");
    }

    #[test]
    fn extension_lease_trace_contains_only_kind_and_numeric_error() {
        assert_eq!(lease_open_error_log_line(true, Some(32)), "extension call=lease_open kind=runtime result=error win32_code=32");
        assert_eq!(lease_open_error_log_line(false, Some(5)), "extension call=lease_open kind=authority result=error win32_code=5");
        assert_eq!(lease_open_error_log_line(false, None), "extension call=lease_open kind=authority result=error win32_code=unavailable");
    }

    #[test]
    fn accepts_minimal_manifest_v2_and_v3() {
        let v3 = parse_manifest(&manifest(MV3)).unwrap();
        assert_eq!(v3.manifest_version, 3);
        assert_eq!(v3.name, "Demo");
        assert_eq!(v3.version, "1.2.3");
        assert_eq!(v3.permissions, vec!["storage", "tabs"]);
        assert_eq!(v3.host_permissions, vec!["https://example.com/*"]);
        let v2 = parse_manifest(&manifest(
            r#"{"manifest_version":2,"name":"Old","version":"0.1"}"#,
        ))
        .unwrap();
        assert_eq!(v2.manifest_version, 2);
        assert!(v2.permissions.is_empty());
        assert!(parse_manifest(&manifest(&format!("\u{feff}{MV3}"))).is_ok());
    }

    #[test]
    fn rejects_bad_manifest_versions_names_and_versions() {
        let cases = [
            r#"{"manifest_version":1,"name":"a","version":"1"}"#,
            r#"{"manifest_version":3.0,"name":"a","version":"1"}"#,
            r#"{"manifest_version":"3","name":"a","version":"1"}"#,
            r#"{"name":"a","version":"1"}"#,
            r#"{"manifest_version":3,"version":"1"}"#,
            r#"{"manifest_version":3,"name":"a"}"#,
            r#"{"manifest_version":3,"name":"  ","version":"1"}"#,
            r#"{"manifest_version":3,"name":"a\u0007b","version":"1"}"#,
            r#"{"manifest_version":3,"name":"a\nb","version":"1"}"#,
            r#"{"manifest_version":3,"name":"a","version":"1.2.3.4.5"}"#,
            r#"{"manifest_version":3,"name":"a","version":"1..2"}"#,
            r#"{"manifest_version":3,"name":"a","version":"70000"}"#,
            r#"{"manifest_version":3,"name":"a","version":"1.-2"}"#,
            r#"{"manifest_version":3,"name":"a","version":"1 2"}"#,
            r#"{"manifest_version":3,"name":"a","version":"1.2.3.4"}"#,
        ];
        for (index, text) in cases.iter().enumerate() {
            let result = parse_manifest(text.as_bytes());
            if index == cases.len() - 1 {
                assert!(result.is_ok(), "{text} should be accepted");
            } else {
                assert!(result.is_err(), "{text} should be rejected");
            }
        }
        let long_name = format!(
            r#"{{"manifest_version":3,"name":"{}","version":"1"}}"#,
            "n".repeat(129)
        );
        assert!(parse_manifest(long_name.as_bytes()).is_err());
    }

    #[test]
    fn rejects_malformed_json_and_duplicate_keys() {
        let bad = [
            "",
            "[]",
            "\"text\"",
            r#"{"manifest_version":3,"name":"a","version":"1"} trailing"#,
            r#"{"manifest_version":3,"name":"a","version":"1",}"#,
            r#"{"manifest_version":3,"name":"a","version":"1","name":"b"}"#,
            r#"{"manifest_version":3,"name":"a\ud800","version":"1"}"#,
            r#"{"manifest_version":3,"name":"a\udc00","version":"1"}"#,
            r#"{"manifest_version":3,"name":"a\x41","version":"1"}"#,
            r#"{"manifest_version":3,"name":"a","version":"1","x":01}"#,
            r#"{"manifest_version":3,"name":"a","version":"1","x":1.}"#,
            r#"{"manifest_version":3,"name":"a","version":"1","x":-}"#,
            r#"{"manifest_version":3,"name":"a","version":"1","x":1e}"#,
        ];
        for text in bad {
            assert!(parse_manifest(text.as_bytes()).is_err(), "{text}");
        }
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        let too_deep = format!(r#"{{"manifest_version":3,"name":"a","version":"1","x":{deep}}}"#);
        assert!(parse_manifest(too_deep.as_bytes()).is_err());
    }

    #[test]
    fn rejects_invalid_utf8_and_oversized_manifests() {
        let mut bytes = MV3.as_bytes().to_vec();
        bytes.push(0xFF);
        assert!(parse_manifest(&bytes).is_err());
        let big = vec![b' '; (MAX_MANIFEST_BYTES + 1) as usize];
        assert_eq!(parse_manifest(&big), Err(ExtensionError::ManifestTooLarge));
    }

    #[test]
    fn rejects_non_string_and_oversized_permission_lists() {
        let cases = [
            r#"{"manifest_version":3,"name":"a","version":"1","permissions":"tabs"}"#,
            r#"{"manifest_version":3,"name":"a","version":"1","permissions":[1]}"#,
            r#"{"manifest_version":3,"name":"a","version":"1","permissions":[{"x":"y"}]}"#,
            r#"{"manifest_version":3,"name":"a","version":"1","host_permissions":[null]}"#,
            r#"{"manifest_version":3,"name":"a","version":"1","permissions":[""]}"#,
        ];
        for text in cases {
            assert!(parse_manifest(text.as_bytes()).is_err(), "{text}");
        }
        let many = format!(
            r#"{{"manifest_version":3,"name":"a","version":"1","permissions":[{}]}}"#,
            vec!["\"tabs\""; MAX_LIST_LEN + 1].join(",")
        );
        assert!(parse_manifest(many.as_bytes()).is_err());
        let long_host = format!(
            r#"{{"manifest_version":3,"name":"a","version":"1","host_permissions":["{}"]}}"#,
            "h".repeat(MAX_LIST_ITEM_CHARS + 1)
        );
        assert!(parse_manifest(long_host.as_bytes()).is_err());
    }

    #[test]
    fn json_accepts_escapes_surrogate_pairs_and_number_forms() {
        let text = r#"{"manifest_version":3,"name":"Caf\u00e9 \ud83d\ude00 \"x\" \/","version":"1","n":[0,-0.5,1e+3,2E-2]}"#;
        let manifest = parse_manifest(text.as_bytes()).unwrap();
        assert_eq!(manifest.name, "Café 😀 \"x\" /");
        assert!(json::parse(r#"{"a":[true,false,null,-0,1.5e2]}"#).is_some());
    }

    #[test]
    fn entry_names_refuse_traversal_streams_and_separators() {
        for name in [
            "..",
            ".",
            "a/b",
            "a\\b",
            "C:evil",
            "x:stream",
            "trailing.",
            "bad ",
            "q?",
            "x\u{7}",
        ] {
            assert!(!is_valid_entry_name(name), "{name}");
        }
        assert!(is_valid_entry_name("background.js"));
        assert!(is_valid_entry_name("icons"));
    }

    #[test]
    fn ids_are_strictly_formatted() {
        assert!(is_valid_id("ext-0123456789abcdef"));
        for id in [
            "",
            "ext-",
            "ext-0123456789ABCDEF",
            "ext-0123456789abcde",
            "ext-0123456789abcdefg",
            "../ext-0123456789abcd",
            "ext-..\\..\\x",
            "ext-0123456789abcdé",
            "ext-0123456789abcd/",
        ] {
            assert!(!is_valid_id(id), "{id}");
        }
        let dir = PathBuf::from(r"C:\data\Extensions");
        assert_eq!(
            folder_for(&dir, "../../Windows"),
            Err(ExtensionError::InvalidRecords)
        );
    }

    #[test]
    fn local_install_cannot_duplicate_an_existing_native_public_key_identity() {
        let root = temp_dir("duplicate-native-key");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extensions/probe-mv3-key");
        assert_eq!(manifest_key_id(&source).unwrap().as_deref(), Some("ciocoifocbggchcecjgibbaeojegkpen"));
        let extensions = root.join("Extensions"); let records = root.join("extensions.txt");
        let first = install(&source, &extensions, &records).unwrap();
        let prepared = prepare_install(&source, &extensions, &records).unwrap();
        let rejected = prepared.folder().unwrap();
        assert!(prepared.commit().is_err()); assert!(!rejected.exists());
        assert_eq!(read_records(&records).unwrap(), vec![first]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn records_lease_refuses_overlapping_writer_and_releases_automatically() {
        let dir = temp_dir("records-lease"); let records = dir.join("extensions.txt");
        let first = RecordsLease::acquire(&records).unwrap();
        assert!(RecordsLease::acquire(&records).is_err());
        drop(first);
        let second = RecordsLease::acquire(&records).unwrap();
        drop(second);
        assert!(!dir.join("extensions.txt.lease").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn prepared_install_rejects_same_size_code_change_after_review() {
        let dir = temp_dir("review-code-change");
        let source = dir.join("source"); fs::create_dir(&source).unwrap();
        fs::write(source.join("manifest.json"), br#"{"manifest_version":3,"name":"Test","version":"1"}"#).unwrap();
        fs::write(source.join("worker.js"), b"original").unwrap();
        let target = dir.join("Extensions"); let records = dir.join("extensions.txt");
        let prepared = prepare_install(&source, &target, &records).unwrap();
        let copy = prepared.folder().unwrap();
        fs::write(copy.join("worker.js"), b"modified").unwrap();
        assert!(prepared.commit().is_err());
        assert!(!copy.exists()); assert!(read_records(&records).unwrap().is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn records_round_trip_and_skip_malformed_lines() {
        let dir = temp_dir("records");
        let file = dir.join("extensions.txt");
        let records = vec![
            Record {
                id: "ext-0123456789abcdef".into(),
                enabled: true,
                name: "Tab\tName\nLine".into(),
                version: "1.0".into(),
            },
            Record {
                id: "ext-fedcba9876543210".into(),
                enabled: false,
                name: "Off".into(),
                version: "2".into(),
            },
        ];
        write_records(&file, &records).unwrap();
        let read = read_records(&file).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].name, "Tab Name Line");
        assert!(read[0].enabled && !read[1].enabled);

        let text = format!(
            "bad line\n../../x\t1\tn\t1\next-0123456789abcdef\t2\tn\t1\next-0123456789abcdee\t1\tdup\t1\next-0123456789abcdee\t1\tdup\t1\n{}",
            ""
        );
        fs::write(&file, text).unwrap();
        let read = read_records(&file).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].id, "ext-0123456789abcdee");
        assert_eq!(read_records(&dir.join("missing.txt")).unwrap(), Vec::new());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_copies_checked_folder_and_uninstall_removes_it() {
        let dir = temp_dir("install");
        let source = dir.join("source");
        fs::create_dir_all(source.join("icons")).unwrap();
        fs::write(source.join("manifest.json"), MV3).unwrap();
        fs::write(source.join("icons").join("a.png"), [1u8, 2, 3]).unwrap();
        let extensions = dir.join("profile").join("Extensions");
        let file = dir.join("profile").join("extensions.txt");

        let inspection = inspect_source(&source).unwrap();
        assert_eq!(inspection.files, 2);
        assert_eq!(inspection.manifest.name, "Demo");

        let record = install(&source, &extensions, &file).unwrap();
        assert!(record.enabled);
        assert!(
            extensions
                .join(&record.id)
                .join("icons")
                .join("a.png")
                .is_file()
        );
        assert_eq!(read_records(&file).unwrap(), vec![record.clone()]);

        set_enabled(&file, &record.id, false).unwrap();
        assert!(!read_records(&file).unwrap()[0].enabled);
        assert_eq!(
            set_enabled(&file, "ext-0000000000000000", true),
            Err(ExtensionError::NotInstalled)
        );

        uninstall(&extensions, &file, &record.id).unwrap();
        assert!(!extensions.join(&record.id).exists());
        assert!(read_records(&file).unwrap().is_empty());
        assert!(source.join("manifest.json").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn inspection_refuses_missing_or_non_plain_manifests() {
        let dir = temp_dir("inspect");
        assert_eq!(
            inspect_source(&dir.join("absent")),
            Err(ExtensionError::NotAFolder)
        );
        assert_eq!(inspect_source(&dir), Err(ExtensionError::MissingManifest));
        fs::create_dir_all(dir.join("manifest.json")).unwrap();
        assert_eq!(inspect_source(&dir), Err(ExtensionError::MissingManifest));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn inspection_enforces_file_count_size_and_depth_caps() {
        let dir = temp_dir("caps");
        fs::write(dir.join("manifest.json"), MV3).unwrap();
        fs::write(dir.join("a.txt"), "12345").unwrap();
        let small = Limits {
            files: 2,
            bytes: 1 << 20,
            depth: 4,
        };
        assert_eq!(
            inspect_with_limits(&dir, Limits { files: 1, ..small }).err(),
            Some(ExtensionError::TooManyFiles)
        );
        assert_eq!(
            inspect_with_limits(&dir, Limits { bytes: 10, ..small }).err(),
            Some(ExtensionError::TooLarge)
        );
        assert!(inspect_with_limits(&dir, small).is_ok());

        let mut nested = dir.clone();
        for depth in 0..4 {
            nested = nested.join(format!("d{depth}"));
            fs::create_dir_all(&nested).unwrap();
        }
        assert_eq!(
            inspect_with_limits(&dir, Limits { depth: 3, ..small }).err(),
            Some(ExtensionError::TooDeep)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn inspection_refuses_links_and_junction_like_entries() {
        let dir = temp_dir("links");
        let outside = temp_dir("links-outside");
        fs::write(dir.join("manifest.json"), MV3).unwrap();
        fs::write(outside.join("secret.txt"), "keep").unwrap();
        make_junction(&dir.join("linked"), &outside);
        assert_eq!(inspect_source(&dir), Err(ExtensionError::LinkNotAllowed));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn remove_never_follows_a_link_out_of_extensions() {
        let dir = temp_dir("escape");
        let outside = temp_dir("escape-outside");
        fs::write(outside.join("precious.txt"), "keep").unwrap();
        let extensions = dir.join("Extensions");
        fs::create_dir_all(&extensions).unwrap();
        let id = "ext-0123456789abcdef";
        make_junction(&extensions.join(id), &outside);
        assert_eq!(
            remove_folder(&extensions, id),
            Err(ExtensionError::OutsideExtensions)
        );
        assert!(outside.join("precious.txt").exists());
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn startup_plan_loads_only_enabled_present_folders() {
        let dir = temp_dir("plan");
        let extensions = dir.join("Extensions");
        let file = dir.join("extensions.txt");
        let on = "ext-0123456789abcdef";
        let off = "ext-fedcba9876543210";
        let missing = "ext-00000000000000aa";
        fs::create_dir_all(extensions.join(on)).unwrap();
        fs::create_dir_all(extensions.join(off)).unwrap();
        let record = |id: &str, enabled| Record {
            id: id.into(),
            enabled,
            name: "n".into(),
            version: "1".into(),
        };
        write_records(
            &file,
            &[record(on, true), record(off, false), record(missing, true)],
        )
        .unwrap();
        let plan = startup_plan(&extensions, &file);
        assert!(plan.installed);
        assert_eq!(plan.enabled, vec![(on.to_owned(), extensions.join(on))]);
        assert!(!startup_plan(&extensions, &dir.join("none.txt")).installed);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_results_keep_the_reason_until_success() {
        let id = "ext-1111111111111111";
        record_load_result(id, Err("boom".into()));
        assert_eq!(load_failure(id).as_deref(), Some("boom"));
        record_load_result(id, Ok(()));
        assert_eq!(load_failure(id), None);
    }

    #[test]
    fn permission_summary_lists_declared_access() {
        let manifest = parse_manifest(&manifest(MV3)).unwrap();
        let summary = permission_summary(&manifest);
        assert!(summary.contains("storage, tabs"));
        assert!(summary.contains("https://example.com/*"));
    }

    #[test]
    fn content_scripts_and_optional_access_are_listed() {
        let manifest = parse_manifest(&manifest(
            r#"{"manifest_version": 3, "name": "Injector", "version": "1.0",
            "content_scripts": [{"matches": ["<all_urls>"], "js": ["a.js"]},
                                {"matches": ["https://b.test/*"], "js": ["b.js"]}],
            "optional_permissions": ["cookies"],
            "optional_host_permissions": ["*://*/*"]}"#,
        ))
        .unwrap();
        assert_eq!(
            manifest.content_script_matches,
            vec!["<all_urls>", "https://b.test/*"]
        );
        let summary = permission_summary(&manifest);
        assert!(summary.contains("Content scripts run on: <all_urls>, https://b.test/*"));
        assert!(summary.contains("Optional permissions: cookies"));
        assert!(summary.contains("Optional site access: *://*/*"));
    }

    #[test]
    fn malformed_content_scripts_are_rejected() {
        for body in [
            r#"{"manifest_version":3,"name":"x","version":"1","content_scripts":"all"}"#,
            r#"{"manifest_version":3,"name":"x","version":"1","content_scripts":[1]}"#,
            r#"{"manifest_version":3,"name":"x","version":"1","content_scripts":[{"matches":"<all_urls>"}]}"#,
        ] {
            assert!(parse_manifest(&manifest(body)).is_err(), "{body}");
        }
    }

    #[test]
    fn declined_install_leaves_nothing_behind() {
        let dir = temp_dir("declined");
        let source = dir.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("manifest.json"), MV3).unwrap();
        let extensions = dir.join("profile").join("Extensions");
        let file = dir.join("profile").join("extensions.txt");
        let mut seen = None;
        let outcome = install_confirmed(&source, &extensions, &file, &mut |copy| {
            seen = Some(copy.manifest.name.clone());
            false
        });
        assert_eq!(outcome, Ok(None));
        assert_eq!(seen.as_deref(), Some("Demo"));
        assert!(read_records(&file).unwrap().is_empty());
        let leftover = fs::read_dir(&extensions).map_or(0, |entries| entries.count());
        assert_eq!(leftover, 0, "the copy is deleted when declined");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn confirmed_install_shows_the_copy_and_records_it() {
        let dir = temp_dir("confirmed");
        let source = dir.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("manifest.json"),
            r#"{"manifest_version": 3, "name": "Injector", "version": "2.0",
            "content_scripts": [{"matches": ["<all_urls>"], "js": ["a.js"]}]}"#,
        )
        .unwrap();
        fs::write(source.join("a.js"), "1").unwrap();
        let extensions = dir.join("profile").join("Extensions");
        let file = dir.join("profile").join("extensions.txt");
        let mut prompt = String::new();
        let record = install_confirmed(&source, &extensions, &file, &mut |copy| {
            prompt = permission_summary(&copy.manifest);
            assert_eq!(copy.files, 2);
            true
        })
        .unwrap()
        .expect("accepted");
        assert!(prompt.contains("<all_urls>"));
        assert!(extensions.join(&record.id).join("a.js").is_file());
        assert_eq!(read_records(&file).unwrap(), vec![record]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_refuses_links_during_the_copy_and_keeps_nothing() {
        let dir = temp_dir("install-links");
        let outside = temp_dir("install-links-outside");
        let source = dir.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("manifest.json"), MV3).unwrap();
        fs::write(outside.join("secret.txt"), "keep").unwrap();
        make_junction(&source.join("linked"), &outside);
        let extensions = dir.join("profile").join("Extensions");
        let file = dir.join("profile").join("extensions.txt");
        assert_eq!(
            install_confirmed(&source, &extensions, &file, &mut |_| true),
            Err(ExtensionError::LinkNotAllowed)
        );
        let leftover = fs::read_dir(&extensions).map_or(0, |entries| entries.count());
        assert_eq!(leftover, 0);
        assert!(read_records(&file).unwrap().is_empty());
        assert_eq!(
            fs::read_to_string(outside.join("secret.txt")).unwrap(),
            "keep"
        );
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&outside);
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("extensions")
            .join(name)
    }

    #[test]
    fn probe_fixtures_are_accepted_or_refused_as_intended() {
        for name in ["probe-mv3", "probe-mv3-locales", "probe-mv3-key"] {
            let inspection = inspect_source(&fixture(name))
                .unwrap_or_else(|error| panic!("{name} should be accepted: {error}"));
            let manifest = inspection.manifest;
            assert_eq!(manifest.manifest_version, 3, "{name}");
            assert_eq!(manifest.permissions, vec!["storage"], "{name}");
            assert_eq!(manifest.host_permissions, vec!["http://127.0.0.1/*"], "{name}");
            assert_eq!(manifest.content_script_matches, vec!["http://127.0.0.1/*"], "{name}");
            assert!(manifest.optional_permissions.is_empty(), "{name}");
        }
        assert_eq!(
            inspect_source(&fixture("probe-mv3-metadata")).err(),
            Some(ExtensionError::ReservedRootName)
        );
    }

    #[test]
    fn prepared_copy_is_not_installed_until_explicit_commit_and_cancel_cleans_it() {
        let dir = temp_dir("prepared-consent");
        let extensions = dir.join("Extensions");
        let records = dir.join("extensions.txt");
        let prepared = prepare_install(&fixture("probe-mv3"), &extensions, &records).unwrap();
        let folder = prepared.folder().unwrap();
        assert!(folder.join("manifest.json").exists());
        assert!(read_records(&records).unwrap().is_empty());
        assert!(startup_plan(&extensions, &records).enabled.is_empty());
        drop(prepared);
        assert!(!folder.exists());
        assert!(read_records(&records).unwrap().is_empty());
        let prepared = prepare_install(&fixture("probe-mv3"), &extensions, &records).unwrap();
        let record = prepared.commit().unwrap();
        assert_eq!(read_records(&records).unwrap(), vec![record]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn changed_reviewed_declarations_cannot_commit() {
        let dir = temp_dir("prepared-change");
        let extensions = dir.join("Extensions");
        let records = dir.join("extensions.txt");
        let prepared = prepare_install(&fixture("probe-mv3"), &extensions, &records).unwrap();
        let folder = prepared.folder().unwrap();
        fs::write(folder.join("manifest.json"), r#"{"manifest_version":3,"name":"Changed","version":"1","permissions":["tabs"]}"#).unwrap();
        assert!(prepared.commit().is_err());
        assert!(!folder.exists());
        assert!(read_records(&records).unwrap().is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn locales_fixture_keeps_its_raw_name_and_its_locales_folder_through_install() {
        let source = fixture("probe-mv3-locales");
        assert_eq!(
            inspect_source(&source).unwrap().manifest.name,
            "__MSG_extName__"
        );
        let dir = temp_dir("fixture-locales");
        let extensions = dir.join("Extensions");
        let file = dir.join("extensions.txt");
        let record = install(&source, &extensions, &file).unwrap();
        let copy = extensions.join(&record.id);
        assert!(copy.join("_locales").join("en").join("messages.json").is_file());
        assert!(copy.join("worker.js").is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn metadata_fixture_is_refused_and_install_leaves_nothing_behind() {
        let dir = temp_dir("fixture-metadata");
        let extensions = dir.join("Extensions");
        let file = dir.join("extensions.txt");
        assert_eq!(
            install_confirmed(&fixture("probe-mv3-metadata"), &extensions, &file, &mut |_| true),
            Err(ExtensionError::ReservedRootName)
        );
        assert_eq!(fs::read_dir(&extensions).map_or(0, |entries| entries.count()), 0);
        assert!(read_records(&file).unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_root_locales_folder_may_start_with_an_underscore() {
        let dir = temp_dir("underscore");
        fs::write(dir.join("manifest.json"), MV3).unwrap();

        fs::create_dir_all(dir.join("_locales").join("en")).unwrap();
        fs::write(dir.join("_locales").join("en").join("messages.json"), "{}").unwrap();
        assert!(inspect_source(&dir).is_ok(), "_locales is the one allowed name");
        let _ = fs::remove_dir_all(dir.join("_locales"));

        // Refused whether the entry is a folder or a file, and the case must match exactly.
        for (name, is_dir) in [
            ("_metadata", true),
            ("_other", true),
            ("_file.txt", false),
            ("_Locales", true),
            ("__MACOSX", true),
        ] {
            let path = dir.join(name);
            if is_dir {
                fs::create_dir_all(&path).unwrap();
            } else {
                fs::write(&path, "x").unwrap();
            }
            assert_eq!(
                inspect_source(&dir).err(),
                Some(ExtensionError::ReservedRootName),
                "{name}"
            );
            let _ = if is_dir { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
        }

        // A file named _locales is not the locales folder.
        fs::write(dir.join("_locales"), "x").unwrap();
        assert_eq!(inspect_source(&dir).err(), Some(ExtensionError::ReservedRootName));
        let _ = fs::remove_file(dir.join("_locales"));

        // The rule covers the root only; deeper names are the runtime's business.
        fs::create_dir_all(dir.join("lib").join("_vendor")).unwrap();
        assert!(inspect_source(&dir).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_key_and_default_locale_are_accepted_and_ignored() {
        let manifest = parse_manifest(&manifest(
            r#"{"manifest_version":3,"name":"__MSG_n__","version":"1","default_locale":"en","key":"MIIBIjANBgkq"}"#,
        ))
        .unwrap();
        assert_eq!(manifest.name, "__MSG_n__");
    }

    #[test]
    fn names_and_permissions_with_invisible_format_characters_are_refused() {
        // Right-to-left override, isolates, zero-width, soft hyphen, BOM, tag character and the
        // line and paragraph separators.
        for c in [
            '\u{202e}', '\u{202a}', '\u{2066}', '\u{2069}', '\u{200b}', '\u{200d}', '\u{200f}',
            '\u{00ad}', '\u{feff}', '\u{2060}', '\u{e0041}', '\u{2028}', '\u{2029}',
        ] {
            let escaped = c.escape_unicode();
            let cases = [
                format!(r#"{{"manifest_version":3,"name":"Safe{c}Name","version":"1"}}"#),
                format!(r#"{{"manifest_version":3,"name":"a","version":"1","permissions":["tab{c}s"]}}"#),
                format!(r#"{{"manifest_version":3,"name":"a","version":"1","host_permissions":["https://a.test{c}/*"]}}"#),
                format!(r#"{{"manifest_version":3,"name":"a","version":"1","optional_permissions":["{c}"]}}"#),
                format!(r#"{{"manifest_version":3,"name":"a","version":"1","optional_host_permissions":["x{c}"]}}"#),
                format!(r#"{{"manifest_version":3,"name":"a","version":"1","content_scripts":[{{"matches":["https://a.test/{c}*"]}}]}}"#),
            ];
            for text in cases {
                assert!(
                    parse_manifest(text.as_bytes()).is_err(),
                    "{escaped} must be refused, not stripped: {text}"
                );
            }
        }
        // The same characters written as JSON escapes are caught after decoding.
        assert!(
            parse_manifest(br#"{"manifest_version":3,"name":"a\u202eb","version":"1"}"#).is_err()
        );
        // Ordinary non-ASCII text is still fine.
        let ok = parse_manifest(
            "{\"manifest_version\":3,\"name\":\"Café 日本語 😀\",\"version\":\"1\"}".as_bytes(),
        )
        .unwrap();
        assert_eq!(ok.name, "Café 日本語 😀");
    }

    /// A manifest that fills every list to its cap with the longest items, with the one pattern that
    /// matters (`<all_urls>`) as the very last site-access entry.
    fn padded_manifest() -> Manifest {
        let filler = |prefix: &str| -> Vec<String> {
            (0..MAX_LIST_LEN)
                .map(|i| format!("{prefix}{i}{}", "x".repeat(MAX_LIST_ITEM_CHARS))[..MAX_LIST_ITEM_CHARS].to_owned())
                .collect()
        };
        let mut host_permissions = filler("https://pad");
        *host_permissions.last_mut().unwrap() = "<all_urls>".to_owned();
        Manifest {
            name: "n".repeat(MAX_NAME_CHARS),
            version: "65535.65535.65535.65535".to_owned(),
            manifest_version: 3,
            permissions: filler("perm"),
            host_permissions,
            content_script_matches: filler("https://script"),
            content_script_declarations: Vec::new(),
            optional_permissions: filler("opt"),
            optional_host_permissions: filler("https://optsite"),
        }
    }

    #[test]
    fn summary_is_capped_counted_and_cannot_hide_site_access() {
        let manifest = padded_manifest();
        let summary = permission_summary(&manifest);
        assert!(
            summary.chars().count() < MAX_PROMPT_CHARS / 2,
            "summary is {} characters",
            summary.chars().count()
        );
        // Counts are real counts of the manifest, not of what is shown.
        assert!(summary.contains("256 permissions"), "{summary}");
        assert!(summary.contains("256 site access"), "{summary}");
        assert!(summary.contains("256 content-script matches"), "{summary}");
        assert!(summary.contains("more"), "{summary}");
        // The broad grant is flagged up front even though `<all_urls>` is not among the items shown.
        assert!(!summary.contains("<all_urls>, "));
        let broad = summary.find("Every website").expect("broad access is flagged");
        let site = summary.find("Site access:").unwrap();
        let permissions = summary.find("Permissions:").unwrap();
        let optional = summary.find("Optional permissions:").unwrap();
        assert!(broad < site && site < permissions && permissions < optional);
        assert!(summary.find("Content scripts run on:").unwrap() < permissions);
    }

    #[test]
    fn summary_flags_every_website_only_for_broad_patterns() {
        let summary_for = |body: &str| permission_summary(&parse_manifest(&manifest(body)).unwrap());
        for pattern in ["<all_urls>", "*://*/*", "http://*/*", "https://*/*"] {
            for key in ["host_permissions", "permissions"] {
                let body = format!(
                    r#"{{"manifest_version":3,"name":"x","version":"1","{key}":["{pattern}"]}}"#
                );
                assert!(summary_for(&body).contains("Every website: yes"), "{key} {pattern}");
            }
            let script = format!(
                r#"{{"manifest_version":3,"name":"x","version":"1","content_scripts":[{{"matches":["{pattern}"]}}]}}"#
            );
            assert!(summary_for(&script).contains("Every website: yes"), "script {pattern}");
        }
        let optional = summary_for(
            r#"{"manifest_version":3,"name":"x","version":"1","optional_host_permissions":["*://*/*"]}"#,
        );
        assert!(optional.contains("Every website (optional): yes"));
        assert!(!optional.contains("Every website: yes"));
        let narrow = summary_for(MV3);
        assert!(!narrow.contains("Every website"), "{narrow}");
        for pattern in ["https://*.example.com/*", "https://example.com/*", "http://127.0.0.1/*"] {
            let body = format!(
                r#"{{"manifest_version":3,"name":"x","version":"1","host_permissions":["{pattern}"]}}"#
            );
            assert!(!summary_for(&body).contains("Every website"), "{pattern}");
        }
    }

    #[test]
    fn short_lists_are_shown_in_full_with_their_counts() {
        let summary = permission_summary(&parse_manifest(&manifest(MV3)).unwrap());
        assert!(summary.contains("2 permissions"), "{summary}");
        assert!(summary.contains("1 site access"), "{summary}");
        assert!(summary.contains("Permissions: storage, tabs"), "{summary}");
        assert!(summary.contains("Site access: https://example.com/*"), "{summary}");
        assert!(!summary.contains("more"), "{summary}");
    }

    #[test]
    fn install_prompt_stays_short_and_does_not_claim_bowser_enforces_permissions() {
        let prompt = install_prompt(&padded_manifest());
        assert!(
            prompt.chars().count() <= MAX_PROMPT_CHARS,
            "prompt is {} characters",
            prompt.chars().count()
        );
        let prompt = install_prompt(&parse_manifest(&manifest(MV3)).unwrap());
        assert!(prompt.starts_with("Install \"Demo\" version 1.2.3?"), "{prompt}");
        assert!(prompt.contains("https://example.com/*"));
        assert!(prompt.contains("has everything it declares"), "{prompt}");
        assert!(prompt.contains("cannot narrow"), "{prompt}");
        assert!(prompt.contains("Only install extensions you trust"), "{prompt}");
        let lower = prompt.to_lowercase();
        assert!(!lower.contains("webview2 grants"), "{prompt}");
        assert!(!lower.contains("bowser enforces"), "{prompt}");
        assert!(!lower.contains("web store"), "{prompt}");
        assert!(!lower.contains("chrome"), "{prompt}");
    }

    const WEBVIEW_ID: &str = "hekkainhmjhmmgdonhilehemnlbachak";

    #[test]
    fn declared_page_reads_the_popup_and_both_options_forms() {
        let both = br#"{"action":{"default_popup":"popup.html"},"options_ui":{"page":"opts/o.html"},"options_page":"old.html"}"#;
        assert_eq!(declared_page(both, PageKind::Popup).as_deref(), Some("popup.html"));
        assert_eq!(declared_page(both, PageKind::Options).as_deref(), Some("opts/o.html"));
        let legacy = br#"{"options_page":"old.html"}"#;
        assert_eq!(declared_page(legacy, PageKind::Options).as_deref(), Some("old.html"));
        assert_eq!(declared_page(legacy, PageKind::Popup), None);
        for wrong in [
            &br#"{"action":{"default_popup":3}}"#[..],
            br#"{"action":"popup.html"}"#,
            br#"{"action":{}}"#,
            b"not json",
            b"",
        ] {
            assert_eq!(declared_page(wrong, PageKind::Popup), None);
        }
    }

    #[test]
    fn page_url_uses_the_observed_scheme_the_webview_id_and_the_path() {
        assert_eq!(EXTENSION_PAGE_SCHEME, "chrome-extension");
        assert_eq!(
            page_url(WEBVIEW_ID, "popup.html").unwrap().as_str(),
            "chrome-extension://hekkainhmjhmmgdonhilehemnlbachak/popup.html"
        );
        assert_eq!(
            page_url(WEBVIEW_ID, "ui/pages/popup-1.html").unwrap().as_str(),
            "chrome-extension://hekkainhmjhmmgdonhilehemnlbachak/ui/pages/popup-1.html"
        );
    }

    #[test]
    fn page_url_refuses_every_path_that_is_not_plain_and_relative() {
        let long = "a/".repeat(200) + "x.html";
        for bad in [
            "",
            "/popup.html",
            "../popup.html",
            "a/../popup.html",
            "a/./popup.html",
            "./popup.html",
            "a//popup.html",
            "popup.html/",
            "https://evil.example/x.html",
            "chrome-extension://other/x.html",
            "javascript:alert(1)",
            "data:text/html,x",
            "C:/Windows/x.html",
            "C:x.html",
            "a\\popup.html",
            "popup.html?x=1",
            "popup.html#x",
            "%2e%2e/popup.html",
            "popup.html ",
            " popup.html",
            "popup.html.",
            "p\u{f6}pup.html",
            "pop\0up.html",
            "pop up.html",
            long.as_str(),
        ] {
            assert_eq!(page_url(WEBVIEW_ID, bad), Err(PageError::UnsafePath), "{bad:?}");
        }
    }

    #[test]
    fn page_url_refuses_ids_that_are_not_the_observed_form() {
        for bad in [
            "",
            "short",
            "HEKKAINHMJHMMGDONHILEHEMNLBACHAK",
            "hekkainhmjhmmgdonhilehemnlbachakk",
            "hekkainhmjhmmgdonhilehemnlbachaq",
            "hekkainhmjhmmgdonhilehemnlbac.ak",
            "hekkainhmjhmmgdonhilehemnlbac@ak",
            "evil.example/hekkainhmjhmmgdonhi",
        ] {
            assert_eq!(page_url(bad, "popup.html"), Err(PageError::InvalidWebviewId), "{bad:?}");
        }
    }

    #[test]
    fn declares_page_is_true_only_for_a_manifest_that_names_one() {
        assert!(declares_page(&fixture("probe-mv3"), PageKind::Popup));
        assert!(declares_page(&fixture("probe-mv3"), PageKind::Options));
        assert!(!declares_page(&fixture("probe-mv3-key"), PageKind::Popup));
        let missing = temp_dir("declares-missing");
        assert!(!declares_page(&missing, PageKind::Popup));
        let _ = fs::remove_dir_all(&missing);
    }

    #[test]
    fn every_page_error_has_its_own_plain_message() {
        let all = [
            PageError::NoPage,
            PageError::UnsafePath,
            PageError::MissingFile,
            PageError::InvalidWebviewId,
            PageError::Unreadable,
        ];
        let messages: std::collections::BTreeSet<String> =
            all.iter().map(ToString::to_string).collect();
        assert_eq!(messages.len(), all.len());
        assert!(messages.iter().all(|message| message.ends_with('.') && !message.contains("Error")));
    }

    #[test]
    fn is_page_url_accepts_only_what_page_url_builds() {
        for good in ["popup.html", "ui/pages/popup-1.html"] {
            let built = page_url(WEBVIEW_ID, good).unwrap();
            assert!(is_page_url(&built), "{built}");
        }
        for bad in [
            "extension://hekkainhmjhmmgdonhilehemnlbachak/popup.html",
            "ms-browser-extension://hekkainhmjhmmgdonhilehemnlbachak/popup.html",
            "https://hekkainhmjhmmgdonhilehemnlbachak/popup.html",
            "chrome-extension://hekkainhmjhmmgdonhilehemnlbachak/popup.html?x=1",
            "chrome-extension://hekkainhmjhmmgdonhilehemnlbachak/popup.html#x",
            "chrome-extension://hekkainhmjhmmgdonhilehemnlbachak:81/popup.html",
            "chrome-extension://user@hekkainhmjhmmgdonhilehemnlbachak/popup.html",
            "chrome-extension://hekkainhmjhmmgdonhilehemnlbachak/",
            "chrome-extension://hekkainhmjhmmgdonhilehemnlbachak",
            "chrome-extension://hekkainhmjhmmgdonhilehemnlbachak/a%20b.html",
            "chrome-extension://HEKKAINHMJHMMGDONHILEHEMNLBACHAK/popup.html",
            "chrome-extension://other/popup.html",
        ] {
            let url = url::Url::parse(bad).unwrap();
            assert!(!is_page_url(&url), "{bad}");
        }
    }

    fn page_folder(name: &str, manifest_body: &str, files: &[&str]) -> PathBuf {
        let dir = temp_dir(name);
        fs::write(dir.join("manifest.json"), manifest_body).unwrap();
        for file in files {
            let path = dir.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "<html></html>").unwrap();
        }
        dir
    }

    #[test]
    fn extension_page_url_needs_the_declared_file_to_exist_inside_the_folder() {
        let dir = page_folder(
            "page-ok",
            r#"{"action":{"default_popup":"ui/popup.html"},"options_page":"options.html"}"#,
            &["ui/popup.html"],
        );
        assert_eq!(
            extension_page_url(&dir, PageKind::Popup, WEBVIEW_ID).unwrap().as_str(),
            "chrome-extension://hekkainhmjhmmgdonhilehemnlbachak/ui/popup.html"
        );
        // Declared but absent.
        assert_eq!(
            extension_page_url(&dir, PageKind::Options, WEBVIEW_ID),
            Err(PageError::MissingFile)
        );
        // A folder where a file is expected is not a page.
        fs::create_dir_all(dir.join("options.html")).unwrap();
        assert_eq!(
            extension_page_url(&dir, PageKind::Options, WEBVIEW_ID),
            Err(PageError::MissingFile)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn extension_page_url_reports_no_page_and_unreadable_manifests() {
        let none = page_folder("page-none", r#"{"manifest_version":3}"#, &[]);
        assert_eq!(extension_page_url(&none, PageKind::Popup, WEBVIEW_ID), Err(PageError::NoPage));
        let bad = page_folder("page-bad", "not json", &[]);
        assert_eq!(extension_page_url(&bad, PageKind::Popup, WEBVIEW_ID), Err(PageError::Unreadable));
        let missing = temp_dir("page-nomanifest");
        assert_eq!(
            extension_page_url(&missing, PageKind::Popup, WEBVIEW_ID),
            Err(PageError::Unreadable)
        );
        for dir in [none, bad, missing] {
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn extension_page_url_refuses_a_declared_path_that_escapes_or_has_a_scheme() {
        for declared in [
            r#""../outside.html""#,
            r#""https://evil.example/p.html""#,
            r#""a\\b.html""#,
            r#""/abs.html""#,
        ] {
            let body = format!(r#"{{"action":{{"default_popup":{declared}}}}}"#);
            let dir = page_folder("page-escape", &body, &[]);
            fs::write(dir.parent().unwrap().join("outside.html"), "x").unwrap();
            assert_eq!(
                extension_page_url(&dir, PageKind::Popup, WEBVIEW_ID),
                Err(PageError::UnsafePath),
                "{declared}"
            );
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn extension_page_url_never_follows_a_junction_out_of_the_folder() {
        let dir = page_folder("page-link", r#"{"action":{"default_popup":"link/popup.html"}}"#, &[]);
        let outside = temp_dir("page-link-outside");
        fs::write(outside.join("popup.html"), "<html></html>").unwrap();
        make_junction(&dir.join("link"), &outside);
        assert_eq!(
            extension_page_url(&dir, PageKind::Popup, WEBVIEW_ID),
            Err(PageError::MissingFile)
        );
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn the_probe_fixture_declares_a_popup_and_an_options_page_that_exist() {
        let source = fixture("probe-mv3");
        assert_eq!(
            extension_page_url(&source, PageKind::Popup, WEBVIEW_ID).unwrap().as_str(),
            "chrome-extension://hekkainhmjhmmgdonhilehemnlbachak/popup.html"
        );
        assert_eq!(
            extension_page_url(&source, PageKind::Options, WEBVIEW_ID).unwrap().as_str(),
            "chrome-extension://hekkainhmjhmmgdonhilehemnlbachak/options.html"
        );
        // The other probe variants declare no popup.
        assert_eq!(
            extension_page_url(&fixture("probe-mv3-key"), PageKind::Popup, WEBVIEW_ID),
            Err(PageError::NoPage)
        );
    }
}
