//! Defensive reader for the Netscape bookmark HTML export format.

use super::{Bookmark, validate_bookmark};

const MAX_HTML_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 32;

pub fn parse_netscape_bookmarks(html: &str) -> Vec<Bookmark> {
    parse_netscape_bookmarks_detailed(html).0
}

pub fn parse_netscape_bookmarks_detailed(html: &str) -> (Vec<Bookmark>, usize) {
    if html.len() > MAX_HTML_BYTES {
        return (Vec::new(), 0);
    }
    let bytes = html.as_bytes();
    let mut pos = 0;
    let mut folders: Vec<String> = Vec::new();
    let mut entries = Vec::new();
    while let Some(start) = find_byte(bytes, b'<', pos) {
        let Some(end) = find_byte(bytes, b'>', start + 1) else {
            break;
        };
        let tag = html[start + 1..end].trim();
        let lower = tag.to_ascii_lowercase();
        if lower
            .split_ascii_whitespace()
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("h3"))
        {
            if let Some(close) = find_ascii_case_insensitive(html, "</h3>", end + 1) {
                if folders.len() < MAX_DEPTH {
                    let name = decode_entities(&html[end + 1..close]);
                    folders.push(clean_field(&name, 512));
                }
            }
        } else if lower.starts_with("/dl") {
            folders.pop();
        } else if lower.starts_with('a')
            && (lower.len() == 1 || lower.as_bytes().get(1).is_some_and(u8::is_ascii_whitespace))
        {
            if let Some(href) = attribute(tag, "href") {
                if let Some(close) = find_ascii_case_insensitive(html, "</a>", end + 1) {
                    let candidate = Bookmark::new(
                        decode_entities(&html[end + 1..close]),
                        decode_entities(&href),
                        folders.join(" / "),
                    );
                    entries.push(candidate);
                    if entries.len() >= MAX_ENTRIES {
                        break;
                    }
                }
            }
        }
        pos = end + 1;
    }
    let candidate_count = entries.len();
    let safe = entries
        .into_iter()
        .filter_map(validate_bookmark)
        .collect::<Vec<_>>();
    let skipped = candidate_count.saturating_sub(safe.len());
    (safe, skipped)
}

fn find_byte(bytes: &[u8], byte: u8, from: usize) -> Option<usize> {
    bytes
        .get(from..)?
        .iter()
        .position(|b| *b == byte)
        .map(|i| from + i)
}

fn find_ascii_case_insensitive(text: &str, needle: &str, from: usize) -> Option<usize> {
    text.get(from..)?
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
        .map(|i| from + i)
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let start = i;
        while bytes
            .get(i)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
        {
            i += 1;
        }
        if start == i {
            i += 1;
            continue;
        }
        let key = &tag[start..i];
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if bytes.get(i) != Some(&b'=') {
            continue;
        }
        i += 1;
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let quote = *bytes.get(i)?;
        if quote != b'\'' && quote != b'"' {
            continue;
        }
        i += 1;
        let value_start = i;
        while bytes.get(i).is_some_and(|b| *b != quote) {
            i += 1;
        }
        let value = tag.get(value_start..i)?.to_owned();
        if key.eq_ignore_ascii_case(name) {
            return Some(value);
        }
        if bytes.get(i).is_some() {
            i += 1;
        }
    }
    None
}

fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn clean_field(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_owned()
}
