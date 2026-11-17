//! The signed spoiler term-update feed for Bowser's built-in Spoiler Firewall (`feed/schema.md`,
//! docs/spoiler-firewall.md "Term-update feed").
//!
//! Why: the bundled term list cannot know mission names or late-game characters before launch, and a
//! Bowser release is slower than the news. A small data-only file, signed with an Ed25519 key whose public
//! half is compiled in, can ADD terms or RAISE a term's weight; it can never remove or loosen anything,
//! and it is never executed. The engine side is `SFW.lexicon.extend` (`engine/lexicon.js`), called by
//! `bowser-host.js` with the verified `lexicon` object that `spoiler::config_json` embeds as `"feed"`.
//!
//! Wire format (identical to the Chrome extension, `hosts/chrome/feed.js`): one JSON envelope
//! `{"format":1,"payload":"<base64 of the exact payload bytes>","sig":"<base64 Ed25519 signature>"}`; the
//! payload is UTF-8 JSON `{"format":1,"seq":N,"issued":D,"expires":D,"lexicon":{"version"?,"add"?,"raise"?}}`.
//! The signature covers the payload bytes as decoded from base64, so nothing is re-serialised before it is
//! checked. Checks, in order: size, envelope, signature, payload, schema, rollback. A rejected or
//! unreachable feed changes nothing: the last good feed (or the built-in list alone) keeps protecting.
//!
//! Policy decisions (also in the docs): an expired feed is still applied, since it can only add
//! protection, and the settings page says "out of date"; a lower `seq` than the stored one is a rollback
//! and is refused; the same `seq` is a no-op.
//!
//! Network: one anonymous HTTPS GET (the updater's WinHTTP code: no cookies, no referrer, 10 s timeouts,
//! 256 KiB cap, at most five HTTPS-only redirects) at startup and every 12 hours while running, off the UI
//! thread, only while `spoiler_feed` is on. The last good envelope is kept beside settings.txt as
//! `spoiler-feed.json` (no browsing data in it) and re-verified on every load.

use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use ed25519_compact::{PublicKey, Signature};

/// Where the feed is published (a commit to this path by the maintainer). 404 until then.
pub const FEED_URL: &str = "https://raw.githubusercontent.com/grantcharlie1000-tech/Bowser/main/extensions/spoiler-firewall/feed/published/feed.json";
/// Whole downloaded body, and the decoded payload.
pub const MAX_BODY_BYTES: usize = 256 * 1024;
pub const CHECK_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);
/// Waited before the first check so the first window is not competing with a network round trip.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(5);
pub const FILE_NAME: &str = "spoiler-feed.json";
/// `SFW.lexicon.limits.maxJson` / `maxTerms`: a feed past either would be refused by the engine.
const MAX_LEXICON_JSON: usize = 200_000;
const MAX_TERMS: usize = 5000;
const MAX_SEQ: f64 = 2_147_483_647.0;

/// Production verification keys, one base64 key per line (`#` comments), shared with the Chrome host.
const PUBLIC_KEY_TEXT: &str = include_str!("../extensions/spoiler-firewall/feed/public-key.txt");

const ADD_CATEGORIES: [&str; 9] = [
    "franchise", "entityStrong", "entityWeak", "cueStrong", "cueWeak", "marker", "game", "shapeVerb", "shapeEnd",
];
const RAISE_TARGETS: [&str; 6] = ["entityWeak", "entityStrong", "franchise", "cueWeak", "cueStrong", "marker"];

// ---------------------------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    Size,
    Envelope,
    Format,
    NoKey,
    Signature,
    Payload,
    Seq,
    Dates,
    Lexicon,
    Rollback,
}

impl Rejection {
    /// The same codes the Chrome host reports.
    pub fn code(self) -> &'static str {
        match self {
            Self::Size => "size",
            Self::Envelope => "envelope",
            Self::Format => "format",
            Self::NoKey => "nokey",
            Self::Signature => "signature",
            Self::Payload => "payload",
            Self::Seq => "seq",
            Self::Dates => "dates",
            Self::Lexicon => "lexicon",
            Self::Rollback => "rollback",
        }
    }
}

/// A feed that passed every check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    pub seq: u32,
    pub issued: String,
    pub expires: String,
    expires_ms: i64,
    /// The `lexicon` object, re-serialised compactly (what `config_json` embeds as `"feed"`).
    pub lexicon_json: String,
    pub terms: usize,
}

impl Feed {
    /// Past `expires` (a bare date lasts through that UTC day). Still applied; only the UI says so.
    pub fn is_expired(&self, now_ms: i64) -> bool {
        now_ms > self.expires_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    pub feed: Feed,
    /// This exact `seq` is already the highest seen: nothing to store.
    pub current: bool,
}

/// The compiled-in production keys (raw 32-byte Ed25519 public keys).
pub fn production_keys() -> Vec<[u8; 32]> {
    parse_keys(PUBLIC_KEY_TEXT)
}

pub fn parse_keys(text: &str) -> Vec<[u8; 32]> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(crate::blocker::json::base64_decode)
        .filter_map(|bytes| <[u8; 32]>::try_from(bytes.as_slice()).ok())
        .collect()
}

/// Verifies a downloaded body. `highest_seq` is the highest `seq` held (0 when none).
pub fn verify_envelope(body: &[u8], keys: &[[u8; 32]], highest_seq: u32) -> Result<Verified, Rejection> {
    if body.len() > MAX_BODY_BYTES {
        return Err(Rejection::Size);
    }
    let text = std::str::from_utf8(body).map_err(|_| Rejection::Envelope)?;
    let envelope = json::parse(text).ok_or(Rejection::Envelope)?;
    let object = envelope.as_object().ok_or(Rejection::Envelope)?;
    let payload_text = object.get("payload").and_then(Json::as_str).ok_or(Rejection::Envelope)?;
    let sig_text = object.get("sig").and_then(Json::as_str).ok_or(Rejection::Envelope)?;
    if object.get("format").and_then(Json::as_f64) != Some(1.0) {
        return Err(Rejection::Format);
    }
    let payload = strict_base64(payload_text).ok_or(Rejection::Envelope)?;
    let sig_bytes = strict_base64(sig_text).ok_or(Rejection::Envelope)?;
    if payload.len() > MAX_BODY_BYTES {
        return Err(Rejection::Envelope);
    }
    let signature = Signature::from_slice(&sig_bytes).map_err(|_| Rejection::Envelope)?;
    let keys: Vec<PublicKey> = keys.iter().filter_map(|key| PublicKey::from_slice(key).ok()).collect();
    if keys.is_empty() {
        return Err(Rejection::NoKey);
    }
    if !keys.iter().any(|key| key.verify(&payload, &signature).is_ok()) {
        return Err(Rejection::Signature);
    }
    // Only now is anything inside the payload looked at.
    let payload_text = std::str::from_utf8(&payload).map_err(|_| Rejection::Payload)?;
    let parsed = json::parse(payload_text).ok_or(Rejection::Payload)?;
    let feed = check_payload(&parsed)?;
    if feed.seq < highest_seq {
        return Err(Rejection::Rollback);
    }
    Ok(Verified { current: feed.seq == highest_seq, feed })
}

/// Standard padded base64, nothing else (no whitespace, no URL alphabet).
fn strict_base64(text: &str) -> Option<Vec<u8>> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=')) {
        return None;
    }
    crate::blocker::json::base64_decode(text)
}

fn check_payload(value: &Json) -> Result<Feed, Rejection> {
    let object = value.as_object().ok_or(Rejection::Payload)?;
    if object.get("format").and_then(Json::as_f64) != Some(1.0) {
        return Err(Rejection::Format);
    }
    let seq = match object.get("seq").and_then(Json::as_f64) {
        Some(n) if n.is_finite() && n.fract() == 0.0 && (1.0..=MAX_SEQ).contains(&n) => n as u32,
        _ => return Err(Rejection::Seq),
    };
    let issued = object.get("issued").and_then(Json::as_str).ok_or(Rejection::Dates)?;
    let expires = object.get("expires").and_then(Json::as_str).ok_or(Rejection::Dates)?;
    let expires_ms = match (parse_date(issued, false), parse_date(expires, true)) {
        (Some(a), Some(b)) if b >= a => b,
        _ => return Err(Rejection::Dates),
    };
    let lexicon = object.get("lexicon").ok_or(Rejection::Lexicon)?;
    let (lexicon_json, terms) = check_lexicon(lexicon).ok_or(Rejection::Lexicon)?;
    Ok(Feed {
        seq,
        issued: issued.to_owned(),
        expires: expires.to_owned(),
        expires_ms,
        lexicon_json,
        terms,
    })
}

/// Validates the `lexicon` object and returns it re-serialised, with the number of terms in it.
/// Same rules as `validLexicon` in `hosts/chrome/feed.js`.
fn check_lexicon(value: &Json) -> Option<(String, usize)> {
    let object = value.as_object()?;
    if object.keys().any(|key| !matches!(key.as_str(), "version" | "add" | "raise")) {
        return None;
    }
    let mut members: Vec<String> = Vec::new();
    let mut count = 0usize;
    if let Some(version) = object.get("version") {
        let version = version.as_str()?;
        if version.is_empty()
            || version.len() > 40
            || !version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '+' | '-'))
        {
            return None;
        }
        members.push(format!("\"version\":{}", crate::spoiler::json_string(version)));
    }
    if let Some(add) = object.get("add") {
        let add = add.as_object()?;
        let mut cats = Vec::new();
        for (category, list) in add.iter() {
            if !ADD_CATEGORIES.contains(&category.as_str()) {
                return None;
            }
            let terms: Vec<String> = list
                .as_array()?
                .iter()
                .map(|term| term.as_str().map(crate::spoiler::json_string))
                .collect::<Option<_>>()?;
            count += terms.len();
            cats.push(format!("{}:[{}]", crate::spoiler::json_string(category), terms.join(",")));
        }
        members.push(format!("\"add\":{{{}}}", cats.join(",")));
    }
    if let Some(raise) = object.get("raise") {
        let raise = raise.as_object()?;
        let mut pairs = Vec::new();
        for (term, target) in raise.iter() {
            let target = target.as_str()?;
            if !RAISE_TARGETS.contains(&target) {
                return None;
            }
            pairs.push(format!("{}:{}", crate::spoiler::json_string(term), crate::spoiler::json_string(target)));
        }
        count += pairs.len();
        members.push(format!("\"raise\":{{{}}}", pairs.join(",")));
    }
    if count > MAX_TERMS {
        return None;
    }
    let json = format!("{{{}}}", members.join(","));
    (json.encode_utf16().count() <= MAX_LEXICON_JSON).then_some((json, count))
}

// ---- dates -------------------------------------------------------------------------------------

/// `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM[:SS[.fff]](Z|+HH:MM)`, milliseconds since the Unix epoch. A bare date is
/// midnight UTC, or the last millisecond of that day when `end_of_day`. Calendar and clock ranges are checked
/// (the same rules as `parseDate` in feed.js).
pub fn parse_date(text: &str, end_of_day: bool) -> Option<i64> {
    let bytes = text.as_bytes();
    let digits = |from: usize, len: usize| -> Option<i64> {
        let part = bytes.get(from..from + len)?;
        part.iter().all(u8::is_ascii_digit).then(|| part.iter().fold(0i64, |n, d| n * 10 + i64::from(d - b'0')))
    };
    if bytes.len() < 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let (year, month, day) = (digits(0, 4)?, digits(5, 2)?, digits(8, 2)?);
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days_in_month = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month[(month - 1) as usize] {
        return None;
    }
    let midnight = days_from_civil(year, month, day) * 86_400_000;
    if bytes.len() == 10 {
        return Some(if end_of_day { midnight + 86_399_999 } else { midnight });
    }
    if bytes[10] != b'T' || bytes.len() < 17 || bytes[13] != b':' {
        return None;
    }
    let (hour, minute) = (digits(11, 2)?, digits(14, 2)?);
    let mut at = 16;
    let mut second = 0;
    let mut millis = 0;
    if bytes.get(at) == Some(&b':') {
        second = digits(at + 1, 2)?;
        at += 3;
        if bytes.get(at) == Some(&b'.') {
            let fraction: Vec<u8> = bytes[at + 1..].iter().copied().take_while(u8::is_ascii_digit).collect();
            if fraction.is_empty() || fraction.len() > 3 {
                return None;
            }
            millis = fraction.iter().fold(0i64, |n, d| n * 10 + i64::from(d - b'0')) * 10i64.pow(3 - fraction.len() as u32);
            at += 1 + fraction.len();
        }
    }
    let offset_ms = match text.get(at..)? {
        "Z" => 0,
        rest if rest.len() == 6 && (rest.starts_with('+') || rest.starts_with('-')) && rest.as_bytes()[3] == b':' => {
            let (oh, om) = (digits(at + 1, 2)?, digits(at + 4, 2)?);
            if oh > 23 || om > 59 {
                return None;
            }
            let magnitude = (oh * 60 + om) * 60_000;
            if rest.starts_with('-') { -magnitude } else { magnitude }
        }
        _ => return None,
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(midnight + ((hour * 60 + minute) * 60 + second) * 1000 + millis - offset_ms)
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// A small strict JSON parser (the blocker's `json.rs` is deliberately not validating)
// ---------------------------------------------------------------------------------------------

mod json {
    /// Containers nested deeper than this are refused (a feed is two or three levels deep).
    const MAX_DEPTH: usize = 12;

    #[derive(Debug, Clone, PartialEq)]
    pub enum Json {
        Null,
        Bool(bool),
        Num(f64),
        Str(String),
        Arr(Vec<Json>),
        Obj(Object),
    }

    /// Members in document order; a repeated key keeps the LAST value (what `JSON.parse` does).
    #[derive(Debug, Clone, PartialEq, Default)]
    pub struct Object(Vec<(String, Json)>);

    impl Object {
        pub fn get(&self, key: &str) -> Option<&Json> {
            self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
        }
        pub fn keys(&self) -> impl Iterator<Item = &String> {
            self.0.iter().map(|(k, _)| k)
        }
        pub fn iter(&self) -> impl Iterator<Item = (&String, &Json)> {
            self.0.iter().map(|(k, v)| (k, v))
        }
        fn insert(&mut self, key: String, value: Json) {
            match self.0.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => self.0.push((key, value)),
            }
        }
    }

    impl Json {
        pub fn as_object(&self) -> Option<&Object> {
            if let Self::Obj(o) = self { Some(o) } else { None }
        }
        pub fn as_array(&self) -> Option<&Vec<Json>> {
            if let Self::Arr(a) = self { Some(a) } else { None }
        }
        pub fn as_str(&self) -> Option<&str> {
            if let Self::Str(s) = self { Some(s) } else { None }
        }
        pub fn as_f64(&self) -> Option<f64> {
            if let Self::Num(n) = self { Some(*n) } else { None }
        }
    }

    /// The whole text must be exactly one JSON value (surrounding whitespace allowed).
    pub fn parse(text: &str) -> Option<Json> {
        let mut parser = Parser { bytes: text.as_bytes(), at: 0 };
        let value = parser.value(0)?;
        parser.skip_ws();
        (parser.at == parser.bytes.len()).then_some(value)
    }

    struct Parser<'a> {
        bytes: &'a [u8],
        at: usize,
    }

    impl Parser<'_> {
        fn skip_ws(&mut self) {
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
        fn literal(&mut self, word: &str, value: Json) -> Option<Json> {
            let end = self.at + word.len();
            (self.bytes.get(self.at..end)? == word.as_bytes()).then(|| {
                self.at = end;
                value
            })
        }
        fn value(&mut self, depth: usize) -> Option<Json> {
            if depth > MAX_DEPTH {
                return None;
            }
            self.skip_ws();
            match *self.bytes.get(self.at)? {
                b'{' => {
                    self.at += 1;
                    let mut object = Object::default();
                    self.skip_ws();
                    if self.eat(b'}') {
                        return Some(Json::Obj(object));
                    }
                    loop {
                        self.skip_ws();
                        let key = self.string()?;
                        self.skip_ws();
                        if !self.eat(b':') {
                            return None;
                        }
                        let value = self.value(depth + 1)?;
                        object.insert(key, value);
                        self.skip_ws();
                        if self.eat(b',') {
                            continue;
                        }
                        return self.eat(b'}').then_some(Json::Obj(object));
                    }
                }
                b'[' => {
                    self.at += 1;
                    let mut items = Vec::new();
                    self.skip_ws();
                    if self.eat(b']') {
                        return Some(Json::Arr(items));
                    }
                    loop {
                        items.push(self.value(depth + 1)?);
                        self.skip_ws();
                        if self.eat(b',') {
                            continue;
                        }
                        return self.eat(b']').then_some(Json::Arr(items));
                    }
                }
                b'"' => self.string().map(Json::Str),
                b't' => self.literal("true", Json::Bool(true)),
                b'f' => self.literal("false", Json::Bool(false)),
                b'n' => self.literal("null", Json::Null),
                b'-' | b'0'..=b'9' => self.number(),
                _ => None,
            }
        }
        fn number(&mut self) -> Option<Json> {
            let start = self.at;
            self.eat(b'-');
            match self.bytes.get(self.at)? {
                b'0' => self.at += 1,
                b'1'..=b'9' => {
                    while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                        self.at += 1;
                    }
                }
                _ => return None,
            }
            if self.eat(b'.') {
                let from = self.at;
                while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                    self.at += 1;
                }
                if self.at == from {
                    return None;
                }
            }
            if matches!(self.bytes.get(self.at), Some(b'e' | b'E')) {
                self.at += 1;
                if matches!(self.bytes.get(self.at), Some(b'+' | b'-')) {
                    self.at += 1;
                }
                let from = self.at;
                while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                    self.at += 1;
                }
                if self.at == from {
                    return None;
                }
            }
            std::str::from_utf8(&self.bytes[start..self.at]).ok()?.parse::<f64>().ok().map(Json::Num)
        }
        fn hex4(&mut self) -> Option<u32> {
            let digits = self.bytes.get(self.at..self.at + 4)?;
            let mut n = 0u32;
            for &d in digits {
                n = n * 16 + (d as char).to_digit(16)?;
            }
            self.at += 4;
            Some(n)
        }
        fn string(&mut self) -> Option<String> {
            if !self.eat(b'"') {
                return None;
            }
            let mut out = String::new();
            loop {
                let rest = self.bytes.get(self.at..)?;
                let run = rest.iter().position(|&b| b == b'"' || b == b'\\' || b < 0x20)?;
                out.push_str(std::str::from_utf8(&rest[..run]).ok()?);
                self.at += run;
                match self.bytes[self.at] {
                    b'"' => {
                        self.at += 1;
                        return Some(out);
                    }
                    b'\\' => {
                        self.at += 1;
                        let escape = *self.bytes.get(self.at)?;
                        self.at += 1;
                        match escape {
                            b'"' => out.push('"'),
                            b'\\' => out.push('\\'),
                            b'/' => out.push('/'),
                            b'b' => out.push('\u{8}'),
                            b'f' => out.push('\u{c}'),
                            b'n' => out.push('\n'),
                            b'r' => out.push('\r'),
                            b't' => out.push('\t'),
                            b'u' => {
                                let first = self.hex4()?;
                                let code = if (0xD800..0xDC00).contains(&first) {
                                    if !(self.eat(b'\\') && self.eat(b'u')) {
                                        return None;
                                    }
                                    let second = self.hex4()?;
                                    if !(0xDC00..0xE000).contains(&second) {
                                        return None;
                                    }
                                    0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                                } else {
                                    first
                                };
                                out.push(char::from_u32(code)?); // a lone low surrogate is not a char
                            }
                            _ => return None,
                        }
                    }
                    _ => return None, // a raw control character
                }
            }
        }
    }
}

use json::Json;

// ---------------------------------------------------------------------------------------------
// State: what is stored, what is applied, what happened last
// ---------------------------------------------------------------------------------------------

/// How the latest check ended (for the settings page).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Not checked yet this run (or updates are off).
    Idle,
    /// A feed was fetched and verified (`current`: it was already held).
    Ok { current: bool },
    /// The server has no feed yet (404): quiet, expected until the maintainer publishes.
    NotPublished,
    /// Offline, timeout, server error.
    Unavailable,
    /// Downloaded but refused: unsigned, altered, rolled back, malformed or oversize.
    Rejected(Rejection),
}

/// Why a download produced no body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchError {
    NotFound,
    TooLarge,
    Unavailable,
}

/// The verified feed held in memory and on disk, plus the last status. Pure (no network, no globals):
/// the worker thread and the tests drive it.
#[derive(Debug)]
pub struct Manager {
    path: PathBuf,
    keys: Vec<[u8; 32]>,
    feed: Option<Feed>,
    pub status: Status,
}

impl Manager {
    /// Loads and re-verifies the stored envelope, if any. An unreadable, unsigned or altered file is ignored
    /// (and left alone: the next good download replaces it).
    pub fn load(dir: &Path, keys: Vec<[u8; 32]>) -> Self {
        let path = dir.join(FILE_NAME);
        let feed = std::fs::metadata(&path)
            .ok()
            .filter(|meta| meta.len() <= MAX_BODY_BYTES as u64)
            .and_then(|_| std::fs::read(&path).ok())
            .and_then(|body| verify_envelope(&body, &keys, 0).ok())
            .map(|verified| verified.feed);
        Self { path, keys, feed, status: Status::Idle }
    }

    pub fn feed(&self) -> Option<&Feed> {
        self.feed.as_ref()
    }

    pub fn highest_seq(&self) -> u32 {
        self.feed.as_ref().map_or(0, |feed| feed.seq)
    }

    /// Applies one download result. Returns true when the applied feed changed (new terms to inject).
    pub fn handle(&mut self, result: Result<Vec<u8>, FetchError>) -> bool {
        let body = match result {
            Ok(body) => body,
            Err(FetchError::NotFound) => {
                self.status = Status::NotPublished;
                return false;
            }
            Err(FetchError::TooLarge) => {
                self.status = Status::Rejected(Rejection::Size);
                return false;
            }
            Err(FetchError::Unavailable) => {
                self.status = Status::Unavailable;
                return false;
            }
        };
        match verify_envelope(&body, &self.keys, self.highest_seq()) {
            Err(rejection) => {
                self.status = Status::Rejected(rejection);
                false
            }
            Ok(verified) if verified.current => {
                self.status = Status::Ok { current: true };
                false
            }
            Ok(verified) => {
                // Keep memory and disk in step: if the write fails the feed still applies this run.
                let _ = write_atomic(&self.path, &body);
                self.feed = Some(verified.feed);
                self.status = Status::Ok { current: false };
                true
            }
        }
    }

    /// The settings page line, worded like the Chrome popup: "Term list: built-in 2026.11.17-1 + update #3
    /// (2026-11-18). Term updates out of date."
    pub fn summary(&self, builtin: &str, enabled: bool, now_ms: i64) -> String {
        let mut text = format!("Term list: built-in {builtin}");
        if let Some(feed) = &self.feed {
            text.push_str(&format!(" + update #{} ({})", feed.seq, feed.issued.get(..10).unwrap_or("")));
        }
        let mut notes: Vec<&str> = Vec::new();
        if !enabled {
            notes.push("Term updates are off.");
        }
        if self.feed.as_ref().is_some_and(|feed| feed.is_expired(now_ms)) {
            notes.push("Term updates out of date.");
        }
        match self.status {
            Status::Rejected(_) => notes.push("Term update rejected."),
            Status::Unavailable if enabled => notes.push("Could not check for term updates."),
            Status::NotPublished if enabled && self.feed.is_none() => notes.push("No term update has been published yet."),
            _ => {}
        }
        if !notes.is_empty() {
            text.push_str(". ");
            text.push_str(&notes.join(" "));
        }
        text
    }
}

fn write_atomic(path: &Path, body: &[u8]) -> std::io::Result<()> {
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, body)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// The bundled lexicon's version (`SFW.lexicon.version`), read from the bundle source; "built-in" if absent.
pub fn builtin_lexicon_version(bundle: &str) -> &str {
    const MARK: &str = "SFW.lexicon = (function () {\n  const VERSION = \"";
    bundle
        .split_once(MARK)
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(version, _)| version)
        .filter(|version| !version.is_empty() && version.len() <= 40 && version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_')))
        .unwrap_or("built-in")
}

// ---------------------------------------------------------------------------------------------
// The running instance: one Manager behind a lock, one worker thread
// ---------------------------------------------------------------------------------------------

struct Shared {
    manager: Mutex<Option<Manager>>,
    wake: Mutex<Wake>,
    cv: Condvar,
    notifier: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

#[derive(Default)]
struct Wake {
    enabled: bool,
    check_now: bool,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| Shared {
        manager: Mutex::new(None),
        wake: Mutex::new(Wake::default()),
        cv: Condvar::new(),
        notifier: Mutex::new(None),
    })
}

/// Called (from the worker thread) after a NEW feed was applied, so the UI thread can re-register the
/// protection script in open tabs. The newest window registers itself here.
pub fn set_notifier(notify: impl Fn() + Send + Sync + 'static) {
    if let Ok(mut slot) = shared().notifier.lock() {
        *slot = Some(Box::new(notify));
    }
}

/// The verified `lexicon` object as JSON, for `spoiler::config_json`'s `"feed"` member. Applied whenever a
/// verified feed is held, whether or not downloading is switched on (a feed only adds protection).
pub fn current_lexicon_json() -> Option<String> {
    let guard = shared().manager.lock().ok()?;
    guard.as_ref()?.feed().map(|feed| feed.lexicon_json.clone())
}

/// Status line for the settings page.
pub fn summary() -> String {
    let builtin = builtin_lexicon_version(crate::spoiler::BUNDLE);
    let enabled = shared().wake.lock().map(|wake| wake.enabled).unwrap_or(true);
    match shared().manager.lock() {
        Ok(guard) => match guard.as_ref() {
            Some(manager) => manager.summary(builtin, enabled, now_ms()),
            None => format!("Term list: built-in {builtin}"),
        },
        Err(_) => format!("Term list: built-in {builtin}"),
    }
}

/// Loads the stored feed now (so the first document already carries it) and, unless `network` is false,
/// starts the worker: first check shortly after start, then every 12 hours, and immediately whenever
/// `set_enabled(true)` turns downloading on. Call once, before the first tab exists.
pub fn start(dir: &Path, enabled: bool, network: bool) {
    let s = shared();
    if let Ok(mut guard) = s.manager.lock() {
        if guard.is_some() {
            return;
        }
        *guard = Some(Manager::load(dir, production_keys()));
    }
    if let Ok(mut wake) = s.wake.lock() {
        wake.enabled = enabled;
    }
    if !network {
        return;
    }
    let _ = std::thread::Builder::new().name("bowser-spoiler-feed".into()).spawn(worker);
}

/// The `spoiler_feed` setting. Turning it on checks right away; turning it off stops downloads (a stored feed
/// stays in force).
pub fn set_enabled(enabled: bool) {
    let s = shared();
    if let Ok(mut wake) = s.wake.lock() {
        if enabled && !wake.enabled {
            wake.check_now = true;
        }
        wake.enabled = enabled;
    }
    s.cv.notify_all();
}

fn worker() {
    let s = shared();
    let mut due = std::time::Instant::now() + FIRST_CHECK_DELAY;
    loop {
        // Sleep until due, or until the switch wakes us.
        let mut wake = match s.wake.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        loop {
            let now = std::time::Instant::now();
            if wake.check_now || (wake.enabled && now >= due) {
                break;
            }
            let wait = if wake.enabled { due.saturating_duration_since(now) } else { CHECK_INTERVAL };
            wake = match s.cv.wait_timeout(wake, wait) {
                Ok((guard, _)) => guard,
                Err(_) => return,
            };
        }
        let enabled = wake.enabled;
        wake.check_now = false;
        drop(wake);
        due = std::time::Instant::now() + CHECK_INTERVAL;
        if !enabled {
            continue;
        }
        let result = fetch();
        let changed = match s.manager.lock() {
            Ok(mut guard) => guard.as_mut().is_some_and(|manager| manager.handle(result)),
            Err(_) => return,
        };
        if changed && let Ok(slot) = s.notifier.lock() && let Some(notify) = slot.as_ref() {
            notify();
        }
    }
}

/// One anonymous GET of [`FEED_URL`] through the updater's WinHTTP code.
fn fetch() -> Result<Vec<u8>, FetchError> {
    use crate::updater::{UpdateError, http};
    let url = crate::updater::validate_https_url(FEED_URL).map_err(|_| FetchError::Unavailable)?;
    let mut body = Vec::new();
    let mut sink = |chunk: &[u8]| -> Result<(), UpdateError> {
        body.extend_from_slice(chunk);
        Ok(())
    };
    match http::get(&url, MAX_BODY_BYTES as u64, &mut sink) {
        Ok(_) => Ok(body),
        Err(UpdateError::Oversize) => Err(FetchError::TooLarge),
        Err(UpdateError::Network(why)) if why == "HTTP status 404" => Err(FetchError::NotFound),
        Err(_) => Err(FetchError::Unavailable),
    }
}

#[cfg(test)]
mod tests;
