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
        "No Chrome, Edge, Brave or Firefox profiles were detected.".to_owned()
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

fn import_bookmark_file(owner: HWND) -> windows::core::Result<()> {
    show_stage(owner, "Step 1 of 5 - Detecting source data");
    let Some(path) = pick_file(owner) else {
        return Ok(());
    };
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
    let existing = std::fs::read_to_string(active.bookmarks_file()).unwrap_or_default();
    let existing = import::parse_bookmark_store(&existing);
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
        Ok(summary) => format!(
            "Imported {} · duplicates {} · skipped {} · failed {}",
            summary.imported,
            summary.duplicates + duplicates,
            summary.skipped + skipped,
            summary.failed
        ),
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
