mod card;
mod corner_fringe;
mod command_palette;
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
mod panel_bookmarks;
mod panel_customize;
mod panel_history;
// --- panel host (lane P) ---
pub(crate) use panel_history::{
    Action as HistoryAction, Line as HistoryLine, Model as HistoryModel, Range as HistoryRange,
    Row as HistoryRow, LIMIT as HISTORY_LIMIT, current_history_file, execute as execute_history_action,
    local_offset as history_local_offset, now_secs as history_now_secs,
};
mod html_panel;
mod html_dialog;
// --- end panel host ---
mod panel_widgets;
pub mod panel_webapp;
pub mod passwords_import;
mod passwords_panel;
pub(crate) mod privacy_panel;
mod profile_rename;
mod settings_panel;
pub mod side_panel;
mod site_info;
mod smooth;
// --- split view ---
mod split_ui;
// --- end split view ---
mod window;

pub(crate) use window::{
    schedule_idle_timer, schedule_power_timer, show_pending_address, sync_address_input,
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

// --- split view ---
/// Opens `url` beside the shown tab once the window procedure runs (link context menu).
pub(crate) fn queue_split_link(owner: HWND, url: url::Url) {
    split_ui::queue_link(owner, url);
}
// --- end split view ---

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
