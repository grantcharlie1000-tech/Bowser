//! Bridge protocol between the HTML chrome page and the Rust host (see docs/html-chrome-protocol.md).
//!
//! Pure and dependency-free. `parse_command` accepts exactly one flat JSON object whose values are
//! strings, plain integers or booleans, and rejects everything else. The `ViewState` and `*_json`
//! helpers build the messages Rust posts to the page; every string is escaped so a value cannot
//! break out of its JSON literal, and theme colours are checked before they are emitted.

use std::{fmt, time::{Duration, Instant}};

/// Largest raw message accepted from the page, in bytes.
pub const MAX_RAW_BYTES: usize = 16 * 1024;
/// Longest text field accepted from the page, in characters.
pub const MAX_TEXT_CHARS: usize = 2048;
/// Most keys one page message may carry (`v` and `t` included). The largest message type uses four
/// more; a seventh key is rejected before its duplicate check runs.
pub const MAX_FIELDS: usize = 6;
/// Most tabs in one `tabs` message. The active tab is always among them.
pub const MAX_TABS: usize = 512;
/// Largest favicon side, in pixels, read from the PNG header.
pub const MAX_FAVICON_DIMENSION: u32 = 256;
/// Sustained page messages per second accepted by one host, and the burst it may send at once.
pub const MESSAGE_RATE_PER_SEC: u32 = 120;
pub const MESSAGE_BURST: u32 = 60;
/// Least time between two menu, caption or drag commands from one host.
pub const POPUP_COMMAND_INTERVAL: Duration = Duration::from_millis(250);
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
    /// Toggles split view, the same action as the Ctrl+Shift+\ shortcut.
    Split,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionAction {
    ToggleMaximize,
    SystemMenu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAction {
    Select,
    Close,
    /// Rust shows the native tab context menu at the cursor.
    Menu,
    Pin,
    Mute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupAction {
    /// Collapse or expand the group.
    Toggle,
    Menu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RailAction {
    /// Left click: what a native click on the button does.
    Click,
    /// Right click: Rust shows the native rail context menu at the cursor.
    Menu,
}

/// A rail button key as it travels on the wire. Grammar: `home`, `customize`, `addws`, `addapp`,
/// `bookmarks`, `history`, `menu`, `ws:<digits>` (at most 20 digits, a u64) and `app:<digits>` (at
/// most 10 digits, a u32). Digits are canonical decimal: no sign, and no leading zero unless the
/// number is `0`, so each key has exactly one spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RailKeyWire {
    Home,
    Customize,
    AddWorkspace,
    AddApp,
    Bookmarks,
    History,
    Menu,
    Workspace(u64),
    App(u32),
}

/// Longest rail key on the wire, in bytes.
pub const MAX_RAIL_KEY_CHARS: usize = 24;
/// Most rail items in one `rail` message. Items past this are not sent.
pub const MAX_RAIL_ITEMS: usize = 128;
/// Longest rail item label, in characters.
pub const MAX_RAIL_LABEL_CHARS: usize = 64;

impl RailKeyWire {
    /// Parses a key of the grammar above. `None` for anything else.
    pub fn parse(raw: &str) -> Option<Self> {
        if raw.len() > MAX_RAIL_KEY_CHARS {
            return None;
        }
        Some(match raw {
            "home" => Self::Home,
            "customize" => Self::Customize,
            "addws" => Self::AddWorkspace,
            "addapp" => Self::AddApp,
            "bookmarks" => Self::Bookmarks,
            "history" => Self::History,
            "menu" => Self::Menu,
            _ => {
                if let Some(digits) = raw.strip_prefix("ws:") {
                    Self::Workspace(canonical_decimal(digits, 20)?)
                } else if let Some(digits) = raw.strip_prefix("app:") {
                    Self::App(u32::try_from(canonical_decimal(digits, 10)?).ok()?)
                } else {
                    return None;
                }
            }
        })
    }

    /// The wire spelling of the key. `parse(to_wire())` returns the same key.
    pub fn to_wire(self) -> String {
        match self {
            Self::Home => "home".to_owned(),
            Self::Customize => "customize".to_owned(),
            Self::AddWorkspace => "addws".to_owned(),
            Self::AddApp => "addapp".to_owned(),
            Self::Bookmarks => "bookmarks".to_owned(),
            Self::History => "history".to_owned(),
            Self::Menu => "menu".to_owned(),
            Self::Workspace(id) => format!("ws:{id}"),
            Self::App(id) => format!("app:{id}"),
        }
    }
}

/// Digits only, at most `max_len` of them, canonical (no leading zero unless the value is `0`).
fn canonical_decimal(digits: &str, max_len: usize) -> Option<u64> {
    if digits.is_empty() || digits.len() > max_len || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return None;
    }
    digits.parse().ok()
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
    Tab { action: TabAction, id: u32 },
    /// Drag-reorder drop: move tab `id` before tab `before` (`0` = end of strip).
    TabMove { id: u32, before: u32 },
    Group { action: GroupAction, key: u32 },
    /// A click or right click on a rail button.
    Rail { action: RailAction, key: RailKeyWire },
    /// Drag-reorder drop of a workspace or app: before `before`, or at the end of its section when
    /// `None`. `key` is always a `ws:` or `app:` key.
    RailMove { key: RailKeyWire, before: Option<RailKeyWire> },
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
    /// More than `MAX_FIELDS` keys in one object.
    TooManyFields,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "message exceeds {MAX_RAW_BYTES} bytes"),
            Self::TooManyFields => write!(f, "message has more than {MAX_FIELDS} fields"),
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
            // Checked before the key is read, so the duplicate scan below never sees a long object.
            if fields.len() == MAX_FIELDS {
                return Err(ProtocolError::TooManyFields);
            }
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

/// A plain integer in `0..=u32::MAX`, read from field `name`.
fn uint_field(fields: &[(String, Value)], name: &'static str) -> Result<u32, ProtocolError> {
    match field(fields, name) {
        Some(Value::Number(raw)) => {
            if !raw.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
                return Err(ProtocolError::WrongType(name));
            }
            let value: i128 = raw
                .parse()
                .map_err(|_| ProtocolError::InvalidValue(name))?;
            u32::try_from(value).map_err(|_| ProtocolError::InvalidValue(name))
        }
        Some(_) => Err(ProtocolError::WrongType(name)),
        None => Err(ProtocolError::MissingField(name)),
    }
}

/// `seq` must be a plain integer in `0..=u32::MAX`.
fn seq_field(fields: &[(String, Value)]) -> Result<u32, ProtocolError> {
    uint_field(fields, "seq")
}

/// A rail key string in field `name`, which must match the key grammar.
fn rail_key_field(
    fields: &[(String, Value)],
    name: &'static str,
) -> Result<RailKeyWire, ProtocolError> {
    RailKeyWire::parse(str_field(fields, name)?).ok_or(ProtocolError::InvalidValue(name))
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
        "tab" => &["a", "id"],
        "tabmove" => &["id", "before"],
        "group" => &["a", "key"],
        "rail" => &["a", "k"],
        "railmove" => &["k", "before"],
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
            "split" => OpenTarget::Split,
            _ => return Err(ProtocolError::InvalidValue("a")),
        }),
        "tab" => Command::Tab {
            action: match str_field(fields, "a")? {
                "select" => TabAction::Select,
                "close" => TabAction::Close,
                "menu" => TabAction::Menu,
                "pin" => TabAction::Pin,
                "mute" => TabAction::Mute,
                _ => return Err(ProtocolError::InvalidValue("a")),
            },
            id: uint_field(fields, "id")?,
        },
        "tabmove" => Command::TabMove {
            id: uint_field(fields, "id")?,
            before: uint_field(fields, "before")?,
        },
        "group" => Command::Group {
            action: match str_field(fields, "a")? {
                "toggle" => GroupAction::Toggle,
                "menu" => GroupAction::Menu,
                _ => return Err(ProtocolError::InvalidValue("a")),
            },
            key: uint_field(fields, "key")?,
        },
        "rail" => Command::Rail {
            action: match str_field(fields, "a")? {
                "click" => RailAction::Click,
                "menu" => RailAction::Menu,
                _ => return Err(ProtocolError::InvalidValue("a")),
            },
            key: rail_key_field(fields, "k")?,
        },
        "railmove" => {
            let key = rail_key_field(fields, "k")?;
            if !matches!(key, RailKeyWire::Workspace(_) | RailKeyWire::App(_)) {
                return Err(ProtocolError::InvalidValue("k"));
            }
            let before = match str_field(fields, "before")? {
                "" => None,
                raw => Some(RailKeyWire::parse(raw).ok_or(ProtocolError::InvalidValue("before"))?),
            };
            Command::RailMove { key, before }
        }
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
        json_string(truncate_utf16(text, MAX_TEXT_CHARS))
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

/// Longest tab title sent to the page, in characters.
pub const MAX_TAB_TITLE_CHARS: usize = 200;
/// Longest group name sent to the page, in characters.
pub const MAX_GROUP_NAME_CHARS: usize = 64;
/// Largest favicon PNG sent to the page, in bytes.
pub const MAX_FAVICON_BYTES: usize = 16 * 1024;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// One tab as the tab strip shows it. The group fields are sent only for a group leader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabItem {
    pub id: u32,
    pub title: String,
    pub loading: bool,
    pub pinned: bool,
    pub muted: bool,
    /// Marker id; `-1` for none.
    pub marker: i32,
    /// Group key; `0` for none.
    pub group: u32,
    /// True for the first tab of a group, the one that carries the group fields.
    pub leader: bool,
    pub group_name: String,
    /// Colour id; `-1` for the accent colour.
    pub group_colour: i32,
    pub collapsed: bool,
    pub group_size: u32,
    /// A favicon is cached for this tab id.
    pub has_favicon: bool,
}

/// `{"v":1,"t":"tabs","active":...,"items":[...]}`: the whole tab strip. Titles are capped at
/// `MAX_TAB_TITLE_CHARS` and group names at `MAX_GROUP_NAME_CHARS`, both on character boundaries.
/// At most `MAX_TABS` items are sent, in their order: the first ones, with the active tab kept in
/// place of the last one when it lies beyond them.
pub fn tabs_json(active: u32, items: &[TabItem]) -> String {
    let mut out = format!("{{\"v\":1,\"t\":\"tabs\",\"active\":{active},\"items\":[");
    for (index, item) in capped_items(active, items).into_iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"id\":{},\"title\":{},\"loading\":{},\"pinned\":{},\"muted\":{},\"marker\":{},\"g\":{},\"gl\":{}",
            item.id,
            json_string(truncate_chars(&item.title, MAX_TAB_TITLE_CHARS)),
            item.loading,
            item.pinned,
            item.muted,
            item.marker,
            item.group,
            item.leader,
        ));
        if item.leader {
            out.push_str(&format!(
                ",\"gname\":{},\"gcol\":{},\"gcollapsed\":{},\"gsize\":{}",
                json_string(truncate_chars(&item.group_name, MAX_GROUP_NAME_CHARS)),
                item.group_colour,
                item.collapsed,
                item.group_size,
            ));
        }
        out.push_str(&format!(",\"fav\":{}}}", item.has_favicon));
    }
    out.push_str("]}");
    out
}

/// The items `tabs_json` sends: all of them up to `MAX_TABS`. Beyond that, the first `MAX_TABS` are
/// kept, except that the active tab replaces the last kept one when it lies further on. Order holds.
fn capped_items(active: u32, items: &[TabItem]) -> Vec<&TabItem> {
    if items.len() <= MAX_TABS {
        return items.iter().collect();
    }
    let mut keep: Vec<usize> = (0..MAX_TABS).collect();
    if let Some(position) = items.iter().position(|item| item.id == active)
        && position >= MAX_TABS
    {
        keep[MAX_TABS - 1] = position;
    }
    keep.into_iter().map(|index| &items[index]).collect()
}

/// `{"v":1,"t":"fav","id":...,"uri":"data:image/png;base64,..."}` for a favicon. `None` unless
/// `png` starts with the PNG signature, is at most `MAX_FAVICON_BYTES` long and its IHDR header
/// gives a width and height from 1 to `MAX_FAVICON_DIMENSION`.
pub fn fav_json(id: u32, png: &[u8]) -> Option<String> {
    if !png_is_sendable(png) {
        return None;
    }
    Some(format!(
        "{{\"v\":1,\"t\":\"fav\",\"id\":{id},\"uri\":\"data:image/png;base64,{}\"}}",
        base64_encode(png)
    ))
}

/// The favicon gate shared by `fav` and `railfav`: a PNG of at most `MAX_FAVICON_BYTES` whose IHDR
/// size is in range.
fn png_is_sendable(png: &[u8]) -> bool {
    png.len() <= MAX_FAVICON_BYTES && png.starts_with(&PNG_SIGNATURE) && png_size_ok(png)
}

/// One left rail button as the rail view shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RailItem {
    /// Wire key (see [`RailKeyWire`]).
    pub key: String,
    /// Icon name: `home`, `customize`, `plus`, `heart`, `clock`, `dots`, `apptile`, `letter`, or
    /// `g:` followed by a workspace glyph name.
    pub icon: String,
    pub label: String,
    /// The current workspace, or the open panel of its kind.
    pub active: bool,
    pub dot: bool,
    /// CSS colour from the safe set, or `""` for the theme accent.
    pub accent: String,
    /// A favicon is cached for this key (`railfav` follows).
    pub has_favicon: bool,
}

/// `{"v":1,"t":"rail","items":[...]}`: the whole rail, in order. At most `MAX_RAIL_ITEMS` items are
/// sent. Labels are capped at `MAX_RAIL_LABEL_CHARS` characters; an accent that is not a safe CSS
/// colour is sent as `""`.
pub fn rail_json(items: &[RailItem]) -> String {
    let mut out = String::from("{\"v\":1,\"t\":\"rail\",\"items\":[");
    for (index, item) in items.iter().take(MAX_RAIL_ITEMS).enumerate() {
        if index > 0 {
            out.push(',');
        }
        let accent = if is_safe_css_colour(&item.accent) { item.accent.as_str() } else { "" };
        out.push_str(&format!(
            "{{\"k\":{},\"icon\":{},\"label\":{},\"active\":{},\"dot\":{},\"accent\":{},\"fav\":{}}}",
            json_string(&item.key),
            json_string(&item.icon),
            json_string(truncate_chars(&item.label, MAX_RAIL_LABEL_CHARS)),
            item.active,
            item.dot,
            json_string(accent),
            item.has_favicon,
        ));
    }
    out.push_str("]}");
    out
}

/// `{"v":1,"t":"railfav","k":...,"uri":"data:image/png;base64,..."}` for a rail button's favicon.
/// `None` under the same gate as `fav_json`.
pub fn railfav_json(key: &str, png: &[u8]) -> Option<String> {
    if !png_is_sendable(png) {
        return None;
    }
    Some(format!(
        "{{\"v\":1,\"t\":\"railfav\",\"k\":{},\"uri\":\"data:image/png;base64,{}\"}}",
        json_string(key),
        base64_encode(png)
    ))
}

/// Reads the IHDR chunk, which must come first: its type at bytes 12..16, then the big-endian width
/// and height at 16..24. Both must be within `1..=MAX_FAVICON_DIMENSION`.
fn png_size_ok(png: &[u8]) -> bool {
    if png.len() < 24 || &png[12..16] != b"IHDR" {
        return false;
    }
    let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    let sides = 1..=MAX_FAVICON_DIMENSION;
    sides.contains(&width) && sides.contains(&height)
}

/// Commands that open a native menu or popup, or move or caption the window. A host lets at most
/// one of these through per `POPUP_COMMAND_INTERVAL`.
pub fn is_popup_command(command: &Command) -> bool {
    matches!(
        command,
        Command::BeginDrag
            | Command::Caption(_)
            | Command::Open(OpenTarget::Menu | OpenTarget::Site | OpenTarget::Split)
            | Command::Tab { action: TabAction::Menu, .. }
            | Command::Group { action: GroupAction::Menu, .. }
            | Command::Rail { action: RailAction::Menu, .. }
    )
}

/// Token bucket bounding how fast one host may send messages: each message takes one token, and
/// tokens refill at `per_second` up to `burst`. Time is passed in, so the bucket has no clock of its
/// own. A new bucket starts full.
#[derive(Clone, Debug, PartialEq)]
pub struct TokenBucket {
    burst: f64,
    per_second: f64,
    tokens: f64,
    last: Option<Instant>,
}

impl TokenBucket {
    pub fn new(burst: u32, per_second: u32) -> Self {
        Self {
            burst: f64::from(burst),
            per_second: f64::from(per_second),
            tokens: f64::from(burst),
            last: None,
        }
    }

    /// Takes one token at `now`. False means the message is over the limit and must be dropped.
    pub fn take(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last {
            let elapsed = now.saturating_duration_since(last).as_secs_f64();
            self.tokens = (self.tokens + elapsed * self.per_second).min(self.burst);
        }
        self.last = Some(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Lets one event through per `interval`. Events inside the interval are dropped and do not restart
/// it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Throttle {
    interval: Duration,
    last: Option<Instant>,
}

impl Throttle {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: None,
        }
    }

    /// True when the event at `now` may pass; it then starts the interval.
    pub fn allow(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last
            && now.saturating_duration_since(last) < self.interval
        {
            return false;
        }
        self.last = Some(now);
        true
    }
}

/// The first `max` characters of `text`, cut on a character boundary.
fn truncate_chars(text: &str, max: usize) -> &str {
    match text.char_indices().nth(max) {
        Some((index, _)) => &text[..index],
        None => text,
    }
}

/// Truncates to UTF-16 code units, matching JavaScript string length.
fn truncate_utf16(text: &str, max: usize) -> &str {
    let mut units = 0;
    for (index, ch) in text.char_indices() {
        if units + ch.len_utf16() > max {
            return &text[..index];
        }
        units += ch.len_utf16();
    }
    text
}

/// Standard base64 (RFC 4648 section 4) with `=` padding.
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(char::from(ALPHABET[((n >> 18) & 63) as usize]));
        out.push(char::from(ALPHABET[((n >> 12) & 63) as usize]));
        out.push(if chunk.len() > 1 {
            char::from(ALPHABET[((n >> 6) & 63) as usize])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(ALPHABET[(n & 63) as usize])
        } else {
            '='
        });
    }
    out
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
    fn rust_messages_match_committed_fixture() {
        let mut leader = plain_tab(7, "Group <one>");
        leader.group = 42;
        leader.leader = true;
        leader.group_name = "Work & Play".to_owned();
        leader.group_colour = 2;
        leader.group_size = 2;
        leader.has_favicon = true;
        let mut member = plain_tab(8, "Second 😀");
        member.group = 42;
        member.collapsed = true;
        let png = png_with_size(16, 16, 24);
        let mut state = sample_state();
        state.scale = 1.25;
        state.solid = true;
        state.theme = vec![("bg", "#123456".to_owned()), ("text", "#fff".to_owned())];
        let rail = [rail_item("home", "Home", ""), rail_item("app:5", "Mail & More", "#abc")];
        let content = format!(
            "{{\"tabs\":{},\"fav\":{},\"rail\":{},\"railfav\":{},\"state\":{},\"setText\":{},\"completion\":{}}}\n",
            tabs_json(7, &[leader, member]),
            fav_json(7, &png).expect("fixture PNG"),
            rail_json(&rail),
            railfav_json("app:5", &png).expect("fixture PNG"),
            state.to_json(),
            set_text_json("https://example.test/😀", false),
            completion_json(9, ".test/"),
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("ui/test/fixtures/rust-messages.json");
        if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some_and(|value| value == "1") {
            std::fs::create_dir_all(path.parent().expect("fixtures directory")).expect("create fixtures directory");
            std::fs::write(&path, content).expect("write Rust messages fixture");
        } else {
            assert_eq!(std::fs::read_to_string(path).expect("committed Rust messages fixture"), content);
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
        assert_eq!(
            ok(r#"{"v":1,"t":"open","a":"split"}"#),
            Command::Open(OpenTarget::Split)
        );
    }

    #[test]
    fn open_split_is_exact_and_strict() {
        for raw in [
            r#"{"v":1,"t":"open","a":"Split"}"#,
            r#"{"v":1,"t":"open","a":"split "}"#,
            r#"{"v":1,"t":"open","a":"toggleSplit"}"#,
            r#"{"v":1,"t":"open","a":"splitview"}"#,
            r#"{"v":1,"t":"open","a":""}"#,
        ] {
            assert_eq!(err(raw), ProtocolError::InvalidValue("a"), "{raw}");
        }
        assert_eq!(
            err(r#"{"v":1,"t":"open","a":null}"#),
            ProtocolError::UnsupportedValue("null")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"open","a":1}"#),
            ProtocolError::WrongType("a")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"open","a":"split","id":1}"#),
            ProtocolError::UnknownKey("id".to_string())
        );
        assert_eq!(
            err(r#"{"v":1,"t":"open"}"#),
            ProtocolError::MissingField("a")
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

    #[test]
    fn set_text_caps_utf16_without_splitting_a_code_point() {
        let exact = format!("{}😀", "a".repeat(2046));
        assert_eq!(set_text_json(&exact, false), format!("{{\"t\":\"setText\",\"text\":\"{exact}\",\"select\":false}}"));
        let split = format!("{}😀tail", "a".repeat(2047));
        assert_eq!(set_text_json(&split, true), format!("{{\"t\":\"setText\",\"text\":\"{}\",\"select\":true}}", "a".repeat(2047)));
        assert_eq!(set_text_json(&"😀".repeat(1025), false), format!("{{\"t\":\"setText\",\"text\":\"{}\",\"select\":false}}", "😀".repeat(1024)));
    }

    #[test]
    fn parses_every_tab_action() {
        for (wire, action) in [
            ("select", TabAction::Select),
            ("close", TabAction::Close),
            ("menu", TabAction::Menu),
            ("pin", TabAction::Pin),
            ("mute", TabAction::Mute),
        ] {
            assert_eq!(
                ok(&format!(r#"{{"v":1,"t":"tab","a":"{wire}","id":12}}"#)),
                Command::Tab { action, id: 12 },
                "{wire}"
            );
        }
    }

    #[test]
    fn parses_tab_id_across_the_full_u32_range() {
        assert_eq!(
            ok(r#"{"v":1,"t":"tab","a":"close","id":0}"#),
            Command::Tab {
                action: TabAction::Close,
                id: 0
            }
        );
        assert_eq!(
            ok(r#"{"id":4294967295,"a":"pin","t":"tab","v":1}"#),
            Command::Tab {
                action: TabAction::Pin,
                id: u32::MAX
            }
        );
    }

    #[test]
    fn rejects_unknown_tab_actions() {
        for raw in [
            r#"{"v":1,"t":"tab","a":"toggle","id":1}"#,
            r#"{"v":1,"t":"tab","a":"Select","id":1}"#,
            r#"{"v":1,"t":"tab","a":"move","id":1}"#,
            r#"{"v":1,"t":"tab","a":"","id":1}"#,
            r#"{"v":1,"t":"tab","a":"select ","id":1}"#,
        ] {
            assert_eq!(err(raw), ProtocolError::InvalidValue("a"), "{raw}");
        }
    }

    #[test]
    fn tab_requires_string_action_and_id() {
        assert_eq!(
            err(r#"{"v":1,"t":"tab","id":1}"#),
            ProtocolError::MissingField("a")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"tab","a":"select"}"#),
            ProtocolError::MissingField("id")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"tab","a":1,"id":1}"#),
            ProtocolError::WrongType("a")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"tab","a":null,"id":1}"#),
            ProtocolError::UnsupportedValue("null")
        );
    }

    #[test]
    fn tab_id_must_be_a_u32() {
        assert_eq!(
            err(r#"{"v":1,"t":"tab","a":"select","id":4294967296}"#),
            ProtocolError::InvalidValue("id")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"tab","a":"select","id":-1}"#),
            ProtocolError::InvalidValue("id")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"tab","a":"select","id":99999999999999999999999999}"#),
            ProtocolError::InvalidValue("id")
        );
        for raw in [
            r#"{"v":1,"t":"tab","a":"select","id":1.5}"#,
            r#"{"v":1,"t":"tab","a":"select","id":1e3}"#,
            r#"{"v":1,"t":"tab","a":"select","id":"1"}"#,
            r#"{"v":1,"t":"tab","a":"select","id":true}"#,
        ] {
            assert_eq!(err(raw), ProtocolError::WrongType("id"), "{raw}");
        }
        assert_eq!(
            err(r#"{"v":1,"t":"tab","a":"select","id":{"n":1}}"#),
            ProtocolError::UnsupportedValue("nested object")
        );
    }

    #[test]
    fn tab_rejects_keys_it_does_not_use() {
        for (raw, key) in [
            (r#"{"v":1,"t":"tab","a":"select","id":1,"on":true}"#, "on"),
            (r#"{"v":1,"t":"tab","a":"select","id":1,"seq":1}"#, "seq"),
            (r#"{"v":1,"t":"tab","a":"select","id":1,"text":""}"#, "text"),
            (r#"{"v":1,"t":"tab","a":"select","id":1,"before":2}"#, "before"),
            (r#"{"v":1,"t":"tab","a":"select","id":1,"key":2}"#, "key"),
        ] {
            assert_eq!(err(raw), ProtocolError::UnknownKey(key.to_string()), "{raw}");
        }
    }

    #[test]
    fn parses_tab_move() {
        assert_eq!(
            ok(r#"{"v":1,"t":"tabmove","id":3,"before":9}"#),
            Command::TabMove { id: 3, before: 9 }
        );
        // `before` of 0 means the end of the strip.
        assert_eq!(
            ok(r#"{"before":0,"id":3,"t":"tabmove","v":1}"#),
            Command::TabMove { id: 3, before: 0 }
        );
    }

    #[test]
    fn tab_move_requires_both_ids_as_u32() {
        assert_eq!(
            err(r#"{"v":1,"t":"tabmove","id":3}"#),
            ProtocolError::MissingField("before")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"tabmove","before":3}"#),
            ProtocolError::MissingField("id")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"tabmove","id":3,"before":"9"}"#),
            ProtocolError::WrongType("before")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"tabmove","id":-3,"before":9}"#),
            ProtocolError::InvalidValue("id")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"tabmove","id":3,"before":4294967296}"#),
            ProtocolError::InvalidValue("before")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"tabmove","id":3,"before":null}"#),
            ProtocolError::UnsupportedValue("null")
        );
    }

    #[test]
    fn tab_move_rejects_extra_keys() {
        assert_eq!(
            err(r#"{"v":1,"t":"tabmove","id":3,"before":9,"a":"select"}"#),
            ProtocolError::UnknownKey("a".to_string())
        );
        assert_eq!(
            err(r#"{"v":1,"t":"tabmove","id":3,"before":9,"key":1}"#),
            ProtocolError::UnknownKey("key".to_string())
        );
    }

    #[test]
    fn parses_every_group_action() {
        assert_eq!(
            ok(r#"{"v":1,"t":"group","a":"toggle","key":4}"#),
            Command::Group {
                action: GroupAction::Toggle,
                key: 4
            }
        );
        assert_eq!(
            ok(r#"{"key":0,"a":"menu","t":"group","v":1}"#),
            Command::Group {
                action: GroupAction::Menu,
                key: 0
            }
        );
        assert_eq!(
            ok(r#"{"v":1,"t":"group","a":"toggle","key":4294967295}"#),
            Command::Group {
                action: GroupAction::Toggle,
                key: u32::MAX
            }
        );
    }

    #[test]
    fn group_rejects_bad_action_key_and_extra_keys() {
        for raw in [
            r#"{"v":1,"t":"group","a":"select","key":4}"#,
            r#"{"v":1,"t":"group","a":"Toggle","key":4}"#,
            r#"{"v":1,"t":"group","a":"collapse","key":4}"#,
        ] {
            assert_eq!(err(raw), ProtocolError::InvalidValue("a"), "{raw}");
        }
        assert_eq!(
            err(r#"{"v":1,"t":"group","a":"toggle"}"#),
            ProtocolError::MissingField("key")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"group","a":"toggle","key":-4}"#),
            ProtocolError::InvalidValue("key")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"group","a":"toggle","key":"4"}"#),
            ProtocolError::WrongType("key")
        );
        assert_eq!(
            err(r#"{"v":1,"t":"group","a":"toggle","key":4,"id":1}"#),
            ProtocolError::UnknownKey("id".to_string())
        );
        assert_eq!(
            err(r#"{"v":1,"t":"group","key":4}"#),
            ProtocolError::MissingField("a")
        );
    }

    #[test]
    fn new_messages_enforce_version_and_duplicate_rules() {
        assert_eq!(
            err(r#"{"v":2,"t":"tab","a":"select","id":1}"#),
            ProtocolError::UnsupportedVersion
        );
        assert_eq!(
            err(r#"{"v":1,"t":"tabmove","id":1,"before":2,"before":3}"#),
            ProtocolError::DuplicateKey("before".to_string())
        );
        assert_eq!(
            err(r#"{"v":1,"t":"group","a":"toggle","a":"menu","key":1}"#),
            ProtocolError::DuplicateKey("a".to_string())
        );
        assert_eq!(
            err(r#"{"v":1,"t":"tabs"}"#),
            ProtocolError::UnknownType("tabs".to_string())
        );
    }

    fn plain_tab(id: u32, title: &str) -> TabItem {
        TabItem {
            id,
            title: title.to_string(),
            loading: false,
            pinned: false,
            muted: false,
            marker: -1,
            group: 0,
            leader: false,
            group_name: String::new(),
            group_colour: -1,
            collapsed: false,
            group_size: 0,
            has_favicon: false,
        }
    }

    #[test]
    fn tabs_json_for_an_empty_strip() {
        assert_eq!(
            tabs_json(0, &[]),
            r#"{"v":1,"t":"tabs","active":0,"items":[]}"#
        );
    }

    #[test]
    fn tabs_json_for_one_tab() {
        let tab = TabItem {
            loading: true,
            pinned: true,
            muted: true,
            marker: 2,
            has_favicon: true,
            ..plain_tab(7, "Home")
        };
        assert_eq!(
            tabs_json(7, &[tab]),
            concat!(
                r#"{"v":1,"t":"tabs","active":7,"items":["#,
                r#"{"id":7,"title":"Home","loading":true,"pinned":true,"muted":true,"marker":2,"g":0,"gl":false,"fav":true}"#,
                "]}"
            )
        );
    }

    #[test]
    fn tabs_json_for_a_group_sends_group_fields_on_the_leader_only() {
        let leader = TabItem {
            group: 5,
            leader: true,
            group_name: "Work".to_string(),
            group_colour: 3,
            collapsed: true,
            group_size: 2,
            ..plain_tab(10, "Mail")
        };
        // Members carry the same group key but no group fields, even when the values are set.
        let member = TabItem {
            group: 5,
            group_name: "ignored".to_string(),
            group_colour: 9,
            collapsed: true,
            group_size: 2,
            ..plain_tab(11, "Calendar")
        };
        let loose = plain_tab(12, "Loose");
        assert_eq!(
            tabs_json(11, &[leader, member, loose]),
            concat!(
                r#"{"v":1,"t":"tabs","active":11,"items":["#,
                r#"{"id":10,"title":"Mail","loading":false,"pinned":false,"muted":false,"marker":-1,"g":5,"gl":true,"gname":"Work","gcol":3,"gcollapsed":true,"gsize":2,"fav":false},"#,
                r#"{"id":11,"title":"Calendar","loading":false,"pinned":false,"muted":false,"marker":-1,"g":5,"gl":false,"fav":false},"#,
                r#"{"id":12,"title":"Loose","loading":false,"pinned":false,"muted":false,"marker":-1,"g":0,"gl":false,"fav":false}"#,
                "]}"
            )
        );
    }

    #[test]
    fn tabs_json_escapes_titles_and_group_names() {
        let tab = TabItem {
            leader: true,
            group: 1,
            group_name: "a\"</b>".to_string(),
            ..plain_tab(1, "x<\"y\\\n\u{2028}")
        };
        let json = tabs_json(1, &[tab]);
        assert!(json.contains("\"title\":\"x\\u003c\\\"y\\\\\\n\\u2028\""), "{json}");
        assert!(json.contains("\"gname\":\"a\\\"\\u003c/b>\""), "{json}");
        assert!(!json.contains("</b"), "{json}");
        assert!(!json.contains('\n'), "{json}");
    }

    #[test]
    fn tab_titles_are_capped_on_character_boundaries() {
        let long = "a".repeat(300);
        let json = tabs_json(0, &[plain_tab(1, &long)]);
        assert!(json.contains(&format!(r#""title":"{}""#, "a".repeat(200))), "{json}");
        assert!(!json.contains(&"a".repeat(201)));

        let accents = "é".repeat(250);
        let json = tabs_json(0, &[plain_tab(1, &accents)]);
        assert!(json.contains(&format!(r#""title":"{}""#, "é".repeat(200))), "{json}");

        let exact = "b".repeat(200);
        let json = tabs_json(0, &[plain_tab(1, &exact)]);
        assert!(json.contains(&format!(r#""title":"{exact}""#)), "{json}");
    }

    #[test]
    fn group_names_are_capped_at_64_characters() {
        let tab = TabItem {
            leader: true,
            group: 1,
            group_name: "g".repeat(100),
            ..plain_tab(1, "t")
        };
        let json = tabs_json(0, &[tab]);
        assert!(json.contains(&format!(r#""gname":"{}""#, "g".repeat(64))), "{json}");
        assert!(!json.contains(&"g".repeat(65)));
    }

    #[test]
    fn tabs_json_is_a_single_flat_line_of_valid_structure() {
        let items = [plain_tab(1, "one"), plain_tab(2, "two")];
        let json = tabs_json(2, &items);
        assert!(json.starts_with(r#"{"v":1,"t":"tabs","active":2,"items":["#));
        assert!(json.ends_with("]}"));
        assert_eq!(json.matches(r#""id":"#).count(), 2);
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
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
        }
        assert_eq!(base64_encode(&[0xFB, 0xFF]), "+/8=");
        assert_eq!(base64_encode(&[0xFB, 0xEF, 0xBE]), "++++");
        assert_eq!(base64_encode(&PNG_SIGNATURE), "iVBORw0KGgo=");
    }

    /// A PNG prefix: signature, then an IHDR chunk with the given size, padded with zeros to `len`.
    fn png_with_size(width: u32, height: u32, len: usize) -> Vec<u8> {
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend_from_slice(&[0, 0, 0, 13]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&width.to_be_bytes());
        png.extend_from_slice(&height.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        png.resize(len.max(png.len()), 0);
        png
    }

    #[test]
    fn fav_json_wraps_a_valid_png() {
        let png = png_with_size(16, 16, 64);
        let json = fav_json(u32::MAX, &png).expect("png accepted");
        assert_eq!(
            json,
            format!(
                r#"{{"v":1,"t":"fav","id":4294967295,"uri":"data:image/png;base64,{}"}}"#,
                base64_encode(&png)
            )
        );
        assert!(json.starts_with(r#"{"v":1,"t":"fav","id":4294967295,"uri":"data:image/png;base64,"#));
    }

    #[test]
    fn fav_json_rejects_non_png_and_oversized_input() {
        assert_eq!(fav_json(1, &[]), None);
        assert_eq!(fav_json(1, &[0xFF, 0xD8, 0xFF]), None);
        assert_eq!(fav_json(1, b"GIF89a"), None);
        // One byte off the signature (0x0B instead of 0x0A at the end).
        let mut off = png_with_size(16, 16, 64);
        off[7] = 0x0B;
        assert_eq!(fav_json(1, &off), None);
        // Signature must be at the very start.
        let mut shifted = vec![0u8];
        shifted.extend_from_slice(&png_with_size(16, 16, 64));
        assert_eq!(fav_json(1, &shifted), None);

        let mut exact = png_with_size(16, 16, MAX_FAVICON_BYTES);
        assert_eq!(exact.len(), MAX_FAVICON_BYTES);
        assert!(fav_json(1, &exact).is_some());
        exact.push(0);
        assert_eq!(fav_json(1, &exact), None);
    }

    #[test]
    fn fav_json_checks_the_ihdr_size() {
        // Dimensions 1 to 256 pass; 0 and 257 fail on either side.
        for (width, height) in [(1, 1), (256, 256), (16, 256), (256, 1)] {
            assert!(fav_json(1, &png_with_size(width, height, 64)).is_some(), "{width}x{height}");
        }
        for (width, height) in [(0, 16), (16, 0), (0, 0), (257, 16), (16, 257), (u32::MAX, 16)] {
            assert_eq!(fav_json(1, &png_with_size(width, height, 64)), None, "{width}x{height}");
        }
        // The chunk type must be IHDR at bytes 12..16.
        let mut wrong_chunk = png_with_size(16, 16, 64);
        wrong_chunk[12..16].copy_from_slice(b"IDAT");
        assert_eq!(fav_json(1, &wrong_chunk), None);
        // A header cut off before the height is refused.
        assert_eq!(fav_json(1, &png_with_size(16, 16, 64)[..23]), None);
    }

    #[test]
    fn too_many_fields_fail_as_soon_as_the_seventh_key_starts() {
        assert_eq!(err(r#"{"v":1,"t":"tab","a":"select","id":1,"x":1,"y":2,"z":3}"#), ProtocolError::TooManyFields);
        // Exactly six keys: rejected for their names, not for the count.
        assert_eq!(
            err(r#"{"v":1,"t":"ready","a":"x","b":"y","c":"z","d":"w"}"#),
            ProtocolError::UnknownKey("a".to_string())
        );
        // A seventh key is refused even when the object is otherwise malformed later on.
        assert_eq!(
            err(r#"{"v":1,"t":"ready","a":1,"b":2,"c":3,"d":4,"e":5,"f":6,"g":"#),
            ProtocolError::TooManyFields
        );
    }

    #[test]
    fn tabs_json_sends_at_most_max_tabs_and_keeps_the_active_tab() {
        let items: Vec<TabItem> = (1..=600).map(|id| plain_tab(id, "t")).collect();
        let count = |json: &str| json.matches(r#""id":"#).count();
        let json = tabs_json(3, &items);
        assert_eq!(count(&json), MAX_TABS);
        assert!(json.contains(r#""id":1,"#) && json.contains(r#""id":512,"#));
        assert!(!json.contains(r#""id":513,"#));
        // Active tab beyond the cap replaces the last kept tab and keeps its place in order.
        let json = tabs_json(600, &items);
        assert_eq!(count(&json), MAX_TABS);
        assert!(json.contains(r#""id":600,"#));
        assert!(!json.contains(r#""id":512,"#));
        assert!(json.find(r#""id":511,"#) < json.find(r#""id":600,"#));
        // Exactly MAX_TABS items are all sent.
        assert_eq!(count(&tabs_json(1, &items[..MAX_TABS])), MAX_TABS);
    }

    #[test]
    fn token_bucket_allows_a_burst_then_refills_at_the_rate() {
        let t0 = Instant::now();
        let mut bucket = TokenBucket::new(60, 120);
        assert_eq!((0..60).filter(|_| bucket.take(t0)).count(), 60, "burst");
        assert!(!bucket.take(t0), "empty bucket drops");
        // 10 ms refills 1.2 tokens: one message passes.
        let t1 = t0 + Duration::from_millis(10);
        assert!(bucket.take(t1));
        assert!(!bucket.take(t1));
        // A long pause refills only up to the burst.
        let t2 = t1 + Duration::from_secs(60);
        assert_eq!((0..100).filter(|_| bucket.take(t2)).count(), 60);
    }

    #[test]
    fn token_bucket_sustains_the_configured_rate() {
        let t0 = Instant::now();
        let mut bucket = TokenBucket::new(MESSAGE_BURST, MESSAGE_RATE_PER_SEC);
        // Drain the burst, then one message every 10 ms (100 per second) never runs dry.
        for _ in 0..MESSAGE_BURST {
            assert!(bucket.take(t0));
        }
        for step in 1..=500u32 {
            assert!(bucket.take(t0 + Duration::from_millis(u64::from(step) * 10)), "step {step}");
        }
        // Clock going backwards neither refills nor breaks the bucket.
        let mut back = TokenBucket::new(1, 1);
        assert!(back.take(t0 + Duration::from_secs(5)));
        assert!(!back.take(t0));
    }

    #[test]
    fn throttle_passes_one_event_per_interval() {
        let t0 = Instant::now();
        let mut gate = Throttle::new(POPUP_COMMAND_INTERVAL);
        assert!(gate.allow(t0));
        assert!(!gate.allow(t0 + Duration::from_millis(249)));
        // Dropped events do not restart the interval.
        assert!(gate.allow(t0 + Duration::from_millis(250)));
        assert!(!gate.allow(t0 + Duration::from_millis(300)));
        assert!(gate.allow(t0 + Duration::from_millis(500)));
    }

    #[test]
    fn popup_commands_are_the_menu_caption_and_drag_set() {
        for command in [
            Command::BeginDrag,
            Command::Caption(CaptionAction::ToggleMaximize),
            Command::Caption(CaptionAction::SystemMenu),
            Command::Open(OpenTarget::Menu),
            Command::Open(OpenTarget::Site),
            Command::Open(OpenTarget::Split),
            Command::Tab { action: TabAction::Menu, id: 2 },
            Command::Group { action: GroupAction::Menu, key: 2 },
        ] {
            assert!(is_popup_command(&command), "{command:?}");
        }
        for command in [
            Command::Ready,
            Command::Nav(NavAction::Back),
            Command::Open(OpenTarget::NewTab),
            Command::Tab { action: TabAction::Select, id: 2 },
            Command::Tab { action: TabAction::Close, id: 2 },
            Command::Group { action: GroupAction::Toggle, key: 2 },
            Command::TabMove { id: 2, before: 0 },
            Command::Rail { action: RailAction::Click, key: RailKeyWire::Home },
            Command::RailMove { key: RailKeyWire::Workspace(2), before: None },
        ] {
            assert!(!is_popup_command(&command), "{command:?}");
        }
        assert!(is_popup_command(&Command::Rail {
            action: RailAction::Menu,
            key: RailKeyWire::App(3),
        }));
    }

    fn rail_item(key: &str, label: &str, accent: &str) -> RailItem {
        RailItem {
            key: key.to_owned(),
            icon: "letter".to_owned(),
            label: label.to_owned(),
            active: true,
            dot: false,
            accent: accent.to_owned(),
            has_favicon: false,
        }
    }

    #[test]
    fn rail_key_grammar_round_trips() {
        let keys = [
            RailKeyWire::Home,
            RailKeyWire::Customize,
            RailKeyWire::AddWorkspace,
            RailKeyWire::AddApp,
            RailKeyWire::Bookmarks,
            RailKeyWire::History,
            RailKeyWire::Menu,
            RailKeyWire::Workspace(0),
            RailKeyWire::Workspace(42),
            RailKeyWire::Workspace(u64::MAX),
            RailKeyWire::App(0),
            RailKeyWire::App(u32::MAX),
        ];
        for key in keys {
            let wire = key.to_wire();
            assert!(wire.len() <= MAX_RAIL_KEY_CHARS, "{wire}");
            assert_eq!(RailKeyWire::parse(&wire), Some(key), "{wire}");
        }
        assert_eq!(RailKeyWire::Workspace(7).to_wire(), "ws:7");
        assert_eq!(RailKeyWire::App(12).to_wire(), "app:12");
    }

    #[test]
    fn rail_key_grammar_is_strict() {
        for raw in [
            "", "Home", "home ", " home", "addWs", "ws", "ws:", "app:", "ws:-1", "app:-1", "ws:+1",
            "ws:1.5", "ws:1e3", "ws: 1", "ws:a", "app:x", "ws:0x1", "ws:01", "app:007",
            "app:4294967296", "app:99999999999", "ws:100000000000000000000",
            "ws:18446744073709551616", "app:1:2", "ws:1 ", "bookmark", "menu:1", "home:1",
            "ws:\u{0661}", "xx:1",
        ] {
            assert_eq!(RailKeyWire::parse(raw), None, "{raw:?}");
        }
        // Digit limits: 20 for workspaces, 10 for apps, both inclusive.
        assert!(RailKeyWire::parse("ws:18446744073709551615").is_some());
        assert!(RailKeyWire::parse("app:4294967295").is_some());
        assert!(RailKeyWire::parse(&format!("ws:{}", "9".repeat(20))).is_none());
        assert!(RailKeyWire::parse(&format!("ws:{}", "9".repeat(21))).is_none());
        assert!(RailKeyWire::parse(&format!("ws:{}", "9".repeat(18))).is_some());
        assert!(RailKeyWire::parse(&format!("app:{}", "9".repeat(11))).is_none());
    }

    #[test]
    fn parses_rail_click_and_menu() {
        assert_eq!(
            ok(r#"{"v":1,"t":"rail","a":"click","k":"ws:3"}"#),
            Command::Rail { action: RailAction::Click, key: RailKeyWire::Workspace(3) }
        );
        assert_eq!(
            ok(r#"{"v":1,"t":"rail","a":"menu","k":"app:9"}"#),
            Command::Rail { action: RailAction::Menu, key: RailKeyWire::App(9) }
        );
        for key in ["home", "customize", "addws", "addapp", "bookmarks", "history", "menu"] {
            let raw = format!(r#"{{"v":1,"t":"rail","a":"click","k":"{key}"}}"#);
            assert!(matches!(ok(&raw), Command::Rail { .. }), "{key}");
        }
    }

    #[test]
    fn rail_rejects_bad_actions_keys_and_fields() {
        assert!(matches!(
            err(r#"{"v":1,"t":"rail","a":"drag","k":"home"}"#),
            ProtocolError::InvalidValue("a")
        ));
        assert!(matches!(
            err(r#"{"v":1,"t":"rail","a":"click","k":"ws:01"}"#),
            ProtocolError::InvalidValue("k")
        ));
        assert!(matches!(
            err(r#"{"v":1,"t":"rail","a":"click","k":"unknown"}"#),
            ProtocolError::InvalidValue("k")
        ));
        assert!(matches!(
            err(r#"{"v":1,"t":"rail","a":"click"}"#),
            ProtocolError::MissingField("k")
        ));
        assert!(matches!(
            err(r#"{"v":1,"t":"rail","a":"click","k":5}"#),
            ProtocolError::WrongType("k")
        ));
        assert!(matches!(
            err(r#"{"v":1,"t":"rail","k":"home"}"#),
            ProtocolError::MissingField("a")
        ));
        assert!(matches!(
            err(r#"{"v":1,"t":"rail","a":"click","k":"home","x":1}"#),
            ProtocolError::UnknownKey(_)
        ));
        assert!(matches!(
            err(r#"{"v":2,"t":"rail","a":"click","k":"home"}"#),
            ProtocolError::UnsupportedVersion
        ));
    }

    #[test]
    fn parses_rail_move_with_and_without_a_target() {
        assert_eq!(
            ok(r#"{"v":1,"t":"railmove","k":"ws:2","before":"ws:5"}"#),
            Command::RailMove {
                key: RailKeyWire::Workspace(2),
                before: Some(RailKeyWire::Workspace(5)),
            }
        );
        assert_eq!(
            ok(r#"{"v":1,"t":"railmove","k":"app:1","before":""}"#),
            Command::RailMove { key: RailKeyWire::App(1), before: None }
        );
        // The target may be any key of the grammar; Rust checks it against the model later.
        assert_eq!(
            ok(r#"{"v":1,"t":"railmove","k":"app:1","before":"addapp"}"#),
            Command::RailMove { key: RailKeyWire::App(1), before: Some(RailKeyWire::AddApp) }
        );
    }

    #[test]
    fn rail_move_moves_only_workspaces_and_apps() {
        for key in ["home", "customize", "addws", "addapp", "bookmarks", "history", "menu"] {
            let raw = format!(r#"{{"v":1,"t":"railmove","k":"{key}","before":""}}"#);
            assert!(
                matches!(err(&raw), ProtocolError::InvalidValue("k")),
                "{key}"
            );
        }
        assert!(matches!(
            err(r#"{"v":1,"t":"railmove","k":"ws:1","before":"nope"}"#),
            ProtocolError::InvalidValue("before")
        ));
        assert!(matches!(
            err(r#"{"v":1,"t":"railmove","k":"ws:1"}"#),
            ProtocolError::MissingField("before")
        ));
        assert!(matches!(
            err(r#"{"v":1,"t":"railmove","k":"ws:1","before":null}"#),
            ProtocolError::UnsupportedValue("null")
        ));
        assert!(matches!(
            err(r#"{"v":1,"t":"railmove","k":"ws:1","before":"","a":"click"}"#),
            ProtocolError::UnknownKey(_)
        ));
    }

    #[test]
    fn rail_json_is_one_flat_versioned_object() {
        let items = vec![
            RailItem {
                key: "home".to_owned(),
                icon: "home".to_owned(),
                label: "Home".to_owned(),
                active: false,
                dot: true,
                accent: String::new(),
                has_favicon: false,
            },
            RailItem {
                key: "ws:4".to_owned(),
                icon: "g:code".to_owned(),
                label: "Work \"A\"".to_owned(),
                active: true,
                dot: false,
                accent: "#4385d1".to_owned(),
                has_favicon: false,
            },
        ];
        assert_eq!(
            rail_json(&items),
            concat!(
                r#"{"v":1,"t":"rail","items":["#,
                r#"{"k":"home","icon":"home","label":"Home","active":false,"dot":true,"accent":"","fav":false},"#,
                r##"{"k":"ws:4","icon":"g:code","label":"Work \"A\"","active":true,"dot":false,"accent":"#4385d1","fav":false}"##,
                "]}"
            )
        );
        assert_eq!(rail_json(&[]), r#"{"v":1,"t":"rail","items":[]}"#);
    }

    #[test]
    fn rail_json_caps_labels_and_item_count() {
        let long = "x".repeat(MAX_RAIL_LABEL_CHARS + 30);
        let json = rail_json(&[rail_item("ws:1", &long, "")]);
        assert!(json.contains(&format!("\"label\":\"{}\"", "x".repeat(MAX_RAIL_LABEL_CHARS))));
        assert!(!json.contains(&"x".repeat(MAX_RAIL_LABEL_CHARS + 1)));

        // Multi-byte labels are cut on character boundaries.
        let accents = "é".repeat(MAX_RAIL_LABEL_CHARS + 1);
        let json = rail_json(&[rail_item("ws:1", &accents, "")]);
        assert!(json.contains(&format!("\"label\":\"{}\"", "é".repeat(MAX_RAIL_LABEL_CHARS))));

        let many: Vec<RailItem> = (0..MAX_RAIL_ITEMS + 10)
            .map(|index| rail_item(&format!("ws:{index}"), "w", ""))
            .collect();
        let json = rail_json(&many);
        assert_eq!(json.matches("\"k\":").count(), MAX_RAIL_ITEMS);
        assert!(json.contains("\"k\":\"ws:127\"") && !json.contains("\"k\":\"ws:128\""));
    }

    #[test]
    fn rail_json_drops_unsafe_accents_to_the_theme_accent() {
        for accent in [
            "url(http://x/)",
            "red;}",
            "expression(alert(1))",
            "#12345",
            "rgb(0,0,0) ",
            "\"",
        ] {
            let json = rail_json(&[rail_item("ws:1", "w", accent)]);
            assert!(json.contains("\"accent\":\"\""), "{accent:?} -> {json}");
        }
        let json = rail_json(&[rail_item("ws:1", "w", "rgba(1,2,3,0.5)")]);
        assert!(json.contains("\"accent\":\"rgba(1,2,3,0.5)\""), "{json}");
    }

    #[test]
    fn rail_json_escapes_labels_that_try_to_break_out() {
        let json = rail_json(&[rail_item("ws:1", "</script>\"}],\"x\":1", "")]);
        // `<` is written as a \u escape and the quotes are escaped, so the label stays one string.
        assert!(!json.contains("</script>"), "{json}");
        let escaped_lt = format!("\\u{:04x}/script>", u32::from('<'));
        assert!(json.contains(&format!("{escaped_lt}\\\"}}],\\\"x\\\":1")), "{json}");
        assert_eq!(json.matches("\"k\":").count(), 1);
    }

    #[test]
    fn railfav_json_uses_the_fav_gate() {
        let png = png_with_size(16, 16, 64);
        let json = railfav_json("app:5", &png).expect("png accepted");
        assert!(json.starts_with(r#"{"v":1,"t":"railfav","k":"app:5","uri":"data:image/png;base64,"#));
        assert!(json.ends_with("\"}"));
        assert_eq!(
            railfav_json("app:5", &png_with_size(16, 16, MAX_FAVICON_BYTES + 1)),
            None,
            "oversized"
        );
        assert_eq!(railfav_json("app:5", &png_with_size(300, 16, 64)), None, "wide");
        assert_eq!(railfav_json("app:5", b"GIF89a"), None, "not png");
        assert_eq!(railfav_json("app:5", &[]), None);
        // Same bytes as `fav`, so the two messages agree on what may be sent.
        assert!(fav_json(1, &png).is_some() && railfav_json("app:1", &png).is_some());
    }
}
