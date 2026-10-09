fn main() {
    println!("cargo:rerun-if-changed=bowser.manifest");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
            concat!(env!("CARGO_MANIFEST_DIR"), "/bowser.manifest")
        );
    }
}
