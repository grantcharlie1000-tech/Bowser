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
    profile,
};

const PICK: usize = 1;
const CONFIRM: usize = 2;
const CANCEL: usize = 3;
const PASSWORDS: usize = 4;

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
                Entry::Separator,
                Entry::Item(Item::new(CANCEL, "Close", Icon::Close)),
            ],
            footer: format!(
                "{browser_summary} History databases and external search settings are skipped. Browser passwords must be exported by the browser to CSV; Bowser cannot decrypt Chrome or Edge password stores."
            ),
        },
    );
    match chosen.map(|item| item.id) {
        Some(PICK) => import_bookmark_file(owner),
        Some(PASSWORDS) => passwords_panel::open(owner),
        _ => Ok(()),
    }
}

fn import_bookmark_file(owner: HWND) -> windows::core::Result<()> {
    let Some(path) = pick_file(owner) else {
        return Ok(());
    };
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
    let active = profile::active().map_err(|_| {
        windows::core::Error::new(
            windows::Win32::Foundation::E_FAIL,
            "Bowser could not find the active profile.",
        )
    })?;
    let existing = std::fs::read_to_string(active.bookmarks_file()).unwrap_or_default();
    let existing = import::parse_bookmark_store(&existing);
    let (new, duplicates) = import::dedupe_bookmarks(bookmarks, &existing);
    if new.is_empty() {
        let _ = menu::track(
            owner,
            anchor(owner),
            Menu {
                header: None,
                entries: vec![Entry::Item(Item::new(CANCEL, "Close", Icon::Close))],
                footer: format!(
                    "Nothing new to import. {duplicates} duplicate(s), {skipped} invalid URL(s) skipped."
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
                    format!("Import {} bookmarks", new.len()),
                    Icon::Check,
                )),
                Entry::Item(Item::new(CANCEL, "Cancel", Icon::Close)),
            ],
            footer: format!(
                "{} duplicate(s) and {skipped} invalid URL(s) will be skipped. Only HTTP and HTTPS URLs are accepted. Bowser reads the selected file but never changes it.",
                duplicates
            ),
        },
    );
    if preview.is_none_or(|item| item.id != CONFIRM) {
        return Ok(());
    }
    let summary = store::import_bookmarks(&active.bookmarks_file(), new);
    let footer = match summary {
        Ok(summary) => format!(
            "Imported {} · duplicates {} · skipped {} · failed {}",
            summary.imported,
            summary.duplicates + duplicates,
            summary.skipped + skipped,
            summary.failed
        ),
        Err(error) => format!("Import failed: {error}"),
    };
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
