//! Page tools, the window's side (thread T7): the registry commands for page capture and for notes made
//! from the page, the clipboard bitmap, and the "Insert note" item of a text field's context menu.
//!
//! The registry (`crate::commands::registry`) points at these through `Handler::Run`. The work itself
//! is in `crate::app` (`capture_page`, `read_selection`, `insert_text`) and `crate::notes_book`; this
//! file shows the toasts and puts the result where the user asked.

use std::{rc::Rc, time::{SystemTime, UNIX_EPOCH}};

use windows::Win32::Foundation::HWND;

use super::notes_panel;
use crate::{
    app::{App, AppRef, CaptureResult, Selected},
    commands::registry::Context,
    notes_book,
    page_tools::{CaptureMode, CaptureTarget},
    profile::Profile,
    tabs::TabId,
};

const TITLE: &str = "Page tools";
/// Most notes the "Insert note" and "Append to a note" menus list.
const MENU_NOTES: usize = 15;

fn now() -> (u64, u64) {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    (since.as_secs(), since.as_micros() as u64)
}

fn clip_label(title: &str) -> String {
    let one: String = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > 40 { format!("{}…", one.chars().take(40).collect::<String>()) } else { one }
}

/// The notes a text field's context menu may insert, as (id, label), in the book's order.
pub(crate) fn context_note_items() -> Vec<(u64, String)> {
    let Ok(profile) = crate::profile::active() else { return Vec::new() };
    let book = notes_panel::html_book(&profile);
    book.ordered(book.sort).into_iter().filter(|n| !n.body.trim().is_empty()).take(MENU_NOTES).map(|n| (n.id, clip_label(&n.title))).collect()
}

/// A note was picked in a text field's context menu: its text goes where the cursor is.
pub(crate) fn insert_note_into_page(app: &AppRef, tab: TabId, id: u64) {
    let Ok(profile) = crate::profile::active() else { return };
    let Some(note) = notes_panel::html_notes(&profile).into_iter().find(|n| n.id == id) else { return };
    App::insert_text(app, tab, &note.body);
}

/// Adds a note to the profile's notes. Returns the id.
fn add_note(profile: &Profile, title: &str, body: &str, addr: &str) -> Result<u64, String> {
    let mut book = notes_panel::html_book(profile);
    let (secs, micros) = now();
    let id = book.add(title, body, addr, 0, secs, micros).map_err(|refused| refused.to_string())?;
    notes_panel::save_html_book(profile, &book).map_err(|error| format!("Notes could not be saved: {error}"))?;
    Ok(id)
}

fn notify(hwnd: HWND, message: &str) {
    super::window::show_toast(hwnd, message, TITLE);
}

/// "Add selection to a new note": the selected text becomes a note, with the page's address.
pub(crate) fn run_note_from_selection(cx: &Context) {
    let (hwnd, tab) = (cx.hwnd, App::active_tab_id(cx.app));
    App::read_selection(
        cx.app,
        tab,
        Box::new(move |selected| match selected {
            Selected::Text { text, title, addr } => {
                let Ok(profile) = crate::profile::active() else { return };
                let (note_title, body) = notes_book::from_selection(&text, &title);
                match add_note(&profile, &note_title, &body, &addr) {
                    Ok(_) => {
                        notify(hwnd, "Added to a new note");
                    }
                    Err(message) => notify(hwnd, &message),
                }
            }
            Selected::Nothing => notify(hwnd, "Select some text on the page first"),
            Selected::Unavailable => notify(hwnd, "Notes can take text from web pages"),
        }),
    );
}

/// "Add selection to an existing note": a list of notes, and the pick gets the text added to its end.
pub(crate) fn run_note_append(cx: &Context) {
    let (hwnd, tab) = (cx.hwnd, App::active_tab_id(cx.app));
    App::read_selection(
        cx.app,
        tab,
        Box::new(move |selected| {
            let text = match selected {
                Selected::Text { text, .. } => text,
                Selected::Nothing => return notify(hwnd, "Select some text on the page first"),
                Selected::Unavailable => return notify(hwnd, "Notes can take text from web pages"),
            };
            let Ok(profile) = crate::profile::active() else { return };
            let book = notes_panel::html_book(&profile);
            let items: Vec<(u64, String)> = book.ordered(book.sort).into_iter().take(MENU_NOTES).map(|n| (n.id, clip_label(&n.title))).collect();
            if items.is_empty() {
                return notify(hwnd, "There are no notes yet. Add the selection to a new note");
            }
            super::window::show_note_pick_menu(
                hwnd,
                items,
                Box::new(move |id| {
                    let Ok(profile) = crate::profile::active() else { return };
                    let mut book = notes_panel::html_book(&profile);
                    let result = book.append(id, &notes_book::clip_chars(&text, notes_book::MAX_BODY_CHARS)).map_err(|refused| refused.to_string());
                    match result.and_then(|()| notes_panel::save_html_book(&profile, &book).map_err(|e| format!("Notes could not be saved: {e}"))) {
                        Ok(()) => {
                            notify(hwnd, "Added to the note");
                            }
                        Err(message) => notify(hwnd, &message),
                    }
                }),
            );
        }),
    );
}

// ---- capture ----

fn capture(cx: &Context, mode: CaptureMode, target: CaptureTarget) {
    let hwnd = cx.hwnd;
    let app = cx.app.clone();
    let tab = App::active_tab_id(cx.app);
    App::capture_page(
        cx.app,
        tab,
        mode,
        target,
        Rc::new(move |result| deliver(hwnd, &app, mode, result)),
    );
}

fn deliver(hwnd: HWND, _app: &AppRef, mode: CaptureMode, result: CaptureResult) {
    let cut = |cut: bool| if cut { " The page was longer than a capture covers, so the rest is left out." } else { "" };
    match result {
        CaptureResult::Saved { path, cut: c } => notify(hwnd, &format!("Saved {} as {}.{}", mode.label(), path.file_name().and_then(|n| n.to_str()).unwrap_or("a picture"), cut(c))),
        CaptureResult::Clipboard { dib, cut: c } => {
            if copy_bitmap(hwnd, &dib) {
                notify(hwnd, &format!("Copied the {}.{}", mode.label(), cut(c)));
            } else {
                notify(hwnd, "The clipboard is busy. Try again");
            }
        }
        CaptureResult::Note { path, page_title, addr, cut: c } => {
            let Ok(profile) = crate::profile::active() else { return };
            let file_url = url::Url::from_file_path(&path).map(|u| u.to_string()).unwrap_or_default();
            let (title, body) = notes_book::from_capture(&page_title, &file_url);
            match add_note(&profile, &title, &body, &addr) {
                Ok(_) => {
                    notify(hwnd, &format!("Saved the {} and added a note.{}", mode.label(), cut(c)));
                }
                Err(message) => notify(hwnd, &message),
            }
        }
        CaptureResult::NothingSelected => notify(hwnd, "Select something on the page first"),
        CaptureResult::Unavailable => notify(hwnd, "Capture works on web pages"),
        CaptureResult::Busy => notify(hwnd, "A capture is already running"),
        CaptureResult::Failed(message) => notify(hwnd, &message),
    }
}

/// Puts a bottom-up 24-bit bitmap on the clipboard as `CF_DIB`.
fn copy_bitmap(hwnd: HWND, dib: &[u8]) -> bool {
    use windows::Win32::{
        Foundation::{GlobalFree, HANDLE},
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        },
    };
    const CF_DIB: u32 = 8;
    unsafe {
        let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, dib.len()) else { return false };
        let target = GlobalLock(memory).cast::<u8>();
        if target.is_null() {
            let _ = GlobalFree(Some(memory));
            return false;
        }
        std::ptr::copy_nonoverlapping(dib.as_ptr(), target, dib.len());
        let _ = GlobalUnlock(memory);
        if OpenClipboard(Some(hwnd)).is_err() {
            let _ = GlobalFree(Some(memory));
            return false;
        }
        let _ = EmptyClipboard();
        // On success the clipboard owns the memory.
        let ok = SetClipboardData(CF_DIB, Some(HANDLE(memory.0))).is_ok();
        if !ok {
            let _ = GlobalFree(Some(memory));
        }
        let _ = CloseClipboard();
        ok
    }
}

macro_rules! capture_commands {
    ($($name:ident: $mode:ident, $target:ident;)*) => {
        $(pub(crate) fn $name(cx: &Context) { capture(cx, CaptureMode::$mode, CaptureTarget::$target); })*
    };
}

capture_commands! {
    run_capture_visible: Visible, File;
    run_capture_full: FullPage, File;
    run_capture_selection: Selection, File;
    run_capture_visible_copy: Visible, Clipboard;
    run_capture_full_copy: FullPage, Clipboard;
    run_capture_selection_copy: Selection, Clipboard;
    run_capture_visible_note: Visible, Note;
    run_capture_full_note: FullPage, Note;
    run_capture_selection_note: Selection, Note;
}
