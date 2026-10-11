//! Read Chromium's JSON Bookmarks file without opening or modifying browser databases.

use super::{Bookmark, validate_bookmark};
use crate::extensions::json::{self, Value};

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 32;

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
