//! Workspace rules (T10): sites that open in a named workspace. When a tab of the active
//! workspace commits a page of a site with a rule, the app moves the tab to that workspace and
//! switches to it (`App::apply_workspace_rule`). Rules name the workspace, so renaming the workspace
//! leaves its rules pointing nowhere until they are edited; a rule whose workspace does not exist
//! does nothing.
//!
//! One plain file per profile, `workspace-rules.txt`, beside `workspaces.txt` (kept apart so an older
//! Bowser still reads its workspaces file). A guest or private window never writes it.

use std::{io, path::Path};

pub const FILE_NAME: &str = "workspace-rules.txt";
const HEADER: &str = "bowser-workspace-rules 1";
/// Most rules kept.
pub const MAX_RULES: usize = 64;
const MAX_FILE_BYTES: usize = 64 * 1024;
/// Longest workspace name a rule may hold (the workspace file's own limit).
const MAX_NAME_BYTES: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rule {
    /// A host without scheme, port or `www.`, lower case: `example.com` matches it and its
    /// subdomains.
    pub site: String,
    /// The workspace the site opens in.
    pub workspace: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuleError {
    BadSite,
    BadName,
    Duplicate,
    TooMany,
}

impl RuleError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::BadSite => "Enter a site such as example.com.",
            Self::BadName => "Choose a workspace.",
            Self::Duplicate => "That site already has a rule.",
            Self::TooMany => "Bowser keeps at most 64 workspace rules.",
        }
    }
}

/// A typed site, made plain: trimmed and lower-cased, scheme, path, port and a leading `www.`
/// dropped. `None` when what is left is not a host name.
pub fn normalise_site(input: &str) -> Option<String> {
    let mut text = input.trim().to_ascii_lowercase();
    if let Some(at) = text.find("://") {
        text = text[at + 3..].to_owned();
    }
    let text = text.split(['/', '?', '#']).next().unwrap_or_default();
    let text = text.rsplit_once('@').map_or(text, |(_, host)| host);
    let text = match text.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => text,
    };
    let text = text.strip_prefix("www.").unwrap_or(text).trim_end_matches('.');
    let valid_label = |label: &str| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    (text.len() <= 253 && text.contains('.') && text.split('.').all(valid_label)).then(|| text.to_owned())
}

fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && name.len() <= MAX_NAME_BYTES && !name.chars().any(char::is_control)).then(|| name.to_owned())
}

/// Adds a rule for `site` (as typed) to open in `workspace`.
pub fn add(rules: &mut Vec<Rule>, site: &str, workspace: &str) -> Result<(), RuleError> {
    let site = normalise_site(site).ok_or(RuleError::BadSite)?;
    let workspace = clean_name(workspace).ok_or(RuleError::BadName)?;
    if rules.iter().any(|rule| rule.site == site) {
        return Err(RuleError::Duplicate);
    }
    if rules.len() >= MAX_RULES {
        return Err(RuleError::TooMany);
    }
    rules.push(Rule { site, workspace });
    Ok(())
}

/// Removes the rule for `site`; whether there was one.
pub fn remove(rules: &mut Vec<Rule>, site: &str) -> bool {
    let before = rules.len();
    rules.retain(|rule| rule.site != site);
    rules.len() != before
}

/// The rule for `url`: its host is the rule's site or a subdomain of it, and the longest such site
/// wins (`mail.example.com` over `example.com`). Only web pages match.
pub fn matching<'a>(rules: &'a [Rule], url: &url::Url) -> Option<&'a Rule> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let host = host.trim_end_matches('.');
    rules
        .iter()
        .filter(|rule| host == rule.site || host.strip_suffix(rule.site.as_str()).is_some_and(|rest| rest.ends_with('.')))
        .max_by_key(|rule| rule.site.len())
}

pub fn serialize(rules: &[Rule]) -> String {
    let mut out = format!("{HEADER}\n");
    for rule in rules.iter().take(MAX_RULES) {
        out.push_str(&rule.site);
        out.push('\t');
        out.push_str(&encode(&rule.workspace));
        out.push('\n');
    }
    out
}

/// Reads the file's text. A file with another header gives no rules; a line that does not hold a
/// valid site and name is skipped, so one bad line never loses the rest.
pub fn parse(text: &str) -> Vec<Rule> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(HEADER) {
        return Vec::new();
    }
    let mut rules = Vec::new();
    for line in lines {
        let Some((site, name)) = line.split_once('\t') else { continue };
        let Some(name) = decode(name.trim_end_matches('\r')) else { continue };
        if normalise_site(site).as_deref() != Some(site) {
            continue;
        }
        let _ = add(&mut rules, site, &name);
    }
    rules
}

pub fn load(dir: &Path) -> Vec<Rule> {
    match std::fs::read(dir.join(FILE_NAME)) {
        Ok(bytes) if bytes.len() <= MAX_FILE_BYTES => std::str::from_utf8(&bytes).map(parse).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Writes the rules atomically; no rules removes the file.
pub fn save(dir: &Path, rules: &[Rule]) -> io::Result<()> {
    let path = dir.join(FILE_NAME);
    if rules.is_empty() {
        return match std::fs::remove_file(&path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    }
    std::fs::create_dir_all(dir)?;
    let temporary = path.with_extension(format!("txt.{}.tmp", std::process::id()));
    let result = std::fs::write(&temporary, serialize(rules)).and_then(|()| std::fs::rename(&temporary, &path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b" -_.".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let digits = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            out.push(u8::from_str_radix(digits, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> url::Url {
        url::Url::parse(text).unwrap()
    }

    #[test]
    fn typed_sites_are_made_plain_hosts() {
        assert_eq!(normalise_site("  https://WWW.Example.com:8080/path?q#f ").as_deref(), Some("example.com"));
        assert_eq!(normalise_site("mail.example.co.uk").as_deref(), Some("mail.example.co.uk"));
        assert_eq!(normalise_site("user@example.com").as_deref(), Some("example.com"));
        for bad in ["", "localhost", "exa mple.com", "-x.com", "x..com", "javascript:alert(1)"] {
            assert_eq!(normalise_site(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_rule_matches_its_site_and_subdomains_and_the_longest_site_wins() {
        let mut rules = Vec::new();
        add(&mut rules, "example.com", "Work").unwrap();
        add(&mut rules, "mail.example.com", "Mail").unwrap();
        assert_eq!(matching(&rules, &url("https://example.com/")).map(|r| r.workspace.as_str()), Some("Work"));
        assert_eq!(matching(&rules, &url("https://docs.example.com/x")).map(|r| r.workspace.as_str()), Some("Work"));
        assert_eq!(matching(&rules, &url("https://mail.example.com/")).map(|r| r.workspace.as_str()), Some("Mail"));
        assert_eq!(matching(&rules, &url("https://notexample.com/")), None, "a suffix is not a subdomain");
        assert_eq!(matching(&rules, &url("file:///c:/example.com")), None);
        assert_eq!(add(&mut rules, "https://www.example.com", "Other"), Err(RuleError::Duplicate));
        assert_eq!(add(&mut rules, "x.org", "  "), Err(RuleError::BadName));
        assert!(remove(&mut rules, "example.com"));
        assert!(!remove(&mut rules, "example.com"));
        assert_eq!(matching(&rules, &url("https://docs.example.com/")), None);
    }

    #[test]
    fn rules_round_trip_and_bad_lines_are_skipped() {
        let mut rules = Vec::new();
        add(&mut rules, "example.com", "Work and play 100% / Ünï").unwrap();
        assert_eq!(add(&mut rules, "tabbed.com", "Work\tand play"), Err(RuleError::BadName), "no control characters");
        add(&mut rules, "news.org", "Reading").unwrap();
        let text = serialize(&rules);
        assert_eq!(parse(&text), rules);
        let damaged = format!("{text}not a rule\nBAD.com\tX\nok.net\t%ZZ\n");
        assert_eq!(parse(&damaged), rules, "an unplain site and a bad escape are skipped");
        assert!(parse("some other file\nexample.com\tWork\n").is_empty());
        let mut many = Vec::new();
        for n in 0..MAX_RULES {
            add(&mut many, &format!("s{n}.com"), "W").unwrap();
        }
        assert_eq!(add(&mut many, "one-more.com", "W"), Err(RuleError::TooMany));
    }

    #[test]
    fn rules_save_load_and_an_empty_list_removes_the_file() {
        let dir = std::env::temp_dir().join(format!("bowser-ws-rules-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut rules = Vec::new();
        add(&mut rules, "example.com", "Work").unwrap();
        save(&dir, &rules).unwrap();
        assert_eq!(load(&dir), rules);
        save(&dir, &[]).unwrap();
        assert!(!dir.join(FILE_NAME).exists());
        assert!(load(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
