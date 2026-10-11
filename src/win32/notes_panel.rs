//! Themed, profile-local notes and task editor.

use crate::{productivity as data, profile};
use super::{menu::{self, Entry, Icon, Item, Menu}, profile_rename, show_error_message};
use windows::Win32::Foundation::{HWND, POINT};

const NOTE_BASE: usize = 20_000;
const TASK_BASE: usize = 30_000;

pub(crate) fn open(owner: HWND) {
    let Ok(profile) = profile::active() else { return };
    if !data::profile_allows_productivity(&profile) { return; }
    let notes = data::read_note_list(&profile.notes_file());
    let tasks = data::read_tasks(&profile.tasks_file());
    let mut entries = vec![
        Entry::Item(Item::new(1, "Add note…", Icon::Plus)),
        Entry::Item(Item::new(2, "Add task…", Icon::Plus)),
        Entry::Separator,
    ];
    for (index, note) in notes.iter().enumerate() {
        let edit = NOTE_BASE + index * 2;
        entries.push(Entry::Item(Item::new(edit, format!("Note: {}", note.title), Icon::Info).submenu_with(vec![
            Entry::Item(Item::new(edit, "Edit note…", Icon::Info)),
            Entry::Item(Item::new(edit + 1, "Delete note", Icon::Close)),
        ])));
    }
    for (index, task) in tasks.iter().enumerate() {
        let base = TASK_BASE + index * 3;
        let status = if task.complete { "✓" } else { "○" };
        entries.push(Entry::Item(Item::new(base, format!("{status} {}", task.text), Icon::Check).submenu_with(vec![
            Entry::Item(Item::new(base, if task.complete { "Reopen task" } else { "Complete task" }, Icon::Check)),
            Entry::Item(Item::new(base + 1, "Edit task…", Icon::Info)),
            Entry::Item(Item::new(base + 2, "Delete task", Icon::Close)),
        ])));
    }
    if notes.is_empty() && tasks.is_empty() {
        entries.push(Entry::Item(Item::new(3, "No notes or tasks yet", Icon::Info).disabled()));
    }
    let mut point = POINT::default();
    unsafe { let _ = windows::Win32::Graphics::Gdi::ClientToScreen(owner, &mut point); }
    let Some(chosen) = menu::track(owner, point, Menu { header: None, entries, footer: "Notes & Tasks".into() }) else { return; };
    let result = match chosen.id {
        1 => {
            let Some(title) = profile_rename::prompt(owner, "") else { return; };
            let Some(body) = profile_rename::prompt(owner, "") else { return; };
            data::add_note(&profile.notes_file(), data::Note { id: next_id(), title, body })
        }
        2 => {
            let Some(text) = profile_rename::prompt(owner, "") else { return; };
            data::add_task(&profile.tasks_file(), data::Task { id: next_id(), complete: false, text })
        }
        id if (NOTE_BASE..TASK_BASE).contains(&id) => {
            let index = (id - NOTE_BASE) / 2;
            let Some(note) = notes.get(index) else { return; };
            if id % 2 == 1 {
                if !super::ask_yes_no("Delete this note?") { return; }
                data::delete_note(&profile.notes_file(), note.id).map(|_| ())
            } else {
                let Some(title) = profile_rename::prompt(owner, &note.title) else { return; };
                let Some(body) = profile_rename::prompt(owner, &note.body) else { return; };
                data::edit_note(&profile.notes_file(), note.id, title, body).map(|_| ())
            }
        }
        id if id >= TASK_BASE => {
            let index = (id - TASK_BASE) / 3;
            let Some(task) = tasks.get(index) else { return; };
            match (id - TASK_BASE) % 3 {
                0 => data::complete_task(&profile.tasks_file(), task.id, !task.complete).map(|_| ()),
                1 => {
                    let Some(text) = profile_rename::prompt(owner, &task.text) else { return; };
                    data::edit_task(&profile.tasks_file(), task.id, text).map(|_| ())
                }
                _ => {
                    if !super::ask_yes_no("Delete this task?") { return; }
                    data::delete_task(&profile.tasks_file(), task.id).map(|_| ())
                }
            }
        }
        _ => return,
    };
    if let Err(error) = result { show_error_message(&format!("Notes & Tasks could not be saved: {error}")); }
}

fn next_id() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_micros() as u64
}
