//! A small strict JSON reader for the two places text crosses back into Rust: the page script's
//! answer to `ExecuteScript` and the worker page's messages. Both come from a web page's world, so
//! this refuses anything it does not need: floats and exponents, duplicate keys, raw control
//! characters, trailing text, and nesting past [`MAX_DEPTH`]. A lone surrogate escape (which a page
//! can legitimately hold in a text node) becomes U+FFFD, because a Rust string cannot hold one.

/// Containers nested deeper than this are refused. The answers read here are two levels deep.
const MAX_DEPTH: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    /// Whole numbers only.
    Int(i64),
    Str(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Object(members) => members.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(number) => Some(*number),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The member names of an object, for checking that nothing unexpected is present.
    pub fn keys(&self) -> Vec<&str> {
        match self {
            Self::Object(members) => members.iter().map(|(name, _)| name.as_str()).collect(),
            _ => Vec::new(),
        }
    }
}

/// Parses `text` as exactly one JSON value (surrounding whitespace allowed).
pub fn parse(text: &str) -> Option<Value> {
    let mut reader = Reader { bytes: text.as_bytes(), text, at: 0 };
    reader.space();
    let value = reader.value(0)?;
    reader.space();
    (reader.at == reader.bytes.len()).then_some(value)
}

struct Reader<'a> {
    bytes: &'a [u8],
    text: &'a str,
    at: usize,
}

impl Reader<'_> {
    fn space(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.bytes.get(self.at) == Some(&byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn word(&mut self, word: &str) -> bool {
        if self.text[self.at..].starts_with(word) {
            self.at += word.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        match *self.bytes.get(self.at)? {
            b'"' => self.string().map(Value::Str),
            b'[' if depth < MAX_DEPTH => {
                self.at += 1;
                self.space();
                let mut items = Vec::new();
                if self.eat(b']') {
                    return Some(Value::Array(items));
                }
                loop {
                    self.space();
                    items.push(self.value(depth + 1)?);
                    self.space();
                    if self.eat(b',') {
                        continue;
                    }
                    return self.eat(b']').then_some(Value::Array(items));
                }
            }
            b'{' if depth < MAX_DEPTH => {
                self.at += 1;
                self.space();
                let mut members: Vec<(String, Value)> = Vec::new();
                if self.eat(b'}') {
                    return Some(Value::Object(members));
                }
                loop {
                    self.space();
                    let name = self.string()?;
                    if members.iter().any(|(seen, _)| *seen == name) {
                        return None;
                    }
                    self.space();
                    if !self.eat(b':') {
                        return None;
                    }
                    self.space();
                    members.push((name, self.value(depth + 1)?));
                    self.space();
                    if self.eat(b',') {
                        continue;
                    }
                    return self.eat(b'}').then_some(Value::Object(members));
                }
            }
            b't' => self.word("true").then_some(Value::Bool(true)),
            b'f' => self.word("false").then_some(Value::Bool(false)),
            b'n' => self.word("null").then_some(Value::Null),
            b'-' | b'0'..=b'9' => self.int().map(Value::Int),
            _ => None,
        }
    }

    fn int(&mut self) -> Option<i64> {
        let start = self.at;
        self.eat(b'-');
        let digits = self.at;
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
        }
        let length = self.at - digits;
        // No leading zeros, no fraction, no exponent, and nothing that could overflow.
        if length == 0 || length > 15 || (length > 1 && self.bytes[digits] == b'0') {
            return None;
        }
        if matches!(self.bytes.get(self.at), Some(b'.' | b'e' | b'E')) {
            return None;
        }
        self.text[start..self.at].parse().ok()
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.text.get(self.at..self.at + 4)?;
        if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        self.at += 4;
        u32::from_str_radix(digits, 16).ok()
    }

    fn string(&mut self) -> Option<String> {
        if !self.eat(b'"') {
            return None;
        }
        let mut out = String::new();
        loop {
            let run = self.at;
            while let Some(&byte) = self.bytes.get(self.at) {
                if byte == b'"' || byte == b'\\' || byte < 0x20 {
                    break;
                }
                self.at += 1;
            }
            out.push_str(self.text.get(run..self.at)?);
            match *self.bytes.get(self.at)? {
                b'"' => {
                    self.at += 1;
                    return Some(out);
                }
                b'\\' => {
                    self.at += 1;
                    let escape = *self.bytes.get(self.at)?;
                    self.at += 1;
                    out.push(match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let unit = self.hex4()?;
                            let low = |reader: &mut Self| {
                                let ahead = reader.text.get(reader.at..reader.at + 2)?;
                                if ahead != "\\u" {
                                    return None;
                                }
                                let saved = reader.at;
                                reader.at += 2;
                                match reader.hex4() {
                                    Some(low) if (0xDC00..0xE000).contains(&low) => Some(low),
                                    _ => {
                                        reader.at = saved;
                                        None
                                    }
                                }
                            };
                            if (0xD800..0xDC00).contains(&unit) {
                                match low(self) {
                                    Some(low) => char::from_u32(0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00))
                                        .unwrap_or('\u{FFFD}'),
                                    None => '\u{FFFD}',
                                }
                            } else {
                                char::from_u32(unit).unwrap_or('\u{FFFD}')
                            }
                        }
                        _ => return None,
                    });
                }
                _ => return None, // a raw control character
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_shapes_the_answers_use() {
        let value = parse(r#" {"v":1,"c":"ok","s":["a","b\n"],"n":null,"t":true} "#).unwrap();
        assert_eq!(value.get("v").and_then(Value::as_int), Some(1));
        assert_eq!(value.get("c").and_then(Value::as_str), Some("ok"));
        assert_eq!(value.get("s").and_then(Value::as_array).map(<[Value]>::len), Some(2));
        assert_eq!(value.get("s").unwrap().as_array().unwrap()[1].as_str(), Some("b\n"));
        assert_eq!(value.get("n"), Some(&Value::Null));
        assert_eq!(value.get("t"), Some(&Value::Bool(true)));
        assert_eq!(value.keys(), ["v", "c", "s", "n", "t"]);
        assert_eq!(parse("[]"), Some(Value::Array(Vec::new())));
        assert_eq!(parse("{}"), Some(Value::Object(Vec::new())));
        assert_eq!(parse("-7").and_then(|v| v.as_int()), Some(-7));
    }

    #[test]
    fn decodes_escapes_and_pairs_and_replaces_a_lone_surrogate() {
        let text = parse(r#""\u00e9\ud83d\ude00\"\\\/\b\f\t \ud800 \udc00x""#).unwrap();
        assert_eq!(text.as_str(), Some("é😀\"\\/\u{8}\u{c}\t \u{FFFD} \u{FFFD}x"));
        assert_eq!(parse("\"héllo ✓ 😀 日本\"").unwrap().as_str(), Some("héllo ✓ 😀 日本"));
    }

    #[test]
    fn refuses_everything_that_is_not_plain_json() {
        for bad in [
            "",
            " ",
            "nul",
            "1.5",
            "1e3",
            "01",
            "-",
            "1234567890123456",
            "[1,]",
            "[1 2]",
            "{\"a\":1,}",
            "{\"a\":1,\"a\":2}",
            "{a:1}",
            "{\"a\" 1}",
            "'x'",
            "\"x",
            "\"a\nb\"",
            "\"\u{1}\"",
            "\"\\x41\"",
            "\"\\u12\"",
            "\"\\u12G4\"",
            "[1] x",
            "{} {}",
            "[[[[[]]]]]",
            "[[[[1]]]] ",
        ] {
            // The last case is exactly at the limit and is allowed; every other one is refused.
            if bad == "[[[[1]]]] " {
                assert!(parse(bad).is_some());
                continue;
            }
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn nesting_past_the_limit_is_refused_without_recursing_deeply() {
        let deep = "[".repeat(10_000) + &"]".repeat(10_000);
        assert_eq!(parse(&deep), None);
    }
}
