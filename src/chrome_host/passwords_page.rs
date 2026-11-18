//! The wire of the settings page's "Passwords and autofill" section (`bowser://settings/passwords`).
//!
//! - `{t:"snap",p:"passwords",d:{...}}` carries the saved logins (never their passwords), the sites
//!   never saved for, the saved addresses, the saved cards (brand and last four digits only), the
//!   preferences, an import preview or result, and how the last command went. Two one-shot fields
//!   carry a secret on request: `reveal` (one login's password, after "Show") and `generated`
//!   (a password the generator just made). Lists are arrays of arrays so a large vault stays within
//!   the page's snapshot key budget.
//! - `{t:"set",a:"pw-…"|"af-…",…}` comes back; [`map_command`] accepts each action with exactly the
//!   fields it needs and nothing else.

use super::protocol::{PanelFields, snap_json};
use crate::{
    autofill::{Address, Card, Prefs},
    passwords::CredentialSummary,
};

/// Most logins one snapshot lists; a bigger vault is narrowed by searching on the host.
pub const MAX_LISTED: usize = 4000;
const MAX_ERROR_CHARS: usize = 240;

/// How Bowser answered the page's last command. Sent once, with the snapshot that follows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ack {
    #[default]
    None,
    Saved,
    Deleted,
    Copied,
    Imported,
    Exported,
    Failed,
}

impl Ack {
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Saved => "saved",
            Self::Deleted => "deleted",
            Self::Copied => "copied",
            Self::Imported => "imported",
            Self::Exported => "exported",
            Self::Failed => "failed",
        }
    }
}

/// An import waiting for confirmation, or the result of the last one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportView {
    Preview { provider: &'static str, new: usize, duplicates: usize, conflicts: usize, rejected: usize },
    Done { added: usize, skipped: usize, failed: usize },
}

/// Everything one snapshot shows.
pub struct View<'a> {
    pub available: bool,
    pub guest: bool,
    pub prefs: Prefs,
    pub logins: &'a [CredentialSummary],
    pub total: usize,
    pub query: &'a str,
    pub never: &'a [String],
    pub addresses: &'a [Address],
    pub cards: &'a [Card],
    pub import: Option<ImportView>,
    pub ack: Ack,
    pub error: &'a str,
    pub reveal: Option<(u64, &'a str)>,
    pub generated: Option<&'a str>,
}

pub fn snapshot_json(view: &View<'_>) -> String {
    snap_json("passwords", |out| {
        out.bool("available", view.available);
        out.bool("guest", view.guest);
        out.object("prefs", |o| {
            o.bool("save", view.prefs.offer_passwords);
            o.bool("addresses", view.prefs.addresses);
            o.bool("cards", view.prefs.cards);
        });
        out.uint("total", view.total as u64);
        out.string("q", view.query);
        out.array("l", |a| {
            for login in view.logins.iter().take(MAX_LISTED) {
                a.array(|row| {
                    row.uint(login.id.wire());
                    row.string(&login.host);
                    row.string(&login.origin);
                    row.string(&login.username);
                    row.uint(u64::from(login.strength));
                    row.bool(login.reused);
                });
            }
        });
        out.array("never", |a| {
            for site in view.never.iter().take(MAX_LISTED) {
                a.string(site);
            }
        });
        out.array("addr", |a| {
            for address in view.addresses {
                a.array(|row| {
                    row.uint(address.id);
                    for key in Address::KEYS {
                        row.string(address.get(key).unwrap_or(""));
                    }
                });
            }
        });
        out.array("cards", |a| {
            for card in view.cards {
                a.array(|row| {
                    row.uint(card.id);
                    row.string(&card.name);
                    row.string(card.brand());
                    row.string(card.last4());
                    row.uint(u64::from(card.exp_month));
                    row.uint(u64::from(card.exp_year));
                    row.string(&card.nickname);
                });
            }
        });
        match view.import {
            None => out.null("import"),
            Some(ImportView::Preview { provider, new, duplicates, conflicts, rejected }) => out.object("import", |o| {
                o.string("stage", "preview");
                o.string("provider", provider);
                o.uint("new", new as u64);
                o.uint("dup", duplicates as u64);
                o.uint("conflict", conflicts as u64);
                o.uint("bad", rejected as u64);
            }),
            Some(ImportView::Done { added, skipped, failed }) => out.object("import", |o| {
                o.string("stage", "done");
                o.uint("added", added as u64);
                o.uint("skipped", skipped as u64);
                o.uint("failed", failed as u64);
            }),
        }
        out.string("ack", view.ack.key());
        let clean: String = view.error.chars().take(MAX_ERROR_CHARS).map(|c| if c.is_control() { ' ' } else { c }).collect();
        out.string("error", clean.trim());
        match view.reveal {
            Some((id, password)) => out.object("reveal", |o| {
                o.uint("id", id);
                o.string("value", password);
            }),
            None => out.null("reveal"),
        }
        match view.generated {
            Some(password) => out.string("generated", password),
            None => out.null("generated"),
        }
    })
    .expect("passwords is a panel token")
}

/// Which saved login, address or card a command is about, by its wire id.
pub type WireId = u64;

/// A validated command from the section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Refresh,
    Search(String),
    Reveal(WireId),
    /// Copy a login's username (`false`) or password (`true`) to the clipboard.
    Copy(WireId, bool),
    /// Add (`None`) or replace a login: origin, username, password.
    SaveLogin(Option<WireId>, String, String, String),
    DeleteLogin(WireId),
    Generate(String),
    CopyGenerated,
    Import,
    ImportConfirm,
    ImportCancel,
    Export,
    ForgetNever(String),
    Pref(String, bool),
    /// Add (`None`) or replace an address from `key=value` lines.
    SaveAddress(Option<WireId>, String),
    DeleteAddress(WireId),
    /// Add (`None`) or replace a card from `key=value` lines.
    SaveCard(Option<WireId>, String),
    DeleteCard(WireId),
}

/// Maps `{t:"set",a:"pw-…"|"af-…"}`; anything else (another section's command, or one with fields
/// it does not take) is `None`.
pub fn map_command(f: &PanelFields) -> Option<Action> {
    if !(f.a.starts_with("pw-") || f.a.starts_with("af-")) || f.parent.is_some() || f.before.is_some() || f.url.is_some() {
        return None;
    }
    let text = f.text.as_deref();
    let only = |id: bool, n: bool, on: bool, txt: bool| f.id.is_some() == id && f.n.is_some() == n && f.on.is_some() == on && text.is_some() == txt;
    let optional_id = f.n.is_none() && f.on.is_none() && text.is_some();
    Some(match f.a.as_str() {
        "pw-refresh" if only(false, false, false, false) => Action::Refresh,
        "pw-search" if only(false, false, false, true) => Action::Search(text?.chars().take(200).collect()),
        "pw-reveal" if only(true, false, false, false) => Action::Reveal(f.id?),
        "pw-copy" if only(true, true, false, false) && matches!(f.n, Some(0 | 1)) => Action::Copy(f.id?, f.n == Some(1)),
        "pw-save" if optional_id => {
            let mut parts = text?.splitn(3, '\n');
            let (origin, user, pass) = (parts.next()?, parts.next()?, parts.next()?);
            Action::SaveLogin(f.id, origin.trim().to_owned(), user.trim().to_owned(), pass.to_owned())
        }
        "pw-delete" if only(true, false, false, false) => Action::DeleteLogin(f.id?),
        "pw-generate" if only(false, false, false, true) => Action::Generate(text?.to_owned()),
        "pw-copy-generated" if only(false, false, false, false) => Action::CopyGenerated,
        "pw-import" if only(false, false, false, false) => Action::Import,
        "pw-import-confirm" if only(false, false, false, false) => Action::ImportConfirm,
        "pw-import-cancel" if only(false, false, false, false) => Action::ImportCancel,
        "pw-export" if only(false, false, false, false) => Action::Export,
        "pw-never-remove" if only(false, false, false, true) => Action::ForgetNever(text?.to_owned()),
        "pw-pref" if only(false, false, true, true) && matches!(text, Some("save" | "addresses" | "cards")) => Action::Pref(text?.to_owned(), f.on?),
        "af-address-save" if optional_id => Action::SaveAddress(f.id, text?.to_owned()),
        "af-address-delete" if only(true, false, false, false) => Action::DeleteAddress(f.id?),
        "af-card-save" if optional_id => Action::SaveCard(f.id, text?.to_owned()),
        "af-card-delete" if only(true, false, false, false) => Action::DeleteCard(f.id?),
        _ => return None,
    })
}

/// Reads `key=value` lines into an address. Unknown keys refuse the whole text.
pub fn parse_address(id: u64, text: &str) -> Option<Address> {
    let mut address = Address::default();
    address.id = id;
    for line in text.lines().filter(|line| !line.is_empty()) {
        let (key, value) = line.split_once('=')?;
        if !address.set(key, value) {
            return None;
        }
    }
    (!address.is_empty()).then_some(address)
}

/// What a card form sent: the card, and whether its number was left blank (keep the saved one).
pub fn parse_card(text: &str) -> Option<(String, String, u8, u16, String)> {
    let (mut name, mut number, mut month, mut year, mut nick) = (String::new(), String::new(), 0u8, 0u16, String::new());
    for line in text.lines().filter(|line| !line.is_empty()) {
        let (key, value) = line.split_once('=')?;
        match key {
            "name" => name = value.to_owned(),
            "number" => number = value.chars().filter(char::is_ascii_digit).collect(),
            "month" => month = if value.is_empty() { 0 } else { value.parse().ok()? },
            "year" => year = if value.is_empty() { 0 } else { value.parse().ok()? },
            "nick" => nick = value.to_owned(),
            _ => return None,
        }
    }
    if year > 0 && year < 100 {
        year += 2000;
    }
    Some((name, number, month, year, nick))
}

/// Builds a card, keeping `saved`'s number when the form left it blank.
pub fn card_from(parts: (String, String, u8, u16, String), saved: Option<&Card>) -> Option<Card> {
    let (name, number, month, year, nick) = parts;
    let number = if number.is_empty() { saved?.number().to_owned() } else { number };
    Card::new(&name, &number, month, year, &nick)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(a: &str) -> PanelFields {
        PanelFields { a: a.into(), ..PanelFields::default() }
    }

    #[test]
    fn each_command_takes_exactly_its_fields() {
        assert_eq!(map_command(&fields("pw-refresh")), Some(Action::Refresh));
        assert_eq!(map_command(&PanelFields { id: Some(7), ..fields("pw-reveal") }), Some(Action::Reveal(7)));
        assert_eq!(map_command(&PanelFields { id: Some(7), n: Some(1), ..fields("pw-copy") }), Some(Action::Copy(7, true)));
        assert_eq!(map_command(&PanelFields { id: Some(7), n: Some(2), ..fields("pw-copy") }), None);
        assert_eq!(
            map_command(&PanelFields { text: Some("https://a.example\n me \npass\nword".into()), ..fields("pw-save") }),
            Some(Action::SaveLogin(None, "https://a.example".into(), "me".into(), "pass\nword".into()))
        );
        assert_eq!(map_command(&PanelFields { text: Some("https://a.example\nme".into()), ..fields("pw-save") }), None);
        assert_eq!(map_command(&PanelFields { text: Some("save".into()), on: Some(false), ..fields("pw-pref") }), Some(Action::Pref("save".into(), false)));
        assert_eq!(map_command(&PanelFields { text: Some("other".into()), on: Some(false), ..fields("pw-pref") }), None);
        assert_eq!(map_command(&PanelFields { id: Some(3), text: Some("name=Jane".into()), ..fields("af-address-save") }), Some(Action::SaveAddress(Some(3), "name=Jane".into())));
        for bad in [PanelFields { id: Some(1), ..fields("pw-refresh") }, fields("pw-reveal"), fields("pw-unknown"), fields("ptset"),
            PanelFields { id: Some(1), url: Some("x".into()), ..fields("pw-delete") }] {
            assert_eq!(map_command(&bad), None, "{bad:?}");
        }
    }

    #[test]
    fn address_and_card_forms_parse_strictly() {
        let address = parse_address(0, "name=Jane Doe\npostal=LS1 1AA\n").unwrap();
        assert_eq!((address.name.as_str(), address.postal_code.as_str()), ("Jane Doe", "LS1 1AA"));
        assert!(parse_address(0, "evil=1").is_none() && parse_address(0, "").is_none() && parse_address(0, "name").is_none());
        let parts = parse_card("name=Jane\nnumber=4242 4242 4242 4242\nmonth=4\nyear=28\nnick=").unwrap();
        assert_eq!(parts, ("Jane".into(), "4242424242424242".into(), 4, 2028, String::new()));
        let card = card_from(parts, None).unwrap();
        let kept = card_from(parse_card("name=J\nnumber=\nmonth=5\nyear=2030").unwrap(), Some(&card)).unwrap();
        assert_eq!((kept.last4(), kept.exp_month), ("4242", 5));
        assert!(card_from(parse_card("number=").unwrap(), None).is_none());
        assert!(parse_card("cvc=123").is_none());
    }

    /// The page's test (ui/test/passwords.mjs) parses this very file, so the two sides cannot drift.
    #[test]
    fn the_snapshot_is_the_fixture_the_page_is_tested_with() {
        let mut address = Address::default();
        address.id = 5;
        address.set("name", "Jane Doe");
        address.set("city", "Leeds");
        let mut card = Card::new("Jane Doe", "4242424242424242", 4, 2028, "Work").unwrap();
        card.id = 6;
        let never = vec!["https://never.example".to_owned()];
        let json = snapshot_json(&View {
            available: true, guest: false, prefs: Prefs::default(), logins: &[], total: 0, query: "", never: &never,
            addresses: std::slice::from_ref(&address), cards: std::slice::from_ref(&card),
            import: Some(ImportView::Preview { provider: "Bitwarden", new: 3, duplicates: 1, conflicts: 0, rejected: 2 }),
            ack: Ack::Saved, error: "", reveal: None, generated: Some("Gen#1"),
        });
        let fixture = include_str!("../../ui/test/fixtures/passwords-snapshot.json");
        assert_eq!(json, fixture.trim());
        assert!(!json.contains("4242424242424242"), "a snapshot never carries a card number");
    }
}
