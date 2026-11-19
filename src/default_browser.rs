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
const CLIENT: &str = r"Software\Clients\StartMenuInternet\Bowser";
const REGISTERED_APPLICATIONS: &str = r"Software\RegisteredApplications";
const PROG_ID_COMMAND: &str = r"Software\Classes\Bowser.HTML\shell\open\command";

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
/// which [`crate::performance::LaunchOptions`] opens as the initial address. Windows pastes `%1` in
/// unescaped, so a `"` in a link can split it into more arguments; the parser therefore treats
/// every argument after the address as data and refuses any flag there (SEC-2).
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

    let prog_id_key = format!(r"Software\Classes\{PROG_ID}");
    write_key_value(&prog_id_key, "", "Bowser HTML Document")?;
    write_key_value(&format!(r"{prog_id_key}\DefaultIcon"), "", &icon)?;
    write_key_value(PROG_ID_COMMAND, "", &open_command)?;
    write_key_value(REGISTERED_APPLICATIONS, "Bowser", &format!(r"{CLIENT}\Capabilities"))?;

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

/// Windows Settings' Default apps page, opened on Bowser's own entry (Windows 11 reads
/// `registeredAppUser`: the `RegisteredApplications` value name `register_hkcu` writes, under HKCU).
/// Windows 10 ignores the query and shows the Default apps page, where Web browser is listed.
const DEFAULT_APPS_PAGE: PCWSTR = w!("ms-settings:defaultapps?registeredAppUser=Bowser");

/// Registers Bowser, then takes the user to Windows' Default apps settings for it, where one click
/// makes it the default. `LaunchAdvancedAssociationUI` alone is not enough: on Windows 10 and 11 it
/// returns success without showing anything, so it is only the fallback when Settings cannot open.
/// One user-facing error message on failure, matching the settings page's existing ack/error wire.
pub fn make_default(exe: &Path) -> Result<(), String> {
    register_hkcu(exe)
        .map_err(|error| format!("Bowser could not be registered as a browser: {error}"))?;
    if open_default_apps_settings() {
        return Ok(());
    }
    launch_advanced_association_ui()
        .map_err(|error| format!("Windows' default apps settings could not be opened: {error}"))
}

/// Opens Windows Settings at Bowser's Default apps page. False when the shell refused.
fn open_default_apps_settings() -> bool {
    use windows::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    // ShellExecuteW reports failure with a value of 32 or less.
    let result = unsafe { ShellExecuteW(None, w!("open"), DEFAULT_APPS_PAGE, None, None, SW_SHOWNORMAL) };
    (result.0 as isize) > 32
}

/// Checks the HTTP choice and this copy's registration. A stale choice must leave the Settings
/// action available so it can repair the registration.
pub fn is_http_default() -> Option<bool> {
    let choice = current_http_handler()?;
    Some(
        choice.eq_ignore_ascii_case(PROG_ID)
            && current_exe().is_some_and(|exe| registration_matches(&exe, registry_string)),
    )
}

fn registration_matches(exe: &Path, read: impl Fn(&str, &str) -> Option<String>) -> bool {
    let capabilities = format!(r"{CLIENT}\Capabilities");
    let url_associations = format!(r"{capabilities}\URLAssociations");
    let client_command = format!(r"{CLIENT}\shell\open\command");
    let command = open_command(&exe.to_string_lossy());
    [
        (REGISTERED_APPLICATIONS, "Bowser", capabilities.as_str()),
        (&url_associations, "http", PROG_ID),
        (&url_associations, "https", PROG_ID),
        (&client_command, "", command.as_str()),
        (PROG_ID_COMMAND, "", command.as_str()),
    ]
    .into_iter()
    .all(|(path, name, expected)| {
        read(path, name).is_some_and(|value| value.eq_ignore_ascii_case(expected))
    })
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
    let value = if name.is_empty() { PCWSTR::null() } else { PCWSTR(name_wide.as_ptr()) };
    let mut size = 0;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(path_wide.as_ptr()),
            value,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(path_wide.as_ptr()),
            value,
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
    fn missing_or_stale_registration_leaves_default_action_available() {
        let exe = Path::new(r"C:\Programs\Bowser\Bowser.exe");
        let read = |path: &str, name: &str| match name {
            "Bowser" => Some(format!(r"{CLIENT}\Capabilities")),
            "http" | "https" => Some(PROG_ID.to_owned()),
            "" if path.ends_with(r"\shell\open\command") => {
                Some(open_command(&exe.to_string_lossy()))
            }
            _ => None,
        };
        assert!(registration_matches(exe, read));
        assert!(!registration_matches(exe, |path, name| {
            (path != REGISTERED_APPLICATIONS).then(|| read(path, name)).flatten()
        }));
        assert!(!registration_matches(exe, |path, name| {
            if path == PROG_ID_COMMAND {
                return Some(open_command(r"C:\Programs\Bowser Stable 2\Bowser.exe"));
            }
            read(path, name)
        }));
    }

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
