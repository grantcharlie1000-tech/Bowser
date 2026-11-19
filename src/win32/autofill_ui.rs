//! Autofill in the page: the menu under a field, filling a chosen entry, suggesting a strong
//! password, and offering to save what a sign-in or checkout form submitted.
//!
//! Messages come from the detector (`src/autofill/page.js`) through the engine, which has
//! already checked them against the tab's live top-level origin (`passwords::validate_login_message`).
//! Nothing saved goes to a page until the user picks it in Bowser's own menu, and then only as a
//! fill script pinned to the origin the menu was built for.

use std::{cell::RefCell, collections::HashSet, time::{Duration, Instant}};
use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    UI::WindowsAndMessaging::GetWindowRect,
};
use zeroize::Zeroizing;

use crate::{
    app::{App, AppRef},
    autofill::{Address, Card, FieldKind, FillMode, Group, Wallet, fill_script},
    passwords::{self, GeneratorOptions, LoginMatch, LoginMessage, LoginMessageKind, PasswordStore},
    tabs::TabId,
    win32::menu::{self, Entry, Icon, Item, Menu, Placement},
};

/// A generated password is saved without asking when the same site submits it within this time.
const GENERATED_TTL: Duration = Duration::from_secs(15 * 60);

const ID_GENERATE: usize = 200;
const ID_ADDRESS: usize = 300;
const ID_CARD: usize = 600;
const ID_MANAGE: usize = 900;
const MAX_MENU_ROWS: usize = 8;

thread_local! {
    /// The password Bowser last generated, for which origin and when.
    static GENERATED: RefCell<Option<(String, Zeroizing<String>, Instant)>> = const { RefCell::new(None) };
    /// Sites where the user chose "Not now" this session.
    static DECLINED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static PROMPT_OPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Every validated page message for `tab_id` comes here (from `WM_LOGIN_MESSAGE`).
pub(crate) fn handle_page_message(hwnd: HWND, app: &AppRef, tab_id: TabId, message: LoginMessage) {
    if App::active_tab_id(app) != tab_id {
        return;
    }
    let Some(current) = App::tab_committed_url(app, tab_id) else { return };
    if passwords::secure_origin(current.as_str()).as_deref() != Some(message.origin.as_str()) {
        return;
    }
    match message.kind {
        LoginMessageKind::FormSeen => {}
        LoginMessageKind::Submitted => offer_save_login(hwnd, message),
        LoginMessageKind::AutofillRequest => {
            if let Some((kind, rect)) = message.field {
                let anchor = App::page_field_anchor(app, tab_id, rect.x, rect.y, rect.h);
                show_fill_menu(hwnd, tab_id, &message.origin, kind, anchor);
            }
        }
        LoginMessageKind::AutofillSubmitted => offer_save_form(hwnd, message),
        // The suggestion card's click: the same action as "Use a strong password" in the fill menu.
        LoginMessageKind::GeneratePassword => fill_generated(app, tab_id, &message.origin),
    }
}

/// The toolbar's "Fill password" command: the same menu, for the page's sign-in fields.
pub(crate) fn offer_fill(hwnd: HWND, app: &AppRef) {
    let Some((tab_id, url)) = App::active_site(app) else { return };
    let Some(origin) = passwords::secure_origin(url.as_str()) else { return };
    show_fill_menu(hwnd, tab_id, &origin, FieldKind::CurrentPassword, None);
}

fn show_fill_menu(hwnd: HWND, tab_id: TabId, origin: &str, kind: FieldKind, anchor: Option<POINT>) {
    let wallet = Wallet::open_default().ok();
    let prefs = wallet.as_ref().map(|w| w.prefs).unwrap_or_default();
    let mut entries = Vec::new();
    let mut logins = Vec::new();
    let mut addresses: Vec<Address> = Vec::new();
    let mut cards: Vec<Card> = Vec::new();
    match kind.group() {
        Group::Login => {
            if let Ok(vault) = PasswordStore::open_default() {
                logins = vault.get_for_origin(origin);
            }
            let site = origin.split_once("://").map_or(origin, |(_, rest)| rest);
            if !logins.is_empty() {
                entries.push(Entry::SectionHeader(format!("Saved for {site}")));
                for (index, login) in logins.iter().take(MAX_MENU_ROWS).enumerate() {
                    entries.push(Entry::Item(Item::new(index + 1, login.username.clone(), Icon::Key).glyph("key")));
                }
            }
            if kind == FieldKind::NewPassword || logins.is_empty() || kind == FieldKind::CurrentPassword {
                if !entries.is_empty() {
                    entries.push(Entry::Separator);
                }
                entries.push(Entry::Item(
                    Item::new(ID_GENERATE, "Use a strong password", Icon::Shield)
                        .glyph("sparkle")
                        .described("Bowser makes a unique 20-character password, fills it in and saves it when you submit"),
                ));
            }
        }
        Group::Identity => {
            if prefs.addresses {
                addresses = wallet.as_ref().map(|w| w.addresses().to_vec()).unwrap_or_default();
                if !addresses.is_empty() {
                    entries.push(Entry::SectionHeader("Fill address".to_owned()));
                }
                for (index, address) in addresses.iter().take(MAX_MENU_ROWS).enumerate() {
                    entries.push(Entry::Item(Item::new(ID_ADDRESS + index, address.summary(), Icon::Person).glyph("person")));
                }
            }
        }
        Group::Card => {
            if prefs.cards {
                cards = wallet.as_ref().map(|w| w.cards().to_vec()).unwrap_or_default();
                if !cards.is_empty() {
                    entries.push(Entry::SectionHeader("Pay with".to_owned()));
                }
                for (index, card) in cards.iter().take(MAX_MENU_ROWS).enumerate() {
                    let mut label = card.summary();
                    let expiry = card.expiry();
                    if !expiry.is_empty() {
                        label = format!("{label}  \u{00b7}  {expiry}");
                    }
                    entries.push(Entry::Item(Item::new(ID_CARD + index, label, Icon::Lock).glyph("card")));
                }
            }
        }
    }
    if !entries.is_empty() {
        entries.push(Entry::Separator);
    }
    let manage = match kind.group() {
        Group::Login => "Manage passwords\u{2026}",
        Group::Identity => "Manage addresses\u{2026}",
        Group::Card => "Manage payment methods\u{2026}",
    };
    entries.push(Entry::Item(Item::new(ID_MANAGE, manage, Icon::Gear).glyph("gear")));
    let (anchor, placement) = match anchor {
        Some(point) => (point, Placement::BelowLeft),
        None => match toolbar_anchor(hwnd) {
            Some(point) => (point, Placement::BelowRight),
            None => return,
        },
    };
    let origin = origin.to_owned();
    let menu = Menu { header: None, entries, footer: "Bowser fills only this site, and only after you choose".to_owned() };
    menu::show(hwnd, anchor, placement, menu, "autofill", move |app, chosen| {
        let Some(chosen) = chosen else { return };
        match chosen.id {
            ID_MANAGE => open_manager(hwnd, app),
            ID_GENERATE => fill_generated(app, tab_id, &origin),
            id if (1..=MAX_MENU_ROWS).contains(&id) => {
                let Some(login) = logins.get(id - 1) else { return };
                // The vault is read again: the entry may have changed since the menu opened.
                let Ok(vault) = PasswordStore::open_default() else { return };
                let Some(id) = vault.find_wire(login.id.wire()) else { return };
                let Ok(password) = vault.reveal(id) else { return };
                let script = fill_script(&origin, FillMode::Login, &[(FieldKind::Username, &login.username), (FieldKind::CurrentPassword, &password)]);
                let _ = App::execute_script_for_origin(app, tab_id, &origin, &script);
            }
            id if (ID_ADDRESS..ID_ADDRESS + MAX_MENU_ROWS).contains(&id) => {
                let Some(address) = addresses.get(id - ID_ADDRESS) else { return };
                let values: Vec<(FieldKind, String)> = FieldKind::ALL.into_iter().filter_map(|k| address.value_for(k).map(|v| (k, v))).collect();
                fill_values(app, tab_id, &origin, &values);
            }
            id if (ID_CARD..ID_CARD + MAX_MENU_ROWS).contains(&id) => {
                let Some(card) = cards.get(id - ID_CARD) else { return };
                let values: Vec<(FieldKind, String)> = FieldKind::ALL.into_iter().filter_map(|k| card.value_for(k).map(|v| (k, v))).collect();
                fill_values(app, tab_id, &origin, &values);
            }
            _ => {}
        }
    });
}

fn fill_values(app: &AppRef, tab_id: TabId, origin: &str, values: &[(FieldKind, String)]) {
    let values: Vec<(FieldKind, &str)> = values.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let script = fill_script(origin, FillMode::Form, &values);
    let _ = App::execute_script_for_origin(app, tab_id, origin, &script);
}

/// Generates a strong password, fills it into the page's new-password fields and remembers it, so
/// the sign-up that follows is saved without a question.
fn fill_generated(app: &AppRef, tab_id: TabId, origin: &str) {
    let Ok(password) = passwords::generate_strong(GeneratorOptions::default()) else { return };
    let script = fill_script(origin, FillMode::Generated, &[(FieldKind::NewPassword, &password)]);
    if App::execute_script_for_origin(app, tab_id, origin, &script).is_ok() {
        GENERATED.with(|slot| *slot.borrow_mut() = Some((origin.to_owned(), Zeroizing::new(password.to_string()), Instant::now())));
    }
}

fn generated_for(origin: &str, password: &str) -> bool {
    GENERATED.with(|slot| {
        slot.borrow().as_ref().is_some_and(|(site, generated, at)| site == origin && generated.as_str() == password && at.elapsed() < GENERATED_TTL)
    })
}

/// Opens the Passwords & autofill section of the settings page (the native panel without the
/// HTML chrome).
pub(crate) fn open_manager(hwnd: HWND, app: &AppRef) {
    if super::settings_page::available(hwnd) {
        super::settings_page::open(hwnd, app, Some(crate::internal_page::Section::Passwords), false);
    } else if let Err(error) = super::passwords_panel::open(hwnd) {
        super::show_error_message(&format!("Passwords could not be opened: {error}"));
    }
}

struct PromptGuard;
impl Drop for PromptGuard {
    fn drop(&mut self) {
        PROMPT_OPEN.with(|open| open.set(false));
    }
}

fn offer_save_login(hwnd: HWND, mut message: LoginMessage) {
    let (Some(username), Some(password)) = (message.username.take(), message.password.take()) else { return };
    let origin = message.origin.clone();
    if !Wallet::open_default().map(|w| w.prefs.offer_passwords).unwrap_or(true) {
        return;
    }
    let Ok(mut vault) = PasswordStore::open_default() else { return };
    if vault.is_never_for_site(&origin) {
        return;
    }
    let matched = vault.match_login(&origin, &username, &password);
    if matches!(matched, LoginMatch::Unchanged(_)) {
        return;
    }
    // A password Bowser generated for this site is saved as soon as the form is submitted.
    if generated_for(&origin, &password) && !username.is_empty() {
        let saved = match matched {
            LoginMatch::Missing => vault.add(&origin, &username, &password).is_ok(),
            LoginMatch::Changed(id) => vault.update(id, &origin, &username, &password).is_ok(),
            LoginMatch::Unchanged(_) => true,
        };
        if saved {
            GENERATED.with(|slot| slot.borrow_mut().take());
            return;
        }
    }
    // "Not now" quiets this one account on this site for the session. Another account, or the same site after
    // the prompt was only dismissed, is still offered: a single click-away used to silence the whole site, so a
    // later sign-in was never offered.
    let declined_key = format!("login:{origin}\u{1f}{}", &*username);
    if DECLINED.with(|sites| sites.borrow().contains(&declined_key)) || PROMPT_OPEN.with(|open| open.replace(true)) {
        return;
    }
    let guard = PromptGuard;
    let Some(anchor) = toolbar_anchor(hwnd) else { return };
    let update = matches!(matched, LoginMatch::Changed(_));
    let strength = passwords::strength(&password);
    let mut entries = vec![
        Entry::SectionHeader(passwords::save_prompt_header(update, &origin, &username)),
        Entry::Item(Item::new(2, if update { "Update password" } else { "Save password" }, Icon::Key).glyph("key")),
        Entry::Item(Item::new(1, "Not now", Icon::Close).glyph("close")),
        Entry::Item(Item::new(3, "Never for this site", Icon::Shield).glyph("block")),
    ];
    if strength.is_weak() {
        entries.push(Entry::Separator);
        entries.push(Entry::Item(Item::new(4, "This password is weak. Change it, then use a strong one", Icon::Warning).glyph("warning").disabled()));
    }
    let footer = if update { "Saving replaces the old password for this username" } else { "Encrypted for your Windows account on this PC" };
    let menu = Menu { header: None, entries, footer: footer.to_owned() };
    menu::show(hwnd, anchor, Placement::BelowRight, menu, "save-login", move |_app, chosen| {
        let _guard = guard;
        match chosen.map(|c| c.id) {
            Some(2) => {
                let Ok(mut vault) = PasswordStore::open_default() else { return };
                match vault.match_login(&origin, &username, &password) {
                    LoginMatch::Missing => { let _ = vault.add(&origin, &username, &password); }
                    LoginMatch::Changed(id) => { let _ = vault.update(id, &origin, &username, &password); }
                    LoginMatch::Unchanged(_) => {}
                }
            }
            Some(3) => {
                if let Ok(mut vault) = PasswordStore::open_default() {
                    let _ = vault.mark_never_for_site(&origin, true);
                }
            }
            Some(1) => {
                DECLINED.with(|sites| { sites.borrow_mut().insert(declined_key.clone()); });
            }
            _ => {}
        }
    });
}

fn offer_save_form(hwnd: HWND, message: LoginMessage) {
    let Ok(wallet) = Wallet::open_default() else { return };
    let values: Vec<(FieldKind, String)> = message.values.iter().map(|(k, v)| (*k, v.to_string())).collect();
    let card = Card::from_form(&values).filter(|card| wallet.prefs.cards && !wallet.cards().iter().any(|c| c.number() == card.number()));
    let address = Some(Address::from_form(&values))
        .filter(|a| wallet.prefs.addresses && a.worth_saving() && !wallet.addresses().iter().any(|saved| saved.same_as(a)));
    if card.is_none() && address.is_none() {
        return;
    }
    let key = format!("form:{}", message.origin);
    if DECLINED.with(|sites| sites.borrow().contains(&key)) || PROMPT_OPEN.with(|open| open.replace(true)) {
        return;
    }
    let guard = PromptGuard;
    let Some(anchor) = toolbar_anchor(hwnd) else { return };
    let mut entries = Vec::new();
    match (&card, &address) {
        (Some(card), _) => {
            entries.push(Entry::SectionHeader(format!("Save {} for faster checkout?", card.summary())));
        }
        (None, Some(address)) => {
            entries.push(Entry::SectionHeader(format!("Save address \u{201c}{}\u{201d}?", truncate(&address.summary(), 40))));
        }
        (None, None) => return,
    }
    if card.is_some() {
        entries.push(Entry::Item(Item::new(1, "Save card", Icon::Lock).glyph("card")));
    }
    if address.is_some() {
        entries.push(Entry::Item(Item::new(2, if card.is_some() { "Save card and address" } else { "Save address" }, Icon::Person).glyph("person")));
    }
    entries.push(Entry::Item(Item::new(3, "Not now", Icon::Close).glyph("close")));
    let footer = if card.is_some() { "The security code is never saved" } else { "Saved addresses fill checkout and sign-up forms" };
    let menu = Menu { header: None, entries, footer: footer.to_owned() };
    menu::show(hwnd, anchor, Placement::BelowRight, menu, "save-form", move |_app, chosen| {
        let _guard = guard;
        let Some(choice) = chosen.map(|c| c.id) else {
            DECLINED.with(|sites| { sites.borrow_mut().insert(key); });
            return;
        };
        let Ok(mut wallet) = Wallet::open_default() else { return };
        if matches!(choice, 1 | 2) {
            if let Some(card) = card {
                let _ = wallet.save_card(card);
            }
        }
        if choice == 2 {
            if let Some(address) = address {
                let _ = wallet.save_address(address);
            }
        }
        if choice == 3 {
            DECLINED.with(|sites| { sites.borrow_mut().insert(key); });
        }
    });
}

fn truncate(text: &str, chars: usize) -> String {
    let mut cut: String = text.chars().take(chars).collect();
    if cut.len() < text.len() {
        cut.push('\u{2026}');
    }
    cut
}

/// Under the right end of the toolbar, where the save prompts appear.
fn toolbar_anchor(hwnd: HWND) -> Option<POINT> {
    let mut window = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut window) }.ok()?;
    Some(POINT { x: window.right - 24, y: window.top + 72 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_passwords_are_remembered_per_site_only() {
        GENERATED.with(|slot| *slot.borrow_mut() = Some(("https://a.example".into(), Zeroizing::new("Gen#1".into()), Instant::now())));
        assert!(generated_for("https://a.example", "Gen#1"));
        assert!(!generated_for("https://b.example", "Gen#1"));
        assert!(!generated_for("https://a.example", "other"));
        assert_eq!(truncate("abcdef", 3), "abc\u{2026}");
        assert_eq!(truncate("abc", 3), "abc");
    }
}
