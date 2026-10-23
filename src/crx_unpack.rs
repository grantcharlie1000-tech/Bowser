//! Safe unpacker for the ZIP inside a verified CRX. Library only: no WebView2, no UI, no network.
//!
//! [`unpack`] takes a [`VerifiedCrx`] and reads nothing but [`VerifiedCrx::archive`], so the bytes it
//! writes are the bytes the proofs covered. It writes regular files into an empty folder the caller
//! just created and never touches anything outside it. Every check that can run on the central
//! directory runs before the first byte is written; whatever was written before a late failure is
//! removed again (only the files and folders this call created).
//!
//! The caps match `extensions::inspect_source` so an unpacked folder passes that inspection:
//! 1 MiB manifest, 2000 files, 128 MiB total, depth 12, no links, and no root name starting with `_`
//! except the `_locales` folder. On top of those: a per-file cap equal to the total (no single file
//! may be larger than the whole package is allowed to be), a 100:1 ratio cap on every entry that
//! expands past 1 MiB, and refusal of encrypted entries and of every compression method other than
//! stored and deflate. Size-policy rejections carry the entry name and the measured and allowed
//! sizes so the user can be told what was refused.
//!
//! [`manifest_with_key`] is a pure helper and is deliberately not called by [`unpack`];
//! [`unpack_stamped`] is the entry point that unpacks and then writes the developer key into the
//! manifest so the extension id is stable.

use crate::crx::VerifiedCrx;
use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use zip::CompressionMethod;
use zip::ZipArchive;
use zip::result::ZipError;

/// Largest `manifest.json` (same number as `extensions::MAX_MANIFEST_BYTES`).
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// Most files (same number as `extensions::MAX_FILES`).
pub const MAX_FILES: usize = 2_000;
/// Most uncompressed bytes over all files (same number as `extensions::MAX_TOTAL_BYTES`).
pub const MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
/// Deepest folder nesting below the root (same number as `extensions::MAX_DEPTH`).
pub const MAX_DEPTH: usize = 12;
/// Largest single file. Equal to [`MAX_TOTAL_BYTES`]: real Web Store packages ship one large asset
/// (a game's 64 MiB data file, a wasm model), and the aggregate cap already bounds what is written.
pub const MAX_FILE_BYTES: u64 = MAX_TOTAL_BYTES;
/// Most central-directory entries, files and folders together.
pub const MAX_ENTRIES: usize = MAX_FILES * 2;
/// Entries whose expanded size is over this are checked against [`MAX_RATIO`], however small their
/// compressed size is.
pub const RATIO_CHECK_FLOOR: u64 = 1024 * 1024;
/// Worst accepted uncompressed:compressed ratio on an entry that expands past [`RATIO_CHECK_FLOOR`].
pub const MAX_RATIO: u64 = 100;
/// Longest entry name kept in an error, in characters.
const MAX_ERROR_NAME_CHARS: usize = 60;

const LOCALES_FOLDER: &str = "_locales";
const MANIFEST_FILE_NAME: &str = "manifest.json";
const MAX_NAME_BYTES: usize = 255;
const COPY_CHUNK: usize = 64 * 1024;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// What a size-policy rejection measured: the entry that tripped the rule (bounded and stripped of
/// control and invisible characters, so it is safe to show), the size it reached and the size the
/// rule allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryLimit {
    pub name: String,
    pub measured: u64,
    pub allowed: u64,
}

impl EntryLimit {
    fn new(entry_name: &str, measured: u64, allowed: u64) -> Self {
        Self { name: display_name(entry_name), measured, allowed }
    }
}

/// A short, printable form of an archive entry name for an error message: control, bidi and other
/// invisible characters replaced, and at most [`MAX_ERROR_NAME_CHARS`] characters (the tail is kept,
/// since the file name is what identifies the entry).
pub fn display_name(name: &str) -> String {
    let clean: Vec<char> = name
        .chars()
        .map(|c| if c.is_control() || crate::extensions::is_invisible_format(c) { '\u{fffd}' } else { c })
        .collect();
    if clean.len() <= MAX_ERROR_NAME_CHARS {
        return clean.into_iter().collect();
    }
    let tail = &clean[clean.len() - (MAX_ERROR_NAME_CHARS - 1)..];
    std::iter::once('\u{2026}').chain(tail.iter().copied()).collect()
}

/// A byte count as `64.1 MiB`, `812 KiB` or `37 bytes`.
pub fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{} KiB", bytes / KIB)
    } else {
        format!("{bytes} bytes")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnpackError {
    /// The destination is missing, not a plain folder, or a link.
    BadDestination,
    /// The destination already holds something. Unpacking only ever fills an empty folder.
    DestinationNotEmpty,
    /// The bytes are not a ZIP this unpacker can read.
    MalformedArchive,
    TooManyEntries,
    TooManyFiles,
    TooDeep,
    /// Declared uncompressed sizes add up to more than [`MAX_TOTAL_BYTES`]. The detail names the
    /// entry that crossed the cap and the running total including it.
    TooLarge(EntryLimit),
    /// One entry declares more than [`MAX_FILE_BYTES`].
    FileTooLarge(EntryLimit),
    ManifestTooLarge,
    /// An entry that expands past [`RATIO_CHECK_FLOOR`] claims a ratio worse than [`MAX_RATIO`]:1.
    /// `measured` is its expanded size and `allowed` the most its compressed size permits.
    CompressionRatio(EntryLimit),
    /// An entry name is empty, absolute, has a drive, `..`, backslashes, NUL or other unsafe text.
    InvalidName,
    /// A root name starting with `_` that is not `_locales`, or `_locales` as a plain file.
    ReservedRootName,
    /// The same path appears twice (case-insensitively), or a file and a folder share a path.
    DuplicateEntry,
    Encrypted,
    /// A compression method other than stored or deflate.
    UnsupportedCompression,
    /// A symlink, device or other entry that is not a plain file or folder, or a folder with data.
    UnsupportedEntry,
    /// An entry's real size differs from its declared size.
    SizeMismatch,
    /// An entry's data is corrupt (bad deflate stream or CRC).
    CorruptEntry,
    /// A written path resolved outside the destination or is a link.
    EscapedDestination,
    Io,
}

impl UnpackError {
    /// True when Bowser's own package rules refused an otherwise readable archive (size, count,
    /// depth, names, entry kinds, compression), as opposed to damage, a bad destination or an I/O
    /// failure. The signature and identity proofs have already passed when this is reported.
    pub fn is_policy_rejection(&self) -> bool {
        !matches!(
            self,
            Self::BadDestination
                | Self::DestinationNotEmpty
                | Self::MalformedArchive
                | Self::SizeMismatch
                | Self::CorruptEntry
                | Self::EscapedDestination
                | Self::Io
        )
    }

    /// A sentence for the user about a policy rejection: what was refused, with the entry name and
    /// the measured and allowed sizes where the rule has them. `None` for anything else.
    pub fn policy_message(&self) -> Option<String> {
        let sizes = |limit: &EntryLimit| {
            format!("{}, {} > {}", limit.name, format_size(limit.measured), format_size(limit.allowed))
        };
        Some(match self {
            Self::FileTooLarge(limit) => {
                format!("This extension contains a file larger than Bowser allows ({}).", sizes(limit))
            }
            Self::TooLarge(limit) => {
                format!("This extension unpacks to more than Bowser allows in total ({}).", sizes(limit))
            }
            Self::CompressionRatio(limit) => format!(
                "This extension contains a file that expands far more than its packed size allows ({}).",
                sizes(limit)
            ),
            error if error.is_policy_rejection() => {
                format!("This extension package breaks one of Bowser's package rules: {error}.")
            }
            _ => return None,
        })
    }
}

impl fmt::Display for UnpackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge(limit) => {
                return write!(
                    f,
                    "archive expands to more than the size cap ({}, total {} > {})",
                    limit.name,
                    format_size(limit.measured),
                    format_size(limit.allowed)
                );
            }
            Self::FileTooLarge(limit) => {
                return write!(
                    f,
                    "an archive entry is larger than the per-file cap ({}, {} > {})",
                    limit.name,
                    format_size(limit.measured),
                    format_size(limit.allowed)
                );
            }
            Self::CompressionRatio(limit) => {
                return write!(
                    f,
                    "an archive entry compresses suspiciously well ({}, expands to {}, at most {} allowed)",
                    limit.name,
                    format_size(limit.measured),
                    format_size(limit.allowed)
                );
            }
            _ => {}
        }
        f.write_str(match self {
            Self::TooLarge(_) | Self::FileTooLarge(_) | Self::CompressionRatio(_) => unreachable!("written above"),
            Self::BadDestination => "destination is not a plain folder",
            Self::DestinationNotEmpty => "destination folder is not empty",
            Self::MalformedArchive => "archive is not a readable ZIP",
            Self::TooManyEntries => "archive has too many entries",
            Self::TooManyFiles => "archive has too many files",
            Self::TooDeep => "archive nests folders too deeply",
            Self::ManifestTooLarge => "manifest.json is larger than Bowser accepts",
            Self::InvalidName => "an archive entry has an unsafe name",
            Self::ReservedRootName => "an archive entry uses a reserved root name",
            Self::DuplicateEntry => "an archive entry appears twice",
            Self::Encrypted => "archive contains an encrypted entry",
            Self::UnsupportedCompression => "archive uses an unsupported compression method",
            Self::UnsupportedEntry => "archive contains a link or other unsupported entry",
            Self::SizeMismatch => "an archive entry is not the size it declares",
            Self::CorruptEntry => "an archive entry is corrupt",
            Self::EscapedDestination => "an entry resolved outside the destination",
            Self::Io => "could not write the unpacked files",
        })
    }
}

impl std::error::Error for UnpackError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnpackSummary {
    pub files: usize,
    pub bytes: u64,
}

/// Unpacks the archive of a verified CRX into `dest`, a folder the caller just created and that is
/// still empty. Nothing is installed, loaded or recorded.
pub fn unpack(crx: &VerifiedCrx<'_>, dest: &Path) -> Result<UnpackSummary, UnpackError> {
    unpack_archive(crx.archive(), dest)
}

fn unpack_archive(archive: &[u8], dest: &Path) -> Result<UnpackSummary, UnpackError> {
    unpack_tracked(archive, dest, false).map(|(summary, _)| summary)
}

/// [`unpack_archive`] that also returns what it created, so a caller that fails afterwards can remove
/// exactly those paths. On error everything is already rolled back and nothing is returned.
fn unpack_tracked(archive: &[u8], dest: &Path, omit_store_metadata: bool) -> Result<(UnpackSummary, Created), UnpackError> {
    check_destination(dest)?;
    let canonical_dest = fs::canonicalize(dest).map_err(|_| UnpackError::Io)?;

    let declared = declared_entry_count(archive)?;
    if declared > MAX_ENTRIES {
        return Err(UnpackError::TooManyEntries);
    }
    let mut zip = ZipArchive::new(Cursor::new(archive)).map_err(|_| UnpackError::MalformedArchive)?;
    // The zip crate keeps entries in a map keyed by name, so a repeated name silently replaces the
    // earlier entry and shrinks `len()`. The count in the end-of-central-directory record is the
    // number of headers actually present.
    if zip.len() != declared {
        return Err(UnpackError::DuplicateEntry);
    }

    let plan = plan_entries(&mut zip, omit_store_metadata)?;

    let mut created = Created::default();
    match write_entries(&mut zip, &plan, dest, &canonical_dest, &mut created) {
        Ok(summary) => Ok((summary, created)),
        Err(error) => {
            created.roll_back();
            Err(error)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StampError {
    /// The unpack itself was refused; see [`UnpackError`]. Nothing was stamped.
    Unpack(UnpackError),
    /// The archive has no `manifest.json` at its root.
    ManifestMissing,
    /// `manifest.json` is a folder, a link or anything else but a plain file inside the destination.
    ManifestNotAFile,
    ManifestTooLarge,
    /// The manifest cannot take a key (not a plain JSON object, or it holds comments).
    ManifestKey(ManifestKeyError),
    Io,
}

impl fmt::Display for StampError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unpack(error) => error.fmt(f),
            Self::ManifestMissing => f.write_str("the package has no manifest.json"),
            Self::ManifestNotAFile => f.write_str("manifest.json is not a plain file"),
            Self::ManifestTooLarge => f.write_str("manifest.json is larger than Bowser accepts"),
            Self::ManifestKey(_) => f.write_str("manifest.json cannot take a stable key (is it plain JSON?)"),
            Self::Io => f.write_str("could not write the stamped manifest"),
        }
    }
}

impl std::error::Error for StampError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StampedUnpack {
    /// The extension id the stamped `key` derives: always [`VerifiedCrx::extension_id`].
    pub id: String,
    pub summary: UnpackSummary,
}

/// [`unpack`], then sets the manifest's top-level `key` to the CRX's developer public key so the id
/// WebView2 assigns is the CRX's id wherever the folder lives, instead of one derived from its path.
///
/// Fails closed: if the manifest cannot be stamped (missing, not a plain file, over the manifest cap,
/// not JSON this scanner can read, or holding comments, which [`manifest_with_key`] refuses), every
/// file the unpack created is removed again and only the empty destination stays. A package whose id
/// would not be stable is never left behind to load. Nothing is installed or loaded here.
pub fn unpack_stamped(crx: &VerifiedCrx<'_>, dest: &Path) -> Result<StampedUnpack, StampError> {
    let (summary, mut created) = unpack_tracked(crx.archive(), dest, true).map_err(StampError::Unpack)?;
    let stamped = fs::canonicalize(dest)
        .map_err(|_| StampError::Io)
        .and_then(|canonical_dest| stamp_manifest(dest, &canonical_dest, crx.developer_public_key()));
    match stamped {
        Ok(()) => Ok(StampedUnpack { id: crx.extension_id().to_owned(), summary }),
        Err(error) => {
            created.roll_back();
            Err(error)
        }
    }
}

/// Rewrites `dest/manifest.json` with the key. The file is removed and re-created with `create_new`,
/// which refuses to open anything that appeared in between, so a link planted meanwhile is never
/// followed. A failure part-way can leave a partial file; the caller rolls the folder back then.
fn stamp_manifest(dest: &Path, canonical_dest: &Path, spki: &[u8]) -> Result<(), StampError> {
    let path = dest.join(MANIFEST_FILE_NAME);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(StampError::ManifestMissing),
        Err(_) => return Err(StampError::Io),
    };
    if !meta.is_file() || is_link(&meta) || verify_inside(canonical_dest, &path, false).is_err() {
        return Err(StampError::ManifestNotAFile);
    }
    if meta.len() > MAX_MANIFEST_BYTES {
        return Err(StampError::ManifestTooLarge);
    }
    let mut original = Vec::new();
    fs::File::open(&path)
        .and_then(|file| file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut original))
        .map_err(|_| StampError::Io)?;
    if original.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(StampError::ManifestTooLarge);
    }
    let stamped = manifest_with_key(&original, spki).map_err(StampError::ManifestKey)?;
    // The key makes the manifest bigger, so the cap applies to the stamped bytes too.
    if stamped.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(StampError::ManifestTooLarge);
    }

    fs::remove_file(&path).map_err(|_| StampError::Io)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| StampError::Io)?;
    file.write_all(&stamped).map_err(|_| StampError::Io)?;
    file.flush().map_err(|_| StampError::Io)?;
    drop(file);
    verify_inside(canonical_dest, &path, false).map_err(|_| StampError::ManifestNotAFile)
}

/// The destination must be a plain, empty folder.
fn check_destination(dest: &Path) -> Result<(), UnpackError> {
    let meta = fs::symlink_metadata(dest).map_err(|_| UnpackError::BadDestination)?;
    if !meta.is_dir() || is_link(&meta) {
        return Err(UnpackError::BadDestination);
    }
    let mut entries = fs::read_dir(dest).map_err(|_| UnpackError::Io)?;
    match entries.next() {
        None => Ok(()),
        Some(_) => Err(UnpackError::DestinationNotEmpty),
    }
}

/// Entry count from the end-of-central-directory record, found the way zip 1.1.4 finds it (the last
/// signature in the final 64 KiB + 22 bytes whose comment fits in the file), so both read the same
/// record. The archive must be exactly what that record describes: its comment ends at the end of the
/// file, the central directory ends where the record starts, and there is no zip64 locator (zip 1.1.4
/// would take the file count from the zip64 record instead). Nothing this small needs zip64, and
/// refusing it also keeps a forged count from sizing the crate's file table before [`MAX_ENTRIES`].
fn declared_entry_count(archive: &[u8]) -> Result<usize, UnpackError> {
    const SIGNATURE: &[u8; 4] = b"PK\x05\x06";
    const LOCATOR_SIGNATURE: &[u8; 4] = b"PK\x06\x07";
    const FIXED_LEN: usize = 22;
    const LOCATOR_DISTANCE: usize = 20;
    if archive.len() < FIXED_LEN {
        return Err(UnpackError::MalformedArchive);
    }
    let last = archive.len() - FIXED_LEN;
    let first = last.saturating_sub(u16::MAX as usize);
    let comment_len = |at: usize| u16::from_le_bytes([archive[at + 20], archive[at + 21]]) as usize;
    let at = (first..=last)
        .rev()
        .find(|&at| &archive[at..at + 4] == SIGNATURE && at + FIXED_LEN + comment_len(at) <= archive.len())
        .ok_or(UnpackError::MalformedArchive)?;
    if at + FIXED_LEN + comment_len(at) != archive.len() {
        return Err(UnpackError::MalformedArchive);
    }
    if at >= LOCATOR_DISTANCE && &archive[at - LOCATOR_DISTANCE..at - LOCATOR_DISTANCE + 4] == LOCATOR_SIGNATURE {
        return Err(UnpackError::MalformedArchive);
    }
    let on_disk = u16::from_le_bytes([archive[at + 8], archive[at + 9]]);
    let total = u16::from_le_bytes([archive[at + 10], archive[at + 11]]);
    let central_size = u32::from_le_bytes([archive[at + 12], archive[at + 13], archive[at + 14], archive[at + 15]]);
    let central_offset = u32::from_le_bytes([archive[at + 16], archive[at + 17], archive[at + 18], archive[at + 19]]);
    if on_disk != total || total == u16::MAX {
        return Err(UnpackError::MalformedArchive);
    }
    if central_offset as u64 + central_size as u64 != at as u64 {
        return Err(UnpackError::MalformedArchive);
    }
    Ok(total as usize)
}

#[derive(Debug)]
struct Planned {
    index: usize,
    /// Validated path components, never empty.
    parts: Vec<String>,
    folder: bool,
    size: u64,
    omit_metadata: bool,
}

/// Validates every entry from central-directory metadata alone; nothing is inflated or written.
fn plan_entries(zip: &mut ZipArchive<Cursor<&[u8]>>, omit_store_metadata: bool) -> Result<Vec<Planned>, UnpackError> {
    let mut plan = Vec::with_capacity(zip.len());
    let mut explicit: HashSet<String> = HashSet::new();
    let mut files: HashSet<String> = HashSet::new();
    let mut folders: HashSet<String> = HashSet::new();
    let mut file_count = 0usize;
    let mut total = 0u64;

    for index in 0..zip.len() {
        let entry = match zip.by_index(index) {
            Ok(entry) => entry,
            Err(ZipError::UnsupportedArchive(why)) if why == ZipError::PASSWORD_REQUIRED => {
                return Err(UnpackError::Encrypted);
            }
            // zip 1.1.4 refuses a method it was not built with here, before any data is read.
            Err(ZipError::UnsupportedArchive(why)) if why.starts_with("Compression method") => {
                return Err(UnpackError::UnsupportedCompression);
            }
            Err(_) => return Err(UnpackError::MalformedArchive),
        };
        let (parts, folder) = split_entry_name(entry.name())?;
        let size = entry.size();
        let compressed = entry.compressed_size();

        if !matches!(entry.compression(), CompressionMethod::Stored | CompressionMethod::Deflated) {
            return Err(UnpackError::UnsupportedCompression);
        }
        check_mode(entry.unix_mode(), folder)?;
        if folder && size != 0 {
            return Err(UnpackError::UnsupportedEntry);
        }
        if entry.compression() == CompressionMethod::Stored && size != compressed {
            return Err(UnpackError::SizeMismatch);
        }

        let depth = if folder { parts.len() } else { parts.len() - 1 };
        if depth > MAX_DEPTH {
            return Err(UnpackError::TooDeep);
        }
        let root = parts[0].as_str();
        // A verified store CRX normally contains Chrome-owned _metadata. It cannot be loaded
        // unpacked by WebView2. Validate and count it like every other entry, but never materialize
        // it. This is confined to the signature-verified stamped path, not arbitrary local folders.
        let omit_metadata = omit_store_metadata && root == "_metadata" && (parts.len() > 1 || folder);
        if root.starts_with('_') && !omit_metadata && (root != LOCALES_FOLDER || (parts.len() == 1 && !folder)) {
            return Err(UnpackError::ReservedRootName);
        }

        let key = parts.join("/").to_lowercase();
        if !explicit.insert(key.clone()) {
            return Err(UnpackError::DuplicateEntry);
        }
        let ancestors: Vec<String> = (1..parts.len()).map(|n| parts[..n].join("/").to_lowercase()).collect();
        if ancestors.iter().any(|ancestor| files.contains(ancestor)) {
            return Err(UnpackError::DuplicateEntry);
        }
        if folder {
            if files.contains(&key) {
                return Err(UnpackError::DuplicateEntry);
            }
            folders.insert(key);
        } else {
            if folders.contains(&key) {
                return Err(UnpackError::DuplicateEntry);
            }
            files.insert(key);
            file_count += 1;
            if file_count > MAX_FILES {
                return Err(UnpackError::TooManyFiles);
            }
            if parts.len() == 1 && root.eq_ignore_ascii_case(MANIFEST_FILE_NAME) && size > MAX_MANIFEST_BYTES {
                return Err(UnpackError::ManifestTooLarge);
            }
            if ratio_exceeded(compressed, size) {
                let allowed = compressed.max(1).saturating_mul(MAX_RATIO);
                return Err(UnpackError::CompressionRatio(EntryLimit::new(entry.name(), size, allowed)));
            }
            if size > MAX_FILE_BYTES {
                return Err(UnpackError::FileTooLarge(EntryLimit::new(entry.name(), size, MAX_FILE_BYTES)));
            }
            total = total.saturating_add(size);
            if total > MAX_TOTAL_BYTES {
                return Err(UnpackError::TooLarge(EntryLimit::new(entry.name(), total, MAX_TOTAL_BYTES)));
            }
        }
        folders.extend(ancestors);
        plan.push(Planned { index, parts, folder, size, omit_metadata });
    }
    Ok(plan)
}

/// True when an entry that expands past [`RATIO_CHECK_FLOOR`] expands past [`MAX_RATIO`]:1 of its
/// compressed size. Judged on the expanded size: a tiny compressed entry that inflates to many MiB
/// is the zip bomb, so a small compressed size never exempts it. A legitimate large asset that barely
/// compresses (64 MiB packed to 53 MiB) is nowhere near the ratio.
fn ratio_exceeded(compressed: u64, uncompressed: u64) -> bool {
    uncompressed > RATIO_CHECK_FLOOR && uncompressed > compressed.max(1).saturating_mul(MAX_RATIO)
}

/// Splits an entry name into validated components. A trailing `/` marks a folder entry.
fn split_entry_name(name: &str) -> Result<(Vec<String>, bool), UnpackError> {
    let (path, folder) = match name.strip_suffix('/') {
        Some(path) => (path, true),
        None => (name, false),
    };
    if path.is_empty() || name.contains('\\') {
        return Err(UnpackError::InvalidName);
    }
    let parts: Vec<String> = path.split('/').map(str::to_owned).collect();
    if !parts.iter().all(|part| is_valid_component(part)) {
        return Err(UnpackError::InvalidName);
    }
    Ok((parts, folder))
}

/// One plain name Windows will create as written: no separators, drive or stream syntax, control
/// characters, trailing dot or space (Windows strips those), or reserved device name.
fn is_valid_component(name: &str) -> bool {
    // Windows also maps COM and LPT followed by a superscript 1, 2 or 3, and the console handles
    // CONIN$ and CONOUT$, to \\.\ devices.
    const DEVICES: [&str; 30] = [
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1",
        "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9", "com\u{b9}", "com\u{b2}", "com\u{b3}",
        "lpt\u{b9}", "lpt\u{b2}", "lpt\u{b3}", "conin$", "conout$",
    ];
    let stem = name.split('.').next().unwrap_or("").trim_end_matches(' ').to_ascii_lowercase();
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.len() <= MAX_NAME_BYTES
        && !name.ends_with(['.', ' '])
        && !name.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
        && !DEVICES.contains(&stem.as_str())
}

/// Entries must be plain files or folders. ZIP has no hard links; a symlink is a Unix mode of
/// `S_IFLNK` and anything else that is not a regular file or folder is refused too.
fn check_mode(mode: Option<u32>, folder: bool) -> Result<(), UnpackError> {
    const TYPE_MASK: u32 = 0o170000;
    const REGULAR: u32 = 0o100000;
    const FOLDER: u32 = 0o040000;
    match mode.map(|mode| mode & TYPE_MASK) {
        None | Some(0) => Ok(()),
        Some(REGULAR) if !folder => Ok(()),
        Some(FOLDER) if folder => Ok(()),
        Some(_) => Err(UnpackError::UnsupportedEntry),
    }
}

/// What this call created inside the destination, in creation order, so a failure can remove
/// exactly that and nothing else.
#[derive(Default)]
struct Created {
    paths: Vec<(PathBuf, bool)>,
}

impl Created {
    fn roll_back(&mut self) {
        while let Some((path, folder)) = self.paths.pop() {
            // `remove_dir` only removes an empty folder, so nothing foreign can be deleted.
            let _ = if folder { fs::remove_dir(&path) } else { fs::remove_file(&path) };
        }
    }
}

fn write_entries(
    zip: &mut ZipArchive<Cursor<&[u8]>>,
    plan: &[Planned],
    dest: &Path,
    canonical_dest: &Path,
    created: &mut Created,
) -> Result<UnpackSummary, UnpackError> {
    let mut summary = UnpackSummary { files: 0, bytes: 0 };
    for item in plan {
        if item.omit_metadata {
            if !item.folder {
                let mut entry = zip.by_index(item.index).map_err(|_| UnpackError::CorruptEntry)?;
                copy_bounded(&mut entry, &mut std::io::sink(), item.size)?;
            }
            continue;
        }
        if item.folder {
            ensure_folders(dest, canonical_dest, &item.parts, created)?;
            continue;
        }
        let (name, parents) = item.parts.split_last().expect("planned entries are never empty");
        let parent = ensure_folders(dest, canonical_dest, parents, created)?;
        let target = parent.join(name);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|_| UnpackError::Io)?;
        created.paths.push((target.clone(), false));

        let mut entry = zip.by_index(item.index).map_err(|_| UnpackError::CorruptEntry)?;
        let written = copy_bounded(&mut entry, &mut file, item.size)?;
        file.flush().map_err(|_| UnpackError::Io)?;
        drop(file);

        verify_inside(canonical_dest, &target, false)?;
        summary.files += 1;
        summary.bytes += written;
    }
    Ok(summary)
}

/// Creates each missing folder of `parts` under `dest`, checking after every step that it is a plain
/// folder still inside the destination. Returns the full path.
fn ensure_folders(
    dest: &Path,
    canonical_dest: &Path,
    parts: &[String],
    created: &mut Created,
) -> Result<PathBuf, UnpackError> {
    let mut path = dest.to_path_buf();
    for part in parts {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() && !is_link(&meta) => {}
            Ok(_) => return Err(UnpackError::EscapedDestination),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&path).map_err(|_| UnpackError::Io)?;
                created.paths.push((path.clone(), true));
            }
            Err(_) => return Err(UnpackError::Io),
        }
        verify_inside(canonical_dest, &path, true)?;
    }
    Ok(path)
}

/// The path must resolve to a plain file (or folder) strictly inside the destination.
fn verify_inside(canonical_dest: &Path, path: &Path, folder: bool) -> Result<(), UnpackError> {
    let meta = fs::symlink_metadata(path).map_err(|_| UnpackError::EscapedDestination)?;
    if is_link(&meta) || meta.is_dir() != folder || (!folder && !meta.is_file()) {
        return Err(UnpackError::EscapedDestination);
    }
    let real = fs::canonicalize(path).map_err(|_| UnpackError::EscapedDestination)?;
    if real == canonical_dest || !real.starts_with(canonical_dest) {
        return Err(UnpackError::EscapedDestination);
    }
    Ok(())
}

/// Copies exactly `declared` bytes. The reader is bounded at `declared + 1`, so an entry that
/// inflates past its declared size is caught after at most one extra byte and nothing past the
/// declared size is written.
fn copy_bounded(reader: &mut impl Read, out: &mut impl Write, declared: u64) -> Result<u64, UnpackError> {
    let mut reader = reader.take(declared + 1);
    let mut buffer = vec![0u8; COPY_CHUNK];
    let mut written = 0u64;
    loop {
        let read = reader.read(&mut buffer).map_err(|_| UnpackError::CorruptEntry)?;
        if read == 0 {
            break;
        }
        if written + read as u64 > declared {
            return Err(UnpackError::SizeMismatch);
        }
        out.write_all(&buffer[..read]).map_err(|_| UnpackError::Io)?;
        written += read as u64;
    }
    if written != declared {
        return Err(UnpackError::SizeMismatch);
    }
    Ok(written)
}

fn is_link(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_type().is_symlink() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestKeyError {
    /// The manifest is not a JSON object this helper can scan.
    Malformed,
    EmptyKey,
}

/// Returns `manifest` with a top-level `key` member set to the standard base64 of `spki` (the
/// developer's DER SubjectPublicKeyInfo) and every other byte unchanged: an existing `key` value is
/// replaced in place, otherwise `,"key":"…"` is appended after the last member.
///
/// Pure: it touches no files and [`unpack`] does not call it; [`unpack_stamped`] does. JSON comments
/// are refused as [`ManifestKeyError::Malformed`] (Chrome would accept them), so such a manifest
/// fails closed rather than being stamped by a guess.
pub fn manifest_with_key(manifest: &[u8], spki: &[u8]) -> Result<Vec<u8>, ManifestKeyError> {
    if spki.is_empty() {
        return Err(ManifestKeyError::EmptyKey);
    }
    let value = format!("\"{}\"", base64(spki));
    let object = scan_top_level_object(manifest)?;
    let mut out = Vec::with_capacity(manifest.len() + value.len() + 8);
    if object.key_values.is_empty() {
        let (at, text) = match object.last_member_end {
            Some(end) => (end, format!(",\"key\":{value}")),
            None => (object.open + 1, format!("\"key\":{value}")),
        };
        out.extend_from_slice(&manifest[..at]);
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(&manifest[at..]);
    } else {
        let mut from = 0;
        for &(start, end) in &object.key_values {
            out.extend_from_slice(&manifest[from..start]);
            out.extend_from_slice(value.as_bytes());
            from = end;
        }
        out.extend_from_slice(&manifest[from..]);
    }
    Ok(out)
}

const MAX_SCAN_DEPTH: usize = 64;

struct TopLevelObject {
    /// Index of the opening `{`.
    open: usize,
    /// Byte just past the last member's value, if there is a member.
    last_member_end: Option<usize>,
    /// Byte ranges of the values of every top-level member named `key`.
    key_values: Vec<(usize, usize)>,
}

fn scan_top_level_object(bytes: &[u8]) -> Result<TopLevelObject, ManifestKeyError> {
    let mut at = if bytes.starts_with(b"\xEF\xBB\xBF") { 3 } else { 0 };
    at = skip_whitespace(bytes, at);
    if bytes.get(at) != Some(&b'{') {
        return Err(ManifestKeyError::Malformed);
    }
    let open = at;
    at = skip_whitespace(bytes, at + 1);
    let mut object = TopLevelObject { open, last_member_end: None, key_values: Vec::new() };
    if bytes.get(at) == Some(&b'}') {
        at += 1;
    } else {
        loop {
            if bytes.get(at) != Some(&b'"') {
                return Err(ManifestKeyError::Malformed);
            }
            let name_end = skip_string(bytes, at)?;
            let is_key = string_equals(&bytes[at + 1..name_end - 1], b"key");
            at = skip_whitespace(bytes, name_end);
            if bytes.get(at) != Some(&b':') {
                return Err(ManifestKeyError::Malformed);
            }
            let value_start = skip_whitespace(bytes, at + 1);
            let value_end = skip_value(bytes, value_start, 1)?;
            if is_key {
                object.key_values.push((value_start, value_end));
            }
            object.last_member_end = Some(value_end);
            at = skip_whitespace(bytes, value_end);
            match bytes.get(at) {
                Some(b',') => at = skip_whitespace(bytes, at + 1),
                Some(b'}') => {
                    at += 1;
                    break;
                }
                _ => return Err(ManifestKeyError::Malformed),
            }
        }
    }
    if skip_whitespace(bytes, at) != bytes.len() {
        return Err(ManifestKeyError::Malformed);
    }
    Ok(object)
}

fn skip_whitespace(bytes: &[u8], mut at: usize) -> usize {
    while matches!(bytes.get(at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        at += 1;
    }
    at
}

/// `at` is the opening quote; returns the index just past the closing quote.
fn skip_string(bytes: &[u8], at: usize) -> Result<usize, ManifestKeyError> {
    let mut i = at + 1;
    while let Some(&byte) = bytes.get(i) {
        match byte {
            b'"' => return Ok(i + 1),
            b'\\' => i += 2,
            0..=0x1F => return Err(ManifestKeyError::Malformed),
            _ => i += 1,
        }
    }
    Err(ManifestKeyError::Malformed)
}

/// Compares a raw string body (between the quotes) with `expected` after decoding JSON escapes.
fn string_equals(body: &[u8], expected: &[u8]) -> bool {
    if !body.contains(&b'\\') {
        return body == expected;
    }
    let mut decoded = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        if body[i] != b'\\' {
            decoded.push(body[i]);
            i += 1;
            continue;
        }
        match body.get(i + 1) {
            Some(b'u') => {
                let code = body
                    .get(i + 2..i + 6)
                    .and_then(|hex| std::str::from_utf8(hex).ok())
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .and_then(char::from_u32)
                    .unwrap_or('\u{FFFD}');
                let mut utf8 = [0u8; 4];
                decoded.extend_from_slice(code.encode_utf8(&mut utf8).as_bytes());
                i += 6;
            }
            Some(&byte) => {
                decoded.push(match byte {
                    b'b' => 8,
                    b'f' => 12,
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    other => other,
                });
                i += 2;
            }
            None => return false,
        }
    }
    decoded == expected
}

/// Returns the index just past the JSON value starting at `at`.
fn skip_value(bytes: &[u8], at: usize, depth: usize) -> Result<usize, ManifestKeyError> {
    if depth > MAX_SCAN_DEPTH {
        return Err(ManifestKeyError::Malformed);
    }
    match bytes.get(at) {
        Some(b'"') => skip_string(bytes, at),
        Some(b'{') => skip_container(bytes, at, depth, b'}', true),
        Some(b'[') => skip_container(bytes, at, depth, b']', false),
        Some(_) => {
            let end = at
                + bytes[at..]
                    .iter()
                    .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
                    .count();
            let token = &bytes[at..end];
            let numeric = !token.is_empty()
                && token.iter().all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'))
                && token.iter().any(u8::is_ascii_digit);
            if numeric || matches!(token, b"true" | b"false" | b"null") {
                Ok(end)
            } else {
                Err(ManifestKeyError::Malformed)
            }
        }
        None => Err(ManifestKeyError::Malformed),
    }
}

fn skip_container(bytes: &[u8], at: usize, depth: usize, close: u8, members: bool) -> Result<usize, ManifestKeyError> {
    let mut i = skip_whitespace(bytes, at + 1);
    if bytes.get(i) == Some(&close) {
        return Ok(i + 1);
    }
    loop {
        if members {
            if bytes.get(i) != Some(&b'"') {
                return Err(ManifestKeyError::Malformed);
            }
            i = skip_whitespace(bytes, skip_string(bytes, i)?);
            if bytes.get(i) != Some(&b':') {
                return Err(ManifestKeyError::Malformed);
            }
            i = skip_whitespace(bytes, i + 1);
        }
        i = skip_whitespace(bytes, skip_value(bytes, i, depth + 1)?);
        match bytes.get(i) {
            Some(b',') => i = skip_whitespace(bytes, i + 1),
            Some(&byte) if byte == close => return Ok(i + 1),
            _ => return Err(ManifestKeyError::Malformed),
        }
    }
}

/// Standard base64 (RFC 4648 section 4) with padding.
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let block = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(ALPHABET[(block >> 18) as usize & 63] as char);
        out.push(ALPHABET[(block >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(block >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[block as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // ---- in-memory ZIP builder -------------------------------------------------------------

    struct Entry {
        name: Vec<u8>,
        method: u16,
        flags: u16,
        made_by: u16,
        attr: u32,
        crc: u32,
        data: Vec<u8>,
        compressed_field: u32,
        size_field: u32,
    }

    const UNIX_MADE_BY: u16 = (3 << 8) | 20;
    const REGULAR: u32 = 0o100644 << 16;
    const FOLDER: u32 = 0o040755 << 16;

    fn crc32(data: &[u8]) -> u32 {
        // Table driven so a 64 MiB fixture is checksummed quickly even in a debug build.
        static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
        let table = TABLE.get_or_init(|| {
            let mut table = [0u32; 256];
            for (n, slot) in table.iter_mut().enumerate() {
                let mut crc = n as u32;
                for _ in 0..8 {
                    crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
                }
                *slot = crc;
            }
            table
        });
        let mut crc = !0u32;
        for &byte in data {
            crc = table[((crc ^ byte as u32) & 0xFF) as usize] ^ (crc >> 8);
        }
        !crc
    }

    /// A stored (method 0) entry.
    fn stored(name: &str, payload: &[u8]) -> Entry {
        let folder = name.ends_with('/');
        Entry {
            name: name.as_bytes().to_vec(),
            method: 0,
            flags: 0,
            made_by: UNIX_MADE_BY,
            attr: if folder { FOLDER } else { REGULAR },
            crc: crc32(payload),
            data: payload.to_vec(),
            compressed_field: payload.len() as u32,
            size_field: payload.len() as u32,
        }
    }

    /// A deflated (method 8) entry whose stream is one non-compressed deflate block.
    fn deflated(name: &str, payload: &[u8]) -> Entry {
        assert!(payload.len() <= u16::MAX as usize);
        let len = payload.len() as u16;
        let mut stream = vec![0x01];
        stream.extend_from_slice(&len.to_le_bytes());
        stream.extend_from_slice(&(!len).to_le_bytes());
        stream.extend_from_slice(payload);
        Entry {
            method: 8,
            compressed_field: stream.len() as u32,
            data: stream,
            ..stored(name, payload)
        }
    }

    /// A deflated (method 8) entry of any size whose stream is non-compressed blocks of at most
    /// 65535 bytes: a real deflate stream at a ratio just under 1:1, like an already-packed asset.
    fn deflated_large(name: &str, payload: Vec<u8>) -> Entry {
        let crc = crc32(&payload);
        let blocks = payload.chunks(u16::MAX as usize).count().max(1);
        let mut stream = Vec::with_capacity(payload.len() + blocks * 5);
        let mut chunks = payload.chunks(u16::MAX as usize).peekable();
        if chunks.peek().is_none() {
            stream.extend_from_slice(&[0x01, 0, 0, 0xFF, 0xFF]);
        }
        while let Some(chunk) = chunks.next() {
            let len = chunk.len() as u16;
            stream.push(u8::from(chunks.peek().is_none()));
            stream.extend_from_slice(&len.to_le_bytes());
            stream.extend_from_slice(&(!len).to_le_bytes());
            stream.extend_from_slice(chunk);
        }
        Entry {
            method: 8,
            crc,
            compressed_field: stream.len() as u32,
            size_field: payload.len() as u32,
            data: stream,
            ..stored(name, b"")
        }
    }

    /// An entry that only claims sizes in its headers (no payload behind them), for rules that run
    /// on the central directory before any byte is read.
    fn declared(name: &str, compressed: u64, size: u64) -> Entry {
        Entry { method: 8, compressed_field: compressed as u32, size_field: size as u32, ..stored(name, b"tiny") }
    }

    /// Runs only the central-directory planning on `entries`.
    fn plan(entries: &[Entry]) -> Result<Vec<Planned>, UnpackError> {
        let archive = zip_bytes(entries);
        let mut zip = ZipArchive::new(Cursor::new(&archive[..])).expect("readable archive");
        plan_entries(&mut zip, false)
    }

    fn limit(name: &str, measured: u64, allowed: u64) -> EntryLimit {
        EntryLimit { name: name.to_owned(), measured, allowed }
    }

    /// Local entries followed by the central directory; returns the bytes, the central directory
    /// offset and its length. No end record yet.
    fn zip_parts(entries: &[Entry]) -> (Vec<u8>, u32, u32) {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for entry in entries {
            let offset = out.len() as u32;
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&entry.flags.to_le_bytes());
            out.extend_from_slice(&entry.method.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&33u16.to_le_bytes());
            out.extend_from_slice(&entry.crc.to_le_bytes());
            out.extend_from_slice(&entry.compressed_field.to_le_bytes());
            out.extend_from_slice(&entry.size_field.to_le_bytes());
            out.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&entry.name);
            out.extend_from_slice(&entry.data);

            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&entry.made_by.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&entry.flags.to_le_bytes());
            central.extend_from_slice(&entry.method.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&33u16.to_le_bytes());
            central.extend_from_slice(&entry.crc.to_le_bytes());
            central.extend_from_slice(&entry.compressed_field.to_le_bytes());
            central.extend_from_slice(&entry.size_field.to_le_bytes());
            central.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&entry.attr.to_le_bytes());
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(&entry.name);
        }
        let central_offset = out.len() as u32;
        out.extend_from_slice(&central);
        (out, central_offset, central.len() as u32)
    }

    /// An end-of-central-directory record. `comment_len` is the declared length and need not match
    /// `comment`.
    fn end_record(count: u16, central_len: u32, central_offset: u32, comment: &[u8], comment_len: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&central_len.to_le_bytes());
        out.extend_from_slice(&central_offset.to_le_bytes());
        out.extend_from_slice(&comment_len.to_le_bytes());
        out.extend_from_slice(comment);
        out
    }

    /// A zip64 end record followed by its locator, as they sit right before a zip32 end record.
    /// `at` is where the record starts in the archive.
    fn zip64_tail(at: u64, count: u64, central_len: u64, central_offset: u64) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0x0606_4b50u32.to_le_bytes());
        out.extend_from_slice(&44u64.to_le_bytes());
        out.extend_from_slice(&45u16.to_le_bytes());
        out.extend_from_slice(&45u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&central_len.to_le_bytes());
        out.extend_from_slice(&central_offset.to_le_bytes());
        out.extend_from_slice(&0x0706_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out
    }

    fn zip_bytes(entries: &[Entry]) -> Vec<u8> {
        let (mut out, central_offset, central_len) = zip_parts(entries);
        out.extend_from_slice(&end_record(entries.len() as u16, central_len, central_offset, &[], 0));
        out
    }

    // ---- scratch folders -------------------------------------------------------------------

    /// A folder this test created; removed (only that folder) on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "bowser-crx-unpack-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir(&path).expect("create scratch folder");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Every path under `root`, relative with `/`, sorted; folders end in `/`.
    fn listing(root: &Path) -> Vec<String> {
        fn walk(root: &Path, at: &Path, out: &mut Vec<String>) {
            for entry in fs::read_dir(at).unwrap() {
                let entry = entry.unwrap();
                let relative = entry.path().strip_prefix(root).unwrap().to_owned();
                let text = relative.to_string_lossy().replace('\\', "/");
                if entry.file_type().unwrap().is_dir() {
                    out.push(format!("{text}/"));
                    walk(root, &entry.path(), out);
                } else {
                    out.push(text);
                }
            }
        }
        let mut out = Vec::new();
        walk(root, root, &mut out);
        out.sort();
        out
    }

    const MANIFEST: &str = r#"{"manifest_version": 3, "name": "Demo", "version": "1.2.3"}"#;

    fn run(entries: &[Entry]) -> (Scratch, Result<UnpackSummary, UnpackError>) {
        let scratch = Scratch::new();
        let result = unpack_archive(&zip_bytes(entries), &scratch.0);
        (scratch, result)
    }

    fn assert_rejected(entries: &[Entry], expected: UnpackError) {
        let (scratch, result) = run(entries);
        assert_eq!(result, Err(expected));
        assert_eq!(listing(&scratch.0), Vec::<String>::new(), "destination must be unchanged");
    }

    // ---- unpacking -------------------------------------------------------------------------

    #[test]
    fn unpacks_a_stored_and_a_deflated_entry() {
        let (scratch, result) = run(&[
            stored("manifest.json", MANIFEST.as_bytes()),
            deflated("js/background.js", b"console.log('hi');"),
            stored("js/", b""),
        ]);
        assert_eq!(result, Ok(UnpackSummary { files: 2, bytes: (MANIFEST.len() + b"console.log('hi');".len()) as u64 }));
        assert_eq!(fs::read(scratch.0.join("manifest.json")).unwrap(), MANIFEST.as_bytes());
        assert_eq!(fs::read(scratch.0.join("js").join("background.js")).unwrap(), b"console.log('hi');");
        assert_eq!(listing(&scratch.0), ["js/", "js/background.js", "manifest.json"]);
    }

    #[test]
    fn unpacked_folder_passes_inspect_source() {
        let (scratch, result) = run(&[
            stored("manifest.json", MANIFEST.as_bytes()),
            stored("_locales/en/messages.json", b"{}"),
            deflated("a/b.js", b"1"),
        ]);
        result.unwrap();
        let inspection = crate::extensions::inspect_source(&scratch.0).expect("inspect_source");
        assert_eq!(inspection.files, 3);
    }

    #[test]
    fn rejects_traversal_absolute_drive_and_odd_names() {
        for name in [
            "../x",
            "a/../../x",
            "a/../x",
            "./x",
            "a/./x",
            "/x",
            "C:\\x",
            "C:/x",
            "a\\b",
            "a//b",
            "x\0y",
            "",
            "/",
            "CON",
            "nul.txt",
            "a/trailing.",
            "a/trailing ",
            "a:stream",
        ] {
            assert_rejected(&[stored(name, b"x")], UnpackError::InvalidName);
        }
    }

    #[test]
    fn reserved_root_names_follow_inspect_source() {
        assert_rejected(&[stored("_metadata/verified_contents.json", b"x")], UnpackError::ReservedRootName);
        assert_rejected(&[stored("_metadata", b"x")], UnpackError::ReservedRootName);
        assert_rejected(&[stored("_other/", b"")], UnpackError::ReservedRootName);
        assert_rejected(&[stored("_locales", b"x")], UnpackError::ReservedRootName);
        assert_rejected(&[stored("a/_x", b"1"), stored("_x", b"1")], UnpackError::ReservedRootName);

        let (scratch, result) = run(&[
            stored("manifest.json", MANIFEST.as_bytes()),
            stored("_locales/en/messages.json", b"{}"),
            stored("sub/_metadata/ok.txt", b"1"),
        ]);
        result.unwrap();
        assert!(scratch.0.join("_locales").join("en").join("messages.json").is_file());
        assert!(scratch.0.join("sub").join("_metadata").join("ok.txt").is_file());
    }

    #[test]
    fn rejects_a_repeated_entry() {
        assert_rejected(
            &[stored("a.txt", b"1"), stored("b.txt", b"2"), stored("a.txt", b"3")],
            UnpackError::DuplicateEntry,
        );
        assert_rejected(&[stored("a.txt", b"1"), stored("A.TXT", b"2")], UnpackError::DuplicateEntry);
        assert_rejected(&[stored("d/", b""), stored("d/", b"")], UnpackError::DuplicateEntry);
        assert_rejected(&[stored("a", b"1"), stored("a/b", b"2")], UnpackError::DuplicateEntry);
        assert_rejected(&[stored("a/b", b"2"), stored("a", b"1")], UnpackError::DuplicateEntry);
        // A folder entry after the files that implied it is legitimate.
        let (_scratch, result) = run(&[stored("a/b.txt", b"1"), stored("a/", b"")]);
        assert_eq!(result, Ok(UnpackSummary { files: 1, bytes: 1 }));
    }

    #[test]
    fn rejects_a_declared_size_over_the_caps_without_writing() {
        // Half-packed claims keep these clear of the ratio rule, so the size rules are what refuse.
        let huge = declared("big.bin", (MAX_FILE_BYTES + 1) / 2, MAX_FILE_BYTES + 1);
        assert_rejected(&[huge], UnpackError::FileTooLarge(limit("big.bin", MAX_FILE_BYTES + 1, MAX_FILE_BYTES)));

        let mut manifest = stored("manifest.json", b"{}");
        manifest.size_field = (MAX_MANIFEST_BYTES + 1) as u32;
        manifest.method = 8;
        assert_rejected(&[manifest], UnpackError::ManifestTooLarge);

        // Three 64 MiB files are 192 MiB declared: the second fills the total, the third crosses it.
        let half = MAX_TOTAL_BYTES / 2;
        assert_rejected(
            &[declared("a", half / 2, half), declared("b", half / 2, half), declared("c", half / 2, half)],
            UnpackError::TooLarge(limit("c", half * 3, MAX_TOTAL_BYTES)),
        );
    }

    #[test]
    fn per_file_and_total_limits_have_exact_boundaries() {
        assert_eq!(MAX_FILE_BYTES, MAX_TOTAL_BYTES, "no single file may exceed the whole package budget");
        // One file of exactly the cap, or two that add up to exactly the cap, are planned.
        assert!(plan(&[declared("one", MAX_FILE_BYTES / 2, MAX_FILE_BYTES)]).is_ok());
        let half = MAX_TOTAL_BYTES / 2;
        assert_eq!(plan(&[declared("a", half / 2, half), declared("b", half / 2, half)]).map(|p| p.len()), Ok(2));
        // One byte more on either rule is refused, and the error reports the measured size.
        assert_eq!(
            plan(&[declared("one", MAX_FILE_BYTES / 2, MAX_FILE_BYTES + 1)]).unwrap_err(),
            UnpackError::FileTooLarge(limit("one", MAX_FILE_BYTES + 1, MAX_FILE_BYTES))
        );
        assert_eq!(
            plan(&[declared("a", half / 2, half), declared("b", half / 2, half + 1)]).unwrap_err(),
            UnpackError::TooLarge(limit("b", MAX_TOTAL_BYTES + 1, MAX_TOTAL_BYTES))
        );
    }

    #[test]
    fn the_real_store_package_sizes_are_accepted() {
        // aomkpefnllinimbhddlfhelelngakbbn: one 67,207,321 byte entry packed to 55,332,675 inside an
        // archive of 88,260,626 bytes expanded over 178 entries.
        const BIG: u64 = 67_207_321;
        const PACKED: u64 = 55_332_675;
        const REST: u64 = 88_260_626 - BIG;
        assert!(BIG > 32 * 1024 * 1024 && BIG < MAX_FILE_BYTES);
        assert!(!ratio_exceeded(PACKED, BIG));
        let mut entries = vec![declared("Game_Source/Build/Ultimate Car Driving.data", PACKED, BIG)];
        entries.push(declared("Game_Source/Build/rest.bin", REST / 2, REST));
        assert_eq!(plan(&entries).map(|p| p.len()), Ok(2));
    }

    #[test]
    fn unpacks_a_67_mib_deflated_entry_end_to_end() {
        // Not a checked-in fixture: generated bytes, checksummed and framed as a real deflate stream
        // (non-compressed blocks, so ratio ~1:1 like an already-packed game data file).
        const BIG: usize = 67_207_321;
        let payload: Vec<u8> = (0..BIG).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        let (scratch, result) = run(&[
            stored("manifest.json", MANIFEST.as_bytes()),
            deflated_large("Game_Source/Build/Ultimate Car Driving.data", payload),
        ]);
        assert_eq!(result, Ok(UnpackSummary { files: 2, bytes: (MANIFEST.len() + BIG) as u64 }));
        let written = scratch.0.join("Game_Source").join("Build").join("Ultimate Car Driving.data");
        assert_eq!(fs::metadata(&written).unwrap().len(), BIG as u64);
    }

    #[test]
    fn a_large_entry_still_rolls_back_on_a_crc_or_size_mismatch() {
        // 33 MiB: over the old 32 MiB cap, so this entry could not even be tried before.
        let payload = || vec![0x5Au8; 33 * 1024 * 1024];
        let mut bad_crc = deflated_large("assets/big.bin", payload());
        bad_crc.crc ^= 1;
        let (scratch, result) = run(&[stored("manifest.json", b"{}"), stored("sub/ok.txt", b"1"), bad_crc]);
        assert_eq!(result, Err(UnpackError::CorruptEntry));
        assert_eq!(listing(&scratch.0), Vec::<String>::new(), "destination must be unchanged");

        let mut short = deflated_large("assets/big.bin", payload());
        short.size_field -= 1;
        let (scratch, result) = run(&[stored("manifest.json", b"{}"), short]);
        assert_eq!(result, Err(UnpackError::SizeMismatch));
        assert_eq!(listing(&scratch.0), Vec::<String>::new(), "destination must be unchanged");
    }

    #[test]
    fn ratio_rule_is_judged_on_the_expanded_size() {
        const MIB: u64 = 1024 * 1024;
        // Over 1 MiB expanded, more than 100:1 of the packed size is refused, however small the
        // packed size (these slipped through when only packed sizes over 1 MiB were checked).
        assert!(ratio_exceeded(1024, 100 * MIB));
        assert!(ratio_exceeded(0, 2 * MIB));
        assert!(ratio_exceeded(10 * 1024, 2 * MIB));
        assert!(ratio_exceeded(2 * MIB, 2 * MIB * 101));
        // Exactly 100:1, or at most 1 MiB expanded, is fine; so is a barely compressed large entry.
        assert!(!ratio_exceeded(20_480, 2_048_000));
        assert!(!ratio_exceeded(10, MIB));
        assert!(!ratio_exceeded(1, 1000));
        assert!(!ratio_exceeded(55_332_675, 67_207_321));

        let bomb = declared("bomb.bin", 1024, 100 * MIB);
        assert_rejected(&[bomb], UnpackError::CompressionRatio(limit("bomb.bin", 100 * MIB, 1024 * MAX_RATIO)));
    }

    #[test]
    fn size_errors_name_the_entry_and_bound_what_they_show() {
        let long = format!("{}/{}", "d".repeat(40), "f".repeat(100));
        let error = plan(&[declared(&long, MAX_FILE_BYTES, MAX_FILE_BYTES + 1)]).unwrap_err();
        let UnpackError::FileTooLarge(detail) = &error else { panic!("{error:?}") };
        assert_eq!(detail.name.chars().count(), MAX_ERROR_NAME_CHARS);
        assert!(detail.name.starts_with('\u{2026}') && long.ends_with(&detail.name[detail.name.char_indices().nth(1).unwrap().0..]));
        assert_eq!(display_name("a\u{202e}b\u{0}c\nd"), "a\u{fffd}b\u{fffd}c\u{fffd}d");

        let message = UnpackError::FileTooLarge(limit("Build/a.data", 67_207_321, MAX_FILE_BYTES)).policy_message().unwrap();
        assert_eq!(message, "This extension contains a file larger than Bowser allows (Build/a.data, 64.1 MiB > 128.0 MiB).");
        assert!(error.to_string().contains("per-file cap"), "{error}");
        assert_eq!(format_size(0), "0 bytes");
        assert_eq!(format_size(2048), "2 KiB");
        // Damage and I/O failures are not policy rejections and have no policy sentence.
        for damage in [UnpackError::CorruptEntry, UnpackError::SizeMismatch, UnpackError::MalformedArchive, UnpackError::Io] {
            assert!(!damage.is_policy_rejection() && damage.policy_message().is_none(), "{damage:?}");
        }
        assert!(UnpackError::Encrypted.is_policy_rejection());
        assert!(UnpackError::Encrypted.policy_message().unwrap().contains("package rules"));
    }

    #[test]
    fn rejects_encrypted_entries() {
        let mut entry = stored("secret.txt", b"x");
        entry.flags = 1;
        assert_rejected(&[entry], UnpackError::Encrypted);
    }

    #[test]
    fn rejects_symlinks_and_other_special_entries() {
        let mut link = stored("link", b"target");
        link.attr = 0o120777 << 16;
        assert_rejected(&[link], UnpackError::UnsupportedEntry);

        let mut device = stored("dev", b"");
        device.attr = 0o020666 << 16;
        assert_rejected(&[device], UnpackError::UnsupportedEntry);

        let mut folder_with_data = stored("d/", b"");
        folder_with_data.size_field = 5;
        assert_rejected(&[folder_with_data], UnpackError::UnsupportedEntry);

        let mut file_as_folder = stored("d/", b"");
        file_as_folder.attr = REGULAR;
        assert_rejected(&[file_as_folder], UnpackError::UnsupportedEntry);
    }

    #[test]
    fn rejects_other_compression_methods_without_panicking() {
        for method in [1u16, 9, 12, 14, 93, 95, 98] {
            let mut entry = stored("a.bin", b"x");
            entry.method = method;
            let (scratch, result) = run(&[entry]);
            assert_eq!(result, Err(UnpackError::UnsupportedCompression), "method {method}");
            assert_eq!(listing(&scratch.0), Vec::<String>::new());
        }
        // Method 99 marks WinZip AES; without its extra field the header is malformed. Either way it
        // is refused.
        let mut aes = stored("a.bin", b"x");
        aes.method = 99;
        assert_rejected(&[aes], UnpackError::MalformedArchive);
    }

    #[test]
    fn enforces_depth_and_file_count_caps() {
        let deep = |levels: usize| format!("{}f.txt", "d/".repeat(levels));
        let (_scratch, ok) = run(&[stored(&deep(MAX_DEPTH), b"1")]);
        assert_eq!(ok, Ok(UnpackSummary { files: 1, bytes: 1 }));
        assert_rejected(&[stored(&deep(MAX_DEPTH + 1), b"1")], UnpackError::TooDeep);
        assert_rejected(&[stored(&"d/".repeat(MAX_DEPTH + 1), b"")], UnpackError::TooDeep);

        let many: Vec<Entry> = (0..=MAX_FILES).map(|i| stored(&format!("f{i}"), b"")).collect();
        assert_rejected(&many, UnpackError::TooManyFiles);
        let (_scratch, ok) = run(&many[..MAX_FILES]);
        assert_eq!(ok, Ok(UnpackSummary { files: MAX_FILES, bytes: 0 }));
    }

    #[test]
    fn rejects_a_non_empty_destination_and_leaves_it_alone() {
        let scratch = Scratch::new();
        fs::write(scratch.0.join("keep.txt"), b"mine").unwrap();
        let archive = zip_bytes(&[stored("keep.txt", b"theirs"), stored("manifest.json", b"{}")]);
        assert_eq!(unpack_archive(&archive, &scratch.0), Err(UnpackError::DestinationNotEmpty));
        assert_eq!(fs::read(scratch.0.join("keep.txt")).unwrap(), b"mine");
        assert_eq!(listing(&scratch.0), ["keep.txt"]);
    }

    #[test]
    fn rejects_a_missing_destination_and_a_file_destination() {
        let scratch = Scratch::new();
        let archive = zip_bytes(&[stored("a", b"1")]);
        assert_eq!(unpack_archive(&archive, &scratch.0.join("missing")), Err(UnpackError::BadDestination));
        let file = scratch.0.join("file");
        fs::write(&file, b"x").unwrap();
        assert_eq!(unpack_archive(&archive, &file), Err(UnpackError::BadDestination));
    }

    #[test]
    fn rejects_garbage_and_truncated_archives() {
        let scratch = Scratch::new();
        assert_eq!(unpack_archive(b"", &scratch.0), Err(UnpackError::MalformedArchive));
        assert_eq!(unpack_archive(b"not a zip at all, just text", &scratch.0), Err(UnpackError::MalformedArchive));
        let mut archive = zip_bytes(&[stored("a", b"1")]);
        archive.truncate(archive.len() - 5);
        assert_eq!(unpack_archive(&archive, &scratch.0), Err(UnpackError::MalformedArchive));
        assert_eq!(listing(&scratch.0), Vec::<String>::new());
    }

    #[test]
    fn rolls_back_files_written_before_a_late_failure() {
        // The second entry's CRC is wrong, which only shows once its data is read, after the first
        // entry and a folder were already written.
        let mut bad = stored("sub/bad.txt", b"payload");
        bad.crc ^= 0xFFFF;
        let (scratch, result) = run(&[stored("manifest.json", b"{}"), stored("sub/ok.txt", b"1"), bad]);
        assert_eq!(result, Err(UnpackError::CorruptEntry));
        assert_eq!(listing(&scratch.0), Vec::<String>::new(), "destination must be unchanged");
        assert!(scratch.0.is_dir(), "the caller's folder itself stays");
    }

    #[test]
    fn rejects_data_longer_than_declared_and_rolls_back() {
        let mut lying = deflated("lie.txt", b"0123456789");
        lying.size_field = 4;
        let (scratch, result) = run(&[stored("first", b"1"), lying]);
        assert_eq!(result, Err(UnpackError::SizeMismatch));
        assert_eq!(listing(&scratch.0), Vec::<String>::new());

        let mut short = deflated("short.txt", b"0123456789");
        short.size_field = 40;
        let (scratch, result) = run(&[short]);
        assert!(result.is_err());
        assert_eq!(listing(&scratch.0), Vec::<String>::new());
    }

    #[test]
    fn rejects_a_stored_entry_whose_sizes_disagree() {
        let mut entry = stored("a.bin", b"abcd");
        entry.size_field = 9;
        assert_rejected(&[entry], UnpackError::SizeMismatch);
    }

    fn two_manifests() -> [Entry; 3] {
        [stored("manifest.json", b"FIRST"), stored("manifest.json", b"SECOND"), stored("b.txt", b"b")]
    }

    #[test]
    fn rejects_a_repeated_entry_hidden_behind_a_fake_end_record() {
        // The real end record (3 entries) carries, in its comment, a second end record that claims 2
        // entries and a comment running past the end of the file. zip 1.1.4 skips the fake one and
        // reads the real one, then keeps only the last manifest.json: 2 entries, matching the fake.
        let (mut archive, offset, len) = zip_parts(&two_manifests());
        let fake = end_record(2, 0, 0, &[], 0xFFFF);
        archive.extend_from_slice(&end_record(3, len, offset, &fake, fake.len() as u16));

        let scratch = Scratch::new();
        assert_eq!(unpack_archive(&archive, &scratch.0), Err(UnpackError::DuplicateEntry));
        assert_eq!(listing(&scratch.0), Vec::<String>::new(), "destination must be unchanged");
    }

    #[test]
    fn rejects_a_zip64_locator_so_the_zip_crate_cannot_use_another_count() {
        // The zip32 record has an offset that cannot be valid and claims 2 entries; zip 1.1.4 then
        // takes the file count (3) from the zip64 record and keeps 2 of the 3 headers.
        let (mut archive, offset, len) = zip_parts(&two_manifests());
        let tail = zip64_tail(archive.len() as u64, 3, len as u64, offset as u64);
        archive.extend_from_slice(&tail);
        archive.extend_from_slice(&end_record(2, 0, 0xFFFF_FFF0, &[], 0));

        let scratch = Scratch::new();
        assert_eq!(unpack_archive(&archive, &scratch.0), Err(UnpackError::MalformedArchive));
        assert_eq!(listing(&scratch.0), Vec::<String>::new(), "destination must be unchanged");
    }

    #[test]
    fn rejects_windows_device_names_with_superscripts_and_console_names() {
        let parent = Scratch::new();
        let dest = parent.0.join("dest");
        fs::create_dir(&dest).unwrap();

        let mut names: Vec<String> = Vec::new();
        for stem in ["COM", "LPT"] {
            for superscript in ['\u{00B9}', '\u{00B2}', '\u{00B3}'] {
                names.push(format!("{stem}{superscript}"));
            }
            for digit in 1..=9 {
                names.push(format!("{stem}{digit}"));
            }
        }
        names.extend(["CONIN$", "CONOUT$", "conin$", "Conout$"].map(str::to_owned));
        assert_eq!(names.len(), 24 + 4);

        for name in &names {
            for path in [name.clone(), format!("sub/{name}"), format!("{name}.txt")] {
                // Bit 11 marks the name as UTF-8; without it the zip crate reads it as CP437.
                let mut entry = stored(&path, b"x");
                entry.flags = 0x800;
                let result = unpack_archive(&zip_bytes(&[entry]), &dest);
                assert_eq!(result, Err(UnpackError::InvalidName), "{path:?}");
            }
        }
        assert_eq!(listing(&dest), Vec::<String>::new(), "destination must be unchanged");
        assert_eq!(listing(&parent.0), ["dest/"], "nothing may appear outside the destination");
    }

    // ---- manifest key helper ---------------------------------------------------------------

    #[test]
    fn base64_matches_the_standard_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xFB, 0xFF, 0xFE]), "+//+");
    }

    fn key(manifest: &str, spki: &[u8]) -> String {
        String::from_utf8(manifest_with_key(manifest.as_bytes(), spki).unwrap()).unwrap()
    }

    #[test]
    fn key_is_appended_when_absent_and_nothing_else_changes() {
        let spki = [0xFB, 0xFF, 0xFE];
        assert_eq!(
            key(r#"{"name": "Demo", "version": "1"}"#, &spki),
            r#"{"name": "Demo", "version": "1","key":"+//+"}"#
        );
        assert_eq!(key("{}", &spki), r#"{"key":"+//+"}"#);
        assert_eq!(key("{ }", &spki), r#"{"key":"+//+" }"#);
        assert_eq!(
            key("{\n  \"name\": \"Demo\"\n}\n", &spki),
            "{\n  \"name\": \"Demo\",\"key\":\"+//+\"\n}\n"
        );
        assert_eq!(
            manifest_with_key(b"\xEF\xBB\xBF{\"a\":1}", &spki).unwrap(),
            b"\xEF\xBB\xBF{\"a\":1,\"key\":\"+//+\"}"
        );
    }

    #[test]
    fn key_is_replaced_in_place_when_present() {
        let spki = [0xFB, 0xFF, 0xFE];
        assert_eq!(
            key(r#"{"key" : "OLD", "name": "Demo"}"#, &spki),
            r#"{"key" : "+//+", "name": "Demo"}"#
        );
        assert_eq!(key(r#"{"k\u0065y":null,"a":[1,{"key":"x"}]}"#, &spki), r#"{"k\u0065y":"+//+","a":[1,{"key":"x"}]}"#);
        assert_eq!(key(r#"{"key":"A","key":"B"}"#, &spki), r#"{"key":"+//+","key":"+//+"}"#);
    }

    #[test]
    fn nested_and_lookalike_keys_are_left_alone() {
        let spki = [0xFB, 0xFF, 0xFE];
        assert_eq!(
            key(r#"{"a":{"key":"inner"},"keys":1,"b":"\"key\":"}"#, &spki),
            r#"{"a":{"key":"inner"},"keys":1,"b":"\"key\":","key":"+//+"}"#
        );
    }

    #[test]
    fn key_helper_rejects_non_objects_and_broken_json() {
        for bad in ["", "[]", "\"x\"", "{", "{\"a\":}", "{\"a\":1,}", "{\"a\":1} x", "{\"a\" 1}", "{a:1}", "{\"a\":tru}"] {
            assert_eq!(manifest_with_key(bad.as_bytes(), &[1]), Err(ManifestKeyError::Malformed), "{bad:?}");
        }
        assert_eq!(manifest_with_key(b"{}", &[]), Err(ManifestKeyError::EmptyKey));
    }

    #[test]
    fn key_injection_is_not_part_of_unpack() {
        let (scratch, result) = run(&[stored("manifest.json", MANIFEST.as_bytes())]);
        result.unwrap();
        assert_eq!(fs::read(scratch.0.join("manifest.json")).unwrap(), MANIFEST.as_bytes());
    }

    // ---- stamping --------------------------------------------------------------------------

    /// Independent standard-base64 decoder (the production side only encodes).
    fn base64_decode(text: &str) -> Vec<u8> {
        let value = |c: u8| match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            other => panic!("not base64: {other}"),
        };
        let bytes = text.as_bytes();
        assert_eq!(bytes.len() % 4, 0);
        let mut out = Vec::new();
        for chunk in bytes.chunks(4) {
            let pad = chunk.iter().filter(|&&c| c == b'=').count();
            let block = chunk[..4 - pad].iter().fold(0u32, |acc, &c| acc << 6 | value(c) as u32) << (6 * pad);
            out.extend_from_slice(&block.to_be_bytes()[1..4 - pad]);
        }
        out
    }

    /// The value of the first `"key":"…"` member in `manifest`, decoded.
    fn stamped_key(manifest: &str) -> Vec<u8> {
        let start = manifest.find("\"key\":\"").expect("a key member") + "\"key\":\"".len();
        let end = start + manifest[start..].find('"').expect("closing quote");
        base64_decode(&manifest[start..end])
    }

    fn sample_spki() -> Vec<u8> {
        (0..294u32).map(|i| (i * 7 + 3) as u8).collect()
    }

    fn verified_unpack(archive: &[u8], dest: &Path) -> (String, Result<StampedUnpack, StampError>, Vec<u8>) {
        let (crx, id, spki) = crate::crx::tests::signed_crx_for_tests(archive);
        let verified = crate::crx::verify_crx3(&crx, &id).expect("test CRX verifies");
        assert_eq!(verified.developer_public_key(), spki.as_slice());
        (id, unpack_stamped(&verified, dest), spki)
    }

    #[test]
    fn verified_staging_omits_store_metadata_but_keeps_all_safeguards() {
        let scratch = Scratch::new();
        let archive = zip_bytes(&[
            stored("manifest.json", MANIFEST.as_bytes()),
            stored("_metadata/verified_contents.json", b"{\"signed\":true}"),
            stored("_metadata/computed_hashes.json", b"{}"),
        ]);
        let (_, result, _) = verified_unpack(&archive, &scratch.0);
        assert_eq!(result.unwrap().summary.files, 1);
        assert!(!scratch.0.join("_metadata").exists());
        assert!(crate::extensions::inspect_source(&scratch.0).is_ok());
        for unsafe_name in ["_metadata/../escape", "_metadata/CON", "_metadata/a:stream", "_other/a"] {
            let scratch = Scratch::new();
            let archive = zip_bytes(&[stored("manifest.json", MANIFEST.as_bytes()), stored(unsafe_name, b"x")]);
            let (_, result, _) = verified_unpack(&archive, &scratch.0);
            assert!(result.is_err(), "{unsafe_name}");
            assert!(fs::read_dir(&scratch.0).unwrap().next().is_none());
        }
    }

    #[test]
    fn stamping_inserts_one_top_level_key_and_changes_nothing_else() {
        let manifest = "{\n  \"manifest_version\": 3,\n  \"name\": \"Demo\",\n  \"nested\": {\"key\": \"inner\"},\n  \"version\": \"1.2.3\"\n}\n";
        let spki = sample_spki();
        let stamped = String::from_utf8(manifest_with_key(manifest.as_bytes(), &spki).unwrap()).unwrap();

        assert_eq!(stamped_key(&stamped), spki);
        let inserted = format!(",\"key\":\"{}\"", base64(&spki));
        assert_eq!(stamped.matches(&inserted).count(), 1);
        assert_eq!(stamped.replacen(&inserted, "", 1), manifest, "only the insertion may differ");
    }

    #[test]
    fn unpack_stamped_writes_the_key_of_the_crx_developer_proof() {
        let scratch = Scratch::new();
        let archive = zip_bytes(&[
            stored("manifest.json", MANIFEST.as_bytes()),
            deflated("js/background.js", b"console.log('hi');"),
        ]);
        let (id, result, spki) = verified_unpack(&archive, &scratch.0);
        let stamped = result.expect("unpack_stamped");

        assert_eq!(stamped.id, id);
        assert_eq!(stamped.summary.files, 2);
        let manifest = String::from_utf8(fs::read(scratch.0.join("manifest.json")).unwrap()).unwrap();
        let key = stamped_key(&manifest);
        assert_eq!(key, spki);
        assert_eq!(crate::crx::extension_id_from_public_key(&key).as_deref(), Some(id.as_str()));
        assert_eq!(manifest.replacen(&format!(",\"key\":\"{}\"", base64(&spki)), "", 1), MANIFEST);
        assert_eq!(fs::read(scratch.0.join("js").join("background.js")).unwrap(), b"console.log('hi');");
        assert_eq!(listing(&scratch.0), ["js/", "js/background.js", "manifest.json"]);
    }

    #[test]
    fn unpack_stamped_folder_passes_inspect_source() {
        let scratch = Scratch::new();
        let archive = zip_bytes(&[stored("manifest.json", MANIFEST.as_bytes()), stored("a.js", b"1")]);
        let (_id, result, _spki) = verified_unpack(&archive, &scratch.0);
        result.expect("unpack_stamped");
        let inspection = crate::extensions::inspect_source(&scratch.0).expect("inspect_source");
        assert_eq!(inspection.files, 2);
    }

    #[test]
    fn an_existing_top_level_key_is_replaced_by_the_developer_key() {
        let scratch = Scratch::new();
        let manifest = r#"{"key" : "ATTACKER", "name": "Demo", "nested": {"key": "inner"}}"#;
        let archive = zip_bytes(&[stored("manifest.json", manifest.as_bytes())]);
        let (_id, result, spki) = verified_unpack(&archive, &scratch.0);
        result.expect("unpack_stamped");

        let written = String::from_utf8(fs::read(scratch.0.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(
            written,
            format!(r#"{{"key" : "{}", "name": "Demo", "nested": {{"key": "inner"}}}}"#, base64(&spki))
        );
        assert!(!written.contains("ATTACKER"));
    }

    #[test]
    fn a_manifest_with_a_comment_fails_closed_and_leaves_no_files() {
        let scratch = Scratch::new();
        let manifest = "{\n  // Chrome accepts this\n  \"manifest_version\": 3\n}";
        let archive = zip_bytes(&[
            stored("manifest.json", manifest.as_bytes()),
            stored("js/a.js", b"1"),
            stored("_locales/en/messages.json", b"{}"),
        ]);
        let (_id, result, _spki) = verified_unpack(&archive, &scratch.0);
        assert_eq!(result, Err(StampError::ManifestKey(ManifestKeyError::Malformed)));
        assert_eq!(listing(&scratch.0), Vec::<String>::new(), "destination must be unchanged");
        assert!(scratch.0.is_dir(), "the caller's folder itself stays");
    }

    #[test]
    fn a_missing_or_non_file_manifest_fails_and_leaves_no_files() {
        let scratch = Scratch::new();
        let archive = zip_bytes(&[stored("js/a.js", b"1")]);
        let (_id, result, _spki) = verified_unpack(&archive, &scratch.0);
        assert_eq!(result, Err(StampError::ManifestMissing));
        assert_eq!(listing(&scratch.0), Vec::<String>::new());

        let scratch = Scratch::new();
        let archive = zip_bytes(&[stored("manifest.json/", b""), stored("js/a.js", b"1")]);
        let (_id, result, _spki) = verified_unpack(&archive, &scratch.0);
        assert_eq!(result, Err(StampError::ManifestNotAFile));
        assert_eq!(listing(&scratch.0), Vec::<String>::new());
    }

    #[test]
    fn an_oversize_manifest_is_refused_before_it_is_read_whole() {
        let scratch = Scratch::new();
        let path = scratch.0.join("manifest.json");
        fs::write(&path, vec![b' '; MAX_MANIFEST_BYTES as usize + 1]).unwrap();
        let canonical = fs::canonicalize(&scratch.0).unwrap();
        assert_eq!(stamp_manifest(&scratch.0, &canonical, &[1, 2, 3]), Err(StampError::ManifestTooLarge));
        assert_eq!(fs::metadata(&path).unwrap().len(), MAX_MANIFEST_BYTES + 1, "left as it was");
    }

    #[test]
    fn a_manifest_that_only_crosses_the_cap_once_stamped_fails_closed_and_leaves_no_files() {
        let scratch = Scratch::new();
        let head = r#"{"manifest_version":3,"name":"Demo""#;
        let tail = "}";
        let padding = " ".repeat(MAX_MANIFEST_BYTES as usize - head.len() - tail.len() - 10);
        let manifest = format!("{head}{padding}{tail}");
        assert_eq!(manifest.len() as u64, MAX_MANIFEST_BYTES - 10, "just under the cap");
        let archive = zip_bytes(&[stored("manifest.json", manifest.as_bytes()), stored("js/a.js", b"1")]);
        let (_id, result, _spki) = verified_unpack(&archive, &scratch.0);
        assert_eq!(result, Err(StampError::ManifestTooLarge));
        assert_eq!(listing(&scratch.0), Vec::<String>::new(), "destination must be unchanged");
        assert!(scratch.0.is_dir(), "the caller's folder itself stays");
    }

    #[test]
    fn unpack_stamped_leaves_a_non_empty_destination_alone() {
        let scratch = Scratch::new();
        fs::write(scratch.0.join("keep.txt"), b"mine").unwrap();
        let archive = zip_bytes(&[stored("manifest.json", MANIFEST.as_bytes())]);
        let (_id, result, _spki) = verified_unpack(&archive, &scratch.0);
        assert_eq!(result, Err(StampError::Unpack(UnpackError::DestinationNotEmpty)));
        assert_eq!(listing(&scratch.0), ["keep.txt"]);
        assert_eq!(fs::read(scratch.0.join("keep.txt")).unwrap(), b"mine");
    }
}
