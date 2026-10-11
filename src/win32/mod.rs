mod card;
mod main_menu;
mod settings_panel;
pub mod frame;
pub mod glass;
mod menu_theme;
pub mod passwords_import;
mod passwords_panel;
mod smooth;
mod window;

pub(crate) use window::schedule_idle_timer;

use windows::{
    Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
    },
    core::w,
};

pub use window::run;

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

pub(crate) const INPUT_ID: u16 = 100;
pub(crate) const WM_SUBMIT: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 10;
pub(crate) const WM_HISTORY_BACK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 11;
pub(crate) const WM_HISTORY_FORWARD: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 12;
pub(crate) const WM_NAVIGATION_PROGRESS: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 13;
/// Posted to the main window when tab state (title, favicon, loading, list) changed; WPARAM = tab id.
pub(crate) const WM_TABS_CHANGED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 14;
/// Forwarded page accelerators; WPARAM=virtual key, LPARAM=ctrl/shift/alt bit flags.
pub(crate) const WM_SHORTCUT_KEY: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 15;
/// Posted by the foreground WinEvent hook so the UI thread re-checks whether Bowser is in front.
pub(crate) const WM_FOREGROUND_SYNC: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 16;
pub(crate) const WM_IDLE_TRIM_TIMER: usize = 1;
pub(crate) const WM_CARET_BLINK_TIMER: usize = 2;
/// Runs only while a chrome transition is moving; it stops itself once the motion has settled.
pub(crate) const WM_ANIMATION_TIMER: usize = 3;
/// One-shot debounce for session snapshots; armed by a tab change, never repeating.
pub(crate) const WM_SESSION_SAVE_TIMER: usize = 4;
/// One-shot hover delay for the tab card; armed when the pointer settles on an inactive tab.
pub(crate) const WM_CARD_HOVER_TIMER: usize = 5;
/// Advances the address bar navigation progress transition while it is visible.
pub(crate) const WM_PROGRESS_TIMER: usize = 6;

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
