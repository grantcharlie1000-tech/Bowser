//! "Make Bowser the default browser" (docs/design/settings-page.md): registers Bowser as a
//! candidate under `HKCU` only (no admin, no `HKLM`), then asks Windows to show its own defaults
//! UI so the user picks it there. Also reports whether Bowser is the current HTTP default, for the
//! settings page's read-only status row.
//!
//! This is an action the user takes, not a silent hijack: registering never itself makes Bowser the
//! default, it only makes Bowser an option Windows' own dialog (and Settings > Default apps) can
//! offer. Only `LaunchAdvancedAssociationUI` can change the live default (Windows 8+ blocks a
//! program from setting its own default directly).

use std::path::Path;

use windows::{
    Win32::{
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
            Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
                RegCreateKeyExW, RegSetValueExW,
            },
        },
        UI::Shell::{ApplicationAssociationRegistrationUI, IApplicationAssociationRegistrationUI},
    },
    core::{PCWSTR, w},
};

/// The `StartMenuInternet` registration name and the ProgID for Bowser's HTML handler.
const APP_NAME: PCWSTR = w!("Bowser");
const PROG_ID: &str = "Bowser.HTML";

fn to_wide_z(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Creates (or opens) `HKEY_CURRENT_USER\path` with write access.
fn create_key(path: &str) -> windows::core::Result<HKEY> {
    let wide = to_wide_z(path);
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(wide.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
        .ok()?;
    }
    Ok(key)
}

/// Sets `key`'s named value (or its default value, when `name` is empty) to the string `data`.
fn set_string(key: HKEY, name: &str, data: &str) -> windows::core::Result<()> {
    let name_wide = to_wide_z(name);
    let data_wide = to_wide_z(data);
    let bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(data_wide.as_ptr().cast::<u8>(), data_wide.len() * 2)
    };
    unsafe {
        RegSetValueExW(
            key,
            if name.is_empty() { PCWSTR::null() } else { PCWSTR(name_wide.as_ptr()) },
            None,
            REG_SZ,
            Some(bytes),
        )
        .ok()
    }
}

fn write_key_value(path: &str, name: &str, data: &str) -> windows::core::Result<()> {
    let key = create_key(path)?;
    let result = set_string(key, name, data);
    unsafe {
        let _ = RegCloseKey(key);
    }
    result
}

/// The `shell\open\command` Windows runs for a link or file: `%1` becomes one bare argument,
/// which [`crate::performance::LaunchOptions`] opens as the initial address.
pub(crate) fn open_command(exe: &str) -> String {
    format!("\"{exe}\" \"%1\"")
}

/// Registers Bowser under `HKEY_CURRENT_USER` as a candidate browser: the `StartMenuInternet`
/// entry, its capabilities, the `Bowser.HTML` ProgID that actually launches it, and the
/// `RegisteredApplications` pointer Windows looks for. Every write is HKCU; nothing needs admin.
pub fn register_hkcu(exe: &Path) -> windows::core::Result<()> {
    let exe = exe.to_string_lossy();
    let open_command = open_command(&exe);
    let icon = format!("{exe},0");

    const CLIENT: &str = r"Software\Clients\StartMenuInternet\Bowser";
    write_key_value(CLIENT, "", "Bowser")?;
    write_key_value(&format!(r"{CLIENT}\Capabilities"), "ApplicationName", "Bowser")?;
    write_key_value(&format!(r"{CLIENT}\Capabilities"), "ApplicationIcon", &icon)?;
    write_key_value(
        &format!(r"{CLIENT}\Capabilities"),
        "ApplicationDescription",
        "A minimal native Windows browser.",
    )?;
    write_key_value(&format!(r"{CLIENT}\Capabilities\URLAssociations"), "http", PROG_ID)?;
    write_key_value(&format!(r"{CLIENT}\Capabilities\URLAssociations"), "https", PROG_ID)?;
    write_key_value(&format!(r"{CLIENT}\Capabilities\FileAssociations"), ".htm", PROG_ID)?;
    write_key_value(&format!(r"{CLIENT}\Capabilities\FileAssociations"), ".html", PROG_ID)?;
    write_key_value(&format!(r"{CLIENT}\shell\open\command"), "", &open_command)?;

    write_key_value("Software\\RegisteredApplications", "Bowser", &format!(r"{CLIENT}\Capabilities"))?;

    let prog_id_key = format!(r"Software\Classes\{PROG_ID}");
    write_key_value(&prog_id_key, "", "Bowser HTML Document")?;
    write_key_value(&format!(r"{prog_id_key}\DefaultIcon"), "", &icon)?;
    write_key_value(&format!(r"{prog_id_key}\shell\open\command"), "", &open_command)?;

    Ok(())
}

/// Shows Windows' own "Choose default apps" / defaults picker, scoped to Bowser's registration.
/// Only this (not a registry write) can make Bowser the live default: Windows 8+ requires the user
/// to pick it there.
pub fn launch_advanced_association_ui() -> windows::core::Result<()> {
    unsafe {
        let registration: IApplicationAssociationRegistrationUI =
            CoCreateInstance(&ApplicationAssociationRegistrationUI, None, CLSCTX_INPROC_SERVER)?;
        registration.LaunchAdvancedAssociationUI(APP_NAME)
    }
}

/// Registers Bowser, then opens the Windows defaults UI. One user-facing error message on failure,
/// matching the settings page's existing ack/error wire.
pub fn make_default(exe: &Path) -> Result<(), String> {
    register_hkcu(exe)
        .map_err(|error| format!("Bowser could not be registered as a browser: {error}"))?;
    launch_advanced_association_ui()
        .map_err(|error| format!("Windows' default apps dialog could not be opened: {error}"))
}

/// Reads the `UserChoice` Windows has recorded for the `http` URL association, for the settings
/// page's read-only status. `Some(true)`/`Some(false)` when it could be read, `None` otherwise (for
/// example on a build where the key does not exist yet).
pub fn is_http_default() -> Option<bool> {
    current_http_handler().map(|prog_id| prog_id.eq_ignore_ascii_case(PROG_ID))
}

fn current_http_handler() -> Option<String> {
    const USER_CHOICE: &str =
        r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\http\UserChoice";
    registry_string(USER_CHOICE, "ProgId")
}

fn registry_string(path: &str, name: &str) -> Option<String> {
    use windows::Win32::System::Registry::{RRF_RT_REG_SZ, RegGetValueW};
    let path_wide = to_wide_z(path);
    let name_wide = to_wide_z(name);
    let mut buffer = [0u16; 260];
    let mut size = (buffer.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(path_wide.as_ptr()),
            PCWSTR(name_wide.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status.is_err() {
        return None;
    }
    let chars = (size as usize / 2).saturating_sub(1).min(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..chars]))
}

/// The running executable's path, for registration. `None` when it cannot be determined (the
/// caller then refuses rather than registering a wrong or empty command line).
pub fn current_exe() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prog_id_matching_is_case_insensitive() {
        assert!("Bowser.HTML".eq_ignore_ascii_case(PROG_ID));
        assert!("bowser.html".eq_ignore_ascii_case(PROG_ID));
        assert!(!"Chrome.HTML".eq_ignore_ascii_case(PROG_ID));
    }

    #[test]
    fn current_exe_resolves_to_a_real_path() {
        let exe = current_exe().expect("current exe");
        assert!(exe.is_absolute());
    }
}
