//! Read Chromium's JSON Bookmarks file without opening or modifying browser databases.

use super::{Bookmark, validate_bookmark};
use crate::extensions::json::{self, Value};
use crate::navigation::SearchEngine;

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 32;

/// Returns the compatible Bowser engine and whether a source preference was present but unsupported.
pub fn parse_search_preference(text: &str) -> (Option<SearchEngine>, bool) {
    let Some(Value::Object(root)) = json::parse(text) else {
        return (None, false);
    };
    let Some(Value::Object(provider)) = json::get(&root, "default_search_provider") else {
        return (None, false);
    };
    let string = |key| match json::get(provider, key) {
        Some(Value::String(value)) => Some(value.as_str()),
        _ => None,
    };
    let mut hints = String::new();
    for key in ["keyword", "name", "search_url"] {
        if let Some(value) = string(key) {
            hints.push_str(&value.to_ascii_lowercase());
            hints.push(' ');
        }
    }
    let url_candidate = string("search_url")
        .and_then(|value| url::Url::parse(&value.replace("{searchTerms}", "query")).ok());
    let host = url_candidate
        .as_ref()
        .and_then(url::Url::host_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let engine =
        if host == "google.com" || host.ends_with(".google.com") || hints.contains("google") {
            Some(SearchEngine::Google)
        } else if host == "duckduckgo.com"
            || host.ends_with(".duckduckgo.com")
            || hints.contains("duckduckgo")
        {
            Some(SearchEngine::DuckDuckGo)
        } else {
            None
        };
    (engine, engine.is_none())
}

pub fn parse_bookmarks(text: &str) -> Result<Vec<Bookmark>, &'static str> {
    parse_bookmarks_detailed(text).map(|(items, _)| items)
}

pub fn parse_bookmarks_detailed(text: &str) -> Result<(Vec<Bookmark>, usize), &'static str> {
    if text.len() > MAX_BYTES {
        return Err("Bookmarks file exceeds the 32 MiB limit");
    }
    let Value::Object(root) = json::parse(text).ok_or("Bookmarks file is not valid JSON")? else {
        return Err("Bookmarks file must contain a JSON object");
    };
    let Some(Value::Object(roots)) = json::get(&root, "roots") else {
        return Err("Bookmarks roots are missing");
    };
    let mut found = Vec::new();
    for (_, value) in roots {
        visit(value, "", 0, &mut found);
        if found.len() >= MAX_ENTRIES {
            break;
        }
    }
    let candidate_count = found.len();
    let safe = found
        .into_iter()
        .filter_map(validate_bookmark)
        .collect::<Vec<_>>();
    let skipped = candidate_count.saturating_sub(safe.len());
    Ok((safe, skipped))
}

fn visit(value: &Value, folder: &str, depth: usize, found: &mut Vec<Bookmark>) {
    if depth > MAX_DEPTH || found.len() >= MAX_ENTRIES {
        return;
    }
    let Value::Object(fields) = value else { return };
    let string = |key| match json::get(fields, key) {
        Some(Value::String(s)) => Some(s.as_str()),
        _ => None,
    };
    match string("type") {
        Some("url") => {
            if let (Some(title), Some(url)) = (string("name"), string("url")) {
                found.push(Bookmark::new(title, url, folder));
            }
        }
        Some("folder") => {
            let name = string("name").unwrap_or("");
            let path = if folder.is_empty() {
                name.to_owned()
            } else {
                format!("{folder} / {name}")
            };
            if let Some(Value::Array(children)) = json::get(fields, "children") {
                for child in children {
                    visit(child, &path, depth + 1, found);
                    if found.len() >= MAX_ENTRIES {
                        break;
                    }
                }
            }
        }
        _ => {}
    }
}
