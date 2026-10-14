//! Install an extension from a `.crx` file the user picked on this computer. No network.
//!
//! Read at most [`MAX_CRX_FILE_BYTES`], verify the file against the id it declares
//! ([`crate::crx::verify_crx3_declared`]), unpack it stamped with its developer key into a new empty
//! temporary folder, then hand that folder to [`crate::extensions::install_confirmed`], so the
//! existing permission prompt and the copy into the profile are the only install path. The temporary
//! folder is removed when the call ends, whatever the outcome.
//!
//! A verified signature proves the file is intact and was made by the key that owns its id. It does
//! not say where the file came from, and nothing here claims it came from a store.

use crate::crx::{self, CrxError};
use crate::crx_unpack::{self, StampError};
use crate::extensions::{self, ExtensionError, Inspection, Record};
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Largest `.crx` file Bowser reads: 144 MiB. A larger file is refused before it is parsed.
pub const MAX_CRX_FILE_BYTES: u64 = 144 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrxFileError {
    /// The picked path is not a plain file.
    NotAFile,
    /// The file is larger than [`MAX_CRX_FILE_BYTES`].
    TooLarge,
    Read,
    /// The file did not verify; see [`CrxError`].
    Crx(CrxError),
    /// The verified archive could not be unpacked or stamped; see [`StampError`].
    Stamp(StampError),
    /// No temporary folder could be created.
    Staging,
    /// The unpacked folder was refused or could not be installed; see [`ExtensionError`].
    Install(ExtensionError),
}

impl fmt::Display for CrxFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAFile => f.write_str("That is not a file."),
            Self::TooLarge => f.write_str("That file is larger than 144 MiB, which Bowser does not read."),
            Self::Read => f.write_str("The file could not be read."),
            Self::Crx(error) => write!(f, "The file did not verify as a signed CRX package: {error}."),
            Self::Stamp(error) => write!(f, "The package could not be unpacked: {error}."),
            Self::Staging => f.write_str("Bowser could not make a temporary folder to unpack the package."),
            Self::Install(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CrxFileError {}

/// Reads `path`, refusing anything that is not a plain file or is larger than `cap` before a byte
/// is parsed. A file that grows while it is read is refused too.
fn read_capped(path: &Path, cap: u64) -> Result<Vec<u8>, CrxFileError> {
    let meta = fs::metadata(path).map_err(|_| CrxFileError::Read)?;
    if !meta.is_file() {
        return Err(CrxFileError::NotAFile);
    }
    if meta.len() > cap {
        return Err(CrxFileError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).map_err(|_| CrxFileError::TooLarge)?);
    fs::File::open(path)
        .and_then(|file| file.take(cap + 1).read_to_end(&mut bytes))
        .map_err(|_| CrxFileError::Read)?;
    if bytes.len() as u64 > cap {
        return Err(CrxFileError::TooLarge);
    }
    Ok(bytes)
}

/// Installs the `.crx` file at `path` into the profile, asking `confirm` first. `Ok(None)` when it
/// declines: the copy is deleted and nothing is recorded.
pub fn install_crx_file(
    path: &Path,
    extensions_dir: &Path,
    records_file: &Path,
    confirm: &mut dyn FnMut(&Inspection) -> bool,
) -> Result<Option<Record>, CrxFileError> {
    install_crx_file_in(path, &std::env::temp_dir(), extensions_dir, records_file, confirm)
}

fn install_crx_file_in(
    path: &Path,
    staging_root: &Path,
    extensions_dir: &Path,
    records_file: &Path,
    confirm: &mut dyn FnMut(&Inspection) -> bool,
) -> Result<Option<Record>, CrxFileError> {
    let staging = Staging::create(staging_root)?;
    // The package bytes (up to 144 MiB) are not needed once unpacked, so they go before the prompt.
    {
        let crx = read_capped(path, MAX_CRX_FILE_BYTES)?;
        let verified = crx::verify_crx3_declared(&crx).map_err(CrxFileError::Crx)?;
        crx_unpack::unpack_stamped(&verified, &staging.0).map_err(CrxFileError::Stamp)?;
    }
    extensions::install_confirmed(&staging.0, extensions_dir, records_file, confirm)
        .map_err(CrxFileError::Install)
}

/// A new, empty folder under a root, removed with everything in it on drop.
struct Staging(PathBuf);

impl Staging {
    fn create(root: &Path) -> Result<Self, CrxFileError> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let path = root.join(format!(
            "bowser-crx-{}-{nanos:x}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // `create_dir` fails if the name exists, so what is unpacked here starts out empty and ours.
        fs::create_dir(&path).map_err(|_| CrxFileError::Staging)?;
        Ok(Self(path))
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "bowser-crx-file-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir(&path).expect("create scratch folder");
            Self(path)
        }

        fn sub(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const MANIFEST: &str = "{\"manifest_version\":3,\"name\":\"Demo\",\"version\":\"1.0\"}";

    fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o644);
        for (name, contents) in files {
            zip.start_file(*name, options).unwrap();
            zip.write_all(contents.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    /// The signed CRX of a one-file-plus-manifest extension, and the developer SPKI.
    fn good_crx() -> (Vec<u8>, Vec<u8>) {
        let (crx, _id, spki) = crate::crx::tests::signed_crx_for_tests(&zip_of(&[
            ("manifest.json", MANIFEST),
            ("background.js", "console.log(1);"),
        ]));
        (crx, spki)
    }

    /// A profile as `install_confirmed` sees it, a staging root, and the picked file's folder.
    struct Setup {
        scratch: Scratch,
    }

    impl Setup {
        fn new() -> Self {
            let scratch = Scratch::new();
            fs::create_dir(scratch.sub("staging")).unwrap();
            fs::create_dir(scratch.sub("picked")).unwrap();
            Self { scratch }
        }

        fn pick(&self, bytes: &[u8]) -> PathBuf {
            let path = self.scratch.sub("picked").join("demo.crx");
            fs::write(&path, bytes).unwrap();
            path
        }

        fn extensions_dir(&self) -> PathBuf {
            self.scratch.sub("profile").join("Extensions")
        }

        fn records_file(&self) -> PathBuf {
            self.scratch.sub("profile").join("extensions.txt")
        }

        fn run(
            &self,
            path: &Path,
            confirm: &mut dyn FnMut(&Inspection) -> bool,
        ) -> Result<Option<Record>, CrxFileError> {
            install_crx_file_in(
                path,
                &self.scratch.sub("staging"),
                &self.extensions_dir(),
                &self.records_file(),
                confirm,
            )
        }

        fn records(&self) -> Vec<Record> {
            extensions::read_records(&self.records_file()).unwrap()
        }

        /// Names of the folders copied into the profile's extensions directory.
        fn copies(&self) -> Vec<String> {
            fs::read_dir(self.extensions_dir())
                .map(|entries| entries.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect())
                .unwrap_or_default()
        }

        fn staging_is_empty(&self) -> bool {
            fs::read_dir(self.scratch.sub("staging")).unwrap().next().is_none()
        }

        fn assert_nothing_installed(&self) {
            assert_eq!(self.records(), Vec::new(), "no record");
            assert_eq!(self.copies(), Vec::<String>::new(), "no copy");
            assert!(self.staging_is_empty(), "no temporary folder left");
        }
    }

    #[test]
    fn a_signed_crx_is_verified_stamped_and_recorded_with_its_key_in_the_copy() {
        let setup = Setup::new();
        let (crx, spki) = good_crx();
        let path = setup.pick(&crx);
        let mut prompted = Vec::new();
        let record = setup
            .run(&path, &mut |inspection| {
                prompted.push(inspection.manifest.name.clone());
                true
            })
            .unwrap()
            .expect("accepted");

        assert_eq!(prompted, ["Demo"], "the existing prompt ran once, on the unpacked manifest");
        assert_eq!((record.name.as_str(), record.version.as_str(), record.enabled), ("Demo", "1.0", true));
        assert_eq!(setup.records(), vec![record.clone()]);
        assert_eq!(setup.copies(), [record.id.clone()]);
        let copy = setup.extensions_dir().join(&record.id);
        let stamped = crate::crx_unpack::manifest_with_key(MANIFEST.as_bytes(), &spki).unwrap();
        assert_eq!(fs::read(copy.join("manifest.json")).unwrap(), stamped, "the copy keeps the developer key");
        assert!(String::from_utf8(stamped).unwrap().contains("\"key\":\""));
        assert_eq!(fs::read(copy.join("background.js")).unwrap(), b"console.log(1);");
        assert!(setup.staging_is_empty(), "the temporary folder is gone");
        assert!(path.is_file(), "the picked file is left alone");
    }

    #[test]
    fn a_tampered_byte_installs_nothing_and_never_asks() {
        let setup = Setup::new();
        let (mut crx, _) = good_crx();
        let last = crx.len() - 1;
        crx[last] ^= 1;
        let path = setup.pick(&crx);
        let mut asked = false;
        let result = setup.run(&path, &mut |_| {
            asked = true;
            true
        });
        assert_eq!(result, Err(CrxFileError::Crx(CrxError::SignatureInvalid)));
        assert!(!asked);
        setup.assert_nothing_installed();
    }

    #[test]
    fn a_file_over_the_cap_is_refused_before_it_is_parsed() {
        let setup = Setup::new();
        let path = setup.scratch.sub("picked").join("huge.crx");
        // Zeros are not a CRX: had this been parsed the error would be a CRX one, not TooLarge.
        fs::File::create(&path).unwrap().set_len(MAX_CRX_FILE_BYTES + 1).unwrap();
        let mut asked = false;
        let result = setup.run(&path, &mut |_| {
            asked = true;
            true
        });
        assert_eq!(result, Err(CrxFileError::TooLarge));
        assert!(!asked);
        setup.assert_nothing_installed();
    }

    #[test]
    fn the_read_cap_allows_exactly_its_size_and_refuses_one_byte_more() {
        let scratch = Scratch::new();
        let path = scratch.sub("grown.crx");
        fs::write(&path, [0u8; 64]).unwrap();
        assert_eq!(read_capped(&path, 63), Err(CrxFileError::TooLarge));
        assert_eq!(read_capped(&path, 64).unwrap().len(), 64, "exactly the cap is allowed");
    }

    #[test]
    fn declining_the_prompt_deletes_the_copy_and_records_nothing() {
        let setup = Setup::new();
        let (crx, _) = good_crx();
        let path = setup.pick(&crx);
        let mut copies_seen_while_asking = Vec::new();
        let result = setup.run(&path, &mut |_| {
            copies_seen_while_asking = setup.copies();
            false
        });
        assert_eq!(result, Ok(None));
        assert_eq!(copies_seen_while_asking.len(), 1, "the prompt inspects the copy that would run");
        setup.assert_nothing_installed();
    }

    #[test]
    fn a_file_that_is_not_a_crx_or_not_a_file_installs_nothing() {
        let setup = Setup::new();
        let path = setup.pick(b"PK this is a plain zip, not a CRX");
        assert!(matches!(setup.run(&path, &mut |_| true), Err(CrxFileError::Crx(_))));
        assert_eq!(setup.run(&setup.scratch.sub("picked"), &mut |_| true), Err(CrxFileError::NotAFile));
        assert_eq!(setup.run(&setup.scratch.sub("missing.crx"), &mut |_| true), Err(CrxFileError::Read));
        setup.assert_nothing_installed();
    }

    #[test]
    fn a_verified_package_without_a_manifest_installs_nothing() {
        let setup = Setup::new();
        let (crx, _id, _spki) = crate::crx::tests::signed_crx_for_tests(&zip_of(&[("a.js", "1")]));
        let path = setup.pick(&crx);
        assert_eq!(
            setup.run(&path, &mut |_| true),
            Err(CrxFileError::Stamp(StampError::ManifestMissing))
        );
        setup.assert_nothing_installed();
    }

    #[test]
    fn messages_never_claim_a_store() {
        let errors = [
            CrxFileError::NotAFile,
            CrxFileError::TooLarge,
            CrxFileError::Read,
            CrxFileError::Crx(CrxError::SignatureInvalid),
            CrxFileError::Stamp(StampError::ManifestMissing),
            CrxFileError::Staging,
            CrxFileError::Install(ExtensionError::TooManyInstalled),
        ];
        for error in errors {
            let text = error.to_string().to_lowercase();
            assert!(!text.contains("store") && !text.contains("chrome"), "{text}");
        }
    }
}
