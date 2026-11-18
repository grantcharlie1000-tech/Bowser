//! Runs the settings page's "Passwords and autofill" commands (`chrome_host::passwords_page`) and
//! builds its snapshots. Every command reopens the vault or wallet, so another window's changes
//! are never overwritten with a stale copy.
//!
//! Secrets reach the page only on request and only once: a revealed password, or a generated one,
//! is put in the next snapshot and then dropped (zeroized) here.

use std::{cell::RefCell, collections::HashMap};
use windows::Win32::Foundation::HWND;
use zeroize::Zeroizing;

use crate::{
    autofill::{Prefs, Wallet},
    chrome_host::passwords_page::{self as wire, Ack, Action, ImportView},
    passwords::{self, GeneratorOptions, ImportPlan, PasswordError, PasswordStore},
};

#[derive(Default)]
struct State {
    query: String,
    ack: Ack,
    error: String,
    reveal: Option<(u64, Zeroizing<String>)>,
    generated: Option<Zeroizing<String>>,
    /// The generated password goes to the page once; it stays here for "Copy".
    generated_sent: bool,
    plan: Option<ImportPlan>,
    import: Option<ImportView>,
}

thread_local! {
    static STATES: RefCell<HashMap<isize, State>> = RefCell::new(HashMap::new());
}

fn with<T>(owner: HWND, f: impl FnOnce(&mut State) -> T) -> T {
    STATES.with(|states| f(states.borrow_mut().entry(owner.0 as isize).or_default()))
}

fn fail(owner: HWND, message: impl Into<String>) {
    let message = message.into();
    with(owner, |s| {
        s.ack = Ack::Failed;
        s.error = message;
    });
}

fn ok(owner: HWND, ack: Ack) {
    with(owner, |s| {
        s.ack = ack;
        s.error.clear();
    });
}

fn message(error: PasswordError) -> String {
    match error {
        PasswordError::InsecureOrigin => "Use the site's full https:// address (http:// works only for localhost).".into(),
        PasswordError::Duplicate => "That login is already saved.".into(),
        PasswordError::Conflict => "A different password is already saved for that site and username.".into(),
        PasswordError::Disabled => "Guest windows keep no saved passwords or autofill data.".into(),
        PasswordError::InvalidStore => "Fill in the site, username and password.".into(),
        PasswordError::MissingEntry => "That entry was changed in another window. The list is up to date now.".into(),
        PasswordError::UnsupportedHeaders => "That file is not a password export Bowser can read. Export a CSV from your password manager or browser.".into(),
        PasswordError::InvalidCsv => "That CSV file could not be read. Export it again and choose the new file.".into(),
        PasswordError::TooLarge | PasswordError::TooManyRows => "That export is too large to import at once (10,000 logins at most).".into(),
        other => format!("{other}."),
    }
}

/// Runs one command for the page in `owner`'s settings tab.
pub(crate) fn run(owner: HWND, action: Action) {
    match action {
        Action::Refresh => {}
        Action::Search(query) => with(owner, |s| s.query = query),
        Action::Reveal(id) => match PasswordStore::open_default().and_then(|vault| {
            let found = vault.find_wire(id).ok_or(PasswordError::MissingEntry)?;
            vault.reveal(found)
        }) {
            Ok(password) => with(owner, |s| s.reveal = Some((id, Zeroizing::new(password.to_string())))),
            Err(error) => fail(owner, message(error)),
        },
        Action::Copy(id, password) => {
            let result = PasswordStore::open_default().and_then(|vault| {
                let found = vault.find_wire(id).ok_or(PasswordError::MissingEntry)?;
                if password {
                    Ok(Zeroizing::new(vault.reveal(found)?.to_string()))
                } else {
                    let user = vault.list().into_iter().find(|entry| entry.id == found).map(|entry| entry.username);
                    user.map(Zeroizing::new).ok_or(PasswordError::MissingEntry)
                }
            });
            match result {
                Ok(text) if password && super::passwords_panel::copy_password(owner, &text) => ok(owner, Ack::Copied),
                Ok(text) if !password && super::window::copy_text_to_clipboard(owner, &text) => ok(owner, Ack::Copied),
                Ok(_) => fail(owner, "The clipboard is busy. Try again."),
                Err(error) => fail(owner, message(error)),
            }
        }
        Action::SaveLogin(id, origin, user, pass) => {
            let origin = if origin.contains("://") { origin } else { format!("https://{origin}") };
            let result = PasswordStore::open_default().and_then(|mut vault| match id {
                None => vault.add(&origin, &user, &pass).map(|_| ()),
                Some(id) => {
                    let found = vault.find_wire(id).ok_or(PasswordError::MissingEntry)?;
                    vault.update(found, &origin, &user, &pass).map(|_| ())
                }
            });
            match result {
                Ok(()) => {
                    ok(owner, Ack::Saved);
                    with(owner, |s| s.reveal = None);
                }
                Err(error) => fail(owner, message(error)),
            }
        }
        Action::DeleteLogin(id) => match PasswordStore::open_default().and_then(|mut vault| {
            let found = vault.find_wire(id).ok_or(PasswordError::MissingEntry)?;
            vault.delete(found)
        }) {
            Ok(()) => ok(owner, Ack::Deleted),
            Err(error) => fail(owner, message(error)),
        },
        Action::Generate(options) => {
            let Some(options) = GeneratorOptions::parse(&options) else {
                return fail(owner, "Choose at least one kind of character and a length from 8 to 128.");
            };
            match passwords::generate_strong(options) {
                Ok(password) => with(owner, |s| {
                    s.generated = Some(Zeroizing::new(password.to_string()));
                    s.generated_sent = false;
                }),
                Err(error) => fail(owner, message(error)),
            }
        }
        Action::CopyGenerated => {
            let generated = with(owner, |s| s.generated.clone());
            match generated {
                Some(password) if super::passwords_panel::copy_password(owner, &password) => ok(owner, Ack::Copied),
                Some(_) => fail(owner, "The clipboard is busy. Try again."),
                None => fail(owner, "Generate a password first."),
            }
        }
        Action::Import => {
            let Some(path) = crate::download_shell::open_dialog(owner, "Choose a password export (.csv)", "Password exports", "csv") else { return };
            match super::passwords_import::preview_file(&path) {
                Ok(previewed) => {
                    let provider = previewed.plan.provider().label;
                    let counts = previewed.counts;
                    with(owner, |s| {
                        s.import = Some(ImportView::Preview {
                            provider,
                            new: counts.imported,
                            duplicates: counts.duplicates,
                            conflicts: counts.conflicts,
                            rejected: counts.errors,
                        });
                        s.plan = Some(previewed.plan);
                    });
                    ok(owner, Ack::None);
                }
                Err(super::passwords_import::ImportError::VaultUnavailable) => fail(owner, "Saved passwords could not be opened for this profile."),
                Err(super::passwords_import::ImportError::Unreadable) => {
                    fail(owner, "That file is not a password export Bowser can read. Export a CSV from your password manager or browser, then choose it here.")
                }
            }
        }
        Action::ImportConfirm => {
            let Some(plan) = with(owner, |s| s.plan.take()) else { return };
            match super::passwords_import::commit_plan(plan) {
                Ok(outcome) => {
                    with(owner, |s| s.import = Some(ImportView::Done { added: outcome.succeeded, skipped: outcome.skipped, failed: outcome.failed }));
                    ok(owner, Ack::Imported);
                }
                Err(_) => fail(owner, "Saved passwords could not be written. Nothing was imported."),
            }
        }
        Action::ImportCancel => with(owner, |s| {
            s.plan = None;
            s.import = None;
        }),
        Action::Export => {
            let dir = crate::download_shell::default_downloads_dir().unwrap_or_default();
            let Some(path) = crate::download_shell::save_dialog(owner, &dir, "Bowser Passwords.csv") else { return };
            let result = PasswordStore::open_default().and_then(|vault| {
                let csv = vault.export_csv();
                std::fs::write(&path, csv.as_bytes()).map_err(PasswordError::from)
            });
            match result {
                Ok(()) => ok(owner, Ack::Exported),
                Err(error) => fail(owner, message(error)),
            }
        }
        Action::ForgetNever(origin) => match PasswordStore::open_default().and_then(|mut vault| vault.mark_never_for_site(&origin, false)) {
            Ok(()) => ok(owner, Ack::Saved),
            Err(error) => fail(owner, message(error)),
        },
        Action::Pref(key, on) => match Wallet::open_default().and_then(|mut wallet| {
            let mut prefs = wallet.prefs;
            prefs.set(&key, on);
            wallet.set_prefs(prefs)
        }) {
            Ok(()) => ok(owner, Ack::Saved),
            Err(error) => fail(owner, message(error)),
        },
        Action::SaveAddress(id, text) => {
            let Some(address) = wire::parse_address(id.unwrap_or(0), &text) else {
                return fail(owner, "Fill in at least one field of the address.");
            };
            match Wallet::open_default().and_then(|mut wallet| wallet.save_address(address)) {
                Ok(_) => ok(owner, Ack::Saved),
                Err(error) => fail(owner, message(error)),
            }
        }
        Action::DeleteAddress(id) => match Wallet::open_default().and_then(|mut wallet| wallet.delete_address(id)) {
            Ok(()) => ok(owner, Ack::Deleted),
            Err(error) => fail(owner, message(error)),
        },
        Action::SaveCard(id, text) => {
            let result = Wallet::open_default().and_then(|mut wallet| {
                let parts = wire::parse_card(&text).ok_or(PasswordError::InvalidStore)?;
                let saved = id.and_then(|id| wallet.card(id)).cloned();
                let mut card = wire::card_from(parts, saved.as_ref()).ok_or(PasswordError::InvalidGenerator)?;
                card.id = id.unwrap_or(0);
                wallet.save_card(card)
            });
            match result {
                Ok(_) => ok(owner, Ack::Saved),
                Err(PasswordError::InvalidGenerator | PasswordError::InvalidStore) => {
                    fail(owner, "Check the card number and expiry date.")
                }
                Err(PasswordError::Duplicate) => fail(owner, "That card is already saved."),
                Err(error) => fail(owner, message(error)),
            }
        }
        Action::DeleteCard(id) => match Wallet::open_default().and_then(|mut wallet| wallet.delete_card(id)) {
            Ok(()) => ok(owner, Ack::Deleted),
            Err(error) => fail(owner, message(error)),
        },
    }
}

/// The section's snapshot, read now. One-shot fields (an answer, a revealed or generated
/// password) are included once and then cleared.
pub(crate) fn snapshot(owner: HWND, guest: bool) -> String {
    let vault = PasswordStore::open_default();
    let wallet = Wallet::open_default();
    let available = vault.is_ok();
    let (query, ack, error, reveal, generated, import) = with(owner, |s| {
        let generated = (!s.generated_sent).then(|| s.generated.clone()).flatten();
        s.generated_sent = true;
        (s.query.clone(), std::mem::take(&mut s.ack), std::mem::take(&mut s.error), s.reveal.take(), generated, s.import)
    });
    let (logins, total, never) = match &vault {
        Ok(vault) => (vault.search(&query), vault.len(), vault.never_sites().to_vec()),
        Err(_) => (Vec::new(), 0, Vec::new()),
    };
    let (prefs, addresses, cards) = match &wallet {
        Ok(wallet) => (wallet.prefs, wallet.addresses().to_vec(), wallet.cards().to_vec()),
        Err(_) => (Prefs::default(), Vec::new(), Vec::new()),
    };
    // A finished import is reported once.
    if matches!(import, Some(ImportView::Done { .. })) {
        with(owner, |s| s.import = None);
    }
    wire::snapshot_json(&wire::View {
        available,
        guest,
        prefs,
        logins: &logins,
        total,
        query: &query,
        never: &never,
        addresses: &addresses,
        cards: &cards,
        import,
        ack,
        error: &error,
        reveal: reveal.as_ref().map(|(id, value)| (*id, value.as_str())),
        generated: generated.as_ref().map(|value| value.as_str()),
    })
}

/// Forgets the page's state when its window closes.
pub(crate) fn forget(owner: HWND) {
    STATES.with(|states| {
        states.borrow_mut().remove(&(owner.0 as isize));
    });
}
