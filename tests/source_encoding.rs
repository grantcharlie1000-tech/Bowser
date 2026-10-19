//! Source text must be exact UTF-8, not UTF-8 that went through a Windows-1252 round trip.
//!
//! A file saved through an ANSI editor, or read by Windows PowerShell 5.1 without a byte-order
//! mark, turns `…` (E2 80 A6) into three characters (U+00E2 U+20AC U+00A6), which then ship in
//! menus and reports. This test reads every text file in the repository and fails on any run of
//! characters that is really UTF-8 decoded as Windows-1252, and on any PowerShell script with
//! non-ASCII text but no BOM.

use std::path::{Path, PathBuf};

/// Directories that hold generated, vendored or tool output, not source.
const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", "dist", "graphify-out", ".claude"];

/// Extensions of the files that are text.
const TEXT: &[&str] = &[
    "rs", "ts", "tsx", "js", "mjs", "cjs", "css", "html", "htm", "json", "md", "toml", "yml", "yaml", "ps1", "psm1", "cmd", "bat",
    "txt", "svg", "rc", "manifest", "csv",
];

/// The Windows-1252 byte that decodes to `ch`, if any. Bytes 0x81, 0x8D, 0x8F, 0x90 and 0x9D are
/// unassigned and come through as the C1 control of the same value.
fn cp1252_byte(ch: char) -> Option<u8> {
    const HIGH: [char; 32] = [
        '\u{20AC}', '\u{81}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}', '\u{02C6}', '\u{2030}', '\u{0160}',
        '\u{2039}', '\u{0152}', '\u{8D}', '\u{017D}', '\u{8F}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}',
        '\u{2013}', '\u{2014}', '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{9D}', '\u{017E}', '\u{0178}',
    ];
    match ch as u32 {
        0xA0..=0xFF => Some(ch as u32 as u8),
        _ => HIGH.iter().position(|&high| high == ch).map(|at| 0x80 + at as u8),
    }
}

/// Every run of non-ASCII characters in `text` that is UTF-8 read as Windows-1252, with what it
/// should have been.
fn mojibake(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut check = |run: &str| {
        let Some(bytes) = run.chars().map(cp1252_byte).collect::<Option<Vec<u8>>>() else { return };
        if let Ok(meant) = String::from_utf8(bytes)
            && meant != run
        {
            found.push((run.to_owned(), meant));
        }
    };
    let mut start = None;
    for (at, ch) in text.char_indices() {
        match (ch.is_ascii(), start) {
            (false, None) => start = Some(at),
            (true, Some(from)) => {
                check(&text[from..at]);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        check(&text[from..]);
    }
    found
}

fn text_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if !SKIP_DIRS.iter().any(|skip| name == *skip) {
                text_files(&path, out);
            }
        } else if kind.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| TEXT.iter().any(|text| ext.eq_ignore_ascii_case(text)))
        {
            out.push(path);
        }
    }
}

#[test]
fn the_detector_finds_double_encoded_text_and_leaves_real_text_alone() {
    // "Edit…", "Zoom · 100%", "Ctrl+−", "page’s" after a Windows-1252 round trip.
    for (bad, good) in [
        ("Edit\u{E2}\u{20AC}\u{A6}", "\u{2026}"),
        ("Zoom \u{C2}\u{B7} 100%", "\u{B7}"),
        ("Ctrl+\u{E2}\u{2C6}\u{2019}", "\u{2212}"),
        ("page\u{E2}\u{20AC}\u{2122}s", "\u{2019}"),
        ("PASS \u{E2}\u{20AC}\u{201D} ok", "\u{2014}"),
    ] {
        assert_eq!(mojibake(bad), vec![(bad.chars().filter(|ch| !ch.is_ascii()).collect(), good.to_owned())], "{bad}");
    }
    for good in ["Edit\u{2026}", "Zoom \u{B7} 100%", "Ctrl+\u{2212}", "caf\u{E9}\u{2026}", "\u{A3}5 \u{2014} \u{A9} 2026", "\u{1F600}", "na\u{EF}ve"] {
        assert!(mojibake(good).is_empty(), "{good}");
    }
}

#[test]
fn source_files_hold_no_double_encoded_utf8() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    text_files(root, &mut files);
    assert!(files.iter().any(|path| path.ends_with("src/win32/window.rs")), "the scan must reach src/");
    let mut problems = Vec::new();
    for path in &files {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let shown = path.strip_prefix(root).unwrap_or(path).display().to_string();
        let is_powershell = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("ps1") || ext.eq_ignore_ascii_case("psm1"));
        let bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
        if is_powershell && !bom && !bytes.is_ascii() {
            // Windows PowerShell 5.1 reads a BOM-less script as ANSI, so its non-ASCII text
            // comes out double-encoded in everything the script writes.
            problems.push(format!("{shown}: non-ASCII PowerShell script without a UTF-8 BOM"));
        }
        let Ok(text) = std::str::from_utf8(&bytes) else { continue };
        for (line, content) in text.lines().enumerate() {
            for (bad, meant) in mojibake(content) {
                problems.push(format!("{shown}:{}: {bad:?} should be {meant:?}", line + 1));
            }
        }
    }
    assert!(problems.is_empty(), "double-encoded text:\n{}", problems.join("\n"));
}
