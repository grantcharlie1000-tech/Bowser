//! Runs tests/script_checks.ps1 under Windows PowerShell 5.1, the shell the install, update and
//! uninstall scripts are run with. The checks load functions from the scripts' syntax trees and
//! work only inside a temporary directory: no script body, registry write or real profile.

use std::{path::PathBuf, process::Command};

#[test]
fn powershell_scripts_survive_odd_paths_and_replace_files_safely() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let temp = std::env::temp_dir().join(format!("bowser-script-checks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).unwrap();
    let output = Command::new("powershell.exe")
        // pwsh -> Rust -> Windows PowerShell otherwise inherits PS7-only module paths.
        // Remove only this child's variable so Windows PowerShell rebuilds its own defaults.
        .env_remove("PSModulePath")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(repo.join("tests").join("script_checks.ps1"))
        .arg("-Repo")
        .arg(&repo)
        .arg("-Temp")
        .arg(&temp)
        .output()
        .expect("Windows PowerShell runs");
    let _ = std::fs::remove_dir_all(&temp);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("{stdout}");
    assert!(
        output.status.success() && stdout.contains("script checks passed"),
        "{stdout}\n{stderr}"
    );
}
