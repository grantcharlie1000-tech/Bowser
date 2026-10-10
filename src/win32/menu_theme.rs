//! Dark or light native popup menus that follow the chrome appearance.
//!
//! Win32 popup menus ignore the app's colours and draw light on every Windows 10 build, which
//! clashes with a dark chrome. `uxtheme` exposes an app-wide menu mode by ordinal only
//! (`SetPreferredAppMode` is 135, `FlushMenuThemes` is 136, both since 1903), so the entry points
//! are resolved at runtime and the call is a no-op wherever they are missing.

use std::sync::atomic::{AtomicU8, Ordering};

use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        System::LibraryLoader::{GetProcAddress, LoadLibraryW},
        UI::WindowsAndMessaging::{HMENU, TRACK_POPUP_MENU_FLAGS, TrackPopupMenu},
    },
    core::{BOOL, PCSTR, w},
};

use crate::theme::Appearance;

const FORCE_DARK: i32 = 2;
const FORCE_LIGHT: i32 = 3;
const UNSET: u8 = 0;
const APPLIED_LIGHT: u8 = 1;
const APPLIED_DARK: u8 = 2;

static APPLIED: AtomicU8 = AtomicU8::new(UNSET);

/// Makes the next popup menu match `appearance`. Cheap when nothing changed.
pub(crate) fn apply(appearance: Appearance) {
    let wanted = if appearance == Appearance::Dark {
        APPLIED_DARK
    } else {
        APPLIED_LIGHT
    };
    if APPLIED.swap(wanted, Ordering::Relaxed) == wanted {
        return;
    }
    unsafe {
        let Ok(module) = LoadLibraryW(w!("uxtheme.dll")) else {
            return;
        };
        let set_mode = GetProcAddress(module, PCSTR(135 as *const u8));
        let flush = GetProcAddress(module, PCSTR(136 as *const u8));
        let (Some(set_mode), Some(flush)) = (set_mode, flush) else {
            return;
        };
        let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set_mode);
        let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
        set_mode(if wanted == APPLIED_DARK {
            FORCE_DARK
        } else {
            FORCE_LIGHT
        });
        flush();
    }
}

/// `TrackPopupMenu` after matching the menu appearance to the chrome.
///
/// # Safety
/// Same contract as `TrackPopupMenu`.
pub(crate) unsafe fn track(
    menu: HMENU,
    flags: TRACK_POPUP_MENU_FLAGS,
    x: i32,
    y: i32,
    hwnd: HWND,
    exclude: Option<*const RECT>,
) -> BOOL {
    apply(super::glass::appearance());
    unsafe { TrackPopupMenu(menu, flags, x, y, None, hwnd, exclude) }
}
