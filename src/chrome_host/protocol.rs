//! Bridge protocol between the HTML chrome page and the Rust host (see docs/html-chrome-protocol.md).
//!
//! Pure and dependency-free. `parse_command` accepts exactly one flat JSON object whose values are
//! strings, plain integers or booleans, and rejects everything else. The `ViewState` and `*_json`
//! helpers build the messages Rust posts to the page; every string is escaped so a value cannot
//! break out of its JSON literal, and theme colours are checked before they are emitted.

use std::fmt;

/// Largest raw message accepted from the page, in bytes.
pub const MAX_RAW_BYTES: usize = 16 * 1024;
/// Longest text field accepted from the page, in characters.
pub const MAX_TEXT_CHARS: usize = 2048;
/// Theme keys Rust may send to the page. Anything else is dropped.
pub const THEME_TOKENS: [&str; 10] = [
    "bg",
    "fill",
    "border",
    "focusBorder",
    "text",
    "placeholder",
    "completion",
    "selection",
    "hover",
    "progress",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavAction {
    Back,
    Forward,
    Reload,
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenTarget {
    Menu,
    Site,
    NewTab,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionAction {
    ToggleMaximize,
    SystemMenu,
}

/// A validated message from the chrome page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Ready,
    Nav(NavAction),
    Edit { seq: u32, text: String },
    Submit(String),
    Escape,
    Focus(bool),
    Open(OpenTarget),
    /// Left mousedown on an empty toolbar area: start a native window move.
    BeginDrag,
    Caption(CaptionAction),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    /// Raw message longer than `MAX_RAW_BYTES`.
    TooLarge,
    /// Not valid JSON within the accepted subset; the payload names the problem.
    Syntax(&'static str),
    /// A value that is a nested object, an array or null.
    UnsupportedValue(&'static str),
    DuplicateKey(String),
    UnknownKey(String),
    UnknownType(String),
    MissingField(&'static str),
    WrongType(&'static str),
    /// `v` is a number other than 1.
    UnsupportedVersion,
    /// Right type, but outside the allowed set or range.
    InvalidValue(&'static str),
    /// Text longer than `MAX_TEXT_CHARS` characters.
    TooLong(&'static str),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "message exceeds {MAX_RAW_BYTES} bytes"),
            Self::Syntax(what) => write!(f, "malformed message: {what}"),
            Self::UnsupportedValue(what) => write!(f, "unsupported value: {what}"),
            Self::DuplicateKey(key) => write!(f, "duplicate key {key:?}"),
            Self::UnknownKey(key) => write!(f, "unknown key {key:?}"),
            Self::UnknownType(kind) => write!(f, "unknown message type {kind:?}"),
            Self::MissingField(name) => write!(f, "missing field {name:?}"),
            Self::WrongType(name) => write!(f, "wrong type for field {name:?}"),
            Self::UnsupportedVersion => write!(f, "unsupported protocol version"),
            Self::InvalidValue(name) => write!(f, "invalid value for field {name:?}"),
            Self::TooLong(name) => write!(f, "field {name:?} exceeds {MAX_TEXT_CHARS} characters"),
        }
    }
}

impl std::error::Error for ProtocolError {}

/// Parses one page message. Pure: no I/O, no allocation beyond the returned value.
pub fn parse_command(raw: &str) -> Result<Command, ProtocolError> {
    if raw.len() > MAX_RAW_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let mut parser = Parser { src: raw, pos: 0 };
    let fields = parser.parse_object()?;
    parser.skip_ws();
    if parser.pos != raw.len() {
        return Err(ProtocolError::Syntax("trailing characters"));
    }
    build_command(&fields)
}

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Str(String),
    /// The literal as written, validated against the JSON number grammar.
    Number(String),
    Bool(bool),
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn rest(&self) -> &str {
        &self.src[self.pos..]
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, byte: u8, message: &'static str) -> Result<(), ProtocolError> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(ProtocolError::Syntax(message))
        }
    }

    fn parse_object(&mut self) -> Result<Vec<(String, Value)>, ProtocolError> {
        self.skip_ws();
        self.eat(b'{', "expected '{'")?;
        let mut fields: Vec<(String, Value)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(fields);
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(ProtocolError::Syntax("expected key"));
            }
            let key = self.parse_string()?;
            if fields.iter().any(|(existing, _)| *existing == key) {
                return Err(ProtocolError::DuplicateKey(key));
            }
            self.skip_ws();
            self.eat(b':', "expected ':'")?;
            self.skip_ws();
            let value = self.parse_value()?;
            fields.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(fields);
                }
                _ => return Err(ProtocolError::Syntax("expected ',' or '}'")),
            }
        }
    }

    fn parse_value(&mut self) -> Result<Value, ProtocolError> {
        match self.peek() {
            Some(b'"') => Ok(Value::Str(self.parse_string()?)),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') if self.rest().starts_with("null") => {
                Err(ProtocolError::UnsupportedValue("null"))
            }
            Some(b'{') => Err(ProtocolError::UnsupportedValue("nested object")),
            Some(b'[') => Err(ProtocolError::UnsupportedValue("array")),
            Some(b'-' | b'0'..=b'9') => Ok(Value::Number(self.parse_number()?)),
            _ => Err(ProtocolError::Syntax("expected value")),
        }
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, ProtocolError> {
        if self.rest().starts_with(word) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(ProtocolError::Syntax("invalid literal"))
        }
    }

    fn parse_number(&mut self) -> Result<String, ProtocolError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => self.skip_digits(),
            _ => return Err(ProtocolError::Syntax("invalid number")),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            self.require_digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            self.require_digits()?;
        }
        Ok(self.src[start..self.pos].to_string())
    }

    fn skip_digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
    }

    fn require_digits(&mut self) -> Result<(), ProtocolError> {
        if !matches!(self.peek(), Some(b'0'..=b'9')) {
            return Err(ProtocolError::Syntax("invalid number"));
        }
        self.skip_digits();
        Ok(())
    }

    fn parse_string(&mut self) -> Result<String, ProtocolError> {
        self.eat(b'"', "expected string")?;
        let mut out = String::new();
        loop {
            let Some(c) = self.rest().chars().next() else {
                return Err(ProtocolError::Syntax("unterminated string"));
            };
            self.pos += c.len_utf8();
            match c {
                '"' => return Ok(out),
                '\\' => out.push(self.parse_escape()?),
                c if (c as u32) < 0x20 => {
                    return Err(ProtocolError::Syntax("control character in string"));
                }
                c => out.push(c),
            }
        }
    }

    fn parse_escape(&mut self) -> Result<char, ProtocolError> {
        let Some(e) = self.rest().chars().next() else {
            return Err(ProtocolError::Syntax("unterminated string"));
        };
        self.pos += e.len_utf8();
        Ok(match e {
            '"' => '"',
            '\\' => '\\',
            '/' => '/',
            'b' => '\u{8}',
            'f' => '\u{c}',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'u' => return self.parse_unicode_escape(),
            _ => return Err(ProtocolError::Syntax("invalid escape")),
        })
    }

    fn parse_unicode_escape(&mut self) -> Result<char, ProtocolError> {
        let high = self.parse_hex4()?;
        let code = if (0xD800..=0xDBFF).contains(&high) {
            if !self.rest().starts_with("\\u") {
                return Err(ProtocolError::Syntax("unpaired surrogate"));
            }
            self.pos += 2;
            let low = self.parse_hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(ProtocolError::Syntax("unpaired surrogate"));
            }
            0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)
        } else if (0xDC00..=0xDFFF).contains(&high) {
            return Err(ProtocolError::Syntax("unpaired surrogate"));
        } else {
            high
        };
        char::from_u32(code).ok_or(ProtocolError::Syntax("unpaired surrogate"))
    }

    fn parse_hex4(&mut self) -> Result<u32, ProtocolError> {
        let src = self.src;
        let Some(digits) = src.as_bytes().get(self.pos..self.pos + 4) else {
            return Err(ProtocolError::Syntax("truncated \\u escape"));
        };
        let mut value = 0;
        for &byte in digits {
            let digit = (byte as char)
                .to_digit(16)
                .ok_or(ProtocolError::Syntax("invalid \\u escape"))?;
            value = value * 16 + digit;
        }
        self.pos += 4;
        Ok(value)
    }
}

fn field<'a>(fields: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
    fields
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

fn str_field<'a>(
    fields: &'a [(String, Value)],
    name: &'static str,
) -> Result<&'a str, ProtocolError> {
    match field(fields, name) {
        Some(Value::Str(text)) => Ok(text.as_str()),
        Some(_) => Err(ProtocolError::WrongType(name)),
        None => Err(ProtocolError::MissingField(name)),
    }
}

fn text_field(fields: &[(String, Value)]) -> Result<String, ProtocolError> {
    let text = str_field(fields, "text")?;
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(ProtocolError::TooLong("text"));
    }
    Ok(text.to_string())
}

fn bool_field(fields: &[(String, Value)], name: &'static str) -> Result<bool, ProtocolError> {
    match field(fields, name) {
        Some(Value::Bool(flag)) => Ok(*flag),
        Some(_) => Err(ProtocolError::WrongType(name)),
        None => Err(ProtocolError::MissingField(name)),
    }
}

/// `seq` must be a plain integer in `0..=u32::MAX`.
fn seq_field(fields: &[(String, Value)]) -> Result<u32, ProtocolError> {
    match field(fields, "seq") {
        Some(Value::Number(raw)) => {
            if !raw.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
                return Err(ProtocolError::WrongType("seq"));
            }
            let value: i128 = raw
                .parse()
                .map_err(|_| ProtocolError::InvalidValue("seq"))?;
            u32::try_from(value).map_err(|_| ProtocolError::InvalidValue("seq"))
        }
        Some(_) => Err(ProtocolError::WrongType("seq")),
        None => Err(ProtocolError::MissingField("seq")),
    }
}

fn build_command(fields: &[(String, Value)]) -> Result<Command, ProtocolError> {
    match field(fields, "v") {
        Some(Value::Number(raw)) if raw == "1" => {}
        Some(Value::Number(_)) => return Err(ProtocolError::UnsupportedVersion),
        Some(_) => return Err(ProtocolError::WrongType("v")),
        None => return Err(ProtocolError::MissingField("v")),
    }
    let kind = str_field(fields, "t")?;
    let allowed: &[&str] = match kind {
        "ready" | "escape" => &[],
        "nav" | "open" => &["a"],
        "edit" => &["seq", "text"],
        "submit" => &["text"],
        "focus" => &["on"],
        "drag" => &[],
        "caption" => &["a"],
        other => return Err(ProtocolError::UnknownType(other.to_string())),
    };
    if let Some((key, _)) = fields
        .iter()
        .find(|(key, _)| key != "v" && key != "t" && !allowed.contains(&key.as_str()))
    {
        return Err(ProtocolError::UnknownKey(key.clone()));
    }
    Ok(match kind {
        "ready" => Command::Ready,
        "escape" => Command::Escape,
        "nav" => Command::Nav(match str_field(fields, "a")? {
            "back" => NavAction::Back,
            "forward" => NavAction::Forward,
            "reload" => NavAction::Reload,
            "stop" => NavAction::Stop,
            _ => return Err(ProtocolError::InvalidValue("a")),
        }),
        "edit" => Command::Edit {
            seq: seq_field(fields)?,
            text: text_field(fields)?,
        },
        "submit" => Command::Submit(text_field(fields)?),
        "focus" => Command::Focus(bool_field(fields, "on")?),
        "drag" => Command::BeginDrag,
        "caption" => Command::Caption(match str_field(fields, "a")? {
            "toggleMaximize" => CaptionAction::ToggleMaximize,
            "systemMenu" => CaptionAction::SystemMenu,
            _ => return Err(ProtocolError::InvalidValue("a")),
        }),
        "open" => Command::Open(match str_field(fields, "a")? {
            "menu" => OpenTarget::Menu,
            "site" => OpenTarget::Site,
            "newtab" => OpenTarget::NewTab,
            _ => return Err(ProtocolError::InvalidValue("a")),
        }),
        _ => return Err(ProtocolError::UnknownType(kind.to_string())),
    })
}

/// Security summary shown in the omnibox; the wire strings are fixed by the contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    Secure,
    NotSecure,
    CertProblem,
    Internal,
    Unknown,
    None,
}

impl Security {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Secure => "secure",
            Self::NotSecure => "notsecure",
            Self::CertProblem => "certproblem",
            Self::Internal => "internal",
            Self::Unknown => "unknown",
            Self::None => "none",
        }
    }
}

/// Everything the page needs to paint one frame of the toolbar.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewState {
    /// Device pixel ratio hint. Non-finite or non-positive values fall back to 1.
    pub scale: f32,
    pub solid: bool,
    pub dark: bool,
    pub back: bool,
    pub forward: bool,
    pub loading: bool,
    /// Clamped to 0..=100 when serialised.
    pub progress: u8,
    pub security: Security,
    pub label: String,
    /// Colour tokens. Entries whose key is not in `THEME_TOKENS`, or whose value fails
    /// `is_safe_css_colour`, are dropped by `to_json`.
    pub theme: Vec<(&'static str, String)>,
}

impl ViewState {
    /// Serialises a `state` message. Output is one JSON object.
    pub fn to_json(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str("{\"t\":\"state\",\"scale\":");
        out.push_str(&format_scale(self.scale));
        for (name, value) in [
            ("solid", self.solid),
            ("dark", self.dark),
            ("back", self.back),
            ("forward", self.forward),
            ("loading", self.loading),
        ] {
            out.push_str(",\"");
            out.push_str(name);
            out.push_str("\":");
            out.push_str(if value { "true" } else { "false" });
        }
        out.push_str(",\"progress\":");
        out.push_str(&self.progress.min(100).to_string());
        out.push_str(",\"security\":");
        out.push_str(&json_string(self.security.as_str()));
        out.push_str(",\"label\":");
        out.push_str(&json_string(&self.label));
        out.push_str(",\"theme\":{");
        let mut written: Vec<&str> = Vec::new();
        for (token, value) in &self.theme {
            if !THEME_TOKENS.contains(token)
                || written.contains(token)
                || !is_safe_css_colour(value)
            {
                continue;
            }
            if !written.is_empty() {
                out.push(',');
            }
            written.push(*token);
            out.push_str(&json_string(token));
            out.push(':');
            out.push_str(&json_string(value));
        }
        out.push_str("}}");
        out
    }
}

/// `{"t":"setText","text":...,"select":...}`: Rust sets the omnibox text.
pub fn set_text_json(text: &str, select: bool) -> String {
    format!(
        "{{\"t\":\"setText\",\"text\":{},\"select\":{select}}}",
        json_string(text)
    )
}

/// `{"t":"focusOmnibox","select":...}`.
pub fn focus_json(select: bool) -> String {
    format!("{{\"t\":\"focusOmnibox\",\"select\":{select}}}")
}

/// `{"t":"completion","seq":...,"suffix":...}`: inline completion for edit `seq`.
pub fn completion_json(seq: u32, suffix: &str) -> String {
    format!(
        "{{\"t\":\"completion\",\"seq\":{seq},\"suffix\":{}}}",
        json_string(suffix)
    )
}

/// Accepts only `#` followed by 3, 4, 6 or 8 hex digits, or a lowercase `rgb(`/`rgba(` call whose
/// arguments are digits, commas, dots, spaces and percent signs. Everything else is rejected, so
/// the value cannot carry `;`, `}`, `(` nesting, `url(` or `expression(`.
pub fn is_safe_css_colour(value: &str) -> bool {
    if value.len() > 64 {
        return false;
    }
    if let Some(hex) = value.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    let args = value
        .strip_prefix("rgba(")
        .or_else(|| value.strip_prefix("rgb("))
        .and_then(|rest| rest.strip_suffix(')'));
    match args {
        Some(args) => {
            args.bytes().any(|b| b.is_ascii_digit())
                && args
                    .bytes()
                    .all(|b| b.is_ascii_digit() || matches!(b, b',' | b'.' | b' ' | b'%'))
        }
        None => false,
    }
}

fn format_scale(scale: f32) -> String {
    if scale.is_finite() && scale > 0.0 {
        format!("{scale}")
    } else {
        "1".to_string()
    }
}

/// Encodes `text` as a JSON string literal. Besides the mandatory escapes it writes `<` and the
/// two line separators as `\u` escapes, so the output is safe inside a script context too.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '<' | '\u{2028}' | '\u{2029}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(raw: &str) -> Command {
        parse_command(raw).unwrap_or_else(|e| panic!("{raw:?} rejected: {e}"))
    }

    fn err(raw: &str) -> ProtocolError {
        match parse_command(raw) {
            Ok(command) => panic!("{raw:?} accepted as {command:?}"),
            Err(error) => error,
        }
    }

    fn sample_state() -> ViewState {
        ViewState {
            scale: 1.0,
            solid: false,
            dark: true,
            back: false,
            forward: false,
            loading: false,
            progress: 0,
            security: Security::Secure,
            label: "Secure".to_string(),
            theme: Vec::new(),
        }
    }

    #[test]
    fn parses_ready_and_escape() {
        assert_eq!(ok(r#"{"v":1,"t":"ready"}"#), Command::Ready);
        assert_eq!(ok(r#"{"v":1,"t":"escape"}"#), Command::Escape);
    }

    #[test]
    fn parses_every_nav_action() {
        assert_eq!(
            ok(r#"{"v":1,"t":"nav","a":"back"}"#),
            Command::Nav(NavAction::Back)
        );
        assert_eq!(
            ok(r#"{"v":1,"t":"nav","a":"forward"}"#),
            Command::Nav(NavAction::Forward)
        );
        assert_eq!(
            ok(r#"{"v":1,"t":"nav","a":"reload"}"#),
            Command::Nav(NavAction::Reload)
        );
        assert_eq!(
            ok(r#"{"v":1,"t":"nav","a":"stop"}"#),
            Command::Nav(NavAction::Stop)
        );
    }

    #[test]
    fn parses_every_open_target() {
        assert_eq!(
            ok(r#"{"v":1,"t":"open","a":"menu"}"#),
            Command::Open(OpenTarget::Menu)
        );
        assert_eq!(
            ok(r#"{"v":1,"t":"open","a":"site"}"#),
            Command::Open(OpenTarget::Site)
        );
        assert_eq!(
            ok(r#"{"v":1,"t":"open","a":"newtab"}"#),
            Command::Open(OpenTarget::NewTab)
        );
    }

    #[test]
    fn parses_drag() {
        assert_eq!(ok(r#"{"v":1,"t":"drag"}"#), Command::BeginDrag);
        assert_eq!(ok(r#"{"t":"drag","v":1}"#), Command::BeginDrag);
    }

    #[test]
    fn parses_every_caption_action() {
        assert_eq!(
            ok(r#"{"v":1,"t":"caption","a":"toggleMaximize"}"#),
            Command::Caption(CaptionAction::ToggleMaximize)
        );
        assert_eq!(
            ok(r#"{"v":1,"t":"caption","a":"systemMenu"}"#),
            Command::Caption(CaptionAction::SystemMenu)
        );
    }

    #[test]
    fn rejects_unknown_caption_values() {
        for raw in [
            r#"{"v":1,"t":"caption","a":"close"}"#,
            r#"{"v":1,"t":"caption","a":"ToggleMaximize"}"#,
            r#"{"v":1,"t":"caption","a":"toggle_maximize"}"#,
            r#"{"v":1,"t":"caption","a":""}"#,
        ] {
            assert_eq!(err(raw), ProtocolError::InvalidValue("a"), "{raw}");
        }
    }

    #[test]
    fn caption_requires_string_action() {
        assert_eq!(
            err(r#"{"v":1,"t":"caption"}"#),
            ProtocolError::MissingField("a")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"caption","a":1}"#),
            ProtocolError::WrongType("a")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"caption","a":null}"#),
            ProtocolError::UnsupportedValue("null")
        );
    }

    #[test]
    fn drag_and_caption_reject_extra_keys() {
        assert_eq!(
            err(r#"{"v":1,"t":"drag","a":"systemMenu"}"#),
            ProtocolError::UnknownKey("a".to_string())
        );
        assert_eq!(
            err(r#"{"v":1,"t":"drag","x":0}"#),
            ProtocolError::UnknownKey("x".to_string())
        );
        assert_eq!(
            err(r#"{"v":1,"t":"caption","a":"systemMenu","x":0}"#),
            ProtocolError::UnknownKey("x".to_string())
        );
        assert_eq!(
            err(r#"{"v":1,"t":"caption","a":"systemMenu","text":"x"}"#),
            ProtocolError::UnknownKey("text".to_string())
        );
    }

    #[test]
    fn parses_focus_both_ways() {
        assert_eq!(ok(r#"{"v":1,"t":"focus","on":true}"#), Command::Focus(true));
        assert_eq!(
            ok(r#"{"v":1,"t":"focus","on":false}"#),
            Command::Focus(false)
        );
    }

    #[test]
    fn parses_edit_and_submit() {
        assert_eq!(
            ok(r#"{"v":1,"t":"edit","seq":0,"text":""}"#),
            Command::Edit {
                seq: 0,
                text: String::new()
            }
        );
        assert_eq!(
            ok(r#"{"v":1,"t":"edit","seq":7,"text":"rust lang"}"#),
            Command::Edit {
                seq: 7,
                text: "rust lang".to_string()
            }
        );
        assert_eq!(
            ok(r#"{"v":1,"t":"submit","text":"https://a.example/?q=\"x\"&y=\\z"}"#),
            Command::Submit("https://a.example/?q=\"x\"&y=\\z".to_string())
        );
    }

    #[test]
    fn seq_accepts_full_u32_range_only() {
        assert_eq!(
            ok(r#"{"v":1,"t":"edit","seq":4294967295,"text":"x"}"#),
            Command::Edit {
                seq: u32::MAX,
                text: "x".to_string()
            }
        );
        assert_eq!(
            err(r#"{"v":1,"t":"edit","seq":4294967296,"text":"x"}"#),
            ProtocolError::InvalidValue("seq")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"edit","seq":-1,"text":"x"}"#),
            ProtocolError::InvalidValue("seq")
        );
        assert_eq!(
            err(
                r#"{"v":1,"t":"edit","seq":99999999999999999999999999999999999999999999,"text":"x"}"#
            ),
            ProtocolError::InvalidValue("seq")
        );
    }

    #[test]
    fn seq_rejects_non_integers_and_wrong_types() {
        for raw in [
            r#"{"v":1,"t":"edit","seq":1.5,"text":""}"#,
            r#"{"v":1,"t":"edit","seq":1e2,"text":""}"#,
            r#"{"v":1,"t":"edit","seq":"1","text":""}"#,
            r#"{"v":1,"t":"edit","seq":true,"text":""}"#,
        ] {
            assert_eq!(err(raw), ProtocolError::WrongType("seq"), "{raw}");
        }
        assert_eq!(
            err(r#"{"v":1,"t":"edit","text":""}"#),
            ProtocolError::MissingField("seq")
        );
    }

    #[test]
    fn tolerates_whitespace_and_key_order() {
        assert_eq!(
            ok(" \t{ \"t\" : \"ready\" ,\r\n \"v\" : 1 }\n"),
            Command::Ready
        );
        assert_eq!(
            ok(r#"{"a":"back","t":"nav","v":1}"#),
            Command::Nav(NavAction::Back)
        );
    }

    #[test]
    fn rejects_unknown_nav_and_open_values() {
        assert_eq!(
            err(r#"{"v":1,"t":"nav","a":"home"}"#),
            ProtocolError::InvalidValue("a")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"nav","a":"Back"}"#),
            ProtocolError::InvalidValue("a")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"open","a":"devtools"}"#),
            ProtocolError::InvalidValue("a")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"nav"}"#),
            ProtocolError::MissingField("a")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"nav","a":3}"#),
            ProtocolError::WrongType("a")
        );
    }

    #[test]
    fn raw_size_boundary() {
        let prefix = r#"{"v":1,"t":"ready""#;
        let exact = format!("{prefix}{}}}", " ".repeat(MAX_RAW_BYTES - prefix.len() - 1));
        assert_eq!(exact.len(), MAX_RAW_BYTES);
        assert_eq!(ok(&exact), Command::Ready);

        let over = format!("{prefix}{}}}", " ".repeat(MAX_RAW_BYTES - prefix.len()));
        assert_eq!(over.len(), MAX_RAW_BYTES + 1);
        assert_eq!(err(&over), ProtocolError::TooLarge);
    }

    #[test]
    fn rejects_nested_object_and_array_values() {
        assert_eq!(
            err(r#"{"v":1,"t":"submit","text":{"a":"b"}}"#),
            ProtocolError::UnsupportedValue("nested object")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"submit","text":["a"]}"#),
            ProtocolError::UnsupportedValue("array")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"ready","x":{}}"#),
            ProtocolError::UnsupportedValue("nested object")
        );
    }

    #[test]
    fn rejects_top_level_non_object() {
        for raw in [
            r#"[{"v":1,"t":"ready"}]"#,
            r#""ready""#,
            "1",
            "null",
            "true",
            "",
        ] {
            assert!(matches!(err(raw), ProtocolError::Syntax(_)), "{raw}");
        }
    }

    #[test]
    fn rejects_null_values() {
        assert_eq!(
            err(r#"{"v":1,"t":"submit","text":null}"#),
            ProtocolError::UnsupportedValue("null")
        );
        assert!(matches!(
            err(r#"{"v":1,"t":"focus","on":nul}"#),
            ProtocolError::Syntax(_)
        ));
    }

    #[test]
    fn rejects_duplicate_keys() {
        assert_eq!(
            err(r#"{"v":1,"v":1,"t":"ready"}"#),
            ProtocolError::DuplicateKey("v".to_string())
        );
        assert_eq!(
            err(r#"{"v":1,"t":"ready","t":"escape"}"#),
            ProtocolError::DuplicateKey("t".to_string())
        );
        // An escaped key is the same key once decoded.
        assert_eq!(
            err(r#"{"v":1,"t":"ready","t":"escape"}"#),
            ProtocolError::DuplicateKey("t".to_string())
        );
    }

    #[test]
    fn rejects_unknown_keys_per_message_type() {
        assert_eq!(
            err(r#"{"v":1,"t":"ready","x":1}"#),
            ProtocolError::UnknownKey("x".to_string())
        );
        assert_eq!(
            err(r#"{"v":1,"t":"submit","text":"a","seq":1}"#),
            ProtocolError::UnknownKey("seq".to_string())
        );
        assert_eq!(
            err(r#"{"v":1,"t":"nav","a":"back","text":"a"}"#),
            ProtocolError::UnknownKey("text".to_string())
        );
        assert_eq!(
            err(r#"{"v":1,"t":"edit","seq":1,"text":"","on":true}"#),
            ProtocolError::UnknownKey("on".to_string())
        );
        assert_eq!(
            err(r#"{"v":1,"t":"focus","on":true,"a":"menu"}"#),
            ProtocolError::UnknownKey("a".to_string())
        );
    }

    #[test]
    fn rejects_unknown_message_type() {
        assert_eq!(
            err(r#"{"v":1,"t":"explode"}"#),
            ProtocolError::UnknownType("explode".to_string())
        );
        assert_eq!(
            err(r#"{"v":1,"t":"state"}"#),
            ProtocolError::UnknownType("state".to_string())
        );
        assert_eq!(
            err(r#"{"v":1,"t":"Ready"}"#),
            ProtocolError::UnknownType("Ready".to_string())
        );
    }

    #[test]
    fn enforces_version_one() {
        assert_eq!(
            err(r#"{"v":2,"t":"ready"}"#),
            ProtocolError::UnsupportedVersion
        );
        assert_eq!(
            err(r#"{"v":0,"t":"ready"}"#),
            ProtocolError::UnsupportedVersion
        );
        assert_eq!(
            err(r#"{"v":1.5,"t":"ready"}"#),
            ProtocolError::UnsupportedVersion
        );
        assert_eq!(
            err(r#"{"v":"1","t":"ready"}"#),
            ProtocolError::WrongType("v")
        );
        assert_eq!(
            err(r#"{"v":true,"t":"ready"}"#),
            ProtocolError::WrongType("v")
        );
        assert_eq!(err(r#"{"t":"ready"}"#), ProtocolError::MissingField("v"));
    }

    #[test]
    fn requires_string_type_tag() {
        assert_eq!(err(r#"{"v":1}"#), ProtocolError::MissingField("t"));
        assert_eq!(err(r#"{"v":1,"t":1}"#), ProtocolError::WrongType("t"));
        assert_eq!(
            err(r#"{"v":1,"t":null}"#),
            ProtocolError::UnsupportedValue("null")
        );
        assert_eq!(err("{}"), ProtocolError::MissingField("v"));
    }

    #[test]
    fn rejects_wrong_field_types() {
        assert_eq!(
            err(r#"{"v":1,"t":"focus","on":"yes"}"#),
            ProtocolError::WrongType("on")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"focus","on":1}"#),
            ProtocolError::WrongType("on")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"focus"}"#),
            ProtocolError::MissingField("on")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"submit","text":42}"#),
            ProtocolError::WrongType("text")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"submit"}"#),
            ProtocolError::MissingField("text")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"edit","seq":1}"#),
            ProtocolError::MissingField("text")
        );
    }

    #[test]
    fn text_limit_counts_characters() {
        let max_ascii = "a".repeat(MAX_TEXT_CHARS);
        assert_eq!(
            ok(&format!(r#"{{"v":1,"t":"submit","text":"{max_ascii}"}}"#)),
            Command::Submit(max_ascii.clone())
        );
        let too_long = "a".repeat(MAX_TEXT_CHARS + 1);
        assert_eq!(
            err(&format!(r#"{{"v":1,"t":"submit","text":"{too_long}"}}"#)),
            ProtocolError::TooLong("text")
        );

        // Multi-byte characters count once each, not per byte.
        let max_euro = "€".repeat(MAX_TEXT_CHARS);
        assert_eq!(
            ok(&format!(
                r#"{{"v":1,"t":"edit","seq":0,"text":"{max_euro}"}}"#
            )),
            Command::Edit {
                seq: 0,
                text: max_euro.clone()
            }
        );
        let too_long_euro = "€".repeat(MAX_TEXT_CHARS + 1);
        assert_eq!(
            err(&format!(
                r#"{{"v":1,"t":"edit","seq":0,"text":"{too_long_euro}"}}"#
            )),
            ProtocolError::TooLong("text")
        );
    }

    #[test]
    fn rejects_raw_control_characters_in_strings() {
        assert_eq!(
            err("{\"v\":1,\"t\":\"submit\",\"text\":\"a\u{1}b\"}"),
            ProtocolError::Syntax("control character in string")
        );
        assert_eq!(
            err("{\"v\":1,\"t\":\"submit\",\"text\":\"a\tb\"}"),
            ProtocolError::Syntax("control character in string")
        );
        assert_eq!(
            err("{\"v\":1,\"t\":\"submit\",\"text\":\"a\nb\"}"),
            ProtocolError::Syntax("control character in string")
        );
        assert_eq!(
            err("{\"v\":1,\"t\":\"submit\",\"text\":\"a\u{7}\"}"),
            ProtocolError::Syntax("control character in string")
        );
    }

    #[test]
    fn rejects_invalid_escapes() {
        for raw in [
            r#"{"v":1,"t":"submit","text":"\x41"}"#,
            r#"{"v":1,"t":"submit","text":"\'"}"#,
            r#"{"v":1,"t":"submit","text":"\u12G4"}"#,
            r#"{"v":1,"t":"submit","text":"\u12"}"#,
            r#"{"v":1,"t":"submit","text":"\ud800"}"#,
            r#"{"v":1,"t":"submit","text":"\ud800A"}"#,
            r#"{"v":1,"t":"submit","text":"\ud800x"}"#,
            r#"{"v":1,"t":"submit","text":"\udc00"}"#,
            r#"{"v":1,"t":"submit","text":"\ud83d\ud83d"}"#,
        ] {
            assert!(matches!(err(raw), ProtocolError::Syntax(_)), "{raw}");
        }
    }

    #[test]
    fn accepts_all_standard_escapes() {
        assert_eq!(
            ok(r#"{"v":1,"t":"submit","text":"\" \\ \/ \b \f \n \r \t"}"#),
            Command::Submit("\" \\ / \u{8} \u{c} \n \r \t".to_string())
        );
        assert_eq!(
            ok(r#"{"v":1,"t":"submit","text":"A\u0001\u001Fé"}"#),
            Command::Submit("A\u{1}\u{1f}\u{e9}".to_string())
        );
    }

    #[test]
    fn decodes_surrogate_pairs() {
        assert_eq!(
            ok(r#"{"v":1,"t":"submit","text":"😀"}"#),
            Command::Submit("😀".to_string())
        );
        assert_eq!(
            ok(r#"{"v":1,"t":"submit","text":"😀�"}"#),
            Command::Submit("😀\u{fffd}".to_string())
        );
    }

    #[test]
    fn passes_raw_unicode_through() {
        assert_eq!(
            ok("{\"v\":1,\"t\":\"submit\",\"text\":\"héllo ✓ 😀 日本\"}"),
            Command::Submit("héllo ✓ 😀 日本".to_string())
        );
    }

    #[test]
    fn rejects_unterminated_and_trailing_input() {
        assert_eq!(
            err(r#"{"v":1,"t":"submit","text":"abc"#),
            ProtocolError::Syntax("unterminated string")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"submit","text":"abc\"#),
            ProtocolError::Syntax("unterminated string")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"ready"} x"#),
            ProtocolError::Syntax("trailing characters")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"ready"}{"v":1,"t":"ready"}"#),
            ProtocolError::Syntax("trailing characters")
        );
    }

    #[test]
    fn rejects_structural_syntax_errors() {
        assert!(matches!(
            err(r#"{"v":1,"t":"ready",}"#),
            ProtocolError::Syntax(_)
        ));
        assert!(matches!(
            err(r#"{,"v":1,"t":"ready"}"#),
            ProtocolError::Syntax(_)
        ));
        assert!(matches!(
            err(r#"{"v" 1,"t":"ready"}"#),
            ProtocolError::Syntax(_)
        ));
        assert!(matches!(
            err(r#"{"v":1 "t":"ready"}"#),
            ProtocolError::Syntax(_)
        ));
        assert!(matches!(
            err(r#"{v:1,"t":"ready"}"#),
            ProtocolError::Syntax(_)
        ));
        assert!(matches!(
            err(r#"{"v":1,"t":"ready""#),
            ProtocolError::Syntax(_)
        ));
        assert!(matches!(err("   "), ProtocolError::Syntax(_)));
    }

    #[test]
    fn rejects_malformed_numbers_and_literals() {
        assert!(matches!(
            err(r#"{"v":01,"t":"ready"}"#),
            ProtocolError::Syntax(_)
        ));
        assert!(matches!(
            err(r#"{"v":1,"t":"edit","seq":-,"text":""}"#),
            ProtocolError::Syntax(_)
        ));
        assert!(matches!(
            err(r#"{"v":1,"t":"edit","seq":1.,"text":""}"#),
            ProtocolError::Syntax(_)
        ));
        assert!(matches!(
            err(r#"{"v":1,"t":"edit","seq":+1,"text":""}"#),
            ProtocolError::Syntax(_)
        ));
        assert!(matches!(
            err(r#"{"v":1,"t":"focus","on":tru}"#),
            ProtocolError::Syntax(_)
        ));
        assert!(matches!(
            err(r#"{"v":1,"t":"focus","on":True}"#),
            ProtocolError::Syntax(_)
        ));
    }

    #[test]
    fn json_string_round_trips_through_parser() {
        for text in [
            "",
            "plain",
            "a\"b\\c/d",
            "\u{0}\u{1f}\u{7f}",
            "line\nbreak\rtab\tbs\u{8}ff\u{c}",
            "<script>&</script>",
            "\u{2028}\u{2029}",
            "héllo ✓ 😀 日本",
            "\u{10ffff}",
        ] {
            let encoded = json_string(text);
            assert!(!encoded.bytes().any(|b| b < 0x20), "{encoded:?}");
            assert!(!encoded.contains('<'), "{encoded:?}");
            assert!(!encoded.contains('\u{2028}') && !encoded.contains('\u{2029}'));

            let mut parser = Parser {
                src: encoded.as_str(),
                pos: 0,
            };
            assert_eq!(parser.parse_string().unwrap(), text);
            assert_eq!(parser.pos, encoded.len());

            let raw = format!(r#"{{"v":1,"t":"submit","text":{encoded}}}"#);
            assert_eq!(ok(&raw), Command::Submit(text.to_string()));
        }
    }

    #[test]
    fn json_string_escapes_as_documented() {
        assert_eq!(json_string("a\"b\\c"), r#""a\"b\\c""#);
        assert_eq!(json_string("\n\r\t\u{8}\u{c}"), r#""\n\r\t\b\f""#);
        assert_eq!(json_string("\u{1}\u{1f}"), r#""\u0001\u001f""#);
        assert_eq!(json_string("<b>"), format!("\"{}u003cb>\"", '\\'));
        assert_eq!(
            json_string("\u{2028}\u{2029}"),
            format!("\"{b}u2028{b}u2029\"", b = '\\')
        );
        assert_eq!(json_string("é😀"), "\"é😀\"");
    }

    #[test]
    fn state_json_exact_output() {
        let state = ViewState {
            scale: 1.0,
            solid: true,
            dark: false,
            back: true,
            forward: false,
            loading: true,
            progress: 42,
            security: Security::Secure,
            label: "Secure".to_string(),
            theme: vec![
                ("bg", "#101010".to_string()),
                ("text", "rgba(1, 2, 3, 0.5)".to_string()),
            ],
        };
        assert_eq!(
            state.to_json(),
            r##"{"t":"state","scale":1,"solid":true,"dark":false,"back":true,"forward":false,"loading":true,"progress":42,"security":"secure","label":"Secure","theme":{"bg":"#101010","text":"rgba(1, 2, 3, 0.5)"}}"##
        );
    }

    #[test]
    fn state_json_with_empty_theme() {
        let json = sample_state().to_json();
        assert!(json.ends_with(r#""label":"Secure","theme":{}}"#), "{json}");
        assert!(json.contains(r#""dark":true"#));
    }

    #[test]
    fn progress_is_clamped_to_100() {
        let mut state = sample_state();
        state.progress = 250;
        assert!(state.to_json().contains(r#""progress":100,"#));
        state.progress = 100;
        assert!(state.to_json().contains(r#""progress":100,"#));
        state.progress = 0;
        assert!(state.to_json().contains(r#""progress":0,"#));
        state.progress = u8::MAX;
        assert!(state.to_json().contains(r#""progress":100,"#));
    }

    #[test]
    fn security_wire_strings() {
        assert_eq!(Security::Secure.as_str(), "secure");
        assert_eq!(Security::NotSecure.as_str(), "notsecure");
        assert_eq!(Security::CertProblem.as_str(), "certproblem");
        assert_eq!(Security::Internal.as_str(), "internal");
        assert_eq!(Security::Unknown.as_str(), "unknown");
        assert_eq!(Security::None.as_str(), "none");
    }

    #[test]
    fn state_label_is_escaped() {
        let mut state = sample_state();
        state.label = "</script>\"x\"".to_string();
        let json = state.to_json();
        let expected = format!("\"label\":\"{b}u003c/script>{b}\"x{b}\"\"", b = '\\');
        assert!(json.contains(&expected), "{json}");
        assert!(!json.contains("</script"));
    }

    #[test]
    fn scale_formats_as_json_number() {
        let mut state = sample_state();
        state.scale = 1.25;
        assert!(state.to_json().starts_with(r#"{"t":"state","scale":1.25,"#));
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, -2.0] {
            state.scale = bad;
            assert!(
                state.to_json().starts_with(r#"{"t":"state","scale":1,"#),
                "scale {bad}"
            );
        }
    }

    #[test]
    fn theme_drops_unknown_tokens_and_unsafe_values() {
        let mut state = sample_state();
        state.theme = vec![
            ("bg", "#fff".to_string()),
            ("evil", "#000".to_string()),
            ("text", "url(http://x)".to_string()),
            ("fill", "#fff;background:red".to_string()),
            ("border", "rgb(0,0,0)}</style>".to_string()),
            ("hover", "expression(alert(1))".to_string()),
            ("selection", "rgba(10 20 30 50%)".to_string()),
            ("bg", "#000".to_string()),
        ];
        let json = state.to_json();
        assert!(
            json.ends_with(r##""theme":{"bg":"#fff","selection":"rgba(10 20 30 50%)"}}"##),
            "{json}"
        );
    }

    #[test]
    fn theme_keeps_every_whitelisted_token() {
        let mut state = sample_state();
        state.theme = THEME_TOKENS
            .iter()
            .map(|token| (*token, "#abcdef".to_string()))
            .collect();
        let json = state.to_json();
        for token in THEME_TOKENS {
            assert!(
                json.contains(&format!(r##""{token}":"#abcdef""##)),
                "{token}"
            );
        }
    }

    #[test]
    fn css_colour_accepts_hex_and_rgb_forms() {
        for good in [
            "#fff",
            "#ffff",
            "#ffffff",
            "#ffffffff",
            "#AbC123",
            "rgb(1, 2, 3)",
            "rgb(1,2,3)",
            "rgba(0,0,0,0.5)",
            "rgba(10 20 30 50%)",
            "rgb(100%, 50%, 0%)",
        ] {
            assert!(is_safe_css_colour(good), "{good} should be safe");
        }
    }

    #[test]
    fn css_colour_rejects_everything_else() {
        for bad in [
            "",
            "#",
            "#ff",
            "#fffff",
            "#fffffff",
            "#ggg",
            "#fff ",
            " #fff",
            "red",
            "url(x)",
            "rgb()",
            "rgb( )",
            "rgb(a,b,c)",
            "rgb(1,2,3",
            "rgb(1,2,3);",
            "rgb(0,0,0)}",
            "rgb(0,0,0)rgb(1,1,1)",
            "rgb(1,2,3)\n",
            "RGB(1,2,3)",
            "rgb(1/2/3)",
            "hsl(0, 0%, 0%)",
            "expression(alert(1))",
            "#fff; background:url(x)",
            "rgba(1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7)",
        ] {
            assert!(!is_safe_css_colour(bad), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn set_text_focus_and_completion_messages() {
        assert_eq!(
            set_text_json("a\"b", true),
            r#"{"t":"setText","text":"a\"b","select":true}"#
        );
        assert_eq!(
            set_text_json("<x>", false),
            format!(
                r#"{{"t":"setText","text":"{}u003cx>","select":false}}"#,
                '\\'
            )
        );
        assert_eq!(focus_json(true), r#"{"t":"focusOmnibox","select":true}"#);
        assert_eq!(focus_json(false), r#"{"t":"focusOmnibox","select":false}"#);
        assert_eq!(
            completion_json(9, "ple.com/"),
            r#"{"t":"completion","seq":9,"suffix":"ple.com/"}"#
        );
        assert_eq!(
            completion_json(u32::MAX, "\u{2028}"),
            format!(
                r#"{{"t":"completion","seq":4294967295,"suffix":"{}u2028"}}"#,
                '\\'
            )
        );
    }
}
