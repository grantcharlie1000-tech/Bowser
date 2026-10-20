//! Browser migration parsers and the per-profile plain-text bookmark/history stores.

pub mod bookmarks_html;
pub mod chromium;
pub mod firefox;
pub mod store;

use std::{fs, path::PathBuf};
use zeroize::Zeroize;

pub const MAX_URL_CHARS: usize = 4096;
pub const MAX_TITLE_CHARS: usize = 1024;
pub const MAX_FOLDER_CHARS: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
    pub folder: String,
}

impl Bookmark {
    pub fn new(
        title: impl Into<String>,
        url: impl Into<String>,
        folder: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            url: url.into(),
            folder: folder.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub unix_ts: i64,
    pub url: String,
    pub title: String,
}

impl HistoryEntry {
    pub fn new(unix_ts: i64, url: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            unix_ts,
            url: url.into(),
            title: title.into(),
        }
    }
}

/// In-memory credential representation for adapters. Passwords are never formatted or logged.
pub struct ParsedCredential {
    pub url: String,
    pub username: String,
    pub password: String,
}

impl Drop for ParsedCredential {
    fn drop(&mut self) {
        self.url.zeroize();
        self.username.zeroize();
        self.password.zeroize();
    }
}

/// Adapter seam for future credential sources. The existing consent-based CSV panel remains the
/// only wired importer; callers must obtain explicit confirmation before passing rows to the vault.
pub trait CredentialRowsSource {
    fn parse_rows(&self, input: &[u8]) -> Result<Vec<ParsedCredential>, &'static str>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserProfile {
    pub browser: &'static str,
    pub name: String,
    pub root: PathBuf,
    pub bookmarks: Option<PathBuf>,
}

pub fn detect_browser_profiles() -> Vec<BrowserProfile> {
    let mut profiles = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        for (browser, user_data) in [
            ("Chrome", local.join("Google/Chrome/User Data")),
            ("Edge", local.join("Microsoft/Edge/User Data")),
            ("Brave", local.join("BraveSoftware/Brave-Browser/User Data")),
        ] {
            if let Ok(entries) = fs::read_dir(&user_data) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir()
                        && (path.join("Bookmarks").is_file()
                            || entry.file_name() == "Default"
                            || entry.file_name().to_string_lossy().starts_with("Profile "))
                    {
                        profiles.push(BrowserProfile {
                            browser,
                            name: entry.file_name().to_string_lossy().into_owned(),
                            bookmarks: path
                                .join("Bookmarks")
                                .is_file()
                                .then(|| path.join("Bookmarks")),
                            root: path,
                        });
                    }
                }
            }
        }
    }
    if let Some(roaming) = std::env::var_os("APPDATA").map(PathBuf::from) {
        let root = roaming.join("Mozilla/Firefox/Profiles");
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    profiles.push(BrowserProfile {
                        browser: "Firefox",
                        name: entry.file_name().to_string_lossy().into_owned(),
                        root: path,
                        bookmarks: None,
                    });
                }
            }
        }
    }
    profiles.sort_by(|a, b| a.browser.cmp(b.browser).then_with(|| a.name.cmp(&b.name)));
    profiles
}

fn clean_field(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\t' | '\r' | '\n') {
                ' '
            } else {
                c
            }
        })
        .take(max_chars)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn validate_bookmark(mut item: Bookmark) -> Option<Bookmark> {
    if item.url.len() > MAX_URL_CHARS {
        return None;
    }
    let parsed = url::Url::parse(item.url.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    item.url = parsed.to_string();
    item.title = clean_field(&item.title, MAX_TITLE_CHARS);
    item.folder = clean_field(&item.folder, MAX_FOLDER_CHARS);
    Some(item)
}

fn normalized_url(value: &str) -> Option<String> {
    let mut parsed = url::Url::parse(value).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    parsed.set_fragment(None);
    Some(parsed.to_string())
}

pub fn dedupe_bookmarks(items: Vec<Bookmark>, existing: &[Bookmark]) -> (Vec<Bookmark>, usize) {
    let mut seen = existing
        .iter()
        .filter_map(|b| normalized_url(&b.url))
        .collect::<std::collections::HashSet<_>>();
    let mut unique = Vec::new();
    let mut duplicates = 0;
    for item in items {
        let Some(item) = validate_bookmark(item) else {
            continue;
        };
        if seen.insert(normalized_url(&item.url).unwrap_or_default()) {
            unique.push(item);
        } else {
            duplicates += 1;
        }
    }
    (unique, duplicates)
}

pub fn encode_bookmarks(items: &[Bookmark]) -> String {
    items
        .iter()
        .map(|b| {
            format!(
                "{}\t{}\t{}\n",
                clean_field(&b.title, MAX_TITLE_CHARS),
                clean_field(&b.url, MAX_URL_CHARS),
                clean_field(&b.folder, MAX_FOLDER_CHARS)
            )
        })
        .collect()
}

pub fn parse_bookmark_store(text: &str) -> Vec<Bookmark> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            Some(Bookmark::new(
                fields.next()?,
                fields.next()?,
                fields.next()?,
            ))
        })
        .collect()
}

pub fn encode_history(items: &[HistoryEntry]) -> String {
    items
        .iter()
        .filter_map(|h| {
            let parsed = url::Url::parse(&h.url).ok()?;
            if !matches!(parsed.scheme(), "http" | "https") {
                return None;
            }
            Some(format!(
                "{}\t{}\t{}\n",
                h.unix_ts,
                parsed,
                clean_field(&h.title, MAX_TITLE_CHARS)
            ))
        })
        .collect()
}

pub fn parse_history_store(text: &str) -> Vec<HistoryEntry> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            Some(HistoryEntry::new(
                fields.next()?.parse().ok()?,
                fields.next()?,
                fields.next()?,
            ))
        })
        .collect()
}

fn atomic_write(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("missing parent"))?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmark_urls_are_scheme_checked_and_titles_are_sanitized() {
        let valid = Bookmark {
            title: "  News\t\nfeed  ".into(),
            url: "https://example.com/path".into(),
            folder: "Toolbar\tFolder".into(),
        };
        assert_eq!(
            validate_bookmark(valid),
            Some(Bookmark {
                title: "News feed".into(),
                url: "https://example.com/path".into(),
                folder: "Toolbar Folder".into()
            })
        );
        for url in ["javascript:alert(1)", "data:text/plain,x", "file:///secret"] {
            assert!(validate_bookmark(Bookmark::new("bad", url, "")).is_none());
        }
    }

    #[test]
    fn duplicate_bookmarks_are_removed_by_normalized_url() {
        let entries = vec![
            Bookmark::new("one", "https://example.com/a", ""),
            Bookmark::new("two", "https://EXAMPLE.com:443/a#top", ""),
            Bookmark::new("three", "http://example.com/a", ""),
        ];
        let (unique, duplicates) = dedupe_bookmarks(entries, &[]);
        assert_eq!(unique.len(), 2);
        assert_eq!(duplicates, 1);
    }

    #[test]
    fn store_lines_strip_delimiters_and_round_trip() {
        let bookmarks = vec![Bookmark::new(
            "A\tTitle",
            "https://example.com",
            "Folder\nA",
        )];
        let encoded = encode_bookmarks(&bookmarks);
        assert_eq!(encoded, "A Title\thttps://example.com\tFolder A\n");
        assert_eq!(
            parse_bookmark_store(&encoded),
            vec![Bookmark::new("A Title", "https://example.com", "Folder A")]
        );
        let history = vec![HistoryEntry::new(42, "https://example.com", "A\tTitle")];
        let encoded = encode_history(&history);
        assert_eq!(encoded, "42\thttps://example.com/\tA Title\n");
        assert_eq!(
            parse_history_store(&encoded),
            vec![HistoryEntry::new(42, "https://example.com/", "A Title")]
        );
    }

    #[test]
    fn history_and_bookmark_html_fixture_parsers_validate_and_extract() {
        let (parsed, skipped) = bookmarks_html::parse_netscape_bookmarks_detailed(include_str!(
            "../../tests/fixtures/bookmarks.html"
        ));
        assert_eq!(parsed.len(), 1);
        assert_eq!(skipped, 1);
        assert_eq!(parsed[0].title, "Example & News");
        assert_eq!(parsed[0].folder, "Toolbar");
    }

    #[test]
    fn chromium_bookmarks_fixture_recurses_folders_and_skips_unsafe_urls() {
        let (parsed, skipped) = chromium::parse_bookmarks_detailed(include_str!(
            "../../tests/fixtures/chromium_bookmarks.json"
        ))
        .unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(skipped, 1);
        assert_eq!(parsed[0].folder, "Bookmarks bar");
        assert_eq!(parsed[0].url, "https://example.com/");
    }

    #[test]
    fn bookmark_import_merges_deduplicates_and_writes_the_shared_format() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("bowser-import-test-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bookmarks.txt");
        let summary = store::import_bookmarks(
            &path,
            vec![
                Bookmark::new("A", "https://example.com", "Bar"),
                Bookmark::new("B", "https://EXAMPLE.com:443/#fragment", "Other"),
            ],
        )
        .unwrap();
        assert_eq!(summary.imported, 1);
        assert_eq!(summary.duplicates, 1);
        let links = |dir: &std::path::Path| {
            crate::bookmarks::BookmarkStore::load(dir).unwrap().flat_links()
        };
        assert_eq!(
            links(&dir),
            vec![("A".into(), "https://example.com/".into(), "Bar".into())]
        );
        let second = store::import_bookmarks(
            &path,
            vec![Bookmark::new("C", "https://example.org", "Other")],
        )
        .unwrap();
        assert_eq!(second.imported, 1);
        assert_eq!(
            links(&dir),
            vec![
                ("A".into(), "https://example.com/".into(), "Bar".into()),
                ("C".into(), "https://example.org/".into(), "Other".into()),
            ]
        );
        assert!(!path.exists(), "imports go to the bookmark tree, not the legacy file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// P-02: once a profile has a bookmark tree (anyone who ever bookmarked a page), imported
    /// bookmarks must land in it, with their folders, so the bar and panel show them.
    #[test]
    fn imported_bookmarks_reach_an_existing_bookmark_tree() {
        let dir = std::env::temp_dir().join(format!("bowser-import-v2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bookmarks.txt");
        crate::productivity::add_bookmark(&path, "https://a.example/", "A").unwrap();
        assert!(dir.join("bookmarks-v2.txt").exists());
        let summary = store::import_bookmarks(
            &path,
            vec![
                Bookmark::new("B", "https://b.example/", "Bookmarks bar / Work"),
                Bookmark::new("A again", "https://a.example/#x", ""),
            ],
        )
        .unwrap();
        assert_eq!((summary.imported, summary.duplicates), (1, 1));
        let tree = crate::bookmarks::BookmarkStore::load(&dir).unwrap();
        assert!(tree.is_bookmarked("https://a.example/"));
        assert!(tree.is_bookmarked("https://b.example/"), "{:?}", tree.items());
        assert!(
            tree.flat_links().contains(&(
                "B".into(),
                "https://b.example/".into(),
                "Bookmarks bar/Work".into()
            )),
            "{:?}",
            tree.flat_links()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// P-03: parsing a Netscape export is linear. 5,000 bookmarks took 18.5 s (debug) when every
    /// `<a>` lower-cased the whole rest of the file to find its `</a>`.
    #[test]
    fn a_large_netscape_export_parses_quickly() {
        let mut html = String::from("<!DOCTYPE NETSCAPE-Bookmark-file-1>\n<DL><p>\n<DT><H3>Folder</H3>\n<DL><p>\n");
        for i in 0..5_000 {
            html.push_str(&format!(
                "<DT><A HREF=\"https://site{i}.example/page\" ADD_DATE=\"1700000000\">Site {i}</A>\n"
            ));
        }
        html.push_str("</DL><p>\n</DL><p>\n");
        // A file of opening tags with no closing ones must not be quadratic either.
        html.push_str(&"<a href=\"https://x.example/\">".repeat(5_000));
        let started = std::time::Instant::now();
        let (parsed, _) = bookmarks_html::parse_netscape_bookmarks_detailed(&html);
        let elapsed = started.elapsed();
        assert_eq!(parsed.len(), 5_000);
        assert_eq!(parsed[4_999].folder, "Folder");
        assert!(elapsed < std::time::Duration::from_secs(3), "took {elapsed:?}");
    }

    /// P-03: importing into the tree is linear too, and stops at the tree's limits instead of
    /// writing a file that would not load.
    #[test]
    fn importing_many_links_into_the_tree_is_linear_and_bounded() {
        let mut tree = crate::bookmarks::BookmarkStore::new();
        let links: Vec<(String, String, String)> = (0..6_000)
            .map(|i| (format!("F{}", i % 50), format!("Site {i}"), format!("https://site{i}.example/")))
            .collect();
        let started = std::time::Instant::now();
        let counts = tree.import_links(links.iter().map(|(f, t, u)| (f.as_str(), t.as_str(), u.as_str())));
        assert!(started.elapsed() < std::time::Duration::from_secs(3), "took {:?}", started.elapsed());
        assert_eq!(counts.added + counts.limited, 6_000);
        assert!(counts.limited > 0);
        assert_eq!(tree.items().len(), 5_000);
        let reparsed = crate::bookmarks::BookmarkStore::parse(&tree.serialize()).unwrap();
        assert_eq!(reparsed.items().len(), 5_000);
    }

    #[test]
    fn firefox_lz4_decoder_handles_literals_and_matches_and_rejects_bad_blocks() {
        use firefox::decompress_lz4_block;

        assert_eq!(
            decompress_lz4_block(&[0x50, b'h', b'e', b'l', b'l', b'o'], 5).unwrap(),
            b"hello"
        );
        // "abcabcabc": three literals followed by a six-byte match at offset three.
        assert_eq!(
            decompress_lz4_block(&[0x32, b'a', b'b', b'c', 3, 0], 9).unwrap(),
            b"abcabcabc"
        );
        assert!(decompress_lz4_block(&[0x10, b'a'], 2).is_err());
        assert!(decompress_lz4_block(&[0x00, 0, 0], 4).is_err());
        assert!(decompress_lz4_block(&[0x00, 1, 0], 4).is_err());
        assert!(decompress_lz4_block(&[0x00], 16 * 1024 * 1024 + 1).is_err());
        assert_eq!(
            firefox::decompress_mozlz4(b"mozLz40\0\x02\0\0\0\x20[]").unwrap(),
            b"[]"
        );
        assert!(firefox::decompress_mozlz4(b"bad-header").is_err());
        assert!(firefox::decompress_mozlz4(b"mozLz40\0\x01\0\0\x01").is_err());
    }

    #[test]
    fn firefox_places_have_folder_paths_and_reject_non_http_urls() {
        let json = r#"{"type":"text/x-moz-place-container","children":[{"type":"text/x-moz-place-container","title":"Toolbar","children":[{"type":"text/x-moz-place","title":"Example","uri":"https://example.com"},{"type":"text/x-moz-place","title":"Unsafe","uri":"javascript:alert(1)"}]}]}"#;
        let (items, skipped) = firefox::parse_bookmarks(json).unwrap();
        assert_eq!(
            items,
            vec![Bookmark::new("Example", "https://example.com/", "Toolbar")]
        );
        assert_eq!(skipped, 1);
    }

    #[test]
    fn chromium_preferences_only_map_google_or_duckduckgo_search_providers() {
        use crate::navigation::SearchEngine;
        let google = r#"{"default_search_provider":{"enabled":true,"keyword":"google.com","search_url":"https://www.google.com/search?q={searchTerms}"}}"#;
        assert_eq!(
            chromium::parse_search_preference(google),
            (Some(SearchEngine::Google), false)
        );
        let ddg = r#"{"default_search_provider":{"keyword":"DuckDuckGo","search_url":"https://duckduckgo.com/?q={searchTerms}"}}"#;
        assert_eq!(
            chromium::parse_search_preference(ddg),
            (Some(SearchEngine::DuckDuckGo), false)
        );
        let unsupported = r#"{"default_search_provider":{"keyword":"bing.com","search_url":"https://www.bing.com/search?q={searchTerms}"}}"#;
        assert_eq!(chromium::parse_search_preference(unsupported), (None, true));
    }
}
