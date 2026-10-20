//! Fail-closed inventory for the default-off, generic engine-owned menu experiment.
//!
//! This is deliberately separate from the permissive startup/display enumeration. It does not
//! attribute a menu command to a signer, Bowser record, or extension name. Unknown native entries
//! remain in the inventory and receive no special exemption or trust.
//!
//! The request owner must keep `MenuInventoryAuthority` alive until its final dispatch or refusal,
//! and drop it on cancellation, timeout, navigation, or window close. Native enumeration retains
//! only its exact profile and a callback, NEVER this authority or its runtime lease. The callback
//! supplied by the request owner must likewise capture only a weak/cancellable request reference.
//! A silent/late native callback therefore cannot itself retain the runtime lease forever.
//!
//! The runtime lease and snapshot equality fence cooperating Bowser operations only. They are not
//! an epoch, an ABA defense, a transaction with WebView2, or protection against external writes.
//! Native getters are bounded in count and returned string size, not in synchronous execution time.

use super::{
    CoTaskMemPWSTR, EngineCore, ICoreWebView2, ICoreWebView2BrowserExtension,
    ICoreWebView2BrowserExtensionList, ICoreWebView2Profile7, Interface,
    ProfileGetBrowserExtensionsCompletedHandler, PWSTR,
};
use crate::extensions::{Record, RecordsLease};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashSet},
    fs::OpenOptions,
    io::Read,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    rc::Rc,
};
use windows::core::{BOOL, IUnknown};

const MAX_AUTHORITY_BYTES: u64 = 1024 * 1024;
/// Allows bounded unknown native entries in addition to Bowser's at-most-64 managed records.
/// Crossing this limit refuses the whole inventory; no prefix is accepted.
pub(super) const MAX_NATIVE_EXTENSIONS: u32 = 256;
const NATIVE_ID_UNITS: usize = 32;

type InventoryResult<T> = std::result::Result<T, String>;
pub(super) type NativeInventoryDone = Box<dyn FnOnce(InventoryResult<NativeInventory>)>;

/// Complete, canonical inventory of one exact COM profile. There are no trusted/ignored rows.
#[derive(Clone)]
pub(super) struct NativeInventory {
    profile_identity: IUnknown,
    entries: BTreeMap<String, bool>,
}

impl NativeInventory {
    pub(super) fn entries(&self) -> &BTreeMap<String, bool> {
        &self.entries
    }

    /// Order-independent equality of EVERY native ID and enabled flag, and exact profile identity.
    pub(super) fn same_as(&self, other: &Self) -> bool {
        self.profile_identity.as_raw() == other.profile_identity.as_raw()
            && self.entries == other.entries
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AuthorityFiles {
    // Keep exact bytes as well as parsed meaning: even harmless textual changes invalidate a
    // pending request. This detects observed changes, not changes-and-restores between reads.
    records_text: String,
    mapping_text: String,
    records: Vec<Record>,
    mapping: BTreeMap<String, String>,
}

/// A snapshot with its REQUIRED, request-owned cross-process native-operation lease.
/// This type is intentionally not Clone and cannot be retained by `enumerate`'s implementation.
pub(super) struct MenuInventoryAuthority {
    records_file: PathBuf,
    mapping_file: PathBuf,
    profile: ICoreWebView2Profile7,
    profile_identity: IUnknown,
    files: AuthorityFiles,
    _runtime_lease: RecordsLease,
}

impl MenuInventoryAuthority {
    /// Must be called on the UI thread with the exact context-menu sender's webview.
    /// No file/path/profile/lease fallback is accepted, including an absent records file.
    pub(super) fn acquire(
        core: &Rc<RefCell<EngineCore>>,
        event_webview: &ICoreWebView2,
    ) -> InventoryResult<Self> {
        let (records_file, mapping_file, profile, profile_identity) =
            current_owner(core, event_webview)?;
        if records_file == mapping_file || records_file.parent() != mapping_file.parent() {
            return Err("The menu authority paths are inconsistent.".into());
        }
        let lease = RecordsLease::runtime(&records_file)
            .map_err(|_| "The extension runtime is busy or cannot be locked.".to_owned())?;
        let files = read_authority_files(&records_file, &mapping_file)?;
        let authority = Self {
            records_file,
            mapping_file,
            profile,
            profile_identity,
            files,
            _runtime_lease: lease,
        };
        // COM calls can reenter the UI thread. Recheck the owner after snapshot construction.
        authority.revalidate_owner_and_files(core, event_webview)?;
        Ok(authority)
    }

    /// Clone only the native profile, never the request-owned runtime lease.
    pub(super) fn profile(&self) -> ICoreWebView2Profile7 {
        self.profile.clone()
    }

    /// Revalidate the captured owner, exact configured paths and authority files. No Core or
    /// RefCell borrow is held over any COM call. The request owner still performs its final pure
    /// document/request/window checks after this method's native getters return.
    pub(super) fn current(
        &self,
        core: &Rc<RefCell<EngineCore>>,
        event_webview: &ICoreWebView2,
    ) -> InventoryResult<()> {
        self.revalidate_owner_and_files(core, event_webview)
    }

    /// The async handler retains the exact profile identity, NOT `self` or its runtime lease.
    /// The caller must use weak request ownership in `done` and ignore late/cancelled completion.
    pub(super) fn enumerate(&self, done: NativeInventoryDone) {
        enumerate_native(&self.profile, done);
    }

    /// Clone-only final fence after all native getters. No COM, file I/O, or user callback;
    /// this complements (never replaces) current()/validate_unchanged().
    pub(super) fn pure_owner_current(&self, core:&Rc<RefCell<EngineCore>>) -> bool {
        core.try_borrow().is_ok_and(|state| !state.closed && state.browser_extensions
            && state.extensions_refused.is_none()
            && state.extension_records_file.as_ref()==Some(&self.records_file)
            && state.extension_ids_file.as_ref()==Some(&self.mapping_file)
            && state.extension_profile.as_ref().is_some_and(|profile|profile.as_raw()==self.profile.as_raw()))
    }

    /// Recheck before presentation and immediately before final dispatch. This does not itself
    /// enumerate: supply the fresh complete inventory received from `enumerate`.
    pub(super) fn validate(
        &self,
        core: &Rc<RefCell<EngineCore>>,
        event_webview: &ICoreWebView2,
        inventory: &NativeInventory,
    ) -> InventoryResult<()> {
        if self.profile_identity.as_raw() != inventory.profile_identity.as_raw() {
            return Err("The native inventory belongs to a different profile.".into());
        }
        self.revalidate_owner_and_files(core, event_webview)?;
        validate_consistency(&self.files.records, &self.files.mapping, &inventory.entries)
    }

    /// Additionally require full native inventory equality with the inventory originally shown.
    pub(super) fn validate_unchanged(
        &self,
        core: &Rc<RefCell<EngineCore>>,
        event_webview: &ICoreWebView2,
        shown: &NativeInventory,
        current: &NativeInventory,
    ) -> InventoryResult<()> {
        if !shown.same_as(current) {
            return Err("The native extension inventory changed while the menu was pending.".into());
        }
        self.validate(core, event_webview, current)
    }

    fn revalidate_owner_and_files(
        &self,
        core: &Rc<RefCell<EngineCore>>,
        event_webview: &ICoreWebView2,
    ) -> InventoryResult<()> {
        let (records, mapping, _profile, identity) = current_owner(core, event_webview)?;
        if records != self.records_file
            || mapping != self.mapping_file
            || identity.as_raw() != self.profile_identity.as_raw()
        {
            return Err("The owning extension profile changed.".into());
        }
        let current = read_authority_files(&records, &mapping)?;
        if current != self.files {
            return Err("The extension authority snapshot changed while the menu was pending.".into());
        }
        Ok(())
    }
}

fn current_owner(
    core: &Rc<RefCell<EngineCore>>,
    event_webview: &ICoreWebView2,
) -> InventoryResult<(PathBuf, PathBuf, ICoreWebView2Profile7, IUnknown)> {
    let (records, mapping, profile) = {
        let state = core.try_borrow().map_err(|_| "The engine is busy.".to_owned())?;
        if state.closed || !state.browser_extensions || state.extensions_refused.is_some() {
            return Err("The extension runtime is not available for this menu.".into());
        }
        (
            state.extension_records_file.clone()
                .ok_or_else(|| "The owning extension records path is unavailable.".to_owned())?,
            state.extension_ids_file.clone()
                .ok_or_else(|| "The owning extension mapping path is unavailable.".to_owned())?,
            state.extension_profile.clone()
                .ok_or_else(|| "The native extension profile is not ready.".to_owned())?,
        )
    };
    let event_profile = unsafe {
        event_webview.cast::<super::ICoreWebView2_13>()
            .and_then(|view| view.Profile())
    }.map_err(|_| "The event's native profile is unavailable.".to_owned())?;
    let identity: IUnknown = profile.cast()
        .map_err(|_| "The owning native profile identity is unavailable.".to_owned())?;
    let event_identity: IUnknown = event_profile.cast()
        .map_err(|_| "The event's native profile identity is unavailable.".to_owned())?;
    // Canonical IUnknown identity, not names, native IDs, interface-specific pointer values, a
    // browser process ID, or an assumption that two normal-profile controllers must match.
    if identity.as_raw().is_null() || identity.as_raw() != event_identity.as_raw() {
        return Err("The context-menu event belongs to a different native profile.".into());
    }
    let mut private = BOOL::default();
    unsafe { event_profile.IsInPrivateModeEnabled(&mut private) }
        .map_err(|_| "The native profile mode is unavailable.".to_owned())?;
    if private.0 != 0 {
        return Err("Native extension menus are unavailable in this private profile.".into());
    }
    // Native getters may reenter the UI thread. A final pure Core check must observe the same
    // profile interface and configured paths after those getters, without a borrow across COM.
    {
        let state = core.try_borrow().map_err(|_| "The engine is busy.".to_owned())?;
        if state.closed || !state.browser_extensions || state.extensions_refused.is_some()
            || state.extension_records_file.as_ref() != Some(&records)
            || state.extension_ids_file.as_ref() != Some(&mapping)
            || !state.extension_profile.as_ref()
                .is_some_and(|current| current.as_raw() == profile.as_raw())
        {
            return Err("The owning extension profile changed during native validation.".into());
        }
    }
    Ok((records, mapping, profile, identity))
}

fn read_required_plain_text(path: &Path) -> InventoryResult<String> {
    // No NotFound => empty fallback. Open the reparse point itself; reject it using handle metadata.
    let file = OpenOptions::new().read(true).custom_flags(0x0020_0000).open(path)
        .map_err(|_| "A required extension authority file is unavailable.".to_owned())?;
    let meta = file.metadata()
        .map_err(|_| "Extension authority metadata is unavailable.".to_owned())?;
    if !meta.is_file() || meta.file_attributes() & 0x400 != 0 || meta.len() > MAX_AUTHORITY_BYTES {
        return Err("Extension authority is not a bounded plain file.".into());
    }
    let mut text = String::new();
    file.take(MAX_AUTHORITY_BYTES + 1).read_to_string(&mut text)
        .map_err(|_| "Extension authority could not be read as bounded UTF-8.".to_owned())?;
    if text.len() as u64 > MAX_AUTHORITY_BYTES {
        return Err("Extension authority exceeded its safe size.".into());
    }
    Ok(text)
}

fn read_authority_files(records: &Path, mapping: &Path) -> InventoryResult<AuthorityFiles> {
    let records_text = read_required_plain_text(records)?;
    let mapping_text = read_required_plain_text(mapping)?;
    let records = parse_records(&records_text)?;
    let mapping = parse_mapping(&mapping_text)?;
    validate_records_mapping(&records, &mapping)?;
    Ok(AuthorityFiles { records_text, mapping_text, records, mapping })
}

fn parse_records(text: &str) -> InventoryResult<Vec<Record>> {
    if text.len() as u64 > MAX_AUTHORITY_BYTES {
        return Err("Extension records exceeded their safe size.".into());
    }
    let mut records = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines() {
        let mut fields = line.split('\t');
        let (Some(id), Some(enabled), Some(name), Some(version), None) =
            (fields.next(), fields.next(), fields.next(), fields.next(), fields.next())
        else { return Err("Extension records are malformed.".into()); };
        if records.len() >= crate::extensions::MAX_INSTALLED
            || !crate::extensions::is_valid_id(id)
            || !seen.insert(id)
            || !matches!(enabled, "0" | "1")
            || name.is_empty()
            || name.chars().count() > 128
            || name.chars().any(|c| c.is_control() || crate::extensions::is_invisible_format(c))
            || version.is_empty()
            || version.len() > 64
            || !version.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            return Err("Extension records are invalid, duplicated, or over budget.".into());
        }
        records.push(Record {
            id: id.to_owned(), enabled: enabled == "1", name: name.to_owned(), version: version.to_owned(),
        });
    }
    Ok(records)
}

fn native_id_valid(id: &str) -> bool {
    id.len() == NATIVE_ID_UNITS && id.bytes().all(|byte| (b'a'..=b'p').contains(&byte))
}

fn parse_mapping(text: &str) -> InventoryResult<BTreeMap<String, String>> {
    if text.len() as u64 > MAX_AUTHORITY_BYTES {
        return Err("Extension mapping exceeded its safe size.".into());
    }
    let mut mapping = BTreeMap::new();
    let mut native_ids = HashSet::new();
    for line in text.lines() {
        let Some((bowser, native)) = line.split_once('\t') else {
            return Err("Extension mapping is malformed.".into());
        };
        if mapping.len() >= crate::extensions::MAX_INSTALLED
            || !crate::extensions::is_valid_id(bowser)
            || !native_id_valid(native)
            || mapping.insert(bowser.to_owned(), native.to_owned()).is_some()
            || !native_ids.insert(native)
        {
            return Err("Extension mapping is invalid, duplicated, or over budget.".into());
        }
    }
    Ok(mapping)
}

fn validate_records_mapping(records: &[Record], mapping: &BTreeMap<String, String>) -> InventoryResult<()> {
    if mapping.keys().any(|id| !records.iter().any(|record| &record.id == id)) {
        return Err("An extension mapping has no owning record.".into());
    }
    if records.iter().any(|record| record.enabled && !mapping.contains_key(&record.id)) {
        return Err("An enabled extension record has no native mapping.".into());
    }
    Ok(())
}

fn validate_consistency(
    records: &[Record],
    mapping: &BTreeMap<String, String>,
    native: &BTreeMap<String, bool>,
) -> InventoryResult<()> {
    validate_records_mapping(records, mapping)?;
    for record in records {
        if let Some(native_id) = mapping.get(&record.id)
            && native.get(native_id) != Some(&record.enabled)
        {
            return Err("The extension records, mapping, and native enabled state disagree.".into());
        }
    }
    // Do not filter unknown native rows or call them trusted. They remain part of exact equality.
    Ok(())
}

pub(super) fn enumerate_native(profile: &ICoreWebView2Profile7, done: NativeInventoryDone) {
    let identity = match profile.cast::<IUnknown>() {
        Ok(identity) if !identity.as_raw().is_null() => identity,
        _ => { done(Err("The native inventory profile identity is unavailable.".into())); return; }
    };
    let slot: Rc<RefCell<Option<NativeInventoryDone>>> = Rc::new(RefCell::new(Some(done)));
    let completion = slot.clone();
    let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |status, list| {
        let result = match (status, list) {
            (Ok(()), Some(list)) => native_entries(&list)
                .map(|entries| NativeInventory { profile_identity: identity.clone(), entries }),
            (Ok(()), None) => Err("The native inventory returned a null list.".into()),
            (Err(_), _) => Err("The native extension inventory failed.".into()),
        };
        let done = completion.borrow_mut().take();
        if let Some(done) = done { done(result); }
        Ok(())
    }));
    if unsafe { profile.GetBrowserExtensions(&handler) }.is_err() {
        let done = slot.borrow_mut().take();
        if let Some(done) = done { done(Err("The native inventory request could not start.".into())); }
    }
}

fn native_entries(list: &ICoreWebView2BrowserExtensionList) -> InventoryResult<BTreeMap<String, bool>> {
    if list.as_raw().is_null() {
        return Err("The native inventory list is null.".into());
    }
    let mut count = 0;
    unsafe { list.Count(&mut count) }
        .map_err(|_| "The native inventory count is unavailable.".to_owned())?;
    validate_native_count(count)?;
    let mut entries = BTreeMap::new();
    for index in 0..count {
        let extension = unsafe { list.GetValueAtIndex(index) }
            .map_err(|_| "A native inventory entry is unavailable.".to_owned())?;
        if extension.as_raw().is_null() {
            return Err("A native inventory entry is null.".into());
        }
        let id = native_extension_id(&extension)?;
        let mut enabled = BOOL::default();
        unsafe { extension.IsEnabled(&mut enabled) }
            .map_err(|_| "A native extension enabled state is unavailable.".to_owned())?;
        // BOOL's documented false/nonzero semantics are retained. No default flag is accepted
        // unless the getter succeeded, even for native rows not in Bowser's mapping.
        insert_native_row(&mut entries, id, enabled.as_bool())?;
    }
    let mut after = 0;
    unsafe { list.Count(&mut after) }
        .map_err(|_| "The native inventory count could not be rechecked.".to_owned())?;
    validate_native_completeness(count, after, entries.len())?;
    Ok(entries)
}

fn validate_native_count(count: u32) -> InventoryResult<()> {
    if count > MAX_NATIVE_EXTENSIONS {
        Err("The complete native inventory exceeds its safe count.".into())
    } else {
        Ok(())
    }
}

fn insert_native_row(entries: &mut BTreeMap<String, bool>, id: String, enabled: bool) -> InventoryResult<()> {
    if !native_id_valid(&id) || entries.contains_key(&id) {
        return Err("The native inventory contains invalid or duplicate IDs.".into());
    }
    entries.insert(id, enabled);
    Ok(())
}

fn validate_native_completeness(before: u32, after: u32, found: usize) -> InventoryResult<()> {
    validate_native_count(before)?;
    if after != before || found != before as usize {
        Err("The native inventory count changed or was incomplete.".into())
    } else {
        Ok(())
    }
}

fn native_extension_id(extension: &ICoreWebView2BrowserExtension) -> InventoryResult<String> {
    let mut raw = PWSTR::null();
    let result = unsafe { extension.Id(&mut raw) };
    // Own any returned allocation even on failure, before checking status or contents.
    let _allocation = CoTaskMemPWSTR::from(raw);
    result.map_err(|_| "A native extension ID is unavailable.".to_owned())?;
    let length = unsafe { super::bounded_wide_len(raw, NATIVE_ID_UNITS) }
        .ok_or_else(|| "A native extension ID is null or over budget.".to_owned())?;
    if length != NATIVE_ID_UNITS {
        return Err("A native extension ID has an invalid length.".into());
    }
    // A successful COM string getter guarantees valid storage through its terminating NUL.
    // Scan at most 33 UTF-16 units; do not call the unbounded PWSTR to_string helper.
    let units = unsafe { std::slice::from_raw_parts(raw.0, length) };
    let id = String::from_utf16(units)
        .map_err(|_| "A native extension ID is not valid UTF-16.".to_owned())?;
    if !native_id_valid(&id) {
        return Err("A native extension ID is invalid.".into());
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = "ext-0123456789abcdef";
    const NATIVE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const UNKNOWN: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn record(enabled: bool) -> Record {
        Record { id: OWNER.into(), enabled, name: "One".into(), version: "1.0".into() }
    }

    fn mapping() -> BTreeMap<String, String> {
        BTreeMap::from([(OWNER.into(), NATIVE.into())])
    }

    #[test]
    fn every_records_row_is_validated_without_truncation() {
        let row = format!("{OWNER}\t1\tOne\t1.0\n");
        assert_eq!(parse_records(&row).unwrap(), [record(true)]);
        for bad in [
            format!("{row}{row}"),
            format!("{row}bad\n"),
            format!("{OWNER}\ttrue\tOne\t1.0\n"),
            format!("{OWNER}\t1\tOne\t1a\n"),
            format!("{OWNER}\t1\t\t1.0\n"),
            format!("{OWNER}\t1\tOne\u{202e}\t1.0\n"),
            format!("{OWNER}\t1\t{}\t1.0\n", "x".repeat(129)),
        ] { assert!(parse_records(&bad).is_err(), "accepted {bad:?}"); }
        assert!(parse_records(&"x".repeat(MAX_AUTHORITY_BYTES as usize + 1)).is_err());
        let too_many: String = (0..=crate::extensions::MAX_INSTALLED)
            .map(|id| format!("ext-{id:016x}\t0\tOne\t1\n")).collect();
        assert!(parse_records(&too_many).is_err());
    }

    #[test]
    fn duplicate_invalid_or_extra_mapping_fields_refuse_the_whole_file() {
        let row = format!("{OWNER}\t{NATIVE}\n");
        assert_eq!(parse_mapping(&row).unwrap(), mapping());
        for bad in [
            format!("{row}{row}"),
            format!("{row}ext-1111111111111111\t{NATIVE}\n"),
            format!("{row}bad\n"),
            format!("{OWNER}\t{NATIVE}\textra\n"),
            format!("{OWNER}\t{}\n", "q".repeat(32)),
            format!("{OWNER}\t{}\n", "a".repeat(31)),
            format!("{OWNER}\t{}\n", "a".repeat(33)),
            format!("{OWNER}\t{}\n", "A".repeat(32)),
        ] { assert!(parse_mapping(&bad).is_err(), "accepted {bad:?}"); }
        assert!(parse_mapping(&"x".repeat(MAX_AUTHORITY_BYTES as usize + 1)).is_err());
        let too_many: String = (0..=crate::extensions::MAX_INSTALLED).map(|id| {
            let native: String = format!("{id:032x}").bytes().map(|byte| {
                let digit = if byte <= b'9' { byte - b'0' } else { byte - b'a' + 10 };
                (b'a' + digit) as char
            }).collect();
            format!("ext-{id:016x}\t{native}\n")
        }).collect();
        assert!(parse_mapping(&too_many).is_err());
    }

    #[test]
    fn records_and_mapping_must_be_consistent_before_enumeration() {
        assert!(validate_records_mapping(&[], &mapping()).is_err());
        assert!(validate_records_mapping(&[record(true)], &BTreeMap::new()).is_err());
        assert!(validate_records_mapping(&[record(false)], &BTreeMap::new()).is_ok());
        assert!(validate_records_mapping(&[record(true)], &mapping()).is_ok());
    }

    #[test]
    fn every_mapped_native_row_must_exist_and_match_the_record_enabled_flag() {
        for enabled in [false, true] {
            assert!(validate_consistency(&[record(enabled)], &mapping(), &BTreeMap::new()).is_err());
            let wrong = BTreeMap::from([(NATIVE.into(), !enabled)]);
            assert!(validate_consistency(&[record(enabled)], &mapping(), &wrong).is_err());
            let right = BTreeMap::from([(NATIVE.into(), enabled)]);
            assert!(validate_consistency(&[record(enabled)], &mapping(), &right).is_ok());
        }
    }

    #[test]
    fn native_count_and_every_id_are_bounded_complete_and_unique() {
        assert!(validate_native_count(MAX_NATIVE_EXTENSIONS).is_ok());
        assert!(validate_native_count(MAX_NATIVE_EXTENSIONS + 1).is_err());
        assert!(validate_native_completeness(0, 0, 0).is_ok());
        assert!(validate_native_completeness(2, 2, 2).is_ok());
        assert!(validate_native_completeness(2, 3, 2).is_err());
        assert!(validate_native_completeness(2, 2, 1).is_err());
        assert!(validate_native_completeness(MAX_NATIVE_EXTENSIONS + 1, MAX_NATIVE_EXTENSIONS + 1, MAX_NATIVE_EXTENSIONS as usize + 1).is_err());
        let mut rows = BTreeMap::new();
        assert!(insert_native_row(&mut rows, NATIVE.into(), true).is_ok());
        assert!(insert_native_row(&mut rows, NATIVE.into(), false).is_err());
        assert_eq!(rows.get(NATIVE), Some(&true), "duplicate input never overwrites a row");
        for bad in ["".to_owned(), "a".repeat(31), "q".repeat(32), "A".repeat(32), "a".repeat(33)] {
            assert!(insert_native_row(&mut rows, bad, true).is_err());
        }
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn unknown_native_rows_are_preserved_without_claiming_ownership() {
        let mut native = BTreeMap::from([(NATIVE.into(), true), (UNKNOWN.into(), true)]);
        assert!(validate_consistency(&[record(true)], &mapping(), &native).is_ok());
        assert_eq!(native.len(), 2);
        assert!(native.contains_key(UNKNOWN));
        let before = native.clone();
        native.insert(UNKNOWN.into(), false);
        assert_ne!(before, native, "unknown state participates in exact snapshot equality");
        assert!(validate_consistency(&[], &BTreeMap::new(), &native).is_ok());
    }

    #[test]
    fn exact_authority_snapshot_includes_record_text_and_mapping_text() {
        let first = AuthorityFiles {
            records_text: format!("{OWNER}\t1\tOne\t1.0\n"),
            mapping_text: format!("{OWNER}\t{NATIVE}\n"),
            records: vec![record(true)], mapping: mapping(),
        };
        let mut changed = first.clone();
        changed.records_text = changed.records_text.replace('\n', "\r\n");
        assert_ne!(first, changed);
        let mut changed = first.clone();
        changed.mapping_text.pop();
        assert_ne!(first, changed);
        let mut changed = first.clone();
        changed.records[0].name = "Other".into();
        assert_ne!(first, changed);
    }

    #[test]
    fn required_authority_file_never_falls_back_to_empty_when_missing() {
        let missing = std::env::temp_dir().join(format!(
            "bowser-menu-authority-missing-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
        ));
        assert!(read_required_plain_text(&missing).is_err());
    }

    #[test]
    fn callback_source_does_not_capture_authority_or_lease() {
        let source = include_str!("menu_inventory.rs");
        let body = source.split("fn enumerate_native(").nth(1).unwrap()
            .split("fn native_entries(").next().unwrap();
        assert!(!body.contains("RecordsLease"));
        assert!(!body.contains("MenuInventoryAuthority"));
        assert!(!body.contains("self"));
        assert!(body.contains("(Ok(()), None) => Err("));
    }
}
