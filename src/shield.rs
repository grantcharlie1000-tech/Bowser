//! Bowser Shield: the SRL rule engine (parse, compile, decide).
//!
//! SRL is a small line-oriented rule language: `block`, `allow`, `hide` and `off`, aimed at a host,
//! a host label, a path fragment, a CSS selector or a whole site. This module is the pure matcher
//! from `extensions/bowser-shield/lib/shield-core.js`, which stays in the repository as the
//! specification and as the Node test oracle. There is no Windows API here and no I/O: the
//! WebView2 hook lives in `engine/webview2.rs` and reaches this through `crate::blocker`.

use std::{borrow::Cow, collections::HashMap};

/// The starter list, compiled in. Generic third-party ad and tracker URL shapes, no vendor names.
pub const STARTER_RULES: &str = include_str!("../extensions/bowser-shield/rules/starter.srl");

/// Hard limits from the SRL specification.
pub const MAX_LIST_BYTES: usize = 256 * 1024;
pub const MAX_RULES: usize = 2000;
pub const MAX_LINE_CHARS: usize = 512;
pub const MAX_HOST_LIST: usize = 16;
pub const MAX_OFF_RULES: usize = 500;
pub const MAX_HOST_LABELS: usize = 24;
pub const MAX_ERRORS: usize = 100;
/// Longest path-and-query text, in bytes, that path rules are run against. A bound on the work one
/// request can cost; the specification has no such cap, a real path never gets near it.
pub const MAX_PATH_TEXT_BYTES: usize = 8192;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestKind {
    Script,
    Image,
    Frame,
    Xhr,
    Media,
    Font,
    Style,
    Ping,
    Other,
}

/// Every request kind. `should_block` without a kind asks "does any rule apply to this URL".
pub const TYPE_ALL: u16 = 511;

impl RequestKind {
    pub fn mask(self) -> u16 {
        1 << (self as u16)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verb {
    Block,
    Allow,
    Hide,
    Off,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    Host,
    Label,
    Path,
    Css,
    Site,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Party {
    Any,
    First,
    Third,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rule {
    pub line: usize,
    pub verb: Verb,
    pub target: Target,
    pub value: String,
    pub types: u16,
    pub party: Party,
    pub only: Option<Vec<String>>,
    pub except: Option<Vec<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Issue {
    pub line: usize,
    pub column: usize,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct ParsedList {
    pub rules: Vec<Rule>,
    pub errors: Vec<Issue>,
    pub warnings: Vec<Issue>,
}

/// Parses one SRL list. Bad lines are recorded in `errors` and skipped; a hard limit stops the
/// parse at the line that passes it.
pub fn parse_list(src: &str) -> ParsedList {
    let mut out = ParsedList::default();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut off_count = 0;
    let mut bytes = 0usize;
    for (index, raw) in src.split('\n').enumerate() {
        let line_no = index + 1;
        bytes += raw.len() + usize::from(index > 0);
        if bytes > MAX_LIST_BYTES {
            out.errors.push(issue(
                line_no,
                1,
                "list-too-large",
                format!("list is over the 256 KiB limit at line {line_no}"),
            ));
            break;
        }
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if is_blank_or_comment(line) {
            continue;
        }
        let parsed = if let Some(tab) = line.find('\t') {
            Err(bad(
                "tab",
                line[..tab].encode_utf16().count() + 1,
                "tabs are not allowed; separate words with spaces",
            ))
        } else if line.encode_utf16().count() > MAX_LINE_CHARS {
            Err(bad(
                "line-too-long",
                MAX_LINE_CHARS + 1,
                format!("a line can be at most {MAX_LINE_CHARS} characters"),
            ))
        } else if line.starts_with(' ') {
            Err(bad("leading-space", 1, "a rule must start in the first column"))
        } else {
            parse_rule(line)
        };
        let (mut rule, key) = match parsed {
            Ok(parsed) => parsed,
            Err(problem) => {
                if out.errors.len() >= MAX_ERRORS {
                    out.errors.push(issue(
                        line_no,
                        1,
                        "too-many-errors",
                        format!("stopped after {MAX_ERRORS} errors, at line {line_no}"),
                    ));
                    break;
                }
                out.errors
                    .push(issue(line_no, problem.column, problem.code, problem.message));
                continue;
            }
        };
        if let Some(first) = seen.get(&key) {
            out.warnings.push(issue(
                line_no,
                1,
                "duplicate-rule",
                format!("same as line {first}; ignored"),
            ));
            continue;
        }
        if out.rules.len() >= MAX_RULES {
            out.errors.push(issue(
                line_no,
                1,
                "too-many-rules",
                format!("list has more than {MAX_RULES} rules, passed at line {line_no}"),
            ));
            break;
        }
        if rule.verb == Verb::Off && off_count >= MAX_OFF_RULES {
            out.errors.push(issue(
                line_no,
                1,
                "too-many-off",
                format!("list has more than {MAX_OFF_RULES} off rules, passed at line {line_no}"),
            ));
            break;
        }
        seen.insert(key, line_no);
        if rule.verb == Verb::Off {
            off_count += 1;
        }
        rule.line = line_no;
        out.rules.push(rule);
    }
    out
}

fn issue(line: usize, column: usize, code: &'static str, message: String) -> Issue {
    Issue {
        line,
        column,
        code,
        message,
    }
}

/// A line problem found before it has a line number.
struct Bad {
    code: &'static str,
    column: usize,
    message: String,
}

fn bad(code: &'static str, column: usize, message: impl Into<String>) -> Bad {
    Bad {
        code,
        column,
        message: message.into(),
    }
}

fn is_blank_or_comment(line: &str) -> bool {
    let rest = line.trim_start_matches(' ');
    rest.is_empty() || rest.starts_with('#')
}

struct Token<'a> {
    text: &'a str,
    /// Byte offset in the line.
    start: usize,
}

/// Words split on single or repeated ASCII spaces; nothing else separates words.
fn tokenize(line: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        while i < bytes.len() && bytes[i] != b' ' {
            i += 1;
        }
        tokens.push(Token {
            text: &line[start..i],
            start,
        });
    }
    tokens
}

const TYPE_NAMES: [&str; 9] = [
    "script", "image", "frame", "xhr", "media", "font", "style", "ping", "other",
];

/// The qualifier words, in the order `Seen` below tracks them.
const QUALIFIERS: [&str; 4] = ["kind", "party", "only", "except"];

fn verb_name(verb: Verb) -> &'static str {
    match verb {
        Verb::Block => "block",
        Verb::Allow => "allow",
        Verb::Hide => "hide",
        Verb::Off => "off",
    }
}

fn target_allowed(verb: Verb, target: Target) -> bool {
    match verb {
        Verb::Block | Verb::Allow => matches!(target, Target::Host | Target::Label | Target::Path),
        Verb::Hide => target == Target::Css,
        Verb::Off => target == Target::Site,
    }
}

fn qualifier_allowed(verb: Verb, qualifier: usize) -> bool {
    match verb {
        Verb::Block | Verb::Allow => true,
        Verb::Hide => qualifier >= 2,
        Verb::Off => false,
    }
}

fn parse_rule(line: &str) -> Result<(Rule, String), Bad> {
    let tokens = tokenize(line);
    let column = |token: &Token| line[..token.start].encode_utf16().count() + 1;
    let verb_token = &tokens[0];
    let verb = match verb_token.text {
        "block" => Verb::Block,
        "allow" => Verb::Allow,
        "hide" => Verb::Hide,
        "off" => Verb::Off,
        _ => {
            return Err(bad(
                "bad-verb",
                column(verb_token),
                "expected block, allow, hide or off at the start of the line",
            ));
        }
    };
    let name = verb_name(verb);
    let target_token = match tokens.get(1) {
        Some(token) if token.text.contains(':') => token,
        other => {
            return Err(bad(
                "bad-target",
                other.map_or(column(verb_token) + name.len(), column),
                format!("expected kind:value after {name}, such as host:ads.example"),
            ));
        }
    };
    let colon = target_token.text.find(':').unwrap_or(0);
    let (kind_text, value) = (&target_token.text[..colon], &target_token.text[colon + 1..]);
    let target = match kind_text {
        "host" => Target::Host,
        "label" => Target::Label,
        "path" => Target::Path,
        "css" => Target::Css,
        "site" => Target::Site,
        _ => {
            return Err(bad(
                "bad-kind",
                column(target_token),
                format!("unknown kind \"{kind_text}\"; use host, label, path, css or site"),
            ));
        }
    };
    if !target_allowed(verb, target) {
        return Err(bad(
            "kind-not-allowed",
            column(target_token),
            format!("{name} does not take {kind_text}:"),
        ));
    }
    let problem = match target {
        Target::Host if !valid_host(value) => {
            Some(("bad-host", format!("not a valid host: {value}")))
        }
        Target::Site if !valid_host(value) => {
            Some(("bad-site", format!("not a valid site: {value}")))
        }
        Target::Label => label_problem(value).map(|(code, text)| (code, text.to_owned())),
        Target::Path => path_problem(value).map(|(code, text)| (code, text.to_owned())),
        Target::Css => css_problem(value).map(|(code, text)| (code, text.to_owned())),
        _ => None,
    };
    if let Some((code, message)) = problem {
        return Err(bad(code, column(target_token) + colon + 1, message));
    }

    let mut rule = Rule {
        line: 0,
        verb,
        target,
        value: value.to_owned(),
        types: TYPE_ALL,
        party: if matches!(verb, Verb::Block | Verb::Allow) {
            Party::Third
        } else {
            Party::Any
        },
        only: None,
        except: None,
    };

    let mut seen = [false; 4];
    let mut k = 2;
    while k < tokens.len() {
        let word = &tokens[k];
        let Some(qualifier) = QUALIFIERS.iter().position(|q| *q == word.text) else {
            return Err(if target == Target::Css {
                bad(
                    "css-space",
                    column(word),
                    format!(
                        "a selector cannot contain spaces; \"{}\" is not a qualifier",
                        word.text
                    ),
                )
            } else {
                bad(
                    "bad-qualifier",
                    column(word),
                    format!("unknown qualifier \"{}\"", word.text),
                )
            });
        };
        if !qualifier_allowed(verb, qualifier) {
            return Err(bad(
                "qualifier-not-allowed",
                column(word),
                format!("{name} does not take \"{}\"", word.text),
            ));
        }
        if seen[qualifier] {
            return Err(bad(
                "duplicate-qualifier",
                column(word),
                format!("\"{}\" appears more than once", word.text),
            ));
        }
        seen[qualifier] = true;
        let Some(arg) = tokens.get(k + 1) else {
            return Err(bad(
                "qualifier-no-value",
                column(word) + word.text.len(),
                format!("\"{}\" needs a value", word.text),
            ));
        };
        match qualifier {
            0 => {
                let mut mask = 0;
                for part in arg.text.split(',') {
                    let Some(bit) = TYPE_NAMES.iter().position(|n| *n == part) else {
                        return Err(bad(
                            "bad-type",
                            column(arg),
                            format!("unknown request kind \"{part}\""),
                        ));
                    };
                    mask |= 1 << bit;
                }
                rule.types = mask;
            }
            1 => {
                rule.party = match arg.text {
                    "any" => Party::Any,
                    "first" => Party::First,
                    "third" => Party::Third,
                    _ => {
                        return Err(bad(
                            "bad-party",
                            column(arg),
                            "party must be first, third or any",
                        ));
                    }
                };
            }
            _ => {
                let hosts = parse_host_list(arg.text, word.text, column(arg))?;
                if qualifier == 2 {
                    rule.only = Some(hosts);
                } else {
                    rule.except = Some(hosts);
                }
            }
        }
        k += 2;
    }

    let key = tokens
        .iter()
        .map(|t| t.text)
        .collect::<Vec<_>>()
        .join(" ");
    Ok((rule, key))
}

fn parse_host_list(text: &str, what: &str, column: usize) -> Result<Vec<String>, Bad> {
    let parts: Vec<&str> = text.split(',').collect();
    if parts.len() > MAX_HOST_LIST {
        return Err(bad(
            "list-too-long",
            column,
            format!("{what} takes at most {MAX_HOST_LIST} hosts"),
        ));
    }
    for part in &parts {
        if !valid_host(part) {
            return Err(bad(
                "bad-host",
                column,
                format!("not a valid host in {what}: {part}"),
            ));
        }
    }
    Ok(parts.into_iter().map(str::to_owned).collect())
}

/// A lowercase DNS name with at least two labels and four characters.
pub fn valid_host(value: &str) -> bool {
    if value.len() < 4 || value.len() > 253 {
        return false;
    }
    let mut labels = 0;
    for label in value.split('.') {
        labels += 1;
        let bytes = label.as_bytes();
        let edge = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
        if bytes.is_empty()
            || bytes.len() > 63
            || !edge(bytes[0])
            || !edge(bytes[bytes.len() - 1])
            || !bytes.iter().all(|&b| edge(b) || b == b'-')
        {
            return false;
        }
    }
    labels >= 2
}

fn label_problem(value: &str) -> Option<(&'static str, &'static str)> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || !bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
    {
        return Some(("bad-label", "a label may only use a-z, 0-9 and -"));
    }
    if bytes.len() < 4 {
        return Some(("label-too-short", "a label must be at least 4 characters"));
    }
    if bytes.len() > 40 {
        return Some(("bad-label", "a label can be at most 40 characters"));
    }
    if bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
        return Some(("bad-label", "a label cannot start or end with -"));
    }
    if bytes.iter().all(u8::is_ascii_digit) {
        return Some(("label-all-digits", "a label cannot be all digits"));
    }
    if !bytes.iter().any(u8::is_ascii_lowercase) {
        return Some(("bad-label", "a label needs at least one letter"));
    }
    None
}

fn path_problem(value: &str) -> Option<(&'static str, &'static str)> {
    if !value.starts_with('/') {
        return Some(("path-no-slash", "a path must start with /"));
    }
    let length = value.encode_utf16().count();
    if !(3..=200).contains(&length) {
        return Some(("bad-path", "a path must be 3 to 200 characters"));
    }
    if value
        .bytes()
        .any(|b| !(0x21..=0x7e).contains(&b) || matches!(b, b'#' | b'"' | b'\\'))
    {
        return Some((
            "bad-path",
            "a path may not contain spaces, #, \" or \\ or non-ASCII characters",
        ));
    }
    None
}

fn css_problem(value: &str) -> Option<(&'static str, &'static str)> {
    if value.contains(',') {
        return Some((
            "css-comma",
            "a selector cannot contain a comma; use one rule per selector",
        ));
    }
    if value.contains(['{', '}', ';']) {
        return Some(("bad-css", "a selector cannot contain { } or ;"));
    }
    if !value.contains(['#', '.', '[']) {
        return Some((
            "css-too-broad",
            "selector must be narrowed by an id, class or attribute",
        ));
    }
    let length = value.encode_utf16().count();
    if !(3..=200).contains(&length) {
        return Some(("css-length", "a selector must be 3 to 200 characters"));
    }
    if value.chars().any(|c| c <= ' ' || c == '\u{7f}') {
        return Some((
            "bad-css",
            "a selector cannot contain spaces or control characters",
        ));
    }
    None
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    Pass,
    Allow,
    Block,
    Off,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Decision {
    pub verdict: Verdict,
    /// The file line of the rule that decided, 0 for `Pass`.
    pub line: usize,
}

/// Rule indexes for one verb (`block` or `allow`), each list in rule order.
#[derive(Debug, Default)]
struct Index {
    hosts: HashMap<String, Vec<usize>>,
    labels: Vec<usize>,
    /// Rule index and the lowercased path fragment.
    paths: Vec<(usize, String)>,
}

impl Index {
    fn add(&mut self, rule: &Rule, index: usize) {
        match rule.target {
            Target::Host => self.hosts.entry(rule.value.clone()).or_default().push(index),
            Target::Label => self.labels.push(index),
            _ => self.paths.push((index, rule.value.to_ascii_lowercase())),
        }
    }

    fn has_paths(&self) -> bool {
        !self.paths.is_empty()
    }

    /// The lowest rule index that matches the request and passes its qualifiers.
    fn first_match(
        &self,
        rules: &[Rule],
        request: &Request<'_>,
        path: Option<&str>,
    ) -> Option<usize> {
        let mut best: Option<usize> = None;
        for suffix in host_suffixes(request.host) {
            if let Some(ids) = self.hosts.get(suffix)
                && let Some(&found) = ids.iter().find(|&&id| applies(&rules[id], request))
            {
                best = Some(best.map_or(found, |b| b.min(found)));
            }
        }
        for &id in &self.labels {
            if best.is_some_and(|b| id >= b) {
                break;
            }
            if request.host.split('.').any(|label| label.contains(rules[id].value.as_str()))
                && applies(&rules[id], request)
            {
                best = Some(id);
                break;
            }
        }
        if let Some(path) = path {
            for (id, needle) in &self.paths {
                if best.is_some_and(|b| *id >= b) {
                    break;
                }
                if path.contains(needle.as_str()) && applies(&rules[*id], request) {
                    best = Some(*id);
                    break;
                }
            }
        }
        best
    }
}

/// What a rule is checked against: the request host, the kinds asked about, whether it is
/// first-party to the page, and the page's site.
struct Request<'a> {
    host: &'a str,
    site: &'a str,
    kinds: u16,
    first: bool,
}

fn applies(rule: &Rule, request: &Request<'_>) -> bool {
    if rule.types & request.kinds == 0 {
        return false;
    }
    match rule.party {
        Party::First if !request.first => return false,
        Party::Third if request.first => return false,
        _ => {}
    }
    if let Some(except) = &rule.except
        && except.iter().any(|h| host_matches(request.site, h))
    {
        return false;
    }
    if let Some(only) = &rule.only
        && !only.iter().any(|h| host_matches(request.site, h))
    {
        return false;
    }
    true
}

/// `host` equals `value` or ends with `.` plus `value`: the label-boundary suffix test.
fn host_matches(host: &str, value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    host == value
        || (host.len() > value.len()
            && host.ends_with(value)
            && host.as_bytes()[host.len() - value.len() - 1] == b'.')
}

fn same_party(a: &str, b: &str) -> bool {
    host_matches(a, b) || host_matches(b, a)
}

fn label_count(host: &str) -> usize {
    host.bytes().filter(|&b| b == b'.').count() + 1
}

/// `a.b.c` then `b.c` then `c`: every suffix that starts on a label boundary.
pub(crate) fn host_suffixes(host: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(host), |h| h.split_once('.').map(|(_, rest)| rest))
}

/// Lowercase, without one trailing dot.
fn normalise_site(site: &str) -> Cow<'_, str> {
    let site = site.strip_suffix('.').unwrap_or(site);
    if site.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(site.to_ascii_lowercase())
    } else {
        Cow::Borrowed(site)
    }
}

/// Splits an `http(s)` URL into (lowercase host, the path-and-query text with no fragment, which
/// is empty or starts with `/` or `?`). `None` for other schemes, IPv6 literals, an empty host and
/// anything that is not a URL.
fn split_url(url: &str) -> Option<(Cow<'_, str>, &str)> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| {
            let (scheme, rest) = url.split_once("://")?;
            (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
                .then_some(rest)
        })?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let authority = authority.rsplit('@').next()?;
    if authority.starts_with('[') {
        return None;
    }
    let host = authority.split(':').next()?;
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() {
        return None;
    }
    let tail = tail.split('#').next().unwrap_or("");
    let host = if host.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(host.to_ascii_lowercase())
    } else {
        Cow::Borrowed(host)
    };
    Some((host, tail))
}

/// The path and query, lowercased and capped, the text path rules are matched against.
fn path_text(tail: &str) -> String {
    let mut end = tail.len().min(MAX_PATH_TEXT_BYTES);
    while !tail.is_char_boundary(end) {
        end -= 1;
    }
    let tail = &tail[..end];
    let mut text = String::with_capacity(tail.len() + 1);
    if !tail.starts_with('/') {
        text.push('/');
    }
    text.push_str(tail);
    text.make_ascii_lowercase();
    text
}

#[derive(Debug, Default)]
pub struct Shield {
    rules: Vec<Rule>,
    errors: Vec<Issue>,
    block: Index,
    allow: Index,
    /// `off site:` value to the lowest rule index that names it.
    off: HashMap<String, usize>,
}

impl Shield {
    /// Compiles several lists together: each is parsed under its own limits, then the rules are
    /// concatenated in order, so an earlier list wins a tie.
    pub fn compile(lists: &[&str]) -> Self {
        let mut shield = Self::default();
        for list in lists {
            let parsed = parse_list(list);
            shield.errors.extend(parsed.errors);
            for rule in parsed.rules {
                let index = shield.rules.len();
                match rule.verb {
                    Verb::Block => shield.block.add(&rule, index),
                    Verb::Allow => shield.allow.add(&rule, index),
                    Verb::Off => {
                        shield.off.entry(rule.value.clone()).or_insert(index);
                    }
                    Verb::Hide => {}
                }
                shield.rules.push(rule);
            }
        }
        shield
    }

    /// The starter list followed by the user's list, if any.
    pub fn with_starter(user: Option<&str>) -> Self {
        match user {
            Some(user) => Self::compile(&[STARTER_RULES, user]),
            None => Self::compile(&[STARTER_RULES]),
        }
    }

    /// Errors from every list, in order.
    pub fn errors(&self) -> &[Issue] {
        &self.errors
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// `kinds` is a bit set of request kinds (see [`RequestKind::mask`], [`TYPE_ALL`]).
    /// `honor_off` is false for private tabs, which ignore `off site:` rules.
    pub fn decide(&self, url: &str, site_host: &str, kinds: u16, honor_off: bool) -> Decision {
        const PASS: Decision = Decision {
            verdict: Verdict::Pass,
            line: 0,
        };
        let Some((host, tail)) = split_url(url) else {
            return PASS;
        };
        let site = normalise_site(site_host);
        if label_count(&host) > MAX_HOST_LABELS || label_count(&site) > MAX_HOST_LABELS {
            return PASS;
        }
        if honor_off
            && let Some(index) = host_suffixes(&site)
                .filter_map(|suffix| self.off.get(suffix))
                .min()
        {
            return Decision {
                verdict: Verdict::Off,
                line: self.rules[*index].line,
            };
        }
        let request = Request {
            host: &host,
            site: &site,
            kinds,
            first: same_party(&host, &site),
        };
        let path = (self.allow.has_paths() || self.block.has_paths()).then(|| path_text(tail));
        for (index, verdict) in [
            (self.allow.first_match(&self.rules, &request, path.as_deref()), Verdict::Allow),
            (self.block.first_match(&self.rules, &request, path.as_deref()), Verdict::Block),
        ] {
            if let Some(index) = index {
                return Decision {
                    verdict,
                    line: self.rules[index].line,
                };
            }
        }
        PASS
    }

    fn hide_rules(&self) -> impl Iterator<Item = &Rule> {
        self.rules.iter().filter(|rule| rule.verb == Verb::Hide)
    }

    /// The selectors that apply on a top-level site: except is tested before only.
    pub fn hidden_selectors(&self, site_host: &str) -> Vec<String> {
        let site = normalise_site(site_host);
        let mut selectors: Vec<String> = Vec::new();
        for rule in self.hide_rules() {
            if rule.except.as_ref().is_some_and(|e| e.iter().any(|h| host_matches(&site, h))) {
                continue;
            }
            if rule.only.as_ref().is_some_and(|o| !o.iter().any(|h| host_matches(&site, h))) {
                continue;
            }
            if !selectors.contains(&rule.value) {
                selectors.push(rule.value.clone());
            }
        }
        selectors
    }

    /// The document-start script that hides the compiled `hide` rules, or `None` when there are
    /// none. `skip_sites` are further sites the script must leave alone (the user's exceptions);
    /// `off site:` rules are added to them unless `honor_off` is false.
    ///
    /// The script is a function of `location.hostname` and the embedded data only. It makes one
    /// stylesheet and inserts each rule on its own, so a selector the browser rejects costs only
    /// itself, and CSSOM insertion is not subject to a page's style Content-Security-Policy. It
    /// runs in the top frame of http(s) pages.
    pub fn cosmetic_script(&self, skip_sites: &[String], honor_off: bool) -> Option<String> {
        let mut rows: Vec<(&Rule, String)> = Vec::new();
        for rule in self.hide_rules() {
            let key = format!("{:?}{:?}{}", rule.only, rule.except, rule.value);
            if !rows.iter().any(|(_, seen)| *seen == key) {
                rows.push((rule, key));
            }
        }
        if rows.is_empty() {
            return None;
        }
        let mut skip: Vec<&str> = skip_sites.iter().map(String::as_str).collect();
        if honor_off {
            let mut off: Vec<&str> = self.off.keys().map(String::as_str).collect();
            off.sort_unstable();
            skip.extend(off);
        }
        skip.sort_unstable();
        skip.dedup();
        let mut script = String::from(SCRIPT_HEAD);
        script.push_str(&js_array(&skip));
        script.push_str(";var R=[");
        for (n, (rule, _)) in rows.iter().enumerate() {
            if n > 0 {
                script.push(',');
            }
            script.push('[');
            script.push_str(&js_string(&rule.value));
            for list in [&rule.only, &rule.except] {
                script.push(',');
                match list {
                    Some(list) => script.push_str(&js_array(list)),
                    None => script.push('0'),
                }
            }
            script.push(']');
        }
        script.push(']');
        script.push_str(SCRIPT_TAIL);
        Some(script)
    }
}

/// The start of the cosmetic script, up to the skip list `Q`. Page APIs used: `window`,
/// `location`, `document` and `CSSStyleSheet`. No network, no timers, no observers.
const SCRIPT_HEAD: &str = "(function(){try{if(window.top!==window||(location.protocol!==\"http:\"&&location.protocol!==\"https:\"))return;var h=location.hostname.toLowerCase();if(h.slice(-1)===\".\")h=h.slice(0,-1);function m(v){return h===v||h.slice(-v.length-1)===\".\"+v}var Q=";

/// The rest, after the rule table `R`: `[selector, only|0, except|0]`.
const SCRIPT_TAIL: &str = ";if(Q.some(m))return;var c=[];R.forEach(function(r){if(r[2]&&r[2].some(m))return;if(r[1]&&!r[1].some(m))return;c.push(r[0]+\"{display:none!important}\")});if(!c.length)return;function f(s){c.forEach(function(x){try{s.insertRule(x,s.cssRules.length)}catch(e){}})}var d=document,e=d.documentElement;if(e){var s=d.createElement(\"style\");e.appendChild(s);if(s.sheet)f(s.sheet)}else if(d.adoptedStyleSheets&&window.CSSStyleSheet){var t=new CSSStyleSheet();f(t);d.adoptedStyleSheets=[t]}}catch(e){}})();";

fn js_array<S: AsRef<str>>(items: &[S]) -> String {
    let items: Vec<String> = items.iter().map(|item| js_string(item.as_ref())).collect();
    format!("[{}]", items.join(","))
}

/// A JavaScript double-quoted string literal for `text` that cannot end early or close a script.
fn js_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '<' => out.push_str("\\u003c"),
            c if c < ' ' || matches!(c, '\u{7f}' | '\u{2028}' | '\u{2029}') => {
                out.push_str(&format!("\\u{:04x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: &str = "news.example";

    fn run(rules: &[&str], url: &str, site: &str, kind: RequestKind) -> (Verdict, usize) {
        let src = rules.join("\n");
        let shield = Shield::compile(&[&src]);
        let d = shield.decide(url, site, kind.mask(), true);
        (d.verdict, d.line)
    }

    fn blocks(rules: &[&str], url: &str, site: &str) -> bool {
        run(rules, url, site, RequestKind::Script).0 == Verdict::Block
    }

    fn errors_of(src: &str) -> Vec<(usize, &'static str)> {
        parse_list(src)
            .errors
            .iter()
            .map(|e| (e.line, e.code))
            .collect()
    }

    // ---- host, label and path matching ----------------------------------------------------------

    #[test]
    fn host_rule_matches_the_host_and_every_subdomain() {
        let rules = ["block host:ad-delivery.example"];
        for url in [
            "https://ad-delivery.example/x",
            "https://img.ad-delivery.example/x",
            "https://a.b.c.ad-delivery.example/x",
            "https://ad-delivery.example:8443/x",
            "https://ad-delivery.example./x",
            "HTTPS://AD-DELIVERY.EXAMPLE/X",
            "https://user:pw@ad-delivery.example/x",
        ] {
            assert_eq!(run(&rules, url, SITE, RequestKind::Script), (Verdict::Block, 1), "{url}");
        }
    }

    #[test]
    fn host_rule_stops_on_the_label_boundary() {
        let rules = ["block host:ad-delivery.example"];
        for url in [
            "https://notad-delivery.example/x",
            "https://ad-delivery.example.co/x",
            "https://addelivery.example/x",
            "https://other.example/ad-delivery.example/y",
        ] {
            assert_eq!(run(&rules, url, SITE, RequestKind::Script).0, Verdict::Pass, "{url}");
        }
    }

    #[test]
    fn label_rule_matches_inside_one_label_and_cannot_straddle_a_dot() {
        let rules = ["block label:click-track"];
        assert!(blocks(&rules, "https://eu.click-tracker.example/a", SITE));
        assert!(blocks(&rules, "https://click-track.example/a", SITE));
        assert!(blocks(&rules, "https://x.preclick-track9.example/a", SITE));
        assert!(!blocks(&rules, "https://click.track.example/a", SITE));
        assert!(!blocks(&rules, "https://x.example/click-track/a", SITE), "not in the path");
    }

    #[test]
    fn path_rule_matches_path_and_query_case_insensitively_but_never_host_or_fragment() {
        let rules = ["block path:/ad-slot/"];
        assert!(blocks(&rules, "https://x.example/AD-SLOT/1", SITE));
        assert!(blocks(&rules, "https://x.example/a?u=/ad-slot/", SITE));
        assert!(!blocks(&rules, "https://x.example/a#/ad-slot/", SITE), "fragment");
        assert!(!blocks(&rules, "https://ad-slot.example/x", SITE), "host");
        assert!(blocks(&["block path:/AD-Slot/"], "https://x.example/ad-slot/1", SITE));
        assert!(blocks(&["block path:/collect?id="], "https://x.example/collect?id=7", SITE));
        assert!(blocks(&rules, "https://x.example?u=/ad-slot/", SITE), "query with no path");
    }

    #[test]
    fn path_text_beyond_the_cap_is_not_matched() {
        let rules = ["block path:/zz-ad/"];
        let near = format!("https://x.example/{}/zz-ad/1", "p".repeat(500));
        assert!(blocks(&rules, &near, SITE));
        let far = format!("https://x.example/{}/zz-ad/1", "p".repeat(MAX_PATH_TEXT_BYTES));
        assert!(!blocks(&rules, &far, SITE));
    }

    // ---- party, kind, only and except -----------------------------------------------------------

    #[test]
    fn party_defaults_to_third_and_a_site_is_its_own_first_party() {
        let rules = ["block host:a.example"];
        assert!(!blocks(&rules, "https://a.example/x", "a.example"));
        assert!(!blocks(&rules, "https://cdn.a.example/x", "a.example"));
        assert!(!blocks(&rules, "https://a.example/x", "cdn.a.example"));
        assert!(blocks(&rules, "https://a.example/x", "b.example"));
        assert!(blocks(&rules, "https://a.example/x", ""), "unknown site: everything is third party");
        assert!(
            blocks(&["block host:x.example"], "https://x.example/x", "www.example"),
            "a sibling subdomain is third party"
        );
    }

    #[test]
    fn party_any_and_first_are_honoured() {
        assert!(blocks(&["block host:a.example party any"], "https://a.example/x", "a.example"));
        assert!(blocks(&["block host:a.example party first"], "https://a.example/x", "a.example"));
        assert!(!blocks(&["block host:a.example party first"], "https://a.example/x", "b.example"));
        assert_eq!(
            run(
                &["allow host:a.example", "block host:a.example party any"],
                "https://a.example/x",
                "a.example",
                RequestKind::Script
            ),
            (Verdict::Block, 2),
            "allow defaults to third party too"
        );
    }

    #[test]
    fn kind_qualifier_limits_a_rule_to_those_request_kinds() {
        let rules = ["block host:a.example kind image"];
        assert_eq!(run(&rules, "https://a.example/x", "b.example", RequestKind::Script).0, Verdict::Pass);
        assert_eq!(run(&rules, "https://a.example/x", "b.example", RequestKind::Image).0, Verdict::Block);
        let several = ["block host:a.example kind xhr,font"];
        assert_eq!(run(&several, "https://a.example/x", "b.example", RequestKind::Font).0, Verdict::Block);
        assert_eq!(run(&several, "https://a.example/x", "b.example", RequestKind::Xhr).0, Verdict::Block);
        assert_eq!(run(&several, "https://a.example/x", "b.example", RequestKind::Ping).0, Verdict::Pass);
        let other = ["block host:a.example kind other"];
        assert_eq!(run(&other, "https://a.example/x", "b.example", RequestKind::Other).0, Verdict::Block);
    }

    #[test]
    fn request_kinds_map_to_distinct_bits_within_type_all() {
        let kinds = [
            RequestKind::Script,
            RequestKind::Image,
            RequestKind::Frame,
            RequestKind::Xhr,
            RequestKind::Media,
            RequestKind::Font,
            RequestKind::Style,
            RequestKind::Ping,
            RequestKind::Other,
        ];
        let mut seen = 0u16;
        for kind in kinds {
            assert_eq!(kind.mask().count_ones(), 1);
            assert_eq!(seen & kind.mask(), 0, "{kind:?} repeats a bit");
            seen |= kind.mask();
        }
        assert_eq!(seen, TYPE_ALL);
        assert_eq!(RequestKind::Xhr.mask(), 8);
        assert_eq!(RequestKind::Ping.mask(), 128);
        assert_eq!(RequestKind::Other.mask(), 256);
    }

    #[test]
    fn asking_with_every_kind_matches_a_kind_restricted_rule() {
        let shield = Shield::compile(&["block host:a.example kind image"]);
        assert_eq!(shield.decide("https://a.example/x", "b.example", TYPE_ALL, true).verdict, Verdict::Block);
        assert_eq!(
            shield.decide("https://a.example/x", "b.example", RequestKind::Script.mask(), true).verdict,
            Verdict::Pass
        );
    }

    #[test]
    fn only_and_except_use_label_boundary_suffixes_and_except_comes_first() {
        let only = ["block host:a.example only shop.example"];
        assert!(blocks(&only, "https://a.example/x", "www.shop.example"));
        assert!(blocks(&only, "https://a.example/x", "shop.example"));
        assert!(!blocks(&only, "https://a.example/x", "shop.example.net"));
        assert!(!blocks(&only, "https://a.example/x", "notshop.example"));
        let except = ["block host:a.example except shop.example"];
        assert!(!blocks(&except, "https://a.example/x", "www.shop.example"));
        assert!(blocks(&except, "https://a.example/x", "other.example"));
        let both = ["block host:a.example only shop.example except www.shop.example"];
        assert!(!blocks(&both, "https://a.example/x", "www.shop.example"), "except before only");
        assert!(blocks(&both, "https://a.example/x", "shop.example"));
        assert!(!blocks(&both, "https://a.example/x", "other.example"));
    }

    // ---- precedence -----------------------------------------------------------------------------

    #[test]
    fn off_beats_allow_and_allow_beats_block() {
        let all = ["off site:s.example", "allow host:a.example", "block host:a.example"];
        assert_eq!(run(&all, "https://a.example/x", "s.example", RequestKind::Script), (Verdict::Off, 1));
        assert_eq!(run(&all, "https://a.example/x", "www.s.example", RequestKind::Script), (Verdict::Off, 1));
        assert_eq!(run(&all, "https://a.example/x", "xs.example", RequestKind::Script), (Verdict::Allow, 2));
        assert_eq!(
            run(&["block host:a.example", "allow host:a.example"], "https://a.example/x", "s.example", RequestKind::Script),
            (Verdict::Allow, 2),
            "a later allow still beats an earlier block"
        );
        assert_eq!(
            run(&["allow label:adserver", "block host:adserver.example"], "https://adserver.example/x", "s.example", RequestKind::Script),
            (Verdict::Allow, 1)
        );
        assert_eq!(
            run(&["allow host:a.example only other.example", "block host:a.example"], "https://a.example/x", "s.example", RequestKind::Script),
            (Verdict::Block, 2),
            "an allow that does not apply lets the block through"
        );
    }

    #[test]
    fn off_stops_on_the_label_boundary_and_covers_first_party_requests() {
        let rules = ["off site:s.example", "block host:s.example party any"];
        assert_eq!(run(&rules, "https://s.example/x", "s.example", RequestKind::Script), (Verdict::Off, 1));
        assert_eq!(
            run(&rules, "https://s.example/x", "xs.example", RequestKind::Script),
            (Verdict::Block, 2),
            "xs.example is not s.example, so its off rule does not apply"
        );
        assert_eq!(run(&["off site:s.example"], "https://a.example/x", "s.example.net", RequestKind::Script).0, Verdict::Pass);
    }

    #[test]
    fn private_lookups_ignore_off_rules() {
        let shield = Shield::compile(&["off site:s.example\nblock host:a.example"]);
        assert_eq!(shield.decide("https://a.example/x", "s.example", TYPE_ALL, true).verdict, Verdict::Off);
        let private = shield.decide("https://a.example/x", "s.example", TYPE_ALL, false);
        assert_eq!((private.verdict, private.line), (Verdict::Block, 2));
    }

    #[test]
    fn the_lowest_line_decides_and_lines_count_comments_and_blanks() {
        assert_eq!(
            run(&["block label:adserver", "block host:adserver.example"], "https://adserver.example/x", "s.example", RequestKind::Script),
            (Verdict::Block, 1)
        );
        assert_eq!(
            run(&["# note", "", "block path:/zz-ad/", "block label:adserver"], "https://adserver.example/zz-ad/1", "s.example", RequestKind::Script),
            (Verdict::Block, 3)
        );
        assert_eq!(
            run(&["block host:a.example kind image", "block host:a.example"], "https://a.example/x", "s.example", RequestKind::Script),
            (Verdict::Block, 2),
            "an earlier rule that fails its qualifiers is skipped"
        );
        let two_lists = Shield::compile(&["block host:a.example", "block host:a.example kind script"]);
        assert_eq!(two_lists.decide("https://a.example/x", "s.example", RequestKind::Script.mask(), true).line, 1);
    }

    // ---- what is not a request, and the label limit ---------------------------------------------

    #[test]
    fn only_http_and_https_urls_are_considered() {
        let rules = ["block host:a.example party any", "off site:s.example"];
        for url in [
            "data:text/plain;base64,aGk=",
            "blob:https://a.example/1234",
            "about:blank",
            "ms-browser-extension://a.example/page.html",
            "ftp://a.example/x",
            "not a url",
            "https://[::1]/x",
            "https:///x",
        ] {
            assert_eq!(run(&rules, url, "t.example", RequestKind::Image).0, Verdict::Pass, "{url}");
            assert_eq!(run(&rules, url, "s.example", RequestKind::Image).0, Verdict::Pass, "{url}");
        }
    }

    #[test]
    fn hosts_and_sites_over_24_labels_pass_without_a_walk() {
        let rules = ["block label:adserver"];
        let ok = "https://adserver.b.c.d.e.f.g.h.i.j.k.l.m.n.o.p.q.r.s.t.u.v.w.x/x";
        let too_many = "https://adserver.a.b.c.d.e.f.g.h.i.j.k.l.m.n.o.p.q.r.s.t.u.v.w.x/x";
        assert!(blocks(&rules, ok, SITE));
        assert!(!blocks(&rules, too_many, SITE));
        let deep_site = "a.b.c.d.e.f.g.h.i.j.k.l.m.n.o.p.q.r.s.t.u.v.w.x.example";
        assert!(!blocks(&["block host:a.example"], "https://a.example/x", deep_site));
    }

    #[test]
    fn hide_rules_never_decide_a_request_and_an_empty_list_passes() {
        assert_eq!(run(&["hide css:div.ad"], "https://a.example/x", "s.example", RequestKind::Script).0, Verdict::Pass);
        assert_eq!(run(&[], "https://a.example/x", "s.example", RequestKind::Script).0, Verdict::Pass);
    }

    // ---- the parser -----------------------------------------------------------------------------

    #[test]
    fn a_bad_line_is_skipped_and_the_others_still_decide() {
        let rules = ["block host:nodots", "block host:a.example"];
        assert_eq!(run(&rules, "https://a.example/x", "s.example", RequestKind::Script), (Verdict::Block, 2));
        let parsed = parse_list(&rules.join("\n"));
        assert_eq!(parsed.rules.len(), 1);
        assert_eq!(parsed.errors.len(), 1);
        assert_eq!((parsed.errors[0].line, parsed.errors[0].code), (1, "bad-host"));
        assert!(!parsed.errors[0].message.is_empty());
    }

    #[test]
    fn a_rule_gets_the_default_party_and_all_kinds() {
        let parsed = parse_list("block host:a.example\nallow label:click-track\nhide css:div.a\noff site:bank.example");
        assert!(parsed.errors.is_empty());
        let parties: Vec<Party> = parsed.rules.iter().map(|r| r.party).collect();
        assert_eq!(parties, [Party::Third, Party::Third, Party::Any, Party::Any]);
        assert!(parsed.rules.iter().all(|r| r.types == TYPE_ALL));
        assert_eq!(parsed.rules[0].only, None);
        assert_eq!(parsed.rules[0].except, None);
    }

    #[test]
    fn qualifiers_may_come_in_any_order() {
        let a = parse_list("block host:a.example only b.example party first kind script,image");
        let b = parse_list("block host:a.example kind script,image party first only b.example");
        assert!(a.errors.is_empty() && b.errors.is_empty());
        assert_eq!(a.rules, b.rules);
        assert_eq!(a.rules[0].types, 3);
        assert_eq!(a.rules[0].party, Party::First);
        assert_eq!(a.rules[0].only, Some(vec!["b.example".to_owned()]));
    }

    #[test]
    fn spacing_comments_and_line_endings_are_tolerated() {
        let parsed = parse_list("# first\r\n\r\n   \r\n  # indented\r\nblock    host:a.example    party   any   \r\nallow host:b.example\r\n");
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
        let lines: Vec<usize> = parsed.rules.iter().map(|r| r.line).collect();
        assert_eq!(lines, [5, 6]);
        assert_eq!(parsed.rules[0].party, Party::Any);
    }

    #[test]
    fn a_hash_inside_a_selector_is_kept() {
        let parsed = parse_list("hide css:div#sidebar>iframe[src*=\"/ad-slot/\"]\nhide css:[data-widget-type=\"sponsored\"]");
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
        assert_eq!(parsed.rules[0].value, "div#sidebar>iframe[src*=\"/ad-slot/\"]");
        assert_eq!(parsed.rules[1].target, Target::Css);
    }

    #[test]
    fn a_selector_containing_a_brace_or_semicolon_is_rejected() {
        let parsed = parse_list("hide css:div.a{color:red}\nhide css:div.b}\nhide css:div.c;color:red\nhide css:div.d{");
        assert!(parsed.rules.is_empty());
        assert_eq!(parsed.errors.len(), 4);
        for issue in &parsed.errors {
            assert_eq!(issue.code, "bad-css");
            assert_eq!(issue.message, "a selector cannot contain { } or ;");
        }
    }

    #[test]
    fn selector_shapes_that_are_too_broad_or_unsafe_are_rejected() {
        for (src, code) in [
            ("hide css:div", "css-too-broad"),
            ("hide css:*", "css-too-broad"),
            ("hide css:body", "css-too-broad"),
            ("hide css:div[a],span[b]", "css-comma"),
            ("hide css:div[a] > iframe", "css-space"),
            ("hide css:div.a\u{1}b", "bad-css"),
            ("hide css:d", "css-too-broad"),
            ("block css:div[a]", "kind-not-allowed"),
            ("hide host:a.example", "kind-not-allowed"),
            ("off path:/a/b/", "kind-not-allowed"),
            ("hide css:div[a] kind script", "qualifier-not-allowed"),
            ("hide css:div[a] party any", "qualifier-not-allowed"),
            ("off site:a.example only b.example", "qualifier-not-allowed"),
        ] {
            assert_eq!(errors_of(src), [(1, code)], "{src:?}");
        }
        let long = format!("hide css:div.{}", "a".repeat(200));
        assert_eq!(errors_of(&long), [(1, "css-length")]);
    }

    #[test]
    fn lexical_rules_for_hosts_labels_and_paths() {
        for (src, code) in [
            ("block host:example", "bad-host"),
            ("block host:-a.example", "bad-host"),
            ("block host:a-.example", "bad-host"),
            ("block host:A.example", "bad-host"),
            ("block host:a..example", "bad-host"),
            ("block host:a.b", "bad-host"),
            ("block host:a_b.example", "bad-host"),
            ("off site:localhost", "bad-site"),
            ("block label:ad", "label-too-short"),
            ("block label:ads", "label-too-short"),
            ("block label:1234", "label-all-digits"),
            ("block label:click.track", "bad-label"),
            ("block label:-adserver", "bad-label"),
            ("block label:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "bad-label"),
            ("block path:ad-slot", "path-no-slash"),
            ("block path:/a", "bad-path"),
            ("block path:/a\"b", "bad-path"),
            ("block path:/a#b", "bad-path"),
            ("block path:/a\\b", "bad-path"),
        ] {
            assert_eq!(errors_of(src), [(1, code)], "{src:?}");
        }
        assert!(errors_of("block label:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").is_empty(), "40 is allowed");
        assert!(errors_of("block host:192.168.1.1").is_empty());
        assert!(errors_of("block path:/metrics/collect?id= party any").is_empty());
    }

    #[test]
    fn syntax_errors_name_the_problem() {
        for (src, code, column) in [
            ("Block host:a.example", "bad-verb", 1),
            ("deny host:a.example", "bad-verb", 1),
            ("block", "bad-target", 6),
            ("block host", "bad-target", 7),
            ("block domain:a.example", "bad-kind", 7),
            ("block Host:a.example", "bad-kind", 7),
            ("block host:a.example party both", "bad-party", 28),
            ("block host:a.example kind script,bogus", "bad-type", 27),
            ("block host:a.example kind script,", "bad-type", 27),
            ("block host:a.example only b.example,nodots", "bad-host", 27),
            ("block host:a.example party any party first", "duplicate-qualifier", 32),
            ("block host:a.example bogus x", "bad-qualifier", 22),
            ("block host:a.example only", "qualifier-no-value", 26),
            ("block\thost:a.example", "tab", 6),
            (" block host:a.example", "leading-space", 1),
        ] {
            let parsed = parse_list(src);
            assert!(parsed.rules.is_empty(), "{src:?}");
            assert_eq!(parsed.errors.len(), 1, "{src:?}");
            assert_eq!(parsed.errors[0].code, code, "{src:?}");
            assert_eq!(parsed.errors[0].column, column, "{src:?} column");
        }
    }

    #[test]
    fn host_lists_are_limited_to_16_entries() {
        let hosts = |n: usize| (0..n).map(|i| format!("h{i}.example")).collect::<Vec<_>>().join(",");
        assert_eq!(errors_of(&format!("block host:a.example only {}", hosts(17))), [(1, "list-too-long")]);
        let ok = parse_list(&format!("block host:a.example only {}", hosts(16)));
        assert!(ok.errors.is_empty());
        assert_eq!(ok.rules[0].only.as_ref().map(Vec::len), Some(16));
        assert_eq!(errors_of(&format!("hide css:div.a except {}", hosts(17))), [(1, "list-too-long")]);
    }

    #[test]
    fn a_line_over_512_characters_is_rejected_without_stopping_the_parse() {
        let long = format!("block label:{}", "a".repeat(501));
        assert_eq!(long.len(), 513);
        let src = format!("block host:a.example\n{long}\nblock host:b.example");
        let parsed = parse_list(&src);
        assert_eq!(parsed.rules.len(), 2);
        assert_eq!(parsed.errors.iter().map(|e| (e.line, e.code)).collect::<Vec<_>>(), [(2, "line-too-long")]);
        let fits = parse_list(&format!("block label:{} only {}.example", "b".repeat(40), "c".repeat(100)));
        assert!(fits.errors.iter().all(|e| e.code != "line-too-long"));
    }

    #[test]
    fn the_2001st_rule_stops_the_parse_and_later_lines_are_not_read() {
        let mut lines: Vec<String> = (0..2001).map(|i| format!("block host:h{i}.example")).collect();
        let exact = parse_list(&lines[..2000].join("\n"));
        assert!(exact.errors.is_empty());
        assert_eq!(exact.rules.len(), 2000);
        lines.push("block host:nodots".to_owned());
        let parsed = parse_list(&lines.join("\n"));
        assert_eq!(parsed.rules.len(), 2000);
        assert_eq!(parsed.errors.iter().map(|e| (e.line, e.code)).collect::<Vec<_>>(), [(2001, "too-many-rules")]);
    }

    #[test]
    fn duplicates_warn_and_do_not_count_toward_the_rule_limit() {
        let mut lines: Vec<String> = (0..2000).map(|i| format!("block host:h{i}.example")).collect();
        lines.push("block   host:h0.example".to_owned());
        let parsed = parse_list(&lines.join("\n"));
        assert!(parsed.errors.is_empty());
        assert_eq!(parsed.rules.len(), 2000);
        assert_eq!(parsed.warnings.len(), 1);
        assert_eq!((parsed.warnings[0].line, parsed.warnings[0].code), (2001, "duplicate-rule"));
    }

    #[test]
    fn the_501st_off_rule_stops_the_parse() {
        let src: Vec<String> = (0..501).map(|i| format!("off site:s{i}.example")).collect();
        let parsed = parse_list(&src.join("\n"));
        assert_eq!(parsed.rules.len(), 500);
        assert_eq!(parsed.errors.iter().map(|e| (e.line, e.code)).collect::<Vec<_>>(), [(501, "too-many-off")]);
    }

    #[test]
    fn text_over_256_kib_stops_at_the_line_that_passes_it() {
        // Line 1 is 20 bytes; every other line is 100 bytes plus a newline.
        let filler = format!("# {}", "x".repeat(98));
        let mut lines = vec![filler; 3000];
        lines[0] = "block host:a.example".to_owned();
        let parsed = parse_list(&lines.join("\n"));
        assert_eq!(parsed.errors.iter().map(|e| (e.line, e.code)).collect::<Vec<_>>(), [(2597, "list-too-large")]);
        assert_eq!(parsed.rules.len(), 1);
        assert!(parse_list(&"#".repeat(MAX_LIST_BYTES)).errors.is_empty(), "exactly 256 KiB is accepted");
        let multibyte = format!("#{}", "é".repeat(131_072));
        assert_eq!(errors_of(&multibyte), [(1, "list-too-large")], "bytes, not characters");
    }

    #[test]
    fn parsing_gives_up_after_100_recorded_errors() {
        let hundred = vec!["bogus"; 100].join("\n");
        let parsed = parse_list(&format!("{hundred}\nblock host:a.example"));
        assert_eq!(parsed.errors.len(), 100);
        assert_eq!(parsed.rules.len(), 1);
        let more = vec!["bogus"; 150].join("\n");
        let parsed = parse_list(&more);
        assert_eq!(parsed.errors.len(), 101);
        assert_eq!((parsed.errors[100].line, parsed.errors[100].code), (101, "too-many-errors"));
    }

    // ---- compiling lists together and the starter list ------------------------------------------

    #[test]
    fn lists_are_parsed_under_their_own_limits_and_the_earlier_one_wins_a_tie() {
        let first = "block host:a.example";
        let second = "block host:a.example kind script\nblock host:b.example";
        let shield = Shield::compile(&[first, second]);
        assert_eq!(shield.rule_count(), 3);
        assert_eq!(shield.decide("https://a.example/", "s.example", RequestKind::Script.mask(), true).line, 1);
        assert_eq!(shield.decide("https://b.example/", "s.example", RequestKind::Script.mask(), true).line, 2);
        let bad_first = Shield::compile(&["nonsense", "block host:c.example"]);
        assert_eq!(bad_first.errors().len(), 1);
        assert_eq!(bad_first.rule_count(), 1);
    }

    #[test]
    fn the_starter_list_compiles_clean_and_is_generic() {
        let parsed = parse_list(STARTER_RULES);
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        let count = |verb: Verb| parsed.rules.iter().filter(|r| r.verb == verb).count();
        assert_eq!((count(Verb::Block), count(Verb::Hide), count(Verb::Allow), count(Verb::Off)), (38, 10, 0, 0));
        assert!(
            !parsed.rules.iter().any(|r| r.target == Target::Host),
            "the starter list names no vendor host"
        );
    }

    #[test]
    fn the_starter_list_blocks_ad_shaped_third_party_urls_and_spares_first_party() {
        let shield = Shield::with_starter(None);
        assert!(shield.errors().is_empty());
        let kind = RequestKind::Script.mask();
        assert_eq!(shield.decide("https://adserver.cdn.example/x.js", SITE, kind, true).verdict, Verdict::Block);
        assert_eq!(shield.decide("https://x.example/ad-slot/1", SITE, kind, true).verdict, Verdict::Block);
        assert_eq!(
            shield.decide("https://x.example/pixel.gif", SITE, RequestKind::Image.mask(), true).verdict,
            Verdict::Block
        );
        assert_eq!(
            shield.decide("https://x.example/pixel.gif", SITE, kind, true).verdict,
            Verdict::Pass,
            "that rule is for images only"
        );
        assert_eq!(shield.decide("https://adserver.news.example/x.js", SITE, kind, true).verdict, Verdict::Pass);
        assert_eq!(shield.decide("https://cdn.example/lib.js", SITE, kind, true).verdict, Verdict::Pass);
    }

    #[test]
    fn user_rules_follow_the_starter_list() {
        let shield = Shield::with_starter(Some("allow label:adserver only friend.example\nblock host:extra.example"));
        let kind = RequestKind::Script.mask();
        assert_eq!(shield.decide("https://extra.example/x", SITE, kind, true).verdict, Verdict::Block);
        assert_eq!(shield.decide("https://adserver.cdn.example/x", "www.friend.example", kind, true).verdict, Verdict::Allow);
        assert_eq!(shield.decide("https://adserver.cdn.example/x", SITE, kind, true).verdict, Verdict::Block);
    }

    // ---- cosmetic rules -------------------------------------------------------------------------

    #[test]
    fn hidden_selectors_apply_by_site_with_except_before_only_and_no_repeats() {
        let shield = Shield::compile(&[
            "hide css:div.a\nhide css:div.b only x.example,y.example\nhide css:div.c except z.example\nhide css:div.d only x.example except www.x.example\nhide css:div.a\nblock host:a.example",
            "hide css:div.a\nhide css:div.other",
        ]);
        let mut on_news = shield.hidden_selectors("news.example");
        on_news.sort();
        assert_eq!(on_news, ["div.a", "div.c", "div.other"]);
        let on_x = shield.hidden_selectors("x.example");
        assert!(on_x.contains(&"div.b".to_owned()) && on_x.contains(&"div.d".to_owned()));
        let on_www_x = shield.hidden_selectors("WWW.X.Example.");
        assert!(on_www_x.contains(&"div.b".to_owned()));
        assert!(!on_www_x.contains(&"div.d".to_owned()), "except before only");
        assert!(!shield.hidden_selectors("z.example").contains(&"div.c".to_owned()));
        assert!(!shield.hidden_selectors("notx.example").contains(&"div.b".to_owned()));
        assert!(!shield.hidden_selectors("x.example.net").contains(&"div.b".to_owned()));
        assert_eq!(on_news.iter().filter(|s| *s == "div.a").count(), 1);
    }

    #[test]
    fn a_list_without_hide_rules_has_no_script() {
        assert!(Shield::compile(&["block host:a.example"]).cosmetic_script(&[], true).is_none());
    }

    #[test]
    fn js_strings_cannot_end_early_or_close_a_script() {
        fn unescape(literal: &str) -> String {
            let body = literal.strip_prefix('"').and_then(|s| s.strip_suffix('"')).expect("quoted");
            let mut out = String::new();
            let mut chars = body.chars();
            while let Some(c) = chars.next() {
                assert_ne!(c, '"', "unescaped quote inside {literal}");
                assert_ne!(c, '\n', "raw newline inside {literal}");
                if c != '\\' {
                    out.push(c);
                    continue;
                }
                match chars.next().expect("escape") {
                    '\\' => out.push('\\'),
                    '"' => out.push('"'),
                    'u' => {
                        let hex: String = chars.by_ref().take(4).collect();
                        out.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
                    }
                    other => panic!("unexpected escape \\{other} in {literal}"),
                }
            }
            out
        }
        for text in [
            "div[src*=\"/ad-slot/\"]",
            "\"];alert(1);//",
            "div.a\\:b",
            "a</script><script>alert(1)</script>",
            "line\u{2028}sep\u{2029}x",
            "ctl\u{1}\u{7f}x",
            "ünïcode.\u{1F600}",
            "",
        ] {
            let literal = js_string(text);
            assert!(!literal.contains("</"), "{literal}");
            assert!(!literal.contains('\u{2028}') && !literal.contains('\u{2029}'));
            assert_eq!(unescape(&literal), text, "{literal}");
        }
    }

    #[test]
    fn a_hostile_selector_stays_inside_its_string_in_the_script() {
        let shield = Shield::compile(&["hide css:div.a\"+alert(1)+\""]);
        assert!(shield.errors().is_empty(), "{:?}", shield.errors());
        let script = shield.cosmetic_script(&[], true).expect("script");
        assert!(!script.contains("div.a\"+alert(1)"), "{script}");
        assert!(script.contains("div.a\\\"+alert(1)+\\\""), "{script}");
    }

    #[test]
    fn the_script_uses_only_the_page_apis_it_needs() {
        let shield = Shield::with_starter(None);
        let script = shield.cosmetic_script(&["bank.example".to_owned()], true).expect("script");
        for banned in ["chrome.", "MutationObserver", "fetch(", "XMLHttpRequest", "import(", "eval(", "new Function"] {
            assert!(!script.contains(banned), "{banned}");
        }
        assert!(script.contains("display:none!important"));
        assert!(script.contains("location.hostname"));
        assert!(script.contains("\"bank.example\""), "the skip list is embedded");
        assert!(script.len() < 8 * 1024, "tiny: {} bytes", script.len());
    }

    /// Runs the generated script under Node with a stub page, when Node is installed.
    #[test]
    fn the_script_hides_the_right_selectors_per_site_when_run() {
        let node = std::process::Command::new("node").arg("--version").output();
        if node.map(|o| !o.status.success()).unwrap_or(true) {
            eprintln!("node not found: skipping the script execution test");
            return;
        }
        let shield = Shield::compile(&[
            "hide css:div.a\nhide css:div.b only x.example\nhide css:div.c except z.example\nhide css:div[a=\"q\"] except off.example",
        ]);
        let script = shield
            .cosmetic_script(&["skipped.example".to_owned()], true)
            .expect("script");
        let dir = std::env::temp_dir().join(format!("bowser-shield-js-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("cosmetic.js");
        std::fs::write(&script_path, &script).unwrap();
        let harness = dir.join("harness.js");
        std::fs::write(
            &harness,
            r#"
const fs = require("fs");
const src = fs.readFileSync(process.argv[2], "utf8");
function run(page) {
  const inserted = [];
  class CSSStyleSheet {
    constructor() { this.cssRules = []; }
    insertRule(rule, index) {
      if (typeof rule !== "string" || index !== this.cssRules.length) throw new Error("bad insert");
      this.cssRules.push(rule); inserted.push(rule);
    }
  }
  const win = { CSSStyleSheet };
  win.top = page.frame ? {} : win;
  const doc = {
    adoptedStyleSheets: [],
    documentElement: page.early ? null : {
      appendChild(style) { style.sheet = new CSSStyleSheet(); this.style = style; }
    },
    createElement() { return {}; },
  };
  new Function("window", "document", "location", "CSSStyleSheet", src)(
    win, doc, { hostname: page.host, protocol: page.protocol || "https:" }, CSSStyleSheet);
  return inserted;
}
const cases = JSON.parse(process.argv[3]);
console.log(JSON.stringify(cases.map(run)));
"#,
        )
        .unwrap();
        let cases = r#"[{"host":"news.example"},{"host":"WWW.X.Example","early":true},{"host":"z.example"},{"host":"off.example"},{"host":"skipped.example"},{"host":"sub.skipped.example","early":true},{"host":"news.example","protocol":"file:"},{"host":"news.example","frame":true}]"#;
        let out = std::process::Command::new("node")
            .arg(&harness)
            .arg(&script_path)
            .arg(cases)
            .output()
            .expect("run node");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let stdout = String::from_utf8_lossy(&out.stdout);
        let rule = |sel: &str| format!("\"{}{{display:none!important}}\"", sel.replace('"', "\\\""));
        let list = |sels: &[&str]| format!("[{}]", sels.iter().map(|s| rule(s)).collect::<Vec<_>>().join(","));
        let expected = format!(
            "[{},{},{},{},[],[],[],[]]",
            list(&["div.a", "div.c", "div[a=\"q\"]"]),
            list(&["div.a", "div.b", "div.c", "div[a=\"q\"]"]),
            list(&["div.a", "div[a=\"q\"]"]),
            list(&["div.a", "div.c"]),
        );
        assert_eq!(stdout.trim(), expected);
    }
}
