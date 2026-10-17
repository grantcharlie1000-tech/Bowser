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

// --- toolbars and feeds (T13) ---
/// "Feeds" and "Subscribe to this page's feed": the Feeds panel, at the feeds the page in front offers.
pub(crate) fn run_open_feeds(cx: &Context) {
    super::window::open_feeds_panel(cx.hwnd, cx.app);
}
// --- end toolbars and feeds ---

// --- wave-1 integration: registry commands for T1's periodic reload, T3's privacy tools and T2's
// address bar, so Quick Commands, the cheat sheet, the keyboard editor and chains reach them ---

/// "Reload this tab every…": the periodic reload presets for the tab in front (T1).
pub(crate) fn run_periodic_reload(cx: &Context) {
    super::window::show_periodic_reload_menu(cx.hwnd, cx.app);
}

/// "Stop reloading all tabs" (T1).
pub(crate) fn run_stop_all_reloads(cx: &Context) {
    let stopped = App::disable_all_reloads(cx.app);
    let message = match stopped {
        0 => "No tab is reloading on a timer".to_owned(),
        1 => "Stopped reloading 1 tab".to_owned(),
        n => format!("Stopped reloading {n} tabs"),
    };
    super::window::show_toast(cx.hwnd, &message, "Periodic reload");
}

/// "Delete browsing data": Settings > Privacy, where T3's Delete browsing data block is.
pub(crate) fn run_delete_browsing_data(cx: &Context) {
    super::settings_page::open(cx.hwnd, cx.app, Some(Section::Privacy), false);
}

/// The words the toast uses for a blocking level.
fn level_words(level: crate::blocker::Level) -> &'static str {
    match level {
        crate::blocker::Level::None => "no blocking",
        crate::blocker::Level::Trackers => "block trackers",
        crate::blocker::Level::Ads => "block trackers and ads",
    }
}

/// "Next blocking level for this site": none, trackers, trackers and ads, then none again (T3's
/// per-site level, the same choice the shield makes).
pub(crate) fn run_cycle_blocking_level(cx: &Context) {
    use crate::blocker::SiteChoice;
    let Some((host, site)) = App::active_web_site(cx.app) else {
        super::window::show_toast(cx.hwnd, "Blocking levels are set per web page", "Blocking");
        return;
    };
    let current = App::blocker_state_for(cx.app, &host, false).map_or(crate::blocker::Level::Ads, |(level, _)| level);
    let next = current.next();
    match App::choose_site_level(cx.app, next) {
        SiteChoice::Saved => super::window::show_toast(cx.hwnd, &format!("{site}: {}", level_words(next)), "Blocking"),
        SiteChoice::ConfiguredInSettings => super::window::show_toast(cx.hwnd, &format!("{site} is on the list of sites the blocker skips"), "Blocking"),
        SiteChoice::NoSite => super::window::show_toast(cx.hwnd, "This page has no site to set a level for", "Blocking"),
    }
}

/// "QR code for this page" (T2's address bar dialog).
pub(crate) fn run_qr_code(cx: &Context) {
    match App::tab_committed_url(cx.app, App::active_tab_id(cx.app)).filter(|url| matches!(url.scheme(), "http" | "https")) {
        Some(url) => super::address_dialog::show_qr(cx.hwnd, cx.app, url.as_str()),
        None => super::window::show_toast(cx.hwnd, "A QR code needs a web page", "QR code"),
    }
}

/// Flips T2's `address_bar_show`. Returns the new value.
pub(crate) fn toggle_address_bar_setting(settings: &mut crate::settings::Settings) -> bool {
    let show = !settings.address.bar_show;
    let _ = settings.address.set("address_bar_show", if show { "true" } else { "false" });
    settings.address.bar_show
}

/// "Show or hide the address bar" (T2's Address bar > Show the address bar), saved like the page's switch.
pub(crate) fn run_toggle_address_bar(cx: &Context) {
    let Ok(profile) = crate::profile::active() else { return };
    let path = profile.settings_file();
    let Ok(mut settings) = crate::settings::read_strict(&path) else {
        super::show_error_message("settings.txt could not be read, so the address bar was not changed.");
        return;
    };
    toggle_address_bar_setting(&mut settings);
    match crate::settings::save(&path, &settings) {
        Ok(()) => App::apply_live_settings(cx.app, &settings),
        Err(error) => super::show_error_message(&format!("Settings could not be saved: {error}")),
    }
}
// --- end wave-1 integration ---

// --- auto-hide (T11) ---
/// "Toggle auto-hide UI" (Ctrl+F11): flips the saved master switch (Settings > Appearance > Auto-hide
/// toolbars and tabs), as in Vivaldi, so the choice lasts across restarts.
pub(crate) fn run_toggle_auto_hide(cx: &Context) {
    let Ok(profile) = crate::profile::active() else { return };
    let path = profile.settings_file();
    let Ok(mut settings) = crate::settings::read_strict(&path) else {
        super::show_error_message("settings.txt could not be read, so auto-hide was not changed.");
        return;
    };
    settings.frameless = !settings.frameless;
    match crate::settings::save(&path, &settings) {
        Ok(()) => App::apply_live_settings(cx.app, &settings),
        Err(error) => super::show_error_message(&format!("Settings could not be saved: {error}")),
    }
}

/// "Full screen" (F11): the window fills its monitor with no frame, or goes back.
pub(crate) fn run_toggle_fullscreen(cx: &Context) {
    super::window::toggle_fullscreen(cx.hwnd, cx.app);
}
// --- end auto-hide ---

#[cfg(test)]
mod tests {
    use super::*;

    /// Integration: T2's suggestion wake-up and T4's chain step were both WM_APP + 0x5E1, so the main
    /// window's message match never reached the address bar's arm. The window's own wave-1 messages
    /// must stay distinct.
    #[test]
    fn the_main_window_messages_of_the_wave_1_threads_are_distinct() {
        let ids = [
            super::super::address_bar::WM_ADDRESS_SUGGEST,
            super::super::WM_CHAIN_STEP,
            super::super::bookmark_bar::WM_BOOKMARK_BAR_COMMAND,
        ];
        let unique: std::collections::HashSet<u32> = ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len(), "{ids:x?}");
    }

    #[test]
    fn the_address_bar_command_flips_the_saved_switch_and_it_survives_a_reload() {
        let mut settings = crate::settings::Settings::default();
        let before = settings.address.bar_show;
        assert_eq!(toggle_address_bar_setting(&mut settings), !before);
        let text = crate::settings::render_updated("", &settings);
        assert_eq!(crate::settings::Settings::parse(&text).address.bar_show, !before, "{text}");
        assert_eq!(toggle_address_bar_setting(&mut settings), before);
    }

    #[test]
    fn the_new_commands_are_registered_where_the_palette_cheat_sheet_and_chains_find_them() {
        use crate::commands::registry::{builtin, Handler, PaletteKind};
        for id in [
            "reopen-tab", "closed-tabs", "save-all-session", "open-session", "periodic-reload", "stop-all-reloads",
            "delete-browsing-data", "cycle-blocking-level", "qr-code", "toggle-address-bar",
        ] {
            let def = builtin(id).unwrap_or_else(|| panic!("{id} is not registered"));
            assert_ne!(def.palette, PaletteKind::Hidden, "{id} shows in Quick commands");
            assert!(matches!(def.handler, Handler::Run(_) | Handler::Action(_) | Handler::Key(_)));
        }
        // Chains name commands by id: each of them is a step a chain may hold.
        let live = crate::commands::Live::new(crate::commands::store::Store::default());
        let listed: Vec<String> = crate::commands::palette_commands(&live).into_iter().map(|c| c.id).collect();
        for id in ["periodic-reload", "delete-browsing-data", "cycle-blocking-level", "qr-code", "toggle-address-bar", "reopen-tab"] {
            assert!(listed.iter().any(|listed| listed == id), "{id} reaches the palette");
            let chain = crate::commands::chains::Chain { id: 1, name: "x".into(), steps: vec![crate::commands::chains::Step { delay_ms: 0, command: id.into() }] };
            let known = |id: &str| builtin(id).is_some();
            assert_eq!(crate::commands::chains::validate(&chain, &[], known), Ok(()), "{id} can be a chain step");
        }
    }

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
