#[path = "src/chrome_host/fingerprint.rs"]
mod fingerprint;

fn main() {
    println!("cargo:rerun-if-changed=bowser.manifest");
    println!("cargo:rerun-if-changed=bowser.rc");
    println!("cargo:rerun-if-changed=icons/bowser.ico");
    println!("cargo:rerun-if-changed=Cargo.toml");
    for path in ["ui/src", "ui/dist", "ui/dist.manifest"] {
        println!("cargo:rerun-if-changed={path}");
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(root.join("ui/dist.manifest"))
        .expect("Missing ui/dist.manifest; run node ui/write-manifest.mjs after building the UI");
    let src = fingerprint::tree_fingerprint(&root.join("ui/src")).expect("Cannot fingerprint ui/src");
    let dist = fingerprint::tree_fingerprint(&root.join("ui/dist")).expect("Cannot fingerprint ui/dist");
    let expected_dist = fingerprint::manifest_value(&manifest, "dist").expect("Invalid dist fingerprint in ui/dist.manifest");
    if dist != expected_dist {
        panic!("ui/dist differs from ui/dist.manifest; run npm run build in ui, or node ui/write-manifest.mjs after a verified dist update");
    }
    let expected_src = fingerprint::manifest_value(&manifest, "src").expect("Invalid src fingerprint in ui/dist.manifest");
    if src != expected_src {
        println!("cargo:warning=ui/src differs from ui/dist.manifest; rebuild the UI before shipping embedded assets");
    }
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
            concat!(env!("CARGO_MANIFEST_DIR"), "/bowser.manifest")
        );
        let resource = compile_icon(root);
        println!("cargo:rustc-link-arg-bins={}", resource.display());
    }
}

/// The text of `bowser.rc` plus a `VERSIONINFO` resource built from the Cargo version, so an
/// installed `Bowser.exe` says which version it is (Explorer's Properties, and
/// `src/self_install.rs`, which never replaces a newer installed copy with an older download).
fn resource_script(rc: &str, version: &str) -> String {
    let mut parts = version.split('.').map(|part| part.parse::<u16>().unwrap_or(0));
    let (major, minor, patch) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    format!(
        "{rc}\n1 VERSIONINFO\nFILEVERSION {major},{minor},{patch},0\nPRODUCTVERSION {major},{minor},{patch},0\n\
         FILEFLAGSMASK 0x3fL\nFILEFLAGS 0x0L\nFILEOS 0x40004L\nFILETYPE 0x1L\nFILESUBTYPE 0x0L\nBEGIN\n\
         BLOCK \"StringFileInfo\"\nBEGIN\nBLOCK \"040904b0\"\nBEGIN\n\
         VALUE \"FileDescription\", \"Bowser\"\nVALUE \"FileVersion\", \"{major}.{minor}.{patch}\"\n\
         VALUE \"InternalName\", \"bowser\"\nVALUE \"OriginalFilename\", \"Bowser.exe\"\n\
         VALUE \"ProductName\", \"Bowser\"\nVALUE \"ProductVersion\", \"{major}.{minor}.{patch}\"\nEND\nEND\n\
         BLOCK \"VarFileInfo\"\nBEGIN\nVALUE \"Translation\", 0x409, 1200\nEND\nEND\n"
    )
}

fn compile_icon(root: &std::path::Path) -> std::path::PathBuf {
    let rc = find_rc_exe();
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let out = out_dir.join("bowser.res");
    let source = std::fs::read_to_string(root.join("bowser.rc")).expect("Cannot read bowser.rc");
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    // The script is written to OUT_DIR, so the icon path it names is made absolute.
    let root_text = root.display().to_string().replace('\\', "\\\\");
    let source = source.replace("\"icons\\\\", &format!("\"{root_text}\\\\icons\\\\"));
    let script = out_dir.join("bowser-version.rc");
    std::fs::write(&script, resource_script(&source, &version)).expect("Cannot write the resource script");
    let status = std::process::Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&out)
        .arg(&script)
        .current_dir(root)
        .status()
        .unwrap_or_else(|error| panic!("failed to run {}: {error}", rc.display()));
    if !status.success() {
        panic!("rc.exe failed with {status}");
    }
    out
}

fn find_rc_exe() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("RC") {
        let path = std::path::PathBuf::from(path);
        if path.is_file() {
            return path;
        }
    }
    let kits = std::path::Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut found = Vec::new();
    if let Ok(versions) = std::fs::read_dir(kits) {
        for version in versions.flatten() {
            let candidate = version.path().join("x64").join("rc.exe");
            if candidate.is_file() {
                found.push(candidate);
            }
        }
    }
    found.sort();
    found.pop().unwrap_or_else(|| panic!("Windows SDK rc.exe was not found"))
}
