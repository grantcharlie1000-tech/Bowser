mod card;
mod corner_fringe;
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
// --- tab options --- the Sessions panel.
mod sessions_panel;
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
// --- end split view ---
mod tab_ghost; // the native ghost of a held tab (docs/design/tab-drag.md)
mod window;

pub(crate) use window::{
    gated_chords, refresh_edit_font, schedule_idle_timer, schedule_power_timer, set_link_timer, set_tab_tick, set_thumbnail_timer,
    show_pending_address, sync_address_input,
};

use windows::{
    Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
    },
    core::w,
};

pub use window::{rail_activate, run};

pub fn show_runtime_error(error: &crate::engine::RuntimeUnavailable) {
    show_error_message(&format!("{error}"));
}

pub fn show_error_message(message: &str) {
    let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = MessageBoxW(
            Some(HWND::default()),
            windows::core::PCWSTR(text.as_ptr()),
            w!("Bowser"),
            MB_OK | MB_ICONERROR,
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
pub(crate) const WM_SPLIT_BOUNDS_TIMER: usize = 7;
/// Slow fallback that re-reads the power source for efficiency mode (Auto only).
pub(crate) const WM_POWER_TIMER: usize = 0xB060;
/// Delays the first-run import offer until after the first chrome paint.
pub(crate) const WM_IMPORT_OFFER_TIMER: usize = 0xB055;
/// Defers the active tab's reader probe until after the page's first paint.
pub(crate) const WM_READER_PROBE_TIMER: usize = 0xB05E;
/// Google Translate answered a batch (posted from its worker thread); `WPARAM` is the job id and
/// `LPARAM` the request id whose result is waiting in `app::page_translate`.
pub(crate) const WM_TRANSLATE_NET: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5D1;
/// Fails a translation step that never answered.
pub(crate) const WM_TRANSLATE_WATCH_TIMER: usize = 0xB072;

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
