//! Registry commands that need a window: running a command chain, the keyboard cheat sheet, the
//! per-site "prioritise website shortcuts" switch and the shortcut to Keyboard settings.
//!
//! The registry (`crate::commands::registry`) points at these through `Handler::Run`; `run_registered`
//! in `window.rs` is the single place that runs any command by id.
//!
//! A chain runs on a helper thread that only waits and posts `WM_CHAIN_STEP`; every command itself
//! runs on the window thread, as every other command does. Nothing waits on the UI thread.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::PostMessageW,
};

use super::WM_CHAIN_STEP;
use crate::{
    app::{App, AppRef},
    commands::{
        self,
        chains::{self, Chain, Planned},
        registry::Context,
        store::{Store, normalise_site},
    },
    internal_page::Section,
};

thread_local! {
    /// Running chains by run id, each with its plan. A run leaves the table with its last step.
    static RUNS: RefCell<HashMap<u32, Vec<Planned>>> = RefCell::new(HashMap::new());
    static NEXT_RUN: Cell<u32> = const { Cell::new(1) };
}

/// Starts the saved chain `number` for the window `hwnd`.
pub(crate) fn start_chain(hwnd: HWND, store: &Store, number: u32) {
    if let Some(chain) = store.chains.iter().find(|chain| chain.id == number) {
        start(hwnd, chain);
    }
}

/// Runs `chain` now: its first command after its own delay, each next one after its. Also used by
/// the editor's Test Chain button with a chain that is not saved yet.
pub(crate) fn start(hwnd: HWND, chain: &Chain) {
    let plan = chains::plan(chain);
    if plan.is_empty() {
        return;
    }
    let run = NEXT_RUN.with(|next| next.replace(next.get().wrapping_add(1).max(1)));
    RUNS.with(|runs| runs.borrow_mut().insert(run, plan.clone()));
    let raw = hwnd.0 as isize;
    let _ = chains::schedule(plan, move |step| unsafe {
        PostMessageW(Some(HWND(raw as *mut _)), WM_CHAIN_STEP, WPARAM(run as usize), LPARAM(step.index as isize)).is_ok()
    });
}

/// `WM_CHAIN_STEP`: the delay before step `index` of `run` has passed, so run its command.
pub(crate) fn chain_step(hwnd: HWND, app: &AppRef, run: u32, index: usize) {
    let step = RUNS.with(|runs| {
        let mut runs = runs.borrow_mut();
        let plan = runs.get(&run)?;
        let step = plan.get(index).cloned();
        if index + 1 >= plan.len() {
            runs.remove(&run);
        }
        step
    });
    if let Some(step) = step {
        // An id that no longer exists (a command removed since the chain was saved) is skipped.
        super::window::run_registered(hwnd, app, &step.command);
    }
}

pub(crate) fn run_cheat_sheet(cx: &Context) {
    super::window::show_shortcuts_sheet(cx.hwnd, cx.app);
}

pub(crate) fn run_keyboard_settings(cx: &Context) {
    super::settings_page::open(cx.hwnd, cx.app, Some(Section::Keyboard), false);
}

/// Turns "prioritise website shortcuts" on or off for the site in front.
pub(crate) fn run_toggle_site_shortcuts(cx: &Context) {
    let host = App::tab_committed_url(cx.app, App::active_tab_id(cx.app))
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .and_then(|url| url.host_str().and_then(normalise_site));
    let Some(host) = host else {
        super::window::show_toast(cx.hwnd, "Website shortcuts are set per web page", "Keyboard");
        return;
    };
    let mut store = commands::live().store.clone();
    let message = match toggle_site(&mut store, &host) {
        true => format!("Website shortcuts come first on {host}"),
        false => format!("Browser shortcuts come first on {host}"),
    };
    match commands::save_and_install(store) {
        Ok(_) => {
            super::settings_page::commands_changed(cx.hwnd);
            super::window::show_toast(cx.hwnd, &message, "Keyboard");
        }
        Err(error) => super::show_error_message(&error),
    }
}

/// Adds `host` to the prioritised sites, or removes it (and any parent domain covering it) when it is
/// already covered. True when the site is prioritised afterwards.
pub(crate) fn toggle_site(store: &mut Store, host: &str) -> bool {
    if store.site_prioritised(host) {
        store.sites.retain(|site| !(host == site || host.ends_with(&format!(".{site}"))));
        false
    } else {
        let _ = store.add_site(host);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_a_site_adds_it_and_then_removes_it_with_any_parent_that_covered_it() {
        let mut store = Store::default();
        assert!(toggle_site(&mut store, "docs.example.com"));
        assert!(store.site_prioritised("docs.example.com"));
        assert!(!toggle_site(&mut store, "docs.example.com"));
        assert!(store.sites.is_empty());
        store.add_site("example.com").unwrap();
        assert!(!toggle_site(&mut store, "docs.example.com"), "a covering parent is removed too");
        assert!(store.sites.is_empty());
        assert!(toggle_site(&mut store, "other.example"));
        assert_eq!(store.sites, ["other.example"]);
    }
}
