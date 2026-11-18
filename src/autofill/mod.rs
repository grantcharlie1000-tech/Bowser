//! Form autofill: saved addresses and payment cards, the kinds of field a page can ask for, and
//! the scripts that find and fill those fields.
//!
//! The wallet lives in `Autofill.dat` beside `Passwords.dat` and is protected the same way:
//! DPAPI for the current Windows user, replaced atomically on every write
//! (`passwords::write_protected`). Card security codes are never stored or captured.
//!
//! The in-page side is two pieces of JavaScript that share one field classifier
//! (`fields.js`): `page.js`, added to every eligible top-level document, offers a fill button in
//! a recognised field and reports submitted forms; and the fill script built by [`fill_script`],
//! which runs only after the user picks an entry in Bowser's own menu.

use std::{fmt, path::PathBuf};
use zeroize::{Zeroize, Zeroizing};

use crate::passwords::{self, PasswordError};

pub const FILE_NAME: &str = "Autofill.dat";
const MAGIC: &[u8; 8] = b"BWSRAF01";
const VERSION: u32 = 1;
const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 200;
pub const MAX_VALUE_BYTES: usize = 512;

/// The classifier shared by the detector and the fill script (defines `kindOf`, `scopeOf`, `vis`).
const FIELDS_JS: &str = include_str!("fields.js");
/// The detector added to every top-level document whose profile saves sign-ins.
pub const PAGE_SCRIPT: &str = concat!(
    "(()=>{if(window.top!==window||!window.chrome||!window.chrome.webview)return;",
    include_str!("fields.js"),
    include_str!("page.js"),
    "})();"
);
const FILL_JS: &str = include_str!("fill.js");

/// A kind of form field, named as the HTML `autocomplete` tokens name them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldKind {
    Username,
    Email,
    CurrentPassword,
    NewPassword,
    GivenName,
    FamilyName,
    FullName,
    Organization,
    Tel,
    AddressLine1,
    AddressLine2,
    City,
    Region,
    PostalCode,
    Country,
    CcName,
    CcNumber,
    CcExp,
    CcExpMonth,
    CcExpYear,
    CcCsc,
}

/// What a field belongs to, which decides what Bowser offers for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Login,
    Identity,
    Card,
}

impl FieldKind {
    pub const ALL: [FieldKind; 21] = [
        Self::Username, Self::Email, Self::CurrentPassword, Self::NewPassword, Self::GivenName, Self::FamilyName,
        Self::FullName, Self::Organization, Self::Tel, Self::AddressLine1, Self::AddressLine2, Self::City, Self::Region,
        Self::PostalCode, Self::Country, Self::CcName, Self::CcNumber, Self::CcExp, Self::CcExpMonth, Self::CcExpYear,
        Self::CcCsc,
    ];

    pub fn token(self) -> &'static str {
        match self {
            Self::Username => "username",
            Self::Email => "email",
            Self::CurrentPassword => "current-password",
            Self::NewPassword => "new-password",
            Self::GivenName => "given-name",
            Self::FamilyName => "family-name",
            Self::FullName => "name",
            Self::Organization => "organization",
            Self::Tel => "tel",
            Self::AddressLine1 => "address-line1",
            Self::AddressLine2 => "address-line2",
            Self::City => "address-level2",
            Self::Region => "address-level1",
            Self::PostalCode => "postal-code",
            Self::Country => "country",
            Self::CcName => "cc-name",
            Self::CcNumber => "cc-number",
            Self::CcExp => "cc-exp",
            Self::CcExpMonth => "cc-exp-month",
            Self::CcExpYear => "cc-exp-year",
            Self::CcCsc => "cc-csc",
        }
    }

    pub fn from_token(token: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.token() == token)
    }

    /// The group a field of this kind belongs to. An email field joins a login when its form also
    /// has a password; the page decides that and reports the email as `username` there.
    pub fn group(self) -> Group {
        match self {
            Self::Username | Self::CurrentPassword | Self::NewPassword => Group::Login,
            Self::CcName | Self::CcNumber | Self::CcExp | Self::CcExpMonth | Self::CcExpYear | Self::CcCsc => Group::Card,
            _ => Group::Identity,
        }
    }

    /// Whether a page may report this kind's value in a submitted form. Passwords travel in their
    /// own message, and a card's security code is never captured.
    pub fn capturable(self) -> bool {
        !matches!(self, Self::Username | Self::CurrentPassword | Self::NewPassword | Self::CcCsc)
    }
}

/// A saved name and address, plus the contact details forms ask for with them.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Address {
    pub id: u64,
    pub name: String,
    pub organization: String,
    pub email: String,
    pub tel: String,
    pub line1: String,
    pub line2: String,
    pub city: String,
    pub region: String,
    pub postal_code: String,
    pub country: String,
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Address").field("id", &self.id).field("fields", &"[PERSONAL]").finish()
    }
}

impl Address {
    /// The ten stored fields in file order, with the keys the settings page uses.
    pub const KEYS: [&'static str; 10] =
        ["name", "organization", "email", "tel", "line1", "line2", "city", "region", "postal", "country"];

    fn fields(&self) -> [&String; 10] {
        [&self.name, &self.organization, &self.email, &self.tel, &self.line1, &self.line2, &self.city, &self.region, &self.postal_code, &self.country]
    }

    fn fields_mut(&mut self) -> [&mut String; 10] {
        [&mut self.name, &mut self.organization, &mut self.email, &mut self.tel, &mut self.line1, &mut self.line2, &mut self.city, &mut self.region, &mut self.postal_code, &mut self.country]
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        Self::KEYS.iter().position(|k| *k == key).map(|i| self.fields()[i].as_str())
    }

    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let Some(index) = Self::KEYS.iter().position(|k| *k == key) else { return false };
        let Some(value) = clean_value(value) else { return false };
        *self.fields_mut()[index] = value;
        true
    }

    pub fn is_empty(&self) -> bool {
        self.fields().iter().all(|value| value.is_empty())
    }

    /// The given name and family name, split at the last space of the full name.
    pub fn split_name(&self) -> (&str, &str) {
        let name = self.name.trim();
        match name.rsplit_once(' ') {
            Some((given, family)) => (given.trim(), family.trim()),
            None => (name, ""),
        }
    }

    /// One line for menus: "Jane Doe, 1 High Street".
    pub fn summary(&self) -> String {
        [self.name.as_str(), self.line1.as_str(), self.email.as_str(), self.postal_code.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .take(2)
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The value this address fills into a field of `kind`, if it has one.
    pub fn value_for(&self, kind: FieldKind) -> Option<String> {
        let (given, family) = self.split_name();
        let value = match kind {
            FieldKind::FullName => self.name.as_str(),
            FieldKind::GivenName => given,
            FieldKind::FamilyName => family,
            FieldKind::Organization => &self.organization,
            FieldKind::Email => &self.email,
            FieldKind::Tel => &self.tel,
            FieldKind::AddressLine1 => &self.line1,
            FieldKind::AddressLine2 => &self.line2,
            FieldKind::City => &self.city,
            FieldKind::Region => &self.region,
            FieldKind::PostalCode => &self.postal_code,
            FieldKind::Country => &self.country,
            _ => "",
        };
        (!value.is_empty()).then(|| value.to_owned())
    }

    /// Builds an address from a submitted form's values (the page's `autofill-submitted`).
    pub fn from_form(values: &[(FieldKind, String)]) -> Self {
        let get = |kind: FieldKind| values.iter().find(|(k, _)| *k == kind).map(|(_, v)| v.trim().to_owned()).unwrap_or_default();
        let mut name = get(FieldKind::FullName);
        if name.is_empty() {
            name = [get(FieldKind::GivenName), get(FieldKind::FamilyName)]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
        }
        let mut address = Self {
            id: 0,
            name,
            organization: get(FieldKind::Organization),
            email: get(FieldKind::Email),
            tel: get(FieldKind::Tel),
            line1: get(FieldKind::AddressLine1),
            line2: get(FieldKind::AddressLine2),
            city: get(FieldKind::City),
            region: get(FieldKind::Region),
            postal_code: get(FieldKind::PostalCode),
            country: get(FieldKind::Country),
        };
        for value in address.fields_mut() {
            *value = clean_value(value).unwrap_or_default();
        }
        address
    }

    /// Whether `other` is the same person at the same place, ignoring case and spacing, so a form
    /// submitted with a saved address is not offered again.
    pub fn same_as(&self, other: &Address) -> bool {
        let norm = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
        let key = |a: &Address| (norm(&a.name), norm(&a.line1), norm(&a.postal_code), norm(&a.email));
        let (mine, theirs) = (key(self), key(other));
        // Fields the form did not have do not count against a match.
        let agree = |a: &String, b: &String| a.is_empty() || b.is_empty() || a == b;
        agree(&mine.0, &theirs.0) && agree(&mine.1, &theirs.1) && agree(&mine.2, &theirs.2) && agree(&mine.3, &theirs.3)
    }

    /// Whether a submitted form carried enough of an address or contact card to be worth saving.
    pub fn worth_saving(&self) -> bool {
        let filled = self.fields().iter().filter(|value| !value.is_empty()).count();
        filled >= 3 && (!self.name.is_empty() || !self.line1.is_empty())
    }
}

/// A saved payment card. The number zeroizes on drop and never appears in Debug output.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Card {
    pub id: u64,
    pub name: String,
    number: Zeroizing<String>,
    pub exp_month: u8,
    pub exp_year: u16,
    pub nickname: String,
}

impl fmt::Debug for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Card").field("id", &self.id).field("brand", &self.brand()).field("last4", &self.last4()).finish()
    }
}

impl Card {
    /// A card from its parts. The number keeps digits only and must pass the Luhn check; the
    /// expiry month is 1-12 (0 when unknown) and the year four digits (0 when unknown).
    pub fn new(name: &str, number: &str, exp_month: u8, exp_year: u16, nickname: &str) -> Option<Self> {
        let digits: Zeroizing<String> = Zeroizing::new(number.chars().filter(char::is_ascii_digit).collect());
        if !luhn_valid(&digits) || exp_month > 12 || !(exp_year == 0 || (2000..=2099).contains(&exp_year)) {
            return None;
        }
        Some(Self { id: 0, name: clean_value(name)?, number: digits, exp_month, exp_year, nickname: clean_value(nickname)? })
    }

    pub fn number(&self) -> &str {
        &self.number
    }

    pub fn last4(&self) -> &str {
        &self.number[self.number.len().saturating_sub(4)..]
    }

    pub fn brand(&self) -> &'static str {
        card_brand(&self.number)
    }

    /// "Visa •••• 4242", or the nickname when the user gave one.
    pub fn summary(&self) -> String {
        let masked = format!("{} \u{2022}\u{2022}\u{2022}\u{2022} {}", self.brand(), self.last4());
        if self.nickname.is_empty() { masked } else { format!("{} ({masked})", self.nickname) }
    }

    /// "04/28", or an empty string when the expiry is unknown.
    pub fn expiry(&self) -> String {
        if self.exp_month == 0 || self.exp_year == 0 {
            return String::new();
        }
        format!("{:02}/{:02}", self.exp_month, self.exp_year % 100)
    }

    pub fn value_for(&self, kind: FieldKind) -> Option<String> {
        let value = match kind {
            FieldKind::CcName => self.name.clone(),
            FieldKind::CcNumber => self.number.to_string(),
            FieldKind::CcExp => self.expiry(),
            FieldKind::CcExpMonth if self.exp_month > 0 => format!("{:02}", self.exp_month),
            FieldKind::CcExpYear if self.exp_year > 0 => self.exp_year.to_string(),
            _ => String::new(),
        };
        (!value.is_empty()).then_some(value)
    }

    /// A card from a submitted checkout form, if its number is a valid card number.
    pub fn from_form(values: &[(FieldKind, String)]) -> Option<Self> {
        let get = |kind: FieldKind| values.iter().find(|(k, _)| *k == kind).map(|(_, v)| v.trim()).unwrap_or("");
        let (mut month, mut year) = (parse_month(get(FieldKind::CcExpMonth)), parse_year(get(FieldKind::CcExpYear)));
        if let Some((m, y)) = parse_expiry(get(FieldKind::CcExp)) {
            month = month.or(Some(m));
            year = year.or(Some(y));
        }
        Self::new(get(FieldKind::CcName), get(FieldKind::CcNumber), month.unwrap_or(0), year.unwrap_or(0), "")
    }
}

impl Drop for Address {
    fn drop(&mut self) {
        for value in self.fields_mut() {
            value.zeroize();
        }
    }
}

/// Keeps single-line printable text of at most [`MAX_VALUE_BYTES`], trimmed. `None` when the
/// value is too long.
fn clean_value(value: &str) -> Option<String> {
    let cleaned: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| !matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (cleaned.len() <= MAX_VALUE_BYTES).then_some(cleaned)
}

/// The Luhn checksum every payment card number carries, over 12 to 19 digits.
pub fn luhn_valid(digits: &str) -> bool {
    if !(12..=19).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let sum: u32 = digits
        .bytes()
        .rev()
        .enumerate()
        .map(|(i, b)| {
            let d = u32::from(b - b'0');
            if i % 2 == 1 { if d * 2 > 9 { d * 2 - 9 } else { d * 2 } } else { d }
        })
        .sum();
    sum % 10 == 0
}

/// The card network a number's leading digits belong to.
pub fn card_brand(digits: &str) -> &'static str {
    let prefix = |n: usize| digits.get(..n).and_then(|p| p.parse::<u32>().ok()).unwrap_or(0);
    match () {
        _ if digits.starts_with('4') => "Visa",
        _ if (51..=55).contains(&prefix(2)) || (2221..=2720).contains(&prefix(4)) => "Mastercard",
        _ if matches!(prefix(2), 34 | 37) => "American Express",
        _ if digits.starts_with("6011") || digits.starts_with("65") || (644..=649).contains(&prefix(3)) => "Discover",
        _ if (3528..=3589).contains(&prefix(4)) => "JCB",
        _ if matches!(prefix(2), 36 | 38 | 39) || (300..=305).contains(&prefix(3)) => "Diners Club",
        _ if digits.starts_with("62") => "UnionPay",
        _ if matches!(prefix(2), 50 | 56 | 57 | 58 | 63 | 67) => "Maestro",
        _ => "Card",
    }
}

fn parse_month(text: &str) -> Option<u8> {
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    digits.parse::<u8>().ok().filter(|m| (1..=12).contains(m))
}

fn parse_year(text: &str) -> Option<u16> {
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    match digits.len() {
        2 => digits.parse::<u16>().ok().map(|y| 2000 + y),
        4 => digits.parse::<u16>().ok().filter(|y| (2000..=2099).contains(y)),
        _ => None,
    }
}

/// "04/28", "4/2028", "04-28" and "0428" as a month and four-digit year.
pub fn parse_expiry(text: &str) -> Option<(u8, u16)> {
    let text = text.trim();
    let (month, year) = match text.split_once(['/', '-', ' ', '.']) {
        Some((m, y)) => (m, y),
        None if text.len() == 4 || text.len() == 6 => text.split_at(2),
        None => return None,
    };
    Some((parse_month(month)?, parse_year(year)?))
}

/// What the wallet lets Bowser do, chosen on the settings page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prefs {
    /// Offer to save passwords people type into sign-in forms.
    pub offer_passwords: bool,
    /// Offer and fill saved addresses.
    pub addresses: bool,
    /// Offer and fill saved payment cards.
    pub cards: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { offer_passwords: true, addresses: true, cards: true }
    }
}

impl Prefs {
    fn bits(self) -> u32 {
        // Stored inverted so that a fresh (zeroed) field means "everything on".
        u32::from(!self.offer_passwords) | u32::from(!self.addresses) << 1 | u32::from(!self.cards) << 2
    }
    fn from_bits(bits: u32) -> Self {
        Self { offer_passwords: bits & 1 == 0, addresses: bits & 2 == 0, cards: bits & 4 == 0 }
    }

    /// Sets one preference by its settings-page key.
    pub fn set(&mut self, key: &str, on: bool) -> bool {
        match key {
            "save" => self.offer_passwords = on,
            "addresses" => self.addresses = on,
            "cards" => self.cards = on,
            _ => return false,
        }
        true
    }
}

/// The saved addresses and cards of the running profile.
pub struct Wallet {
    path: PathBuf,
    pub prefs: Prefs,
    next_id: u64,
    addresses: Vec<Address>,
    cards: Vec<Card>,
}

impl fmt::Debug for Wallet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Wallet").field("addresses", &self.addresses.len()).field("cards", &self.cards.len()).finish()
    }
}

impl Wallet {
    /// Opens the running profile's wallet. Guest profiles have none.
    pub fn open_default() -> Result<Self, PasswordError> {
        let profile = crate::profile::active().map_err(|_| PasswordError::Io)?;
        if profile.is_guest() {
            return Err(PasswordError::Disabled);
        }
        Self::open(profile.autofill_file())
    }

    fn open(path: PathBuf) -> Result<Self, PasswordError> {
        let mut wallet = Self { path, prefs: Prefs::default(), next_id: 1, addresses: Vec::new(), cards: Vec::new() };
        if let Some(plain) = passwords::read_protected(&wallet.path, MAX_FILE_BYTES)? {
            wallet.decode(&plain)?;
        }
        Ok(wallet)
    }

    pub fn addresses(&self) -> &[Address] {
        &self.addresses
    }

    pub fn cards(&self) -> &[Card] {
        &self.cards
    }

    pub fn address(&self, id: u64) -> Option<&Address> {
        self.addresses.iter().find(|a| a.id == id)
    }

    pub fn card(&self, id: u64) -> Option<&Card> {
        self.cards.iter().find(|c| c.id == id)
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1) & ((1 << 53) - 1);
        id
    }

    /// Adds an address (`id` 0) or replaces the one with that id. Returns its id.
    pub fn save_address(&mut self, mut address: Address) -> Result<u64, PasswordError> {
        if address.is_empty() {
            return Err(PasswordError::InvalidStore);
        }
        let before = (self.addresses.clone(), self.next_id);
        if address.id == 0 {
            if self.addresses.len() >= MAX_ENTRIES {
                return Err(PasswordError::TooManyRows);
            }
            address.id = self.take_id();
            self.addresses.push(address.clone());
        } else {
            let slot = self.addresses.iter_mut().find(|a| a.id == address.id).ok_or(PasswordError::MissingEntry)?;
            *slot = address.clone();
        }
        self.persist_or(|wallet| (wallet.addresses, wallet.next_id) = before)?;
        Ok(address.id)
    }

    /// Adds a card (`id` 0) or replaces the one with that id. A card whose number is already saved
    /// is refused as a duplicate.
    pub fn save_card(&mut self, mut card: Card) -> Result<u64, PasswordError> {
        if self.cards.iter().any(|c| c.id != card.id && c.number() == card.number()) {
            return Err(PasswordError::Duplicate);
        }
        let before = (self.cards.clone(), self.next_id);
        if card.id == 0 {
            if self.cards.len() >= MAX_ENTRIES {
                return Err(PasswordError::TooManyRows);
            }
            card.id = self.take_id();
            self.cards.push(card.clone());
        } else {
            let slot = self.cards.iter_mut().find(|c| c.id == card.id).ok_or(PasswordError::MissingEntry)?;
            *slot = card.clone();
        }
        self.persist_or(|wallet| (wallet.cards, wallet.next_id) = before)?;
        Ok(card.id)
    }

    pub fn delete_address(&mut self, id: u64) -> Result<(), PasswordError> {
        let index = self.addresses.iter().position(|a| a.id == id).ok_or(PasswordError::MissingEntry)?;
        let removed = self.addresses.remove(index);
        self.persist_or(|wallet| wallet.addresses.insert(index, removed))
    }

    pub fn delete_card(&mut self, id: u64) -> Result<(), PasswordError> {
        let index = self.cards.iter().position(|c| c.id == id).ok_or(PasswordError::MissingEntry)?;
        let removed = self.cards.remove(index);
        self.persist_or(|wallet| wallet.cards.insert(index, removed))
    }

    pub fn set_prefs(&mut self, prefs: Prefs) -> Result<(), PasswordError> {
        let before = std::mem::replace(&mut self.prefs, prefs);
        self.persist_or(|wallet| wallet.prefs = before)
    }

    /// Deletes every address and card (Delete Browsing Data). Preferences stay.
    pub fn delete_all(&mut self) -> Result<usize, PasswordError> {
        let count = self.addresses.len() + self.cards.len();
        let before = (std::mem::take(&mut self.addresses), std::mem::take(&mut self.cards));
        self.persist_or(|wallet| (wallet.addresses, wallet.cards) = before)?;
        Ok(count)
    }

    /// Writes the wallet, or puts memory back as it was with `undo` when the write fails.
    fn persist_or(&mut self, undo: impl FnOnce(&mut Self)) -> Result<(), PasswordError> {
        let result = self.encode().and_then(|plain| passwords::write_protected(&self.path, &plain));
        if result.is_err() {
            undo(self);
        }
        result
    }

    fn encode(&self) -> Result<Zeroizing<Vec<u8>>, PasswordError> {
        let mut out = Zeroizing::new(Vec::with_capacity(4096));
        out.extend_from_slice(MAGIC);
        out.extend(VERSION.to_le_bytes());
        out.extend(self.prefs.bits().to_le_bytes());
        out.extend(self.next_id.to_le_bytes());
        out.extend((self.addresses.len() as u32).to_le_bytes());
        for address in &self.addresses {
            out.extend(address.id.to_le_bytes());
            for value in address.fields() {
                put_str(&mut out, value)?;
            }
        }
        out.extend((self.cards.len() as u32).to_le_bytes());
        for card in &self.cards {
            out.extend(card.id.to_le_bytes());
            put_str(&mut out, &card.name)?;
            put_str(&mut out, &card.number)?;
            put_str(&mut out, &card.nickname)?;
            out.push(card.exp_month);
            out.extend(card.exp_year.to_le_bytes());
        }
        if out.len() > MAX_FILE_BYTES {
            return Err(PasswordError::InvalidStore);
        }
        Ok(out)
    }

    fn decode(&mut self, bytes: &[u8]) -> Result<(), PasswordError> {
        let mut r = Reader { bytes, at: 0 };
        if r.take(8)? != MAGIC || r.u32()? != VERSION {
            return Err(PasswordError::InvalidStore);
        }
        self.prefs = Prefs::from_bits(r.u32()?);
        self.next_id = r.u64()?.max(1);
        let count = r.u32()? as usize;
        if count > MAX_ENTRIES {
            return Err(PasswordError::InvalidStore);
        }
        for _ in 0..count {
            let mut address = Address::default();
            address.id = r.u64()?;
            for value in address.fields_mut() {
                *value = r.string()?;
            }
            self.addresses.push(address);
        }
        let count = r.u32()? as usize;
        if count > MAX_ENTRIES {
            return Err(PasswordError::InvalidStore);
        }
        for _ in 0..count {
            let id = r.u64()?;
            let (name, number, nickname) = (r.string()?, Zeroizing::new(r.string()?), r.string()?);
            let exp_month = r.take(1)?[0];
            let exp_year = u16::from_le_bytes(r.take(2)?.try_into().map_err(|_| PasswordError::InvalidStore)?);
            let mut card = Card::new(&name, &number, exp_month, exp_year, &nickname).ok_or(PasswordError::InvalidStore)?;
            card.id = id;
            self.cards.push(card);
        }
        if r.at != bytes.len() {
            return Err(PasswordError::InvalidStore);
        }
        Ok(())
    }
}

fn put_str(out: &mut Vec<u8>, value: &str) -> Result<(), PasswordError> {
    if value.len() > MAX_VALUE_BYTES {
        return Err(PasswordError::InvalidStore);
    }
    out.extend((value.len() as u32).to_le_bytes());
    out.extend(value.as_bytes());
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], PasswordError> {
        let end = self.at.checked_add(n).filter(|&end| end <= self.bytes.len()).ok_or(PasswordError::InvalidStore)?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }
    fn u32(&mut self) -> Result<u32, PasswordError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| PasswordError::InvalidStore)?))
    }
    fn u64(&mut self) -> Result<u64, PasswordError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().map_err(|_| PasswordError::InvalidStore)?))
    }
    fn string(&mut self) -> Result<String, PasswordError> {
        let n = self.u32()? as usize;
        if n > MAX_VALUE_BYTES {
            return Err(PasswordError::InvalidStore);
        }
        std::str::from_utf8(self.take(n)?).map(str::to_owned).map_err(|_| PasswordError::InvalidStore)
    }
}

/// What a fill script puts into the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillMode {
    /// A saved login: username (and email-as-username) and the current password.
    Login,
    /// A freshly generated password for every new-password field (and its confirmation).
    Generated,
    /// A saved address or a saved card.
    Form,
}

impl FillMode {
    fn token(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Generated => "generate",
            Self::Form => "form",
        }
    }
}

/// Builds a top-document-only script that fills `values` into the recognised fields around the
/// page's focused field (its form, or the closest form-like container), and nowhere else. It does
/// nothing unless the document still has the exact `origin` the values were chosen for. The
/// returned buffer zeroizes on drop: it holds the values until WebView2 copies it.
pub fn fill_script(origin: &str, mode: FillMode, values: &[(FieldKind, &str)]) -> Zeroizing<String> {
    let capacity = values.iter().map(|(_, v)| v.len() * 6 + 24).sum::<usize>() + FIELDS_JS.len() + FILL_JS.len() + origin.len() * 6 + 64;
    let mut script = Zeroizing::new(String::with_capacity(capacity));
    script.push_str("(()=>{if(window.top!==window)return false;const O=");
    append_js_string(&mut script, origin);
    script.push_str(";if(location.origin!==O)return false;const M=");
    append_js_string(&mut script, mode.token());
    script.push_str(";const V=new Map([");
    for (index, (kind, value)) in values.iter().enumerate() {
        if index > 0 {
            script.push(',');
        }
        script.push('[');
        append_js_string(&mut script, kind.token());
        script.push(',');
        append_js_string(&mut script, value);
        script.push(']');
    }
    script.push_str("]);");
    script.push_str(FIELDS_JS);
    script.push_str(FILL_JS);
    script.push_str("})()");
    script
}

/// A JavaScript string literal for `value`, safe inside a script (quotes, backslashes, line and
/// paragraph separators and every control character escaped).
fn append_js_string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '<' => output.push_str("\\u003c"),
            '\u{2028}' => output.push_str("\\u2028"),
            '\u{2029}' => output.push_str("\\u2029"),
            character if character.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(output, "\\u{:04x}", character as u32);
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_through_its_token_and_groups_sensibly() {
        for kind in FieldKind::ALL {
            assert_eq!(FieldKind::from_token(kind.token()), Some(kind));
        }
        assert_eq!(FieldKind::from_token("constructor"), None);
        assert_eq!(FieldKind::NewPassword.group(), Group::Login);
        assert_eq!(FieldKind::CcExpYear.group(), Group::Card);
        assert_eq!(FieldKind::PostalCode.group(), Group::Identity);
        assert!(!FieldKind::CcCsc.capturable() && !FieldKind::CurrentPassword.capturable() && FieldKind::CcNumber.capturable());
    }

    #[test]
    fn card_numbers_are_checked_branded_and_masked() {
        assert!(luhn_valid("4242424242424242") && luhn_valid("378282246310005") && !luhn_valid("4242424242424241"));
        assert!(!luhn_valid("42424") && !luhn_valid("4242 4242 4242 4242"));
        let cases = [("4242424242424242", "Visa"), ("5555555555554444", "Mastercard"), ("2223003122003222", "Mastercard"),
            ("378282246310005", "American Express"), ("6011111111111117", "Discover"), ("3530111333300000", "JCB"),
            ("36227206271667", "Diners Club"), ("6200000000000005", "UnionPay")];
        for (number, brand) in cases {
            assert_eq!(card_brand(number), brand, "{number}");
        }
        let card = Card::new("Jane Doe", "4242 4242 4242 4242", 4, 2028, "").unwrap();
        assert_eq!(card.summary(), "Visa \u{2022}\u{2022}\u{2022}\u{2022} 4242");
        assert_eq!(card.expiry(), "04/28");
        assert_eq!(card.value_for(FieldKind::CcExpMonth).as_deref(), Some("04"));
        assert_eq!(card.value_for(FieldKind::CcExpYear).as_deref(), Some("2028"));
        assert_eq!(card.value_for(FieldKind::CcCsc), None);
        assert!(!format!("{card:?}").contains("42424242"));
        assert!(Card::new("x", "4242424242424242", 13, 2028, "").is_none());
        assert!(Card::new("x", "4242424242424242", 1, 1999, "").is_none());
    }

    #[test]
    fn expiry_dates_parse_in_the_usual_spellings() {
        assert_eq!(parse_expiry("04/28"), Some((4, 2028)));
        assert_eq!(parse_expiry("4/2028"), Some((4, 2028)));
        assert_eq!(parse_expiry("12-30"), Some((12, 2030)));
        assert_eq!(parse_expiry("0428"), Some((4, 2028)));
        assert_eq!(parse_expiry("13/28"), None);
        assert_eq!(parse_expiry("soon"), None);
    }

    #[test]
    fn a_submitted_checkout_becomes_an_address_and_a_card() {
        let form = |pairs: &[(&str, &str)]| -> Vec<(FieldKind, String)> {
            pairs.iter().map(|(k, v)| (FieldKind::from_token(k).unwrap(), (*v).to_owned())).collect()
        };
        let values = form(&[("given-name", "Jane"), ("family-name", "Doe"), ("address-line1", " 1  High\tStreet "),
            ("address-level2", "Leeds"), ("postal-code", "LS1 1AA"), ("cc-number", "4242-4242-4242-4242"), ("cc-exp", "04 / 28")]);
        let address = Address::from_form(&values);
        assert_eq!(address.name, "Jane Doe");
        assert_eq!(address.line1, "1 High Street");
        assert!(address.worth_saving());
        assert_eq!(address.summary(), "Jane Doe, 1 High Street");
        assert_eq!(address.split_name(), ("Jane", "Doe"));
        assert_eq!(address.value_for(FieldKind::GivenName).as_deref(), Some("Jane"));
        let card = Card::from_form(&values).unwrap();
        assert_eq!((card.last4(), card.exp_month, card.exp_year), ("4242", 4, 2028));
        assert!(Card::from_form(&form(&[("cc-number", "1234")])).is_none());
        let partial = Address::from_form(&form(&[("given-name", "JANE"), ("family-name", "doe"), ("postal-code", "ls11aa")]));
        assert!(partial.same_as(&address));
        assert!(!Address::from_form(&form(&[("email", "a@b.c")])).worth_saving());
    }

    #[test]
    fn the_wallet_encoding_round_trips_and_rejects_trailing_bytes() {
        let mut wallet = Wallet { path: PathBuf::new(), prefs: Prefs { cards: false, ..Prefs::default() }, next_id: 7, addresses: Vec::new(), cards: Vec::new() };
        let mut address = Address::default();
        address.id = 3;
        assert!(address.set("name", "Jane Doe") && address.set("country", "United Kingdom") && !address.set("nope", "x"));
        wallet.addresses.push(address);
        let mut card = Card::new("Jane Doe", "5555555555554444", 1, 2030, "Work").unwrap();
        card.id = 4;
        wallet.cards.push(card);
        let bytes = wallet.encode().unwrap();
        let mut back = Wallet { path: PathBuf::new(), prefs: Prefs::default(), next_id: 1, addresses: Vec::new(), cards: Vec::new() };
        back.decode(&bytes).unwrap();
        assert_eq!(back.prefs, wallet.prefs);
        assert_eq!(back.next_id, 7);
        assert_eq!(back.addresses, wallet.addresses);
        assert_eq!(back.cards, wallet.cards);
        let mut longer = bytes.to_vec();
        longer.push(0);
        let mut again = Wallet { path: PathBuf::new(), prefs: Prefs::default(), next_id: 1, addresses: Vec::new(), cards: Vec::new() };
        assert!(again.decode(&longer).is_err());
        assert_eq!(Prefs::from_bits(0), Prefs::default());
    }

    #[test]
    fn fill_scripts_escape_values_and_pin_the_origin() {
        let script = fill_script("https://example.com", FillMode::Form, &[(FieldKind::FullName, "a\"b</script>\u{2028}")]);
        assert!(script.starts_with("(()=>{if(window.top!==window)return false;const O=\"https://example.com\";if(location.origin!==O)return false;"));
        let expected = ["[\"name\",\"a\\\"b", "\\u003c/script>", "\\u2028\"]"].concat();
        assert!(script.contains(&expected), "{}", &*script);
        assert!(!script.contains("</script>"));
        assert!(script.ends_with("})()"));
        assert!(PAGE_SCRIPT.starts_with("(()=>{if(window.top!==window"));
    }
}
