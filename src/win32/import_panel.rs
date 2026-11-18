//! Consent-based bookmark import entry using Bowser's themed menu and the shared file dialog.

use std::path::PathBuf;
use windows::{
    Win32::{
        Foundation::HWND,
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
        UI::{
            Shell::{
                FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FileOpenDialog, IFileOpenDialog,
                SIGDN_FILESYSPATH,
            },
            WindowsAndMessaging::GetWindowRect,
        },
    },
    core::PCWSTR,
};

use super::{
    menu::{self, Entry, Icon, Item, Menu},
    passwords_panel,
};
use crate::{
    import::{self, store},
    navigation::SearchEngine,
    profile, settings,
};

const PICK: usize = 1;
const CONFIRM: usize = 2;
const CANCEL: usize = 3;
const PASSWORDS: usize = 4;
const FIREFOX: usize = 5;

/// Opens a themed, consent-based import flow. Browser source files are opened read-only.
pub fn open(owner: HWND) -> windows::core::Result<()> {
    let profiles = import::detect_browser_profiles();
    let browser_summary = if profiles.is_empty() {
        "No Chrome, Edge, Brave, Opera, Vivaldi, Helium or Firefox profiles were detected.".to_owned()
    } else {
        format!(
            "Detected {} browser profile(s). Select a bookmarks export to preview.",
            profiles.len()
        )
    };
    let firefox_item = Item::new(FIREFOX, "Use newest Firefox bookmark backup...", Icon::Grid);
    let firefox_item = if profiles
        .iter()
        .any(|p| p.browser == "Firefox" && import::firefox::newest_backup(&p.root).is_some())
    {
        firefox_item
    } else {
        firefox_item.disabled()
    };
    let chosen = menu::track(
        owner,
        anchor(owner),
        Menu {
            header: None,
            entries: vec![
                Entry::Item(Item::new(
                    PICK,
                    "Choose Chromium Bookmarks or Netscape HTML…",
                    Icon::Grid,
                )),
                Entry::Item(Item::new(
                    PASSWORDS,
                    "Import passwords from an official CSV…",
                    Icon::Key,
                )),
                Entry::Item(firefox_item),
                Entry::Separator,
                Entry::Item(Item::new(CANCEL, "Close", Icon::Close)),
            ],
            footer: format!(
                "{browser_summary} History import is not supported (browser history databases are SQLite). Browser passwords must be exported by the browser to CSV; Bowser cannot decrypt Chrome or Edge password stores."
            ),
        },
    );
    match chosen.map(|item| item.id) {
        Some(PICK) => import_bookmark_file(owner),
        Some(FIREFOX) => import_firefox_backup(owner),
        Some(PASSWORDS) => passwords_panel::open(owner),
        _ => Ok(()),
    }
}

/// The browsers the welcome's Import step offers (docs/design/welcome.md): the wire key, the name the
/// importer finds them under (`import::detect_browser_profiles`), in the order tried, and the name to show.
pub(crate) const BROWSERS: [(&str, &[&str], &str); 7] = [
    ("chrome", &["Chrome"], "Chrome"),
    ("edge", &["Edge"], "Edge"),
    ("brave", &["Brave"], "Brave"),
    ("firefox", &["Firefox"], "Firefox"),
    ("opera", &["Opera", "Opera GX"], "Opera"),
    ("vivaldi", &["Vivaldi"], "Vivaldi"),
    ("helium", &["Helium"], "Helium"),
];

/// One click on the welcome's Import step: imports `browser`'s bookmarks (and, for Chromium
/// browsers, its search setting) straight from the profile this PC has, with no native menu or
/// window: picking the browser is the consent, and the welcome says the outcome in its own status
/// line. The Default profile goes first. `Ok` and `Err` carry that sentence.
pub(crate) fn import_from(owner: HWND, browser: &str) -> Result<String, String> {
    let Some(&(_, found_as, name)) = BROWSERS.iter().find(|(key, _, _)| *key == browser) else {
        return Err("That browser is not one Bowser imports from.".into());
    };
    // What the welcome's import found, for --diagnostic-log (no paths or names: only the outcome).
    let note = |event: &str| {
        if let Some(app) = super::window::app_of(owner) && let Ok(state) = app.try_borrow() {
            let _ = state.log.record(event);
        }
    };
    let nothing = || format!("No {name} bookmarks were found on this PC.");
    if browser == "firefox" {
        // The newest automatic backup across this PC's Firefox profiles.
        let newest = import::detect_browser_profiles()
            .into_iter()
            .filter(|profile| profile.browser == "Firefox")
            .filter_map(|profile| import::firefox::newest_backup(&profile.root))
            .max_by_key(|path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok());
        let Some(path) = newest else {
            note("welcome_import_none");
            return Err(nothing());
        };
        note("welcome_import_found");
        let (bookmarks, skipped) = import::firefox::read_backup(&path)?;
        return write_bookmarks(bookmarks, skipped, None, false, name);
    }
    let mut profiles: Vec<_> = import::detect_browser_profiles()
        .into_iter()
        .filter_map(|profile| {
            let rank = found_as.iter().position(|as_name| *as_name == profile.browser)?;
            profile.bookmarks.map(|path| (rank, profile.name, path))
        })
        .collect();
    // The first name tried (Opera before Opera GX), then each one's Default profile.
    profiles.sort_by_key(|(rank, profile, _)| (*rank, profile != "Default"));
    let Some((_, _, path)) = profiles.into_iter().next() else {
        note("welcome_import_none");
        return Err(nothing());
    };
    note("welcome_import_found");
    import_bookmark_path_quietly(&path, name)
}

/// The File tile (and Settings' Import button): Windows' own file dialog for a bookmarks file
/// (Chromium `Bookmarks` or Netscape HTML, as every browser exports) or a password CSV export,
/// imported with no native menu or window. `Ok(None)` when the dialog was cancelled; otherwise
/// the sentence the page shows.
pub(crate) fn import_file(owner: HWND) -> Result<Option<String>, String> {
    let Some(path) = crate::download_shell::open_dialog_filters(
        owner,
        "Choose a bookmarks file or a password export",
        &[("Bookmarks or passwords", "*.html;*.htm;Bookmarks;*.json;*.csv"), ("All files", "*.*")],
    ) else {
        return Ok(None);
    };
    let is_csv = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("csv"));
    if is_csv { import_passwords_quietly(&path) } else { import_bookmark_path_quietly(&path, "that file") }.map(Some)
}

fn import_bookmark_path_quietly(path: &std::path::Path, from: &str) -> Result<String, String> {
    let (bookmarks, skipped) = store::read_bookmark_file(path)?;
    let (search_engine, unsupported_setting) = path
        .parent()
        .map(|parent| parent.join("Preferences"))
        .and_then(|preferences| std::fs::read_to_string(preferences).ok())
        .map_or((None, false), |text| import::chromium::parse_search_preference(&text));
    write_bookmarks(bookmarks, skipped, search_engine, unsupported_setting, from)
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// De-duplicates against the active profile, writes what is new, applies a supported search
/// setting, and says what happened in one or two short sentences.
fn write_bookmarks(
    bookmarks: Vec<import::Bookmark>,
    skipped: usize,
    search_engine: Option<SearchEngine>,
    unsupported_setting: bool,
    from: &str,
) -> Result<String, String> {
    let active = profile::active().map_err(|_| "Bowser could not find the active profile.".to_owned())?;
    let existing = store::existing_bookmarks(&active.bookmarks_file());
    let found = bookmarks.len();
    let (new, duplicates) = import::dedupe_bookmarks(bookmarks, &existing);
    let mut text = if new.is_empty() {
        if found == 0 {
            format!("No bookmarks were found in {from}.")
        } else if duplicates > 0 {
            format!("Your bookmarks from {from} are already in Bowser.")
        } else {
            format!("None of the bookmarks in {from} could be imported.")
        }
    } else {
        let summary = store::import_bookmarks(&active.bookmarks_file(), new)
            .map_err(|error| format!("Bowser could not save the bookmarks: {error}"))?;
        if summary.imported > 0 {
            // The bar, panel and suggestions re-read the tree.
            super::window::post_to_every_window(super::WM_RAIL_CHANGED);
        }
        let mut text = format!("Imported {} from {from}.", plural(summary.imported, "bookmark", "bookmarks"));
        if summary.failed > 0 {
            text.push_str(&format!(" {} could not be added.", summary.failed));
        }
        text
    };
    let _ = (skipped, unsupported_setting); // Unsafe or unsupported entries are left out quietly.
    if let Some(engine) = search_engine {
        let mut updated = settings::Settings::load(&active.settings_file());
        updated.search_engine = Some(engine);
        if settings::save(&active.settings_file(), &updated).is_ok() {
            text.push_str(&format!(" Search now uses {}.", engine.label()));
        }
    }
    Ok(text)
}

/// A password CSV export (from a browser or password manager): previewed and committed in one go,
/// since choosing the file is the consent. Only counts and fixed sentences leave here.
fn import_passwords_quietly(path: &std::path::Path) -> Result<String, String> {
    use super::passwords_import::{ImportError, commit_plan, preview_file};
    let previewed = preview_file(path).map_err(|error| match error {
        ImportError::VaultUnavailable => "Saved passwords could not be opened for this profile.".to_owned(),
        ImportError::Unreadable => "That file is not a password export Bowser can read. Export a CSV from your password manager or browser, then choose it here.".to_owned(),
    })?;
    let counts = previewed.counts;
    if counts.imported == 0 {
        return Ok(if counts.duplicates + counts.conflicts > 0 {
            "These passwords are already saved in Bowser.".to_owned()
        } else {
            "No passwords were found in that file.".to_owned()
        });
    }
    let outcome = commit_plan(previewed.plan).map_err(|_| "Saved passwords could not be written. Nothing was imported.".to_owned())?;
    let mut text = format!("Imported {}.", plural(outcome.succeeded, "password", "passwords"));
    if outcome.failed > 0 {
        text.push_str(&format!(" {} could not be read.", outcome.failed));
    }
    Ok(text)
}

fn import_bookmark_file(owner: HWND) -> windows::core::Result<()> {
    show_stage(owner, "Step 1 of 5 - Detecting source data");
    let Some(path) = pick_file(owner) else {
        return Ok(());
    };
    import_bookmark_path(owner, path)
}

fn import_bookmark_path(owner: HWND, path: PathBuf) -> windows::core::Result<()> {
    show_stage(owner, "Step 2 of 5 - Reading bookmarks");
    let (bookmarks, skipped) = match store::read_bookmark_file(&path) {
        Ok(bookmarks) => bookmarks,
        Err(message) => {
            let _ = menu::track(
                owner,
                anchor(owner),
                Menu {
                    header: None,
                    entries: vec![Entry::Item(Item::new(CANCEL, "Close", Icon::Close))],
                    footer: message,
                },
            );
            return Ok(());
        }
    };
    show_stage(owner, "Step 3 of 5 - Validating URLs");
    let preferences = path.parent().map(|parent| parent.join("Preferences"));
    let (search_engine, unsupported_setting) = preferences
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map_or((None, false), |text| {
            import::chromium::parse_search_preference(&text)
        });
    preview_and_import(
        owner,
        bookmarks,
        skipped,
        search_engine,
        unsupported_setting,
    )
}

fn import_firefox_backup(owner: HWND) -> windows::core::Result<()> {
    show_stage(owner, "Step 1 of 5 - Detecting Firefox profiles");
    let choices = import::detect_browser_profiles()
        .into_iter()
        .filter(|profile| profile.browser == "Firefox")
        .filter_map(|profile| {
            import::firefox::newest_backup(&profile.root).map(|path| (profile.name, path))
        })
        .collect::<Vec<_>>();
    let entries = choices
        .iter()
        .enumerate()
        .map(|(id, (name, _))| Entry::Item(Item::new(id, name, Icon::Grid)))
        .collect();
    let Some(choice) = menu::track(owner, anchor(owner), Menu {
        header: None, entries,
        footer: "Uses the newest automatic bookmark backup for this Firefox profile. The source file remains read-only.".into(),
    }) else { return Ok(()) };
    let Some((_, path)) = choices.get(choice.id) else {
        return Ok(());
    };
    show_stage(owner, "Step 2 of 5 - Reading Firefox bookmark backup");
    match import::firefox::read_backup(path) {
        Ok((bookmarks, skipped)) => {
            show_stage(owner, "Step 3 of 5 - Validating URLs");
            preview_and_import(owner, bookmarks, skipped, None, false)
        }
        Err(message) => {
            show_message(owner, &message);
            Ok(())
        }
    }
}

fn preview_and_import(
    owner: HWND,
    bookmarks: Vec<import::Bookmark>,
    skipped: usize,
    search_engine: Option<SearchEngine>,
    unsupported_setting: bool,
) -> windows::core::Result<()> {
    let active = profile::active().map_err(|_| {
        windows::core::Error::new(
            windows::Win32::Foundation::E_FAIL,
            "Bowser could not find the active profile.",
        )
    })?;
    let existing = store::existing_bookmarks(&active.bookmarks_file());
    let (new, duplicates) = import::dedupe_bookmarks(bookmarks, &existing);
    show_stage(owner, "Step 4 of 5 · De-duplicating bookmarks");
    if new.is_empty() && search_engine.is_none() && !unsupported_setting {
        let _ = menu::track(
            owner,
            anchor(owner),
            Menu {
                header: None,
                entries: vec![Entry::Item(Item::new(CANCEL, "Close", Icon::Close))],
                footer: format!(
                    "Nothing new to import. {duplicates} duplicate(s), {skipped} unsafe or unsupported URL(s) skipped."
                ),
            },
        );
        return Ok(());
    }
    let preview = menu::track(
        owner,
        anchor(owner),
        Menu {
            header: None,
            entries: vec![
                Entry::Item(Item::new(
                    CONFIRM,
                    if new.is_empty() {
                        format!(
                            "Apply {} search setting",
                            search_engine.map_or("supported", |engine| engine.label())
                        )
                    } else {
                        format!("Import {} bookmarks", new.len())
                    },
                    Icon::Check,
                )),
                Entry::Item(Item::new(CANCEL, "Cancel", Icon::Close)),
            ],
            footer: format!(
                "{} duplicate(s) and {skipped} unsafe or unsupported URL(s) will be skipped. {} History import is not supported (browser history databases are SQLite). Source data stays read-only.",
                duplicates,
                search_engine.map_or_else(
                    || if unsupported_setting {
                        "Unsupported browser search settings will be skipped.".to_owned()
                    } else {
                        "No supported search setting found.".to_owned()
                    },
                    |engine| format!("Search engine preview: {}.", engine.label())
                )
            ),
        },
    );
    if preview.is_none_or(|item| item.id != CONFIRM) {
        return Ok(());
    }
    show_stage(owner, "Step 5 of 5 · Writing to the Bowser profile");
    let mut footer = match store::import_bookmarks(&active.bookmarks_file(), new) {
        Ok(summary) => {
            if summary.imported > 0 {
                // The bar, panel and suggestions re-read the tree.
                super::window::post_to_every_window(super::WM_RAIL_CHANGED);
            }
            let mut text = format!(
                "Imported {} · duplicates {} · skipped {} · failed {}",
                summary.imported,
                summary.duplicates + duplicates,
                summary.skipped + skipped,
                summary.failed
            );
            for reason in &summary.reasons {
                text.push(' ');
                text.push_str(reason);
            }
            text
        }
        Err(error) => {
            format!(
                "Imported 0 · duplicates {duplicates} · skipped {skipped} · failed 1. Bowser profile write failed (the profile may be locked): {error}"
            )
        }
    };
    if let Some(engine) = search_engine {
        let mut updated = settings::Settings::load(&active.settings_file());
        updated.search_engine = Some(engine);
        if settings::save(&active.settings_file(), &updated).is_ok() {
            footer.push_str(&format!(" Search engine set to {}.", engine.label()));
        } else {
            footer.push_str(" Failed 1 settings write: the search setting could not be saved; bookmarks may have been imported.");
        }
    } else if unsupported_setting {
        footer.push_str(" Unsupported browser search settings were skipped.");
    }
    let _ = menu::track(
        owner,
        anchor(owner),
        Menu {
            header: None,
            entries: vec![Entry::Item(Item::new(CANCEL, "Close", Icon::Close))],
            footer,
        },
    );
    Ok(())
}

fn show_stage(owner: HWND, text: &str) {
    let _ = menu::track(
        owner,
        anchor(owner),
        Menu {
            header: None,
            entries: vec![Entry::Item(Item::new(CANCEL, "Continue", Icon::Check))],
            footer: text.to_owned(),
        },
    );
}

fn show_message(owner: HWND, text: &str) {
    let _ = menu::track(
        owner,
        anchor(owner),
        Menu {
            header: None,
            entries: vec![Entry::Item(Item::new(CANCEL, "Close", Icon::Close))],
            footer: text.to_owned(),
        },
    );
}

fn pick_file(owner: HWND) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let title: Vec<u16> = "Choose a Chromium Bookmarks file or Netscape bookmarks HTML"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        dialog
            .SetOptions(FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM)
            .ok()?;
        dialog.SetTitle(PCWSTR(title.as_ptr())).ok()?;
        dialog.Show(Some(owner)).ok()?;
        let item = dialog.GetResult().ok()?;
        let display = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = display.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(display.0.cast_const().cast()));
        path
    }
}

fn anchor(owner: HWND) -> windows::Win32::Foundation::POINT {
    let mut rect = windows::Win32::Foundation::RECT::default();
    unsafe {
        let _ = GetWindowRect(owner, &mut rect);
    }
    windows::Win32::Foundation::POINT {
        x: rect.right - 24,
        y: rect.top + 72,
    }
}

#[cfg(test)]
mod tests {
    use super::BROWSERS;

    #[test]
    fn the_welcome_browsers_are_the_keys_the_settings_wire_accepts_in_its_order() {
        let keys: Vec<&str> = BROWSERS.iter().map(|(key, _, _)| *key).collect();
        assert_eq!(keys, crate::chrome_host::settings::IMPORT_BROWSERS);
        assert!(BROWSERS.iter().all(|(_, found_as, name)| !found_as.is_empty() && !name.is_empty()));
    }
}
