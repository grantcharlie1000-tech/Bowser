#[path = "src/chrome_host/fingerprint.rs"]
mod fingerprint;

fn main() {
    println!("cargo:rerun-if-changed=bowser.manifest");
    println!("cargo:rerun-if-changed=bowser.rc");
    println!("cargo:rerun-if-changed=icons/bowser.ico");
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

fn compile_icon(root: &std::path::Path) -> std::path::PathBuf {
    let rc = find_rc_exe();
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("bowser.res");
    let status = std::process::Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&out)
        .arg(root.join("bowser.rc"))
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
