#[path = "src/chrome_host/fingerprint.rs"]
mod fingerprint;

fn main() {
    println!("cargo:rerun-if-changed=bowser.manifest");
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
    }
}
