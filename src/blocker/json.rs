//! Just enough JSON for the blocker: read DevTools messages and drop members from one object
//! without re-serialising the rest of it.
//!
//! This is not a validating parser. It finds value boundaries in well-formed JSON and returns
//! `None` on anything it cannot follow, so a caller can always fall back to "leave it alone".

use std::{borrow::Cow, ops::Range};

/// Containers nested deeper than this are treated as malformed.
const MAX_DEPTH: usize = 1024;

/// One member of an object. `key` is the raw text between the quotes (escapes left as written,
/// which is exact for the plain ASCII keys callers look up). `span` covers `"key":value`.
pub(crate) struct Member<'a> {
    pub key: &'a str,
    pub value: &'a str,
    pub span: Range<usize>,
}

pub(crate) fn skip_ws(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t' | b'\n' | b'\r') {
        index += 1;
    }
    index
}

/// Index just past the string that opens at `bytes[index]`.
fn skip_string(bytes: &[u8], index: usize) -> Option<usize> {
    let mut i = index + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

/// Index just past the object or array that opens at `bytes[index]`.
fn skip_container(bytes: &[u8], index: usize) -> Option<usize> {
    let mut closers = Vec::new();
    let mut i = index;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i = skip_string(bytes, i)?;
                continue;
            }
            b'{' => closers.push(b'}'),
            b'[' => closers.push(b']'),
            close @ (b'}' | b']') => {
                if closers.pop()? != close {
                    return None;
                }
                if closers.is_empty() {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        if closers.len() > MAX_DEPTH {
            return None;
        }
        i += 1;
    }
    None
}

/// Index just past the value that starts exactly at `index`.
fn skip_value(bytes: &[u8], index: usize) -> Option<usize> {
    match *bytes.get(index)? {
        b'"' => skip_string(bytes, index),
        b'{' | b'[' => skip_container(bytes, index),
        _ => {
            let end = bytes[index..]
                .iter()
                .position(|b| !(b.is_ascii_alphanumeric() || matches!(b, b'-' | b'+' | b'.')))
                .map_or(bytes.len(), |offset| index + offset);
            (end > index).then_some(end)
        }
    }
}

/// The members of the object whose `{` is at `start`, and the index just past its `}`.
pub(crate) fn object_members(text: &str, start: usize) -> Option<(Vec<Member<'_>>, usize)> {
    let bytes = text.as_bytes();
    if bytes.get(start) != Some(&b'{') {
        return None;
    }
    let mut members = Vec::new();
    let mut i = skip_ws(bytes, start + 1);
    if bytes.get(i) == Some(&b'}') {
        return Some((members, i + 1));
    }
    loop {
        if bytes.get(i) != Some(&b'"') {
            return None;
        }
        let key_end = skip_string(bytes, i)?;
        let key = text.get(i + 1..key_end - 1)?;
        let colon = skip_ws(bytes, key_end);
        if bytes.get(colon) != Some(&b':') {
            return None;
        }
        let value_start = skip_ws(bytes, colon + 1);
        let value_end = skip_value(bytes, value_start)?;
        members.push(Member {
            key,
            value: text.get(value_start..value_end)?,
            span: i..value_end,
        });
        i = skip_ws(bytes, value_end);
        match bytes.get(i) {
            Some(b',') => i = skip_ws(bytes, i + 1),
            Some(b'}') => return Some((members, i + 1)),
            _ => return None,
        }
    }
}

/// The members of `text` when the whole of it (bar surrounding whitespace) is one object.
pub(crate) fn whole_object(text: &str) -> Option<Vec<Member<'_>>> {
    let start = skip_ws(text.as_bytes(), 0);
    let (members, end) = object_members(text, start)?;
    text[end..].trim().is_empty().then_some(members)
}

/// The value of the first member called `key`.
pub(crate) fn field<'a>(members: &[Member<'a>], key: &str) -> Option<&'a str> {
    members
        .iter()
        .find(|member| member.key == key)
        .map(|member| member.value)
}

/// The raw elements of the array `text` (exactly one array, no surrounding whitespace).
pub(crate) fn array_elements(text: &str) -> Option<Vec<&str>> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'[') {
        return None;
    }
    let mut elements = Vec::new();
    let mut i = skip_ws(bytes, 1);
    if bytes.get(i) == Some(&b']') {
        return (i + 1 == bytes.len()).then_some(elements);
    }
    loop {
        let end = skip_value(bytes, i)?;
        elements.push(text.get(i..end)?);
        i = skip_ws(bytes, end);
        match bytes.get(i) {
            Some(b',') => i = skip_ws(bytes, i + 1),
            Some(b']') => return (i + 1 == bytes.len()).then_some(elements),
            _ => return None,
        }
    }
}

fn hex4(chars: &mut std::str::Chars<'_>) -> Option<u32> {
    let mut value = 0;
    for _ in 0..4 {
        value = value * 16 + chars.next()?.to_digit(16)?;
    }
    Some(value)
}

/// Decodes a raw JSON string value (quotes included). A lone surrogate becomes U+FFFD.
pub(crate) fn string_value(raw: &str) -> Option<String> {
    string_value_limited(raw, usize::MAX).map(Cow::into_owned)
}

pub(crate) fn string_value_limited(raw: &str, limit: usize) -> Option<Cow<'_, str>> {
    let inner = raw.strip_prefix('"')?.strip_suffix('"')?;
    if !inner.contains('\\') {
        return (inner.len() <= limit).then_some(Cow::Borrowed(inner));
    }
    if inner.len() > limit.saturating_mul(6) {
        return None;
    }
    let mut out = String::with_capacity(inner.len().min(limit));
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        let decoded = if ch != '\\' {
            ch
        } else {
            match chars.next()? {
                '"' => '"',
                '\\' => '\\',
                '/' => '/',
                'b' => '\u{8}',
                'f' => '\u{c}',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'u' => {
                    let unit = hex4(&mut chars)?;
                    let paired = (0xD800..0xDC00).contains(&unit).then(|| {
                        let mut ahead = chars.clone();
                        let low = (ahead.next() == Some('\\') && ahead.next() == Some('u'))
                            .then(|| hex4(&mut ahead))
                            .flatten()
                            .filter(|low| (0xDC00..0xE000).contains(low))?;
                        chars = ahead;
                        char::from_u32(0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00))
                    });
                    match paired {
                        Some(pair) => pair.unwrap_or('\u{FFFD}'),
                        None => char::from_u32(unit).unwrap_or('\u{FFFD}'),
                    }
                }
                _ => return None,
            }
        };
        if decoded.len_utf8() > limit - out.len() {
            return None;
        }
        out.push(decoded);
    }
    Some(Cow::Owned(out))
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_DECODE: [u8; 256] = {
    let mut values = [u8::MAX; 256];
    let mut index = 0;
    while index < BASE64.len() {
        values[BASE64[index] as usize] = index as u8;
        index += 1;
    }
    values
};

/// Standard base64 (RFC 4648 section 4) with `=` padding.
pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if index <= chunk.len() {
                out.push(BASE64[(n >> shift) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes standard padded base64; `None` on any other character or a bad length.
pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    base64_decode_limited(text, usize::MAX)
}

pub(crate) fn base64_decode_limited(text: &str, limit: usize) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.len() % 4 != 0 {
        return None;
    }
    let padding = if bytes.ends_with(b"==") {
        2
    } else {
        usize::from(bytes.ends_with(b"="))
    };
    let size = bytes.len() / 4 * 3 - padding;
    if size > limit {
        return None;
    }
    let mut out = Vec::with_capacity(size);
    // Padding is allowed only at the end. Every other byte must be in the RFC 4648 alphabet.
    for chunk in bytes[..bytes.len() - padding].chunks(4) {
        let mut n = 0u32;
        for &byte in chunk {
            let value = BASE64_DECODE[usize::from(byte)];
            if value == u8::MAX {
                return None;
            }
            n = (n << 6) | u32::from(value);
        }
        n <<= 6 * (4 - chunk.len());
        let decoded = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&decoded[..chunk.len() - 1]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_members_finds_each_value_and_its_span() {
        let text = r#" {"a":1, "b" : {"x":[1,"]}"]}, "c":"q\"}" ,"d":null} "#;
        let members = whole_object(text).expect("object");
        let keys: Vec<&str> = members.iter().map(|m| m.key).collect();
        assert_eq!(keys, ["a", "b", "c", "d"]);
        assert_eq!(field(&members, "b"), Some(r#"{"x":[1,"]}"]}"#));
        assert_eq!(field(&members, "c"), Some(r#""q\"}""#));
        assert_eq!(field(&members, "d"), Some("null"));
        assert_eq!(&text[members[1].span.clone()], r#""b" : {"x":[1,"]}"]}"#);
        assert!(whole_object("{}").is_some_and(|m| m.is_empty()));
    }

    #[test]
    fn malformed_json_is_refused_rather_than_guessed() {
        for text in [
            "",
            "[1]",
            "{\"a\":1",
            "{\"a\" 1}",
            "{\"a\":[1}",
            "{\"a\":{]}",
            "{\"a\":1,}",
            "{a:1}",
            "{\"a\":\"open}",
            "{\"a\":1} trailing",
        ] {
            assert!(whole_object(text).is_none(), "{text:?}");
        }
    }

    #[test]
    fn array_elements_splits_top_level_values() {
        assert_eq!(
            array_elements(r#"[{"name":"a","value":"1"}, "x,y" ,3]"#),
            Some(vec![r#"{"name":"a","value":"1"}"#, r#""x,y""#, "3"])
        );
        assert_eq!(array_elements("[]"), Some(Vec::new()));
        assert_eq!(array_elements("[1,]"), None);
        assert_eq!(array_elements("[1]x"), None);
    }

    #[test]
    fn string_value_decodes_escapes_and_surrogate_pairs() {
        assert_eq!(string_value(r#""plain""#).as_deref(), Some("plain"));
        assert_eq!(
            string_value(r#""a\"b\\c\/d\n<é""#).as_deref(),
            Some("a\"b\\c/d\n<é")
        );
        assert_eq!(string_value(r#""😀""#).as_deref(), Some("😀"));
        assert_eq!(string_value(r#""\ud83dx""#).as_deref(), Some("\u{FFFD}x"));
        assert_eq!(string_value(r#""\q""#), None);
        assert_eq!(string_value("unquoted"), None);
    }

    #[test]
    fn decoded_limits_count_bytes_and_padding() {
        assert_eq!(string_value_limited(r#""\u00e9""#, 2).as_deref(), Some("é"));
        assert_eq!(string_value_limited(r#""\u00e9""#, 1), None);
        assert_eq!(string_value_limited(r#""é""#, 1), None);
        assert_eq!(
            base64_decode_limited("Zg==", 1).as_deref(),
            Some(b"f".as_slice())
        );
        assert_eq!(base64_decode_limited("Zm8=", 1), None);
        assert_eq!(base64_decode_limited("Zm9v", 2), None);
    }

    #[test]
    fn base64_matches_rfc4648_vectors_and_round_trips() {
        for (input, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(input.as_bytes()), encoded, "{input:?}");
            assert_eq!(
                base64_decode(encoded).as_deref(),
                Some(input.as_bytes()),
                "{encoded:?}"
            );
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(base64_decode(&base64_encode(&all)), Some(all));
        for bad in ["Zg=", "Zg=a", "Z===", "Zg==Zg==", "Zm9*"] {
            assert_eq!(base64_decode(bad), None, "{bad:?}");
        }
    }
}
