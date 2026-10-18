//! Bowser's small, curated catalogue and verified package preparation. The catalogue is not a
//! Google search API. No website or catalogue metadata can grant installation consent.
use std::{fs, path::{Path, PathBuf}, sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering}};
use crate::{crx_fetch, extensions::{self, PreparedInstall, Manifest}};

pub const SOURCE_MARKER: &str = "bowser-store-source.txt";
pub const BROWSE_URL: &str = "https://chromewebstore.google.com/category/extensions";

pub struct CatalogueEntry {
    pub id: &'static str, pub name: &'static str, pub description: &'static str,
    pub publisher: &'static str, pub category: &'static str, pub source: &'static str,
    pub homepage: &'static str, pub checked: &'static str,
}

/// Manually checked primary listings, not scraped ratings, user counts or an invented public API.
/// See docs/store-catalogue.md for the refresh/provenance process and exact evidence policy.
pub const CATALOGUE: &[CatalogueEntry] = &[
    CatalogueEntry { id: "ddkjiahejlhfcafbddmgiahcphecmpfh", name: "uBlock Origin Lite", description: "A declarative MV3 content blocker from the uBlock Origin project.", publisher: "Raymond Hill (gorhill)", category: "Privacy & security", source: "https://chromewebstore.google.com/detail/ublock-origin-lite/ddkjiahejlhfcafbddmgiahcphecmpfh", homepage: "https://github.com/uBlockOrigin/uBOL-home", checked: "2026-10-18" },
    CatalogueEntry { id: "nngceckbapebfimnlniiiahkandclblb", name: "Bitwarden Password Manager", description: "An open-source password manager. Bowser compatibility must be checked before trusting it with real accounts.", publisher: "Bitwarden Inc.", category: "Privacy & security", source: "https://chromewebstore.google.com/detail/bitwarden-password-manage/nngceckbapebfimnlniiiahkandclblb", homepage: "https://bitwarden.com/", checked: "2026-10-18" },
    CatalogueEntry { id: "clngdbkpkpeebahjckkjfobafhncgmne", name: "Stylus", description: "An open-source user-style manager for changing website appearance with CSS.", publisher: "stylus.openstyles", category: "Appearance", source: "https://chromewebstore.google.com/detail/stylus/clngdbkpkpeebahjckkjfobafhncgmne", homepage: "https://github.com/openstyles/stylus", checked: "2026-10-18" },
    CatalogueEntry { id: "iaiomicjabeggjcfkbimgmglanimpnae", name: "Tab Session Manager", description: "Save and restore window and tab sessions. Tab-management APIs are not yet verified in Bowser.", publisher: "Sienori", category: "Productivity", source: "https://chromewebstore.google.com/detail/tab-session-manager/iaiomicjabeggjcfkbimgmglanimpnae", homepage: "https://github.com/sienori/Tab-Session-Manager", checked: "2026-10-18" },
];

pub fn entry(id: &str) -> Option<&'static CatalogueEntry> { CATALOGUE.iter().find(|entry| entry.id == id) }

/// Historical, exact-version evidence, never a promise about a newly resolved package/runtime.
pub fn catalogue_evidence(id: &str) -> (&'static str, &'static str) {
    match id {
        "ddkjiahejlhfcafbddmgiahcphecmpfh" => ("partial", "Tested version 2026.1012.1545 on WebView2 154.0.4258.62: one real default ad-domain request was blocked with a successful zero-extension/control comparison. Popup action/active-tab targeting remains unsupported. Other package/runtime versions are untested."),
        "clngdbkpkpeebahjckkjfobafhncgmne" => ("partial", "Tested version 2.4.14 on WebView2 154.0.4258.62: genuine manager imported a local style; exact-URL CSS effect, unmatched-URL exclusion and removal worked. Manager access required diagnostic own-page navigation; the imported style also survived a new executable process. Normal Manage routing remains unverified. Other versions are untested."),
        "nngceckbapebfimnlniiiahkandclblb" => ("untested", "Version 2026.10.0 loaded on WebView2 154.0.4258.62, but dummy-vault autofill is untested. The hosted popup's current-window query targets its own page, not the active Bowser browsing tab. Do not trust real credentials to unverified functionality."),
        "iaiomicjabeggjcfkbimgmglanimpnae" => ("untested", "Version 7.4.0 loaded on WebView2 154.0.4258.62. Native window IDs group individual WebViews, not Bowser's browser window. A real Save stored one Bowser window/two tabs as two native windows/two tabs. Correct grouping and restore are unverified; restoration may close native windows. Test only disposable sessions."),
        _ => ("untested", "No exact-version functionality evidence is available. Installation alone does not prove compatibility."),
    }
}


pub fn valid_request_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

pub fn request_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("store-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

const MAX_DOWNLOAD_WORKERS: usize = 2;
static DOWNLOAD_WORKERS: AtomicUsize = AtomicUsize::new(0);
pub(crate) struct DownloadPermit;
impl Drop for DownloadPermit {
    fn drop(&mut self) { DOWNLOAD_WORKERS.fetch_sub(1, Ordering::AcqRel); }
}
/// Cancelling is cooperative, so a new click must not create unlimited blocked WinHTTP threads.
pub(crate) fn acquire_download_permit() -> Option<DownloadPermit> {
    let mut count = DOWNLOAD_WORKERS.load(Ordering::Acquire);
    loop {
        if count >= MAX_DOWNLOAD_WORKERS { return None; }
        match DOWNLOAD_WORKERS.compare_exchange_weak(count, count + 1, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return Some(DownloadPermit),
            Err(observed) => count = observed,
        }
    }
}

struct Staging(PathBuf);
impl Staging {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("bowser-{}", request_id()));
        fs::create_dir(&path).map_err(|_| "A fresh download staging folder could not be created.".to_owned())?;
        Ok(Self(path))
    }
}
impl Drop for Staging { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

pub struct Review {
    pub prepared: PreparedInstall,
    pub name: String,
    pub store_id: String,
    extensions_dir: PathBuf,
    records_file: PathBuf,
}

impl Review {
    /// Revalidate identity ownership at consent, not only when the download began. Two windows
    /// may have prepared the same Store identity before either one was recorded.
    pub fn commit(self) -> Result<extensions::Record, String> {
        ensure_not_installed(&self.store_id, &self.extensions_dir, &self.records_file)?;
        if installed_source(&self.prepared.folder().map_err(|error| error.to_string())?).as_deref() != Some(self.store_id.as_str()) {
            return Err("The staged package source identity changed. Nothing was installed.".into());
        }
        self.prepared.commit_checked_named(&self.name, |records| {
            if identity_already_present(&self.store_id, &self.extensions_dir, &self.records_file, records) {
                return Err(extensions::ExtensionError::InvalidManifest("This extension identity was installed by another window during review. Nothing else was installed."));
            }
            Ok(())
        }).map_err(|error| error.to_string())
    }
}

fn identity_already_present(id: &str, extensions_dir: &Path, records_file: &Path, records: &[extensions::Record]) -> bool {
    // A legacy local CRX/unpacked installation can have the same native key without a Store
    // marker. Check its key and the existing native mapping, not catalogue/source claims alone.
    let mapping_path = records_file.with_file_name("extension_ids.txt");
    let mapping = fs::symlink_metadata(&mapping_path).ok().filter(|meta| meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 64 * 1024)
        .and_then(|_| fs::read_to_string(mapping_path).ok()).unwrap_or_default();
    records.iter().any(|record| {
        let native_match = mapping.lines().any(|line| line.split_once('\t').is_some_and(|(bowser, native)| bowser == record.id && native == id));
        native_match || extensions::folder_for(extensions_dir, &record.id).is_ok_and(|folder| {
            extensions::manifest_key_id(&folder).ok().flatten().or_else(|| installed_source(&folder)).as_deref() == Some(id)
        })
    })
}

fn ensure_not_installed(id: &str, extensions_dir: &Path, records_file: &Path) -> Result<(), String> {
    let records = extensions::read_records(records_file).map_err(|error| error.to_string())?;
    if identity_already_present(id, extensions_dir, records_file, &records) {
        return Err("This extension identity is already installed in this profile. Verified extension updates are not enabled yet.".to_owned());
    }
    Ok(())
}

/// Downloads and verifies before touching a profile, then prepares its immutable, unrecorded copy.
/// Cancellation can never commit or load. WinHTTP's finite timeouts bound a request already in flight.
pub fn prepare(
    id: &str, runtime: &str, extensions_dir: &Path, records_file: &Path, cancelled: &AtomicBool,
) -> Result<Review, String> {
    crx_fetch::parse_extension_id(id).map_err(|error| error.to_string())?;
    let check_cancelled = || if cancelled.load(Ordering::Acquire) { Err("Download cancelled. Nothing was installed.".to_owned()) } else { Ok(()) };
    check_cancelled()?;
    // Also checked again at consent, because another window may install during download/review.
    ensure_not_installed(id, extensions_dir, records_file)?;
    let staging = Staging::new()?;
    let original = crx_fetch::download_crx_cancellable(id, runtime, cancelled).map_err(|error| format!("Package download failed: {error}. This is not a runtime compatibility verdict."))?;
    check_cancelled()?;
    crx_fetch::verify_and_unpack(&original, id, &staging.0).map_err(|error| format!("Package verification failed: {error}. This is not a runtime compatibility verdict."))?;
    check_cancelled()?;
    let inspection = extensions::inspect_source(&staging.0).map_err(|error| error.to_string())?;
    let name = resolved_name(&staging.0, &inspection.manifest)?;
    if !extensions::store_review_fits(&inspection.manifest) {
        return Err("The complete permission and content-script review is too large for Bowser to display safely. Nothing was installed; declarations were not truncated.".to_owned());
    }
    let marker = staging.0.join(SOURCE_MARKER);
    if marker.exists() { return Err("The package contains a reserved Bowser source marker.".to_owned()); }
    fs::write(&marker, id).map_err(|_| "The verified package source could not be recorded.".to_owned())?;
    let prepared = extensions::prepare_install(&staging.0, extensions_dir, records_file).map_err(|error| error.to_string())?;
    let prepared = prepared.retain_verified_store_original(id, &original).map_err(|error| format!("Installation was not accepted. {error} Nonexecuting staging may remain."))?;
    check_cancelled()?;
    Ok(Review { prepared, name, store_id: id.to_owned(), extensions_dir: extensions_dir.to_owned(), records_file: records_file.to_owned() })
}

pub fn installed_source(folder: &Path) -> Option<String> {
    let path = folder.join(SOURCE_MARKER);
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() != 32 { return None; }
    let id = fs::read_to_string(path).ok()?;
    crx_fetch::parse_extension_id(&id).ok()
}

/// Resolves a localized name from the validated copy, not from catalogue claims or remote HTML.
/// Only a plain bounded locale file is read. Missing/malformed localized names fail the review.
pub fn resolved_name(folder: &Path, manifest: &Manifest) -> Result<String, String> {
    let Some(key) = manifest.name.strip_prefix("__MSG_").and_then(|name| name.strip_suffix("__")) else { return Ok(manifest.name.clone()); };
    use extensions::json::{self, Value};
    let raw = fs::read_to_string(folder.join("manifest.json")).map_err(|_| "The manifest name could not be resolved.")?;
    let Some(Value::Object(fields)) = json::parse(&raw) else { return Err("The manifest name could not be resolved.".into()); };
    let Some(Value::String(locale)) = json::get(&fields, "default_locale") else { return Err("The localized name has no default locale.".into()); };
    if locale.is_empty() || locale.len() > 35 || !locale.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')) { return Err("The default locale is not a safe locale name.".into()); }
    let path = folder.join("_locales").join(locale).join("messages.json");
    // The whole folder was already inspected for links/junctions and size limits. Check this file
    // again before reading it, and bound the parser independently.
    let meta = fs::symlink_metadata(&path).map_err(|_| "The localized name file is missing.")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > extensions::MAX_MANIFEST_BYTES { return Err("The localized name file is not safe to read.".into()); }
    let text = fs::read_to_string(path).map_err(|_| "The localized name file could not be read.")?;
    let Some(Value::Object(messages)) = json::parse(&text) else { return Err("The localized name file is malformed.".into()); };
    let Some((_, Value::Object(message))) = messages.iter().find(|(name, _)| name.eq_ignore_ascii_case(key)) else { return Err("The localized name message is missing.".into()); };
    let Some(Value::String(name)) = json::get(message, "message") else { return Err("The localized name message is malformed.".into()); };
    if name.is_empty() || name.chars().count() > 128 || name.chars().any(|c| c.is_control() || extensions::is_invisible_format(c)) || name.contains('$') { return Err("The localized extension name is unsafe or needs unsupported substitutions.".into()); }
    Ok(name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogue_has_real_distinct_validated_sources() {
        let mut ids = std::collections::HashSet::new();
        for item in CATALOGUE {
            assert!(ids.insert(item.id));
            assert_eq!(crx_fetch::parse_store_url(item.source).unwrap(), item.id);
            assert_eq!(url::Url::parse(item.homepage).unwrap().scheme(), "https");
        }
    }
    #[test]
    fn cooperative_downloads_have_a_bounded_worker_budget() {
        let first = acquire_download_permit().unwrap();
        let second = acquire_download_permit().unwrap();
        assert!(acquire_download_permit().is_none());
        drop(first);
        let replacement = acquire_download_permit().unwrap();
        assert!(acquire_download_permit().is_none());
        drop(second); drop(replacement);
        assert_eq!(DOWNLOAD_WORKERS.load(Ordering::Acquire), 0);
    }

    #[test]
    fn concurrent_reviews_cannot_commit_the_same_store_identity_twice() {
        let root = std::env::temp_dir().join(format!("bowser-store-duplicate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root); fs::create_dir_all(&root).unwrap();
        let source = root.join("source"); fs::create_dir(&source).unwrap();
        fs::write(source.join("manifest.json"), br#"{"manifest_version":3,"name":"Resolved name","version":"1"}"#).unwrap();
        let id = CATALOGUE[0].id; fs::write(source.join(SOURCE_MARKER), id).unwrap();
        let extensions_dir = root.join("Extensions"); let records_file = root.join("extensions.txt");
        let review = || Review {
            prepared: extensions::prepare_install(&source, &extensions_dir, &records_file).unwrap(),
            name: "Resolved name".into(), store_id: id.into(),
            extensions_dir: extensions_dir.clone(), records_file: records_file.clone(),
        };
        let first = review(); let second = review();
        let second_folder = second.prepared.folder().unwrap();
        let record = first.commit().unwrap();
        assert!(second.commit().is_err());
        assert!(!second_folder.exists());
        assert_eq!(extensions::read_records(&records_file).unwrap(), vec![record]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn historical_compatibility_is_exact_and_never_a_global_chrome_promise() {
        let (label, note) = catalogue_evidence(CATALOGUE[0].id);
        assert_eq!(label, "partial"); assert!(note.contains("2026.1012.1545") && note.contains("154.0.4258.62") && note.contains("Other package/runtime versions are untested"));
        let (label, note) = catalogue_evidence(CATALOGUE[2].id);
        assert_eq!(label, "partial"); assert!(note.contains("2.4.14") && note.contains("diagnostic own-page navigation"));
        assert_eq!(catalogue_evidence(CATALOGUE[1].id).0, "untested");
        assert_eq!(catalogue_evidence(CATALOGUE[3].id).0, "untested");
        assert_eq!(catalogue_evidence("not-a-catalogue-entry").0, "untested");
    }

    #[test]
    fn review_tokens_are_bounded_and_not_paths() {
        assert!(valid_request_id(&request_id()));
        for bad in ["", "../a", "a b", "a\\b", "a\n"] { assert!(!valid_request_id(bad)); }
        assert!(!valid_request_id(&"a".repeat(65)));
    }
}
