//! A small XML reader for feeds and OPML, built to be safe on text from the network.
//!
//! What it does not do is the point: no DTD (a document with `<!DOCTYPE` is refused, so no entity can
//! be declared and nothing external is ever read or fetched), no namespaces resolved, no schema, no
//! script. Entities are the five predefined ones and numeric references; anything else stays literal
//! text. Input size, nesting depth, element count, attribute count and text length are all capped, so a
//! hostile document costs a bounded amount of memory and time.

/// Largest document read, in bytes. The caller fetches at most this much anyway.
pub const MAX_DOCUMENT_BYTES: usize = 3 * 1024 * 1024;
/// Deepest nesting accepted.
pub const MAX_DEPTH: usize = 48;
/// Most elements in one document.
pub const MAX_ELEMENTS: usize = 60_000;
/// Most attributes on one element.
const MAX_ATTRIBUTES: usize = 32;
/// Longest single text run or attribute value kept, in characters.
const MAX_TEXT_CHARS: usize = 200_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Element {
    /// The name as written, with any prefix (`media:title`).
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XmlError {
    /// Not text Bowser can read as XML (no root element, broken markup, a bad encoding).
    Malformed,
    /// A DOCTYPE (entity declarations, external subsets) is never accepted.
    Doctype,
    /// More depth, elements or bytes than the limits allow.
    TooBig,
}

impl Element {
    /// The part of the name after any prefix: `title` for `media:title`.
    pub fn local(&self) -> &str {
        self.name.rsplit_once(':').map_or(self.name.as_str(), |(_, local)| local)
    }

    /// The first child element named exactly `name` (prefix included).
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|element| element.name == name)
    }

    /// Every child element.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|node| match node {
            Node::Element(element) => Some(element),
            Node::Text(_) => None,
        })
    }

    /// Child elements named exactly `name`.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.elements().filter(move |element| element.name == name)
    }

    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    /// All the text inside, concatenated, without markup.
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.collect_text(&mut out);
        out
    }

    fn collect_text(&self, out: &mut String) {
        for node in &self.children {
            match node {
                Node::Text(text) => out.push_str(text),
                Node::Element(element) => element.collect_text(out),
            }
        }
    }

    /// The trimmed text of the first child named `name`, if it has any.
    pub fn child_text(&self, name: &str) -> Option<String> {
        let text = self.child(name)?.text();
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }
}

/// Decodes the bytes of a document to text: UTF-8 (with or without a BOM), UTF-16 with a BOM, or the
/// single-byte Latin-1 family when the declaration names it.
pub fn decode(bytes: &[u8]) -> Result<String, XmlError> {
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(XmlError::TooBig);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, false);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, true);
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(text.to_owned());
    }
    // Not UTF-8: honour a declared single-byte encoding, otherwise refuse.
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).to_ascii_lowercase();
    if head.contains("iso-8859-1") || head.contains("latin1") || head.contains("windows-1252") || head.contains("us-ascii") {
        return Ok(bytes.iter().map(|&byte| char::from(byte)).collect());
    }
    Err(XmlError::Malformed)
}

fn utf16(bytes: &[u8], big_endian: bool) -> Result<String, XmlError> {
    if bytes.len() % 2 != 0 {
        return Err(XmlError::Malformed);
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| if big_endian { u16::from_be_bytes([pair[0], pair[1]]) } else { u16::from_le_bytes([pair[0], pair[1]]) })
        .collect();
    String::from_utf16(&units).map_err(|_| XmlError::Malformed)
}

/// Parses `text` into its root element.
pub fn parse(text: &str) -> Result<Element, XmlError> {
    if text.len() > MAX_DOCUMENT_BYTES {
        return Err(XmlError::TooBig);
    }
    let mut parser = Parser { text, bytes: text.as_bytes(), at: 0, elements: 0 };
    parser.document()
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
    elements: usize,
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':' | b'-' | b'.') || byte >= 0x80
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    fn starts_with(&self, prefix: &str) -> bool {
        self.rest().starts_with(prefix)
    }

    fn skip_space(&mut self) {
        while self.at < self.bytes.len() && self.bytes[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    /// Skips past `end`, which must occur; the text up to it is returned.
    fn skip_to(&mut self, end: &str) -> Result<&'a str, XmlError> {
        let rest = self.rest();
        let at = rest.find(end).ok_or(XmlError::Malformed)?;
        self.at += at + end.len();
        Ok(&rest[..at])
    }

    fn document(&mut self) -> Result<Element, XmlError> {
        if self.starts_with("\u{feff}") {
            self.at += 3;
        }
        // Prolog: the declaration, comments, processing instructions and white space. A DOCTYPE is refused.
        loop {
            self.skip_space();
            if self.starts_with("<?") {
                self.skip_to("?>")?;
            } else if self.starts_with("<!--") {
                self.skip_to("-->")?;
            } else if self.starts_with("<!") {
                return Err(XmlError::Doctype);
            } else {
                break;
            }
        }
        if !self.starts_with("<") {
            return Err(XmlError::Malformed);
        }
        let root = self.element(0)?;
        // Epilog: only comments, processing instructions and white space.
        loop {
            self.skip_space();
            if self.at >= self.bytes.len() {
                return Ok(root);
            } else if self.starts_with("<?") {
                self.skip_to("?>")?;
            } else if self.starts_with("<!--") {
                self.skip_to("-->")?;
            } else {
                return Err(XmlError::Malformed);
            }
        }
    }

    fn name(&mut self) -> Result<String, XmlError> {
        let start = self.at;
        while self.at < self.bytes.len() && is_name_byte(self.bytes[self.at]) {
            self.at += 1;
        }
        if self.at == start || self.at - start > 256 {
            return Err(XmlError::Malformed);
        }
        Ok(self.text[start..self.at].to_owned())
    }

    fn element(&mut self, depth: usize) -> Result<Element, XmlError> {
        if depth >= MAX_DEPTH {
            return Err(XmlError::TooBig);
        }
        self.elements += 1;
        if self.elements > MAX_ELEMENTS {
            return Err(XmlError::TooBig);
        }
        self.at += 1; // '<'
        let name = self.name()?;
        let mut element = Element { name, ..Element::default() };
        // Attributes.
        loop {
            self.skip_space();
            match self.bytes.get(self.at) {
                Some(b'/') => {
                    if self.bytes.get(self.at + 1) != Some(&b'>') {
                        return Err(XmlError::Malformed);
                    }
                    self.at += 2;
                    return Ok(element);
                }
                Some(b'>') => {
                    self.at += 1;
                    break;
                }
                Some(_) => {
                    if element.attributes.len() >= MAX_ATTRIBUTES {
                        return Err(XmlError::TooBig);
                    }
                    let key = self.name()?;
                    self.skip_space();
                    if self.bytes.get(self.at) != Some(&b'=') {
                        return Err(XmlError::Malformed);
                    }
                    self.at += 1;
                    self.skip_space();
                    let quote = match self.bytes.get(self.at) {
                        Some(&quote @ (b'"' | b'\'')) => quote,
                        _ => return Err(XmlError::Malformed),
                    };
                    self.at += 1;
                    let close = if quote == b'"' { "\"" } else { "'" };
                    let raw = self.skip_to(close)?;
                    if raw.contains('<') {
                        return Err(XmlError::Malformed);
                    }
                    element.attributes.push((key, decode_entities(truncate(raw))));
                }
                None => return Err(XmlError::Malformed),
            }
        }
        // Content.
        loop {
            if self.at >= self.bytes.len() {
                return Err(XmlError::Malformed);
            }
            if self.starts_with("</") {
                self.at += 2;
                let closing = self.name()?;
                self.skip_space();
                if self.bytes.get(self.at) != Some(&b'>') || closing != element.name {
                    return Err(XmlError::Malformed);
                }
                self.at += 1;
                return Ok(element);
            } else if self.starts_with("<!--") {
                self.skip_to("-->")?;
            } else if self.starts_with("<![CDATA[") {
                self.at += 9;
                let raw = self.skip_to("]]>")?;
                push_text(&mut element, truncate(raw).to_owned());
            } else if self.starts_with("<?") {
                self.skip_to("?>")?;
            } else if self.starts_with("<!") {
                return Err(XmlError::Doctype);
            } else if self.starts_with("<") {
                let child = self.element(depth + 1)?;
                element.children.push(Node::Element(child));
            } else {
                let rest = self.rest();
                let end = rest.find('<').unwrap_or(rest.len());
                let raw = &rest[..end];
                self.at += end;
                push_text(&mut element, decode_entities(truncate(raw)));
            }
        }
    }
}

fn truncate(text: &str) -> &str {
    match text.char_indices().nth(MAX_TEXT_CHARS) {
        Some((at, _)) => &text[..at],
        None => text,
    }
}

fn push_text(element: &mut Element, text: String) {
    if text.is_empty() {
        return;
    }
    if let Some(Node::Text(last)) = element.children.last_mut() {
        last.push_str(&text);
    } else {
        element.children.push(Node::Text(text));
    }
}

/// The byte index of the `;` that closes the entity `rest` starts with (`rest[0]` is `&`), looked for only
/// in the next ten bytes: searching the whole remainder per `&` made a run of `&` quadratic.
pub(crate) fn entity_end(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut at = 0;
    while at < bytes.len().min(11) {
        if bytes[at] == b';' {
            return Some(at);
        }
        at += 1;
    }
    None
}

/// Calls `decode` with the name of each `&name;` in `raw` (at most nine bytes between `&` and `;`) and puts
/// its answer in place of the entity; a `None` answer leaves the `&` as text. One pass over the bytes.
pub(crate) fn replace_entities(raw: &str, decode: impl Fn(&str) -> Option<char>) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let (mut start, mut at) = (0, 0);
    while at < bytes.len() {
        if bytes[at] == b'&'
            && let Some(end) = entity_end(&raw[at..])
            && let Some(c) = decode(&raw[at + 1..at + end])
        {
            out.push_str(&raw[start..at]);
            out.push(c);
            at += end + 1;
            start = at;
            continue;
        }
        at += 1;
    }
    out.push_str(&raw[start..]);
    out
}

/// Replaces the five predefined entities and numeric character references. Any other `&name;` stays as
/// it is, so a document can never name something the reader would look up.
pub fn decode_entities(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_owned();
    }
    replace_entities(raw, |entity| {
        match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity.strip_prefix('#').and_then(|number| {
                let code = match number.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                    None => number.parse().ok()?,
                };
                char::from_u32(code).filter(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r'))
            }),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_document_is_a_tree() {
        let root = parse(r#"<?xml version="1.0"?><!-- c --><a x="1 &amp; 2"><b>hi &lt;there&gt;</b><c/><![CDATA[<raw>]]></a>"#).unwrap();
        assert_eq!(root.name, "a");
        assert_eq!(root.attribute("x"), Some("1 & 2"));
        assert_eq!(root.child_text("b").as_deref(), Some("hi <there>"));
        assert!(root.child("c").is_some());
        assert_eq!(root.text(), "hi <there><raw>");
    }

    #[test]
    fn a_doctype_is_refused_so_no_entity_is_ever_declared() {
        let xxe = r#"<?xml version="1.0"?><!DOCTYPE foo [<!ENTITY xxe SYSTEM "file:///c:/windows/win.ini">]><foo>&xxe;</foo>"#;
        assert_eq!(parse(xxe), Err(XmlError::Doctype));
        let laughs = r#"<!DOCTYPE lolz [<!ENTITY lol "lol"><!ENTITY lol2 "&lol;&lol;">]><lolz>&lol2;</lolz>"#;
        assert_eq!(parse(laughs), Err(XmlError::Doctype));
        assert_eq!(parse("<a><!DOCTYPE x></a>"), Err(XmlError::Doctype));
    }

    #[test]
    fn unknown_entities_stay_literal_and_are_never_looked_up() {
        let root = parse("<a>&xxe; &#65; &#x42; &amp;amp; &bad</a>").unwrap();
        assert_eq!(root.text(), "&xxe; A B &amp; &bad");
        assert_eq!(decode_entities("&#0;&#1;"), "&#0;&#1;", "control characters are not produced");
    }

    #[test]
    fn a_long_run_of_ampersands_decodes_in_linear_time() {
        let hostile = "&".repeat(2_000_000);
        let started = std::time::Instant::now();
        assert_eq!(decode_entities(&hostile), hostile);
        assert_eq!(decode_entities(&format!("{hostile}&amp;")), format!("{hostile}&"));
        assert!(started.elapsed() < std::time::Duration::from_secs(1), "took {:?}", started.elapsed());
        assert_eq!(decode_entities("&amp;&#x1F600;&lt"), "&\u{1F600}&lt");
    }

    #[test]
    fn broken_markup_is_refused() {
        for bad in ["", "plain text", "<a>", "<a></b>", "<a b></a>", "<a b='1'", "<a b=1></a>", "<a><b></a></b>", "<a/>trailing", "<a x='<'/>"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn depth_element_and_size_limits_hold() {
        let deep = format!("{}{}", "<a>".repeat(MAX_DEPTH + 1), "</a>".repeat(MAX_DEPTH + 1));
        assert_eq!(parse(&deep), Err(XmlError::TooBig));
        let ok = format!("{}{}", "<a>".repeat(MAX_DEPTH - 1), "</a>".repeat(MAX_DEPTH - 1));
        assert!(parse(&ok).is_ok());
        let wide = format!("<r>{}</r>", "<i/>".repeat(MAX_ELEMENTS));
        assert_eq!(parse(&wide), Err(XmlError::TooBig));
        let attrs = format!("<r {}/>", (0..40).map(|n| format!("a{n}='1'")).collect::<Vec<_>>().join(" "));
        assert_eq!(parse(&attrs), Err(XmlError::TooBig));
        assert_eq!(decode(&vec![b'a'; MAX_DOCUMENT_BYTES + 1]), Err(XmlError::TooBig));
    }

    #[test]
    fn bytes_decode_as_utf8_utf16_or_declared_latin_1() {
        assert_eq!(decode(b"\xEF\xBB\xBF<a>\xC3\xA9</a>").unwrap(), "<a>é</a>");
        let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain("<a>é</a>".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(decode(&utf16).unwrap(), "<a>é</a>");
        assert_eq!(decode(b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>\xE9</a>").unwrap(), "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>é</a>");
        assert_eq!(decode(b"<a>\xE9</a>"), Err(XmlError::Malformed), "undeclared non-UTF-8 is refused");
    }

    #[test]
    fn prefixes_are_kept_in_the_name_and_split_by_local() {
        let root = parse("<entry xmlns:yt='x'><yt:videoId>abc</yt:videoId></entry>").unwrap();
        assert_eq!(root.child_text("yt:videoId").as_deref(), Some("abc"));
        assert_eq!(root.child("yt:videoId").unwrap().local(), "videoId");
        assert_eq!(root.child("videoId"), None);
    }
}
