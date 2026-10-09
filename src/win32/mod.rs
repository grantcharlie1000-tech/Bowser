mod window;

use windows::{
    Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
    },
    core::w,
};

pub use window::{dpi_scale, run};

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

pub(crate) const INPUT_ID: u16 = 100;
pub(crate) const WM_SUBMIT: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 10;
pub(crate) const WM_HISTORY_BACK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 11;
pub(crate) const WM_HISTORY_FORWARD: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 12;

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
