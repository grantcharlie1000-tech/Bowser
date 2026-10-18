//! Bowser's small, curated catalogue and verified package preparation. The catalogue is not a
//! Google search API. No website or catalogue metadata can grant installation consent.
use std::{fs, path::{Path, PathBuf}, sync::atomic::{AtomicBool, AtomicU64, Ordering}};
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

pub fn valid_request_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

pub fn request_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("store-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
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
}

/// Downloads and verifies before touching a profile, then prepares its immutable, unrecorded copy.
/// Cancellation can never commit or load. WinHTTP's finite timeouts bound a request already in flight.
pub fn prepare(
    id: &str, runtime: &str, extensions_dir: &Path, records_file: &Path, cancelled: &AtomicBool,
) -> Result<Review, String> {
    crx_fetch::parse_extension_id(id).map_err(|error| error.to_string())?;
    let check_cancelled = || if cancelled.load(Ordering::Acquire) { Err("Download cancelled. Nothing was installed.".to_owned()) } else { Ok(()) };
    check_cancelled()?;
    // Prevent duplicate signing identities, including while the first installation needs a restart.
    for record in extensions::read_records(records_file).map_err(|error| error.to_string())? {
        if let Ok(folder) = extensions::folder_for(extensions_dir, &record.id) {
            if installed_source(&folder).as_deref() == Some(id) {
                return Err("This extension is already installed in this profile. Use its update control.".to_owned());
            }
        }
    }
    let staging = Staging::new()?;
    crx_fetch::fetch_and_unpack(id, runtime, &staging.0).map_err(|error| format!("Package download or verification failed: {error}. This is not a runtime compatibility verdict."))?;
    check_cancelled()?;
    let inspection = extensions::inspect_source(&staging.0).map_err(|error| error.to_string())?;
    let name = resolved_name(&staging.0, &inspection.manifest)?;
    let marker = staging.0.join(SOURCE_MARKER);
    if marker.exists() { return Err("The package contains a reserved Bowser source marker.".to_owned()); }
    fs::write(&marker, id).map_err(|_| "The verified package source could not be recorded.".to_owned())?;
    let prepared = extensions::prepare_install(&staging.0, extensions_dir, records_file).map_err(|error| error.to_string())?;
    check_cancelled()?;
    Ok(Review { prepared, name, store_id: id.to_owned() })
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
    fn review_tokens_are_bounded_and_not_paths() {
        assert!(valid_request_id(&request_id()));
        for bad in ["", "../a", "a b", "a\\b", "a\n"] { assert!(!valid_request_id(bad)); }
        assert!(!valid_request_id(&"a".repeat(65)));
    }
}
