//! The Windows side of downloads (docs/design/downloads.md): the Downloads known folder, the native
//! Save and folder dialogs, and the shell actions on a finished file. Small on purpose: the rules
//! live in `crate::downloads` (unit-tested); this file only calls the shell and cannot be exercised
//! headless (dialogs and Explorer need an interactive desktop).
//!
//! Every file action takes a path Rust recorded for a download, never text from a page, and acts
//! only if it is still a regular file ([`regular_file`]).

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

use windows::{
    Win32::{
        Foundation::HWND,
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
        UI::{
            Shell::{
                Common::COMDLG_FILTERSPEC, FOLDERID_Downloads, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR,
                FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog, FileSaveDialog,
                IFileDialog, IFileOpenDialog, IFileSaveDialog, IShellItem, KF_FLAG_DEFAULT, SHCreateItemFromParsingName,
                SHGetKnownFolderPath, SHOpenFolderAndSelectItems, SHParseDisplayName, ILFree, SIGDN_FILESYSPATH,
                ShellExecuteW,
            },
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
    core::{HSTRING, PCWSTR, w},
};

/// The user's Downloads folder: the known folder, else `%USERPROFILE%\Downloads`. It is not created here.
pub fn default_downloads_dir() -> Option<PathBuf> {
    let known = unsafe {
        SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None).ok().and_then(|raw| {
            let text = raw.to_string().ok();
            CoTaskMemFree(Some(raw.0.cast_const().cast()));
            text
        })
    };
    known
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("Downloads")))
        .filter(|path| path.is_absolute())
}

/// Whether `path` exists and is a regular file (not a folder, not missing).
pub fn regular_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

/// Opens a recorded file with its default app.
pub fn open_file(path: &Path) -> bool {
    if !path.is_absolute() || !regular_file(path) {
        return false;
    }
    let target = HSTRING::from(path.as_os_str());
    // ShellExecuteW reports failure with a value of 32 or less.
    let result = unsafe { ShellExecuteW(None, w!("open"), &target, None, None, SW_SHOWNORMAL) };
    (result.0 as isize) > 32
}

/// Shows a recorded file selected in Explorer; falls back to `explorer /select,`.
pub fn show_in_folder(path: &Path) -> bool {
    if !path.is_absolute() || !regular_file(path) {
        return false;
    }
    if select_in_explorer(path) {
        return true;
    }
    // The argument is one string, `/select,"C:\path"`: Explorer takes no other quoting.
    std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .is_ok()
}

fn select_in_explorer(path: &Path) -> bool {
    let wide = HSTRING::from(path.as_os_str());
    unsafe {
        let mut pidl = std::ptr::null_mut();
        if SHParseDisplayName(&wide, None, &mut pidl, 0, None).is_err() || pidl.is_null() {
            return false;
        }
        let shown = SHOpenFolderAndSelectItems(pidl, None, 0).is_ok();
        ILFree(Some(pidl));
        shown
    }
}

/// Opens a folder in Explorer.
pub fn open_folder(dir: &Path) -> bool {
    if !dir.is_absolute() || !dir.is_dir() {
        return false;
    }
    let target = HSTRING::from(dir.as_os_str());
    let result = unsafe { ShellExecuteW(None, w!("open"), &target, None, None, SW_SHOWNORMAL) };
    (result.0 as isize) > 32
}

fn item_for(path: &Path) -> Option<IShellItem> {
    let wide = HSTRING::from(path.as_os_str());
    unsafe { SHCreateItemFromParsingName(&wide, None).ok() }
}

fn chosen_path(dialog: &IFileDialog) -> Option<PathBuf> {
    unsafe {
        let item = dialog.GetResult().ok()?;
        let display = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = display.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(display.0.cast_const().cast()));
        path.filter(|path| path.is_absolute())
    }
}

/// The folder picker for Settings > Downloads. `None` when cancelled or it could not be shown.
pub fn pick_folder(owner: HWND, start: Option<&Path>) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog
            .SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR)
            .ok()?;
        dialog.SetTitle(w!("Choose the downloads folder")).ok()?;
        if let Some(item) = start.and_then(item_for) {
            let _ = dialog.SetFolder(&item);
        }
        dialog.Show(Some(owner)).ok()?;
        chosen_path(&dialog)
    }
}

/// The open dialog for importing notes: a Markdown or text file. `None` when cancelled.
pub fn open_notes_dialog(owner: HWND) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog
            .SetOptions(windows::Win32::UI::Shell::FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR)
            .ok()?;
        dialog.SetTitle(w!("Import notes")).ok()?;
        let filters = [
            COMDLG_FILTERSPEC { pszName: w!("Markdown and text files"), pszSpec: w!("*.md;*.markdown;*.txt") },
            COMDLG_FILTERSPEC { pszName: w!("All files"), pszSpec: w!("*.*") },
        ];
        let _ = dialog.SetFileTypes(&filters);
        let _ = dialog.SetFileTypeIndex(1);
        dialog.Show(Some(owner)).ok()?;
        chosen_path(&dialog)
    }
}

/// The Save dialog for "Ask where to save each file": `name` preselected in `dir`, the file's own
/// extension as the first filter (so a name typed without one gets it). `None` when cancelled.
pub fn save_dialog(owner: HWND, dir: &Path, name: &str) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog
            .SetOptions(FOS_OVERWRITEPROMPT | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR)
            .ok()?;
        dialog.SetTitle(w!("Save as")).ok()?;
        if let Some(item) = item_for(dir) {
            let _ = dialog.SetFolder(&item);
        }
        let file_name = HSTRING::from(name);
        let _ = dialog.SetFileName(&file_name);
        let (_, ext) = crate::downloads::split_extension(name);
        // Kept alive until Show returns: the dialog reads the specs while it runs.
        let kind = HSTRING::from(format!("{} files", ext.trim_start_matches('.').to_uppercase()));
        let spec = HSTRING::from(format!("*{ext}"));
        let default_ext = HSTRING::from(ext.trim_start_matches('.'));
        if !ext.is_empty() {
            let filters = [
                COMDLG_FILTERSPEC { pszName: PCWSTR(kind.as_ptr()), pszSpec: PCWSTR(spec.as_ptr()) },
                COMDLG_FILTERSPEC { pszName: w!("All files"), pszSpec: w!("*.*") },
            ];
            let _ = dialog.SetFileTypes(&filters);
            let _ = dialog.SetFileTypeIndex(1);
            let _ = dialog.SetDefaultExtension(&default_ext);
        }
        dialog.Show(Some(owner)).ok()?;
        chosen_path(&dialog)
    }
}
