#[test]
fn chrome_sources_exclude_sensitive_store_terms() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let forbidden = [
        ["pass", "words"].concat(),
        ["va", "ult"].concat(),
        ["creden", "tial"].concat(),
    ];
    for dir in ["src/chrome_host", "ui/src"] {
        let mut pending = vec![root.join(dir)];
        while let Some(path) = pending.pop() {
            for entry in std::fs::read_dir(path).expect("source directory") {
                let path = entry.expect("source entry").path();
                if path.is_dir() { pending.push(path); continue; }
                if path.file_name().is_some_and(|name| name == "guard.rs") { continue; }
                let source = std::fs::read_to_string(&path).expect("source text").to_ascii_lowercase();
                for term in &forbidden {
                    assert!(!source.contains(term), "{} contains {term}", path.display());
                }
            }
        }
    }
}
