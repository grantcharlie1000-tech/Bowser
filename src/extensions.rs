//! Unpacked browser extensions for one profile.
//!
//! This module decides what may be installed and keeps the files on disk consistent. It never runs
//! extension code. WebView2 loads the copied folders through `ICoreWebView2Profile7`; the engine
//! reports load results back here so the extension menu can show why one failed.
//!
//! Layout inside a profile: `Extensions\<id>\` holds the copied folder and `extensions.txt` lists
//! one installed extension per line as `id<TAB>enabled(0|1)<TAB>name<TAB>version`.
//!
//! Scope: only unpacked folders chosen by the user. Chrome Web Store installs are not supported.

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

/// Non-blank text of at most `max` characters with no control characters.
fn is_plain_text(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= max && !text.chars().any(char::is_control)
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
/// names or any cap. `visit` receives each entry's path relative to `root`.
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
            let meta = fs::symlink_metadata(entry.path()).map_err(|_| ExtensionError::Io)?;
            if is_link(&meta) {
                return Err(ExtensionError::LinkNotAllowed);
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
    let mut records = read_records(records_file)?;
    if records.len() >= MAX_INSTALLED {
        return Err(ExtensionError::TooManyInstalled);
    }
    if !is_plain_dir(source) {
        return Err(ExtensionError::NotAFolder);
    }
    ensure_plain_dir(extensions_dir)?;
    let id = new_id(&records, extensions_dir)?;
    let target = contained_path(extensions_dir, &id)?;
    fs::create_dir(&target).map_err(|_| ExtensionError::Io)?;
    let inspected = copy_tree(source, &target).and_then(|()| inspect_source(&target));
    let inspection = match inspected {
        Ok(inspection) => inspection,
        Err(error) => {
            let _ = remove_folder(extensions_dir, &id);
            return Err(error);
        }
    };
    if !confirm(&inspection) {
        let _ = remove_folder(extensions_dir, &id);
        return Ok(None);
    }
    let record = Record {
        id,
        enabled: true,
        name: inspection.manifest.name,
        version: inspection.manifest.version,
    };
    records.push(record.clone());
    if let Err(error) = write_records(records_file, &records) {
        let _ = remove_folder(extensions_dir, &record.id);
        return Err(error);
    }
    Ok(Some(record))
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
    let mut records = read_records(records_file)?;
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
    let mut records = read_records(records_file)?;
    let before = records.len();
    records.retain(|record| record.id != id);
    if records.len() == before {
        return Err(ExtensionError::NotInstalled);
    }
    write_records(records_file, &records)?;
    remove_folder(extensions_dir, id)
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
    if let Some(parent) = records_file.parent() {
        fs::create_dir_all(parent).map_err(|_| ExtensionError::Io)?;
    }
    if let Some(parent) = records_file.parent() {
        fs::create_dir_all(parent).map_err(|_| ExtensionError::Io)?;
    }
    let mut temporary = records_file.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let result = (|| -> io::Result<()> {
        let mut file = File::create(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, records_file)
    })();
    result.map_err(|_| {
        let _ = fs::remove_file(&temporary);
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

/// Human-readable summary for the install confirmation. It lists everything that grants access:
/// declared and optional permissions, host access, and the pages each content script runs on
/// (`<all_urls>` included).
pub fn permission_summary(manifest: &Manifest) -> String {
    let list = |items: &[String]| {
        if items.is_empty() {
            "none".to_owned()
        } else {
            items.join(", ")
        }
    };
    format!(
        "Manifest version {}.\nPermissions: {}\nSite access: {}\nContent scripts run on: {}\nOptional permissions: {}\nOptional site access: {}",
        manifest.manifest_version,
        list(&manifest.permissions),
        list(&manifest.host_permissions),
        list(&manifest.content_script_matches),
        list(&manifest.optional_permissions),
        list(&manifest.optional_host_permissions)
    )
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
}
