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
    // Lower-cased once (ASCII only, so every byte offset matches `html`); each closing tag is
    // then found from a remembered position instead of re-scanning the rest of the file.
    let lowered = html.to_ascii_lowercase();
    let mut h3_close = Closer::new("</h3>");
    let mut a_close = Closer::new("</a>");
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
            if let Some(close) = h3_close.find(&lowered, end + 1) {
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
                if let Some(close) = a_close.find(&lowered, end + 1) {
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

/// Finds a lower-case `needle` in already lower-cased text for positions that only move forward,
/// remembering the last answer: the next match at or after a later `from` is the same one, and
/// once there is none there never is. All the calls together scan the text once.
struct Closer {
    needle: &'static str,
    /// The last search's `from` and its result.
    last: Option<(usize, Option<usize>)>,
}

impl Closer {
    fn new(needle: &'static str) -> Self {
        Self { needle, last: None }
    }

    fn find(&mut self, lowered: &str, from: usize) -> Option<usize> {
        if let Some((searched_from, found)) = self.last {
            if from >= searched_from {
                match found {
                    None => return None,
                    Some(at) if at >= from => return Some(at),
                    Some(_) => {}
                }
            }
        }
        let found = lowered
            .get(from..)
            .and_then(|rest| rest.find(self.needle))
            .map(|i| from + i);
        self.last = Some((from, found));
        found
    }
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
