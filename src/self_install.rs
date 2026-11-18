//! First run of a downloaded `Bowser.exe`: put it where the updater looks after it, then start that
//! copy.
//!
//! The updater only replaces a copy that runs from `%LOCALAPPDATA%\Programs\Bowser` or a
//! `Bowser Stable[ N]` folder there ([`crate::updater::installed_location`]). A release build opened
//! from anywhere else (the Downloads folder, a USB stick) would otherwise never update. So before
//! anything else starts, such a launch:
//!
//! 1. uses an installed copy that is this build or newer, if there is one;
//! 2. otherwise writes this executable into the first install folder it can replace (an open copy
//!    cannot be replaced, so the next `Bowser Stable N` folder is used, as `Open-Bowser.ps1` does);
//! 3. points the Desktop and Start menu `Bowser` shortcuts at that copy;
//! 4. starts it with the same arguments, and this process exits.
//!
//! Any failure leaves things as they were and Bowser simply runs from where it was opened.
//! Only release builds do this ([`RELEASE_BUILD`], set by the release workflow); `cargo build`
//! copies, `--dev`, benchmark, agent and custom `--data-dir` launches never install themselves.
//! Profile data is not touched: it lives in `%LOCALAPPDATA%\Bowser` whichever copy runs.

use std::path::{Path, PathBuf};

use crate::updater::{Version, installed_location, sha256_file};

/// True only for builds made by the release workflow, which sets `BOWSER_RELEASE_BUILD=1`.
pub const RELEASE_BUILD: bool = option_env!("BOWSER_RELEASE_BUILD").is_some();

/// The install folders, in the order `Open-Bowser.ps1` and the updater use them.
pub fn candidate_dirs(programs: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![programs.join("Bowser"), programs.join("Bowser Stable")];
    dirs.extend((2..=8).map(|n| programs.join(format!("Bowser Stable {n}"))));
    dirs
}

/// What one install folder holds, compared with the running executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// No `Bowser.exe` there.
    Empty,
    /// Byte-for-byte this build.
    Same,
    /// Another build. `None` when it carries no readable version (builds before 0.0.36 have none).
    Other(Option<Version>),
}

/// What to do, given the slots in candidate order.
#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    /// Start the copy in this folder as it is: it is this build, or a newer one the updater keeps.
    Use(usize),
    /// Try writing this build into these folders, in order, until one accepts it. A folder whose
    /// copy is open refuses the replacement, so the next one is tried.
    Install(Vec<usize>),
}

/// Decides between using an installed copy and installing this one. Never downgrades: an installed
/// copy that is newer than `running` is used rather than replaced. Pure.
pub fn plan(slots: &[Slot], running: Version) -> Plan {
    if let Some(i) = slots.iter().position(|slot| *slot == Slot::Same) {
        return Plan::Use(i);
    }
    let newest_newer = slots
        .iter()
        .enumerate()
        .filter_map(|(i, slot)| match slot {
            Slot::Other(Some(version)) if *version >= running => Some((i, *version)),
            _ => None,
        })
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)));
    if let Some((i, _)) = newest_newer {
        return Plan::Use(i);
    }
    Plan::Install((0..slots.len()).collect())
}

/// Whether this launch should consider installing itself at all.
pub fn wanted(dev: bool, benchmark: bool, custom_data_dir: bool) -> bool {
    RELEASE_BUILD && !dev && !benchmark && !custom_data_dir
}

/// Runs the first-run install. Returns true when the installed copy was started and this process
/// should exit; false means carry on running this executable.
pub fn run() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).filter(|p| p.is_absolute()) else {
        return false;
    };
    if installed_location(&exe, Some(&local)) {
        return false;
    }
    let Ok((own_hash, own_len)) = sha256_file(&exe) else {
        return false;
    };
    let dirs = candidate_dirs(&local.join("Programs"));
    let slots: Vec<Slot> = dirs
        .iter()
        .map(|dir| slot_of(&dir.join("Bowser.exe"), own_hash, own_len))
        .collect();
    let target = match plan(&slots, Version::current()) {
        Plan::Use(i) => dirs[i].join("Bowser.exe"),
        Plan::Install(order) => {
            let Ok(bytes) = std::fs::read(&exe) else {
                return false;
            };
            if bytes.len() as u64 != own_len || crate::updater::sha256(&bytes).ok() != Some(own_hash) {
                return false;
            }
            let Some(installed) = order
                .into_iter()
                .find_map(|i| install_into(&dirs[i], &bytes, own_hash))
            else {
                return false;
            };
            installed
        }
    };
    win::write_shortcuts(&target);
    let Some(dir) = target.parent() else {
        return false;
    };
    std::process::Command::new(&target)
        .args(std::env::args_os().skip(1))
        .current_dir(dir)
        .spawn()
        .is_ok()
}

fn slot_of(exe: &Path, own_hash: [u8; 32], own_len: u64) -> Slot {
    let Ok(meta) = std::fs::metadata(exe) else {
        return Slot::Empty;
    };
    if !meta.is_file() {
        return Slot::Empty;
    }
    if meta.len() == own_len && sha256_file(exe).is_ok_and(|(hash, _)| hash == own_hash) {
        return Slot::Same;
    }
    Slot::Other(win::file_version(exe))
}

/// Writes `bytes` to `<dir>\Bowser.exe` through a staged file and one rename, so a copy that is
/// open (and so cannot be replaced) is left exactly as it was. The bytes are written fresh rather
/// than copied, so the download's mark of the web does not follow the installed file.
fn install_into(dir: &Path, bytes: &[u8], hash: [u8; 32]) -> Option<PathBuf> {
    let target = dir.join("Bowser.exe");
    let staged = dir.join("Bowser.exe.download");
    std::fs::create_dir_all(dir).ok()?;
    if std::fs::write(&staged, bytes).is_err() {
        let _ = std::fs::remove_file(&staged);
        return None;
    }
    if std::fs::rename(&staged, &target).is_err() {
        let _ = std::fs::remove_file(&staged);
        return None;
    }
    sha256_file(&target)
        .is_ok_and(|(written, _)| written == hash)
        .then_some(target)
}

mod win {
    use std::path::{Path, PathBuf};

    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    };
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize};
    use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    use windows::core::{GUID, HSTRING, w};

    use crate::updater::Version;

    /// The `FILEVERSION` of an executable as major.minor.patch, if it has one.
    pub(super) fn file_version(exe: &Path) -> Option<Version> {
        let path = HSTRING::from(exe.as_os_str());
        unsafe {
            let size = GetFileVersionInfoSizeW(&path, None);
            if size == 0 {
                return None;
            }
            let mut data = vec![0u8; size as usize];
            GetFileVersionInfoW(&path, None, size, data.as_mut_ptr().cast()).ok()?;
            let mut info: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut len = 0u32;
            if !VerQueryValueW(data.as_ptr().cast(), w!("\\"), &mut info, &mut len).as_bool()
                || info.is_null()
                || (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>()
            {
                return None;
            }
            let fixed = &*(info as *const VS_FIXEDFILEINFO);
            if fixed.dwSignature != 0xFEEF_04BD {
                return None;
            }
            Some(Version(
                fixed.dwFileVersionMS >> 16,
                fixed.dwFileVersionMS & 0xffff,
                fixed.dwFileVersionLS >> 16,
            ))
        }
    }

    fn known_folder(id: &GUID) -> Option<PathBuf> {
        unsafe {
            let path = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
            let text = path.to_string().ok();
            CoTaskMemFree(Some(path.0 as *const _));
            text.map(PathBuf::from).filter(|dir| dir.is_absolute())
        }
    }

    /// Desktop `Bowser.lnk` and Start menu `Bowser\Bowser.lnk`, the same two `Open-Bowser.ps1`
    /// makes, pointed at `exe`. Best effort: a missing shortcut never stops Bowser from opening.
    pub(super) fn write_shortcuts(exe: &Path) {
        let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        let mut files = Vec::new();
        if let Some(desktop) = known_folder(&FOLDERID_Desktop) {
            files.push(desktop.join("Bowser.lnk"));
        }
        if let Some(programs) = known_folder(&FOLDERID_Programs) {
            let folder = programs.join("Bowser");
            if std::fs::create_dir_all(&folder).is_ok() {
                files.push(folder.join("Bowser.lnk"));
            }
        }
        for file in files {
            let _ = crate::win32::profile_shortcut::write_link(exe, "", "Open Bowser", None, &file);
        }
        if com {
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V35: Version = Version(0, 0, 35);

    #[test]
    fn candidates_match_the_installer_and_the_updater() {
        let programs = Path::new(r"C:\Users\me\AppData\Local\Programs");
        let dirs = candidate_dirs(programs);
        assert_eq!(dirs.len(), 9);
        assert_eq!(dirs[0], programs.join("Bowser"));
        assert_eq!(dirs[1], programs.join("Bowser Stable"));
        assert_eq!(dirs[8], programs.join("Bowser Stable 8"));
        let local = Path::new(r"C:\Users\me\AppData\Local");
        for dir in dirs {
            assert!(installed_location(&dir.join("Bowser.exe"), Some(local)), "{}", dir.display());
        }
    }

    #[test]
    fn nothing_installed_installs_into_the_first_folder_first() {
        assert_eq!(plan(&[Slot::Empty; 9], V35), Plan::Install((0..9).collect()));
    }

    #[test]
    fn the_same_build_already_installed_is_used() {
        let mut slots = [Slot::Empty; 9];
        slots[2] = Slot::Same;
        slots[0] = Slot::Other(None);
        assert_eq!(plan(&slots, V35), Plan::Use(2));
    }

    #[test]
    fn a_newer_installed_copy_is_never_downgraded() {
        let mut slots = [Slot::Empty; 9];
        slots[0] = Slot::Other(Some(Version(0, 0, 36)));
        assert_eq!(plan(&slots, V35), Plan::Use(0));
        slots[1] = Slot::Other(Some(Version(0, 1, 0)));
        assert_eq!(plan(&slots, V35), Plan::Use(1), "the newest copy wins");
    }

    #[test]
    fn older_or_unversioned_copies_are_replaced() {
        let mut slots = [Slot::Empty; 9];
        slots[0] = Slot::Other(None);
        slots[1] = Slot::Other(Some(Version(0, 0, 34)));
        assert_eq!(plan(&slots, V35), Plan::Install((0..9).collect()));
    }

    #[test]
    fn local_builds_never_install_themselves() {
        // Unit tests are never built with BOWSER_RELEASE_BUILD, so nothing here may install.
        assert!(!wanted(false, false, false));
        assert!(!wanted(true, false, false) && !wanted(false, true, false) && !wanted(false, false, true));
    }
}
