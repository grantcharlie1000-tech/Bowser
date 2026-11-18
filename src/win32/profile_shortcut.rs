//! Desktop shortcuts that open one profile ("Create desktop shortcut" in the profile menu and on
//! the settings page). The shortcut runs this executable with `--profile <id>` and carries the
//! profile's taskbar identity, so a shortcut pinned to the taskbar is the same button the
//! profile's windows use.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IPersistFile, STGM_READ};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{FOLDERID_Desktop, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath, ShellLink};
use windows::core::{GUID, HSTRING, Interface, PCWSTR, PWSTR};

use crate::profile::Profile;

/// `System.AppUserModel.ID` (propkey.h).
const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 5,
};

/// `Bowser - <name>.lnk`, made a safe file name the way downloads are (no characters Windows
/// refuses, no direction overrides or other invisible characters, a length Windows accepts).
pub(crate) fn shortcut_file_name(display_name: &str) -> String {
    crate::downloads::sanitize_file_name(&format!("Bowser - {}.lnk", display_name.replace(['/', '\\'], "_")))
}

/// The command-line arguments a shortcut for `profile` runs with.
pub(crate) fn shortcut_arguments(profile: &Profile, data_root: Option<&Path>) -> String {
    let mut args = String::new();
    if let Some(root) = data_root {
        // Quoted for CommandLineToArgvW; a Windows path cannot contain a double quote.
        args.push_str(&format!("--data-dir \"{}\" ", root.display()));
    }
    args.push_str("--profile ");
    args.push_str(profile.listed_name());
    args
}

fn desktop_dir() -> Option<PathBuf> {
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None).ok()?;
        let text = path.to_string().ok();
        CoTaskMemFree(Some(path.0 as *const _));
        text.map(PathBuf::from).filter(|dir| dir.is_absolute())
    }
}

/// Writes `Bowser - <display name>.lnk` to the desktop, replacing an older one of that name.
pub(crate) fn create_desktop_shortcut(profile: &Profile) -> Result<(), String> {
    if profile.is_guest() {
        return Err("Guest windows cannot have a shortcut.".to_owned());
    }
    let exe = std::env::current_exe().map_err(|_| "Bowser could not find its own program file.".to_owned())?;
    let desktop = desktop_dir().ok_or_else(|| "Bowser could not find your desktop folder.".to_owned())?;
    let file = desktop.join(shortcut_file_name(&profile.display_name()));
    let arguments = shortcut_arguments(profile, crate::profile::data_root_override());
    let description = format!("Open Bowser with the {} profile", profile.display_name());
    write_link(&exe, &arguments, &description, profile.app_user_model_id().as_deref(), &file)
        .map_err(|error| format!("Windows refused to save it ({error})."))
}

pub(crate) fn write_link(exe: &Path, arguments: &str, description: &str, app_id: Option<&str>, file: &Path) -> windows::core::Result<()> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(exe.as_os_str()))?;
        link.SetArguments(&HSTRING::from(arguments))?;
        link.SetDescription(&HSTRING::from(description))?;
        link.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0)?;
        if let Some(dir) = exe.parent() {
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
        }
        if let Some(app_id) = app_id {
            let store: IPropertyStore = link.cast()?;
            let mut wide: Vec<u16> = app_id.encode_utf16().chain(std::iter::once(0)).collect();
            // SetValue copies the string; `wide` outlives the call and is freed by Rust.
            let value = PROPVARIANT {
                Anonymous: PROPVARIANT_0 {
                    Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                        vt: VT_LPWSTR,
                        Anonymous: PROPVARIANT_0_0_0 { pwszVal: PWSTR(wide.as_mut_ptr()) },
                        ..Default::default()
                    }),
                },
            };
            store.SetValue(&PKEY_APP_USER_MODEL_ID, &value)?;
            store.Commit()?;
        }
        let persist: IPersistFile = link.cast()?;
        persist.Save(&HSTRING::from(file.as_os_str()), true)
    }
}

/// Removes the desktop shortcut for a deleted profile, but only a `.lnk` that is Bowser's own:
/// it must start this executable with exactly the arguments Bowser writes for that profile.
pub(crate) fn remove_desktop_shortcut(profile: &Profile, display_name: &str) {
    let (Ok(exe), Some(desktop)) = (std::env::current_exe(), desktop_dir()) else { return };
    let file = desktop.join(shortcut_file_name(display_name));
    if !file.is_file() {
        return;
    }
    let wanted = shortcut_arguments(profile, crate::profile::data_root_override());
    if read_link(&file).is_ok_and(|(path, arguments)| arguments == wanted && path.eq_ignore_ascii_case(&exe.to_string_lossy())) {
        let _ = std::fs::remove_file(file);
    }
}

fn read_link(file: &Path) -> windows::core::Result<(String, String)> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let persist: IPersistFile = link.cast()?;
        persist.Load(PCWSTR(HSTRING::from(file.as_os_str()).as_ptr()), STGM_READ)?;
        let mut path = [0u16; 1024];
        link.GetPath(&mut path, std::ptr::null_mut(), 0)?;
        let mut arguments = [0u16; 1024];
        link.GetArguments(&mut arguments)?;
        let text = |buffer: &[u16]| {
            let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
            String::from_utf16_lossy(&buffer[..end])
        };
        Ok((text(&path), text(&arguments)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_names_are_safe_file_names() {
        assert_eq!(shortcut_file_name("Work"), "Bowser - Work.lnk");
        assert_eq!(shortcut_file_name("a/b:c?"), "Bowser - a_b_c_.lnk");
        assert_eq!(shortcut_file_name("a\\b"), "Bowser - a_b.lnk");
        assert_eq!(shortcut_file_name("Café ☕"), "Bowser - Café ☕.lnk");
        assert!(!shortcut_file_name("evil\u{202e}knl.exe").contains('\u{202e}'), "no direction override");
    }

    #[test]
    fn shortcut_arguments_name_the_profile_and_keep_a_custom_data_root() {
        let app = Path::new(r"C:\data\Bowser");
        let work = Profile::named(app, "Work").unwrap();
        assert_eq!(shortcut_arguments(&work, None), "--profile Work");
        assert_eq!(shortcut_arguments(&Profile::default_in(app), None), "--profile default");
        assert_eq!(
            shortcut_arguments(&work, Some(Path::new(r"D:\my data"))),
            "--data-dir \"D:\\my data\" --profile Work"
        );
    }
}
