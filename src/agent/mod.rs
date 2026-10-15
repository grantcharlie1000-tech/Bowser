//! Opt-in localhost interface for agents. Pure command, snapshot, and session logic lives here so
//! tests can run it with no window. [`host`] talks to WebView2 only when `--agent` is on.
//!
//! Arbitrary page JavaScript runs as the local user. The listener binds to loopback and refuses any
//! other peer. There is no token. `ExecuteScriptWithResult` on the pinned `webview2-com` 0.39.1
//! bindings reports a thrown script as an error. If that call is unavailable, `ExecuteScript`
//! reports a throw as JSON `null`, the same result as a script that returns `null`.

mod host;

use std::{
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    time::Duration,
};

use url::Url;

use crate::blocker::json::{array_elements, field, string_value, whole_object};

pub use host::serve;

/// Default loopback port. Agents can rely on this when `--agent-port` is omitted.
pub const DEFAULT_PORT: u16 = 47321;

const MAX_SESSION_LEN: usize = 64;
const MAX_NODES: usize = 40;
const MAX_NAME: usize = 80;
const MAX_EXPRESSION: usize = 64 * 1024;
const MAX_PROSE: usize = 4000;

/// One host keeps this many named WebView2 profiles. A later name is refused until `stop`.
pub const MAX_SESSIONS: usize = 8;

/// WebView2 environment variables that can retarget the profile or the browser. The agent host
/// refuses to start while any of these is set, matching the human `App::run` check.
pub const WEBVIEW_OVERRIDES: &[&str] = &[
    "WEBVIEW2_USER_DATA_FOLDER",
    "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
    "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
    "WEBVIEW2_RELEASE_CHANNEL_PREFERENCE",
];

/// The first known override among `present`, which are variable names that are actually set.
pub fn blocked_webview_override<'a>(
    present: impl IntoIterator<Item = &'a str>,
) -> Option<&'static str> {
    for name in present {
        if let Some(known) = WEBVIEW_OVERRIDES.iter().copied().find(|item| *item == name) {
            return Some(known);
        }
    }
    None
}

pub fn webview_override_from_process() -> Option<&'static str> {
    blocked_webview_override(
        WEBVIEW_OVERRIDES
            .iter()
            .copied()
            .filter(|name| std::env::var_os(name).is_some()),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Snapshot,
    Text { markdown: bool },
    Navigate(String),
    Click { refer: Option<String>, selector: Option<String> },
    Fill { refer: String, text: String },
    Press { key: String },
    Script(String),
    Back,
    Stop,
}

impl Op {
    fn name(&self) -> &'static str {
        match self {
            Self::Snapshot => "snapshot",
            Self::Text { .. } => "text",
            Self::Navigate(_) => "navigate",
            Self::Click { .. } => "click",
            Self::Fill { .. } => "fill",
            Self::Press { .. } => "press",
            Self::Script(_) => "script",
            Self::Back => "back",
            Self::Stop => "stop",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub session: String,
    pub op: Op,
}

/// Pieces of one `--agent-call`. Absent fields are omitted flags.
#[derive(Clone, Debug, Default)]
pub struct CallParts {
    pub op: String,
    pub session: Option<String>,
    pub url: Option<String>,
    pub refer: Option<String>,
    pub selector: Option<String>,
    pub text: Option<String>,
    pub key: Option<String>,
    pub expression: Option<String>,
    pub format: Option<String>,
}

/// What [`crate::performance::LaunchOptions`] knows about the agent flags, after the data
/// directory has been resolved. Validation of the combination lives here, not in the window.
pub struct AgentLaunch {
    pub serve: bool,
    pub call: Option<String>,
    pub bind_host: Option<String>,
    pub port: Option<u16>,
    pub session: Option<String>,
    pub url: Option<String>,
    pub refer: Option<String>,
    pub selector: Option<String>,
    pub text: Option<String>,
    pub key: Option<String>,
    pub expression: Option<String>,
    pub format: Option<String>,
    pub data_dir: bool,
    pub dev: bool,
    pub guest: bool,
    pub new_window: bool,
    pub profile: bool,
    pub open_url: bool,
    pub benchmark: bool,
}

/// True only when this process should open the loopback listener. A normal launch is false.
pub fn should_listen(agent: bool) -> bool {
    agent
}

pub fn parse_port(value: &str) -> Result<u16, String> {
    let port: u16 = value
        .parse()
        .map_err(|_| "Agent port must be a number from 1 to 65535".to_owned())?;
    if port == 0 {
        return Err("Agent port must be a number from 1 to 65535".into());
    }
    Ok(port)
}

/// Accepts a literal loopback address. Hostnames, `0.0.0.0`, and LAN addresses are refused.
pub fn parse_loopback_bind(host: &str, port: u16) -> Result<SocketAddr, String> {
    if port == 0 {
        return Err("Agent port must be a number from 1 to 65535".into());
    }
    let ip: IpAddr = host.parse().map_err(|_| {
        format!("Agent bind address must be a loopback IP literal, not {host}")
    })?;
    if !ip.is_loopback() {
        return Err(format!(
            "Agent interface only binds to loopback, not {host}"
        ));
    }
    Ok(SocketAddr::new(ip, port))
}

pub fn peer_is_loopback(addr: SocketAddr) -> bool {
    addr.ip().is_loopback()
}

pub fn validate_agent_launch(launch: &AgentLaunch) -> Result<(), String> {
    let command_bits = launch.session.is_some()
        || launch.url.is_some()
        || launch.refer.is_some()
        || launch.selector.is_some()
        || launch.text.is_some()
        || launch.key.is_some()
        || launch.expression.is_some()
        || launch.format.is_some();
    if !launch.serve && launch.call.is_none() {
        if launch.bind_host.is_some() || launch.port.is_some() || command_bits {
            return Err("Agent arguments require --agent or --agent-call".into());
        }
        return Ok(());
    }
    if launch.serve && launch.call.is_some() {
        return Err("--agent and --agent-call cannot be combined".into());
    }
    if launch.serve {
        if !launch.data_dir {
            return Err("--agent requires --data-dir".into());
        }
        if launch.dev
            || launch.guest
            || launch.new_window
            || launch.profile
            || launch.open_url
            || launch.benchmark
        {
            return Err(
                "--agent cannot be combined with profile, guest, dev, benchmark, or window options"
                    .into(),
            );
        }
        if command_bits {
            return Err("Command arguments require --agent-call".into());
        }
        let host = launch.bind_host.as_deref().unwrap_or("127.0.0.1");
        let port = launch.port.unwrap_or(DEFAULT_PORT);
        parse_loopback_bind(host, port)?;
        return Ok(());
    }
    if launch.bind_host.is_some() {
        return Err("--agent-bind is only valid with --agent".into());
    }
    let port = launch.port.unwrap_or(DEFAULT_PORT);
    parse_loopback_bind("127.0.0.1", port)?;
    command_from_parts(&CallParts {
        op: launch.call.clone().unwrap_or_default(),
        session: launch.session.clone(),
        url: launch.url.clone(),
        refer: launch.refer.clone(),
        selector: launch.selector.clone(),
        text: launch.text.clone(),
        key: launch.key.clone(),
        expression: launch.expression.clone(),
        format: launch.format.clone(),
    })?;
    Ok(())
}

pub fn command_from_parts(parts: &CallParts) -> Result<Command, String> {
    let session = parts.session.clone().unwrap_or_else(|| "default".to_owned());
    check_session_name(&session)?;
    let op = match parts.op.as_str() {
        "snapshot" => Op::Snapshot,
        "text" => {
            let markdown = match parts.format.as_deref() {
                None | Some("text") => false,
                Some("markdown") => true,
                Some(other) => {
                    return Err(format!("--format must be text or markdown, not {other}"));
                }
            };
            Op::Text { markdown }
        }
        "navigate" => {
            let url = parts
                .url
                .as_deref()
                .ok_or_else(|| "--url is required for navigate".to_owned())?;
            Op::Navigate(navigation_url(url)?)
        }
        "click" => {
            if parts.refer.is_none() && parts.selector.is_none() {
                return Err("click requires --ref or --selector".into());
            }
            if let Some(refer) = &parts.refer {
                check_ref(refer)?;
            }
            if let Some(selector) = &parts.selector {
                check_selector(selector)?;
            }
            Op::Click {
                refer: parts.refer.clone(),
                selector: parts.selector.clone(),
            }
        }
        "fill" => {
            let refer = parts
                .refer
                .clone()
                .ok_or_else(|| "fill requires --ref".to_owned())?;
            check_ref(&refer)?;
            let text = parts
                .text
                .clone()
                .ok_or_else(|| "fill requires --text".to_owned())?;
            Op::Fill { refer, text }
        }
        "press" => {
            let key = parts
                .key
                .clone()
                .ok_or_else(|| "press requires --key".to_owned())?;
            if key.is_empty() || key.chars().count() > 32 || key.chars().any(|ch| ch.is_control())
            {
                return Err("press key must be a short name such as Enter".into());
            }
            Op::Press { key }
        }
        "script" => {
            let expression = parts
                .expression
                .clone()
                .ok_or_else(|| "script requires --expression".to_owned())?;
            if expression.is_empty() || expression.len() > MAX_EXPRESSION {
                return Err("script expression is empty or too long".into());
            }
            Op::Script(expression)
        }
        "back" => Op::Back,
        "stop" => Op::Stop,
        other => return Err(format!("Unknown agent command: {other}")),
    };
    Ok(Command { session, op })
}

pub fn navigation_url(text: &str) -> Result<String, String> {
    let url = Url::parse(text).map_err(|error| format!("Invalid URL: {error}"))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("The address must be an HTTP(S) URL without credentials".into());
    }
    Ok(url.to_string())
}

pub fn check_session_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > MAX_SESSION_LEN
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(
            "Session name must be 1-64 characters of letters, digits, hyphen, or underscore"
                .into(),
        );
    }
    Ok(())
}

/// WebView2 user-data folder for one named agent session. Two names never share a folder.
pub fn session_dir(root: &Path, name: &str) -> Result<PathBuf, String> {
    check_session_name(name)?;
    Ok(root.join("agent-sessions").join(name))
}

fn check_ref(refer: &str) -> Result<(), String> {
    let Some(rest) = refer.strip_prefix('e') else {
        return Err(format!("ref must look like e1, not {refer}"));
    };
    if rest.is_empty()
        || rest.len() > 4
        || !rest.bytes().all(|byte| byte.is_ascii_digit())
        || rest.starts_with('0')
    {
        return Err(format!("ref must look like e1, not {refer}"));
    }
    Ok(())
}

fn check_selector(selector: &str) -> Result<(), String> {
    if selector.is_empty()
        || selector.len() > 200
        || selector.chars().any(|ch| ch.is_control())
    {
        return Err("selector is empty, too long, or contains a control character".into());
    }
    Ok(())
}

pub fn command_line(command: &Command) -> String {
    let mut body = format!(
        "{{\"op\":{},\"session\":{}",
        json_string(command.op.name()),
        json_string(&command.session)
    );
    match &command.op {
        Op::Text { markdown } => {
            body.push_str(",\"format\":");
            body.push_str(&json_string(if *markdown { "markdown" } else { "text" }));
        }
        Op::Navigate(url) => {
            body.push_str(",\"url\":");
            body.push_str(&json_string(url));
        }
        Op::Click { refer, selector } => {
            if let Some(refer) = refer {
                body.push_str(",\"ref\":");
                body.push_str(&json_string(refer));
            }
            if let Some(selector) = selector {
                body.push_str(",\"selector\":");
                body.push_str(&json_string(selector));
            }
        }
        Op::Fill { refer, text } => {
            body.push_str(",\"ref\":");
            body.push_str(&json_string(refer));
            body.push_str(",\"text\":");
            body.push_str(&json_string(text));
        }
        Op::Press { key } => {
            body.push_str(",\"key\":");
            body.push_str(&json_string(key));
        }
        Op::Script(expression) => {
            body.push_str(",\"expression\":");
            body.push_str(&json_string(expression));
        }
        Op::Snapshot | Op::Back | Op::Stop => {}
    }
    body.push('}');
    body
}

pub fn parse_command(text: &str) -> Result<Command, String> {
    let members = whole_object(text).ok_or_else(|| "Agent command must be one JSON object".to_owned())?;
    let op = string_field(&members, "op")?;
    let session = match field(&members, "session") {
        Some(raw) => string_value(raw).ok_or_else(|| "session must be a string".to_owned())?,
        None => "default".to_owned(),
    };
    let mut parts = CallParts {
        op,
        session: Some(session),
        ..CallParts::default()
    };
    if let Some(raw) = field(&members, "url") {
        parts.url = Some(string_value(raw).ok_or_else(|| "url must be a string".to_owned())?);
    }
    if let Some(raw) = field(&members, "ref") {
        parts.refer = Some(string_value(raw).ok_or_else(|| "ref must be a string".to_owned())?);
    }
    if let Some(raw) = field(&members, "selector") {
        parts.selector =
            Some(string_value(raw).ok_or_else(|| "selector must be a string".to_owned())?);
    }
    if let Some(raw) = field(&members, "text") {
        parts.text = Some(string_value(raw).ok_or_else(|| "text must be a string".to_owned())?);
    }
    if let Some(raw) = field(&members, "key") {
        parts.key = Some(string_value(raw).ok_or_else(|| "key must be a string".to_owned())?);
    }
    if let Some(raw) = field(&members, "expression") {
        parts.expression =
            Some(string_value(raw).ok_or_else(|| "expression must be a string".to_owned())?);
    }
    if let Some(raw) = field(&members, "format") {
        parts.format =
            Some(string_value(raw).ok_or_else(|| "format must be a string".to_owned())?);
    }
    command_from_parts(&parts)
}

/// One JSON encoding of what `ExecuteScript` already JSON-encoded. A string or number is not
/// wrapped a second time. Objects and arrays are returned unchanged after they parse.
pub fn decode_script_result(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("empty script result".into());
    }
    if raw == "null" || raw == "true" || raw == "false" {
        return Ok(raw.to_owned());
    }
    if raw.starts_with('"') {
        let text = string_value(raw).ok_or_else(|| "script result string is not valid JSON".to_owned())?;
        return Ok(json_string(&text));
    }
    if raw.starts_with('{') {
        whole_object(raw).ok_or_else(|| "script result object is not valid JSON".to_owned())?;
        return Ok(raw.to_owned());
    }
    if raw.starts_with('[') {
        array_elements(raw).ok_or_else(|| "script result array is not valid JSON".to_owned())?;
        return Ok(raw.to_owned());
    }
    if is_json_number(raw) {
        return Ok(raw.to_owned());
    }
    Err("script result is not a JSON value".into())
}

fn is_json_number(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let mut index = 0;
    if bytes.first() == Some(&b'-') {
        index = 1;
    }
    if index >= bytes.len() {
        return false;
    }
    if bytes[index] == b'0' {
        index += 1;
    } else if bytes[index].is_ascii_digit() {
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
    } else {
        return false;
    }
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    if index < bytes.len() && matches!(bytes[index], b'e' | b'E') {
        index += 1;
        if index < bytes.len() && matches!(bytes[index], b'+' | b'-') {
            index += 1;
        }
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    index == bytes.len()
}

pub fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

pub fn err_json(message: &str) -> String {
    format!("{{\"ok\":false,\"error\":{}}}", json_string(message))
}

pub fn ok_result(value_json: &str) -> String {
    format!("{{\"ok\":true,\"result\":{value_json}}}")
}

pub fn ok_url(url: &str) -> String {
    format!("{{\"ok\":true,\"url\":{}}}", json_string(url))
}

pub fn ok_stop() -> String {
    "{\"ok\":true,\"stopped\":true}".to_owned()
}

pub fn listening_line(addr: SocketAddr) -> String {
    format!(
        "{{\"event\":\"listening\",\"addr\":{}}}",
        json_string(&addr.to_string())
    )
}

pub fn response_ok(body: &str) -> bool {
    whole_object(body)
        .and_then(|members| field(&members, "ok"))
        .is_some_and(|value| value == "true")
}

fn string_field(
    members: &[crate::blocker::json::Member<'_>],
    key: &str,
) -> Result<String, String> {
    let raw = field(members, key).ok_or_else(|| format!("missing {key}"))?;
    string_value(raw).ok_or_else(|| format!("{key} must be a string"))
}

/// Compact tree. Interactive rows carry a stable `eN` ref. Raw markup fields are not copied.
pub fn shape_page_json(decoded_object: &str) -> Result<String, String> {
    let members = whole_object(decoded_object)
        .ok_or_else(|| "snapshot was not a JSON object".to_owned())?;
    let url = string_field(&members, "url")?;
    let title = clean_label(&string_field(&members, "title")?);
    let nodes_raw = field(&members, "nodes").ok_or_else(|| "snapshot is missing nodes".to_owned())?;
    let mut lines = Vec::new();
    for element in array_elements(nodes_raw.trim())
        .ok_or_else(|| "snapshot nodes were not an array".to_owned())?
    {
        if lines.len() >= MAX_NODES {
            break;
        }
        let node = whole_object(element).ok_or_else(|| "snapshot node was not an object".to_owned())?;
        let role = clean_label(&string_field(&node, "role")?);
        if role.is_empty() {
            continue;
        }
        let name = clean_label(&string_field(&node, "name").unwrap_or_default());
        let refer = string_field(&node, "ref").unwrap_or_default();
        let interactive = field(&node, "interactive") == Some("true");
        if interactive {
            if check_ref(&refer).is_err() {
                continue;
            }
            lines.push(format!("{role} \"{name}\" {refer}"));
        } else if !name.is_empty() {
            lines.push(format!("{role} \"{name}\""));
        }
    }
    let mut tree = format!("{url}\n{title}");
    if !lines.is_empty() {
        tree.push('\n');
        tree.push_str(&lines.join("\n"));
    }
    Ok(format!(
        "{{\"ok\":true,\"url\":{},\"title\":{},\"tree\":{}}}",
        json_string(&url),
        json_string(&title),
        json_string(&tree)
    ))
}

pub fn text_response(decoded_object: &str, markdown: bool) -> Result<String, String> {
    let members = whole_object(decoded_object)
        .ok_or_else(|| "text result was not a JSON object".to_owned())?;
    let url = string_field(&members, "url")?;
    let title = clean_label(&string_field(&members, "title")?);
    let text = clean_prose(&string_field(&members, "text")?);
    let body = if markdown {
        format!("# {title}\n\n{text}")
    } else {
        text
    };
    let format = if markdown { "markdown" } else { "text" };
    Ok(format!(
        "{{\"ok\":true,\"url\":{},\"title\":{},\"format\":{},\"text\":{}}}",
        json_string(&url),
        json_string(&title),
        json_string(format),
        json_string(&body)
    ))
}

/// `strict` is click, fill, and press: only `{ok:true}` succeeds. A throw that arrived as `null`
/// is a failure. A raw `script` result (`strict` false) is embedded once, except `{ok:false}`.
pub fn action_response(decoded: &str, strict: bool) -> String {
    if let Some(members) = whole_object(decoded) {
        if field(&members, "ok") == Some("false") {
            let message = string_field(&members, "error").unwrap_or_else(|_| "failed".to_owned());
            return err_json(&message);
        }
        if strict && field(&members, "ok") == Some("true") && field(&members, "error").is_none() {
            return "{\"ok\":true}".to_owned();
        }
    }
    if strict {
        return err_json("action did not report success");
    }
    ok_result(decoded)
}

pub const MAX_RESPONSE_BYTES: usize = 1_000_000;

/// Responses larger than one megabyte become an error so a page cannot pin the client.
pub fn bounded_response(body: &str) -> String {
    if body.len() <= MAX_RESPONSE_BYTES {
        body.to_owned()
    } else {
        err_json("result is too large")
    }
}

fn clean_label(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch == '<' || ch == '>' || ch.is_control() {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(MAX_NAME).collect()
}

fn clean_prose(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch == '<' || ch == '>' || ch.is_control() {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(MAX_PROSE).collect()
}

pub fn snapshot_script() -> &'static str {
    r#"(function(){var sel="a,button,input,textarea,select,summary,[role='button'],[role='link'],[role='textbox'],[role='checkbox'],[role='radio'],[role='combobox'],[role='tab'],[contenteditable='true']";function hidden(el){if(!el||el.nodeType!==1)return true;var s=getComputedStyle(el);if(s.display==="none"||s.visibility==="hidden")return true;if(el.tagName==="INPUT"&&String(el.type||"").toLowerCase()==="hidden")return true;return false}function roleOf(el){var r=el.getAttribute("role");if(r)return r.toLowerCase();var t=el.tagName.toLowerCase();if(t==="a")return "link";if(t==="button"||t==="summary")return "button";if(t==="textarea")return "textbox";if(t==="select")return "combobox";if(t==="input"){var ty=(el.type||"text").toLowerCase();if(ty==="checkbox")return "checkbox";if(ty==="radio")return "radio";if(ty==="button"||ty==="submit"||ty==="reset")return "button";if(ty==="hidden")return "";return "textbox"}if(/^h[1-6]$/.test(t))return "heading";return t}function nameOf(el){var aria=el.getAttribute("aria-label");if(aria)return aria;if(el.labels&&el.labels[0])return el.labels[0].innerText||"";var by=el.getAttribute("aria-labelledby");if(by){var parts=by.split(/\s+/).map(function(id){var n=document.getElementById(id);return n?(n.innerText||""):""}).join(" ").trim();if(parts)return parts}var placeholder=el.getAttribute("placeholder");if(placeholder)return placeholder;if(el.alt)return el.alt;return el.innerText||el.value||el.getAttribute("title")||""}function clean(s){return String(s||"").replace(/\s+/g," ").trim().slice(0,80)}window.__bowserAgent={};var nodes=[];var n=0;var seen=[];function has(el){for(var i=0;i<seen.length;i++)if(seen[i]===el)return true;return false}function push(el,interactive){if(has(el)||hidden(el))return;var role=roleOf(el);if(!role)return;seen.push(el);var ref="";if(interactive){n+=1;if(n>40)return;ref="e"+n;window.__bowserAgent[ref]=el}var name=clean(nameOf(el));if(!interactive&&!name)return;if(nodes.length>=40)return;nodes.push({role:role,name:name,ref:ref,interactive:!!interactive})}var list=document.querySelectorAll(sel);for(var i=0;i<list.length;i++)push(list[i],true);var extra=document.querySelectorAll("h1,h2,h3,p,li,label");for(var j=0;j<extra.length;j++)push(extra[j],false);return {url:location.href,title:document.title,nodes:nodes}})()"#
}

pub fn text_script() -> &'static str {
    r#"(function(){var root=document.querySelector("main")||document.body;var text=root?(root.innerText||""):"";text=String(text).replace(/[ \t]+\n/g,"\n").trim().slice(0,4000);return {url:location.href,title:document.title,text:text}})()"#
}

pub fn navigate_script(url: &str) -> String {
    format!(r#"(function(){{location.assign({url});return {{ok:true}}}})()"#, url = json_string(url))
}

pub fn back_script() -> &'static str {
    "(function(){history.back();return {ok:true}})()"
}

pub fn click_script(refer: Option<&str>, selector: Option<&str>) -> String {
    let refer = refer.map(json_string).unwrap_or_else(|| "null".to_owned());
    let selector = selector.map(json_string).unwrap_or_else(|| "null".to_owned());
    format!(
        r#"(function(){{var ref={refer};var sel={selector};var el=(ref&&window.__bowserAgent&&window.__bowserAgent[ref])||(sel&&document.querySelector(sel));if(!el)return {{ok:false,error:"not found"}};el.click();return {{ok:true}}}})()"#
    )
}

pub fn fill_script(refer: &str, text: &str) -> String {
    format!(
        r#"(function(){{var el=window.__bowserAgent&&window.__bowserAgent[{refer}];if(!el)return {{ok:false,error:"not found"}};el.focus();var text={text};if("value" in el){{var proto=Object.getPrototypeOf(el);var desc=proto&&Object.getOwnPropertyDescriptor(proto,"value");if(desc&&desc.set){{desc.set.call(el,text)}}else{{el.value=text}}el.dispatchEvent(new Event("input",{{bubbles:true}}));el.dispatchEvent(new Event("change",{{bubbles:true}}))}}else{{el.textContent=text}}return {{ok:true}}}})()"#,
        refer = json_string(refer),
        text = json_string(text)
    )
}

pub fn press_script(key: &str) -> String {
    format!(
        r#"(function(){{var key={key};var el=document.activeElement||document.body;var init={{key:key,bubbles:true,cancelable:true}};el.dispatchEvent(new KeyboardEvent("keydown",init));el.dispatchEvent(new KeyboardEvent("keyup",init));return {{ok:true}}}})()"#,
        key = json_string(key)
    )
}

pub fn eval_script(expression: &str) -> String {
    format!("(function(){{return (\n{expression}\n);}})()")
}

pub fn client_exchange(addr: SocketAddr, command: &Command) -> Result<String, String> {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .map_err(|error| format!("Cannot connect to the agent interface at {addr}: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(90)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    let line = command_line(command);
    stream
        .write_all(line.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|error| format!("Cannot send the agent command: {error}"))?;
    read_line(&mut stream, 1_000_000)
}

pub(crate) fn read_line(stream: &mut impl Read, limit: usize) -> Result<String, String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let read = stream
            .read(&mut byte)
            .map_err(|error| format!("Agent connection closed: {error}"))?;
        if read == 0 || byte[0] == b'\n' {
            break;
        }
        if byte[0] != b'\r' {
            buf.push(byte[0]);
        }
        if buf.len() > limit {
            return Err("Agent message is too large".into());
        }
    }
    String::from_utf8(buf).map_err(|_| "Agent message is not UTF-8".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::performance::LaunchOptions;
    use std::path::Path;

    fn parse(args: &[&str]) -> Result<LaunchOptions, String> {
        LaunchOptions::parse_from(
            args.iter().map(|arg| (*arg).to_owned()),
            Path::new("benchmarks/fixtures"),
        )
    }

    #[test]
    fn argv_without_agent_leaves_the_interface_off_and_rejects_unknown_flags() {
        let options = parse(&["--data-dir", r"C:\bowser-agent-parse-test"]).unwrap();
        assert!(!options.agent);
        assert!(options.agent_call.is_none());
        assert!(!should_listen(options.agent));
        let error = parse(&["--not-a-real-flag"]).unwrap_err();
        assert!(error.contains("Unknown argument"), "{error}");
    }

    #[test]
    fn a_non_loopback_bind_address_is_rejected() {
        let error = parse_loopback_bind("0.0.0.0", DEFAULT_PORT).unwrap_err();
        assert!(error.contains("loopback"), "{error}");
        assert!(parse_loopback_bind("192.168.1.5", DEFAULT_PORT).is_err());
        assert!(parse_loopback_bind("localhost", DEFAULT_PORT).is_err());
        assert_eq!(
            parse_loopback_bind("127.0.0.1", DEFAULT_PORT).unwrap(),
            "127.0.0.1:47321".parse().unwrap()
        );
        assert!(parse_loopback_bind("::1", DEFAULT_PORT).unwrap().ip().is_loopback());
        let refused = parse(&[
            "--agent",
            "--data-dir",
            r"C:\bowser-agent-parse-test",
            "--agent-bind",
            "0.0.0.0",
        ])
        .unwrap_err();
        assert!(refused.contains("loopback"), "{refused}");
        assert!(!peer_is_loopback("10.0.0.8:47321".parse().unwrap()));
        assert!(peer_is_loopback("127.0.0.1:47321".parse().unwrap()));
    }

    #[test]
    fn script_results_are_decoded_once() {
        let string_value = decode_script_result(r#""hello""#).unwrap();
        assert_eq!(string_value, r#""hello""#);
        assert_eq!(
            ok_result(&string_value),
            r#"{"ok":true,"result":"hello"}"#
        );
        assert_eq!(ok_result(&decode_script_result("42").unwrap()), r#"{"ok":true,"result":42}"#);
        assert_eq!(decode_script_result("null").unwrap(), "null");
        assert_eq!(
            decode_script_result(r#"{"a":1}"#).unwrap(),
            r#"{"a":1}"#
        );
    }

    #[test]
    fn snapshot_shape_has_url_title_role_name_and_ref_without_markup() {
        let page = r#"{"url":"http://127.0.0.1/fixture","title":"Agent fixture","html":"<main id=\"agent-fixture-markup\">","nodes":[{"role":"button","name":"Save","ref":"e1","interactive":true},{"role":"textbox","name":"Name","ref":"e2","interactive":true},{"role":"paragraph","name":"waiting","ref":"","interactive":false}]}"#;
        let shaped = shape_page_json(page).unwrap();
        assert!(shaped.contains("Agent fixture"), "{shaped}");
        assert!(shaped.contains("http://127.0.0.1/fixture"), "{shaped}");
        assert!(shaped.contains("button"), "{shaped}");
        assert!(shaped.contains("Save"), "{shaped}");
        assert!(shaped.contains("e1"), "{shaped}");
        assert!(shaped.contains("textbox"), "{shaped}");
        assert!(shaped.contains("Name"), "{shaped}");
        assert!(!shaped.contains("agent-fixture-markup"), "{shaped}");
        assert!(!shaped.contains("<main"), "{shaped}");
        assert!(!shaped.contains("<button"), "{shaped}");
    }

    #[test]
    fn two_session_names_map_to_distinct_storage_identities() {
        let root = Path::new(r"C:\bowser-agent-parse-test");
        let alpha = session_dir(root, "alpha").unwrap();
        let beta = session_dir(root, "beta").unwrap();
        assert_ne!(alpha, beta);
        assert_eq!(session_dir(root, "alpha").unwrap(), alpha);
        assert!(session_dir(root, r"..\alpha").is_err());
        assert!(session_dir(root, "a/b").is_err());
        assert!(session_dir(root, "").is_err());
    }

    #[test]
    fn commands_round_trip_and_actions_are_page_scripts() {
        let fill = command_from_parts(&CallParts {
            op: "fill".into(),
            session: Some("alpha".into()),
            refer: Some("e2".into()),
            text: Some("Ada".into()),
            ..CallParts::default()
        })
        .unwrap();
        let parsed = parse_command(&command_line(&fill)).unwrap();
        assert_eq!(parsed, fill);
        let script = fill_script("e2", "Ada");
        assert!(script.contains("el.value"), "{script}");
        assert!(script.contains("e2"), "{script}");
        assert!(!script.to_lowercase().contains("sendinput"));
        let click = click_script(Some("e1"), None);
        assert!(click.contains(".click()"), "{click}");
        let navigate = navigate_script("http://127.0.0.1/fixture");
        assert!(navigate.contains("location.assign"), "{navigate}");
        assert!(back_script().contains("history.back()"));
        assert!(press_script("Enter").contains("KeyboardEvent"));
        let options = parse(&[
            "--agent",
            "--data-dir",
            r"C:\bowser-agent-parse-test",
        ])
        .unwrap();
        assert!(should_listen(options.agent));
        assert!(options.agent_call.is_none());
        assert!(navigation_url("http://127.0.0.1/page.html").is_ok());
        assert!(navigation_url("file:///C:/temp/page.html").is_err());
        assert!(navigation_url("https://user:secret@example.com/").is_err());
    }

    #[test]
    fn fixture_page_is_present_for_the_live_smoke() {
        let html = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agent/page.html"),
        )
        .unwrap();
        assert!(html.contains("<title>Agent fixture</title>"));
        assert!(html.contains("agent-fixture-markup"));
        assert!(html.contains(">Save<"));
        assert!(html.contains("saved:"));
    }

    #[test]
    fn strict_actions_require_ok_true_and_overrides_are_named() {
        assert_eq!(action_response(r#"{"ok":true}"#, true), r#"{"ok":true}"#);
        let missed = action_response("null", true);
        assert!(missed.contains(r#""ok":false"#), "{missed}");
        assert_eq!(
            action_response(r#"{"ok":false,"error":"not found"}"#, true),
            err_json("not found")
        );
        assert_eq!(
            action_response("null", false),
            r#"{"ok":true,"result":null}"#
        );
        assert_eq!(
            blocked_webview_override(["PATH", "WEBVIEW2_USER_DATA_FOLDER"]),
            Some("WEBVIEW2_USER_DATA_FOLDER")
        );
        assert!(blocked_webview_override(["PATH"]).is_none());
        assert!(bounded_response(r#"{"ok":true}"#).contains(r#""ok":true"#));
        let huge = "x".repeat(MAX_RESPONSE_BYTES + 1);
        assert!(bounded_response(&huge).contains("too large"));
    }
}
