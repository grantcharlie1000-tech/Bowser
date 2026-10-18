//! Retained signed originals for NEW Store installs. This authenticates package bytes only;
//! it grants no consent, selects no generation and implements no native update/recovery path.
use super::{ExtensionError, PreparedInstall, is_plain_dir, is_plain_file, is_valid_id, tree_fingerprint};
use std::{fs, io::{Read, Write}, os::windows::{fs::OpenOptionsExt, io::AsRawHandle}, path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}};

const ROOT: &str = "ExtensionPackages";
const RECEIPT: &str = "receipt.txt";
const ORIGINAL: &str = "original.crx";
const RECEIPT_CAP: u64 = 1024;
const HEADER: &str = "bowser-signed-artifact-v1";

fn refused() -> ExtensionError { ExtensionError::InvalidManifest("The signed package or cleanup state could not be verified. Unverifiable nonexecuting files may remain.") }
fn hex(bytes: &[u8; 32]) -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() }
fn hash(bytes: &[u8]) -> Result<[u8; 32], ExtensionError> {
    let mut hash = crate::crx::PackageSha256::new().ok_or(ExtensionError::Io)?;
    if !hash.update(bytes) { return Err(ExtensionError::Io); }
    hash.finish().ok_or(ExtensionError::Io)
}
fn digest_text(text: &str) -> bool { text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) }

fn root(extensions: &Path, create: bool) -> Result<PathBuf, ExtensionError> {
    let profile = extensions.parent().ok_or(ExtensionError::OutsideExtensions)?;
    if !is_plain_dir(profile) { return Err(ExtensionError::OutsideExtensions); }
    let root = profile.join(ROOT);
    if create {
        match fs::create_dir(&root) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err(ExtensionError::Io),
        }
    }
    if !is_plain_dir(&root) { return Err(ExtensionError::OutsideExtensions); }
    Ok(root)
}
fn plain_child(parent: &Path, child: &Path) -> Result<(), ExtensionError> {
    if !is_plain_dir(parent) || !is_plain_dir(child) { return Err(ExtensionError::OutsideExtensions); }
    let parent = fs::canonicalize(parent).map_err(|_| ExtensionError::Io)?;
    let child = fs::canonicalize(child).map_err(|_| ExtensionError::Io)?;
    if child.parent() != Some(parent.as_path()) { return Err(ExtensionError::OutsideExtensions); }
    Ok(())
}
fn read_plain(path: &Path, cap: u64) -> Result<Vec<u8>, ExtensionError> {
    let file = fs::OpenOptions::new().read(true).custom_flags(super::FILE_FLAG_OPEN_REPARSE_POINT).open(path).map_err(|_| refused())?;
    let meta = file.metadata().map_err(|_| refused())?;
    if !is_plain_file(&meta) || meta.len() > cap { return Err(refused()); }
    let mut bytes = Vec::new(); file.take(cap + 1).read_to_end(&mut bytes).map_err(|_| refused())?;
    if bytes.len() as u64 != meta.len() || bytes.len() as u64 > cap { return Err(refused()); }
    Ok(bytes)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), ExtensionError> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).custom_flags(super::FILE_FLAG_OPEN_REPARSE_POINT).open(path).map_err(|_| ExtensionError::Io)?;
    if !is_plain_file(&file.metadata().map_err(|_| ExtensionError::Io)?) { return Err(refused()); }
    file.write_all(bytes).and_then(|()| file.sync_all()).map_err(|_| ExtensionError::Io)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileId(u32, u64);
struct Guard { file: fs::File, id: FileId }
fn guarded(path: &Path, directory: bool) -> Result<Guard, ExtensionError> {
    use windows::Win32::{Foundation::HANDLE, Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle}};
    let file = fs::OpenOptions::new().read(true).access_mode(0x8000_0000 | 0x0001_0000)
        .share_mode(if directory { 3 } else { 1 })
        .custom_flags(super::FILE_FLAG_OPEN_REPARSE_POINT | 0x0200_0000).open(path).map_err(|_| refused())?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }.map_err(|_| refused())?;
    if info.dwFileAttributes & super::FILE_ATTRIBUTE_REPARSE_POINT != 0 || (info.dwFileAttributes & 0x10 != 0) != directory { return Err(refused()); }
    Ok(Guard { file, id: FileId(info.dwVolumeSerialNumber, (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow)) })
}
fn delete_guarded(guard: &Guard) -> Result<(), ExtensionError> {
    use windows::Win32::{Foundation::HANDLE, Storage::FileSystem::{FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle}};
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    unsafe { SetFileInformationByHandle(HANDLE(guard.file.as_raw_handle()), FileDispositionInfo, (&info as *const FILE_DISPOSITION_INFO).cast(), std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32) }.map_err(|_| refused())
}
fn guarded_bytes(guard: &Guard, cap: u64) -> Result<Vec<u8>, ExtensionError> {
    let meta = guard.file.metadata().map_err(|_| refused())?;
    if !is_plain_file(&meta) || meta.len() > cap { return Err(refused()); }
    let mut bytes = Vec::new(); (&guard.file).take(cap + 1).read_to_end(&mut bytes).map_err(|_| refused())?;
    if bytes.len() as u64 != meta.len() { return Err(refused()); } Ok(bytes)
}
fn exact_shape(path: &Path, names: &[&str]) -> Result<(), ExtensionError> {
    let found = fs::read_dir(path).map_err(|_| refused())?.take(names.len() + 1).collect::<Result<Vec<_>, _>>().map_err(|_| refused())?;
    if found.len() != names.len() || !names.iter().all(|name| found.iter().any(|entry| entry.file_name() == *name)) { return Err(refused()); }
    Ok(())
}
fn remove_verified_tree(path: &Path, fingerprint: &[u8; 32]) -> Result<(), ExtensionError> {
    let mut directories = vec![guarded(path, true)?]; let mut files = Vec::new(); let mut entries = Vec::new();
    super::walk_with_limits(path, super::LIMITS, &mut |relative, kind| { entries.push((relative.to_owned(), kind)); Ok(()) })?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (relative, kind) in entries {
        match kind {
            super::EntryKind::Folder => directories.push(guarded(&path.join(relative), true)?),
            super::EntryKind::File => files.push(guarded(&path.join(relative), false)?),
        }
    }
    // Every known entry is held against modification/replacement while the existing complete
    // fingerprint implementation authenticates it. New unknown entries are never enumerated
    // for deletion and prevent the final empty-directory deletion instead.
    if tree_fingerprint(path)? != *fingerprint { return Err(refused()); }
    for file in files { delete_guarded(&file)?; drop(file); }
    for directory in directories.into_iter().rev() { delete_guarded(&directory)?; drop(directory); }
    Ok(())
}

fn remove_authenticated(root: &Path, path: &Path, generation: &str, expected: &str, package_hash: &[u8; 32], identities: Option<(FileId, FileId)>) -> Result<(), ExtensionError> {
    plain_child(root, path)?; let owner = guarded(path, true)?;
    let generation_path = path.join(generation); plain_child(path, &generation_path)?; let child = guarded(&generation_path, true)?;
    if identities.is_some_and(|(a, b)| a != owner.id || b != child.id) { return Err(refused()); }
    exact_shape(path, &[generation])?; exact_shape(&generation_path, &[ORIGINAL, RECEIPT])?;
    let original = guarded(&generation_path.join(ORIGINAL), false)?;
    let receipt = guarded(&generation_path.join(RECEIPT), false)?;
    if guarded_bytes(&receipt, RECEIPT_CAP)? != expected.as_bytes() || hash(&guarded_bytes(&original, crate::crx_fetch::MAX_DOWNLOAD_BYTES)?)? != *package_hash { return Err(refused()); }
    // Delete these authenticated entries by their already-validated handles. Never recursively
    // delete a directory: a newly added unknown child makes the empty-directory deletion fail.
    delete_guarded(&original)?; delete_guarded(&receipt)?; drop(original); drop(receipt);
    delete_guarded(&child)?; drop(child); delete_guarded(&owner)?; Ok(())
}

// Fixed Rust-only filesystem failure boundary. Not compiled into the application, exposed to a
// page, or a substitute for a native COM result. Tests still hit the real record writer/error.
#[cfg(test)]
type PublishProbe = Box<dyn FnOnce(&Path)>;
#[cfg(test)]
thread_local! { static AFTER_PUBLISH: std::cell::RefCell<Option<PublishProbe>> = const { std::cell::RefCell::new(None) }; }

/// Private capability for a directory created by this preparation. Drop never removes a
/// preexisting final record directory after a failed publish, and nothing here loads code.
#[derive(Debug)]
pub(super) struct Stage {
    root: PathBuf, path: PathBuf, record: String, generation: String,
    receipt: String, package_hash: [u8; 32], keep: bool, published: bool,
    identity: FileId, generation_identity: Option<FileId>, complete: bool,
}
impl Stage {
    pub(super) fn prepare(prepared: &PreparedInstall, native_id: &str, bytes: &[u8]) -> Result<Self, ExtensionError> {
        if bytes.len() as u64 > crate::crx_fetch::MAX_DOWNLOAD_BYTES || !is_valid_id(&prepared.id) { return Err(refused()); }
        if tree_fingerprint(&prepared.folder()?)? != prepared.fingerprint { return Err(refused()); }
        let verified = crate::crx::verify_crx3(bytes, native_id).map_err(|_| refused())?;
        let package_hash = hash(bytes)?; let generation = hex(&package_hash);
        let root = root(&prepared.extensions_dir, true)?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| ExtensionError::Io)?.as_nanos();
        let path = root.join(format!("pending-{}-{nonce:x}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&path).map_err(|_| ExtensionError::Io)?;
        let receipt = format!("{HEADER}\nrecord={}\nsigning-id={native_id}\nversion={}\npackage-sha256={generation}\ntree-sha256={}\n", prepared.id, prepared.inspection.manifest.version, hex(&prepared.fingerprint));
        let identity = guarded(&path, true)?.id;
        let mut stage = Self { root, path, record: prepared.id.clone(), generation, receipt, package_hash, keep: false, published: false, identity, generation_identity: None, complete: false };
        let tree = stage.path.join("verification-tree"); fs::create_dir(&tree).map_err(|_| ExtensionError::Io)?;
        crate::crx_unpack::unpack_stamped(&verified, &tree).map_err(|_| refused())?;
        // The only nonsigned file is this exact app-owned source marker. An archive cannot
        // supply it, and every other copied/stamped byte must match the authenticated replay.
        write_new(&tree.join(crate::store::SOURCE_MARKER), native_id.as_bytes())?;
        if tree_fingerprint(&tree)? != prepared.fingerprint { return Err(refused()); }
        plain_child(&stage.path, &tree)?; remove_verified_tree(&tree, &prepared.fingerprint)?;
        let generation_dir = stage.path.join(&stage.generation); fs::create_dir(&generation_dir).map_err(|_| ExtensionError::Io)?;
        stage.generation_identity = Some(guarded(&generation_dir, true)?.id);
        write_new(&generation_dir.join(ORIGINAL), bytes)?;
        write_new(&generation_dir.join(RECEIPT), stage.receipt.as_bytes())?;
        stage.validate()?; stage.complete = true; Ok(stage)
    }
    fn validate(&self) -> Result<(), ExtensionError> {
        plain_child(&self.root, &self.path)?;
        if guarded(&self.path, true)?.id != self.identity { return Err(refused()); }
        exact_shape(&self.path, &[self.generation.as_str()])?;
        let generation = self.path.join(&self.generation); plain_child(&self.path, &generation)?;
        if Some(guarded(&generation, true)?.id) != self.generation_identity { return Err(refused()); }
        exact_shape(&generation, &[ORIGINAL, RECEIPT])?;
        if read_plain(&generation.join(RECEIPT), RECEIPT_CAP)? != self.receipt.as_bytes()
            || hash(&read_plain(&generation.join(ORIGINAL), crate::crx_fetch::MAX_DOWNLOAD_BYTES)?)? != self.package_hash { return Err(refused()); }
        Ok(())
    }
    pub(super) fn publish(&mut self) -> Result<(), ExtensionError> {
        self.validate()?;
        if self.published { return Err(refused()); }
        let target = self.root.join(&self.record);
        match fs::symlink_metadata(&target) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            _ => return Err(refused()),
        }
        fs::rename(&self.path, &target).map_err(|_| ExtensionError::Io)?;
        self.path = target;
        if guarded(&self.path, true)?.id != self.identity { return Err(refused()); }
        self.published = true;
        #[cfg(test)]
        if let Some(probe) = AFTER_PUBLISH.with(|slot| slot.borrow_mut().take()) { probe(&self.path); }
        Ok(())
    }
    pub(super) fn accept(&mut self) { self.keep = true; }
}
impl Drop for Stage {
    fn drop(&mut self) {
        if !self.keep && self.complete {
            if let Some(child) = self.generation_identity {
                let _ = remove_authenticated(&self.root, &self.path, &self.generation, &self.receipt, &self.package_hash, Some((self.identity, child)));
            }
        }
        // An incomplete, changed or replaced stage is preserved outside the runtime's loaded
        // folders. No record is created and no claim of complete erasure is made on uncertainty.
    }
}

/// Removal cleans only the exact single-generation shape created above. Unknown files, invalid
/// receipts or unverifiable originals are preserved and reported as a partial cleanup failure.
pub(super) fn remove_for_record(extensions: &Path, record: &str) -> Result<(), ExtensionError> {
    if !is_valid_id(record) { return Err(refused()); }
    let profile = extensions.parent().ok_or(ExtensionError::OutsideExtensions)?;
    let unchecked_root = profile.join(ROOT);
    match fs::symlink_metadata(&unchecked_root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        _ => (),
    }
    let root = root(extensions, false)?; let folder = root.join(record);
    match fs::symlink_metadata(&folder) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        _ => (),
    }
    plain_child(&root, &folder)?;
    let entries: Vec<_> = fs::read_dir(&folder).map_err(|_| refused())?.take(2).collect::<Result<_, _>>().map_err(|_| refused())?;
    if entries.len() != 1 { return Err(refused()); }
    let generation = entries[0].file_name().to_str().filter(|name| digest_text(name)).ok_or_else(refused)?.to_owned();
    let path = folder.join(&generation); plain_child(&folder, &path)?;
    let names: Vec<_> = fs::read_dir(&path).map_err(|_| refused())?.take(3).collect::<Result<Vec<_>, _>>().map_err(|_| refused())?.into_iter().map(|entry| entry.file_name()).collect();
    if names.len() != 2 || !names.iter().any(|name| name == ORIGINAL) || !names.iter().any(|name| name == RECEIPT) { return Err(refused()); }
    let text = String::from_utf8(read_plain(&path.join(RECEIPT), RECEIPT_CAP)?).map_err(|_| refused())?;
    let lines: Vec<_> = text.lines().collect();
    if lines.len() != 6 || lines[0] != HEADER || lines[1] != format!("record={record}") || lines[4] != format!("package-sha256={generation}") || !text.ends_with('\n') || text.contains('\r') { return Err(refused()); }
    let native = lines[2].strip_prefix("signing-id=").filter(|id| crate::crx_fetch::parse_extension_id(id).is_ok()).ok_or_else(refused)?;
    if !lines[3].strip_prefix("version=").is_some_and(super::is_valid_version)
        || !lines[5].strip_prefix("tree-sha256=").is_some_and(digest_text) { return Err(refused()); }
    let bytes = read_plain(&path.join(ORIGINAL), crate::crx_fetch::MAX_DOWNLOAD_BYTES)?;
    if hex(&hash(&bytes)?) != generation || crate::crx::verify_crx3(&bytes, native).is_err() { return Err(refused()); }
    remove_authenticated(&root, &folder, &generation, &text, &hash(&bytes)?, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    struct Fixture { root: PathBuf, extensions: PathBuf, records: PathBuf, bytes: Vec<u8>, native: String, prepared: Option<PreparedInstall> }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!("bowser-original-artifact-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&root).unwrap(); let profile = root.join("profile"); fs::create_dir(&profile).unwrap();
            let source = root.join("source"); fs::create_dir(&source).unwrap();
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).unix_permissions(0o644);
            for (name, text) in [("manifest.json", r#"{"manifest_version":3,"name":"Signed fixture","version":"1.0","permissions":["storage"]}"#), ("background.js", "console.log(1);"), ("assets/nested/probe.txt", "genuine nested replay cleanup")] {
                zip.start_file(name, options).unwrap(); zip.write_all(text.as_bytes()).unwrap();
            }
            let (bytes, native, _) = crate::crx::tests::signed_crx_for_tests(&zip.finish().unwrap().into_inner());
            crate::crx_fetch::verify_and_unpack(&bytes, &native, &source).unwrap();
            fs::write(source.join(crate::store::SOURCE_MARKER), &native).unwrap();
            let extensions = profile.join("Extensions"); let records = profile.join("extensions.txt");
            let prepared = Some(super::super::prepare_install(&source, &extensions, &records).unwrap());
            Self { root, extensions, records, bytes, native, prepared }
        }
        fn attach(&mut self) { let prepared = self.prepared.take().unwrap(); self.prepared = Some(prepared.retain_verified_store_original(&self.native, &self.bytes).unwrap()); }
        fn artifact(&self) -> &Stage { self.prepared.as_ref().unwrap().retained_original.as_ref().unwrap() }
        fn original(&self) -> PathBuf { let stage = self.artifact(); stage.path.join(&stage.generation).join(ORIGINAL) }
    }
    impl Drop for Fixture {
        fn drop(&mut self) { drop(self.prepared.take()); let _ = fs::remove_dir_all(&self.root); }
    }

    #[test]
    fn signed_original_cancel_removes_unconsented_copy_and_artifact_without_record() {
        let mut fixture = Fixture::new(); fixture.attach();
        let package = fixture.prepared.as_ref().unwrap().folder().unwrap(); let pending = fixture.artifact().path.clone();
        assert!(fixture.original().exists()); assert!(!fixture.records.exists());
        drop(fixture.prepared.take());
        assert!(!package.exists()); assert!(!pending.exists()); assert!(!fixture.records.exists());
    }

    #[test]
    fn signed_original_commit_retains_exact_reverified_package_outside_loaded_tree_and_remove_cleans_it() {
        let mut fixture = Fixture::new(); fixture.attach();
        let generation = fixture.artifact().generation.clone(); let package = fixture.prepared.as_ref().unwrap().folder().unwrap();
        let record = fixture.prepared.take().unwrap().commit().unwrap();
        let retained = root(&fixture.extensions, false).unwrap().join(&record.id).join(generation);
        assert!(!retained.starts_with(&package));
        assert_eq!(fs::read(package.join("assets/nested/probe.txt")).unwrap(), b"genuine nested replay cleanup");
        let raw = read_plain(&retained.join(ORIGINAL), crate::crx_fetch::MAX_DOWNLOAD_BYTES).unwrap();
        assert_eq!(raw, fixture.bytes); crate::crx::verify_crx3(&raw, &fixture.native).unwrap();
        let receipt = fs::read_to_string(retained.join(RECEIPT)).unwrap();
        assert!(receipt.contains(&format!("tree-sha256={}", hex(&tree_fingerprint(&package).unwrap()))));
        super::super::uninstall(&fixture.extensions, &fixture.records, &record.id).unwrap();
        assert!(!retained.exists()); assert!(!package.exists());
        assert!(super::super::read_authority_records(&fixture.records).unwrap().is_empty());
    }

    #[test]
    fn signed_original_same_length_raw_tampering_fails_commit_and_preserves_uncertain_residue() {
        let mut fixture = Fixture::new(); fixture.attach(); let original = fixture.original();
        let pending = fixture.artifact().path.clone(); let mut bytes = fs::read(&original).unwrap(); bytes[0] ^= 1; fs::write(&original, bytes).unwrap();
        assert!(fixture.prepared.take().unwrap().commit().is_err());
        assert!(!fixture.records.exists()); assert!(pending.exists(), "changed bytes are preserved as unconsented nonexecuting residue");
    }

    #[test]
    fn signed_original_receipt_tampering_fails_consent_commit() {
        let mut fixture = Fixture::new(); fixture.attach(); let receipt = fixture.original().with_file_name(RECEIPT);
        fs::write(receipt, b"bowser-signed-artifact-v1\nrecord=ext-0000000000000000\n").unwrap();
        assert!(fixture.prepared.take().unwrap().commit().is_err()); assert!(!fixture.records.exists());
    }

    #[test]
    fn signed_original_replay_rejects_changed_copy_even_with_valid_manifest_key() {
        let mut fixture = Fixture::new(); let prepared = fixture.prepared.take().unwrap();
        fs::write(prepared.folder().unwrap().join("background.js"), "console.log(2);").unwrap();
        assert!(prepared.retain_verified_store_original(&fixture.native, &fixture.bytes).is_err());
        assert!(!fixture.records.exists());
    }

    #[test]
    fn signed_original_publish_collision_preserves_preexisting_data_and_never_records_install() {
        let mut fixture = Fixture::new(); fixture.attach(); let record = fixture.artifact().record.clone();
        let collision = root(&fixture.extensions, false).unwrap().join(record); fs::create_dir(&collision).unwrap(); fs::write(collision.join("keep"), b"preexisting").unwrap();
        assert!(fixture.prepared.take().unwrap().commit().is_err());
        assert!(!fixture.records.exists()); assert_eq!(fs::read(collision.join("keep")).unwrap(), b"preexisting");
    }

    #[test]
    fn signed_original_record_save_failure_cleans_published_uncommitted_artifact_and_preserves_temp() {
        let mut fixture = Fixture::new(); fixture.attach(); let record = fixture.artifact().record.clone();
        let temporary = PathBuf::from(format!("{}.tmp-{}", fixture.records.display(), std::process::id())); fs::write(&temporary, b"preexisting").unwrap();
        assert!(fixture.prepared.take().unwrap().commit().is_err());
        assert!(!root(&fixture.extensions, false).unwrap().join(record).exists()); assert!(!fixture.records.exists());
        assert_eq!(fs::read(temporary).unwrap(), b"preexisting");
    }

    #[test]
    fn signed_original_cleanup_preserves_unknown_artifact_files_as_partial_failure() {
        let mut fixture = Fixture::new(); fixture.attach(); let record = fixture.prepared.take().unwrap().commit().unwrap();
        let retained = root(&fixture.extensions, false).unwrap().join(&record.id); fs::write(retained.join("keep"), b"unknown data").unwrap();
        assert!(super::super::uninstall(&fixture.extensions, &fixture.records, &record.id).is_err());
        assert!(super::super::read_authority_records(&fixture.records).unwrap().is_empty());
        assert_eq!(fs::read(retained.join("keep")).unwrap(), b"unknown data");
    }
    #[test]
    fn signed_original_invalid_signature_or_wrong_identity_never_creates_retention() {
        let mut fixture = Fixture::new(); let prepared = fixture.prepared.take().unwrap();
        let mut corrupt = fixture.bytes.clone(); let last = corrupt.len() - 1; corrupt[last] ^= 1;
        assert!(prepared.retain_verified_store_original(&fixture.native, &corrupt).is_err());
        assert!(!fixture.extensions.parent().unwrap().join(ROOT).exists()); assert!(!fixture.records.exists());
        let mut fixture = Fixture::new(); let prepared = fixture.prepared.take().unwrap();
        assert!(prepared.retain_verified_store_original("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", &fixture.bytes).is_err());
        assert!(!fixture.extensions.parent().unwrap().join(ROOT).exists()); assert!(!fixture.records.exists());
    }

    #[test]
    fn signed_original_retention_refuses_junction_root_without_touching_target() {
        let mut fixture = Fixture::new(); let outside = fixture.root.join("outside"); fs::create_dir(&outside).unwrap(); fs::write(outside.join("keep"), b"untouched").unwrap();
        let link = fixture.extensions.parent().unwrap().join(ROOT);
        let result = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&link).arg(&outside).output().unwrap();
        assert!(result.status.success());
        let prepared = fixture.prepared.take().unwrap(); assert!(prepared.retain_verified_store_original(&fixture.native, &fixture.bytes).is_err());
        assert_eq!(fs::read(outside.join("keep")).unwrap(), b"untouched"); assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
        fs::remove_dir(link).unwrap(); assert!(!fixture.records.exists());
    }

    #[test]
    fn signed_original_replay_rejects_self_consistent_changed_source_with_valid_key() {
        let mut fixture = Fixture::new(); drop(fixture.prepared.take());
        let source = fixture.root.join("source"); fs::write(source.join("background.js"), "console.log(2);").unwrap();
        let prepared = super::super::prepare_install(&source, &fixture.extensions, &fixture.records).unwrap();
        assert_eq!(tree_fingerprint(&prepared.folder().unwrap()).unwrap(), prepared.fingerprint);
        assert_eq!(super::super::manifest_key_id(&prepared.folder().unwrap()).unwrap().as_deref(), Some(fixture.native.as_str()));
        assert!(prepared.retain_verified_store_original(&fixture.native, &fixture.bytes).is_err());
        assert!(!fixture.records.exists());
    }

    #[test]
    fn signed_original_pending_unknown_file_survives_cancel_and_failed_commit() {
        for cancel in [true, false] {
            let mut fixture = Fixture::new(); fixture.attach(); let pending = fixture.artifact().path.clone();
            fs::write(pending.join("keep"), b"unknown data").unwrap();
            if cancel { drop(fixture.prepared.take()); } else { assert!(fixture.prepared.take().unwrap().commit().is_err()); }
            assert_eq!(fs::read(pending.join("keep")).unwrap(), b"unknown data"); assert!(!fixture.records.exists());
        }
    }

    #[test]
    fn signed_original_replaced_pending_directory_survives_cancel_and_failed_commit() {
        for cancel in [true, false] {
            let mut fixture = Fixture::new(); fixture.attach(); let pending = fixture.artifact().path.clone(); let moved = pending.with_extension("original-owned");
            fs::rename(&pending, &moved).unwrap(); fs::create_dir(&pending).unwrap(); fs::write(pending.join("keep"), b"replacement data").unwrap();
            if cancel { drop(fixture.prepared.take()); } else { assert!(fixture.prepared.take().unwrap().commit().is_err()); }
            assert_eq!(fs::read(pending.join("keep")).unwrap(), b"replacement data"); assert!(moved.exists()); assert!(!fixture.records.exists());
        }
    }

    #[test]
    fn signed_original_unknown_file_survives_after_actual_record_write_failure() {
        let mut fixture = Fixture::new(); fixture.attach(); let record = fixture.artifact().record.clone();
        let final_path = root(&fixture.extensions, false).unwrap().join(record);
        let temporary = PathBuf::from(format!("{}.tmp-{}", fixture.records.display(), std::process::id()));
        let fault = temporary.clone();
        AFTER_PUBLISH.with(|slot| *slot.borrow_mut() = Some(Box::new(move |published| {
            fs::write(published.join("keep"), b"unknown data").unwrap(); fs::write(fault, b"preexisting").unwrap();
        })));
        assert!(fixture.prepared.take().unwrap().commit().is_err());
        assert_eq!(fs::read(final_path.join("keep")).unwrap(), b"unknown data"); assert_eq!(fs::read(temporary).unwrap(), b"preexisting"); assert!(!fixture.records.exists());
    }

    #[test]
    fn signed_original_replaced_directory_survives_after_actual_record_write_failure() {
        let mut fixture = Fixture::new(); fixture.attach(); let record = fixture.artifact().record.clone();
        let final_path = root(&fixture.extensions, false).unwrap().join(record); let saved = final_path.with_extension("saved-owned");
        let temporary = PathBuf::from(format!("{}.tmp-{}", fixture.records.display(), std::process::id()));
        let fault = temporary.clone(); let saved_probe = saved.clone();
        AFTER_PUBLISH.with(|slot| *slot.borrow_mut() = Some(Box::new(move |published| {
            fs::rename(published, &saved_probe).unwrap(); fs::create_dir(published).unwrap();
            fs::write(published.join("keep"), b"replacement data").unwrap(); fs::write(fault, b"preexisting").unwrap();
        })));
        assert!(fixture.prepared.take().unwrap().commit().is_err());
        assert_eq!(fs::read(final_path.join("keep")).unwrap(), b"replacement data"); assert!(saved.exists());
        assert_eq!(fs::read(temporary).unwrap(), b"preexisting"); assert!(!fixture.records.exists());
    }

}
