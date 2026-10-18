mod card;
// --- address bar, search and bookmarks (src/address_settings.rs) ---
mod address_bar;
mod address_dialog;
mod bookmark_bar;
// --- end address bar, search and bookmarks ---
mod corner_fringe;
mod command_actions; // run_registered helpers: command chains, the cheat sheet, per-site shortcut priority
mod command_palette;
mod composer;
mod devtools_dock;
pub mod frame;
pub mod glass;
mod hover_square;
mod import_panel;
mod island_menu;
mod menu;
// --- popup host (lane M) ---
mod html_popup;
// --- hover layer ---
mod html_hover;
// --- omnibox suggest ---
mod html_suggest;
// --- end omnibox suggest ---
// --- end hover layer ---
// --- end popup host ---
mod notes_panel;
mod page_tools_ui; // capture and notes-from-the-page commands (thread T7)
// --- tab options --- the Sessions panel.
mod sessions_panel;
mod feeds_panel; // the Feeds panel's host side and its fetch workers (thread T13)
// --- end tab options ---
mod panel_bookmarks;
mod panel_customize;
mod panel_history;
// --- panel host (lane P) ---
pub(crate) use panel_history::{
    Action as HistoryAction, Line as HistoryLine, Model as HistoryModel, Range as HistoryRange,
    Row as HistoryRow, LIMIT as HISTORY_LIMIT, current_history_file, execute as execute_history_action,
    local_offset as history_local_offset, now_secs as history_now_secs, civil_date as history_civil_date,
};
mod html_panel;
mod flyout;
mod html_dialog;
// --- end panel host ---
mod panel_widgets;
pub mod panel_webapp;
pub mod passwords_import;
mod passwords_panel;
pub(crate) mod privacy_panel;
mod protection; // blocking, cookie notices, history retention, website appearance (thread T3)
mod profile_rename;
mod settings_page;
mod history_page; // bowser://history (docs/design/history-page.md)
mod downloads_page; // bowser://downloads (docs/design/downloads.md)
mod downloads_ui; // the toolbar download button and the bubble (docs/design/downloads.md)
mod settings_panel;
pub mod side_panel;
mod site_info;
mod smooth;
// --- split view ---
mod split_ui;
pub(crate) mod start_ui; // the start page over a blank new tab (docs/design/start-page.md)
// --- end split view ---
mod tab_ghost; // the native ghost of a held tab (docs/design/tab-drag.md)
mod window;

pub(crate) use window::{
    gated_chords, refresh_edit_font, refresh_theme, schedule_idle_timer, schedule_power_timer, set_link_timer, set_tab_tick, set_theme_timer,
    set_thumbnail_timer, show_pending_address, sync_address_input,
};
pub(crate) use window::address_bar_revealed; // --- layout (thread T6) ---
pub(crate) use html_panel::settings_changed as panel_settings_changed; // --- look state (fix group B) ---
pub(crate) use glass::{local_minutes, set_scheduled_look};
// Registry handlers (`crate::commands::registry`) that need a window.
pub(crate) use command_actions::{run_cheat_sheet, run_keyboard_settings, run_open_feeds, run_toggle_site_shortcuts};
pub(crate) use command_actions::run_bookmarks_panel; // --- latency (fix E) ---
// --- wave-1 integration --- T1, T2 and T3 commands in the registry.
// --- auto-hide (T11) --- Auto-hide and full screen (registry commands).
pub(crate) use command_actions::{run_toggle_auto_hide, run_toggle_fullscreen};
pub(crate) use window::{auto_hide_effective, auto_hide_page_created, auto_hide_sync, page_fullscreen, page_fullscreen_owner, zoomed_or_full};
// --- end auto-hide ---
pub(crate) use command_actions::{
    run_cycle_blocking_level, run_delete_browsing_data, run_periodic_reload, run_qr_code, run_stop_all_reloads, run_toggle_address_bar,
};

use windows::{
    Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::MessageBoxW,
    },
    core::w,
};

pub use window::{rail_activate, run};

pub fn show_runtime_error(error: &crate::engine::RuntimeUnavailable) {
    show_error_message(&format!("{error}"));
}

/// Unit tests must surface errors, not wait forever for a human to dismiss a native dialog.
#[cfg(test)]
pub fn show_error_message(message: &str) {
    panic!("Unexpected native error dialog in unit test: {message}");
}

#[cfg(not(test))]
pub fn show_error_message(message: &str) {
    let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = MessageBoxW(
            Some(HWND::default()),
            windows::core::PCWSTR(text.as_ptr()),
            w!("Bowser"),
            windows::Win32::UI::WindowsAndMessaging::MB_OK
                | windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
        );
    }
}

/// Native Yes/No question; `true` for Yes.
pub fn ask_yes_no(message: &str) -> bool {
    let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(
            Some(HWND::default()),
            windows::core::PCWSTR(text.as_ptr()),
            w!("Bowser"),
            windows::Win32::UI::WindowsAndMessaging::MB_YESNO
                | windows::Win32::UI::WindowsAndMessaging::MB_ICONQUESTION,
        ) == windows::Win32::UI::WindowsAndMessaging::IDYES
    }
}

/// Native Yes/No question owned by the browser window; `true` for Yes.
pub(crate) fn ask_yes_no_owned(owner: HWND, message: &str) -> bool {
    let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(
            Some(owner),
            windows::core::PCWSTR(text.as_ptr()),
            w!("Bowser"),
            windows::Win32::UI::WindowsAndMessaging::MB_YESNO
                | windows::Win32::UI::WindowsAndMessaging::MB_ICONQUESTION,
        ) == windows::Win32::UI::WindowsAndMessaging::IDYES
    }
}

/// The style of a question whose Yes destroys data: No is the default button, so Enter keeps it.
pub(crate) fn destructive_question_style() -> windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE {
    use windows::Win32::UI::WindowsAndMessaging::{MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO};
    MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2
}

/// Native Yes/No question owned by `owner` (modal to it), defaulting to No; `true` only for Yes.
pub(crate) fn ask_destructive_owned(owner: HWND, message: &str) -> bool {
    let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(Some(owner), windows::core::PCWSTR(text.as_ptr()), w!("Bowser"), destructive_question_style())
            == windows::Win32::UI::WindowsAndMessaging::IDYES
    }
}

#[cfg(test)]
#[test]
fn the_destructive_question_defaults_to_no() {
    use windows::Win32::UI::WindowsAndMessaging::{MB_DEFBUTTON2, MB_YESNO};
    let style = destructive_question_style();
    assert_eq!(style.0 & MB_DEFBUTTON2.0, MB_DEFBUTTON2.0, "No (the second button) is the default");
    assert_eq!(style.0 & 0xF, MB_YESNO.0, "Yes and No buttons");
    // The saved-password deletion asks this way, owned by its window (so it cannot fall behind
    // it), not with the unowned default-Yes `ask_yes_no`.
    let source = include_str!("protection.rs");
    let at = source.find("\"Delete every saved password").expect("the password question");
    let call = &source[source[..at].rfind("super::").unwrap()..at];
    assert_eq!(call, "super::ask_destructive_owned(owner, ", "the password question is owned and defaults to No");
}

pub(crate) const INPUT_ID: u16 = 100;
pub(crate) const WM_SUBMIT: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 10;
pub(crate) const WM_HISTORY_BACK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 11;
pub(crate) const WM_HISTORY_FORWARD: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 12;
pub(crate) const WM_NAVIGATION_PROGRESS: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 13;
/// Posted to the main window when tab state (title, favicon, loading, list) changed; WPARAM = tab id.
pub(crate) const WM_TABS_CHANGED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 14;
/// Profile workspace or sidebar configuration changed; rebuild the cached rail view.
pub(crate) const WM_RAIL_CHANGED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 19;
/// Forwarded page accelerators; WPARAM=virtual key, LPARAM=ctrl/shift/alt bit flags.
pub(crate) const WM_SHORTCUT_KEY: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 15;
/// Posted by a WebView2 controller when the user moves focus into a pane; WPARAM = tab id.
pub(crate) const WM_PANE_FOCUSED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 17;
/// A validated top-frame login submission was queued from a WebView2 callback.
pub(crate) const WM_LOGIN_MESSAGE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 18;
/// HTML chrome mode only: the tab engine's environment creation finished (with or without an
/// environment the chrome can use). The window then creates, or gives up on, its HTML chrome.
pub(crate) const WM_HTML_CHROME_ENV: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x7D;
// --- panel host (lane P) ---
/// HTML history view failed; native content factories are restored on the UI thread.
pub(crate) const WM_HTML_PANEL_FAILED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x82;
// --- end panel host ---
// --- split view ---
/// The split view's HTML host queued a command (`split_ui::run_queued` runs it).
pub(crate) const WM_SPLIT_COMMAND: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5B1;
/// A page asked to open a link in split view (the URL waits in `split_ui`'s queue).
pub(crate) const WM_SPLIT_LINK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5B2;
// --- end split view ---
/// The settings page's HTML host queued a command (`settings_page::run_queued` runs it).
pub(crate) const WM_SETTINGS_COMMAND: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5C1;
/// The history page's HTML host queued a command (`history_page::run_queued` runs it).
pub(crate) const WM_HISTORY_PAGE_COMMAND: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5C2;
/// The downloads page's HTML host queued a command (`downloads_page::run_queued` runs it).
pub(crate) const WM_DOWNLOADS_PAGE_COMMAND: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5C3;
/// The downloads list changed (posted from `App`'s downloads observer); `WPARAM` is 0 for a progress
/// tick and 1 for any other change (`downloads_ui::changed` throttles the first, never the second).
pub(crate) const WM_DOWNLOADS_CHANGED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5C4;
/// The installed extensions changed (installed, turned on or off, removed, or a load result came
/// in): the toolbar's extensions button and the settings page's Extensions section read them again.
pub(crate) const WM_EXTENSIONS_CHANGED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5C5;
/// A step of a running command chain is due (`command_actions::chain_step` runs it).
pub(crate) const WM_CHAIN_STEP: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5E1;
/// Trailing flush of a throttled progress update (`downloads_ui`).
pub(crate) const WM_DOWNLOADS_TIMER: usize = 0xB070;
/// Posted by the foreground WinEvent hook so the UI thread re-checks whether Bowser is in front.
pub(crate) const WM_FOREGROUND_SYNC: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 16;
pub(crate) const WM_IDLE_TRIM_TIMER: usize = 1;
pub(crate) const WM_CARET_BLINK_TIMER: usize = 2;
/// Runs only while a chrome transition is moving; it stops itself once the motion has settled.
pub(crate) const WM_ANIMATION_TIMER: usize = 3;
/// One-shot debounce for session snapshots; armed by a tab change, never repeating.
pub(crate) const WM_SESSION_SAVE_TIMER: usize = 4;
/// Debounces the small host-only privacy statistics file after blocked requests.
pub(crate) const WM_PRIVACY_STATS_TIMER: usize = 7;
/// Advances the address bar navigation progress transition while it is visible.
pub(crate) const WM_PROGRESS_TIMER: usize = 6;
/// Coalesces split divider moves to one controller bounds update per frame.
pub(crate) const WM_SPLIT_BOUNDS_TIMER: usize = 8;
/// Slow fallback that re-reads the power source for efficiency mode (Auto only).
pub(crate) const WM_POWER_TIMER: usize = 0xB060;
/// Delays the first-run import offer until after the first chrome paint.
pub(crate) const WM_IMPORT_OFFER_TIMER: usize = 0xB055;
/// Defers the active tab's reader probe until after the page's first paint.
pub(crate) const WM_READER_PROBE_TIMER: usize = 0xB073;
/// Google Translate answered a batch (posted from its worker thread); `WPARAM` is the job id and
/// `LPARAM` the request id whose result is waiting in `app::page_translate`.
pub(crate) const WM_TRANSLATE_NET: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5D1;
/// Fails a translation step that never answered.
pub(crate) const WM_TRANSLATE_WATCH_TIMER: usize = 0xB072;
/// A capture's join/encode/save worker finished; `WPARAM` is the worker id whose result is waiting in
/// `app::page_actions`.
pub(crate) const WM_CAPTURE_DONE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5D9;

// --- settings page (docs/design/settings-page.md) ---
/// Opens `bowser://settings` at `section` in window `hwnd` (the native Settings window when the
/// window has no HTML chrome). `reuse_blank`: a blank normal shown tab becomes the page.
pub(crate) fn open_settings_page(hwnd: HWND, app: &crate::app::AppRef, section: Option<crate::internal_page::Section>, reuse_blank: bool) {
    settings_page::open(hwnd, app, section, reuse_blank);
}

/// The shown settings tab moved to `section` (a restored or typed address).
pub(crate) fn settings_page_go(hwnd: HWND, section: Option<crate::internal_page::Section>) {
    settings_page::go(hwnd, section);
}

/// Reload on a settings tab: the page reads everything again.
pub(crate) fn settings_page_refresh(hwnd: HWND) {
    settings_page::refresh(hwnd);
}

/// Gives the keyboard to the settings page.
pub(crate) fn settings_page_focus(hwnd: HWND) {
    settings_page::focus(hwnd);
}
// --- end settings page ---

// --- history page (docs/design/history-page.md) ---
/// Opens `bowser://history` in window `hwnd` (the History side panel when the window has no HTML
/// chrome). `reuse_blank`: a blank normal shown tab becomes the page.
/// An `@bookmarks` / `@history` / `@tabs` nickname was submitted: open the scope's first result.
pub(crate) fn address_scoped_submit(hwnd: HWND, app: &crate::app::AppRef, scope: crate::address_settings::Scope, rest: &str) {
    address_bar::scoped_submit(hwnd, app, scope, rest);
}

pub(crate) fn open_history_page(hwnd: HWND, app: &crate::app::AppRef, reuse_blank: bool) {
    history_page::open(hwnd, app, reuse_blank);
}

/// Reload on a history tab: the page reads history again.
pub(crate) fn history_page_refresh(hwnd: HWND) {
    history_page::refresh(hwnd);
}

/// Gives the keyboard to the history page.
pub(crate) fn history_page_focus(hwnd: HWND) {
    history_page::focus(hwnd);
}
// --- end history page ---

// --- downloads (docs/design/downloads.md) ---
/// Opens `bowser://downloads` in window `hwnd` (a no-op without the HTML chrome, where downloads
/// still save and Explorer shows them). `reuse_blank`: a blank normal shown tab becomes the page.
pub(crate) fn open_downloads_page(hwnd: HWND, app: &crate::app::AppRef, reuse_blank: bool) {
    downloads_page::open(hwnd, app, reuse_blank);
}

/// Reload on a downloads tab: the page reads the list again.
pub(crate) fn downloads_page_refresh(hwnd: HWND) {
    downloads_page::refresh(hwnd);
}

/// Gives the keyboard to the downloads page.
pub(crate) fn downloads_page_focus(hwnd: HWND) {
    downloads_page::focus(hwnd);
}
// --- end downloads ---

/// Tells every Bowser window that the installed extensions changed (docs/extensions.md). Posted, so
/// it is safe from an engine callback; each window catches up from its own window procedure.
pub(crate) fn extensions_changed() {
    window::post_to_every_window(WM_EXTENSIONS_CHANGED);
}

// --- split view ---
/// Opens `url` beside the shown tab once the window procedure runs (link context menu).
pub(crate) fn queue_split_link(owner: HWND, url: url::Url) {
    split_ui::queue_link(owner, url);
}
// --- end split view ---

// --- tab stacks, tiling and workspaces (T10) ---
mod stack_ui;
pub(crate) use stack_ui::{run_stack_all_by_site, run_stack_site, run_tile_grid, run_tile_horizontal, run_tile_vertical, run_unstack, run_untile, run_next_workspace, run_previous_workspace};
/// The tile look changed ("Show a title bar on each tile", "Remove the space around tiles"): the
/// window lays the shown split out again once its procedure runs.
pub(crate) const WM_SPLIT_LOOK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5B3;
pub(crate) fn split_look_changed(hwnd: HWND) {
    post(hwnd, WM_SPLIT_LOOK, Default::default(), Default::default());
}
// --- end tab stacks, tiling and workspaces ---

// --- developer tools dock ---
/// Snapshot of every top-level window, taken right before `OpenDevToolsWindow` so the DevTools
/// window it creates can be found afterwards by diffing against it.
pub(crate) fn dev_tools_dock_snapshot() -> Vec<isize> {
    devtools_dock::snapshot_top_level()
}

/// True and closes `tab`'s docked DevTools pane (or cancels its pending discovery) when it was
/// open; false when there was nothing to close, so the caller opens a new one instead.
pub(crate) fn dev_tools_dock_close_if_open(hwnd: HWND, tab: crate::tabs::TabId) -> bool {
    devtools_dock::close_if_open(hwnd, tab)
}

/// Starts the search for the DevTools window `OpenDevToolsWindow` just asked WebView2's browser
/// process to create for `tab`, so it can be reparented into the dock once found.
pub(crate) fn dev_tools_dock_discover(hwnd: HWND, tab: crate::tabs::TabId, before: Vec<isize>) {
    devtools_dock::discover(hwnd, tab, before);
}

/// The tab closed: drops its tracked DevTools pane, closing it if it was open.
pub(crate) fn dev_tools_dock_tab_closed(hwnd: HWND, tab: crate::tabs::TabId) {
    devtools_dock::tab_closed(hwnd, tab);
}
// --- end developer tools dock ---

// --- latency (fix E) --- the blocker builds its lists on a worker thread.
/// The main window, once it exists: where a finished blocker build is announced.
static BLOCKER_WAKE: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Called by the blocker's build worker: asks the main window to swap the new rules in (the
/// window's `WM_SETTINGS_COMMAND` runs `protection::take_built`). Before the window exists the
/// build waits; [`set_blocker_wake`] collects it.
pub(crate) fn blocker_built_wake() {
    let raw = BLOCKER_WAKE.load(std::sync::atomic::Ordering::Acquire);
    if raw != 0 {
        post(HWND(raw as *mut _), WM_SETTINGS_COMMAND, Default::default(), Default::default());
    }
}

/// The main window exists: later builds are announced to it, and one that finished before it
/// existed is collected now.
pub(crate) fn set_blocker_wake(hwnd: HWND) {
    BLOCKER_WAKE.store(hwnd.0 as isize, std::sync::atomic::Ordering::Release);
    blocker_built_wake();
}
// --- end latency ---

pub(crate) fn post(
    hwnd: HWND,
    message: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
            Some(hwnd),
            message,
            wparam,
            lparam,
        );
    }
}

pub(crate) fn set_input_text(hwnd: HWND, value: &str) -> bool {
    let value: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        if windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(
            hwnd,
            windows::core::PCWSTR(value.as_ptr()),
        )
        .is_err()
        {
            return false;
        }
        let mut readback = vec![0u16; value.len()];
        let copied = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut readback);
        copied >= 0
            && copied as usize == value.len() - 1
            && readback[..copied as usize] == value[..value.len() - 1]
    }
}

// --- page tools (T7) ---
pub(crate) use page_tools_ui::{
    context_note_items, insert_note_into_page, run_capture_full, run_capture_full_copy, run_capture_full_note,
    run_capture_selection, run_capture_selection_copy, run_capture_selection_note, run_capture_visible,
    run_capture_visible_copy, run_capture_visible_note, run_note_append, run_note_from_selection,
};
// --- end page tools ---

#[cfg(test)]
mod error_dialog_tests {
    #[test]
    #[should_panic(expected = "Unexpected native error dialog in unit test: settings write failed")]
    fn unexpected_error_dialog_fails_instead_of_waiting_for_desktop_input() {
        super::show_error_message("settings write failed");
    }
}
