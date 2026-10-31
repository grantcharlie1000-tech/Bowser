//! Registry commands that need a window: running a command chain, the keyboard cheat sheet, the
//! per-site "prioritise website shortcuts" switch and the shortcut to Keyboard settings.
//!
//! The registry (`crate::commands::registry`) points at these through `Handler::Run`; `run_registered`
//! in `window.rs` is the single place that runs any command by id.
//!
//! A chain runs on a helper thread that only waits and posts `WM_CHAIN_STEP`; every command itself
//! runs on the window thread, as every other command does. Nothing waits on the UI thread.

use std::collections::HashMap;

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

/// The most chains that may run at once; a further start is refused until one finishes.
pub(crate) const MAX_RUNS: usize = 8;

/// Why a chain did not start.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// The same chain is still running.
    AlreadyRunning,
    /// `MAX_RUNS` chains are already running.
    TooMany,
}

/// Running chains by run id, each with its chain id and plan. A run leaves the table with its last
/// step, or when its helper thread can no longer post (the window is gone), so nothing leaks.
#[derive(Default)]
pub(crate) struct Runs {
    runs: HashMap<u32, (u32, Vec<Planned>)>,
    next: u32,
}

impl Runs {
    /// Registers a run of chain `chain` with `plan`, unless that chain is already running or too
    /// many are.
    pub(crate) fn begin(&mut self, chain: u32, plan: Vec<Planned>) -> Result<u32, Refused> {
        if self.runs.values().any(|(id, _)| *id == chain) {
            return Err(Refused::AlreadyRunning);
        }
        if self.runs.len() >= MAX_RUNS {
            return Err(Refused::TooMany);
        }
        self.next = self.next.wrapping_add(1).max(1);
        while self.runs.contains_key(&self.next) {
            self.next = self.next.wrapping_add(1).max(1);
        }
        self.runs.insert(self.next, (chain, plan));
        Ok(self.next)
    }

    /// Step `index` of `run`, removing the run after its last step.
    pub(crate) fn step(&mut self, run: u32, index: usize) -> Option<Planned> {
        let (_, plan) = self.runs.get(&run)?;
        let step = plan.get(index).cloned();
        if index + 1 >= plan.len() {
            self.runs.remove(&run);
        }
        step
    }

    /// Drops `run` (its helper thread stopped early).
    pub(crate) fn abandon(&mut self, run: u32) {
        self.runs.remove(&run);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.runs.len()
    }
}

/// Shared with the helper threads so a run whose post fails can leave the table. Held only briefly.
static RUNS: std::sync::Mutex<Option<Runs>> = std::sync::Mutex::new(None);

fn with_runs<T>(f: impl FnOnce(&mut Runs) -> T) -> T {
    let mut guard = RUNS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(Runs::default))
}

/// Starts the saved chain `number` for the window `hwnd`.
pub(crate) fn start_chain(hwnd: HWND, store: &Store, number: u32) {
    if let Some(chain) = store.chains.iter().find(|chain| chain.id == number) {
        start(hwnd, chain);
    }
}

/// Runs `chain` now: its first command after its own delay, each next one after its. Also used by
/// the editor's Test Chain button with a chain that is not saved yet. A chain that is already
/// running, or a start beyond `MAX_RUNS` running chains, is refused with a toast.
pub(crate) fn start(hwnd: HWND, chain: &Chain) {
    let plan = chains::plan(chain);
    if plan.is_empty() {
        return;
    }
    let run = match with_runs(|runs| runs.begin(chain.id, plan.clone())) {
        Ok(run) => run,
        Err(Refused::AlreadyRunning) => {
            super::window::show_toast(hwnd, &format!("{} is already running", chain.name), "Command chains");
            return;
        }
        Err(Refused::TooMany) => {
            super::window::show_toast(hwnd, "Too many command chains are running", "Command chains");
            return;
        }
    };
    let raw = hwnd.0 as isize;
    let _ = chains::schedule(plan, move |step| {
        let posted = unsafe {
            PostMessageW(Some(HWND(raw as *mut _)), WM_CHAIN_STEP, WPARAM(run as usize), LPARAM(step.index as isize)).is_ok()
        };
        if !posted {
            with_runs(|runs| runs.abandon(run));
        }
        posted
    });
}

/// `WM_CHAIN_STEP`: the delay before step `index` of `run` has passed, so run its command.
pub(crate) fn chain_step(hwnd: HWND, app: &AppRef, run: u32, index: usize) {
    let step = with_runs(|runs| runs.step(run, index));
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

// --- latency (fix E) --- "Bookmarks panel" (Ctrl+B by default; rebindable like every command).
pub(crate) fn run_bookmarks_panel(cx: &Context) {
    let _ = super::window::rail_activate(cx.hwnd, cx.app, super::side_panel::PanelId::Bookmarks);
}

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
        SiteChoice::SaveFailed => {} // The app already showed the save error; no success toast.
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
    save_shortcut_change(cx, &path, &settings);
}

// --- fix group C: a shortcut's change reaches an open settings page ---
/// Saves a change a shortcut made to `settings.txt`, applies it, and has an open settings page read
/// the file again: the page's draft would otherwise still hold the old value, and its next Save (of
/// any row) would put it back.
fn save_shortcut_change(cx: &Context, path: &std::path::Path, settings: &crate::settings::Settings) {
    match crate::settings::save(path, settings) {
        Ok(()) => {
            App::apply_live_settings(cx.app, settings);
            super::settings_page::refresh(cx.hwnd);
        }
        Err(error) => super::show_error_message(&format!("Settings could not be saved: {error}")),
    }
}
// --- end fix group C ---
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
    save_shortcut_change(cx, &path, &settings);
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

    // --- fix group C ---
    /// Ctrl+F11 and "Show or hide the address bar" save straight to the file; an open settings page
    /// must read it again (`settings_page::refresh`), since the page's draft only follows a snapshot
    /// and its next Save sends every row. The page side (a refreshed snapshot replaces the draft, so
    /// a later Save keeps the file's value) is tested in ui/test/settings-page.mjs.
    #[test]
    fn the_shortcut_toggles_refresh_an_open_settings_page() {
        let source = include_str!("command_actions.rs").replace("\r\n", "\n");
        let body = |name: &str| {
            let start = source.find(&format!("fn {name}(")).unwrap_or_else(|| panic!("{name}"));
            let end = source[start..].find("\n}\n").map_or(source.len(), |end| start + end);
            source[start..end].to_owned()
        };
        for name in ["run_toggle_auto_hide", "run_toggle_address_bar"] {
            let body = body(name);
            assert!(body.contains("save_shortcut_change(cx,"), "{name} saves through the refreshing helper");
            assert!(!body.contains("settings::save("), "{name} has no save of its own that skips the refresh");
        }
        let helper = body("save_shortcut_change");
        let applied = helper.find("apply_live_settings").expect("applies");
        let refreshed = helper.find("settings_page::refresh(cx.hwnd)").expect("refreshes the page");
        assert!(applied < refreshed && helper[..applied].contains("Ok(())"), "only after a save that worked");

        // The sidebar drag saves its width from memory the same way: the page reads it again too.
        let window = include_str!("window.rs").replace("\r\n", "\n");
        let start = window.find("fn finish_sidebar_drag(").expect("finish_sidebar_drag");
        let drag = &window[start..start + window[start..].find("\n}\n").expect("end")];
        let saved = drag.find("App::save_settings(app)").expect("saves");
        assert!(drag[saved..].contains("settings_page::refresh(hwnd)"), "the drag refreshes the page after saving");
    }
    // --- end fix group C ---

    #[test]
    fn a_running_chain_is_not_started_twice_runs_are_capped_and_a_failed_post_leaves_no_entry() {
        let plan = |n: usize| (0..n).map(|index| Planned { index, delay_ms: 0, command: "reload".into() }).collect::<Vec<_>>();
        let mut runs = Runs::default();
        let first = runs.begin(1, plan(2)).unwrap();
        assert_eq!(runs.begin(1, plan(2)), Err(Refused::AlreadyRunning));
        for chain in 2..=MAX_RUNS as u32 {
            runs.begin(chain, plan(1)).unwrap();
        }
        assert_eq!(runs.begin(99, plan(1)), Err(Refused::TooMany));
        assert_eq!(runs.len(), MAX_RUNS);
        // Steps run in order and the last one ends the run, which frees a slot and the chain id.
        assert_eq!(runs.step(first, 0).map(|s| s.index), Some(0));
        assert_eq!(runs.step(first, 1).map(|s| s.index), Some(1));
        assert_eq!(runs.step(first, 1), None, "the run is gone after its last step");
        let again = runs.begin(1, plan(3)).unwrap();
        // A helper whose post failed abandons its run: nothing stays behind.
        runs.abandon(again);
        assert_eq!(runs.len(), MAX_RUNS - 1);
        assert!(runs.begin(1, plan(1)).is_ok());
    }

    #[test]
    fn a_chain_whose_window_is_gone_leaves_the_running_table_and_can_start_again() {
        use crate::commands::chains::Step;
        // No window has this handle, so the helper thread's first post fails.
        let gone = HWND(0x7FFF_FFF0usize as *mut _);
        let chain = Chain { id: 0xC4A1, name: "Gone".into(), steps: vec![Step { command: "reload".into(), delay_ms: 0 }, Step { command: "reload".into(), delay_ms: 0 }] };
        start(gone, &chain);
        let running = || with_runs(|runs| runs.runs.values().any(|(id, _)| *id == chain.id));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while running() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!running(), "the run was abandoned, not leaked");
        // So the same chain is not refused as "already running" afterwards.
        assert!(with_runs(|runs| runs.begin(chain.id, chains::plan(&chain))).map(|run| with_runs(|runs| runs.abandon(run))).is_ok());
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
