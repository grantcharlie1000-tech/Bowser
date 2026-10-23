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
    /// The extension's own icon as a `data:image/...;base64,` URL, or empty when it declares none
    /// or the file is not a small plain PNG/JPEG/GIF/WebP. Never blocks or fails the review.
    pub icon: String,
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
    crx_fetch::verify_and_unpack(&original, id, &staging.0).map_err(|error| verify_failure_message(&error))?;
    check_cancelled()?;
    let inspection = extensions::inspect_source(&staging.0).map_err(|error| error.to_string())?;
    let name = resolved_name(&staging.0, &inspection.manifest)?;
    if !extensions::store_review_fits(&inspection.manifest) {
        return Err("The complete permission and content-script review is too large for Bowser to display safely. Nothing was installed; declarations were not truncated.".to_owned());
    }
    let icon = icon_data_url(&staging.0);
    let marker = staging.0.join(SOURCE_MARKER);
    if marker.exists() { return Err("The package contains a reserved Bowser source marker.".to_owned()); }
    fs::write(&marker, id).map_err(|_| "The verified package source could not be recorded.".to_owned())?;
    let prepared = extensions::prepare_install(&staging.0, extensions_dir, records_file).map_err(|error| error.to_string())?;
    let prepared = prepared.retain_verified_store_original(id, &original).map_err(|error| format!("Installation was not accepted. {error} Nonexecuting staging may remain."))?;
    check_cancelled()?;
    Ok(Review { prepared, name, store_id: id.to_owned(), icon, extensions_dir: extensions_dir.to_owned(), records_file: records_file.to_owned() })
}

/// The review-screen text for a package that did not make it from verified bytes to a staged
/// folder. A signature or identity failure and a rejection by Bowser's own extraction rules are
/// different facts for the user, so they are worded differently; neither is a compatibility verdict.
fn verify_failure_message(error: &crx_fetch::FetchError) -> String {
    use crate::crx_unpack::StampError;
    const NOT_A_VERDICT: &str = "Nothing was installed. This is not a runtime compatibility verdict.";
    match error {
        crx_fetch::FetchError::Unpack(StampError::Unpack(rejection)) => match rejection.policy_message() {
            Some(message) => format!("{message} {NOT_A_VERDICT}"),
            None => format!("The package verified, but it could not be unpacked: {rejection}. {NOT_A_VERDICT}"),
        },
        crx_fetch::FetchError::Unpack(other) => format!("The package verified, but it could not be unpacked: {other}. {NOT_A_VERDICT}"),
        _ => format!("Package verification failed: {error}. {NOT_A_VERDICT}"),
    }
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

/// Largest icon file read for the review, and the largest declared size still worth showing: the
/// tile is far smaller than either, so a larger declaration is a heavy file, not a better one.
const MAX_ICON_BYTES: u64 = 64 * 1024;
const PREFERRED_ICON_SIZE: u32 = 128;
const MAX_ICON_SIZE: u32 = 256;
const MAX_ICON_ATTEMPTS: usize = 6;

/// `(size, path)` pairs from an `icons`-style object (`{"16": "a.png", "128": "b.png"}`). A bare string
/// (only valid for `default_icon`) names one file of unknown size and counts as the preferred size.
/// Keys that are not plain sizes and values that are not strings are skipped.
fn declared_sizes(value: Option<&extensions::json::Value>) -> Vec<(u32, String)> {
    use extensions::json::Value;
    match value {
        Some(Value::String(path)) => vec![(PREFERRED_ICON_SIZE, path.clone())],
        Some(Value::Object(fields)) => fields.iter().filter_map(|(key, value)| match value {
            Value::String(path) if !key.is_empty() && key.len() <= 4 && key.bytes().all(|b| b.is_ascii_digit()) => Some((key.parse().ok()?, path.clone())),
            _ => None,
        }).collect(),
        _ => Vec::new(),
    }
}

/// Best first: the largest size up to 128, then the smallest above it up to 256. Sizes outside
/// 1..=256 are dropped.
fn icon_preference(mut sizes: Vec<(u32, String)>) -> Vec<String> {
    sizes.retain(|(size, _)| (1..=MAX_ICON_SIZE).contains(size));
    sizes.sort_by_key(|(size, _)| if *size <= PREFERRED_ICON_SIZE { (0, u32::MAX - size) } else { (1, *size) });
    sizes.into_iter().map(|(_, path)| path).collect()
}

/// Icon paths in the order to try: `icons`, then the toolbar action's `default_icon`.
fn icon_candidates(manifest: &str) -> Vec<String> {
    use extensions::json::{self, Value};
    let Some(Value::Object(fields)) = json::parse(manifest.strip_prefix('\u{feff}').unwrap_or(manifest)) else { return Vec::new(); };
    let mut paths = icon_preference(declared_sizes(json::get(&fields, "icons").filter(|icons| matches!(icons, Value::Object(_)))));
    for key in ["action", "browser_action"] {
        if let Some(Value::Object(action)) = json::get(&fields, key) { paths.extend(icon_preference(declared_sizes(json::get(action, "default_icon")))); }
    }
    paths
}

/// The image type the bytes themselves show, for the formats the review tile accepts. The file name
/// is never consulted, and SVG (which can carry script) is not an accepted format.
fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) { Some("image/png") }
    else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) { Some("image/jpeg") }
    else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") { Some("image/gif") }
    else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" { Some("image/webp") }
    else { None }
}

/// The extension's icon from its verified, unpacked folder as a data URL, or empty. Every problem
/// (no declaration, unsafe path, link, over 64 KiB, not a PNG/JPEG/GIF/WebP) simply gives no icon.
pub fn icon_data_url(folder: &Path) -> String {
    let Some(manifest) = extensions::read_declared_file(folder, "manifest.json", extensions::MAX_MANIFEST_BYTES) else { return String::new(); };
    let Ok(manifest) = String::from_utf8(manifest) else { return String::new(); };
    icon_candidates(&manifest).iter().take(MAX_ICON_ATTEMPTS).find_map(|path| {
        let bytes = extensions::read_declared_file(folder, path, MAX_ICON_BYTES)?;
        let mime = sniff_image_mime(&bytes)?;
        Some(format!("data:{mime};base64,{}", crate::chrome_host::protocol::base64_encode(&bytes)))
    }).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_store_discovery_is_separate_from_trusted_review() {
        assert_eq!(BROWSE_URL, "https://chromewebstore.google.com/category/extensions");
        let source = include_str!("win32/store.rs");
        let discovery = source.split("pub(crate) fn open(").nth(1).unwrap().split("fn open_review(").next().unwrap();
        assert!(discovery.contains("App::navigate_current(app, &url)"));
        assert!(!discovery.contains("open_internal_page"));
        assert!(source.contains("open_review(owner, app);"));
        let app = include_str!("app.rs");
        let restored = app.split("fn navigate_internal(").nth(1).unwrap();
        assert!(restored.contains("Self::navigate_tab(app, tab_id, &destination)"));
    }

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
            name: "Resolved name".into(), store_id: id.into(), icon: String::new(),
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
    fn extraction_policy_and_signature_failures_read_differently() {
        use crate::crx_unpack::{EntryLimit, StampError, UnpackError};
        let big = crx_fetch::FetchError::Unpack(StampError::Unpack(UnpackError::FileTooLarge(EntryLimit {
            name: "Build/Ultimate Car Driving.data".into(), measured: 150 << 20, allowed: 128 << 20,
        })));
        let message = verify_failure_message(&big);
        assert_eq!(message, "This extension contains a file larger than Bowser allows (Build/Ultimate Car Driving.data, 150.0 MiB > 128.0 MiB). Nothing was installed. This is not a runtime compatibility verdict.");
        assert!(!message.contains("verification failed"));

        let verify = verify_failure_message(&crx_fetch::FetchError::AppIdMismatch);
        assert!(verify.starts_with("Package verification failed:") && verify.ends_with("not a runtime compatibility verdict."), "{verify}");

        let damaged = verify_failure_message(&crx_fetch::FetchError::Unpack(StampError::Unpack(UnpackError::CorruptEntry)));
        assert!(damaged.starts_with("The package verified, but it could not be unpacked:"), "{damaged}");
        let manifest = verify_failure_message(&crx_fetch::FetchError::Unpack(StampError::ManifestMissing));
        assert!(manifest.contains("could not be unpacked") && manifest.contains("manifest.json"), "{manifest}");
        let rule = verify_failure_message(&crx_fetch::FetchError::Unpack(StampError::Unpack(UnpackError::Encrypted)));
        assert!(rule.contains("package rules"), "{rule}");
    }

    #[test]
    fn review_tokens_are_bounded_and_not_paths() {
        assert!(valid_request_id(&request_id()));
        for bad in ["", "../a", "a b", "a\\b", "a\n"] { assert!(!valid_request_id(bad)); }
        assert!(!valid_request_id(&"a".repeat(65)));
    }

    // ---- Extension icon ----

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D];
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F'];
    const GIF: &[u8] = b"GIF89a\x01\x00\x01\x00";
    const WEBP: &[u8] = b"RIFF\x04\x00\x00\x00WEBPVP8 ";

    /// A throwaway package folder `root/pkg` with `manifest.json` and the given files.
    fn icon_package(name: &str, manifest: &str, files: &[(&str, &[u8])]) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("bowser-store-icon-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let pkg = root.join("pkg"); fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("manifest.json"), manifest).unwrap();
        for (path, bytes) in files {
            let target = pkg.join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, bytes).unwrap();
        }
        (root, pkg)
    }
    fn data_url(mime: &str, bytes: &[u8]) -> String { format!("data:{mime};base64,{}", crate::chrome_host::protocol::base64_encode(bytes)) }
    fn png_with(tag: &[u8]) -> Vec<u8> { [PNG, tag].concat() }

    #[test]
    fn the_icon_is_the_largest_declared_size_up_to_128() {
        let (root, pkg) = icon_package("largest", r#"{"icons":{"16":"a16.png","48":"a48.png","128":"a128.png","512":"a512.png"}}"#,
            &[("a16.png", &png_with(b"16")), ("a48.png", &png_with(b"48")), ("a128.png", &png_with(b"128")), ("a512.png", &png_with(b"512"))]);
        assert_eq!(icon_data_url(&pkg), data_url("image/png", &png_with(b"128")));
        // Declaration order does not matter, and a missing 128 file falls to the next best.
        fs::write(pkg.join("manifest.json"), r#"{"icons":{"512":"a512.png","48":"a48.png","128":"gone.png","16":"a16.png"}}"#).unwrap();
        assert_eq!(icon_data_url(&pkg), data_url("image/png", &png_with(b"48")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn with_nothing_up_to_128_the_smallest_larger_icon_is_used_up_to_256() {
        let (root, pkg) = icon_package("above", r#"{"icons":{"300":"c.png","200":"b.png","160":"a.png"}}"#,
            &[("a.png", &png_with(b"160")), ("b.png", &png_with(b"200")), ("c.png", &png_with(b"300"))]);
        assert_eq!(icon_data_url(&pkg), data_url("image/png", &png_with(b"160")));
        fs::write(pkg.join("manifest.json"), r#"{"icons":{"300":"c.png","512":"d.png"}}"#).unwrap();
        assert_eq!(icon_data_url(&pkg), "", "nothing small enough is declared");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_action_default_icon_is_used_when_icons_is_absent() {
        let files: &[(&str, &[u8])] = &[("x16.png", &png_with(b"x16")), ("x32.png", &png_with(b"x32")), ("one.png", &png_with(b"one"))];
        for (name, manifest, tag) in [
            ("action-map", r#"{"action":{"default_icon":{"16":"x16.png","32":"x32.png"}}}"#, &b"x32"[..]),
            ("action-string", r#"{"action":{"default_icon":"one.png"}}"#, b"one"),
            ("browser-action", r#"{"browser_action":{"default_icon":{"16":"x16.png"}}}"#, b"x16"),
            ("icons-first", r#"{"icons":{"16":"x16.png"},"action":{"default_icon":"one.png"}}"#, b"x16"),
        ] {
            let (root, pkg) = icon_package(name, manifest, files);
            assert_eq!(icon_data_url(&pkg), data_url("image/png", &png_with(tag)), "{name}");
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn an_unsafe_icon_path_is_never_read() {
        let (root, pkg) = icon_package("paths", "{}", &[("real.png", PNG)]);
        fs::write(root.join("evil.png"), PNG).unwrap();
        let absolute = pkg.join("real.png").to_string_lossy().replace('\\', "/");
        for bad in ["../evil.png", "a/../../evil.png", "./real.png", "/real.png", absolute.as_str(), "C:/Windows/x.png", "C:real.png", "..\\evil.png", "real.png\\", "sub\\real.png",
                    "", "real.png:stream", "https://example.com/a.png", "data:image/png;base64,AAAA", "real.png ", "rea%6c.png", "nul", "real.png."] {
            let manifest = format!(r#"{{"icons":{{"128":{}}}}}"#, crate::chrome_host::protocol::json_string(bad));
            fs::write(pkg.join("manifest.json"), manifest).unwrap();
            assert_eq!(icon_data_url(&pkg), "", "{bad:?}");
        }
        // The same declaration is fine when it is a plain relative path.
        fs::write(pkg.join("manifest.json"), r#"{"icons":{"128":"real.png"}}"#).unwrap();
        assert_eq!(icon_data_url(&pkg), data_url("image/png", PNG));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_folder_a_link_or_a_junction_is_not_an_icon_file() {
        let (root, pkg) = icon_package("links", r#"{"icons":{"128":"dir.png"}}"#, &[]);
        fs::create_dir(pkg.join("dir.png")).unwrap();
        assert_eq!(icon_data_url(&pkg), "", "a folder named like an image");

        // A junction inside the package must not lead out of it.
        let outside = root.join("outside"); fs::create_dir(&outside).unwrap();
        fs::write(outside.join("icon.png"), PNG).unwrap();
        let link = pkg.join("img");
        let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&link).arg(&outside).output().expect("cmd.exe runs");
        assert!(made.status.success() && link.is_dir(), "mklink /J failed");
        fs::write(pkg.join("manifest.json"), r#"{"icons":{"128":"img/icon.png"}}"#).unwrap();
        assert_eq!(icon_data_url(&pkg), "", "a junction in the path");

        // A file symlink needs a privilege some accounts lack; check it when it can be made.
        let target = pkg.join("target.png"); fs::write(&target, PNG).unwrap();
        if std::os::windows::fs::symlink_file(&target, pkg.join("link.png")).is_ok() {
            fs::write(pkg.join("manifest.json"), r#"{"icons":{"128":"link.png"}}"#).unwrap();
            assert_eq!(icon_data_url(&pkg), "", "a symlinked icon file");
        }
        // Remove the junction itself first so cleanup cannot reach into `outside`'s target by link.
        let _ = fs::remove_dir(&link);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_oversize_icon_is_skipped_without_failing_anything() {
        let mut exact = PNG.to_vec(); exact.resize(MAX_ICON_BYTES as usize, 7);
        let mut over = PNG.to_vec(); over.resize(MAX_ICON_BYTES as usize + 1, 7);
        let (root, pkg) = icon_package("size", r#"{"icons":{"128":"big.png"}}"#, &[("big.png", &over), ("exact.png", &exact), ("small.png", &png_with(b"s"))]);
        assert_eq!(icon_data_url(&pkg), "");
        fs::write(pkg.join("manifest.json"), r#"{"icons":{"128":"exact.png"}}"#).unwrap();
        let url = icon_data_url(&pkg);
        assert_eq!(url, data_url("image/png", &exact), "exactly 64 KiB is allowed");
        assert!(url.len() < 90 * 1024, "{}", url.len());
        // Too big at 128 falls to a smaller usable icon.
        fs::write(pkg.join("manifest.json"), r#"{"icons":{"128":"big.png","32":"small.png"}}"#).unwrap();
        assert_eq!(icon_data_url(&pkg), data_url("image/png", &png_with(b"s")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn svg_and_non_images_are_refused_whatever_the_file_is_called() {
        let svg: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"/>"#;
        let (root, pkg) = icon_package("kinds", "{}", &[("a.svg", svg), ("b.png", svg), ("c.png", b"hello, not an image"), ("d.png", b""), ("e.png", b"\x89PNG"), ("f.png", b"<html><script>1</script></html>")]);
        for name in ["a.svg", "b.png", "c.png", "d.png", "e.png", "f.png"] {
            fs::write(pkg.join("manifest.json"), format!(r#"{{"icons":{{"128":"{name}"}}}}"#)).unwrap();
            assert_eq!(icon_data_url(&pkg), "", "{name}");
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_sniffed_type_wins_over_the_file_extension() {
        let (root, pkg) = icon_package("sniff", "{}", &[("jpeg-as.png", JPEG), ("png-as.svg", PNG), ("gif-as.jpg", GIF), ("webp-as.png", WEBP), ("png-as.bin", PNG)]);
        for (name, mime, bytes) in [("jpeg-as.png", "image/jpeg", JPEG), ("png-as.svg", "image/png", PNG), ("gif-as.jpg", "image/gif", GIF), ("webp-as.png", "image/webp", WEBP), ("png-as.bin", "image/png", PNG)] {
            fs::write(pkg.join("manifest.json"), format!(r#"{{"icons":{{"128":"{name}"}}}}"#)).unwrap();
            assert_eq!(icon_data_url(&pkg), data_url(mime, bytes), "{name}");
        }
        for (bytes, expected) in [(PNG, Some("image/png")), (JPEG, Some("image/jpeg")), (GIF, Some("image/gif")), (&b"GIF87a.."[..], Some("image/gif")), (WEBP, Some("image/webp")),
            (&b"RIFF\0\0\0\0WAVEfmt "[..], None), (&b"RIFF\0\0\0\0WEB"[..], None), (&b"GIF8"[..], None), (&[0xFF, 0xD8][..], None), (&b"BM\0\0"[..], None), (&b"<svg"[..], None), (&b""[..], None)] {
            assert_eq!(sniff_image_mime(bytes), expected, "{bytes:?}");
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_icon_declarations_give_an_empty_icon_not_an_error() {
        let (root, pkg) = icon_package("malformed", "{}", &[("a.png", PNG)]);
        for manifest in [
            "", "not json", "[]", "null", r#"{"icons":5}"#, r#"{"icons":null}"#, r#"{"icons":["a.png"]}"#, r#"{"icons":"a.png"}"#, r#"{"icons":{}}"#,
            r#"{"icons":{"128":3}}"#, r#"{"icons":{"128":null}}"#, r#"{"icons":{"128":["a.png"]}}"#, r#"{"icons":{"big":"a.png","-1":"a.png","1e2":"a.png","":"a.png","0":"a.png","99999999999":"a.png"}}"#,
            r#"{"action":"x","browser_action":5}"#, r#"{"action":{"default_icon":7}}"#, r#"{"action":{"default_icon":{"16":false}}}"#, r#"{"icons":{"128":"missing.png"}}"#,
        ] {
            fs::write(pkg.join("manifest.json"), manifest).unwrap();
            assert_eq!(icon_data_url(&pkg), "", "{manifest}");
        }
        fs::write(pkg.join("manifest.json"), [0xFF, 0xFE, 0x00]).unwrap();
        assert_eq!(icon_data_url(&pkg), "", "not UTF-8");
        fs::remove_file(pkg.join("manifest.json")).unwrap();
        assert_eq!(icon_data_url(&pkg), "", "no manifest");
        assert_eq!(icon_data_url(&root.join("no-such-folder")), "");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_byte_order_mark_does_not_hide_the_icon() {
        let (root, pkg) = icon_package("bom", "\u{feff}{\"icons\":{\"16\":\"a.png\"}}", &[("a.png", PNG)]);
        assert_eq!(icon_data_url(&pkg), data_url("image/png", PNG));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_icon_data_url_round_trips_for_every_length_modulo_three() {
        for len in [12, 13, 14, 100, 101, 102, 4096, 4097, 4098] {
            let mut bytes = PNG.to_vec(); bytes.resize(len, 0);
            for (i, b) in bytes.iter_mut().enumerate().skip(PNG.len()) { *b = (i * 31 % 251) as u8; }
            let (root, pkg) = icon_package(&format!("rt-{len}"), r#"{"icons":{"128":"i.png"}}"#, &[("i.png", &bytes)]);
            let url = icon_data_url(&pkg);
            let encoded = url.strip_prefix("data:image/png;base64,").unwrap_or_else(|| panic!("{len}: {url}"));
            assert_eq!(encoded.len() % 4, 0, "{len}");
            assert_eq!(crate::blocker::json::base64_decode(encoded).as_deref(), Some(&bytes[..]), "{len}");
            let _ = fs::remove_dir_all(root);
        }
    }
}
