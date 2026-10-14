//! The settings page (`bowser://settings`, docs/design/settings-page.md): one trusted HTML host
//! (`View::Settings`) per window, in a Bowser-owned child window over the page area of the shown
//! settings tab. The tab has no content WebView2; this host is the page.
//!
//! Lifetime follows the split host (`split_ui.rs`): created on first use, hidden and suspended while
//! no settings tab is shown, closed with the window. Commands are queued from the WebView2 callback
//! and run from the window procedure, and only while a settings tab is shown. The wire and the
//! field checks are in `chrome_host::settings`.
//!
//! Every function runs on the UI thread. No `RefCell` borrow of the runtime table is held while
//! calling into `App`, WebView2 or window functions, which may re-enter this module.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, IsWindow, RegisterClassW, SetWindowPos,
            ShowWindow, HWND_TOP, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WNDCLASSW, WS_CHILD,
            WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
        },
    },
    core::{w, PCWSTR},
};

use crate::{
    app::{App, AppRef},
    chrome_host::{
        self,
        host::{Accel, AccelHandler, ChromeHost, View},
        protocol::{Command, PanelFields, PanelType},
        settings::{self as wire, Ack, Action},
    },
    internal_page::{InternalPage, Section},
    split_view::Pane,
    tabs::TabId,
};

const CLASS: PCWSTR = w!("BowserSettingsPage");

/// Where the page is shown: over settings tab `tab`, at `rect` (client coordinates).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shown {
    tab: TabId,
    rect: RECT,
}

#[derive(Default)]
struct Runtime {
    container: Option<HWND>,
    host: Option<ChromeHost>,
    requested: bool,
    ready: bool,
    failed: bool,
    suspended: bool,
    shown: Option<Shown>,
    /// The tab the page last showed, so a different settings tab moves the page to its section.
    last_tab: Option<TabId>,
    /// Last theme state the page accepted.
    state: Option<String>,
    /// Last snapshot sent, and whether a fresh one is due (settings are read from disk only then).
    snapshot: Option<String>,
    dirty: bool,
    /// A section to show (`settingsgo`), sent once the page is ready and shown.
    go: Option<Option<Section>>,
    focus_pending: bool,
    ack: Ack,
    error: String,
    /// The page's own update check: running, and how the last one ended.
    checking: bool,
    check_result: &'static str,
}

thread_local! {
    static RUNTIMES: RefCell<HashMap<isize, Runtime>> = RefCell::new(HashMap::new());
    static QUEUE: RefCell<Vec<(isize, PanelFields)>> = const { RefCell::new(Vec::new()) };
}

fn with<T>(owner: HWND, f: impl FnOnce(&mut Runtime) -> T) -> Option<T> {
    RUNTIMES.with(|runtimes| {
        let mut runtimes = runtimes.try_borrow_mut().ok()?;
        Some(f(runtimes.entry(owner.0 as isize).or_insert_with(|| Runtime { check_result: "none", ..Runtime::default() })))
    })
}

fn rect_size(rect: RECT) -> RECT {
    RECT { left: 0, top: 0, right: (rect.right - rect.left).max(1), bottom: (rect.bottom - rect.top).max(1) }
}

/// Whether this window can draw the settings page: its HTML chrome is on, the chrome environment
/// exists and the page's host has not failed. Otherwise Settings is the native window.
fn available(owner: HWND) -> bool {
    chrome_host::html_ui_enabled()
        && super::window::html_environment(owner).is_some()
        && !with(owner, |rt| rt.failed).unwrap_or(false)
}

/// Opens the settings page at `section` (main menu, palette, omnibox). `reuse_blank`: a blank
/// normal shown tab becomes the page (the omnibox) instead of a new tab opening (the menu).
pub(crate) fn open(owner: HWND, app: &AppRef, section: Option<Section>, reuse_blank: bool) {
    if !available(owner) {
        if let Err(error) = super::settings_panel::open(owner, app) {
            super::show_error_message(&format!("Settings could not be opened: {error}"));
        }
        return;
    }
    let _ = App::open_internal_page(app, InternalPage::Settings(section), reuse_blank);
    go(owner, section);
}

/// Shows `section` on the page (now, or as soon as it is ready) and gives it the keyboard.
pub(crate) fn go(owner: HWND, section: Option<Section>) {
    let _ = with(owner, |rt| {
        rt.go = Some(section);
        rt.focus_pending = true;
        rt.dirty = true;
    });
    super::window::split_ui_invalidate(owner);
}

/// F5 on a settings tab: read everything again.
pub(crate) fn refresh(owner: HWND) {
    let _ = with(owner, |rt| rt.dirty = true);
    super::window::split_ui_invalidate(owner);
}

/// Gives the keyboard to the page (after the omnibox submits or escapes on a settings tab).
pub(crate) fn focus(owner: HWND) {
    let host = with(owner, |rt| (rt.ready && !rt.failed && rt.shown.is_some()).then(|| rt.host.clone()).flatten()).flatten();
    match host {
        Some(host) => { let _ = host.focus(); }
        None => { let _ = with(owner, |rt| rt.focus_pending = true); }
    }
}

/// Whether the page covers the viewport (or a pane) right now: the overlay then frames it like a
/// page instead of painting the blank-tab surface over it.
pub(crate) fn shown(owner: HWND) -> bool {
    with(owner, |rt| rt.ready && !rt.failed && rt.shown.is_some()).unwrap_or(false)
}

/// The split pane the page covers, if it is shown inside a split.
pub(crate) fn pane(owner: HWND, app: &AppRef) -> Option<Pane> {
    let tab = with(owner, |rt| rt.ready.then_some(rt.shown).flatten().map(|shown| shown.tab)).flatten()?;
    App::split_view(app)?.pane_for(tab)
}

/// The page's "Check for updates" finished with `result` (`staged`, `uptodate`, ...).
pub(crate) fn update_checked(owner: HWND, result: &'static str) {
    let _ = with(owner, |rt| {
        rt.checking = false;
        rt.check_result = result;
        rt.dirty = true;
    });
    super::window::split_ui_invalidate(owner);
}

// ----- the host and its container window -----

fn register_class() -> bool {
    thread_local! { static REGISTERED: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) }; }
    if let Some(done) = REGISTERED.with(|slot| slot.get()) {
        return done;
    }
    let instance = unsafe { GetModuleHandleW(None) }.map(|module| module.into()).unwrap_or_default();
    let class = WNDCLASSW { lpfnWndProc: Some(container_proc), hInstance: instance, lpszClassName: CLASS, ..Default::default() };
    let ok = unsafe { RegisterClassW(&class) } != 0;
    REGISTERED.with(|slot| slot.set(Some(ok)));
    ok
}

unsafe extern "system" fn container_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// The host could not be created or its page failed: the window falls back to the native Settings
/// window, and an open settings tab is left showing the blank-tab surface.
fn fail(owner: HWND) {
    let _ = with(owner, |rt| rt.failed = true);
    super::window::split_ui_invalidate(owner);
}

/// Creates the container (visible at one pixel, see host.rs) and starts the controller, once.
fn ensure_host(owner: HWND) {
    let start = with(owner, |rt| {
        if rt.requested || rt.failed {
            return false;
        }
        rt.requested = true;
        true
    })
    .unwrap_or(false);
    if !start {
        return;
    }
    let Some(env) = super::window::html_environment(owner) else {
        let _ = with(owner, |rt| rt.requested = false);
        return;
    };
    if !register_class() {
        fail(owner);
        return;
    }
    let instance = unsafe { GetModuleHandleW(None) }.map(|module| module.into()).ok();
    let container = unsafe {
        CreateWindowExW(Default::default(), CLASS, w!(""), WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS, 0, 0, 1, 1, Some(owner), None, instance, None)
    };
    let Ok(container) = container else {
        fail(owner);
        return;
    };
    unsafe {
        let _ = SetWindowPos(container, Some(HWND_TOP), 0, 0, 1, 1, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
    let _ = with(owner, |rt| rt.container = Some(container));
    let raw = owner.0 as isize;
    let on_command: Rc<dyn Fn(Command)> = Rc::new(move |command| {
        // Never inside WebView2's callback: commands open windows and menus and close tabs.
        let owner = HWND(raw as *mut _);
        match command {
            Command::Ready => {
                let _ = with(owner, |rt| {
                    rt.ready = true;
                    rt.state = None;
                    rt.snapshot = None;
                    rt.dirty = true;
                    // A reloaded page starts at the top: show the tab's section again.
                    if rt.go.is_none() {
                        rt.last_tab = None;
                    }
                });
                super::window::split_ui_invalidate(owner);
            }
            Command::Panel { kind: PanelType::Settings, f } => queue(owner, f),
            _ => {}
        }
    });
    let on_failed: Rc<dyn Fn()> = Rc::new(move || fail(HWND(raw as *mut _)));
    let on_accelerator: AccelHandler = Rc::new(move |accel: Accel| super::window::html_settings_accelerator(HWND(raw as *mut _), accel));
    let created = ChromeHost::create(&env, container, RECT { left: 0, top: 0, right: 1, bottom: 1 }, View::Settings, on_command, on_failed, Some(on_accelerator), move |result| {
        let owner = HWND(raw as *mut _);
        match result {
            Ok(host) => {
                let keep = with(owner, |rt| {
                    if rt.failed {
                        return false;
                    }
                    rt.host = Some(host.clone());
                    true
                })
                .unwrap_or(false);
                if !keep {
                    host.close();
                }
                super::window::split_ui_invalidate(owner);
            }
            Err(_) => fail(owner),
        }
    });
    if created.is_err() {
        fail(owner);
    }
}

/// Queues a page command and posts `WM_SETTINGS_COMMAND` so it runs from the window procedure.
fn queue(owner: HWND, fields: PanelFields) {
    let full = QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        if queue.len() >= 16 {
            return true;
        }
        queue.push((owner.0 as isize, fields));
        false
    });
    if !full {
        crate::win32::post(owner, super::WM_SETTINGS_COMMAND, WPARAM(0), LPARAM(0));
    }
}

/// Closes the host (window closing or chrome failure).
pub(crate) fn close(owner: HWND) {
    let removed = RUNTIMES.with(|runtimes| runtimes.try_borrow_mut().ok().and_then(|mut all| all.remove(&(owner.0 as isize))));
    if let Some(runtime) = removed {
        if let Some(host) = runtime.host {
            host.close();
        }
        if let Some(container) = runtime.container {
            unsafe {
                if IsWindow(Some(container)).as_bool() {
                    let _ = DestroyWindow(container);
                }
            }
        }
    }
}

/// Where the page should be now: over the shown settings tab, or over a settings tab's pane in a
/// shown split (the focused one when both are).
fn wanted(owner: HWND, app: &AppRef) -> Option<Shown> {
    let active = App::active_tab_id(app);
    if let Some(split) = App::split_view(app) {
        let panes = [split.focused, split.focused.other()];
        let pane = panes.into_iter().find(|pane| App::internal_page(app, split.tab(*pane)).is_some_and(InternalPage::is_settings))?;
        let tab = split.tab(pane);
        let rect = match App::split_bounds(app) {
            Some((left, right)) => if pane == Pane::Left { left } else { right },
            None if split.focused == pane => super::window::split_viewport(owner)?,
            None => return None,
        };
        return Some(Shown { tab, rect });
    }
    App::internal_page(app, active).filter(|page| page.is_settings())?;
    Some(Shown { tab: active, rect: super::window::split_viewport(owner)? })
}

/// Brings the host in line with the shown tab. Cheap when nothing changed; called from every
/// chrome paint.
pub(crate) fn sync(owner: HWND, app: &AppRef) {
    if !chrome_host::html_ui_enabled() {
        return;
    }
    let want = wanted(owner, app);
    if want.is_some() {
        ensure_host(owner);
    }
    let Some((host, container, ready, shown, suspended)) = with(owner, |rt| (rt.host.clone(), rt.container, rt.ready && !rt.failed, rt.shown, rt.suspended)) else {
        return;
    };
    let (Some(host), Some(container), true) = (host, container, ready) else {
        return;
    };
    let Some(want) = want else {
        if shown.is_some() {
            let _ = host.set_visible(false);
            unsafe {
                let _ = ShowWindow(container, SW_HIDE);
            }
            let suspend = with(owner, |rt| {
                rt.shown = None;
                !std::mem::replace(&mut rt.suspended, true)
            })
            .unwrap_or(false);
            if suspend && host.try_suspend().is_err() {
                let _ = with(owner, |rt| rt.suspended = false);
            }
        }
        return;
    };
    if suspended {
        if host.resume().is_err() {
            return;
        }
        let _ = with(owner, |rt| rt.suspended = false);
    }
    // Another settings tab (or the first show): its own section, a fresh snapshot.
    let new_tab = with(owner, |rt| rt.last_tab != Some(want.tab)).unwrap_or(false);
    if new_tab {
        let section = App::internal_page(app, want.tab).and_then(InternalPage::section);
        let _ = with(owner, |rt| {
            rt.last_tab = Some(want.tab);
            rt.go.get_or_insert(section);
            rt.dirty = true;
        });
    }
    // Theme first, then content, then show: the page never appears unthemed or stale.
    let state = super::window::split_host_state(owner);
    if with(owner, |rt| rt.state.as_deref() != Some(state.as_str())).unwrap_or(false) && host.post_json(&state).is_ok() {
        let _ = with(owner, |rt| rt.state = Some(state));
    }
    if with(owner, |rt| std::mem::take(&mut rt.dirty)).unwrap_or(false) {
        let snapshot = snapshot(owner, app);
        // An answer is always delivered, even when the values did not change (a second identical
        // save still gets its "Saved").
        let fresh = with(owner, |rt| rt.ack != Ack::None || rt.snapshot.as_deref() != Some(snapshot.as_str())).unwrap_or(false);
        if fresh && host.post_json(&snapshot).is_ok() {
            let _ = with(owner, |rt| {
                rt.snapshot = Some(snapshot);
                // An answer is reported once.
                rt.ack = Ack::None;
                rt.error.clear();
            });
        }
    }
    if let Some(section) = with(owner, |rt| rt.go.take()).flatten() {
        let _ = host.post_json(&wire::go_json(section));
    }
    if shown != Some(want) {
        let size = rect_size(want.rect);
        unsafe {
            let _ = SetWindowPos(container, Some(HWND_TOP), want.rect.left, want.rect.top, size.right, size.bottom, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        let _ = host.set_bounds(size);
        let _ = host.set_visible(true);
        let _ = with(owner, |rt| rt.shown = Some(want));
    }
    // The keyboard goes to the page when it was opened or a settings tab was selected, unless the
    // user is typing in the omnibox.
    let asked = with(owner, |rt| std::mem::take(&mut rt.focus_pending)).unwrap_or(false);
    let appeared = shown.is_none() && !super::window::omnibox_focused(owner);
    if (asked || appeared) && want.tab == App::active_tab_id(app) {
        let _ = host.focus();
    }
}

fn snapshot(owner: HWND, app: &AppRef) -> String {
    use crate::updater::{LastCheck, Status};
    let (ack, error, checking, check_result) = with(owner, |rt| (rt.ack, rt.error.clone(), rt.checking, rt.check_result)).unwrap_or((Ack::None, String::new(), false, "none"));
    let profile = crate::profile::active().ok();
    let read = profile.as_ref().map(|profile| crate::settings::read_strict(&profile.settings_file()));
    let readable = matches!(read, Some(Ok(_)));
    let settings = match read {
        Some(Ok(settings)) => settings,
        _ => crate::settings::Settings::default(),
    };
    let custom_search = profile.as_ref().is_some_and(|profile| !crate::navigation::SearchProvider::from_config(None, &profile.search_provider_file()).is_default());
    let guest = profile.as_ref().is_some_and(|profile| profile.is_guest());
    let name = profile.as_ref().map_or_else(|| "Bowser".to_owned(), |profile| profile.listed_name().to_owned());
    let avatar = profile.as_ref().map_or_else(|| "B".to_owned(), |profile| crate::profile::ProfileMetadata::load(profile).avatar_glyph(profile.listed_name()));
    let status = crate::updater::status();
    let (update, update_version, last_check) = match &status {
        Status::Unavailable => ("unavailable", String::new(), "none"),
        Status::Checking => ("checking", String::new(), "none"),
        Status::Ready(version) => ("ready", version.to_string(), "none"),
        Status::Idle { last } => ("idle", String::new(), match last {
            None => "none",
            Some(LastCheck::UpToDate) => "uptodate",
            Some(LastCheck::Failed) => "failed",
            Some(LastCheck::Other) => "other",
        }),
    };
    // The page's own check is "checking" until it reports, whatever the background thread says.
    let update = if checking && update != "ready" { "checking" } else { update };
    let version = crate::updater::Version::current().to_string();
    wire::snapshot_json(&wire::Snapshot {
        settings: &settings,
        readable,
        custom_search,
        efficiency_active: App::efficiency_active(app),
        auto_update: crate::updater::auto_update_enabled(),
        history_enabled: profile.as_ref().is_some_and(|profile| profile.history_enabled()),
        guest,
        profile: &name,
        avatar: &avatar,
        version: &version,
        update,
        update_version: &update_version,
        last_check,
        check_result,
        ack,
        error: &error,
    })
}

/// Records how a change went; the next snapshot reports it.
fn answer(owner: HWND, result: Result<(), String>) {
    let _ = with(owner, |rt| {
        rt.dirty = true;
        match result {
            Ok(()) => {
                rt.ack = Ack::Saved;
                rt.error.clear();
            }
            Err(error) => {
                rt.ack = Ack::Failed;
                rt.error = error;
            }
        }
    });
}

/// Runs the queued commands for `owner` (from `WM_SETTINGS_COMMAND`). A command acts only while
/// the page is shown over a settings tab; anything else is dropped.
pub(crate) fn run_queued(owner: HWND, app: &AppRef) {
    let commands: Vec<PanelFields> = QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        let (mine, rest): (Vec<_>, Vec<_>) = queue.drain(..).partition(|(id, _)| *id == owner.0 as isize);
        *queue = rest;
        mine.into_iter().map(|(_, fields)| fields).collect()
    });
    for fields in commands {
        let Some(tab) = with(owner, |rt| rt.shown.map(|shown| shown.tab)).flatten() else { continue };
        if !App::internal_page(app, tab).is_some_and(InternalPage::is_settings) {
            continue;
        }
        if let Some(action) = wire::map_command(&fields) {
            run(owner, app, tab, action);
        }
    }
    super::window::split_ui_invalidate(owner);
}

fn run(owner: HWND, app: &AppRef, tab: TabId, action: Action) {
    let profile = crate::profile::active();
    match action {
        Action::Save(text) => {
            let Ok(profile) = profile else { return answer(owner, Err("The profile is not ready.".into())) };
            let path = profile.settings_file();
            let Ok(mut settings) = crate::settings::read_strict(&path) else {
                return answer(owner, Err("settings.txt could not be read, so nothing was saved. Open the settings file to fix it.".into()));
            };
            if !wire::apply_settings_text(&mut settings, &text) {
                return answer(owner, Err("That value is not allowed. Nothing was saved.".into()));
            }
            let saved = crate::settings::save(&path, &settings);
            if saved.is_ok() {
                App::apply_tab_layout_settings(app, settings.tab_layout, settings.sidebar_width);
                App::apply_efficiency_mode(app, settings.efficiency_mode);
                App::apply_sleep_settings(app, settings.sleep_enabled, settings.sleep_after_secs);
            }
            answer(owner, saved.map_err(|error| format!("Settings could not be saved: {error}. Nothing was saved.")));
        }
        Action::Reset => {
            let Ok(profile) = profile else { return answer(owner, Err("The profile is not ready.".into())) };
            let defaults = crate::settings::Settings::default();
            let saved = crate::settings::save(&profile.settings_file(), &defaults);
            if saved.is_ok() {
                App::apply_tab_layout_settings(app, defaults.tab_layout, defaults.sidebar_width);
                App::apply_efficiency_mode(app, defaults.efficiency_mode);
                App::apply_sleep_settings(app, defaults.sleep_enabled, defaults.sleep_after_secs);
            }
            answer(owner, saved.map_err(|error| format!("Settings could not be reset: {error}.")));
        }
        Action::OpenFile => {
            let Ok(profile) = profile else { return };
            if let Err(message) = super::settings_panel::launch_settings_file(&profile.settings_file()) {
                answer(owner, Err(message));
            }
        }
        Action::AutoUpdate(on) => {
            answer(owner, crate::updater::set_auto_update(on).map_err(|error| format!("Automatic updates could not be changed: {error}")));
        }
        Action::History(on) => {
            // A guest never saves history; the page shows the switch off and disabled.
            let Ok(profile) = profile else { return };
            if profile.is_guest() {
                return;
            }
            answer(owner, profile.set_history_enabled(on).map_err(|error| format!("History could not be changed: {error}")));
        }
        Action::Section(section) => App::set_internal_section(app, tab, InternalPage::Settings(section)),
        Action::CheckUpdates => {
            let start = with(owner, |rt| {
                let idle = !rt.checking;
                rt.checking = true;
                rt.check_result = "none";
                rt.dirty = true;
                idle
            })
            .unwrap_or(false);
            if start && !super::window::check_for_updates_from_page(owner) {
                update_checked(owner, "failed");
            }
        }
        Action::RestartUpdate => {
            if matches!(crate::updater::status(), crate::updater::Status::Ready(_)) {
                super::window::restart_to_update_from_page(owner);
            }
        }
        Action::Privacy => super::privacy_panel::open(owner, app),
        Action::Profiles => super::window::show_profile_menu_from_page(owner),
        Action::Import => {
            if let Err(error) = super::import_panel::open(owner) {
                super::show_error_message(&format!("Browser import could not be opened: {error}"));
            }
        }
        Action::Refresh => {
            let _ = with(owner, |rt| rt.dirty = true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_rect_is_never_empty() {
        assert_eq!(rect_size(RECT { left: 10, top: 20, right: 10, bottom: 15 }), RECT { left: 0, top: 0, right: 1, bottom: 1 });
        assert_eq!(rect_size(RECT { left: 10, top: 20, right: 410, bottom: 320 }), RECT { left: 0, top: 0, right: 400, bottom: 300 });
    }
}
